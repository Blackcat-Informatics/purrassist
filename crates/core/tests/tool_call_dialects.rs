// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Conformance of the built-in dialect grammars: hand-authored anchor
//! fixtures of canonical markup, seeded grammar witnesses, source spans,
//! dialect attribution in mixed and multi-dialect sets, and the result and
//! declaration productions.
use purrassist_core::grammar::edit::EditOperator;
use purrassist_core::grammar::witness::{self, JsonArguments, WitnessOptions};
use purrassist_core::grammar::{CaptureKind, Grammar};
use purrassist_core::json::{self, Node, Value};
use purrassist_core::tool_call::{
    ArgOrigin, Closure, CompiledTool, DeclarationForm, DialectId, DialectSet, GenerationStop,
    ProductionKind, RawCall, ReasoningPolicy, RecognizedTurn, Recognizer, Segment, ToolSet,
};
use purrassist_core::{Budget, Governor};
use std::path::PathBuf;

/// One anchor fixture: canonical markup, its declared tools, and the calls
/// it denotes.
struct Fixture {
    file: String,
    markup: String,
    tools: ToolSet,
    expected: Vec<Expected>,
}

struct Expected {
    name: String,
    arguments: Node,
    id: Option<String>,
}

fn directory(dialect: &DialectId) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/tool-calls/conformance")
        .join(dialect.id())
}

fn string_member(node: &Node, key: &str) -> Option<String> {
    node.get(key).and_then(Node::as_str).map(str::to_owned)
}

fn fixtures(dialect: &DialectId) -> Vec<Fixture> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory(dialect))
        .expect("fixture directory")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let file = path.display().to_string();
            let text = std::fs::read_to_string(&path).expect("fixture text");
            let document = json::parse(&text).unwrap_or_else(|error| panic!("{file}: {error:?}"));
            let markup = string_member(&document, "markup").expect("markup");
            let tools = document.get("tools").expect("tools").to_json();
            let tools = ToolSet::from_openai(&tools, Budget::default())
                .unwrap_or_else(|error| panic!("{file}: {error}"));
            let Some(Value::Array(calls)) = document.get("expected_calls").map(|node| &node.value)
            else {
                panic!("{file}: expected_calls array");
            };
            let expected = calls
                .iter()
                .map(|call| Expected {
                    name: string_member(call, "name").expect("name"),
                    arguments: call.get("arguments").expect("arguments").clone(),
                    id: string_member(call, "id"),
                })
                .collect();
            Fixture {
                file,
                markup,
                tools,
                expected,
            }
        })
        .collect()
}

fn dialect_set(dialects: &[DialectId]) -> DialectSet {
    DialectSet::new(
        dialects.iter().cloned(),
        &mut Governor::new(Budget::default()),
    )
    .expect("dialect set")
}

fn recognize(set: &DialectSet, tools: &ToolSet, markup: &str) -> RecognizedTurn {
    let mut governor = Governor::new(Budget::default());
    let mut recognizer =
        Recognizer::with_tools(set, tools, ReasoningPolicy::default(), &mut governor)
            .expect("recognizer");
    recognizer
        .push(markup.as_bytes(), &mut governor)
        .expect("push");
    recognizer
        .finish(GenerationStop::Stop, &mut governor)
        .expect("finish")
}

/// The single reading of every call of the turn's selected segmentation.
fn calls(turn: &RecognizedTurn) -> Vec<&RawCall> {
    turn.calls()
        .into_iter()
        .map(|segment| {
            assert_eq!(segment.readings.len(), 1, "one reading per call");
            &segment.readings[0]
        })
        .collect()
}

/// A call read as canonical markup: closed, at no cost, with no edit but
/// the dialect's own readings (framing newlines, the stringified-arguments
/// decoding, a whole-JSON body, nested parameter elements).
fn assert_canonical(call: &RawCall, context: &str) {
    assert_eq!(call.closure, Closure::Closed, "{context}");
    assert_eq!(call.cost, 0, "{context}");
    assert!(
        call.arguments.is_resolved(),
        "{context}: {:?}",
        call.arguments.unresolved
    );
    for edit in &call.edits {
        assert!(
            matches!(
                edit.operator,
                EditOperator::FramingNewline
                    | EditOperator::StringifiedArguments
                    | EditOperator::JsonBody
                    | EditOperator::NestedParameter
            ),
            "{context}: unexpected edit {edit:?}"
        );
    }
    assert_eq!(
        call.arguments.arguments_were_stringified,
        call.edits
            .iter()
            .any(|edit| edit.operator == EditOperator::StringifiedArguments),
        "{context}"
    );
}

fn assert_expected(
    turn: &RecognizedTurn,
    expected: &[Expected],
    dialect: &DialectId,
    context: &str,
) {
    assert!(!turn.is_ambiguous(), "{context}");
    for segment in turn.selected() {
        assert!(
            !matches!(segment, Segment::Rejected { .. }),
            "{context}: rejected attempt {segment:?}"
        );
    }
    let calls = calls(turn);
    assert_eq!(
        calls.len(),
        expected.len(),
        "{context}: {:#?}",
        turn.pieces()
    );
    for (order, (call, expected)) in calls.iter().zip(expected).enumerate() {
        let context = format!("{context} call {order}");
        assert_eq!(&call.dialect, dialect, "{context}");
        assert_eq!(call.kind, ProductionKind::Call, "{context}");
        assert_eq!(call.order, order, "{context}");
        assert_eq!(
            call.name.as_ref().map(|name| name.value.as_str()),
            Some(expected.name.as_str()),
            "{context}"
        );
        assert_eq!(
            call.id.as_ref().map(|id| id.value.as_str()),
            expected.id.as_deref(),
            "{context}"
        );
        assert!(
            call.arguments.root.equivalent(&expected.arguments),
            "{context}: {} != {}",
            call.arguments.root.to_json(),
            expected.arguments.to_json()
        );
        // Exact spellings and member order too.
        assert_eq!(
            call.arguments.root.to_json(),
            expected.arguments.to_json(),
            "{context}"
        );
        // The codec types the arguments by the declared tool when the name
        // precedes them.
        let arguments = call
            .captures
            .iter()
            .find(|capture| capture.kind == CaptureKind::Arguments)
            .expect("arguments capture");
        let named_first = call
            .name
            .as_ref()
            .is_some_and(|name| name.span.end <= arguments.span.start);
        assert_eq!(
            call.arguments.tool.as_deref(),
            named_first.then_some(expected.name.as_str()),
            "{context}: decoded against the declared tool"
        );
        assert_canonical(call, &context);
    }
}

#[test]
fn all_registered_dialects_have_anchor_fixtures() {
    let ids: Vec<&str> = DialectId::BUILT_IN.iter().map(DialectId::id).collect();
    assert_eq!(
        ids,
        [
            "hermes.v1",
            "llama3-json.v1",
            "mistral-v3.v1",
            "openai-tool-calls.v1",
            "qwen3-coder-xml.v1"
        ]
    );
    for dialect in DialectId::BUILT_IN {
        let fixtures = fixtures(&dialect);
        assert!(
            fixtures.len() >= 3,
            "{}: {} fixtures",
            dialect.id(),
            fixtures.len()
        );
        // Single call, several calls, and nested or typed arguments.
        assert!(fixtures.iter().any(|fixture| fixture.expected.len() == 1));
        assert!(fixtures.iter().any(|fixture| fixture.expected.len() > 1));
        assert!(fixtures.iter().any(|fixture| {
            fixture
                .expected
                .iter()
                .any(|call| call.arguments.to_json().contains("{\"backup\""))
        }));
    }
}

#[test]
fn anchor_fixtures_parse_to_expected_calls() {
    for dialect in DialectId::BUILT_IN {
        let set = dialect_set(core::slice::from_ref(&dialect));
        for fixture in fixtures(&dialect) {
            let turn = recognize(&set, &fixture.tools, &fixture.markup);
            assert_expected(&turn, &fixture.expected, &dialect, &fixture.file);
            // A leading reasoning block is a reasoning segment, not content.
            if fixture.markup.starts_with("<think>") {
                assert!(
                    matches!(turn.selected()[0], Segment::Reasoning(_)),
                    "{}",
                    fixture.file
                );
            }
            // OpenAI arguments are canonically stringified; the others are not.
            let stringified = dialect == DialectId::OpenAiToolCalls;
            for call in calls(&turn) {
                assert_eq!(
                    call.arguments.arguments_were_stringified, stringified,
                    "{}",
                    fixture.file
                );
            }
        }
    }
}

fn witness_options(dialect: &DialectId, seed: u64) -> WitnessOptions {
    WitnessOptions {
        seed,
        json_arguments: if *dialect == DialectId::OpenAiToolCalls && seed % 4 != 3 {
            JsonArguments::Stringified
        } else {
            JsonArguments::Object
        },
    }
}

#[test]
fn witness_fixtures_parse() {
    for dialect in DialectId::BUILT_IN {
        let set = dialect_set(core::slice::from_ref(&dialect));
        for fixture in fixtures(&dialect) {
            let shapes: Vec<_> = fixture
                .tools
                .tools()
                .iter()
                .map(CompiledTool::argument_shape)
                .collect();
            for seed in 0..12 {
                let options = witness_options(&dialect, seed);
                let mut governor = Governor::new(Budget::default());
                let generated =
                    witness::generate(dialect.grammar(), &shapes, options, &mut governor)
                        .unwrap_or_else(|error| panic!("{} seed {seed}: {error}", fixture.file));
                let again = witness::generate(
                    dialect.grammar(),
                    &shapes,
                    options,
                    &mut Governor::new(Budget::default()),
                )
                .expect("deterministic");
                assert_eq!(generated, again, "{} seed {seed}", fixture.file);
                let expected: Vec<Expected> = generated
                    .calls
                    .iter()
                    .map(|call| Expected {
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                        id: call.id.clone(),
                    })
                    .collect();
                let turn = recognize(&set, &fixture.tools, &generated.text);
                let context = format!("{} seed {seed}: {:?}", fixture.file, generated.text);
                assert_expected(&turn, &expected, &dialect, &context);
            }
        }
    }
}

/// Decode a JSON string's escaped content (the bytes between its quotes).
fn unescape(content: &str) -> String {
    let node = json::parse(&format!("\"{content}\"")).expect("escaped string content");
    node.as_str().expect("a string").to_owned()
}

/// A Llama 3 JSON or Mistral v3 call's span holds one JSON call object
/// (after `<|python_tag|>`, or with the `[TOOL_CALLS]` array framing around
/// it) whose name, id and arguments are the call's.
fn assert_json_call_object(dialect: &DialectId, raw: &str, call: &RawCall, file: &str) {
    let start = raw.find('{').expect("object start");
    let end = raw.rfind('}').expect("object end");
    let object = json::parse(&raw[start..=end]).expect("call object JSON");
    assert_eq!(
        string_member(&object, "name").as_deref(),
        call.name.as_ref().map(|name| name.value.as_str()),
        "{file}"
    );
    if *dialect == DialectId::MistralV3 {
        assert_eq!(
            string_member(&object, "id"),
            call.id.as_ref().map(|id| id.value.clone()),
            "{file}"
        );
    } else {
        assert!(
            raw.starts_with('{') || raw.starts_with("<|python_tag|>"),
            "{file}"
        );
    }
    let arguments = object
        .get("parameters")
        .or_else(|| object.get("arguments"))
        .expect("arguments");
    assert!(arguments.equivalent(&call.arguments.root), "{file}");
}

#[test]
fn ir_spans_slice_back_to_source() {
    for dialect in DialectId::BUILT_IN {
        let set = dialect_set(core::slice::from_ref(&dialect));
        for fixture in fixtures(&dialect) {
            let source = fixture.markup.as_str();
            let turn = recognize(&set, &fixture.tools, source);
            // Segments partition the source.
            let mut at = 0;
            for segment in turn.selected() {
                assert_eq!(segment.span().start, at, "{}", fixture.file);
                at = segment.span().end;
            }
            assert_eq!(at, source.len(), "{}", fixture.file);
            for call in calls(&turn) {
                let raw = &source[call.span.clone()];
                if dialect == DialectId::OpenAiToolCalls {
                    // A whole tool_calls element: one JSON object.
                    let element = json::parse(raw).expect("element JSON");
                    assert_eq!(
                        string_member(&element, "id"),
                        call.id.as_ref().map(|id| id.value.clone())
                    );
                    let function = element.get("function").expect("function");
                    assert_eq!(
                        string_member(function, "name").as_deref(),
                        call.name.as_ref().map(|name| name.value.as_str())
                    );
                    let arguments = string_member(function, "arguments").expect("arguments");
                    let decoded = json::parse(&arguments).expect("stringified arguments");
                    assert!(decoded.equivalent(&call.arguments.root), "{}", fixture.file);
                } else if matches!(dialect, DialectId::MistralV3 | DialectId::Llama3Json) {
                    assert_json_call_object(&dialect, raw, call, &fixture.file);
                } else {
                    assert!(raw.starts_with("<tool_call>"), "{}: {raw:?}", fixture.file);
                    assert!(raw.ends_with("</tool_call>"), "{}: {raw:?}", fixture.file);
                }
                let name = call.name.as_ref().expect("name");
                assert_eq!(&source[name.span.clone()], name.value);
                if let Some(id) = &call.id {
                    assert_eq!(&source[id.span.clone()], id.value);
                }
                for capture in &call.captures {
                    assert!(
                        call.span.start <= capture.span.start && capture.span.end <= call.span.end,
                        "{}: capture outside its call",
                        fixture.file
                    );
                }
                assert!(!call.arguments.leaves.is_empty(), "{}", fixture.file);
                for leaf in &call.arguments.leaves {
                    let value = call
                        .arguments
                        .root
                        .pointer(&leaf.pointer)
                        .expect("leaf pointer resolves");
                    let raw = &source[leaf.raw.clone()];
                    let context = format!("{} {} {raw:?}", fixture.file, leaf.pointer);
                    match leaf.origin {
                        ArgOrigin::Json => {
                            let parsed = json::parse(raw).expect("leaf JSON");
                            assert!(parsed.equivalent(value), "{context}");
                            assert_eq!(parsed.to_json(), value.to_json(), "{context}");
                        }
                        ArgOrigin::XmlText | ArgOrigin::Raw => {
                            assert_eq!(Some(raw), value.as_str(), "{context}");
                        }
                        ArgOrigin::StringifiedJson => {
                            let parsed = json::parse(&unescape(raw)).expect("escaped leaf JSON");
                            assert!(parsed.equivalent(value), "{context}");
                            assert_eq!(parsed.to_json(), value.to_json(), "{context}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn mixed_dialects_in_one_message() {
    let tools = ToolSet::from_openai(
        r#"[{"type":"function","function":{"name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}},
            {"type":"function","function":{"name":"read_file","parameters":{"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer"}}}}}]"#,
        Budget::default(),
    )
    .expect("tools");
    let markup = "Checking two things.\n<tool_call>\n{\"name\": \"get_weather\", \"arguments\": {\"city\": \"Oslo\"}}\n</tool_call>\nAnd the file:\n<tool_call>\n<function=read_file>\n<parameter=path>\nREADME.md\n</parameter>\n<parameter=limit>\n20\n</parameter>\n</function>\n</tool_call>\n";
    let set = dialect_set(&[DialectId::Qwen3CoderXml, DialectId::Hermes]);
    let turn = recognize(&set, &tools, markup);
    let calls = calls(&turn);
    assert_eq!(calls.len(), 2, "{:#?}", turn.pieces());
    assert_eq!(calls[0].dialect, DialectId::Hermes);
    assert_eq!(calls[0].order, 0);
    assert_eq!(calls[0].name.as_ref().expect("name").value, "get_weather");
    assert_eq!(calls[0].arguments.root.to_json(), r#"{"city":"Oslo"}"#);
    assert_eq!(calls[1].dialect, DialectId::Qwen3CoderXml);
    assert_eq!(calls[1].order, 1);
    assert_eq!(calls[1].name.as_ref().expect("name").value, "read_file");
    assert_eq!(
        calls[1].arguments.root.to_json(),
        r#"{"path":"README.md","limit":20}"#
    );
    for call in &calls {
        assert_canonical(call, markup);
    }
    let content: Vec<&str> = turn
        .selected()
        .into_iter()
        .filter_map(|segment| match segment {
            Segment::Content(span) => Some(&markup[span.clone()]),
            _ => None,
        })
        .collect();
    assert_eq!(
        content,
        ["Checking two things.\n", "\nAnd the file:\n", "\n"]
    );
}

#[test]
fn all_dialects_allowed_no_cross_talk() {
    let all = dialect_set(&DialectId::BUILT_IN);
    for dialect in DialectId::BUILT_IN {
        let alone = dialect_set(core::slice::from_ref(&dialect));
        for fixture in fixtures(&dialect) {
            let own = recognize(&alone, &fixture.tools, &fixture.markup);
            let shared = recognize(&all, &fixture.tools, &fixture.markup);
            assert_expected(&shared, &fixture.expected, &dialect, &fixture.file);
            assert_eq!(calls(&own), calls(&shared), "{}", fixture.file);
        }
    }
}

#[test]
fn dialect_ids_round_trip() {
    for dialect in DialectId::BUILT_IN {
        assert_eq!(DialectId::from_id(dialect.id()), Some(dialect.clone()));
        assert_eq!(dialect.grammar().id(), dialect.id());
        assert_eq!(dialect.name(), dialect.id());
        // The embedded source compiles, under a caller's governor, to the
        // same dialect.
        let source = dialect.source().expect("built-in source");
        let mut governor = Governor::new(Budget::default());
        let compiled = Grammar::compile_governed(source, &mut governor).expect("compiles");
        assert!(governor.usage().work > 0);
        assert_eq!(DialectId::custom(compiled), dialect);
        // No delta production: streaming deltas are not part of v1.
        assert!(dialect.grammar().production("delta").is_none());
    }
    assert_eq!(DialectId::from_id("hermes"), None);
    assert_eq!(DialectId::from_id("qwen3-coder-xml.v2"), None);
    let custom = Grammar::compile_governed(
        "grammar \"hermes.v1\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";",
        &mut Governor::new(Budget::default()),
    )
    .expect("compiles");
    // Same id, different grammar: not the built-in dialect.
    assert_ne!(DialectId::custom(custom), DialectId::Hermes);
}

#[test]
fn reasoning_markers_are_declared() {
    for dialect in [DialectId::Qwen3CoderXml, DialectId::Hermes] {
        assert_eq!(
            dialect.grammar().reasoning(),
            Some((&b"<think>"[..], &b"</think>"[..]))
        );
    }
    assert_eq!(DialectId::OpenAiToolCalls.grammar().reasoning(), None);
    assert_eq!(DialectId::Llama3Json.grammar().reasoning(), None);
    assert_eq!(DialectId::MistralV3.grammar().reasoning(), None);
    let set = dialect_set(&DialectId::BUILT_IN);
    assert_eq!(
        set.reasoning_markers(),
        [(b"<think>".to_vec(), b"</think>".to_vec())]
    );
}

fn results(turn: &RecognizedTurn) -> Vec<&RawCall> {
    turn.selected()
        .into_iter()
        .filter_map(|segment| match segment {
            Segment::Result(result) => Some(&result.readings[0]),
            _ => None,
        })
        .collect()
}

#[test]
fn result_productions_read_tool_results() {
    let tools = ToolSet::compile([], Budget::default()).expect("no tools");
    let content = |result: &RawCall, source: &str| {
        result
            .capture_text(CaptureKind::Content, source.as_bytes())
            .map(str::to_owned)
    };
    for dialect in [DialectId::Qwen3CoderXml, DialectId::Hermes] {
        let source = "<tool_response>\n{\"temperature\": 22.0}\n</tool_response>";
        let turn = recognize(
            &dialect_set(core::slice::from_ref(&dialect)),
            &tools,
            source,
        );
        let found = results(&turn);
        assert_eq!(found.len(), 1, "{}", dialect.id());
        assert_eq!(found[0].kind, ProductionKind::Result);
        assert_eq!(
            content(found[0], source).as_deref(),
            Some("\n{\"temperature\": 22.0}\n")
        );
    }
    // OpenAI request tool messages: string content and text-part arrays,
    // compact and pretty-printed.
    let set = dialect_set(&[DialectId::OpenAiToolCalls]);
    for (source, expected) in [
        (
            r#"{"role": "tool", "tool_call_id": "call_1", "content": "22.0 \"C\""}"#,
            r#""22.0 \"C\"""#,
        ),
        (
            r#"{"role":"tool","tool_call_id":"call_2","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}"#,
            r#"[{"type":"text","text":"a"},{"type":"text","text":"b"}]"#,
        ),
        (
            "{\n  \"role\": \"tool\",\n  \"tool_call_id\": \"call_3\",\n  \"content\": \"ok\"\n}",
            "\"ok\"",
        ),
    ] {
        let turn = recognize(&set, &tools, source);
        let found = results(&turn);
        assert_eq!(found.len(), 1, "{source}");
        assert_eq!(content(found[0], source).as_deref(), Some(expected));
        let id = found[0].id.as_ref().expect("tool_call_id");
        assert_eq!(&source[id.span.clone()], id.value);
        assert!(id.value.starts_with("call_"));
    }
    // Mistral v3 tool results carry their content as a JSON string and the
    // call id they answer.
    let set = dialect_set(&[DialectId::MistralV3]);
    let source =
        r#"[TOOL_RESULTS] {"content": "22.0 \"C\"", "call_id": "a1B2c3D4e"}[/TOOL_RESULTS]"#;
    let turn = recognize(&set, &tools, source);
    let found = results(&turn);
    assert_eq!(found.len(), 1, "{:#?}", turn.pieces());
    assert_eq!(
        content(found[0], source).as_deref(),
        Some(r#""22.0 \"C\"""#)
    );
    assert_eq!(
        found[0].id.as_ref().map(|id| id.value.as_str()),
        Some("a1B2c3D4e")
    );
    // Llama 3 tool results are ipython messages.
    let set = dialect_set(&[DialectId::Llama3Json]);
    let source = "<|start_header_id|>ipython<|end_header_id|>\n\n{\"temperature\": 22.0}<|eot_id|>";
    let turn = recognize(&set, &tools, source);
    let found = results(&turn);
    assert_eq!(found.len(), 1, "{:#?}", turn.pieces());
    assert_eq!(
        content(found[0], source).as_deref(),
        Some("{\"temperature\": 22.0}")
    );
    // An assistant message envelope never opens a result or a call attempt.
    let set = dialect_set(&[DialectId::OpenAiToolCalls]);
    let source = r#"{"role": "assistant", "content": "plain text", "tool_calls": []}"#;
    let turn = recognize(&set, &tools, source);
    assert!(
        matches!(turn.selected()[..], [Segment::Content(_)]),
        "{:#?}",
        turn.pieces()
    );
}

/// A dialect whose `call` is the built-in dialect's `declaration`, so the
/// recognizer can check that declarations match the production.
fn declaration_reader(dialect: &DialectId) -> DialectSet {
    let source = dialect
        .source()
        .expect("built-in source")
        .replace("\ncall = ", "\nprinted-call = ")
        .replace("\nresult = ", "\nprinted-result = ")
        .replace("\ndeclaration = ", "\ncall = ")
        .replacen("grammar \"", "grammar \"declaration-of-", 1);
    let source = format!("{source}\nresult = \"<unused-result>\";\ndeclaration = \"<unused>\";\n");
    let mut governor = Governor::new(Budget::default());
    let grammar = Grammar::compile_governed(&source, &mut governor).expect("declaration grammar");
    DialectSet::new([DialectId::custom(grammar)], &mut governor).expect("set")
}

#[test]
fn declaration_productions_match_tool_declarations() {
    let empty = ToolSet::compile([], Budget::default()).expect("no tools");
    for dialect in [
        DialectId::OpenAiToolCalls,
        DialectId::Hermes,
        DialectId::Llama3Json,
        DialectId::MistralV3,
    ] {
        let set = declaration_reader(&dialect);
        for fixture in fixtures(&dialect) {
            for tool in fixture.tools.tools() {
                let declaration = tool
                    .render_declaration(DeclarationForm::OpenAi)
                    .expect("declaration");
                let turn = recognize(&set, &empty, &declaration);
                let found = calls(&turn);
                assert_eq!(found.len(), 1, "{declaration}");
                assert_eq!(found[0].span, 0..declaration.len(), "{declaration}");
                assert_eq!(found[0].name.as_ref().expect("name").value, tool.name());
                // The parameters schema is read as the `arguments` value.
                assert!(found[0].arguments.root.equivalent(tool.schema_document()));
            }
        }
    }
    // Qwen3-Coder lists tools as XML function entries.
    let set = declaration_reader(&DialectId::Qwen3CoderXml);
    let declaration = "<function>\n<name>read_file</name>\n<description>Read a text file.</description>\n<parameters>\n<parameter>\n<name>path</name>\n<type>string</type>\n</parameter>\n</parameters>\n</function>";
    let turn = recognize(&set, &empty, declaration);
    let found = calls(&turn);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].span, 0..declaration.len());
    assert_eq!(found[0].name.as_ref().expect("name").value, "read_file");
}
