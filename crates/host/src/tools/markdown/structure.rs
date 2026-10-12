// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `plan_markdown_structure`: every structural Markdown operation
//! (section insert/remove/move/rename, list move/renumber, table row/column
//! move, reference add/relabel/remove/unlink) behind one typed `op` selector,
//! sharing `plan_markdown_section`'s source-preserving, digest-bound plan
//! shape. Each `Document` method this dispatches to already validates its own
//! proposal by reparsing; this module only decodes arguments and reports
//! evidence of which structural operation ran.
use super::index;
use purrassist::{
    Governor, Receipt, StopReason,
    contract::{Evidence, Resource, SourceSpan},
    json::{Node, Value},
    markdown::{
        BlockKind, Document, ListItemEdit, SectionInsertion, TableColumnEdit, TableRowEdit,
    },
};

pub(super) fn call(
    args: &Node,
    document: &Document,
    governor: &mut Governor,
) -> Result<Node, StopReason> {
    let field = |name| super::super::field(args, name).map_err(StopReason::InvalidInput);
    let plan = match field("op")? {
        "section_insert" => {
            let at = insertion(args)?;
            document.insert_section(at, field("text")?, governor)?
        }
        "section_remove" => document.remove_section(index(args, "heading")?, governor)?,
        "section_move" => {
            let after = insertion(args)?;
            let new_level = optional_level(args)?;
            document.move_section(index(args, "heading")?, after, new_level, governor)?
        }
        "section_rename" => {
            document.rename_heading(index(args, "heading")?, field("title")?, governor)?
        }
        "list_move" => document.edit_list_item(
            &super::block_path(args)?,
            &ListItemEdit::Move {
                to: index(args, "to")?,
            },
            governor,
        )?,
        "list_renumber" => document.renumber_list(
            &super::block_path(args)?,
            unsigned(args, "start")?,
            governor,
        )?,
        "table_row_move" => document.edit_table_row(
            &super::block_path(args)?,
            &TableRowEdit::Move {
                index: index(args, "index")?,
                to: index(args, "to")?,
            },
            governor,
        )?,
        "table_column_move" => document.edit_table_column(
            &super::block_path(args)?,
            &TableColumnEdit::Move {
                index: index(args, "index")?,
                to: index(args, "to")?,
            },
            governor,
        )?,
        "reference_add" => {
            let title = optional_title(args)?;
            document.add_reference(field("label")?, field("destination")?, title, governor)?
        }
        "reference_relabel" => document.relabel_reference(
            &super::block_path(args)?,
            index(args, "occurrence")?,
            field("new_label")?,
            governor,
        )?,
        "reference_remove" => document.remove_reference(
            &super::block_path(args)?,
            index(args, "occurrence")?,
            governor,
        )?,
        "reference_unlink" => document.unlink_reference(
            &super::block_path(args)?,
            index(args, "occurrence")?,
            governor,
        )?,
        _ => {
            return Err(StopReason::InvalidInput(
                "unknown structural operation".into(),
            ));
        }
    };
    Ok(super::plan(&plan))
}

pub(super) fn record(
    args: &Node,
    document: &Document,
    receipt: &mut Receipt,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let op = super::super::field(args, "op").map_err(StopReason::InvalidInput)?;
    // Reserve every ledger entry and explanation before constructing or publishing any.
    governor.charge(Resource::Work, 1024)?;
    governor.charge(Resource::ScratchBytes, 4096)?;
    let mut transformation = None;
    if matches!(op, "section_move" | "section_rename") {
        let block = &document.blocks()[index(args, "heading")?];
        let BlockKind::Heading { level, title } = &block.kind else {
            unreachable!("the successful structural operation selected a heading")
        };
        let setext = block.lines.len() >= 2;
        let title_lines = if setext { block.lines.len() - 1 } else { 1 };
        if op == "section_move" {
            if let Some(new_level) = optional_level(args)?.filter(|new| new != level) {
                let old_style = if setext { "setext" } else { "ATX" };
                let new_style = if setext && new_level <= 2 {
                    "setext"
                } else {
                    "ATX"
                };
                let detail = if setext && new_level <= 2 {
                    "Only underline marker characters change; title lines, underline width/spacing, line endings, body bytes, and nested heading levels remain exact."
                } else if setext {
                    "Title lines are joined with spaces; the first title indentation and underline line ending become the ATX heading indentation and ending. The underline is removed; body bytes and nested heading levels remain exact."
                } else {
                    "The top heading receives the requested ATX prefix and canonical title spacing; body bytes and nested heading levels remain exact."
                };
                let joined_prefix = title.trim_end_matches('#');
                let escaped = setext
                    && new_level > 2
                    && joined_prefix.len() != title.len()
                    && (joined_prefix.is_empty() || joined_prefix.ends_with([' ', '\t', '\n']));
                transformation = Some((
                    "markdown.section-relevel.v1",
                    format!(
                        "section_move: heading level {level} -> {new_level}; style {old_style} -> {new_style}; original title lines: {title_lines}. {detail} Literal trailing hash sequence escaped: {escaped}. Original spelling is retained for exact inverse restoration."
                    ),
                    block.span.clone(),
                ));
            }
        } else if title_lines > 1 {
            transformation = Some((
                "markdown.heading-title-collapse.v1",
                format!(
                    "section_rename: replaced {title_lines} setext title lines with the supplied single-line title; retained first-line indentation, final title-line ending, exact underline, and all outside bytes. Original title lines are retained for exact inverse restoration."
                ),
                block.lines[0].start..block.lines[title_lines - 1].end,
            ));
        }
    }
    governor.charge(
        Resource::EvidenceEntries,
        if transformation.is_some() { 3 } else { 2 },
    )?;
    let explanation = format!(
        "Structural operation {op} was reparsed and checked against its documented shape; outside bytes remain exact and the original source is retained."
    );
    super::publish_record(
        document,
        receipt,
        super::Record {
            validator: "markdown.structure.v1",
            constraint: "the selected section, list, table, or reference structure and every unselected block reparse to the intended shape",
            rule: "markdown.structure-edit.v1",
            explanation,
        },
    );
    if let Some((rule, explanation, bytes)) = transformation {
        receipt.evidence.push(Evidence {
            rule: rule.into(),
            explanation,
            span: Some(SourceSpan {
                source: document.digest(),
                bytes,
            }),
            destination: "/edits".into(),
        });
    }
    Ok(())
}

fn insertion(args: &Node) -> Result<SectionInsertion, StopReason> {
    match super::super::field(args, "position").map_err(StopReason::InvalidInput)? {
        "start" => Ok(SectionInsertion::Start),
        "after" => Ok(SectionInsertion::After(index(args, "after_heading")?)),
        _ => Err(StopReason::InvalidInput("unknown section position".into())),
    }
}
fn optional_level(args: &Node) -> Result<Option<u8>, StopReason> {
    match args.get("new_level").map(|node| &node.value) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => {
            let level = value
                .to_usize()
                .and_then(|value| u8::try_from(value).ok())
                .ok_or_else(|| {
                    StopReason::InvalidInput("new_level exceeds a heading level".into())
                })?;
            Ok(Some(level))
        }
        _ => Err(StopReason::InvalidInput(
            "new_level must be an integer".into(),
        )),
    }
}
fn optional_title(args: &Node) -> Result<Option<&str>, StopReason> {
    match args.get("title").map(|node| &node.value) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.as_str())),
        _ => Err(StopReason::InvalidInput(
            "title must be a string or null".into(),
        )),
    }
}
fn unsigned(args: &Node, name: &str) -> Result<u64, StopReason> {
    let Some(Value::Number(value)) = args.get(name).map(|node| &node.value) else {
        return Err(StopReason::InvalidInput(format!("missing integer {name}")));
    };
    value
        .to_usize()
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| StopReason::InvalidInput(format!("{name} exceeds address space")))
}
