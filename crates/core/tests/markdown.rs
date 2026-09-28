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

#[test]
fn html_block_classes_protect_embedded_heading_text() {
    for (open, close) in [
        ("<SCRIPT type=module>", "</sCrIpT>"),
        ("<!--", "-->"),
        ("<?processing", "?>"),
        ("<!DOCTYPE", ">"),
        ("<![CDATA[", "]]>"),
        ("<div class='card'>", ""),
        ("<widget data-value='a > b'>", ""),
    ] {
        let source = format!("{open}\n# protected\n{close}\n\n# visible\n");
        let document = Document::parse(&source, &mut Governor::new(Budget::default())).unwrap();
        assert!(
            matches!(document.blocks()[0].kind, BlockKind::Html),
            "{source:?}"
        );
        assert_eq!(document.sections().len(), 1, "{source:?}");
    }
    for source in [
        "paragraph\n<widget>\ntext\n",
        "<scripture> word\ntext\n",
        "<widget x='unterminated>\ntext\n",
        "éclair\ntext\n",
    ] {
        let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
        assert_eq!(document.blocks().len(), 1, "{source:?}");
        assert_eq!(document.blocks()[0].kind, BlockKind::Paragraph);
    }
}

#[test]
fn paragraph_continuations_and_setext_precedence_are_structural() {
    for (source, title) in [
        ("first\nsecond\n---\n", "first\nsecond"),
        ("first\r\n    continued\r\n===\r\n", "first\ncontinued"),
    ] {
        let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
        assert_eq!(document.blocks().len(), 1);
        assert!(
            matches!(&document.blocks()[0].kind, BlockKind::Heading {title: actual, ..} if actual == title)
        );
    }
    for source in [
        "first\n    continued\n",
        "first\n2. continued\n",
        "first\n-\n",
    ] {
        let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
        assert_eq!(document.blocks().len(), 1);
    }
}

#[test]
fn context_preserves_blocks_section_ancestry_and_utf8_boundaries() {
    let source = "# Parent\r\nintro\r\n## Child\r\ncafé\r\n# Next\r\nend\r\n";
    let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
    let start = source.find("café").unwrap();
    let context = document
        .context(start..start + 5, 0, &mut Governor::new(Budget::default()))
        .unwrap();
    assert_eq!(context.text, "café\r\n");
    assert_eq!(context.sections.len(), 2);
    assert_eq!(context.source, document.digest());
    assert_eq!(&source[context.span], context.text);
    assert!(
        document
            .context(
                start + 4..start + 5,
                0,
                &mut Governor::new(Budget::default())
            )
            .is_err()
    );
    let exhausted = document.context(
        start..start + 5,
        0,
        &mut Governor::new(Budget {
            scratch_bytes: 1,
            ..Budget::default()
        }),
    );
    assert!(matches!(
        exhausted,
        Err(purrassist_core::StopReason::BudgetExceeded(_))
    ));
    let all = document
        .context(0..0, usize::MAX, &mut Governor::new(Budget::default()))
        .unwrap();
    assert_eq!(all.text, source);
    let empty = Document::parse("", &mut Governor::new(Budget::default())).unwrap();
    assert_eq!(
        empty
            .context(0..0, 0, &mut Governor::new(Budget::default()))
            .unwrap()
            .text,
        ""
    );
}

#[test]
fn task_edits_change_only_state_and_support_conditional_undo() {
    let source = "1. [X] café\r\n2. [ ] later\r\n";
    let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
    let plan = document.set_task(0, false).unwrap();
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("task edit failed")
    };
    assert_eq!(applied.text, "1. [ ] café\r\n2. [ ] later\r\n");
    let Outcome::Resolved(undone) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo failed")
    };
    assert_eq!(undone.text, source);
    assert!(matches!(
        plan.apply("different", Budget::default()).outcome,
        Outcome::Unresolved
    ));
    assert_eq!(document.set_task(0, true).unwrap().edits, Vec::new());
}

#[test]
fn nested_containers_isolate_sections_and_keep_original_ranges() {
    let source = "# Document\n- first\n  ## Item heading\n  > quote\n  > ```md\n  > # code\n  > ```\n- second\n# Next\n";
    let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
    assert_eq!(document.sections().len(), 2);
    let item = &document.blocks()[1];
    assert!(matches!(item.kind, BlockKind::ListItem { .. }));
    assert!(matches!(
        item.children[1].kind,
        BlockKind::Heading { level: 2, .. }
    ));
    let quote = &item.children[2];
    assert_eq!(quote.kind, BlockKind::Quote);
    assert!(matches!(
        quote.children[1].kind,
        BlockKind::CodeFence { closed: true, .. }
    ));
    assert_eq!(
        &source[quote.children[1].span.clone()],
        "```md\n  > # code\n  > ```\n"
    );
    assert!(matches!(
        document.blocks()[2].kind,
        BlockKind::ListItem { .. }
    ));
}

#[test]
fn container_depth_is_explicitly_bounded() {
    let result = Document::parse(
        "> > > > text\n",
        &mut Governor::new(Budget {
            depth: 2,
            ..Budget::default()
        }),
    );
    assert!(matches!(
        result,
        Err(purrassist_core::StopReason::BudgetExceeded(
            purrassist_core::contract::Resource::Depth
        ))
    ));
}

#[test]
fn nested_fence_closure_restores_lazy_paragraph_continuation() {
    for source in [
        "> > ```\n> > code\n> > ```\n> > paragraph\nlazy\n",
        "> 1. ```\n>    code\n>    ```\n>    paragraph\nlazy\n",
    ] {
        let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
        assert_eq!(document.blocks().len(), 1, "{source:?}");
        assert_eq!(document.blocks()[0].kind, BlockKind::Quote);
        assert_eq!(document.blocks()[0].span.end, source.len());
    }
    let source = "> ```\n> > ```\n> # still code\n> ```\nafter\n";
    let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
    assert_eq!(document.blocks().len(), 2);
    assert_eq!(document.blocks()[0].children.len(), 1);
    assert!(matches!(
        document.blocks()[0].children[0].kind,
        BlockKind::CodeFence { closed: true, .. }
    ));
}

#[test]
fn checkbox_spelling_inside_list_code_is_not_a_task() {
    let source = "-     [x] code\n";
    let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
    assert!(matches!(
        document.blocks()[0].kind,
        BlockKind::ListItem { checked: None, .. }
    ));
    assert_eq!(
        document.blocks()[0].children[0].kind,
        BlockKind::IndentedCode
    );
    assert!(document.set_task(0, false).is_err());
}

#[test]
fn nested_task_paths_preserve_prefixes_siblings_and_exact_undo() {
    use purrassist_core::{Budget, Governor, Outcome, markdown::Document};
    let source = "> - [ ] café\r\n>   - [X] inner\r\n> - [ ] sibling\r\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    let path = [0, 0, 1];
    let plan = document.set_task_at(&path, false, &mut governor).unwrap();
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(&source[plan.edits[0].range.clone()], "X");
    assert_eq!(plan.edits[0].replacement, " ");
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply")
    };
    assert_eq!(applied.text, source.replace("[X]", "[ ]"));
    let Outcome::Resolved(undone) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo")
    };
    assert_eq!(undone.text, source);
    assert_eq!(
        plan.apply(&format!("{source}stale"), Budget::default())
            .outcome,
        Outcome::Unresolved
    );
    assert_eq!(
        document
            .set_task_at(&path, true, &mut governor)
            .unwrap()
            .edits,
        Vec::new()
    );
    for path in [&[][..], &[9][..], &[0, 0, 0][..]] {
        assert!(document.set_task_at(path, true, &mut governor).is_err());
    }
    let mut cancelled = Governor::new(Budget::default());
    cancelled.cancel();
    assert_eq!(
        document
            .set_task_at(&path, false, &mut cancelled)
            .unwrap_err(),
        purrassist_core::StopReason::Cancelled
    );
}
