// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The shared validation tool reports nested observations to CLI/MCP callers.
use purrassist::json;
use purrassist_host::tools::Tools;

#[test]
fn validation_tool_retains_nested_failed_branch_evidence() {
    let root =
        std::env::temp_dir().join(format!("purrassist-schema-evidence-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let result = tools
        .call(
            "validate_json",
            &json::parse(
                r#"{
        "schema":{"properties":{"email":{"anyOf":[{"format":"email","pattern":"^x"},true]}}},
        "instance":{"email":"bad"}
    }"#,
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(result.get("valid").unwrap().to_json(), "true");
    assert_eq!(
        result
            .pointer("/format_evidence/0/constraint")
            .unwrap()
            .as_str(),
        Some("/email")
    );
    assert_eq!(
        result
            .pointer("/format_evidence/0/passed")
            .unwrap()
            .to_json(),
        "false"
    );
    assert!(
        result
            .pointer("/pattern_evidence/0/rule")
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with("pattern.engine.")
    );
    std::fs::remove_dir_all(root).unwrap();
}
