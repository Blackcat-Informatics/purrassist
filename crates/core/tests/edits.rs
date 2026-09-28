// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Exact preservation, overlap rejection, and conditional undo for text edits.
use purrassist_core::{
    Budget, Outcome,
    digest::Digest,
    edits::{EditPlan, TextEdit, locate},
};

#[test]
fn multiple_edits_undo_exact_bytes_and_reject_stale_sources() {
    let source = "αβ\r\nhello\r\nend";
    let plan = EditPlan {
        source: Digest::of(source.as_bytes()),
        edits: vec![
            TextEdit {
                range: 6..11,
                replacement: "world!".into(),
            },
            TextEdit {
                range: 0..2,
                replacement: "Ω".into(),
            },
        ],
    };
    let result = plan.apply(source, Budget::default());
    let Outcome::Resolved(applied) = result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(applied.text, "Ωβ\r\nworld!\r\nend");
    let result = applied.inverse.apply(&applied.text, Budget::default());
    let Outcome::Resolved(undone) = result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(undone.text, source);
    assert_eq!(
        applied.inverse.apply("changed", Budget::default()).outcome,
        Outcome::Unresolved
    );
}
#[test]
fn adjacent_deletions_have_a_valid_inverse() {
    let source = "abcdef";
    let plan = EditPlan {
        source: Digest::of(source.as_bytes()),
        edits: vec![
            TextEdit {
                range: 1..3,
                replacement: String::new(),
            },
            TextEdit {
                range: 3..5,
                replacement: String::new(),
            },
        ],
    };
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply failed");
    };
    assert_eq!(applied.text, "af");
    let Outcome::Resolved(undone) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo failed");
    };
    assert_eq!(undone.text, source);
}
#[test]
fn overlapping_matches_and_non_scalar_boundaries_are_not_arbitrary_edits() {
    let Outcome::Ambiguous(plans) = locate("aaa", "aa", "x", Budget::default()).outcome else {
        panic!("missing overlapping matches");
    };
    assert_eq!(plans.len(), 2);
    let source = "αβ";
    let plan = EditPlan {
        source: Digest::of(source.as_bytes()),
        edits: vec![TextEdit {
            range: 1..2,
            replacement: "x".into(),
        }],
    };
    assert_eq!(
        plan.apply(source, Budget::default()).outcome,
        Outcome::Unresolved
    );
}
