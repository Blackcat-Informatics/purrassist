// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Execute the shipped CLI and MCP stdio transport, including lifecycle and receipts.
use purrassist::json::{self, Node, Value};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn stdio_lifecycle_and_recovery_use_real_tool_operations() {
    let directory =
        std::env::temp_dir().join(format!("purrassist-mcp-test-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .args(["mcp", "--root"])
        .arg(&directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"tools/list\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"decode_json\",\"arguments\":{\"input\":\"[[42]]\",\"schema\":{\"type\":\"integer\"}}}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"not-a-method\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"plan_replacement\",\"arguments\":{\"source\":\"hello world\",\"needle\":\"wurld\",\"replacement\":\"earth\",\"max_edits\":1}}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"unknown_tool\",\"arguments\":{}}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":7,\"id\":8,\"method\":\"ping\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":9.5,\"method\":\"ping\"}\n",
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let messages: Vec<Node> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| json::parse(line).unwrap())
        .collect();
    assert_eq!(
        messages.len(),
        9,
        "MCP responses: {messages:?}; binary {}",
        env!("CARGO_BIN_EXE_purrassist")
    );
    assert!(messages[0].get("error").is_some());
    assert_eq!(
        messages[1]
            .pointer("/result/protocolVersion")
            .and_then(Node::as_str),
        Some("2025-11-25")
    );
    let Value::Array(tools) = &messages[2].pointer("/result/tools").unwrap().value else {
        panic!("tool list");
    };
    assert!(
        tools
            .iter()
            .any(|tool| tool.get("name").and_then(Node::as_str) == Some("apply_file"))
    );
    assert_eq!(
        messages[3]
            .pointer("/result/structuredContent/status")
            .and_then(Node::as_str),
        Some("resolved")
    );
    assert_eq!(
        messages[3]
            .pointer("/result/structuredContent/value")
            .unwrap()
            .to_json(),
        "42"
    );
    assert!(
        messages[3]
            .pointer("/result/structuredContent/receipt")
            .is_some()
    );
    assert!(messages[4].get("error").is_some());
    assert_eq!(
        messages[5]
            .pointer("/result/structuredContent/status")
            .and_then(Node::as_str),
        Some("best_guess")
    );
    assert_eq!(
        messages[5]
            .pointer("/result/structuredContent/plans/0/edits/0/start")
            .unwrap()
            .to_json(),
        "6"
    );
    for (index, code) in [(6, "-32602"), (7, "-32600"), (8, "-32600")] {
        assert_eq!(
            messages[index].pointer("/error/code").unwrap().to_json(),
            code
        );
    }
    fs::remove_dir_all(directory).unwrap();
}
