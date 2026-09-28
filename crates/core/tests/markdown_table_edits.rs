// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source preservation, exact undo, and structural rejection for GFM cell edits.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason, contract::Resource, markdown::Document,
};

#[test]
fn cell_edits_preserve_nested_prefixes_margins_and_undo() {
    for (source, path, old, new, row, column) in [
        (
            "| café | B |\r\n| :--- | ---: |\r\n| a\\|b | keep |\r\n",
            vec![0],
            "a\\|b",
            "**iced**",
            1,
            0,
        ),
        (
            "> | A | B |\r\n> | --- | --- |\r\n> | tea | keep |\r\n",
            vec![0, 0],
            "tea",
            "café\\|cold",
            1,
            0,
        ),
        (
            "- | A | B |\n  | --- | --- |\n  | tea | keep |\n",
            vec![0, 0],
            "A",
            "Drink",
            0,
            0,
        ),
        (
            "A | B\n--- | ---\ntea | keep",
            vec![0],
            "keep",
            "**cold**",
            1,
            1,
        ),
        (
            "| A | B |\n| --- | --- |\n|   | keep |\n",
            vec![0],
            "|   |",
            "|   tea|",
            1,
            0,
        ),
    ] {
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(source, &mut g).unwrap();
        let replacement = if old == "|   |" { "tea" } else { new };
        let plan = doc
            .replace_table_cell(&path, row, column, replacement, &mut g)
            .unwrap();
        assert_eq!(plan.edits.len(), 1);
        let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
            panic!("apply")
        };
        assert_eq!(applied.text, source.replacen(old, new, 1));
        let Outcome::Resolved(undone) = applied
            .inverse
            .apply(&applied.text, Budget::default())
            .outcome
        else {
            panic!("undo")
        };
        assert_eq!(undone.text, source);
        assert_eq!(
            plan.apply(&format!("{source}changed"), Budget::default())
                .outcome,
            Outcome::Unresolved
        );
        let updated = Document::parse(&applied.text, &mut g).unwrap();
        assert!(
            updated
                .replace_table_cell(&path, row, column, replacement, &mut g)
                .unwrap()
                .edits
                .is_empty()
        );
    }
}

#[test]
fn invalid_cells_and_structural_changes_fail_without_a_plan() {
    let source = "| A | B |\n| --- | --- |\n| tea |\n| a | b | extra |\n";
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(source, &mut g).unwrap();
    for (path, row, column, value) in [
        (vec![], 0, 0, "x"),
        (vec![99], 0, 0, "x"),
        (vec![0], 99, 0, "x"),
        (vec![0], 1, 1, "x"),
        (vec![0], 2, 2, "x"),
        (vec![0], 0, 0, "a|b"),
        (vec![0], 0, 0, "a\nb"),
        (vec![0], 0, 0, "a\rb"),
    ] {
        assert!(matches!(
            doc.replace_table_cell(&path, row, column, value, &mut g),
            Err(StopReason::InvalidInput(_))
        ));
    }
    let mut exhausted = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert_eq!(
        doc.replace_table_cell(&[0], 0, 0, "x", &mut exhausted),
        Err(StopReason::BudgetExceeded(Resource::Work))
    );
    let mut cancelled = Governor::new(Budget::default());
    cancelled.cancel();
    assert!(
        doc.replace_table_cell(&[0], 0, 0, "x", &mut cancelled)
            .is_err()
    );
    // Escaping a boundary pipe would merge adjacent cells and must be rejected.
    let doc = Document::parse("|A|B|\n|-|-|\n", &mut g).unwrap();
    assert!(doc.replace_table_cell(&[0], 0, 0, "x\\", &mut g).is_err());
}
