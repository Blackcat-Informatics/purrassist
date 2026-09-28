// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Column edits preserve raw cells, ragged-row meaning, and exact conditional undo.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    edits::EditPlan,
    markdown::{Alignment, BlockKind, Document, TableColumnEdit},
};

fn apply(plan: &EditPlan, source: &str) -> String {
    let Outcome::Resolved(value) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply")
    };
    let Outcome::Resolved(undone) = value.inverse.apply(&value.text, Budget::default()).outcome
    else {
        panic!("undo")
    };
    assert_eq!(undone.text, source);
    assert_eq!(
        plan.apply(&format!("{source}changed"), Budget::default())
            .outcome,
        Outcome::Unresolved
    );
    value.text
}

#[test]
fn nested_columns_preserve_crlf_escaped_cells_padding_and_ignored_extras() {
    let source = "> | A | B |\r\n> | :--- | ---: |\r\n> | a\\|b | c |\r\n> | lone |\r\n> | x | y | extra |\r\n";
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(source, &mut g).unwrap();
    let edit = TableColumnEdit::Insert {
        index: 1,
        header: " C ".into(),
        alignment: Alignment::Center,
        cells: vec!["new".into(), "empty".into(), "keep".into()],
    };
    let plan = doc.edit_table_column(&[0, 0], &edit, &mut g).unwrap();
    assert_eq!(
        apply(&plan, source),
        "> | A | C | B |\r\n> | :--- | :---: | ---: |\r\n> | a\\|b | new | c |\r\n> | lone | empty ||\r\n> | x | keep | y | extra |\r\n"
    );
    let plan = doc
        .edit_table_column(&[0, 0], &TableColumnEdit::Remove { index: 1 }, &mut g)
        .unwrap();
    assert_eq!(
        apply(&plan, source),
        "> | A |\r\n> | :--- |\r\n> | a\\|b |\r\n> | lone |\r\n> | x | extra |\r\n"
    );
}

#[test]
fn insertion_at_each_boundary_and_alignment_changes_keep_table_structure() {
    for (source, aligned) in [
        ("A | B\n--- | ---\na | b", "A | B\n--- | ---:\na | b"),
        (
            "- | A | B |\n  | --- | --- |\n  |\n",
            "- | A | B |\n  | --- | ---: |\n  |\n",
        ),
    ] {
        let path = if source.starts_with('-') {
            vec![0, 0]
        } else {
            vec![0]
        };
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(source, &mut g).unwrap();
        for index in 0..=2 {
            let edit = TableColumnEdit::Insert {
                index,
                header: "café".into(),
                alignment: Alignment::Left,
                cells: vec!["**new**".into()],
            };
            let output = apply(
                &doc.edit_table_column(&path, &edit, &mut g).unwrap(),
                source,
            );
            let updated = Document::parse(&output, &mut g).unwrap();
            let BlockKind::Table {
                header,
                alignments,
                rows,
            } = &updated.block_at(&path).unwrap().kind
            else {
                panic!("table")
            };
            assert_eq!(header[index], "café");
            assert_eq!(alignments[index], Alignment::Left);
            assert_eq!(rows[0][index], "**new**");
        }
        let edit = TableColumnEdit::Align {
            index: 1,
            alignment: Alignment::Right,
        };
        let plan = doc.edit_table_column(&path, &edit, &mut g).unwrap();
        assert_eq!(plan.edits.len(), 1);
        assert_eq!(apply(&plan, source), aligned);
        assert_eq!(
            doc.edit_table_column(
                &path,
                &TableColumnEdit::Align {
                    index: 0,
                    alignment: Alignment::Default
                },
                &mut g
            )
            .unwrap()
            .edits,
            []
        );
    }
}

#[test]
fn missing_columns_final_removal_and_invalid_cells_fail_explicitly() {
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("| A |\n| --- |\n", &mut g).unwrap();
    for edit in [
        TableColumnEdit::Remove { index: 0 },
        TableColumnEdit::Remove { index: 9 },
        TableColumnEdit::Align {
            index: 9,
            alignment: Alignment::Right,
        },
        TableColumnEdit::Insert {
            index: 2,
            header: "B".into(),
            alignment: Alignment::Default,
            cells: vec![],
        },
        TableColumnEdit::Insert {
            index: 0,
            header: "a|b".into(),
            alignment: Alignment::Default,
            cells: vec![],
        },
        TableColumnEdit::Insert {
            index: 0,
            header: "a\nb".into(),
            alignment: Alignment::Default,
            cells: vec![],
        },
        TableColumnEdit::Insert {
            index: 0,
            header: "B".into(),
            alignment: Alignment::Default,
            cells: vec!["extra".into()],
        },
    ] {
        assert!(matches!(
            doc.edit_table_column(&[0], &edit, &mut g),
            Err(StopReason::InvalidInput(_))
        ));
    }
    let edit = TableColumnEdit::Insert {
        index: 1,
        header: "B".into(),
        alignment: Alignment::Default,
        cells: vec![],
    };
    assert_eq!(
        apply(
            &doc.edit_table_column(&[0], &edit, &mut g).unwrap(),
            doc.source()
        ),
        "| A | B |\n| --- | --- |\n"
    );
    let mut exhausted = Governor::new(Budget {
        scratch_bytes: 0,
        ..Budget::default()
    });
    assert_eq!(
        doc.edit_table_column(&[0], &edit, &mut exhausted),
        Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
    let mut cancelled = Governor::new(Budget::default());
    cancelled.cancel();
    assert!(doc.edit_table_column(&[0], &edit, &mut cancelled).is_err());
}

#[test]
fn sparse_table_padding_is_admitted_before_expansion() {
    let source = format!(
        "{}\n{}\n{}",
        "| a ".repeat(64) + "|",
        "| --- ".repeat(64) + "|",
        "|\n".repeat(64)
    );
    let doc = Document::parse(&source, &mut Governor::new(Budget::default())).unwrap();
    let mut g = Governor::new(Budget {
        scratch_bytes: 200_000,
        ..Budget::default()
    });
    assert_eq!(
        doc.edit_table_column(&[0], &TableColumnEdit::Remove { index: 0 }, &mut g),
        Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
    // Alignment does not materialize padding and must not reserve its rectangular growth.
    let mut g = Governor::new(Budget {
        scratch_bytes: 300_000,
        ..Budget::default()
    });
    assert!(
        doc.edit_table_column(
            &[0],
            &TableColumnEdit::Align {
                index: 0,
                alignment: Alignment::Right,
            },
            &mut g
        )
        .is_ok()
    );
}
