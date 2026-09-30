// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `inspect_code`: governed grammar inspection via
//! [`purrassist_syntax::parse_governed`]/[`purrassist_syntax::inspect_with_allocation`],
//! reporting each issue's JSX rename repair options and the parse's
//! [`purrassist_syntax::AllocationReport`] alongside the existing
//! issues/symbols content, wrapped like every other governed tool.
use purrassist::{
    AssistResult, CancellationToken, Outcome, Receipt,
    json::{Node, Value},
    syntax::{self, AllocationReport, Language, Report, SyntaxIssue},
    wire::{array, integer, object, string},
};

pub(super) fn call(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let effort = super::effort(args)?;
    let source = super::field(args, "source")?;
    let language = language(super::field(args, "language")?)?;
    let mut governor = super::governor(effort.budget(), cancellation);
    let mut receipt = Receipt::new(
        source.as_bytes(),
        "syntax.inspect.v1",
        effort,
        effort.budget(),
    );
    let outcome = receipt
        .retain_source(source, &mut governor)
        .and_then(|()| syntax::inspect_with_allocation(source, language, &mut governor));
    let outcome = match outcome {
        Ok((report, allocation)) => Outcome::Resolved(report_node(&report, allocation)),
        Err(reason) => {
            receipt.stopped = reason;
            Outcome::Unresolved
        }
    };
    receipt.usage = governor.usage();
    Ok(AssistResult { outcome, receipt }.to_node())
}
fn language(name: &str) -> Result<Language, String> {
    match name {
        "python" => Ok(Language::Python),
        "rust" => Ok(Language::Rust),
        "javascript" => Ok(Language::JavaScript),
        "typescript" => Ok(Language::TypeScript),
        "tsx" => Ok(Language::Tsx),
        _ => Err("unsupported language".into()),
    }
}
fn report_node(report: &Report, allocation: AllocationReport) -> Node {
    object([
        ("valid", Node::new(Value::Bool(report.valid))),
        ("source", string(report.source.to_string())),
        ("parser", string(report.parser)),
        ("issues", array(report.issues.iter().map(issue_node))),
        (
            "symbols",
            array(report.symbols.iter().map(|symbol| {
                object([
                    ("name", string(&symbol.name)),
                    ("kind", string(&symbol.kind)),
                    ("start", integer(size(symbol.span.start))),
                    ("end", integer(size(symbol.span.end))),
                ])
            })),
        ),
        (
            "allocation",
            object([
                ("allocation_calls", integer(allocation.allocation_calls)),
                ("retained_bytes", integer(allocation.retained_bytes)),
                ("peak_bytes", integer(allocation.peak_bytes)),
                (
                    "max_overshoot_bytes",
                    integer(allocation.max_overshoot_bytes),
                ),
            ]),
        ),
    ])
}
fn issue_node(issue: &SyntaxIssue) -> Node {
    object([
        ("kind", string(&issue.kind)),
        ("start", integer(size(issue.span.start))),
        ("end", integer(size(issue.span.end))),
        ("missing", Node::new(Value::Bool(issue.missing))),
        (
            "rename",
            issue.rename.as_ref().map_or_else(
                || Node::new(Value::Null),
                |(opening, closing)| {
                    object([
                        (
                            "opening_name",
                            object([
                                ("start", integer(size(opening.start))),
                                ("end", integer(size(opening.end))),
                            ]),
                        ),
                        (
                            "closing_name",
                            object([
                                ("start", integer(size(closing.start))),
                                ("end", integer(size(closing.end))),
                            ]),
                        ),
                    ])
                },
            ),
        ),
    ])
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
