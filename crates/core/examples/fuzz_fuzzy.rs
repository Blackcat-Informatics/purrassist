// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-isolated fuzzy-substring-edit probe for the source-region fuzz
//! runner (`scripts/fuzz.py --target fuzzy`).
//!
//! Bounded stdin is split on the ASCII record-separator control byte
//! (`\u{1e}`) into four UTF-8 fields: a source text, a needle, a
//! replacement, and a decimal maximum-edit-distance (defaulting to `1`,
//! clamped to eight). Checks: planning never panics; every `Resolved`,
//! `BestGuess`, or `Ambiguous` plan applies, and applying its inverse to
//! that output restores the exact original source (apply-then-invert
//! identity); an `Ambiguous` outcome's alternative count matches the
//! receipt's alternative ledger count.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    edits::{EditPlan, locate_fuzzy_governed},
};
use std::io::{self, Read};

fn verify_roundtrip(plan: &EditPlan, source: &str) {
    let applied = plan.apply(source, Budget::default());
    let Outcome::Resolved(applied) = applied.outcome else {
        assert!(
            matches!(
                applied.receipt.stopped,
                StopReason::BudgetExceeded(_) | StopReason::Cancelled
            ),
            "a plan produced by locate_fuzzy failed to apply: {:?}",
            applied.receipt.stopped
        );
        return;
    };
    let restored = applied.inverse.apply(&applied.text, Budget::default());
    let Outcome::Resolved(restored) = restored.outcome else {
        panic!(
            "invert-apply failed for a locate_fuzzy plan: {:?}",
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
    let mut fields = input.split('\u{1e}');
    let source = fields.next().unwrap_or("");
    let needle = fields.next().unwrap_or("");
    let replacement = fields.next().unwrap_or("");
    let max_edits: usize = fields
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
        .min(8);

    let mut governor = Governor::new(Budget::default());
    let result = locate_fuzzy_governed(source, needle, replacement, max_edits, &mut governor);
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
