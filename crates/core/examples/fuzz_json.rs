// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-isolated JSON property probe for the source-region fuzz runner.
use purrassist_core::{
    Budget, Outcome, StopReason,
    digest::Digest,
    json,
    schema::{Schema, recovery::RecoveryRequest},
};
use std::io::{self, Read};

fn check_node(node: &json::Node) {
    let encoded = node.to_json();
    let reparsed = json::parse(&encoded).expect("serialized node must parse");
    assert!(node.equivalent(&reparsed), "round-trip changed JSON value");
    assert_eq!(encoded, reparsed.to_json(), "serialization is not stable");
}

fn main() {
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "probe input exceeds 4096 bytes");
    let Ok(input) = std::str::from_utf8(&bytes) else {
        println!("invalid-utf8");
        return;
    };
    if let Ok(node) = json::parse(input) {
        check_node(&node);
    }
    let budget = Budget {
        work: 20_000,
        candidates: 32,
        scratch_bytes: 1_000_000,
        ..Budget::default()
    };
    for source in [
        r#"{"type":"object","required":["name","items"],"properties":{"name":{"type":"string"},"items":{"type":"array","items":{"type":"integer"}}},"additionalProperties":false}"#,
        r#"{"type":"array","items":{"enum":["tea","coffee","water"]},"minItems":1,"uniqueItems":true}"#,
        r#"{"oneOf":[{"type":"integer","minimum":-10,"maximum":10},{"type":"boolean"}]}"#,
        r#"{"type":"object","properties":{"x":{"type":"number"}},"required":["x"]}"#,
    ] {
        let schema = Schema::compile(source).unwrap();
        let result = schema.recover(
            input,
            RecoveryRequest {
                budget: Some(budget),
                ..RecoveryRequest::default()
            },
        );
        assert_eq!(result.receipt.source, Digest::of(&bytes));
        assert!(result.receipt.usage.work <= budget.work);
        assert!(result.receipt.usage.candidates <= budget.candidates);
        assert!(result.receipt.usage.peak_scratch_bytes <= budget.scratch_bytes);
        let nodes = match &result.outcome {
            Outcome::Resolved(node) => {
                assert!(!result.receipt.contains_guesses());
                std::slice::from_ref(node)
            }
            Outcome::BestGuess(node) => {
                assert!(result.receipt.contains_guesses());
                std::slice::from_ref(node)
            }
            Outcome::Ambiguous(nodes) => {
                assert!(nodes.len() > 1);
                assert_eq!(nodes.len(), result.receipt.alternatives.len());
                nodes.as_slice()
            }
            Outcome::Unresolved => &[],
        };
        if !nodes.is_empty() {
            assert_eq!(result.receipt.stopped, StopReason::Completed);
        }
        for node in nodes {
            check_node(node);
            assert!(
                schema.validate(node).unwrap().valid,
                "invalid recovery success"
            );
        }
    }
    println!("ok");
}
