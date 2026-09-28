// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Caller-owned cancellation across shared governors and real engine entry points.
use purrassist_core::{
    Budget, CancellationToken, Governor, Outcome, StopReason,
    contract::Resource,
    edits,
    matching::{self, Candidate, MatchRequest},
    schema::{Schema, recovery::RecoveryRequest},
};

#[test]
fn another_owner_cancels_a_live_governor_without_losing_prior_usage() {
    let token = CancellationToken::new();
    let attached = token.clone();
    let (ready, started) = std::sync::mpsc::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut governor = Governor::with_cancellation(Budget::default(), attached);
        let schema = Schema::compile_governed(r#"{"type":"integer"}"#, &mut governor).unwrap();
        let first = schema.recover_governed("42", RecoveryRequest::default(), &mut governor);
        assert!(matches!(first.outcome, Outcome::Resolved(_)));
        let usage = governor.usage();
        assert!(usage.work > 0);
        ready.send(()).unwrap();
        resumed.recv().unwrap();
        let stopped = schema.recover_governed("[[42]]", RecoveryRequest::default(), &mut governor);
        assert!(matches!(stopped.outcome, Outcome::Unresolved));
        assert_eq!(stopped.receipt.stopped, StopReason::Cancelled);
        assert_eq!(stopped.receipt.usage, usage);
        assert_eq!(
            governor.charge(Resource::Work, 0),
            Err(StopReason::Cancelled)
        );
    });
    started.recv().unwrap();
    token.cancel();
    token.cancel();
    resume.send(()).unwrap();
    worker.join().unwrap();
    assert!(token.is_cancelled());
}

#[test]
fn cancellation_is_shared_but_local_exhaustion_is_not() {
    let token = CancellationToken::new();
    let mut exhausted = Governor::with_cancellation(
        Budget {
            work: 0,
            ..Budget::default()
        },
        token.clone(),
    );
    let mut sibling = Governor::with_cancellation(Budget::default(), token.clone());
    let reason = StopReason::BudgetExceeded(Resource::Work);
    assert_eq!(exhausted.charge(Resource::Work, 1), Err(reason.clone()));
    assert_eq!(sibling.charge(Resource::Work, 1), Ok(()));
    token.cancel();
    exhausted.cancel();
    assert_eq!(exhausted.check(), Err(reason));
    assert_eq!(sibling.check(), Err(StopReason::Cancelled));
    assert_eq!(sibling.clone().check(), Err(StopReason::Cancelled));
    assert_eq!(Governor::new(Budget::default()).check(), Ok(()));
}

#[test]
fn planning_and_matching_use_the_supplied_limits_and_cancellation() {
    let token = CancellationToken::new();
    let budget = Budget {
        work: 23_456,
        ..Budget::default()
    };
    let mut governor = Governor::with_cancellation(budget, token.clone());
    governor.charge(Resource::Work, 17).unwrap();
    let found = matching::rank_governed(
        "tea",
        &[Candidate::new("tea", "tea")],
        &MatchRequest::default(),
        &mut governor,
    );
    assert!(matches!(found.outcome, Outcome::Resolved(_)));
    assert_eq!(found.receipt.budget, budget);
    assert_eq!(found.receipt.usage, governor.usage());
    let usage = governor.usage();
    token.cancel();
    for result in [
        edits::locate_governed("tea", "tea", "coffee", &mut governor),
        edits::locate_fuzzy_governed("tea", "tea", "coffee", 1, &mut governor),
        edits::plan_unified_governed("tea", "@@ -1 +1 @@\n-tea\n+coffee\n", &mut governor),
    ] {
        assert!(matches!(result.outcome, Outcome::Unresolved));
        assert_eq!(result.receipt.budget, budget);
        assert_eq!(result.receipt.usage, usage);
        assert_eq!(result.receipt.stopped, StopReason::Cancelled);
    }
    let stopped = matching::rank_governed("tea", &[], &MatchRequest::default(), &mut governor);
    assert!(matches!(stopped.outcome, Outcome::Unresolved));
    assert_eq!(stopped.receipt.stopped, StopReason::Cancelled);
}
