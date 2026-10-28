// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Actual registry output contracts consumed through the prepared-tool API.
use purrassist::{Budget, Governor, json, tool_call::ToolSet};
use purrassist_host::tools::Tools;

#[test]
fn registry_match_result_uses_its_exact_admitted_output_validator() {
    let definitions = Tools::definitions().unwrap();
    let json::Value::Array(declarations) = &definitions.value else {
        panic!("registry declarations");
    };
    let declaration = declarations
        .iter()
        .find(|node| node.get("name").and_then(json::Node::as_str) == Some("match_strings"))
        .unwrap();
    let mut governor = Governor::new(Budget::default());
    let source = declaration.to_json_governed(&mut governor).unwrap();
    let compiled = ToolSet::from_mcp_governed(&source, &mut governor).unwrap();
    let tool = compiled.tool("match_strings").unwrap();
    let args = json::parse(r#"{"query":"hello","candidates":["hello"]}"#).unwrap();
    assert!(tool.schema().validate(&args).unwrap().valid);
    let root =
        std::env::temp_dir().join(format!("purrassist-output-contract-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let host = Tools::new(&root).unwrap();
    let result = host.call("match_strings", &args).unwrap();
    let output = tool.output_schema().unwrap();
    assert!(
        output
            .validate_with(&result, &mut Governor::new(Budget::default()))
            .unwrap()
            .valid
    );
    assert!(!output.validate(&json::parse("{}").unwrap()).unwrap().valid);
    assert!(
        !output
            .validate(&json::parse("false").unwrap())
            .unwrap()
            .valid
    );
    assert!(tool.schema().validate(&args).unwrap().valid);
    std::fs::remove_dir_all(root).unwrap();
}
