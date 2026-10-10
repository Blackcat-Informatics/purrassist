// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Tool-call rule goldens (A3, A3a–A3q, A13): each fixture under
//! `tests/data/tool-call-goldens/` pairs a model turn (`input`) with its
//! tool declarations (`tools_openai`, the exact `OpenAI` `tools` text or
//! value; `tools_mcp`, the exact MCP declaration or `tools/list` text or
//! value; or `tools`, a builder specification for aliases, repeatable
//! parameters and unknown-parameter modes adapters cannot express), an
//! optional custom `grammar`, and the request (`dialects`, `policy`,
//! `effects`, `stop`, `tool_choice`, `effort`, `reasoning`, `ids`,
//! `ambiguity`). The outcome, finish, usability verdict and usable-call
//! counts, calls, rejected attempts, the content and the marker-free
//! reasoning text (each range as text), calls embedded in reasoning, failed validations and the recorded rule ids are
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
    RepairPolicy, StrictForced, ToolCall, ToolCalls, ToolChoice, ToolDefinition,
    ToolRecoveryRequest, ToolSet, UnknownParameters, UnresolvedReason,
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
    "openai_finish_reason",
    "emitted",
    "verdict",
    "usable_calls",
    "calls",
    "rejected",
    "embedded",
    "content",
    "reasoning_text",
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

/// The exact tools text of `key`: a string member verbatim, else the
/// value's compact JSON.
fn tools_text(golden: &Node, key: &str) -> Option<String> {
    golden
        .get(key)
        .map(|node| node.as_str().map_or_else(|| node.to_json(), str::to_owned))
}

/// The exact `OpenAI` tools text (`tools_openai`).
fn openai_text(golden: &Node) -> Option<String> {
    tools_text(golden, "tools_openai")
}

fn tool_set(golden: &Node, path: &Path) -> ToolSet {
    match (
        openai_text(golden),
        tools_text(golden, "tools_mcp"),
        golden.get("tools"),
    ) {
        (Some(text), None, None) => ToolSet::from_openai(&text, Budget::default())
            .unwrap_or_else(|error| panic!("{}: tools_openai: {error}", path.display())),
        (None, Some(text), None) => ToolSet::from_mcp(&text, Budget::default())
            .unwrap_or_else(|error| panic!("{}: tools_mcp: {error}", path.display())),
        (None, None, Some(tools)) => {
            ToolSet::compile(items(tools).iter().map(definition), Budget::default())
                .unwrap_or_else(|error| panic!("{}: tools: {error}", path.display()))
        }
        _ => panic!(
            "{}: exactly one of `tools_openai`, `tools_mcp` and `tools` is required",
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

/// The `tool_choice` input: the `OpenAI` form, or the Anthropic form when
/// it is an object whose `type` is not `"function"` (as the host reads it),
/// with the optional `tool_choice_strict` object
/// `{exactly_one, allow_content, allow_reasoning}` (all booleans, all
/// required) applied to a forced choice.
fn tool_choice(golden: &Node, choice: &Node, path: &Path) -> ToolChoice {
    let anthropic = matches!(choice.value, Value::Object(_))
        && choice.get("type").and_then(Node::as_str) != Some("function");
    let parsed = if anthropic {
        ToolChoice::from_anthropic(choice)
    } else {
        ToolChoice::from_openai(choice)
    }
    .unwrap_or_else(|error| panic!("{}: tool_choice: {error:?}", path.display()));
    let Some(strict) = golden.get("tool_choice_strict") else {
        return parsed;
    };
    let flag = |key: &str| match strict.get(key).map(|node| &node.value) {
        Some(Value::Bool(flag)) => *flag,
        other => panic!(
            "{}: tool_choice_strict.{key} must be a boolean, not {other:?}",
            path.display()
        ),
    };
    let name = match parsed {
        ToolChoice::Forced(name) => name,
        ToolChoice::ForcedStrict(existing) => existing.name,
        other => panic!(
            "{}: tool_choice_strict needs a forced tool_choice, not {other:?}",
            path.display()
        ),
    };
    ToolChoice::ForcedStrict(StrictForced {
        name,
        exactly_one: flag("exactly_one"),
        allow_content: flag("allow_content"),
        allow_reasoning: flag("allow_reasoning"),
    })
}

fn request(golden: &Node, path: &Path, input: &[u8], dialects: DialectSet) -> ToolRecoveryRequest {
    let effort = golden
        .get("effort")
        .and_then(Node::as_str)
        .map_or(Effort::Standard, effort_of);
    let mut request = ToolRecoveryRequest::sized_for(effort, input, dialects);
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
        request = request.tool_choice(tool_choice(golden, choice, path));
    } else {
        assert!(
            golden.get("tool_choice_strict").is_none(),
            "{}: `tool_choice_strict` needs a forced `tool_choice`",
            path.display()
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
    if let Some(emit) = golden.get("emit").and_then(Node::as_str) {
        request = request.emit(match emit {
            "openai" => purrassist_core::tool_call::EmitTarget::OpenAiToolCalls,
            other => panic!("unknown emit target `{other}`"),
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
        .map(|_| request(&node, path, &input, default_dialect_set(&node, path)));
    Golden {
        path: path.to_owned(),
        tools: tool_set(&node, path),
        request: request(&node, path, &input, dialect_set(&node, path)),
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
                    member(
                        "dialect",
                        rejected
                            .dialect
                            .as_ref()
                            .map_or(Node::new(Value::Null), |dialect| string(dialect.id())),
                    ),
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

fn reading_fields(output: &[u8], calls: &ToolCalls) -> Vec<Member> {
    let mut fields = vec![
        member("finish", string(calls.finish.code())),
        member("openai_finish_reason", string(calls.openai_finish_reason())),
        member("verdict", string(calls.verdict().code())),
        member(
            "usable_calls",
            object(vec![
                member("strict", integer(calls.usable().len())),
                member(
                    "accepting_guesses",
                    integer(calls.usable_accepting_guesses().len()),
                ),
            ]),
        ),
        member(
            "calls",
            Node::new(Value::Array(calls.calls.iter().map(call_node).collect())),
        ),
        member("rejected", rejected_nodes(calls)),
        member(
            "embedded",
            Node::new(Value::Array(calls.embedded.iter().map(call_node).collect())),
        ),
        // The assistant content, each range as text (bytes when not UTF-8),
        // so a golden pins what stays prose beside its calls and attempts.
        member(
            "content",
            Node::new(Value::Array(
                calls
                    .content
                    .iter()
                    .map(|range| {
                        let bytes = &output[range.clone()];
                        string(
                            std::str::from_utf8(bytes)
                                .map_or_else(|_| format!("{bytes:?}"), str::to_owned),
                        )
                    })
                    .collect(),
            )),
        ),
        // The marker-free text of each reasoning block, one entry per block
        // (`ToolCalls::reasoning_blocks`, never merged), as text (bytes when
        // not UTF-8). Not the `reasoning` fixture input, the policy.
        member(
            "reasoning_text",
            Node::new(Value::Array(
                calls
                    .reasoning_blocks
                    .iter()
                    .map(|block| {
                        let bytes = &output[block.text.clone()];
                        string(
                            std::str::from_utf8(bytes)
                                .map_or_else(|_| format!("{bytes:?}"), str::to_owned),
                        )
                    })
                    .collect(),
            )),
        ),
    ];
    // An emission, when the golden requests one: the calls it prints and
    // why it skips the rest.
    if let Some(emission) = &calls.emitted {
        let (printed, skipped) = match emission {
            purrassist_core::tool_call::Emission::OpenAiToolCalls(emitted) => {
                (emitted.calls.len(), &emitted.skipped)
            }
            purrassist_core::tool_call::Emission::Dialect { emitted, .. } => {
                (emitted.calls.len(), &emitted.skipped)
            }
        };
        let skipped: Vec<String> = skipped
            .iter()
            .map(|skipped| match skipped {
                purrassist_core::tool_call::Skipped::Call { index, reason } => {
                    format!("call {index}: {}", reason.code())
                }
                purrassist_core::tool_call::Skipped::Rejected { index, .. } => {
                    format!("rejected {index}")
                }
            })
            .collect();
        fields.push(member(
            "emitted",
            object(vec![
                member("calls", integer(printed)),
                member("skipped", strings(&skipped)),
            ]),
        ));
    }
    fields
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

fn captured_fields(output: &[u8], result: &AssistResult<ToolCalls>) -> Vec<Member> {
    let mut fields = vec![
        member("outcome", string(outcome_kind(&result.outcome))),
        member("stopped", string(result.receipt.stopped.to_string())),
    ];
    match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => {
            fields.extend(reading_fields(output, calls));
        }
        Outcome::Ambiguous(readings) => fields.push(member(
            "readings",
            Node::new(Value::Array(
                readings
                    .iter()
                    .map(|calls| object(reading_fields(output, calls)))
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
    output: &[u8],
    result: &AssistResult<ToolCalls>,
    default: Option<&AssistResult<ToolCalls>>,
) -> Node {
    let mut kept: Vec<Member> = members(golden)
        .iter()
        .filter(|member| !CAPTURED.contains(&member.key.as_str()) && member.key != DEFAULT_EXPECT)
        .cloned()
        .collect();
    let pinned = object(captured_fields(output, result));
    if let Some(default) = default {
        let default = object(captured_fields(output, default));
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
                pretty(&reblessed(
                    &golden.node,
                    &golden.input,
                    result,
                    default.as_ref(),
                )),
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
        let actual = object(captured_fields(&golden.input, result));
        failures.extend(mismatches(path, "pinned", &golden.node, &actual));
        match (&default, golden.node.get(DEFAULT_EXPECT)) {
            (Some(default), Some(expected)) => {
                let actual = object(captured_fields(&golden.input, default));
                if actual.equivalent(&object(captured_fields(&golden.input, result))) {
                    failures.push(format!(
                        "{}: `{DEFAULT_EXPECT}` restates the pinned result; drop it",
                        path.display()
                    ));
                }
                failures.extend(mismatches(path, "default allow-list", expected, &actual));
            }
            (Some(default), None) => {
                let actual = object(captured_fields(&golden.input, default));
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

/// R11 (G19): the arguments and rules of the single call of golden `name`,
/// after asserting the byte-exact render-back of its MCP declaration in all
/// three wire forms (the `parameters` text is embedded verbatim).
fn mcp_golden(name: &str) -> (Golden, AssistResult<ToolCalls>) {
    let (golden, result) = golden(name);
    let source = tools_text(&golden.node, "tools_mcp").expect("tools_mcp text");
    let listing = parse(&source).expect("tools json");
    let declaration = &items(listing.get("tools").expect("tools"))[0];
    let span = declaration.span.clone().expect("declaration span");
    assert_eq!(
        golden
            .tools
            .render_declaration("search", DeclarationForm::Mcp)
            .expect("rendered")
            .as_bytes(),
        &source.as_bytes()[span],
        "{name}: MCP render-back"
    );
    let schema = declaration
        .get("inputSchema")
        .expect("inputSchema")
        .to_json();
    for (form, wrapped) in [
        (
            DeclarationForm::OpenAi,
            format!(
                r#"{{"type":"function","function":{{"name":"search","description":"Search the index.","parameters":{schema}}}}}"#
            ),
        ),
        (
            DeclarationForm::Anthropic,
            format!(
                r#"{{"name":"search","description":"Search the index.","input_schema":{schema}}}"#
            ),
        ),
    ] {
        assert_eq!(
            golden
                .tools
                .render_declaration("search", form)
                .expect("rendered"),
            wrapped,
            "{name}: {form:?} render-back"
        );
    }
    (golden, result)
}

/// R11 (G19): a root `$ref` MCP declaration (the FastMCP/pydantic shape)
/// projects its target's parameters. Expectations are written here, not
/// taken from the blessed files.
#[test]
fn mcp_root_ref_calls_resolve_with_typed_arguments() {
    for (name, arguments, rules, outcome) in [
        // Qwen3-Coder XML: `limit` decoded to the declared integer, `tags`
        // read as the JSON array it spells.
        (
            "mcp_root_ref_qwen3_xml_resolved",
            r#"{"query":"rust lifetimes","limit":5,"tags":["a","b"]}"#,
            &[][..],
            CallOutcome::Resolved,
        ),
        (
            "mcp_root_ref_hermes_json_resolved",
            r#"{"query":"rust","limit":3,"mode":"fast"}"#,
            &[][..],
            CallOutcome::Resolved,
        ),
        // 2020-12: the root's `extra` (boolean) and the target's `query`;
        // `limit` is declared by both, typed by the exact schema.
        (
            "mcp_root_defs_2020_12_resolved",
            r#"{"extra":true,"query":"q","limit":2}"#,
            &["tool_call.value.schema-typed"][..],
            CallOutcome::Resolved,
        ),
        (
            "mcp_root_all_of_ref_resolved",
            r#"{"query":"q","limit":7}"#,
            &[][..],
            CallOutcome::Resolved,
        ),
        // The union of branch names; `count` is typed by the exact schema.
        (
            "mcp_root_any_of_union_resolved",
            r#"{"url":"http://example.com","count":4}"#,
            &["tool_call.value.schema-typed"][..],
            CallOutcome::Resolved,
        ),
    ] {
        let (_, result) = mcp_golden(name);
        let call = the_call(&result);
        assert_eq!(call.outcome, outcome, "{name}");
        assert_eq!(call.arguments.to_json(), arguments, "{name}");
        assert_eq!(rules_of(call), rules, "{name}");
        assert!(!has_guess(call), "{name}");
        assert_eq!(reading(&result).usable().len(), 1, "{name}");
    }
}

/// R11 (G19): a misspelled parameter name is repaired through the root
/// `$ref` exactly as for the inline twin, in both prompt dialects.
#[test]
fn mcp_root_ref_misspelled_parameter_repairs_like_inline() {
    for (via_ref, inline) in [
        (
            "mcp_root_ref_misspelled_parameter_repaired",
            "mcp_inline_misspelled_parameter_repaired",
        ),
        (
            "mcp_root_ref_misspelled_qwen_parameter_repaired",
            "mcp_inline_misspelled_qwen_parameter_repaired",
        ),
    ] {
        let (_, referenced) = mcp_golden(via_ref);
        let (_, direct) = mcp_golden(inline);
        let (referenced, direct) = (the_call(&referenced), the_call(&direct));
        assert_eq!(
            referenced.arguments.to_json(),
            r#"{"query":"rust"}"#,
            "{via_ref}"
        );
        assert_eq!(direct.arguments.to_json(), referenced.arguments.to_json());
        assert_eq!(rules_of(referenced), ["json.key.match"], "{via_ref}");
        assert_eq!(rules_of(direct), rules_of(referenced));
        assert_eq!(referenced.outcome, direct.outcome);
        assert!(has_guess(referenced), "{via_ref}: a renamed key is a guess");
    }
}

/// R11 (G19): a root `$ref` that names no object schema keeps the
/// pass-through projection: no repair, the arguments verbatim, and the
/// exact schema's verdict.
#[test]
fn mcp_root_ref_unprojectable_stays_pass_through() {
    let (golden, result) = mcp_golden("mcp_root_ref_unprojectable_stays_pass_through");
    let call = the_call(&result);
    assert_eq!(call.arguments.to_json(), r#"{"qurey":"rust"}"#);
    assert_eq!(rules_of(call), ["tool_call.parameter.pass-through"]);
    assert_eq!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::SchemaValidation
        }
    );
    let shape = golden.tools.argument_shape("search").expect("shape");
    assert_eq!(shape.arguments.parameters.len(), 0);
    assert_eq!(
        shape.arguments.unknown_parameters,
        UnknownParameters::PassThrough
    );
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

/// A strict forced run that must fail its tool-choice check: the whole-turn
/// contract (finish `Unresolved`, verdict `NothingUsable`, nothing emitted)
/// and the strict constraint among the failed validations.
fn assert_strict_failed(name: &str, constraint: &str) -> ToolCalls {
    let (_, result) = golden(name);
    let calls = reading(&result).clone();
    assert!(!calls.tool_choice.passed, "{name}: {calls:#?}");
    assert_eq!(calls.tool_choice.choice.constraint(), constraint, "{name}");
    assert!(
        result
            .receipt
            .validations
            .iter()
            .any(|validation| !validation.passed && validation.constraint == constraint),
        "{name}: {:#?}",
        result.receipt.validations
    );
    assert_eq!(calls.finish, Finish::Unresolved, "{name}");
    assert_eq!(calls.verdict().code(), "nothing_usable", "{name}");
    assert!(calls.emitted_calls().is_empty(), "{name}");
    // A failed check leaves nothing to run (A3): both counts are zero.
    assert!(
        calls.usable().is_empty() && calls.usable_accepting_guesses().is_empty(),
        "{name}"
    );
    calls
}

#[test]
fn forced_strict_two_calls() {
    let calls = assert_strict_failed(
        "forced_strict_two_calls",
        "tool_choice=forced[exactly-one,no-content]:read_file",
    );
    assert_eq!(calls.calls.len(), 2);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[1].outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::ToolChoiceCardinality
        }
    );
    assert_eq!(
        calls.tool_choice.diagnostics.len(),
        1,
        "{:?}",
        calls.tool_choice.diagnostics
    );
    assert!(calls.tool_choice.diagnostics[0].contains("1 extra call"));
}

#[test]
fn forced_strict_call_plus_prose() {
    let calls = assert_strict_failed(
        "forced_strict_call_plus_prose",
        "tool_choice=forced[exactly-one,no-content]:read_file",
    );
    // The call itself is fine; the prose is what fails the choice.
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert!(
        calls
            .tool_choice
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.contains("content")),
        "{:?}",
        calls.tool_choice.diagnostics
    );
    assert!(!calls.tool_choice.diagnostics.is_empty(), "{calls:#?}");
}

#[test]
fn forced_strict_single_call_passes() {
    let (_, result) = golden("forced_strict_single_call_passes");
    let calls = reading(&result);
    assert!(calls.tool_choice.passed, "{:?}", calls.tool_choice);
    assert_eq!(
        calls.tool_choice.choice.constraint(),
        "tool_choice=forced[exactly-one,no-content]:read_file"
    );
    assert_eq!(calls.finish, Finish::Calls);
    assert_eq!(calls.verdict().code(), "run");
    assert_eq!(calls.emitted_calls().len(), 1);
    // Whitespace-only content frames the call.
    assert_eq!(calls.content.len(), 2);
    assert!(
        result
            .receipt
            .validations
            .iter()
            .all(|validation| validation.passed)
    );
}

#[test]
fn forced_strict_non_ascii_whitespace_is_content() {
    // U+00A0 after the call is content, not framing whitespace.
    let (golden, _) = golden("forced_strict_single_call_passes");
    let mut input = golden.input.clone();
    input.truncate(input.len() - 1);
    input.extend_from_slice("\u{a0}".as_bytes());
    let result = golden.tools.recover(&input, &golden.request);
    let calls = reading(&result);
    assert!(!calls.tool_choice.passed, "{calls:#?}");
    assert_eq!(calls.finish, Finish::Unresolved);
    assert!(calls.emitted_calls().is_empty(), "{calls:#?}");
    assert!(
        calls
            .tool_choice
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("content")),
        "{:?}",
        calls.tool_choice.diagnostics
    );
    // ASCII whitespace of every kind still passes.
    let mut input = golden.input.clone();
    input.truncate(input.len() - 1);
    input.extend_from_slice(b" \t\r\n\x0c");
    let result = golden.tools.recover(&input, &golden.request);
    assert!(reading(&result).tool_choice.passed);
}

#[test]
fn forced_strict_tool_result_is_content() {
    // A tool result the model wrote after its call is content, so it fails
    // no-content even though no prose surrounds the call.
    let (golden, _) = golden("forced_strict_single_call_passes");
    let mut input = golden.input.clone();
    input.extend_from_slice(b"<tool_response>\n{\"text\":\"a\"}\n</tool_response>\n");
    let result = golden.tools.recover(&input, &golden.request);
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 1, "{calls:#?}");
    assert!(calls.rejected.is_empty(), "{calls:#?}");
    assert!(!calls.tool_choice.passed, "{calls:#?}");
    assert_eq!(
        calls.tool_choice.diagnostics.len(),
        1,
        "{:?}",
        calls.tool_choice.diagnostics
    );
    assert!(
        calls.tool_choice.diagnostics[0].contains("content"),
        "{:?}",
        calls.tool_choice.diagnostics
    );
    assert_eq!(calls.finish, Finish::Unresolved);
    assert!(calls.emitted_calls().is_empty(), "{calls:#?}");
}

#[test]
fn forced_strict_reasoning_rejected() {
    let calls = assert_strict_failed(
        "forced_strict_reasoning_rejected",
        "tool_choice=forced[exactly-one,no-content,no-reasoning]:read_file",
    );
    assert_eq!(calls.reasoning.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    // Only the reasoning fails: the remaining content is a newline.
    assert_eq!(calls.tool_choice.diagnostics.len(), 1);
    assert!(calls.tool_choice.diagnostics[0].contains("reasoning"));
}

/// A second call attempt that is rejected (a `read_file` body with trailing
/// junk) fails every strict option at once: it is a call beyond the one
/// (`exactly-one`) and bytes outside it (`no-content`), so the turn is
/// `nothing_usable` with nothing emitted though the first call is valid.
#[test]
fn forced_strict_rejected_second_attempt() {
    let calls = assert_strict_failed(
        "forced_strict_rejected_second_attempt",
        "tool_choice=forced[exactly-one,no-content,no-reasoning]:read_file",
    );
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(calls.rejected.len(), 1);
    assert!(
        calls
            .tool_choice
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.starts_with("1 rejected call attempt beyond")),
        "{:?}",
        calls.tool_choice.diagnostics
    );
    assert!(
        calls
            .tool_choice
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.starts_with("rejected call attempt at bytes 75..163")),
        "{:?}",
        calls.tool_choice.diagnostics
    );
}

/// The Anthropic exactly-one form allows content, but an attempt in a
/// dialect that is not supported is still a call attempt beyond the one
/// call: the turn fails although the attempt's bytes are also content.
#[test]
fn forced_strict_anthropic_unsupported_dialect_attempt() {
    let calls = assert_strict_failed(
        "forced_strict_anthropic_unsupported_dialect_attempt",
        "tool_choice=forced[exactly-one]:read_file",
    );
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(calls.rejected.len(), 1);
    assert!(
        matches!(
            calls.rejected[0].reason,
            purrassist_core::tool_call::Rejection::UnsupportedDialect { .. }
        ),
        "{:?}",
        calls.rejected
    );
    assert_eq!(calls.tool_choice.diagnostics.len(), 1);
    assert!(calls.tool_choice.diagnostics[0].starts_with("1 rejected call attempt beyond"));
}

#[test]
fn forced_strict_anthropic_disable_parallel() {
    let calls = assert_strict_failed(
        "forced_strict_anthropic_disable_parallel",
        "tool_choice=forced[exactly-one]:read_file",
    );
    assert_eq!(calls.calls.len(), 2);
    assert_eq!(
        calls.calls[1].outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::ToolChoiceCardinality
        }
    );
    // Content is allowed by this form: the prose is not what fails.
    assert!(!calls.content.is_empty(), "{calls:#?}");
    assert!(
        calls
            .tool_choice
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.contains("content")),
        "{:?}",
        calls.tool_choice.diagnostics
    );
    let (_, result) = golden("forced_strict_anthropic_one_call_with_prose_passes");
    let calls = reading(&result);
    assert!(calls.tool_choice.passed, "{:?}", calls.tool_choice);
    assert_eq!(calls.finish, Finish::Calls);
    assert_eq!(calls.emitted_calls().len(), 1);
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

/// Assert a nested array-of-objects golden: one resolved call holding
/// `arguments`, whose rules include every id in `rules` (with counts).
fn assert_nested_items(name: &str, arguments: &str, rules: &[(&str, usize)]) {
    let (_, result) = golden(name);
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{name}: {call:#?}");
    assert_eq!(call.arguments.to_json(), arguments, "{name}");
    assert!(call.alternatives.is_empty(), "{name}: {call:#?}");
    let used = rules_of(call);
    for (rule, count) in rules {
        assert_eq!(
            used.iter().filter(|id| **id == *rule).count(),
            *count,
            "{name}: {rule} in {used:?}"
        );
    }
    for absent in [
        "json.syntax.bare-string",
        "tool_call.parameter.pass-through",
        "tool_call.markup.stray-closer",
    ] {
        assert!(!used.contains(&absent), "{name}: {absent} in {used:?}");
    }
}

const OUTFITS: &str = r#"{"outfits":[{"wearer":"Alice","garments":["a red coat"]},{"wearer":"Bob","garments":["a grey scarf"]}]}"#;

#[test]
fn xml_array_nested_object_items() {
    // Issue #8 section 1: repeated nested elements are an array of objects.
    assert_nested_items(
        "xml_array_nested_object_items",
        OUTFITS,
        &[
            ("tool_call.markup.nested-parameter", 2),
            ("tool_call.xml.array-item", 2),
            ("tool_call.parameter.collect", 1),
        ],
    );
}

#[test]
fn xml_array_nested_object_items_in_object() {
    assert_nested_items(
        "xml_array_nested_object_items_in_object",
        r#"{"plan":{"cast":["Alice","Bob"],"outfits":[{"wearer":"Alice","garments":["a red coat"]},{"wearer":"Bob","garments":["a grey scarf"]}]}}"#,
        &[
            ("tool_call.markup.nested-parameter", 3),
            ("tool_call.xml.array-item", 2),
            ("tool_call.parameter.collect", 1),
        ],
    );
}

#[test]
fn xml_array_nested_object_items_mixed() {
    // A JSON-object item and a nested item collect in source order.
    assert_nested_items(
        "xml_array_nested_object_items_mixed",
        OUTFITS,
        &[
            ("tool_call.markup.nested-parameter", 1),
            ("tool_call.xml.array-item", 2),
            ("tool_call.parameter.collect", 1),
        ],
    );
}

#[test]
fn xml_array_nested_object_item_unclosed() {
    assert_nested_items(
        "xml_array_nested_object_item_unclosed",
        OUTFITS,
        &[
            ("tool_call.markup.nested-parameter", 2),
            ("tool_call.markup.parameter-closed-by-element", 1),
            ("tool_call.xml.array-item", 2),
        ],
    );
}

#[test]
fn xml_array_nested_object_item_unclosed_before_sibling() {
    assert_nested_items(
        "xml_array_nested_object_item_unclosed_before_sibling",
        OUTFITS,
        &[
            ("tool_call.markup.nested-parameter", 2),
            ("tool_call.markup.missing-parameter-close", 1),
            ("tool_call.xml.array-item", 2),
            ("tool_call.parameter.collect", 1),
        ],
    );
}

#[test]
fn xml_array_unmodeled_items_nested() {
    assert_nested_items(
        "xml_array_unmodeled_items_nested",
        OUTFITS,
        &[
            ("tool_call.markup.nested-parameter", 2),
            ("tool_call.xml.array-item", 2),
            ("tool_call.value.schema-typed", 2),
        ],
    );
}

#[test]
fn xml_array_string_item_keeps_nested_tags() {
    // An array of strings never nests: the tags are the item's text.
    let (_, result) = golden("xml_array_string_item_keeps_nested_tags");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"tags":["<parameter=wearer>Alice</parameter>"]}"#
    );
    assert!(!rules_of(call).contains(&"tool_call.markup.nested-parameter"));
    assert!(rules_of(call).contains(&"tool_call.value.closer-in-value"));
}

#[test]
fn xml_array_nested_object_same_name_member() {
    // Limit: an item scope declaring the array's own name owns the inner
    // opener, so it is the item's member (a child), not a sibling item.
    let (_, result) = golden("xml_array_nested_object_same_name_member");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"children":[{"name":"a","children":[{"name":"b"}]}]}"#
    );
    let used = rules_of(call);
    let count = |rule: &str| used.iter().filter(|id| **id == rule).count();
    assert_eq!(count("tool_call.markup.nested-parameter"), 2);
    assert_eq!(count("tool_call.xml.array-item"), 2);
    assert_eq!(count("tool_call.parameter.collect"), 0);
    assert_eq!(count("tool_call.markup.missing-parameter-close"), 0);
}

/// Count of `rule` among the rules a call recorded.
fn rule_count(call: &ToolCall, rule: &str) -> usize {
    rules_of(call).iter().filter(|id| **id == rule).count()
}

#[test]
fn xml_array_recursive_ref_items_nest() {
    // Issue #8 section 1, gap G2: a recursive `$ref` item takes the member
    // names its exact schema declares from the scope graph, so the inner
    // opener of the array's own name is the item's child, never the next
    // sibling. The input is balanced: no repair is needed (the engine used
    // to read it as two siblings with a missing and a stray closer).
    let (_, result) = golden("xml_array_recursive_ref_items_nest");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"children":[{"name":"a","children":[{"name":"b"}]}]}"#
    );
    assert_eq!(rule_count(call, "tool_call.markup.nested-parameter"), 2);
    assert_eq!(rule_count(call, "tool_call.xml.array-item"), 2);
    assert_eq!(
        rule_count(call, "tool_call.markup.missing-parameter-close"),
        0
    );
    assert_eq!(
        rule_count(call, "tool_call.markup.unmodeled-scope-boundary"),
        0
    );
    assert_eq!(rule_count(call, "tool_call.markup.stray-closer"), 0);
    assert_eq!(rule_count(call, "tool_call.parameter.collect"), 0);
    assert_eq!(reading(&result).verdict().code(), "run");
}

#[test]
fn xml_array_recursive_ref_items_equal_the_inline_schema() {
    // The same bytes under the inline (modeled) twin of the recursive
    // schema read identically: same arguments, same rules.
    let (golden, result) = golden("xml_array_recursive_ref_items_nest");
    let inline = ToolSet::from_openai(
        r#"[{"type":"function","function":{"name":"tree","parameters":{"type":"object","properties":{"children":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"children":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}},"required":["name"]}}},"required":["children"]}}}]"#,
        Budget::default(),
    )
    .expect("inline twin");
    let twin = inline.recover(&golden.input, &golden.request);
    let (call, twin) = (the_call(&result), the_call(&twin));
    assert_eq!(call.arguments.to_json(), twin.arguments.to_json());
    assert_eq!(rules_of(call), rules_of(twin));
}

#[test]
fn xml_array_ref_items_repeated_member_collects() {
    // Repeated array members inside a `$ref` item collect into one array,
    // in source order, as the inline schema's do.
    let (_, result) = golden("xml_array_ref_items_repeated_member_collects");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"children":[{"name":"a","children":[{"name":"b"},{"name":"c"}]}]}"#
    );
    assert_eq!(rule_count(call, "tool_call.markup.nested-parameter"), 3);
    assert_eq!(rule_count(call, "tool_call.xml.array-item"), 3);
    // The merge is reported, as the inline twin's is: once, for the member
    // that occurred twice.
    assert_eq!(rule_count(call, "tool_call.parameter.collect"), 1);
    assert_eq!(
        rule_count(call, "tool_call.markup.missing-parameter-close"),
        0
    );
    assert_eq!(rule_count(call, "tool_call.markup.stray-closer"), 0);
}

/// The `$ref` and inline forms of one schema record the same rules for the
/// same bytes, repeated array members included: collection has one source of
/// truth (resolution's `tool_call.parameter.collect`), fed by the codec's
/// merge marker under a scope the typed shape does not declare.
#[test]
fn xml_array_ref_items_and_inline_twin_record_the_same_rules() {
    let (golden, _) = golden("xml_array_ref_items_repeated_member_collects");
    let inline = ToolSet::from_openai(
        r#"[{"type":"function","function":{"name":"tree","parameters":{"type":"object","properties":{"children":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"children":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}},"required":["name"]}}},"required":["children"]}}}]"#,
        Budget::default(),
    )
    .expect("inline twin");
    let text = std::str::from_utf8(&golden.input).expect("utf-8 input");
    let three = text.replace(
        "<parameter=name>c</parameter></parameter>",
        "<parameter=name>c</parameter></parameter><parameter=children><parameter=name>d</parameter></parameter>",
    );
    assert_ne!(three, text);
    let destinations = |call: &ToolCall| -> Vec<String> {
        call.repairs
            .evidence
            .iter()
            .filter(|evidence| evidence.rule == "tool_call.parameter.collect")
            .map(|evidence| evidence.destination.clone())
            .collect()
    };
    // Two occurrences, then three: one collect each, at the same place.
    for input in [text, three.as_str()] {
        let reference = golden.tools.recover(input.as_bytes(), &golden.request);
        let twin = inline.recover(input.as_bytes(), &golden.request);
        let (reference, twin) = (the_call(&reference), the_call(&twin));
        assert_eq!(reference.outcome, CallOutcome::Resolved, "{reference:#?}");
        assert_eq!(reference.arguments.to_json(), twin.arguments.to_json());
        assert_eq!(rules_of(reference), rules_of(twin), "{input}");
        assert_eq!(rule_count(reference, "tool_call.parameter.collect"), 1);
        assert_eq!(destinations(reference), destinations(twin));
    }
}

#[test]
fn xml_array_ref_items_member_shares_outer_name() {
    // Gap G2 (b): a `$ref` item member named like an outer parameter stays
    // inside its item (it used to be hoisted to the top level).
    let (_, result) = golden("xml_array_ref_items_member_shares_outer_name");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"name":"party","outfits":[{"name":"Alice","size":3},{"name":"Bob","size":4}]}"#
    );
    assert_eq!(rule_count(call, "tool_call.markup.nested-parameter"), 2);
    assert_eq!(rule_count(call, "tool_call.xml.array-item"), 2);
    assert_eq!(
        rule_count(call, "tool_call.markup.missing-parameter-close"),
        0
    );
    assert_eq!(rule_count(call, "tool_call.markup.stray-closer"), 0);
    assert_eq!(reading(&result).verdict().code(), "run");
}

#[test]
fn xml_array_ref_items_keep_path_not_hijacked() {
    // Security review R1: in rm(path, keep: array of $ref{path}) the keep
    // item's `path` stays inside the item; the top-level path is the one
    // the model wrote as such.
    let (_, result) = golden("xml_array_ref_items_keep_path_not_hijacked");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"path":"/tmp/x","keep":[{"path":"/home/user/important"}]}"#
    );
    assert_eq!(rule_count(call, "tool_call.markup.nested-parameter"), 1);
    assert_eq!(rule_count(call, "tool_call.xml.array-item"), 1);
    assert_eq!(
        rule_count(call, "tool_call.markup.missing-parameter-close"),
        0
    );
    assert_eq!(rule_count(call, "tool_call.markup.stray-closer"), 0);
}

#[test]
fn xml_array_ref_items_outer_array_collision() {
    // Completion audit F1 repro 2: each note's `tags` stays inside its
    // note (one item of the note's tags); the outer `tags` is unaffected.
    let (_, result) = golden("xml_array_ref_items_outer_array_collision");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"tags":["work"],"notes":[{"text":"buy milk","tags":["home"]}]}"#
    );
    assert_eq!(rule_count(call, "tool_call.markup.nested-parameter"), 1);
    assert_eq!(rule_count(call, "tool_call.xml.array-item"), 3);
    assert_eq!(
        rule_count(call, "tool_call.markup.missing-parameter-close"),
        0
    );
    assert_eq!(rule_count(call, "tool_call.markup.stray-closer"), 0);
}

#[test]
fn xml_object_ref_param_member_shares_outer_name() {
    // The single `$ref` object parameter (same root cause, pre-existing):
    // its members come from the same scope graph, so `cfg.path` stays in
    // `cfg` and is never the top-level `path`.
    let (_, result) = golden("xml_object_ref_param_member_shares_outer_name");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"path":"/a","cfg":{"path":"/b","mode":"x"}}"#
    );
    assert_eq!(rule_count(call, "tool_call.markup.nested-parameter"), 1);
    assert_eq!(
        rule_count(call, "tool_call.markup.missing-parameter-close"),
        0
    );
    assert_eq!(rule_count(call, "tool_call.markup.stray-closer"), 0);
}

#[test]
fn xml_array_anyof_items_union_nest() {
    // Union nesting rule: an `anyOf` item scope is the union of its object
    // branches' members, and the item nests because an object branch
    // declares it; typing stays with the exact schema.
    let (_, result) = golden("xml_array_anyof_items_union_nest");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"path":"/a","keep":[{"path":"/b","n":3}]}"#
    );
    assert_eq!(rule_count(call, "tool_call.markup.nested-parameter"), 1);
    assert_eq!(rule_count(call, "tool_call.xml.array-item"), 1);
    assert_eq!(rule_count(call, "tool_call.value.schema-typed"), 1);
    assert_eq!(
        rule_count(call, "tool_call.markup.missing-parameter-close"),
        0
    );
    assert_eq!(rule_count(call, "tool_call.markup.stray-closer"), 0);
}

#[test]
fn xml_unmodeled_opaque_scope_boundary() {
    // Plan review F2: an item whose schema names no member has unknown
    // membership, so closing it at the opener of `path` (an enclosing
    // scope's parameter) is a guess. The hoisted path must never run on a
    // mutating tool: the operation is a best guess and nothing is usable.
    let (golden, result) = golden("xml_unmodeled_opaque_scope_boundary");
    let Outcome::BestGuess(calls) = &result.outcome else {
        panic!("expected a best guess, got {:#?}", result.outcome);
    };
    let call = &calls.calls[0];
    assert_eq!(
        call.arguments.to_json(),
        r#"{"keep":[{}],"path":"/home/user/important"}"#
    );
    assert_eq!(
        rule_count(call, "tool_call.markup.unmodeled-scope-boundary"),
        1
    );
    assert_eq!(rule_count(call, "tool_call.markup.stray-closer"), 1);
    assert_eq!(
        rule_count(call, "tool_call.markup.missing-parameter-close"),
        0
    );
    assert!(has_guess(call), "{call:#?}");
    assert_eq!(call.effects, Some(Effects::Mutating));
    assert_eq!(calls.verdict().code(), "nothing_usable");
    assert!(calls.usable().is_empty() && calls.usable_accepting_guesses().is_empty());
    // The default policy of a mutating tool (`Structural`) does not admit
    // the guessed rule; under `Tolerant` the call itself is a best guess,
    // and still not usable on a mutating tool.
    assert!(is_unresolved(&call.outcome), "{call:#?}");
    let tolerant = golden.tools.recover(
        &golden.input,
        &golden
            .request
            .clone()
            .policy(PolicySelection::All(RepairPolicy::Tolerant)),
    );
    let tolerant = the_call(&tolerant);
    assert_eq!(tolerant.outcome, CallOutcome::BestGuess, "{tolerant:#?}");
    assert_eq!(
        tolerant.arguments.to_json(),
        r#"{"keep":[{}],"path":"/home/user/important"}"#
    );
    assert!(!tolerant.is_usable_accepting_guesses());
}

#[test]
fn xml_array_nested_object_items_keep_their_own_spans() {
    // Each item's values point at its own occurrence, never at item 0.
    let (golden, result) = golden("xml_array_nested_object_items");
    let call = the_call(&result);
    for (pointer, text) in [
        ("/outfits/0/wearer", "Alice"),
        ("/outfits/1/wearer", "Bob"),
        ("/outfits/0/garments/0", "a red coat"),
        ("/outfits/1/garments/0", "a grey scarf"),
    ] {
        let span = call
            .arguments
            .pointer(pointer)
            .and_then(|node| node.span.clone())
            .unwrap_or_else(|| panic!("{pointer} has a span"));
        assert!(
            std::str::from_utf8(&golden.input[span.clone()])
                .unwrap()
                .contains(text),
            "{pointer}: {span:?}"
        );
    }
    let bob = call.arguments.pointer("/outfits/1/wearer").unwrap();
    let bob = bob.span.clone().unwrap();
    assert_eq!(
        std::str::from_utf8(&golden.input[bob]).unwrap().trim(),
        "Bob"
    );
    let alice = call.arguments.pointer("/outfits/0/wearer").unwrap();
    let alice = alice.span.clone().unwrap();
    assert_eq!(
        std::str::from_utf8(&golden.input[alice]).unwrap().trim(),
        "Alice"
    );
}

#[test]
fn xml_array_nested_object_item_missing_required_is_not_resolved() {
    // The item schema's `required: [wearer]` still binds a nested item.
    let (golden, _) = golden("xml_array_nested_object_items");
    let text = std::str::from_utf8(&golden.input).unwrap();
    let broken = text.replace("<parameter=wearer>\nBob\n</parameter>\n", "");
    assert_ne!(broken, text);
    let result = golden.tools.recover(broken.as_bytes(), &golden.request);
    for call in &reading(&result).calls {
        assert_ne!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    }
    assert_eq!(reading(&result).usable().len(), 0);
}

#[test]
fn xml_array_nested_object_item_fault_names_its_own_item() {
    // An undecodable value in the second occurrence is reported at
    // `/xs/1/n`, not at the first occurrence's `/xs/0/n`.
    let tools = ToolSet::compile(
        [ToolDefinition::new("f").param(
            "xs",
            Param::array(Param::object(
                ObjectSpec::new().param("n", Param::integer()),
            )),
        )],
        Budget::default(),
    )
    .unwrap();
    let input = "<tool_call>\n<function=f>\n<parameter=xs>\n<parameter=n>\n1\n</parameter>\n</parameter>\n<parameter=xs>\n<parameter=n>\n{{{ 1 2 zz\n</parameter>\n</parameter>\n</function>\n</tool_call>";
    let result = tools.recover(input.as_bytes(), &ToolRecoveryRequest::default());
    let call = the_call(&result);
    match &call.outcome {
        CallOutcome::Unresolved {
            reason: UnresolvedReason::InvalidArgument { pointer, .. },
        } => assert_eq!(pointer, "/xs/1/n", "{call:#?}"),
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
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

/// G6: with the tool known only after name resolution (`Nums` for `nums`),
/// each repeated scalar item's `decode-typed` evidence cites that item's own
/// bytes, found by span, not the first occurrence's (both share the source
/// pointer `/xs`).
#[test]
fn xml_repeated_scalar_items_near_miss_tool_spans() {
    let (golden, result) = golden("xml_repeated_scalar_items_near_miss_tool_spans");
    let call = the_call(&result);
    assert_eq!(call.arguments.to_json(), r#"{"xs":[11,2222]}"#, "{call:#?}");
    let input = std::str::from_utf8(&golden.input).expect("utf-8 input");
    let (first, last) = (input.find("11").unwrap(), input.find("2222").unwrap());
    assert_eq!((first, last), (43, 74), "the fixture's own offsets");
    let span = |destination: &str| -> Vec<(usize, usize)> {
        call.repairs
            .evidence
            .iter()
            .filter(|evidence| evidence.rule == "tool_call.value.decode-typed")
            .filter(|evidence| evidence.destination.ends_with(destination))
            .map(|evidence| {
                let span = evidence.span.as_ref().expect("a source span");
                (span.bytes.start, span.bytes.end)
            })
            .collect()
    };
    assert_eq!(span("/arguments/xs/0"), [(43, 45)]);
    assert_eq!(span("/arguments/xs/1"), [(74, 78)]);
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
    assert!(
        rejected
            .candidate_dialects
            .contains(rejected.dialect.as_ref().expect("a parser's attempt"))
    );
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
            0..90,
        ),
        (
            "json_body_stray_closer_trailing_text",
            DialectId::Qwen3CoderXml,
            0..111,
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
            (rejected.dialect.as_ref(), &rejected.span),
            (Some(&dialect), &reach),
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

/// T6: closer text a Qwen3-Coder XML value holds as paired markup
/// (`<parameter>v</parameter>`: the closer closes an element the value
/// opened). Only the reading that holds it parses cleanly, so the call is
/// `Resolved` with the full value, whatever the layout ("Newlines have zero
/// meaning here"), and records `tool_call.value.closer-in-value`, a
/// non-guess evidence entry whose span names both places the value could
/// end: from the first closer the value holds (where the other reading ends
/// it) to where it ends. (An unpaired closer followed by text, such as
/// `</function></tool_call> then stop.`, is `Ambiguous` instead: see
/// `qwen_text_after_unpaired_closer_is_ambiguous`.)
#[test]
fn qwen_closer_text_held_by_evidence() {
    // The content, the closer it holds, and the closer that ends the value.
    for (name, content, held, ending) in [
        (
            "qwen_value_holds_parameter_element",
            "x <parameter>v</parameter> y",
            "</parameter> y",
            "\n</parameter>\n</function>",
        ),
        (
            "qwen_unframed_value_holds_parameter_element",
            "x <parameter>v</parameter> y",
            "</parameter> y",
            "</parameter>\n</function>",
        ),
    ] {
        let (golden, result) = golden(name);
        let call = the_call(&result);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{name}: {call:#?}");
        assert_eq!(
            call.arguments.get("content").and_then(Node::as_str),
            Some(content),
            "{name}"
        );
        assert_eq!(reading(&result).usable().len(), 1, "{name}");
        let input = String::from_utf8(golden.input).expect("UTF-8 input");
        let evidence: Vec<_> = call
            .repairs
            .evidence
            .iter()
            .filter(|evidence| evidence.rule == "tool_call.value.closer-in-value")
            .collect();
        assert_eq!(evidence.len(), 1, "{name}: {call:#?}");
        let span = evidence[0].span.as_ref().expect("a span").bytes.clone();
        let start = input.find(held).expect("held closer");
        let end =
            input.rfind(ending).expect("ending closer") + usize::from(ending.starts_with('\n'));
        assert_eq!(span, start..end, "{name}");
        assert!(call.repairs.inferences.is_empty(), "{name}: {call:#?}");
    }
}

/// T6: a value holding a call's closers inline (`End with
/// </function></tool_call>`) and then left unclosed reads two equally clean
/// calls, so the turn is `Ambiguous` with both values: no single value to
/// run, on a mutating tool or any other. The call reading on past the
/// closers is itself `Ambiguous` (its value holds text after unpaired
/// closers), with both values.
#[test]
fn qwen_inline_call_closers_unclosed_are_ambiguous() {
    let (_, result) = golden("qwen_value_holds_call_closers_unclosed");
    let Outcome::Ambiguous(alternatives) = &result.outcome else {
        panic!("{:?}", result.outcome)
    };
    let mut contents: Vec<&str> = alternatives
        .iter()
        .flat_map(|calls| {
            let call = &calls.calls[0];
            core::iter::once(&call.arguments)
                .chain(call.alternatives.iter().map(|reading| &reading.arguments))
        })
        .map(|arguments| {
            arguments
                .get("content")
                .and_then(Node::as_str)
                .expect("a content value")
        })
        .collect();
    contents.sort_unstable();
    contents.dedup();
    assert_eq!(
        contents,
        ["End with ", "End with </function></tool_call> then stop."]
    );
    for calls in alternatives {
        let call = &calls.calls[0];
        let holds = core::iter::once(&call.arguments)
            .chain(call.alternatives.iter().map(|reading| &reading.arguments))
            .any(|arguments| {
                arguments
                    .get("content")
                    .and_then(Node::as_str)
                    .is_some_and(|content| content.contains("then stop"))
            });
        if holds {
            assert_eq!(call.outcome, CallOutcome::Ambiguous, "{call:#?}");
        }
    }
}

/// Adversary pass 6 (F14), maintainer: "Newlines have zero meaning here."
/// A verbatim value holding an unpaired closer (one closing no element the
/// value opened and no call it holds) followed by text before the closer
/// that ends it is `Ambiguous` between the reading holding the text
/// (`tool_call.value.text-after-closer`) and the reading ending the value
/// at that closer and dropping the text as text between parameters
/// (`tool_call.markup.text-between-parameters`), wherever the closer sits on
/// its line (mid-line, on a line of its own, indented, in a value not
/// framed by newlines) and whether it is a parameter or a call closer.
/// Never usable on the mutating tool (issue #3: "Never accept trailing free
/// text as an argument").
#[test]
fn qwen_text_after_unpaired_closer_is_ambiguous() {
    // The golden, the parameter, the value holding the text, and the value
    // ending at the first unpaired closer.
    for (name, parameter, held, first) in [
        (
            "qwen_same_line_closer_then_text_is_ambiguous",
            "a",
            "rm -rf build</parameter> Then I set b.",
            "rm -rf build",
        ),
        (
            "qwen_unframed_closer_then_text_is_ambiguous",
            "a",
            "rm -rf build</parameter> Then I set b.",
            "rm -rf build",
        ),
        (
            "qwen_same_line_closer_then_prose_is_ambiguous",
            "path",
            "a.py</parameter>\nI will now write the file.",
            "a.py",
        ),
        (
            "qwen_prose_after_value_closer_is_ambiguous",
            "path",
            "a.py\n</parameter>\nI will now write the file.",
            "a.py",
        ),
        (
            "qwen_indented_closer_then_text_is_ambiguous",
            "content",
            "print(1)\n </parameter>\nDone, that is the file.",
            "print(1)\n ",
        ),
        (
            "qwen_prose_after_last_value_closer_is_ambiguous",
            "content",
            "x\n</parameter>\nDone, the file is written.",
            "x",
        ),
        (
            "qwen_value_holds_text_between_closers",
            "content",
            "x\n</parameter>\nmore text",
            "x",
        ),
        (
            "qwen_value_holds_call_closers",
            "content",
            "End with </function></tool_call> then stop.",
            "End with ",
        ),
        (
            "qwen_bare_function_value_holds_function_closer",
            "content",
            "see </function> here",
            "see ",
        ),
    ] {
        let (_, result) = golden(name);
        let call = the_call(&result);
        assert_eq!(call.outcome, CallOutcome::Ambiguous, "{name}: {call:#?}");
        assert!(reading(&result).usable().is_empty(), "{name}");
        assert!(
            reading(&result).usable_accepting_guesses().is_empty(),
            "{name}"
        );
        let mut readings: Vec<(&str, Vec<&str>)> =
            core::iter::once((&call.arguments, &call.repairs))
                .chain(
                    call.alternatives
                        .iter()
                        .map(|reading| (&reading.arguments, &reading.repairs)),
                )
                .map(|(arguments, repairs)| {
                    let value = arguments
                        .get(parameter)
                        .and_then(Node::as_str)
                        .expect("the parameter");
                    let rules = repairs
                        .evidence
                        .iter()
                        .map(|evidence| evidence.rule.as_str())
                        .filter(|rule| {
                            rule.starts_with("tool_call.value.text")
                                || rule.ends_with("text-between-parameters")
                        })
                        .collect();
                    (value, rules)
                })
                .collect();
        readings.sort_unstable();
        let mut expected = vec![
            (held, vec!["tool_call.value.text-after-closer"]),
            (first, vec!["tool_call.markup.text-between-parameters"]),
        ];
        expected.sort_unstable();
        assert_eq!(readings, expected, "{name}");
    }
}

/// Adversary pass 6 (F19): the closers that complete a call written inline
/// after its value (`x</parameter></function></tool_call>`), then another
/// call's opener: two calls, never one call holding the other, both
/// `Resolved` (as at 72f34f7); the closers after the second call are its
/// stray closers.
#[test]
fn qwen_inline_closers_then_next_call_are_two_calls() {
    let (_, result) = golden("qwen_inline_closers_then_next_call_are_two_calls");
    let calls = reading(&result);
    let [first, second] = &calls.calls[..] else {
        panic!("{calls:#?}")
    };
    assert_eq!(first.tool.as_deref(), Some("write_file"));
    assert_eq!(
        first.arguments.to_json(),
        r#"{"path":"a.py","content":"x"}"#
    );
    assert_eq!(second.tool.as_deref(), Some("set"));
    assert_eq!(second.arguments.to_json(), r#"{"a":"rm"}"#);
    for call in [first, second] {
        assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
        assert!(call.alternatives.is_empty(), "{call:#?}");
    }
    let strays = second
        .repairs
        .evidence
        .iter()
        .filter(|evidence| evidence.rule == "tool_call.markup.stray-closer")
        .count();
    assert_eq!(strays, 3, "{second:#?}");
    assert_eq!(calls.usable().len(), 2);
}

/// Adversary pass 6 (F13 layout): a doubled `</parameter>` followed only by
/// whitespace before the next parameter is a stray closer whatever the
/// layout: `a.py</parameter></parameter>` and an indented second closer
/// read as `a.py\n</parameter>\n</parameter>` does, the value `a.py`,
/// `Resolved`.
#[test]
fn qwen_doubled_closer_is_stray_in_any_layout() {
    for name in [
        "qwen_doubled_closer_after_value_is_stray",
        "qwen_same_line_doubled_closer_is_stray",
        "qwen_indented_doubled_closer_is_stray",
    ] {
        let (_, result) = golden(name);
        let call = the_call(&result);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{name}: {call:#?}");
        assert_eq!(
            call.arguments.get("path").and_then(Node::as_str),
            Some("a.py"),
            "{name}"
        );
        let rules: Vec<&str> = call
            .repairs
            .evidence
            .iter()
            .map(|evidence| evidence.rule.as_str())
            .collect();
        assert!(
            rules.contains(&"tool_call.markup.stray-closer"),
            "{name}: {rules:?}"
        );
        assert!(
            !rules.contains(&"tool_call.value.closer-in-value"),
            "{name}: {rules:?}"
        );
    }
}

/// A value followed by a call's closers on their own lines, then the real
/// closers: the value ends at its first closer and the closers after the
/// call are stray (issue #3 §3: "repeated or stray closers are all one
/// class"), never closer text the value holds.
#[test]
fn qwen_closing_sequence_after_value_is_stray() {
    let (_, result) = golden("qwen_value_ends_with_closing_sequence");
    let calls = reading(&result);
    let [call] = &calls.calls[..] else {
        panic!("{calls:#?}")
    };
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.get("content").and_then(Node::as_str),
        Some("x")
    );
    let rules: Vec<&str> = call
        .repairs
        .evidence
        .iter()
        .map(|evidence| evidence.rule.as_str())
        .collect();
    assert!(
        rules.contains(&"tool_call.markup.stray-closer"),
        "{rules:?}"
    );
    assert!(
        !rules.contains(&"tool_call.value.closer-in-value"),
        "{rules:?}"
    );
    assert!(calls.content.is_empty(), "{:?}", calls.content);
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

/// Llama 3 JSON: the object's final brace is its closer, so a missing one
/// at the end of output, before the next call, or a repeated one is
/// repaired, and every repaired call is `Resolved`.
#[test]
fn llama3_closing_brace_repairs() {
    for (name, rule, count) in [
        (
            "llama3_missing_final_brace_at_end",
            "tool_call.markup.missing-closers-at-end",
            1,
        ),
        (
            "llama3_missing_brace_between_calls",
            "tool_call.markup.missing-wrapper-close",
            2,
        ),
        (
            "llama3_repeated_final_brace",
            "tool_call.markup.stray-closer",
            1,
        ),
        (
            "llama3_stringified_parameters",
            "tool_call.arguments.decode-string",
            1,
        ),
    ] {
        let (_, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), count, "{name}: {calls:#?}");
        assert_eq!(calls.finish, Finish::Calls, "{name}");
        assert!(calls.rejected.is_empty(), "{name}");
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.outcome == CallOutcome::Resolved)
        );
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.dialect == DialectId::Llama3Json)
        );
        assert!(rules_of(&calls.calls[0]).contains(&rule), "{name}");
        assert_eq!(calls.usable().len(), count, "{name}");
    }
}

/// Llama 3 JSON never hides content or invents a call: JSON without the
/// call-shaped `"parameters"` evidence stays content with no rejected
/// attempt, a built-in Python-syntax call after `<|python_tag|>` is a
/// rejected attempt, and a call cut off by a length stop is truncated.
#[test]
fn llama3_evidence_and_honesty() {
    let (golden_turn, result) = golden("llama3_json_content_not_a_call");
    let calls = reading(&result);
    assert!(calls.calls.is_empty() && calls.rejected.is_empty());
    assert_eq!(calls.finish, Finish::ContentOnly);
    assert_eq!(calls.content.len(), 1);
    assert_eq!(calls.content[0], 0..golden_turn.input.len());

    let (_, result) = golden("llama3_python_tag_builtin_call_rejected");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 0);
    assert_eq!(calls.rejected.len(), 1);
    assert_eq!(calls.rejected[0].dialect, Some(DialectId::Llama3Json));
    assert_eq!(calls.finish, Finish::Unresolved);

    let (_, result) = golden("llama3_truncated_arguments_length");
    let call = the_call(&result);
    assert_eq!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    assert!(!call.is_usable());
    assert_eq!(reading(&result).finish, Finish::Truncated);
}

/// Mistral v3: each element of a `[TOOL_CALLS]` array is its own call; a
/// missing or repeated closing bracket, an unclosed last object, and a
/// bare object without brackets are repaired or read with every call
/// `Resolved`, model ids kept as source ids.
#[test]
fn mistral_array_repairs() {
    for (name, rule, count) in [
        (
            "mistral_missing_array_close_at_end",
            Some("tool_call.markup.missing-closers-at-end"),
            2,
        ),
        (
            "mistral_missing_array_close_before_next",
            Some("tool_call.markup.missing-wrapper-close"),
            2,
        ),
        (
            "mistral_last_object_unclosed_at_stop",
            Some("tool_call.markup.missing-closers-at-end"),
            2,
        ),
        (
            "mistral_repeated_array_close",
            Some("tool_call.markup.stray-closer"),
            1,
        ),
        (
            "mistral_bare_object",
            Some("tool_call.markup.missing-token"),
            1,
        ),
        (
            "mistral_unbracketed_calls",
            Some("tool_call.markup.missing-token"),
            2,
        ),
        (
            "mistral_unbracketed_calls_by_line",
            Some("tool_call.markup.missing-token"),
            2,
        ),
        (
            "mistral_unbracketed_call_then_json_prose",
            Some("tool_call.markup.missing-token"),
            1,
        ),
    ] {
        let (_, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), count, "{name}: {calls:#?}");
        assert_eq!(calls.finish, Finish::Calls, "{name}");
        assert!(calls.rejected.is_empty(), "{name}");
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.outcome == CallOutcome::Resolved)
        );
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.dialect == DialectId::MistralV3)
        );
        if let Some(rule) = rule {
            assert!(
                calls
                    .calls
                    .iter()
                    .any(|call| rules_of(call).contains(&rule)),
                "{name}"
            );
        }
        assert_eq!(calls.usable().len(), count, "{name}");
    }
    let (_, result) = golden("mistral_missing_array_close_at_end");
    let ids: Vec<Option<&str>> = reading(&result)
        .calls
        .iter()
        .map(|call| call.source_id.as_deref())
        .collect();
    assert_eq!(ids, [Some("a1B2c3D4e"), Some("f5G6h7J8k")]);
    let (_, result) = golden("mistral_duplicate_model_ids");
    let calls = reading(&result);
    assert_eq!(calls.calls[0].id, "same12345");
    assert_ne!(calls.calls[1].id, "same12345");
    assert!(rules_of(&calls.calls[1]).contains(&"tool_call.id.dedup"));
    assert!(
        calls
            .calls
            .iter()
            .all(|call| call.outcome == CallOutcome::Resolved)
    );
}

/// A Mistral v3 array cut off inside its last object: the earlier calls
/// stay whole and usable, the last is truncated.
#[test]
fn mistral_truncated_last_object() {
    let (_, result) = golden("mistral_truncated_last_object");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 2);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[1].outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    assert_eq!(calls.usable().len(), 1);
    assert_eq!(calls.finish, Finish::TruncatedWithCalls);
}

/// The `[TOOL_CALLS]` token is the evidence: a Llama 3 object inside a
/// Mistral array is part of the Mistral call, never a competing reading.
#[test]
fn mistral_evidence_beats_llama3_object() {
    let (_, result) = golden("mistral_parameters_key_beats_llama3");
    assert!(matches!(result.outcome, Outcome::Resolved(_)));
    let call = the_call(&result);
    assert_eq!(call.dialect, DialectId::MistralV3);
    assert_eq!(call.candidate_dialects, [DialectId::MistralV3]);
    assert_eq!(call.source_id.as_deref(), Some("a1B2c3D4e"));
    assert_eq!(call.outcome, CallOutcome::Resolved);
}

/// Unbracketed Mistral v3 objects: every call-shaped object after a call
/// continues the attempt, and nothing else is absorbed. Text after the last
/// call, a plain JSON object included, stays content; a length stop inside
/// the second object truncates that call alone.
#[test]
fn mistral_unbracketed_calls_read_by_evidence() {
    let (golden_turn, result) = golden("mistral_unbracketed_call_then_json_prose");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 1);
    let content: Vec<&[u8]> = calls
        .content
        .iter()
        .map(|range| &golden_turn.input[range.clone()])
        .collect();
    assert_eq!(
        content,
        [&b"\nThe reply will look like {\"city\": \"Paris\", \"temp\": 21}."[..]]
    );
    let (golden_turn, result) = golden("mistral_unbracketed_calls_by_line");
    let calls = reading(&result);
    let content: Vec<&[u8]> = calls
        .content
        .iter()
        .map(|range| &golden_turn.input[range.clone()])
        .collect();
    assert_eq!(content, [&b"\nChecking both now."[..]]);
    let (_, result) = golden("mistral_unbracketed_calls");
    let ids: Vec<Option<&str>> = reading(&result)
        .calls
        .iter()
        .map(|call| call.source_id.as_deref())
        .collect();
    assert_eq!(ids, [Some("a1B2c3D4e"), Some("f5G6h7J8k")]);
    let (_, result) = golden("mistral_unbracketed_truncated_second");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 2);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[1].outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    assert_eq!(calls.finish, Finish::TruncatedWithCalls);
}

/// Mistral v11: an omitted `[ARGS]`, whitespace around the name, an
/// unclosed arguments object at a stop, and a second call whose
/// `[TOOL_CALLS]` is omitted are all read with every call `Resolved`; the
/// model's `[CALL_ID]` ids are kept; prose after a call is content.
#[test]
fn mistral11_repairs() {
    for (name, rule, count) in [
        (
            "mistral11_missing_args_token",
            Some("tool_call.markup.missing-token"),
            1,
        ),
        ("mistral11_whitespace_around_name", None, 1),
        (
            "mistral11_unclosed_arguments_at_stop",
            Some("json.syntax.close-container"),
            1,
        ),
        (
            "mistral11_second_token_missing",
            Some("tool_call.markup.missing-token"),
            2,
        ),
        ("mistral11_call_ids", None, 2),
        ("mistral11_text_after_call", None, 1),
    ] {
        let (_, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), count, "{name}: {calls:#?}");
        assert!(calls.rejected.is_empty(), "{name}");
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.outcome == CallOutcome::Resolved)
        );
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.dialect == DialectId::MistralV11)
        );
        if let Some(rule) = rule {
            assert!(
                calls
                    .calls
                    .iter()
                    .any(|call| rules_of(call).contains(&rule)),
                "{name}"
            );
        }
    }
    let (_, result) = golden("mistral11_call_ids");
    let ids: Vec<Option<&str>> = reading(&result)
        .calls
        .iter()
        .map(|call| call.source_id.as_deref())
        .collect();
    assert_eq!(ids, [Some("a1B2c3D4e"), Some("f5G6h7J8k")]);
    let (golden_turn, result) = golden("mistral11_text_after_call");
    let calls = reading(&result);
    assert_eq!(calls.content.len(), 1);
    assert!(golden_turn.input[calls.content[0].clone()].starts_with(b"Done."));
    let (_, result) = golden("mistral11_text_after_call_length");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(calls.finish, Finish::TruncatedWithCalls);
    let (_, result) = golden("mistral11_truncated_arguments");
    assert_eq!(
        the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
}

/// Mistral v3 and v11 share the `[TOOL_CALLS]` opener; each canonical call
/// is read by its own dialect alone.
#[test]
fn mistral_dialects_disambiguate_by_body() {
    let (_, result) = golden("mistral_v3_and_v11_canonical");
    assert!(matches!(result.outcome, Outcome::Resolved(_)));
    let calls = reading(&result);
    let dialects: Vec<(&DialectId, &[DialectId])> = calls
        .calls
        .iter()
        .map(|call| (&call.dialect, call.candidate_dialects.as_slice()))
        .collect();
    assert_eq!(
        dialects,
        [
            (&DialectId::MistralV3, &[DialectId::MistralV3][..]),
            (&DialectId::MistralV11, &[DialectId::MistralV11][..])
        ]
    );
    assert_eq!(calls.rejected.len(), 0);
}

/// A Mistral v11 call ends with its arguments' JSON value: under a length
/// stop, a call whose value is complete is a complete call (`Resolved`,
/// finish `truncated_with_calls`), and a call cut inside its value is
/// truncated.
#[test]
fn mistral11_length_stop_at_a_complete_call() {
    let (_, result) = golden("mistral11_complete_call_at_length");
    assert!(matches!(result.outcome, Outcome::Resolved(_)));
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 2);
    assert!(
        calls
            .calls
            .iter()
            .all(|call| call.outcome == CallOutcome::Resolved)
    );
    assert_eq!(calls.usable().len(), 2);
    assert_eq!(calls.finish, Finish::TruncatedWithCalls);
    let (_, result) = golden("mistral11_truncated_arguments");
    assert_eq!(
        the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    assert_eq!(reading(&result).finish, Finish::Truncated);
}

/// DeepSeek-V3.1: a missing section or call closer, an omitted section
/// opener, and tokens spelled in ASCII are read with every call
/// `Resolved` and the repair recorded; reasoning before the calls is
/// reasoning; a confusable opener with no call after it is a rejected
/// attempt, never silently content.
#[test]
fn deepseek31_repairs() {
    for (name, rule, count) in [
        (
            "deepseek31_missing_section_end",
            Some("tool_call.markup.missing-closers-at-end"),
            2,
        ),
        (
            "deepseek31_missing_call_end_between_calls",
            Some("tool_call.markup.missing-function-close"),
            2,
        ),
        (
            "deepseek31_missing_section_begin",
            Some("tool_call.markup.missing-token"),
            1,
        ),
        (
            "deepseek31_ascii_tokens",
            Some("tool_call.markup.terminal-confusable"),
            1,
        ),
        (
            "deepseek31_ascii_bar_space_tokens",
            Some("tool_call.markup.terminal-confusable"),
            1,
        ),
        ("deepseek31_reasoning_then_calls", None, 1),
        ("deepseek31_reasoning_starts_open", None, 1),
    ] {
        let (_, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), count, "{name}: {calls:#?}");
        assert_eq!(calls.rejected.len(), 0, "{name}");
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.outcome == CallOutcome::Resolved)
        );
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.dialect == DialectId::DeepSeekV31)
        );
        if let Some(rule) = rule {
            assert!(
                calls
                    .calls
                    .iter()
                    .any(|call| rules_of(call).contains(&rule)),
                "{name}"
            );
        }
    }
    for name in [
        "deepseek31_reasoning_then_calls",
        "deepseek31_reasoning_starts_open",
    ] {
        let (_, result) = golden(name);
        let calls = reading(&result);
        assert_eq!(calls.reasoning.len(), 1, "{name}");
        assert!(calls.content.is_empty(), "{name}: {:?}", calls.content);
    }
    let (_, result) = golden("deepseek31_truncated_arguments");
    assert_eq!(
        the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    let (_, result) = golden("deepseek31_confusable_opener_rejected");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 0);
    assert_eq!(calls.rejected.len(), 1);
    assert_eq!(calls.rejected[0].dialect, Some(DialectId::DeepSeekV31));
    assert_eq!(calls.finish, Finish::Unresolved);
}

/// DeepSeek-V3 / R1: a missing, bare, foreign-language or repeated fence
/// line, a missing type, a missing section or call closer, an omitted
/// section opener, and tokens spelled in ASCII are read with every call
/// `Resolved` and each repair recorded.
#[test]
fn deepseek3_repairs() {
    for (name, rules, count) in [
        (
            "deepseek3_missing_open_fence",
            &["tool_call.markup.missing-token"][..],
            1,
        ),
        (
            "deepseek3_fence_without_info",
            &["tool_call.markup.missing-token"],
            1,
        ),
        (
            "deepseek3_missing_close_fence",
            &["tool_call.markup.missing-token"],
            1,
        ),
        (
            "deepseek3_unfenced_arguments",
            &["tool_call.markup.missing-token"],
            1,
        ),
        (
            "deepseek3_repeated_fence_lines",
            &["tool_call.extract.fence"],
            1,
        ),
        (
            "deepseek3_other_info_string",
            &["tool_call.extract.fence", "tool_call.markup.missing-token"],
            1,
        ),
        (
            "deepseek3_missing_type",
            &["tool_call.markup.missing-token"],
            1,
        ),
        (
            "deepseek3_missing_section_end",
            &["tool_call.markup.missing-closers-at-end"],
            2,
        ),
        (
            "deepseek3_missing_call_end_between_calls",
            &["tool_call.markup.missing-function-close"],
            2,
        ),
        (
            "deepseek3_missing_section_begin",
            &["tool_call.markup.missing-token"],
            1,
        ),
        (
            "deepseek3_ascii_tokens",
            &["tool_call.markup.terminal-confusable"],
            1,
        ),
        ("deepseek3_r1_reasoning_then_calls", &[], 2),
        ("deepseek3_reasoning_starts_open", &[], 1),
    ] {
        let (_, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), count, "{name}: {calls:#?}");
        assert_eq!(calls.rejected.len(), 0, "{name}");
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.outcome == CallOutcome::Resolved
                    && call.dialect == DialectId::DeepSeekV3),
            "{name}"
        );
        for rule in rules {
            assert!(
                calls.calls.iter().any(|call| rules_of(call).contains(rule)),
                "{name}: {rule}"
            );
        }
    }
}

/// DeepSeek-V3 / R1: reasoning before the calls is reasoning; a type other
/// than `function` is a data loss (a best guess); a length stop inside the
/// fenced arguments truncates the call.
#[test]
fn deepseek3_reasoning_type_and_truncation() {
    for name in [
        "deepseek3_r1_reasoning_then_calls",
        "deepseek3_reasoning_starts_open",
    ] {
        let (_, result) = golden(name);
        let calls = reading(&result);
        assert_eq!(calls.reasoning.len(), 1, "{name}");
    }
    // A type other than `function` is dropped as a data loss: the call is
    // read as a function call, a best guess.
    let (_, result) = golden("deepseek3_type_not_function");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::BestGuess);
    assert_eq!(
        loss_kinds(call, "tool_call.extract.prose"),
        [LossKind::Data]
    );
    let (_, result) = golden("deepseek3_truncated_arguments");
    assert_eq!(
        the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    assert_eq!(reading(&result).finish, Finish::Truncated);
}

/// DeepSeek-V3 and V3.1 share their section and call tokens: with every
/// built-in dialect allowed, each canonical body resolves as its own dialect
/// alone, in one turn, and a V3.1 call to a tool named `function` is never
/// read as V3.
#[test]
fn deepseek3_and_v31_disambiguate() {
    let (_, result) = golden("deepseek3_and_v31_in_one_turn");
    assert!(matches!(result.outcome, Outcome::Resolved(_)));
    let calls = reading(&result);
    let read: Vec<_> = calls
        .calls
        .iter()
        .map(|call| {
            (
                call.name.as_deref(),
                call.dialect.id(),
                call.candidate_dialects.len(),
                call.outcome.clone(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (
                Some("get_weather"),
                "deepseek-v3.v1",
                1,
                CallOutcome::Resolved
            ),
            (
                Some("read_file"),
                "deepseek-v3.1.v1",
                1,
                CallOutcome::Resolved
            ),
        ]
    );
    assert_eq!(calls.rejected.len(), 0);
    let (_, result) = golden("deepseek31_tool_named_function");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.dialect, DialectId::DeepSeekV31);
    assert_eq!(call.candidate_dialects, [DialectId::DeepSeekV31]);
}

/// A DeepSeek-V3 call cut mid-JSON under a stop: with every dialect
/// allowed, the DeepSeek-V3 reading (which names the declared `read_file`)
/// beats DeepSeek-V3.1's reading of the type word `function` as a tool
/// name, with every repair recorded; with only DeepSeek-V3.1 allowed, its
/// own reading is all there is, and nothing is executable.
#[test]
fn deepseek3_unclosed_arguments_at_stop() {
    let (_, result) = golden("deepseek3_unclosed_arguments_at_stop");
    let call = the_call(&result);
    assert_eq!(call.dialect, DialectId::DeepSeekV3);
    assert_eq!(call.tool.as_deref(), Some("read_file"));
    assert_eq!(call.arguments.to_json(), r#"{"path":"src/ma"}"#);
    for rule in [
        "json.syntax.close-container",
        "tool_call.markup.missing-token",
        "tool_call.markup.missing-closers-at-end",
    ] {
        assert!(rules_of(call).contains(&rule), "{rule}");
    }
    assert!(matches!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Malformed { .. }
        }
    ));
    let (_, result) = golden("deepseek3_unclosed_arguments_at_stop_v31_only");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].dialect, DialectId::DeepSeekV31);
    assert_eq!(calls.calls[0].tool, None);
    assert!(is_unresolved(&calls.calls[0].outcome));
    assert_eq!(calls.usable().len(), 0);
}

/// Kimi-K2: missing section and call closers, an omitted section opener,
/// the singular section-token spellings, a missing `:IDX`, unprefixed and
/// dotted ids, duplicate ids and Thinking reasoning are read with every
/// call `Resolved` and each repair or inference recorded; each well-formed
/// `functions.NAME:IDX` is the call's model id; a name ends at the id's final
/// `:IDX`.
#[test]
fn kimi_repairs() {
    for (name, rule, count) in [
        (
            "kimi_missing_section_end",
            Some("tool_call.markup.missing-closers-at-end"),
            2,
        ),
        (
            "kimi_missing_call_end_between_calls",
            Some("tool_call.markup.missing-function-close"),
            2,
        ),
        (
            "kimi_missing_section_begin",
            Some("tool_call.markup.missing-token"),
            1,
        ),
        (
            "kimi_singular_section_tokens",
            Some("tool_call.markup.terminal-confusable"),
            1,
        ),
        ("kimi_missing_call_index", Some("tool_call.id.dedup"), 1),
        ("kimi_duplicate_ids", Some("tool_call.id.dedup"), 2),
        ("kimi_thinking_reasoning_then_calls", None, 2),
        ("kimi_unprefixed_and_dotted_ids", None, 2),
    ] {
        let (_, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), count, "{name}: {calls:#?}");
        assert_eq!(calls.rejected.len(), 0, "{name}");
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.outcome == CallOutcome::Resolved
                    && call.dialect == DialectId::KimiK2),
            "{name}"
        );
        if let Some(rule) = rule {
            assert!(
                calls.calls.iter().any(|call| rules_of(call).contains(&rule)
                    || call
                        .repairs
                        .inferences
                        .iter()
                        .any(|inference| inference.evidence.rule == rule)),
                "{name}: {rule}"
            );
        }
    }
    let (_, result) = golden("kimi_unprefixed_and_dotted_ids");
    let ids: Vec<_> = reading(&result)
        .calls
        .iter()
        .map(|call| (call.tool.as_deref(), call.source_id.as_deref()))
        .collect();
    assert_eq!(
        ids,
        [
            (Some("mcp.search"), Some("functions.mcp.search:0")),
            (Some("read_file"), Some("read_file:1")),
        ]
    );
    let (_, result) = golden("kimi_missing_call_index");
    assert_eq!(the_call(&result).source_id, None);
    let (_, result) = golden("kimi_thinking_reasoning_then_calls");
    assert_eq!(reading(&result).reasoning.len(), 1);
    let (_, result) = golden("kimi_truncated_arguments");
    assert_eq!(
        the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    // A name may hold colons: it ends at the id's final `:IDX`.
    let (_, result) = golden("kimi_colon_name_declared");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.tool.as_deref(), Some("ns:tool"));
    assert_eq!(call.source_id.as_deref(), Some("functions.ns:tool:0"));
    // An undeclared colon name is an unknown tool, never coerced to a
    // declared prefix of it.
    let (_, result) = golden("kimi_colon_name_undeclared");
    let call = the_call(&result);
    assert_eq!(call.tool, None);
    assert!(matches!(
        &call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::UnknownTool { name }
        } if name == "ns:other"
    ));
}

/// `tool_call.name.from-arguments`: a call that names no tool (a Kimi-K2
/// bare call counter, a Hermes object without `name`) is read as the one
/// declared tool its arguments fit, a guess; several fitting tools make it
/// `Ambiguous`, none leave it `Unresolved`; a mutating tool's guess is never
/// usable, and `Structural` does not admit the rule.
#[test]
fn name_from_arguments() {
    const RULE: &str = "tool_call.name.from-arguments";
    for (name, tool) in [
        ("name_from_arguments_unique", "get_weather"),
        ("hermes_nameless_arguments", "read_file"),
    ] {
        let (_, result) = golden(name);
        let call = the_call(&result);
        assert_eq!(call.outcome, CallOutcome::BestGuess, "{name}");
        assert_eq!(call.tool.as_deref(), Some(tool), "{name}");
        assert!(call.is_usable_accepting_guesses(), "{name}");
        assert!(
            call.repairs
                .inferences
                .iter()
                .any(|inference| inference.evidence.rule == RULE),
            "{name}"
        );
    }
    let (_, result) = golden("name_from_arguments_ambiguous");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Ambiguous);
    let mut tools: Vec<&str> = core::iter::once(call.tool.as_deref())
        .chain(
            call.alternatives
                .iter()
                .map(|alternative| alternative.tool.as_deref()),
        )
        .flatten()
        .collect();
    tools.sort_unstable();
    assert_eq!(tools, ["list_dir", "read_file"]);
    let (_, result) = golden("name_from_arguments_none");
    assert_eq!(
        the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::MissingName
        }
    );
    let (_, result) = golden("name_from_arguments_mutating_guard");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::BestGuess);
    assert_eq!(call.tool.as_deref(), Some("write_file"));
    assert!(!call.is_usable_accepting_guesses());
    assert_eq!(reading(&result).usable_accepting_guesses().len(), 0);
    let (_, result) = golden("name_from_arguments_structural");
    assert!(matches!(
        &the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::RuleNotAdmitted { rule, .. }
        } if rule == RULE
    ));
}

/// Anthropic `tool_use` blocks: in an assistant message or a whole API
/// response (`thinking` and `redacted_thinking` blocks are reasoning), with
/// their members reordered, without their type but with a `toolu_` id,
/// with `"arguments"` for `"input"`, with stringified
/// input, a missing closing brace, or an unclosed envelope, each resolve as
/// `anthropic-tool-use.v1` alone with every repair recorded.
#[test]
fn anthropic_tool_use() {
    for (name, rule, count) in [
        ("anthropic_message_all_dialects", None, 2),
        ("anthropic_response_with_thinking", None, 1),
        ("anthropic_redacted_thinking", None, 1),
        ("anthropic_members_reordered", None, 1),
        ("anthropic_caller_metadata", None, 1),
        ("anthropic_untyped_toolu_id", None, 1),
        (
            "anthropic_arguments_key",
            Some("tool_call.markup.terminal-confusable"),
            1,
        ),
        (
            "anthropic_missing_block_brace",
            Some("tool_call.markup.missing-closers-at-end"),
            1,
        ),
        ("anthropic_envelope_unclosed", None, 1),
        (
            "anthropic_stringified_input",
            Some("tool_call.arguments.decode-string"),
            1,
        ),
    ] {
        let (_, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), count, "{name}: {calls:#?}");
        assert_eq!(calls.rejected.len(), 0, "{name}");
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.outcome == CallOutcome::Resolved
                    && call.candidate_dialects == [DialectId::AnthropicToolUse]),
            "{name}"
        );
        if let Some(rule) = rule {
            assert!(
                calls
                    .calls
                    .iter()
                    .any(|call| rules_of(call).contains(&rule)),
                "{name}: {rule}"
            );
        }
    }
    for name in [
        "anthropic_response_with_thinking",
        "anthropic_redacted_thinking",
    ] {
        let (_, result) = golden(name);
        assert_eq!(reading(&result).reasoning.len(), 1, "{name}");
    }
    let (_, result) = golden("anthropic_message_all_dialects");
    let ids: Vec<_> = reading(&result)
        .calls
        .iter()
        .map(|call| call.source_id.as_deref())
        .collect();
    assert_eq!(ids, [Some("toolu_01"), Some("toolu_02")]);
    // A confusable key before the block's evidence is not read as input;
    // Llama 3 JSON reads the object, its trailing id the source id.
    let (_, result) = golden("llama3_trailing_id_member");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.dialect, DialectId::Llama3Json);
    assert_eq!(call.source_id.as_deref(), Some("toolu_01"));
    let (_, result) = golden("llama3_id_first_parameters");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.candidate_dialects, [DialectId::Llama3Json]);
    assert_eq!(call.source_id.as_deref(), Some("toolu_01"));
    let (_, result) = golden("llama3_leading_id_member");
    assert_eq!(the_call(&result).source_id.as_deref(), Some("call_7"));
    // A block inside a Hermes wrapper is one Hermes call, its "input" key
    // recorded as a confusable spelling.
    let (_, result) = golden("anthropic_inside_hermes_wrapper");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.candidate_dialects, [DialectId::Hermes]);
    assert_eq!(call.source_id.as_deref(), Some("toolu_01"));
    assert!(rules_of(call).contains(&"tool_call.markup.terminal-confusable"));
    let (_, result) = golden("anthropic_untyped_without_evidence");
    let calls = reading(&result);
    assert_eq!((calls.calls.len(), calls.rejected.len()), (0, 0));
    let (_, result) = golden("anthropic_truncated_input");
    assert_eq!(
        the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    // Streaming events are rejected attempts, never calls or silent
    // content.
    let (_, result) = golden("anthropic_stream_events_rejected");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 0);
    assert_eq!(calls.rejected.len(), 2);
    assert_eq!(calls.finish, Finish::Unresolved);
}

/// A redacted-thinking block's text is the opaque encrypted payload, not
/// reasoning prose.
#[test]
fn anthropic_redacted_thinking_text_is_the_opaque_payload() {
    let (golden, result) = golden("anthropic_redacted_thinking");
    let blocks = &reading(&result).reasoning_blocks;
    assert_eq!(blocks.len(), 1);
    let texts: Vec<&[u8]> = blocks
        .iter()
        .map(|block| &golden.input[block.text.clone()])
        .collect();
    assert_eq!(texts, [b"EmwKAhgB".as_slice()]);
    assert_eq!(
        golden
            .node
            .get("reasoning_text")
            .map(|node| strings_of(Some(node))),
        Some(vec!["EmwKAhgB".to_owned()])
    );
}

/// A closer that begins inside the opening marker (the final quote of the
/// spaced opener begins `"}`) does not close the block: the block runs
/// unterminated, and its text is never a reversed range.
///
/// The input is 41 bytes. The opener `{"type": "redacted_thinking",
/// "data": "` is bytes 0..39, so the text begins at 39 with the `}`; the
/// only `"}` is at 38..40, before the text, and is rejected.
#[test]
fn anthropic_redacted_thinking_closer_overlaps_opener() {
    let (golden, result) = golden("anthropic_redacted_thinking_closer_overlaps_opener");
    assert_eq!(golden.input.len(), 41);
    let calls = reading(&result);
    assert_eq!(calls.reasoning_blocks.len(), 1);
    let block = &calls.reasoning_blocks[0];
    assert_eq!(block.span, 0..41);
    assert_eq!(block.text, 39..41);
    assert_eq!(&golden.input[block.text.clone()], b"}x");
    assert!(ledger_rules(&result).contains(&"tool_call.reasoning.unterminated".to_owned()));
    assert_eq!(
        golden
            .node
            .get("reasoning_text")
            .map(|node| strings_of(Some(node))),
        Some(vec!["}x".to_owned()])
    );
    // A block the turn starts in has no opener to overlap: the same
    // input is recovered with ordered ranges.
    let request = ToolRecoveryRequest::sized_for(
        Effort::Standard,
        &golden.input,
        dialect_set(&golden.node, &golden.path),
    )
    .reasoning(ReasoningPolicy::Separate { starts_open: true });
    let open = golden.tools.recover(&golden.input, &request);
    for block in &reading(&open).reasoning_blocks {
        assert!(block.text.start <= block.text.end, "{block:?}");
        assert!(
            block.span.start <= block.text.start && block.text.end <= block.span.end,
            "{block:?}"
        );
        let _ = &golden.input[block.text.clone()];
    }
}

/// Every reasoning block of every golden, under the pinned and the
/// default allow-list runs, slices its input by `text` and `span`, and
/// its text lies inside its span.
#[test]
fn reasoning_block_ranges_slice_over_every_golden() {
    let mut checked = 0;
    for (golden, result) in all_goldens().into_iter().chain(all_default_runs()) {
        let readings: Vec<&ToolCalls> = match &result.outcome {
            Outcome::Resolved(calls) | Outcome::BestGuess(calls) => vec![calls],
            Outcome::Ambiguous(readings) => readings.iter().collect(),
            Outcome::Unresolved => Vec::new(),
        };
        for calls in readings {
            for block in &calls.reasoning_blocks {
                let name = golden.path.display();
                assert!(block.text.start <= block.text.end, "{name}: {block:?}");
                assert!(
                    block.span.start <= block.text.start && block.text.end <= block.span.end,
                    "{name}: {block:?}"
                );
                assert!(block.span.end <= golden.input.len(), "{name}: {block:?}");
                let _ = &golden.input[block.span.clone()];
                let _ = &golden.input[block.text.clone()];
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no golden holds a reasoning block");
}

/// Harmony (gpt-oss): a call message in the documented and renderer
/// layouts, with or without `<|start|>assistant`, after an analysis message
/// (reasoning) or a commentary preamble (content), and its repairs; built-in
/// recipients are calls (to a declared tool of that name, or Unresolved),
/// never silent content.
#[test]
fn harmony_messages() {
    for (name, rule, count) in [
        ("harmony_analysis_then_call", None, 1),
        ("harmony_recipient_in_role", None, 1),
        ("harmony_without_start", None, 1),
        ("harmony_preamble_then_call", None, 1),
        ("harmony_two_messages", None, 2),
        ("harmony_builtin_browser_declared", None, 1),
        (
            "harmony_end_for_call",
            Some("tool_call.markup.closer-variant"),
            1,
        ),
        (
            "harmony_missing_call_at_end",
            Some("tool_call.markup.missing-closers-at-end"),
            1,
        ),
        (
            "harmony_channel_spelling",
            Some("tool_call.markup.terminal-confusable"),
            1,
        ),
        (
            "harmony_recipient_without_namespace",
            Some("tool_call.markup.missing-token"),
            1,
        ),
    ] {
        let (_, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), count, "{name}: {calls:#?}");
        assert_eq!(calls.rejected.len(), 0, "{name}");
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.outcome == CallOutcome::Resolved
                    && call.candidate_dialects == [DialectId::Harmony]),
            "{name}"
        );
        if let Some(rule) = rule {
            assert!(
                calls
                    .calls
                    .iter()
                    .any(|call| rules_of(call).contains(&rule)),
                "{name}: {rule}"
            );
        }
    }
    let (_, result) = golden("harmony_analysis_then_call");
    assert_eq!(reading(&result).reasoning.len(), 1);
    let (_, result) = golden("harmony_final_answer_is_content");
    let calls = reading(&result);
    assert_eq!((calls.calls.len(), calls.rejected.len()), (0, 0));
    assert_eq!(calls.reasoning.len(), 1);
    for name in ["harmony_recipient_after_message", "harmony_trailing_text"] {
        let (_, result) = golden(name);
        let call = the_call(&result);
        assert_eq!(call.outcome, CallOutcome::BestGuess, "{name}");
        assert_eq!(
            loss_kinds(call, "tool_call.extract.prose"),
            [LossKind::Data],
            "{name}"
        );
    }
    let (_, result) = golden("harmony_truncated_arguments");
    assert_eq!(
        the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    for name in [
        "harmony_builtin_browser_undeclared",
        "harmony_builtin_python",
    ] {
        let (_, result) = golden(name);
        assert!(
            matches!(
                the_call(&result).outcome,
                CallOutcome::Unresolved {
                    reason: UnresolvedReason::UnknownTool { .. }
                }
            ),
            "{name}"
        );
    }
}

/// Bare JSON: an unwrapped call object is recovered alone, in prose, in a
/// fence (a guess), in an array, with its members in either order, with a
/// trailing id and with stringified arguments; a call object inside
/// another dialect's wrapper is that dialect's alone. JSON content that is
/// not call-shaped (a record with a name, a nameless arguments object, a
/// non-object arguments value, an extra member, an object cut short) and
/// declaration-shaped objects (a pasted declaration, a JSON Schema as the
/// arguments of a tool that does not take it, in bare JSON or untagged
/// Llama 3 JSON) stay content: no call and no rejected attempt.
#[test]
fn bare_json_calls() {
    for (name, count) in [
        ("bare_json_whole_turn", 1),
        ("bare_json_call_in_prose", 1),
        ("bare_json_array_of_calls", 2),
        ("bare_json_arguments_first_with_id", 1),
        ("bare_json_stringified_arguments", 1),
        ("bare_json_schema_for_schema_tool", 1),
    ] {
        let (_, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), count, "{name}: {calls:#?}");
        assert_eq!(calls.rejected.len(), 0, "{name}");
        assert!(
            calls
                .calls
                .iter()
                .all(|call| call.outcome == CallOutcome::Resolved
                    && call.candidate_dialects == [DialectId::BareJson]),
            "{name}"
        );
    }
    let (_, result) = golden("bare_json_call_in_prose");
    assert_eq!(reading(&result).content.len(), 2);
    let (_, result) = golden("bare_json_arguments_first_with_id");
    assert_eq!(the_call(&result).id, "call_42");
    assert_eq!(the_call(&result).source_id.as_deref(), Some("call_42"));
    let (_, result) = golden("bare_json_stringified_arguments");
    assert!(rules_of(the_call(&result)).contains(&"tool_call.arguments.decode-string"));
    let (_, result) = golden("bare_json_fenced_call");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::BestGuess);
    assert!(rules_of(call).contains(&"tool_call.context.fenced-example"));
    let (_, result) = golden("bare_json_in_hermes_wrapper");
    assert_eq!(the_call(&result).candidate_dialects, [DialectId::Hermes]);
    for name in [
        "bare_json_answer_object_with_name",
        "bare_json_extra_member_is_content",
        "bare_json_truncated_object_is_content",
        "bare_json_schema_is_content",
        "bare_json_pasted_declaration_is_content",
        "llama3_declaration_is_content",
    ] {
        let (golden, result) = golden(name);
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
        let calls = reading(&result);
        assert_eq!(
            (calls.calls.len(), calls.rejected.len()),
            (0, 0),
            "{name}: {calls:#?}"
        );
        // Every byte is content.
        let covered: usize = calls.content.iter().map(ExactSizeIterator::len).sum();
        assert_eq!(covered, golden.input.len(), "{name}");
    }
    // After <|python_tag|> the object is a call whatever its arguments.
    let (_, result) = golden("llama3_python_tag_declaration_shaped_call");
    assert!(matches!(
        the_call(&result).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::MissingRequired { .. }
        }
    ));
}

/// Call markup no allowed dialect reads is never silent content: each
/// family not built in yet, and each built-in dialect left out of the
/// allow-list, is one rejected attempt with
/// `Rejection::UnsupportedDialect` naming the family, whose bytes stay
/// content, and the turn finishes `Unresolved`. Text that is not call
/// markup (prose with parentheses, math, a Python code block) is never
/// reported.
#[test]
fn unparsed_markup_is_reported() {
    use purrassist_core::tool_call::{MarkupFamily, Rejection};
    for (name, family) in [
        ("unparsed_pythonic_list", MarkupFamily::Pythonic),
        ("unparsed_pythonic_after_prose", MarkupFamily::Pythonic),
        ("unparsed_gemma3_tool_code", MarkupFamily::Gemma3ToolCode),
        ("unparsed_glm45", MarkupFamily::Glm45),
        ("unparsed_kimi_k2_stripped", MarkupFamily::KimiK2Stripped),
        ("unparsed_harmony_stripped", MarkupFamily::HarmonyStripped),
        (
            "unparsed_deepseek_v3_stripped",
            MarkupFamily::DeepSeekV3Stripped,
        ),
        (
            "unparsed_name_arguments_stripped",
            MarkupFamily::NameArgumentsStripped,
        ),
        ("unparsed_mistral_v3_not_allowed", MarkupFamily::MistralV3),
        ("unparsed_harmony_not_allowed", MarkupFamily::Harmony),
        (
            "unparsed_anthropic_not_allowed",
            MarkupFamily::AnthropicToolUse,
        ),
        ("unparsed_hermes_not_allowed", MarkupFamily::Hermes),
        ("unparsed_kimi_not_allowed", MarkupFamily::KimiK2),
    ] {
        let (_, result) = golden(name);
        let calls = reading(&result);
        assert!(calls.calls.is_empty(), "{name}");
        assert_eq!(calls.finish, Finish::Unresolved, "{name}");
        let reported: Vec<_> = calls
            .rejected
            .iter()
            .filter(|rejected| rejected.reason == Rejection::UnsupportedDialect { family })
            .collect();
        assert_eq!(reported.len(), 1, "{name}: {:#?}", calls.rejected);
        let rejected = reported[0];
        assert_eq!(rejected.dialect, family.dialect(), "{name}");
        assert!(rejected.candidate_dialects.is_empty(), "{name}");
        assert_eq!(family.built_in(), name.ends_with("_not_allowed"), "{name}");
        // The bytes stay content.
        assert!(
            calls
                .content
                .iter()
                .any(|range| range.start <= rejected.span.start && rejected.span.end <= range.end),
            "{name}"
        );
        assert_eq!(
            rejected.repairs.evidence[0].rule, "tool_call.markup.unparsed-dialect",
            "{name}"
        );
    }
    // The GLM <tool_call> opener is Hermes's and Qwen3-Coder XML's rejected
    // attempt; only the body after it is reported.
    let (_, result) = golden("unparsed_glm45");
    let calls = reading(&result);
    assert_eq!(calls.rejected.len(), 2);
    assert!(calls.rejected[0].span.end <= calls.rejected[1].span.start);
    let (_, result) = golden("unparsed_negatives_stay_content");
    let calls = reading(&result);
    assert_eq!((calls.calls.len(), calls.rejected.len()), (0, 0));
    assert_eq!(calls.finish, Finish::ContentOnly);
}

/// A repeated non-repeatable parameter is never a silent last-wins: with
/// every value valid the call is `Ambiguous` with both readings and the
/// primary labelled as merely first-ranked; when the declared enumeration
/// accepts only one value, that reading is chosen on evidence (a
/// `BestGuess`, never `Resolved`), and the choice cites the blocked
/// alternative. An array parameter still collects its occurrences.
#[test]
fn repeated_scalar_choice_is_labelled() {
    let choice = |call: &ToolCall| {
        call.repairs
            .inferences
            .iter()
            .find(|inference| inference.evidence.rule == "tool_call.parameter.repeated-choice")
            .map(|inference| inference.evidence.explanation.clone())
            .expect("a recorded choice")
    };
    let (_, result) = golden("repeated_scalar_both_readings");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Ambiguous);
    assert_eq!(call.alternatives.len(), 1);
    assert_ne!(call.alternatives[0].arguments, call.arguments);
    assert!(choice(call).contains("first-ranked"), "{}", choice(call));
    let (_, result) = golden("repeated_scalar_enum_one_valid");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::BestGuess);
    assert_eq!(
        call.arguments.get("unit").and_then(Node::as_str),
        Some("celsius")
    );
    assert!(choice(call).contains("on evidence"), "{}", choice(call));
    assert!(choice(call).contains("blocked"), "{}", choice(call));
    let (_, result) = golden("repeated_array_param_is_array");
    let call = the_call(&result);
    assert!(rules_of(call).contains(&"tool_call.parameter.collect"));
    assert!(!rules_of(call).contains(&"tool_call.parameter.repeated-choice"));
}

/// A bare `<function=` element is Qwen3-Coder XML markup whether or not it
/// is complete: with the dialect allowed it is a call attempt (read or
/// rejected), and with the dialect left out of the allow-list both `text
/// <function=f>` and `<function=f` are reported as `qwen3-coder-xml.v1`.
#[test]
fn bare_function_markup_is_attempted_or_reported() {
    use purrassist_core::tool_call::{MarkupFamily, detect_unparsed};
    let mut governor = Governor::new(Budget::default());
    let hermes = DialectSet::new([DialectId::Hermes], &mut governor).unwrap();
    for input in ["text <function=f>", "<function=f"] {
        let whole = 0..input.len();
        let found = detect_unparsed(
            input.as_bytes(),
            core::slice::from_ref(&whole),
            &hermes,
            &mut governor,
        )
        .unwrap();
        assert_eq!(found.len(), 1, "{input:?}");
        assert_eq!(found[0].family, MarkupFamily::Qwen3CoderXml, "{input:?}");
        // Allowed, it is the dialect's own attempt, never content.
        let tools = ToolSet::compile([ToolDefinition::new("f")], Budget::default()).unwrap();
        let result = tools.recover(input.as_bytes(), &ToolRecoveryRequest::default());
        let calls = reading(&result);
        assert!(
            !calls.calls.is_empty() || !calls.rejected.is_empty(),
            "{input:?}: {calls:#?}"
        );
        assert!(
            calls
                .calls
                .iter()
                .map(|call| &call.dialect)
                .chain(
                    calls
                        .rejected
                        .iter()
                        .filter_map(|rejected| rejected.dialect.as_ref())
                )
                .all(|dialect| *dialect == DialectId::Qwen3CoderXml),
            "{input:?}"
        );
    }
}

/// F3 envelope tolerance: every JSON-envelope dialect (Hermes, bare JSON,
/// Mistral v3, Anthropic, Llama 3 JSON, `OpenAI`) reads a call whose
/// envelope carries one JSON syntax slip (a trailing comma, single quotes,
/// bare keys, a missing comma or colon, a comment, a raw control character
/// beside the name's quotes) under the `json.syntax.*` rule the json codec
/// records for the same slip inside arguments. The repair is a non-guess
/// single reading with no data loss, so the call is `Resolved` exactly as
/// with the slip inside its arguments, on a `Mutating` tool too (demotion
/// parity). The name read is the declared name itself, never a near miss.
/// JSON-like prose stays content: no call and no rejected attempt.
#[test]
fn envelope_json_syntax_is_tolerated() {
    let classes = [
        ("trailing_comma", "json.syntax.trailing-comma"),
        ("single_quotes", "json.syntax.string"),
        ("bare_keys", "json.syntax.bare-key"),
        ("missing_comma", "json.syntax.missing-comma"),
        ("missing_colon", "json.syntax.missing-colon"),
        ("comment", "json.syntax.comment"),
        ("control_char_in_name", "json.syntax.string"),
        ("control_char_opening_name", "json.syntax.string"),
    ];
    let dialects = [
        ("hermes", DialectId::Hermes),
        ("bare_json", DialectId::BareJson),
        ("mistral_v3", DialectId::MistralV3),
        ("anthropic", DialectId::AnthropicToolUse),
        ("llama3", DialectId::Llama3Json),
        ("openai", DialectId::OpenAiToolCalls),
    ];
    for (class, rule) in classes {
        for (label, dialect) in &dialects {
            let name = format!("envelope_{class}_{label}");
            let (_, result) = golden(&name);
            assert!(matches!(result.outcome, Outcome::Resolved(_)), "{name}");
            assert!(reading(&result).rejected.is_empty(), "{name}");
            let call = the_call(&result);
            assert_eq!(call.tool.as_deref(), Some("read_file"), "{name}");
            assert_eq!(call.arguments.to_json(), r#"{"path":"a"}"#, "{name}");
            assert_eq!(call.outcome, CallOutcome::Resolved, "{name}");
            assert_eq!(
                call.candidate_dialects,
                core::slice::from_ref(dialect),
                "{name}"
            );
            let rules = rules_of(call);
            assert!(rules.contains(&rule), "{name}: {rules:?}");
            assert!(
                !rules.contains(&"tool_call.name.match"),
                "{name}: {rules:?}"
            );
            assert!(!has_guess(call) && !has_data_loss(call), "{name}");
        }
    }
    let (_, result) = golden("envelope_python_repr_dict_hermes");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.arguments.to_json(), r#"{"path":"a","follow":true}"#);
    let (golden, result) = golden("envelope_tolerance_never_reads_prose");
    let calls = reading(&result);
    assert_eq!(calls.finish, Finish::ContentOnly);
    assert_eq!((calls.calls.len(), calls.rejected.len()), (0, 0));
    let covered: usize = calls.content.iter().map(ExactSizeIterator::len).sum();
    assert_eq!(covered, golden.input.len());

    // Parity on a mutating tool: the same slip in the envelope and in the
    // arguments both leave the call `Resolved`.
    let tools = ToolSet::compile(
        [ToolDefinition::new("write_file").param("path", Param::string().required())],
        Budget::default(),
    )
    .unwrap();
    for input in [
        "<tool_call>{name: \"write_file\", arguments: {\"path\": \"a\"}}</tool_call>",
        "<tool_call>{\"name\": \"write_file\", \"arguments\": {path: \"a\"}}</tool_call>",
        "<tool_call>{'name': 'write_file', 'arguments': {\"path\": \"a\"},}</tool_call>",
        "<tool_call>{\"name\": \"write_file\", \"arguments\": {'path': 'a',}}</tool_call>",
    ] {
        let result = tools.recover(input.as_bytes(), &ToolRecoveryRequest::default());
        assert!(matches!(result.outcome, Outcome::Resolved(_)), "{input}");
        assert_eq!(the_call(&result).outcome, CallOutcome::Resolved, "{input}");
    }
}

/// F4: bytes that are not well-formed UTF-8 inside a call's argument keep
/// the call, in every dialect and codec (Hermes, Kimi-K2 and `OpenAI`
/// stringified arguments through `json`, Qwen3-Coder XML through
/// `xml-params`, Harmony's python body through `raw`). Each maximal
/// malformed subpart is read as U+FFFD and each run recorded as a
/// `tool_call.value.invalid-utf8` data loss whose evidence span is the
/// run's exact bytes and whose original retains them, so the call is at
/// most `BestGuess`, usable on a read-only tool and never on a `Mutating`
/// one. A malformed byte in a tool name is a rejected attempt with reason
/// `invalid_utf8` at its offset (no built-in name grammar admits it); one
/// in a parameter name is read with U+FFFD and the parameter-name rules
/// decide.
#[test]
fn invalid_utf8_in_arguments_keeps_the_call() {
    for name in [
        "invalid_utf8_argument_hermes",
        "invalid_utf8_argument_kimi",
        "invalid_utf8_argument_openai_stringified",
        "invalid_utf8_argument_qwen_xml",
    ] {
        let (golden, result) = golden(name);
        let at = golden
            .input
            .iter()
            .position(|&byte| byte == 0xE9)
            .expect("the byte");
        assert!(matches!(result.outcome, Outcome::BestGuess(_)), "{name}");
        let call = the_call(&result);
        assert_eq!(call.tool.as_deref(), Some("read_file"), "{name}");
        assert_eq!(call.outcome, CallOutcome::BestGuess, "{name}");
        assert_eq!(
            call.arguments.to_json(),
            "{\"path\":\"caf\u{FFFD}.txt\"}",
            "{name}"
        );
        assert_eq!(
            loss_kinds(call, "tool_call.value.invalid-utf8"),
            [LossKind::Data],
            "{name}"
        );
        let loss = call
            .repairs
            .losses
            .iter()
            .find(|loss| loss.loss.evidence.rule == "tool_call.value.invalid-utf8")
            .expect("the loss");
        let span = loss.loss.evidence.span.as_ref().expect("a span");
        assert_eq!(span.bytes, at..at + 1, "{name}");
        assert_eq!(loss.loss.original, "\\xE9", "{name}");
        assert!(!has_guess(call), "{name}");
        assert_eq!(
            reading(&result).usable_accepting_guesses().len(),
            1,
            "{name}"
        );
    }
    // The raw codec keeps the body too.
    let (_, result) = golden("invalid_utf8_argument_raw_harmony_python");
    let call = the_call(&result);
    assert_eq!(call.arguments.to_json(), "\"print(\\\"caf\u{FFFD}\\\")\"");
    assert!(rules_of(call).contains(&"tool_call.value.invalid-utf8"));
    // One U+FFFD per maximal malformed subpart, one loss per run.
    let (_, result) = golden("invalid_utf8_argument_runs_and_cut");
    let call = the_call(&result);
    assert_eq!(
        call.arguments.to_json(),
        "{\"path\":\"a\u{FFFD}\u{FFFD}b\u{FFFD}\u{FFFD}\u{FFFD}c\u{FFFD}\"}"
    );
    assert_eq!(loss_kinds(call, "tool_call.value.invalid-utf8").len(), 3);
    // The Mutating guard: kept and itemized, never usable.
    let (_, result) = golden("invalid_utf8_argument_mutating_guard");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::BestGuess);
    assert!(has_data_loss(&calls.calls[0]));
    assert!(calls.usable().is_empty() && calls.usable_accepting_guesses().is_empty());
    // A malformed byte in a tool name: a rejected attempt, never dropped.
    for name in [
        "invalid_utf8_in_tool_name_hermes",
        "invalid_utf8_in_tool_name_qwen_xml",
    ] {
        let (golden, result) = golden(name);
        let calls = reading(&result);
        assert!(calls.calls.is_empty(), "{name}");
        assert_eq!(calls.rejected.len(), 1, "{name}");
        let at = golden
            .input
            .iter()
            .position(|&byte| byte == 0xE9)
            .expect("the byte");
        assert!(
            matches!(calls.rejected[0].reason, purrassist_core::tool_call::Rejection::InvalidUtf8 { at: offset } if offset == at),
            "{name}: {:?}",
            calls.rejected[0].reason
        );
    }
    // A malformed byte in a parameter name: read with U+FFFD, then the
    // parameter-name rules.
    let (_, result) = golden("invalid_utf8_in_parameter_name_qwen_xml");
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::BestGuess);
    assert!(rules_of(call).contains(&"tool_call.value.invalid-utf8"));
}

/// F5: over every golden, pinned and under the default allow-list, the
/// usability verdict on the wire (`value.verdict`, `value.usable_calls`,
/// `value.usable_calls_accepting_guesses`) is [`ToolCalls::verdict`] and
/// the usable-call counts, and the CLI's exit code
/// ([`purrassist_core::tool_call::exit_code`]) agrees with the documented
/// rule restated independently here: 0 exactly when the tool-choice check
/// passed and a call is usable accepting guesses or the turn is clean
/// content, 3 when instead a call (or the operation) is ambiguous, 4
/// otherwise. `status` is the reading's certainty and is not consulted.
#[test]
fn verdict_and_exit_code_agree_over_goldens() {
    use purrassist_core::tool_call::{Verdict, exit_code};
    let mut verdicts = std::collections::BTreeSet::new();
    for (golden, result) in all_goldens().into_iter().chain(all_default_runs()) {
        let name = golden.path.display().to_string();
        let code = exit_code(&result.outcome);
        let (Outcome::Resolved(calls) | Outcome::BestGuess(calls)) = &result.outcome else {
            let expected = if matches!(result.outcome, Outcome::Ambiguous(_)) {
                3
            } else {
                4
            };
            assert_eq!(code, expected, "{name}");
            continue;
        };
        let value = result.to_node();
        let value = value.get("value").expect("a value");
        let verdict = calls.verdict();
        verdicts.insert(verdict.code());
        assert_eq!(
            value.get("verdict").and_then(Node::as_str),
            Some(verdict.code()),
            "{name}"
        );
        assert_eq!(
            value.get("usable_calls").map(Node::to_json),
            Some(calls.usable().len().to_string()),
            "{name}"
        );
        assert_eq!(
            value
                .get("usable_calls_accepting_guesses")
                .map(Node::to_json),
            Some(calls.usable_accepting_guesses().len().to_string()),
            "{name}"
        );
        let passed = calls.tool_choice.passed;
        let usable = !calls.usable_accepting_guesses().is_empty();
        let content = calls.finish == Finish::ContentOnly;
        let ambiguous = calls
            .calls
            .iter()
            .any(|call| call.outcome == CallOutcome::Ambiguous);
        let expected = if passed && (usable || content) {
            0
        } else if passed && ambiguous {
            3
        } else {
            4
        };
        assert_eq!(code, expected, "{name}");
        assert_eq!(
            code == 0,
            matches!(verdict, Verdict::Run | Verdict::NoCalls),
            "{name}"
        );
        assert_eq!(verdict == Verdict::Run, passed && usable, "{name}");
    }
    // The goldens exercise every verdict.
    assert_eq!(
        verdicts.into_iter().collect::<Vec<_>>(),
        ["ambiguous", "no_calls", "nothing_usable", "run"]
    );
}

/// F7: over every golden, pinned and under the default allow-list, the
/// finish, the `OpenAI` `finish_reason` and the `OpenAI` emission agree with
/// the verdict:
///
/// - a finish with calls (`calls`, `truncated_with_calls`) implies the
///   verdict `run`; a `run` turn finishes otherwise only as `unresolved`,
///   when something else is wrong beside the calls that run (a rejected
///   attempt, or an unresolved or ambiguous call);
/// - a failed tool-choice check (even beside calls that would run on their
///   own) finishes `unresolved`, with the verdict `nothing_usable` and
///   nothing emitted;
/// - the `OpenAI` emission holds a call exactly when the verdict is `run`;
/// - `finish_reason` is `"length"` exactly when the stop reason is a
///   length stop (whatever the finish), and otherwise `"tool_calls"`
///   exactly when the emission holds a call.
#[test]
fn finish_and_finish_reason_agree_with_the_verdict() {
    use purrassist_core::tool_call::{IdPolicy, Verdict, emit};
    for (golden, result) in all_goldens().into_iter().chain(all_default_runs()) {
        let name = golden.path.display().to_string();
        let (Outcome::Resolved(calls) | Outcome::BestGuess(calls)) = &result.outcome else {
            continue;
        };
        let verdict = calls.verdict();
        let with_calls = matches!(calls.finish, Finish::Calls | Finish::TruncatedWithCalls);
        let truncated = matches!(calls.finish, Finish::Truncated | Finish::TruncatedWithCalls);
        if with_calls {
            assert_eq!(verdict, Verdict::Run, "{name}");
        } else if verdict == Verdict::Run {
            let wrong = !calls.rejected.is_empty()
                || calls.calls.iter().any(|call| {
                    matches!(
                        call.outcome,
                        CallOutcome::Unresolved { .. } | CallOutcome::Ambiguous
                    )
                });
            assert!(calls.finish == Finish::Unresolved && wrong, "{name}");
        }
        if !calls.tool_choice.passed {
            assert_eq!(calls.finish, Finish::Unresolved, "{name}");
            assert_eq!(verdict, Verdict::NothingUsable, "{name}");
        }
        let emitted = emit::openai_tool_calls(
            calls,
            &IdPolicy::Prefix {
                prefix: "c".into(),
                start: 0,
            },
        )
        .expect("emission");
        let Value::Array(array) = &emitted.output.value else {
            panic!("{name}: an array")
        };
        assert_eq!(!array.is_empty(), verdict == Verdict::Run, "{name}");
        assert_eq!(array.len(), calls.emitted_calls().len(), "{name}");
        // `"length"` exactly for a length stop (a truncated finish is one),
        // otherwise `"tool_calls"` exactly when the emission holds a call.
        let length_stop = golden.request.stop == purrassist_core::tool_call::GenerationStop::Length;
        assert_eq!(calls.length_stop, length_stop, "{name}");
        if truncated {
            assert!(length_stop, "{name}");
        }
        let reason = calls.openai_finish_reason();
        assert_eq!(reason == "length", length_stop, "{name}");
        assert_eq!(
            reason == "tool_calls",
            !length_stop && !array.is_empty(),
            "{name}"
        );
    }
}

/// S1: a reasoning block never swallows calls, under the default allow-list
/// and every reasoning mode. A block the turn starts in (`starts_open`)
/// with no closer is closed at the first call opener
/// (`tool_call.reasoning.unclosed`) and the call recovered, for a call of
/// every JSON-bodied and XML dialect: another dialect's `explicit` closer
/// (Anthropic's `"}`, ordinary JSON inside the call) never ends it. A block
/// with a closer ends exactly after the closer, the call inside offered as
/// an embedded alternative and the text after it content.
#[test]
fn reasoning_never_swallows_calls() {
    // One range, as a vector.
    let only = |range: core::ops::Range<usize>| core::iter::once(range).collect::<Vec<_>>();
    for dialect in [
        "hermes",
        "mistral_v3",
        "llama3",
        "bare_json",
        "qwen_xml",
        "anthropic",
    ] {
        let name = format!("reasoning_starts_open_unclosed_{dialect}");
        let (_, result) = golden(&name);
        let calls = reading(&result);
        assert_eq!(calls.calls.len(), 1, "{name}");
        assert!(
            rules_of(&calls.calls[0]).contains(&"tool_call.reasoning.unclosed"),
            "{name}"
        );
        let opener = "Let me look. ".len();
        assert_eq!(calls.reasoning, only(0..opener), "{name}");
        assert_eq!(calls.calls[0].span.start, opener, "{name}");

        let name = format!("reasoning_starts_open_closed_{dialect}");
        let (fixture, result) = golden(&name);
        let calls = reading(&result);
        assert!(calls.calls.is_empty(), "{name}");
        assert_eq!(calls.embedded.len(), 1, "{name}");
        let end = fixture.input.len() - "Answer.".len();
        assert_eq!(calls.reasoning, only(0..end), "{name}: ends after </think>");
        assert_eq!(calls.content, only(end..fixture.input.len()), "{name}");
    }
    let (_, result) = golden("reasoning_starts_open_swallow_repro");
    let calls = reading(&result);
    assert_eq!(
        (calls.calls.len(), calls.reasoning.clone()),
        (1, only(0..13))
    );
    let (fixture, result) = golden("reasoning_starts_open_think_closer_repro");
    let calls = reading(&result);
    let end = fixture.input.len() - "done".len();
    assert_eq!(calls.reasoning, only(0..end));
    assert_eq!(calls.embedded.len(), 1);
    let (_, result) = golden("reasoning_separate_closed_block_calls");
    let calls = reading(&result);
    assert_eq!((calls.calls.len(), calls.embedded.len()), (1, 4));
    let (_, result) = golden("reasoning_separate_unclosed_block_calls");
    assert_eq!(reading(&result).calls.len(), 2);
    let (_, result) = golden("reasoning_as_content_calls");
    let calls = reading(&result);
    assert_eq!(calls.calls.len(), 2);
    assert_eq!(calls.reasoning.len(), 0);
}

/// S2: Qwen3-Coder XML text of a parameter whose schema the typed model does
/// not express (pydantic `Optional[...]`'s `anyOf`, `$ref`, `oneOf`, a type
/// array) is typed by the tool's exact schema: the JSON reading the text
/// spells is taken where the schema admits it and not the verbatim string
/// (`tool_call.value.schema-typed`, definition-grounded and not a guess, so
/// the call is `Resolved` even on a mutating tool), a string branch keeps
/// the verbatim text, and with no validating reading the text stays
/// verbatim with its `schema_validation` diagnosis. The keyword that drove
/// the typing is reported `used`. The adapter still renders each
/// declaration back byte for byte.
#[test]
fn unmodeled_schemas_type_xml_text() {
    for (name, arguments, used) in [
        (
            "schema_typed_optional_int",
            r#"{"q":"cats","limit":5}"#,
            Some("anyOf"),
        ),
        (
            "schema_typed_optional_int_null",
            r#"{"limit":null}"#,
            Some("anyOf"),
        ),
        (
            "schema_typed_optional_str_numeric",
            r#"{"zip":"12345"}"#,
            None,
        ),
        (
            "schema_typed_string_or_integer_keeps_string",
            r#"{"id":"12345"}"#,
            Some("anyOf"),
        ),
        ("schema_typed_ref_scalar", r#"{"n":7}"#, Some("$ref")),
        (
            "schema_typed_ref_object_inline",
            r#"{"opts":{"depth":2,"label":"x"}}"#,
            Some("$ref"),
        ),
        (
            "schema_typed_ref_object_nested",
            r#"{"opts":{"depth":2,"label":"42"}}"#,
            Some("$ref"),
        ),
        ("schema_typed_oneof_enum", r#"{"mode":"fast"}"#, None),
        (
            "schema_typed_recursive_ref",
            r#"{"t":{"v":1,"kids":[{"v":2,"kids":[]}]}}"#,
            Some("$ref"),
        ),
        ("schema_typed_type_array", r#"{"x":3}"#, Some("type")),
    ] {
        let (golden, result) = golden(name);
        let call = the_call(&result);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{name}");
        assert_eq!(call.arguments.to_json(), arguments, "{name}");
        assert!(!has_guess(call), "{name}");
        let typed = rules_of(call).contains(&"tool_call.value.schema-typed");
        assert_eq!(typed, used.is_some(), "{name}");
        if let Some(keyword) = used {
            assert!(
                call.keywords
                    .iter()
                    .any(|use_| use_.keyword == keyword && use_.used),
                "{name}: {:#?}",
                call.keywords
            );
        }
        let source = openai_text(&golden.node).expect("tools_openai text");
        let declarations = parse(&source).expect("tools json");
        let span = items(&declarations)[0].span.clone().expect("span");
        let rendered = golden
            .tools
            .render_declaration("search", DeclarationForm::OpenAi)
            .expect("rendered");
        assert_eq!(rendered.as_bytes(), &source.as_bytes()[span], "{name}");
    }
    let (_, result) = golden("schema_typed_unmatched_keeps_verbatim");
    let call = the_call(&result);
    assert_eq!(call.arguments.to_json(), r#"{"limit":"abc"}"#);
    assert_eq!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::SchemaValidation
        }
    );
    assert!(call.keywords.iter().all(|use_| !use_.used));
}

/// G7: the issue's shape with a reasoning-marker spelling in a JSON string of
/// a repeated nested-parameter item. The marker is value text.
#[test]
fn xml_repeated_items_reasoning_marker_in_json_string() {
    let (_, result) = golden("xml_repeated_items_reasoning_marker_in_json_string");
    let calls = reading(&result);
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"outfits":[{"wearer":"Alice","garments":["a red coat"]},{"wearer":"Bob","garments":["a <think> scarf"]}]}"#
    );
    assert!(calls.reasoning.is_empty(), "{:#?}", calls.reasoning);
    assert_eq!(calls.reasoning_blocks, []);
    assert_eq!(rule_count(call, "tool_call.reasoning.marker-cut"), 0);
}

/// G7: the same for repeated JSON-array occurrences of the parameter.
#[test]
fn xml_repeated_json_array_items_reasoning_marker_in_json_string() {
    let (_, result) = golden("xml_repeated_json_array_items_reasoning_marker_in_json_string");
    let calls = reading(&result);
    let call = the_call(&result);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments.to_json(),
        r#"{"outfits":[{"wearer":"Alice"},{"wearer":"Bob","garments":["a <think> scarf"]}]}"#
    );
    assert!(calls.reasoning.is_empty(), "{:#?}", calls.reasoning);
    assert_eq!(calls.reasoning_blocks, []);
    assert_eq!(rule_count(call, "tool_call.reasoning.marker-cut"), 0);
}
