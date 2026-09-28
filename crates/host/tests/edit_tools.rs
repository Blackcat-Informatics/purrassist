// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-bound plans flow through planning, application, syntax validation, and undo.
use purrassist::{
    CancellationToken,
    digest::Digest,
    json::{self, Node},
    wire::{array, integer, object, string},
};
use purrassist_host::tools::Tools;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "purrassist-edit-tools-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn tools(&self) -> Tools {
        Tools::new(&self.0).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn plan(source: &str, edits: &[(u64, u64, &str)]) -> Node {
    object([
        ("source", string(Digest::of(source.as_bytes()).to_string())),
        (
            "edits",
            array(edits.iter().map(|(start, end, replacement)| {
                object([
                    ("start", integer(*start)),
                    ("end", integer(*end)),
                    ("replacement", string(*replacement)),
                ])
            })),
        ),
    ])
}
fn apply(tools: &Tools, source: &str, plan: Node) -> Node {
    tools
        .call(
            "apply_edit",
            &object([("source", string(source)), ("plan", plan)]),
        )
        .unwrap()
}

#[test]
fn nested_markdown_plan_applies_and_its_wire_inverse_restores_exact_bytes() {
    let root = Root::new();
    let tools = root.tools();
    let source = "> - [ ] café\r\n>   suite";
    let proposal = tools
        .call(
            "plan_markdown_task",
            &object([
                ("source", string(source)),
                ("path", array([integer(0), integer(0)])),
                ("checked", Node::new(json::Value::Bool(true))),
            ]),
        )
        .unwrap();
    let result = apply(&tools, source, proposal.get("value").unwrap().clone());
    assert_eq!(result.get("status").unwrap().as_str(), Some("resolved"));
    let output = result.pointer("/value/text").unwrap().as_str().unwrap();
    assert_eq!(output, "> - [x] café\r\n>   suite");
    let inverse = result.pointer("/value/inverse").unwrap().clone();
    assert_eq!(
        apply(&tools, output, inverse)
            .pointer("/value/text")
            .unwrap()
            .as_str(),
        Some(source)
    );
    assert_eq!(
        result.pointer("/receipt/sources/0/text").unwrap().as_str(),
        Some(source)
    );
}

#[test]
fn stale_overlapping_and_non_scalar_ranges_never_return_success() {
    let root = Root::new();
    let tools = root.tools();
    let source = "café";
    for proposal in [
        plan("other", &[]),
        plan(source, &[(0, 2, "x"), (1, 3, "y")]),
        plan(source, &[(3, 4, "e")]),
        plan(source, &[(0, 100, "x")]),
    ] {
        let result = apply(&tools, source, proposal);
        assert_eq!(result.get("status").unwrap().as_str(), Some("unresolved"));
        assert_eq!(
            result
                .pointer("/receipt/termination/kind")
                .unwrap()
                .as_str(),
            Some("invalid_input")
        );
        assert_eq!(
            result.pointer("/receipt/sources/0/text").unwrap().as_str(),
            Some(source)
        );
    }
    let result = apply(&tools, source, plan(source, &[(3, 5, "e")]));
    assert_eq!(
        result.pointer("/value/text").unwrap().as_str(),
        Some("cafe")
    );
}

#[test]
fn buffer_application_validates_supported_code_without_writing_files() {
    let root = Root::new();
    let tools = root.tools();
    let source = "def tea():\n    return 1\n";
    let args = object([
        ("source", string(source)),
        ("language", string("python")),
        ("plan", plan(source, &[(4, 7, "coffee")])),
    ]);
    let result = tools.call("apply_edit", &args).unwrap();
    assert_eq!(
        result.pointer("/value/text").unwrap().as_str(),
        Some("def coffee():\n    return 1\n")
    );
    assert_eq!(
        result.pointer("/receipt/operation").unwrap().as_str(),
        Some("syntax.apply.v1")
    );
    let invalid = object([
        ("source", string(source)),
        ("language", string("python")),
        (
            "plan",
            plan(
                source,
                &[(0, u64::try_from(source.len()).unwrap(), "def !!!")],
            ),
        ),
    ]);
    assert_eq!(
        tools
            .call("apply_edit", &invalid)
            .unwrap()
            .get("status")
            .unwrap()
            .as_str(),
        Some("unresolved")
    );
    assert!(!root.0.join("tea.py").exists());
}

#[test]
fn file_plan_commit_is_conditional_and_journal_undo_restores_original() {
    let root = Root::new();
    let tools = root.tools();
    let source = "alpha\r\nbeta\r\nend";
    std::fs::write(root.0.join("notes.txt"), source).unwrap();
    let args = object([
        ("path", string("notes.txt")),
        (
            "expected_digest",
            string(Digest::of(source.as_bytes()).to_string()),
        ),
        ("plan", plan(source, &[(0, 5, "first"), (7, 11, "second")])),
    ]);
    let result = tools.call("apply_file", &args).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.0.join("notes.txt")).unwrap(),
        "first\r\nsecond\r\nend"
    );
    assert!(tools.call("apply_file", &args).is_err());
    tools
        .call(
            "undo_file",
            &object([(
                "transaction",
                result.pointer("/commit/transaction").unwrap().clone(),
            )]),
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.0.join("notes.txt")).unwrap(),
        source
    );
    for proposal in [
        plan("stale", &[]),
        plan(source, &[(0, 8, "x"), (2, 9, "y")]),
    ] {
        let invalid = object([
            ("path", string("notes.txt")),
            (
                "expected_digest",
                string(Digest::of(source.as_bytes()).to_string()),
            ),
            ("plan", proposal),
        ]);
        assert!(tools.call("apply_file", &invalid).is_err());
        assert_eq!(
            std::fs::read_to_string(root.0.join("notes.txt")).unwrap(),
            source
        );
    }
    let token = CancellationToken::new();
    token.cancel();
    assert!(
        tools
            .call_with_cancellation("apply_file", &args, &token)
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(root.0.join("notes.txt")).unwrap(),
        source
    );
}

#[test]
fn file_plan_retains_extension_validation_and_rejects_competing_inputs() {
    let root = Root::new();
    let tools = root.tools();
    let source = "def tea():\n    return 1\n";
    std::fs::write(root.0.join("tea.py"), source).unwrap();
    let expected = Digest::of(source.as_bytes()).to_string();
    let invalid = object([
        ("path", string("tea.py")),
        ("expected_digest", string(&expected)),
        (
            "plan",
            plan(
                source,
                &[(0, u64::try_from(source.len()).unwrap(), "def !!!")],
            ),
        ),
    ]);
    assert!(tools.call("apply_file", &invalid).is_err());
    assert_eq!(
        std::fs::read_to_string(root.0.join("tea.py")).unwrap(),
        source
    );
    let both = object([
        ("path", string("tea.py")),
        ("expected_digest", string(&expected)),
        ("plan", plan(source, &[])),
        ("replacement", string("")),
    ]);
    assert!(tools.call("apply_file", &both).is_err());
    let neither = object([
        ("path", string("tea.py")),
        ("expected_digest", string(expected)),
    ]);
    assert!(tools.call("apply_file", &neither).is_err());
}

#[test]
fn application_reports_budget_exhaustion_and_rejects_oversized_offsets() {
    let root = Root::new();
    let tools = root.tools();
    let source = "x".repeat(100_001);
    let args = object([
        ("source", string(&source)),
        ("plan", plan(&source, &[])),
        ("effort", string("exact")),
    ]);
    let stopped = tools.call("apply_edit", &args).unwrap();
    assert_eq!(stopped.get("status").unwrap().as_str(), Some("unresolved"));
    assert_eq!(
        stopped
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("budget_exceeded")
    );
    let digest = Digest::of(b"tea");
    let args = json::parse(&format!(r#"{{"source":"tea","plan":{{"source":"{digest}","edits":[{{"start":1e100,"end":1e100,"replacement":""}}]}}}}"#)).unwrap();
    let invalid = tools.call("apply_edit", &args).unwrap();
    assert_eq!(invalid.get("status").unwrap().as_str(), Some("unresolved"));
    assert_eq!(
        invalid
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("invalid_input")
    );
}
