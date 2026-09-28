// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Editing-index preservation, fenced-code isolation, task/table recognition.
use purrassist_core::{
    Budget, Governor, Outcome,
    markdown::{Alignment, BlockKind, Document},
};

#[test]
fn section_edits_preserve_headings_fences_and_line_endings() {
    let source = "# One\r\nold\r\n```md\r\n# Not a heading\r\n```\r\n# Two\r\nkeep\r\n";
    let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
    let sections = document.sections();
    assert_eq!(sections.len(), 2);
    let plan = document
        .replace_section(sections[0].heading, "new\r\n")
        .unwrap();
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("edit failed");
    };
    assert_eq!(applied.text, "# One\r\nnew\r\n# Two\r\nkeep\r\n");
    let joined: String = document
        .blocks()
        .iter()
        .map(|block| &source[block.span.clone()])
        .collect();
    assert_eq!(joined, source);
}
#[test]
fn tables_tasks_and_unclosed_code_keep_exact_source_ranges() {
    let source = "Title\n===\n\n| A | B |\n| :--- | ---: |\n| a\\|b | c |\n\n- [x] done\n~~~rust\n# ignored\n";
    let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
    assert_eq!(document.sections().len(), 1);
    assert!(document.blocks().iter().any(|block| matches!(&block.kind, BlockKind::Table { alignments, rows, .. } if *alignments == [Alignment::Left, Alignment::Right] && rows[0][0] == "a\\|b")));
    assert!(document.blocks().iter().any(|block| matches!(
        block.kind,
        BlockKind::ListItem {
            checked: Some(true),
            ..
        }
    )));
    assert!(
        document
            .blocks()
            .iter()
            .any(|block| matches!(block.kind, BlockKind::CodeFence { closed: false, .. }))
    );
}
#[test]
fn empty_table_edge_cells_and_checkbox_word_boundaries_survive() {
    let source = "||title||\n|---|---|---|\n||value||\n\n- [x]word\n- [x] word\n";
    let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
    let BlockKind::Table { header, rows, .. } = &document.blocks()[0].kind else {
        panic!("expected table")
    };
    assert_eq!(header, &["", "title", ""]);
    assert_eq!(rows, &[vec!["", "value", ""]]);
    let states: Vec<_> = document
        .blocks()
        .iter()
        .filter_map(|block| match block.kind {
            BlockKind::ListItem { checked, .. } => Some(checked),
            _ => None,
        })
        .collect();
    assert_eq!(states, vec![None, Some(true)]);
}
