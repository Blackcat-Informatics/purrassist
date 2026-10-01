// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Typed tool definitions: names, collisions, defaults, budgets, order
//! independence, exact conversion, argument shapes, and rendered schemas.
use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason,
    contract::Resource,
    json::{self, Node},
    tool_call::{
        CompiledTool, Effects, FromNode, NameLiteral, ObjectSpec, Param, RepairPolicy, ShapeStep,
        ToolDefinition, ToolSet, ToolSetError, UnknownParameters, ValueShape,
    },
};

fn parse(text: &str) -> Node {
    json::parse(text).unwrap()
}

fn compile(definitions: impl IntoIterator<Item = ToolDefinition>) -> Result<ToolSet, ToolSetError> {
    ToolSet::compile(definitions, Budget::default())
}

fn invalid_reason(result: Result<ToolSet, ToolSetError>) -> (String, String) {
    match result {
        Err(ToolSetError::InvalidDefinition { tool, reason }) => (tool, reason),
        other => panic!("expected an invalid definition, got {other:?}"),
    }
}

/// A representative set exercising every parameter type and modifier.
fn catalog() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new("read_file")
            .alias("cat")
            .description("Read a file")
            .effects(Effects::ReadOnly)
            .param("path", Param::string().required().alias("file"))
            .param("limit", Param::integer().default(parse("100")))
            .param("encoding", Param::enumeration(&["utf-8", "latin-1"])),
        ToolDefinition::new("write_file")
            .param("path", Param::string().required().pattern("^[^\\u0000]+$"))
            .param("content", Param::string().required().alias("text"))
            .param(
                "mode",
                Param::enumeration(&["overwrite", "append"]).default(parse(r#""overwrite""#)),
            ),
        ToolDefinition::new("search")
            .effects(Effects::ReadOnly)
            .unknown_parameters(UnknownParameters::PassThrough)
            .param("query", Param::string().required().alias("q"))
            .param(
                "filters",
                Param::array(Param::object(
                    ObjectSpec::new()
                        .param("field", Param::string().required())
                        .param("value", Param::string().alias("equals"))
                        .unknown_parameters(UnknownParameters::Error),
                )),
            )
            .param("tag", Param::enumeration(&["doc", "code"]).repeatable())
            .param(
                "options",
                Param::object(
                    ObjectSpec::new()
                        .param("depth", Param::integer().default(parse("2")))
                        .param("ratio", Param::number()),
                ),
            ),
    ]
}

#[test]
fn rejects_empty_and_invalid_names() {
    for name in ["", "read file", "read/file", "lire_é", "tab\t", "a#b"] {
        let (tool, reason) = invalid_reason(compile([ToolDefinition::new(name)]));
        assert_eq!(tool, name);
        assert!(reason.contains("[A-Za-z0-9_.:-]+"), "{reason}");
    }
    let (tool, _) = invalid_reason(compile([ToolDefinition::new("ok").alias("not ok")]));
    assert_eq!(tool, "ok");
    for name in ["", "two words"] {
        let (tool, reason) = invalid_reason(compile([
            ToolDefinition::new("ok").param(name, Param::string())
        ]));
        assert_eq!(tool, "ok");
        assert!(reason.contains("parameter name or alias"), "{reason}");
    }
    let (_, reason) = invalid_reason(compile([
        ToolDefinition::new("ok").param("x", Param::string().alias("bad alias"))
    ]));
    assert!(reason.contains("#/bad alias"), "{reason}");
    // No length cap: a long but valid name compiles.
    let long = "n".repeat(10_000);
    assert!(
        compile([ToolDefinition::new(long.clone())])
            .unwrap()
            .tool(&long)
            .is_some()
    );
    assert!(compile([ToolDefinition::new("a.b:c-d_9")]).is_ok());
}

#[test]
fn rejects_normalization_collisions() {
    let error = compile([
        ToolDefinition::new("read_file"),
        ToolDefinition::new("Read-File"),
    ])
    .unwrap_err();
    assert_eq!(
        error,
        ToolSetError::Collision {
            first: "Read-File".into(),
            second: "read_file".into(),
            key: "read_file".into(),
        }
    );
    for (left, right) in [("a.b", "A:B"), ("x-y", "x_y"), ("same", "same")] {
        assert!(matches!(
            compile([ToolDefinition::new(left), ToolDefinition::new(right)]),
            Err(ToolSetError::Collision { .. })
        ));
    }
    // Runs of separators are not collapsed.
    assert!(compile([ToolDefinition::new("a--b"), ToolDefinition::new("a-b")]).is_ok());
}

#[test]
fn alias_collisions_rejected() {
    // Alias against another tool's name.
    assert_eq!(
        compile([
            ToolDefinition::new("list").alias("Read.File"),
            ToolDefinition::new("read_file"),
        ])
        .unwrap_err(),
        ToolSetError::Collision {
            first: "Read.File".into(),
            second: "read_file".into(),
            key: "read_file".into(),
        }
    );
    // Alias against another tool's alias.
    assert!(matches!(
        compile([
            ToolDefinition::new("a").alias("shared"),
            ToolDefinition::new("b").alias("SHARED"),
        ]),
        Err(ToolSetError::Collision { key, .. }) if key == "shared"
    ));
    // Alias against the tool's own name.
    assert!(matches!(
        compile([ToolDefinition::new("ls").alias("LS")]),
        Err(ToolSetError::Collision { .. })
    ));
    // Distinct aliases resolve exactly and only exactly.
    let tools = compile([ToolDefinition::new("ls").alias("dir").alias("list")]).unwrap();
    assert_eq!(tools.lookup_exact("list").unwrap().name(), "ls");
    assert!(tools.lookup_exact("LIST").is_none());
}

#[test]
fn parameter_name_collisions_rejected() {
    assert_eq!(
        compile([ToolDefinition::new("t")
            .param("file_path", Param::string())
            .param("File-Path", Param::string())])
        .unwrap_err(),
        ToolSetError::Collision {
            first: "t#/File-Path".into(),
            second: "t#/file_path".into(),
            key: "file_path".into(),
        }
    );
    // A parameter alias colliding with a sibling's name.
    assert!(matches!(
        compile([ToolDefinition::new("t")
            .param("path", Param::string())
            .param("dest", Param::string().alias("PATH"))]),
        Err(ToolSetError::Collision { .. })
    ));
    // Nested scopes are separate, but collide internally, including inside arrays.
    assert!(
        compile([ToolDefinition::new("t").param("x", Param::string()).param(
            "o",
            Param::object(ObjectSpec::new().param("x", Param::string()))
        )])
        .is_ok()
    );
    assert_eq!(
        compile([ToolDefinition::new("t").param(
            "rows",
            Param::array(Param::object(
                ObjectSpec::new()
                    .param("a.b", Param::string())
                    .param("a_b", Param::string())
            ))
        )])
        .unwrap_err(),
        ToolSetError::Collision {
            first: "t#/rows[]/a.b".into(),
            second: "t#/rows[]/a_b".into(),
            key: "a_b".into(),
        }
    );
}

#[test]
fn defaults_exposed_not_applied() {
    let tools = compile(catalog()).unwrap();
    let defaults = tools.defaults("search").unwrap();
    assert_eq!(defaults.len(), 1);
    assert_eq!(
        defaults[0].path,
        [
            ShapeStep::Parameter("options".into()),
            ShapeStep::Parameter("depth".into())
        ]
    );
    assert_eq!(defaults[0].value.to_json(), "2");
    let read = tools.tool("read_file").unwrap();
    assert_eq!(
        read.defaults()[0].path,
        [ShapeStep::Parameter("limit".into())]
    );
    assert_eq!(
        read.argument_shape()
            .arguments
            .parameter("limit")
            .unwrap()
            .default,
        Some(parse("100"))
    );
    assert!(
        read.json_schema()
            .contains(r#""limit":{"type":"integer","default":100}"#)
    );
    // Not applied: the schema still accepts arguments that omit the parameter,
    // and validating them does not synthesize the default.
    let arguments = parse(r#"{"path":"a"}"#);
    assert!(read.schema().validate(&arguments).unwrap().valid);
    assert!(arguments.get("limit").is_none());
    // Defaults of required parameters are also exposed but never satisfy `required`.
    let tools = compile([
        ToolDefinition::new("t").param("n", Param::integer().required().default(parse("1")))
    ])
    .unwrap();
    assert!(
        !tools
            .tool("t")
            .unwrap()
            .schema()
            .validate(&parse("{}"))
            .unwrap()
            .valid
    );
    assert_eq!(tools.defaults("t").unwrap().len(), 1);
    assert!(tools.defaults("missing").is_none());
}

#[test]
fn invalid_default_rejected() {
    let cases = [
        Param::integer().default(parse(r#""10""#)),
        Param::integer().default(parse("1.5")),
        Param::boolean().default(parse("0")),
        Param::enumeration(&["a", "b"]).default(parse(r#""c""#)),
        Param::string().pattern("^[0-9]+$").default(parse(r#""x""#)),
        Param::array(Param::integer()).default(parse(r#"[1,"two"]"#)),
        Param::object(ObjectSpec::new().param("k", Param::string().required()))
            .default(parse("{}")),
        Param::object(ObjectSpec::new()).default(parse(r#"{"extra":1}"#)),
    ];
    for spec in cases {
        let (tool, reason) = invalid_reason(compile([ToolDefinition::new("t").param("p", spec)]));
        assert_eq!(tool, "t");
        assert!(
            reason.contains("#/p") && reason.contains("does not conform"),
            "{reason}"
        );
    }
    // Integral spellings and pass-through objects conform.
    assert!(
        compile([ToolDefinition::new("t").param("p", Param::integer().default(parse("1e2")))])
            .is_ok()
    );
    assert!(
        compile([ToolDefinition::new("t")
            .unknown_parameters(UnknownParameters::PassThrough)
            .param(
                "p",
                Param::object(ObjectSpec::new()).default(parse(r#"{"extra":1}"#))
            )])
        .is_ok()
    );
}

#[test]
fn invalid_pattern_rejected() {
    let (tool, reason) = invalid_reason(compile([
        ToolDefinition::new("t").param("p", Param::string().pattern("(unclosed"))
    ]));
    assert_eq!(tool, "t");
    assert!(reason.contains("invalid pattern `(unclosed`"), "{reason}");
    let (_, reason) = invalid_reason(compile([
        ToolDefinition::new("t").param("p", Param::array(Param::string().pattern("[z-a]")))
    ]));
    assert!(reason.contains("#/p[]"), "{reason}");
    // Misplaced modifiers are definition errors too.
    for (spec, needle) in [
        (
            Param::integer().pattern("^1$"),
            "`pattern` applies only to string",
        ),
        (
            Param::enumeration(&["a"]).pattern("a"),
            "`pattern` applies only to string",
        ),
        (
            Param::array(Param::string().required()),
            "array item specification",
        ),
        (
            Param::array(Param::string().alias("x")),
            "array item specification",
        ),
        (
            Param::array(Param::string().default(parse(r#""x""#))),
            "array item specification",
        ),
        (Param::enumeration(&[]), "at least one value"),
        (
            Param::enumeration(&["a", "b", "a"]),
            "`a` is declared more than once",
        ),
    ] {
        let (_, reason) = invalid_reason(compile([ToolDefinition::new("t").param("p", spec)]));
        assert!(reason.contains(needle), "{reason}");
    }
}

fn zeroed(resource: Resource) -> Budget {
    let mut budget = Budget::default();
    match resource {
        Resource::Work => budget.work = 0,
        Resource::Candidates => budget.candidates = 0,
        Resource::InputBytes => budget.input_bytes = 0,
        Resource::ScratchBytes => budget.scratch_bytes = 0,
        Resource::Depth => budget.depth = 0,
        Resource::EvidenceEntries => budget.evidence_entries = 0,
        Resource::HelperCalls => budget.helper_calls = 0,
    }
    budget
}

#[test]
fn compile_budget_exhaustion_is_explicit() {
    // Dimensions compilation consumes fail explicitly and stickily.
    for resource in [
        Resource::Work,
        Resource::InputBytes,
        Resource::ScratchBytes,
        Resource::Depth,
    ] {
        let mut governor = Governor::new(zeroed(resource));
        assert_eq!(
            ToolSet::compile_governed(catalog(), &mut governor).unwrap_err(),
            ToolSetError::Resource(StopReason::BudgetExceeded(resource)),
            "{resource:?}"
        );
        assert_eq!(governor.check(), Err(StopReason::BudgetExceeded(resource)));
    }
    // Compilation proposes no candidates, records no evidence, and calls no
    // helpers, so zeroing those dimensions cannot stop it.
    for resource in [
        Resource::Candidates,
        Resource::EvidenceEntries,
        Resource::HelperCalls,
    ] {
        assert!(
            ToolSet::compile(catalog(), zeroed(resource)).is_ok(),
            "{resource:?}"
        );
    }
    // Every budget between zero and the measured requirement fails explicitly.
    let mut governor = Governor::new(Budget::default());
    ToolSet::compile_governed(catalog(), &mut governor).unwrap();
    let needed = governor.usage().work;
    assert!(needed > 0);
    for work in [needed / 4, needed / 2, needed - 1] {
        assert_eq!(
            ToolSet::compile(
                catalog(),
                Budget {
                    work,
                    ..Budget::default()
                }
            )
            .unwrap_err(),
            ToolSetError::Resource(StopReason::BudgetExceeded(Resource::Work))
        );
    }
    assert!(
        ToolSet::compile(
            catalog(),
            Budget {
                work: needed,
                ..Budget::default()
            }
        )
        .is_ok()
    );
    // Deep nesting is bounded by the depth budget, not a constant cap.
    let mut nested = Param::string();
    for _ in 0..8 {
        nested = Param::object(ObjectSpec::new().param("inner", nested));
    }
    let deep = || [ToolDefinition::new("deep").param("outer", nested.clone())];
    assert_eq!(
        ToolSet::compile(
            deep(),
            Budget {
                depth: 5,
                ..Budget::default()
            }
        )
        .unwrap_err(),
        ToolSetError::Resource(StopReason::BudgetExceeded(Resource::Depth))
    );
    assert!(
        ToolSet::compile(
            deep(),
            Budget {
                depth: 32,
                ..Budget::default()
            }
        )
        .is_ok()
    );
}

#[test]
fn compile_cancelled() {
    let token = CancellationToken::new();
    token.cancel();
    let mut governor = Governor::with_cancellation(Budget::default(), token);
    assert_eq!(
        ToolSet::compile_governed(catalog(), &mut governor).unwrap_err(),
        ToolSetError::Resource(StopReason::Cancelled)
    );
    let mut governor = Governor::new(Budget::default());
    governor.cancel();
    assert_eq!(
        ToolSet::compile_governed(Vec::new(), &mut governor).unwrap_err(),
        ToolSetError::Resource(StopReason::Cancelled)
    );
}

fn permutations(items: &[ToolDefinition]) -> Vec<Vec<ToolDefinition>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut result = Vec::new();
    for index in 0..items.len() {
        let mut rest = items.to_vec();
        let first = rest.remove(index);
        for mut tail in permutations(&rest) {
            tail.insert(0, first.clone());
            result.push(tail);
        }
    }
    result
}

#[test]
fn compile_independent_of_declaration_order() {
    let reference = compile(catalog()).unwrap();
    let names: Vec<_> = reference.tools().iter().map(CompiledTool::name).collect();
    assert_eq!(names, ["read_file", "search", "write_file"]);
    let probes = [
        "cat",
        "read_file",
        "search",
        "write_file",
        "missing",
        "Search",
    ];
    for order in permutations(&catalog()) {
        let tools = compile(order).unwrap();
        for (left, right) in reference.tools().iter().zip(tools.tools()) {
            assert_eq!(left.name(), right.name());
            assert_eq!(left.json_schema(), right.json_schema());
            assert_eq!(left.argument_shape(), right.argument_shape());
            assert_eq!(left.defaults(), right.defaults());
            assert_eq!(left.aliases(), right.aliases());
            assert_eq!(left.schema().digest(), right.schema().digest());
        }
        for probe in probes {
            assert_eq!(
                reference.lookup_exact(probe).map(CompiledTool::name),
                tools.lookup_exact(probe).map(CompiledTool::name)
            );
        }
    }
    // Errors are order-independent too: the same collision is reported.
    let colliding = [
        ToolDefinition::new("b_x"),
        ToolDefinition::new("a"),
        ToolDefinition::new("B-X"),
        ToolDefinition::new("A.x").alias("b.X"),
    ];
    let expected = compile(colliding.clone()).unwrap_err();
    for order in permutations(&colliding) {
        assert_eq!(compile(order).unwrap_err(), expected);
    }
}

#[test]
fn from_node_exact_integer_ranges() {
    macro_rules! boundaries {
        ($($target:ty),*) => {$(
            let min = <$target>::MIN;
            let max = <$target>::MAX;
            assert_eq!(<$target>::from_node(&parse(&min.to_string())), Ok(min));
            assert_eq!(<$target>::from_node(&parse(&max.to_string())), Ok(max));
            let above = u128::try_from(max).unwrap().checked_add(1).map_or_else(
                || "340282366920938463463374607431768211456".to_owned(),
                |value| value.to_string(),
            );
            let error = <$target>::from_node(&parse(&above)).unwrap_err();
            assert_eq!(error.expected, concat!("integer within the ", stringify!($target), " range"));
            assert_eq!(error.found, format!("number {above}"));
            let below = if min == 0 as $target {
                "-1".to_owned()
            } else {
                format!("-{}", i128::try_from(min).unwrap().unsigned_abs() + 1)
            };
            assert!(<$target>::from_node(&parse(&below)).is_err(), "{below}");
            assert_eq!(<$target>::from_node(&parse("1e1")), Ok(10 as $target));
            assert_eq!(<$target>::from_node(&parse("100.000")), Ok(100 as $target));
            assert_eq!(<$target>::from_node(&parse("1.2e1")), Ok(12 as $target));
            assert_eq!(<$target>::from_node(&parse("-0")), Ok(0 as $target));
            assert_eq!(<$target>::from_node(&parse("0e999999999999")), Ok(0 as $target));
            assert_eq!(<$target>::from_node(&parse("0.1e1")), Ok(1 as $target));
            for fractional in ["1.5", "1e-1", "0.5", "10.01"] {
                assert!(<$target>::from_node(&parse(fractional)).is_err(), "{fractional}");
            }
            assert!(<$target>::from_node(&parse("1e999999999999")).is_err());
            assert!(<$target>::from_node(&parse(r#""5""#)).is_err());
            assert!(<$target>::from_node(&parse("true")).is_err());
        )*};
    }
    boundaries!(i8, i16, i32, i64, isize, u8, u16, u32, u64, u128, usize);
    // i128's negative boundary exceeds every other width's magnitude.
    assert_eq!(
        i128::from_node(&parse(&i128::MIN.to_string())),
        Ok(i128::MIN)
    );
    assert_eq!(
        i128::from_node(&parse(&i128::MAX.to_string())),
        Ok(i128::MAX)
    );
    assert!(i128::from_node(&parse("170141183460469231731687303715884105728")).is_err());
    assert!(i128::from_node(&parse("-170141183460469231731687303715884105729")).is_err());
    assert!(i128::from_node(&parse("1.5")).is_err());
    // u128 overflow by one, written in exponent form as well.
    assert!(u128::from_node(&parse("340282366920938463463374607431768211456")).is_err());
    assert!(u128::from_node(&parse("1e39")).is_err());
    assert_eq!(u128::from_node(&parse("1e38")), Ok(10_u128.pow(38)));
    // Floats and exact numbers.
    assert_eq!(f64::from_node(&parse("0.1")), Ok(0.1));
    assert!(f64::from_node(&parse("1e400")).is_err());
    assert_eq!(
        json::Number::from_node(&parse("1.50")).unwrap().as_str(),
        "1.50"
    );
}

#[test]
fn from_node_nested_pointer() {
    let error = Vec::<Option<u8>>::from_node(&parse("[1, null, 300]")).unwrap_err();
    assert_eq!(error.pointer, "/2");
    assert_eq!(error.found, "number 300");
    let error = Vec::<Vec<bool>>::from_node(&parse("[[true], [false, 1]]")).unwrap_err();
    assert_eq!(error.pointer, "/1/1");
    assert_eq!(error.expected, "boolean");
    assert_eq!(
        error.to_string(),
        "at /1/1: expected boolean, found number 1"
    );
    let error = Option::<Vec<String>>::from_node(&parse(r#"["a", {"k": 1}]"#)).unwrap_err();
    assert_eq!(error.pointer, "/1");
    assert_eq!(error.found, "object with 1 members");
    let error = String::from_node(&parse("[]")).unwrap_err();
    assert_eq!(error.pointer, "");
    assert_eq!(
        error.to_string(),
        "at the root: expected string, found array of 0 items"
    );
    assert_eq!(
        Vec::<Option<i32>>::from_node(&parse("[null, -3]")),
        Ok(vec![None, Some(-3)])
    );
    assert_eq!(
        Node::from_node(&parse(r#"{"a":1}"#)).unwrap().to_json(),
        r#"{"a":1}"#
    );
}

#[test]
fn argument_grammar_lists_parameter_literals() {
    let tools = compile(catalog()).unwrap();
    let read = &tools.argument_shape("read_file").unwrap().arguments;
    assert_eq!(
        read.names,
        [
            NameLiteral {
                literal: "encoding".into(),
                canonical: "encoding".into(),
                alias: false
            },
            NameLiteral {
                literal: "file".into(),
                canonical: "path".into(),
                alias: true
            },
            NameLiteral {
                literal: "limit".into(),
                canonical: "limit".into(),
                alias: false
            },
            NameLiteral {
                literal: "path".into(),
                canonical: "path".into(),
                alias: false
            },
        ]
    );
    assert_eq!(read.resolve_name("file"), Some("path"));
    assert_eq!(read.required, ["path"]);
    assert_eq!(read.unknown_parameters, UnknownParameters::DropWithLoss);
    assert_eq!(
        read.parameter("encoding").unwrap().value,
        ValueShape::Enumeration(vec!["utf-8".into(), "latin-1".into()])
    );
    assert_eq!(read.parameter("limit").unwrap().value, ValueShape::Integer);
    let declared: Vec<_> = read
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect();
    assert_eq!(declared, ["path", "limit", "encoding"]);

    let write = &tools.argument_shape("write_file").unwrap().arguments;
    let ValueShape::String(Some(constraint)) = &write.parameter("path").unwrap().value else {
        panic!("expected a pattern constraint");
    };
    let mut governor = Governor::new(Budget::default());
    assert_eq!(
        constraint.pattern.is_match("a.txt", &mut governor),
        Ok(true)
    );
    assert_eq!(
        write.parameter("content").unwrap().value,
        ValueShape::String(None)
    );
    assert_eq!(write.resolve_name("text"), Some("content"));

    let search = tools.argument_shape("search").unwrap();
    assert_eq!(search.tool, "search");
    let arguments = &search.arguments;
    assert_eq!(arguments.unknown_parameters, UnknownParameters::PassThrough);
    let filters = arguments.parameter("filters").unwrap();
    // Array parameters accept repeated occurrences without the modifier.
    assert!(!filters.repeatable && !filters.required);
    // A repeatable scalar keeps its item shape; its argument value is an array.
    let tag = arguments.parameter("tag").unwrap();
    assert!(tag.repeatable);
    assert_eq!(
        tag.value,
        ValueShape::Enumeration(vec!["doc".into(), "code".into()])
    );
    let ValueShape::Array(item) = &filters.value else {
        panic!("expected an array");
    };
    let ValueShape::Object(row) = item.as_ref() else {
        panic!("expected object items");
    };
    assert_eq!(row.resolve_name("equals"), Some("value"));
    assert_eq!(row.required, ["field"]);
    assert_eq!(row.unknown_parameters, UnknownParameters::Error);
    let ValueShape::Object(options) = &arguments.parameter("options").unwrap().value else {
        panic!("expected an object");
    };
    // Nested objects inherit the tool's resolved unknown-parameter handling.
    assert_eq!(options.unknown_parameters, UnknownParameters::PassThrough);
    assert_eq!(
        options.parameter("ratio").unwrap().value,
        ValueShape::Number
    );
    assert_eq!(
        options.parameter("ratio").unwrap().value.type_name(),
        "number"
    );
}

#[test]
fn rendered_schema_validates_conforming_args_and_rejects_violations() {
    let tools = compile(catalog()).unwrap();
    assert_eq!(
        tools.render_json_schema("read_file").unwrap(),
        r#"{"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer","default":100},"encoding":{"type":"string","enum":["utf-8","latin-1"]}},"required":["path"],"additionalProperties":false}"#
    );
    let search_text = tools.render_json_schema("search").unwrap();
    assert!(search_text.contains(r#""filters":{"type":"array","items":{"type":"object","properties":{"field":{"type":"string"},"value":{"type":"string"}},"required":["field"],"additionalProperties":false}}"#), "{search_text}");
    assert!(
        search_text.ends_with(r#""required":["query"],"additionalProperties":true}"#),
        "{search_text}"
    );
    assert_eq!(
        tools.tool("search").unwrap().schema_document().to_json(),
        search_text
    );
    let cases = [
        ("read_file", r#"{"path":"a"}"#, true),
        (
            "read_file",
            r#"{"path":"a","limit":1e1,"encoding":"utf-8"}"#,
            true,
        ),
        ("read_file", r#"{"path":" a "}"#, true),
        ("read_file", "{}", false),
        ("read_file", r#"{"path":1}"#, false),
        ("read_file", r#"{"path":"a","limit":1.5}"#, false),
        ("read_file", r#"{"path":"a","encoding":"UTF-8"}"#, false),
        ("read_file", r#"{"path":"a","extra":true}"#, false),
        ("read_file", r#"{"file":"a"}"#, false),
        (
            "write_file",
            r#"{"path":"a","content":"b","mode":"append"}"#,
            true,
        ),
        ("write_file", r#"{"path":"","content":"b"}"#, false),
        ("search", r#"{"query":"q","extra":1}"#, true),
        (
            "search",
            r#"{"query":"q","filters":[{"field":"f","value":"v"}],"options":{"depth":3,"ratio":0.5,"more":1}}"#,
            true,
        ),
        (
            "search",
            r#"{"query":"q","filters":[{"value":"v"}]}"#,
            false,
        ),
        (
            "search",
            r#"{"query":"q","filters":[{"field":"f","other":1}]}"#,
            false,
        ),
        ("search", r#"{"query":"q","options":{"depth":"3"}}"#, false),
    ];
    for (tool, arguments, valid) in cases {
        let report = tools
            .tool(tool)
            .unwrap()
            .schema()
            .validate(&parse(arguments))
            .unwrap();
        assert_eq!(
            report.valid, valid,
            "{tool} {arguments}: {:?}",
            report.violations
        );
    }
}

#[test]
fn effects_default_policy_and_unknown_parameter_defaults() {
    let tools = compile([
        ToolDefinition::new("unspecified"),
        ToolDefinition::new("mutating").effects(Effects::Mutating),
        ToolDefinition::new("reader").effects(Effects::ReadOnly),
        ToolDefinition::new("strict_reader")
            .effects(Effects::ReadOnly)
            .policy(RepairPolicy::Structural)
            .unknown_parameters(UnknownParameters::Error),
        ToolDefinition::new("loose_writer")
            .policy(RepairPolicy::Tolerant)
            .unknown_parameters(UnknownParameters::PassThrough),
    ])
    .unwrap();
    let expect = |name: &str, effects, policy, unknown, additional: &str| {
        let tool = tools.tool(name).unwrap();
        assert_eq!(tool.effects(), effects, "{name}");
        assert_eq!(tool.policy(), policy, "{name}");
        assert_eq!(tool.unknown_parameters(), unknown, "{name}");
        assert_eq!(
            tool.argument_shape().arguments.unknown_parameters,
            unknown,
            "{name}"
        );
        assert!(tool.json_schema().contains(additional), "{name}");
    };
    let closed = r#""additionalProperties":false"#;
    let open = r#""additionalProperties":true"#;
    expect(
        "unspecified",
        Effects::Mutating,
        RepairPolicy::Structural,
        UnknownParameters::Error,
        closed,
    );
    expect(
        "mutating",
        Effects::Mutating,
        RepairPolicy::Structural,
        UnknownParameters::Error,
        closed,
    );
    expect(
        "reader",
        Effects::ReadOnly,
        RepairPolicy::Tolerant,
        UnknownParameters::DropWithLoss,
        closed,
    );
    expect(
        "strict_reader",
        Effects::ReadOnly,
        RepairPolicy::Structural,
        UnknownParameters::Error,
        closed,
    );
    expect(
        "loose_writer",
        Effects::Mutating,
        RepairPolicy::Tolerant,
        UnknownParameters::PassThrough,
        open,
    );
    assert_eq!(Effects::default(), Effects::Mutating);
    // Effect-derived defaults resolve at compile time, regardless of call order.
    let tools = compile([ToolDefinition::new("late").effects(Effects::ReadOnly)]).unwrap();
    assert_eq!(
        tools.tool("late").unwrap().unknown_parameters(),
        UnknownParameters::DropWithLoss
    );
}

#[test]
fn repeatable_scalar_renders_array_schema() {
    let tools = compile([ToolDefinition::new("t")
        .param(
            "label",
            Param::string()
                .pattern("^[a-z]+$")
                .description("One label")
                .repeatable()
                .required(),
        )
        .param("count", Param::integer().repeatable())
        .param(
            "point",
            Param::object(ObjectSpec::new().param("x", Param::number().required())).repeatable(),
        )
        .param("flag", Param::boolean())])
    .unwrap();
    let compiled = tools.tool("t").unwrap();
    assert_eq!(
        compiled.json_schema(),
        concat!(
            r#"{"type":"object","properties":{"#,
            r#""label":{"type":"array","description":"One label","items":{"type":"string","pattern":"^[a-z]+$"}},"#,
            r#""count":{"type":"array","items":{"type":"integer"}},"#,
            r#""point":{"type":"array","items":{"type":"object","properties":{"x":{"type":"number"}},"required":["x"],"additionalProperties":false}},"#,
            r#""flag":{"type":"boolean"}},"#,
            r#""required":["label"],"additionalProperties":false}"#
        )
    );
    let arguments = &compiled.argument_shape().arguments;
    let label = arguments.parameter("label").unwrap();
    assert!(label.repeatable && label.required);
    assert!(
        matches!(&label.value, ValueShape::String(Some(constraint)) if constraint.source == "^[a-z]+$")
    );
    assert_eq!(
        arguments.parameter("count").unwrap().value,
        ValueShape::Integer
    );
    assert!(matches!(
        arguments.parameter("point").unwrap().value,
        ValueShape::Object(_)
    ));
    assert!(!arguments.parameter("flag").unwrap().repeatable);
    let schema = compiled.schema();
    for (instance, valid) in [
        (r#"{"label":["a"]}"#, true),
        (
            r#"{"label":["a","b"],"count":[1,2],"point":[{"x":1}]}"#,
            true,
        ),
        (r#"{"label":"a"}"#, false),
        (r#"{"label":["A"]}"#, false),
        (r#"{"label":["a"],"count":3}"#, false),
        (r#"{"label":["a"],"point":[{"y":1}]}"#, false),
        (r#"{"label":["a"],"flag":[true]}"#, false),
    ] {
        assert_eq!(
            schema.validate(&parse(instance)).unwrap().valid,
            valid,
            "{instance}"
        );
    }
}

#[test]
fn repeatable_on_array_rejected() {
    for spec in [
        Param::array(Param::string()).repeatable(),
        Param::array(Param::object(ObjectSpec::new())).repeatable(),
    ] {
        let (tool, reason) = invalid_reason(compile([ToolDefinition::new("t").param("p", spec)]));
        assert_eq!(tool, "t");
        assert!(
            reason.contains("#/p")
                && reason.contains("array parameters always accept repeated occurrences"),
            "{reason}"
        );
    }
    // Nested inside an object scope, too.
    let (_, reason) = invalid_reason(compile([ToolDefinition::new("t").param(
        "o",
        Param::object(ObjectSpec::new().param("xs", Param::array(Param::integer()).repeatable())),
    )]));
    assert!(reason.contains("#/o/xs"), "{reason}");
    // An array item cannot itself be repeatable.
    let (_, reason) = invalid_reason(compile([
        ToolDefinition::new("t").param("p", Param::array(Param::string().repeatable()))
    ]));
    assert!(reason.contains("array item specification"), "{reason}");
}

#[test]
fn repeatable_default_must_be_array() {
    for default in [r#""a""#, "1", r#"["a",1]"#, "null"] {
        let (_, reason) = invalid_reason(compile([ToolDefinition::new("t")
            .param("p", Param::string().repeatable().default(parse(default)))]));
        assert!(reason.contains("does not conform"), "{default}: {reason}");
    }
    let (_, reason) = invalid_reason(compile([ToolDefinition::new("t").param(
        "p",
        Param::enumeration(&["x", "y"])
            .repeatable()
            .default(parse(r#"["x","z"]"#)),
    )]));
    assert!(reason.contains("does not conform"), "{reason}");
    let tools = compile([ToolDefinition::new("t")
        .param(
            "p",
            Param::string().repeatable().default(parse(r#"["a","b"]"#)),
        )
        .param(
            "e",
            Param::enumeration(&["x", "y"])
                .repeatable()
                .default(parse("[]")),
        )])
    .unwrap();
    let defaults = tools.defaults("t").unwrap();
    assert_eq!(defaults[0].value.to_json(), r#"["a","b"]"#);
    assert_eq!(defaults[1].value.to_json(), "[]");
    assert!(
        tools
            .render_json_schema("t")
            .unwrap()
            .contains(r#""p":{"type":"array","items":{"type":"string"},"default":["a","b"]}"#)
    );
}
