// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Fixed public-provider transcripts; no model, network, or credential is used.
use purrassist::{
    Budget, CancellationToken, Governor, Outcome, Schema, StopReason, contract::Resource, json,
};
use purrassist_host::helpers::{Pending, Provider};

fn prepare(provider: Provider) -> Pending {
    let mut governor = Governor::new(Budget {
        helper_calls: 1,
        ..Budget::default()
    });
    Pending::prepare(
        provider,
        "caller-selected-model",
        "forty two",
        &Schema::compile(r#"{"type":"integer","minimum":40}"#).unwrap(),
        512,
        &mut governor,
    )
    .unwrap()
}
#[test]
fn public_wire_formats_produce_locally_validated_explicit_guesses() {
    for (provider, response) in [
        (
            Provider::OpenAiResponses,
            r#"{"status":"completed","output":[{"type":"reasoning","summary":[]},{"type":"message","content":[{"type":"output_text","text":"[[42]]"}]}]}"#,
        ),
        (
            Provider::AnthropicMessages,
            r#"{"stop_reason":"end_turn","content":[{"type":"text","text":"42"}]}"#,
        ),
        (
            Provider::ChatCompletions,
            r#"{"choices":[{"finish_reason":"stop","message":{"content":"42"}}]}"#,
        ),
    ] {
        let pending = prepare(provider);
        assert!(json::parse(&pending.request().body).is_ok());
        let result = pending.complete(200, response);
        let Outcome::BestGuess(value) = result.outcome else {
            panic!("{result:?}");
        };
        assert_eq!(value.to_json(), "42");
        assert!(result.receipt.contains_guesses());
        assert_eq!(result.receipt.usage.helper_calls, 1);
        assert!(result.receipt.usage.work <= result.receipt.budget.work);
        assert!(result.receipt.usage.peak_scratch_bytes <= result.receipt.budget.scratch_bytes);
    }
}
#[test]
fn completed_usage_includes_every_phase_scratch_and_candidates() {
    // `Pending::complete` retains one cumulative governor through request
    // preparation, response extraction, and schema recovery. Preparation alone charges
    // no `Resource::Candidates` (recovery is what proposes candidates), so a
    // nonzero count here proves the local-recovery phase's usage was folded
    // in, not dropped; likewise `total_scratch_charged` (a monotone sum,
    // unlike the high-water `peak_scratch_bytes` also asserted elsewhere)
    // must reflect real charges from more than one phase.
    let result = prepare(Provider::AnthropicMessages).complete(
        200,
        r#"{"stop_reason":"end_turn","content":[{"type":"text","text":"42"}]}"#,
    );
    assert!(matches!(result.outcome, Outcome::BestGuess(_)));
    assert!(result.receipt.usage.candidates > 0);
    assert!(result.receipt.usage.total_scratch_charged > 0);
    assert!(result.receipt.usage.total_scratch_charged >= result.receipt.usage.peak_scratch_bytes);
}

#[test]
fn pending_completion_retains_cancellation_and_prior_candidate_usage() {
    let schema = Schema::compile("true").unwrap();
    let token = CancellationToken::new();
    let mut governor = Governor::with_cancellation(
        Budget {
            helper_calls: 1,
            ..Budget::default()
        },
        token.clone(),
    );
    let pending = Pending::prepare(
        Provider::AnthropicMessages,
        "example-model",
        "42",
        &schema,
        16,
        &mut governor,
    )
    .unwrap();
    token.cancel();
    let stopped = pending.complete(
        200,
        r#"{"stop_reason":"end_turn","content":[{"type":"text","text":"42"}]}"#,
    );
    assert_eq!(stopped.outcome, Outcome::Unresolved);
    assert_eq!(stopped.receipt.stopped, StopReason::Cancelled);
    assert_eq!(stopped.receipt.usage, governor.usage());

    let mut governor = Governor::new(Budget {
        helper_calls: 1,
        candidates: 1,
        ..Budget::default()
    });
    governor.charge(Resource::Candidates, 1).unwrap();
    let pending = Pending::prepare(
        Provider::AnthropicMessages,
        "example-model",
        "42",
        &schema,
        16,
        &mut governor,
    )
    .unwrap();
    let exhausted = pending.complete(
        200,
        r#"{"stop_reason":"end_turn","content":[{"type":"text","text":"42"}]}"#,
    );
    assert_eq!(exhausted.outcome, Outcome::Unresolved);
    assert_eq!(
        exhausted.receipt.stopped,
        StopReason::BudgetExceeded(Resource::Candidates)
    );
    assert_eq!(exhausted.receipt.usage.candidates, 1);
}

#[test]
fn helper_ledger_admission_is_atomic_and_never_yields_partial_success() {
    let mut governor = Governor::new(Budget {
        helper_calls: 1,
        evidence_entries: 2,
        ..Budget::default()
    });
    let pending = Pending::prepare(
        Provider::AnthropicMessages,
        "example-model",
        "42",
        &Schema::compile("true").unwrap(),
        16,
        &mut governor,
    )
    .unwrap();
    let result = pending.complete(
        200,
        r#"{"stop_reason":"end_turn","content":[{"type":"text","text":"42"}]}"#,
    );
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(Resource::EvidenceEntries)
    );
    assert_eq!(result.receipt.evidence, Vec::new());
    assert_eq!(result.receipt.inferences, Vec::new());
    assert_eq!(result.receipt.usage.evidence_entries, 1);
    assert_eq!(result.receipt.sources[0].text(), Some("42"));
}
#[test]
fn helpers_are_disabled_by_default_and_refusals_are_not_reinterpreted() {
    let mut governor = Governor::new(Budget::default());
    assert!(
        Pending::prepare(
            Provider::OpenAiResponses,
            "caller-model",
            "input",
            &Schema::compile("true").unwrap(),
            512,
            &mut governor,
        )
        .is_err()
    );
    let refusal = r#"{"status":"completed","output":[{"type":"message","content":[{"type":"refusal","refusal":"declined"}]}]}"#;
    assert_eq!(
        prepare(Provider::OpenAiResponses)
            .complete(200, refusal)
            .outcome,
        Outcome::Unresolved
    );
    assert_eq!(
        prepare(Provider::AnthropicMessages)
            .complete(429, "{}")
            .outcome,
        Outcome::Unresolved
    );
    assert_eq!(
        prepare(Provider::ChatCompletions)
            .complete(
                200,
                r#"{"choices":[{"finish_reason":"length","message":{"content":"42"}}]}"#
            )
            .outcome,
        Outcome::Unresolved
    );
}
#[test]
fn helper_preparation_exhaustion_yields_budget_exceeded_and_no_request() {
    // Enough helper calls to pass that check, but not enough scratch budget
    // for the schema/prompt precharge: preparation must fail before
    // allocating the request body, not with a partially built request.
    let mut governor = Governor::new(Budget {
        helper_calls: 1,
        scratch_bytes: 4,
        ..Budget::default()
    });
    let outcome = Pending::prepare(
        Provider::AnthropicMessages,
        "caller-selected-model",
        "forty two",
        &Schema::compile(r#"{"type":"integer","minimum":40}"#).unwrap(),
        512,
        &mut governor,
    );
    assert_eq!(
        outcome.err(),
        Some(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
    // Nothing was admitted beyond the checks preceding the precharge: no
    // request body was serialized, and a fresh helper call is still refused
    // by the now-sticky governor, never silently retried.
    assert!(
        Pending::prepare(
            Provider::AnthropicMessages,
            "caller-selected-model",
            "forty two",
            &Schema::compile(r#"{"type":"integer"}"#).unwrap(),
            512,
            &mut governor,
        )
        .is_err()
    );
}
