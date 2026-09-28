// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared table-row plans and structural validation receipts.
use purrassist::{
    Governor, Receipt, StopReason,
    contract::{Evidence, Resource, SourceSpan, Validation},
    json::{Node, Value},
    markdown::{Alignment, Document, TableColumnEdit, TableRowEdit},
};

pub(super) fn row(
    args: &Node,
    document: &Document,
    governor: &mut Governor,
) -> Result<Node, StopReason> {
    let index = super::index(args, "index")?;
    let edit = match super::super::field(args, "action").map_err(StopReason::InvalidInput)? {
        "insert" => {
            let cells = cells(args, governor)?;
            TableRowEdit::Insert { index, cells }
        }
        "remove" => TableRowEdit::Remove { index },
        _ => return Err(StopReason::InvalidInput("unknown table row action".into())),
    };
    Ok(super::plan(document.edit_table_row(
        &super::block_path(args)?,
        &edit,
        governor,
    )?))
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
        validator: "markdown.table-structure.v1".into(),
        constraint:
            "indexed source selection and table cell/row/column content match the requested edit"
                .into(),
        passed: true,
        diagnostics: Vec::new(),
    });
    receipt.evidence.push(Evidence {
        rule: "markdown.table-edit.v1".into(),
        explanation: "The source-bound proposal identifies exact changed ranges. Changed proposals were reparsed and checked against the complete expected table; unchanged cells require no mutation.".into(),
        span: Some(SourceSpan { source: document.digest(), bytes: 0..document.source().len() }),
        destination: "/edits".into(),
    });
    Ok(())
}

fn cells(args: &Node, governor: &mut Governor) -> Result<Vec<String>, StopReason> {
    let Some(Node {
        value: Value::Array(values),
        ..
    }) = args.get("cells")
    else {
        return Err(StopReason::InvalidInput("cells must be an array".into()));
    };
    governor.charge(
        Resource::ScratchBytes,
        super::size(values.len()).saturating_mul(32),
    )?;
    values
        .iter()
        .map(|node| {
            let value = node
                .as_str()
                .ok_or_else(|| StopReason::InvalidInput("cell must be a string".into()))?;
            governor.charge(Resource::Work, super::size(value.len()).saturating_add(1))?;
            governor.charge(Resource::ScratchBytes, super::size(value.len()))?;
            Ok(value.into())
        })
        .collect()
}

pub(super) fn column(
    args: &Node,
    document: &Document,
    governor: &mut Governor,
) -> Result<Node, StopReason> {
    let index = super::index(args, "index")?;
    let edit = match super::super::field(args, "action").map_err(StopReason::InvalidInput)? {
        "insert" => {
            let header = super::super::field(args, "header").map_err(StopReason::InvalidInput)?;
            governor.charge(Resource::ScratchBytes, super::size(header.len()))?;
            governor.charge(Resource::Work, super::size(header.len()).saturating_add(1))?;
            TableColumnEdit::Insert {
                index,
                header: header.into(),
                alignment: alignment(args)?,
                cells: cells(args, governor)?,
            }
        }
        "remove" => TableColumnEdit::Remove { index },
        "align" => TableColumnEdit::Align {
            index,
            alignment: alignment(args)?,
        },
        _ => {
            return Err(StopReason::InvalidInput(
                "unknown table column action".into(),
            ));
        }
    };
    Ok(super::plan(document.edit_table_column(
        &super::block_path(args)?,
        &edit,
        governor,
    )?))
}
fn alignment(args: &Node) -> Result<Alignment, StopReason> {
    match super::super::field(args, "alignment").map_err(StopReason::InvalidInput)? {
        "default" => Ok(Alignment::Default),
        "left" => Ok(Alignment::Left),
        "right" => Ok(Alignment::Right),
        "center" => Ok(Alignment::Center),
        _ => Err(StopReason::InvalidInput("unknown table alignment".into())),
    }
}
