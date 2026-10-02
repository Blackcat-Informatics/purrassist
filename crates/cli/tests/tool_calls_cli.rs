// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Execute the shipped `recover-tool-calls` command on raw model output.
use purrassist::digest::Digest;
use purrassist::json::{self, Node, Value};
use purrassist::tool_call::turn_id_prefix;
use std::fmt::Write as _;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

const TOOLS: &str = r#"[{"type":"function","function":{"name":"list_dir","parameters":{"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}},"required":["path"]}}}]"#;

fn tools_file() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "purrassist-tool-calls-cli-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, TOOLS).unwrap();
    path
}

fn run(arguments: &[&str], stdin: &[u8]) -> (Output, PathBuf) {
    let tools = tools_file();
    let mut child = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .arg("recover-tool-calls")
        .arg("--tools")
        .arg(&tools)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    (child.wait_with_output().unwrap(), tools)
}

fn result(output: &Output) -> Node {
    json::parse(&String::from_utf8(output.stdout.clone()).unwrap()).unwrap()
}

#[test]
fn qwen_xml_recovers_via_cli() {
    let turn = b"<tool_call>\n<function=list_dir>\n<parameter=depth>\n2\n<parameter=path>\nsrc\n</parameter>\n</function>\n</tool_call>";
    let (output, tools) = run(&["--emit", "openai"], turn);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/arguments").unwrap().to_json(),
        r#"{"depth":2,"path":"src"}"#
    );
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("qwen3-coder-xml.v1")
    );
    let emitted = node.pointer("/value/emitted/output").unwrap();
    let Value::Array(calls) = &emitted.value else {
        panic!("emitted output must be the tool_calls array: {emitted:?}")
    };
    assert_eq!(calls.len(), 1);
    assert!(calls[0].get("role").is_none());
    // The default id: `call_`, the first 12 hex digits of the turn's
    // SHA-256, and the call index.
    assert_eq!(
        emitted.to_json(),
        r#"[{"id":"call_ba5fc71a5a13_0","type":"function","function":{"name":"list_dir","arguments":"{\"depth\":2,\"path\":\"src\"}"}}]"#
    );
    assert_eq!(
        format!("{}0", turn_id_prefix(&Digest::of(turn))),
        "call_ba5fc71a5a13_0"
    );

    // The same output read from a FILE argument gives the same result.
    let turn_path = tools.with_extension("txt");
    fs::write(&turn_path, turn).unwrap();
    let (from_file, other_tools) = run(&["--emit", "openai", turn_path.to_str().unwrap()], b"");
    assert_eq!(from_file.stdout, output.stdout);
    for path in [tools, other_tools, turn_path] {
        fs::remove_file(path).unwrap();
    }
}

/// G5: with zero configuration, two different turns of a conversation get
/// disjoint call ids, the same turn always gets the same ids, and the
/// explicit numbering options are unchanged.
#[test]
fn default_ids_are_disjoint_across_turns() {
    let call = |path: &str| {
        format!(
            "<tool_call>\n<function=list_dir>\n<parameter=path>\n{path}\n</parameter>\n</function>\n</tool_call>\n"
        )
    };
    let ids = |arguments: &[&str], turn: &str| {
        let (output, tools) = run(arguments, turn.as_bytes());
        fs::remove_file(tools).unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let node = result(&output);
        let Some(Value::Array(calls)) = node.pointer("/value/calls").map(|calls| &calls.value)
        else {
            panic!("no calls: {node:?}")
        };
        calls
            .iter()
            .map(|call| call.get("id").and_then(Node::as_str).unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let first = format!("{}{}", call("src"), call("docs"));
    let second = format!("{}{}", call("tests"), call("benches"));
    let round_one = ids(&[], &first);
    let round_two = ids(&[], &second);
    let tag = |turn: &str| turn_id_prefix(&Digest::of(turn.as_bytes()));
    assert_eq!(
        round_one,
        [format!("{}0", tag(&first)), format!("{}1", tag(&first))]
    );
    assert_eq!(
        round_two,
        [format!("{}0", tag(&second)), format!("{}1", tag(&second))]
    );
    assert!(round_one.iter().all(|id| !round_two.contains(id)));
    assert_eq!(ids(&[], &first), round_one);
    assert_eq!(
        ids(&["--ids-prefix", "call_"], &first),
        ["call_0", "call_1"]
    );
    assert_eq!(ids(&["--ids-start", "2"], &second), ["call_2", "call_3"]);
}

/// G7: the CLI's default request sizes its candidate budget to the turn: a
/// 64-call turn whose every call needs a markup repair, with a contested
/// stray `<tool_call>` line after each, recovers every call `Resolved`.
#[test]
fn busy_turn_resolves_every_call_by_default() {
    let mut turn = String::new();
    for index in 0..64 {
        let _ = write!(
            turn,
            "<tool_call>\n<function=list_dir>\n<parameter=depth>\n{index}\n<parameter=path>\nsrc/{index}\n</parameter>\n</function>\n</tool_call>\n<tool_call>\n"
        );
    }
    let (output, tools) = run(&[], turn.as_bytes());
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = result(&output);
    assert_eq!(
        node.get("status").and_then(Node::as_str),
        Some("best_guess")
    );
    for index in 0..64 {
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/outcome/status"))
                .and_then(Node::as_str),
            Some("resolved"),
            "call {index}"
        );
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/arguments"))
                .map(Node::to_json),
            Some(format!(r#"{{"depth":{index},"path":"src/{index}"}}"#))
        );
    }
    assert!(node.pointer("/value/calls/64").is_none());
    fs::remove_file(tools).unwrap();
}

#[test]
fn invalid_utf8_input_recorded() {
    let turn = b"note \xff\xfe <tool_call>{\"name\": \"list_dir\", \"arguments\": {\"path\": \"a\"}}</tool_call>";
    let (output, tools) = run(&[], turn);
    let node = result(&output);
    // A data loss in the input's encoding makes the selected reading a best
    // guess, which exits 0.
    assert_eq!(
        node.get("status").and_then(Node::as_str),
        Some("best_guess")
    );
    assert_eq!(output.status.code(), Some(0));
    let digits = b"0123456789abcdef";
    let hex: String = turn
        .iter()
        .flat_map(|byte| {
            [
                char::from(digits[usize::from(byte >> 4)]),
                char::from(digits[usize::from(byte & 0x0f)]),
            ]
        })
        .collect();
    assert_eq!(
        node.pointer("/receipt/sources/0/bytes_hex")
            .and_then(Node::as_str),
        Some(hex.as_str())
    );
    let Value::Array(losses) = &node.pointer("/receipt/losses").unwrap().value else {
        panic!("losses")
    };
    assert!(losses.iter().any(|loss| {
        loss.pointer("/evidence/rule").and_then(Node::as_str)
            == Some("tool_call.input.invalid-utf8")
    }));
    assert_eq!(
        node.pointer("/value/calls/0/tool").and_then(Node::as_str),
        Some("list_dir")
    );
    fs::remove_file(tools).unwrap();
}

#[test]
fn repeated_option_rejected() {
    for repeated in [
        ["--policy", "tolerant", "--policy", "structural"],
        ["--stop", "length", "--stop", "stop"],
        ["--emit", "openai", "--emit", "hermes.v1"],
        ["--effort", "quick", "--effort", "deep"],
        ["--ids-prefix", "a", "--ids-prefix", "b"],
        [
            "--effects",
            "list_dir=read-only",
            "--effects",
            "list_dir=mutating",
        ],
    ] {
        let (output, tools) = run(&repeated, b"");
        assert_eq!(output.status.code(), Some(1), "{repeated:?}");
        assert!(output.stdout.is_empty(), "{repeated:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("repeated") || stderr.contains("twice"),
            "{repeated:?}: {stderr}"
        );
        fs::remove_file(tools).unwrap();
    }
    let tools = tools_file();
    let output = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .arg("recover-tool-calls")
        .arg("--tools")
        .arg(&tools)
        .arg("--tools")
        .arg(&tools)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    fs::remove_file(tools).unwrap();
}

#[test]
fn stop_sequence_option() {
    let turn = br#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}"#;
    let (output, tools) = run(&["--stop", "stop-sequence=</tool_call>"], turn);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = result(&output);
    assert_eq!(
        node.pointer("/value/calls/0/arguments").unwrap().to_json(),
        r#"{"path":"src"}"#
    );
    let Value::Array(evidence) = &node.pointer("/receipt/evidence").unwrap().value else {
        panic!("evidence")
    };
    assert!(evidence.iter().any(|entry| {
        entry.get("rule").and_then(Node::as_str) == Some("tool_call.stop.consumed-closer")
    }));
    // Without the stop sequence the same unclosed call is not cleanly closed.
    let (plain, plain_tools) = run(&[], turn);
    assert_ne!(plain.stdout, output.stdout);
    fs::remove_file(tools).unwrap();
    fs::remove_file(plain_tools).unwrap();
}

#[test]
fn ids_source_option() {
    let turn = br#"{"id":"call_abc","type":"function","function":{"name":"list_dir","arguments":"{\"path\":\"src\"}"}}"#;
    let (output, tools) = run(
        &[
            "--dialect",
            "openai-tool-calls.v1",
            "--ids-source",
            "--emit",
            "openai",
        ],
        turn,
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = result(&output);
    assert_eq!(
        node.pointer("/value/calls/0/source_id")
            .and_then(Node::as_str),
        Some("call_abc")
    );
    assert_eq!(
        node.pointer("/value/calls/0/id").and_then(Node::as_str),
        Some("call_abc")
    );
    assert_eq!(
        node.pointer("/value/emitted/output/0/id")
            .and_then(Node::as_str),
        Some("call_abc")
    );
    fs::remove_file(tools).unwrap();

    for conflicting in [
        ["--ids-source", "--ids-prefix", "t_"],
        ["--ids-start", "3", "--ids-source"],
    ] {
        let (output, tools) = run(&conflicting, turn);
        assert_eq!(output.status.code(), Some(1), "{conflicting:?}");
        assert!(output.stdout.is_empty(), "{conflicting:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--ids-source"),
            "{conflicting:?}"
        );
        fs::remove_file(tools).unwrap();
    }
    let (repeated, tools) = run(&["--ids-source", "--ids-source"], turn);
    assert_eq!(repeated.status.code(), Some(1));
    fs::remove_file(tools).unwrap();
}

/// Llama 3 JSON with no `--dialect`: the template's call object, with and
/// without `<|python_tag|>`, is `Resolved` under the default allow-list, and
/// a Hermes call emitted as `llama3-json.v1` recovers to the same call.
#[test]
fn llama3_json_recovers_and_emits_via_cli() {
    for turn in [
        &br#"{"name": "list_dir", "parameters": {"path": "src", "depth": 2}}"#[..],
        b"<|python_tag|>{\n    \"type\": \"function\",\n    \"name\": \"list_dir\",\n    \"parameters\": {\"path\": \"src\", \"depth\": 2}\n}<|eom_id|>",
    ] {
        let (output, tools) = run(&[], turn);
        assert_eq!(output.status.code(), Some(0));
        let node = result(&output);
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
        fs::remove_file(tools).unwrap();
    }
    let hermes = b"<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\", \"depth\": 2}}\n</tool_call>";
    let (output, tools) = run(&["--emit", "llama3-json.v1"], hermes);
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted markup")
        .to_owned();
    assert_eq!(
        emitted,
        r#"{"name": "list_dir", "parameters": {"path":"src","depth":2}}"#
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], emitted.as_bytes());
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/arguments").unwrap().to_json(),
        r#"{"path":"src","depth":2}"#
    );
    fs::remove_file(tools).unwrap();
}

/// Mistral v3 with no `--dialect`: a `[TOOL_CALLS]` array is one call per
/// element, each keeping the model's id, and two Hermes calls emitted as
/// `mistral-v3.v1` print as one array that recovers to the same calls.
#[test]
fn mistral_v3_recovers_and_emits_via_cli() {
    let turn = br#"[TOOL_CALLS] [{"name": "list_dir", "arguments": {"path": "src"}, "id": "a1B2c3D4e"}, {"name": "list_dir", "arguments": {"path": "docs", "depth": 1}, "id": "f5G6h7J8k"}]</s>"#;
    let (output, tools) = run(&[], turn);
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    for (index, (id, arguments)) in [
        ("a1B2c3D4e", r#"{"path":"src"}"#),
        ("f5G6h7J8k", r#"{"path":"docs","depth":1}"#),
    ]
    .into_iter()
    .enumerate()
    {
        let call = |field: &str| node.pointer(&format!("/value/calls/{index}/{field}"));
        assert_eq!(
            call("dialect").and_then(Node::as_str),
            Some("mistral-v3.v1")
        );
        assert_eq!(call("id").and_then(Node::as_str), Some(id));
        assert_eq!(call("source_id").and_then(Node::as_str), Some(id));
        assert_eq!(call("arguments").unwrap().to_json(), arguments);
    }
    fs::remove_file(tools).unwrap();
    let hermes = b"<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}\n</tool_call>\n<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"docs\"}}\n</tool_call>";
    let (output, tools) = run(&["--emit", "mistral-v3.v1"], hermes);
    let node = result(&output);
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted markup")
        .to_owned();
    let prefix = turn_id_prefix(&Digest::of(hermes));
    assert_eq!(
        emitted,
        format!(
            r#"[TOOL_CALLS] [{{"name": "list_dir", "arguments": {{"path":"src"}}, "id": "{prefix}0"}}, {{"name": "list_dir", "arguments": {{"path":"docs"}}, "id": "{prefix}1"}}]"#
        )
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], emitted.as_bytes());
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/1/source_id")
            .and_then(Node::as_str),
        Some(format!("{prefix}1").as_str())
    );
    fs::remove_file(tools).unwrap();
}
