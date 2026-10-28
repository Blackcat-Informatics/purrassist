// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact admitted return contracts and non-lossy declaration export.
use purrassist_core::{
    Budget, Governor, json,
    schema::Schema,
    tool_call::{DeclarationForm, EmitError, ToolDefinition, ToolSet, ToolSetError, ToolSource},
};

#[test]
fn exact_whitespace_spans_numbers_and_independent_validation_roundtrip() {
    let source = " \n{\"type\":\"object\",\"properties\":{\"n\":{\"$ref\":\"#/$defs/N\"},\"forbidden\":false},\"$defs\":{\"N\":{\"const\":1.00e2}},\"required\":[\"n\"]}\t ";
    let tools = ToolSet::compile(
        [ToolDefinition::new("count").output_schema(source)],
        Budget::default(),
    )
    .unwrap();
    let tool = tools.tool("count").unwrap();
    assert_eq!(tool.output_schema_text(), Some(source));
    let output = tool.output_schema().unwrap();
    let mut governor = Governor::new(Budget::default());
    assert!(
        output
            .validate_with(&json::parse(r#"{"n":100}"#).unwrap(), &mut governor)
            .unwrap()
            .valid
    );
    assert!(
        !output
            .validate(&json::parse(r#"{"n":100,"forbidden":0}"#).unwrap())
            .unwrap()
            .valid
    );
    assert!(
        tool.schema()
            .validate(&json::parse("{}").unwrap())
            .unwrap()
            .valid
    );
    assert!(
        !tool
            .schema()
            .validate(&json::parse(r#"{"n":100}"#).unwrap())
            .unwrap()
            .valid
    );
    let wire = tool.render_declaration(DeclarationForm::Mcp).unwrap();
    let wrapped = format!(" [ {wire} ] ");
    let imported = ToolSet::from_mcp(&wrapped, Budget::default()).unwrap();
    let again = imported.tool("count").unwrap();
    assert_eq!(again.output_schema_text(), Some(source));
    assert_eq!(
        again.output_schema().unwrap().digest(),
        Schema::compile(source).unwrap().digest()
    );
    assert_eq!(
        again.render_declaration(DeclarationForm::Mcp).unwrap(),
        wire
    );
    let ToolSource::Declaration(declaration) = again.source() else {
        panic!("MCP source");
    };
    assert_eq!(declaration.output_schema_text(), Some(source));
    assert_eq!(&wrapped[declaration.output_schema_span().unwrap()], source);
    for form in DeclarationForm::ALL {
        if form != DeclarationForm::Mcp {
            assert!(
                matches!(
                    again.render_declaration(form),
                    Err(EmitError::Unsupported { .. })
                ),
                "{form:?}"
            );
        }
    }
}

#[test]
fn absence_true_false_scalar_and_array_contracts_do_not_gain_wire_authority() {
    let tools = ToolSet::compile([ToolDefinition::new("absent")], Budget::default()).unwrap();
    assert!(tools.tool("absent").unwrap().output_schema().is_none());
    assert!(tools.tool("absent").unwrap().output_schema_text().is_none());
    for (schema, value, valid) in [
        ("true", "1", true),
        ("false", "1", false),
        (r#"{"type":"integer"}"#, "1", true),
        (
            r#"{"type":"array","items":{"type":"integer"}}"#,
            "[1,2]",
            true,
        ),
    ] {
        let tools = ToolSet::compile(
            [ToolDefinition::new("result").output_schema(schema)],
            Budget::default(),
        )
        .unwrap();
        let tool = tools.tool("result").unwrap();
        assert_eq!(tool.output_schema_text(), Some(schema));
        assert_eq!(
            tool.output_schema()
                .unwrap()
                .validate(&json::parse(value).unwrap())
                .unwrap()
                .valid,
            valid
        );
        for form in DeclarationForm::ALL {
            assert!(
                matches!(
                    tool.render_declaration(form),
                    Err(EmitError::Unsupported { .. })
                ),
                "{form:?}"
            );
        }
    }
}

#[test]
fn output_admission_rejects_malformed_neighbors_without_ignoring_contracts() {
    for schema in [
        "true",
        "false",
        "{}",
        r#"{"type":"array"}"#,
        r#"{"type":"object","type":"object"}"#,
        r#"{"type":"object","$schema":"urn:unknown"}"#,
        r##"{"type":"object","$ref":"#/missing"}"##,
        r#"{"type":"object","required":5}"#,
    ] {
        let source = format!(r#"{{"name":"result","inputSchema":{{}},"outputSchema":{schema}}}"#);
        let error = ToolSet::from_mcp(&source, Budget::default()).unwrap_err();
        assert!(
            matches!(error,ToolSetError::InvalidDefinition {ref tool,ref reason} if tool=="result" && reason.contains("output schema")),
            "{error:?}"
        );
    }
    let duplicate =
        r#"{"name":"result","outputSchema":{"type":"object"},"outputSchema":{"type":"object"}}"#;
    assert!(matches!(
        ToolSet::from_mcp(duplicate, Budget::default()),
        Err(ToolSetError::InvalidDefinition { .. })
    ));
    for invalid in [
        "42",
        r#"{"$schema":"urn:unknown"}"#,
        r##"{"$ref":"#/missing"}"##,
    ] {
        assert!(matches!(
            ToolSet::compile(
                [ToolDefinition::new("result").output_schema(invalid)],
                Budget::default()
            ),
            Err(ToolSetError::InvalidDefinition { .. })
        ));
    }
}
