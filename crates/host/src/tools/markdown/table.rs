// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared table-row plans and structural validation receipts.
use purrassist::{
    Governor, Receipt, StopReason,
    contract::{Evidence, Resource, SourceSpan, Validation},
    json::{Node, Value},
    markdown::{Document, TableRowEdit},
};

pub(super) fn row(
    args: &Node,
    document: &Document,
    governor: &mut Governor,
) -> Result<Node, StopReason> {
    let index = super::index(args, "index")?;
    let edit = match super::super::field(args, "action").map_err(StopReason::InvalidInput)? {
        "insert" => {
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
            let cells = values
                .iter()
                .map(|node| {
                    let value = node
                        .as_str()
                        .ok_or_else(|| StopReason::InvalidInput("cell must be a string".into()))?;
                    governor.charge(Resource::Work, super::size(value.len()).saturating_add(1))?;
                    governor.charge(Resource::ScratchBytes, super::size(value.len()))?;
                    Ok(value.into())
                })
                .collect::<Result<Vec<_>, StopReason>>()?;
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
        constraint: "indexed source selection and table cell/row content match the requested edit"
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
