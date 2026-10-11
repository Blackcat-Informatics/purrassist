// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Evidence for `purrassist-derive`: derived definitions equal hand-built
//! ones, the generated `FromNode` converts real recovered arguments exactly
//! and reports each failure at its pointer, and the generated code is
//! hygienic. Only `purrassist-core` is a dependency here (the core-flavored
//! derives); the facade flavor is exercised from the facade crate.
#![allow(dead_code, non_camel_case_types, clippy::redundant_pub_crate)]

use purrassist_core::json::{self, Node, Number, Value};
use purrassist_core::tool_call::{
    CallOutcome, DialectId, DialectSet, Effects, FromNode, FromNodeError, ObjectSpec, Param,
    ParameterSpec, RepairPolicy, ShapeStep, ToolArguments, ToolDefinition as Definition,
    ToolParameter, ToolRecoveryRequest, ToolSet, ToolSetError, UnknownParameters,
};
use purrassist_core::{Budget, Governor, Outcome};
use purrassist_derive::{ToolDefinition, ToolEnum};

// ----------------------------------------------------------------- helpers

fn parse(text: &str) -> Node {
    json::parse(text).expect("test JSON")
}

fn compile(definitions: impl IntoIterator<Item = Definition>) -> ToolSet {
    ToolSet::compile(definitions, Budget::default()).expect("tools compile")
}

fn request(dialect: DialectId) -> ToolRecoveryRequest {
    let set = DialectSet::new([dialect], &mut Governor::new(Budget::default())).expect("dialects");
    ToolRecoveryRequest::default().dialects(set)
}

/// The arguments of the single call `markup` carries, which must have
/// resolved without a guess.
fn recovered(tools: &ToolSet, dialect: DialectId, markup: &str) -> Node {
    let result = tools.recover(markup.as_bytes(), &request(dialect));
    let Outcome::Resolved(calls) = &result.outcome else {
        panic!("not resolved: {:#?}", result.outcome);
    };
    assert_eq!(calls.calls.len(), 1, "{markup}");
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved, "{markup}");
    calls.calls[0].arguments.clone()
}

fn hermes(name: &str, arguments: &str) -> String {
    format!("<tool_call>\n{{\"name\": \"{name}\", \"arguments\": {arguments}}}\n</tool_call>")
}

fn error(pointer: &str, expected: &'static str, found: &str) -> FromNodeError {
    FromNodeError {
        pointer: pointer.into(),
        expected,
        found: found.into(),
    }
}

// ------------------------------------------------------------------ T-map

#[derive(ToolEnum, Debug, PartialEq)]
enum Unit {
    Px,
    Em,
}

#[derive(ToolDefinition, Debug, PartialEq)]
struct Dims {
    width: u32,
    height: Option<u32>,
}

// One field of every type the derive maps.
#[derive(ToolDefinition, Debug, PartialEq)]
struct Everything {
    text: String,
    a_i8: i8,
    a_i16: i16,
    a_i32: i32,
    a_i64: i64,
    a_i128: i128,
    a_isize: isize,
    a_u8: u8,
    a_u16: u16,
    a_u32: u32,
    a_u64: u64,
    a_u128: u128,
    a_usize: usize,
    float: f64,
    exact: Number,
    flag: bool,
    list: Vec<String>,
    maybe: Option<String>,
    nested: Dims,
    unit: Unit,
    nested_list: Vec<Dims>,
    maybe_nested: Option<Dims>,
    units: Vec<Unit>,
    maybe_list: Option<Vec<u8>>,
    matrix: Vec<Vec<u8>>,
}

fn dims() -> ParameterSpec {
    Param::object(
        ObjectSpec::new()
            .param("width", Param::integer().required())
            .param("height", Param::integer()),
    )
}

fn unit() -> ParameterSpec {
    Param::enumeration(&["Px", "Em"])
}

fn everything_by_hand() -> Definition {
    Definition::new("everything")
        .param("text", Param::string().required())
        .param("a_i8", Param::integer().required())
        .param("a_i16", Param::integer().required())
        .param("a_i32", Param::integer().required())
        .param("a_i64", Param::integer().required())
        .param("a_i128", Param::integer().required())
        .param("a_isize", Param::integer().required())
        .param("a_u8", Param::integer().required())
        .param("a_u16", Param::integer().required())
        .param("a_u32", Param::integer().required())
        .param("a_u64", Param::integer().required())
        .param("a_u128", Param::integer().required())
        .param("a_usize", Param::integer().required())
        .param("float", Param::number().required())
        .param("exact", Param::number().required())
        .param("flag", Param::boolean().required())
        .param("list", Param::array(Param::string()).required())
        .param("maybe", Param::string())
        .param("nested", dims().required())
        .param("unit", unit().required())
        .param("nested_list", Param::array(dims()).required())
        .param("maybe_nested", dims())
        .param("units", Param::array(unit()).required())
        .param("maybe_list", Param::array(Param::integer()))
        .param(
            "matrix",
            Param::array(Param::array(Param::integer())).required(),
        )
}

#[test]
fn every_mapped_type_equals_the_hand_built_definition() {
    assert_eq!(Everything::tool_definition(), everything_by_hand());
    let tools = compile([Everything::tool_definition()]);
    let schema = tools.render_json_schema("everything").expect("schema");
    for fragment in [
        r#""text":{"type":"string"}"#,
        r#""a_u8":{"type":"integer"}"#,
        r#""float":{"type":"number"}"#,
        r#""flag":{"type":"boolean"}"#,
        r#""list":{"type":"array","items":{"type":"string"}}"#,
        r#""unit":{"type":"string","enum":["Px","Em"]}"#,
        r#""nested":{"type":"object","properties":{"width":{"type":"integer"},"height":{"type":"integer"}},"required":["width"],"additionalProperties":false}"#,
        r#""matrix":{"type":"array","items":{"type":"array","items":{"type":"integer"}}}"#,
    ] {
        assert!(schema.contains(fragment), "{fragment} in {schema}");
    }
    // `maybe`, `maybe_nested` and `maybe_list` (the `Option` fields) are the
    // only members left out of the top-level `required` list.
    let document = tools.tool("everything").unwrap().schema_document();
    let required: Vec<&str> = match &document.get("required").unwrap().value {
        Value::Array(items) => items.iter().map(|item| item.as_str().unwrap()).collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(required.len(), 25 - 3, "{required:?}");
    for optional in ["maybe", "maybe_nested", "maybe_list"] {
        assert!(!required.contains(&optional), "{optional}");
    }
    assert!(required.contains(&"nested") && required.contains(&"matrix"));
}

#[test]
fn every_mapped_type_round_trips_through_from_node() {
    let node = parse(
        r#"{"text":"t","a_i8":-8,"a_i16":-16,"a_i32":-32,"a_i64":-64,
            "a_i128":-170141183460469231731687303715884105728,"a_isize":-1,
            "a_u8":8,"a_u16":16,"a_u32":32,"a_u64":18446744073709551615,
            "a_u128":340282366920938463463374607431768211455,"a_usize":1,
            "float":2.5,"exact":1.50,"flag":true,"list":["x","y"],
            "nested":{"width":3},"unit":"Em","nested_list":[{"width":1,"height":2},{"width":4}],
            "maybe_nested":{"width":5},"units":["Px","Em"],"maybe_list":[1,2],
            "matrix":[[1],[2,3],[]]}"#,
    );
    let expected = Everything {
        text: "t".into(),
        a_i8: -8,
        a_i16: -16,
        a_i32: -32,
        a_i64: -64,
        a_i128: i128::MIN,
        a_isize: -1,
        a_u8: 8,
        a_u16: 16,
        a_u32: 32,
        a_u64: u64::MAX,
        a_u128: u128::MAX,
        a_usize: 1,
        float: 2.5,
        exact: "1.50".parse().unwrap(),
        flag: true,
        list: vec!["x".into(), "y".into()],
        maybe: None,
        nested: Dims {
            width: 3,
            height: None,
        },
        unit: Unit::Em,
        nested_list: vec![
            Dims {
                width: 1,
                height: Some(2),
            },
            Dims {
                width: 4,
                height: None,
            },
        ],
        maybe_nested: Some(Dims {
            width: 5,
            height: None,
        }),
        units: vec![Unit::Px, Unit::Em],
        maybe_list: Some(vec![1, 2]),
        matrix: vec![vec![1], vec![2, 3], vec![]],
    };
    assert_eq!(Everything::from_node(&node), Ok(expected));
    assert_eq!(
        Number::from_node(node.get("exact").unwrap())
            .unwrap()
            .as_str(),
        "1.50",
        "an exact number keeps its spelling"
    );
}

#[test]
fn nesting_depth_counts_derived_levels_only() {
    assert_eq!(<Unit as ToolParameter>::DEPTH, 0);
    assert_eq!(<Dims as ToolParameter>::DEPTH, 1);
    assert_eq!(<Option<Vec<Dims>> as ToolParameter>::DEPTH, 1);
    assert_eq!(<Everything as ToolParameter>::DEPTH, 2);
    assert_eq!(<Planner as ToolParameter>::DEPTH, 2);
    assert_eq!(<Ping as ToolParameter>::DEPTH, 1);
}

// ----------------------------------------------------------------- ATTR-tool

#[derive(ToolDefinition)]
#[tool(
    name = "lookup",
    alias = "find",
    alias = "get",
    description = "Look something up."
)]
struct Named {
    key: String,
}

#[test]
fn tool_name_alias_and_description() {
    let by_hand = Definition::new("lookup")
        .alias("find")
        .alias("get")
        .description("Look something up.")
        .param("key", Param::string().required());
    assert_eq!(Named::tool_definition(), by_hand);
    let tools = compile([Named::tool_definition()]);
    let tool = tools.lookup_exact("find").expect("alias");
    assert_eq!(tool.name(), "lookup");
    assert_eq!(tools.lookup_exact("get").unwrap().name(), "lookup");
    assert_eq!(tool.description(), Some("Look something up."));
    assert_eq!(tool.aliases(), ["find", "get"]);
}

#[derive(ToolDefinition)]
#[tool(effects = read_only)]
struct ReadOnlyTool {}
#[derive(ToolDefinition)]
#[tool(effects = mutating)]
struct MutatingTool {}
#[derive(ToolDefinition)]
#[tool(policy = structural)]
struct StructuralTool {}
#[derive(ToolDefinition)]
#[tool(policy = tolerant)]
struct TolerantTool {}
#[derive(ToolDefinition)]
#[tool(unknown = error)]
struct ErrorTool {}
#[derive(ToolDefinition)]
#[tool(unknown = drop)]
struct DropTool {}
#[derive(ToolDefinition)]
#[tool(unknown = pass)]
struct PassTool {
    #[param(extra)]
    rest: Node,
}

#[test]
fn tool_effects_policy_and_unknown_values() {
    let tools = compile([
        ReadOnlyTool::tool_definition(),
        MutatingTool::tool_definition(),
        StructuralTool::tool_definition(),
        TolerantTool::tool_definition(),
        ErrorTool::tool_definition(),
        DropTool::tool_definition(),
        PassTool::tool_definition(),
    ]);
    let tool = |name: &str| tools.tool(name).unwrap();
    assert_eq!(
        ReadOnlyTool::tool_definition(),
        Definition::new("read_only_tool").effects(Effects::ReadOnly)
    );
    assert_eq!(
        MutatingTool::tool_definition(),
        Definition::new("mutating_tool").effects(Effects::Mutating)
    );
    assert_eq!(
        StructuralTool::tool_definition(),
        Definition::new("structural_tool").policy(RepairPolicy::Structural)
    );
    assert_eq!(
        TolerantTool::tool_definition(),
        Definition::new("tolerant_tool").policy(RepairPolicy::Tolerant)
    );
    assert_eq!(
        ErrorTool::tool_definition(),
        Definition::new("error_tool").unknown_parameters(UnknownParameters::Error)
    );
    assert_eq!(
        DropTool::tool_definition(),
        Definition::new("drop_tool").unknown_parameters(UnknownParameters::DropWithLoss)
    );
    assert_eq!(
        PassTool::tool_definition(),
        Definition::new("pass_tool").unknown_parameters(UnknownParameters::PassThrough)
    );
    assert_eq!(tool("read_only_tool").effects(), Effects::ReadOnly);
    assert_eq!(tool("mutating_tool").effects(), Effects::Mutating);
    assert_eq!(tool("structural_tool").policy(), RepairPolicy::Structural);
    assert_eq!(tool("tolerant_tool").policy(), RepairPolicy::Tolerant);
    assert_eq!(
        tool("error_tool").unknown_parameters(),
        UnknownParameters::Error
    );
    assert_eq!(
        tool("drop_tool").unknown_parameters(),
        UnknownParameters::DropWithLoss
    );
    assert_eq!(
        tool("pass_tool").unknown_parameters(),
        UnknownParameters::PassThrough
    );
}

// ---------------------------------------------------------------- ATTR-param

#[derive(ToolDefinition)]
struct Params {
    #[param(
        name = "path",
        alias = "file",
        alias = "filename",
        description = "Where."
    )]
    location: String,
    #[param(repeat)]
    tags: Vec<String>,
    #[param(repeat, alias = "kw")]
    keywords: Option<Vec<String>>,
    #[param(default = 10)]
    limit: u32,
    #[param(pattern = "^[a-z]+$")]
    slug: String,
    #[param(pattern = "^x", repeat)]
    codes: Vec<String>,
    #[param(pattern = "^y")]
    optional_slug: Option<String>,
    #[param(pattern = "^z")]
    slugs: Vec<String>,
}

#[test]
fn param_keys_equal_the_hand_built_definition() {
    let by_hand = Definition::new("params")
        .param(
            "path",
            Param::string()
                .required()
                .alias("file")
                .alias("filename")
                .description("Where."),
        )
        .param("tags", Param::string().repeatable().required())
        .param("keywords", Param::string().repeatable().alias("kw"))
        .param(
            "limit",
            Param::integer().default(Node::new(Value::Number("10".parse().unwrap()))),
        )
        .param("slug", Param::string().pattern("^[a-z]+$").required())
        .param(
            "codes",
            Param::string().pattern("^x").repeatable().required(),
        )
        .param("optional_slug", Param::string().pattern("^y"))
        .param(
            "slugs",
            Param::array(Param::string().pattern("^z")).required(),
        );
    assert_eq!(Params::tool_definition(), by_hand);

    let tools = compile([Params::tool_definition()]);
    let shape = &tools.argument_shape("params").unwrap().arguments;
    assert_eq!(shape.resolve_name("file"), Some("path"));
    assert_eq!(shape.resolve_name("filename"), Some("path"));
    assert_eq!(shape.resolve_name("kw"), Some("keywords"));
    assert_eq!(shape.resolve_name("path"), Some("path"));
    let defaults = tools.defaults("params").unwrap();
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0].path, [ShapeStep::Parameter("limit".into())]);
    assert_eq!(defaults[0].value.to_json(), "10");
    let schema = tools.render_json_schema("params").unwrap();
    assert!(
        schema.contains(r#""tags":{"type":"array","items":{"type":"string"}}"#),
        "a repeatable parameter renders as an array: {schema}"
    );
    assert!(
        schema.contains(r#""limit":{"type":"integer","default":10}"#),
        "{schema}"
    );
    assert!(
        schema.contains(r#""slug":{"type":"string","pattern":"^[a-z]+$"}"#),
        "{schema}"
    );
}

// -------------------------------------------------------------------- naming

#[derive(ToolDefinition)]
struct ReadFile {
    r#type: String,
    r#match: Option<u8>,
}
#[derive(ToolDefinition)]
struct HTTPRequest {}
#[derive(ToolDefinition)]
struct ReadFile2 {}
#[derive(ToolDefinition)]
struct Read2File {}
#[derive(ToolDefinition)]
struct r#Raw {}
#[derive(ToolDefinition)]
struct Already_Snake {}

#[derive(ToolEnum, Debug, PartialEq)]
enum Verbatim {
    HTTPRequest,
    r#type,
    Snake_case,
    #[param(name = "explicit-name")]
    Renamed,
    Éclair,
}

#[test]
fn names_follow_the_naming_decision() {
    assert_eq!(ReadFile::tool_definition().name(), "read_file");
    assert_eq!(HTTPRequest::tool_definition().name(), "http_request");
    assert_eq!(ReadFile2::tool_definition().name(), "read_file2");
    assert_eq!(Read2File::tool_definition().name(), "read2_file");
    assert_eq!(Raw::tool_definition().name(), "raw");
    assert_eq!(Already_Snake::tool_definition().name(), "already_snake");
    assert_eq!(
        ReadFile::tool_definition(),
        Definition::new("read_file")
            .param("type", Param::string().required())
            .param("match", Param::integer())
    );
    // Enum values are the variant idents verbatim, `r#` removed, unless named.
    assert_eq!(
        <Verbatim as ToolParameter>::parameter(),
        Param::enumeration(&[
            "HTTPRequest",
            "type",
            "Snake_case",
            "explicit-name",
            "Éclair"
        ])
    );
    for (text, expected) in [
        ("HTTPRequest", Verbatim::HTTPRequest),
        ("type", Verbatim::r#type),
        ("Snake_case", Verbatim::Snake_case),
        ("explicit-name", Verbatim::Renamed),
        ("Éclair", Verbatim::Éclair),
    ] {
        assert_eq!(
            Verbatim::from_node(&Node::new(Value::String(text.into()))),
            Ok(expected)
        );
    }
    assert!(
        Verbatim::from_node(&parse(r#""Renamed""#)).is_err(),
        "the variant ident is replaced by its name"
    );
}

#[derive(ToolDefinition, Debug, PartialEq)]
#[tool(name = "unicode.tool")]
struct Ünïcode {
    #[param(name = "naive", description = "Naïve — 日本語 \u{e9}")]
    naïve: String,
    #[param(name = "nihongo")]
    nihongo: Option<String>,
}

#[test]
fn unicode_identifiers_names_and_values_convert() {
    assert_eq!(
        Ünïcode::tool_definition(),
        Definition::new("unicode.tool")
            .param(
                "naive",
                Param::string()
                    .required()
                    .description("Naïve — 日本語 \u{e9}")
            )
            .param("nihongo", Param::string())
    );
    assert_eq!(
        Ünïcode::from_node(&parse(r#"{"nihongo":"x","naive":"y"}"#)),
        Ok(Ünïcode {
            naïve: "y".into(),
            nihongo: Some("x".into())
        })
    );
}

// ------------------------------------------------------------- descriptions

/// Reads a file.
///
///   Second paragraph, indented.
#[derive(ToolDefinition)]
struct DocumentedTool {
    /// The path.
    path: String,
    /// Replaced by the explicit key.
    #[param(description = "Explicit wins.")]
    other: u8,
    /// Not trimmed to nothing.
    ///
    /// Two paragraphs.
    third: Option<u8>,
    /// A nested object.
    nested: DocumentedNested,
    #[param(description = "Field text wins.")]
    nested_own: DocumentedNested,
    plain: Dims,
}

/// Nested struct description.
#[derive(ToolDefinition)]
struct DocumentedNested {
    value: u8,
}

/// The enum's own text.
#[derive(ToolEnum)]
enum DocumentedEnum {
    /// Variant docs have no schema sink.
    One,
}

/// Overridden.
#[derive(ToolDefinition)]
#[tool(description = "Explicit tool description.")]
struct ExplicitOverDoc {}

#[derive(ToolDefinition)]
#[tool(description = "line1\nline2 \"quoted\" \u{e9} \\ tab\t")]
struct Escapes {}

#[derive(ToolDefinition)]
#[tool(description = r#"raw "x" \n"#)]
struct RawEscapes {}

#[test]
fn doc_comments_are_descriptions_and_explicit_keys_win() {
    let nested = |description: Option<&str>| {
        let object = Param::object(ObjectSpec::new().param("value", Param::integer().required()));
        match description {
            Some(text) => object.description(text),
            None => object,
        }
    };
    let by_hand = Definition::new("documented_tool")
        .description("Reads a file.\n\nSecond paragraph, indented.")
        .param("path", Param::string().required().description("The path."))
        .param(
            "other",
            Param::integer().required().description("Explicit wins."),
        )
        .param(
            "third",
            Param::integer().description("Not trimmed to nothing.\n\nTwo paragraphs."),
        )
        .param(
            "nested",
            nested(Some("Nested struct description."))
                .required()
                .description("A nested object."),
        )
        .param(
            "nested_own",
            nested(Some("Nested struct description."))
                .required()
                .description("Field text wins."),
        )
        .param("plain", dims().required());
    assert_eq!(DocumentedTool::tool_definition(), by_hand);
    // Without a field description, the nested struct's own description is used.
    assert_eq!(
        <DocumentedNested as ToolParameter>::parameter(),
        nested(Some("Nested struct description."))
    );
    assert_eq!(
        <DocumentedEnum as ToolParameter>::parameter(),
        Param::enumeration(&["One"]).description("The enum's own text.")
    );
    assert_eq!(
        ExplicitOverDoc::tool_definition(),
        Definition::new("explicit_over_doc").description("Explicit tool description.")
    );
    assert_eq!(
        Escapes::tool_definition(),
        Definition::new("escapes").description("line1\nline2 \"quoted\" \u{e9} \\ tab\t")
    );
    assert_eq!(
        RawEscapes::tool_definition(),
        Definition::new("raw_escapes").description("raw \"x\" \\n")
    );
    compile([DocumentedTool::tool_definition()]);
}

// ----------------------------------------------------------------- defaults

#[derive(ToolDefinition, Debug, PartialEq)]
struct Defaults {
    #[param(default = 0)]
    count: u8,
    #[param(default = -1.5e3)]
    ratio: f64,
    #[param(default = 2.50)]
    exact: Number,
    #[param(default = true)]
    flag: bool,
    #[param(default = "a\n\u{1F600}\"b\\")]
    text: String,
    #[param(default = r#"raw "q""#)]
    raw: String,
    #[param(default = [1, 2, 3,])]
    list: Vec<u8>,
    #[param(default = "dflt")]
    maybe: Option<String>,
    #[param(default = { "width": 1, "height": 2 })]
    dims: Dims,
    #[param(default = [{ "width": 7 }, { "width": 8, "height": 9 }])]
    many: Vec<Dims>,
    #[param(default = [])]
    empty: Vec<String>,
    #[param(default = -0)]
    negative_zero: i64,
    #[param(default = 123456789012345678901234567890)]
    huge: Number,
    #[param(default = 1E+2)]
    hundred: u32,
    #[param(default = "Px")]
    unit: Unit,
}

#[test]
fn absent_members_convert_to_their_defaults() {
    let expected = Defaults {
        count: 0,
        ratio: -1500.0,
        exact: "2.50".parse().unwrap(),
        flag: true,
        text: "a\n\u{1F600}\"b\\".into(),
        raw: "raw \"q\"".into(),
        list: vec![1, 2, 3],
        maybe: Some("dflt".into()),
        dims: Dims {
            width: 1,
            height: Some(2),
        },
        many: vec![
            Dims {
                width: 7,
                height: None,
            },
            Dims {
                width: 8,
                height: Some(9),
            },
        ],
        empty: vec![],
        negative_zero: 0,
        huge: "123456789012345678901234567890".parse().unwrap(),
        hundred: 100,
        unit: Unit::Px,
    };
    assert_eq!(Defaults::from_node(&parse("{}")), Ok(expected));
    // A present member is used, not the default; explicit null for an Option.
    let node = parse(r#"{"count":9,"maybe":null,"list":[],"flag":false}"#);
    let converted = Defaults::from_node(&node).unwrap();
    assert_eq!(
        (
            converted.count,
            converted.maybe,
            converted.list,
            converted.flag
        ),
        (9, None, vec![], false)
    );
}

#[test]
fn defaults_are_declared_in_the_definition_and_pass_compilation() {
    let tools = compile([Defaults::tool_definition()]);
    let defaults: Vec<(String, String)> = tools
        .defaults("defaults")
        .unwrap()
        .iter()
        .map(|default| {
            let ShapeStep::Parameter(name) = &default.path[0] else {
                panic!()
            };
            (name.clone(), default.value.to_json())
        })
        .collect();
    assert_eq!(defaults[0], ("count".into(), "0".into()));
    assert_eq!(defaults[1], ("ratio".into(), "-1.5e3".into()));
    assert_eq!(defaults[2], ("exact".into(), "2.50".into()));
    assert_eq!(defaults[4], ("text".into(), r#""a\n😀\"b\\""#.into()));
    assert_eq!(defaults[6], ("list".into(), "[1,2,3]".into()));
    assert_eq!(
        defaults[8],
        ("dims".into(), r#"{"width":1,"height":2}"#.into())
    );
    assert_eq!(
        defaults[9],
        (
            "many".into(),
            r#"[{"width":7},{"width":8,"height":9}]"#.into()
        )
    );
    assert_eq!(defaults[11], ("negative_zero".into(), "-0".into()));
    assert_eq!(defaults[12].1, "123456789012345678901234567890");
    // A defaulted parameter is not required, `Option` or not.
    let schema = tools.render_json_schema("defaults").unwrap();
    let document = tools.tool("defaults").unwrap().schema_document();
    assert!(document.get("required").is_none(), "{schema}");
}

#[derive(ToolDefinition, Debug, PartialEq)]
struct WrongDefault {
    #[param(default = "x")]
    n: u32,
}

#[derive(ToolDefinition, Debug, PartialEq)]
struct NullDefault {
    #[param(default = null)]
    maybe: Option<u8>,
}

#[test]
fn a_default_of_the_wrong_type_is_caught_twice() {
    let compiled = ToolSet::compile([WrongDefault::tool_definition()], Budget::default());
    assert!(
        matches!(&compiled, Err(ToolSetError::InvalidDefinition { tool, .. }) if tool == "wrong_default"),
        "{compiled:?}"
    );
    assert_eq!(
        WrongDefault::from_node(&parse("{}")),
        Err(error("/n", "integer within the u32 range", "string"))
    );
    // `null` is not a valid default of an optional integer either; the
    // conversion of an absent member would yield `None` from it.
    assert!(ToolSet::compile([NullDefault::tool_definition()], Budget::default()).is_err());
    assert_eq!(
        NullDefault::from_node(&parse("{}")),
        Ok(NullDefault { maybe: None })
    );
}

// ------------------------------------------------------------- extra / pass

#[derive(ToolDefinition, Debug, PartialEq)]
#[tool(unknown = pass)]
struct WithExtra {
    name: String,
    #[param(extra)]
    rest: Node,
}

#[test]
fn undeclared_members_land_in_the_extra_field_through_recovery() {
    let tools = compile([WithExtra::tool_definition()]);
    let arguments = recovered(
        &tools,
        DialectId::Hermes,
        &hermes(
            "with_extra",
            r#"{"zeta": 1, "name": "n", "alpha": [2, {"k": null}]}"#,
        ),
    );
    let converted = WithExtra::from_node(&arguments).unwrap();
    assert_eq!(converted.name, "n");
    assert_eq!(
        converted.rest.to_json(),
        r#"{"zeta":1,"alpha":[2,{"k":null}]}"#
    );
    // No undeclared members: an empty object, not null.
    let none = WithExtra::from_node(&parse(r#"{"name":"n"}"#)).unwrap();
    assert_eq!(none.rest.to_json(), "{}");
    // Duplicates of undeclared members are kept; a declared duplicate is an error.
    let twice = WithExtra::from_node(&parse(r#"{"name":"n","x":1,"x":2}"#)).unwrap();
    assert_eq!(twice.rest.to_json(), r#"{"x":1,"x":2}"#);
    assert_eq!(
        WithExtra::from_node(&parse(r#"{"name":"n","name":"m"}"#)),
        Err(error("/name", "a single occurrence", "duplicate member"))
    );
    assert_eq!(
        WithExtra::tool_definition(),
        Definition::new("with_extra")
            .unknown_parameters(UnknownParameters::PassThrough)
            .param("name", Param::string().required())
    );
}

// ------------------------------------------------------------- D-8 ranges

#[derive(ToolDefinition, Debug, PartialEq)]
struct Ranged {
    count: u8,
}

#[test]
fn integer_ranges_are_enforced_by_from_node_only() {
    let tools = compile([Ranged::tool_definition()]);
    let arguments = recovered(
        &tools,
        DialectId::Hermes,
        &hermes("ranged", r#"{"count": 300}"#),
    );
    assert_eq!(
        arguments.to_json(),
        r#"{"count":300}"#,
        "recovery and the schema accept 300"
    );
    assert!(
        tools
            .tool("ranged")
            .unwrap()
            .schema()
            .validate(&arguments)
            .unwrap()
            .valid
    );
    assert_eq!(
        Ranged::from_node(&arguments),
        Err(error("/count", "integer within the u8 range", "number 300"))
    );
    assert_eq!(
        Ranged::from_node(&parse(r#"{"count": 255}"#)),
        Ok(Ranged { count: 255 })
    );
    assert_eq!(
        Ranged::from_node(&parse(r#"{"count": 1.5}"#)),
        Err(error("/count", "integer within the u8 range", "number 1.5"))
    );
}

// ------------------------------------------- FN1-FN3 through the real pipeline

#[derive(ToolEnum, Debug, PartialEq)]
enum Season {
    #[param(name = "winter")]
    Winter,
    #[param(name = "summer")]
    Summer,
}

/// Where and what.
#[derive(ToolDefinition, Debug, PartialEq)]
struct Outfit {
    wearer: String,
    garments: Vec<String>,
    season: Option<Season>,
}

#[derive(ToolDefinition, Debug, PartialEq)]
#[tool(name = "plan_outfits", effects = read_only)]
struct Planner {
    outfits: Vec<Outfit>,
    lead: Option<Outfit>,
    seasons: Vec<Season>,
    count: Option<u16>,
}

fn expected_outfits() -> Planner {
    Planner {
        outfits: vec![
            Outfit {
                wearer: "Alice".into(),
                garments: vec!["a red coat".into()],
                season: Some(Season::Winter),
            },
            Outfit {
                wearer: "Bob".into(),
                garments: vec!["a grey scarf".into(), "boots".into()],
                season: None,
            },
        ],
        lead: Some(Outfit {
            wearer: "Cy".into(),
            garments: vec![],
            season: None,
        }),
        seasons: vec![Season::Summer, Season::Winter],
        count: Some(2),
    }
}

#[test]
fn from_node_reads_a_hermes_turn() {
    let tools = compile([Planner::tool_definition()]);
    let markup = hermes(
        "plan_outfits",
        r#"{"outfits": [
            {"wearer": "Alice", "garments": ["a red coat"], "season": "winter"},
            {"wearer": "Bob", "garments": ["a grey scarf", "boots"]}],
          "lead": {"wearer": "Cy", "garments": []},
          "seasons": ["summer", "winter"], "count": 2}"#,
    );
    let arguments = recovered(&tools, DialectId::Hermes, &markup);
    assert_eq!(Planner::from_node(&arguments), Ok(expected_outfits()));
}

#[test]
fn from_node_reads_a_qwen3_coder_turn_with_nested_items() {
    let tools = compile([Planner::tool_definition()]);
    let markup = "<tool_call>\n<function=plan_outfits>\n\
        <parameter=outfits>\n<parameter=wearer>\nAlice\n</parameter>\n\
        <parameter=garments>\n[\"a red coat\"]\n</parameter>\n\
        <parameter=season>\nwinter\n</parameter>\n</parameter>\n\
        <parameter=outfits>\n<parameter=wearer>\nBob\n</parameter>\n\
        <parameter=garments>\n[\"a grey scarf\", \"boots\"]\n</parameter>\n</parameter>\n\
        <parameter=lead>\n<parameter=wearer>\nCy\n</parameter>\n<parameter=garments>\n[]\n</parameter>\n</parameter>\n\
        <parameter=seasons>\n[\"summer\", \"winter\"]\n</parameter>\n\
        <parameter=count>\n2\n</parameter>\n</function>\n</tool_call>";
    let arguments = recovered(&tools, DialectId::Qwen3CoderXml, markup);
    assert_eq!(Planner::from_node(&arguments), Ok(expected_outfits()));
}

#[test]
fn qwen_and_hermes_recover_equal_arguments() {
    let tools = compile([Planner::tool_definition()]);
    let qwen = "<tool_call>\n<function=plan_outfits>\n<parameter=outfits>\n<parameter=wearer>\nAlice\n</parameter>\n<parameter=garments>\n[]\n</parameter>\n</parameter>\n<parameter=seasons>\n[]\n</parameter>\n</function>\n</tool_call>";
    let json = hermes(
        "plan_outfits",
        r#"{"outfits": [{"wearer": "Alice", "garments": []}], "seasons": []}"#,
    );
    let from_qwen = Planner::from_node(&recovered(&tools, DialectId::Qwen3CoderXml, qwen)).unwrap();
    let from_json = Planner::from_node(&recovered(&tools, DialectId::Hermes, &json)).unwrap();
    assert_eq!(from_qwen, from_json);
    assert_eq!(from_json.outfits[0].wearer, "Alice");
}

// ---------------------------------------------------------------- negatives

#[derive(ToolDefinition, Debug, PartialEq)]
struct Check {
    name: String,
    count: u8,
    items: Vec<Item>,
    opt: Option<bool>,
    order: Option<Season>,
}

#[derive(ToolDefinition, Debug, PartialEq)]
struct Item {
    sub: String,
}

fn check(text: &str) -> Result<Check, FromNodeError> {
    Check::from_node(&parse(text))
}

#[test]
fn from_node_reports_each_failure_at_its_pointer() {
    let good = r#""name":"n","count":1,"items":[]"#;
    assert_eq!(
        check(&format!("{{{good}}}")),
        Ok(Check {
            name: "n".into(),
            count: 1,
            items: vec![],
            opt: None,
            order: None
        })
    );
    // Missing, in declaration order.
    assert_eq!(
        check("{}"),
        Err(error("/name", "required member", "missing"))
    );
    assert_eq!(
        check(r#"{"name":"n"}"#),
        Err(error("/count", "required member", "missing"))
    );
    assert_eq!(
        check(r#"{"name":"n","count":1}"#),
        Err(error("/items", "required member", "missing"))
    );
    // Duplicate.
    assert_eq!(
        check(&format!(r#"{{{good},"name":"m"}}"#)),
        Err(error("/name", "a single occurrence", "duplicate member"))
    );
    assert_eq!(
        check(&format!(r#"{{{good},"opt":true,"opt":false}}"#)),
        Err(error("/opt", "a single occurrence", "duplicate member"))
    );
    // Undeclared.
    assert_eq!(
        check(&format!(r#"{{{good},"zzz":1}}"#)),
        Err(error("/zzz", "a declared member", "undeclared member"))
    );
    // Only canonical names are read (aliases are resolved by recovery).
    assert_eq!(
        check(&format!(r#"{{{good},"Name":1}}"#)),
        Err(error("/Name", "a declared member", "undeclared member"))
    );
    // Wrong type.
    assert_eq!(
        check(r#"{"name":5,"count":1,"items":[]}"#),
        Err(error("/name", "string", "number 5"))
    );
    assert_eq!(
        check(r#"{"name":"n","count":"1","items":[]}"#),
        Err(error("/count", "integer within the u8 range", "string"))
    );
    assert_eq!(
        check(&format!(r#"{{{good},"order":"ASC"}}"#)),
        Err(error("/order", "one of \"winter\", \"summer\"", "string"))
    );
    assert_eq!(
        check(&format!(r#"{{{good},"order":5}}"#)),
        Err(error("/order", "one of \"winter\", \"summer\"", "number 5"))
    );
    // Nested pointers.
    assert_eq!(
        check(r#"{"name":"n","count":1,"items":[{"sub":"a"},{"sub":7}]}"#),
        Err(error("/items/1/sub", "string", "number 7"))
    );
    assert_eq!(
        check(r#"{"name":"n","count":1,"items":[{}]}"#),
        Err(error("/items/0/sub", "required member", "missing"))
    );
    assert_eq!(
        check(r#"{"name":"n","count":1,"items":[{"sub":"a","sub":"b"}]}"#),
        Err(error(
            "/items/0/sub",
            "a single occurrence",
            "duplicate member"
        ))
    );
    assert_eq!(
        check(r#"{"name":"n","count":1,"items":[{"sub":"a","x":1}]}"#),
        Err(error(
            "/items/0/x",
            "a declared member",
            "undeclared member"
        ))
    );
    assert_eq!(
        check(r#"{"name":"n","count":1,"items":{}}"#),
        Err(error("/items", "array", "object with 0 members"))
    );
    // Not an object.
    assert_eq!(check("[]"), Err(error("", "object", "array of 0 items")));
    assert_eq!(check("null"), Err(error("", "object", "null")));
    // Option: null and absence are both None.
    let explicit = check(&format!(r#"{{{good},"opt":null,"order":null}}"#)).unwrap();
    assert_eq!((explicit.opt, explicit.order), (None, None));
}

#[derive(ToolDefinition, Debug, PartialEq)]
struct Pair {
    first: String,
    second: String,
}

#[test]
fn a_duplicate_is_not_reported_as_missing() {
    let node = parse(r#"{"first":"a","first":"b","second":"c"}"#);
    // `Node::get` cannot tell the two situations apart: both are `None`.
    assert!(node.get("first").is_none());
    assert!(parse("{}").get("first").is_none());
    assert_eq!(
        Pair::from_node(&node),
        Err(error("/first", "a single occurrence", "duplicate member"))
    );
    // Even when the duplicated member is the only thing wrong, and when the
    // other member is also absent, the duplicate is reported first.
    assert_eq!(
        Pair::from_node(&parse(r#"{"first":"a","first":"b"}"#)),
        Err(error("/first", "a single occurrence", "duplicate member"))
    );
    assert_eq!(
        Pair::from_node(&parse(r#"{"second":"c"}"#)),
        Err(error("/first", "required member", "missing"))
    );
}

#[test]
fn enum_conversion_is_exact() {
    for node in ["\"ASC\"", "5", "null", "true", "[]", "\"\""] {
        let result = Season::from_node(&parse(node));
        let failure = result.unwrap_err();
        assert_eq!(failure.pointer, "");
        assert_eq!(failure.expected, "one of \"winter\", \"summer\"");
    }
    assert_eq!(
        Season::from_node(&parse("5")).unwrap_err().found,
        "number 5"
    );
    assert_eq!(Season::from_node(&parse("null")).unwrap_err().found, "null");
    assert_eq!(Season::from_node(&parse(r#""summer""#)), Ok(Season::Summer));
    assert!(Season::from_node(&parse(r#"" summer""#)).is_err());
}

// ----------------------------------------------------------- zero parameters

#[derive(ToolDefinition, Debug, PartialEq)]
struct Ping;

#[derive(ToolDefinition, Debug, PartialEq)]
struct Pong {}

#[test]
fn unit_and_empty_structs_are_tools_without_parameters() {
    assert_eq!(Ping::tool_definition(), Definition::new("ping"));
    assert_eq!(Pong::tool_definition(), Definition::new("pong"));
    assert_eq!(Ping::from_node(&parse("{}")), Ok(Ping));
    assert_eq!(Pong::from_node(&parse("{}")), Ok(Pong {}));
    assert_eq!(
        Ping::from_node(&parse(r#"{"a":1}"#)),
        Err(error("/a", "a declared member", "undeclared member"))
    );
    assert_eq!(
        Ping::from_node(&parse("3")),
        Err(error("", "object", "number 3"))
    );
    compile([Ping::tool_definition(), Pong::tool_definition()]);
}

// ------------------------------------------------------------ nested options

#[derive(ToolDefinition, Debug, PartialEq)]
#[tool(name = "ignored", alias = "ignored2", effects = read_only, policy = tolerant, unknown = pass)]
struct NestedOpts {
    value: u8,
    #[param(extra)]
    rest: Node,
}

#[derive(ToolDefinition, Debug, PartialEq)]
struct OuterOfNested {
    inner: NestedOpts,
}

#[test]
fn tool_keys_on_a_nested_struct_are_ignored_except_unknown() {
    assert_eq!(
        <NestedOpts as ToolParameter>::parameter(),
        Param::object(
            ObjectSpec::new()
                .unknown_parameters(UnknownParameters::PassThrough)
                .param("value", Param::integer().required())
        )
    );
    assert_eq!(
        OuterOfNested::from_node(&parse(r#"{"inner":{"value":1,"more":[true]}}"#))
            .unwrap()
            .inner
            .rest
            .to_json(),
        r#"{"more":[true]}"#
    );
    compile([OuterOfNested::tool_definition()]);
}

// ----------------------------------------------------------- determinism

mod first {
    use purrassist_derive::ToolDefinition;
    #[derive(ToolDefinition)]
    pub(crate) struct Twin {
        pub(crate) a: u8,
        #[param(default = [1, 2])]
        pub(crate) b: Vec<u8>,
    }
}

mod second {
    use purrassist_derive::ToolDefinition;
    #[derive(ToolDefinition)]
    pub(crate) struct Twin {
        pub(crate) a: u8,
        #[param(default = [1, 2])]
        pub(crate) b: Vec<u8>,
    }
}

#[test]
fn expansion_is_a_pure_function_of_the_input() {
    use purrassist_core::tool_call::ToolArguments as _;
    assert_eq!(
        first::Twin::tool_definition(),
        second::Twin::tool_definition()
    );
}

// ---------------------------------------------------------------- hygiene

mod hygiene {
    #![allow(
        non_upper_case_globals,
        non_camel_case_types,
        dead_code,
        unused_imports
    )]
    use ::purrassist_core::json::Node;
    use ::purrassist_core::tool_call::{FromNode, ToolArguments};
    use ::purrassist_derive::{ToolDefinition, ToolEnum};

    // Items the generated code could capture if it were unhygienic.
    const node: u8 = 0;
    struct members;
    struct member;
    struct slot;
    struct extra;
    const spec: u8 = 0;
    const value: u8 = 0;
    const error: u8 = 0;
    mod core {}
    mod std {}
    mod purrassist_core {}
    mod purrassist {}
    struct Slot;
    struct Members;
    struct FromNodeError;
    struct Node_;

    #[derive(ToolEnum, Debug, PartialEq)]
    enum Mode {
        A,
        B,
    }

    #[derive(ToolDefinition, Debug, PartialEq)]
    #[tool(unknown = pass)]
    struct Hygienic {
        node: ::std::string::String,
        members: ::core::option::Option<u8>,
        member: ::std::vec::Vec<u8>,
        slot: Mode,
        error: ::core::option::Option<Mode>,
        #[param(default = [1])]
        value: ::std::vec::Vec<u8>,
        spec: bool,
        #[param(extra)]
        extra: Node,
    }

    #[test]
    fn generated_code_ignores_shadowing_items() {
        let definition = Hygienic::tool_definition();
        assert_eq!(definition.name(), "hygienic");
        let converted = Hygienic::from_node(&purrassist_core_shadow_free_parse(
            r#"{"node":"n","member":[1],"slot":"A","spec":true,"more":1}"#,
        ))
        .unwrap();
        assert_eq!(converted.node, "n");
        assert_eq!(converted.members, None);
        assert_eq!(converted.value, [1]);
        assert_eq!(converted.slot, Mode::A);
        assert_eq!(converted.extra.to_json(), r#"{"more":1}"#);
    }

    // `purrassist_core` is shadowed by a local module here, so reach the real
    // crate through an absolute path.
    fn purrassist_core_shadow_free_parse(text: &str) -> ::purrassist_core::json::Node {
        ::purrassist_core::json::parse(text).unwrap()
    }
}
