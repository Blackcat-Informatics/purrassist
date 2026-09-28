// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Type-enforced scratch accounting: live scratch returns to baseline once a
//! transient search completes, and a byte-precise budget yields explicit
//! exhaustion rather than a silently unbounded allocation.
use purrassist_core::{
    AssistResult, Budget, Governor, Outcome, StopReason,
    contract::Resource,
    json::Node,
    pattern::Pattern,
    schema::{Schema, recovery::RecoveryRequest},
};

#[test]
fn repeated_pattern_matches_return_scratch_to_its_pre_call_level() {
    let mut governor = Governor::new(Budget::default());
    let pattern = Pattern::compile_governed("(a|b)+c", &mut governor).unwrap();
    let baseline = governor.scratch_meter().live();
    assert!(baseline > 0, "compiling the pattern itself stays charged");

    assert!(pattern.is_match("aabbc", &mut governor).unwrap());
    assert_eq!(governor.scratch_meter().live(), baseline);

    assert!(!pattern.is_match("aabbd", &mut governor).unwrap());
    assert_eq!(governor.scratch_meter().live(), baseline);

    // A longer, backtracking-heavy search charges (and must release) far
    // more scratch mid-search than the tiny searches above; repeating it
    // must not accumulate any of that as permanently live scratch.
    let haystack = "a".repeat(64) + "c";
    for _ in 0..25 {
        let _ = pattern.is_match(&haystack, &mut governor).unwrap();
        assert_eq!(
            governor.scratch_meter().live(),
            baseline,
            "is_match must release every byte it charges once it returns"
        );
    }
}

/// A schema/input pair whose recovery exercises a formerly-uncharged
/// allocation: `object_proposals` clones `members` to rename `color` to the
/// declared `colour`, and `change` clones the whole candidate to hold that
/// proposal (crates/core/src/schema/recovery.rs, crates/core/src/schema/
/// recovery/proposals.rs). Both sites are charged via `Governor::lease_scratch`
/// before the clone, so the boundary below is exact to the byte.
fn recover_at(scratch_bytes: u64) -> AssistResult<Node> {
    let schema = Schema::compile(
        r#"{"type":"object","properties":{"colour":{"type":"string"}},"required":["colour"]}"#,
    )
    .unwrap();
    schema.recover(
        r#"{"color":"red"}"#,
        RecoveryRequest {
            budget: Some(Budget {
                scratch_bytes,
                ..Budget::default()
            }),
            ..RecoveryRequest::default()
        },
    )
}

#[test]
fn a_scratch_budget_one_byte_below_a_charged_allocation_yields_exhaustion_and_no_output() {
    let generous = 1 << 20;
    assert!(
        matches!(
            recover_at(generous).outcome,
            Outcome::Resolved(_) | Outcome::BestGuess(_) | Outcome::Ambiguous(_)
        ),
        "a generous scratch budget must still let this small recovery succeed"
    );

    // Binary search for the exact minimal scratch budget this recovery
    // needs. Every governed charge here is a check-before-count admission
    // (`Governor::charge`/`Governor::lease_scratch`), so this boundary is
    // exact to the byte, not a probabilistic threshold.
    let (mut low, mut high) = (0_u64, generous);
    while low + 1 < high {
        let mid = low + (high - low) / 2;
        let succeeds = matches!(
            recover_at(mid).outcome,
            Outcome::Resolved(_) | Outcome::BestGuess(_) | Outcome::Ambiguous(_)
        );
        if succeeds {
            high = mid;
        } else {
            low = mid;
        }
    }
    // `high` is the minimal budget that succeeds; `low == high - 1` must
    // fail with an explicit, byte-precise exhaustion and no partial answer.
    let starved = recover_at(low);
    assert_eq!(
        starved.receipt.stopped,
        StopReason::BudgetExceeded(Resource::ScratchBytes)
    );
    assert_eq!(starved.outcome, Outcome::Unresolved);
    assert!(
        starved.receipt.evidence.is_empty()
            && starved.receipt.losses.is_empty()
            && starved.receipt.inferences.is_empty()
            && starved.receipt.alternatives.is_empty(),
        "exhaustion mid-search must not publish any candidate's ledgers"
    );

    // One byte more succeeds again, confirming `low` was not a fluke of an
    // unrelated resource (the search only ever varies `scratch_bytes` here).
    assert!(matches!(
        recover_at(high).outcome,
        Outcome::Resolved(_) | Outcome::BestGuess(_) | Outcome::Ambiguous(_)
    ));
}
