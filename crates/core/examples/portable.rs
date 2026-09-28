// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Deterministic native/WASI acceptance transcript using the actual core engines.
use purrassist_core::{
    Effort, Governor, Outcome,
    digest::Digest,
    matching,
    schema::{Schema, recovery::RecoveryRequest},
    text::{self, Normalization},
    wire::{array, object, string},
};

fn main() {
    let schema = Schema::compile(r#"{"type":"object","required":["temperature","drink"],"properties":{"temperature":{"type":"number"},"drink":{"enum":["tea","coffee"]}},"additionalProperties":false}"#).expect("schema");
    let mut transcript = Vec::new();
    for source in [
        "```json\n[[{temperatur: '42', drink: 'Tea',}]]\n```",
        r#"{"temperature":42,"drink":"tea"}"#,
        r#"{"temperature":1,"temperature":2,"drink":"tea"}"#,
    ] {
        let result = schema.recover(source, RecoveryRequest::default());
        let (status, values) = match result.outcome {
            Outcome::Resolved(node) => ("resolved", vec![node]),
            Outcome::BestGuess(node) => ("best_guess", vec![node]),
            Outcome::Ambiguous(nodes) => ("ambiguous", nodes),
            Outcome::Unresolved => ("unresolved", Vec::new()),
        };
        transcript.push(object([
            ("source", string(result.receipt.source.to_string())),
            ("status", string(status)),
            ("values", array(values)),
            (
                "rules",
                array(
                    result
                        .receipt
                        .evidence
                        .iter()
                        .map(|entry| string(&entry.rule)),
                ),
            ),
            ("termination", string(result.receipt.stopped.to_string())),
        ]));
    }
    let mut governor = Governor::new(Effort::Standard.budget());
    let distance = matching::damerau_levenshtein("CA", "ABC", &mut governor).expect("distance");
    assert_eq!(distance, 2);
    assert_eq!(text::comparison_key("Straße"), "strasse");
    transcript.push(string(text::normalize("\u{212b}각ﬃ", Normalization::Nfkc)));
    let output = array(transcript).to_json();
    println!("{}", Digest::of(output.as_bytes()));
    println!("{output}");
}
