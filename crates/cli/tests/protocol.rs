// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Execute the shipped CLI and MCP stdio transport, including lifecycle and receipts.
use purrassist::{
    digest::Digest,
    json::{self, Node, Value},
    schema::Schema,
    wire::{integer, object, string},
};
use purrassist_host::{files::Store, tools::Tools};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
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
    let mut messages: Vec<Node> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| json::parse(line).unwrap())
        .collect();
    // JSON-RPC permits tool responses to complete out of request order.
    messages.sort_by_key(|message| match message.get("id").map(|id| &id.value) {
        Some(Value::Number(number)) => number.to_usize().unwrap(),
        _ => 7,
    });
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
fn broken_stdout_terminates_even_when_stdin_remains_open() {
    let root = std::env::temp_dir().join(format!(
        "purrassist-mcp-output-failure-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .args(["mcp", "--root"])
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    drop(child.stdout.take());
    writeln!(input, r#"{{"jsonrpc":"2.0","id":1,"method":"ping"}}"#).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("MCP output failure left the process waiting on open stdin");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert!(!status.success());
    drop(input);
    fs::remove_dir_all(root).unwrap();
}

/// One CLI/MCP equivalence fixture for a single registry entry: deterministic
/// arguments (a function of a fresh, tool-owned root directory, so file-based
/// tools can create/pre-commit whatever they need), plus an operation-specific
/// assertion on the shared result.
struct Fixture {
    operation: &'static str,
    arguments: Box<dyn Fn(&Path) -> String>,
    check: Box<dyn Fn(&Node)>,
}
fn fx(
    operation: &'static str,
    arguments: impl Fn(&Path) -> String + 'static,
    check: impl Fn(&Node) + 'static,
) -> Fixture {
    Fixture {
        operation,
        arguments: Box::new(arguments),
        check: Box::new(check),
    }
}

/// Exactly one fixture per `purrassist_host::tools::registry` entry;
/// `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` fails
/// if any registered tool lacks one.
#[allow(clippy::too_many_lines)]
fn fixtures() -> Vec<Fixture> {
    vec![
        fx(
            "match_embeddings",
            |_| {
                r#"{"space":"caller-v1","query":[1,0],"candidates":[{"id":"tea","vector":[1,0]},{"id":"other","vector":[0,1]}]}"#.into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/value/id").and_then(Node::as_str),
                    Some("tea")
                );
            },
        ),
        fx(
            "render_markdown",
            |_| r#"{"source":"~~café~~\r\n","dialect":"gfm"}"#.into(),
            |node| {
                assert_eq!(
                    node.pointer("/value/html").and_then(Node::as_str),
                    Some("<p><del>café</del></p>\n")
                );
            },
        ),
        fx(
            "inspect_markdown",
            |_| r##"{"source":"# Tea\n"}"##.into(),
            |node| {
                assert_eq!(
                    node.pointer("/value/blocks/0/kind/type")
                        .and_then(Node::as_str),
                    Some("heading")
                );
            },
        ),
        fx(
            "markdown_context",
            |_| r##"{"source":"# Tea\r\n- [ ] café\r\n","start":13,"end":18}"##.into(),
            |node| {
                assert_eq!(
                    node.pointer("/value/text").and_then(Node::as_str),
                    Some("- [ ] café\r\n")
                );
            },
        ),
        fx(
            "plan_markdown_list_item",
            |_| {
                r#"{"source":"> - hot\n","path":[0,0],"action":"after","marker":"-","body":"iced"}"#
                    .into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/value/edits/0/replacement")
                        .and_then(Node::as_str),
                    Some("> - iced\n")
                );
            },
        ),
        fx(
            "plan_markdown_section",
            |_| r##"{"source":"# Tea\nHot.\n","heading":0,"body":"Cold.\n"}"##.into(),
            |node| {
                assert_eq!(
                    node.pointer("/value/edits/0/replacement")
                        .and_then(Node::as_str),
                    Some("Cold.\n")
                );
            },
        ),
        fx(
            "plan_markdown_reference",
            |_| {
                r#"{"source":"> [tea]: /hot\n","path":[0,0],"occurrence":0,"destination":"/iced","title":null}"#.into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/value/edits/0/replacement")
                        .and_then(Node::as_str),
                    Some("<\\/iced>")
                );
            },
        ),
        fx(
            "plan_markdown_table_column",
            |_| {
                r#"{"source":"| A |\n| --- |\n","path":[0],"action":"align","index":0,"alignment":"center"}"#.into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/value/edits/0/replacement")
                        .and_then(Node::as_str),
                    Some("| :---: |")
                );
            },
        ),
        fx(
            "plan_markdown_table_row",
            |_| {
                r#"{"source":"> | A |\n> | --- |\n","path":[0,0],"action":"insert","index":0,"cells":["tea"]}"#.into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/value/edits/0/replacement")
                        .and_then(Node::as_str),
                    Some("> | tea |\n")
                );
            },
        ),
        fx(
            "plan_markdown_table_cell",
            |_| {
                r#"{"source":"> | A |\n> | --- |\n> | tea |\n","path":[0,0],"row":1,"column":0,"markdown":"**iced**"}"#.into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/value/edits/0/replacement")
                        .and_then(Node::as_str),
                    Some("**iced**")
                );
            },
        ),
        fx(
            "plan_markdown_task",
            |_| r#"{"source":"> - [x] tea\r\n","path":[0,0],"checked":false}"#.into(),
            |node| {
                assert_eq!(
                    node.pointer("/value/edits/0/replacement")
                        .and_then(Node::as_str),
                    Some(" ")
                );
            },
        ),
        fx(
            "plan_markdown_structure",
            |_| {
                r##"{"source":"# Tea\nHot.\n","op":"section_rename","heading":0,"title":"Coffee"}"##
                    .into()
            },
            |node| {
                assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
                let replacement = node
                    .pointer("/value/edits/0/replacement")
                    .and_then(Node::as_str)
                    .unwrap();
                assert!(replacement.starts_with("# Coffee"), "{replacement:?}");
            },
        ),
        fx(
            "decode_json",
            |_| r#"{"input":"[[42]]","schema":{"type":"integer"}}"#.into(),
            |node| assert_eq!(node.pointer("/value").unwrap().to_json(), "42"),
        ),
        fx(
            "validate_json",
            |_| r#"{"schema":{"type":"integer"},"instance":42}"#.into(),
            |node| assert_eq!(node.get("valid").unwrap().to_json(), "true"),
        ),
        fx(
            "generate_json_witness",
            |_| {
                r#"{"schema":{"type":"object","required":["a"],"properties":{"a":{"type":"integer","minimum":3}}}}"#
                    .into()
            },
            |node| {
                assert_eq!(node.get("outcome").and_then(Node::as_str), Some("witness"));
                assert_eq!(node.get("witness").unwrap().to_json(), r#"{"a":3}"#);
                assert_eq!(node.pointer("/refutation").unwrap().to_json(), "[]");
            },
        ),
        fx(
            "match_strings",
            |_| {
                r#"{"query":"iced tea","candidates":[{"id":"tea","text":"cold tea","aliases":["iced tea"]}],"english_phonetics":false}"#.into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/matches/0/id").and_then(Node::as_str),
                    Some("tea")
                );
            },
        ),
        fx(
            "resolve_string",
            |_| {
                r#"{"query":"reader","candidates":[{"id":"a","text":"reader"},{"id":"b","text":"reader"}],"minimum_score":10000,"minimum_margin":0}"#.into()
            },
            |node| {
                // The two candidates tie exactly: the wire response must carry
                // one alternative per candidate and a top-level `ambiguity`
                // annotation (Task 10's forest-annotation requirement).
                assert_eq!(node.get("status").and_then(Node::as_str), Some("ambiguous"));
                let Value::Array(alternatives) = &node.get("alternatives").unwrap().value else {
                    panic!("expected an alternatives array")
                };
                assert_eq!(alternatives.len(), 2);
                let Value::Array(ledgers) = &node.pointer("/receipt/alternatives").unwrap().value
                else {
                    panic!("expected receipt alternatives")
                };
                assert_eq!(ledgers.len(), 2, "one ledger per candidate");
                let ambiguity = node
                    .pointer("/receipt/ambiguity")
                    .expect("an ambiguity annotation");
                assert_eq!(ambiguity.get("derivations").unwrap().to_json(), "2");
                let Value::Array(distinguishing) = &ambiguity.get("distinguishing").unwrap().value
                else {
                    panic!("expected a distinguishing list")
                };
                assert_eq!(
                    distinguishing.len(),
                    1,
                    "two alternatives have one adjacent pair"
                );
            },
        ),
        fx(
            "match_paths",
            |_| {
                r#"{"query":"raeder.rs","context":"src","paths":["src/reader.rs","tests/reader.rs"]}"#.into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/value/path").and_then(Node::as_str),
                    Some("src/reader.rs")
                );
            },
        ),
        fx(
            "resolve_intent",
            |_| {
                r#"{"input":"hot tea cooled","domain":{"id":"drinks.v1","rules":[{"id":"hot","phrase":"hot","assignments":{"temperature":"hot"}},{"id":"tea","phrase":"tea","assignments":{"drink":"tea"}},{"id":"cooled","phrase":"cooled","assignments":{"temperature":"cold"}}]},"concepts":[{"id":"iced-tea","label":"cup of iced tea","attributes":{"drink":"tea","temperature":"cold"}}]}"#.into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/value/concept/id").and_then(Node::as_str),
                    Some("iced-tea")
                );
            },
        ),
        fx(
            "plan_replacement",
            |_| {
                r#"{"source":"hello world","needle":"wurld","replacement":"earth","max_edits":1}"#
                    .into()
            },
            |node| {
                assert_eq!(
                    node.pointer("/plans/0/edits/0/start").unwrap().to_json(),
                    "6"
                );
            },
        ),
        fx(
            "plan_diff",
            |_| {
                r#"{"source":"one\ntwo\n","patch":"--- a/example\n+++ b/example\n@@ -1,2 +1,2 @@\n one\n-two\n+three\n"}"#
                    .into()
            },
            |node| assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved")),
        ),
        fx(
            "inspect_code",
            |_| r#"{"source":"fn f() {}","language":"rust"}"#.into(),
            |node| {
                assert_eq!(node.pointer("/value/valid").unwrap().to_json(), "true");
                assert!(node.pointer("/value/allocation/retained_bytes").is_some());
            },
        ),
        fx(
            "read_file",
            |root| {
                fs::write(root.join("hello.txt"), "hi\n").unwrap();
                r#"{"path":"hello.txt"}"#.into()
            },
            |node| assert_eq!(node.get("source").and_then(Node::as_str), Some("hi\n")),
        ),
        fx(
            "apply_edit",
            |_| {
                let digest = Digest::of(b"tea");
                format!(
                    r#"{{"source":"tea","plan":{{"source":"{digest}","edits":[{{"start":0,"end":3,"replacement":"coffee"}}]}}}}"#
                )
            },
            |node| {
                assert_eq!(
                    node.pointer("/value/text").and_then(Node::as_str),
                    Some("coffee")
                );
            },
        ),
        fx(
            "apply_file",
            |root| {
                fs::write(root.join("target.txt"), "tea\n").unwrap();
                let digest = Digest::of(b"tea\n");
                format!(
                    r#"{{"path":"target.txt","expected_digest":"{digest}","replacement":"coffee\n"}}"#
                )
            },
            |node| {
                assert_eq!(
                    node.pointer("/commit/path").and_then(Node::as_str),
                    Some("target.txt")
                );
            },
        ),
        fx(
            "undo_file",
            |root| {
                fs::write(root.join("undo.txt"), "tea\n").unwrap();
                let store = Store::open(root, 16 * 1024 * 1024).unwrap();
                let snapshot = store.read("undo.txt").unwrap();
                let commit = store.commit(&snapshot, "coffee\n").unwrap();
                format!(r#"{{"transaction":"{}"}}"#, commit.id)
            },
            |node| assert_eq!(node.get("path").and_then(Node::as_str), Some("undo.txt")),
        ),
        fx(
            "recover_files",
            |root| {
                fs::write(root.join("r.txt"), "a\n").unwrap();
                let store = Store::open(root, 16 * 1024 * 1024).unwrap();
                let snapshot = store.read("r.txt").unwrap();
                store.commit(&snapshot, "b\n").unwrap();
                "{}".into()
            },
            |node| {
                let Value::Array(entries) = &node.value else {
                    panic!("expected an array")
                };
                assert!(
                    entries
                        .iter()
                        .any(|entry| entry.get("kind").and_then(Node::as_str) == Some("finalized"))
                );
            },
        ),
        fx(
            "commit_files",
            |root| {
                fs::write(root.join("c1.txt"), "one\n").unwrap();
                fs::write(root.join("c2.txt"), "two\n").unwrap();
                let d1 = Digest::of(b"one\n");
                let d2 = Digest::of(b"two\n");
                format!(
                    r#"{{"files":[{{"path":"c1.txt","expected_digest":"{d1}","replacement":"ONE\n"}},{{"path":"c2.txt","expected_digest":"{d2}","replacement":"TWO\n"}}]}}"#
                )
            },
            |node| {
                let Value::Array(entries) = &node.value else {
                    panic!("expected an array")
                };
                assert_eq!(entries.len(), 2);
                assert!(
                    entries.iter().all(
                        |entry| entry.get("status").and_then(Node::as_str) == Some("committed")
                    )
                );
            },
        ),
        fx(
            "prepare_helper",
            |_| {
                r#"{"provider":"anthropic_messages","model":"test-model","input":"forty two","schema":{"type":"integer"},"max_output_tokens":16}"#.into()
            },
            |node| {
                assert_eq!(
                    node.get("path").and_then(Node::as_str),
                    Some("/v1/messages")
                );
                assert_ne!(node.get("pending").and_then(Node::as_str).unwrap(), "");
            },
        ),
        fx(
            "complete_helper",
            |root| {
                let tools = Tools::new(root).unwrap();
                let prepare_args = json::parse(
                    r#"{"provider":"anthropic_messages","model":"test-model","input":"forty two","schema":{"type":"integer"},"max_output_tokens":16}"#,
                )
                .unwrap();
                let prepared = tools.call("prepare_helper", &prepare_args).unwrap();
                let pending = prepared.get("pending").and_then(Node::as_str).unwrap();
                let body = r#"{"id":"msg_1","type":"message","role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"42"}]}"#;
                object([
                    ("pending", string(pending)),
                    ("status", integer(200)),
                    ("body", string(body)),
                ])
                .to_json()
            },
            |node| {
                assert_eq!(
                    node.get("status").and_then(Node::as_str),
                    Some("best_guess")
                );
                assert_eq!(node.pointer("/value").unwrap().to_json(), "42");
            },
        ),
    ]
}

fn fresh_dir(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "purrassist-protocol-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn run_cli(operation: &str, arguments: &str, root: &Path) -> Node {
    let mut cli = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .args(["call", operation, "--root"])
        .arg(root)
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
        "{operation}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    json::parse(&String::from_utf8(output.stdout).unwrap()).unwrap()
}

fn run_mcp(operation: &str, arguments: &str, root: &Path) -> Node {
    let mut mcp = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .args(["mcp", "--root"])
        .arg(root)
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
        "{operation}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let messages: Vec<_> = text
        .lines()
        .map(|line| json::parse(line).unwrap())
        .collect();
    assert_eq!(messages.len(), 2, "{operation}: {messages:?}");
    messages[1]
        .pointer("/result/structuredContent")
        .unwrap()
        .clone()
}

/// The operation registry's single dispatch path (`crates/host/src/tools/registry.rs`)
/// backs `Tools::definitions()`, the CLI `call` command, and MCP `tools/list`/
/// `tools/call`; this iterates every one of its entries, requiring a fixture
/// for each (a missing fixture fails the test outright), running it through
/// both the CLI and MCP transports against independent, identically prepared
/// root directories, and validating the shared result against that entry's
/// own registered output schema.
#[test]
fn every_registry_entry_has_a_fixture_validated_through_cli_and_mcp() {
    let definitions = Tools::definitions().unwrap();
    let Value::Array(tools) = &definitions.value else {
        panic!("definitions must be an array")
    };
    let fixtures = fixtures();
    for tool in tools {
        let name = tool.get("name").and_then(Node::as_str).unwrap();
        let output_schema = Schema::compile(&tool.get("outputSchema").unwrap().to_json())
            .unwrap_or_else(|error| panic!("{name}: registered output schema is invalid: {error}"));
        let fixture = fixtures
            .iter()
            .find(|fixture| fixture.operation == name)
            .unwrap_or_else(|| panic!("missing CLI/MCP equivalence fixture for tool {name:?}"));
        let dir_a = fresh_dir("cli");
        let dir_b = fresh_dir("mcp");
        let arguments_a = (fixture.arguments)(&dir_a);
        let arguments_b = (fixture.arguments)(&dir_b);
        assert_eq!(
            arguments_a, arguments_b,
            "{name}: fixture arguments must be deterministic across independent roots"
        );
        let direct = run_cli(name, &arguments_a, &dir_a);
        let via_mcp = run_mcp(name, &arguments_b, &dir_b);
        assert_eq!(
            direct.to_json(),
            via_mcp.to_json(),
            "{name}: CLI and MCP results diverged"
        );
        let report = output_schema.validate(&direct).unwrap();
        assert!(
            report.valid,
            "{name}: output violates its registered output schema: {:?}",
            report.violations
        );
        (fixture.check)(&direct);
        fs::remove_dir_all(&dir_a).unwrap();
        fs::remove_dir_all(&dir_b).unwrap();
    }
    assert_eq!(
        fixtures.len(),
        tools.len(),
        "every fixture must name a real registry entry"
    );
}

/// MCP `tools/list` must enumerate exactly the registry's tool names (same
/// set, independent of order).
#[test]
fn mcp_tools_list_names_equal_the_registry() {
    let root = fresh_dir("tools-list");
    let mut mcp = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .args(["mcp", "--root"])
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let frames = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test\",\"version\":\"1\"}}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
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
    let text = String::from_utf8(output.stdout).unwrap();
    let messages: Vec<_> = text
        .lines()
        .map(|line| json::parse(line).unwrap())
        .collect();
    let Value::Array(listed) = &messages[1].pointer("/result/tools").unwrap().value else {
        panic!("tools/list result must be an array")
    };
    let mut listed_names: Vec<&str> = listed
        .iter()
        .map(|tool| tool.get("name").and_then(Node::as_str).unwrap())
        .collect();
    listed_names.sort_unstable();
    let definitions = Tools::definitions().unwrap();
    let Value::Array(registered) = &definitions.value else {
        panic!("definitions must be an array")
    };
    let mut registered_names: Vec<&str> = registered
        .iter()
        .map(|tool| tool.get("name").and_then(Node::as_str).unwrap())
        .collect();
    registered_names.sort_unstable();
    assert_eq!(listed_names, registered_names);
    fs::remove_dir_all(root).unwrap();
}

/// `generate_json_witness` (Task 10's witness-generation surface) through
/// both transports for each of the three [`WitnessOutcome`][algebra outcome]
/// shapes — a witness, a proven-unsatisfiable refutation, and an explicit
/// exhaustion (here a non-progressing `$ref` cycle) — beyond the single
/// fixture `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp`
/// requires per tool: CLI and MCP must agree byte-for-byte and every result
/// must validate against the tool's own registered output schema.
#[test]
fn generate_json_witness_agrees_across_cli_and_mcp_for_every_outcome() {
    let definitions = Tools::definitions().unwrap();
    let Value::Array(tools) = &definitions.value else {
        panic!("definitions must be an array")
    };
    let tool = tools
        .iter()
        .find(|tool| tool.get("name").and_then(Node::as_str) == Some("generate_json_witness"))
        .expect("generate_json_witness must be registered");
    let output_schema = Schema::compile(&tool.get("outputSchema").unwrap().to_json()).unwrap();
    for (label, arguments, outcome) in [
        (
            "witness",
            r#"{"schema":{"type":"object","required":["a"],"properties":{"a":{"type":"integer","minimum":3}}}}"#,
            "witness",
        ),
        (
            "unsatisfiable",
            r#"{"schema":{"allOf":[{"type":"string"},{"type":"number"}]}}"#,
            "unsatisfiable",
        ),
        ("exhausted", r##"{"schema":{"$ref":"#"}}"##, "exhausted"),
    ] {
        let dir_a = fresh_dir(&format!("witness-cli-{label}"));
        let dir_b = fresh_dir(&format!("witness-mcp-{label}"));
        let direct = run_cli("generate_json_witness", arguments, &dir_a);
        let via_mcp = run_mcp("generate_json_witness", arguments, &dir_b);
        assert_eq!(
            direct.to_json(),
            via_mcp.to_json(),
            "generate_json_witness[{label}]: CLI and MCP results diverged"
        );
        assert_eq!(
            direct.get("outcome").and_then(Node::as_str),
            Some(outcome),
            "generate_json_witness[{label}]: unexpected outcome {direct:?}"
        );
        let report = output_schema.validate(&direct).unwrap();
        assert!(
            report.valid,
            "generate_json_witness[{label}]: output violates its registered output schema: {:?}",
            report.violations
        );
        fs::remove_dir_all(&dir_a).unwrap();
        fs::remove_dir_all(&dir_b).unwrap();
    }
}
