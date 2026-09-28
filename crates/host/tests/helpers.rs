// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Fixed public-provider transcripts; no model, network, or credential is used.
use purrassist::{Budget, Outcome, Schema, json};
use purrassist_host::helpers::{Pending, Provider};

fn prepare(provider: Provider) -> Pending {
    Pending::prepare(
        provider,
        "caller-selected-model",
        "forty two",
        &Schema::compile(r#"{"type":"integer","minimum":40}"#).unwrap(),
        512,
        Budget {
            helper_calls: 1,
            ..Budget::default()
        },
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
fn helpers_are_disabled_by_default_and_refusals_are_not_reinterpreted() {
    assert!(
        Pending::prepare(
            Provider::OpenAiResponses,
            "caller-model",
            "input",
            &Schema::compile("true").unwrap(),
            512,
            Budget::default()
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
