// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Approximate selection and unified patch preservation regressions.
use purrassist_core::{
    Budget, Outcome, StopReason,
    contract::Resource,
    edits::{locate_fuzzy, plan_unified},
};

#[test]
fn fuzzy_minima_preserve_ties_and_exact_precedence() {
    let result = locate_fuzzy("hello world", "wurld", "earth", 1, Budget::default());
    let Outcome::BestGuess(plan) = result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(plan.edits[0].range, 6..11);
    assert_eq!(result.receipt.inferences.len(), 1);
    assert!(matches!(
        locate_fuzzy("world wurld", "wurld", "earth", 1, Budget::default()).outcome,
        Outcome::Resolved(_)
    ));
    let Outcome::Ambiguous(plans) =
        locate_fuzzy("cat bat", "hat", "dog", 1, Budget::default()).outcome
    else {
        panic!("tie lost")
    };
    // Deleting the needle's initial h also makes each "at" an equally good region.
    assert_eq!(
        plans
            .iter()
            .map(|plan| plan.edits[0].range.clone())
            .collect::<Vec<_>>(),
        vec![0..3, 1..3, 4..7, 5..7]
    );
    let result = locate_fuzzy("猫咪", "猫猫", "犬", 1, Budget::default());
    assert!(matches!(result.outcome, Outcome::Ambiguous(_)));
}

#[test]
fn exhaustion_cannot_accept_a_partial_match() {
    let budget = Budget {
        candidates: 1,
        ..Budget::default()
    };
    let result = locate_fuzzy("cat cat", "cat", "dog", 1, budget);
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(Resource::Candidates)
    );
}

#[test]
fn shifted_patch_preserves_crlf_and_undo() {
    let source = "prefix\r\none\r\ntwo\r\ntail\r\n";
    let patch = "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n one\n-two\n+three\n tail\n";
    let result = plan_unified(source, patch, Budget::default());
    let Outcome::Resolved(plan) = result.outcome else {
        panic!("{result:?}")
    };
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("application")
    };
    assert_eq!(applied.text, "prefix\r\none\r\nthree\r\ntail\r\n");
    let Outcome::Resolved(undo) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo")
    };
    assert_eq!(undo.text, source);
}

#[test]
fn patch_context_ambiguity_and_global_order_are_explicit() {
    let patch = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+A\n";
    assert!(matches!(
        plan_unified("a\na\n", patch, Budget::default()).outcome,
        Outcome::Ambiguous(_)
    ));
    let ordered = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+A\n@@ -2 +2 @@\n-b\n+B\n";
    let result = plan_unified("b\na\nb\n", ordered, Budget::default());
    let Outcome::Resolved(plan) = result.outcome else {
        panic!("{result:?}")
    };
    let Outcome::Resolved(applied) = plan.apply("b\na\nb\n", Budget::default()).outcome else {
        panic!("apply")
    };
    assert_eq!(applied.text, "b\nA\nB\n");
}

#[test]
fn insertion_deletion_and_missing_final_newline() {
    for (source, patch, expected) in [
        ("", "--- /dev/null\n+++ b/x\n@@ -0,0 +1 @@\n+new\n", "new\n"),
        ("old\n", "--- a/x\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n", ""),
        (
            "old",
            "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n",
            "new",
        ),
    ] {
        let result = plan_unified(source, patch, Budget::default());
        let Outcome::Resolved(plan) = result.outcome else {
            panic!("{result:?}")
        };
        let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
            panic!("apply")
        };
        assert_eq!(applied.text, expected);
    }
}

#[test]
fn malformed_hunk_never_returns_a_partial_edit() {
    for patch in [
        "--- a/x\n+++ b/x\n@@ -1,2 +1 @@\n-a\n+b\n",
        "--- a/x\n+++ b/x\n@@ -0 +1 @@\n-a\n+b\n",
        "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n--- a/y\n+++ b/y\n",
    ] {
        let result = plan_unified("a\n", patch, Budget::default());
        assert_eq!(result.outcome, Outcome::Unresolved);
        assert!(matches!(
            result.receipt.stopped,
            StopReason::InvalidInput(_)
        ));
    }
}
