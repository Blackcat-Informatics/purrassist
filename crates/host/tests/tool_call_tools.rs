// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The `recover_tool_calls` tool runs exactly core tool-call recovery, keeps
//! exhaustion an explicit unresolved outcome, and rejects malformed requests.
use purrassist::{
    Effort, Governor, Schema,
    json::{self, Node},
    tool_call::{EmitTarget, GenerationStop, IdPolicy, ToolChoice, ToolRecoveryRequest, ToolSet},
};
use purrassist_host::tools::Tools;
use std::path::PathBuf;

const TOOLS: &str = r#"[{"type":"function","function":{"name":"list_dir","description":"List a directory","parameters":{"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}},"required":["path"]}}}]"#;
const QWEN: &str = "Looking.\n<tool_call>\n<function=list_dir>\n<parameter=depth>\n2\n<parameter=path>\nsrc\n</parameter>\n</function>\n</tool_call>";

fn tools(tag: &str) -> (Tools, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "purrassist-tool-call-tools-{tag}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    (Tools::new(&root).unwrap(), root)
}

fn arguments(output: &str, extra: &str) -> Node {
    json::parse(&format!(
        r#"{{"output":{},"tools":{TOOLS}{extra}}}"#,
        json::quote(output)
    ))
    .unwrap()
}

#[test]
fn final_result_matches_core_recover() {
    let (tools, root) = tools("core");
    let args = arguments(
        QWEN,
        r#","stop":"tool_calls","tool_choice":{"type":"function","function":{"name":"list_dir"}},"effects":{"list_dir":"read-only"},"ids":{"prefix":"t_","start":4},"emit":"openai","effort":"deep""#,
    );
    let actual = tools.call("recover_tool_calls", &args).unwrap();

    let request = ToolRecoveryRequest::sized(Effort::Deep, QWEN.len())
        .stop(GenerationStop::ToolCalls)
        .tool_choice(ToolChoice::Forced("list_dir".into()))
        .effects("list_dir", purrassist::tool_call::Effects::ReadOnly)
        .ids(IdPolicy::Prefix {
            prefix: "t_".into(),
            start: 4,
        })
        .emit(EmitTarget::OpenAiToolCalls);
    let mut governor = Governor::new(request.budget);
    let declarations = json::parse(TOOLS).unwrap().to_json();
    let set = ToolSet::from_openai_governed(&declarations, &mut governor).unwrap();
    let expected = set.recover_governed(QWEN.as_bytes(), &request, &mut governor);

    assert_eq!(actual.to_json(), expected.to_node().to_json());
    assert_eq!(
        actual.get("status").and_then(Node::as_str),
        Some("resolved")
    );
    assert_eq!(
        actual.pointer("/value/calls/0/id").and_then(Node::as_str),
        Some("t_4")
    );
    assert_eq!(
        actual
            .pointer("/value/emitted/output/0/function/name")
            .and_then(Node::as_str),
        Some("list_dir")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn exhaustion_unresolved() {
    let (tools, root) = tools("exhaustion");
    let declarations: Vec<String> = (0..300)
        .map(|index| {
            format!(
                r#"{{"type":"function","function":{{"name":"t{index}","parameters":{{"type":"object","properties":{{"a":{{"type":"string"}}}}}}}}}}"#
            )
        })
        .collect();
    let args = json::parse(&format!(
        r#"{{"output":"<tool_call>{{\"name\": \"t1\", \"arguments\": {{}}}}</tool_call>","tools":[{}],"effort":"exact"}}"#,
        declarations.join(",")
    ))
    .unwrap();
    let result = tools.call("recover_tool_calls", &args).unwrap();
    assert_eq!(
        result.get("status").and_then(Node::as_str),
        Some("unresolved")
    );
    assert_eq!(
        result
            .pointer("/receipt/termination/kind")
            .and_then(Node::as_str),
        Some("budget_exceeded")
    );
    assert_eq!(
        result.pointer("/receipt/operation").and_then(Node::as_str),
        Some("tool_call.recover.v1")
    );
    assert_eq!(result.pointer("/value").unwrap().to_json(), "null");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_tools_is_tool_error() {
    let (tools, root) = tools("malformed");
    for declarations in [
        r#"{"type":"function"}"#,
        r#"[{"type":"function","function":{"name":"bad name!"}}]"#,
        r#"[{"type":"function","function":{"name":"a"}},{"type":"function","function":{"name":"a"}}]"#,
    ] {
        let args = json::parse(&format!(r#"{{"output":"","tools":{declarations}}}"#)).unwrap();
        let error = tools.call("recover_tool_calls", &args).unwrap_err();
        assert!(!error.is_empty(), "{declarations}");
    }
    let mcp_shape_as_anthropic =
        json::parse(r#"{"output":"","tools":{"tools":[]},"tools_format":"anthropic"}"#).unwrap();
    assert!(
        tools
            .call("recover_tool_calls", &mcp_shape_as_anthropic)
            .is_err()
    );
    let duplicate_dialect = arguments("", r#","dialects":["hermes.v1","hermes.v1"]"#);
    assert!(
        tools
            .call("recover_tool_calls", &duplicate_dialect)
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn tool_choice_json_forms() {
    let (tools, root) = tools("choice");
    let output = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    for (choice, constraint) in [
        (r#""auto""#, "tool_choice=auto"),
        (r#""none""#, "tool_choice=none"),
        (r#""required""#, "tool_choice=required"),
        (
            r#"{"type":"function","function":{"name":"list_dir"}}"#,
            "tool_choice=forced:list_dir",
        ),
        (r#"{"type":"auto"}"#, "tool_choice=auto"),
        (r#"{"type":"any"}"#, "tool_choice=required"),
        (r#"{"type":"none"}"#, "tool_choice=none"),
        (
            r#"{"type":"tool","name":"list_dir"}"#,
            "tool_choice=forced:list_dir",
        ),
    ] {
        let args = arguments(output, &format!(r#","tool_choice":{choice}"#));
        let result = tools.call("recover_tool_calls", &args).unwrap();
        assert_eq!(
            result
                .pointer("/value/tool_choice/constraint")
                .and_then(Node::as_str),
            Some(constraint),
            "{choice}"
        );
    }
    for malformed in [
        r#""sometimes""#,
        r#"{"type":"function","function":{}}"#,
        r#"{"type":"tool"}"#,
        r#"{"type":"auto","disable_parallel_tool_use":true}"#,
        r#"{"name":"list_dir"}"#,
    ] {
        let args = arguments(output, &format!(r#","tool_choice":{malformed}"#));
        let error = tools.call("recover_tool_calls", &args).unwrap_err();
        assert!(
            error.contains("invalid tool choice"),
            "{malformed}: {error}"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

/// A complete call followed by a dangling `<tool_call>` under the default
/// allow-list (every built-in dialect): the call stays usable, the dangling
/// attempt names both dialects that share the opener, and the result
/// validates against the tool's published output schema.
#[test]
fn dangling_opener_after_a_call_keeps_the_call() {
    let (tools, root) = tools("dangling");
    let output = format!("{QWEN}\n<tool_call>");
    let actual = tools
        .call(
            "recover_tool_calls",
            &arguments(&output, r#","effects":{"list_dir":"read-only"}"#),
        )
        .unwrap();
    assert_eq!(
        actual.get("status").and_then(Node::as_str),
        Some("best_guess"),
        "{}",
        actual.to_json()
    );
    assert_eq!(
        actual
            .pointer("/value/calls/0/outcome/status")
            .and_then(Node::as_str),
        Some("resolved")
    );
    assert_eq!(
        actual
            .pointer("/value/calls/0/candidate_dialects")
            .map(Node::to_json)
            .as_deref(),
        Some(r#"["qwen3-coder-xml.v1"]"#)
    );
    assert_eq!(
        actual
            .pointer("/value/rejected/0/candidate_dialects")
            .map(Node::to_json)
            .as_deref(),
        Some(r#"["hermes.v1","qwen3-coder-xml.v1"]"#)
    );
    let definitions = Tools::definitions().unwrap();
    let json::Value::Array(definitions) = definitions.value else {
        panic!("tool definitions must be an array")
    };
    let definition = definitions
        .iter()
        .find(|definition| {
            definition.get("name").and_then(Node::as_str) == Some("recover_tool_calls")
        })
        .unwrap();
    let schema = Schema::compile(&definition.get("outputSchema").unwrap().to_json()).unwrap();
    assert!(schema.validate(&actual).unwrap().valid);
    std::fs::remove_dir_all(root).unwrap();
}
