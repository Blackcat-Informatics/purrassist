// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Structural Markdown operations added for the patch algebra (Task 9):
//! `insert_section`, `remove_section`, `move_section`, `rename_heading`,
//! `ListItemEdit::Move`, `renumber_list`, `TableRowEdit::Move`,
//! `TableColumnEdit::Move`, implicit-cell materialization, and the
//! reference-definition operations `add_reference`, `relabel_reference`,
//! `remove_reference`, `unlink_reference`. Each op is checked for success,
//! byte-identical inverse round-trip, CRLF/UTF-8 preservation, stale-digest
//! refusal, and resource exhaustion.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    edits::{EditPlan, TextEdit},
    markdown::{Dialect, Document, ListItemEdit, SectionInsertion, TableColumnEdit, TableRowEdit},
};

/// Apply `plan` to `source`, assert it resolves, and assert the resulting
/// inverse restores `source` byte-for-byte. Returns the applied text.
fn round_trip(source: &str, plan: &EditPlan) -> String {
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("expected apply to resolve for {source:?} with {plan:?}")
    };
    let Outcome::Resolved(undo) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("expected inverse to resolve")
    };
    assert_eq!(undo.text, source, "inverse must restore the exact original");
    applied.text
}

fn assert_stale(source: &str, plan: &EditPlan) {
    let mutated = format!("{source}\n<!-- changed -->\n");
    assert_eq!(
        plan.apply(&mutated, Budget::default()).outcome,
        Outcome::Unresolved,
        "a plan bound to a stale source must not apply"
    );
}

fn exhausted() -> Governor {
    Governor::new(Budget {
        work: 0,
        ..Budget::default()
    })
}

#[test]
fn task_edits_preserve_original_ancestor_lazy_indent_spans() {
    use purrassist_core::markdown::BlockKind;
    for newline in ["\n", "\r\n", "\r"] {
        for indent in ["  ", "    ", "\t"] {
            let source = format!("> - [ ] tea{newline}{indent}--{newline}{indent}|\"{newline}");
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let item = document.block_at(&[0, 0]).unwrap();
            assert_eq!(item.children.len(), 1);
            let paragraph = &item.children[0];
            assert!(matches!(paragraph.kind, BlockKind::Paragraph));
            assert_eq!(paragraph.lines.len(), 3);
            let second_line = source.find(newline).unwrap() + newline.len();
            assert_eq!(paragraph.lines[1].start, second_line);
            assert!(source[paragraph.lines[1].clone()].starts_with(indent));
            let plan = document.set_task_at(&[0, 0], true, &mut governor).unwrap();
            assert_eq!(plan.edits.len(), 1);
            assert_eq!(plan.edits[0].range, 5..6);
            let expected = format!("> - [x] tea{newline}{indent}--{newline}{indent}|\"{newline}");
            assert_eq!(round_trip(&source, &plan), expected);
            assert_stale(&source, &plan);
            assert_eq!(
                document
                    .set_task_at(&[0, 0], true, &mut exhausted())
                    .unwrap_err(),
                StopReason::BudgetExceeded(Resource::Work)
            );
        }
    }
}

#[test]
fn task_edits_preserve_lazy_setext_text_and_all_later_paragraph_lines() {
    use purrassist_core::markdown::BlockKind;
    for newline in ["\n", "\r\n", "\r"] {
        for (prefix, path) in [("- ", vec![0]), ("> - ", vec![0, 0])] {
            let source = format!("{prefix}[ ] tea{newline}--{newline}|\"{newline}");
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let item = document.block_at(&path).unwrap();
            assert_eq!(item.children.len(), 1);
            assert!(matches!(item.children[0].kind, BlockKind::Paragraph));
            assert_eq!(item.children[0].lines.len(), 3);
            assert_eq!(item.children[0].span.end, source.len());
            let plan = document.set_task_at(&path, true, &mut governor).unwrap();
            assert_eq!(plan.edits.len(), 1);
            assert_eq!(plan.edits[0].range, prefix.len() + 1..prefix.len() + 2);
            let expected = format!("{prefix}[x] tea{newline}--{newline}|\"{newline}");
            assert_eq!(round_trip(&source, &plan), expected);
            assert_stale(&source, &plan);
            assert_eq!(
                document
                    .set_task_at(&path, true, &mut exhausted())
                    .unwrap_err(),
                StopReason::BudgetExceeded(Resource::Work)
            );
            let changed = Document::parse(&expected, &mut governor).unwrap();
            let html = changed.render_html(Dialect::Gfm, &mut governor).unwrap();
            assert!(
                html.contains("type=\"checkbox\"> tea\n--\n|&quot;"),
                "{html:?}"
            );
        }
    }
}

#[test]
fn insert_section_succeeds_preserves_crlf_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "# Tea\nHot.\n".replace('\n', newline);
        let addition = "# Coffee\nBlack.\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc
            .insert_section(SectionInsertion::Start, &addition, &mut g)
            .unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(applied, format!("{addition}{source}"));
        assert_stale(&source, &plan);
        assert_eq!(
            doc.insert_section(SectionInsertion::Start, &addition, &mut exhausted())
                .unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    // After an existing section, and rejecting a non-heading insertion.
    let source = "# Tea\nHot.\n# Coffee\nBlack.\n";
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(source, &mut g).unwrap();
    let plan = doc
        .insert_section(SectionInsertion::After(0), "# Iced\nCold.\n", &mut g)
        .unwrap();
    let applied = round_trip(source, &plan);
    assert_eq!(applied, "# Tea\nHot.\n# Iced\nCold.\n# Coffee\nBlack.\n");
    assert!(
        doc.insert_section(SectionInsertion::Start, "not a heading\n", &mut g)
            .is_err()
    );
    assert!(
        doc.insert_section(SectionInsertion::After(99), "# X\n", &mut g)
            .is_err()
    );
}

#[test]
fn remove_section_succeeds_preserves_crlf_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "# Tea\nHot.\n## Iced\nCold.\n# Coffee\nBlack.\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc.remove_section(0, &mut g).unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(applied, format!("# Coffee{newline}Black.{newline}"));
        assert_stale(&source, &plan);
        assert_eq!(
            doc.remove_section(0, &mut exhausted()).unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("# Tea\nHot.\n", &mut g).unwrap();
    assert!(doc.remove_section(99, &mut g).is_err());
}

#[test]
fn rename_heading_succeeds_preserves_crlf_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "# Tea\nHot.\n# Keep\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc.rename_heading(0, "Chai", &mut g).unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(
            applied,
            format!("# Chai{newline}Hot.{newline}# Keep{newline}")
        );
        assert_stale(&source, &plan);
        assert_eq!(
            doc.rename_heading(0, "Chai", &mut exhausted()).unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    // Setext headings keep their underline; identical titles are a no-op.
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("Tea\n===\nHot.\n", &mut g).unwrap();
    let plan = doc.rename_heading(0, "Chai", &mut g).unwrap();
    let applied = round_trip("Tea\n===\nHot.\n", &plan);
    assert_eq!(applied, "Chai\n===\nHot.\n");
    assert_eq!(doc.rename_heading(0, "Tea", &mut g).unwrap().edits, []);
    assert!(doc.rename_heading(0, "no\nbreaks", &mut g).is_err());
    assert!(doc.rename_heading(99, "X", &mut g).is_err());
}

#[test]
fn multiline_setext_rename_preserves_underline_containers_spans_and_inverse() {
    for newline in ["\n", "\r\n", "\r"] {
        for underline in ["   ---  ", "  ===\t"] {
            let before = "> before\n> - nested\n\n".replace('\n', newline);
            let original_title = "  Café\nβeta\nthird\n".replace('\n', newline);
            let after = "\n- after\n  > quote\n\n# Keep\n".replace('\n', newline);
            let source = format!("{before}{original_title}{underline}{newline}{after}");
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let heading = document
                .blocks()
                .iter()
                .position(|block| {
                    matches!(
                        &block.kind, purrassist_core::markdown::BlockKind::Heading { title, .. }
                        if title == "Café\nβeta\nthird"
                    )
                })
                .unwrap();
            let plan = document
                .rename_heading(heading, "Chāi", &mut governor)
                .unwrap();
            assert_eq!(plan.edits.len(), 1);
            assert_eq!(
                plan.edits[0].range,
                before.len()..before.len() + original_title.len()
            );
            assert_eq!(plan.edits[0].replacement, format!("  Chāi{newline}"));
            assert_eq!(
                round_trip(&source, &plan),
                format!("{before}  Chāi{newline}{underline}{newline}{after}")
            );
            let applied = plan.apply(&source, Budget::default());
            assert_eq!(applied.receipt.losses.len(), 1);
            assert_eq!(applied.receipt.losses[0].original, original_title);
            assert_eq!(
                applied.receipt.losses[0]
                    .evidence
                    .span
                    .as_ref()
                    .unwrap()
                    .bytes,
                plan.edits[0].range
            );
            assert_stale(&source, &plan);
            assert_eq!(
                document
                    .rename_heading(heading, "Chāi", &mut exhausted())
                    .unwrap_err(),
                StopReason::BudgetExceeded(Resource::Work)
            );
            for invalid_title in ["", "\tcode", "# Different style", "two\nlines"] {
                assert!(
                    matches!(
                        document.rename_heading(heading, invalid_title, &mut governor),
                        Err(StopReason::InvalidInput(_))
                    ),
                    "title {invalid_title:?} must not change the selected heading's structure"
                );
            }
        }
    }
    let source = "alpha\r\nbeta\n  ---\r\nbody\r\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    let plan = document
        .rename_heading(0, "renamed", &mut governor)
        .unwrap();
    assert_eq!(round_trip(source, &plan), "renamed\n  ---\r\nbody\r\n");
}

#[test]
fn move_section_reorders_relevels_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "# Tea\nHot.\n# Coffee\nBlack.\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc
            .move_section(0, SectionInsertion::After(2), None, &mut g)
            .unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(
            applied,
            format!("# Coffee{newline}Black.{newline}# Tea{newline}Hot.{newline}")
        );
        assert_stale(&source, &plan);
        assert_eq!(
            doc.move_section(0, SectionInsertion::After(2), None, &mut exhausted())
                .unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    // Re-leveling the moved section's own top heading.
    let source = "# Tea\nHot.\n# Coffee\nBlack.\n";
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(source, &mut g).unwrap();
    let plan = doc
        .move_section(0, SectionInsertion::After(2), Some(2), &mut g)
        .unwrap();
    let applied = round_trip(source, &plan);
    assert_eq!(applied, "# Coffee\nBlack.\n## Tea\nHot.\n");
    assert!(
        doc.move_section(0, SectionInsertion::After(0), None, &mut g)
            .is_err()
    );
}

#[test]
fn list_item_move_reorders_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "- a\n- b\n- c\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc
            .edit_list_item(&[0], &ListItemEdit::Move { to: 2 }, &mut g)
            .unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(applied, format!("- b{newline}- c{newline}- a{newline}"));
        assert_stale(&source, &plan);
        assert_eq!(
            doc.edit_list_item(&[0], &ListItemEdit::Move { to: 2 }, &mut exhausted())
                .unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("- a\n- b\n", &mut g).unwrap();
    assert!(
        doc.edit_list_item(&[0], &ListItemEdit::Move { to: 99 }, &mut g)
            .is_err()
    );
    assert_eq!(
        doc.edit_list_item(&[0], &ListItemEdit::Move { to: 0 }, &mut g)
            .unwrap()
            .edits,
        []
    );
}

#[test]
fn renumber_list_resequences_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "3. a\n7. b\n9. c\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc.renumber_list(&[0], 1, &mut g).unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(applied, format!("1. a{newline}2. b{newline}3. c{newline}"));
        assert_stale(&source, &plan);
        assert_eq!(
            doc.renumber_list(&[0], 1, &mut exhausted()).unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    // Digit-width growth (9 -> 10) shifts everything after correctly.
    let source = "8. a\n9. b\n1. c\n";
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(source, &mut g).unwrap();
    let plan = doc.renumber_list(&[0], 9, &mut g).unwrap();
    let applied = round_trip(source, &plan);
    assert_eq!(applied, "9. a\n10. b\n11. c\n");
    let mut g = Governor::new(Budget::default());
    let bullets = Document::parse("- a\n", &mut g).unwrap();
    assert!(bullets.renumber_list(&[0], 1, &mut g).is_err());
}

#[test]
fn table_row_move_reorders_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "| A |\n| --- |\n| 1 |\n| 2 |\n| 3 |\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let edit = TableRowEdit::Move { index: 0, to: 2 };
        let plan = doc.edit_table_row(&[0], &edit, &mut g).unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(
            applied,
            format!("| A |{newline}| --- |{newline}| 2 |{newline}| 3 |{newline}| 1 |{newline}")
        );
        assert_stale(&source, &plan);
        assert_eq!(
            doc.edit_table_row(&[0], &edit, &mut exhausted())
                .unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("| A |\n| --- |\n| 1 |\n", &mut g).unwrap();
    assert!(
        doc.edit_table_row(&[0], &TableRowEdit::Move { index: 0, to: 99 }, &mut g)
            .is_err()
    );
}

#[test]
fn table_column_move_reorders_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "| A | B | C |\n| --- | --- | --- |\n| 1 | 2 | 3 |\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let edit = TableColumnEdit::Move { index: 0, to: 2 };
        let plan = doc.edit_table_column(&[0], &edit, &mut g).unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(
            applied,
            format!("| B | C | A |{newline}| --- | --- | --- |{newline}| 2 | 3 | 1 |{newline}")
        );
        assert_stale(&source, &plan);
        assert_eq!(
            doc.edit_table_column(&[0], &edit, &mut exhausted())
                .unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("| A |\n| --- |\n| 1 |\n", &mut g).unwrap();
    assert!(
        doc.edit_table_column(&[0], &TableColumnEdit::Move { index: 0, to: 99 }, &mut g)
            .is_err()
    );
}

#[test]
fn table_cell_materialization_succeeds_preserves_crlf_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "| A | B | C |\n| --- | --- | --- |\n| tea |\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc.replace_table_cell(&[0], 1, 2, "iced", &mut g).unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(
            applied,
            format!("| A | B | C |{newline}| --- | --- | --- |{newline}| tea |  | iced |{newline}")
        );
        assert_stale(&source, &plan);
        assert_eq!(
            doc.replace_table_cell(&[0], 1, 2, "iced", &mut exhausted())
                .unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
}

#[test]
fn add_reference_succeeds_preserves_crlf_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "[tea]\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc
            .add_reference("tea", "/tea", Some("Tea"), &mut g)
            .unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(
            applied,
            format!("[tea]{newline}{newline}[tea]: <\\/tea> \"Tea\"{newline}")
        );
        assert_stale(&source, &plan);
        assert_eq!(
            doc.add_reference("tea", "/tea", Some("Tea"), &mut exhausted())
                .unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("x\n", &mut g).unwrap();
    assert!(doc.add_reference("", "/tea", None, &mut g).is_err());
    assert!(doc.add_reference("t[e]a", "/tea", None, &mut g).is_err());
}

#[test]
fn relabel_reference_updates_definition_and_uses_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "[tea] and [tea][] and [x][tea]\n\n[tea]: /tea\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc.relabel_reference(&[2], 0, "chai", &mut g).unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(
            applied,
            format!("[chai] and [chai][] and [x][chai]{newline}{newline}[chai]: /tea{newline}")
        );
        assert_stale(&source, &plan);
        assert_eq!(
            doc.relabel_reference(&[2], 0, "chai", &mut exhausted())
                .unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("[tea]\n\n[tea]: /tea\n", &mut g).unwrap();
    assert!(doc.relabel_reference(&[2], 0, "", &mut g).is_err());
    assert!(doc.relabel_reference(&[99], 0, "chai", &mut g).is_err());
}

#[test]
fn remove_reference_inlines_uses_losslessly_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "See [tea]\n\n[tea]: /tea \"Tea\"\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc.remove_reference(&[2], 0, &mut g).unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(
            applied,
            format!("See [tea](<\\/tea> \"Tea\"){newline}{newline}")
        );
        assert_stale(&source, &plan);
        assert_eq!(
            doc.remove_reference(&[2], 0, &mut exhausted()).unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("[tea]\n\n[tea]: /tea\n", &mut g).unwrap();
    assert!(doc.remove_reference(&[99], 0, &mut g).is_err());
}

#[test]
fn unlink_reference_turns_uses_into_plain_text_and_refuses_stale_or_exhausted() {
    for newline in ["\n", "\r\n"] {
        let source = "See [tea]\n\n[tea]: /tea\n".replace('\n', newline);
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc.unlink_reference(&[2], 0, &mut g).unwrap();
        let applied = round_trip(&source, &plan);
        assert_eq!(applied, format!("See tea{newline}{newline}"));
        assert_stale(&source, &plan);
        assert_eq!(
            doc.unlink_reference(&[2], 0, &mut exhausted()).unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
    }
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("[tea]\n\n[tea]: /tea\n", &mut g).unwrap();
    assert!(doc.unlink_reference(&[99], 0, &mut g).is_err());
}

#[test]
fn unlink_reference_records_a_loss_per_affected_use_and_definition() {
    let source = "See [tea] and [tea][]\n\n[tea]: /tea\n";
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(source, &mut g).unwrap();
    let plan = doc.unlink_reference(&[2], 0, &mut g).unwrap();
    let result = plan.apply(source, Budget::default());
    let Outcome::Resolved(_) = result.outcome else {
        panic!("apply")
    };
    // One Loss per rewritten byte range: the removed definition plus each
    // of the two uses.
    assert_eq!(result.receipt.losses.len(), 3);
}

/// A full reference (`[drink][tea]`) used in a top-level paragraph, inside a
/// block quote, inside a list item, and inside a GFM table cell, plus one
/// spelled identically inside an inline code span (which must stay literal
/// since `CommonMark` never resolves a reference there).
const REFERENCE_USE_CONTEXTS: &str = "See [drink][tea].\n\n> Quote [drink][tea].\n\n\
- List [drink][tea].\n\n| Drink |\n| --- |\n| [drink][tea] |\n\n\
`[drink][tea]` code.\n\n[tea]: /tea\n";

#[test]
fn relabel_reference_updates_every_resolving_context_and_preserves_rendered_html() {
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(REFERENCE_USE_CONTEXTS, &mut g).unwrap();
    let before_html = doc.render_html(Dialect::Gfm, &mut g).unwrap();
    let plan = doc.relabel_reference(&[10], 0, "chai", &mut g).unwrap();
    let applied = round_trip(REFERENCE_USE_CONTEXTS, &plan);
    assert_eq!(
        applied,
        "See [drink][chai].\n\n> Quote [drink][chai].\n\n\
- List [drink][chai].\n\n| Drink |\n| --- |\n| [drink][chai] |\n\n\
`[drink][tea]` code.\n\n[chai]: /tea\n"
    );
    // The code span's identical spelling is never a reference use: it stays
    // exactly as written.
    assert!(applied.contains("`[drink][tea]` code."));
    let updated = Document::parse(&applied, &mut g).unwrap();
    let after_html = updated.render_html(Dialect::Gfm, &mut g).unwrap();
    assert_eq!(
        before_html, after_html,
        "relabeling only the invisible label must not change any rendered link"
    );
}

#[test]
fn remove_reference_inlines_every_resolving_context_and_preserves_rendered_html() {
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(REFERENCE_USE_CONTEXTS, &mut g).unwrap();
    let before_html = doc.render_html(Dialect::Gfm, &mut g).unwrap();
    let plan = doc.remove_reference(&[10], 0, &mut g).unwrap();
    let applied = round_trip(REFERENCE_USE_CONTEXTS, &plan);
    assert_eq!(
        applied,
        "See [drink](<\\/tea>).\n\n> Quote [drink](<\\/tea>).\n\n\
- List [drink](<\\/tea>).\n\n| Drink |\n| --- |\n| [drink](<\\/tea>) |\n\n\
`[drink][tea]` code.\n\n"
    );
    assert!(applied.contains("`[drink][tea]` code."));
    let updated = Document::parse(&applied, &mut g).unwrap();
    let after_html = updated.render_html(Dialect::Gfm, &mut g).unwrap();
    assert_eq!(
        before_html, after_html,
        "inlining a definition's destination must not change any rendered link"
    );
}

#[test]
fn unlink_reference_turns_every_resolving_context_into_plain_text() {
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(REFERENCE_USE_CONTEXTS, &mut g).unwrap();
    let plan = doc.unlink_reference(&[10], 0, &mut g).unwrap();
    let applied = round_trip(REFERENCE_USE_CONTEXTS, &plan);
    assert_eq!(
        applied,
        "See drink.\n\n> Quote drink.\n\n- List drink.\n\n\
| Drink |\n| --- |\n| drink |\n\n`[drink][tea]` code.\n\n"
    );
    // The code span is not a use and is never unlinked.
    assert!(applied.contains("`[drink][tea]` code."));
}

#[test]
fn setext_section_moves_relevel_every_valid_level_and_restore_exact_source() {
    for newline in ["\n", "\r\n", "\r"] {
        for (marker, old_level) in [('=', 1), ('-', 2)] {
            for multiline in [false, true] {
                for new_level in 1..=6 {
                    let before = "# Keep\n> quote\n- list\n\nOutside\n\n".replace('\n', newline);
                    let title = if multiline {
                        "  Café\nβeta\n"
                    } else {
                        "  Café\n"
                    }
                    .replace('\n', newline);
                    let underline = format!("  {marker}{marker}{marker}{marker} \t{newline}");
                    let body = "body\n\n### Nested\nunchanged\n\n".replace('\n', newline);
                    let source = format!("{before}{title}{underline}{body}");
                    let mut g = Governor::new(Budget::default());
                    let doc = Document::parse(&source, &mut g).unwrap();
                    let heading = doc.blocks().iter().position(|block| matches!(&block.kind, purrassist_core::markdown::BlockKind::Heading { title, .. } if title.starts_with("Café"))).unwrap();
                    let plan = doc
                        .move_section(heading, SectionInsertion::Start, Some(new_level), &mut g)
                        .unwrap();
                    let moved_heading = if new_level <= 2 {
                        let new_marker = if new_level == 1 { '=' } else { '-' };
                        format!(
                            "{title}{}",
                            underline.replace(marker, &new_marker.to_string())
                        )
                    } else {
                        format!(
                            "  {} Café{}{newline}",
                            "#".repeat(new_level as usize),
                            if multiline { " βeta" } else { "" }
                        )
                    };
                    let applied = round_trip(&source, &plan);
                    assert_eq!(
                        applied,
                        format!("{moved_heading}{body}{before}"),
                        "{old_level} -> {new_level}"
                    );
                    let updated = Document::parse(&applied, &mut g).unwrap();
                    assert!(
                        matches!(&updated.blocks()[0].kind, purrassist_core::markdown::BlockKind::Heading { level, .. } if *level == new_level)
                    );
                    assert_stale(&source, &plan);
                    assert_eq!(
                        doc.move_section(
                            heading,
                            SectionInsertion::Start,
                            Some(new_level),
                            &mut exhausted()
                        )
                        .unwrap_err(),
                        StopReason::BudgetExceeded(Resource::Work)
                    );
                    for invalid in [0, 7] {
                        assert!(matches!(
                            doc.move_section(
                                heading,
                                SectionInsertion::Start,
                                Some(invalid),
                                &mut g
                            ),
                            Err(StopReason::InvalidInput(_))
                        ));
                    }
                }
            }
        }
    }
    for title in ["literal ###", "first\nliteral ###"] {
        let source = format!("{title}\n---\n\nbody\n");
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).unwrap();
        let plan = doc
            .move_section(0, SectionInsertion::Start, Some(3), &mut g)
            .unwrap();
        let applied = round_trip(&source, &plan);
        let old_html = doc.render_html(Dialect::CommonMark, &mut g).unwrap();
        let updated = Document::parse(&applied, &mut g).unwrap();
        let new_html = updated.render_html(Dialect::CommonMark, &mut g).unwrap();
        assert_eq!(
            old_html
                .replace("h2>", "h3>")
                .replace("first\nliteral", "first literal"),
            new_html
        );
    }
}

#[test]
fn tasks_use_the_first_paragraph_and_toggle_split_markers_with_exact_undo() {
    use purrassist_core::markdown::BlockKind;
    for newline in ["\n", "\r\n", "\r"] {
        for (prefix, continuation, path) in [("- ", "  ", vec![0]), ("> - ", ">   ", vec![0, 0])] {
            for middle in [" ", "\t", "\u{b}", "\u{c}", newline] {
                let marker = if middle == newline {
                    format!("[{newline}{continuation}] ")
                } else {
                    format!("[{middle}] ")
                };
                let source = format!("{prefix}{marker}todo{newline}");
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert!(
                    matches!(
                        document.block_at(&path).unwrap().kind,
                        BlockKind::ListItem {
                            checked: Some(false),
                            ..
                        }
                    ),
                    "{source:?}"
                );
                let plan = document.set_task_at(&path, true, &mut governor).unwrap();
                let expected = format!("{prefix}[x] todo{newline}");
                let changed = round_trip(&source, &plan);
                assert_eq!(changed, expected, "{source:?}");
                assert_stale(&source, &plan);
                let updated = Document::parse(&changed, &mut governor).unwrap();
                assert!(matches!(
                    updated.block_at(&path).unwrap().kind,
                    BlockKind::ListItem {
                        checked: Some(true),
                        ..
                    }
                ));
                assert_eq!(
                    updated
                        .set_task_at(&path, true, &mut governor)
                        .unwrap()
                        .edits,
                    Vec::<purrassist_core::edits::TextEdit>::new()
                );
                assert!(updated.set_task_at(&path, false, &mut exhausted()).is_err());
            }
        }
        for marker in ["[x]", "[X]", "[ ]"] {
            let source = format!("- {marker}{newline}  todo{newline}");
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let checked = marker != "[ ]";
            assert!(
                matches!(document.blocks()[0].kind, BlockKind::ListItem { checked: Some(state), .. } if state == checked)
            );
            let expected = format!(
                "<ul>\n<li><input{} disabled=\"\" type=\"checkbox\"> todo</li>\n</ul>\n",
                if checked { " checked=\"\"" } else { "" }
            );
            assert_eq!(
                document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                expected
            );
            round_trip(
                &source,
                &document.set_task_at(&[0], !checked, &mut governor).unwrap(),
            );
        }
    }
    let source = "- [def]: /target\n  [ ] todo\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    let plan = document.set_task_at(&[0], true, &mut governor).unwrap();
    assert_eq!(round_trip(source, &plan), "- [def]: /target\n  [x] todo\n");
    assert_eq!(
        document.render_html(Dialect::Gfm, &mut governor).unwrap(),
        "<ul>\n<li><input disabled=\"\" type=\"checkbox\"> todo</li>\n</ul>\n"
    );
}

#[test]
fn tasks_after_definitions_allow_leading_spaces_and_preserve_marker_edit_spans() {
    use purrassist_core::markdown::BlockKind;
    for newline in ["\n", "\r\n", "\r"] {
        for (prefix, continuation, path) in [
            ("- ", " ", vec![0]),
            ("- ", "", vec![0]),
            ("- ", "  ", vec![0]),
            ("> - ", ">  ", vec![0, 0]),
            ("- - ", "   ", vec![0, 0]),
        ] {
            for marker in ["[ ]", "[x]", "[X]"] {
                let source =
                    format!("{prefix}[x]: /target{newline}{continuation}{marker} todo{newline}");
                let current = marker != "[ ]";
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert!(
                    matches!(document.block_at(&path).unwrap().kind,
                    BlockKind::ListItem { checked: Some(state), .. } if state == current),
                    "{source:?}"
                );
                let plan = document
                    .set_task_at(&path, !current, &mut governor)
                    .unwrap();
                assert_eq!(plan.edits.len(), 1);
                let bracket = source.rfind(marker).unwrap();
                assert_eq!(plan.edits[0].range, bracket + 1..bracket + 2);
                let replacement = if current { " " } else { "x" };
                let expected = format!(
                    "{prefix}[x]: /target{newline}{continuation}[{replacement}] todo{newline}"
                );
                assert_eq!(round_trip(&source, &plan), expected);
                assert_stale(&source, &plan);
                let noop = document.set_task_at(&path, current, &mut governor).unwrap();
                assert_eq!(noop.edits, Vec::<TextEdit>::new());
                assert!(
                    document
                        .set_task_at(&path, !current, &mut exhausted())
                        .is_err()
                );
                let html = document.render_html(Dialect::Gfm, &mut governor).unwrap();
                assert!(
                    html.contains(if current {
                        "<input checked=\"\" disabled=\"\" type=\"checkbox\"> todo"
                    } else {
                        "<input disabled=\"\" type=\"checkbox\"> todo"
                    }),
                    "{source:?}: {html:?}"
                );
            }
        }
        let source = format!("-    [def]: /target{newline}\t[x] todo{newline}");
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(&source, &mut governor).unwrap();
        assert!(matches!(
            document.blocks()[0].kind,
            BlockKind::ListItem { checked: None, .. }
        ));
        assert!(document.set_task_at(&[0], true, &mut governor).is_err());
        assert!(
            !document
                .render_html(Dialect::Gfm, &mut governor)
                .unwrap()
                .contains("<input")
        );
    }
}
