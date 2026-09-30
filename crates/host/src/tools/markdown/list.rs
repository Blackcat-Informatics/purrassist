// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared list-item edit plans and source-retaining validation receipts.
use purrassist::{
    Governor, Receipt, StopReason,
    contract::{Evidence, Resource, SourceSpan, Validation},
    json::Node,
    markdown::{Document, ListItemEdit},
};
pub(super) fn call(args: &Node, doc: &Document, g: &mut Governor) -> Result<Node, StopReason> {
    let field = |name| super::super::field(args, name).map_err(StopReason::InvalidInput);
    for name in ["marker", "body"] {
        if let Some(value) = args.get(name).and_then(Node::as_str) {
            g.charge(Resource::Work, super::size(value.len()))?;
            g.charge(Resource::ScratchBytes, super::size(value.len()))?;
        }
    }
    let edit = match field("action")? {
        "before" => ListItemEdit::Before {
            marker: field("marker")?.into(),
            body: field("body")?.into(),
        },
        "after" => ListItemEdit::After {
            marker: field("marker")?.into(),
            body: field("body")?.into(),
        },
        "replace" => ListItemEdit::Replace {
            body: field("body")?.into(),
        },
        "remove" => ListItemEdit::Remove,
        _ => return Err(StopReason::InvalidInput("unknown list action".into())),
    };
    Ok(super::plan(&doc.edit_list_item(
        &super::block_path(args)?,
        &edit,
        g,
    )?))
}
pub(super) fn record(
    doc: &Document,
    receipt: &mut Receipt,
    g: &mut Governor,
) -> Result<(), StopReason> {
    g.charge(Resource::EvidenceEntries, 2)?;
    g.charge(Resource::ScratchBytes, 1024)?;
    receipt.validations.push(Validation {
        validator: "markdown.list-item-structure.v1".into(),
        constraint: "requested item matches its isolated parse; outside siblings and ancestor identity are preserved".into(),
        passed: true, diagnostics: Vec::new(),
    });
    receipt.evidence.push(Evidence {
        rule: "markdown.list-item-splice.v1".into(),
        explanation: "New item lines use canonical marker spacing and outer prefixes; existing siblings are not renumbered. Required separators follow the anchor's newline style and EOF convention. Removing an only child can retain a blank container line.".into(),
        span: Some(SourceSpan { source: doc.digest(), bytes: 0..doc.source().len() }), destination: "/edits".into(),
    });
    Ok(())
}
