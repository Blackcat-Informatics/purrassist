// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared ledger publication retains each Markdown operation's distinct contract.
use purrassist::{
    digest::Digest,
    json,
    wire::{object, string},
};
use purrassist_host::tools::Tools;

#[test]
fn record_metadata_and_source_spans_preserve_each_operation_contract() {
    let root =
        std::env::temp_dir().join(format!("purrassist-markdown-record-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    for (name, source, args, validator, constraint, rule, explanation) in [
        (
            "plan_markdown_section",
            "# Tea\r\n\r\nHot.\r\n",
            r#"{"heading":0,"body":"Cold.\r\n"}"#,
            "markdown.section-structure.v1",
            "selected heading, section boundary, ancestors, and surrounding block structure remain unchanged after reparsing",
            "markdown.section-body.v1",
            "Replaced the selected body; nested lines receive canonical outer container prefixes. Identical logical bodies keep original spelling. Outside bytes remain exact and original source is retained.",
        ),
        (
            "plan_markdown_reference",
            "[Tea]: /hot\r\n",
            r#"{"path":[0],"occurrence":0,"destination":"/cold","title":null}"#,
            "markdown.reference-definition.v1",
            "selected decoded fields match the request; ordered unselected definition labels and values remain unchanged",
            "markdown.reference-fields.v1",
            "The source-bound field region is encoded with escapes/entities and checked by reparsing all definitions. Multiline field spelling may collapse; labels and surrounding bytes remain retained.",
        ),
        (
            "plan_markdown_table_cell",
            "| Tea |\r\n| --- |\r\n| café |\r\n",
            r#"{"path":[0],"row":0,"column":0,"markdown":"cold"}"#,
            "markdown.table-structure.v1",
            "indexed source selection and table cell/row/column content match the requested edit",
            "markdown.table-edit.v1",
            "The source-bound proposal identifies exact changed ranges. Changed proposals were reparsed and checked against the complete expected table; unchanged cells require no mutation.",
        ),
        (
            "plan_markdown_structure",
            "# Tea\r\n\r\nKeep.\r\n",
            r#"{"op":"section_rename","heading":0,"title":"Café"}"#,
            "markdown.structure.v1",
            "the selected section, list, table, or reference structure and every unselected block reparse to the intended shape",
            "markdown.structure-edit.v1",
            "Structural operation section_rename was reparsed and checked against its documented shape; outside bytes remain exact and the original source is retained.",
        ),
    ] {
        let json::Value::Object(mut fields) = json::parse(args).unwrap().value else {
            panic!("object")
        };
        let json::Value::Object(source_field) = object([("source", string(source))]).value else {
            panic!("object")
        };
        fields.extend(source_field);
        let result = tools
            .call(name, &json::Node::new(json::Value::Object(fields)))
            .unwrap();
        assert_eq!(result.get("status").unwrap().as_str(), Some("resolved"));
        let receipt = result.get("receipt").unwrap();
        let validation = receipt.pointer("/validations/0").unwrap();
        assert_eq!(
            validation.get("validator").unwrap().as_str(),
            Some(validator)
        );
        assert_eq!(
            validation.get("constraint").unwrap().as_str(),
            Some(constraint)
        );
        assert_eq!(validation.get("passed").unwrap().to_json(), "true");
        assert_eq!(validation.get("diagnostics").unwrap().to_json(), "[]");
        let evidence = receipt.pointer("/evidence/0").unwrap();
        assert_eq!(evidence.get("rule").unwrap().as_str(), Some(rule));
        assert_eq!(
            evidence.get("explanation").unwrap().as_str(),
            Some(explanation)
        );
        assert_eq!(
            evidence.get("destination").unwrap().as_str(),
            Some("/edits")
        );
        assert_eq!(
            evidence.pointer("/span/source").unwrap().as_str(),
            Some(Digest::of(source.as_bytes()).to_string().as_str())
        );
        assert_eq!(evidence.pointer("/span/start").unwrap().to_json(), "0");
        assert_eq!(
            evidence.pointer("/span/end").unwrap().to_json(),
            source.len().to_string()
        );
        assert_eq!(receipt.pointer("/evidence/1"), None);
        assert_eq!(receipt.pointer("/validations/1"), None);
    }
    std::fs::remove_dir_all(root).unwrap();
}
