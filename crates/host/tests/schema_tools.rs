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
