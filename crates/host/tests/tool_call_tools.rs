// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The `recover_tool_calls` tool runs exactly core tool-call recovery, keeps
//! exhaustion an explicit unresolved outcome, and rejects malformed requests.
use purrassist::{
    Effort, Governor, Schema,
    digest::Digest,
    json::{self, Node},
    tool_call::{
        DialectId, EmitTarget, GenerationStop, IdPolicy, ToolChoice, ToolRecoveryRequest, ToolSet,
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
    let expected = set.recover_governed(
        QWEN.as_bytes(),
        &request,
        purrassist::tool_call::DialectSet::built_in(),
        &mut governor,
    );

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
/// `<tool_call>` line after each that three dialects contest, recovers every
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
            .map_or_else(String::new, Node::to_json)
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
        Some((128 + 16 * 448).to_string())
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

#[test]
fn tool_choice_strict_member() {
    let (tools, root) = tools("strict");
    let call = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    let forced = r#"{"type":"function","function":{"name":"list_dir"}}"#;
    let anthropic = r#"{"type":"tool","name":"list_dir","disable_parallel_tool_use":true}"#;
    let run = |output: &str, choice: &str, strict: &str| {
        let args = arguments(
            output,
            &format!(r#","tool_choice":{choice},"tool_choice_strict":{strict}"#),
        );
        tools.call("recover_tool_calls", &args)
    };
    let constraint = |result: &Node| {
        result
            .pointer("/value/tool_choice/constraint")
            .and_then(Node::as_str)
            .map(str::to_owned)
    };
    let passed = |result: &Node| {
        matches!(
            result
                .pointer("/value/tool_choice/passed")
                .map(|node| &node.value),
            Some(json::Value::Bool(true))
        )
    };
    let all = r#"{"exactly_one":true,"allow_content":false,"allow_reasoning":false}"#;
    let result = run(call, forced, all).unwrap();
    assert_eq!(
        constraint(&result).as_deref(),
        Some("tool_choice=forced[exactly-one,no-content,no-reasoning]:list_dir")
    );
    assert!(passed(&result));
    // The same options as core, over the wire: prose fails no-content.
    let prose = format!("Sure.\n{call}");
    let result = run(&prose, forced, all).unwrap();
    assert!(!passed(&result));
    assert_eq!(
        result.pointer("/value/verdict").and_then(Node::as_str),
        Some("nothing_usable")
    );
    // Lenient options render the plain forced constraint.
    let lenient = r#"{"exactly_one":false,"allow_content":true,"allow_reasoning":true}"#;
    let result = run(&prose, forced, lenient).unwrap();
    assert_eq!(
        constraint(&result).as_deref(),
        Some("tool_choice=forced:list_dir")
    );
    assert!(passed(&result));
    // Beside the Anthropic exactly-one flag.
    let result = run(
        &prose,
        anthropic,
        r#"{"exactly_one":true,"allow_content":false,"allow_reasoning":true}"#,
    )
    .unwrap();
    assert_eq!(
        constraint(&result).as_deref(),
        Some("tool_choice=forced[exactly-one,no-content]:list_dir")
    );
    assert!(!passed(&result));
    // The Anthropic flag alone is strict exactly-one with content allowed.
    let result = tools
        .call(
            "recover_tool_calls",
            &arguments(&prose, &format!(r#","tool_choice":{anthropic}"#)),
        )
        .unwrap();
    assert_eq!(
        constraint(&result).as_deref(),
        Some("tool_choice=forced[exactly-one]:list_dir")
    );
    assert!(passed(&result));
    std::fs::remove_dir_all(root).unwrap();
}

/// A second call attempt that is rejected fails the strict forced choice
/// over MCP (R4, G3): the first call is valid on its own, yet the verdict is
/// `nothing_usable`, both usable counts are 0 (A3) and nothing is emitted.
#[test]
fn strict_choice_counts_a_rejected_second_attempt() {
    let (tools, root) = tools("strict-rejected");
    let call = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    let attempt = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "lib"}} junk junk {{{</tool_call>"#;
    let output = format!("{call}\n{attempt}");
    let forced = r#"{"type":"function","function":{"name":"list_dir"}}"#;
    let anthropic = r#"{"type":"tool","name":"list_dir","disable_parallel_tool_use":true}"#;
    let all = r#"{"exactly_one":true,"allow_content":false,"allow_reasoning":false}"#;
    for (choice, strict) in [(forced, Some(all)), (anthropic, None)] {
        let extra = format!(
            r#","dialects":["hermes.v1"],"emit":"openai","tool_choice":{choice}{}"#,
            strict.map_or_else(String::new, |strict| format!(
                r#","tool_choice_strict":{strict}"#
            ))
        );
        let result = tools
            .call("recover_tool_calls", &arguments(&output, &extra))
            .unwrap();
        let text = |pointer: &str| result.pointer(pointer).map(json::Node::to_json);
        assert_eq!(
            text("/value/tool_choice/passed").as_deref(),
            Some("false"),
            "{choice}"
        );
        assert_eq!(
            text("/value/verdict").as_deref(),
            Some("\"nothing_usable\"")
        );
        assert_eq!(text("/value/usable_calls").as_deref(), Some("0"));
        assert_eq!(
            text("/value/usable_calls_accepting_guesses").as_deref(),
            Some("0")
        );
        assert_eq!(text("/value/emitted/output").as_deref(), Some("[]"));
        let diagnostics = text("/value/tool_choice/diagnostics").unwrap();
        assert!(
            diagnostics.contains("1 rejected call attempt beyond"),
            "{diagnostics}"
        );
    }
    // Without the attempt, the same options pass and one call is usable.
    let extra =
        format!(r#","dialects":["hermes.v1"],"tool_choice":{forced},"tool_choice_strict":{all}"#);
    let result = tools
        .call("recover_tool_calls", &arguments(call, &extra))
        .unwrap();
    assert_eq!(
        result
            .pointer("/value/usable_calls")
            .map(json::Node::to_json)
            .as_deref(),
        Some("1")
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// A failed tool choice leaves no usable call over MCP whatever the choice
/// (A3), though each call's own outcome is still `resolved`.
#[test]
fn failed_tool_choice_zeroes_the_usable_counts() {
    let (tools, root) = tools("usable-zero");
    let call = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    let output = format!("{call}\n{call}");
    let run = |choice: &str, strict: Option<&str>| {
        let extra = format!(
            r#","tool_choice":{choice}{}"#,
            strict.map_or_else(String::new, |strict| format!(
                r#","tool_choice_strict":{strict}"#
            ))
        );
        tools
            .call("recover_tool_calls", &arguments(&output, &extra))
            .unwrap()
    };
    let counts = |result: &Node| {
        (
            result
                .pointer("/value/tool_choice/passed")
                .map(json::Node::to_json),
            result
                .pointer("/value/usable_calls")
                .map(json::Node::to_json),
            result
                .pointer("/value/usable_calls_accepting_guesses")
                .map(json::Node::to_json),
        )
    };
    let owned = |passed: &str, strict: &str, guesses: &str| {
        (
            Some(passed.to_owned()),
            Some(strict.to_owned()),
            Some(guesses.to_owned()),
        )
    };
    // Passing: both calls usable. Failing cardinality: none.
    assert_eq!(
        counts(&run(
            r#"{"type":"function","function":{"name":"list_dir"}}"#,
            None
        )),
        owned("true", "2", "2")
    );
    assert_eq!(
        counts(&run(
            r#"{"type":"function","function":{"name":"list_dir"}}"#,
            Some(r#"{"exactly_one":true,"allow_content":true,"allow_reasoning":true}"#)
        )),
        owned("false", "0", "0")
    );
    assert_eq!(counts(&run(r#""none""#, None)), owned("false", "0", "0"));
    std::fs::remove_dir_all(root).unwrap();
}

/// A strict choice forbidding reasoning cannot be enforced when reasoning is
/// content (R4, G15): the tool result is `unresolved` with an
/// `invalid_input` termination, never a passed check.
#[test]
fn strict_no_reasoning_beside_content_reasoning_is_invalid_input() {
    let (tools, root) = tools("strict-reasoning");
    let call = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    let output = format!("<think>plan</think>\n{call}");
    let forced = r#"{"type":"function","function":{"name":"list_dir"}}"#;
    let strict = r#"{"exactly_one":true,"allow_content":false,"allow_reasoning":false}"#;
    let extra =
        format!(r#","reasoning":"content","tool_choice":{forced},"tool_choice_strict":{strict}"#);
    let result = tools
        .call("recover_tool_calls", &arguments(&output, &extra))
        .unwrap();
    assert_eq!(
        result.pointer("/status").and_then(Node::as_str),
        Some("unresolved")
    );
    assert_eq!(
        result
            .pointer("/receipt/termination/kind")
            .and_then(Node::as_str),
        Some("invalid_input")
    );
    let detail = result
        .pointer("/receipt/termination/detail")
        .and_then(Node::as_str)
        .unwrap();
    assert!(detail.contains("allow_reasoning"), "{detail}");
    assert!(
        result
            .pointer("/value")
            .is_some_and(|value| matches!(value.value, json::Value::Null))
    );
    // Reasoning allowed, or separate reasoning, is a normal result.
    let allowed = r#"{"exactly_one":true,"allow_content":true,"allow_reasoning":true}"#;
    let extra =
        format!(r#","reasoning":"content","tool_choice":{forced},"tool_choice_strict":{allowed}"#);
    let result = tools
        .call("recover_tool_calls", &arguments(&output, &extra))
        .unwrap();
    assert_eq!(
        result.pointer("/status").and_then(Node::as_str),
        Some("resolved")
    );
    let extra = format!(r#","tool_choice":{forced},"tool_choice_strict":{strict}"#);
    let result = tools
        .call("recover_tool_calls", &arguments(&output, &extra))
        .unwrap();
    assert_eq!(
        result.pointer("/status").and_then(Node::as_str),
        Some("resolved")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn tool_choice_strict_member_errors() {
    let (tools, root) = tools("strict-errors");
    let call = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    let forced = r#"{"type":"function","function":{"name":"list_dir"}}"#;
    let anthropic = r#"{"type":"tool","name":"list_dir","disable_parallel_tool_use":true}"#;
    let all = r#"{"exactly_one":true,"allow_content":false,"allow_reasoning":false}"#;
    let run = |choice: &str, strict: &str| {
        let args = arguments(
            call,
            &format!(r#","tool_choice":{choice},"tool_choice_strict":{strict}"#),
        );
        tools.call("recover_tool_calls", &args)
    };
    // Errors: not forced, contradicting the flag, or malformed.
    for (choice, strict, message) in [
        (r#""auto""#, all, "requires a forced tool_choice"),
        (r#""required""#, all, "requires a forced tool_choice"),
        (r#"{"type":"any"}"#, all, "requires a forced tool_choice"),
        (
            anthropic,
            r#"{"exactly_one":false,"allow_content":true,"allow_reasoning":true}"#,
            "disable_parallel_tool_use",
        ),
        (forced, "true", "/tool_choice_strict"),
        (
            forced,
            r#"{"exactly_one":true,"allow_content":false}"#,
            "allow_reasoning",
        ),
        (
            forced,
            r#"{"exactly_one":1,"allow_content":false,"allow_reasoning":true}"#,
            "/tool_choice_strict/exactly_one",
        ),
        (
            forced,
            r#"{"exactly_one":true,"allow_content":false,"allow_reasoning":true,"x":true}"#,
            "tool_choice_strict",
        ),
    ] {
        let error = run(choice, strict).unwrap_err();
        assert!(error.contains(message), "{choice} {strict}: {error}");
    }
    // Without a tool_choice at all it is not forced either.
    let args = arguments(call, &format!(r#","tool_choice_strict":{all}"#));
    let error = tools.call("recover_tool_calls", &args).unwrap_err();
    assert!(error.contains("requires a forced tool_choice"), "{error}");
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
        Some(r#"["glm-4.5.v1","hermes.v1","qwen3-coder-xml.v1"]"#)
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

/// The result carries `reasoning_blocks` (one `{span, text}` per block,
/// never merged, `text` without the markers) next to the merged `reasoning`
/// ranges, and validates against the published output schema, which requires
/// the member.
#[test]
fn reasoning_blocks_are_published_and_validate() {
    let (tools, root) = tools("reasoning-blocks");
    let output = format!("<think>a</think><think>b</think>\n{QWEN}");
    let actual = tools
        .call("recover_tool_calls", &arguments(&output, ""))
        .unwrap();
    assert_eq!(
        actual
            .pointer("/value/reasoning")
            .map(Node::to_json)
            .as_deref(),
        Some(r#"[{"start":0,"end":32}]"#),
        "{}",
        actual.to_json()
    );
    let blocks = actual.pointer("/value/reasoning_blocks").unwrap();
    assert_eq!(
        blocks.to_json(),
        r#"[{"span":{"start":0,"end":16},"text":{"start":7,"end":8}},{"span":{"start":16,"end":32},"text":{"start":23,"end":24}}]"#
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
    // The member is required, and a block holds exactly a span and a text.
    let member = format!(r#""reasoning_blocks":{},"#, blocks.to_json());
    let json = actual.to_json();
    assert!(json.contains(&member));
    let without = json::parse(&json.replace(&member, "")).unwrap();
    assert!(!schema.validate(&without).unwrap().valid);
    let extra = json::parse(&json.replace(
        r#"{"span":{"start":0,"end":16},"text""#,
        r#"{"extra":1,"span":{"start":0,"end":16},"text""#,
    ))
    .unwrap();
    assert!(!schema.validate(&extra).unwrap().valid);
    // No reasoning, no blocks.
    let plain = tools
        .call("recover_tool_calls", &arguments(QWEN, ""))
        .unwrap();
    assert_eq!(
        plain
            .pointer("/value/reasoning_blocks")
            .map(Node::to_json)
            .as_deref(),
        Some("[]")
    );
    assert!(schema.validate(&plain).unwrap().valid);
    std::fs::remove_dir_all(root).unwrap();
}

/// F5: `status` is the certainty of the reading of the turn and
/// `value.verdict` is the usability verdict. A turn read with certainty
/// whose only call names an undeclared tool, misses a required argument, or
/// violates the tool choice is `status: "resolved"`, `finish: "unresolved"`
/// and `verdict: "nothing_usable"` with no usable call, a successful tool
/// result (`isError` false); a usable call is `verdict: "run"`; every
/// result validates against the published output schema.
#[test]
fn status_is_the_reading_and_verdict_is_usability() {
    let (tools, root) = tools("verdict");
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
    let call = |name: &str, arguments: &str| {
        format!(r#"<tool_call>{{"name": "{name}", "arguments": {arguments}}}</tool_call>"#)
    };
    for (output, extra, verdict, usable) in [
        (
            call("rm_rf", r#"{"path": "src"}"#),
            "",
            "nothing_usable",
            "0",
        ),
        (call("list_dir", "{}"), "", "nothing_usable", "0"),
        (
            call("list_dir", r#"{"path": "src"}"#),
            r#","tool_choice":"none""#,
            "nothing_usable",
            "0",
        ),
        (call("list_dir", r#"{"path": "src"}"#), "", "run", "1"),
        ("Just text.".to_owned(), "", "no_calls", "0"),
    ] {
        let result = tools
            .call("recover_tool_calls", &arguments(&output, extra))
            .unwrap();
        assert_eq!(
            result.get("status").and_then(Node::as_str),
            Some("resolved"),
            "{output}"
        );
        assert_eq!(
            result.pointer("/value/verdict").and_then(Node::as_str),
            Some(verdict),
            "{output}{extra}"
        );
        assert_eq!(
            result
                .pointer("/value/usable_calls")
                .map(Node::to_json)
                .as_deref(),
            Some(usable),
            "{output}{extra}"
        );
        if verdict == "nothing_usable" {
            assert_eq!(
                result.pointer("/value/finish").and_then(Node::as_str),
                Some("unresolved"),
                "{output}{extra}"
            );
        }
        assert!(schema.validate(&result).unwrap().valid, "{output}{extra}");
    }
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

/// Every core built-in is admitted by both host dialect selector fields.
#[test]
fn every_builtin_dialect_selector_is_admitted_over_mcp() {
    let (tools, root) = tools("dialect-selectors");
    for dialect in DialectId::BUILT_IN {
        let id = json::quote(dialect.id());
        let extra = format!(r#","dialects":[{id}],"emit":{id}"#);
        let result = tools.call("recover_tool_calls", &arguments(QWEN, &extra));
        assert!(result.is_ok(), "{}: {result:?}", dialect.id());
    }
    for extra in [
        r#","dialects":["unknown-dialect.v1"]"#,
        r#","emit":"unknown-dialect.v1""#,
    ] {
        assert!(
            tools
                .call("recover_tool_calls", &arguments(QWEN, extra))
                .is_err()
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

/// The MCP tool recovers Gemma through default and explicit allow-lists, and
/// reports omitted dialects without losing their markup.
#[test]
fn gemma_and_unparsed_markup_over_mcp() {
    let (tools, root) = tools("gemma-unparsed");
    let output = "```tool_code\nlist_dir(path=\"src\")\n```";
    for extra in ["", r#","dialects":["gemma3-tool-code.v1"]"#] {
        let node = tools
            .call("recover_tool_calls", &arguments(output, extra))
            .unwrap();
        assert_eq!(
            node.pointer("/value/finish").and_then(Node::as_str),
            Some("calls")
        );
        assert_eq!(
            node.pointer("/value/calls/0/dialect")
                .and_then(Node::as_str),
            Some("gemma3-tool-code.v1")
        );
        assert_eq!(
            node.pointer("/value/calls/0/arguments/path")
                .and_then(Node::as_str),
            Some("src")
        );
    }
    let output = r#"functions.list_dir:0{"path":"src"}"#;
    let node = tools
        .call(
            "recover_tool_calls",
            &arguments(output, r#","dialects":["hermes.v1"]"#),
        )
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
        Some("kimi-k2-stripped.v1")
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
