// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The `recover_tool_calls` tool runs exactly core tool-call recovery, keeps
//! exhaustion an explicit unresolved outcome, and rejects malformed requests.
use purrassist::{
    Effort, Governor, Schema,
    digest::Digest,
    json::{self, Node},
    tool_call::{
        EmitTarget, GenerationStop, IdPolicy, ToolChoice, ToolRecoveryRequest, ToolSet,
        turn_id_prefix,
    },
};
use purrassist_host::tools::Tools;
use std::fmt::Write as _;
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

    let request = ToolRecoveryRequest::sized(Effort::Deep, QWEN.as_bytes())
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

/// G5: with no `ids` option, the MCP tool numbers calls from each turn's
/// digest, so two rounds get disjoint ids without caller state; the same
/// turn gets the same ids; a model's own `OpenAI` id is kept; and an
/// explicit `{"prefix", "start"}` numbers exactly as before.
#[test]
fn default_ids_are_disjoint_across_turns() {
    let (tools, root) = tools("ids");
    let ids = |output: &str, extra: &str| {
        let actual = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        (0..)
            .map_while(|index| {
                actual
                    .pointer(&format!("/value/calls/{index}/id"))
                    .and_then(Node::as_str)
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>()
    };
    let second = QWEN.replace("src", "docs");
    let round_one = ids(QWEN, "");
    let round_two = ids(&second, "");
    let tag = |output: &str| turn_id_prefix(&Digest::of(output.as_bytes()));
    assert_eq!(round_one, [format!("{}0", tag(QWEN))]);
    assert_eq!(round_two, [format!("{}0", tag(&second))]);
    assert_ne!(round_one, round_two);
    assert_eq!(ids(QWEN, ""), round_one);
    assert_eq!(ids(QWEN, r#","ids":{}"#), ["call_0"]);
    assert_eq!(ids(QWEN, r#","ids":{"start":3}"#), ["call_3"]);
    let openai = r#"[{"id":"call_model","type":"function","function":{"name":"list_dir","arguments":"{\"path\":\"src\"}"}}]"#;
    assert_eq!(ids(openai, ""), ["call_model"]);
    std::fs::remove_dir_all(root).unwrap();
}

/// G7: the default MCP request sizes its candidate budget to the turn. A
/// 64-call turn whose every call needs a markup repair, with a stray
/// `<tool_call>` line after each that two dialects contest, recovers every
/// call `Resolved` with no effort option.
#[test]
fn busy_turn_resolves_every_call_by_default() {
    let (tools, root) = tools("busy");
    let mut output = String::new();
    for index in 0..64 {
        let _ = write!(
            output,
            "<tool_call>\n<function=list_dir>\n<parameter=depth>\n{index}\n<parameter=path>\nsrc/{index}\n</parameter>\n</function>\n</tool_call>\n<tool_call>\n"
        );
    }
    let actual = tools
        .call("recover_tool_calls", &arguments(&output, ""))
        .unwrap();
    assert_eq!(
        actual.get("status").and_then(Node::as_str),
        Some("best_guess"),
        "{}",
        actual
            .pointer("/receipt/termination")
            .map(Node::to_json)
            .unwrap_or_default()
    );
    for index in 0..64 {
        assert_eq!(
            actual
                .pointer(&format!("/value/calls/{index}/outcome/status"))
                .and_then(Node::as_str),
            Some("resolved"),
            "call {index}"
        );
    }
    assert!(actual.pointer("/value/calls/64").is_none());
    assert_eq!(
        actual
            .pointer("/receipt/budget/candidates")
            .map(Node::to_json),
        Some((128 + 16 * 256).to_string())
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

/// The MCP tool accepts `llama3-json.v1` in its dialect allow-list and as
/// an emission target, and reads Llama 3 JSON under the default allow-list.
#[test]
fn llama3_json_dialect_over_mcp() {
    let (tools, root) = tools("llama3");
    let output = r#"{"name": "list_dir", "parameters": {"path": "src", "depth": 2}}"#;
    for extra in ["", r#","dialects":["llama3-json.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
        assert_eq!(
            node.pointer("/value/calls/0/dialect")
                .and_then(Node::as_str),
            Some("llama3-json.v1")
        );
        assert_eq!(
            node.pointer("/value/calls/0/arguments").unwrap().to_json(),
            r#"{"path":"src","depth":2}"#
        );
    }
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","emit":"llama3-json.v1""#),
        )
        .unwrap();
    assert_eq!(
        node.pointer("/value/emitted/output/text")
            .and_then(Node::as_str),
        Some(r#"{"name": "list_dir", "parameters": {"depth":2,"path":"src"}}"#)
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool accepts `mistral-v3.v1` in its allow-list and as an
/// emission target, and keeps the model's call ids.
#[test]
fn mistral_v3_dialect_over_mcp() {
    let (tools, root) = tools("mistral");
    let output =
        r#"[TOOL_CALLS] [{"name": "list_dir", "arguments": {"path": "src"}, "id": "a1B2c3D4e"}]"#;
    for extra in ["", r#","dialects":["mistral-v3.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
        assert_eq!(
            node.pointer("/value/calls/0/dialect")
                .and_then(Node::as_str),
            Some("mistral-v3.v1")
        );
        assert_eq!(
            node.pointer("/value/calls/0/source_id")
                .and_then(Node::as_str),
            Some("a1B2c3D4e")
        );
    }
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","emit":"mistral-v3.v1""#),
        )
        .unwrap();
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted");
    assert!(
        emitted.starts_with(
            r#"[TOOL_CALLS] [{"name": "list_dir", "arguments": {"depth":2,"path":"src"}, "id": "call_"#
        ),
        "{emitted}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool accepts `mistral-v11.v1` in its allow-list and as an
/// emission target.
#[test]
fn mistral_v11_dialect_over_mcp() {
    let (tools, root) = tools("mistral11");
    let output = r#"[TOOL_CALLS]list_dir[CALL_ID]a1B2c3D4e[ARGS]{"path": "src"}"#;
    for extra in ["", r#","dialects":["mistral-v11.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
        assert_eq!(
            node.pointer("/value/calls/0/source_id")
                .and_then(Node::as_str),
            Some("a1B2c3D4e")
        );
    }
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","emit":"mistral-v11.v1""#),
        )
        .unwrap();
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted");
    assert!(
        emitted.starts_with("[TOOL_CALLS]list_dir[CALL_ID]call_"),
        "{emitted}"
    );
    assert!(
        emitted.ends_with(r#"[ARGS]{"depth":2,"path":"src"}"#),
        "{emitted}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool accepts `deepseek-v3.1.v1` in its allow-list and as an
/// emission target.
#[test]
fn deepseek_v31_dialect_over_mcp() {
    let (tools, root) = tools("deepseek31");
    let output = "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>list_dir<｜tool▁sep｜>{\"path\": \"src\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>";
    for extra in ["", r#","dialects":["deepseek-v3.1.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
        assert_eq!(
            node.pointer("/value/calls/0/dialect")
                .and_then(Node::as_str),
            Some("deepseek-v3.1.v1")
        );
    }
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","emit":"deepseek-v3.1.v1""#),
        )
        .unwrap();
    assert_eq!(
        node.pointer("/value/emitted/output/text")
            .and_then(Node::as_str),
        Some(
            "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>list_dir<｜tool▁sep｜>{\"depth\":2,\"path\":\"src\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>"
        )
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool accepts `deepseek-v3.v1` in its allow-list and as an
/// emission target.
#[test]
fn deepseek_v3_dialect_over_mcp() {
    let (tools, root) = tools("deepseek3");
    let output = "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>list_dir\n```json\n{\"path\": \"src\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>";
    for extra in ["", r#","dialects":["deepseek-v3.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
        assert_eq!(
            node.pointer("/value/calls/0/dialect")
                .and_then(Node::as_str),
            Some("deepseek-v3.v1")
        );
    }
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","emit":"deepseek-v3.v1""#),
        )
        .unwrap();
    assert_eq!(
        node.pointer("/value/emitted/output/text")
            .and_then(Node::as_str),
        Some(
            "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>list_dir\n```json\n{\"depth\":2,\"path\":\"src\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>"
        )
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool accepts `kimi-k2.v1` in its allow-list and as an emission
/// target.
#[test]
fn kimi_k2_dialect_over_mcp() {
    let (tools, root) = tools("kimi");
    let output = "<|tool_calls_section_begin|><|tool_call_begin|>functions.list_dir:0<|tool_call_argument_begin|>{\"path\": \"src\"}<|tool_call_end|><|tool_calls_section_end|>";
    for extra in ["", r#","dialects":["kimi-k2.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
        assert_eq!(
            node.pointer("/value/calls/0/dialect")
                .and_then(Node::as_str),
            Some("kimi-k2.v1")
        );
        assert_eq!(
            node.pointer("/value/calls/0/id").and_then(Node::as_str),
            Some("functions.list_dir:0")
        );
    }
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","emit":"kimi-k2.v1""#),
        )
        .unwrap();
    assert_eq!(
        node.pointer("/value/emitted/output/text")
            .and_then(Node::as_str),
        Some(
            "<|tool_calls_section_begin|><|tool_call_begin|>functions.list_dir:0<|tool_call_argument_begin|>{\"depth\":2,\"path\":\"src\"}<|tool_call_end|><|tool_calls_section_end|>"
        )
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool accepts `anthropic-tool-use.v1` in its allow-list and as an
/// emission target.
#[test]
fn anthropic_tool_use_dialect_over_mcp() {
    let (tools, root) = tools("anthropic");
    let output = r#"{"type":"tool_use","id":"toolu_01","name":"list_dir","input":{"path":"src"}}"#;
    for extra in ["", r#","dialects":["anthropic-tool-use.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
        assert_eq!(
            node.pointer("/value/calls/0/dialect")
                .and_then(Node::as_str),
            Some("anthropic-tool-use.v1")
        );
        assert_eq!(
            node.pointer("/value/calls/0/id").and_then(Node::as_str),
            Some("toolu_01")
        );
    }
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","emit":"anthropic-tool-use.v1""#),
        )
        .unwrap();
    let id = node
        .pointer("/value/calls/0/id")
        .and_then(Node::as_str)
        .expect("id")
        .to_owned();
    assert_eq!(
        node.pointer("/value/emitted/output/text")
            .and_then(Node::as_str),
        Some(
            format!(r#"{{"type":"tool_use","id":"{id}","name":"list_dir","input":{{"depth":2,"path":"src"}}}}"#)
                .as_str()
        )
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool accepts `harmony.v1` in its allow-list and as an emission
/// target.
#[test]
fn harmony_dialect_over_mcp() {
    let (tools, root) = tools("harmony");
    let output = "<|channel|>commentary to=functions.list_dir <|constrain|>json<|message|>{\"path\": \"src\"}<|call|>";
    for extra in ["", r#","dialects":["harmony.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
        assert_eq!(
            node.pointer("/value/calls/0/dialect")
                .and_then(Node::as_str),
            Some("harmony.v1")
        );
    }
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","emit":"harmony.v1""#),
        )
        .unwrap();
    assert_eq!(
        node.pointer("/value/emitted/output/text")
            .and_then(Node::as_str),
        Some(
            "<|start|>assistant<|channel|>commentary to=functions.list_dir <|constrain|>json<|message|>{\"depth\":2,\"path\":\"src\"}<|call|>"
        )
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool accepts `bare-json.v1` in its allow-list and as an emission
/// target.
#[test]
fn bare_json_dialect_over_mcp() {
    let (tools, root) = tools("bare-json");
    let output = "Listing: {\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}";
    for extra in ["", r#","dialects":["bare-json.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
        assert_eq!(
            node.pointer("/value/calls/0/dialect")
                .and_then(Node::as_str),
            Some("bare-json.v1")
        );
    }
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","emit":"bare-json.v1""#),
        )
        .unwrap();
    assert_eq!(
        node.pointer("/value/emitted/output/text")
            .and_then(Node::as_str),
        Some("{\"name\":\"list_dir\",\"arguments\":{\"depth\":2,\"path\":\"src\"}}")
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool reports call markup no allowed dialect reads as a rejected
/// attempt with reason `unsupported_dialect` (its output schema admits a
/// `null` dialect and the family members).
#[test]
fn unparsed_markup_over_mcp() {
    let (tools, root) = tools("unparsed");
    let output = "```tool_code\nlist_dir(path=\"src\")\n```";
    let node = tools
        .call("recover_tool_calls", &arguments(output, ""))
        .unwrap();
    assert_eq!(
        node.pointer("/value/finish").and_then(Node::as_str),
        Some("unresolved")
    );
    let rejected = node
        .pointer("/value/rejected/0")
        .expect("a rejected attempt");
    assert_eq!(
        rejected.get("dialect").map(Node::to_json).as_deref(),
        Some("null")
    );
    assert_eq!(
        rejected.pointer("/reason/family").and_then(Node::as_str),
        Some("gemma3-tool-code.v1")
    );
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(QWEN, r#","dialects":["hermes.v1"]"#),
        )
        .unwrap();
    assert_eq!(
        node.pointer("/value/rejected/1/reason/family")
            .and_then(Node::as_str),
        Some("qwen3-coder-xml.v1")
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// `tools` may be a string holding the raw declaration JSON: the adapters
/// read those bytes exactly, so the schema digest is that of the caller's
/// parameters text. A structured `tools` value (already parsed by JSON-RPC)
/// is written back compactly, keeping member order and number spellings,
/// so its digest is that of the compact text.
#[test]
fn raw_declaration_string_is_read_byte_exact() {
    let (tools, root) = tools("raw-declarations");
    let parameters = r#"{"properties" : {"path":{"type":"string"},  "depth":{"maximum":1.50e3, "type":"integer"}},"type":"object","required":["path"]}"#;
    let declarations = format!(
        "[ {{  \"function\" : {{ \"parameters\" : {parameters} , \"name\" : \"list_dir\" }}, \"type\":\"function\" }} ]"
    );
    let output = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    let digest = |text: &str| Schema::compile(text).unwrap().digest().to_string();
    let constraint = |node: &Node| {
        node.pointer("/receipt/validations/0/constraint")
            .and_then(Node::as_str)
            .map(str::to_owned)
    };
    let raw = json::parse(&format!(
        r#"{{"output":{},"tools":{}}}"#,
        json::quote(output),
        json::quote(&declarations)
    ))
    .unwrap();
    let node = tools.call("recover_tool_calls", &raw).unwrap();
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(constraint(&node), Some(digest(parameters)));
    let structured = json::parse(&format!(
        r#"{{"output":{},"tools":{declarations}}}"#,
        json::quote(output)
    ))
    .unwrap();
    let node = tools.call("recover_tool_calls", &structured).unwrap();
    let compact = json::parse(parameters).unwrap().to_json();
    assert!(compact.contains("1.50e3") && compact.starts_with(r#"{"properties""#));
    assert_eq!(constraint(&node), Some(digest(&compact)));
    std::fs::remove_dir_all(root).unwrap();
}
