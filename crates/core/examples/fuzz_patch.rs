// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-isolated patch-algebra probe for the source-region fuzz runner
//! (`scripts/fuzz.py --target patch`).
//!
//! Bounded stdin is split on the ASCII record-separator control byte
//! (`\u{1e}`) into three UTF-8 fields: a base text and two independently
//! modified texts. Checks: [`Patch::diff`], [`Patch::apply`],
//! [`Patch::invert`], [`Patch::commute`], and [`Patch::rebase`] never
//! panic; apply-then-invert restores the exact base text; when two patches
//! derived against the same base commute, applying either one first and
//! the other rebased onto it reaches the same final text regardless of
//! order (the commute law); when they do not commute, `rebase` refuses.
use purrassist_core::{Budget, Governor, Outcome, StopReason, patch::Patch};
use std::io::{self, Read};

fn fields3(input: &str) -> (&str, &str, &str) {
    let mut parts = input.split('\u{1e}');
    (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
        parts.next().unwrap_or(""),
    )
}

/// Apply `patch` to `source`, tolerating only an explicit resource stop
/// (never `InvalidInput`, since every patch here is derived from its own
/// exact base by this same probe).
fn apply_ok(patch: &Patch, source: &str) -> Option<purrassist_core::patch::AppliedEdit> {
    let result = patch.apply(source, Budget::default());
    match result.outcome {
        Outcome::Resolved(applied) => Some(applied),
        Outcome::Unresolved => {
            assert!(
                matches!(
                    result.receipt.stopped,
                    StopReason::BudgetExceeded(_) | StopReason::Cancelled
                ),
                "a well-formed patch failed to apply: {:?}",
                result.receipt.stopped
            );
            None
        }
        Outcome::BestGuess(_) | Outcome::Ambiguous(_) => {
            unreachable!("Patch::apply never guesses or branches")
        }
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
    let (base, modified_a, modified_b) = fields3(input);

    let mut diff_governor = Governor::new(Budget::default());
    let Ok(p) = Patch::diff(base, modified_a, &mut diff_governor) else {
        println!("ok");
        return;
    };
    if let Some(applied) = apply_ok(&p, base) {
        assert_eq!(
            applied.text, modified_a,
            "diff patch does not reproduce its own target text"
        );
        if let Some(restored) = apply_ok(&applied.inverse, &applied.text) {
            assert_eq!(
                restored.text, base,
                "apply-then-invert did not restore the original base text"
            );
        }
    }

    let mut other_diff_governor = Governor::new(Budget::default());
    let Ok(q) = Patch::diff(base, modified_b, &mut other_diff_governor) else {
        println!("ok");
        return;
    };
    let mut commute_governor = Governor::new(Budget::default());
    match p.commute(&q, base, &mut commute_governor) {
        Ok(Some((p_onto_q, q_onto_p))) => {
            if let (Some(via_q), Some(via_p)) = (apply_ok(&q, base), apply_ok(&p, base))
                && let (Some(first), Some(second)) = (
                    apply_ok(&p_onto_q, &via_q.text),
                    apply_ok(&q_onto_p, &via_p.text),
                )
            {
                assert_eq!(
                    first.text, second.text,
                    "commute law violated for base {base:?}"
                );
            }
        }
        Ok(None) => {
            let mut rebase_governor = Governor::new(Budget::default());
            assert!(
                p.rebase(&q, base, &mut rebase_governor).is_err(),
                "non-commuting patches must refuse rebase"
            );
        }
        Err(_) => {}
    }
    println!("ok");
}
