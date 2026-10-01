// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Execute the shipped `recover-tool-calls` command on raw model output.
use purrassist::json::{self, Node, Value};
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
    assert_eq!(
        emitted.to_json(),
        r#"[{"id":"call_0","type":"function","function":{"name":"list_dir","arguments":"{\"depth\":2,\"path\":\"src\"}"}}]"#
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
