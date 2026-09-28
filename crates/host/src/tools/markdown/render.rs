// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Rendering profile validation and canonical wire encoding.
use purrassist::{
    Governor, StopReason,
    contract::Resource,
    digest::Digest,
    json::{Node, Value},
    markdown::{Dialect, Document, Extension, Extensions},
    wire::{array, object, string},
};
const EXTENSIONS: &[(&str, Extension)] = &[
    ("autolinks", Extension::Autolinks),
    ("strikethrough", Extension::Strikethrough),
    ("tables", Extension::Tables),
    ("task_lists", Extension::TaskLists),
    ("tag_filter", Extension::TagFilter),
];
pub(super) fn render(
    args: &Node,
    document: &Document,
    governor: &mut Governor,
) -> Result<Node, StopReason> {
    let (dialect, name, specification) = match args.get("dialect").and_then(Node::as_str) {
        None | Some("commonmark") => (Dialect::CommonMark, "commonmark", "0.31.2"),
        Some("gfm") => (Dialect::Gfm, "gfm", "0.29.0.gfm.13"),
        _ => return Err(StopReason::InvalidInput("unknown Markdown dialect".into())),
    };
    let mut extensions = if dialect == Dialect::Gfm {
        Extensions::gfm()
    } else {
        Extensions::default()
    };
    if let Some(value) = args.get("extensions") {
        let Value::Array(values) = &value.value else {
            return Err(StopReason::InvalidInput(
                "extensions must be an array".into(),
            ));
        };
        extensions = Extensions::default();
        for value in values {
            let Some((_, extension)) = EXTENSIONS
                .iter()
                .find(|(name, _)| Some(*name) == value.as_str())
            else {
                return Err(StopReason::InvalidInput(
                    "unknown Markdown extension".into(),
                ));
            };
            extensions = extensions.with(*extension);
        }
    }
    let html = document.render_with_extensions(dialect, extensions, governor)?;
    governor.charge(Resource::Work, super::size(html.len()).saturating_add(1))?;
    governor.charge(
        Resource::ScratchBytes,
        super::size(html.len())
            .saturating_mul(2)
            .saturating_add(1024),
    )?;
    let output = Digest::of(html.as_bytes());
    Ok(object([
        ("source", string(document.digest().to_string())),
        ("renderer", string("purrassist.markdown.v1")),
        ("dialect", string(name)),
        ("specification", string(specification)),
        (
            "extensions",
            array(
                EXTENSIONS
                    .iter()
                    .filter(|(_, extension)| extensions.contains(*extension))
                    .map(|(name, _)| string(*name)),
            ),
        ),
        ("output", string(output.to_string())),
        ("html", string(html)),
    ]))
}
