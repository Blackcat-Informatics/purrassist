// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-isolated unified-diff planning probe for the source-region fuzz
//! runner (`scripts/fuzz.py --target unified`).
//!
//! Bounded stdin is split on the ASCII record-separator control byte
//! (`\u{1e}`) into two UTF-8 fields: a source buffer and a unified-diff
//! patch text. Checks: planning never panics; every `Resolved`,
//! `BestGuess`, or `Ambiguous` plan applies, and applying its inverse to
//! that output restores the exact original source (apply-then-invert
//! identity); an `Ambiguous` outcome's alternative count matches the
//! receipt's alternative ledger count.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    edits::{EditPlan, plan_unified_governed},
};
use std::io::{self, Read};

fn fields(input: &str) -> (&str, &str) {
    match input.split_once('\u{1e}') {
        Some((a, b)) => (a, b),
        None => (input, ""),
    }
}

fn verify_roundtrip(plan: &EditPlan, source: &str) {
    let applied = plan.apply(source, Budget::default());
    let Outcome::Resolved(applied) = applied.outcome else {
        assert!(
            matches!(
                applied.receipt.stopped,
                StopReason::BudgetExceeded(_) | StopReason::Cancelled
            ),
            "a plan produced by plan_unified failed to apply: {:?}",
            applied.receipt.stopped
        );
        return;
    };
    let restored = applied.inverse.apply(&applied.text, Budget::default());
    let Outcome::Resolved(restored) = restored.outcome else {
        panic!(
            "invert-apply failed for a plan_unified plan: {:?}",
            restored.receipt.stopped
        );
    };
    assert_eq!(
        restored.text, source,
        "apply-then-invert did not restore the original source"
    );
}

fn main() {
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "probe input exceeds 4096 bytes");
    let Ok(input) = std::str::from_utf8(&bytes) else {
        println!("invalid-utf8");
        return;
    };
    let (source, patch_text) = fields(input);
    let mut governor = Governor::new(Budget::default());
    let result = plan_unified_governed(source, patch_text, &mut governor);
    match result.outcome {
        Outcome::Resolved(plan) | Outcome::BestGuess(plan) => verify_roundtrip(&plan, source),
        Outcome::Ambiguous(plans) => {
            assert_eq!(plans.len(), result.receipt.alternatives.len());
            for plan in &plans {
                verify_roundtrip(plan, source);
            }
        }
        Outcome::Unresolved => {}
    }
    println!("ok");
}
