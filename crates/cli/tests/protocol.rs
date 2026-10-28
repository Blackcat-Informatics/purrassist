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
    fmt::Write as _,
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
            "project_result",
            |_| project_arguments(r#" {"orders":[1.00e2,true]} "#, ".orders[]?", 12, "true"),
            |reply| {
                assert_eq!(reply.get("status").and_then(Node::as_str), Some("resolved"));
                assert_eq!(
                    reply.pointer("/artifact/text").and_then(Node::as_str),
                    Some("1.00e2\ntrue\n")
                );
                assert_eq!(
                    reply
                        .pointer("/artifact/measurement/measured")
                        .unwrap()
                        .to_json(),
                    "12"
                );
            },
        ),
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
                r#"{"source":"Hot\r\nTea\r\n  --- \t\r\n\r\nKeep.\r\n","op":"section_move","heading":0,"position":"start","new_level":3}"#.into()
            },
            |node| {
                assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
                let evidence = node.pointer("/receipt/evidence/1").unwrap();
                assert_eq!(
                    evidence.get("rule").and_then(Node::as_str),
                    Some("markdown.section-relevel.v1")
                );
                let detail = evidence.get("explanation").and_then(Node::as_str).unwrap();
                assert!(detail.contains(
                    "heading level 2 -> 3; style setext -> ATX; original title lines: 2"
                ));
                assert!(detail.contains("Title lines are joined with spaces"));
                assert!(detail.contains("Literal trailing hash sequence escaped: false"));
                let mut updated = "Hot\r\nTea\r\n  --- \t\r\n\r\nKeep.\r\n".to_string();
                let Value::Array(edits) = &node.pointer("/value/edits").unwrap().value else {
                    panic!()
                };
                for edit in edits.iter().rev() {
                    let offset = |field| {
                        let Value::Number(number) = &edit.get(field).unwrap().value else {
                            panic!()
                        };
                        number.to_usize().unwrap()
                    };
                    updated.replace_range(
                        offset("start")..offset("end"),
                        edit.get("replacement").and_then(Node::as_str).unwrap(),
                    );
                }
                assert_eq!(updated, "### Hot Tea\r\n\r\nKeep.\r\n");
            },
        ),
        fx(
            "decode_json",
            |_| r#"{"input":"[[42]]","schema":{"type":"integer"}}"#.into(),
            |node| assert_eq!(node.pointer("/value").unwrap().to_json(), "42"),
        ),
        fx(
            "recover_tool_calls",
            |_| {
                r#"{"output":"<tool_call>\n<function=list_dir>\n<parameter=depth>\n2\n<parameter=path>\nsrc\n</parameter>\n</function>\n</tool_call>","tools":[{"type":"function","function":{"name":"list_dir","parameters":{"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}},"required":["path"]}}}],"emit":"openai"}"#.into()
            },
            |node| {
                assert_eq!(node.get("status").and_then(Node::as_str), Some("resolved"));
                assert_eq!(
                    node.pointer("/value/calls/0/arguments").unwrap().to_json(),
                    r#"{"depth":2,"path":"src"}"#
                );
                assert_eq!(
                    node.pointer("/value/emitted/target").and_then(Node::as_str),
                    Some("openai_tool_calls")
                );
            },
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

/// What an `examples/` file is for.
enum Example {
    /// A complete argument object for the tool, run as written.
    Request(&'static str),
    /// An input a request for the tool is assembled from (the temperature
    /// schema and the model reply that `decode_json` recovers against it).
    Input(&'static str),
}

/// Every file under `examples/` and the tool it is for. A file that is not
/// listed here, or a listed file that does not exist, fails
/// `examples_run_through_cli_and_mcp`; the generated tool reference lists the
/// files of each tool from this table.
const EXAMPLES: &[(&str, Example)] = &[
    ("apply-edit.json", Example::Request("apply_edit")),
    (
        "markdown-list-item.json",
        Example::Request("plan_markdown_list_item"),
    ),
    (
        "markdown-reference.json",
        Example::Request("plan_markdown_reference"),
    ),
    (
        "markdown-section.json",
        Example::Request("plan_markdown_section"),
    ),
    (
        "markdown-table-cell.json",
        Example::Request("plan_markdown_table_cell"),
    ),
    (
        "markdown-table-column.json",
        Example::Request("plan_markdown_table_column"),
    ),
    (
        "markdown-table-row.json",
        Example::Request("plan_markdown_table_row"),
    ),
    (
        "match-embeddings.json",
        Example::Request("match_embeddings"),
    ),
    ("match-strings.json", Example::Request("match_strings")),
    ("render-markdown.json", Example::Request("render_markdown")),
    ("resolve-string.json", Example::Request("resolve_string")),
    ("temperature.input.txt", Example::Input("decode_json")),
    ("temperature.schema.json", Example::Input("decode_json")),
];

fn examples_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn read_example(name: &str) -> String {
    let path = examples_directory().join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The compiled output schema of the registered tool `name`.
fn output_schema_of(name: &str) -> Schema {
    let definitions = Tools::definitions().unwrap();
    let Value::Array(tools) = &definitions.value else {
        panic!("definitions must be an array")
    };
    let tool = tools
        .iter()
        .find(|tool| tool.get("name").and_then(Node::as_str) == Some(name))
        .unwrap_or_else(|| panic!("{name}: not a registered tool"));
    Schema::compile(&tool.get("outputSchema").unwrap().to_json()).unwrap()
}

/// Run `arguments` through the CLI and MCP in fresh roots; both results must be
/// identical and valid against the tool's registered output schema.
fn run_both_transports(label: &str, tool: &str, arguments: &str) -> Node {
    let directory_cli = fresh_dir("example-cli");
    let directory_mcp = fresh_dir("example-mcp");
    let direct = run_cli(tool, arguments, &directory_cli);
    let via_mcp = run_mcp(tool, arguments, &directory_mcp);
    assert_eq!(
        direct.to_json(),
        via_mcp.to_json(),
        "{label}: CLI and MCP results diverged"
    );
    let report = output_schema_of(tool).validate(&direct).unwrap();
    assert!(
        report.valid,
        "{label}: output violates the {tool} output schema: {:?}",
        report.violations
    );
    fs::remove_dir_all(&directory_cli).unwrap();
    fs::remove_dir_all(&directory_mcp).unwrap();
    direct
}

/// Every file under `examples/` is a listed request or a listed input, and
/// every request runs: through the CLI `call` command and through MCP
/// `tools/call`, each against its own fresh root, with identical results that
/// validate against the tool's registered output schema. The temperature
/// schema and reply are assembled into a `decode_json` request.
#[test]
fn examples_run_through_cli_and_mcp() {
    let mut on_disk: Vec<String> = fs::read_dir(examples_directory())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    on_disk.sort_unstable();
    let mut listed: Vec<String> = EXAMPLES
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    listed.sort_unstable();
    assert_eq!(
        on_disk, listed,
        "every examples/ file must be listed in EXAMPLES, and every listed file must exist"
    );
    let definitions = Tools::definitions().unwrap();
    let Value::Array(tools) = &definitions.value else {
        panic!("definitions must be an array")
    };
    let registered: Vec<&str> = tools
        .iter()
        .map(|tool| tool.get("name").and_then(Node::as_str).unwrap())
        .collect();
    for (name, example) in EXAMPLES {
        let (Example::Request(tool) | Example::Input(tool)) = example;
        assert!(
            registered.contains(tool),
            "{name}: {tool} is not a registered tool"
        );
        if let Example::Request(tool) = example {
            let arguments = json::parse(&read_example(name))
                .unwrap_or_else(|error| panic!("{name}: {error}"))
                .to_json();
            run_both_transports(name, tool, &arguments);
        }
    }
    let schema = json::parse(&read_example("temperature.schema.json")).unwrap();
    let reply = read_example("temperature.input.txt");
    let arguments = object([("input", string(reply)), ("schema", schema)]).to_json();
    let decoded = run_both_transports("temperature", "decode_json", &arguments);
    assert_eq!(
        decoded.pointer("/value").unwrap().to_json(),
        r#"{"temperature":42}"#
    );
}

/// Run the executable with `arguments`, writing `stdin`; its exit code and stdout.
fn run_cli_command(arguments: &[&str], stdin: &str) -> (i32, String) {
    let mut cli = Command::new(env!("CARGO_BIN_EXE_purrassist"))
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    cli.stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = cli.wait_with_output().unwrap();
    (
        output.status.code().unwrap(),
        String::from_utf8(output.stdout).unwrap(),
    )
}

/// `purrassist validate` prints exactly the `validate_json` tool's output for
/// the same schema and instance, whether the instance is valid (exit 0) or
/// not (exit 4); the `call` command exits 0 either way.
#[test]
fn validate_command_prints_validate_json_output() {
    let directory = fresh_dir("validate-command");
    let schema = r#"{"type":"object","required":["n"],"properties":{"n":{"type":"integer","minimum":3}},"additionalProperties":false}"#;
    let schema_path = directory.join("schema.json");
    fs::write(&schema_path, schema).unwrap();
    for (instance, valid, exit) in [
        (r#"{"n": 7}"#, true, 0),
        (r#"{"n": 1, "extra": true}"#, false, 4),
    ] {
        let instance_path = directory.join("instance.json");
        fs::write(&instance_path, instance).unwrap();
        let (code, printed) = run_cli_command(
            &[
                "validate",
                "--schema",
                schema_path.to_str().unwrap(),
                "--input",
                instance_path.to_str().unwrap(),
            ],
            "",
        );
        let arguments = format!(r#"{{"schema":{schema},"instance":{instance}}}"#);
        let (call_code, tool_output) = run_cli_command(
            &[
                "call",
                "validate_json",
                "--root",
                directory.to_str().unwrap(),
            ],
            &arguments,
        );
        assert_eq!(call_code, 0, "call validate_json exits 0 for {instance}");
        assert_eq!(
            printed, tool_output,
            "validate must print the validate_json output for {instance}"
        );
        assert_eq!(code, exit, "validate exit code for {instance}");
        let node = json::parse(&printed).unwrap();
        assert_eq!(
            node.get("valid").map(|node| &node.value),
            Some(&Value::Bool(valid))
        );
    }
    fs::remove_dir_all(directory).unwrap();
}

const TOOLS_REFERENCE_BEGIN: &str = "<!-- tools-reference:begin -->\n";
const TOOLS_REFERENCE_END: &str = "<!-- tools-reference:end -->\n";

fn tools_document() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/TOOLS.md")
}

/// A short, deterministic description of one JSON Schema node.
fn describe_schema(schema: &Node) -> String {
    match &schema.value {
        Value::Bool(true) => return "any".into(),
        Value::Bool(false) => return "never".into(),
        _ => {}
    }
    match schema.get("type").map(|node| &node.value) {
        Some(Value::String(kind)) if kind == "array" => {
            return schema
                .get("items")
                .and_then(|items| items.get("type"))
                .and_then(Node::as_str)
                .map_or_else(|| "array".into(), |item| format!("array of {item}"));
        }
        Some(Value::String(kind)) => return kind.clone(),
        Some(Value::Array(kinds)) => {
            return kinds
                .iter()
                .filter_map(Node::as_str)
                .collect::<Vec<_>>()
                .join(" or ");
        }
        _ => {}
    }
    if let Some(Node {
        value: Value::Array(values),
        ..
    }) = schema.get("enum")
    {
        let values: Vec<String> = values.iter().map(Node::to_json).collect();
        return format!("one of {}", values.join(", "));
    }
    if let Some(value) = schema.get("const") {
        return format!("always {}", value.to_json());
    }
    if schema.get("oneOf").is_some() || schema.get("anyOf").is_some() {
        return "one of several shapes".into();
    }
    if schema.get("$ref").is_some() {
        return "a shared definition".into();
    }
    "any".into()
}

/// The top-level members of `properties`, as `name: type[, required]` lines
/// at `indent`; "none" when the schema declares none.
fn property_lines(schema: &Node, indent: &str) -> String {
    let required: Vec<&str> = match schema.get("required").map(|node| &node.value) {
        Some(Value::Array(names)) => names.iter().filter_map(Node::as_str).collect(),
        _ => Vec::new(),
    };
    let Some(Node {
        value: Value::Object(members),
        ..
    }) = schema.get("properties")
    else {
        return format!("{indent}- none\n");
    };
    let mut lines = String::new();
    for member in members {
        let needed = if required.contains(&member.key.as_str()) {
            ", required"
        } else {
            ""
        };
        writeln!(
            lines,
            "{indent}- `{}`: {}{needed}",
            member.key,
            describe_schema(&member.value)
        )
        .unwrap();
    }
    if members.is_empty() {
        writeln!(lines, "{indent}- none").unwrap();
    }
    lines
}

/// The generated reference: one entry per registered tool, in registry order.
fn render_tools_reference() -> String {
    let definitions = Tools::definitions().unwrap();
    let Value::Array(tools) = &definitions.value else {
        panic!("definitions must be an array")
    };
    let mut block = String::from(concat!(
        "Generated from `Tools::definitions()` by the test `tools_reference_matches_registry`; ",
        "do not edit by hand. Regenerate with\n",
        "`PURRASSIST_BLESS=1 cargo test -p purrassist-cli --test protocol tools_reference_matches_registry`.\n",
        "`required` is the schema's top-level `required` list; requirements that depend on another ",
        "argument (`oneOf`) are in the tool's input schema, which `tools/list` returns.\n\n",
    ));
    for tool in tools {
        let name = tool.get("name").and_then(Node::as_str).unwrap();
        let description = tool.get("description").and_then(Node::as_str).unwrap();
        let input = tool.get("inputSchema").unwrap();
        let output = tool.get("outputSchema").unwrap();
        let writes = tool
            .pointer("/annotations/readOnlyHint")
            .is_some_and(|hint| hint.value == Value::Bool(false));
        writeln!(block, "- **`{name}`**: {description}").unwrap();
        writeln!(block, "  - MCP: `tools/call` with name `{name}`").unwrap();
        writeln!(
            block,
            "  - CLI: `purrassist call {name} --root DIR < args.json`"
        )
        .unwrap();
        writeln!(
            block,
            "  - Writes files under the root: {}",
            if writes { "yes" } else { "no" }
        )
        .unwrap();
        block.push_str("  - Input:\n");
        block.push_str(&property_lines(input, "    "));
        if output.get("properties").is_none()
            && output.get("type").and_then(Node::as_str) == Some("array")
        {
            let items = output.get("items").unwrap();
            block.push_str("  - Output: an array of objects with:\n");
            block.push_str(&property_lines(items, "    "));
        } else {
            block.push_str("  - Output:\n");
            block.push_str(&property_lines(output, "    "));
        }
        let files: Vec<&str> = EXAMPLES
            .iter()
            .filter(|(_, example)| {
                let (Example::Request(tool) | Example::Input(tool)) = example;
                *tool == name
            })
            .map(|(file, _)| *file)
            .collect();
        if !files.is_empty() {
            let links: Vec<String> = files
                .iter()
                .map(|file| format!("[`examples/{file}`](../examples/{file})"))
                .collect();
            let label = if files.len() == 1 {
                "Example"
            } else {
                "Examples"
            };
            writeln!(block, "  - {label}: {}", links.join(", ")).unwrap();
        }
    }
    block
}

/// The first lines at which `actual` differs from `expected`, as a diff.
fn line_diff(expected: &str, actual: &str) -> String {
    let (expected, actual): (Vec<&str>, Vec<&str>) =
        (expected.lines().collect(), actual.lines().collect());
    let mut diff = String::new();
    let mut shown = 0;
    for index in 0..expected.len().max(actual.len()) {
        let (wanted, found) = (expected.get(index), actual.get(index));
        if wanted == found {
            continue;
        }
        shown += 1;
        if shown > 20 {
            diff.push_str("... more differences follow\n");
            break;
        }
        writeln!(diff, "line {}:", index + 1).unwrap();
        if let Some(line) = found {
            writeln!(diff, "- docs/TOOLS.md: {line}").unwrap();
        }
        if let Some(line) = wanted {
            writeln!(diff, "+ registry:      {line}").unwrap();
        }
    }
    diff
}

/// The block of `docs/TOOLS.md` between the `tools-reference` markers is the
/// registry rendered in registry order (name, description, input properties
/// with type and required, output properties, the MCP and CLI forms, and the
/// example files). `PURRASSIST_BLESS=1` rewrites the block (and panics when
/// `CI` is set, since blessing must never run unattended).
#[test]
fn tools_reference_matches_registry() {
    let bless = std::env::var("PURRASSIST_BLESS").as_deref() == Ok("1");
    assert!(
        !(bless && std::env::var("CI").is_ok()),
        "PURRASSIST_BLESS=1 must never run with CI set"
    );
    let path = tools_document();
    let document = fs::read_to_string(&path).unwrap();
    assert_eq!(
        document.matches(TOOLS_REFERENCE_BEGIN).count(),
        1,
        "docs/TOOLS.md needs exactly one {} line",
        TOOLS_REFERENCE_BEGIN.trim_end()
    );
    assert_eq!(
        document.matches(TOOLS_REFERENCE_END).count(),
        1,
        "docs/TOOLS.md needs exactly one {} line",
        TOOLS_REFERENCE_END.trim_end()
    );
    let start = document.find(TOOLS_REFERENCE_BEGIN).unwrap() + TOOLS_REFERENCE_BEGIN.len();
    let end = document.find(TOOLS_REFERENCE_END).unwrap();
    assert!(
        start <= end,
        "the tools-reference end marker precedes its begin marker"
    );
    let expected = render_tools_reference();
    if bless {
        let blessed = format!("{}{expected}{}", &document[..start], &document[end..]);
        fs::write(&path, blessed).unwrap();
        return;
    }
    let actual = &document[start..end];
    assert!(
        actual == expected,
        "the generated block of docs/TOOLS.md differs from the registry; regenerate it with \
         PURRASSIST_BLESS=1 cargo test -p purrassist-cli --test protocol tools_reference_matches_registry\n{}",
        line_diff(&expected, actual)
    );
}

fn project_arguments(raw: &str, mask: &str, limit: u64, schema: &str) -> String {
    format!(
        r#"{{"result_json":{},"output_schema":{schema},"format":"purrassist-toolgraph-v2","context":{{"outputProjectionJq":{},"maxOutputBytes":{limit},"outputEnforcement":"exact-bytes","historyRetentionTurns":0,"intentTags":[],"loadingStrategy":"always"}}}}"#,
        json::quote(raw),
        json::quote(mask)
    )
}

/// Artifact boundaries and typed processing refusals survive both real transports.
#[test]
fn project_result_cli_mcp_keep_checked_bytes_and_structured_refusals() {
    let output = output_schema_of("project_result");
    for (raw, mask, limit, schema, status, kind) in [
        (r#"["é\n",true]"#, ".[]", 12, "true", "resolved", None),
        (
            r#"["é\n",true]"#,
            ".[]",
            11,
            "true",
            "unresolved",
            Some("output_limit"),
        ),
        (
            r#"{"hidden":false,"a":1}"#,
            ".a",
            64,
            r#"{"properties":{"hidden":{"const":true}}}"#,
            "unresolved",
            Some("schema_rejected"),
        ),
        (
            r#"{"a":1,"a":2}"#,
            ".a",
            64,
            "true",
            "unresolved",
            Some("projection_ambiguous_member"),
        ),
        ("{bad", ".a", 64, "true", "unresolved", Some("json_syntax")),
    ] {
        let args = project_arguments(raw, mask, limit, schema);
        let root = fresh_dir("project-result-parity");
        let cli = run_cli("project_result", &args, &root);
        let mcp = run_mcp("project_result", &args, &root);
        assert_eq!(cli.to_json(), mcp.to_json());
        assert!(output.validate(&cli).unwrap().valid, "{cli:?}");
        assert_eq!(cli.get("status").and_then(Node::as_str), Some(status));
        assert_eq!(cli.get("original_json").and_then(Node::as_str), Some(raw));
        if let Some(kind) = kind {
            assert_eq!(
                cli.pointer("/error/kind").and_then(Node::as_str),
                Some(kind)
            );
            assert!(matches!(cli.get("artifact").unwrap().value, Value::Null));
        } else {
            assert_eq!(
                cli.pointer("/artifact/text").and_then(Node::as_str),
                Some("\"é\\n\"\ntrue\n")
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
