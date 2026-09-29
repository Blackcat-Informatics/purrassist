// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `validate_json`: whole-instance JSON Schema validation reachable over MCP,
//! reusing exactly `purrassist_core::schema::Schema::validate_with`'s
//! evidence. The shipped CLI's `purrassist validate` command emits this same
//! tool's output.
use purrassist::{
    CancellationToken, Receipt,
    json::{Node, Value},
    schema::Schema,
    wire::{array, object, string},
};

struct Report {
    valid: bool,
    violations: Vec<purrassist::schema::Violation>,
    format_evidence: Vec<purrassist::contract::Validation>,
    pattern_evidence: Vec<purrassist::contract::Evidence>,
    unsupported: Vec<purrassist::schema::UnsupportedVocabulary>,
}

pub(super) fn call(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let effort = super::effort(args)?;
    let schema_text = args.get("schema").ok_or("missing schema")?.to_json();
    let mut governor = super::governor(effort.budget(), cancellation);
    let mut receipt = Receipt::new(
        schema_text.as_bytes(),
        "schema.validate.v1",
        effort,
        effort.budget(),
    );
    let outcome = (|| -> Result<Report, String> {
        let schema =
            Schema::compile_governed(&schema_text, &mut governor).map_err(|e| e.to_string())?;
        let instance = args.get("instance").ok_or("missing instance")?;
        let report = schema
            .validate_with(instance, &mut governor)
            .map_err(|e| e.to_string())?;
        Ok(Report {
            valid: report.valid,
            violations: report.violations,
            format_evidence: report.format_evidence,
            pattern_evidence: report.pattern_evidence,
            unsupported: schema.unsupported_vocabularies(),
        })
    })();
    let report = match outcome {
        Ok(report) => report,
        Err(error) => {
            receipt.stopped = purrassist::StopReason::InvalidInput(error);
            Report {
                valid: false,
                violations: Vec::new(),
                format_evidence: Vec::new(),
                pattern_evidence: Vec::new(),
                unsupported: Vec::new(),
            }
        }
    };
    receipt.usage = governor.usage();
    Ok(object([
        ("valid", Node::new(Value::Bool(report.valid))),
        (
            "violations",
            array(report.violations.iter().map(|violation| {
                object([
                    ("instance_path", string(&violation.instance_path)),
                    ("schema_path", string(&violation.schema_path)),
                    ("keyword", string(&violation.keyword)),
                    ("message", string(&violation.message)),
                ])
            })),
        ),
        (
            "format_evidence",
            array(report.format_evidence.iter().map(|entry| {
                object([
                    ("validator", string(&entry.validator)),
                    ("constraint", string(&entry.constraint)),
                    ("passed", Node::new(Value::Bool(entry.passed))),
                    ("diagnostics", array(entry.diagnostics.iter().map(string))),
                ])
            })),
        ),
        (
            "pattern_evidence",
            array(report.pattern_evidence.iter().map(evidence)),
        ),
        (
            "unsupported_vocabularies",
            array(report.unsupported.iter().map(|entry| {
                object([
                    ("uri", string(&entry.uri)),
                    ("keywords", array(entry.keywords.iter().map(string))),
                ])
            })),
        ),
        ("receipt", receipt.to_node()),
    ]))
}
fn evidence(entry: &purrassist::contract::Evidence) -> Node {
    object([
        ("rule", string(&entry.rule)),
        ("explanation", string(&entry.explanation)),
        ("destination", string(&entry.destination)),
        (
            "span",
            entry.span.as_ref().map_or_else(
                || Node::new(Value::Null),
                |span| {
                    object([
                        ("source", string(span.source.to_string())),
                        (
                            "start",
                            purrassist::wire::integer(
                                u64::try_from(span.bytes.start).unwrap_or(u64::MAX),
                            ),
                        ),
                        (
                            "end",
                            purrassist::wire::integer(
                                u64::try_from(span.bytes.end).unwrap_or(u64::MAX),
                            ),
                        ),
                    ])
                },
            ),
        ),
    ])
}
