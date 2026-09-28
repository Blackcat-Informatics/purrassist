// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Reproducible mutation probes for parser/repair composition and preservation.
use purrassist_core::{
    Budget, Outcome, json,
    schema::{Schema, recovery::RecoveryRequest},
};

#[test]
fn bounded_mutations_never_return_a_schema_invalid_success() {
    let schema = Schema::compile(r#"{"type":"object","required":["name","items"],"properties":{"name":{"type":"string"},"items":{"type":"array","items":{"type":"integer"}}},"additionalProperties":false}"#).unwrap();
    let seed = br#"{"name":"tea","items":[1,2,3]}"#;
    let request = RecoveryRequest {
        budget: Some(Budget {
            work: 20_000,
            candidates: 32,
            scratch_bytes: 1_000_000,
            ..Budget::default()
        }),
        ..RecoveryRequest::default()
    };
    let mut state = 0x5a17_27ee_614d_934b_u64;
    for _ in 0..2000 {
        let mut bytes = seed.to_vec();
        for _ in 0..3 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let position = usize::try_from(state >> 32).unwrap() % bytes.len();
            let replacement = u8::try_from(state & 0x7f).unwrap();
            bytes[position] = replacement;
        }
        let input = String::from_utf8(bytes).unwrap();
        if let Ok(node) = json::parse(&input) {
            let output = json::parse(&node.to_json()).unwrap();
            assert!(node.equivalent(&output));
        }
        let result = schema.recover(&input, request);
        let nodes = match result.outcome {
            Outcome::Resolved(node) | Outcome::BestGuess(node) => vec![node],
            Outcome::Ambiguous(nodes) => nodes,
            Outcome::Unresolved => Vec::new(),
        };
        for node in nodes {
            assert!(schema.validate(&node).unwrap().valid, "input: {input:?}");
        }
    }
}
