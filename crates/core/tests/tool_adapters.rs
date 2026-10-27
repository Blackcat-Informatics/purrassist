// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Lossless tool-declaration adapters: byte-exact render-back, exact-text
//! schemas, unmodeled keyword reporting, typed projection, hard errors,
//! budgets, order independence, and cross-form rendering.
use purrassist_core::{
    Budget, CancellationToken, Effort, Governor, Outcome, StopReason,
    contract::Resource,
    json,
    schema::Schema,
    tool_call::{
        CompiledTool, DeclarationForm, DeclarationMetadata, Effects, Nesting, Param,
        ParameterAliases, RepairPolicy, ToolDeclaration, ToolDefinition, ToolRecoveryRequest,
        ToolSet, ToolSetError, ToolSource, UnknownParameters, ValueShape,
    },
};

const OPENAI: &str = include_str!("data/tool-adapters/openai-tools.json");
const MCP: &str = include_str!("data/tool-adapters/mcp-tools-list.json");
const ANTHROPIC: &str = include_str!("data/tool-adapters/anthropic-tools.json");

/// The balanced JSON object starting at byte `start` of `text`, found by an
/// independent bracket scan that skips string contents.
fn object_at(text: &str, start: usize) -> &str {
    let bytes = text.as_bytes();
    assert_eq!(bytes[start], b'{', "no object at {start}");
    let (mut depth, mut in_string, mut escaped) = (0_usize, false, false);
    for (offset, &byte) in bytes[start..].iter().enumerate() {
        if in_string {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return &text[start..=start + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced object at {start}");
}

/// The object whose text begins with `marker`.
fn object_from<'a>(text: &'a str, marker: &str) -> &'a str {
    object_at(text, text.find(marker).expect("marker present"))
}

/// The object value of the member introduced by `marker` (which ends just
/// before the value, possibly with whitespace).
fn value_after<'a>(text: &'a str, marker: &str) -> &'a str {
    let after = text.find(marker).expect("marker present") + marker.len();
    let start = after + text[after..].find('{').expect("object value");
    object_at(text, start)
}

fn declaration(tool: &CompiledTool) -> &ToolDeclaration {
    match tool.source() {
        ToolSource::Declaration(declaration) => declaration,
        ToolSource::Builder => panic!("{} is not adapter-built", tool.name()),
    }
}

/// Assert the exact render-back contract for one adapter-built tool.
fn assert_exact(
    input: &str,
    tools: &ToolSet,
    name: &str,
    form: DeclarationForm,
    expected_declaration: &str,
    expected_parameters: Option<&str>,
) {
    let tool = tools.tool(name).expect("tool present");
    let source = declaration(tool);
    assert_eq!(source.form(), form);
    assert_eq!(source.text(), expected_declaration);
    assert_eq!(&input[source.span()], expected_declaration);
    assert_eq!(
        tools.render_declaration(name, form).as_deref(),
        Ok(expected_declaration)
    );
    assert_eq!(source.parameters_text(), expected_parameters);
    assert_eq!(
        source.parameters_span().map(|span| &input[span]),
        expected_parameters
    );
    assert_eq!(
        tools.render_json_schema(name),
        Some(expected_parameters.unwrap_or(r#"{"type":"object"}"#))
    );
}

#[test]
fn openai_render_back_is_byte_exact() {
    let tools = ToolSet::from_openai(OPENAI, Budget::default()).unwrap();
    let names: Vec<_> = tools.tools().iter().map(CompiledTool::name).collect();
    assert_eq!(names, ["list_dir", "ping", "read_file"]);
    let read = object_from(OPENAI, "{ \"type\" : \"function\"");
    assert!(read.contains("\"strict\":true") && read.contains('\t'));
    assert!(read.contains(r"\u0041") && read.contains(r"\/") && read.contains("1e2"));
    assert_exact(
        OPENAI,
        &tools,
        "read_file",
        DeclarationForm::OpenAi,
        read,
        Some(value_after(read, "\"parameters\":")),
    );
    let list = object_from(
        OPENAI,
        r#"{"type":"function","function":{"name":"list_dir""#,
    );
    assert_exact(
        OPENAI,
        &tools,
        "list_dir",
        DeclarationForm::OpenAi,
        list,
        Some(value_after(list, "\"parameters\":")),
    );
    // The decoded description is retained for rendering only.
    assert_eq!(
        tools.tool("read_file").unwrap().description(),
        Some("Read A file")
    );

    // A single declaration with CRLF line breaks, tabs, and padding.
    let single = "  {\r\n\t\"function\" : {\"name\":\"stat\",\r\n \"parameters\":{ \"properties\" : {\"n\":{\"type\":\"integer\",\"default\":1.0E0}},\t\"type\":\"object\" }},\"type\":\"function\",\"x-trace\":[1e2,-0]}\r\n";
    let tools = ToolSet::from_openai(single, Budget::default()).unwrap();
    let declaration_text = single.trim();
    assert_exact(
        single,
        &tools,
        "stat",
        DeclarationForm::OpenAi,
        declaration_text,
        Some(value_after(declaration_text, "\"parameters\":")),
    );
    assert_eq!(tools.defaults("stat").unwrap()[0].value.to_json(), "1.0E0");
}

#[test]
fn mcp_render_back_is_byte_exact() {
    let tools = ToolSet::from_mcp(MCP, Budget::default()).unwrap();
    let search = object_from(MCP, "{\n      \"name\": \"search_docs\"");
    assert!(search.contains("\"annotations\"") && search.contains("\"_meta\""));
    assert_exact(
        MCP,
        &tools,
        "search_docs",
        DeclarationForm::Mcp,
        search,
        Some(value_after(search, "\"inputSchema\":")),
    );
    let time = object_from(MCP, r#"{"name":"get_time""#);
    assert_exact(
        MCP,
        &tools,
        "get_time",
        DeclarationForm::Mcp,
        time,
        Some(r#"{"type":"object","properties":{}}"#),
    );
    assert_eq!(
        tools.tool("search_docs").unwrap().description(),
        Some("Search the docs / wiki")
    );

    // A bare array and a single tool are accepted too.
    let array = format!("[{search} ,\n{time}]");
    let tools = ToolSet::from_mcp(&array, Budget::default()).unwrap();
    assert_exact(
        &array,
        &tools,
        "search_docs",
        DeclarationForm::Mcp,
        search,
        Some(value_after(search, "\"inputSchema\":")),
    );
    let tools = ToolSet::from_mcp(time, Budget::default()).unwrap();
    assert_exact(
        time,
        &tools,
        "get_time",
        DeclarationForm::Mcp,
        time,
        Some(r#"{"type":"object","properties":{}}"#),
    );
}

#[test]
fn anthropic_render_back_is_byte_exact() {
    let tools = ToolSet::from_anthropic(ANTHROPIC, Budget::default()).unwrap();
    let weather = object_from(ANTHROPIC, "{\"name\": \"get_weather\"");
    assert!(weather.contains("cache_control"));
    assert_exact(
        ANTHROPIC,
        &tools,
        "get_weather",
        DeclarationForm::Anthropic,
        weather,
        Some(value_after(weather, "\"input_schema\":")),
    );
    let count = object_from(ANTHROPIC, "{\"type\": \"custom\"");
    assert_exact(
        ANTHROPIC,
        &tools,
        "count",
        DeclarationForm::Anthropic,
        count,
        Some(r#"{"type":"object","properties":{"n":{"type":"number","default":-0.0}}}"#),
    );
    assert_eq!(tools.defaults("count").unwrap()[0].value.to_json(), "-0.0");
    let single = ToolSet::from_anthropic(weather, Budget::default()).unwrap();
    assert_eq!(
        single.render_declaration("get_weather", DeclarationForm::Anthropic),
        Ok(weather.to_owned())
    );
}

#[test]
fn missing_parameters_schema_round_trips() {
    for (form, input) in [
        (
            DeclarationForm::OpenAi,
            r#"{"type":"function","function":{"name":"ping","description":"Ping"}}"#,
        ),
        (
            DeclarationForm::Mcp,
            r#"{"name":"ping","description":"Ping"}"#,
        ),
        (
            DeclarationForm::Anthropic,
            r#"[ {"description":"Ping","name":"ping"} ]"#,
        ),
    ] {
        let tools = adapt(form, input).unwrap();
        let declaration_text = input.trim_start_matches("[ ").trim_end_matches(" ]");
        assert_exact(input, &tools, "ping", form, declaration_text, None);
        let ping = tools.tool("ping").unwrap();
        assert_eq!(ping.argument_shape().arguments.parameters.len(), 0);
        assert_eq!(ping.unknown_parameters(), UnknownParameters::PassThrough);
        assert_eq!(ping.unmodeled_keywords().len(), 0);
        let arguments = json::parse(r#"{"anything":[1]}"#).unwrap();
        assert!(ping.schema().validate(&arguments).unwrap().valid);
        assert_eq!(
            ping.schema().digest(),
            Schema::compile(r#"{"type":"object"}"#).unwrap().digest()
        );
    }
    let tools = ToolSet::from_openai(
        r#"{"type":"function","function":{"name":"ping","description":"Ping"}}"#,
        Budget::default(),
    )
    .unwrap();
    assert_eq!(
        tools
            .render_declaration("ping", DeclarationForm::Mcp)
            .unwrap(),
        r#"{"name":"ping","description":"Ping","inputSchema":{"type":"object"}}"#
    );
    let tools = ToolSet::from_mcp(r#"{"name":"ping"}"#, Budget::default()).unwrap();
    assert_eq!(
        tools
            .render_declaration("ping", DeclarationForm::OpenAi)
            .unwrap(),
        r#"{"type":"function","function":{"name":"ping"}}"#
    );
}

fn adapt(form: DeclarationForm, input: &str) -> Result<ToolSet, ToolSetError> {
    let mut governor = Governor::new(Budget::default());
    adapt_governed(form, input, &mut governor)
}

fn adapt_governed(
    form: DeclarationForm,
    input: &str,
    governor: &mut Governor,
) -> Result<ToolSet, ToolSetError> {
    match form {
        DeclarationForm::OpenAi => ToolSet::from_openai_governed(input, governor),
        DeclarationForm::Mcp => ToolSet::from_mcp_governed(input, governor),
        DeclarationForm::Anthropic => ToolSet::from_anthropic_governed(input, governor),
        _ => unreachable!("only the forms under test"),
    }
}

fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(pointer, keyword)| ((*pointer).to_owned(), (*keyword).to_owned()))
        .collect()
}

const PROBE: &str = r#"{"name":"probe","inputSchema":{
  "title":"Probe","description":"root description","type":"object",
  "required":["a","ghost"],
  "properties":{
    "a":{"type":["string","null"],"x-a":1},
    "b":{"type":"array","items":{"type":"string","default":"x","minLength":1},"minItems":1},
    "c":{"type":"array","items":true},
    "d":{"type":"integer","enum":[1,2]},
    "e":{"const":"k","description":"fixed"},
    "f":{"type":"string","enum":["x","y"],"pattern":"^x$"},
    "g":{"type":"object","properties":{"h":{"type":"boolean","deprecated":true}},"patternProperties":{"^x-":{}}},
    "i":{"type":"string","properties":{}},
    "j":{"enum":["p","p"]}
  },
  "dependentRequired":{"a":["b"]},
  "if":{"required":["a"]},"then":{"required":["b"]}
}}"#;

#[test]
fn unmodeled_keywords_are_listed() {
    let tools = ToolSet::from_mcp(PROBE, Budget::default()).unwrap();
    assert_eq!(
        tools.tool("probe").unwrap().unmodeled_keywords(),
        pairs(&[
            ("", "title"),
            ("", "description"),
            ("", "required"),
            ("", "dependentRequired"),
            ("", "if"),
            ("", "then"),
            ("/properties/a", "type"),
            ("/properties/a", "x-a"),
            ("/properties/b", "minItems"),
            ("/properties/b/items", "default"),
            ("/properties/b/items", "minLength"),
            ("/properties/c", "items"),
            ("/properties/d", "type"),
            ("/properties/d", "enum"),
            ("/properties/e", "const"),
            ("/properties/f", "pattern"),
            ("/properties/g", "patternProperties"),
            ("/properties/g/properties/h", "deprecated"),
            ("/properties/i", "properties"),
            ("/properties/j", "enum"),
        ])
    );
    let tools = ToolSet::from_openai(OPENAI, Budget::default()).unwrap();
    assert_eq!(
        tools.tool("read_file").unwrap().unmodeled_keywords(),
        pairs(&[
            ("", "$defs"),
            ("/properties/path", "format"),
            ("/properties/ratio", "minimum"),
            ("/properties/ratio", "maximum"),
            ("/properties/target", "anyOf"),
            ("/properties/ref", "$ref"),
            ("/properties/options", "x-vendor"),
        ])
    );
    assert_eq!(
        tools.tool("list_dir").unwrap().unmodeled_keywords().len(),
        0
    );
    // Builder tools only produce modeled keywords.
    let built = ToolSet::compile(
        [ToolDefinition::new("b").param("x", Param::string())],
        Budget::default(),
    )
    .unwrap();
    assert_eq!(built.tool("b").unwrap().unmodeled_keywords().len(), 0);
}

#[test]
fn unmodeled_parameter_shape() {
    let tools = ToolSet::from_openai(OPENAI, Budget::default()).unwrap();
    let arguments = &tools.argument_shape("read_file").unwrap().arguments;
    assert_eq!(
        arguments.parameter("target").unwrap().value,
        ValueShape::Unmodeled
    );
    assert_eq!(
        arguments.parameter("ref").unwrap().value,
        ValueShape::Unmodeled
    );
    assert_eq!(
        arguments.parameter("mode").unwrap().value,
        ValueShape::Enumeration(vec!["r".into(), "w".into()])
    );
    assert_eq!(
        arguments.parameter("ratio").unwrap().value,
        ValueShape::Number
    );
    assert!(matches!(
        &arguments.parameter("path").unwrap().value,
        ValueShape::String(Some(constraint)) if constraint.source == "^[^\\u0000]+$"
    ));
    let ValueShape::Object(options) = &arguments.parameter("options").unwrap().value else {
        panic!("options is an object");
    };
    assert_eq!(
        options.parameter("depth").unwrap().value,
        ValueShape::Integer
    );
    assert_eq!(arguments.required, ["mode", "path"]);
    assert!(arguments.names.iter().all(|literal| !literal.alias));
    let defaults: Vec<_> = tools
        .defaults("read_file")
        .unwrap()
        .iter()
        .map(|default| default.value.to_json())
        .collect();
    assert_eq!(defaults, [r#""r""#, "1.0", "3"]);

    let tools = ToolSet::from_mcp(PROBE, Budget::default()).unwrap();
    let arguments = &tools.argument_shape("probe").unwrap().arguments;
    for name in ["a", "d", "e", "j"] {
        assert_eq!(
            arguments.parameter(name).unwrap().value,
            ValueShape::Unmodeled,
            "{name}"
        );
    }
    assert_eq!(
        arguments.parameter("b").unwrap().value,
        ValueShape::Array(Box::new(ValueShape::String(None)))
    );
    assert_eq!(
        arguments.parameter("c").unwrap().value,
        ValueShape::Array(Box::new(ValueShape::Unmodeled))
    );
    assert_eq!(
        arguments.parameter("f").unwrap().value,
        ValueShape::Enumeration(vec!["x".into(), "y".into()])
    );
    assert_eq!(
        arguments.parameter("i").unwrap().value,
        ValueShape::String(None)
    );
    // `ghost` is required by the exact schema but declares no parameter.
    assert_eq!(arguments.required, ["a"]);
    // Unmodeled values are opaque but still validated by the exact schema.
    let probe = tools.tool("probe").unwrap();
    for (arguments, valid) in [
        (r#"{"a":null,"b":["z"],"ghost":0}"#, true),
        (r#"{"a":null,"b":["z"]}"#, false),
        (r#"{"a":7,"b":["z"],"ghost":0}"#, false),
        (r#"{"a":"s","b":["z"],"ghost":0,"e":"other"}"#, false),
        (r#"{"a":"s","b":["z"],"ghost":0,"d":3}"#, false),
    ] {
        let report = probe
            .schema()
            .validate(&json::parse(arguments).unwrap())
            .unwrap();
        assert_eq!(report.valid, valid, "{arguments}");
    }
}

fn invalid_reason(result: Result<ToolSet, ToolSetError>) -> (String, String) {
    match result {
        Err(ToolSetError::InvalidDefinition { tool, reason }) => (tool, reason),
        other => panic!("expected an invalid definition, got {other:?}"),
    }
}

#[test]
fn invalid_declaration_is_hard_error() {
    let openai = |function: &str| format!(r#"{{"type":"function","function":{function}}}"#);
    let cases: Vec<(DeclarationForm, String, &str, &str)> = vec![
        (DeclarationForm::OpenAi, r#"[{"type":"function",}]"#.into(), "", "not strict JSON"),
        (DeclarationForm::OpenAi, "[1] x".into(), "", "not strict JSON"),
        (DeclarationForm::Mcp, r#""tools""#.into(), "", "must be an array or a single"),
        (DeclarationForm::OpenAi, "[1]".into(), "", "declaration 0 must be a JSON object"),
        (
            DeclarationForm::Anthropic,
            r#"[{"name":"a"},null]"#.into(),
            "",
            "declaration 1 must be a JSON object",
        ),
        (
            DeclarationForm::OpenAi,
            r#"{"type":"tool","function":{"name":"a"}}"#.into(),
            "",
            "`type` must be \"function\"",
        ),
        (
            DeclarationForm::OpenAi,
            r#"{"function":{"name":"a"}}"#.into(),
            "",
            "`type` must be \"function\"",
        ),
        (DeclarationForm::OpenAi, r#"{"type":"function"}"#.into(), "", "`function` must be an object"),
        (DeclarationForm::OpenAi, openai(r#"{"description":"x"}"#), "", "`name` must be a string"),
        (DeclarationForm::OpenAi, openai(r#"{"name":7}"#), "", "`name` must be a string"),
        (DeclarationForm::Mcp, r#"{"name":"a","name":"b"}"#.into(), "", "duplicate `name`"),
        (
            DeclarationForm::OpenAi,
            r#"{"type":"function","function":{"name":"a"},"function":{"name":"b"}}"#.into(),
            "",
            "duplicate `function`",
        ),
        (DeclarationForm::OpenAi, openai(r#"{"name":"a","description":5}"#), "a", "`description` must be a string"),
        (DeclarationForm::OpenAi, openai(r#"{"name":"a","parameters":true}"#), "a", "`parameters` must be a JSON object schema"),
        (DeclarationForm::OpenAi, openai(r#"{"name":"a","parameters":[]}"#), "a", "`parameters` must be a JSON object schema"),
        (DeclarationForm::Mcp, r#"{"name":"a","inputSchema":"{}"}"#.into(), "a", "`inputSchema` must be a JSON object schema"),
        (DeclarationForm::Anthropic, r#"{"name":"a","input_schema":null}"#.into(), "a", "`input_schema` must be a JSON object schema"),
        (
            DeclarationForm::Mcp,
            r#"{"name":"a","inputSchema":{},"inputSchema":{}}"#.into(),
            "a",
            "duplicate `inputSchema`",
        ),
        (DeclarationForm::Anthropic, r#"{"name":"a","input_schema":{"type":"string"}}"#.into(), "a", "must describe an object"),
        (DeclarationForm::Anthropic, r#"{"name":"a","input_schema":{"type":["object"]}}"#.into(), "a", "must describe an object"),
        (DeclarationForm::Mcp, r#"{"name":"a","inputSchema":{"pattern":"("}}"#.into(), "a", "parameters schema rejected"),
        (
            DeclarationForm::Mcp,
            r#"{"name":"a","inputSchema":{"$schema":"http://json-schema.org/draft-04/schema#"}}"#.into(),
            "a",
            "parameters schema rejected",
        ),
        (DeclarationForm::Mcp, r#"{"name":"bad name"}"#.into(), "bad name", "must match"),
        (DeclarationForm::Mcp, r#"{"name":""}"#.into(), "", "must match"),
        (
            DeclarationForm::Mcp,
            r#"{"name":"a","inputSchema":{"properties":{"bad key":{}}}}"#.into(),
            "a",
            "must match",
        ),
        (
            DeclarationForm::Mcp,
            r#"{"name":"a","inputSchema":{"properties":{"n":{"type":"integer","default":"x"}}}}"#.into(),
            "a",
            "does not conform",
        ),
        (
            DeclarationForm::Mcp,
            r##"{"name":"a","inputSchema":{"properties":{"n":{"$ref":"#/$defs/i","default":"x"}},"$defs":{"i":{"type":"integer"}}}}"##.into(),
            "a",
            "does not conform",
        ),
        (DeclarationForm::Mcp, r#"{"tools":{}}"#.into(), "", "`tools` member must be an array"),
    ];
    for (form, input, tool, needle) in cases {
        let (actual_tool, reason) = invalid_reason(adapt(form, &input));
        assert_eq!(actual_tool, tool, "{input}");
        assert!(reason.contains(needle), "{input}: {reason}");
    }
    assert_collisions();
}

/// Name collisions among tools and among parameters are hard errors.
fn assert_collisions() {
    for (form, input) in [
        (
            DeclarationForm::Mcp,
            r#"[{"name":"read_file"},{"name":"Read-File"}]"#,
        ),
        (DeclarationForm::Anthropic, r#"[{"name":"a"},{"name":"a"}]"#),
        (
            DeclarationForm::Mcp,
            r#"{"name":"t","inputSchema":{"properties":{"a_b":{},"a-b":{}}}}"#,
        ),
        (
            DeclarationForm::Mcp,
            r#"{"name":"t","inputSchema":{"properties":{"a":{},"a":{}}}}"#,
        ),
    ] {
        assert!(
            matches!(adapt(form, input), Err(ToolSetError::Collision { .. })),
            "{input}"
        );
    }
}

#[test]
fn adapter_tools_default_mutating() {
    for tools in [
        ToolSet::from_openai(OPENAI, Budget::default()).unwrap(),
        ToolSet::from_mcp(MCP, Budget::default()).unwrap(),
        ToolSet::from_anthropic(ANTHROPIC, Budget::default()).unwrap(),
    ] {
        for tool in tools.tools() {
            assert_eq!(tool.effects(), Effects::Mutating, "{}", tool.name());
            assert_eq!(tool.policy(), RepairPolicy::Structural, "{}", tool.name());
            assert_eq!(tool.aliases().len(), 0);
        }
    }
    let unknown = |schema: &str| {
        let input = format!(r#"{{"name":"t","inputSchema":{schema}}}"#);
        ToolSet::from_mcp(&input, Budget::default())
            .unwrap()
            .tool("t")
            .unwrap()
            .unknown_parameters()
    };
    for (schema, expected) in [
        (
            r#"{"additionalProperties":false}"#,
            UnknownParameters::Error,
        ),
        (
            r#"{"unevaluatedProperties":false}"#,
            UnknownParameters::Error,
        ),
        (r#"{"type":"object"}"#, UnknownParameters::PassThrough),
        (
            r#"{"additionalProperties":true}"#,
            UnknownParameters::PassThrough,
        ),
        (
            r#"{"additionalProperties":{"type":"string"}}"#,
            UnknownParameters::PassThrough,
        ),
        (
            r#"{"additionalProperties":false,"patternProperties":{"^x":{}}}"#,
            UnknownParameters::PassThrough,
        ),
        (
            r#"{"unevaluatedProperties":false,"anyOf":[{"properties":{"k":{}}}]}"#,
            UnknownParameters::PassThrough,
        ),
    ] {
        assert_eq!(unknown(schema), expected, "{schema}");
    }
    let tools = ToolSet::from_openai(OPENAI, Budget::default()).unwrap();
    let read = tools.argument_shape("read_file").unwrap();
    assert_eq!(read.arguments.unknown_parameters, UnknownParameters::Error);
    let ValueShape::Object(options) = &read.arguments.parameter("options").unwrap().value else {
        panic!("options is an object");
    };
    assert_eq!(options.unknown_parameters, UnknownParameters::Error);
    assert_eq!(
        tools.tool("list_dir").unwrap().unknown_parameters(),
        UnknownParameters::PassThrough
    );
    // The schema-derived handling agrees with the exact schema.
    let list = tools.tool("list_dir").unwrap();
    let extra = json::parse(r#"{"path":"a","extra":1}"#).unwrap();
    assert!(list.schema().validate(&extra).unwrap().valid);
    let read = tools.tool("read_file").unwrap();
    let extra = json::parse(r#"{"path":"a","mode":"r","extra":1}"#).unwrap();
    assert!(!read.schema().validate(&extra).unwrap().valid);
}

#[test]
fn adapter_schema_compiled_from_exact_text_rejects_violation() {
    let tools = ToolSet::from_openai(OPENAI, Budget::default()).unwrap();
    let read = tools.tool("read_file").unwrap();
    let exact = value_after(
        object_from(OPENAI, "{ \"type\" : \"function\""),
        "\"parameters\":",
    );
    assert_eq!(read.json_schema(), exact);
    // The digest binds the exact bytes: equal to a direct compilation of
    // the slice, different from a compilation of its compact re-rendering.
    let mut governor = Governor::new(Budget::default());
    let direct = Schema::compile_governed(exact, &mut governor).unwrap();
    assert_eq!(read.schema().digest(), direct.digest());
    let compact = json::parse(exact).unwrap().to_json();
    assert_ne!(compact, exact);
    assert_ne!(
        read.schema().digest(),
        Schema::compile(&compact).unwrap().digest()
    );
    // The document is the parsed exact text, spans relative to it.
    let document = read.schema_document();
    assert_eq!(document.span, Some(0..exact.len()));
    assert!(document.equivalent(&json::parse(exact).unwrap()));

    let mut governor = Governor::new(Budget::default());
    let violating = json::parse(r#"{"path":"a","mode":"r","target":true}"#).unwrap();
    let report = read
        .schema()
        .validate_with(&violating, &mut governor)
        .unwrap();
    assert!(!report.valid);
    assert!(
        report
            .violations
            .iter()
            .any(|violation| violation.keyword == "anyOf" && violation.instance_path == "/target"),
        "{:?}",
        report.violations
    );
    assert_eq!(report.schema, direct.digest());
    for arguments in [
        r#"{"path":"a","mode":"r","target":"x","ref":{"x":1.5},"options":{"depth":2}}"#,
        r#"{"path":"a","mode":"w","target":4}"#,
    ] {
        let report = read
            .schema()
            .validate_with(&json::parse(arguments).unwrap(), &mut governor)
            .unwrap();
        assert!(report.valid, "{arguments}: {:?}", report.violations);
    }
    for arguments in [
        r#"{"path":"a","mode":"r","ref":{"x":"no"}}"#,
        r#"{"path":"a","mode":"r","ratio":101}"#,
        r#"{"path":"a","mode":"x"}"#,
    ] {
        let report = read
            .schema()
            .validate_with(&json::parse(arguments).unwrap(), &mut governor)
            .unwrap();
        assert!(!report.valid, "{arguments}");
    }
}

fn zeroed(resource: Resource) -> Budget {
    let mut budget = Budget::default();
    match resource {
        Resource::Work => budget.work = 0,
        Resource::InputBytes => budget.input_bytes = 0,
        Resource::ScratchBytes => budget.scratch_bytes = 0,
        Resource::Depth => budget.depth = 0,
        _ => unreachable!("only dimensions adaptation consumes"),
    }
    budget
}

#[test]
fn adapter_compile_budget_exhaustion_is_explicit() {
    for form in [
        DeclarationForm::OpenAi,
        DeclarationForm::Mcp,
        DeclarationForm::Anthropic,
    ] {
        let input = match form {
            DeclarationForm::OpenAi => OPENAI,
            DeclarationForm::Mcp => MCP,
            _ => ANTHROPIC,
        };
        for resource in [
            Resource::Work,
            Resource::InputBytes,
            Resource::ScratchBytes,
            Resource::Depth,
        ] {
            let mut governor = Governor::new(zeroed(resource));
            assert_eq!(
                adapt_governed(form, input, &mut governor).unwrap_err(),
                ToolSetError::Resource(StopReason::BudgetExceeded(resource)),
                "{form:?} {resource:?}"
            );
            assert_eq!(governor.check(), Err(StopReason::BudgetExceeded(resource)));
        }
        let mut governor = Governor::new(Budget::default());
        adapt_governed(form, input, &mut governor).unwrap();
        let usage = governor.usage();
        for (resource, needed) in [
            (Resource::Work, usage.work),
            (Resource::ScratchBytes, usage.peak_scratch_bytes),
        ] {
            assert!(needed > 0);
            for limit in [needed / 4, needed / 2, needed - 1] {
                let mut budget = Budget::default();
                match resource {
                    Resource::Work => budget.work = limit,
                    _ => budget.scratch_bytes = limit,
                }
                let mut governor = Governor::new(budget);
                assert_eq!(
                    adapt_governed(form, input, &mut governor).unwrap_err(),
                    ToolSetError::Resource(StopReason::BudgetExceeded(resource)),
                    "{form:?} {resource:?} {limit}"
                );
            }
        }
        let mut governor = Governor::new(Budget {
            work: usage.work,
            ..Budget::default()
        });
        assert!(adapt_governed(form, input, &mut governor).is_ok());
        let token = CancellationToken::new();
        token.cancel();
        let mut governor = Governor::with_cancellation(Budget::default(), token);
        assert_eq!(
            adapt_governed(form, input, &mut governor).unwrap_err(),
            ToolSetError::Resource(StopReason::Cancelled)
        );
    }
    // Deep parameter nesting is bounded by the depth budget.
    let mut schema = String::from(r#"{"type":"string"}"#);
    for _ in 0..8 {
        schema = format!(r#"{{"type":"object","properties":{{"inner":{schema}}}}}"#);
    }
    let input = format!(r#"{{"name":"deep","inputSchema":{schema}}}"#);
    let mut governor = Governor::new(Budget {
        depth: 12,
        ..Budget::default()
    });
    assert_eq!(
        ToolSet::from_mcp_governed(&input, &mut governor).unwrap_err(),
        ToolSetError::Resource(StopReason::BudgetExceeded(Resource::Depth))
    );
    assert!(
        ToolSet::from_mcp(
            &input,
            Budget {
                depth: 64,
                ..Budget::default()
            }
        )
        .is_ok()
    );
}

/// Every observable result of a tool set except source spans, which
/// depend on where each declaration sits in the input.
fn observable(tools: &ToolSet) -> Vec<String> {
    tools
        .tools()
        .iter()
        .map(|tool| {
            let renders: Vec<_> = [
                DeclarationForm::OpenAi,
                DeclarationForm::Mcp,
                DeclarationForm::Anthropic,
            ]
            .into_iter()
            .map(|form| tool.render_declaration(form).expect("wire forms render"))
            .collect();
            format!(
                "{}|{:?}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
                tool.name(),
                tool.description(),
                tool.json_schema(),
                tool.schema().digest(),
                tool.argument_shape(),
                tool.defaults(),
                tool.unmodeled_keywords(),
                tool.unknown_parameters(),
                renders,
            )
        })
        .collect()
}

#[test]
fn adapter_order_independent() {
    let tools = ToolSet::from_openai(OPENAI, Budget::default()).unwrap();
    let declarations: Vec<String> = tools
        .tools()
        .iter()
        .map(|tool| declaration(tool).text().to_owned())
        .collect();
    let expected = observable(&tools);
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let input = format!(
            "[{}]",
            order
                .iter()
                .map(|&index| declarations[index].as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        let reordered = ToolSet::from_openai(&input, Budget::default()).unwrap();
        assert_eq!(observable(&reordered), expected, "{order:?}");
        for name in ["read_file", "list_dir", "ping"] {
            assert_eq!(
                reordered.lookup_exact(name).unwrap().name(),
                name,
                "{order:?}"
            );
        }
    }
}

#[test]
fn cross_form_rendering() {
    let tools = ToolSet::from_openai(OPENAI, Budget::default()).unwrap();
    let parameters = tools.render_json_schema("read_file").unwrap();
    let anthropic = tools
        .render_declaration("read_file", DeclarationForm::Anthropic)
        .unwrap();
    assert_eq!(
        anthropic,
        format!(
            r#"{{"name":"read_file","description":"Read A file","input_schema":{parameters}}}"#
        )
    );
    let mcp = tools
        .render_declaration("read_file", DeclarationForm::Mcp)
        .unwrap();
    assert_eq!(
        mcp,
        format!(r#"{{"name":"read_file","description":"Read A file","inputSchema":{parameters}}}"#)
    );
    // The embedded parameters stay exact through another adaptation, so
    // the schema (and its digest) is unchanged across forms.
    for (form, rendered) in [
        (DeclarationForm::Anthropic, &anthropic),
        (DeclarationForm::Mcp, &mcp),
    ] {
        let again = adapt(form, rendered).unwrap();
        let tool = again.tool("read_file").unwrap();
        assert_eq!(tool.json_schema(), parameters);
        assert_eq!(
            tool.schema().digest(),
            tools.tool("read_file").unwrap().schema().digest()
        );
        assert_eq!(
            again.render_declaration("read_file", form).as_deref(),
            Ok(rendered.as_str())
        );
        assert_eq!(
            tool.unmodeled_keywords(),
            tools.tool("read_file").unwrap().unmodeled_keywords()
        );
    }
    // MCP and Anthropic sources render into the other forms as well.
    let mcp_tools = ToolSet::from_mcp(MCP, Budget::default()).unwrap();
    let input_schema = mcp_tools.render_json_schema("search_docs").unwrap();
    assert!(input_schema.contains(r#""x-mcp-hint": "\u0041""#));
    assert_eq!(
        mcp_tools
            .render_declaration("search_docs", DeclarationForm::OpenAi)
            .unwrap(),
        format!(
            r#"{{"type":"function","function":{{"name":"search_docs","description":"Search the docs / wiki","parameters":{input_schema}}}}}"#
        )
    );
    let anthropic_tools = ToolSet::from_anthropic(ANTHROPIC, Budget::default()).unwrap();
    let count = anthropic_tools.render_json_schema("count").unwrap();
    assert_eq!(
        anthropic_tools
            .render_declaration("count", DeclarationForm::Mcp)
            .unwrap(),
        format!(r#"{{"name":"count","inputSchema":{count}}}"#)
    );
    // Builder tools render their canonical schema in every form.
    let built = ToolSet::compile(
        [ToolDefinition::new("echo")
            .description("Say \"hi\"")
            .param("text", Param::string().required())],
        Budget::default(),
    )
    .unwrap();
    let canonical = built.render_json_schema("echo").unwrap();
    assert_eq!(built.tool("echo").unwrap().source(), &ToolSource::Builder);
    assert_eq!(
        built
            .render_declaration("echo", DeclarationForm::Anthropic)
            .unwrap(),
        format!(r#"{{"name":"echo","description":"Say \"hi\"","input_schema":{canonical}}}"#)
    );
    assert_eq!(
        built
            .render_declaration("echo", DeclarationForm::OpenAi)
            .unwrap(),
        format!(
            r#"{{"type":"function","function":{{"name":"echo","description":"Say \"hi\"","parameters":{canonical}}}}}"#
        )
    );
    let reread = ToolSet::from_openai(
        &built
            .render_declaration("echo", DeclarationForm::OpenAi)
            .unwrap(),
        Budget::default(),
    )
    .unwrap();
    assert_eq!(reread.render_json_schema("echo"), Some(canonical));
    assert_eq!(
        reread.argument_shape("echo").unwrap().arguments,
        built.argument_shape("echo").unwrap().arguments
    );
}

/// An MCP `tools/list` result whose schemas declare Draft-07.
const DRAFT07_MCP: &str = r##"{"tools":[
  {"name":"plot","description":"Plot a point","inputSchema":{
    "$schema":"http://json-schema.org/draft-07/schema#",
    "type":"object",
    "definitions":{"num":{"type":"number"}},
    "properties":{
      "point":{"type":"array","items":[{"type":"number"},{"type":"number"}],"additionalItems":false},
      "scale":{"$ref":"#/definitions/num","type":"integer","default":1.5},
      "label":{"type":"string"},
      "color":{"type":"string","enum":["red","blue"]}
    },
    "required":["point"],
    "dependencies":{"label":["color"],"color":{"required":["label"]}},
    "additionalProperties":false
  },"annotations":{"readOnlyHint":true}},
  {"name":"echo","inputSchema":{"$schema":"http://json-schema.org/draft-07/schema","type":"object","properties":{"text":{"type":"string"}},"unevaluatedProperties":false}},
  {"name":"wrapped","inputSchema":{"$schema":"http://json-schema.org/draft-07/schema#","definitions":{"args":{"type":"object","properties":{"q":{"type":"string"}}}},"$ref":"#/definitions/args","properties":{"ignored":{"type":"string"}},"type":"object"}}
],"nextCursor":"c1"}"##;

#[test]
fn draft07_mcp_tools_adapt_losslessly() {
    let tools = ToolSet::from_mcp(DRAFT07_MCP, Budget::default()).unwrap();
    for (name, marker) in [
        ("plot", "{\"name\":\"plot\""),
        ("echo", "{\"name\":\"echo\""),
        ("wrapped", "{\"name\":\"wrapped\""),
    ] {
        let source = object_from(DRAFT07_MCP, marker);
        assert_exact(
            DRAFT07_MCP,
            &tools,
            name,
            DeclarationForm::Mcp,
            source,
            Some(value_after(source, "\"inputSchema\":")),
        );
        assert_eq!(
            tools.tool(name).unwrap().schema().dialect(),
            purrassist_core::schema::Dialect::Draft07
        );
    }

    let plot = tools.tool("plot").unwrap();
    assert_draft07_projection(&tools, plot);
    assert_draft07_defaults(plot);
    assert_draft07_validation(plot);
    assert_draft07_root_rules(&tools);
}

/// Draft-07-only constructs are unmodeled, never mis-projected.
fn assert_draft07_projection(tools: &ToolSet, plot: &CompiledTool) {
    assert_eq!(
        plot.unmodeled_keywords(),
        pairs(&[
            ("", "$schema"),
            ("", "definitions"),
            ("", "dependencies"),
            ("/properties/point", "items"),
            ("/properties/point", "additionalItems"),
            ("/properties/scale", "$ref"),
            ("/properties/scale", "type"),
        ])
    );
    let arguments = &tools.argument_shape("plot").unwrap().arguments;
    assert_eq!(
        arguments.parameter("point").unwrap().value,
        ValueShape::Array(Box::new(ValueShape::Unmodeled))
    );
    assert_eq!(
        arguments.parameter("scale").unwrap().value,
        ValueShape::Unmodeled
    );
    assert_eq!(
        arguments.parameter("color").unwrap().value,
        ValueShape::Enumeration(vec!["red".into(), "blue".into()])
    );
    assert_eq!(arguments.required, ["point"]);
    assert_eq!(arguments.unknown_parameters, UnknownParameters::Error);
}

/// The default 1.5 conforms only because Draft-07 `$ref` overrides `type`.
fn assert_draft07_defaults(plot: &CompiledTool) {
    let defaults: Vec<_> = plot
        .defaults()
        .iter()
        .map(|default| default.value.to_json())
        .collect();
    assert_eq!(defaults, ["1.5"]);
    let scaled = |dialect: &str| {
        format!(
            r##"{{"name":"s","inputSchema":{{"$schema":"{dialect}","definitions":{{"num":{{"type":"number"}}}},"properties":{{"scale":{{"$ref":"#/definitions/num","type":"integer","default":1.5}}}}}}}}"##
        )
    };
    let draft07 = ToolSet::from_mcp(
        &scaled("http://json-schema.org/draft-07/schema#"),
        Budget::default(),
    )
    .unwrap();
    assert_eq!(draft07.defaults("s").unwrap().len(), 1);
    let (tool, reason) = invalid_reason(ToolSet::from_mcp(
        &scaled("https://json-schema.org/draft/2020-12/schema"),
        Budget::default(),
    ));
    assert_eq!(tool, "s");
    assert!(reason.contains("does not conform"), "{reason}");
}

/// The compiled schema validates with Draft-07 semantics.
fn assert_draft07_validation(plot: &CompiledTool) {
    for (arguments, valid) in [
        (r#"{"point":[1,2]}"#, true),
        (r#"{"point":[1,"x"]}"#, false),
        (r#"{"point":[1,2,3]}"#, false),
        (r#"{"point":[1,2],"label":"a"}"#, false),
        (r#"{"point":[1,2],"color":"red"}"#, false),
        (r#"{"point":[1,2],"label":"a","color":"red"}"#, true),
        (r#"{"point":[1,2],"scale":0.5}"#, true),
        (r#"{"point":[1,2],"scale":"x"}"#, false),
        (r#"{"point":[1,2],"extra":1}"#, false),
    ] {
        let report = plot
            .schema()
            .validate(&json::parse(arguments).unwrap())
            .unwrap();
        assert_eq!(report.valid, valid, "{arguments}: {:?}", report.violations);
        assert_eq!(
            report.to_validation().validator,
            "purrassist.schema.draft-07.v1"
        );
    }
    // Under Draft 2020-12 the same `$ref` + sibling `type` rejects 0.5.
    let as_2020 = Schema::compile(
        r##"{"definitions":{"num":{"type":"number"}},"properties":{"scale":{"$ref":"#/definitions/num","type":"integer"}}}"##,
    )
    .unwrap();
    assert!(
        !as_2020
            .validate(&json::parse(r#"{"scale":0.5}"#).unwrap())
            .unwrap()
            .valid
    );
}

/// Draft-07 rules for `unevaluatedProperties` and a root `$ref`.
fn assert_draft07_root_rules(tools: &ToolSet) {
    // `unevaluatedProperties` is not a Draft-07 keyword: extras pass, and
    // the projection does not claim the scope is closed.
    let echo = tools.tool("echo").unwrap();
    assert!(
        echo.schema()
            .validate(&json::parse(r#"{"text":"a","extra":1}"#).unwrap())
            .unwrap()
            .valid
    );
    assert_eq!(
        tools
            .argument_shape("echo")
            .unwrap()
            .arguments
            .unknown_parameters,
        UnknownParameters::PassThrough
    );

    // A Draft-07 root `$ref` overrides its siblings (R11, gap G19). The
    // sibling `properties` stay ignored for projection and stay listed as
    // unmodeled keywords with `$ref` and `definitions` (retained evidence);
    // the local `$ref` target `#/definitions/args` is what describes the
    // arguments, so its `properties` are projected: `q` (a string), not the
    // ignored `ignored`. Before R11 this root projected no parameter.
    let wrapped = &tools.argument_shape("wrapped").unwrap().arguments;
    assert_eq!(wrapped.parameters.len(), 1);
    assert_eq!(wrapped.parameters[0].name, "q");
    assert_eq!(wrapped.parameters[0].value, ValueShape::String(None));
    assert!(wrapped.parameter("ignored").is_none());
    assert_eq!(wrapped.unknown_parameters, UnknownParameters::PassThrough);
    assert_eq!(
        tools.tool("wrapped").unwrap().unmodeled_keywords(),
        pairs(&[
            ("", "$schema"),
            ("", "definitions"),
            ("", "$ref"),
            ("", "properties"),
            ("", "type"),
        ])
    );
    let wrapped_schema = tools.tool("wrapped").unwrap().schema();
    assert!(
        !wrapped_schema
            .validate(&json::parse(r#"{"q":1}"#).unwrap())
            .unwrap()
            .valid
    );
    // The ignored sibling `properties` constrain nothing: validation is the
    // exact Draft-07 schema.
    assert!(
        wrapped_schema
            .validate(&json::parse(r#"{"q":"a","ignored":1}"#).unwrap())
            .unwrap()
            .valid
    );
}

/// The scope graph (issue #8 section 1, gap G2): one node per distinct schema
/// location, a recursive `$ref` a back-edge by node index, unions the union
/// of their object branches, and a typed-definition tool has none.
#[test]
fn scope_graph_has_one_node_per_location_and_back_edges() {
    let tools = ToolSet::from_openai(
        r##"[{"type":"function","function":{"name":"t","parameters":{"type":"object",
        "properties":{"children":{"type":"array","items":{"$ref":"#/$defs/node"}},
                      "root":{"$ref":"#/$defs/node"},
                      "u":{"anyOf":[{"type":"object","properties":{"a":{"type":"string"}}},
                                    {"$ref":"#/$defs/node"},{"type":"null"}]},
                      "free":{"type":"array","items":{"type":"object","additionalProperties":true}},
                      "text":{"$ref":"#/$defs/text"}},
        "$defs":{"node":{"type":"object","properties":{"name":{"type":"string"},
                  "tags":{"type":"array","items":{"type":"string"}},
                  "children":{"type":"array","items":{"$ref":"#/$defs/node"}}}},
                 "text":{"type":"string"}}}}}]"##,
        Budget::default(),
    )
    .unwrap();
    let shape = tools.argument_shape("t").unwrap();
    let nesting = |name: &str| shape.arguments.parameter(name).unwrap().nesting;
    // `children` items, `root` and the `$ref` branch of `u` share the one
    // `node` location; the `anyOf` union is its own location.
    let Nesting::ArrayOfObject(node) = nesting("children") else {
        panic!("{:?}", nesting("children"))
    };
    assert_eq!(nesting("root"), Nesting::Object(node));
    let scope = shape.scopes.node(node).unwrap();
    assert!(scope.declares("name") && scope.declares("children") && scope.declares("tags"));
    assert!(!scope.declares("a") && scope.is_open() && !scope.is_opaque());
    // The recursion is a back-edge to the same node, not an unrolling.
    let child = scope.member("children").unwrap();
    assert_eq!(child.nesting, Nesting::ArrayOfObject(node));
    assert!(child.array);
    let tags = scope.member("tags").unwrap();
    assert_eq!((tags.nesting, tags.array), (Nesting::None, true));
    assert!(!scope.member("name").unwrap().array);
    let Nesting::Object(union) = nesting("u") else {
        panic!("{:?}", nesting("u"))
    };
    assert_ne!(union, node);
    let union = shape.scopes.node(union).unwrap();
    assert!(
        ["a", "name", "tags", "children"]
            .iter()
            .all(|name| union.declares(name))
    );
    // A free-form object item is a typed object (no graph node); a `$ref`
    // to a string names no member, so its scope is opaque.
    assert_eq!(nesting("free"), Nesting::None);
    let Nesting::Object(text) = nesting("text") else {
        panic!("{:?}", nesting("text"))
    };
    assert!(shape.scopes.node(text).unwrap().is_opaque());
    // Nodes: node, union, text.
    assert_eq!(shape.scopes.len(), 3);
    // A typed-definition tool has no graph.
    let typed = ToolSet::compile(
        [ToolDefinition::new("p").param("a", Param::string())],
        Budget::default(),
    )
    .unwrap();
    assert!(typed.argument_shape("p").unwrap().scopes.is_empty());
}

/// Scope graph compilation is charged and bounded by the schema's size, and
/// running out is explicit.
#[test]
fn scope_graph_compilation_is_charged_and_explicit() {
    let defs: Vec<String> = (0..40)
        .map(|index| {
            format!(
                r##""d{index}":{{"type":"object","properties":{{"x":{{"type":"string"}},"next":{{"$ref":"#/$defs/d{}"}}}}}}"##,
                (index + 1) % 40
            )
        })
        .collect();
    let properties: Vec<String> = (0..40)
        .map(|index| format!(r##""p{index}":{{"$ref":"#/$defs/d{index}"}}"##))
        .collect();
    let schema = format!(
        r#"{{"type":"object","properties":{{{}}},"$defs":{{{}}}}}"#,
        properties.join(","),
        defs.join(",")
    );
    let input = format!(r#"{{"name":"wide","inputSchema":{schema}}}"#);
    let mut governor = Governor::new(Budget::default());
    let tools = ToolSet::from_mcp_governed(&input, &mut governor).unwrap();
    let shape = tools.argument_shape("wide").unwrap();
    // 40 locations, each referred to by one parameter and by the previous
    // location's `next`: one node each, however many references.
    assert_eq!(shape.scopes.len(), 40);
    let usage = governor.usage();
    for (resource, needed) in [
        (Resource::Work, usage.work),
        (Resource::ScratchBytes, usage.peak_scratch_bytes),
    ] {
        for limit in [needed / 2, needed - 1] {
            let mut budget = Budget::default();
            match resource {
                Resource::Work => budget.work = limit,
                _ => budget.scratch_bytes = limit,
            }
            assert_eq!(
                ToolSet::from_mcp_governed(&input, &mut Governor::new(budget)).unwrap_err(),
                ToolSetError::Resource(StopReason::BudgetExceeded(resource)),
                "{resource:?} {limit}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Root `$ref` declarations (issue #8, R11, gap G19)
// ---------------------------------------------------------------------------

/// The shape `FastMCP` and pydantic emit for a tool with one model argument.
const FASTMCP_SCHEMA: &str = r##"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"#/definitions/Args","definitions":{"Args":{"title":"Args","type":"object","properties":{"query":{"type":"string","description":"what to find"},"limit":{"type":"integer","default":10},"mode":{"type":"string","enum":["fast","full"]},"tags":{"type":"array","items":{"type":"string"}}},"required":["query"]}}}"##;

fn mcp_tool(name: &str, schema: &str) -> String {
    format!(r#"{{"name":"{name}","description":"d","inputSchema":{schema}}}"#)
}

/// The three wire renderings of a tool named `name` with `schema`.
fn wire_forms(name: &str, schema: &str) -> [(DeclarationForm, String); 3] {
    [
        (
            DeclarationForm::OpenAi,
            format!(
                r#"{{"type":"function","function":{{"name":"{name}","description":"d","parameters":{schema}}}}}"#
            ),
        ),
        (DeclarationForm::Mcp, mcp_tool(name, schema)),
        (
            DeclarationForm::Anthropic,
            format!(r#"{{"name":"{name}","description":"d","input_schema":{schema}}}"#),
        ),
    ]
}

/// Every wire form of a declaration reads back byte-exact from its own
/// source text and re-renders canonically with the verbatim `parameters`
/// text, and the tool built from each form projects the same parameters.
fn assert_render_back(name: &str, schema: &str, parameter_names: &[&str]) {
    for (form, text) in wire_forms(name, schema) {
        let tools = match form {
            DeclarationForm::OpenAi => {
                ToolSet::from_openai(&format!("[{text}]"), Budget::default())
            }
            DeclarationForm::Mcp => ToolSet::from_mcp(&text, Budget::default()),
            _ => ToolSet::from_anthropic(&format!("[{text}]"), Budget::default()),
        }
        .unwrap();
        let tool = tools.tool(name).unwrap();
        assert_eq!(
            tool.render_declaration(form).unwrap(),
            text,
            "{form:?} source"
        );
        assert_eq!(tool.json_schema(), schema, "{form:?} schema text");
        assert_eq!(declaration(tool).parameters_text(), Some(schema));
        // Every other wire form embeds the parameters text verbatim.
        for (other, rendered) in wire_forms(name, schema) {
            assert_eq!(
                tool.render_declaration(other).unwrap(),
                rendered,
                "{form:?} -> {other:?}"
            );
        }
        let projected: Vec<&str> = tools
            .argument_shape(name)
            .unwrap()
            .arguments
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        assert_eq!(projected, parameter_names, "{form:?}");
    }
}

#[test]
fn root_ref_projects_the_target_object() {
    let tools = ToolSet::from_mcp(&mcp_tool("search", FASTMCP_SCHEMA), Budget::default()).unwrap();
    let shape = tools.argument_shape("search").unwrap();
    let names: Vec<_> = shape
        .arguments
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.required))
        .collect();
    assert_eq!(
        names,
        [
            ("query", true),
            ("limit", false),
            ("mode", false),
            ("tags", false)
        ]
    );
    assert_eq!(shape.arguments.required, ["query"]);
    assert_eq!(
        shape.arguments.parameter("limit").unwrap().value,
        ValueShape::Integer
    );
    assert_eq!(
        shape
            .arguments
            .parameter("limit")
            .unwrap()
            .default
            .as_ref()
            .map(json::Node::to_json),
        Some("10".to_owned())
    );
    assert_eq!(
        shape.arguments.parameter("mode").unwrap().value,
        ValueShape::Enumeration(vec!["fast".into(), "full".into()])
    );
    assert_eq!(
        shape.arguments.parameter("tags").unwrap().value,
        ValueShape::Array(Box::new(ValueShape::String(None)))
    );
    assert_eq!(
        shape.arguments.unknown_parameters,
        UnknownParameters::PassThrough
    );
    // The root keywords stay evidence; the target's `title` is listed at
    // the target's pointer, relative to the parameters schema.
    assert_eq!(
        tools.tool("search").unwrap().unmodeled_keywords(),
        pairs(&[
            ("", "$schema"),
            ("", "$ref"),
            ("", "definitions"),
            ("/definitions/Args", "title"),
        ])
    );
    // Validation is the exact declaration and its draft.
    let schema = tools.tool("search").unwrap().schema();
    assert_eq!(schema.dialect(), purrassist_core::schema::Dialect::Draft07);
    for (call, valid) in [
        (r#"{"query":"a"}"#, true),
        (r#"{"query":"a","limit":3,"extra":1}"#, true),
        (r#"{"limit":3}"#, false),
        (r#"{"query":"a","limit":"3"}"#, false),
    ] {
        assert_eq!(
            schema.validate(&json::parse(call).unwrap()).unwrap().valid,
            valid,
            "{call}"
        );
    }
    assert_render_back(
        "search",
        FASTMCP_SCHEMA,
        &["query", "limit", "mode", "tags"],
    );
}

/// The prompt-form declaration of a root-`$ref` tool is the verbatim
/// parameters text, as for every adapter-built tool: projecting the target's
/// parameters changes what repair knows, not what a prompt declares.
#[test]
fn root_ref_prompt_form_declaration_prints_the_exact_schema() {
    let tools = ToolSet::from_mcp(&mcp_tool("search", FASTMCP_SCHEMA), Budget::default()).unwrap();
    let tool = tools.tool("search").unwrap();
    for form in [DeclarationForm::Qwen3CoderXml, DeclarationForm::Hermes] {
        let rendered = tool.render_declaration(form).unwrap();
        assert!(rendered.contains(FASTMCP_SCHEMA), "{form:?}: {rendered}");
        assert!(
            rendered.contains("\"name\":\"search\"") || rendered.contains("<name>search</name>")
        );
    }
    // A hand-inlined schema with the same parameters prints the same
    // parameters text structure: the only difference is the schema bytes.
    let inline =
        r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}"#;
    let inline_tools = ToolSet::from_mcp(&mcp_tool("search", inline), Budget::default()).unwrap();
    let rendered = inline_tools
        .tool("search")
        .unwrap()
        .render_declaration(DeclarationForm::Qwen3CoderXml)
        .unwrap();
    assert!(rendered.contains(inline));
}

#[test]
fn root_ref_defs_2020_12_merges_siblings_with_the_target() {
    // 2020-12: the siblings of `$ref` apply with the target. `extra` is the
    // root's own, `query` the target's; `limit` is declared by both, so its
    // typing stays with the exact schema (unmodeled, not required).
    let schema = r##"{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"#/$defs/Args","properties":{"extra":{"type":"boolean"},"limit":{"type":"integer","minimum":1}},"required":["extra"],"$defs":{"Args":{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"}},"required":["query"]}}}"##;
    let tools = ToolSet::from_mcp(&mcp_tool("search", schema), Budget::default()).unwrap();
    let shape = tools.argument_shape("search").unwrap();
    let arguments = &shape.arguments;
    let names: Vec<_> = arguments
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.required))
        .collect();
    assert_eq!(names, [("extra", true), ("limit", false), ("query", true)]);
    assert_eq!(
        arguments.parameter("extra").unwrap().value,
        ValueShape::Boolean
    );
    assert_eq!(
        arguments.parameter("query").unwrap().value,
        ValueShape::String(None)
    );
    assert_eq!(
        arguments.parameter("limit").unwrap().value,
        ValueShape::Unmodeled
    );
    assert_eq!(arguments.required, ["extra", "query"]);
    let schema_tool = tools.tool("search").unwrap().schema();
    assert_eq!(
        schema_tool.dialect(),
        purrassist_core::schema::Dialect::Draft202012
    );
    for (call, valid) in [
        (r#"{"extra":true,"query":"a"}"#, true),
        (r#"{"extra":true,"query":"a","limit":0}"#, false),
        (r#"{"query":"a"}"#, false),
    ] {
        assert_eq!(
            schema_tool
                .validate(&json::parse(call).unwrap())
                .unwrap()
                .valid,
            valid,
            "{call}"
        );
    }
    // The keywords the target and root carry beyond the projection stay
    // evidence at their own pointers.
    let keywords = tools.tool("search").unwrap().unmodeled_keywords();
    for expected in [("", "$schema"), ("", "$ref"), ("", "$defs")] {
        assert!(
            keywords.contains(&(expected.0.to_owned(), expected.1.to_owned())),
            "{expected:?}"
        );
    }
    assert_render_back("search", schema, &["extra", "limit", "query"]);
}

#[test]
fn root_ref_defs_2020_12_target_alone() {
    let schema = r##"{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"#/$defs/Args","$defs":{"Args":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}}}"##;
    let tools = ToolSet::from_mcp(&mcp_tool("search", schema), Budget::default()).unwrap();
    let shape = tools.argument_shape("search").unwrap();
    assert_eq!(shape.arguments.parameters.len(), 1);
    assert_eq!(shape.arguments.required, ["query"]);
    // The target closes its scope (`additionalProperties: false`), and the
    // exact schema rejects any other member, so unknown members are errors.
    assert_eq!(shape.arguments.unknown_parameters, UnknownParameters::Error);
    let exact = tools.tool("search").unwrap().schema();
    assert!(
        !exact
            .validate(&json::parse(r#"{"query":"a","x":1}"#).unwrap())
            .unwrap()
            .valid
    );
    assert_render_back("search", schema, &["query"]);
}

#[test]
fn root_all_of_single_ref_projects_the_target() {
    let schema = r##"{"$schema":"http://json-schema.org/draft-07/schema#","type":"object","allOf":[{"$ref":"#/definitions/Args"}],"definitions":{"Args":{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"}},"required":["query"]}}}"##;
    let tools = ToolSet::from_mcp(&mcp_tool("search", schema), Budget::default()).unwrap();
    let shape = tools.argument_shape("search").unwrap();
    let names: Vec<_> = shape
        .arguments
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.required))
        .collect();
    assert_eq!(names, [("query", true), ("limit", false)]);
    assert_eq!(
        shape.arguments.parameter("limit").unwrap().value,
        ValueShape::Integer
    );
    // The `$ref` element of the `allOf` overrides its (absent) siblings in
    // Draft-07 and is listed at its own pointer.
    let keywords = tools.tool("search").unwrap().unmodeled_keywords();
    assert!(keywords.contains(&("/allOf/0".to_owned(), "$ref".to_owned())));
    assert!(keywords.contains(&(String::new(), "allOf".to_owned())));
    assert_render_back("search", schema, &["query", "limit"]);
    // An `allOf` of two schemas is not followed: the inline projection.
    let two = r##"{"type":"object","properties":{"a":{"type":"string"}},"allOf":[{"$ref":"#/definitions/A"},{"$ref":"#/definitions/B"}],"definitions":{"A":{"type":"object","properties":{"b":{}}},"B":{"type":"object","properties":{"c":{}}}}}"##;
    let tools = ToolSet::from_mcp(&mcp_tool("t", two), Budget::default()).unwrap();
    let names: Vec<_> = tools
        .argument_shape("t")
        .unwrap()
        .arguments
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect();
    assert_eq!(names, ["a"]);
}

#[test]
fn root_any_of_objects_projects_the_union_of_names() {
    let schema = r##"{"type":"object","anyOf":[{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]},{"$ref":"#/definitions/Url"}],"definitions":{"Url":{"type":"object","properties":{"url":{"type":"string"},"path":{"type":"integer"}},"required":["url"]}}}"##;
    let tools = ToolSet::from_mcp(&mcp_tool("open", schema), Budget::default()).unwrap();
    let shape = tools.argument_shape("open").unwrap();
    let names: Vec<_> = shape
        .arguments
        .parameters
        .iter()
        .map(|parameter| {
            (
                parameter.name.as_str(),
                &parameter.value,
                parameter.required,
            )
        })
        .collect();
    // Typing stays with the exact schema (as R2 unions do): both names are
    // unmodeled and none is marked required.
    assert_eq!(
        names,
        [
            ("path", &ValueShape::Unmodeled, false),
            ("url", &ValueShape::Unmodeled, false)
        ]
    );
    assert_eq!(shape.arguments.required.len(), 0);
    assert_eq!(
        shape.arguments.unknown_parameters,
        UnknownParameters::PassThrough
    );
    // A name declared by several branches has one scope-graph entry that
    // is the union of the branches.
    let path = shape.arguments.parameter("path").unwrap();
    assert!(matches!(path.nesting, Nesting::Object(_)));
    let tool_schema = tools.tool("open").unwrap().schema();
    for (call, valid) in [
        (r#"{"path":"a"}"#, true),
        (r#"{"url":"u","path":3}"#, true),
        (r#"{"path":3}"#, false),
        ("{}", false),
    ] {
        assert_eq!(
            tool_schema
                .validate(&json::parse(call).unwrap())
                .unwrap()
                .valid,
            valid,
            "{call}"
        );
    }
    assert_render_back("open", schema, &["path", "url"]);
    // A branch that is not an object schema: the union is not followed.
    let mixed = r#"{"type":"object","properties":{"a":{"type":"string"}},"oneOf":[{"required":["a"]},{"required":["b"]}]}"#;
    let tools = ToolSet::from_mcp(&mcp_tool("t", mixed), Budget::default()).unwrap();
    let names: Vec<_> = tools
        .argument_shape("t")
        .unwrap()
        .arguments
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect();
    assert_eq!(names, ["a"]);
}

#[test]
fn unprojectable_root_ref_stays_pass_through_with_its_evidence() {
    // A root `$ref` that compiles but names no object schema projects no
    // parameter: a non-object target, a reference cycle, the root itself.
    for (schema, keywords) in [
        (
            r##"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"#/definitions/N","definitions":{"N":{"type":"string"}}}"##,
            pairs(&[("", "$schema"), ("", "$ref"), ("", "definitions")]),
        ),
        (
            r##"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"#/definitions/A","definitions":{"A":{"$ref":"#/definitions/B"},"B":{"$ref":"#/definitions/A"}}}"##,
            pairs(&[("", "$schema"), ("", "$ref"), ("", "definitions")]),
        ),
        (
            r##"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"#"}"##,
            pairs(&[("", "$schema"), ("", "$ref")]),
        ),
    ] {
        let tools = ToolSet::from_mcp(&mcp_tool("t", schema), Budget::default()).unwrap();
        let arguments = &tools.argument_shape("t").unwrap().arguments;
        assert_eq!(arguments.parameters.len(), 0, "{schema}");
        assert_eq!(
            arguments.unknown_parameters,
            UnknownParameters::PassThrough,
            "{schema}"
        );
        assert_eq!(
            tools.tool("t").unwrap().unmodeled_keywords(),
            keywords,
            "{schema}"
        );
        assert_render_back("t", schema, &[]);
    }
    // An unresolvable or external reference never reaches the projection:
    // the exact schema does not compile, so the declaration is invalid.
    for schema in [
        r##"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"#/definitions/Missing"}"##,
        r#"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"https://example.com/args.json"}"#,
    ] {
        assert!(
            matches!(
                ToolSet::from_mcp(&mcp_tool("t", schema), Budget::default()),
                Err(ToolSetError::InvalidDefinition { .. })
            ),
            "{schema}"
        );
    }
    // A target with a non-object `type` is not an object schema either.
    let typed = r##"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"#/definitions/A","definitions":{"A":{"type":["object","null"],"properties":{"q":{"type":"string"}}}}}"##;
    let tools = ToolSet::from_mcp(&mcp_tool("t", typed), Budget::default()).unwrap();
    assert_eq!(
        tools
            .argument_shape("t")
            .unwrap()
            .arguments
            .parameters
            .len(),
        0
    );
}

#[test]
fn root_ref_nested_ref_members_flow_into_the_scope_graph() {
    let schema = r##"{"$schema":"http://json-schema.org/draft-07/schema#","$ref":"#/definitions/Args","definitions":{"Args":{"type":"object","properties":{"filter":{"$ref":"#/definitions/Filter"},"limit":{"type":"integer","default":5}}},"Filter":{"type":"object","properties":{"kind":{"type":"string"}}}}}"##;
    let tools = ToolSet::from_mcp(&mcp_tool("search", schema), Budget::default()).unwrap();
    let shape = tools.argument_shape("search").unwrap();
    let filter = shape.arguments.parameter("filter").unwrap();
    assert_eq!(filter.value, ValueShape::Unmodeled);
    let Nesting::Object(node) = filter.nesting else {
        panic!("a `$ref` member nests as an object scope");
    };
    assert!(shape.scopes.node(node).unwrap().declares("kind"));
    // The default is validated against its exact subschema below the
    // target, and retained.
    assert_eq!(
        shape
            .arguments
            .parameter("limit")
            .unwrap()
            .default
            .as_ref()
            .map(json::Node::to_json),
        Some("5".to_owned())
    );
    let bad = schema.replace(r#""default":5"#, r#""default":"five""#);
    assert!(matches!(
        ToolSet::from_mcp(&mcp_tool("search", &bad), Budget::default()),
        Err(ToolSetError::InvalidDefinition { .. })
    ));
    assert_render_back("search", schema, &["filter", "limit"]);
}

/// A tool whose root `anyOf` has `branches` object schemas of `properties`
/// string properties each; `shared` names the properties alike in every
/// branch instead of distinctly.
fn union_declaration(branches: usize, properties: usize, shared: bool) -> String {
    let branches: Vec<String> = (0..branches)
        .map(|branch| {
            let members: Vec<String> = (0..properties)
                .map(|property| {
                    let name = if shared {
                        format!("p{property}")
                    } else {
                        format!("p{branch}_{property}")
                    };
                    format!(r#""{name}":{{"type":"string"}}"#)
                })
                .collect();
            format!(
                r#"{{"type":"object","properties":{{{}}}}}"#,
                members.join(",")
            )
        })
        .collect();
    mcp_tool(
        "t",
        &format!(r#"{{"type":"object","anyOf":[{}]}}"#, branches.join(",")),
    )
}

/// R11: the charged work and scratch of projecting a root union and
/// compiling its scope graph grow with the declaration's size, not with the
/// branches times the names (issue #8, coordinator follow-up). A 500-branch
/// union of one-property branches still resolves under the default budget,
/// as it does without the projection.
#[test]
fn root_union_projection_cost_is_linear() {
    let measure = |branches: usize, properties: usize, shared: bool| {
        let input = union_declaration(branches, properties, shared);
        let mut governor = Governor::new(Budget::default());
        let tools = ToolSet::from_mcp_governed(&input, &mut governor)
            .unwrap_or_else(|error| panic!("{branches}x{properties}: {error}"));
        let names = tools
            .argument_shape("t")
            .unwrap()
            .arguments
            .parameters
            .len();
        assert_eq!(
            names,
            if shared {
                properties
            } else {
                branches * properties
            }
        );
        let usage = governor.usage();
        let bytes = input.len() as u64;
        // Milli-units per input byte.
        (
            usage.work * 1000 / bytes,
            usage.total_scratch_charged * 1000 / bytes,
        )
    };
    for (properties, shared, small, large) in [
        (1, false, 50, 500),
        (5, false, 25, 200),
        (1, true, 50, 500),
        (5, true, 25, 200),
    ] {
        let (work_small, scratch_small) = measure(small, properties, shared);
        let (work_large, scratch_large) = measure(large, properties, shared);
        assert!(
            work_large * 10 <= work_small * 11,
            "{properties} properties, shared {shared}: work per byte {work_small} -> {work_large}"
        );
        assert!(
            scratch_large * 10 <= scratch_small * 11,
            "{properties} properties, shared {shared}: scratch per byte {scratch_small} -> {scratch_large}"
        );
    }
}

/// R11: recovering a call against a root union costs linear work in the
/// branches, not quadratic. Every branch's keywords describe the arguments
/// object, so a call carries about two keyword validations per branch; the
/// receipt's validation log once compared each against all the call's
/// earlier ones. A 500-branch union resolves under the default budget.
#[test]
fn root_union_call_recovery_cost_is_linear() {
    let work = |branches: usize| {
        let members: Vec<String> = (0..branches)
            .map(|i| format!(r#"{{"type":"object","properties":{{"p{i}":{{"type":"string"}}}}}}"#))
            .collect();
        let declaration = mcp_tool("t", &format!(r#"{{"anyOf":[{}]}}"#, members.join(",")));
        let tools = ToolSet::from_mcp(&declaration, Budget::default()).unwrap();
        let output = "<tool_call>\n{\"name\": \"t\", \"arguments\": {\"p1\": \"x\"}}\n</tool_call>";
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default(),
            purrassist_core::tool_call::DialectSet::built_in(),
        );
        let calls = match &result.outcome {
            purrassist_core::Outcome::Resolved(calls) => calls,
            other => panic!("{branches} branches: {other:?}"),
        };
        assert_eq!(calls.calls[0].arguments.to_json(), r#"{"p1":"x"}"#);
        assert!(calls.calls[0].keywords.len() >= 2 * branches);
        result.receipt.usage.work
    };
    let (small, middle, large) = (work(50), work(200), work(500));
    // Linear growth: the increments are proportional to the branch counts
    // (150 and 450 more than the smallest), within 20%.
    let (to_middle, to_large) = (middle - small, large - small);
    assert!(
        to_large * 150 <= to_middle * 450 * 12 / 10,
        "{small} {middle} {large}"
    );
    assert!(
        to_large * 150 * 12 >= to_middle * 450 * 10,
        "{small} {middle} {large}"
    );
}

const METADATA_MCP: &str = r#"{"name":"search","annotations":{"readOnlyHint":false},"inputSchema":{
 "type":"object","properties":{"query":{"type":"string"},"note":{"type":"string"}},
 "required":["query"],"dependentRequired":{"query":["note"]},"additionalProperties":false}}"#;

fn metadata_for(path: &[&str], aliases: &[&str]) -> DeclarationMetadata {
    DeclarationMetadata {
        tool_name: "search".into(),
        effects: Effects::ReadOnly,
        policy: RepairPolicy::Tolerant,
        parameter_aliases: vec![ParameterAliases {
            property_path: path.iter().map(|name| (*name).to_owned()).collect(),
            aliases: aliases.iter().map(|name| (*name).to_owned()).collect(),
        }],
    }
}

#[test]
fn metadata_alias_recovery_keeps_exact_schema_and_declaration() {
    let plain = ToolSet::from_mcp(METADATA_MCP, Budget::default()).unwrap();
    let empty = ToolSet::from_mcp_with_metadata_governed(
        METADATA_MCP,
        &[],
        &mut Governor::new(Budget::default()),
    )
    .unwrap();
    assert_eq!(observable(&plain), observable(&empty));
    let tools = ToolSet::from_mcp_with_metadata_governed(
        METADATA_MCP,
        &[metadata_for(&["query"], &["q"])],
        &mut Governor::new(Budget::default()),
    )
    .unwrap();
    let tool = tools.tool("search").unwrap();
    let original = plain.tool("search").unwrap();
    assert_eq!(tool.schema().digest(), original.schema().digest());
    assert_eq!(tool.json_schema(), original.json_schema());
    assert_eq!(tool.unmodeled_keywords(), original.unmodeled_keywords());
    assert!(
        tool.unmodeled_keywords()
            .iter()
            .any(|(_, key)| key == "dependentRequired")
    );
    assert_eq!(
        tool.render_declaration(DeclarationForm::Mcp).unwrap(),
        METADATA_MCP
    );
    assert_eq!(tool.effects(), Effects::ReadOnly);
    assert_eq!(tool.policy(), RepairPolicy::Tolerant);
    assert_eq!(tool.unknown_parameters(), UnknownParameters::Error);
    let valid = json::parse(r#"{"query":"needle","note":"source"}"#).unwrap();
    let invalid = json::parse(r#"{"query":"needle"}"#).unwrap();
    assert!(tool.schema().validate(&valid).unwrap().valid);
    assert!(!tool.schema().validate(&invalid).unwrap().valid);
    let request = ToolRecoveryRequest::new(Effort::Exact);
    let result = tools.recover(
        br#"<tool_call>{"name":"search","arguments":{"q":"needle","note":"source"}}</tool_call>"#,
        &request,
        purrassist_core::tool_call::DialectSet::built_in(),
    );
    let Outcome::Resolved(calls) = result.outcome else {
        panic!("{:?}", result.outcome)
    };
    assert_eq!(calls.usable().len(), 1);
    assert!(calls.calls[0].arguments.equivalent(&valid));
    let result = tools.recover(
        br#"<tool_call>{"name":"search","arguments":{"q":"needle"}}</tool_call>"#,
        &request,
        purrassist_core::tool_call::DialectSet::built_in(),
    );
    match result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => {
            assert_eq!(calls.usable_accepting_guesses().len(), 0);
        }
        Outcome::Unresolved => {}
        other @ Outcome::Ambiguous(_) => {
            panic!("invalid exact-schema call must not become an answer: {other:?}");
        }
    }
}

#[test]
fn metadata_aliases_reach_array_members_and_preserve_unmodeled_values() {
    let source = r#"{"name":"search","inputSchema":{"type":"object","properties":{
        "rows":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"],"additionalProperties":false}},
        "payload":{"oneOf":[{"type":"integer"},{"type":"string"}]}
    },"required":["rows","payload"],"additionalProperties":false}}"#;
    let mut metadata = metadata_for(&["rows", "name"], &["label"]);
    metadata.parameter_aliases.push(ParameterAliases {
        property_path: vec!["payload".into()],
        aliases: vec!["body".into()],
    });
    let tools = ToolSet::from_mcp_with_metadata_governed(
        source,
        &[metadata],
        &mut Governor::new(Budget::default()),
    )
    .unwrap();
    let result = tools.recover(
        br#"<tool_call>{"name":"search","arguments":{"rows":[{"label":"Ada"}],"body":7}}</tool_call>"#,
        &ToolRecoveryRequest::new(Effort::Exact), purrassist_core::tool_call::DialectSet::built_in());
    let Outcome::Resolved(calls) = result.outcome else {
        panic!("{:?}", result.outcome)
    };
    assert_eq!(calls.usable().len(), 1);
    assert_eq!(
        calls.calls[0]
            .arguments
            .pointer("/rows/0/name")
            .and_then(json::Node::as_str),
        Some("Ada")
    );
    assert_eq!(
        calls.calls[0].arguments.get("payload").unwrap().to_json(),
        "7"
    );
    let bad = metadata_for(&["payload", "child"], &["alias"]);
    let (tool, reason) = invalid_reason(ToolSet::from_mcp_with_metadata_governed(
        source,
        &[bad],
        &mut Governor::new(Budget::default()),
    ));
    assert_eq!(tool, "search");
    assert!(
        reason.contains("payload") && reason.contains("unmodeled"),
        "{reason}"
    );
}

#[test]
fn metadata_refuses_unknown_duplicate_and_colliding_hints() {
    let valid = metadata_for(&["query"], &["q"]);
    let mut unknown_tool = valid.clone();
    unknown_tool.tool_name = "absent".into();
    let mut duplicate_path = valid.clone();
    duplicate_path
        .parameter_aliases
        .push(duplicate_path.parameter_aliases[0].clone());
    for (metadata, expected_tool, needle) in [
        (vec![unknown_tool], "absent", "unknown tool"),
        (vec![metadata_for(&[], &["q"])], "search", "empty"),
        (
            vec![metadata_for(&["absent"], &["q"])],
            "search",
            "unknown metadata property",
        ),
        (
            vec![metadata_for(&["query", "child"], &["q"])],
            "search",
            "non-object",
        ),
        (
            vec![valid.clone(), valid.clone()],
            "search",
            "duplicate metadata record",
        ),
        (
            vec![duplicate_path],
            "search",
            "duplicate metadata property path",
        ),
        (
            vec![metadata_for(&["query"], &["bad name"])],
            "search",
            "must match",
        ),
    ] {
        let (tool, reason) = invalid_reason(ToolSet::from_mcp_with_metadata_governed(
            METADATA_MCP,
            &metadata,
            &mut Governor::new(Budget::default()),
        ));
        assert_eq!(tool, expected_tool);
        assert!(reason.contains(needle), "{reason}");
    }
    let error = ToolSet::from_mcp_with_metadata_governed(
        METADATA_MCP,
        &[metadata_for(&["query"], &["note"])],
        &mut Governor::new(Budget::default()),
    )
    .unwrap_err();
    let ToolSetError::Collision { first, second, .. } = error else {
        panic!("{error:?}")
    };
    assert!(first.contains("search") && second.contains("search"));
    assert!(
        ToolSet::from_mcp_with_metadata_governed(
            METADATA_MCP,
            &[valid],
            &mut Governor::new(Budget::default()),
        )
        .is_ok()
    );
}

#[test]
fn metadata_diagnostic_backing_storage_is_admitted() {
    let retained = |suffix: &str| {
        let metadata = [metadata_for(&["absent", suffix], &[])];
        let mut governor = Governor::new(Budget::default());
        let error =
            ToolSet::from_mcp_with_metadata_governed(METADATA_MCP, &metadata, &mut governor)
                .unwrap_err();
        let ToolSetError::InvalidDefinition { reason, .. } = error else {
            panic!("expected a path diagnostic: {error:?}");
        };
        (governor.usage().total_scratch_charged, reason.capacity())
    };
    let (baseline_charge, baseline_capacity) = retained("");
    let (charge, capacity) = retained(&"\u{7f}".repeat(1000));
    assert!(
        u64::try_from(capacity - baseline_capacity).unwrap() <= charge - baseline_charge,
        "diagnostic backing growth exceeds its scratch admission",
    );
}

#[test]
fn metadata_work_storage_and_cancellation_are_governed() {
    // Even an unknown name against an empty set must admit the error payload
    // before copying caller-owned text into a diagnostic.
    let mut unknown = metadata_for(&["query"], &["q"]);
    unknown.tool_name = "x".repeat(4096);
    for resource in [Resource::Work, Resource::ScratchBytes] {
        let mut budget = Budget::default();
        match resource {
            Resource::Work => budget.work = 2048,
            _ => budget.scratch_bytes = 2048,
        }
        assert_eq!(
            ToolSet::from_mcp_with_metadata_governed(
                "[]",
                &[unknown.clone()],
                &mut Governor::new(budget),
            )
            .unwrap_err(),
            ToolSetError::Resource(StopReason::BudgetExceeded(resource)),
        );
    }
    // An early unknown segment must not format a large, unvisited suffix
    // without first admitting the complete diagnostic.
    let suffix = "x".repeat(4096);
    let invalid_path = [metadata_for(&["absent", &suffix], &[])];
    let mut baseline = Governor::new(Budget::default());
    ToolSet::from_mcp_governed(METADATA_MCP, &mut baseline).unwrap();
    let budget = Budget {
        work: baseline.usage().work + 2048,
        ..Budget::default()
    };
    assert_eq!(
        ToolSet::from_mcp_with_metadata_governed(
            METADATA_MCP,
            &invalid_path,
            &mut Governor::new(budget),
        )
        .unwrap_err(),
        ToolSetError::Resource(StopReason::BudgetExceeded(Resource::Work)),
    );
    let metadata = [metadata_for(&["query"], &["q"])];
    let mut governor = Governor::new(Budget::default());
    ToolSet::from_mcp_with_metadata_governed(METADATA_MCP, &metadata, &mut governor).unwrap();
    let usage = governor.usage();
    for (resource, needed) in [
        (Resource::Work, usage.work),
        (Resource::ScratchBytes, usage.peak_scratch_bytes),
    ] {
        let mut budget = Budget::default();
        match resource {
            Resource::Work => budget.work = needed - 1,
            _ => budget.scratch_bytes = needed - 1,
        }
        let mut governor = Governor::new(budget);
        assert_eq!(
            ToolSet::from_mcp_with_metadata_governed(METADATA_MCP, &metadata, &mut governor)
                .unwrap_err(),
            ToolSetError::Resource(StopReason::BudgetExceeded(resource))
        );
        assert_eq!(governor.check(), Err(StopReason::BudgetExceeded(resource)));
    }
    let token = CancellationToken::new();
    token.cancel();
    let mut governor = Governor::with_cancellation(Budget::default(), token);
    assert_eq!(
        ToolSet::from_mcp_with_metadata_governed(METADATA_MCP, &metadata, &mut governor)
            .unwrap_err(),
        ToolSetError::Resource(StopReason::Cancelled)
    );
}

#[test]
fn wire_declaration_output_preserves_source_and_canonical_cross_form_bytes() {
    let schema =
        r#"{ "type":"object", "properties":{"amount":{"type":"number","minimum":1.00e2}} }"#;
    let description = r#""snow ☃\n\u0000\"\\\t""#;
    let forms = wire_forms("wire", schema).map(|(form, text)| {
        (
            form,
            text.replace(
                r#""description":"d""#,
                &format!(r#""description":{description}"#),
            ),
        )
    });
    for (source_form, canonical) in &forms {
        let original = canonical.replacen('{', "{ \"vendor\" : {\"kept\":true},\n ", 1);
        let tools = match source_form {
            DeclarationForm::OpenAi => ToolSet::from_openai(&original, Budget::default()),
            DeclarationForm::Mcp => ToolSet::from_mcp(&original, Budget::default()),
            _ => ToolSet::from_anthropic(&original, Budget::default()),
        }
        .expect("real wire adapter accepts the source");
        let tool = tools.tool("wire").expect("source tool");
        for (target, expected) in &forms {
            let mut governor = Governor::new(Budget::default());
            let rendered = tool
                .render_declaration_governed(*target, &mut governor)
                .expect("complete governed output");
            let expected = if target == source_form {
                &original
            } else {
                expected
            };
            assert_eq!(&rendered, expected, "{source_form:?} to {target:?}");
            assert!(governor.usage().work >= u64::try_from(rendered.len()).unwrap());
            assert!(governor.usage().peak_scratch_bytes >= u64::try_from(rendered.len()).unwrap());
            assert_eq!(tool.render_declaration(*target).unwrap(), rendered);
            assert_eq!(tools.render_declaration("wire", *target).unwrap(), rendered);
        }
        assert_eq!(declaration(tool).text(), original);
    }
    let tools = ToolSet::compile(
        [ToolDefinition::new("wire")
            .description("snow ☃\n\0\"\\\t")
            .param("amount", Param::integer())],
        Budget::default(),
    )
    .expect("actual builder source");
    let tool = tools.tool("wire").unwrap();
    for form in [
        DeclarationForm::OpenAi,
        DeclarationForm::Mcp,
        DeclarationForm::Anthropic,
    ] {
        let output = tool
            .render_declaration_governed(form, &mut Governor::new(Budget::default()))
            .unwrap();
        let parsed = json::parse(&output).unwrap();
        let record = parsed.get("function").unwrap_or(&parsed);
        assert_eq!(
            record.get("description").and_then(|node| node.as_str()),
            Some("snow ☃\n\0\"\\\t")
        );
        let key = if form == DeclarationForm::Mcp {
            "inputSchema"
        } else if form == DeclarationForm::Anthropic {
            "input_schema"
        } else {
            "parameters"
        };
        assert_eq!(record.get(key).unwrap().to_json(), tool.json_schema());
    }
}

#[test]
fn wire_declaration_output_obeys_exact_limits_and_cancellation_for_large_copies() {
    let description = "\0雪\\".repeat(1024);
    let schema = format!(
        r#"{{"type":"object","description":"{}"}}"#,
        "s".repeat(16384)
    );
    let original = format!(
        r#"{{ "name":"wire", "description":{}, "inputSchema":{schema} }}"#,
        json::quote(&description)
    );
    let tools = ToolSet::from_mcp(&original, Budget::default()).unwrap();
    let tool = tools.tool("wire").unwrap();
    for form in [
        DeclarationForm::Mcp,
        DeclarationForm::OpenAi,
        DeclarationForm::Anthropic,
    ] {
        let mut governor = Governor::new(Budget::default());
        let complete = tool
            .render_declaration_governed(form, &mut governor)
            .unwrap();
        let usage = governor.usage();
        let exact = Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            ..Budget::default()
        };
        assert_eq!(
            tool.render_declaration_governed(form, &mut Governor::new(exact))
                .unwrap(),
            complete
        );
        for (budget, resource) in [
            (
                Budget {
                    work: exact.work - 1,
                    ..exact
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: exact.scratch_bytes - 1,
                    ..exact
                },
                Resource::ScratchBytes,
            ),
        ] {
            let mut governor = Governor::new(budget);
            let reason = StopReason::BudgetExceeded(resource);
            assert_eq!(
                tool.render_declaration_governed(form, &mut governor),
                Err(purrassist_core::tool_call::EmitError::Resource(
                    reason.clone()
                ))
            );
            assert_eq!(governor.check(), Err(reason));
            assert_eq!(
                governor.scratch_meter().live(),
                0,
                "refused output is dropped"
            );
        }
        let token = CancellationToken::new();
        token.cancel();
        let mut governor = Governor::with_cancellation(exact, token);
        assert_eq!(
            tool.render_declaration_governed(form, &mut governor),
            Err(purrassist_core::tool_call::EmitError::Resource(
                StopReason::Cancelled
            ))
        );
        assert_eq!(governor.usage().work, 0);
        assert_eq!(governor.usage().peak_scratch_bytes, 0);
    }
    assert_eq!(declaration(tool).text(), original);
}
