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
    edits::EditPlan,
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
