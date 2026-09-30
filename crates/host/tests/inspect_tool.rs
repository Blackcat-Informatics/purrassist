// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `inspect_code`/`apply_edit` through the tool boundary: budget exhaustion
//! (measured `ScratchBytes` via tree-sitter, not an estimate), cooperative
//! cancellation, and a two-alternative JSX rename ambiguity.
use purrassist::{
    CancellationToken,
    digest::Digest,
    json::Node,
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
            "purrassist-inspect-tool-{}-{}",
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

#[test]
fn inspect_code_reports_measured_scratch_budget_exhaustion() {
    let root = Root::new();
    let tools = root.tools();
    // Many short, densely tokenized statements: tree-sitter's per-node
    // overhead makes the retained/peak parse memory a large multiple of the
    // raw source bytes, so this source (well inside the 16 MiB `InputBytes`
    // cap, and well inside "maximum" effort's 50M `Work` units) still drives
    // the fixed 64 MiB `ScratchBytes` ceiling every effort preset shares past
    // its limit during parsing itself, before any traversal `Work` charge.
    let source = "var a=1;".repeat(1_200_000);
    assert!(source.len() < 16 * 1024 * 1024, "source exceeds InputBytes");
    let args = object([
        ("source", string(&source)),
        ("language", string("javascript")),
        ("effort", string("maximum")),
    ]);
    let result = tools.call("inspect_code", &args).unwrap();
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
    let detail = result
        .pointer("/receipt/termination/detail")
        .and_then(Node::as_str)
        .unwrap();
    assert!(
        detail.contains("ScratchBytes"),
        "expected a ScratchBytes exhaustion, got {detail:?}"
    );
}

#[test]
fn inspect_code_observes_cancellation() {
    let root = Root::new();
    let tools = root.tools();
    let token = CancellationToken::new();
    token.cancel();
    let args = object([
        ("source", string("fn f() {}")),
        ("language", string("rust")),
    ]);
    assert_eq!(
        tools.call_with_cancellation("inspect_code", &args, &token),
        Err("cancelled".into())
    );
}

#[test]
fn inspect_code_reports_jsx_mismatch_rename_options_and_allocation() {
    let root = Root::new();
    let tools = root.tools();
    let args = object([
        ("source", string("const x = <div></span>;\n")),
        ("language", string("tsx")),
    ]);
    let result = tools.call("inspect_code", &args).unwrap();
    assert_eq!(
        result.get("status").and_then(Node::as_str),
        Some("resolved")
    );
    assert_eq!(result.pointer("/value/valid").unwrap().to_json(), "false");
    let issue = result
        .pointer("/value/issues/0")
        .expect("a jsx.mismatched-tags.v1 issue");
    assert_eq!(
        issue.get("kind").and_then(Node::as_str),
        Some("jsx.mismatched-tags.v1")
    );
    assert!(
        issue
            .get("rename")
            .unwrap()
            .pointer("/opening_name")
            .is_some()
    );
    assert!(
        issue
            .get("rename")
            .unwrap()
            .pointer("/closing_name")
            .is_some()
    );
    let allocation = result
        .pointer("/value/allocation")
        .expect("allocation report");
    let purrassist::json::Value::Number(retained) =
        &allocation.get("retained_bytes").unwrap().value
    else {
        panic!("retained_bytes should be numeric")
    };
    assert!(retained.to_usize().unwrap() > 0);
    let purrassist::json::Value::Number(calls) = &allocation.get("allocation_calls").unwrap().value
    else {
        panic!("allocation_calls should be numeric")
    };
    assert!(calls.to_usize().unwrap() > 0);
}

#[test]
fn apply_edit_reaches_a_two_alternative_jsx_rename_ambiguity_through_the_tool_boundary() {
    let root = Root::new();
    let tools = root.tools();
    let source = "const x = <div></div>;\n";
    let broken = "const x = <div></span>;\n";
    let digest = Digest::of(source.as_bytes());
    let args = object([
        ("source", string(source)),
        (
            "plan",
            object([
                ("source", string(digest.to_string())),
                (
                    "edits",
                    array([object([
                        ("start", integer(0)),
                        ("end", integer(u64::try_from(source.len()).unwrap())),
                        ("replacement", string(broken)),
                    ])]),
                ),
            ]),
        ),
        ("language", string("tsx")),
    ]);
    let result = tools.call("apply_edit", &args).unwrap();
    assert_eq!(
        result.get("status").and_then(Node::as_str),
        Some("ambiguous")
    );
    let Some(purrassist::json::Value::Array(alternatives)) =
        result.get("alternatives").map(|node| &node.value)
    else {
        panic!("expected an alternatives array")
    };
    assert_eq!(alternatives.len(), 2);
    let Some(purrassist::json::Value::Array(ledgers)) = result
        .pointer("/receipt/alternatives")
        .map(|node| &node.value)
    else {
        panic!("expected receipt alternatives")
    };
    assert_eq!(ledgers.len(), 2);
    let ambiguity = result
        .pointer("/receipt/ambiguity")
        .expect("an ambiguity annotation");
    assert_eq!(ambiguity.get("derivations").unwrap().to_json(), "2");
    let texts: Vec<&str> = alternatives
        .iter()
        .map(|value| value.pointer("/text").and_then(Node::as_str).unwrap())
        .collect();
    assert!(texts.contains(&"const x = <div></div>;\n"));
    assert!(texts.contains(&"const x = <span></span>;\n"));
}
