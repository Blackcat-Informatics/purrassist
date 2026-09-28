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
#[test]
fn reference_inspection_selects_duplicates_and_edits_decoded_fields() {
    let root =
        std::env::temp_dir().join(format!("purrassist-reference-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let source = "> [Tea]: /hot 'Warm'\r\n> [tea]: /shadow\r\n";
    let inspection = call(&tools, "inspect_markdown", source, "{}");
    assert_eq!(
        inspection
            .pointer("/value/references/1/path")
            .unwrap()
            .to_json(),
        "[0,0]"
    );
    assert_eq!(
        inspection
            .pointer("/value/references/1/occurrence")
            .unwrap()
            .to_json(),
        "1"
    );
    assert_eq!(
        inspection
            .pointer("/value/references/0/destination")
            .unwrap()
            .as_str(),
        Some("/hot")
    );
    let result = call(
        &tools,
        "plan_markdown_reference",
        source,
        r#"{"path":[0,0],"occurrence":1,"destination":"/iced","title":"Cold"}"#,
    );
    let updated = apply(&result, source);
    assert_eq!(
        updated,
        "> [Tea]: /hot 'Warm'\r\n> [tea]: <\\/iced> \"Cold\"\r\n"
    );
    assert_eq!(
        result
            .pointer("/receipt/validations/0/validator")
            .unwrap()
            .as_str(),
        Some("markdown.reference-definition.v1")
    );
    assert!(
        tools
            .call(
                "plan_markdown_reference",
                &json::parse(r#"{"source":"x","path":[0],"occurrence":0,"destination":"/new"}"#)
                    .unwrap()
            )
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn table_column_tool_preserves_cells_and_validates_action_fields() {
    let root = std::env::temp_dir().join(format!(
        "purrassist-table-column-tool-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let source = "> | A |\r\n> | --- |\r\n> | café |\r\n";
    let result = call(
        &tools,
        "plan_markdown_table_column",
        source,
        r#"{"path":[0,0],"action":"insert","index":1,"header":"B","alignment":"right","cells":["cold"]}"#,
    );
    let inserted = apply(&result, source);
    assert_eq!(
        inserted,
        "> | A | B |\r\n> | --- | ---: |\r\n> | café | cold |\r\n"
    );
    assert_eq!(
        result
            .pointer("/receipt/validations/0/validator")
            .unwrap()
            .as_str(),
        Some("markdown.table-structure.v1")
    );
    let removed = call(
        &tools,
        "plan_markdown_table_column",
        &inserted,
        r#"{"path":[0,0],"action":"remove","index":1}"#,
    );
    assert_eq!(apply(&removed, &inserted), source);
    let aligned = call(
        &tools,
        "plan_markdown_table_column",
        source,
        r#"{"path":[0,0],"action":"align","index":0,"alignment":"center"}"#,
    );
    assert_eq!(apply(&aligned, source), source.replace("---", ":---:"));
    for extra in [
        r#"{"source":"x","path":[0],"action":"remove","index":0,"alignment":"left"}"#,
        r#"{"source":"x","path":[0],"action":"align","index":0}"#,
        r#"{"source":"x","path":[0],"action":"insert","index":0,"header":"B","cells":[]}"#,
    ] {
        assert!(
            tools
                .call("plan_markdown_table_column", &json::parse(extra).unwrap())
                .is_err()
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn table_row_tool_has_validation_and_exact_undo() {
    let root =
        std::env::temp_dir().join(format!("purrassist-table-row-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let source = "> | A |\r\n> | --- |\r\n> | tea |\r\n";
    let result = call(
        &tools,
        "plan_markdown_table_row",
        source,
        r#"{"path":[0,0],"action":"insert","index":1,"cells":["café"]}"#,
    );
    assert_eq!(apply(&result, source), format!("{source}> | café |\r\n"));
    assert_eq!(
        result
            .pointer("/receipt/validations/0/validator")
            .unwrap()
            .as_str(),
        Some("markdown.table-structure.v1")
    );
    let removed = call(
        &tools,
        "plan_markdown_table_row",
        source,
        r#"{"path":[0,0],"action":"remove","index":0}"#,
    );
    assert_eq!(apply(&removed, source), "> | A |\r\n> | --- |\r\n");
    for invalid in [
        r#"{"source":"x","path":[0],"action":"insert","index":0}"#,
        r#"{"source":"x","path":[0],"action":"remove","index":0,"cells":["a"]}"#,
    ] {
        assert!(
            tools
                .call("plan_markdown_table_row", &json::parse(invalid).unwrap())
                .is_err()
        );
    }
    let rejected = call(
        &tools,
        "plan_markdown_table_row",
        source,
        r#"{"path":[0,0],"action":"insert","index":0,"cells":["a|b"]}"#,
    );
    assert_eq!(rejected.get("status").unwrap().as_str(), Some("unresolved"));
    assert_eq!(
        rejected.pointer("/receipt/validations").unwrap().to_json(),
        "[]"
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn nested_table_cell_tool_preserves_source_and_validates_structure() {
    let root = std::env::temp_dir().join(format!("purrassist-table-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let source = "> | A | B |\r\n> | --- | --- |\r\n> | café | keep |\r\n";
    let result = call(
        &tools,
        "plan_markdown_table_cell",
        source,
        r#"{"path":[0,0],"row":1,"column":0,"markdown":"**tea**"}"#,
    );
    assert_eq!(apply(&result, source), source.replace("café", "**tea**"));
    let invalid = call(
        &tools,
        "plan_markdown_table_cell",
        source,
        r#"{"path":[0,0],"row":1,"column":0,"markdown":"a|b"}"#,
    );
    assert_eq!(invalid.get("status").unwrap().as_str(), Some("unresolved"));
    std::fs::remove_dir_all(root).unwrap();
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
    assert_eq!(
        section
            .pointer("/receipt/validations/0/validator")
            .unwrap()
            .as_str(),
        Some("markdown.section-structure.v1")
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
        ("plan_markdown_section", r#"{"heading":0,"body":"```\n"}"#),
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

#[test]
fn rendering_profiles_receipts_and_schema_rejection() {
    let root = std::env::temp_dir().join(format!("purrassist-render-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let source = "~~café~~ www.example.org\r\n";
    for (args, expected) in [
        ("{}", "<p>~~café~~ www.example.org</p>\n"),
        (
            r#"{"dialect":"gfm"}"#,
            "<p><del>café</del> <a href=\"http://www.example.org\">www.example.org</a></p>\n",
        ),
        (
            r#"{"dialect":"gfm","extensions":[]}"#,
            "<p>~~café~~ www.example.org</p>\n",
        ),
        (
            r#"{"extensions":["strikethrough"]}"#,
            "<p><del>café</del> www.example.org</p>\n",
        ),
    ] {
        let result = call(&tools, "render_markdown", source, args);
        assert_eq!(
            result.pointer("/status").unwrap().as_str(),
            Some("resolved")
        );
        assert_eq!(
            result.pointer("/value/html").unwrap().as_str(),
            Some(expected)
        );
        assert_eq!(
            result.pointer("/receipt/sources/0/text").unwrap().as_str(),
            Some(source)
        );
        assert_eq!(
            result.pointer("/value/source").unwrap().as_str(),
            Some(Digest::of(source.as_bytes()).to_string().as_str())
        );
        assert_eq!(
            result.pointer("/value/output").unwrap().as_str(),
            Some(Digest::of(expected.as_bytes()).to_string().as_str())
        );
        assert_eq!(
            result.pointer("/receipt/evidence/0/rule").unwrap().as_str(),
            Some("markdown.render.v1")
        );
    }
    for invalid in [
        r#"{"source":"x","dialect":"other"}"#,
        r#"{"source":"x","extensions":["tables","tables"]}"#,
        r#"{"source":"x","extensions":["unknown"]}"#,
        r#"{"source":"x","extensions":false}"#,
    ] {
        assert!(
            tools
                .call("render_markdown", &json::parse(invalid).unwrap())
                .is_err()
        );
    }
    let failed = call(
        &tools,
        "render_markdown",
        &"x".repeat(50_000),
        r#"{"effort":"exact"}"#,
    );
    assert_eq!(
        failed.pointer("/status").unwrap().as_str(),
        Some("unresolved")
    );
    assert_eq!(failed.get("value").unwrap().value, Value::Null);
    assert_eq!(
        failed
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("budget_exceeded")
    );
    let index = call(&tools, "inspect_markdown", ">\t\tcode\n", "{}");
    assert_eq!(
        offset(
            index
                .pointer("/value/blocks/0/children/0/lines/0/padding")
                .unwrap()
        ),
        2
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_task_tool_uses_paths_from_inspection() {
    let root = std::env::temp_dir().join(format!(
        "purrassist-nested-task-tool-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let source = "> - [ ] café\r\n>   - [X] inner\r\n> - [ ] sibling\r\n";
    let index = call(&tools, "inspect_markdown", source, "{}");
    let path = index
        .pointer("/value/blocks/0/children/0/children/1/path")
        .unwrap();
    assert_eq!(path.to_json(), "[0,0,1]");
    let plan = call(
        &tools,
        "plan_markdown_task",
        source,
        &format!("{{\"path\":{},\"checked\":false}}", path.to_json()),
    );
    assert_eq!(apply(&plan, source), source.replace("[X]", "[ ]"));
    for invalid in [
        r#"{"source":"x","checked":true}"#,
        r#"{"source":"x","block":0,"path":[0],"checked":true}"#,
        r#"{"source":"x","path":[],"checked":true}"#,
        r#"{"source":"x","path":[-1],"checked":true}"#,
    ] {
        assert!(
            tools
                .call("plan_markdown_task", &json::parse(invalid).unwrap())
                .is_err()
        );
    }
    let absent = call(
        &tools,
        "plan_markdown_task",
        source,
        r#"{"path":[0,99],"checked":true}"#,
    );
    assert_eq!(
        absent.pointer("/status").unwrap().as_str(),
        Some("unresolved")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn nested_section_tool_selects_inspected_paths_and_retains_validation() {
    let root = std::env::temp_dir().join(format!("purrassist-section-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let source = "> - # Tea\r\n>   Hot\r\n>   # Keep\r\n>   Tail\r\n";
    let inspected = call(&tools, "inspect_markdown", source, "{}");
    assert_eq!(
        inspected
            .pointer("/value/section_sites/0/path")
            .unwrap()
            .to_json(),
        "[0,0,0]"
    );
    let plan = call(
        &tools,
        "plan_markdown_section",
        source,
        r#"{"path":[0,0,0],"body":"Cold café.\r\n"}"#,
    );
    assert_eq!(apply(&plan, source), source.replace("Hot", "Cold café."));
    assert_eq!(
        plan.pointer("/receipt/validations/0/validator")
            .unwrap()
            .as_str(),
        Some("markdown.section-structure.v1")
    );
    let rejected = call(
        &tools,
        "plan_markdown_section",
        source,
        r#"{"path":[0,0,0],"body":"```\r\n"}"#,
    );
    assert_eq!(
        rejected.pointer("/status").unwrap().as_str(),
        Some("unresolved")
    );
    for extra in [
        r#"{"path":[],"body":""}"#,
        r#"{"path":[0,0,0],"heading":0,"body":""}"#,
        r#"{"body":""}"#,
    ] {
        let Value::Object(mut fields) = json::parse(extra).unwrap().value else {
            panic!("object");
        };
        let Value::Object(base) = object([("source", string(source))]).value else {
            panic!("object");
        };
        fields.extend(base);
        assert!(
            tools
                .call("plan_markdown_section", &Node::new(Value::Object(fields)))
                .is_err()
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
