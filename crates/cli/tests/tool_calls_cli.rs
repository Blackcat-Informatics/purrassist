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

fn tools_file(declarations: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "purrassist-tool-calls-cli-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, declarations).unwrap();
    path
}

fn run(arguments: &[&str], stdin: &[u8]) -> (Output, PathBuf) {
    run_with(TOOLS, arguments, stdin)
}

fn run_with(declarations: &str, arguments: &[&str], stdin: &[u8]) -> (Output, PathBuf) {
    let tools = tools_file(declarations);
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
    // A child that exits before reading its input closes the pipe; its
    // status and stderr (asserted by the caller) say why, so a broken pipe
    // here is not itself the failure.
    if let Err(error) = child.stdin.take().unwrap().write_all(stdin) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
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
    // Hermes also opens at `<tool_call>` but does not read a `<function=`
    // body: the call is Qwen3-Coder XML's alone.
    assert_eq!(
        node.pointer("/value/calls/0/candidate_dialects")
            .map(Node::to_json)
            .as_deref(),
        Some(r#"["qwen3-coder-xml.v1"]"#)
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
/// The stray openers are rejected attempts (the finish is `unresolved`),
/// but every call is usable, so the exit code is 0.
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
    assert_eq!(
        node.pointer("/value/finish").and_then(Node::as_str),
        Some("unresolved")
    );
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

/// F4: invalid UTF-8 inside an argument keeps the call. Read-only and
/// `Tolerant`, the call is a usable best guess (exit 0) whose argument
/// holds U+FFFD, with the `tool_call.value.invalid-utf8` data loss itemized
/// at the byte's exact offset and the byte retained; a `Mutating` tool's
/// call is kept but never usable (exit 4).
#[test]
fn invalid_utf8_in_an_argument_keeps_the_call() {
    let read = r#"[{"type":"function","function":{"name":"read_file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}]"#;
    let turn = b"<tool_call>{\"name\": \"read_file\", \"arguments\": {\"path\":\"caf\xe9.txt\"}}</tool_call>";
    let at = turn.iter().position(|&byte| byte == 0xE9).unwrap();
    let (output, tools) = run_with(
        read,
        &["--policy", "tolerant", "--effects", "read_file=read-only"],
        turn,
    );
    let node = result(&output);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        node.get("status").and_then(Node::as_str),
        Some("best_guess")
    );
    let call = node.pointer("/value/calls/0").expect("the call");
    assert_eq!(
        call.pointer("/arguments/path").and_then(Node::as_str),
        Some("caf\u{FFFD}.txt")
    );
    assert_eq!(
        call.pointer("/outcome/status").and_then(Node::as_str),
        Some("best_guess")
    );
    let Value::Array(losses) = &call.pointer("/repairs/losses").unwrap().value else {
        panic!("losses")
    };
    let loss = losses
        .iter()
        .find(|loss| {
            loss.pointer("/evidence/rule").and_then(Node::as_str)
                == Some("tool_call.value.invalid-utf8")
        })
        .expect("the invalid-utf8 loss");
    assert_eq!(loss.get("kind").and_then(Node::as_str), Some("data"));
    let start = loss.pointer("/evidence/span/start").map(Node::to_json);
    assert_eq!(start, Some(at.to_string()));
    fs::remove_file(tools).unwrap();

    let write = r#"[{"type":"function","function":{"name":"write_file","parameters":{"type":"object","properties":{"content":{"type":"string"}},"required":["content"]}}}]"#;
    let turn = b"<tool_call>{\"name\": \"write_file\", \"arguments\": {\"content\":\"caf\xe9\"}}</tool_call>";
    let (output, tools) = run_with(
        write,
        &["--policy", "tolerant", "--effects", "write_file=mutating"],
        turn,
    );
    let node = result(&output);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(
        node.pointer("/value/calls/0/arguments/content")
            .and_then(Node::as_str),
        Some("caf\u{FFFD}")
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
    let tools = tools_file(TOOLS);
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

/// Mistral v11 with no `--dialect`: `[TOOL_CALLS]NAME[CALL_ID]ID[ARGS]{...}`
/// calls keep the model's ids, and a Hermes call emitted as
/// `mistral-v11.v1` recovers to the same call.
#[test]
fn mistral_v11_recovers_and_emits_via_cli() {
    let turn = br#"[TOOL_CALLS]list_dir[CALL_ID]a1B2c3D4e[ARGS]{"path": "src"}[TOOL_CALLS]list_dir[ARGS]{"path": "docs", "depth": 1}</s>"#;
    let (output, tools) = run(&[], turn);
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/source_id")
            .and_then(Node::as_str),
        Some("a1B2c3D4e")
    );
    for index in 0..2 {
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/dialect"))
                .and_then(Node::as_str),
            Some("mistral-v11.v1")
        );
    }
    fs::remove_file(tools).unwrap();
    let hermes =
        b"<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}\n</tool_call>";
    let (output, tools) = run(&["--emit", "mistral-v11.v1"], hermes);
    let node = result(&output);
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted markup")
        .to_owned();
    let prefix = turn_id_prefix(&Digest::of(hermes));
    assert_eq!(
        emitted,
        format!(r#"[TOOL_CALLS]list_dir[CALL_ID]{prefix}0[ARGS]{{"path":"src"}}"#)
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], emitted.as_bytes());
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("mistral-v11.v1")
    );
    fs::remove_file(tools).unwrap();
}

/// DeepSeek-V3.1 with no `--dialect`: a tool-calls section is one call per
/// call, and a Hermes call emitted as `deepseek-v3.1.v1` recovers to the
/// same call.
#[test]
fn deepseek_v31_recovers_and_emits_via_cli() {
    let turn = "<think>Two listings.</think><｜tool▁calls▁begin｜><｜tool▁call▁begin｜>list_dir<｜tool▁sep｜>{\"path\": \"src\"}<｜tool▁call▁end｜><｜tool▁call▁begin｜>list_dir<｜tool▁sep｜>{\"path\": \"docs\", \"depth\": 1}<｜tool▁call▁end｜><｜tool▁calls▁end｜>";
    let (output, tools) = run(&[], turn.as_bytes());
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    for (index, arguments) in [r#"{"path":"src"}"#, r#"{"path":"docs","depth":1}"#]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/dialect"))
                .and_then(Node::as_str),
            Some("deepseek-v3.1.v1")
        );
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/arguments"))
                .unwrap()
                .to_json(),
            arguments
        );
    }
    fs::remove_file(tools).unwrap();
    let hermes =
        b"<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}\n</tool_call>";
    let (output, tools) = run(&["--emit", "deepseek-v3.1.v1"], hermes);
    let node = result(&output);
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted markup")
        .to_owned();
    assert_eq!(
        emitted,
        "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>list_dir<｜tool▁sep｜>{\"path\":\"src\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>"
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], emitted.as_bytes());
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("deepseek-v3.1.v1")
    );
    fs::remove_file(tools).unwrap();
}

/// DeepSeek-V3 / R1 with no `--dialect`: a tool-calls section with fenced
/// JSON arguments after R1 reasoning is one call per call, the V3.1 form in
/// the same turn stays V3.1, and a Hermes call emitted as `deepseek-v3.v1`
/// recovers to the same call.
#[test]
fn deepseek_v3_recovers_and_emits_via_cli() {
    let turn = "<think>Two listings.</think>\n\n<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>list_dir\n```json\n{\"path\": \"src\"}\n```<｜tool▁call▁end｜>\n<｜tool▁call▁begin｜>function<｜tool▁sep｜>list_dir\n```json\n{\"path\": \"docs\", \"depth\": 1}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>\n<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>list_dir<｜tool▁sep｜>{\"path\": \"tests\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>";
    let (output, tools) = run(&[], turn.as_bytes());
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    for (index, (dialect, arguments)) in [
        ("deepseek-v3.v1", r#"{"path":"src"}"#),
        ("deepseek-v3.v1", r#"{"path":"docs","depth":1}"#),
        ("deepseek-v3.1.v1", r#"{"path":"tests"}"#),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/dialect"))
                .and_then(Node::as_str),
            Some(dialect)
        );
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/arguments"))
                .unwrap()
                .to_json(),
            arguments
        );
    }
    fs::remove_file(tools).unwrap();
    let hermes =
        b"<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}\n</tool_call>";
    let (output, tools) = run(&["--emit", "deepseek-v3.v1"], hermes);
    let node = result(&output);
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted markup")
        .to_owned();
    assert_eq!(
        emitted,
        "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>list_dir\n```json\n{\"path\":\"src\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>"
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], emitted.as_bytes());
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("deepseek-v3.v1")
    );
    fs::remove_file(tools).unwrap();
}

/// Kimi-K2 with no `--dialect`: a Thinking turn's section is one call per
/// call, each keeping its `functions.NAME:IDX` id, and a Hermes call emitted
/// as `kimi-k2.v1` recovers to the same call.
#[test]
fn kimi_k2_recovers_and_emits_via_cli() {
    let turn = "<think>Two listings.</think><|tool_calls_section_begin|><|tool_call_begin|>functions.list_dir:0<|tool_call_argument_begin|>{\"path\": \"src\"}<|tool_call_end|><|tool_call_begin|>functions.list_dir:1<|tool_call_argument_begin|>{\"path\": \"docs\", \"depth\": 1}<|tool_call_end|><|tool_calls_section_end|>";
    let (output, tools) = run(&[], turn.as_bytes());
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    for (index, (id, arguments)) in [
        ("functions.list_dir:0", r#"{"path":"src"}"#),
        ("functions.list_dir:1", r#"{"path":"docs","depth":1}"#),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/dialect"))
                .and_then(Node::as_str),
            Some("kimi-k2.v1")
        );
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/id"))
                .and_then(Node::as_str),
            Some(id)
        );
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/arguments"))
                .unwrap()
                .to_json(),
            arguments
        );
    }
    fs::remove_file(tools).unwrap();
    let hermes =
        b"<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}\n</tool_call>";
    let (output, tools) = run(&["--emit", "kimi-k2.v1"], hermes);
    let node = result(&output);
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted markup")
        .to_owned();
    assert_eq!(
        emitted,
        "<|tool_calls_section_begin|><|tool_call_begin|>functions.list_dir:0<|tool_call_argument_begin|>{\"path\":\"src\"}<|tool_call_end|><|tool_calls_section_end|>"
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], emitted.as_bytes());
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("kimi-k2.v1")
    );
    fs::remove_file(tools).unwrap();
}

/// Anthropic `tool_use` blocks with no `--dialect`: an assistant message's
/// blocks are the calls, each keeping its `toolu_` id, and a Hermes call
/// emitted as `anthropic-tool-use.v1` recovers to the same call.
#[test]
fn anthropic_tool_use_recovers_and_emits_via_cli() {
    let turn = r#"{"role":"assistant","content":[{"type":"text","text":"Two listings."},{"type":"tool_use","id":"toolu_01","name":"list_dir","input":{"path":"src"}},{"type":"tool_use","id":"toolu_02","name":"list_dir","input":{"path":"docs","depth":1}}]}"#;
    let (output, tools) = run(&[], turn.as_bytes());
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    for (index, (id, arguments)) in [
        ("toolu_01", r#"{"path":"src"}"#),
        ("toolu_02", r#"{"path":"docs","depth":1}"#),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/dialect"))
                .and_then(Node::as_str),
            Some("anthropic-tool-use.v1")
        );
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/id"))
                .and_then(Node::as_str),
            Some(id)
        );
        assert_eq!(
            node.pointer(&format!("/value/calls/{index}/arguments"))
                .unwrap()
                .to_json(),
            arguments
        );
    }
    fs::remove_file(tools).unwrap();
    let hermes =
        b"<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}\n</tool_call>";
    let (output, tools) = run(&["--emit", "anthropic-tool-use.v1"], hermes);
    let node = result(&output);
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted markup")
        .to_owned();
    let id = node
        .pointer("/value/calls/0/id")
        .and_then(Node::as_str)
        .expect("id")
        .to_owned();
    assert_eq!(
        emitted,
        format!(r#"{{"type":"tool_use","id":"{id}","name":"list_dir","input":{{"path":"src"}}}}"#)
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], emitted.as_bytes());
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("anthropic-tool-use.v1")
    );
    fs::remove_file(tools).unwrap();
}

/// Harmony with no `--dialect`: a gpt-oss turn's analysis is reasoning and
/// its commentary message to `functions.NAME` the call, and a Hermes call
/// emitted as `harmony.v1` recovers to the same call.
#[test]
fn harmony_recovers_and_emits_via_cli() {
    let turn = "<|channel|>analysis<|message|>List the sources.<|end|><|start|>assistant<|channel|>commentary to=functions.list_dir <|constrain|>json<|message|>{\"path\": \"src\"}<|call|>";
    let (output, tools) = run(&[], turn.as_bytes());
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("harmony.v1")
    );
    assert_eq!(
        node.pointer("/value/calls/0/arguments").unwrap().to_json(),
        r#"{"path":"src"}"#
    );
    fs::remove_file(tools).unwrap();
    let hermes =
        b"<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}\n</tool_call>";
    let (output, tools) = run(&["--emit", "harmony.v1"], hermes);
    let node = result(&output);
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted markup")
        .to_owned();
    assert_eq!(
        emitted,
        "<|start|>assistant<|channel|>commentary to=functions.list_dir <|constrain|>json<|message|>{\"path\":\"src\"}<|call|>"
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], emitted.as_bytes());
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("harmony.v1")
    );
    fs::remove_file(tools).unwrap();
}

/// Bare JSON with no `--dialect`: an unwrapped call object in prose is the
/// call and the prose content, a JSON record with a name stays content, and
/// a Hermes call emitted as `bare-json.v1` recovers to the same call.
#[test]
fn bare_json_recovers_and_emits_via_cli() {
    let turn =
        "Listing it now.\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}\nDone.";
    let (output, tools) = run(&[], turn.as_bytes());
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("bare-json.v1")
    );
    assert_eq!(
        node.pointer("/value/calls/0/arguments").unwrap().to_json(),
        r#"{"path":"src"}"#
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], br#"Record: {"name": "Ada", "born": 1815}"#);
    let node = result(&output);
    assert_eq!(
        node.pointer("/value/finish").and_then(Node::as_str),
        Some("content_only")
    );
    fs::remove_file(tools).unwrap();
    let hermes =
        b"<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"src\"}}\n</tool_call>";
    let (output, tools) = run(&["--emit", "bare-json.v1"], hermes);
    let node = result(&output);
    let emitted = node
        .pointer("/value/emitted/output/text")
        .and_then(Node::as_str)
        .expect("emitted markup")
        .to_owned();
    assert_eq!(
        emitted,
        "{\"name\":\"list_dir\",\"arguments\":{\"path\":\"src\"}}"
    );
    fs::remove_file(tools).unwrap();
    let (output, tools) = run(&[], emitted.as_bytes());
    let node = result(&output);
    assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
    assert_eq!(
        node.pointer("/value/calls/0/dialect")
            .and_then(Node::as_str),
        Some("bare-json.v1")
    );
    fs::remove_file(tools).unwrap();
}

/// Call markup no allowed dialect reads is reported, never silent content:
/// a Pythonic list and `[TOOL_CALLS]` under a Hermes-only allow-list
/// are rejected attempts with reason
/// `unsupported_dialect`, the bytes stay content, and the finish is
/// `unresolved`.
#[test]
fn unparsed_markup_is_reported_via_cli() {
    for (args, turn, family, built_in) in [
        (
            &["--dialect", "hermes.v1"][..],
            &br#"[list_dir(path="src")]"#[..],
            "pythonic.v1",
            true,
        ),
        (
            &["--dialect", "hermes.v1"][..],
            &br#"[TOOL_CALLS] [{"name": "list_dir", "arguments": {"path": "src"}}]"#[..],
            "mistral-v3.v1",
            true,
        ),
    ] {
        let (output, tools) = run(args, turn);
        let node = result(&output);
        assert_eq!(
            node.pointer("/value/finish").and_then(Node::as_str),
            Some("unresolved")
        );
        assert_eq!(
            node.pointer("/value/rejected/0/reason/code")
                .and_then(Node::as_str),
            Some("unsupported_dialect")
        );
        assert_eq!(
            node.pointer("/value/rejected/0/reason/family")
                .and_then(Node::as_str),
            Some(family)
        );
        assert_eq!(
            node.pointer("/value/rejected/0/reason/built_in")
                .map(Node::to_json)
                .as_deref(),
            Some(if built_in { "true" } else { "false" })
        );
        assert_eq!(
            node.pointer("/value/content/0/end")
                .map(Node::to_json)
                .as_deref(),
            Some(turn.len().to_string().as_str())
        );
        fs::remove_file(tools).unwrap();
    }
}

/// Run `turn` and return the exit code and the turn's finish (`None` when
/// the operation published no value).
fn exit_and_finish(declarations: &str, arguments: &[&str], turn: &[u8]) -> (i32, Option<String>) {
    let (output, tools) = run_with(declarations, arguments, turn);
    fs::remove_file(tools).unwrap();
    let node = result(&output);
    (
        output.status.code().expect("an exit code"),
        node.pointer("/value/finish")
            .and_then(Node::as_str)
            .map(str::to_owned),
    )
}

/// The exit code summarizes what the caller can use: 0 for a usable call
/// with the tool choice passed (even beside a rejected attempt) or clean
/// content, 3 for ambiguity with nothing usable, 4 when nothing is usable
/// (an unresolved call, unparsed call markup only, a cut-off turn with no
/// usable call) or the tool choice is violated, even with a usable call.
/// The JSON is the whole answer either way.
#[test]
fn exit_code_reflects_what_is_usable() {
    let call = |arguments: &str| {
        format!("<tool_call>{{\"name\": \"list_dir\", \"arguments\": {arguments}}}</tool_call>")
    };
    let resolved = call(r#"{"path": "src"}"#);
    let cut = "<tool_call>{\"name\": \"list_dir\", \"arguments\": {\"path\": \"sr";
    let both = format!("{resolved}\n{cut}");
    let stray = format!("{resolved}\n<tool_call>\n");
    let hallucinated =
        "<tool_call>{\"name\": \"send_email\", \"arguments\": {\"to\": \"a\"}}</tool_call>";
    let missing = call("{}");
    let repeated = "<tool_call>\n<function=list_dir>\n<parameter=path>\na\n</parameter>\n<parameter=path>\nb\n</parameter>\n</function>\n</tool_call>";
    for (case, arguments, turn, code, finish) in [
        ("resolved calls", &[][..], resolved.as_str(), 0, "calls"),
        (
            "usable call beside a stray rejected opener",
            &[][..],
            stray.as_str(),
            0,
            "unresolved",
        ),
        (
            "clean content",
            &[][..],
            "The directory is empty.",
            0,
            "content_only",
        ),
        (
            "truncated with calls",
            &["--stop", "length"][..],
            both.as_str(),
            0,
            "truncated_with_calls",
        ),
        (
            "truncated, nothing usable",
            &["--stop", "length"][..],
            cut,
            4,
            "truncated",
        ),
        ("hallucinated name", &[][..], hallucinated, 4, "unresolved"),
        (
            "missing required argument",
            &[][..],
            missing.as_str(),
            4,
            "unresolved",
        ),
        (
            "forced tool choice violated",
            &["--tool-choice", "list_dir"][..],
            "No call needed.",
            4,
            "unresolved",
        ),
        (
            "tool choice none with a call",
            &["--tool-choice", "none"][..],
            resolved.as_str(),
            4,
            "unresolved",
        ),
        (
            "tool choice required with no call",
            &["--tool-choice", "required"][..],
            "No call needed.",
            4,
            "unresolved",
        ),
        (
            "Pythonic calls",
            &[][..],
            "[list_dir(path=\"src\")]",
            0,
            "calls",
        ),
        (
            "unparsed markup only",
            &["--dialect", "hermes.v1"][..],
            "[list_dir(path=\"src\")]",
            4,
            "unresolved",
        ),
        (
            "an ambiguous call",
            &["--policy", "tolerant"][..],
            repeated,
            3,
            "unresolved",
        ),
    ] {
        let (exit, finished) = exit_and_finish(TOOLS, arguments, turn.as_bytes());
        assert_eq!(exit, code, "{case}");
        assert_eq!(finished.as_deref(), Some(finish), "{case}");
    }
}

/// Exit 4 when the operation publishes no value: ambiguity rejected under
/// `--ambiguity reject`, and an exhausted candidate budget.
#[test]
fn exit_code_without_a_value() {
    let repeated = "<tool_call>\n<function=list_dir>\n<parameter=path>\na\n</parameter>\n<parameter=path>\nb\n</parameter>\n</function>\n</tool_call>";
    // Rejecting ambiguity leaves nothing usable: the operation is
    // Unresolved.
    let (exit, finished) = exit_and_finish(
        TOOLS,
        &["--policy", "tolerant", "--ambiguity", "reject"],
        repeated.as_bytes(),
    );
    assert_eq!((exit, finished), (4, None));
    // A call with 16 parameters each written twice (2^16 readings)
    // exhausts the candidate budget: Unresolved, no value.
    let mut properties = String::new();
    let mut body = String::new();
    for index in 0..16 {
        let _ = write!(
            properties,
            "{}\"p{index}\":{{\"type\":\"string\"}}",
            if index == 0 { "" } else { "," }
        );
        let _ = write!(
            body,
            "<parameter=p{index}>\na\n</parameter>\n<parameter=p{index}>\nb\n</parameter>\n"
        );
    }
    let declarations = format!(
        r#"[{{"type":"function","function":{{"name":"f","parameters":{{"type":"object","properties":{{{properties}}}}}}}}}]"#
    );
    let turn = format!("<tool_call>\n<function=f>\n{body}</function>\n</tool_call>");
    let (output, tools) = run_with(&declarations, &[], turn.as_bytes());
    fs::remove_file(tools).unwrap();
    assert_eq!(output.status.code(), Some(4));
    let node = result(&output);
    assert_eq!(
        node.get("status").and_then(Node::as_str),
        Some("unresolved")
    );
    assert_eq!(
        node.pointer("/receipt/termination/kind")
            .and_then(Node::as_str),
        Some("budget_exceeded")
    );
}

/// The adapters read the `--tools` file's exact bytes: a declaration with
/// unusual but valid formatting (spaces around colons, `"function"`
/// before `"type"`, `"name"` after `"parameters"`, `1.50e3` as a
/// `maximum`) compiles its schema from the exact parameters text, so the
/// receipt's schema digest is that of the file's bytes, not of a
/// re-serialization.
#[test]
fn tools_file_bytes_reach_the_adapter_exactly() {
    let parameters = r#"{"properties" : {"path":{"type":"string"},  "depth":{"maximum":1.50e3, "type":"integer"}},"type":"object","required":["path"]}"#;
    let declarations = format!(
        "[ {{  \"function\" : {{ \"parameters\" : {parameters} , \"name\" : \"list_dir\" }}, \"type\":\"function\" }} ]\n"
    );
    let turn =
        br#"<tool_call>{"name": "list_dir", "arguments": {"path": "src", "depth": 2}}</tool_call>"#;
    let (output, tools) = run_with(&declarations, &[], turn);
    fs::remove_file(tools).unwrap();
    assert_eq!(output.status.code(), Some(0));
    let node = result(&output);
    let exact = purrassist::Schema::compile(parameters)
        .expect("the exact parameters text compiles")
        .digest()
        .to_string();
    let canonical = purrassist::Schema::compile(&json::parse(parameters).unwrap().to_json())
        .expect("the re-serialized text compiles")
        .digest()
        .to_string();
    assert_ne!(exact, canonical, "formatting must change the digest");
    assert_eq!(
        node.pointer("/receipt/validations/0/constraint")
            .and_then(Node::as_str),
        Some(exact.as_str())
    );
}

/// `--help` lists every built-in dialect id (generated from
/// `DialectId::BUILT_IN`, so the list cannot drift from the shipped set).
#[test]
fn help_lists_every_built_in_dialect() {
    let output = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(!help.contains("{dialects}"), "{help}");
    for dialect in purrassist::tool_call::DialectId::BUILT_IN {
        assert!(
            help.contains(dialect.id()),
            "{} missing from --help",
            dialect.id()
        );
    }
}

/// `--tool-choice NAME` with the `--tool-choice-*` flags is the strict
/// forced choice: the constraint lists the active options, the exit code is
/// 0 when the check passes and 4 when it fails.
#[test]
fn strict_forced_tool_choice_flags() {
    let call = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    let strict = |arguments: &[&str], turn: &str| {
        let (output, tools) = run(arguments, turn.as_bytes());
        fs::remove_file(tools).unwrap();
        let node = result(&output);
        let constraint = node
            .pointer("/value/tool_choice/constraint")
            .and_then(Node::as_str)
            .map(str::to_owned);
        (output.status.code().expect("an exit code"), constraint)
    };
    let all = [
        "--tool-choice",
        "list_dir",
        "--tool-choice-exactly-one",
        "--tool-choice-no-content",
        "--tool-choice-no-reasoning",
    ];
    assert_eq!(
        strict(&all, call),
        (
            0,
            Some("tool_choice=forced[exactly-one,no-content,no-reasoning]:list_dir".into())
        )
    );
    // Two calls fail exactly-one.
    let two = format!("{call}\n{call}");
    assert_eq!(
        strict(
            &["--tool-choice", "list_dir", "--tool-choice-exactly-one"],
            &two
        ),
        (4, Some("tool_choice=forced[exactly-one]:list_dir".into()))
    );
    // Prose fails no-content; framing whitespace does not.
    assert_eq!(
        strict(
            &["--tool-choice", "list_dir", "--tool-choice-no-content"],
            &format!("Sure.\n{call}")
        ),
        (4, Some("tool_choice=forced[no-content]:list_dir".into()))
    );
    assert_eq!(
        strict(
            &["--tool-choice", "list_dir", "--tool-choice-no-content"],
            &format!("\n{call}\n")
        ),
        (0, Some("tool_choice=forced[no-content]:list_dir".into()))
    );
    // A reasoning block fails no-reasoning.
    assert_eq!(
        strict(
            &["--tool-choice", "list_dir", "--tool-choice-no-reasoning"],
            &format!("<think>plan</think>{call}")
        ),
        (4, Some("tool_choice=forced[no-reasoning]:list_dir".into()))
    );
    // Without the flags the choice is the plain forced one.
    assert_eq!(
        strict(&["--tool-choice", "list_dir"], &two),
        (0, Some("tool_choice=forced:list_dir".into()))
    );
}

/// A second call attempt that is rejected fails the strict forced choice
/// (R4, G3): exit 4, `passed: false`, both usable counts 0 (A3), nothing
/// emitted, though the first call is valid on its own.
#[test]
fn strict_forced_flags_count_a_rejected_second_attempt() {
    let call = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    let attempt = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "lib"}} junk junk {{{</tool_call>"#;
    let turn = format!("{call}\n{attempt}");
    let (output, tools) = run(
        &[
            "--dialect",
            "hermes.v1",
            "--tool-choice",
            "list_dir",
            "--tool-choice-exactly-one",
            "--tool-choice-no-content",
            "--emit",
            "openai",
        ],
        turn.as_bytes(),
    );
    fs::remove_file(tools).unwrap();
    assert_eq!(output.status.code(), Some(4));
    let node = result(&output);
    let text = |pointer: &str| node.pointer(pointer).map(Node::to_json);
    assert_eq!(text("/value/tool_choice/passed").as_deref(), Some("false"));
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
    assert!(
        diagnostics.contains("rejected call attempt at bytes"),
        "{diagnostics}"
    );
}

/// `--tool-choice-no-reasoning` with `--reasoning content` is a request
/// error (R4, G15): reasoning is never recognized, so the option could not
/// be enforced. The result is unresolved with an `invalid_input` termination
/// (exit 4), never a passed check.
#[test]
fn strict_no_reasoning_with_content_reasoning_is_invalid_input() {
    let call = r#"<tool_call>{"name": "list_dir", "arguments": {"path": "src"}}</tool_call>"#;
    let turn = format!("<think>plan</think>\n{call}");
    let (output, tools) = run(
        &[
            "--reasoning",
            "content",
            "--tool-choice",
            "list_dir",
            "--tool-choice-no-reasoning",
        ],
        turn.as_bytes(),
    );
    fs::remove_file(tools).unwrap();
    assert_eq!(output.status.code(), Some(4));
    let node = result(&output);
    assert_eq!(
        node.pointer("/status").and_then(Node::as_str),
        Some("unresolved")
    );
    assert_eq!(
        node.pointer("/receipt/termination/kind")
            .and_then(Node::as_str),
        Some("invalid_input")
    );
    let detail = node
        .pointer("/receipt/termination/detail")
        .and_then(Node::as_str)
        .unwrap();
    assert!(detail.contains("allow_reasoning"), "{detail}");
    // Separate reasoning (the default) enforces it as before.
    let (output, tools) = run(
        &["--tool-choice", "list_dir", "--tool-choice-no-reasoning"],
        turn.as_bytes(),
    );
    fs::remove_file(tools).unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(
        result(&output)
            .pointer("/value/tool_choice/constraint")
            .and_then(Node::as_str),
        Some("tool_choice=forced[no-reasoning]:list_dir")
    );
}

/// The `--tool-choice-*` flags are a usage error without a tool name, and
/// each may be given once.
#[test]
fn strict_forced_flags_need_a_tool_name() {
    for arguments in [
        &["--tool-choice-exactly-one"][..],
        &["--tool-choice", "auto", "--tool-choice-no-content"][..],
        &["--tool-choice", "none", "--tool-choice-no-reasoning"][..],
        &["--tool-choice", "required", "--tool-choice-exactly-one"][..],
    ] {
        let (output, tools) = run(arguments, b"No call needed.");
        fs::remove_file(tools).unwrap();
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("--tool-choice NAME"),
            "{arguments:?}: {stderr}"
        );
        assert!(output.stdout.is_empty(), "{arguments:?}");
    }
    let (output, tools) = run(
        &[
            "--tool-choice",
            "list_dir",
            "--tool-choice-no-content",
            "--tool-choice-no-content",
        ],
        b"x",
    );
    fs::remove_file(tools).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("repeated"));
}
