// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared document tools preserve bytes, structural selections, and exact undo.
use purrassist::{
    Budget, Outcome,
    digest::Digest,
    edits::{EditPlan, TextEdit},
    json::{self, Node, Value},
    wire::{object, string},
};
use purrassist_host::tools::Tools;

fn call(tools: &Tools, name: &str, source: &str, extra: &str) -> Node {
    let Value::Object(mut fields) = json::parse(extra).unwrap().value else {
        panic!("object")
    };
    let Value::Object(base) = object([("source", string(source))]).value else {
        panic!("object")
    };
    fields.extend(base);
    tools.call(name, &Node::new(Value::Object(fields))).unwrap()
}
fn offset(node: &Node) -> usize {
    let Value::Number(number) = &node.value else {
        panic!("number")
    };
    number.to_usize().unwrap()
}
fn apply(result: &Node, source: &str) -> String {
    assert_eq!(
        result.pointer("/status").unwrap().as_str(),
        Some("resolved")
    );
    assert_eq!(
        result.pointer("/receipt/sources/0/text").unwrap().as_str(),
        Some(source)
    );
    assert_eq!(
        result.pointer("/value/source").unwrap().as_str(),
        Some(Digest::of(source.as_bytes()).to_string().as_str())
    );
    let Value::Array(edits) = &result.pointer("/value/edits").unwrap().value else {
        panic!("edits")
    };
    let plan = EditPlan {
        source: Digest::of(source.as_bytes()),
        edits: edits
            .iter()
            .map(|edit| TextEdit {
                range: offset(edit.get("start").unwrap())..offset(edit.get("end").unwrap()),
                replacement: edit.get("replacement").unwrap().as_str().unwrap().into(),
            })
            .collect(),
    };
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply")
    };
    let Outcome::Resolved(undone) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo")
    };
    assert_eq!(undone.text, source);
    assert_eq!(
        plan.apply(&format!("{source}changed"), Budget::default())
            .outcome,
        Outcome::Unresolved
    );
    applied.text
}
#[test]
fn document_tools_preserve_crlf_unicode_context_and_edit_identity() {
    let root =
        std::env::temp_dir().join(format!("purrassist-markdown-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let source = "# Tea\r\n- [ ] café\r\n# Other\r\nKeep\r\n";
    let index = call(&tools, "inspect_markdown", source, "{}");
    assert_eq!(
        index.pointer("/value/blocks/1/kind/type").unwrap().as_str(),
        Some("list_item")
    );
    assert_eq!(
        index
            .pointer("/value/blocks/1/children/0/kind/type")
            .unwrap()
            .as_str(),
        Some("paragraph")
    );
    let context = call(
        &tools,
        "markdown_context",
        source,
        r#"{"start":13,"end":18}"#,
    );
    assert_eq!(
        context.pointer("/value/text").unwrap().as_str(),
        Some("- [ ] café\r\n")
    );
    assert_eq!(
        offset(context.pointer("/value/sections/0/heading").unwrap()),
        0
    );
    let task = call(
        &tools,
        "plan_markdown_task",
        source,
        r#"{"block":1,"checked":true}"#,
    );
    assert_eq!(apply(&task, source), source.replacen("[ ]", "[x]", 1));
    let section = call(
        &tools,
        "plan_markdown_section",
        source,
        r#"{"heading":0,"body":"Served cold.\r\n"}"#,
    );
    assert_eq!(
        apply(&section, source),
        "# Tea\r\nServed cold.\r\n# Other\r\nKeep\r\n"
    );
    let unchanged = call(
        &tools,
        "plan_markdown_task",
        source,
        r#"{"block":1,"checked":false}"#,
    );
    assert_eq!(apply(&unchanged, source), source);
    for (name, args) in [
        ("markdown_context", r#"{"start":16,"end":17}"#),
        ("markdown_context", r#"{"start":10,"end":4}"#),
        ("plan_markdown_section", r#"{"heading":1,"body":"no"}"#),
        ("plan_markdown_task", r#"{"block":0,"checked":true}"#),
    ] {
        let failed = call(&tools, name, source, args);
        assert_eq!(
            failed.pointer("/status").unwrap().as_str(),
            Some("unresolved")
        );
        assert_eq!(
            failed
                .pointer("/receipt/termination/kind")
                .unwrap()
                .as_str(),
            Some("invalid_input")
        );
    }
    let exhausted = call(
        &tools,
        "inspect_markdown",
        &"x".repeat(50_000),
        r#"{"effort":"exact"}"#,
    );
    assert_eq!(
        exhausted.pointer("/status").unwrap().as_str(),
        Some("unresolved")
    );
    assert_eq!(
        exhausted
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("budget_exceeded")
    );
    std::fs::remove_dir_all(root).unwrap();
}
