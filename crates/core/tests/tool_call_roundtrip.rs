// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Printing is the inverse of parsing (D2): recovered calls printed in any
//! dialect recover to the same calls; the `OpenAI` `tool_calls` shape (C7);
//! finish reasons (D15); declarations and tool results in every form; the
//! minimal-diff `put`; and the recovery emission option.
use purrassist_core::grammar::GrammarError;
use purrassist_core::grammar::witness::{self, JsonArguments, WitnessOptions};
use purrassist_core::grammar::{CaptureKind, Grammar};
use purrassist_core::json::{self, Node, Value};
use purrassist_core::tool_call::emit::{self, EmitError};
use purrassist_core::tool_call::{
    CallEdit, CallOutcome, CompiledTool, DeclarationForm, DialectId, DialectSet, Effects, Emission,
    EmitTarget, Finish, GenerationStop, IdGenerator, IdPolicy, Param, PolicySelection,
    ReasoningPolicy, Recognizer, RepairPolicy, Segment, SkipReason, Skipped, ToolCall, ToolCalls,
    ToolDefinition, ToolRecoveryRequest, ToolResult, ToolSet, ToolSource, turn_id_prefix,
};
use purrassist_core::wire::WireValue;
use purrassist_core::{Budget, Governor, Outcome};
use std::path::PathBuf;
use std::sync::Arc;

/// One anchor fixture: canonical markup and its declared tools.
struct Fixture {
    file: String,
    markup: String,
    tools: ToolSet,
}

fn fixtures(dialect: &DialectId) -> Vec<Fixture> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/tool-calls/conformance")
        .join(dialect.id());
    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)
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
            let markup = document
                .get("markup")
                .and_then(Node::as_str)
                .expect("markup")
                .to_owned();
            let tools = ToolSet::from_openai(
                &document.get("tools").expect("tools").to_json(),
                Budget::default(),
            )
            .unwrap_or_else(|error| panic!("{file}: {error}"));
            Fixture {
                file,
                markup,
                tools,
            }
        })
        .collect()
}

fn set(dialects: &[DialectId]) -> DialectSet {
    DialectSet::new(
        dialects.iter().cloned(),
        &mut Governor::new(Budget::default()),
    )
    .expect("dialect set")
}

fn recover_in(tools: &ToolSet, output: &[u8], dialect: &DialectId) -> ToolCalls {
    let request = ToolRecoveryRequest::default().dialects(set(core::slice::from_ref(dialect)));
    value(tools.recover(output, &request).outcome)
}

fn value(outcome: Outcome<ToolCalls>) -> ToolCalls {
    match outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
        other => panic!("no single value: {other:?}"),
    }
}

/// Rules a canonical call may record: the dialect's own zero-cost readings.
const FRAMING: [&str; 4] = [
    "tool_call.xml.framing-newline",
    "tool_call.arguments.decode-string",
    "tool_call.markup.json-body",
    "tool_call.markup.nested-parameter",
];

/// The calls of `second` are those of `first`: same tools, equivalent
/// arguments, same order, resolved, and nothing recorded beyond framing.
fn assert_same_calls(first: &ToolCalls, second: &ToolCalls, context: &str) {
    assert!(
        second.rejected.is_empty(),
        "{context}: {:?}",
        second.rejected
    );
    assert_eq!(first.calls.len(), second.calls.len(), "{context}");
    for (before, after) in first.calls.iter().zip(&second.calls) {
        assert_eq!(after.index, before.index, "{context}");
        assert_eq!(after.tool, before.tool, "{context}");
        assert_eq!(
            after.outcome,
            CallOutcome::Resolved,
            "{context}: {after:#?}"
        );
        assert!(
            after.arguments.equivalent(&before.arguments),
            "{context}: {} != {}",
            after.arguments.to_json(),
            before.arguments.to_json()
        );
        for evidence in &after.repairs.evidence {
            assert!(
                FRAMING.contains(&evidence.rule.as_str()),
                "{context}: unexpected {}",
                evidence.rule
            );
        }
        for loss in &after.repairs.losses {
            assert_eq!(
                loss.loss.evidence.rule, "tool_call.xml.framing-newline",
                "{context}"
            );
        }
        assert!(after.repairs.inferences.is_empty(), "{context}");
    }
}

#[test]
fn every_pair_roundtrips() {
    for source in DialectId::BUILT_IN {
        for fixture in fixtures(&source) {
            let first = recover_in(&fixture.tools, fixture.markup.as_bytes(), &source);
            assert!(!first.calls.is_empty(), "{}", fixture.file);
            assert!(
                first.calls.iter().all(ToolCall::is_usable),
                "{}",
                fixture.file
            );
            for target in DialectId::BUILT_IN {
                let context = format!("{} -> {}", fixture.file, target.id());
                let printed = emit::emit(&fixture.tools, &first, &target, &IdPolicy::default())
                    .unwrap_or_else(|error| panic!("{context}: {error}"));
                assert!(
                    printed.skipped.is_empty(),
                    "{context}: {:?}",
                    printed.skipped
                );
                let indices: Vec<usize> = first.calls.iter().map(|call| call.index).collect();
                assert_eq!(printed.calls, indices, "{context}");
                let second = recover_in(&fixture.tools, &printed.output, &target);
                let context = format!("{context}: {}", String::from_utf8_lossy(&printed.output));
                assert_same_calls(&first, &second, &context);
                // Content between printed calls is only their separators.
                for range in &second.content {
                    assert_eq!(&printed.output[range.clone()], b"\n", "{context}");
                }
                // Ids survive where the target carries them. A Kimi-K2 id
                // is composite: `functions.NAME:IDX`, IDX the call's
                // position among the printed calls.
                if target == DialectId::KimiK2 {
                    for (position, (before, after)) in
                        first.calls.iter().zip(&second.calls).enumerate()
                    {
                        let expected = format!(
                            "functions.{}:{position}",
                            before.tool.as_deref().expect("tool")
                        );
                        assert_eq!(after.source_id.as_deref(), Some(expected.as_str()));
                    }
                } else if target.grammar().call_carries_ids() {
                    for (before, after) in first.calls.iter().zip(&second.calls) {
                        assert_eq!(after.source_id.as_deref(), Some(before.id.as_str()));
                    }
                }
                // The canonical form is a fixed point.
                let again = emit::emit(&fixture.tools, &second, &target, &IdPolicy::default())
                    .expect("reprint");
                assert_eq!(again.output, printed.output, "{context}");
            }
            // The OpenAI dialect printing is the tool_calls array, one
            // element per line.
            let array = emit::openai_tool_calls(&first, &IdPolicy::default()).expect("array");
            let Value::Array(elements) = &array.output.value else {
                panic!("an array")
            };
            let lines: Vec<String> = elements.iter().map(Node::to_json).collect();
            let printed = emit::emit(
                &fixture.tools,
                &first,
                &DialectId::OpenAiToolCalls,
                &IdPolicy::default(),
            )
            .expect("openai");
            assert_eq!(
                printed.output,
                lines.join("\n").into_bytes(),
                "{}",
                fixture.file
            );
        }
    }
}

#[test]
fn random_witness_roundtrip_per_dialect() {
    for dialect in DialectId::BUILT_IN {
        for fixture in fixtures(&dialect) {
            let shapes: Vec<_> = fixture
                .tools
                .tools()
                .iter()
                .map(CompiledTool::argument_shape)
                .collect();
            for seed in 0..16 {
                let options = WitnessOptions {
                    seed,
                    json_arguments: if dialect == DialectId::OpenAiToolCalls && seed % 2 == 0 {
                        JsonArguments::Stringified
                    } else {
                        JsonArguments::Object
                    },
                };
                let generated = witness::generate(
                    dialect.grammar(),
                    &shapes,
                    options,
                    &mut Governor::new(Budget::default()),
                )
                .expect("witness");
                let context = format!("{} seed {seed}: {:?}", fixture.file, generated.text);
                let first = recover_in(&fixture.tools, generated.text.as_bytes(), &dialect);
                assert_eq!(first.calls.len(), generated.calls.len(), "{context}");
                let canonical = emit::emit(&fixture.tools, &first, &dialect, &IdPolicy::default())
                    .expect("emit");
                assert!(canonical.skipped.is_empty(), "{context}");
                let second = recover_in(&fixture.tools, &canonical.output, &dialect);
                assert_same_calls(&first, &second, &context);
                let again = emit::emit(&fixture.tools, &second, &dialect, &IdPolicy::default())
                    .expect("emit again");
                assert_eq!(again.output, canonical.output, "{context}");
            }
        }
    }
}

fn payment_tools() -> ToolSet {
    ToolSet::compile(
        [ToolDefinition::new("pay")
            .param("amount", Param::number().required())
            .param("count", Param::integer())
            .param("scale", Param::number())
            .param("memo", Param::string())],
        Budget::default(),
    )
    .expect("tools")
}

const PAYMENTS: &str = "Paying.\n<tool_call>\n{\"name\": \"pay\", \"arguments\": {\"amount\": 1.50, \"count\": -0, \"scale\": 1e3, \"memo\": \"a \\\"b\\\"\"}}\n</tool_call>\n<tool_call>\n{\"name\": \"pay\", \"arguments\": {\"amount\": 2}}\n</tool_call>";

struct Tagged;

/// A generator that numbers every call `x`.
struct Same;

impl IdGenerator for Same {
    fn id(&self, _: usize) -> String {
        "x".into()
    }
}

impl IdGenerator for Tagged {
    fn id(&self, call_index: usize) -> String {
        format!("toolu_{call_index:02}")
    }
}

#[test]
fn openai_shape_exact() {
    let tools = payment_tools();
    let calls = recover_in(&tools, PAYMENTS.as_bytes(), &DialectId::Hermes);
    let prefix = IdPolicy::Prefix {
        prefix: "call_".into(),
        start: 7,
    };
    let array = emit::openai_tool_calls(&calls, &prefix).expect("array");
    assert_eq!(
        array.output.to_json(),
        r#"[{"id":"call_7","type":"function","function":{"name":"pay","arguments":"{\"amount\":1.50,\"count\":-0,\"scale\":1e3,\"memo\":\"a \\\"b\\\"\"}"}},{"id":"call_8","type":"function","function":{"name":"pay","arguments":"{\"amount\":2}"}}]"#
    );
    let generated =
        emit::openai_tool_calls(&calls, &IdPolicy::Generator(Arc::new(Tagged))).expect("array");
    assert_eq!(
        generated.output.to_json(),
        r#"[{"id":"toolu_00","type":"function","function":{"name":"pay","arguments":"{\"amount\":1.50,\"count\":-0,\"scale\":1e3,\"memo\":\"a \\\"b\\\"\"}"}},{"id":"toolu_01","type":"function","function":{"name":"pay","arguments":"{\"amount\":2}"}}]"#
    );
    for array in [&array, &generated] {
        let Value::Array(elements) = &array.output.value else {
            panic!("an array")
        };
        for element in elements {
            let Value::Object(members) = &element.value else {
                panic!("an object")
            };
            let keys: Vec<&str> = members.iter().map(|member| member.key.as_str()).collect();
            assert_eq!(keys, ["id", "type", "function"]);
            assert!(element.get("role").is_none());
            let function = element.get("function").expect("function");
            let arguments = function
                .get("arguments")
                .and_then(Node::as_str)
                .expect("arguments is a string");
            assert!(json::parse(arguments).is_ok());
        }
        let first = elements[0]
            .pointer("/function/arguments")
            .and_then(Node::as_str)
            .expect("arguments");
        for spelling in ["1.50", "-0", "1e3"] {
            assert!(first.contains(spelling), "{first}");
        }
    }
    // Ids match the recovery that used the same policy.
    let request = ToolRecoveryRequest::default().ids(prefix);
    let recovered = value(tools.recover(PAYMENTS.as_bytes(), &request).outcome);
    let ids: Vec<&str> = recovered
        .calls
        .iter()
        .map(|call| call.id.as_str())
        .collect();
    assert_eq!(ids, ["call_7", "call_8"]);
    // A generator that repeats an id is refused, never renumbered.
    assert!(matches!(
        emit::openai_tool_calls(&calls, &IdPolicy::Generator(Arc::new(Same))),
        Err(EmitError::InvalidIds { .. })
    ));
}

#[test]
fn finish_reason_mapping() {
    for (finish, reason) in [
        (Finish::TruncatedWithCalls, "length"),
        (Finish::Truncated, "length"),
        (Finish::Calls, "tool_calls"),
        (Finish::ContentOnly, "stop"),
        (Finish::Unresolved, "stop"),
    ] {
        assert_eq!(finish.openai_finish_reason(), reason, "{finish:?}");
    }
    let tools = payment_tools();
    let finish = |output: &str, stop: GenerationStop| {
        let request = ToolRecoveryRequest::default()
            .stop(stop)
            .dialects(set(&[DialectId::Hermes]));
        let calls = value(tools.recover(output.as_bytes(), &request).outcome);
        (calls.finish, calls.finish.openai_finish_reason())
    };
    let call = r#"<tool_call>{"name": "pay", "arguments": {"amount": 1}}</tool_call>"#;
    assert_eq!(
        finish(call, GenerationStop::Stop),
        (Finish::Calls, "tool_calls")
    );
    assert_eq!(
        finish("hello", GenerationStop::Stop),
        (Finish::ContentOnly, "stop")
    );
    assert_eq!(
        finish(
            &format!("{call}\n<tool_call>{{\"name\": \"pa"),
            GenerationStop::Length
        ),
        (Finish::TruncatedWithCalls, "length")
    );
    assert_eq!(
        finish("<tool_call>{\"name\": \"pa", GenerationStop::Length),
        (Finish::Truncated, "length")
    );
    // "stop" also hides a rejected attempt: the finish says Unresolved.
    assert_eq!(
        finish(
            &format!("{call}<tool_call>{{\"name\": \"pay\", oops}}</tool_call>"),
            GenerationStop::Stop
        ),
        (Finish::Unresolved, "stop")
    );
}

/// A dialect whose `call` is the built-in dialect's `declaration`, so the
/// recognizer can read rendered declarations back.
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

/// The name and parameters text `dialect`'s `declaration` production reads
/// from all of `rendered`.
fn read_declaration(dialect: &DialectId, rendered: &str) -> (String, Option<String>) {
    let empty = ToolSet::compile([], Budget::default()).expect("no tools");
    let reader = declaration_reader(dialect);
    let mut governor = Governor::new(Budget::default());
    let mut recognizer =
        Recognizer::with_tools(&reader, &empty, ReasoningPolicy::default(), &mut governor)
            .expect("recognizer");
    recognizer
        .push(rendered.as_bytes(), &mut governor)
        .expect("push");
    let turn = recognizer
        .finish(GenerationStop::Stop, &mut governor)
        .expect("finish");
    let calls = turn.calls();
    assert_eq!(calls.len(), 1, "{rendered}");
    let read = &calls[0].readings[0];
    assert_eq!(read.span, 0..rendered.len(), "{rendered}");
    let captured = read
        .captures
        .iter()
        .find(|capture| capture.kind == CaptureKind::Arguments)
        .map(|capture| rendered[capture.span.clone()].to_owned());
    (read.name.as_ref().expect("name").value.clone(), captured)
}

#[test]
fn declaration_forms_render_every_tool() {
    let builder = ToolSet::compile(
        [ToolDefinition::new("read_file")
            .description("Read a \"text\" file.")
            .param("path", Param::string().required())
            .param("limit", Param::integer())],
        Budget::default(),
    )
    .expect("builder");
    let adapted = [
        ToolSet::from_openai(
            r#"[{"type": "function", "function": {"name": "search", "description": "Find", "strict": true, "parameters": {"type": "object", "properties": {"q": {"type": "string", "format": "x"}}}}}]"#,
            Budget::default(),
        ),
        ToolSet::from_mcp(
            r#"{"tools": [{"name": "echo", "inputSchema": {"type":"object", "x-vendor": 1}, "annotations": {"readOnlyHint": true}}]}"#,
            Budget::default(),
        ),
        ToolSet::from_anthropic(
            r#"[{"name": "count", "description": "Count", "input_schema": {"type": "object", "properties": {"n": {"type": "integer", "default": -0.0}}}}]"#,
            Budget::default(),
        ),
        ToolSet::from_mcp(r#"{"name": "bare"}"#, Budget::default()),
    ]
    .map(|tools| tools.expect("adapter"));
    let mut seen = 0;
    for tools in core::iter::once(&builder).chain(&adapted) {
        for tool in tools.tools() {
            let (own, parameters) = match tool.source() {
                ToolSource::Builder => (None, Some(tool.json_schema())),
                ToolSource::Declaration(declaration) => {
                    (Some(declaration), declaration.parameters_text())
                }
            };
            for form in DeclarationForm::ALL {
                let rendered = tools
                    .render_declaration(tool.name(), form)
                    .unwrap_or_else(|error| panic!("{} {form:?}: {error}", tool.name()));
                seen += 1;
                if let Some(declaration) = own.filter(|declaration| declaration.form() == form) {
                    assert_eq!(rendered, declaration.text(), "byte-exact source form");
                    continue;
                }
                let schema = parameters.unwrap_or(match form {
                    DeclarationForm::OpenAi
                    | DeclarationForm::Hermes
                    | DeclarationForm::Llama3Json
                    | DeclarationForm::MistralV3
                    | DeclarationForm::MistralV11
                    | DeclarationForm::DeepSeekV3
                    | DeclarationForm::KimiK2 => "",
                    _ => r#"{"type":"object"}"#,
                });
                assert!(
                    rendered.contains(schema),
                    "{} {form:?}: {rendered}",
                    tool.name()
                );
                if let Some(description) = tool.description() {
                    let spelled = if matches!(
                        form,
                        DeclarationForm::Qwen3CoderXml | DeclarationForm::DeepSeekV31
                    ) {
                        description.to_owned()
                    } else {
                        json::quote(description)
                    };
                    assert!(rendered.contains(&spelled), "{rendered}");
                }
                let Some(dialect) = form.dialect() else {
                    continue;
                };
                // The dialect's own declaration production reads it back.
                let (name, captured) = read_declaration(&dialect, &rendered);
                assert_eq!(name, tool.name());
                assert_eq!(
                    captured.as_deref(),
                    parameters.or_else(|| {
                        (matches!(
                            form,
                            DeclarationForm::Qwen3CoderXml | DeclarationForm::DeepSeekV31
                        ))
                        .then_some(r#"{"type":"object"}"#)
                    }),
                    "{rendered}"
                );
            }
        }
    }
    assert_eq!(seen, 5 * DeclarationForm::ALL.len());
    // The builder tool in the Qwen3-Coder form, exactly.
    assert_eq!(
        builder
            .render_declaration("read_file", DeclarationForm::Qwen3CoderXml)
            .expect("qwen"),
        format!(
            "<function>\n<name>read_file</name>\n<description>Read a \"text\" file.</description>\n<parameters>{}</parameters>\n</function>",
            builder.render_json_schema("read_file").expect("schema")
        )
    );
}

/// The DeepSeek-V3 declaration form: the `OpenAI` tool object in a fenced
/// JSON block.
#[test]
fn deepseek_v3_declaration_form() {
    let builder = ToolSet::compile(
        [ToolDefinition::new("read_file")
            .description("Read a \"text\" file.")
            .param("path", Param::string().required())
            .param("limit", Param::integer())],
        Budget::default(),
    )
    .expect("builder");
    assert_eq!(
        builder
            .render_declaration("read_file", DeclarationForm::DeepSeekV3)
            .expect("deepseek-v3"),
        format!(
            "```json\n{{\"type\":\"function\",\"function\":{{\"name\":\"read_file\",\"description\":\"Read a \\\"text\\\" file.\",\"parameters\":{}}}}}\n```",
            builder.render_json_schema("read_file").expect("schema")
        )
    );
}

#[test]
fn tool_results_roundtrip() {
    let empty = ToolSet::compile([], Budget::default()).expect("no tools");
    for dialect in DialectId::BUILT_IN {
        for content in ["22 \"C\" <b>", "line one\n  line two\n", ""] {
            let result = ToolResult {
                id: Some("call_3"),
                name: None,
                content,
            };
            let rendered = emit::render_tool_result(&dialect, &result).expect("rendered");
            let context = format!(
                "{}: {:?}",
                dialect.id(),
                String::from_utf8_lossy(&rendered.bytes)
            );
            let own = set(core::slice::from_ref(&dialect));
            let mut governor = Governor::new(Budget::default());
            let mut recognizer =
                Recognizer::with_tools(&own, &empty, ReasoningPolicy::default(), &mut governor)
                    .expect("recognizer");
            recognizer
                .push(&rendered.bytes, &mut governor)
                .expect("push");
            let turn = recognizer
                .finish(GenerationStop::Stop, &mut governor)
                .expect("finish");
            let found: Vec<_> = turn
                .selected()
                .into_iter()
                .filter_map(|segment| match segment {
                    Segment::Result(result) => Some(&result.readings[0]),
                    _ => None,
                })
                .collect();
            assert_eq!(found.len(), 1, "{context}: {:#?}", turn.pieces());
            assert_eq!(found[0].span, 0..rendered.bytes.len(), "{context}");
            let captured = found[0]
                .capture_text(CaptureKind::Content, &rendered.bytes)
                .unwrap_or_default();
            if matches!(dialect, DialectId::OpenAiToolCalls | DialectId::MistralV3) {
                // A tool message's string content is a JSON string, and it
                // names the call it answers.
                assert_eq!(
                    json::parse(captured).expect("JSON string").as_str(),
                    Some(content),
                    "{context}"
                );
                assert_eq!(found[0].id.as_ref().expect("id").value, "call_3");
                assert_eq!(rendered.omitted, []);
            } else if matches!(dialect, DialectId::MistralV11 | DialectId::KimiK2) {
                // Verbatim content, after the id of the call it answers.
                assert_eq!(captured, content, "{context}");
                assert_eq!(found[0].id.as_ref().expect("id").value, "call_3");
                assert_eq!(rendered.omitted, []);
            } else {
                assert_eq!(captured, content, "{context}");
                assert!(found[0].id.is_none());
                assert_eq!(rendered.omitted, [CaptureKind::Id], "{context}");
            }
        }
    }
}

/// `edited` is `original` with exactly `range` replaced by `text`.
fn assert_spliced(original: &[u8], edited: &[u8], range: core::ops::Range<usize>, text: &str) {
    let mut expected = original[..range.start].to_vec();
    expected.extend_from_slice(text.as_bytes());
    expected.extend_from_slice(&original[range.end..]);
    assert_eq!(
        String::from_utf8_lossy(edited),
        String::from_utf8_lossy(&expected)
    );
}

fn read_tools() -> ToolSet {
    ToolSet::compile(
        [ToolDefinition::new("read")
            .param("path", Param::string().required())
            .param("limit", Param::integer())],
        Budget::default(),
    )
    .expect("tools")
}

fn qwen_call(path: &str, limit: u32) -> String {
    format!(
        "<tool_call>\n<function=read>\n<parameter=path>\n{path}\n</parameter>\n<parameter=limit>\n{limit}\n</parameter>\n</function>\n</tool_call>"
    )
}

fn hermes_call(path: &str, limit: u32) -> String {
    format!(
        "<tool_call>\n{{\"name\": \"read\", \"arguments\": {{\"path\": \"{path}\", \"limit\": {limit}}}}}\n</tool_call>"
    )
}

fn llama3_call(path: &str, limit: u32) -> String {
    format!("{{\"name\": \"read\", \"parameters\": {{\"path\": \"{path}\", \"limit\": {limit}}}}}")
}

fn openai_call(path: &str, limit: u32) -> String {
    format!(
        "{{\"id\": \"c{limit}\", \"type\": \"function\", \"function\": {{\"name\": \"read\", \"arguments\": \"{{\\\"path\\\": \\\"{path}\\\", \\\"limit\\\": {limit}}}\"}}}}"
    )
}

/// Edit the second of three calls of a message with prose and reasoning:
/// only the changed value's bytes differ.
fn put_in(dialect: &DialectId, call: fn(&str, u32) -> String, reasoning: &str, path: &str) {
    let tools = read_tools();
    let original = format!(
        "{reasoning}Reading three files.\n{}\nthen\n{}\nand\n{}\nDone.",
        call("a.rs", 1),
        call("src/b.rs", 2),
        call("c.rs", 3)
    );
    let source = original.as_bytes();
    let recovered = recover_in(&tools, source, dialect);
    assert_eq!(recovered.calls.len(), 3, "{}", dialect.id());
    let span = |pointer: &str| {
        recovered.calls[1]
            .arguments
            .pointer(pointer)
            .and_then(|node| node.span.clone())
            .expect("spanned value")
    };
    // One argument value, by pointer.
    let edited = recovered
        .reemit_with(
            source,
            &[CallEdit::Value {
                call: 1,
                pointer: "/path".into(),
                value: Node::new(Value::String("lib/d e.rs".into())),
            }],
        )
        .expect("edited");
    assert_spliced(source, &edited, span("/path"), path);
    let again = recover_in(&tools, &edited, dialect);
    assert_eq!(
        again.calls[1].arguments.to_json(),
        r#"{"path":"lib/d e.rs","limit":2}"#
    );
    for index in [0, 2] {
        assert_eq!(
            again.calls[index].arguments.to_json(),
            recovered.calls[index].arguments.to_json()
        );
    }
    // Whole arguments differing in one value: the same minimal splice.
    let mut arguments = recovered.calls[1].arguments.clone();
    *arguments.pointer_mut("/limit").expect("limit") = json::parse("40").expect("number");
    let edited = recovered
        .reemit_with(source, &[CallEdit::Arguments { call: 1, arguments }])
        .expect("edited");
    assert_spliced(source, &edited, span("/limit"), "40");
    // Nothing to change: the source back, byte for byte.
    let unchanged = recovered
        .reemit_with(
            source,
            &[CallEdit::Arguments {
                call: 2,
                arguments: recovered.calls[2].arguments.clone(),
            }],
        )
        .expect("unchanged");
    assert_eq!(unchanged, source);
}

/// A call that shares one `[TOOL_CALLS]` array with others: an edit
/// splices only the changed value, and every call recovers as before.
#[test]
fn put_minimal_diff_inside_a_call_array() {
    let tools = read_tools();
    let original = r#"[TOOL_CALLS] [{"name": "read", "arguments": {"path": "a.rs", "limit": 1}, "id": "aaaaaaaaa"}, {"name": "read", "arguments": {"path": "src/b.rs", "limit": 2}, "id": "bbbbbbbbb"}, {"name": "read", "arguments": {"path": "c.rs", "limit": 3}, "id": "ccccccccc"}]"#;
    let source = original.as_bytes();
    let recovered = recover_in(&tools, source, &DialectId::MistralV3);
    assert_eq!(recovered.calls.len(), 3);
    let span = recovered.calls[1]
        .arguments
        .pointer("/path")
        .and_then(|node| node.span.clone())
        .expect("spanned value");
    let edited = recovered
        .reemit_with(
            source,
            &[CallEdit::Value {
                call: 1,
                pointer: "/path".into(),
                value: Node::new(Value::String("lib/d e.rs".into())),
            }],
        )
        .expect("edited");
    assert_spliced(source, &edited, span, "\"lib/d e.rs\"");
    let again = recover_in(&tools, &edited, &DialectId::MistralV3);
    let arguments: Vec<String> = again
        .calls
        .iter()
        .map(|call| call.arguments.to_json())
        .collect();
    assert_eq!(
        arguments,
        [
            r#"{"path":"a.rs","limit":1}"#,
            r#"{"path":"lib/d e.rs","limit":2}"#,
            r#"{"path":"c.rs","limit":3}"#
        ]
    );
    let ids: Vec<Option<&str>> = again
        .calls
        .iter()
        .map(|call| call.source_id.as_deref())
        .collect();
    assert_eq!(
        ids,
        [Some("aaaaaaaaa"), Some("bbbbbbbbb"), Some("ccccccccc")]
    );
}

#[test]
fn put_minimal_diff() {
    let think = "<think>Which files?</think>";
    put_in(&DialectId::Qwen3CoderXml, qwen_call, think, "lib/d e.rs");
    put_in(&DialectId::Hermes, hermes_call, think, "\"lib/d e.rs\"");
    put_in(&DialectId::Llama3Json, llama3_call, "", "\"lib/d e.rs\"");
    put_in(
        &DialectId::OpenAiToolCalls,
        openai_call,
        "",
        "\\\"lib/d e.rs\\\"",
    );
    // A verbatim value that cannot stay verbatim regenerates its enclosing
    // value: the Qwen3-Coder body becomes one JSON object.
    let tools = read_tools();
    let original = qwen_call("a.rs", 1);
    let recovered = recover_in(&tools, original.as_bytes(), &DialectId::Qwen3CoderXml);
    let edit = |pointer: &str, value: Node| {
        recovered.reemit_with(
            original.as_bytes(),
            &[CallEdit::Value {
                call: 0,
                pointer: pointer.into(),
                value,
            }],
        )
    };
    let edited = edit("/path", Node::new(Value::String("x</parameter>y".into()))).expect("edited");
    let again = recover_in(&tools, &edited, &DialectId::Qwen3CoderXml);
    assert_eq!(
        again.calls[0].arguments.to_json(),
        r#"{"path":"x</parameter>y","limit":1}"#
    );
    assert_eq!(
        String::from_utf8_lossy(&edited),
        "<tool_call>\n<function=read>\n{\"path\":\"x\\u003c/parameter>y\",\"limit\":1}\n</function>\n</tool_call>"
    );
    // An edit naming no value, or a source not holding the calls, is refused.
    assert!(matches!(
        edit("/missing", Node::new(Value::Null)),
        Err(EmitError::InvalidEdit { .. })
    ));
    assert!(matches!(
        recovered.reemit_with(
            b"short",
            &[CallEdit::Arguments {
                call: 0,
                arguments: Node::new(Value::Null)
            }]
        ),
        Err(EmitError::InvalidEdit { .. })
    ));
}

/// A message with a usable call, a call to an undeclared tool, and a
/// rejected attempt.
const MIXED: &str = "<tool_call>{\"name\": \"pay\", \"arguments\": {\"amount\": 1.50}}</tool_call>\n<tool_call>{\"name\": \"launch_rockets\", \"arguments\": {}}</tool_call>\n<tool_call>{\"name\": \"pay\", oops}</tool_call>\n<tool_call>{\"name\": \"pay\", \"arguments\": {\"amount\": 3}}</tool_call>";

#[test]
fn emit_option_end_to_end() {
    let tools = payment_tools();
    let hermes = || ToolRecoveryRequest::default().dialects(set(&[DialectId::Hermes]));
    let plain = tools.recover(MIXED.as_bytes(), &hermes());
    assert!(
        plain
            .to_node()
            .get("value")
            .is_none_or(|value| value.get("emitted").is_none())
    );
    let plain = value(plain.outcome);
    assert!(plain.emitted.is_none());
    assert!(plain.to_node().get("emitted").is_none());
    for target in [
        EmitTarget::OpenAiToolCalls,
        EmitTarget::Dialect(DialectId::Hermes),
    ] {
        let request = hermes().emit(target);
        let mut governor = Governor::new(Budget::default());
        let result = tools.recover_governed(MIXED.as_bytes(), &request, &mut governor);
        let calls = value(result.outcome);
        let emission = calls.emitted.clone().expect("emitted");
        let skipped = emission.skipped();
        assert!(
            skipped.iter().any(|skipped| matches!(
                skipped,
                Skipped::Call {
                    index: 1,
                    reason: SkipReason::Unresolved { .. }
                }
            )),
            "{skipped:?}"
        );
        assert!(
            skipped
                .iter()
                .any(|skipped| matches!(skipped, Skipped::Rejected { index: 0, .. }))
        );
        match &emission {
            Emission::OpenAiToolCalls(emitted) => {
                assert_eq!(emitted.calls, [0, 2]);
                // Default ids: the turn's digest tag and the call index.
                let tag = turn_id_prefix(&calls.turn);
                assert_eq!(
                    emitted.output.to_json(),
                    format!(
                        r#"[{{"id":"{tag}0","type":"function","function":{{"name":"pay","arguments":"{{\"amount\":1.50}}"}}}},{{"id":"{tag}2","type":"function","function":{{"name":"pay","arguments":"{{\"amount\":3}}"}}}}]"#
                    )
                );
            }
            Emission::Dialect { dialect, emitted } => {
                assert_eq!(dialect, &DialectId::Hermes);
                assert_eq!(emitted.calls, [0, 2]);
                assert_eq!(
                    emitted.output,
                    b"<tool_call>\n{\"name\":\"pay\",\"arguments\":{\"amount\":1.50}}\n</tool_call>\n<tool_call>\n{\"name\":\"pay\",\"arguments\":{\"amount\":3}}\n</tool_call>"
                );
            }
        }
        // The wire form carries the emission; without it the value is the
        // plain recovery's.
        let mut wire = calls.to_node();
        assert!(wire.get("emitted").is_some());
        let Value::Object(members) = &mut wire.value else {
            panic!("an object")
        };
        members.retain(|member| member.key != "emitted");
        assert_eq!(wire.to_json(), plain.to_node().to_json());
        // Printing is charged to the recovery's governor.
        let mut quiet = Governor::new(Budget::default());
        let _ = tools.recover_governed(MIXED.as_bytes(), &hermes(), &mut quiet);
        assert!(governor.usage().work > quiet.usage().work);
    }
}

#[test]
fn emission_never_emits_unresolved_silently() {
    let tools = payment_tools();
    let calls = value(
        tools
            .recover(
                MIXED.as_bytes(),
                &ToolRecoveryRequest::default()
                    .dialects(set(&[DialectId::Hermes]))
                    .ids(IdPolicy::Source),
            )
            .outcome,
    );
    assert_eq!(calls.calls.len(), 3);
    assert_eq!(calls.rejected.len(), 1);
    let check = |printed: &[usize], skipped: &[Skipped], context: &str| {
        for call in &calls.calls {
            let listed = skipped
                .iter()
                .filter(|skipped| matches!(skipped, Skipped::Call { index, .. } if *index == call.index))
                .count();
            let emitted = printed.contains(&call.index);
            assert_eq!(
                usize::from(emitted) + listed,
                1,
                "{context}: call {}",
                call.index
            );
            if !call.is_usable() {
                assert!(!emitted, "{context}: call {}", call.index);
            }
        }
        assert!(
            skipped
                .iter()
                .any(|skipped| matches!(skipped, Skipped::Rejected { index: 0, .. })),
            "{context}"
        );
    };
    let array = emit::openai_tool_calls(&calls, &IdPolicy::Source).expect("array");
    check(&array.calls, &array.skipped, "openai_tool_calls");
    // Source ids the output does not carry are numbered `call#i`.
    assert_eq!(
        array.output.pointer("/0/id").and_then(Node::as_str),
        Some("call#0")
    );
    for dialect in DialectId::BUILT_IN {
        let printed = emit::emit(&tools, &calls, &dialect, &IdPolicy::Source).expect("emitted");
        check(&printed.calls, &printed.skipped, dialect.id());
        if dialect.grammar().call_carries_ids() && dialect != DialectId::KimiK2 {
            // `call#0` is not an id the OpenAI or Mistral call-id class
            // accepts: those calls are skipped as unrepresentable, never
            // printed with another id.
            assert_eq!(printed.calls, Vec::<usize>::new());
            assert!(printed.skipped.iter().any(|skipped| matches!(
                skipped,
                Skipped::Call {
                    reason: SkipReason::Unrepresentable { .. },
                    ..
                }
            )));
            continue;
        }
        let again = recover_in(&tools, &printed.output, &dialect);
        let tools_called: Vec<_> = again.calls.iter().map(|call| call.tool.clone()).collect();
        assert_eq!(
            tools_called,
            [Some("pay".to_owned()), Some("pay".to_owned())]
        );
        assert_eq!(again.rejected, Vec::new());
    }
}

#[test]
fn mutating_guess_never_emitted() {
    let output = br#"<tool_call>{"name": "write_fiel", "arguments": {"path": "a.rs"}}</tool_call>"#;
    for effects in [Effects::Mutating, Effects::ReadOnly] {
        let tools = ToolSet::compile(
            [ToolDefinition::new("write_file")
                .effects(effects)
                .param("path", Param::string().required())],
            Budget::default(),
        )
        .expect("tools");
        let base = ToolRecoveryRequest::default()
            .dialects(set(&[DialectId::Hermes]))
            .policy(PolicySelection::All(RepairPolicy::Tolerant));
        let Outcome::BestGuess(calls) = tools.recover(output, &base).outcome else {
            panic!("a near-miss name is a best guess")
        };
        let call = &calls.calls[0];
        assert_eq!(call.outcome, CallOutcome::BestGuess);
        assert_eq!(call.tool.as_deref(), Some("write_file"));
        let guessed_write = effects == Effects::Mutating;
        let check = |printed: &[usize], skipped: &[Skipped], context: &str| {
            if guessed_write {
                assert_eq!(printed, &[] as &[usize], "{context}");
                assert_eq!(
                    skipped,
                    [Skipped::Call {
                        index: 0,
                        reason: SkipReason::GuessOnMutatingTool
                    }],
                    "{context}"
                );
            } else {
                assert_eq!(printed, [0], "{context}");
                assert_eq!(skipped, [], "{context}");
            }
        };
        let array = emit::openai_tool_calls(&calls, &IdPolicy::default()).expect("array");
        check(&array.calls, &array.skipped, "openai_tool_calls");
        assert_eq!(
            array.output.to_json() == "[]",
            guessed_write,
            "{}",
            array.output.to_json()
        );
        for dialect in DialectId::BUILT_IN {
            let printed = emit::emit(&tools, &calls, &dialect, &IdPolicy::default()).expect("emit");
            check(&printed.calls, &printed.skipped, dialect.id());
            assert_eq!(printed.output.is_empty(), guessed_write, "{}", dialect.id());
        }
        for target in [
            EmitTarget::OpenAiToolCalls,
            EmitTarget::Dialect(DialectId::Qwen3CoderXml),
        ] {
            let request = base.clone().emit(target);
            let recovered = value(tools.recover(output, &request).outcome);
            let emission = recovered.emitted.expect("emitted");
            let (printed, skipped) = match &emission {
                Emission::OpenAiToolCalls(emitted) => (&emitted.calls, &emitted.skipped),
                Emission::Dialect { emitted, .. } => (&emitted.calls, &emitted.skipped),
            };
            check(printed, skipped, "recovery emission");
        }
        assert_eq!(
            calls.usable_accepting_guesses().len(),
            usize::from(!guessed_write)
        );
    }
}

#[test]
fn qwen_canonical_matches_template_layout() {
    let tools = ToolSet::compile(
        [ToolDefinition::new("write_file")
            .param("path", Param::string().required())
            .param("content", Param::string())
            .param("count", Param::integer())],
        Budget::default(),
    )
    .expect("tools");
    // What the published chat template renders for one assistant tool call.
    let template = "<tool_call>\n<function=write_file>\n<parameter=path>\nsrc/main.rs\n</parameter>\n<parameter=content>\nfn main() {}\n\n</parameter>\n<parameter=count>\n-0\n</parameter>\n</function>\n</tool_call>";
    let calls = recover_in(&tools, template.as_bytes(), &DialectId::Qwen3CoderXml);
    let printed = emit::emit(
        &tools,
        &calls,
        &DialectId::Qwen3CoderXml,
        &IdPolicy::default(),
    )
    .expect("emit");
    assert_eq!(String::from_utf8(printed.output).expect("UTF-8"), template);
    // From another dialect, the same layout.
    let hermes = r#"<tool_call>{"name": "write_file", "arguments": {"path": "src/main.rs", "content": "fn main() {}\n", "count": -0}}</tool_call>"#;
    let calls = recover_in(&tools, hermes.as_bytes(), &DialectId::Hermes);
    let printed = emit::emit(
        &tools,
        &calls,
        &DialectId::Qwen3CoderXml,
        &IdPolicy::default(),
    )
    .expect("emit");
    assert_eq!(String::from_utf8(printed.output).expect("UTF-8"), template);
    // Hermes: the template's wrapper layout around a compact object.
    let printed =
        emit::emit(&tools, &calls, &DialectId::Hermes, &IdPolicy::default()).expect("emit");
    assert_eq!(
        String::from_utf8(printed.output).expect("UTF-8"),
        "<tool_call>\n{\"name\":\"write_file\",\"arguments\":{\"path\":\"src/main.rs\",\"content\":\"fn main() {}\\n\",\"count\":-0}}\n</tool_call>"
    );
}

#[test]
fn layout_statements_are_checked() {
    let compile = |layout: &str| {
        Grammar::compile_governed(
            &format!(
                "grammar \"g\"; closer end = \"</c>\"; ws = /[ \\n]+/; alias = ws; {layout} call = \"<c>\" ws? end; result = \"<r>\"; declaration = \"<d>\";"
            ),
            &mut Governor::new(Budget::default()),
        )
    };
    let grammar = compile("layout alias \"\\n\";").expect("an alias of a class");
    let (alias, _) = grammar.production("alias").expect("alias");
    assert_eq!(grammar.layout(alias), Some(&b"\n"[..]));
    for (layout, expected) in [
        ("layout ws \"x\";", "not a member"),
        ("layout call \" \";", "only a class production"),
        ("layout end \" \";", "closer"),
    ] {
        match compile(layout) {
            Err(GrammarError::InvalidLayout { message, .. }) => {
                assert!(message.contains(expected), "{layout}: {message}");
            }
            other => panic!("{layout}: {other:?}"),
        }
    }
    assert!(matches!(
        compile("layout missing \" \";"),
        Err(GrammarError::UnknownName { .. })
    ));
    assert!(matches!(
        compile("layout ws \" \"; layout ws \"\\n\";"),
        Err(GrammarError::DuplicateName { .. })
    ));
}
