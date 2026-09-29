// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Deterministic native/WASI acceptance transcript replaying the fixed
//! provider wire-format transcripts through [`Pending`], one governor per
//! transcript, with no network, socket, or credential access of any kind.
//!
//! The `(provider, response)` fixtures below are copied verbatim (not
//! imported, since this crate's own tests are a different, un-editable
//! target) from `crates/host/tests/helpers.rs`'s
//! `public_wire_formats_produce_locally_validated_explicit_guesses` test,
//! which is the source of truth for these exact bytes; a comment there names
//! this file as the executable duplicate they must stay identical to.
use purrassist::{
    Budget, Governor, Outcome, Schema,
    digest::Digest,
    wire::{array, object, string},
};
use purrassist_host::helpers::{Pending, Provider};

fn main() {
    let mut transcript = Vec::new();
    // Copied verbatim from crates/host/tests/helpers.rs's
    // `public_wire_formats_produce_locally_validated_explicit_guesses`.
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
        // One governor per transcript: preparation and completion each admit
        // exactly the one helper call this fixture exercises, never sharing
        // budget across providers.
        let mut governor = Governor::new(Budget {
            helper_calls: 1,
            ..Budget::default()
        });
        let schema = Schema::compile(r#"{"type":"integer","minimum":40}"#).expect("schema");
        let pending = Pending::prepare(
            provider,
            "caller-selected-model",
            "forty two",
            &schema,
            512,
            &mut governor,
        )
        .expect("prepare");
        let result = pending.complete(200, response);
        let (status, values) = match result.outcome {
            Outcome::Resolved(node) => ("resolved", vec![node]),
            Outcome::BestGuess(node) => ("best_guess", vec![node]),
            Outcome::Ambiguous(nodes) => ("ambiguous", nodes),
            Outcome::Unresolved => ("unresolved", Vec::new()),
        };
        transcript.push(object([
            ("provider", string(format!("{provider:?}"))),
            ("status", string(status)),
            ("values", array(values)),
            ("receipt", result.receipt.to_node()),
        ]));
    }
    let output = array(transcript).to_json();
    println!("{}", Digest::of(output.as_bytes()));
    println!("{output}");
}
