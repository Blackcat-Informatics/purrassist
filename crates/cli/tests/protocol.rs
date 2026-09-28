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
        "{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"tools/call\",\"params\":{\"name\":\"match_paths\",\"arguments\":{\"query\":\"raeder.rs\",\"context\":\"src\",\"paths\":[\"src/reader.rs\",\"tests/reader.rs\"]}}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"tools/call\",\"params\":{\"name\":\"resolve_intent\",\"arguments\":{\"input\":\"hot tea cooled\",\"domain\":{\"id\":\"drinks.v1\",\"rules\":[{\"id\":\"hot\",\"phrase\":\"hot\",\"assignments\":{\"temperature\":\"hot\"}},{\"id\":\"tea\",\"phrase\":\"tea\",\"assignments\":{\"drink\":\"tea\"}},{\"id\":\"cooled\",\"phrase\":\"cooled\",\"assignments\":{\"temperature\":\"cold\"}}]},\"concepts\":[{\"id\":\"iced-tea\",\"label\":\"cup of iced tea\",\"attributes\":{\"drink\":\"tea\",\"temperature\":\"cold\"}}]}}}\n",
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
        11,
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
    assert_tool_responses(&messages);
    fs::remove_dir_all(directory).unwrap();
}

fn assert_tool_responses(messages: &[Node]) {
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
    assert_eq!(
        messages[9]
            .pointer("/result/structuredContent/status")
            .and_then(Node::as_str),
        Some("best_guess")
    );
    assert_eq!(
        messages[9]
            .pointer("/result/structuredContent/value/path")
            .and_then(Node::as_str),
        Some("src/reader.rs")
    );
    assert_eq!(
        messages[10]
            .pointer("/result/structuredContent/status")
            .and_then(Node::as_str),
        Some("resolved")
    );
    assert_eq!(
        messages[10]
            .pointer("/result/structuredContent/value/concept/id")
            .and_then(Node::as_str),
        Some("iced-tea")
    );
}

#[test]
fn shared_operations_are_identical_through_cli_and_mcp() {
    for (operation, arguments, pointer, expected) in [
        (
            "plan_markdown_reference",
            r#"{"source":"> [tea]: /hot\n","path":[0,0],"occurrence":0,"destination":"/iced","title":null}"#,
            "/value/edits/0/replacement",
            "<\\/iced>",
        ),
        (
            "plan_markdown_table_column",
            r#"{"source":"| A |\n| --- |\n","path":[0],"action":"align","index":0,"alignment":"center"}"#,
            "/value/edits/0/replacement",
            "| :---: |",
        ),
        (
            "plan_markdown_table_row",
            r#"{"source":"> | A |\n> | --- |\n","path":[0,0],"action":"insert","index":0,"cells":["tea"]}"#,
            "/value/edits/0/replacement",
            "> | tea |\n",
        ),
        (
            "plan_markdown_table_cell",
            r#"{"source":"> | A |\n> | --- |\n> | tea |\n","path":[0,0],"row":1,"column":0,"markdown":"**iced**"}"#,
            "/value/edits/0/replacement",
            "**iced**",
        ),
        (
            "match_embeddings",
            r#"{"space":"caller-v1","query":[1,0],"candidates":[{"id":"tea","vector":[1,0]},{"id":"other","vector":[0,1]}]}"#,
            "/value/id",
            "tea",
        ),
        (
            "plan_markdown_task",
            r#"{"source":"> - [x] tea\r\n","path":[0,0],"checked":false}"#,
            "/value/edits/0/replacement",
            " ",
        ),
        (
            "markdown_context",
            r##"{"source":"# Tea\r\n- [ ] café\r\n","start":13,"end":18}"##,
            "/value/text",
            "- [ ] café\r\n",
        ),
        (
            "render_markdown",
            r#"{"source":"~~café~~\r\n","dialect":"gfm"}"#,
            "/value/html",
            "<p><del>café</del></p>\n",
        ),
        (
            "render_markdown",
            r#"{"source":"~~café~~\r\n","dialect":"gfm","extensions":[]}"#,
            "/value/html",
            "<p>~~café~~</p>\n",
        ),
    ] {
        assert_shared_operation(operation, arguments, pointer, expected);
    }
}
fn assert_shared_operation(operation: &str, arguments: &str, pointer: &str, expected: &str) {
    let directory = std::env::temp_dir().join(format!(
        "purrassist-markdown-protocol-{}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let mut cli = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .args(["call", operation, "--root"])
        .arg(&directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    cli.stdin
        .take()
        .unwrap()
        .write_all(arguments.as_bytes())
        .unwrap();
    let output = cli.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let direct = json::parse(&String::from_utf8(output.stdout).unwrap()).unwrap();
    let mut mcp = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .args(["mcp", "--root"])
        .arg(&directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let frames = format!(
        concat!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{{}},\"clientInfo\":{{\"name\":\"test\",\"version\":\"1\"}}}}}}\n",
            "{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}\n",
            "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"{}\",\"arguments\":{}}}}}\n"
        ),
        operation, arguments
    );
    mcp.stdin
        .take()
        .unwrap()
        .write_all(frames.as_bytes())
        .unwrap();
    let output = mcp.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    let messages: Vec<_> = output
        .lines()
        .map(|line| json::parse(line).unwrap())
        .collect();
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages[1]
            .pointer("/result/structuredContent")
            .unwrap()
            .to_json(),
        direct.to_json()
    );
    assert_eq!(direct.pointer(pointer).unwrap().as_str(), Some(expected));
    fs::remove_dir_all(directory).unwrap();
}
