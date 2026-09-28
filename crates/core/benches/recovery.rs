// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Small reproducible quality/latency baseline; no external benchmark framework.
use purrassist_core::{
    Outcome,
    schema::{Schema, recovery::RecoveryRequest},
    wire::{array, integer, object, string},
};
use std::{hint::black_box, time::Instant};

fn main() {
    let scalar = Schema::compile(r#"{"type":"integer"}"#).expect("scalar schema");
    let object_schema = Schema::compile(r#"{"type":"object","required":["temperature"],"properties":{"temperature":{"type":"number"}},"additionalProperties":false}"#).expect("object schema");
    let duplicate_schema = Schema::compile(
        r#"{"type":"object","required":["x"],"properties":{"x":{"type":"integer"}}}"#,
    )
    .expect("duplicate schema");
    let reject = Schema::compile("false").expect("reject schema");
    let mut results = Vec::new();
    for (name, schema, input, status, expected) in [
        ("exact", &scalar, "42", "resolved", vec!["42"]),
        ("nested", &scalar, "[[[[42]]]]", "resolved", vec!["42"]),
        (
            "composed",
            &object_schema,
            "```json\n[[{temperatur: '42',}]]\n```",
            "resolved",
            vec![r#"{"temperature":42}"#],
        ),
        (
            "duplicate",
            &duplicate_schema,
            r#"{"x":1,"x":2}"#,
            "ambiguous",
            vec![r#"{"x":1}"#, r#"{"x":2}"#],
        ),
        (
            "inferred",
            &duplicate_schema,
            "{}",
            "best_guess",
            vec![r#"{"x":0}"#],
        ),
        ("no_candidate", &reject, "42", "unresolved", vec![]),
    ] {
        let mut samples = Vec::new();
        let mut peak_scratch = 0;
        let mut peak_work = 0;
        let mut correct = 0;
        let mut wrong_corrections = 0;
        for _ in 0..200 {
            let start = Instant::now();
            let result = schema.recover(black_box(input), RecoveryRequest::default());
            samples.push(u64::try_from(start.elapsed().as_nanos()).expect("duration fits u64"));
            peak_scratch = peak_scratch.max(result.receipt.usage.peak_scratch_bytes);
            peak_work = peak_work.max(result.receipt.usage.work);
            let (actual_status, mut actual) = match result.outcome {
                Outcome::Resolved(value) => ("resolved", vec![value.to_json()]),
                Outcome::BestGuess(value) => ("best_guess", vec![value.to_json()]),
                Outcome::Ambiguous(values) => (
                    "ambiguous",
                    values
                        .iter()
                        .map(purrassist_core::json::Node::to_json)
                        .collect(),
                ),
                Outcome::Unresolved => ("unresolved", Vec::new()),
            };
            actual.sort();
            let mut expected = expected.clone();
            expected.sort_unstable();
            let matches = actual_status == status && actual == expected;
            correct += u64::from(matches);
            wrong_corrections +=
                u64::from(!matches && matches!(actual_status, "resolved" | "best_guess"));
        }
        samples.sort_unstable();
        assert_eq!(correct, 200, "quality regression in {name}");
        results.push(object([
            ("case", string(name)),
            ("samples", integer(200)),
            ("correct", integer(correct)),
            ("wrong_corrections", integer(wrong_corrections)),
            ("median_ns", integer(samples[100])),
            ("p95_ns", integer(samples[189])),
            ("governor_peak_scratch_bytes", integer(peak_scratch)),
            ("governor_peak_work", integer(peak_work)),
        ]));
    }
    println!("{}", array(results).to_json());
}
