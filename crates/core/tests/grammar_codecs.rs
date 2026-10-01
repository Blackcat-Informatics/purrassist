// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Argument codecs through the recognizer: the `xml-params` codec (D9
//! framing, verbatim strings, typed values, declared-parameter boundaries,
//! nesting, whole-JSON bodies, repeats), the `json` codec (exact spellings,
//! stringified arguments), malformed UTF-8 as unresolved arguments, and
//! chunking invariance of values and governor usage.
use purrassist_core::contract::{Resource, Usage};
use purrassist_core::grammar::Grammar;
use purrassist_core::grammar::codec::CodecKind;
use purrassist_core::grammar::edit::{EditOperator, Grounding, LossKind, Readings};
use purrassist_core::json::Value;
use purrassist_core::provenance::ProvenanceNode;
use purrassist_core::tool_call::{
    ArgOrigin, ArgumentFault, Closure, DialectId, DialectSet, GenerationStop, ObjectSpec, Param,
    RawCall, ReasoningPolicy, RecognizedTurn, Recognizer, Rejection, Segment, ToolDefinition,
    ToolSet,
};
use purrassist_core::{Budget, Governor, StopReason};

const QWEN: &str = include_str!("data/grammar/qwen-mini.grammar");
const HERMES: &str = include_str!("data/grammar/hermes-mini.grammar");

fn set(text: &str) -> DialectSet {
    let mut governor = Governor::new(Budget::default());
    let grammar = Grammar::compile_governed(text, &mut governor).expect("grammar compiles");
    DialectSet::new([DialectId::custom(grammar)], &mut governor).expect("dialect set")
}

fn tools() -> ToolSet {
    let config = ObjectSpec::new()
        .param("depth", Param::integer())
        .param("name", Param::string());
    ToolSet::compile(
        [
            ToolDefinition::new("configure")
                .param("config", Param::object(config))
                .param("label", Param::string().alias("title")),
            ToolDefinition::new("write")
                .param("path", Param::string())
                .param("content", Param::string())
                .param("count", Param::integer())
                .param("ratio", Param::number())
                .param("scale", Param::number())
                .param("flag", Param::boolean())
                .param(
                    "options",
                    Param::object(ObjectSpec::new().param("mode", Param::string())),
                ),
        ],
        Budget::default(),
    )
    .expect("tools compile")
}

fn run(
    set: &DialectSet,
    tools: &ToolSet,
    chunks: &[&[u8]],
    governor: &mut Governor,
) -> Result<RecognizedTurn, StopReason> {
    let mut recognizer = Recognizer::with_tools(set, tools, ReasoningPolicy::default(), governor)?;
    for chunk in chunks {
        recognizer.push(chunk, governor)?;
    }
    recognizer.finish(GenerationStop::Stop, governor)
}

fn recognize(set: &DialectSet, input: &[u8]) -> (RecognizedTurn, Usage) {
    let tools = tools();
    let mut governor = Governor::new(Budget::default());
    let turn = run(set, &tools, &[input], &mut governor).expect("recognized");
    (turn, governor.usage())
}

fn one_call(turn: &RecognizedTurn) -> &RawCall {
    let calls = turn.calls();
    assert_eq!(calls.len(), 1, "{:#?}", turn.pieces());
    &calls[0].readings[0]
}

fn qwen(body: &str) -> String {
    format!("<tool_call>\n<function=write>\n{body}\n</function>\n</tool_call>")
}

fn operators(call: &RawCall) -> Vec<EditOperator> {
    call.edits.iter().map(|edit| edit.operator).collect()
}

fn pointers(call: &RawCall) -> Vec<&str> {
    call.arguments
        .leaves
        .iter()
        .map(|leaf| leaf.pointer.as_str())
        .collect()
}

/// Every source-range spelling of the leaves, as text.
fn raw_texts<'a>(call: &RawCall, input: &'a [u8]) -> Vec<&'a str> {
    call.arguments
        .leaves
        .iter()
        .map(|leaf| core::str::from_utf8(&input[leaf.raw.clone()]).expect("utf-8"))
        .collect()
}

fn rejected(turn: &RecognizedTurn) -> Option<Rejection> {
    turn.selected()
        .into_iter()
        .find_map(|segment| match segment {
            Segment::Rejected { reason, .. } => Some(*reason),
            _ => None,
        })
}

#[test]
fn codecs_are_registered_in_the_dsl() {
    assert_eq!(CodecKind::from_name("json"), Some(CodecKind::Json));
    assert_eq!(
        CodecKind::from_name("xml-params"),
        Some(CodecKind::XmlParams)
    );
    assert_eq!(CodecKind::ALL.len(), 3);
    for kind in CodecKind::ALL {
        assert_eq!(CodecKind::from_name(kind.name()), Some(kind));
    }
}

#[test]
fn nested_shape_xml_equals_json() {
    let expected = r#"{"config":{"depth":3,"name":"x y"},"label":"hi"}"#;
    let nested = "<tool_call>\n<function=configure>\n<parameter=config>\n<parameter=depth>\n3\n</parameter>\n<parameter=name>\nx y\n</parameter>\n</parameter>\n<parameter=label>\nhi\n</parameter>\n</function>\n</tool_call>";
    let inline = "<tool_call>\n<function=configure>\n<parameter=config>\n{\"depth\": 3, \"name\": \"x y\"}\n</parameter>\n<parameter=label>\nhi\n</parameter>\n</function>\n</tool_call>";
    let json = r#"<tool_call>{"name": "configure", "arguments": {"config": {"depth": 3, "name": "x y"}, "label": "hi"}}</tool_call>"#;
    let qwen_set = set(QWEN);
    let hermes_set = set(HERMES);
    let mut shapes = Vec::new();
    for (set, input) in [
        (&qwen_set, nested),
        (&qwen_set, inline),
        (&hermes_set, json),
    ] {
        let (turn, _) = recognize(set, input.as_bytes());
        let call = one_call(&turn);
        assert_eq!(call.closure, Closure::Closed, "{input}");
        assert!(call.arguments.is_resolved());
        assert_eq!(call.arguments.tool.as_deref(), Some("configure"));
        assert_eq!(call.arguments.root.to_json(), expected, "{input}");
        assert_eq!(
            pointers(call),
            ["/config/depth", "/config/name", "/label"],
            "{input}"
        );
        shapes.push(call.arguments.root.to_json());
    }
    assert!(shapes.windows(2).all(|pair| pair[0] == pair[1]));
    // The nested form keeps verbatim raw ranges: the integer is read as JSON
    // and the strings as XML text.
    let (turn, _) = recognize(&qwen_set, nested.as_bytes());
    let call = one_call(&turn);
    assert_eq!(raw_texts(call, nested.as_bytes()), ["3", "x y", "hi"]);
    let origins: Vec<ArgOrigin> = call
        .arguments
        .leaves
        .iter()
        .map(|leaf| leaf.origin)
        .collect();
    assert_eq!(
        origins,
        [ArgOrigin::Json, ArgOrigin::XmlText, ArgOrigin::XmlText]
    );
}

#[test]
fn framing_newline_consumed_and_recorded() {
    let input = qwen("<parameter=content>\n\nline\n\n</parameter>");
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    // Exactly one framing newline on each side.
    assert_eq!(call.arguments.root.to_json(), r#"{"content":"\nline\n"}"#);
    assert_eq!(
        operators(call),
        [EditOperator::FramingNewline, EditOperator::FramingNewline]
    );
    let open = input.find("<parameter=content>").expect("opener") + "<parameter=content>".len();
    let close = input.find("</parameter>").expect("closer");
    assert_eq!(call.edits[0].span, open..open + 1);
    assert_eq!(call.edits[1].span, close - 1..close);
    for edit in &call.edits {
        assert_eq!(edit.original, b"\n");
        assert_eq!(edit.replacement.len(), 0);
        let rule = edit.rule();
        assert_eq!(rule.id, "tool_call.xml.framing-newline");
        assert!(!rule.guess && !rule.repair);
        assert_eq!(rule.readings, Readings::Single);
        assert_eq!(rule.loss, Some(LossKind::Markup));
        assert_eq!(rule.cost, 0);
    }
    // Reading framing is not a repair: the call stays closed and costs nothing.
    assert_eq!(call.closure, Closure::Closed);
    assert_eq!(call.cost, 0);
    let leaf = &call.arguments.leaves[0];
    assert_eq!(leaf.origin, ArgOrigin::XmlText);
    assert_eq!(leaf.raw, open + 1..close - 1);
    // The evidence and the markup loss reach the provenance forest; no guess.
    let nodes = turn.forest().nodes();
    let evidence = nodes.iter().filter(|node| {
        matches!(node, ProvenanceNode::Evidence(evidence) if evidence.rule == "tool_call.xml.framing-newline")
    });
    assert_eq!(evidence.count(), 2);
    assert!(nodes.iter().any(|node| {
        matches!(node, ProvenanceNode::Loss(loss) if loss.evidence.rule == "tool_call.xml.framing-newline")
    }));
    assert!(
        !nodes
            .iter()
            .any(|node| matches!(node, ProvenanceNode::Inference(_)))
    );
}

#[test]
fn string_verbatim_otherwise() {
    let content = "  lead\r\n\n inner\t\n\n trail  ";
    let input = qwen(&format!(
        "<parameter=path>123</parameter>\n<parameter=content>{content}</parameter>\n<parameter=extra>true</parameter>\n<parameter=title> x </parameter>"
    ));
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    let root = &call.arguments.root;
    assert_eq!(root.get("path").and_then(|node| node.as_str()), Some("123"));
    assert_eq!(
        root.get("content").and_then(|node| node.as_str()),
        Some(content)
    );
    // An undeclared parameter and an alias are verbatim strings too.
    assert_eq!(
        root.get("extra").and_then(|node| node.as_str()),
        Some("true")
    );
    assert_eq!(
        root.get("title").and_then(|node| node.as_str()),
        Some(" x ")
    );
    assert!(call.edits.is_empty(), "{:?}", call.edits);
    assert_eq!(call.closure, Closure::Closed);
    assert_eq!(
        raw_texts(call, input.as_bytes()),
        ["123", content, "true", " x "]
    );
    assert!(
        call.arguments
            .leaves
            .iter()
            .all(|leaf| leaf.origin == ArgOrigin::XmlText)
    );
}

#[test]
fn typed_values_parsed_exact_spelling() {
    let input = qwen(
        "<parameter=count>\n-0\n</parameter>\n<parameter=ratio>\n1.50\n</parameter>\n<parameter=scale>1e2</parameter>\n<parameter=flag>\ntrue\n</parameter>\n<parameter=options>\n{\"mode\": \"fast\"}\n</parameter>",
    );
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(
        call.arguments.root.to_json(),
        r#"{"count":-0,"ratio":1.50,"scale":1e2,"flag":true,"options":{"mode":"fast"}}"#
    );
    assert_eq!(
        raw_texts(call, input.as_bytes()),
        ["-0", "1.50", "1e2", "true", "\"fast\""]
    );
    assert!(
        call.arguments
            .leaves
            .iter()
            .all(|leaf| leaf.origin == ArgOrigin::Json)
    );
    // Typed values are not framed strings: their whitespace is JSON's.
    assert!(call.edits.is_empty(), "{:?}", call.edits);
    // The json codec keeps exact spellings too.
    let json = r#"<tool_call>{"name": "write", "arguments": {"count": -0, "ratio": 1.50, "scale": 1e2}}</tool_call>"#;
    let (turn, _) = recognize(&set(HERMES), json.as_bytes());
    let call = one_call(&turn);
    assert_eq!(
        call.arguments.root.to_json(),
        r#"{"count":-0,"ratio":1.50,"scale":1e2}"#
    );
    assert_eq!(raw_texts(call, json.as_bytes()), ["-0", "1.50", "1e2"]);
    assert_eq!(call.arguments.tool.as_deref(), Some("write"));
}

#[test]
fn declared_opener_is_boundary_for_typed_and_string() {
    let check = |call: &RawCall, input: &str, opener: &str| {
        let at = input.find(opener).expect("boundary opener");
        let edit = call
            .edits
            .iter()
            .find(|edit| edit.operator == EditOperator::MissingParameterClose)
            .expect("missing-parameter-close edit");
        assert_eq!(edit.span, at..at);
        assert_eq!(edit.replacement, b"</parameter>");
        let rule = edit.rule();
        assert_eq!(rule.id, "tool_call.markup.missing-parameter-close");
        assert_eq!(rule.readings, Readings::Single);
        assert!(!rule.guess && rule.repair);
        assert_eq!(rule.grounding, Grounding::Definition);
        assert_eq!(call.closure, Closure::Repaired);
    };
    // A string value ends verbatim (after its framing) at the declared opener.
    let input = qwen("<parameter=path>\na.rs\n<parameter=content>\nbody\n</parameter>");
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(
        call.arguments.root.to_json(),
        r#"{"path":"a.rs","content":"body"}"#
    );
    check(call, &input, "<parameter=content>");
    // An alias of a declared parameter is a boundary too.
    let input = "<tool_call><function=configure><parameter=label>a<parameter=title>b</parameter></function></tool_call>";
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(
        call.arguments.root.to_json(),
        r#"{"label":"a","title":"b"}"#
    );
    check(call, input, "<parameter=title>");
    // A typed value complete at the boundary ends there.
    let input = qwen("<parameter=options>{\"mode\": \"x\"}\n<parameter=count>2</parameter>");
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(
        call.arguments.root.to_json(),
        r#"{"options":{"mode":"x"},"count":2}"#
    );
    check(call, &input, "<parameter=count>");
    // A nested object's missing closer ends at an enclosing scope's opener.
    let input = "<tool_call><function=configure><parameter=config><parameter=depth>1</parameter><parameter=label>z</parameter></function></tool_call>";
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(
        call.arguments.root.to_json(),
        r#"{"config":{"depth":1},"label":"z"}"#
    );
    check(call, input, "<parameter=label>");
    // A typed value incomplete at the boundary makes the call malformed.
    let input = qwen("<parameter=options>{\"mode\": \"x\"\n<parameter=count>2</parameter>");
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    assert_eq!(turn.calls().len(), 0);
    assert!(matches!(rejected(&turn), Some(Rejection::Malformed { .. })));
}

#[test]
fn undeclared_opener_is_not_boundary() {
    let input = qwen("<parameter=content>see <parameter=nope>x</parameter>");
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(
        call.arguments.root.to_json(),
        r#"{"content":"see <parameter=nope>x"}"#
    );
    assert_eq!(call.edits.len(), 0);
    // Inside a typed value an undeclared opener is JSON text.
    let input = qwen("<parameter=options>{\"mode\": \"<parameter=nope>\"}</parameter>");
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(
        call.arguments.root.to_json(),
        r#"{"options":{"mode":"<parameter=nope>"}}"#
    );
    // ... and leaves a typed value ended by a declared opener incomplete.
    let input = qwen(
        "<parameter=options>{\"mode\": \"a\", <parameter=nope>x\n<parameter=path>p</parameter>",
    );
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    assert!(matches!(rejected(&turn), Some(Rejection::Malformed { .. })));
    // A tool that is not declared falls back: every opener is a boundary
    // (a syntax repair), and every value is a string.
    let input = "<tool_call><function=mystery><parameter=a>1<parameter=b>2</parameter></function></tool_call>";
    let (turn, _) = recognize(&set(QWEN), input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(call.arguments.root.to_json(), r#"{"a":"1","b":"2"}"#);
    assert_eq!(call.arguments.tool, None);
    assert_eq!(operators(call), [EditOperator::InsertCloser]);
}

#[test]
fn whole_json_body_accepted_only_when_entire() {
    let qwen_set = set(QWEN);
    let input =
        "<tool_call><function=write>\n{\"path\": \"a\", \"count\": 2}\n</function></tool_call>";
    let (turn, _) = recognize(&qwen_set, input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(call.arguments.root.to_json(), r#"{"path":"a","count":2}"#);
    assert_eq!(call.closure, Closure::Closed);
    assert_eq!(raw_texts(call, input.as_bytes()), ["\"a\"", "2"]);
    assert!(
        call.arguments
            .leaves
            .iter()
            .all(|leaf| leaf.origin == ArgOrigin::Json)
    );
    // One stray closer after the object is deleted.
    let input =
        "<tool_call><function=write>\n{\"path\": \"a\"}\n</parameter>\n</function></tool_call>";
    let (turn, _) = recognize(&qwen_set, input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(call.arguments.root.to_json(), r#"{"path":"a"}"#);
    assert_eq!(operators(call), [EditOperator::DeleteStrayCloser]);
    let at = input.find("</parameter>").expect("stray");
    assert_eq!(call.edits[0].span, at..at + "</parameter>".len());
    assert_eq!(call.closure, Closure::Repaired);
    // Anything else after the object rejects the call.
    for input in [
        "<tool_call><function=write>{\"path\": \"a\"} trailing</function></tool_call>",
        "<tool_call><function=write>{\"path\": \"a\"}</parameter></parameter></function></tool_call>",
        "<tool_call><function=write>{\"path\": \"a\"}<parameter=count>1</parameter></function></tool_call>",
    ] {
        let (turn, _) = recognize(&qwen_set, input.as_bytes());
        assert!(turn.calls().is_empty(), "{input}");
        assert!(
            matches!(rejected(&turn), Some(Rejection::Malformed { .. })),
            "{input}"
        );
    }
}

#[test]
fn stringified_arguments_decoded_then_flagged() {
    let input = r#"<tool_call>{"name": "write", "arguments": "{\"path\": \"aé.rs\", \"count\": 2}"}</tool_call>"#;
    let (turn, _) = recognize(&set(HERMES), input.as_bytes());
    let call = one_call(&turn);
    assert!(call.arguments.arguments_were_stringified);
    assert!(call.arguments.is_resolved());
    assert_eq!(call.arguments.tool.as_deref(), Some("write"));
    assert_eq!(
        call.arguments.root.to_json(),
        r#"{"path":"aé.rs","count":2}"#
    );
    // Leaf spans map back through the escape decoding to the source bytes.
    assert_eq!(raw_texts(call, input.as_bytes()), [r#"\"aé.rs\""#, "2"]);
    assert!(
        call.arguments
            .leaves
            .iter()
            .all(|leaf| leaf.origin == ArgOrigin::StringifiedJson)
    );
    let Value::Object(members) = &call.arguments.root.value else {
        panic!("object arguments");
    };
    let key = members[0].key_span.clone().expect("key span");
    assert_eq!(&input[key], r#"\"path\""#);
    let edit = &call.edits[0];
    assert_eq!(edit.operator, EditOperator::StringifiedArguments);
    assert_eq!(edit.rule().id, "tool_call.arguments.stringified");
    let string = input.find("\"{").expect("string")..input.rfind("}\"").expect("end") + 2;
    assert_eq!(edit.span, string);
    assert_eq!(call.closure, Closure::Closed);
    // Inner JSON repairs keep their rule ids and map to source spans.
    let input = r#"<tool_call>{"name": "write", "arguments": "{\"count\": 2,}"}</tool_call>"#;
    let (turn, _) = recognize(&set(HERMES), input.as_bytes());
    let call = one_call(&turn);
    assert_eq!(call.arguments.root.to_json(), r#"{"count":2}"#);
    let trailing = call
        .edits
        .iter()
        .find(|edit| edit.operator == EditOperator::JsonTrailingComma)
        .expect("trailing comma");
    assert_eq!(trailing.rule().id, "json.syntax.trailing-comma");
    assert_eq!(trailing.original, b",");
    assert_eq!(&input.as_bytes()[trailing.span.clone()], b",");
}

#[test]
fn stringified_arguments_invalid_json() {
    for inner in [
        r#"{\"path\": [1}"#,
        "not json at all",
        "[1, 2]",
        r#"{\"a\": 1} tail"#,
    ] {
        let input =
            format!(r#"<tool_call>{{"name": "write", "arguments": "{inner}"}}</tool_call>"#);
        let (turn, _) = recognize(&set(HERMES), input.as_bytes());
        let call = one_call(&turn);
        assert!(call.arguments.arguments_were_stringified, "{inner}");
        assert!(!call.arguments.is_resolved());
        let unresolved = &call.arguments.unresolved[0];
        assert_eq!(unresolved.pointer, "");
        assert_eq!(
            unresolved.fault,
            ArgumentFault::StringifiedArgumentsInvalidJson
        );
        // The raw text is kept: the whole string literal.
        assert_eq!(
            &input[unresolved.raw.clone()],
            format!("\"{inner}\""),
            "{inner}"
        );
        assert_eq!(call.arguments.root.to_json(), "{}");
        assert_eq!(call.arguments.leaves.len(), 0);
    }
}

#[test]
fn repeated_parameter_recorded() {
    for (body, edits) in [
        (
            "<parameter=path>a</parameter><parameter=path>b</parameter>",
            0,
        ),
        ("<parameter=path>a<parameter=path>b</parameter>", 1),
    ] {
        let input = qwen(body);
        let (turn, _) = recognize(&set(QWEN), input.as_bytes());
        let call = one_call(&turn);
        assert_eq!(call.arguments.root.to_json(), r#"{"path":"a","path":"b"}"#);
        assert_eq!(pointers(call), ["/path", "/path"]);
        assert_eq!(raw_texts(call, input.as_bytes()), ["a", "b"]);
        assert_eq!(call.edits.len(), edits);
    }
}

#[test]
fn invalid_utf8_in_argument_unresolved() {
    // An XML string parameter: that parameter is unresolved, the rest decoded.
    let mut input = b"<tool_call><function=write><parameter=path>\na".to_vec();
    let bad = input.len();
    input.extend_from_slice(
        b"\xFFb\n</parameter><parameter=content>ok</parameter></function></tool_call>",
    );
    let (turn, _) = recognize(&set(QWEN), &input);
    let call = one_call(&turn);
    assert_eq!(call.arguments.root.to_json(), r#"{"content":"ok"}"#);
    let unresolved = &call.arguments.unresolved[0];
    assert_eq!(unresolved.pointer, "/path");
    assert_eq!(
        unresolved.fault,
        ArgumentFault::InvalidUtf8InArgument { at: bad }
    );
    assert_eq!(&input[unresolved.raw.clone()], b"\na\xFFb\n");
    // A typed XML parameter.
    let input = b"<tool_call><function=write><parameter=options>{\"mode\": \"\xC3\"}</parameter></function></tool_call>";
    let (turn, _) = recognize(&set(QWEN), input);
    let call = one_call(&turn);
    assert_eq!(call.arguments.unresolved[0].pointer, "/options");
    assert!(matches!(
        call.arguments.unresolved[0].fault,
        ArgumentFault::InvalidUtf8InArgument { .. }
    ));
    // A JSON arguments value: the whole arguments are unresolved, the call
    // is still recognized (its end found through the malformed bytes), and
    // the raw span is kept.
    let input =
        b"<tool_call>{\"name\": \"write\", \"arguments\": {\"path\": \"\xFF\xFE\"}}</tool_call>";
    let (turn, _) = recognize(&set(HERMES), input);
    let call = one_call(&turn);
    assert_eq!(call.name.as_ref().expect("name").value, "write");
    let unresolved = &call.arguments.unresolved[0];
    assert_eq!(unresolved.pointer, "");
    let at = input
        .iter()
        .position(|&byte| byte == 0xFF)
        .expect("bad byte");
    assert_eq!(
        unresolved.fault,
        ArgumentFault::InvalidUtf8InArgument { at }
    );
    // The raw span is the whole slot body, from the `:` to the closing `}`.
    assert_eq!(&input[unresolved.raw.clone()], b" {\"path\": \"\xFF\xFE\"}");
}

#[test]
fn codecs_honor_exhaustion() {
    let input = qwen("<parameter=options>{\"mode\": \"x\"}</parameter>");
    let tools = tools();
    let set = set(QWEN);
    let mut ordinary = Governor::new(Budget::default());
    run(&set, &tools, &[input.as_bytes()], &mut ordinary).expect("recognized");
    let work = ordinary.usage().work;
    let mut starved = Governor::new(Budget {
        work: work - 1,
        ..Budget::default()
    });
    assert_eq!(
        run(&set, &tools, &[input.as_bytes()], &mut starved).unwrap_err(),
        StopReason::BudgetExceeded(Resource::Work)
    );
}

/// Every codec fixture of this file, with its dialect.
fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    let mut fixtures: Vec<(&'static str, Vec<u8>)> = vec![
        (QWEN, b"<tool_call>\n<function=configure>\n<parameter=config>\n<parameter=depth>\n3\n</parameter>\n<parameter=name>\nx y\n</parameter>\n</parameter>\n<parameter=label>\nhi\n</parameter>\n</function>\n</tool_call>".to_vec()),
        (QWEN, qwen("<parameter=content>\n\nline\n\n</parameter>").into_bytes()),
        (QWEN, qwen("<parameter=count>\n-0\n</parameter>\n<parameter=ratio>1.50</parameter><parameter=options>{\"mode\": 'x',}</parameter>").into_bytes()),
        (QWEN, qwen("<parameter=path>\na.rs\n<parameter=content>\nbody\n</parameter>").into_bytes()),
        (QWEN, qwen("<parameter=options>{\"mode\": \"x\"\n<parameter=count>2</parameter>").into_bytes()),
        (QWEN, qwen("<parameter=content>see <parameter=nope>x</parameter>").into_bytes()),
        (QWEN, b"<tool_call><function=write>\n{\"path\": \"a\"}\n</parameter>\n</function></tool_call>".to_vec()),
        (QWEN, b"<tool_call><function=write>{\"path\": \"a\"} trailing</function></tool_call>".to_vec()),
        (QWEN, qwen("<parameter=path>a<parameter=path>b</parameter>").into_bytes()),
        (QWEN, b"<tool_call><function=write><parameter=path>\na\xFFb\n</parameter></function></tool_call>".to_vec()),
        (QWEN, b"<tool_call><function=mystery><parameter=a>1<parameter=b>2</parameter></function></tool_call>".to_vec()),
        (HERMES, br#"<tool_call>{"name": "configure", "arguments": {"config": {"depth": 3}, "label": "hi"}}</tool_call>"#.to_vec()),
        (HERMES, r#"<tool_call>{"name": "write", "arguments": "{\"path\": \"aé.rs\", \"count\": 2,}"}</tool_call>"#.as_bytes().to_vec()),
        (HERMES, br#"<tool_call>{"name": "write", "arguments": "{\"path\": [1}"}</tool_call>"#.to_vec()),
        (HERMES, b"<tool_call>{\"name\": \"write\", \"arguments\": {\"path\": \"\xFF\"}}</tool_call>".to_vec()),
    ];
    fixtures.push((
        HERMES,
        "<tool_call>{\"name\": \"write\", \"arguments\": {\"path\": \"é\" /* c */}}</tool_call>"
            .as_bytes()
            .to_vec(),
    ));
    fixtures
}

#[test]
fn codec_every_split_equals_batch() {
    let tools = tools();
    for (grammar, input) in fixtures() {
        let set = set(grammar);
        let mut whole = Governor::new(Budget::default());
        let batch = run(&set, &tools, &[&input], &mut whole).expect("recognized");
        assert!(
            !batch.calls().is_empty() || rejected(&batch).is_some(),
            "{}",
            String::from_utf8_lossy(&input)
        );
        for split in 0..=input.len() {
            let mut governor = Governor::new(Budget::default());
            let chunks = [&input[..split], &input[split..]];
            let turn = run(&set, &tools, &chunks, &mut governor).expect("recognized");
            assert_eq!(turn, batch, "IR at split {split}");
            assert_eq!(governor.usage(), whole.usage(), "usage at split {split}");
        }
        // Byte-at-a-time pushes as well.
        let mut governor = Governor::new(Budget::default());
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        let turn = run(&set, &tools, &chunks, &mut governor).expect("recognized");
        assert_eq!(turn, batch);
        assert_eq!(governor.usage(), whole.usage());
    }
}
