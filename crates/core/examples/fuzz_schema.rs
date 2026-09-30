// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-isolated JSON Schema compile/witness/recovery probe for the
//! source-region fuzz runner (`scripts/fuzz.py --target schema`).
//!
//! Bounded stdin is split on the ASCII record-separator control byte
//! (`\u{1e}`) into two UTF-8 fields: a schema source and an instance text.
//! Checks: compilation never panics; a generated witness always validates
//! against its own schema; recovery never returns a `Resolved`/`BestGuess`
//! value, or an `Ambiguous` alternative, that fails validation against its
//! own schema (the schema engine is the oracle here, not an independent
//! implementation); receipt usage stays within the supplied budget.
use purrassist_core::{
    Budget, Governor, Outcome,
    schema::{Schema, algebra::WitnessOutcome, recovery::RecoveryRequest},
};
use std::io::{self, Read};

fn fields(input: &str) -> (&str, &str) {
    match input.split_once('\u{1e}') {
        Some((a, b)) => (a, b),
        None => (input, ""),
    }
}

fn main() {
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "probe input exceeds 4096 bytes");
    let Ok(input) = std::str::from_utf8(&bytes) else {
        println!("invalid-utf8");
        return;
    };
    let (schema_source, instance) = fields(input);
    let Ok(schema) = Schema::compile(schema_source) else {
        println!("ok");
        return;
    };
    let budget = Budget {
        work: 20_000,
        candidates: 32,
        scratch_bytes: 1_000_000,
        ..Budget::default()
    };
    let mut witness_governor = Governor::new(budget);
    if let Ok(WitnessOutcome::Witness(node)) =
        schema.generate_witness_governed(&mut witness_governor)
    {
        assert!(
            schema.validate(&node).unwrap().valid,
            "generated witness {:?} fails its own schema {schema_source:?}",
            node.to_json()
        );
    }
    let mut recovery_governor = Governor::new(budget);
    let result = schema.recover_governed(
        instance,
        RecoveryRequest {
            budget: Some(budget),
            ..RecoveryRequest::default()
        },
        &mut recovery_governor,
    );
    assert!(result.receipt.usage.work <= budget.work);
    assert!(result.receipt.usage.candidates <= budget.candidates);
    assert!(result.receipt.usage.peak_scratch_bytes <= budget.scratch_bytes);
    match result.outcome {
        Outcome::Resolved(node) | Outcome::BestGuess(node) => {
            assert!(
                schema.validate(&node).unwrap().valid,
                "recovery of {instance:?} against {schema_source:?} resolved to an \
                 instance that fails its own schema: {:?}",
                node.to_json()
            );
        }
        Outcome::Ambiguous(nodes) => {
            assert_eq!(nodes.len(), result.receipt.alternatives.len());
            for node in &nodes {
                assert!(
                    schema.validate(node).unwrap().valid,
                    "ambiguous recovery alternative for {instance:?} against \
                     {schema_source:?} fails its own schema: {:?}",
                    node.to_json()
                );
            }
        }
        Outcome::Unresolved => {}
    }
    println!("ok");
}
