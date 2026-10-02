// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Tool-call rule goldens (A3, A3a–A3q, A13): each fixture under
//! `tests/data/tool-call-goldens/` pairs a model turn (`input`) with its
//! tool declarations (`tools_openai`, the exact `OpenAI` `tools` text or
//! value, or `tools`, a builder specification for aliases, repeatable
//! parameters and unknown-parameter modes adapters cannot express), an
//! optional custom `grammar`, and the request (`dialects`, `policy`,
//! `effects`, `stop`, `tool_choice`, `effort`, `reasoning`, `ids`,
//! `ambiguity`). The outcome, finish, calls, rejected attempts, calls
//! embedded in reasoning, failed validations and the recorded rule ids are
//! captured from an actual run rather than
//! hand-transcribed.
//!
//! Every fixture also runs under the default allow-list: every built-in
//! dialect (plus the fixture's custom `grammar`, when it declares one), the
//! allow-list `ToolRecoveryRequest::default` and the CLI use. Its result must
//! equal the pinned run's captured fields, unless the fixture states its own
//! `default_expect` (the captured fields of the default run). A fixture whose
//! default run is `Ambiguous`, or loses a call its pinned run recovered,
//! must justify it in `default_justification`.
//!
//! `PURRASSIST_BLESS=1` re-captures every fixture's captured fields from a
//! fresh run (refused when `CI` is set, since blessing must never run
//! unattended, and followed by a deliberate failure asking for a re-run
//! without it); without it, every fixture's captured fields must still
//! match. The named tests additionally assert each required outcome
//! explicitly, so a blessed golden can never weaken one.
use purrassist_core::contract::InferenceKind;
use purrassist_core::grammar::Grammar;
use purrassist_core::grammar::edit::LossKind;
use purrassist_core::json::{Member, Node, Value, parse};
use purrassist_core::tool_call::rules::RULES;
use purrassist_core::tool_call::{
    AmbiguityPolicy, CallOutcome, DeclarationForm, DialectId, DialectSet, Effects, Finish,
    GenerationStop, IdPolicy, ObjectSpec, Param, ParameterSpec, PolicySelection, ReasoningPolicy,
    RepairPolicy, ToolCall, ToolCalls, ToolChoice, ToolDefinition, ToolRecoveryRequest, ToolSet,
    UnknownParameters, UnresolvedReason,
};
use purrassist_core::{AssistResult, Budget, Effort, Governor, Outcome, StopReason};
use std::path::{Path, PathBuf};

fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tool-call-goldens")
}

fn golden_files() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(goldens_dir())
        .expect("goldens directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
}

/// The members a run captures; every other member is fixture input.
const CAPTURED: &[&str] = &[
    "outcome",
    "stopped",
    "finish",
    "calls",
    "rejected",
    "embedded",
    "readings",
    "failed_validations",
    "ledger_rules",
];

/// The member holding the default allow-list run's captured fields when
/// they differ from the pinned run's.
const DEFAULT_EXPECT: &str = "default_expect";

/// The member justifying a default allow-list run that is `Ambiguous` or
/// loses a call the pinned run recovered.
const DEFAULT_JUSTIFICATION: &str = "default_justification";

/// One loaded fixture with its tool set and requests built.
struct Golden {
    path: PathBuf,
    node: Node,
    tools: ToolSet,
    request: ToolRecoveryRequest,
    /// The same request under the default allow-list; `None` when the
    /// fixture pins no dialect list (the pinned run is the default run).
    default_request: Option<ToolRecoveryRequest>,
    input: Vec<u8>,
}

fn text_of<'n>(golden: &'n Node, key: &str, path: &Path) -> Option<&'n str> {
    golden.get(key).map(|node| {
        node.as_str()
            .unwrap_or_else(|| panic!("{}: `{key}` must be a string", path.display()))
    })
}

fn items(node: &Node) -> &[Node] {
    match &node.value {
        Value::Array(items) => items,
        other => panic!("expected an array, got {other:?}"),
    }
}

fn members(node: &Node) -> &[Member] {
    match &node.value {
        Value::Object(members) => members,
        other => panic!("expected an object, got {other:?}"),
    }
}

fn flag(node: &Node, key: &str) -> bool {
    node.get(key)
        .is_some_and(|value| matches!(value.value, Value::Bool(true)))
}

fn strings_of(node: Option<&Node>) -> Vec<String> {
    node.map(|node| {
        items(node)
            .iter()
            .map(|item| item.as_str().expect("string item").to_owned())
            .collect()
    })
    .unwrap_or_default()
}

fn effects_of(code: &str) -> Effects {
    match code {
        "read_only" => Effects::ReadOnly,
        "mutating" => Effects::Mutating,
        other => panic!("unknown effects `{other}`"),
    }
}

fn policy_of(code: &str) -> RepairPolicy {
    match code {
        "structural" => RepairPolicy::Structural,
        "tolerant" => RepairPolicy::Tolerant,
        other => panic!("unknown policy `{other}`"),
    }
}

fn unknown_of(code: &str) -> UnknownParameters {
    match code {
        "error" => UnknownParameters::Error,
        "drop" => UnknownParameters::DropWithLoss,
        "pass" => UnknownParameters::PassThrough,
        other => panic!("unknown unknown-parameters mode `{other}`"),
    }
}

/// A builder parameter: `{type, required?, aliases?, repeatable?, values?
/// (enum), items? (array), params? and unknown? (object)}`.
fn parameter(node: &Node) -> ParameterSpec {
    let kind = node.get("type").and_then(Node::as_str).expect("param type");
    let mut spec = match kind {
        "string" => Param::string(),
        "integer" => Param::integer(),
        "number" => Param::number(),
        "boolean" => Param::boolean(),
        "enum" => {
            let values = strings_of(node.get("values"));
            let values: Vec<&str> = values.iter().map(String::as_str).collect();
            Param::enumeration(&values)
        }
        "array" => Param::array(parameter(node.get("items").expect("array items"))),
        "object" => {
            let mut object = ObjectSpec::new();
            for member in node.get("params").map(members).unwrap_or_default() {
                object = object.param(member.key.clone(), parameter(&member.value));
            }
            if let Some(mode) = node.get("unknown").and_then(Node::as_str) {
                object = object.unknown_parameters(unknown_of(mode));
            }
            Param::object(object)
        }
        other => panic!("unknown param type `{other}`"),
    };
    if flag(node, "required") {
        spec = spec.required();
    }
    if flag(node, "repeatable") {
        spec = spec.repeatable();
    }
    for alias in strings_of(node.get("aliases")) {
        spec = spec.alias(alias);
    }
    spec
}

/// A builder tool: `{name, effects?, policy?, aliases?, unknown?, params?}`.
fn definition(node: &Node) -> ToolDefinition {
    let name = node.get("name").and_then(Node::as_str).expect("tool name");
    let mut tool = ToolDefinition::new(name);
    if let Some(effects) = node.get("effects").and_then(Node::as_str) {
        tool = tool.effects(effects_of(effects));
    }
    if let Some(policy) = node.get("policy").and_then(Node::as_str) {
        tool = tool.policy(policy_of(policy));
    }
    if let Some(mode) = node.get("unknown").and_then(Node::as_str) {
        tool = tool.unknown_parameters(unknown_of(mode));
    }
    for alias in strings_of(node.get("aliases")) {
        tool = tool.alias(alias);
    }
    for member in node.get("params").map(members).unwrap_or_default() {
        tool = tool.param(member.key.clone(), parameter(&member.value));
    }
    tool
}

/// The exact `OpenAI` tools text: a string member verbatim, else the
/// value's compact JSON.
fn openai_text(golden: &Node) -> Option<String> {
    golden
        .get("tools_openai")
        .map(|node| node.as_str().map_or_else(|| node.to_json(), str::to_owned))
}

fn tool_set(golden: &Node, path: &Path) -> ToolSet {
    match (openai_text(golden), golden.get("tools")) {
        (Some(text), None) => ToolSet::from_openai(&text, Budget::default())
            .unwrap_or_else(|error| panic!("{}: tools_openai: {error}", path.display())),
        (None, Some(tools)) => {
            ToolSet::compile(items(tools).iter().map(definition), Budget::default())
                .unwrap_or_else(|error| panic!("{}: tools: {error}", path.display()))
        }
        _ => panic!(
            "{}: exactly one of `tools_openai` and `tools` is required",
            path.display()
        ),
    }
}

fn stop_of(node: &Node) -> GenerationStop {
    if let Some(sequence) = node.get("sequence").and_then(Node::as_str) {
        return GenerationStop::StopSequence(sequence.as_bytes().to_vec());
    }
    match node.as_str().expect("stop code") {
        "stop" => GenerationStop::Stop,
        "length" => GenerationStop::Length,
        "tool_calls" => GenerationStop::ToolCalls,
        "content_filter" => GenerationStop::ContentFilter,
        "unknown" => GenerationStop::Unknown,
        other => panic!("unknown stop `{other}`"),
    }
}

fn effort_of(code: &str) -> Effort {
    match code {
        "exact" => Effort::Exact,
        "quick" => Effort::Quick,
        "standard" => Effort::Standard,
        "deep" => Effort::Deep,
        "maximum" => Effort::Maximum,
        other => panic!("unknown effort `{other}`"),
    }
}

fn custom_dialect(golden: &Node, path: &Path) -> Option<DialectId> {
    text_of(golden, "grammar", path).map(|source| {
        DialectId::custom(
            Grammar::compile_governed(source, &mut Governor::new(Budget::default()))
                .unwrap_or_else(|error| panic!("{}: grammar: {error}", path.display())),
        )
    })
}

/// The default allow-list: every built-in dialect, plus the fixture's
/// custom dialect when it declares a `grammar`.
fn default_dialect_set(golden: &Node, path: &Path) -> DialectSet {
    let mut ids = DialectSet::built_in().dialects().to_vec();
    ids.extend(custom_dialect(golden, path));
    DialectSet::new(ids, &mut Governor::new(Budget::default()))
        .unwrap_or_else(|error| panic!("{}: default dialects: {error}", path.display()))
}

fn dialect_set(golden: &Node, path: &Path) -> DialectSet {
    let custom = custom_dialect(golden, path);
    let ids: Vec<DialectId> = match golden.get("dialects") {
        None => return DialectSet::built_in(),
        Some(dialects) => items(dialects)
            .iter()
            .map(|id| match id.as_str().expect("dialect id") {
                "custom" => custom.clone().expect("a `grammar` for the custom dialect"),
                id => DialectId::from_id(id).unwrap_or_else(|| panic!("dialect `{id}`")),
            })
            .collect(),
    };
    DialectSet::new(ids, &mut Governor::new(Budget::default()))
        .unwrap_or_else(|error| panic!("{}: dialects: {error}", path.display()))
}

fn input_bytes(golden: &Node, path: &Path) -> Vec<u8> {
    let input = golden
        .get("input")
        .unwrap_or_else(|| panic!("{}: missing `input`", path.display()));
    if let Some(text) = input.as_str() {
        return text.as_bytes().to_vec();
    }
    let hex = input
        .get("hex")
        .and_then(Node::as_str)
        .unwrap_or_else(|| panic!("{}: `input` is a string or {{hex}}", path.display()));
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex byte"))
        .collect()
}

fn request(
    golden: &Node,
    path: &Path,
    input_len: usize,
    dialects: DialectSet,
) -> ToolRecoveryRequest {
    let effort = golden
        .get("effort")
        .and_then(Node::as_str)
        .map_or(Effort::Standard, effort_of);
    let mut request = ToolRecoveryRequest::sized(effort, input_len).dialects(dialects);
    if let Some(policy) = golden.get("policy").and_then(Node::as_str) {
        request = request.policy(match policy {
            "per_tool" => PolicySelection::PerTool,
            policy => PolicySelection::All(policy_of(policy)),
        });
    }
    for member in golden.get("effects").map(members).unwrap_or_default() {
        request = request.effects(
            member.key.clone(),
            effects_of(member.value.as_str().expect("effects code")),
        );
    }
    if let Some(stop) = golden.get("stop") {
        request = request.stop(stop_of(stop));
    }
    if let Some(choice) = golden.get("tool_choice") {
        request = request.tool_choice(
            ToolChoice::from_openai(choice)
                .unwrap_or_else(|error| panic!("{}: tool_choice: {error:?}", path.display())),
        );
    }
    if let Some(reasoning) = golden.get("reasoning").and_then(Node::as_str) {
        request = request.reasoning(match reasoning {
            "separate" => ReasoningPolicy::Separate { starts_open: false },
            "starts_open" => ReasoningPolicy::Separate { starts_open: true },
            "as_content" => ReasoningPolicy::AsContent,
            other => panic!("unknown reasoning `{other}`"),
        });
    }
    if let Some(ids) = golden.get("ids").and_then(Node::as_str) {
        request = request.ids(match ids {
            "source" => IdPolicy::Source,
            other => panic!("unknown id policy `{other}`"),
        });
    }
    if let Some(ambiguity) = golden.get("ambiguity").and_then(Node::as_str) {
        request = request.ambiguity(match ambiguity {
            "report" => AmbiguityPolicy::Report,
            "reject" => AmbiguityPolicy::Reject,
            other => panic!("unknown ambiguity `{other}`"),
        });
    }
    request
}

fn load(path: &Path) -> Golden {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let node = parse(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let input = input_bytes(&node, path);
    let default_request = node
        .get("dialects")
        .map(|_| request(&node, path, input.len(), default_dialect_set(&node, path)));
    Golden {
        path: path.to_owned(),
        tools: tool_set(&node, path),
        request: request(&node, path, input.len(), dialect_set(&node, path)),
        default_request,
        input,
        node,
    }
}

impl Golden {
    fn run(&self) -> AssistResult<ToolCalls> {
        self.tools.recover(&self.input, &self.request)
    }

    /// The run under the default allow-list, when it differs from the
    /// pinned request.
    fn run_default(&self) -> Option<AssistResult<ToolCalls>> {
        self.default_request
            .as_ref()
            .map(|request| self.tools.recover(&self.input, request))
    }
}

/// Run the named golden.
fn golden(name: &str) -> (Golden, AssistResult<ToolCalls>) {
    let golden = load(&goldens_dir().join(format!("{name}.json")));
    let result = golden.run();
    (golden, result)
}

/// The single selected reading of a `Resolved` or `BestGuess` outcome.
fn reading(result: &AssistResult<ToolCalls>) -> &ToolCalls {
    match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
        other => panic!("expected one reading, got {other:?}"),
    }
}

fn the_call(result: &AssistResult<ToolCalls>) -> &ToolCall {
    let calls = reading(result);
    assert_eq!(calls.calls.len(), 1, "{:#?}", calls.calls);
    &calls.calls[0]
}

fn member(key: &str, value: Node) -> Member {
    Member {
        key: key.into(),
        key_span: None,
        value,
    }
}

fn string(text: impl Into<String>) -> Node {
    Node::new(Value::String(text.into()))
}

fn strings(values: &[String]) -> Node {
    Node::new(Value::Array(values.iter().cloned().map(string).collect()))
}

const fn object(fields: Vec<Member>) -> Node {
    Node::new(Value::Object(fields))
}

fn integer(value: usize) -> Node {
    parse(&value.to_string()).expect("integer")
}

const fn outcome_kind(outcome: &Outcome<ToolCalls>) -> &'static str {
    match outcome {
        Outcome::Resolved(_) => "resolved",
        Outcome::BestGuess(_) => "best_guess",
        Outcome::Ambiguous(_) => "ambiguous",
        Outcome::Unresolved => "unresolved",
    }
}

fn call_outcome(outcome: &CallOutcome) -> String {
    match outcome {
        CallOutcome::Resolved => "resolved".into(),
        CallOutcome::BestGuess => "best_guess".into(),
        CallOutcome::Ambiguous => "ambiguous".into(),
        CallOutcome::Unresolved { reason } => format!("unresolved: {}: {reason}", reason.code()),
    }
}

fn call_node(call: &ToolCall) -> Node {
    let rules: Vec<String> = call
        .repairs
        .evidence
        .iter()
        .map(|evidence| evidence.rule.clone())
        .collect();
    object(vec![
        member(
            "tool",
            call.tool
                .as_deref()
                .map_or_else(|| Node::new(Value::Null), string),
        ),
        member("arguments", call.arguments.clone()),
        member(
            "candidate_dialects",
            Node::new(Value::Array(
                call.candidate_dialects
                    .iter()
                    .map(|dialect| string(dialect.id()))
                    .collect(),
            )),
        ),
        member("outcome", string(call_outcome(&call.outcome))),
        member("rules", strings(&rules)),
    ])
}

fn rejected_nodes(calls: &ToolCalls) -> Node {
    Node::new(Value::Array(
        calls
            .rejected
            .iter()
            .map(|rejected| {
                object(vec![
                    member(
                        "span",
                        Node::new(Value::Array(vec![
                            integer(rejected.span.start),
                            integer(rejected.span.end),
                        ])),
                    ),
                    member("dialect", string(rejected.dialect.id())),
                    member(
                        "candidate_dialects",
                        Node::new(Value::Array(
                            rejected
                                .candidate_dialects
                                .iter()
                                .map(|dialect| string(dialect.id()))
                                .collect(),
                        )),
                    ),
                    member("reason", string(format!("{:?}", rejected.reason))),
                    member(
                        "raw",
                        string(
                            rejected
                                .text()
                                .map_or_else(|| format!("{:?}", rejected.raw), str::to_owned),
                        ),
                    ),
                ])
            })
            .collect(),
    ))
}

fn reading_fields(calls: &ToolCalls) -> Vec<Member> {
    vec![
        member("finish", string(calls.finish.code())),
        member(
            "calls",
            Node::new(Value::Array(calls.calls.iter().map(call_node).collect())),
        ),
        member("rejected", rejected_nodes(calls)),
        member(
            "embedded",
            Node::new(Value::Array(calls.embedded.iter().map(call_node).collect())),
        ),
    ]
}

/// Every rule id recorded, in order: the receipt's evidence (the selected
/// reading's, then each whole-message alternative's), then the repairs of
/// every call offered only as an alternative from inside reasoning
/// ([`ToolCalls::embedded`], itemized on those calls, not in the receipt).
fn ledger_rules(result: &AssistResult<ToolCalls>) -> Vec<String> {
    let readings: Vec<&ToolCalls> = match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => vec![calls],
        Outcome::Ambiguous(readings) => readings.iter().collect(),
        Outcome::Unresolved => Vec::new(),
    };
    result
        .receipt
        .evidence
        .iter()
        .chain(
            result
                .receipt
                .alternatives
                .iter()
                .flat_map(|alternative| alternative.evidence.iter()),
        )
        .chain(
            readings
                .into_iter()
                .flat_map(|calls| calls.embedded.iter())
                .flat_map(|call| call.repairs.evidence.iter()),
        )
        .map(|evidence| evidence.rule.clone())
        .collect()
}

fn failed_validations(result: &AssistResult<ToolCalls>) -> Vec<String> {
    result
        .receipt
        .validations
        .iter()
        .filter(|validation| !validation.passed)
        .map(|validation| validation.constraint.clone())
        .collect()
}

fn captured_fields(result: &AssistResult<ToolCalls>) -> Vec<Member> {
    let mut fields = vec![
        member("outcome", string(outcome_kind(&result.outcome))),
        member("stopped", string(result.receipt.stopped.to_string())),
    ];
    match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => {
            fields.extend(reading_fields(calls));
        }
        Outcome::Ambiguous(readings) => fields.push(member(
            "readings",
            Node::new(Value::Array(
                readings
                    .iter()
                    .map(|calls| object(reading_fields(calls)))
                    .collect(),
            )),
        )),
        Outcome::Unresolved => {}
    }
    fields.push(member(
        "failed_validations",
        strings(&failed_validations(result)),
    ));
    fields.push(member("ledger_rules", strings(&ledger_rules(result))));
    fields
}

/// Replace every captured field in `golden`, keeping every input member in
/// its original order; the default allow-list run's captured fields are
/// kept as `default_expect` only when they differ from the pinned run's.
fn reblessed(
    golden: &Node,
    result: &AssistResult<ToolCalls>,
    default: Option<&AssistResult<ToolCalls>>,
) -> Node {
    let mut kept: Vec<Member> = members(golden)
        .iter()
        .filter(|member| !CAPTURED.contains(&member.key.as_str()) && member.key != DEFAULT_EXPECT)
        .cloned()
        .collect();
    let pinned = object(captured_fields(result));
    if let Some(default) = default {
        let default = object(captured_fields(default));
        if !default.equivalent(&pinned) {
            kept.push(member(DEFAULT_EXPECT, default));
        }
    }
    kept.extend(members(&pinned).iter().cloned());
    object(kept)
}

/// The mismatches between `actual` captured fields and `expected`.
fn mismatches(path: &Path, label: &str, expected: &Node, actual: &Node) -> Vec<String> {
    let mut failures = Vec::new();
    for field in members(actual) {
        let wanted = expected.get(&field.key);
        if !wanted.is_some_and(|wanted| wanted.equivalent(&field.value)) {
            failures.push(format!(
                "{}: {label} `{}` mismatch\n  expected: {}\n  actual:   {}",
                path.display(),
                field.key,
                wanted.map_or_else(|| "(absent)".into(), Node::to_json),
                field.value.to_json()
            ));
        }
    }
    for key in CAPTURED {
        if expected.get(key).is_some() && actual.get(key).is_none() {
            failures.push(format!(
                "{}: {label} stale captured `{key}`",
                path.display()
            ));
        }
    }
    failures
}

fn pretty(node: &Node) -> String {
    let mut output = String::new();
    write_pretty(node, 0, &mut output);
    output.push('\n');
    output
}

fn write_pretty(node: &Node, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    let inner_pad = "  ".repeat(indent + 1);
    match &node.value {
        Value::Object(members) if members.is_empty() => out.push_str("{}"),
        Value::Object(members) => {
            out.push_str("{\n");
            for (index, member) in members.iter().enumerate() {
                out.push_str(&inner_pad);
                out.push_str(&string(member.key.clone()).to_json());
                out.push_str(": ");
                write_pretty(&member.value, indent + 1, out);
                if index + 1 < members.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push('}');
        }
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                out.push_str(&inner_pad);
                write_pretty(item, indent + 1, out);
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push(']');
        }
        _ => out.push_str(&node.to_json()),
    }
}

fn all_goldens() -> Vec<(Golden, AssistResult<ToolCalls>)> {
    golden_files()
        .iter()
        .map(|path| {
            let golden = load(path);
            let result = golden.run();
            (golden, result)
        })
        .collect()
}

/// Every golden's run under the default allow-list (the pinned run when
/// the golden pins no dialect list).
fn all_default_runs() -> Vec<(Golden, AssistResult<ToolCalls>)> {
    golden_files()
        .iter()
        .map(|path| {
            let golden = load(path);
            let result = golden.run_default().unwrap_or_else(|| golden.run());
            (golden, result)
        })
        .collect()
}

#[test]
fn tool_call_goldens_match_captured_runs() {
    let bless = std::env::var("PURRASSIST_BLESS").as_deref() == Ok("1");
    assert!(
        !(bless && std::env::var("CI").is_ok()),
        "PURRASSIST_BLESS=1 must never run with CI set"
    );
    let goldens = all_goldens();
    assert!(!goldens.is_empty(), "no tool-call goldens found");
    let mut failures = Vec::new();
    for (golden, result) in &goldens {
        let path = &golden.path;
        let default = golden.run_default();
        if bless {
            std::fs::write(
                path,
                pretty(&reblessed(&golden.node, result, default.as_ref())),
            )
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            continue;
        }
        if golden.node.get("outcome").is_none() {
            failures.push(format!(
                "{}: no captured fields; run with PURRASSIST_BLESS=1",
                path.display()
            ));
            continue;
        }
        let actual = object(captured_fields(result));
        failures.extend(mismatches(path, "pinned", &golden.node, &actual));
        match (&default, golden.node.get(DEFAULT_EXPECT)) {
            (Some(default), Some(expected)) => {
                let actual = object(captured_fields(default));
                if actual.equivalent(&object(captured_fields(result))) {
                    failures.push(format!(
                        "{}: `{DEFAULT_EXPECT}` restates the pinned result; drop it",
                        path.display()
                    ));
                }
                failures.extend(mismatches(path, "default allow-list", expected, &actual));
            }
            (Some(default), None) => {
                let actual = object(captured_fields(default));
                failures.extend(mismatches(
                    path,
                    "default allow-list",
                    &golden.node,
                    &actual,
                ));
            }
            (None, Some(_)) => failures.push(format!(
                "{}: `{DEFAULT_EXPECT}` without a pinned `dialects` list",
                path.display()
            )),
            (None, None) => {}
        }
    }
    assert!(
        !bless,
        "{} golden(s) (re)blessed under tests/data/tool-call-goldens/; re-run without PURRASSIST_BLESS to verify",
        goldens.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// Rules no golden can reach through the shipped dialects, each with why.
const UNREACHABLE: &[(&str, &str)] = &[];

#[test]
fn every_rule_has_a_golden() {
    let recorded: Vec<String> = all_goldens()
        .iter()
        .flat_map(|(_, result)| ledger_rules(result))
        .collect();
    let mut missing = Vec::new();
    for rule in RULES {
        let exempt = UNREACHABLE.iter().any(|(id, _)| *id == rule.id);
        let covered = recorded.iter().any(|id| id == rule.id);
        assert!(
            !(exempt && covered),
            "`{}` is exempt but a golden records it; drop the exemption",
            rule.id
        );
        if !exempt && !covered {
            missing.push(rule.id);
        }
    }
    assert!(missing.is_empty(), "rules without a golden: {missing:?}");
}

fn has_guess(call: &ToolCall) -> bool {
    call.repairs
        .inferences
        .iter()
        .any(|inference| inference.kind == InferenceKind::Guess)
}

fn has_data_loss(call: &ToolCall) -> bool {
    call.repairs
        .losses
        .iter()
        .any(|loss| loss.kind == LossKind::Data)
}

/// A13 over every golden: no call on a `Mutating` tool whose ledger holds a
/// guess is `Resolved`, no call with a data loss is `Resolved`, and neither
/// usable selection ever holds a `Mutating` guess.
#[test]
fn no_mutating_guess_resolved_over_goldens() {
    let mut checked = 0;
    for (golden, result) in all_goldens().into_iter().chain(all_default_runs()) {
        let readings: Vec<&ToolCalls> = match &result.outcome {
            Outcome::Resolved(calls) | Outcome::BestGuess(calls) => vec![calls],
            Outcome::Ambiguous(readings) => readings.iter().collect(),
            Outcome::Unresolved => Vec::new(),
        };
        for calls in readings {
            for call in &calls.calls {
                checked += 1;
                let path = golden.path.display();
                if call.effects == Some(Effects::Mutating) && has_guess(call) {
                    assert_ne!(call.outcome, CallOutcome::Resolved, "{path}: {call:#?}");
                }
                if has_data_loss(call) {
                    assert_ne!(call.outcome, CallOutcome::Resolved, "{path}: {call:#?}");
                }
            }
            for embedded in &calls.embedded {
                checked += 1;
                assert_ne!(
                    embedded.outcome,
                    CallOutcome::Resolved,
                    "{}: an embedded call is resolved: {embedded:#?}",
                    golden.path.display()
                );
            }
            for usable in calls
                .usable()
                .into_iter()
                .chain(calls.usable_accepting_guesses())
            {
                assert!(
                    !(usable.effects == Some(Effects::Mutating) && has_guess(usable)),
                    "{}: a mutating guess is usable: {usable:#?}",
                    golden.path.display()
                );
                assert!(!has_data_loss(usable) || usable.outcome != CallOutcome::Resolved);
            }
        }
    }
    assert!(checked > 0);
}

/// The calls a run makes usable when guesses are accepted, as
/// `(tool, arguments)` pairs.
fn usable_calls(result: &AssistResult<ToolCalls>) -> Vec<(Option<String>, String)> {
    match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls
            .usable_accepting_guesses()
            .into_iter()
            .map(|call| (call.tool.clone(), call.arguments.to_json()))
            .collect(),
        Outcome::Ambiguous(_) | Outcome::Unresolved => Vec::new(),
    }
}

/// Under the default allow-list (every built-in dialect), no golden loses
/// a call to ambiguity: a call its pinned run makes usable stays usable,
/// and the default run is `Ambiguous` only when the pinned run is too.
/// A golden whose readings are genuinely different calls must say why in
/// `default_justification`.
#[test]
fn default_allow_list_never_loses_a_call() {
    let mut compared = 0;
    for path in golden_files() {
        let golden = load(&path);
        let Some(default) = golden.run_default() else {
            continue;
        };
        compared += 1;
        if golden.node.get(DEFAULT_JUSTIFICATION).is_some() {
            continue;
        }
        let pinned = golden.run();
        assert!(
            !matches!(default.outcome, Outcome::Ambiguous(_))
                || matches!(pinned.outcome, Outcome::Ambiguous(_)),
            "{}: ambiguous only under the default allow-list: {:#?}",
            path.display(),
            default.outcome
        );
        let kept = usable_calls(&default);
        for call in usable_calls(&pinned) {
            assert!(
                kept.contains(&call),
                "{}: the default allow-list loses {call:?}",
                path.display()
            );
        }
    }
    assert!(compared > 0);
}

/// An ambiguity always carries the evidence that tells its readings apart:
/// no `Ambiguous` operation publishes a ranked-adjacent pair with an empty
/// `distinguishing` list, and no `Ambiguous` call has an empty
/// `distinguishing` list, under the pinned or the default allow-list.
#[test]
fn ambiguity_always_has_distinguishing_evidence() {
    let mut ambiguous = 0;
    for (golden, result) in all_goldens().into_iter().chain(all_default_runs()) {
        let path = golden.path.display();
        let wire = result.to_node();
        if let Some(pairs) = wire
            .pointer("/receipt/ambiguity/distinguishing")
            .filter(|node| !matches!(node.value, Value::Null))
        {
            ambiguous += 1;
            for pair in items(pairs) {
                assert!(
                    !items(pair).is_empty(),
                    "{path}: an ambiguity without distinguishing evidence: {}",
                    wire.to_json()
                );
            }
        }
        let readings: Vec<&ToolCalls> = match &result.outcome {
            Outcome::Resolved(calls) | Outcome::BestGuess(calls) => vec![calls],
            Outcome::Ambiguous(readings) => readings.iter().collect(),
            Outcome::Unresolved => Vec::new(),
        };
        for call in readings.into_iter().flat_map(|calls| &calls.calls) {
            if call.outcome == CallOutcome::Ambiguous {
                ambiguous += 1;
                assert!(
                    !call.distinguishing.is_empty(),
                    "{path}: an ambiguous call without distinguishing evidence: {call:#?}"
                );
            }
        }
    }
    assert!(ambiguous > 0, "no golden exercises an ambiguity");
}

// ---------------------------------------------------------------------------
// Named goldens: each asserts its required outcome explicitly.

fn rules_of(call: &ToolCall) -> Vec<&str> {
    call.repairs
        .evidence
        .iter()
        .map(|evidence| evidence.rule.as_str())
        .collect()
}

fn loss_kinds(call: &ToolCall, rule: &str) -> Vec<LossKind> {
    call.repairs
        .losses
        .iter()
        .filter(|loss| loss.loss.evidence.rule == rule)
        .map(|loss| loss.kind)
        .collect()
}

const fn is_unresolved(outcome: &CallOutcome) -> bool {
    matches!(outcome, CallOutcome::Unresolved { .. })
}

fn assert_mutating_structural(call: &ToolCall) {
    assert_eq!(
        (call.effects, call.policy),
        (Some(Effects::Mutating), Some(RepairPolicy::Structural)),
        "{call:#?}"
    );
}

#[test]
fn closers_only_no_required() {
    let (golden, result) = golden("closers_only_no_required");
    assert_eq!(
        golden.input,
        b"<tool_call>\n<function=f>\n</parameters>\n</parameters>\n</function>\n</tool_call>"
    );
    assert_eq!(outcome_kind(&result.outcome), "resolved");
    let call = the_call(&result);
    assert_mutating_structural(call);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.tool.as_deref(), Some("f"));
    assert_eq!(call.arguments.to_json(), "{}");
    assert_eq!(
        loss_kinds(call, "tool_call.markup.closers-only-empty"),
        [LossKind::Markup]
    );
}

#[test]
fn closers_only_with_required() {
    let (_, result) = golden("closers_only_with_required");
    let call = &reading(&result).calls[0];
    assert!(
        matches!(
            call.outcome,
            CallOutcome::Unresolved {
                reason: UnresolvedReason::MissingRequired { .. }
            }
        ),
        "{call:#?}"
    );
    assert_eq!(reading(&result).usable().len(), 0);
}

#[test]
fn hallucinated_name() {
    for name in ["hallucinated_name", "hallucinated_name_tolerant"] {
        let (golden, result) = golden(name);
        assert!(golden.input.starts_with(b"<tool_call>"), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), 1, "{name}");
        assert!(
            matches!(
                calls.calls[0].outcome,
                CallOutcome::Unresolved {
                    reason: UnresolvedReason::UnknownTool { .. }
                }
            ),
            "{name}: {:#?}",
            calls.calls[0]
        );
        assert!(calls.usable_accepting_guesses().is_empty(), "{name}");
        assert_eq!(calls.finish, Finish::Unresolved, "{name}");
    }
}

#[test]
fn near_miss_name_tolerant() {
    let (_, result) = golden("near_miss_name_tolerant");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::BestGuess, "{call:#?}");
    assert!(rules_of(call).contains(&"tool_call.name.match"));
}

#[test]
fn near_miss_name_structural() {
    let (_, result) = golden("near_miss_name_structural");
    let call = the_call(&result);
    assert!(is_unresolved(&call.outcome), "{call:#?}");
    assert_eq!(reading(&result).usable_accepting_guesses().len(), 0);
}

#[test]
fn adapter_anyof_ref_verbatim_and_enforced() {
    let (golden, result) = golden("adapter_anyof_ref_verbatim_and_enforced");
    let source = openai_text(&golden.node).expect("tools_openai text");
    let declarations = parse(&source).expect("tools json");
    let declaration = &items(&declarations)[0];
    let span = declaration.span.clone().expect("declaration span");
    let rendered = golden
        .tools
        .render_declaration("pick", DeclarationForm::OpenAi)
        .expect("rendered");
    assert_eq!(rendered.as_bytes(), &source.as_bytes()[span]);
    let call = the_call(&result);
    assert!(
        matches!(
            call.outcome,
            CallOutcome::Unresolved {
                reason: UnresolvedReason::SchemaValidation
            }
        ),
        "{call:#?}"
    );
    assert!(
        result
            .receipt
            .validations
            .iter()
            .any(|validation| !validation.passed
                && validation.validator != "purrassist.tool_call.v1"),
        "{:#?}",
        result.receipt.validations
    );
}

#[test]
fn length_vs_stop_same_text() {
    let (length, truncated) = golden("length_vs_stop_same_text");
    let (stop, malformed) = golden("length_vs_stop_same_text_stop");
    assert_eq!(length.input, stop.input);
    assert_eq!(reading(&truncated).finish, Finish::TruncatedWithCalls);
    let calls = reading(&malformed);
    assert_eq!(calls.finish, Finish::Unresolved);
    assert!(
        calls.rejected.iter().any(|rejected| matches!(
            rejected.reason,
            purrassist_core::tool_call::Rejection::Malformed { .. }
                | purrassist_core::tool_call::Rejection::Unterminated
        )),
        "{calls:#?}"
    );
}

#[test]
fn forced_tool_choice_violated() {
    let (_, result) = golden("forced_tool_choice_violated");
    assert!(
        result
            .receipt
            .validations
            .iter()
            .any(|validation| !validation.passed
                && validation.constraint == "tool_choice=forced:read_file"),
        "{:#?}",
        result.receipt.validations
    );
    assert!(!reading(&result).tool_choice.passed);
}

fn assert_parameter_boundary(name: &str, expected: &str) {
    let (_, result) = golden(name);
    let call = the_call(&result);
    assert_mutating_structural(call);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{name}: {call:#?}");
    assert_eq!(call.arguments.to_json(), expected, "{name}");
    assert!(
        rules_of(call).contains(&"tool_call.markup.missing-parameter-close"),
        "{name}: {call:#?}"
    );
}

#[test]
fn unclosed_json_value_then_next_parameter() {
    assert_parameter_boundary(
        "unclosed_json_value_then_next_parameter",
        r#"{"data":{"a":1},"next":"fast"}"#,
    );
}

#[test]
fn unclosed_string_value_then_next_parameter() {
    assert_parameter_boundary(
        "unclosed_string_value_then_next_parameter",
        r#"{"path":"src/main.rs","next":"fast"}"#,
    );
}

#[test]
fn nested_parameter_object_closed() {
    let (_, result) = golden("nested_parameter_object_closed");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"options":{"depth":2,"name":"x"}}"#
    );
    assert!(rules_of(call).contains(&"tool_call.markup.nested-parameter"));
}

#[test]
fn nested_parameter_object_unclosed() {
    let (_, result) = golden("nested_parameter_object_unclosed");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"options":{"depth":2,"name":"x"}}"#
    );
    assert!(rules_of(call).contains(&"tool_call.markup.nested-parameter"));
    assert!(rules_of(call).contains(&"tool_call.markup.parameter-closed-by-element"));
}

#[test]
fn json_body_one_stray_closer() {
    let (_, result) = golden("json_body_one_stray_closer");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(call.arguments.to_json(), r#"{"path":"a.rs"}"#);
    assert!(rules_of(call).contains(&"tool_call.markup.json-body"));
    assert!(rules_of(call).contains(&"tool_call.markup.stray-closer"));
}

#[test]
fn json_body_stray_closer_trailing_text() {
    let (_, result) = golden("json_body_stray_closer_trailing_text");
    let calls = reading(&result);
    assert!(calls.usable_accepting_guesses().is_empty(), "{calls:#?}");
    assert!(
        calls.calls.iter().all(|call| is_unresolved(&call.outcome)),
        "{calls:#?}"
    );
    assert!(!calls.rejected.is_empty(), "{calls:#?}");
    assert_eq!(calls.finish, Finish::Unresolved);
}

#[test]
fn string_param_12345_stays_string() {
    let (_, result) = golden("string_param_12345_stays_string");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.arguments.to_json(), r#"{"code":"12345"}"#);
}

#[test]
fn verbatim_string_significant_whitespace() {
    let (_, result) = golden("verbatim_string_significant_whitespace");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(
        call.arguments.get("content").and_then(Node::as_str),
        Some("  indented\n\tline\n\n")
    );
}

#[test]
fn repeated_scalar_both_readings() {
    let (_, result) = golden("repeated_scalar_both_readings");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Ambiguous, "{call:#?}");
    let mut readings: Vec<String> = core::iter::once(call.arguments.to_json())
        .chain(
            call.alternatives
                .iter()
                .map(|reading| reading.arguments.to_json()),
        )
        .collect();
    readings.sort();
    readings.dedup();
    assert_eq!(readings, [r#"{"path":"a"}"#, r#"{"path":"b"}"#]);
}

#[test]
fn repeated_array_param_is_array() {
    let (_, result) = golden("repeated_array_param_is_array");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(call.arguments.to_json(), r#"{"tags":["a","b"]}"#);
}

#[test]
fn hermes_and_qwen_xml_mixed() {
    let (_, result) = golden("hermes_and_qwen_xml_mixed");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 2, "{calls:#?}");
    assert_eq!(calls.calls[0].dialect, DialectId::Hermes);
    assert_eq!(calls.calls[1].dialect, DialectId::Qwen3CoderXml);
    assert!(calls.calls.iter().all(ToolCall::is_usable));
    assert_eq!(calls.finish, Finish::Calls);
}

/// G1: a complete Qwen3-Coder XML call and a complete Hermes call stay
/// `Resolved` and usable under the default allow-list when a dangling
/// `<tool_call>` follows them; the dangling attempt is one rejected attempt
/// naming both dialects that share the opener.
#[test]
fn mixed_complete_calls_then_dangling_opener() {
    let (golden, result) = golden("mixed_complete_calls_then_dangling_opener");
    assert!(
        golden.node.get("dialects").is_none(),
        "runs the default allow-list"
    );
    let Outcome::BestGuess(calls) = &result.outcome else {
        panic!("expected one reading, got {:#?}", result.outcome);
    };
    assert_eq!(calls.calls.len(), 2, "{calls:#?}");
    assert_eq!(calls.calls[0].dialect, DialectId::Qwen3CoderXml);
    assert_eq!(calls.calls[1].dialect, DialectId::Hermes);
    assert!(calls.calls.iter().all(ToolCall::is_usable));
    assert_eq!(calls.usable().len(), 2);
    assert_eq!(calls.rejected.len(), 1);
    let rejected = &calls.rejected[0];
    assert_eq!(rejected.raw, b"<tool_call>");
    assert_eq!(
        rejected.candidate_dialects,
        [DialectId::Hermes, DialectId::Qwen3CoderXml]
    );
    assert!(rejected.candidate_dialects.contains(&rejected.dialect));
    assert_eq!(
        rejected
            .repairs
            .losses
            .iter()
            .map(|loss| (loss.loss.evidence.rule.as_str(), loss.kind))
            .collect::<Vec<_>>(),
        [("tool_call.markup.dangling-opener", LossKind::Data)]
    );
    assert_eq!(calls.finish, Finish::Unresolved);
    assert_eq!(result.receipt.alternatives.len(), 0);
}

/// G1: the four goldens that lost their calls to a dialect-label tie under
/// the default allow-list give their pinned result, with the rejected
/// attempt the body evidences and both candidate dialects recorded.
#[test]
fn shared_opener_rejections_collapse_under_the_default_allow_list() {
    for (name, dialect, reach) in [
        (
            "dangling_trailing_tool_call",
            DialectId::Qwen3CoderXml,
            93..104,
        ),
        (
            "length_vs_stop_same_text_stop",
            DialectId::Qwen3CoderXml,
            85..128,
        ),
        (
            "trailing_text_inside_wrapper",
            DialectId::Qwen3CoderXml,
            0..66,
        ),
        (
            "json_body_stray_closer_trailing_text",
            DialectId::Qwen3CoderXml,
            0..97,
        ),
    ] {
        let golden = load(&goldens_dir().join(format!("{name}.json")));
        let pinned = golden.run();
        let default = golden.run_default().expect("a pinned dialect list");
        let (pinned, default) = (reading(&pinned), reading(&default));
        assert_eq!(default.calls, pinned.calls, "{name}");
        assert_eq!(default.rejected.len(), 1, "{name}: {default:#?}");
        let rejected = &default.rejected[0];
        assert_eq!(
            (&rejected.dialect, &rejected.span),
            (&dialect, &reach),
            "{name}"
        );
        assert_eq!(
            rejected.candidate_dialects,
            [DialectId::Hermes, DialectId::Qwen3CoderXml],
            "{name}"
        );
        assert_eq!(
            pinned.rejected[0].candidate_dialects,
            [DialectId::Qwen3CoderXml],
            "{name}"
        );
        assert_eq!(default.finish, pinned.finish, "{name}");
    }
}

#[test]
fn fenced_hermes_call() {
    let (_, result) = golden("fenced_hermes_call");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert!(
        !loss_kinds(call, "tool_call.extract.fence").is_empty()
            && loss_kinds(call, "tool_call.extract.fence")
                .iter()
                .all(|kind| *kind == LossKind::Markup),
        "{call:#?}"
    );
}

#[test]
fn prose_preamble_hermes_call() {
    let (_, result) = golden("prose_preamble_hermes_call");
    assert_ne!(outcome_kind(&result.outcome), "resolved");
    let call = the_call(&result);
    assert_eq!(
        loss_kinds(call, "tool_call.extract.prose"),
        [LossKind::Data],
        "{call:#?}"
    );
    assert!(
        matches!(call.outcome, CallOutcome::BestGuess) || is_unresolved(&call.outcome),
        "{call:#?}"
    );
}

#[test]
fn stringified_arguments_invalid_json() {
    let (golden, result) = golden("stringified_arguments_invalid_json");
    let call = the_call(&result);
    assert!(
        matches!(
            &call.outcome,
            CallOutcome::Unresolved {
                reason: UnresolvedReason::InvalidArgument { .. }
            }
        ),
        "{call:#?}"
    );
    assert_eq!(reading(&result).usable_accepting_guesses().len(), 0);
    let raw = &golden.input[call.span.clone()];
    assert_eq!(raw, golden.input.as_slice(), "the call's raw text is kept");
    assert_eq!(result.receipt.sources[0].bytes, golden.input);
}

#[test]
fn call_in_unclosed_reasoning_block() {
    let (_, result) = golden("call_in_unclosed_reasoning_block");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::BestGuess, "{call:#?}");
    assert!(rules_of(call).contains(&"tool_call.reasoning.unclosed"));
}

#[test]
fn dangling_trailing_tool_call() {
    let (_, result) = golden("dangling_trailing_tool_call");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 1, "{calls:#?}");
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(calls.rejected.len(), 1);
    assert_eq!(
        calls.rejected[0]
            .repairs
            .losses
            .iter()
            .map(|loss| (loss.loss.evidence.rule.as_str(), loss.kind))
            .collect::<Vec<_>>(),
        [("tool_call.markup.dangling-opener", LossKind::Data)]
    );
}

#[test]
fn nonverbatim_value_trimmed() {
    let (_, result) = golden("nonverbatim_value_trimmed");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(call.arguments.to_json(), r#"{"n":5}"#);
    assert!(rules_of(call).contains(&"tool_call.value.trim-nonverbatim"));
}

#[test]
fn stray_closers_separated_by_newlines() {
    let (_, result) = golden("stray_closers_separated_by_newlines");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(reading(&result).finish, Finish::Calls);
    assert!(rules_of(call).contains(&"tool_call.markup.stray-closer"));
}

#[test]
fn missing_closers_at_end_under_stop() {
    let (_, result) = golden("missing_closers_at_end_under_stop");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(call.arguments.to_json(), r#"{"a":"x"}"#);
    assert!(rules_of(call).contains(&"tool_call.markup.missing-closers-at-end"));
}

#[test]
fn stop_sequence_consumed_closer() {
    let (_, result) = golden("stop_sequence_consumed_closer");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert!(rules_of(call).contains(&"tool_call.stop.consumed-closer"));
}

#[test]
fn reasoning_starts_open() {
    let (golden, result) = golden("reasoning_starts_open");
    let calls = reading(&result);
    assert_eq!(calls.reasoning.len(), 1);
    let end = calls.reasoning[0].end;
    assert!(golden.input[..end].ends_with(b"</think>"));
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
}

#[test]
fn effort_exact_refuses_repair() {
    let (_, result) = golden("effort_exact_refuses_repair");
    let call = the_call(&result);
    assert!(
        matches!(
            &call.outcome,
            CallOutcome::Unresolved {
                reason: UnresolvedReason::RepairNotPermitted { rule }
            } if rule == "tool_call.markup.stray-closer"
        ),
        "{call:#?}"
    );
    assert_eq!(reading(&result).usable().len(), 0);
}

#[test]
fn openai_member_order_missing_id() {
    let (_, result) = golden("openai_member_order_missing_id");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 2, "{calls:#?}");
    assert_eq!(calls.calls[0].source_id.as_deref(), Some("c1"));
    assert_eq!(calls.calls[1].source_id, None);
    for call in &calls.calls {
        assert_eq!(call.arguments.to_json(), r#"{"city":"Lima"}"#);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    }
    assert_ne!(calls.calls[0].id, calls.calls[1].id);
}

#[test]
fn tool_choice_none_with_calls() {
    let (_, result) = golden("tool_choice_none_with_calls");
    let calls = reading(&result);
    assert!(!calls.tool_choice.passed);
    assert!(
        matches!(
            calls.calls[0].outcome,
            CallOutcome::Unresolved {
                reason: UnresolvedReason::ToolChoiceNone
            }
        ),
        "{:#?}",
        calls.calls[0]
    );
    assert_eq!(calls.usable_accepting_guesses().len(), 0);
    assert_eq!(calls.finish, Finish::Unresolved);
}

#[test]
fn strict_ambiguity_rejected() {
    let (_, result) = golden("strict_ambiguity_rejected");
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(result.receipt.stopped, StopReason::NoCandidate);
    assert!(
        result
            .receipt
            .validations
            .iter()
            .any(|validation| !validation.passed && validation.constraint == "tool_call.ambiguity"),
        "{:#?}",
        result.receipt.validations
    );
}

#[test]
fn invalid_utf8_retained() {
    let (golden, result) = golden("invalid_utf8_in_content");
    assert!(core::str::from_utf8(&golden.input).is_err());
    assert_eq!(outcome_kind(&result.outcome), "best_guess");
    assert_eq!(result.receipt.sources[0].bytes, golden.input);
    assert!(
        result
            .receipt
            .losses
            .iter()
            .any(|loss| loss.evidence.rule == "tool_call.input.invalid-utf8")
    );
    assert_eq!(the_call(&result).outcome, CallOutcome::Resolved);
}

#[test]
fn reasoning_embedded_call_is_only_an_alternative() {
    let (_, result) = golden("reasoning_embedded_call");
    assert_eq!(outcome_kind(&result.outcome), "resolved");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 0);
    assert_eq!(calls.finish, Finish::ContentOnly);
    assert_eq!(calls.embedded.len(), 1);
    let embedded = &calls.embedded[0];
    assert_eq!(embedded.outcome, CallOutcome::BestGuess);
    assert!(rules_of(embedded).contains(&"tool_call.reasoning.embedded-call"));
    assert!(calls.reasoning[0].start <= embedded.span.start);
    assert!(embedded.span.end <= calls.reasoning[0].end);
    assert_eq!(
        (calls.usable().len(), calls.usable_accepting_guesses().len()),
        (0, 0)
    );
}

#[test]
fn fenced_example_is_a_guess() {
    let (_, result) = golden("fenced_example_in_prose");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::BestGuess, "{call:#?}");
    assert!(rules_of(call).contains(&"tool_call.context.fenced-example"));
    assert_eq!(reading(&result).usable().len(), 0);
    let (_, result) = golden("fenced_example_structural");
    let call = the_call(&result);
    assert!(is_unresolved(&call.outcome), "{call:#?}");
    assert_eq!(reading(&result).usable_accepting_guesses().len(), 0);
}

#[test]
fn trailing_text_inside_wrapper_rejected() {
    let (golden, result) = golden("trailing_text_inside_wrapper");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 0, "{calls:#?}");
    assert_eq!(calls.rejected.len(), 1);
    let rejected = &calls.rejected[0];
    assert_eq!(
        rejected.raw.as_slice(),
        &golden.input[rejected.span.clone()]
    );
    assert!(matches!(
        rejected.reason,
        purrassist_core::tool_call::Rejection::Malformed { .. }
    ));
    assert_eq!(calls.finish, Finish::Unresolved);
    assert_eq!(calls.usable_accepting_guesses().len(), 0);
}

const BEFORE_TEXT: &str = "tool_call.markup.missing-wrapper-close.before-text";

#[test]
fn missing_wrapper_close_then_prose_tolerant() {
    let (_, result) = golden("missing_wrapper_close_then_prose_tolerant");
    assert_eq!(outcome_kind(&result.outcome), "best_guess");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::BestGuess, "{call:#?}");
    assert_eq!(call.arguments.to_json(), r#"{"a":"x"}"#);
    assert!(rules_of(call).contains(&BEFORE_TEXT));
    assert!(!rules_of(call).contains(&"tool_call.markup.missing-wrapper-close"));
    let calls = reading(&result);
    assert_eq!(calls.usable().len(), 0);
    assert_eq!(calls.usable_accepting_guesses().len(), 1);
}

#[test]
fn missing_wrapper_close_then_prose_structural() {
    let (_, result) = golden("missing_wrapper_close_then_prose_structural");
    let call = the_call(&result);
    assert_mutating_structural(call);
    assert_eq!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::RuleNotAdmitted {
                rule: BEFORE_TEXT.into(),
                policy: RepairPolicy::Structural,
            }
        }
    );
    let calls = reading(&result);
    assert_eq!(
        (calls.usable().len(), calls.usable_accepting_guesses().len()),
        (0, 0)
    );
    assert_eq!(calls.finish, Finish::Unresolved);
}

#[test]
fn missing_wrapper_close_then_prose_then_call() {
    let (_, result) = golden("missing_wrapper_close_then_prose_then_call");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 2, "{calls:#?}");
    assert_eq!(calls.calls[0].outcome, CallOutcome::BestGuess);
    assert!(rules_of(&calls.calls[0]).contains(&BEFORE_TEXT));
    assert_eq!(calls.calls[1].outcome, CallOutcome::Resolved);
    assert_eq!(calls.content.len(), 1);
    // Before another call's opener with nothing between, the insertion is
    // the only reading.
    let (_, result) = golden("missing_wrapper_close");
    let first = &reading(&result).calls[0];
    assert_eq!(first.outcome, CallOutcome::Resolved);
    assert!(rules_of(first).contains(&"tool_call.markup.missing-wrapper-close"));
}
