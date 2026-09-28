// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-preserving Markdown operations shared by CLI and MCP.
mod render;
use purrassist::{
    AssistResult, Governor, Outcome, Receipt, StopReason,
    contract::Resource,
    json::{Node, Value},
    markdown::{Alignment, Block, BlockKind, Document, Section},
    wire::{array, integer, object, string},
};
use std::ops::Range;

pub(super) fn call(name: &str, args: &Node) -> Result<Node, String> {
    let source = super::field(args, "source")?;
    let effort = super::effort(args)?;
    let mut governor = Governor::new(effort.budget());
    let mut receipt = Receipt::new(source.as_bytes(), name, effort, effort.budget());
    let result = receipt.retain_source(source, &mut governor).and_then(|()| {
        let document = Document::parse(source, &mut governor)?;
        // Reserve output structure and section-index storage before encoding.
        governor.charge(
            Resource::ScratchBytes,
            size(source.len()).saturating_mul(256).saturating_add(4096),
        )?;
        governor.charge(
            Resource::Work,
            size(source.len()).saturating_mul(4).saturating_add(1),
        )?;
        let value = operation(name, args, &document, &mut governor)?;
        if name == "render_markdown" {
            governor.charge(Resource::EvidenceEntries, 1)?;
            governor.charge(Resource::ScratchBytes, 512)?;
            receipt.evidence.push(purrassist::contract::Evidence {
                rule: "markdown.render.v1".into(),
                explanation: "Rendered using the dialect and effective extensions reported in the output; original source remains retained. HTML is not sanitized.".into(),
                span: Some(purrassist::contract::SourceSpan { source: document.digest(), bytes: 0..source.len() }),
                destination: "/html".into(),
            });
        }
        Ok(value)
    });
    let outcome = match result {
        Ok(value) => Outcome::Resolved(value),
        Err(reason) => {
            receipt.stopped = reason;
            Outcome::Unresolved
        }
    };
    receipt.usage = governor.usage();
    Ok(AssistResult { outcome, receipt }.to_node())
}
fn operation(
    name: &str,
    args: &Node,
    document: &Document,
    governor: &mut Governor,
) -> Result<Node, StopReason> {
    match name {
        "render_markdown" => render::render(args, document, governor),
        "inspect_markdown" => Ok(object([
            ("source", string(document.digest().to_string())),
            (
                "blocks",
                array(
                    document
                        .blocks()
                        .iter()
                        .enumerate()
                        .map(|(index, value)| block(value, &[index])),
                ),
            ),
            ("sections", array(document.sections().iter().map(section))),
        ])),
        "markdown_context" => {
            let context = document.context(
                index(args, "start")?..index(args, "end")?,
                if args.get("neighbors").is_some() {
                    index(args, "neighbors")?
                } else {
                    0
                },
                governor,
            )?;
            Ok(object([
                ("source", string(context.source.to_string())),
                ("focus", span(&context.focus)),
                ("span", span(&context.span)),
                ("text", string(context.text)),
                ("sections", array(context.sections.iter().map(section))),
            ]))
        }
        "plan_markdown_section" => {
            let body = super::field(args, "body").map_err(StopReason::InvalidInput)?;
            governor.charge(Resource::InputBytes, size(body.len()))?;
            governor.charge(Resource::Work, size(body.len()))?;
            governor.charge(Resource::ScratchBytes, size(body.len()).saturating_mul(4))?;
            Ok(plan(
                document.replace_section(index(args, "heading")?, body)?,
            ))
        }
        "plan_markdown_task" => {
            let Some(Value::Bool(checked)) = args.get("checked").map(|node| &node.value) else {
                return Err(StopReason::InvalidInput("checked must be boolean".into()));
            };
            let path = if let Some(Node {
                value: Value::Array(values),
                ..
            }) = args.get("path")
            {
                values
                    .iter()
                    .map(|node| match &node.value {
                        Value::Number(value) => value.to_usize().ok_or_else(|| {
                            StopReason::InvalidInput(
                                "block path index exceeds address space".into(),
                            )
                        }),
                        _ => Err(StopReason::InvalidInput(
                            "block path must contain indices".into(),
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                vec![index(args, "block")?]
            };
            Ok(plan(document.set_task_at(&path, *checked, governor)?))
        }
        _ => Err(StopReason::InvalidInput(
            "unknown Markdown operation".into(),
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
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
fn span(range: &Range<usize>) -> Node {
    object([
        ("start", integer(size(range.start))),
        ("end", integer(size(range.end))),
    ])
}
fn section(value: &Section) -> Node {
    object([
        ("heading", integer(size(value.heading))),
        ("span", span(&value.span)),
        ("body", span(&value.body)),
    ])
}
fn plan(value: purrassist::edits::EditPlan) -> Node {
    object([
        ("source", string(value.source.to_string())),
        (
            "edits",
            array(value.edits.into_iter().map(|edit| {
                object([
                    ("start", integer(size(edit.range.start))),
                    ("end", integer(size(edit.range.end))),
                    ("replacement", string(edit.replacement)),
                ])
            })),
        ),
    ])
}
fn block(value: &Block, path: &[usize]) -> Node {
    object([
        ("span", span(&value.span)),
        (
            "lines",
            array(
                value
                    .lines
                    .iter()
                    .zip(&value.line_padding)
                    .map(|(range, padding)| {
                        object([("span", span(range)), ("padding", integer(size(*padding)))])
                    }),
            ),
        ),
        (
            "path",
            array(path.iter().map(|index| integer(size(*index)))),
        ),
        ("kind", kind(&value.kind)),
        (
            "children",
            array(value.children.iter().enumerate().map(|(index, child)| {
                let mut path = path.to_vec();
                path.push(index);
                block(child, &path)
            })),
        ),
    ])
}
fn kind(value: &BlockKind) -> Node {
    let mut fields = Vec::new();
    let name = match value {
        BlockKind::Heading { level, title } => {
            fields.extend([
                ("level", integer(u64::from(*level))),
                ("title", string(title)),
            ]);
            "heading"
        }
        BlockKind::CodeFence {
            info,
            content,
            closed,
        } => {
            fields.extend([
                ("info", string(info)),
                ("content", span(content)),
                ("closed", Node::new(Value::Bool(*closed))),
            ]);
            "code_fence"
        }
        BlockKind::ListItem { ordinal, checked } => {
            fields.extend([
                (
                    "ordinal",
                    ordinal.map_or_else(|| Node::new(Value::Null), integer),
                ),
                (
                    "checked",
                    checked.map_or_else(
                        || Node::new(Value::Null),
                        |value| Node::new(Value::Bool(value)),
                    ),
                ),
            ]);
            "list_item"
        }
        BlockKind::Table {
            header,
            alignments,
            rows,
        } => {
            fields.extend([
                ("header", array(header.iter().map(string))),
                (
                    "alignments",
                    array(alignments.iter().map(|alignment| {
                        string(match alignment {
                            Alignment::Default => "default",
                            Alignment::Left => "left",
                            Alignment::Right => "right",
                            Alignment::Center => "center",
                        })
                    })),
                ),
                (
                    "rows",
                    array(rows.iter().map(|row| array(row.iter().map(string)))),
                ),
            ]);
            "table"
        }
        BlockKind::IndentedCode => "indented_code",
        BlockKind::ThematicBreak => "thematic_break",
        BlockKind::Quote => "quote",
        BlockKind::ReferenceDefinition => "reference_definition",
        BlockKind::Html => "html",
        BlockKind::Paragraph => "paragraph",
        BlockKind::Blank => "blank",
    };
    fields.push(("type", string(name)));
    object(fields)
}
