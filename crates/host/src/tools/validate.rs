// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `validate_json`: whole-instance JSON Schema validation reachable over MCP,
//! reusing exactly `purrassist_core::schema::Schema::validate_with`'s
//! evidence. The shipped CLI's `purrassist validate` command emits this same
//! tool's output.
use purrassist::{
    CancellationToken, Receipt,
    json::{Node, Value},
    schema::{CompilationError, Schema},
    wire::{array, evidence, object, string, validation},
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
    let outcome = (|| -> Result<Report, purrassist::StopReason> {
        let schema =
            Schema::compile_governed(&schema_text, &mut governor).map_err(|error| match error {
                CompilationError::InvalidSchema(error) => {
                    purrassist::StopReason::InvalidInput(error.to_string())
                }
                CompilationError::Resource(reason) => reason,
            })?;
        let instance = args
            .get("instance")
            .ok_or_else(|| purrassist::StopReason::InvalidInput("missing instance".into()))?;
        let report = schema.validate_with(instance, &mut governor)?;
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
            receipt.stopped = error;
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
        ("violations", array(report.violations.iter().map(violation))),
        (
            "format_evidence",
            array(report.format_evidence.iter().map(validation)),
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
pub(super) fn violation(entry: &purrassist::schema::Violation) -> Node {
    object([
        ("instance_path", string(&entry.instance_path)),
        ("schema_path", string(&entry.schema_path)),
        ("keyword", string(&entry.keyword)),
        ("message", string(&entry.message)),
    ])
}

#[cfg(test)]
mod tests {
    #[test]
    fn cancellation_during_the_engine_phase_keeps_its_typed_termination() {
        // Registry admission checks cancellation separately. Exercise the
        // handler's own compilation failure path, which must also preserve it.
        let token = purrassist::CancellationToken::new();
        token.cancel();
        let args = purrassist::json::parse(r#"{"instance":42,"schema":true}"#).unwrap();
        let result = super::call(&args, Some(&token)).unwrap();
        assert_eq!(
            result
                .pointer("/receipt/termination/kind")
                .unwrap()
                .as_str(),
            Some("cancelled")
        );
        assert_eq!(result.get("valid").unwrap().to_json(), "false");
        assert_eq!(result.get("violations").unwrap().to_json(), "[]");
    }
}
