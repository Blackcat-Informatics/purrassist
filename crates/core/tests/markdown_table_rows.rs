// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Nested table row edits, newline preservation, rejection, and conditional undo.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    edits::EditPlan,
    markdown::{Document, TableRowEdit},
};

fn applied(plan: &EditPlan, source: &str) -> String {
    let Outcome::Resolved(result) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply")
    };
    let Outcome::Resolved(undo) = result
        .inverse
        .apply(&result.text, Budget::default())
        .outcome
    else {
        panic!("undo")
    };
    assert_eq!(undo.text, source);
    assert_eq!(
        plan.apply(&format!("{source}changed"), Budget::default())
            .outcome,
        Outcome::Unresolved
    );
    result.text
}

#[test]
fn insertion_inherits_containers_newlines_and_preserves_existing_rows() {
    for (source, path, index, expected) in [
        (
            "> | A |\r\n> | --- |",
            vec![0, 0],
            0,
            "> | A |\r\n> | --- |\r\n> | **café** |",
        ),
        (
            "| A |\n| --- |\n",
            vec![0],
            0,
            "| A |\n| --- |\n| **café** |\n",
        ),
        (
            "| A |\r\n| --- |\r\n| old |\r\n",
            vec![0],
            0,
            "| A |\r\n| --- |\r\n| **café** |\r\n| old |\r\n",
        ),
        (
            "| A |\r| --- |\r| old |",
            vec![0],
            1,
            "| A |\r| --- |\r| old |\r| **café** |",
        ),
        (
            "> | A |\n> | --- |",
            vec![0, 0],
            0,
            "> | A |\n> | --- |\n> | **café** |",
        ),
        (
            "- | A |\n  | --- |\n  | old |\n\nNext\n",
            vec![0, 0],
            1,
            "- | A |\n  | --- |\n  | old |\n  | **café** |\n\nNext\n",
        ),
        (
            "> - | A |\r\n>   | --- |\r\n>   | old |\r\n",
            vec![0, 0, 0],
            0,
            "> - | A |\r\n>   | --- |\r\n>   | **café** |\r\n>   | old |\r\n",
        ),
        (
            "A | B\n--- | ---\na | b",
            vec![0],
            1,
            "A | B\n--- | ---\na | b\n| **café** | a\\|b |",
        ),
    ] {
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(source, &mut g).unwrap();
        let mut cells = vec!["  **café**  ".into()];
        if source.starts_with("A | B") {
            cells.push("a\\|b".into());
        }
        let plan = doc
            .edit_table_row(&path, &TableRowEdit::Insert { index, cells }, &mut g)
            .unwrap();
        assert_eq!(applied(&plan, source), expected, "{source:?}");
    }
}

#[test]
fn removal_preserves_headers_surrounding_containers_and_other_rows() {
    for (source, path, index, expected) in [
        (
            "| A |\n| --- |\n| one |\n| two |\n",
            vec![0],
            0,
            "| A |\n| --- |\n| two |\n",
        ),
        (
            "| A |\n| --- |\n| one |\n| two |",
            vec![0],
            1,
            "| A |\n| --- |\n| one |\n",
        ),
        (
            "> | A |\r\n> | --- |\r\n> | one |\r\n",
            vec![0, 0],
            0,
            "> | A |\r\n> | --- |\r\n",
        ),
        (
            "- | A |\n  | --- |\n  | one |\n\n- sibling\n",
            vec![0, 0],
            0,
            "- | A |\n  | --- |\n\n- sibling\n",
        ),
    ] {
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(source, &mut g).unwrap();
        let plan = doc
            .edit_table_row(&path, &TableRowEdit::Remove { index }, &mut g)
            .unwrap();
        assert_eq!(applied(&plan, source), expected);
    }
}

#[test]
fn invalid_rows_and_exhaustion_never_return_partial_plans() {
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("| A |\n| --- |\n", &mut g).unwrap();
    for edit in [
        TableRowEdit::Remove { index: 0 },
        TableRowEdit::Insert {
            index: 1,
            cells: vec!["a".into()],
        },
        TableRowEdit::Insert {
            index: 0,
            cells: vec![],
        },
        TableRowEdit::Insert {
            index: 0,
            cells: vec!["a".into(), "b".into()],
        },
        TableRowEdit::Insert {
            index: 0,
            cells: vec!["a\nb".into()],
        },
        TableRowEdit::Insert {
            index: 0,
            cells: vec!["a|b".into()],
        },
    ] {
        assert!(matches!(
            doc.edit_table_row(&[0], &edit, &mut g),
            Err(StopReason::InvalidInput(_))
        ));
    }
    let edit = TableRowEdit::Insert {
        index: 0,
        cells: vec![String::new()],
    };
    assert!(doc.edit_table_row(&[], &edit, &mut g).is_err());
    let mut exhausted = Governor::new(Budget {
        scratch_bytes: 1,
        ..Budget::default()
    });
    assert_eq!(
        doc.edit_table_row(&[0], &edit, &mut exhausted),
        Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
    let mut cancelled = Governor::new(Budget::default());
    cancelled.cancel();
    assert!(doc.edit_table_row(&[0], &edit, &mut cancelled).is_err());
    assert_eq!(
        applied(
            &doc.edit_table_row(&[0], &edit, &mut g).unwrap(),
            doc.source()
        ),
        "| A |\n| --- |\n|  |\n"
    );
}
