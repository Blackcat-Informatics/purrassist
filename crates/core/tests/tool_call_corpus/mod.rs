// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The tool-call mutation corpus (`tests/data/evaluation/tool-calls.jsonl`).
//!
//! Shared by the `tool-call-corpus` example, which writes the corpus and its
//! manifest entry, and by `correction_evaluation.rs`, which regenerates it in
//! memory (byte-for-byte equal to the committed file) and evaluates it.
//!
//! Every case starts from a grammar witness ([`witness::generate`], fixed
//! seeds) of one call to a first-party tool, printed in a built-in dialect,
//! and applies the *inverse* of one repair rule or edit operator (dropping a
//! closer, a closer variant, a stray closer, a dropped `</parameter>`, a
//! fence or prose around the arguments, stringified arguments, a cut-off
//! output, a misspelled name or key, a case-changed enumeration value, a
//! whitespace-padded typed value, a JSON syntax slip, ...). The case's
//! `category` is the rule expected to repair it, and its `truth` is the
//! original call(s) when the mutation preserved what the caller needs, or
//! `"Unresolved"` when the only correct answer is abstention:
//!
//! - the mutation destroyed information or made it genuinely ambiguous (a
//!   hallucinated tool name, a missing required argument, a cut-off call, a
//!   repeated argument with two values, text between two closers of a
//!   string value, a value of the wrong JSON type that must never be
//!   coerced, a call that only appears inside reasoning);
//! - the repair is a guess (`Readings::Multiple`) and the tool's effective
//!   repair policy is `Structural`, or the tool is `Mutating`;
//! - the repair records a data loss and the tool is `Mutating` (a
//!   `BestGuess` on a mutating tool is never run).
//!
//! The label is derived from the mutation, the tool definition and the
//! case's policy alone, never from running recovery. Every tool, dialect
//! and seed in `0..SEEDS` contributes every applicable mutation; turns of
//! two calls (one dialect, and Qwen3-Coder XML mixed with Hermes) add the
//! multi-call rules; supplementary seeds then fill categories whose
//! mutation needs a rarer witness shape (a non-empty nested object, an
//! array of several strings, stringified Hermes arguments) up to `TARGET`
//! cases. Every choice is deterministic: witness seeds are fixed, and the
//! few remaining choices use a first-party xorshift generator seeded from
//! the case coordinates.
// Included by path into two crates (the example and the evaluation test):
// each uses a different subset, and its `pub` items are the module's
// interface to the including crate, not a crate's public API.
#![allow(dead_code, unreachable_pub)]

use purrassist_core::grammar::witness::{self, JsonArguments, WitnessOptions};
use purrassist_core::json::{self, Member, Node, Value};
use purrassist_core::tool_call::{
    DialectId, DialectSet, Effects, GenerationStop, ObjectSpec, Param, PolicySelection,
    RepairPolicy, ToolDefinition, ToolRecoveryRequest, ToolSet, ValueShape,
};
use purrassist_core::{Budget, Effort, Governor};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// The corpus path, relative to the `purrassist-core` package root.
pub const RELATIVE_PATH: &str = "tests/data/evaluation/tool-calls.jsonl";

/// The record value of a `truth` that must abstain.
pub const UNRESOLVED: &str = "Unresolved";

/// The witness seeds of every base call.
const SEEDS: u64 = 4;

/// Supplementary witness seeds tried per tool and dialect to bring every
/// category to [`TARGET`] cases.
const SUPPLEMENTARY_SEEDS: u64 = 64;

/// The case count every category is filled to, when its mutation applies
/// to enough witnesses.
const TARGET: usize = 24;

/// The pattern of path-like string parameters: bare-token safe.
const PATH_PATTERN: &str = "^[a-z][a-z0-9_.-]{0,15}$";

/// The first-party tool set every case is recovered against: read-only and
/// mutating tools with typed, enumerated, patterned, nested-object and array
/// parameters, tool aliases and a parameter alias.
#[must_use]
pub fn tools() -> ToolSet {
    let definitions = [
        ToolDefinition::new("read_file")
            .alias("cat")
            .effects(Effects::ReadOnly)
            .param(
                "path",
                Param::string()
                    .required()
                    .alias("file")
                    .pattern(PATH_PATTERN),
            )
            .param("limit", Param::integer())
            .param(
                "encoding",
                Param::enumeration(&["utf-8", "latin-1", "ascii"]),
            ),
        ToolDefinition::new("search_code")
            .effects(Effects::ReadOnly)
            .param("query", Param::string().required())
            .param("mode", Param::enumeration(&["literal", "regex", "fuzzy"]))
            .param("paths", Param::array(Param::string())),
        ToolDefinition::new("get_forecast")
            .effects(Effects::ReadOnly)
            .param(
                "city",
                Param::string().required().pattern("^[A-Z][a-z]{2,10}$"),
            )
            .param(
                "unit",
                Param::enumeration(&["celsius", "fahrenheit"]).required(),
            )
            .param("days", Param::integer()),
        ToolDefinition::new("configure_view")
            .effects(Effects::ReadOnly)
            .param(
                "options",
                Param::object(
                    ObjectSpec::new()
                        .param("depth", Param::integer().required())
                        .param("label", Param::string())
                        .param("compact", Param::boolean()),
                ),
            )
            .param("theme", Param::enumeration(&["light", "dark"])),
        ToolDefinition::new("write_file")
            .alias("save_file")
            .effects(Effects::Mutating)
            .param(
                "path",
                Param::string()
                    .required()
                    .alias("file")
                    .pattern(PATH_PATTERN),
            )
            .param("content", Param::string().required())
            .param("append", Param::boolean()),
        ToolDefinition::new("create_issue")
            .effects(Effects::Mutating)
            .param("title", Param::string().required())
            .param(
                "priority",
                Param::enumeration(&["low", "medium", "high"]).required(),
            )
            .param("labels", Param::array(Param::string()))
            .param("assignee", Param::string().pattern(PATH_PATTERN)),
        ToolDefinition::new("run_command")
            .effects(Effects::Mutating)
            .param("command", Param::string().required())
            .param("timeout_seconds", Param::integer())
            .param(
                "options",
                Param::object(
                    ObjectSpec::new()
                        .param("cwd", Param::string().pattern(PATH_PATTERN))
                        .param("shell", Param::enumeration(&["bash", "zsh"])),
                ),
            ),
        ToolDefinition::new("delete_path")
            .effects(Effects::Mutating)
            .param("path", Param::string().required().pattern(PATH_PATTERN))
            .param("recursive", Param::boolean().required()),
    ];
    ToolSet::compile(definitions, Budget::default()).expect("the corpus tool set compiles")
}

/// A first-party xorshift64 generator (Marsaglia 2003), for the few choices
/// a mutation makes beyond the witness seed.
struct XorShift(u64);

impl XorShift {
    const fn new(seed: u64) -> Self {
        // Never zero: xorshift's only fixed point.
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    const fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound.max(1)).unwrap_or(u64::MAX);
        usize::try_from(self.next() % bound).unwrap_or(0)
    }
}

/// How each tool's repair policy is chosen for a case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Each tool's own policy (`Tolerant` for read-only tools, `Structural`
    /// for mutating ones).
    PerTool,
    /// `Structural` for every tool.
    Structural,
    /// `Tolerant` for every tool.
    Tolerant,
}

impl Policy {
    const fn code(self) -> &'static str {
        match self {
            Self::PerTool => "per_tool",
            Self::Structural => "structural",
            Self::Tolerant => "tolerant",
        }
    }

    fn parse(code: &str) -> Self {
        match code {
            "per_tool" => Self::PerTool,
            "structural" => Self::Structural,
            "tolerant" => Self::Tolerant,
            other => panic!("unknown policy `{other}`"),
        }
    }

    const fn selection(self) -> PolicySelection {
        match self {
            Self::PerTool => PolicySelection::PerTool,
            Self::Structural => PolicySelection::All(RepairPolicy::Structural),
            Self::Tolerant => PolicySelection::All(RepairPolicy::Tolerant),
        }
    }

    /// The effective policy of a tool with `effects`.
    const fn effective(self, effects: Effects) -> RepairPolicy {
        match self {
            Self::PerTool => effects.default_policy(),
            Self::Structural => RepairPolicy::Structural,
            Self::Tolerant => RepairPolicy::Tolerant,
        }
    }
}

/// Why the model stopped, as recorded in a case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ending {
    /// `stop`.
    Stop,
    /// `tool_calls`.
    ToolCalls,
    /// `length`.
    Length,
    /// A stop sequence that consumed the output's last bytes.
    Sequence(&'static str),
}

impl Ending {
    fn node(&self) -> Node {
        match self {
            Self::Stop => string("stop"),
            Self::ToolCalls => string("tool_calls"),
            Self::Length => string("length"),
            Self::Sequence(sequence) => object(vec![("sequence", string(sequence))]),
        }
    }

    fn generation_stop(node: &Node) -> GenerationStop {
        if let Some(sequence) = node.get("sequence").and_then(Node::as_str) {
            return GenerationStop::StopSequence(sequence.as_bytes().to_vec());
        }
        match node.as_str().expect("a stop code") {
            "stop" => GenerationStop::Stop,
            "tool_calls" => GenerationStop::ToolCalls,
            "length" => GenerationStop::Length,
            other => panic!("unknown stop `{other}`"),
        }
    }
}

/// What a mutation leaves for recovery to decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Need {
    /// A single-reading repair (or plain parsing) restores the call.
    Exact,
    /// The repair records a data loss: only a read-only call may run.
    DataLoss,
    /// The repair is a guess: only a read-only call under `Tolerant` may run.
    Guess,
    /// An undeclared parameter: dropped with a loss on read-only tools,
    /// an error on mutating ones.
    Unknown,
    /// The information is gone or genuinely ambiguous: abstain.
    Never,
}

/// One call of a turn: its canonical name and arguments.
#[derive(Clone, Debug)]
pub struct Call {
    /// The canonical tool name.
    pub name: String,
    /// The arguments.
    pub arguments: Node,
}

/// One corpus case.
#[derive(Clone, Debug)]
pub struct Case {
    /// Unique case identity.
    pub id: String,
    /// The rule expected to repair the mutation (`canonical` for unmutated
    /// controls).
    pub category: String,
    /// The dialect allow-list, comma-separated dialect ids.
    pub dialect: String,
    /// How tool repair policies are chosen.
    pub policy: Policy,
    /// Why the model stopped.
    pub stop: Ending,
    /// The model output.
    pub input: String,
    /// The calls a correct answer recovers, in order; `None` to abstain.
    pub truth: Option<Vec<Call>>,
}

impl Case {
    /// The case as a JSON record (members in a fixed order).
    #[must_use]
    pub fn record(&self) -> Node {
        let truth = self.truth.as_ref().map_or_else(
            || string(UNRESOLVED),
            |calls| {
                Node::new(Value::Array(
                    calls
                        .iter()
                        .map(|call| {
                            object(vec![
                                ("name", string(&call.name)),
                                ("arguments", call.arguments.clone()),
                            ])
                        })
                        .collect(),
                ))
            },
        );
        object(vec![
            ("id", string(&self.id)),
            ("category", string(&self.category)),
            ("dialect", string(&self.dialect)),
            ("policy", string(self.policy.code())),
            ("stop", self.stop.node()),
            ("input", string(&self.input)),
            ("truth", truth),
        ])
    }
}

fn string(text: &str) -> Node {
    Node::new(Value::String(text.to_owned()))
}

fn object(members: Vec<(&str, Node)>) -> Node {
    Node::new(Value::Object(
        members
            .into_iter()
            .map(|(key, value)| Member {
                key: key.to_owned(),
                key_span: None,
                value,
            })
            .collect(),
    ))
}

/// The recovery request of a corpus record: `Effort::Standard`, the
/// record's dialect allow-list, policy and stop, every other setting the
/// default.
#[must_use]
pub fn request(record: &Node) -> ToolRecoveryRequest {
    let field = |key: &str| {
        record
            .get(key)
            .unwrap_or_else(|| panic!("record member `{key}`"))
    };
    let dialects: Vec<DialectId> = field("dialect")
        .as_str()
        .expect("dialect list")
        .split(',')
        .map(|id| DialectId::from_id(id).unwrap_or_else(|| panic!("dialect `{id}`")))
        .collect();
    let set = DialectSet::new(dialects, &mut Governor::new(Budget::default()))
        .expect("a valid dialect allow-list");
    ToolRecoveryRequest::new(Effort::Standard)
        .dialects(set)
        .policy(Policy::parse(field("policy").as_str().expect("policy")).selection())
        .stop(Ending::generation_stop(field("stop")))
}

/// The JSON Lines text of `cases`: one compact record per line, ASCII
/// only (every non-ASCII character, which compact JSON writes only inside
/// strings, is written as `\uXXXX` escapes).
#[must_use]
pub fn render(cases: &[Case]) -> String {
    let mut text = String::new();
    for case in cases {
        for character in case.record().to_json().chars() {
            if character.is_ascii() {
                text.push(character);
            } else {
                let mut units = [0_u16; 2];
                for unit in character.encode_utf16(&mut units) {
                    write!(text, "\\u{unit:04x}").expect("writing to a String is infallible");
                }
            }
        }
        text.push('\n');
    }
    text
}

/// The number of distinct requests of `cases`: records without `id`,
/// `category` and `truth`.
#[must_use]
pub fn distinct_requests(cases: &[Case]) -> usize {
    cases
        .iter()
        .map(|case| {
            let mut record = case.record();
            if let Value::Object(members) = &mut record.value {
                members
                    .retain(|member| !matches!(member.key.as_str(), "id" | "category" | "truth"));
            }
            record.to_json()
        })
        .collect::<BTreeSet<_>>()
        .len()
}

// ---------------------------------------------------------------------
// Base calls: one witness per tool, dialect and seed.
// ---------------------------------------------------------------------

/// How a JSON-dialect witness carries its arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    /// XML parameter elements (Qwen3-Coder).
    Xml,
    /// A JSON object.
    Object,
    /// A JSON string holding the object.
    Stringified,
}

/// One witness call and what the mutations need to know about it.
#[derive(Clone, Debug)]
struct Base {
    dialect: DialectId,
    tool: usize,
    seed: u64,
    effects: Effects,
    text: String,
    call: Call,
    form: Form,
    /// The declared parameters, in declaration order.
    params: Vec<Declared>,
}

/// A declared parameter's name, shape, requiredness and alias.
#[derive(Clone, Debug)]
struct Declared {
    name: String,
    shape: ValueShape,
    required: bool,
    alias: Option<String>,
}

impl Base {
    fn members(&self) -> &[Member] {
        match &self.call.arguments.value {
            Value::Object(members) => members,
            _ => unreachable!("arguments are an object"),
        }
    }

    fn declared(&self, name: &str) -> &Declared {
        self.params
            .iter()
            .find(|param| param.name == name)
            .unwrap_or_else(|| panic!("declared parameter `{name}`"))
    }

    /// A short dialect label for case identities.
    const fn label(&self) -> &'static str {
        dialect_label(&self.dialect)
    }
}

const fn dialect_label(dialect: &DialectId) -> &'static str {
    match dialect {
        DialectId::DeepSeekV31 => "deepseek31",
        DialectId::DeepSeekV3 => "deepseek3",
        DialectId::Hermes => "hermes",
        DialectId::Llama3Json => "llama3",
        DialectId::MistralV3 => "mistral3",
        DialectId::MistralV11 => "mistral11",
        DialectId::OpenAiToolCalls => "openai",
        DialectId::Qwen3CoderXml => "qwen",
        DialectId::Custom(_) => "custom",
    }
}

/// The built-in dialects, in corpus order.
const DIALECTS: [DialectId; 8] = [
    DialectId::Qwen3CoderXml,
    DialectId::Hermes,
    DialectId::OpenAiToolCalls,
    DialectId::Llama3Json,
    DialectId::MistralV3,
    DialectId::MistralV11,
    DialectId::DeepSeekV31,
    DialectId::DeepSeekV3,
];

/// The witness of tool `tool` in `dialect` for corpus seed `seed`.
///
/// Qwen3-Coder XML prints parameter elements; `OpenAI` alternates
/// stringified and object arguments by seed; Hermes, Llama 3 JSON and
/// Mistral v3 print objects for the first [`SEEDS`] seeds (so stringifying them is a
/// mutation) and alternate for the supplementary seeds.
fn base(tools: &ToolSet, dialect: &DialectId, tool: usize, seed: u64) -> Base {
    let compiled = &tools.tools()[tool];
    let shape = tools
        .argument_shape(compiled.name())
        .expect("every tool has a shape");
    let form = match dialect {
        DialectId::Qwen3CoderXml => Form::Xml,
        DialectId::Hermes
        | DialectId::Llama3Json
        | DialectId::MistralV3
        | DialectId::MistralV11
        | DialectId::DeepSeekV31
        | DialectId::DeepSeekV3
            if seed < SEEDS =>
        {
            Form::Object
        }
        _ if seed.is_multiple_of(2) => Form::Stringified,
        _ => Form::Object,
    };
    let json_arguments = if form == Form::Stringified {
        JsonArguments::Stringified
    } else {
        JsonArguments::Object
    };
    let witness = witness::generate(
        dialect.grammar(),
        &[shape],
        WitnessOptions {
            seed: seed * 7 + u64::try_from(tool).unwrap_or(0),
            json_arguments,
        },
        &mut Governor::new(Budget::default()),
    )
    .unwrap_or_else(|error| {
        panic!(
            "witness for {} in {}: {error:?}",
            compiled.name(),
            dialect.id()
        )
    });
    let call = witness.calls.into_iter().next().expect("one call");
    let params = shape
        .arguments
        .parameters
        .iter()
        .map(|param| Declared {
            name: param.name.clone(),
            shape: param.value.clone(),
            required: param.required,
            alias: param.aliases.first().cloned(),
        })
        .collect();
    Base {
        dialect: dialect.clone(),
        tool,
        seed,
        effects: compiled.effects(),
        text: witness.text,
        call: Call {
            name: call.name,
            arguments: call.arguments,
        },
        form,
        params,
    }
}

/// The base calls: every tool in every dialect for seeds `0..SEEDS`.
fn bases(tools: &ToolSet) -> Vec<Base> {
    let mut bases = Vec::new();
    for dialect in &DIALECTS {
        for tool in 0..tools.tools().len() {
            for seed in 0..SEEDS {
                bases.push(base(tools, dialect, tool, seed));
            }
        }
    }
    bases
}

// ---------------------------------------------------------------------
// Text surgery on a witness: the arguments region and the name.
// ---------------------------------------------------------------------

/// A witness split around its arguments: `prefix`, the arguments text, and
/// `suffix`.
struct Split {
    prefix: String,
    suffix: String,
}

/// One argument member as printed: its key text and its value text.
#[derive(Clone, Debug)]
struct Printed {
    key: String,
    value: String,
}

/// Whether `shape` is printed verbatim in XML parameter elements.
const fn verbatim(shape: &ValueShape) -> bool {
    matches!(
        shape,
        ValueShape::String(_) | ValueShape::Enumeration(_) | ValueShape::Unmodeled
    )
}

/// The XML text of `value` under `shape`: a string verbatim, anything else
/// as JSON.
fn raw(value: &Node, shape: &ValueShape) -> String {
    match &value.value {
        Value::String(text) if verbatim(shape) => text.clone(),
        _ => value.to_json(),
    }
}

/// `xml-params` elements for `(name, text)` pairs; `closed[i]` says whether
/// element `i` keeps its `</parameter>`.
fn xml_body(elements: &[(String, String, bool)]) -> String {
    let mut body = String::from("\n");
    for (name, text, closed) in elements {
        body.push_str("<parameter=");
        body.push_str(name);
        body.push_str(">\n");
        body.push_str(text);
        body.push('\n');
        if *closed {
            body.push_str("</parameter>\n");
        }
    }
    body
}

impl Base {
    /// The canonical XML elements of the arguments.
    fn xml_elements(&self) -> Vec<(String, String, bool)> {
        self.members()
            .iter()
            .map(|member| {
                let shape = &self.declared(&member.key).shape;
                (member.key.clone(), raw(&member.value, shape), true)
            })
            .collect()
    }

    /// The printed JSON members of the arguments.
    fn printed(&self) -> Vec<Printed> {
        self.members()
            .iter()
            .map(|member| Printed {
                key: json::quote(&member.key),
                value: member.value.to_json(),
            })
            .collect()
    }

    /// The arguments text exactly as the witness printed it.
    fn arguments_text(&self) -> String {
        match self.form {
            Form::Xml => xml_body(&self.xml_elements()),
            Form::Object => self.call.arguments.to_json(),
            Form::Stringified => json::quote(&self.call.arguments.to_json()),
        }
    }

    /// The witness around its arguments text (which occurs exactly once).
    fn split(&self) -> Split {
        let arguments = self.arguments_text();
        let at = self
            .text
            .find(&arguments)
            .unwrap_or_else(|| panic!("arguments {arguments:?} in {:?}", self.text));
        assert!(
            !self.text[at + 1..].contains(&arguments),
            "arguments occur once in {:?}",
            self.text
        );
        Split {
            prefix: self.text[..at].to_owned(),
            suffix: self.text[at + arguments.len()..].to_owned(),
        }
    }

    /// The witness with its arguments text replaced: XML text for the XML
    /// form, the JSON object text otherwise (quoted again when stringified).
    fn with_arguments(&self, arguments: &str) -> String {
        let split = self.split();
        let arguments = if self.form == Form::Stringified {
            json::quote(arguments)
        } else {
            arguments.to_owned()
        };
        format!("{}{arguments}{}", split.prefix, split.suffix)
    }

    /// The witness with its tool name replaced by `name`.
    fn with_name(&self, name: &str) -> String {
        let split = self.split();
        let (old, new) = if self.form == Form::Xml {
            (
                format!("<function={}>", self.call.name),
                format!("<function={name}>"),
            )
        } else if matches!(
            self.dialect,
            DialectId::MistralV11 | DialectId::DeepSeekV31 | DialectId::DeepSeekV3
        ) {
            // A bare name right after the token.
            (self.call.name.clone(), name.to_owned())
        } else {
            (json::quote(&self.call.name), json::quote(name))
        };
        let (prefix, suffix) = if split.prefix.contains(&old) {
            (split.prefix.replacen(&old, &new, 1), split.suffix)
        } else {
            assert!(split.suffix.contains(&old), "name in {:?}", self.text);
            (split.prefix, split.suffix.replacen(&old, &new, 1))
        };
        format!("{prefix}{}{suffix}", self.arguments_text())
    }

    /// JSON object text from printed members.
    fn json_text(members: &[Printed]) -> String {
        let inner: Vec<String> = members
            .iter()
            .map(|member| format!("{}:{}", member.key, member.value))
            .collect();
        format!("{{{}}}", inner.join(","))
    }

    /// The members (by index) whose declared shape satisfies `test`.
    fn members_where(&self, test: impl Fn(&Declared, &Node) -> bool) -> Vec<usize> {
        self.members()
            .iter()
            .enumerate()
            .filter(|(_, member)| test(self.declared(&member.key), &member.value))
            .map(|(index, _)| index)
            .collect()
    }

    /// The wrapper closer at the end of a Qwen or Hermes witness, the
    /// closing brace of a Llama 3 JSON call object, or the last closer of
    /// a Mistral v3 call (the array's bracket, or the bare object's brace).
    fn wrapper_end(&self) -> usize {
        let closer = self.closer();
        self.text
            .rfind(closer)
            .expect("a wrapped witness ends with its closer")
    }

    /// The canonical closer ending a wrapped witness.
    fn closer(&self) -> &'static str {
        match self.dialect {
            DialectId::MistralV3 if self.text.trim_end().ends_with(']') => "]",
            DialectId::Llama3Json | DialectId::MistralV3 => "}",
            DialectId::DeepSeekV31 | DialectId::DeepSeekV3 => DEEPSEEK_SECTION_END,
            _ => "</tool_call>",
        }
    }
}

// ---------------------------------------------------------------------
// Mutations.
// ---------------------------------------------------------------------

/// One mutated output.
struct Mutant {
    category: &'static str,
    variant: String,
    text: String,
    need: Need,
    stop: Ending,
    /// Requires a dialect allow-list with reasoning markers.
    reasoning: bool,
}

fn mutant(category: &'static str, variant: impl Into<String>, text: String, need: Need) -> Mutant {
    Mutant {
        category,
        variant: variant.into(),
        text,
        need,
        stop: Ending::Stop,
        reasoning: false,
    }
}

const NEAR_MISS: [(&str, [&str; 2]); 8] = [
    ("read_file", ["read_fiel", "reed_file"]),
    ("search_code", ["serach_code", "search_cod"]),
    ("get_forecast", ["get_forcast", "get_forecats"]),
    ("configure_view", ["configure_veiw", "configur_view"]),
    ("write_file", ["write_fiel", "wirte_file"]),
    ("create_issue", ["create_isue", "craete_issue"]),
    ("run_command", ["run_comand", "run_commnad"]),
    ("delete_path", ["delete_pth", "delte_path"]),
];

const HALLUCINATED: [&str; 6] = [
    "send_email",
    "translate_text",
    "book_flight",
    "resize_image",
    "post_message",
    "query_database",
];

const KEY_TYPOS: [(&str, &str); 20] = [
    ("path", "pth"),
    ("limit", "limt"),
    ("encoding", "encodng"),
    ("query", "qeury"),
    ("mode", "mdoe"),
    ("paths", "pahts"),
    ("city", "ctiy"),
    ("unit", "unti"),
    ("days", "dyas"),
    ("options", "optons"),
    ("theme", "thme"),
    ("content", "conten"),
    ("append", "apend"),
    ("title", "titel"),
    ("priority", "priorty"),
    ("labels", "lables"),
    ("assignee", "asignee"),
    ("command", "comand"),
    ("timeout_seconds", "timeout_secnods"),
    ("recursive", "recursve"),
];

const ENUM_TYPOS: [(&str, &str); 14] = [
    ("utf-8", "uft-8"),
    ("latin-1", "latni-1"),
    ("ascii", "acsii"),
    ("literal", "litreal"),
    ("regex", "rgex"),
    ("fuzzy", "fuzy"),
    ("celsius", "celsuis"),
    ("fahrenheit", "farenheit"),
    ("light", "ligth"),
    ("dark", "drak"),
    ("medium", "meduim"),
    ("high", "hihg"),
    ("bash", "bsah"),
    ("zsh", "zhs"),
];

fn lookup(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(from, _)| *from == key)
        .map(|(_, to)| *to)
}

const fn is_string_shape(shape: &ValueShape) -> bool {
    matches!(shape, ValueShape::String(_))
}

const fn is_typed_scalar(shape: &ValueShape) -> bool {
    matches!(
        shape,
        ValueShape::Integer | ValueShape::Number | ValueShape::Boolean | ValueShape::Enumeration(_)
    )
}

/// Whether a string can be written as a bare JSON token and read back
/// unchanged.
fn bare_safe(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        && text
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        && !matches!(
            text.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "nan" | "infinity"
        )
}

/// Every single-call mutation of `base`.
#[allow(clippy::too_many_lines)] // one flat list of independent mutations
fn mutations(base: &Base, random: &mut XorShift) -> Vec<Mutant> {
    let mut out = Vec::new();
    let text = &base.text;
    let wrapped = !matches!(
        base.dialect,
        DialectId::OpenAiToolCalls | DialectId::MistralV11
    );
    let xml = base.form == Form::Xml;
    let json_form = !xml;

    // Controls: the witness as printed.
    let mut canonical = mutant("canonical", "witness", text.clone(), Need::Exact);
    if base.seed % 2 == 1 {
        canonical.stop = Ending::ToolCalls;
    }
    out.push(canonical);

    // Names.
    let near = NEAR_MISS
        .iter()
        .find(|(name, _)| *name == base.call.name)
        .map(|(_, typos)| typos[usize::try_from(base.seed % 2).unwrap_or(0)])
        .expect("every tool has near misses");
    out.push(mutant(
        "tool_call.name.match",
        "near-miss",
        base.with_name(near),
        Need::Guess,
    ));
    let hallucinated =
        HALLUCINATED[(base.tool + usize::try_from(base.seed).unwrap_or(0)) % HALLUCINATED.len()];
    out.push(mutant(
        "tool_call.name.match",
        "hallucinated",
        base.with_name(hallucinated),
        Need::Never,
    ));
    let alias = match base.call.name.as_str() {
        "read_file" => Some("cat"),
        "write_file" => Some("save_file"),
        _ => None,
    };
    if let Some(alias) = alias {
        out.push(mutant(
            "tool_call.name.alias",
            "alias",
            base.with_name(alias),
            Need::Exact,
        ));
    }

    // Parameter names.
    let members = base.members();
    if !members.is_empty() {
        let index = random.below(members.len());
        let key = &members[index].key;
        let typo = lookup(&KEY_TYPOS, key).expect("every parameter has a typo");
        out.push(mutant(
            "json.key.match",
            format!("{key}-{typo}"),
            rename_member(base, index, typo),
            Need::Guess,
        ));
    }
    for (index, member) in members.iter().enumerate() {
        if let Some(alias) = &base.declared(&member.key).alias {
            out.push(mutant(
                "tool_call.parameter.alias",
                format!("{}-{alias}", member.key),
                rename_member(base, index, alias),
                Need::Exact,
            ));
        }
    }

    // Unknown, repeated and missing parameters.
    out.push(mutant(
        "tool_call.parameter.drop-unknown",
        "zz_trace",
        add_member(base, "zz_trace", "true"),
        Need::Unknown,
    ));
    let strings = base.members_where(|declared, value| {
        is_string_shape(&declared.shape) && matches!(value.value, Value::String(_))
    });
    if let Some(&index) = strings.first() {
        let key = members[index].key.clone();
        let other = match &base.declared(&key).shape {
            ValueShape::String(Some(pattern)) if pattern.source.starts_with("^[A-Z]") => "Zother",
            ValueShape::String(Some(_)) => "zzother",
            _ => "another value",
        };
        out.push(mutant(
            "tool_call.parameter.repeated",
            key.clone(),
            add_member(
                base,
                &key,
                &if xml {
                    other.to_owned()
                } else {
                    json::quote(other)
                },
            ),
            Need::Never,
        ));
    }
    let required = base.members_where(|declared, _| declared.required);
    if !required.is_empty() {
        let index = required[random.below(required.len())];
        out.push(mutant(
            "json.required.invent",
            members[index].key.clone(),
            remove_member(base, index),
            Need::Never,
        ));
    }

    // Enumeration values.
    let enums =
        base.members_where(|declared, _| matches!(declared.shape, ValueShape::Enumeration(_)));
    if let Some(&index) = enums.first() {
        let literal = members[index]
            .value
            .as_str()
            .expect("an enum literal")
            .to_owned();
        let changed = if base.seed.is_multiple_of(2) {
            literal.to_uppercase()
        } else {
            let mut characters = literal.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + characters.as_str()
            })
        };
        if changed != literal {
            out.push(mutant(
                "tool_call.value.enum-normalize",
                format!("{literal}-{changed}"),
                replace_value(base, index, &changed),
                Need::Exact,
            ));
        }
        if let Some(typo) = lookup(&ENUM_TYPOS, &literal) {
            out.push(mutant(
                "json.enum.match",
                format!("{literal}-{typo}"),
                replace_value(base, index, typo),
                Need::Guess,
            ));
        }
    }

    // Context: fences, reasoning, trailing markup.
    out.push(mutant(
        "tool_call.context.fenced-example",
        "fence",
        format!("For example:\n```\n{text}\n```\n"),
        Need::Guess,
    ));
    let mut unclosed = mutant(
        "tool_call.reasoning.unclosed",
        "think",
        format!("<think>I should call the tool now.\n{text}"),
        Need::Guess,
    );
    unclosed.reasoning = true;
    out.push(unclosed);
    let mut embedded = mutant(
        "tool_call.reasoning.embedded-call",
        "think",
        format!("<think>Maybe: {text}</think>\nNo tool call is needed."),
        Need::Never,
    );
    embedded.reasoning = true;
    out.push(embedded);
    let mut unterminated = mutant(
        "tool_call.reasoning.unterminated",
        "think",
        format!("{text}\n<think>Checking the result next"),
        Need::Exact,
    );
    unterminated.reasoning = true;
    out.push(unterminated);
    out.push(mutant(
        "tool_call.markup.dangling-opener",
        "tool_call",
        format!("{text}\n<tool_call>"),
        Need::Exact,
    ));

    // Truncation.
    let mut complete = mutant("tool_call.truncated", "complete", text.clone(), Need::Exact);
    complete.stop = Ending::Length;
    out.push(complete);
    let split = base.split();
    let arguments = base.arguments_text();
    let mut cut = split.prefix.len() + arguments.len() / 2 + random.below(arguments.len() / 2 + 1);
    cut = cut.min(text.len() - 1);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut truncated = mutant(
        "tool_call.truncated",
        "cut",
        text[..cut].to_owned(),
        Need::Never,
    );
    truncated.stop = Ending::Length;
    out.push(truncated);

    // Wrapper closers (Qwen and Hermes; the object's final brace for
    // Llama 3 JSON, which has no variant spelling and no stop sequence).
    if wrapped {
        let end = base.wrapper_end();
        let closer = base.closer();
        let tagged = !matches!(base.dialect, DialectId::Llama3Json | DialectId::MistralV3);
        let variant = match base.closer() {
            "]" => "bracket",
            "}" => "brace",
            DEEPSEEK_SECTION_END => "tool_calls_end",
            _ => "tool_call",
        };
        out.push(mutant(
            "tool_call.markup.missing-closers-at-end",
            variant,
            text[..end].to_owned(),
            Need::Exact,
        ));
        if tagged {
            let mut consumed = mutant(
                "tool_call.stop.consumed-closer",
                variant,
                text[..end].to_owned(),
                Need::Exact,
            );
            consumed.stop = Ending::Sequence(closer);
            out.push(consumed);
        }
        out.push(mutant(
            "tool_call.markup.missing-wrapper-close.before-text",
            "done",
            format!("{}\nDone.", &text[..end]),
            Need::Guess,
        ));
        out.push(mutant(
            "tool_call.markup.stray-closer",
            variant,
            format!("{text}\n{closer}"),
            Need::Exact,
        ));
        if tagged && !matches!(base.dialect, DialectId::DeepSeekV31 | DialectId::DeepSeekV3) {
            out.push(mutant(
                "tool_call.markup.closer-variant",
                "tool_calls",
                format!("{}</tool_calls>", &text[..end]),
                Need::Exact,
            ));
        }
    }

    if xml {
        xml_mutations(base, random, &mut out);
    }
    if json_form {
        json_mutations(base, random, &mut out);
    }
    if base.dialect == DialectId::Hermes {
        hermes_mutations(base, &mut out);
    }
    if base.dialect == DialectId::MistralV3 {
        mistral_mutations(base, &mut out);
    }
    if base.dialect == DialectId::MistralV11 {
        mistral11_mutations(base, &mut out);
    }
    if matches!(base.dialect, DialectId::DeepSeekV31 | DialectId::DeepSeekV3) {
        deepseek31_mutations(base, &mut out);
    }
    if base.dialect == DialectId::DeepSeekV3 {
        deepseek3_mutations(base, &mut out);
    }
    out
}

/// The closer of a DeepSeek-V3.1 or V3 tool-calls section.
const DEEPSEEK_SECTION_END: &str = "<｜tool▁calls▁end｜>";
/// The opener of a DeepSeek-V3.1 or V3 tool-calls section.
const DEEPSEEK_SECTION_BEGIN: &str = "<｜tool▁calls▁begin｜>";
/// The opener of one DeepSeek-V3.1 or V3 call.
const DEEPSEEK_CALL_BEGIN: &str = "<｜tool▁call▁begin｜>";

/// DeepSeek-V3.1 and V3: the special tokens spelled with ASCII `|` and `_`
/// (a detokenizer's rendering), and the section opener left out.
fn deepseek31_mutations(base: &Base, out: &mut Vec<Mutant>) {
    out.push(mutant(
        "tool_call.markup.terminal-confusable",
        "ascii",
        base.text.replace('｜', "|").replace('▁', "_"),
        Need::Exact,
    ));
    if let Some(rest) = base.text.strip_prefix(DEEPSEEK_SECTION_BEGIN) {
        out.push(mutant(
            "tool_call.markup.missing-token",
            "section-begin",
            rest.trim_start().to_owned(),
            Need::Exact,
        ));
    }
}

/// DeepSeek-V3: the fenced arguments block with a fence line missing, bare
/// (no `json` info string), naming another language, or repeated, and the
/// call's `function` type missing or replaced.
fn deepseek3_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let split = base.split();
    let arguments = base.arguments_text();
    let (Some(open), Some(close)) = (split.prefix.rfind("```json"), split.suffix.find("```"))
    else {
        return;
    };
    let head = &split.prefix[..open];
    let after_open = &split.prefix[open + "```json".len()..];
    let before_close = &split.suffix[..close];
    let tail = &split.suffix[close + "```".len()..];
    let fenced = |open_fence: &str, close_fence: &str| {
        format!("{head}{open_fence}{after_open}{arguments}{before_close}{close_fence}{tail}")
    };
    for (category, variant, text) in [
        (
            "tool_call.markup.missing-token",
            "open-fence",
            fenced("", "```"),
        ),
        (
            "tool_call.markup.missing-token",
            "fence-info",
            fenced("```", "```"),
        ),
        (
            "tool_call.markup.missing-token",
            "close-fence",
            fenced("```json", ""),
        ),
        (
            "tool_call.extract.fence",
            "repeated-fences",
            fenced("```json\n```json", "```\n```"),
        ),
        (
            "tool_call.extract.fence",
            "other-info",
            fenced("```javascript", "```"),
        ),
    ] {
        out.push(mutant(category, variant, text, Need::Exact));
    }
    // The type word right after the call opener.
    let Some(at) = base.text.find(DEEPSEEK_CALL_BEGIN) else {
        return;
    };
    let at = at + DEEPSEEK_CALL_BEGIN.len();
    let at = at + (base.text[at..].len() - base.text[at..].trim_start().len());
    if base.text[at..].starts_with("function") {
        let rest = &base.text[at + "function".len()..];
        out.push(mutant(
            "tool_call.markup.missing-token",
            "type",
            format!("{}{rest}", &base.text[..at]),
            Need::Exact,
        ));
        out.push(mutant(
            "tool_call.extract.prose",
            "type",
            format!("{}tool{rest}", &base.text[..at]),
            Need::DataLoss,
        ));
    }
}

/// Mistral v11: the `[ARGS]` token left out between the name (or id) and
/// an arguments object, read as inserted.
fn mistral11_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let split = base.split();
    if base.form == Form::Object
        && let Some(at) = split.prefix.rfind("[ARGS]")
    {
        out.push(mutant(
            "tool_call.markup.missing-token",
            "args",
            format!(
                "{}{}{}{}",
                &split.prefix[..at],
                &split.prefix[at + "[ARGS]".len()..],
                base.arguments_text(),
                split.suffix
            ),
            Need::Exact,
        ));
    }
}

/// The parts of a bracketed Mistral v3 witness: the text through the token,
/// the array's inside, and the text after the array.
fn mistral_array(base: &Base) -> Option<(&str, &str, &str)> {
    let text = &base.text;
    let after = text.find("[TOOL_CALLS]")? + "[TOOL_CALLS]".len();
    if !text[after..].trim_start().starts_with('[') || base.closer() != "]" {
        return None;
    }
    let open = after + text[after..].find('[')?;
    let close = text.rfind(']')?;
    Some((&text[..open], &text[open + 1..close], &text[close + 1..]))
}

/// Mistral v3: the call array written without its brackets, read as the
/// same call with each omitted bracket recorded as inserted.
fn mistral_mutations(base: &Base, out: &mut Vec<Mutant>) {
    if let Some((head, inside, tail)) = mistral_array(base) {
        out.push(mutant(
            "tool_call.markup.missing-token",
            "bare-object",
            format!("{head}{inside}{tail}"),
            Need::Exact,
        ));
    }
}

/// Rename member `index` to `key`.
fn rename_member(base: &Base, index: usize, key: &str) -> String {
    if base.form == Form::Xml {
        let mut elements = base.xml_elements();
        key.clone_into(&mut elements[index].0);
        base.with_arguments(&xml_body(&elements))
    } else {
        let mut printed = base.printed();
        printed[index].key = json::quote(key);
        base.with_arguments(&Base::json_text(&printed))
    }
}

/// Append a member `key` with value text `value` (XML text or JSON).
fn add_member(base: &Base, key: &str, value: &str) -> String {
    if base.form == Form::Xml {
        let mut elements = base.xml_elements();
        elements.push((key.to_owned(), value.to_owned(), true));
        base.with_arguments(&xml_body(&elements))
    } else {
        let mut printed = base.printed();
        printed.push(Printed {
            key: json::quote(key),
            value: value.to_owned(),
        });
        base.with_arguments(&Base::json_text(&printed))
    }
}

fn remove_member(base: &Base, index: usize) -> String {
    if base.form == Form::Xml {
        let mut elements = base.xml_elements();
        elements.remove(index);
        base.with_arguments(&xml_body(&elements))
    } else {
        let mut printed = base.printed();
        printed.remove(index);
        base.with_arguments(&Base::json_text(&printed))
    }
}

/// Replace member `index`'s string value with `text` (verbatim in XML,
/// quoted in JSON).
fn replace_value(base: &Base, index: usize, text: &str) -> String {
    if base.form == Form::Xml {
        let mut elements = base.xml_elements();
        text.clone_into(&mut elements[index].1);
        base.with_arguments(&xml_body(&elements))
    } else {
        let mut printed = base.printed();
        printed[index].value = json::quote(text);
        base.with_arguments(&Base::json_text(&printed))
    }
}

/// Mutations of the `xml-params` body (Qwen3-Coder XML).
#[allow(clippy::too_many_lines)] // one flat list of independent mutations
fn xml_mutations(base: &Base, random: &mut XorShift, out: &mut Vec<Mutant>) {
    let text = &base.text;
    let elements = base.xml_elements();
    let members = base.members();
    let function_close = text.rfind("</function>").expect("a function closer");

    out.push(mutant(
        "tool_call.markup.missing-function-close",
        "function",
        format!(
            "{}{}",
            &text[..function_close],
            &text[function_close + "</function>".len()..]
        ),
        Need::Exact,
    ));
    out.push(mutant(
        "tool_call.markup.closer-variant",
        "functions",
        format!(
            "{}</functions>{}",
            &text[..function_close],
            &text[function_close + "</function>".len()..]
        ),
        Need::Exact,
    ));
    out.push(mutant(
        "tool_call.markup.stray-closer",
        "function",
        format!(
            "{}</function>\n{}",
            &text[..function_close],
            &text[function_close..]
        ),
        Need::Exact,
    ));
    if !elements.is_empty() {
        let index = random.below(elements.len());
        let mut body = xml_body(&elements);
        let closer = nth_parameter_close(&body, index);
        body.replace_range(closer..closer + "</parameter>".len(), "</parameters>");
        out.push(mutant(
            "tool_call.markup.closer-variant",
            "parameters",
            base.with_arguments(&body),
            Need::Exact,
        ));
        let mut body = xml_body(&elements);
        let closer = nth_parameter_close(&body, index);
        body.insert_str(closer, "</parameter>\n");
        out.push(mutant(
            "tool_call.markup.stray-closer",
            "parameter",
            base.with_arguments(&body),
            Need::Exact,
        ));
        // Text between a string value's closer and a second closer: the
        // value may end at either, so its boundary is ambiguous.
        let strings = base.members_where(|declared, _| is_string_shape(&declared.shape));
        if let Some(&index) = strings.first() {
            let mut body = xml_body(&elements);
            let closer = nth_parameter_close(&body, index) + "</parameter>\n".len();
            body.insert_str(closer, "more text\n</parameter>\n");
            out.push(mutant(
                "tool_call.markup.stray-closer",
                "text-between",
                base.with_arguments(&body),
                Need::Never,
            ));
        }
        // The last element loses its closer: the function closer ends it.
        let mut open = elements.clone();
        let last = open.len() - 1;
        open[last].2 = false;
        out.push(mutant(
            "tool_call.markup.parameter-closed-by-element",
            open[last].0.clone(),
            base.with_arguments(&xml_body(&open)),
            Need::Exact,
        ));
    }
    if elements.len() >= 2 {
        let index = random.below(elements.len() - 1);
        let mut open = elements.clone();
        open[index].2 = false;
        out.push(mutant(
            "tool_call.markup.missing-parameter-close",
            open[index].0.clone(),
            base.with_arguments(&xml_body(&open)),
            Need::Exact,
        ));
    }
    // The whole arguments object as the function body.
    out.push(mutant(
        "tool_call.markup.json-body",
        "object",
        base.with_arguments(&format!("\n{}\n", base.call.arguments.to_json())),
        Need::Exact,
    ));
    // Closers only: `{}` when nothing was passed, a missing required
    // parameter otherwise.
    let closers_only = "\n</parameter>\n</parameters>\n";
    if members.is_empty() {
        out.push(mutant(
            "tool_call.markup.closers-only-empty",
            "empty",
            base.with_arguments(closers_only),
            Need::Exact,
        ));
    } else if base.params.iter().any(|param| param.required) {
        out.push(mutant(
            "tool_call.markup.closers-only-empty",
            "required",
            base.with_arguments(closers_only),
            Need::Never,
        ));
    }
    // Typed values padded with whitespace.
    let typed = base.members_where(|declared, _| is_typed_scalar(&declared.shape));
    if !typed.is_empty() {
        let index = typed[random.below(typed.len())];
        let mut padded = elements.clone();
        padded[index].1 = format!("  {}\t ", padded[index].1);
        out.push(mutant(
            "tool_call.value.trim-nonverbatim",
            padded[index].0.clone(),
            base.with_arguments(&xml_body(&padded)),
            Need::Exact,
        ));
    }
    // Object parameters as nested elements.
    for (index, member) in members.iter().enumerate() {
        let Value::Object(inner) = &member.value.value else {
            continue;
        };
        let ValueShape::Object(shape) = &base.declared(&member.key).shape else {
            continue;
        };
        if inner.is_empty() {
            continue;
        }
        let nested: Vec<(String, String, bool)> = inner
            .iter()
            .map(|field| {
                let declared = shape
                    .parameters
                    .iter()
                    .find(|param| param.name == field.key)
                    .expect("a declared member");
                (field.key.clone(), raw(&field.value, &declared.value), true)
            })
            .collect();
        let mut outer = elements.clone();
        let inner_body = xml_body(&nested);
        // `<parameter=options>\n` + inner elements + `</parameter>`.
        inner_body
            .trim_end_matches('\n')
            .trim_start_matches('\n')
            .clone_into(&mut outer[index].1);
        out.push(mutant(
            "tool_call.markup.nested-parameter",
            member.key.clone(),
            base.with_arguments(&xml_body(&outer)),
            Need::Exact,
        ));
    }
    // Array parameters as plain items.
    for (index, member) in members.iter().enumerate() {
        let Value::Array(items) = &member.value.value else {
            continue;
        };
        let texts: Vec<&str> = items.iter().filter_map(Node::as_str).collect();
        if texts.len() != items.len() || texts.is_empty() {
            continue;
        }
        let mut spread: Vec<(String, String, bool)> = Vec::new();
        for (position, element) in elements.iter().enumerate() {
            if position == index {
                for item in &texts {
                    spread.push((element.0.clone(), (*item).to_owned(), true));
                }
            } else {
                spread.push(element.clone());
            }
        }
        let category = if texts.len() == 1 {
            "tool_call.xml.array-item"
        } else {
            "tool_call.parameter.collect"
        };
        out.push(mutant(
            category,
            member.key.clone(),
            base.with_arguments(&xml_body(&spread)),
            Need::Exact,
        ));
    }
    // An undeclared tool whose value runs into another `<parameter=`.
    if elements.len() >= 2 {
        let mut open = elements;
        open[0].2 = false;
        let renamed = base.with_arguments(&xml_body(&open)).replacen(
            &format!("<function={}>", base.call.name),
            &format!(
                "<function={}>",
                HALLUCINATED[base.tool % HALLUCINATED.len()]
            ),
            1,
        );
        out.push(mutant(
            "tool_call.markup.undeclared-parameter-boundary",
            "hallucinated",
            renamed,
            Need::Never,
        ));
    }
}

/// The byte offset of the `index`-th `</parameter>` in `body`.
fn nth_parameter_close(body: &str, index: usize) -> usize {
    body.match_indices("</parameter>")
        .nth(index)
        .map(|(at, _)| at)
        .expect("a parameter closer")
}

/// Mutations of JSON arguments (Hermes and `OpenAI`).
#[allow(clippy::too_many_lines)] // one flat list of independent mutations
fn json_mutations(base: &Base, random: &mut XorShift, out: &mut Vec<Mutant>) {
    let printed = base.printed();
    let members = base.members();
    let object_text = Base::json_text(&printed);
    assert_eq!(
        object_text,
        base.call.arguments.to_json(),
        "member printing"
    );

    if base.form == Form::Object {
        let split = base.split();
        out.push(mutant(
            "tool_call.arguments.decode-string",
            "stringify",
            format!(
                "{}{}{}",
                split.prefix,
                json::quote(&object_text),
                split.suffix
            ),
            Need::Exact,
        ));
    }
    if !printed.is_empty() {
        let index = random.below(printed.len());
        let mut commented = printed.clone();
        commented[index].value.push_str(" /* note */");
        out.push(mutant(
            "json.syntax.comment",
            printed[index].key.trim_matches('"').to_owned(),
            base.with_arguments(&Base::json_text(&commented)),
            Need::Exact,
        ));
        let mut unquoted_key = printed.clone();
        unquoted_key[index].key.clone_from(&members[index].key);
        out.push(mutant(
            "json.syntax.bare-key",
            members[index].key.clone(),
            base.with_arguments(&Base::json_text(&unquoted_key)),
            Need::Exact,
        ));
        let mut colon = Base::json_text(&printed);
        let at = 1 + printed[0].key.len();
        colon.replace_range(at..=at, " ");
        out.push(mutant(
            "json.syntax.missing-colon",
            members[0].key.clone(),
            base.with_arguments(&colon),
            Need::Exact,
        ));
        let trailing = format!("{},}}", &object_text[..object_text.len() - 1]);
        out.push(mutant(
            "json.syntax.trailing-comma",
            "end",
            base.with_arguments(&trailing),
            Need::Exact,
        ));
    }
    if printed.len() >= 2 {
        let mut text = printed[0].key.clone() + ":" + &printed[0].value;
        text.push(' ');
        let rest: Vec<String> = printed[1..]
            .iter()
            .map(|member| format!("{}:{}", member.key, member.value))
            .collect();
        text.push_str(&rest.join(","));
        out.push(mutant(
            "json.syntax.missing-comma",
            members[0].key.clone(),
            base.with_arguments(&format!("{{{text}}}")),
            Need::Exact,
        ));
    }
    // Booleans in another case.
    let booleans = base.members_where(|_, value| matches!(value.value, Value::Bool(_)));
    if let Some(&index) = booleans.first() {
        let mut cased = printed.clone();
        cased[index].value = if cased[index].value == "true" {
            "True".to_owned()
        } else {
            "FALSE".to_owned()
        };
        out.push(mutant(
            "json.syntax.literal-case",
            members[index].key.clone(),
            base.with_arguments(&Base::json_text(&cased)),
            Need::Exact,
        ));
    }
    // Single-quoted strings (no escapes to reinterpret).
    let plain = base.members_where(|_, value| {
        value.as_str().is_some_and(|text| {
            !text.contains(['\'', '"', '\\']) && !text.chars().any(char::is_control)
        })
    });
    if let Some(&index) = plain.first() {
        let mut quoted = printed.clone();
        quoted[index].value = format!("'{}'", members[index].value.as_str().unwrap_or_default());
        out.push(mutant(
            "json.syntax.string",
            members[index].key.clone(),
            base.with_arguments(&Base::json_text(&quoted)),
            Need::Exact,
        ));
    }
    // Bare string tokens (a guess).
    let tokens = base.members_where(|declared, value| {
        is_string_shape(&declared.shape) && value.as_str().is_some_and(bare_safe)
    });
    if let Some(&index) = tokens.first() {
        let mut unquoted = printed.clone();
        members[index]
            .value
            .as_str()
            .unwrap_or_default()
            .clone_into(&mut unquoted[index].value);
        out.push(mutant(
            "json.syntax.bare-string",
            members[index].key.clone(),
            base.with_arguments(&Base::json_text(&unquoted)),
            Need::Guess,
        ));
    }
    // A missing value: `null` never validates a declared type.
    let optional = base.members_where(|declared, _| !declared.required);
    if let Some(&index) = optional.first() {
        let mut missing = printed.clone();
        missing[index].value = String::new();
        out.push(mutant(
            "json.syntax.missing-value",
            members[index].key.clone(),
            base.with_arguments(&Base::json_text(&missing)),
            Need::Never,
        ));
    }
    // Values of the wrong JSON type are never coerced.
    let integers = base.members_where(|declared, _| declared.shape == ValueShape::Integer);
    if let Some(&index) = integers.first() {
        let mut quoted = printed.clone();
        quoted[index].value = json::quote(&printed[index].value);
        out.push(mutant(
            "json.type.decode-string",
            members[index].key.clone(),
            base.with_arguments(&Base::json_text(&quoted)),
            Need::Never,
        ));
    }
    let free = base.members_where(|declared, _| declared.shape == ValueShape::String(None));
    if let Some(&index) = free.first() {
        let mut number = printed.clone();
        "12345".clone_into(&mut number[index].value);
        out.push(mutant(
            "json.type.stringify-scalar",
            members[index].key.clone(),
            base.with_arguments(&Base::json_text(&number)),
            Need::Never,
        ));
    }
    // Required arguments nested one level down.
    let required = base.members_where(|declared, _| declared.required);
    if !required.is_empty() {
        let nested: Vec<Printed> = required
            .iter()
            .map(|&index| printed[index].clone())
            .collect();
        let mut outer: Vec<Printed> = printed
            .iter()
            .enumerate()
            .filter(|(index, _)| !required.contains(index))
            .map(|(_, member)| member.clone())
            .collect();
        outer.insert(
            0,
            Printed {
                key: json::quote("args"),
                value: Base::json_text(&nested),
            },
        );
        out.push(mutant(
            "json.key.relocate",
            "args",
            base.with_arguments(&Base::json_text(&outer)),
            Need::Guess,
        ));
    }
    // Inside stringified arguments only: a missing final brace, and a
    // final string cut before its quote.
    if base.form == Form::Stringified {
        out.push(mutant(
            "json.syntax.close-container",
            "end",
            base.with_arguments(&object_text[..object_text.len() - 1]),
            Need::Exact,
        ));
        if printed
            .last()
            .is_some_and(|member| member.value.starts_with('"'))
        {
            out.push(mutant(
                "json.syntax.unterminated-string",
                "end",
                base.with_arguments(&object_text[..object_text.len() - 2]),
                Need::Guess,
            ));
        }
    }
}

/// Mutations of the Hermes wrapper's inside.
fn hermes_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let text = &base.text;
    let open = text.find('{').expect("the call object");
    let end = base.wrapper_end();
    let close = text[..end].rfind('}').expect("the call object's end");
    out.push(mutant(
        "tool_call.extract.prose",
        "preamble",
        format!("{}Sure, here is the call: {}", &text[..open], &text[open..]),
        Need::DataLoss,
    ));
    out.push(mutant(
        "tool_call.extract.fence",
        "json",
        format!(
            "{}```json\n{}\n```\n{}",
            &text[..open],
            &text[open..=close],
            &text[end..]
        ),
        Need::Exact,
    ));
}

// ---------------------------------------------------------------------
// Turns of two calls.
// ---------------------------------------------------------------------

/// Mutations of a two-call turn in one dialect (`first` then `second`).
fn pair_mutations(first: &Base, second: &Base) -> Vec<Mutant> {
    let joined = format!("{}\n{}", first.text, second.text);
    let mut out = vec![mutant("canonical", "pair", joined, Need::Exact)];
    if first.dialect == DialectId::OpenAiToolCalls {
        let id = |base: &Base| {
            let at = base.text.find("\"call_")?;
            let end = base.text[at + 1..].find('"')? + at + 1;
            Some(base.text[at..=end].to_owned())
        };
        if let (Some(one), Some(two)) = (id(first), id(second)) {
            out.push(mutant(
                "tool_call.id.dedup",
                "duplicate",
                format!("[{},\n{}]", first.text, second.text.replacen(&two, &one, 1)),
                Need::Exact,
            ));
        }
    } else {
        if matches!(
            first.dialect,
            DialectId::DeepSeekV31 | DialectId::DeepSeekV3
        ) && let Some(head) = first.text.strip_suffix(DEEPSEEK_SECTION_END)
            && let Some(tail) = second.text.strip_prefix(DEEPSEEK_SECTION_BEGIN)
        {
            // Both calls in one section (V3 separates them with a newline),
            // and the same with the first call's closer left out.
            let head = head.trim_end();
            let tail = tail.trim_start();
            let sep = if first.dialect == DialectId::DeepSeekV3 {
                "\n"
            } else {
                ""
            };
            out.push(mutant(
                "canonical",
                "one-section",
                format!("{head}{sep}{tail}"),
                Need::Exact,
            ));
            if let Some(open) = head.strip_suffix("<｜tool▁call▁end｜>") {
                out.push(mutant(
                    "tool_call.markup.missing-function-close",
                    "call-end",
                    format!("{}{sep}{tail}", open.trim_end()),
                    Need::Exact,
                ));
            }
        }
        if first.dialect == DialectId::MistralV11
            && let Some(rest) = second.text.trim_start().strip_prefix("[TOOL_CALLS]")
            && second.split().prefix.contains("[ARGS]")
        {
            // The second call's [TOOL_CALLS] left out.
            out.push(mutant(
                "tool_call.markup.missing-token",
                "second-token",
                format!("{}\n{}", first.text, rest.trim_start()),
                Need::Exact,
            ));
        }
        if let (Some((head, first_inside, _)), Some((_, second_inside, _))) =
            (mistral_array(first), mistral_array(second))
        {
            // Two calls of one unbracketed list, one per line.
            out.push(mutant(
                "tool_call.markup.missing-token",
                "unbracketed-pair",
                format!("{head}{}\n{}", first_inside.trim(), second_inside.trim()),
                Need::Exact,
            ));
        }
        if first.dialect != DialectId::MistralV11 {
            let end = first.wrapper_end();
            out.push(mutant(
                "tool_call.markup.missing-wrapper-close",
                "first",
                format!("{}\n{}", &first.text[..end], second.text),
                Need::Exact,
            ));
            out.push(mutant(
                "tool_call.markup.stray-closer",
                "between",
                format!("{}\n{}\n{}", first.text, first.closer(), second.text),
                Need::Exact,
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------
// The corpus.
// ---------------------------------------------------------------------

/// Whether a mutant needing `need` may still be run, for a tool with
/// `effects` under `policy`: the corpus truth rule (see the module
/// documentation).
const fn answerable(need: Need, effects: Effects, policy: Policy) -> bool {
    let read_only = matches!(effects, Effects::ReadOnly);
    match need {
        Need::Exact => true,
        Need::DataLoss | Need::Unknown => read_only,
        Need::Guess => read_only && matches!(policy.effective(effects), RepairPolicy::Tolerant),
        Need::Never => false,
    }
}

const fn policy_of(seed: u64) -> Policy {
    match seed % 4 {
        2 => Policy::Structural,
        3 => Policy::Tolerant,
        _ => Policy::PerTool,
    }
}

const ALL_DIALECTS: &str = "deepseek-v3.1.v1,deepseek-v3.v1,hermes.v1,llama3-json.v1,mistral-v11.v1,mistral-v3.v1,openai-tool-calls.v1,qwen3-coder-xml.v1";

/// The cases of every single-call mutation of `base`, appended to `cases`;
/// with `under`, only those of categories with fewer than [`TARGET`] cases
/// (counted as they are added).
fn single_cases(
    base: &Base,
    mut under: Option<&mut BTreeMap<String, usize>>,
    cases: &mut Vec<Case>,
) {
    let mut random = XorShift::new(
        (u64::try_from(base.tool).unwrap_or(0) << 16)
            ^ (base.seed << 8)
            ^ u64::from(dialect_label(&base.dialect).as_bytes()[0]),
    );
    let policy = policy_of(base.seed);
    for mutant in mutations(base, &mut random) {
        if let Some(counts) = under.as_deref_mut() {
            let count = counts.entry(mutant.category.to_owned()).or_default();
            if *count >= TARGET {
                continue;
            }
            *count += 1;
        }
        let single = base.seed.is_multiple_of(2) && !mutant.reasoning;
        let truth = answerable(mutant.need, base.effects, policy).then(|| vec![base.call.clone()]);
        cases.push(Case {
            id: format!(
                "{}/{}/s{}/{}/{}",
                base.label(),
                base.call.name,
                base.seed,
                mutant.category,
                mutant.variant
            ),
            category: mutant.category.to_owned(),
            dialect: if single {
                base.dialect.id().to_owned()
            } else {
                ALL_DIALECTS.to_owned()
            },
            policy,
            stop: mutant.stop,
            input: mutant.text,
            truth,
        });
    }
}

/// DeepSeek-V3.1 and V3 sections in one turn, appended to `cases`: the two
/// dialects share their section and call tokens, so each section must be
/// read by its own dialect alone.
fn deepseek_mixed_cases(bases: &[Base], cases: &mut Vec<Case>) {
    let v31: Vec<&Base> = bases
        .iter()
        .filter(|base| base.dialect == DialectId::DeepSeekV31)
        .collect();
    let v3: Vec<&Base> = bases
        .iter()
        .filter(|base| base.dialect == DialectId::DeepSeekV3)
        .collect();
    for (index, (first, second)) in v31.iter().zip(v3.iter().rev()).enumerate() {
        let (first, second) = if index % 2 == 0 {
            (*first, *second)
        } else {
            (*second, *first)
        };
        cases.push(Case {
            id: format!(
                "mixed/{}:{}+{}:{}/s{}+s{}/canonical/mixed",
                first.label(),
                first.call.name,
                second.label(),
                second.call.name,
                first.seed,
                second.seed
            ),
            category: "canonical".to_owned(),
            dialect: "deepseek-v3.1.v1,deepseek-v3.v1".to_owned(),
            policy: Policy::PerTool,
            stop: Ending::Stop,
            input: format!("{}\n{}", first.text, second.text),
            truth: Some(vec![first.call.clone(), second.call.clone()]),
        });
    }
}

/// Every corpus case, in a fixed order.
///
/// # Panics
/// If a witness cannot be generated or two cases share an identity.
#[must_use]
pub fn generate() -> Vec<Case> {
    let tools = tools();
    let bases = bases(&tools);
    let mut cases = Vec::new();
    for base in &bases {
        single_cases(base, None, &mut cases);
    }
    // Supplementary seeds: rarer shapes (non-empty nested objects, arrays
    // of several strings, stringified Hermes arguments ending in a string,
    // ...) until every category reaches `TARGET` cases or the seeds run
    // out. Only mutants of categories still under target are kept.
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for case in &cases {
        *counts.entry(case.category.clone()).or_default() += 1;
    }
    for dialect in &DIALECTS {
        for tool in 0..tools.tools().len() {
            for seed in SEEDS..SEEDS + SUPPLEMENTARY_SEEDS {
                let base = base(&tools, dialect, tool, seed);
                single_cases(&base, Some(&mut counts), &mut cases);
            }
        }
    }
    // Two calls of one dialect: each tool followed by the next one.
    let per_dialect = tools.tools().len() * usize::try_from(SEEDS).unwrap_or(0);
    for chunk in bases.chunks(per_dialect) {
        for (index, first) in chunk.iter().enumerate() {
            let second = &chunk[(index + usize::try_from(SEEDS).unwrap_or(1) + 1) % chunk.len()];
            for mutant in pair_mutations(first, second) {
                cases.push(Case {
                    id: format!(
                        "{}/{}+{}/s{}+s{}/{}/{}",
                        first.label(),
                        first.call.name,
                        second.call.name,
                        first.seed,
                        second.seed,
                        mutant.category,
                        mutant.variant
                    ),
                    category: mutant.category.to_owned(),
                    dialect: if first.seed % 2 == 0 {
                        first.dialect.id().to_owned()
                    } else {
                        ALL_DIALECTS.to_owned()
                    },
                    policy: policy_of(first.seed),
                    stop: mutant.stop,
                    input: mutant.text,
                    truth: Some(vec![first.call.clone(), second.call.clone()]),
                });
            }
        }
    }
    // Mixed Qwen3-Coder XML and Hermes calls in one turn.
    let qwen: Vec<&Base> = bases
        .iter()
        .filter(|base| base.dialect == DialectId::Qwen3CoderXml)
        .collect();
    let hermes: Vec<&Base> = bases
        .iter()
        .filter(|base| base.dialect == DialectId::Hermes)
        .collect();
    for (index, (first, second)) in qwen.iter().zip(hermes.iter().rev()).enumerate() {
        let (first, second) = if index % 2 == 0 {
            (*first, *second)
        } else {
            (*second, *first)
        };
        let joined = format!("{}\n{}", first.text, second.text);
        let stray = format!("{}\n</tool_call>\n{}", first.text, second.text);
        for (category, variant, input) in [
            ("canonical", "mixed", joined),
            ("tool_call.markup.stray-closer", "mixed", stray),
        ] {
            cases.push(Case {
                id: format!(
                    "mixed/{}:{}+{}:{}/s{}+s{}/{category}/{variant}",
                    first.label(),
                    first.call.name,
                    second.label(),
                    second.call.name,
                    first.seed,
                    second.seed
                ),
                category: category.to_owned(),
                dialect: "hermes.v1,qwen3-coder-xml.v1".to_owned(),
                policy: Policy::PerTool,
                stop: Ending::Stop,
                input,
                truth: Some(vec![first.call.clone(), second.call.clone()]),
            });
        }
    }
    deepseek_mixed_cases(&bases, &mut cases);
    let identities: BTreeSet<&str> = cases.iter().map(|case| case.id.as_str()).collect();
    assert_eq!(identities.len(), cases.len(), "case identities are unique");
    cases
}
