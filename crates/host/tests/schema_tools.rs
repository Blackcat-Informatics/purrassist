// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Schema preparation consumes the same effort budget as JSON recovery.
use purrassist::{
    json,
    wire::{object, string},
};
use purrassist_host::tools::Tools;

#[test]
fn decoding_reports_schema_exhaustion_and_keeps_invalid_schema_distinct() {
    let root = std::env::temp_dir().join(format!("purrassist-schema-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let schema = object([("description", string("large annotation ".repeat(7000)))]);
    let args = object([
        ("input", string("42")),
        ("schema", schema),
        ("effort", string("exact")),
    ]);
    let result = tools.call("decode_json", &args).unwrap();
    assert_eq!(
        result.pointer("/status").unwrap().as_str(),
        Some("unresolved")
    );
    assert_eq!(
        result
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("budget_exceeded")
    );
    assert!(
        tools
            .call(
                "decode_json",
                &json::parse(r#"{"input":"42","schema":{"type":false}}"#).unwrap()
            )
            .is_err()
    );
    let exact = tools
        .call(
            "decode_json",
            &json::parse(r#"{"input":"42","schema":{"type":"integer"},"effort":"exact"}"#).unwrap(),
        )
        .unwrap();
    assert_eq!(exact.pointer("/status").unwrap().as_str(), Some("resolved"));
    assert_eq!(
        exact.pointer("/receipt/sources/0/text").unwrap().as_str(),
        Some("42")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validation_preserves_compilation_and_evaluation_exhaustion_reasons() {
    let root =
        std::env::temp_dir().join(format!("purrassist-validation-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let schema = object([("description", string("large annotation ".repeat(7000)))]);
    let args = object([
        ("instance", json::parse("42").unwrap()),
        ("schema", schema),
        ("effort", string("exact")),
    ]);
    let result = tools.call("validate_json", &args).unwrap();
    assert_eq!(
        result
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("budget_exceeded")
    );
    assert_eq!(result.get("valid").unwrap().to_json(), "false");
    assert_eq!(result.get("violations").unwrap().to_json(), "[]");

    let schema = json::parse(&format!(
        r#"{{"type":"array","items":{{"enum":[{}]}}}}"#,
        (0..200)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(",")
    ))
    .unwrap();
    let instance =
        purrassist::json::Node::new(purrassist::json::Value::Array(vec![
            json::parse("199")
                .unwrap();
            1000
        ]));
    let result = tools
        .call(
            "validate_json",
            &object([
                ("instance", instance),
                ("schema", schema),
                ("effort", string("exact")),
            ]),
        )
        .unwrap();
    assert_eq!(
        result
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("budget_exceeded")
    );
    assert_eq!(result.get("violations").unwrap().to_json(), "[]");
    let invalid = tools
        .call(
            "validate_json",
            &json::parse(r#"{"instance":42,"schema":{"type":false}}"#).unwrap(),
        )
        .unwrap();
    assert_eq!(
        invalid
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("invalid_input")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn prepared_helper_receipt_declares_the_admitted_helper_budget() {
    let root =
        std::env::temp_dir().join(format!("purrassist-helper-budget-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let result = tools.call("prepare_helper", &json::parse(r#"{"provider":"anthropic_messages","model":"example-model","input":"42","schema":true,"max_output_tokens":16}"#).unwrap()).unwrap();
    assert_eq!(
        result
            .pointer("/receipt/budget/helper_calls")
            .unwrap()
            .to_json(),
        "1"
    );
    assert_eq!(
        result
            .pointer("/receipt/usage/helper_calls")
            .unwrap()
            .to_json(),
        "1"
    );
    std::fs::remove_dir_all(root).unwrap();
}
