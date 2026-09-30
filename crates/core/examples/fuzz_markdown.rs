// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-isolated Markdown structural-edit and rendering probe for the
//! source-region fuzz runner (`scripts/fuzz.py --target markdown`).
//!
//! Bounded stdin is the exact Markdown source. Checks: parsing never
//! panics; every structural operation this crate exposes (section
//! insert/remove/rename/move, list-item edit/move/renumber, table-row/
//! column edit, and every reference operation) either refuses cleanly or
//! produces a plan whose apply-then-invert restores the exact original
//! source (apply-then-invert identity). Emits one differential record per
//! admitted execution for `scripts/fuzz.py` to check the GFM rendering
//! against `cmark-gfm`.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    edits::EditPlan,
    json::quote,
    markdown::{
        Alignment, BlockKind, Dialect, Document, Extension, Extensions, ListItemEdit,
        SectionInsertion, TableColumnEdit, TableRowEdit,
    },
};
use std::io::{self, Read};

fn verify_roundtrip(plan: &EditPlan, source: &str) {
    let applied = plan.apply(source, Budget::default());
    let Outcome::Resolved(applied) = applied.outcome else {
        assert!(
            matches!(
                applied.receipt.stopped,
                StopReason::BudgetExceeded(_) | StopReason::Cancelled
            ),
            "a structural plan failed to apply: {:?}",
            applied.receipt.stopped
        );
        return;
    };
    let restored = applied.inverse.apply(&applied.text, Budget::default());
    let Outcome::Resolved(restored) = restored.outcome else {
        panic!(
            "invert-apply failed for a structural plan: {:?}",
            restored.receipt.stopped
        );
    };
    assert_eq!(
        restored.text, source,
        "apply-then-invert did not restore the original document"
    );
}

/// Run one structural-edit attempt against a fresh, generously budgeted
/// governor; a refusal (`Err`) is an expected outcome for most attempts
/// (e.g. no such heading) and is silently ignored, but any plan actually
/// produced must satisfy [`verify_roundtrip`].
fn try_op(source: &str, op: impl FnOnce(&mut Governor) -> Result<EditPlan, StopReason>) {
    let mut governor = Governor::new(Budget::default());
    if let Ok(plan) = op(&mut governor) {
        verify_roundtrip(&plan, source);
    }
}

/// Depth-first search for the first block (root or nested) satisfying
/// `predicate`, returning its root/child index path.
fn find_path(
    blocks: &[purrassist_core::markdown::Block],
    predicate: &impl Fn(&purrassist_core::markdown::Block) -> bool,
) -> Option<Vec<usize>> {
    for (index, block) in blocks.iter().enumerate() {
        if predicate(block) {
            return Some(vec![index]);
        }
        if let Some(mut rest) = find_path(&block.children, predicate) {
            rest.insert(0, index);
            return Some(rest);
        }
    }
    None
}

fn check_lists(document: &Document) {
    let source = document.source();
    if let Some(path) = find_path(document.blocks(), &|block| {
        matches!(block.kind, BlockKind::ListItem { .. })
    }) {
        try_op(source, |g| {
            document.edit_list_item(
                &path,
                &ListItemEdit::Replace {
                    body: "replaced\n".into(),
                },
                g,
            )
        });
        try_op(source, |g| {
            document.edit_list_item(&path, &ListItemEdit::Move { to: 0 }, g)
        });
        for edit in [
            ListItemEdit::Before {
                marker: "-".into(),
                body: "[ ] before".into(),
            },
            ListItemEdit::After {
                marker: "-".into(),
                body: "[x] after".into(),
            },
            ListItemEdit::Remove,
        ] {
            try_op(source, |g| document.edit_list_item(&path, &edit, g));
        }
        try_op(source, |g| document.renumber_list(&path, 1, g));
    }
}

fn check_tables(document: &Document) {
    let source = document.source();
    if let Some(path) = find_path(document.blocks(), &|block| {
        matches!(block.kind, BlockKind::Table { .. })
    }) && let Ok(block) = document.block_at(&path)
        && let BlockKind::Table { header, rows, .. } = &block.kind
    {
        let header_len = header.len();
        if !rows.is_empty() {
            try_op(source, |g| {
                document.edit_table_row(&path, &TableRowEdit::Remove { index: 0 }, g)
            });
            try_op(source, |g| {
                document.edit_table_row(
                    &path,
                    &TableRowEdit::Move {
                        index: 0,
                        to: rows.len() - 1,
                    },
                    g,
                )
            });
        }
        try_op(source, |g| {
            document.edit_table_row(
                &path,
                &TableRowEdit::Insert {
                    index: 0,
                    cells: vec!["x".into(); header_len],
                },
                g,
            )
        });
        for edit in [
            TableColumnEdit::Insert {
                index: 0,
                header: "added".into(),
                alignment: Alignment::Right,
                cells: vec!["x".into(); rows.len()],
            },
            TableColumnEdit::Remove { index: 0 },
            TableColumnEdit::Move {
                index: 0,
                to: header_len.saturating_sub(1),
            },
        ] {
            try_op(source, |g| document.edit_table_column(&path, &edit, g));
        }
        try_op(source, |g| {
            document.edit_table_column(
                &path,
                &TableColumnEdit::Align {
                    index: 0,
                    alignment: Alignment::Left,
                },
                g,
            )
        });
    }
}

fn check_references(document: &Document) {
    let source = document.source();
    let mut reference_governor = Governor::new(Budget::default());
    if let Ok(sites) = document.reference_sites(&mut reference_governor)
        && let Some(site) = sites.first()
    {
        let path = site.path.clone();
        let occurrence = site.occurrence;
        try_op(source, |g| {
            document.relabel_reference(&path, occurrence, "newlabel", g)
        });
        try_op(source, |g| document.remove_reference(&path, occurrence, g));
        try_op(source, |g| document.unlink_reference(&path, occurrence, g));
        try_op(source, |g| {
            document.replace_reference(&path, occurrence, "/replacement", Some("updated"), g)
        });
    }
    try_op(source, |g| {
        document.add_reference("newref", "/dest", None, g)
    });
}

/// Publish only declared resource stops; unexpected parser/render errors fail
/// the process so the runner cannot count them as successful executions.
fn completed<T>(result: Result<T, StopReason>, source: &str, stage: &str) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(reason @ (StopReason::BudgetExceeded(_) | StopReason::Cancelled)) => {
            println!("{}", stop_record(source, stage, &reason));
            None
        }
        Err(reason) => panic!("unexpected Markdown {stage} error: {reason:?}"),
    }
}
fn stop_record(source: &str, stage: &str, reason: &StopReason) -> String {
    let (name, resource) = match reason {
        StopReason::BudgetExceeded(resource) => ("BudgetExceeded", quote(&format!("{resource:?}"))),
        StopReason::Cancelled => ("Cancelled", "null".into()),
        other => panic!("unexpected stop reason: {other:?}"),
    };
    format!(
        "{{\"kind\":\"resource-stop\",\"source\":{},\"stage\":{},\"reason\":{},\"resource\":{resource}}}",
        quote(source),
        quote(stage),
        quote(name)
    )
}

fn main() {
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "probe input exceeds 4096 bytes");
    let Ok(source) = std::str::from_utf8(&bytes) else {
        println!("invalid-utf8");
        return;
    };
    println!("ok");
    let mut parse_governor = Governor::new(Budget::default());
    let Some(document) = completed(
        Document::parse(source, &mut parse_governor),
        source,
        "parse",
    ) else {
        return;
    };

    if let Some(section) = document.sections().first() {
        let heading = section.heading;
        try_op(source, |g| document.remove_section(heading, g));
        try_op(source, |g| document.rename_heading(heading, "Renamed", g));
        try_op(source, |g| {
            document.move_section(heading, SectionInsertion::Start, None, g)
        });
    }
    try_op(source, |g| {
        document.insert_section(SectionInsertion::Start, "# Inserted\n\nBody.\n", g)
    });

    check_lists(&document);
    check_tables(&document);
    check_references(&document);

    let mut render_governor = Governor::new(Budget::default());
    let Some(full_html) = completed(
        document.render_html(Dialect::Gfm, &mut render_governor),
        source,
        "render",
    ) else {
        return;
    };
    {
        let limited = std::env::var("PURRASSIST_FUZZ_TASKLIST_REFERENCE_PROFILE")
            .is_ok_and(|profile| profile == "tasklist-neutral");
        let profile = if limited {
            "tasklist-reference-limited"
        } else {
            "gfm"
        };
        let html = if limited {
            let extensions = Extensions::default()
                .with(Extension::Tables)
                .with(Extension::Strikethrough)
                .with(Extension::Autolinks)
                .with(Extension::TagFilter);
            let mut governor = Governor::new(Budget::default());
            let Some(html) = completed(
                document.render_with_extensions(Dialect::Gfm, extensions, &mut governor),
                source,
                "neutral-render",
            ) else {
                return;
            };
            html
        } else {
            full_html.clone()
        };
        println!(
            "{{\"kind\":\"markdown\",\"profile\":{},\"source\":{},\"html\":{},\"full_html\":{}}}",
            quote(profile),
            quote(source),
            quote(&html),
            quote(&full_html)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{completed, stop_record};
    use purrassist_core::{StopReason, contract::Resource};

    #[test]
    fn declared_stops_have_closed_records_and_completed_values_remain_values() {
        assert_eq!(completed(Ok(7), "source", "parse"), Some(7));
        assert_eq!(
            completed::<u8>(Err(StopReason::Cancelled), "source", "render"),
            None
        );
        assert_eq!(
            stop_record(
                "x\n",
                "neutral-render",
                &StopReason::BudgetExceeded(Resource::ScratchBytes)
            ),
            "{\"kind\":\"resource-stop\",\"source\":\"x\\n\",\"stage\":\"neutral-render\",\"reason\":\"BudgetExceeded\",\"resource\":\"ScratchBytes\"}"
        );
        assert_eq!(
            stop_record("source", "parse", &StopReason::Cancelled),
            "{\"kind\":\"resource-stop\",\"source\":\"source\",\"stage\":\"parse\",\"reason\":\"Cancelled\",\"resource\":null}"
        );
    }

    #[test]
    fn unexpected_errors_cannot_be_resource_stops() {
        for reason in [
            StopReason::InvalidInput("mapping inconsistency".into()),
            StopReason::NoCandidate,
            StopReason::Completed,
        ] {
            assert!(
                std::panic::catch_unwind(|| completed::<u8>(
                    Err(reason.clone()),
                    "source",
                    "render"
                ))
                .is_err()
            );
            assert!(std::panic::catch_unwind(|| stop_record("source", "render", &reason)).is_err());
        }
    }
}
