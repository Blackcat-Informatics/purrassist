// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared reference occurrence inspection and verified field-edit receipts.
use purrassist::{
    Governor, Receipt, StopReason,
    contract::{Evidence, Resource, SourceSpan, Validation},
    json::{Node, Value},
    markdown::{Document, ReferenceSite},
    wire::{array, integer, object, string},
};

pub(super) fn sites(document: &Document, governor: &mut Governor) -> Result<Node, StopReason> {
    Ok(array(document.reference_sites(governor)?.iter().map(site)))
}
fn site(value: &ReferenceSite) -> Node {
    object([
        ("source", string(value.source.to_string())),
        (
            "path",
            array(value.path.iter().map(|index| integer(super::size(*index)))),
        ),
        ("occurrence", integer(super::size(value.occurrence))),
        ("span", super::span(&value.span)),
        ("label", string(&value.label)),
        ("destination", string(&value.destination)),
        ("destination_span", super::span(&value.destination_span)),
        (
            "title",
            value
                .title
                .as_ref()
                .map_or_else(|| Node::new(Value::Null), string),
        ),
        (
            "title_span",
            value
                .title_span
                .as_ref()
                .map_or_else(|| Node::new(Value::Null), super::span),
        ),
    ])
}
pub(super) fn call(
    args: &Node,
    document: &Document,
    governor: &mut Governor,
) -> Result<Node, StopReason> {
    let title = match args.get("title").map(|node| &node.value) {
        Some(Value::String(value)) => Some(value.as_str()),
        Some(Value::Null) => None,
        _ => {
            return Err(StopReason::InvalidInput(
                "title must be a string or null".into(),
            ));
        }
    };
    Ok(super::plan(&document.replace_reference(
        &super::block_path(args)?,
        super::index(args, "occurrence")?,
        super::super::field(args, "destination").map_err(StopReason::InvalidInput)?,
        title,
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
        validator: "markdown.reference-definition.v1".into(),
        constraint: "selected decoded fields match the request; ordered unselected definition labels and values remain unchanged".into(),
        passed: true, diagnostics: Vec::new(),
    });
    receipt.evidence.push(Evidence {
        rule: "markdown.reference-fields.v1".into(),
        explanation: "The source-bound field region is encoded with escapes/entities and checked by reparsing all definitions. Multiline field spelling may collapse; labels and surrounding bytes remain retained.".into(),
        span: Some(SourceSpan { source: document.digest(), bytes: 0..document.source().len() }), destination: "/edits".into(),
    });
    Ok(())
}
