// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `plan_markdown_structure`: every Task 9 structural Markdown operation
//! (section insert/remove/move/rename, list move/renumber, table row/column
//! move, reference add/relabel/remove/unlink) behind one typed `op` selector,
//! sharing `plan_markdown_section`'s source-preserving, digest-bound plan
//! shape. Each `Document` method this dispatches to already validates its own
//! proposal by reparsing; this module only decodes arguments and reports
//! evidence of which structural operation ran.
use purrassist::{
    Governor, Receipt, StopReason,
    contract::{Evidence, Resource, SourceSpan, Validation},
    json::{Node, Value},
    markdown::{Document, ListItemEdit, SectionInsertion, TableColumnEdit, TableRowEdit},
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
    document: &Document,
    receipt: &mut Receipt,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    governor.charge(Resource::EvidenceEntries, 2)?;
    governor.charge(Resource::Work, 512)?;
    governor.charge(Resource::ScratchBytes, 1024)?;
    receipt.validations.push(Validation {
        validator: "markdown.structure.v1".into(),
        constraint: "the selected section, list, table, or reference structure and every unselected block reparse to the intended shape".into(),
        passed: true,
        diagnostics: Vec::new(),
    });
    receipt.evidence.push(Evidence {
        rule: "markdown.structure-edit.v1".into(),
        explanation: "The requested structural operation (section insert/remove/move/rename, list move/renumber, table row/column move, or reference add/relabel/remove/unlink) was reparsed and checked against its documented shape; outside bytes remain exact and the original source is retained.".into(),
        span: Some(SourceSpan { source: document.digest(), bytes: 0..document.source().len() }),
        destination: "/edits".into(),
    });
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
fn index(args: &Node, name: &str) -> Result<usize, StopReason> {
    let Some(Value::Number(value)) = args.get(name).map(|node| &node.value) else {
        return Err(StopReason::InvalidInput(format!("missing index {name}")));
    };
    value
        .to_usize()
        .ok_or_else(|| StopReason::InvalidInput(format!("index {name} exceeds address space")))
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
