// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Lossless tool-declaration adapters: byte-exact render-back, exact-text
//! schemas, unmodeled keyword reporting, typed projection, hard errors,
//! budgets, order independence, and cross-form rendering.
use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason,
    contract::Resource,
    json,
    schema::Schema,
    tool_call::{
        CompiledTool, DeclarationForm, Effects, Param, RepairPolicy, ToolDeclaration,
        ToolDefinition, ToolSet, ToolSetError, ToolSource, UnknownParameters, ValueShape,
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
        Some(expected_declaration)
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
        Some(weather.to_owned())
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
            .map(|form| tool.render_declaration(form))
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
            Some(rendered.as_str())
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

    // A Draft-07 root `$ref` overrides its siblings: no parameter is
    // projected from the ignored `properties`, and every member is listed.
    let wrapped = &tools.argument_shape("wrapped").unwrap().arguments;
    assert_eq!(wrapped.parameters.len(), 0);
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
    assert!(
        !tools
            .tool("wrapped")
            .unwrap()
            .schema()
            .validate(&json::parse(r#"{"q":1}"#).unwrap())
            .unwrap()
            .valid
    );
}
