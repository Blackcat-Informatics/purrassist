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
//! whitespace-padded typed value, a JSON syntax slip in the arguments or in
//! a JSON call envelope, a misspelled closer, a byte of a value that is not
//! well-formed UTF-8, a repeated typed parameter with an invalid value, a
//! call running into a reasoning marker, a malformed call between two calls
//! of one attempt, ...). The case's
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
        // Issue #8 section 1: an array of objects, the shape Qwen3-Coder XML
        // writes as repeated nested elements.
        ToolDefinition::new("plan_outfits")
            .effects(Effects::ReadOnly)
            .param(
                "outfits",
                Param::array(Param::object(
                    ObjectSpec::new()
                        .param("wearer", Param::string().required().pattern(PATH_PATTERN))
                        .param("garments", Param::array(Param::string())),
                ))
                .required(),
            ),
    ];
    ToolSet::compile(definitions, Budget::default()).expect("the corpus tool set compiles")
}

/// The adapter tool set (`tool_set: "adapter"` records): an `OpenAI`
/// declaration whose parameters use the schema forms pydantic and MCP
/// servers emit and the typed model does not express (`Optional[...]` as
/// `anyOf` with `null`, a `$ref` to `$defs`, a `oneOf` of constants, a type
/// array), typed in Qwen3-Coder XML by the exact schema itself
/// (`tool_call.value.schema-typed`). `file_tasks` adds the `$ref` item
/// scopes of issue #8 section 1 (gap G2): array items, a recursive `$ref`
/// and a single `$ref` object whose members share the names of outer
/// parameters (`name`, `tags`), and a free-form object item.
#[must_use]
pub fn adapter_tools() -> ToolSet {
    ToolSet::from_openai(
        r##"[{"type":"function","function":{"name":"search_index","description":"Search the index","parameters":{"type":"object","properties":{"query":{"type":"string"},"limit":{"anyOf":[{"type":"integer"},{"type":"null"}],"default":null},"since":{"anyOf":[{"type":"string"},{"type":"null"}],"default":null},"opts":{"$ref":"#/$defs/Opts"},"mode":{"oneOf":[{"const":"fast"},{"const":"safe"}]},"ids":{"type":["array","null"],"items":{"type":"integer"}}},"required":["query"],"$defs":{"Opts":{"type":"object","properties":{"depth":{"type":"integer"},"label":{"type":"string"}},"required":["depth"]}}}}},{"type":"function","function":{"name":"file_tasks","description":"File tasks","parameters":{"type":"object","properties":{"name":{"type":"string"},"tags":{"type":"array","items":{"type":"string"}},"cfg":{"$ref":"#/$defs/Cfg"},"items":{"type":"array","items":{"$ref":"#/$defs/Item"}},"free":{"type":"array","items":{"type":"object","additionalProperties":true}}},"required":["name"],"$defs":{"Cfg":{"type":"object","properties":{"name":{"type":"string"},"mode":{"type":"string"}},"required":["name"]},"Item":{"type":"object","properties":{"name":{"type":"string"},"size":{"type":"integer"},"tags":{"type":"array","items":{"type":"string"}},"children":{"type":"array","items":{"$ref":"#/$defs/Item"}}},"required":["name"]}}}}}]"##,
        Budget::default(),
    )
    .expect("the adapter tool set compiles")
}

/// The tool set a record is recovered against (see [`Case::tool_set`]).
#[must_use]
pub fn tools_for<'t>(record: &Node, tools: &'t ToolSet, adapter: &'t ToolSet) -> &'t ToolSet {
    if record.get("tool_set").and_then(Node::as_str) == Some("adapter") {
        adapter
    } else {
        tools
    }
}

/// The `file_tasks` adapter cases (see [`adapter_tools`]).
fn ref_scope_cases(cases: &mut Vec<Case>) {
    let parameter =
        |name: &str, value: &str| format!("<parameter={name}>\n{value}\n</parameter>\n");
    // Issue #8 section 1, gap G2: the member names of a `$ref` scope belong
    // to that scope even when an outer parameter shares them (`name`,
    // `tags`), at any depth, and a scope that names no member is closed at an
    // enclosing scope's opener only as a guess (a mutating call never runs
    // it). Before the scope graph, these read as siblings or as hoisted
    // top-level parameters, `Resolved`.
    let file =
        |body: &str| format!("<tool_call>\n<function=file_tasks>\n{body}</function>\n</tool_call>");
    let nested = |name: &str, body: &str| format!("<parameter={name}>\n{body}</parameter>\n");
    let ref_entries: Vec<(&str, String, Option<&str>)> = vec![
        (
            "ref-items-colliding-name",
            file(
                &(parameter("name", "party")
                    + &nested(
                        "items",
                        "<parameter=name>Alice</parameter>\n<parameter=size>3</parameter>\n",
                    )
                    + &nested(
                        "items",
                        "<parameter=name>Bob</parameter>\n<parameter=size>4</parameter>\n",
                    )),
            ),
            Some(r#"{"name":"party","items":[{"name":"Alice","size":3},{"name":"Bob","size":4}]}"#),
        ),
        (
            "ref-items-outer-array-collision",
            file(
                &(parameter("tags", "work")
                    + &parameter("name", "chores")
                    + &nested(
                        "items",
                        &(parameter("name", "milk") + &parameter("tags", "home")),
                    )),
            ),
            Some(r#"{"tags":["work"],"name":"chores","items":[{"name":"milk","tags":["home"]}]}"#),
        ),
        (
            "ref-items-recursive",
            file(
                &(parameter("name", "root")
                    + &nested(
                        "items",
                        "<parameter=name>a</parameter><parameter=children><parameter=name>b</parameter></parameter>",
                    )),
            ),
            Some(r#"{"name":"root","items":[{"name":"a","children":[{"name":"b"}]}]}"#),
        ),
        (
            "ref-object-colliding-name",
            file(
                &(parameter("name", "top")
                    + &nested(
                        "cfg",
                        &(parameter("name", "inner") + &parameter("mode", "fast")),
                    )),
            ),
            Some(r#"{"name":"top","cfg":{"name":"inner","mode":"fast"}}"#),
        ),
        (
            "free-object-item-opener-of-outer-name",
            file(&nested("free", &parameter("name", "hoisted"))),
            None,
        ),
    ];
    for (variant, input, truth) in ref_entries {
        for (label, policy) in [
            ("qwen", Policy::PerTool),
            ("structural", Policy::Structural),
        ] {
            cases.push(Case {
                tool_set: Some("adapter"),
                invalid: None,
                id: format!(
                    "adapter/file_tasks/{label}/tool_call.markup.nested-parameter/{variant}"
                ),
                category: "tool_call.markup.nested-parameter".to_owned(),
                dialect: "qwen3-coder-xml.v1".to_owned(),
                policy,
                stop: Ending::Stop,
                input: input.clone(),
                truth: truth.map(|arguments| {
                    vec![Call {
                        name: "file_tasks".to_owned(),
                        arguments: json::parse(arguments).expect("truth arguments"),
                    }]
                }),
            });
        }
    }
}

/// The adapter cases: Qwen3-Coder XML calls to [`adapter_tools`] whose
/// unmodeled parameters the exact schema types, alone and under every
/// built-in dialect, a Hermes JSON control, and text no reading validates
/// (abstain).
fn adapter_cases(cases: &mut Vec<Case>) {
    let qwen = |body: &str| {
        format!("<tool_call>\n<function=search_index>\n{body}</function>\n</tool_call>")
    };
    let parameter =
        |name: &str, value: &str| format!("<parameter={name}>\n{value}\n</parameter>\n");
    let query = parameter("query", "cats");
    let entries: Vec<(&str, &str, String, Option<&str>)> = vec![
        ("optional-int", "tool_call.value.schema-typed", qwen(&(query.clone() + &parameter("limit", "5"))), Some(r#"{"query":"cats","limit":5}"#)),
        ("optional-int-null", "tool_call.value.schema-typed", qwen(&(query.clone() + &parameter("limit", "null"))), Some(r#"{"query":"cats","limit":null}"#)),
        ("optional-str-numeric", "tool_call.value.schema-typed", qwen(&(query.clone() + &parameter("since", "2024"))), Some(r#"{"query":"cats","since":"2024"}"#)),
        ("ref-object-inline", "tool_call.value.schema-typed", qwen(&(query.clone() + &parameter("opts", r#"{"depth": 2, "label": "x"}"#))), Some(r#"{"query":"cats","opts":{"depth":2,"label":"x"}}"#)),
        ("ref-object-nested", "tool_call.value.schema-typed", qwen(&(query.clone() + "<parameter=opts>\n" + &parameter("depth", "3") + &parameter("label", "10") + "</parameter>\n")), Some(r#"{"query":"cats","opts":{"depth":3,"label":"10"}}"#)),
        ("oneof-const", "tool_call.value.schema-typed", qwen(&(query.clone() + &parameter("mode", "safe"))), Some(r#"{"query":"cats","mode":"safe"}"#)),
        ("type-array", "tool_call.value.schema-typed", qwen(&(query.clone() + &parameter("ids", "[1, 2]"))), Some(r#"{"query":"cats","ids":[1,2]}"#)),
        ("unmatched", "tool_call.value.schema-typed", qwen(&(query + &parameter("limit", "many"))), None),
        ("json-control", "canonical", r#"<tool_call>{"name": "search_index", "arguments": {"query": "cats", "limit": 5}}</tool_call>"#.to_owned(), Some(r#"{"query":"cats","limit":5}"#)),
    ];
    ref_scope_cases(cases);
    for (variant, category, input, truth) in entries {
        let single = if category == "canonical" {
            "hermes.v1"
        } else {
            "qwen3-coder-xml.v1"
        };
        // Schema typing is a non-guess single reading: admitted under
        // `Structural` too.
        for (label, dialect, policy) in [
            ("qwen", single, Policy::PerTool),
            ("all", ALL_DIALECTS, Policy::PerTool),
            ("structural", single, Policy::Structural),
        ] {
            cases.push(Case {
                tool_set: Some("adapter"),
                invalid: None,
                id: format!("adapter/search_index/{label}/{category}/{variant}"),
                category: category.to_owned(),
                dialect: dialect.to_owned(),
                policy,
                stop: Ending::Stop,
                input: input.clone(),
                truth: truth.map(|arguments| {
                    vec![Call {
                        name: "search_index".to_owned(),
                        arguments: json::parse(arguments).expect("truth arguments"),
                    }]
                }),
            });
        }
    }
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
    /// The tool set the case is recovered against: `None` for [`tools`],
    /// `Some("adapter")` for [`adapter_tools`] (written to the record only
    /// then, so every other record is unchanged).
    pub tool_set: Option<&'static str>,
    /// For a case whose output is not well-formed UTF-8: the offset of the
    /// one ASCII byte of `input` the output holds as `0xFF` instead. The
    /// record then carries `input` with U+FFFD there and the exact bytes as
    /// `input_hex` (written only then, so every other record is unchanged).
    pub invalid: Option<usize>,
}

impl Case {
    /// The model output's exact bytes.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        let mut bytes = self.input.clone().into_bytes();
        if let Some(at) = self.invalid {
            bytes[at] = 0xFF;
        }
        bytes
    }

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
        let input = String::from_utf8_lossy(&self.bytes()).into_owned();
        let mut members = vec![
            ("id", string(&self.id)),
            ("category", string(&self.category)),
            ("dialect", string(&self.dialect)),
            ("policy", string(self.policy.code())),
            ("stop", self.stop.node()),
            ("input", string(&input)),
            ("truth", truth),
        ];
        if let Some(tool_set) = self.tool_set {
            members.push(("tool_set", string(tool_set)));
        }
        if self.invalid.is_some() {
            let mut hex = String::new();
            for byte in self.bytes() {
                write!(hex, "{byte:02x}").expect("writing to a String is infallible");
            }
            members.push(("input_hex", string(&hex)));
        }
        object(members)
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
        DialectId::AnthropicToolUse => "anthropic",
        DialectId::BareJson => "bare",
        DialectId::DeepSeekV31 => "deepseek31",
        DialectId::DeepSeekV3 => "deepseek3",
        DialectId::Harmony => "harmony",
        DialectId::Hermes => "hermes",
        DialectId::KimiK2 => "kimi",
        DialectId::Llama3Json => "llama3",
        DialectId::MistralV3 => "mistral3",
        DialectId::MistralV11 => "mistral11",
        DialectId::OpenAiToolCalls => "openai",
        DialectId::Qwen3CoderXml => "qwen",
        DialectId::Custom(_) => "custom",
    }
}

/// The built-in dialects, in corpus order.
const DIALECTS: [DialectId; 12] = [
    DialectId::Qwen3CoderXml,
    DialectId::Hermes,
    DialectId::OpenAiToolCalls,
    DialectId::Llama3Json,
    DialectId::MistralV3,
    DialectId::MistralV11,
    DialectId::DeepSeekV31,
    DialectId::DeepSeekV3,
    DialectId::KimiK2,
    DialectId::AnthropicToolUse,
    DialectId::Harmony,
    DialectId::BareJson,
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
        | DialectId::KimiK2
        | DialectId::AnthropicToolUse
        | DialectId::Harmony
        | DialectId::BareJson
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
            DialectId::MistralV11
                | DialectId::DeepSeekV31
                | DialectId::DeepSeekV3
                | DialectId::KimiK2
                | DialectId::Harmony
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
            DialectId::Llama3Json | DialectId::MistralV3 | DialectId::AnthropicToolUse => "}",
            DialectId::DeepSeekV31 | DialectId::DeepSeekV3 => DEEPSEEK_SECTION_END,
            DialectId::KimiK2 => KIMI_SECTION_END,
            DialectId::Harmony => HARMONY_CALL,
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
    /// The call a correct answer recovers, when the mutation changes it (a
    /// value read with U+FFFD); `None` for the base call.
    call: Option<Call>,
    /// See [`Case::invalid`].
    invalid: Option<usize>,
}

fn mutant(category: &'static str, variant: impl Into<String>, text: String, need: Need) -> Mutant {
    Mutant {
        category,
        variant: variant.into(),
        text,
        need,
        stop: Ending::Stop,
        reasoning: false,
        call: None,
        invalid: None,
    }
}

const NEAR_MISS: [(&str, [&str; 2]); 9] = [
    ("read_file", ["read_fiel", "reed_file"]),
    ("search_code", ["serach_code", "search_cod"]),
    ("get_forecast", ["get_forcast", "get_forecats"]),
    ("configure_view", ["configure_veiw", "configur_view"]),
    ("write_file", ["write_fiel", "wirte_file"]),
    ("create_issue", ["create_isue", "craete_issue"]),
    ("run_command", ["run_comand", "run_commnad"]),
    ("delete_path", ["delete_pth", "delte_path"]),
    ("plan_outfits", ["plan_outfit", "paln_outfits"]),
];

const HALLUCINATED: [&str; 6] = [
    "send_email",
    "translate_text",
    "book_flight",
    "resize_image",
    "post_message",
    "query_database",
];

const KEY_TYPOS: [(&str, &str); 21] = [
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
    ("outfits", "outfitz"),
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
    // A bare JSON object has no closer to repair: it is a call only when
    // complete, so a missing or doubled final brace is not mutated here.
    let wrapped = !matches!(
        base.dialect,
        DialectId::OpenAiToolCalls | DialectId::MistralV11 | DialectId::BareJson
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
        let tagged = !matches!(
            base.dialect,
            DialectId::Llama3Json | DialectId::MistralV3 | DialectId::AnthropicToolUse
        );
        let variant = match base.closer() {
            "]" => "bracket",
            "}" => "brace",
            DEEPSEEK_SECTION_END => "tool_calls_end",
            KIMI_SECTION_END => "tool_calls_section_end",
            HARMONY_CALL => "call",
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
        if base.dialect == DialectId::Harmony {
            // A Harmony message has no outside: text after the arguments
            // with no end token is inside the message, dropped as prose.
            out.push(mutant(
                "tool_call.extract.prose",
                "done-at-end",
                format!("{}\nDone.", &text[..end]),
                Need::DataLoss,
            ));
        } else {
            out.push(mutant(
                "tool_call.markup.missing-wrapper-close.before-text",
                "done",
                format!("{}\nDone.", &text[..end]),
                Need::Guess,
            ));
        }
        out.push(mutant(
            "tool_call.markup.stray-closer",
            variant,
            format!("{text}\n{closer}"),
            Need::Exact,
        ));
        if tagged
            && !matches!(
                base.dialect,
                DialectId::DeepSeekV31
                    | DialectId::DeepSeekV3
                    | DialectId::KimiK2
                    | DialectId::Harmony
            )
        {
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
    if base.dialect == DialectId::KimiK2 {
        kimi_mutations(base, &mut out);
    }
    if base.dialect == DialectId::AnthropicToolUse {
        anthropic_mutations(base, &mut out);
    }
    if base.dialect == DialectId::Harmony {
        harmony_mutations(base, &mut out);
    }
    if base.dialect == DialectId::BareJson {
        bare_mutations(base, &mut out);
    }
    if matches!(
        base.dialect,
        DialectId::Hermes
            | DialectId::Llama3Json
            | DialectId::MistralV3
            | DialectId::AnthropicToolUse
            | DialectId::OpenAiToolCalls
            | DialectId::BareJson
    ) {
        envelope_mutations(base, &mut out);
    }
    closer_misspelled_mutations(base, &mut out);
    token_misspelled_mutations(base, &mut out);
    near_closer_mutations(base, &mut out);
    closer_in_value_mutations(base, &mut out);
    bare_word_mutations(base, &mut out);
    invalid_value_mutations(base, &mut out);
    repeated_choice_mutations(base, xml, &mut out);
    if base.dialect == DialectId::MistralV11 {
        marker_cut_mutations(base, &mut out);
    }
    out
}

// ---------------------------------------------------------------------
// Later rules of issue #3: misspelled closers, invalid UTF-8 in a value,
// a repeated parameter chosen on evidence, a call cut at a reasoning
// marker, and a malformed call between two calls of one attempt.
// ---------------------------------------------------------------------

/// The closer spellings of `base`'s witness a misspelling mutates: every
/// multi-byte literal ending a call of its dialect that the witness writes,
/// and Qwen3-Coder XML's `</parameter>`.
fn misspellable(base: &Base) -> Vec<String> {
    let mut spellings: Vec<String> = base
        .dialect
        .grammar()
        .call_ends()
        .iter()
        .flat_map(|end| end.call_closers.iter())
        .filter(|spelling| spelling.len() > 1)
        .filter_map(|spelling| String::from_utf8(spelling.clone()).ok())
        .filter(|spelling| base.text.contains(spelling.as_str()))
        .collect();
    if base.dialect == DialectId::Qwen3CoderXml && base.text.contains("</parameter>") {
        spellings.push("</parameter>".to_owned());
    }
    spellings.sort();
    spellings.dedup();
    spellings
}

/// `spelling` with one slip: its second-to-last ASCII letter or digit
/// deleted (`</function>` as `</functin>`).
fn misspell(spelling: &str) -> Option<String> {
    let letters: Vec<usize> = spelling
        .char_indices()
        .filter(|(_, character)| character.is_ascii_alphanumeric())
        .map(|(at, _)| at)
        .collect();
    let at = *letters.get(letters.len().checked_sub(2)?)?;
    let mut slipped = spelling.to_owned();
    slipped.remove(at);
    Some(slipped)
}

/// `tool_call.markup.closer-misspelled`: the last occurrence of each closer
/// the witness writes, with one slip.
fn closer_misspelled_mutations(base: &Base, out: &mut Vec<Mutant>) {
    for spelling in misspellable(base) {
        let (Some(at), Some(slipped)) = (base.text.rfind(&spelling), misspell(&spelling)) else {
            continue;
        };
        let variant: String = spelling
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        out.push(mutant(
            "tool_call.markup.closer-misspelled",
            variant,
            format!(
                "{}{slipped}{}",
                &base.text[..at],
                &base.text[at + spelling.len()..]
            ),
            Need::Exact,
        ));
    }
}

/// `tool_call.markup.token-misspelled`: the last occurrence of each markup
/// token other than a closer the witness writes (a `<` or `[` spelled
/// literal of its dialect's `call` production: the attempt's own opener,
/// an argument separator, `<function=`, `<parameter=`, `[ARGS]`), with one
/// slip.
fn token_misspelled_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let closers: Vec<Vec<u8>> = base
        .dialect
        .grammar()
        .call_ends()
        .iter()
        .flat_map(|end| end.call_closers.iter().cloned())
        .collect();
    let mut tokens: Vec<String> = base
        .dialect
        .grammar()
        .call_literals()
        .iter()
        .filter(|spelling| {
            spelling.len() > 1 && matches!(spelling[0], b'<' | b'[') && !closers.contains(spelling)
        })
        .filter_map(|spelling| String::from_utf8(spelling.clone()).ok())
        .collect();
    if base.dialect == DialectId::Qwen3CoderXml {
        tokens.push("<parameter=".to_owned());
    }
    tokens.sort();
    tokens.dedup();
    for token in tokens {
        let Some(at) = base.text.rfind(&token) else {
            continue;
        };
        // Not a part of a longer token written there (the attempt's own
        // opener is one of them: T5/F12).
        if base.text[..at].ends_with('<')
            || closers
                .iter()
                .any(|closer| base.text.as_bytes()[at..].starts_with(closer))
        {
            continue;
        }
        let Some(slipped) = misspell(&token) else {
            continue;
        };
        let variant: String = token.chars().filter(char::is_ascii_alphanumeric).collect();
        out.push(mutant(
            "tool_call.markup.token-misspelled",
            variant,
            format!(
                "{}{slipped}{}",
                &base.text[..at],
                &base.text[at + token.len()..]
            ),
            Need::Exact,
        ));
    }
}

/// T5/F9: a canonical value holding text near a closer of the dialect (a
/// case variant, one slip, an over-long spelling, or, inside a JSON string,
/// the closer itself), rotating by seed over the closers and the kinds: the
/// value keeps its whole text, needing no repair (`canonical` cases).
fn near_closer_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let json = match base.dialect {
        DialectId::Qwen3CoderXml => false,
        DialectId::Hermes | DialectId::KimiK2 | DialectId::DeepSeekV31 | DialectId::DeepSeekV3 => {
            true
        }
        _ => return,
    };
    let members = base.members();
    let Some((index, value)) = members.iter().enumerate().find_map(|(index, member)| {
        let Value::String(value) = &member.value.value else {
            return None;
        };
        matches!(base.declared(&member.key).shape, ValueShape::String(None))
            .then(|| (index, value.clone()))
    }) else {
        return;
    };
    let mut closers: Vec<String> = base
        .dialect
        .grammar()
        .call_ends()
        .iter()
        .flat_map(|end| end.call_closers.iter())
        .filter(|spelling| spelling.len() > 1)
        .filter_map(|spelling| String::from_utf8(spelling.clone()).ok())
        .collect();
    if !json {
        closers.push("</parameter>".to_owned());
    }
    closers.sort();
    closers.dedup();
    let pick = usize::try_from(base.seed).unwrap_or(0);
    let closer = &closers[pick % closers.len()];
    let letters: Vec<usize> = closer
        .char_indices()
        .filter(|(_, character)| character.is_ascii_alphanumeric())
        .map(|(at, _)| at)
        .collect();
    let (first, last, slip) = (
        letters[0],
        letters[letters.len() - 1] + 1,
        letters[letters.len() - 2],
    );
    let kinds = if json { 4 } else { 3 };
    let near = match (pick / closers.len()) % kinds {
        0 => format!(
            "{}{}{}",
            &closer[..first],
            closer[first..=first].to_ascii_uppercase(),
            &closer[first + 1..]
        ),
        1 => format!("{}{}", &closer[..slip], &closer[slip + 1..]),
        2 => format!("{}x{}", &closer[..last], &closer[last..]),
        _ => closer.clone(),
    };
    let text = format!("{value} {near}");
    let mut arguments = members.to_vec();
    arguments[index].value = string(&text);
    let mut kept = mutant(
        "canonical",
        "near-closer",
        replace_value(base, index, &text),
        Need::Exact,
    );
    kept.call = Some(Call {
        name: base.call.name.clone(),
        arguments: Node::new(Value::Object(arguments)),
    });
    out.push(kept);
}

/// Whether a string value of `shape` may hold closer text as `value`: its
/// declared pattern, if any, admits it (a value the definition rejects is
/// no reading of the parameter).
fn holds_closer_text(shape: &ValueShape, value: &str) -> bool {
    match shape {
        ValueShape::String(None) => true,
        ValueShape::String(Some(constraint)) => constraint
            .pattern
            .is_match(value, &mut Governor::new(Budget::default()))
            .expect("a corpus value matches within the default budget"),
        _ => false,
    }
}

/// A `tool_call.value.closer-in-value` case: `text` holds member `index`'s
/// string value as `value` (closer text read into it), which a correct
/// answer recovers.
fn closer_text(base: &Base, index: usize, variant: &str, text: String, value: &str) -> Mutant {
    let mut arguments = base.members().to_vec();
    arguments[index].value = string(value);
    let mut held = mutant(
        "tool_call.value.closer-in-value",
        variant,
        text,
        Need::Exact,
    );
    held.call = Some(Call {
        name: base.call.name.clone(),
        arguments: Node::new(Value::Object(arguments)),
    });
    held
}

/// T6: closer text inside a Qwen3-Coder XML string value (the first
/// unconstrained string the witness writes). Every closer that can end the
/// value is weighed by the repairs each reading needs:
///
/// - `codec-closer`: `<parameter>v</parameter>` written inside the value;
///   ending it at that `</parameter>` leaves text between parameters, so the
///   value holds it;
/// - `call-closers`: `</function></tool_call>` written inside the value,
///   then text (` end`): the closers are unpaired (no element of theirs is
///   opened in the value and no call is held), so the reading holding the
///   text and the reading ending the value at `</function>` (the text
///   dropped as text between parameters) tie and the call is `Ambiguous`:
///   abstain (truth changed in adversary pass 6, F14, by the maintainer's
///   decision "Newlines have zero meaning here." with the rule that text
///   after an unpaired closer is `Ambiguous`, never read into the value
///   alone; issue #3: "Never accept trailing free text as an argument");
/// - `variant-closers`: `<tool_calls>`, `</tool_calls>`, `<functions>` and
///   `</functions>` (declared closer variants) inside the value;
/// - `call-markup`: a whole call's markup written inside the value, as a
///   value documenting the format does (`<function=example>...</function>`):
///   its closers are the held call's own, so the value holds it, and no call
///   is read from it;
/// - `wrapped-call-markup`: the same call wrapped in `<tool_call>`, then
///   text: likewise (the reading that ends the value inside the held call
///   leaves its closers as stray markup to repair, so the value holds it;
///   the maintainer's decision on issue #3: "If only one reading lets the
///   rest of the call parse cleanly, it wins");
/// - `closing-sequence` (`tool_call.markup.stray-closer`): the value of the
///   last member is followed by exactly the closers of a call
///   (`</parameter>`, `</function>`, `</tool_call>`), each on its own line:
///   the value ends at the first, and the others are stray closers (issue
///   #3 §3: "repeated or stray closers are all one class"), so the truth is
///   the value as written;
/// - `closers-then-call` (`tool_call.markup.stray-closer`): those closers,
///   then a whole call of a tool the set does not declare: the value ends at
///   its first closer (issue #3: "Never accept trailing free text as an
///   argument"), the call after it names no declared tool, and the closers
///   after that are stray, so the truth is the value as written;
/// - `inline-closers-then-call` (`tool_call.markup.stray-closer`, adversary
///   pass 6, F19): the same with the closers written inline after the value
///   (`v</parameter></function></tool_call>`): newlines have zero meaning,
///   so it reads as `closers-then-call` does, the same truth.
fn closer_in_value_mutations(base: &Base, out: &mut Vec<Mutant>) {
    if base.dialect != DialectId::Qwen3CoderXml || base.form != Form::Xml {
        return;
    }
    let members = base.members();
    let Some((index, value)) = members.iter().enumerate().find_map(|(index, member)| {
        let Value::String(value) = &member.value.value else {
            return None;
        };
        matches!(base.declared(&member.key).shape, ValueShape::String(None))
            .then(|| (index, value.clone()))
    }) else {
        return;
    };
    // Adversary pass 6 (F14), maintainer: "Newlines have zero meaning
    // here." `</function></tool_call>` closes nothing the value opened and
    // text follows it, so the call is Ambiguous between the value holding
    // the text and the value ending at `</function>`: abstain.
    out.push(mutant(
        "tool_call.value.closer-in-value",
        "call-closers",
        replace_value(base, index, &format!("{value} </function></tool_call> end")),
        Need::Never,
    ));
    for (variant, held) in [
        (
            "codec-closer",
            format!("{value} <parameter>v</parameter> end"),
        ),
        (
            "variant-closers",
            format!("{value} <tool_calls> and </tool_calls>, <functions>f</functions> end"),
        ),
    ] {
        out.push(closer_text(
            base,
            index,
            variant,
            replace_value(base, index, &held),
            &held,
        ));
    }
    let documented = "<function=example>\n<parameter=example_arg>\n1\n</parameter>\n</function>";
    let held = format!("{value} {documented} end");
    out.push(closer_text(
        base,
        index,
        "call-markup",
        replace_value(base, index, &held),
        &held,
    ));
    let held = format!("{value} <tool_call>\n{documented}\n</tool_call> end");
    out.push(closer_text(
        base,
        index,
        "wrapped-call-markup",
        replace_value(base, index, &held),
        &held,
    ));
    if index + 1 == members.len() {
        let held = format!("{value}\n</parameter>\n</function>\n</tool_call>");
        out.push(mutant(
            "tool_call.markup.stray-closer",
            "closing-sequence",
            replace_value(base, index, &held),
            Need::Exact,
        ));
        let held = format!(
            "{value}\n</parameter>\n</function>\n</tool_call>\n<tool_call>\n{documented}\n</tool_call>"
        );
        out.push(mutant(
            "tool_call.markup.stray-closer",
            "closers-then-call",
            replace_value(base, index, &held),
            Need::Exact,
        ));
        // A value that ends in a newline cannot be followed inline by its
        // closer: the newline is then the value's framing newline (consumed
        // as `tool_call.xml.framing-newline`), so the text no longer spells
        // the value and the truth would not be what the text says.
        if !value.ends_with('\n') {
            let held = format!(
                "{value}</parameter></function></tool_call>\n<tool_call>\n{documented}\n</tool_call>"
            );
            out.push(mutant(
                "tool_call.markup.stray-closer",
                "inline-closers-then-call",
                replace_value(base, index, &held),
                Need::Exact,
            ));
        }
    }
}

/// T5/F10: a bare word in an arguments object is never a key unless its `:`
/// follows: a stray word after the first member's value (`{"a": "y" oops}`),
/// and prose after an arguments object left unclosed. No parameter name is
/// invented and no free text is read as an argument, so the call cannot be
/// read: abstain (`json.syntax.bare-key` cases, `Unresolved` truth).
fn bare_word_mutations(base: &Base, out: &mut Vec<Mutant>) {
    if base.form != Form::Object {
        return;
    }
    let mut printed = base.printed();
    if printed.is_empty() {
        return;
    }
    printed[0].value.push_str(" oops");
    out.push(mutant(
        "json.syntax.bare-key",
        "stray-word",
        base.with_arguments(&Base::json_text(&printed)),
        Need::Never,
    ));
    let split = base.split();
    let arguments = base.arguments_text();
    let Some(open) = arguments.strip_suffix('}') else {
        return;
    };
    out.push(mutant(
        "json.syntax.bare-key",
        "prose-after-unclosed",
        format!(
            "{}{} oops. Sorry, my answer is 42 (see {{1,2}} set).",
            split.prefix,
            open.trim_end()
        ),
        Need::Never,
    ));
}

/// `tool_call.value.invalid-utf8`: the first byte of a string value the
/// witness writes once, as plain ASCII, replaced by `0xFF`; the value is
/// read with U+FFFD for it, a data loss.
fn invalid_value_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let members = base.members();
    let Some((index, value)) = members.iter().enumerate().find_map(|(index, member)| {
        let Value::String(value) = &member.value.value else {
            return None;
        };
        // An unconstrained string: the value read with U+FFFD is the
        // value, with no enumeration or pattern to correct it toward.
        (matches!(base.declared(&member.key).shape, ValueShape::String(None))
            && value.len() >= 2
            && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            && base.text.matches(value.as_str()).count() == 1)
            .then(|| (index, value.clone()))
    }) else {
        return;
    };
    let at = base
        .text
        .find(value.as_str())
        .expect("the value is written");
    let mut arguments = members.to_vec();
    arguments[index].value = string(&format!("\u{FFFD}{}", &value[1..]));
    let mut slipped = mutant(
        "tool_call.value.invalid-utf8",
        members[index].key.clone(),
        base.text.clone(),
        Need::DataLoss,
    );
    slipped.invalid = Some(at);
    slipped.call = Some(Call {
        name: base.call.name.clone(),
        arguments: Node::new(Value::Object(arguments)),
    });
    out.push(slipped);
}

/// `tool_call.parameter.repeated-choice`: a typed parameter written a
/// second time with a value its type rejects; the reading that keeps the
/// valid occurrence is chosen on evidence, a guess.
fn repeated_choice_mutations(base: &Base, xml: bool, out: &mut Vec<Mutant>) {
    let typed = base.members_where(|declared, _| {
        matches!(
            declared.shape,
            ValueShape::Integer | ValueShape::Number | ValueShape::Boolean
        )
    });
    let Some(&index) = typed.first() else {
        return;
    };
    let key = base.members()[index].key.clone();
    out.push(mutant(
        "tool_call.parameter.repeated-choice",
        key.clone(),
        add_member(
            base,
            &key,
            if xml {
                "not-a-value"
            } else {
                "\"not-a-value\""
            },
        ),
        Need::Guess,
    ));
}

/// `tool_call.reasoning.marker-cut`: a Mistral v11 call with no `[ARGS]`
/// running into a reasoning block's closing `[/THINK]` (cut there, so never
/// usable) before the witness call.
fn marker_cut_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let mut cut = mutant(
        "tool_call.reasoning.marker-cut",
        "name-only",
        format!(
            "[THINK]Plan: [TOOL_CALLS]{}[/THINK]{}",
            base.call.name, base.text
        ),
        Need::Exact,
    );
    cut.reasoning = true;
    out.push(cut);
    // The mirror: a call outside reasoning that runs into a block's opening
    // `[THINK]` is cut there; the block holds the marker and its text.
    let mut opener = mutant(
        "tool_call.reasoning.marker-cut",
        "opening-marker",
        format!(
            "[TOOL_CALLS]{}[THINK]Plan.[/THINK]{}",
            base.call.name, base.text
        ),
        Need::Exact,
    );
    opener.reasoning = true;
    out.push(opener);
}

/// `{name}` in `text` with ` oops` written after its first occurrence: a
/// call whose name runs into prose, which no grammar reads.
fn break_name(text: &str, name: &str) -> Option<String> {
    let at = text.find(name)? + name.len();
    Some(format!("{} oops{}", &text[..at], &text[at..]))
}

/// The JSON envelope slips: the call object around the arguments carries
/// the JSON syntax slip the json codec repairs inside arguments, under the
/// same rule (a `json-envelope` grammar).
const ENVELOPE_SLIPS: [(&str, &str); 8] = [
    ("json.syntax.trailing-comma", "envelope"),
    ("json.syntax.string", "envelope-single-quotes"),
    ("json.syntax.bare-key", "envelope"),
    ("json.syntax.missing-comma", "envelope"),
    ("json.syntax.missing-colon", "envelope"),
    ("json.syntax.comment", "envelope"),
    ("json.syntax.string", "envelope-control-close"),
    ("json.syntax.string", "envelope-control-open"),
];

/// The byte offset of the first `byte` of `text` outside a JSON string.
fn outside_strings(text: &str, byte: u8) -> Option<usize> {
    let mut quoted = false;
    for (at, &current) in text.as_bytes().iter().enumerate() {
        match current {
            b'"' => quoted = !quoted,
            _ if current == byte && !quoted => return Some(at),
            _ => {}
        }
    }
    None
}

/// `text` with the quotes of every `"key":` member key removed.
fn unquote_keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('"') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let word = &after[..close];
        let tail = &after[close + 1..];
        let key = !word.is_empty()
            && word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && tail.trim_start().starts_with(':');
        if key {
            out.push_str(word);
        } else {
            out.push_str(&rest[open..=open + close + 1]);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Two of [`ENVELOPE_SLIPS`] in the call envelope of a JSON-envelope
/// witness, chosen by tool and seed so each dialect meets every slip: the
/// arguments text is untouched. Each is a non-guess single-reading repair
/// (`Need::Exact`).
fn envelope_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let split = base.split();
    let arguments = base.arguments_text();
    let (prefix, suffix) = (split.prefix.as_str(), split.suffix.as_str());
    let name = json::quote(&base.call.name);
    // Apply `edit` to the prefix, or else to the suffix.
    let either = |edit: &dyn Fn(&str) -> Option<String>| {
        edit(prefix)
            .map(|edited| format!("{edited}{arguments}{suffix}"))
            .or_else(|| edit(suffix).map(|edited| format!("{prefix}{arguments}{edited}")))
    };
    let first = usize::try_from(base.seed).unwrap_or(0) + 4 * base.tool;
    for index in [first % 8, (first + 4) % 8] {
        let (category, variant) = ENVELOPE_SLIPS[index];
        let text = match index {
            0 => suffix
                .rfind('}')
                .map(|at| format!("{prefix}{arguments}{},{}", &suffix[..at], &suffix[at..])),
            1 => Some(format!(
                "{}{arguments}{}",
                prefix.replace('"', "'"),
                suffix.replace('"', "'")
            )),
            2 => Some(format!(
                "{}{arguments}{}",
                unquote_keys(prefix),
                unquote_keys(suffix)
            )),
            3 => either(&|part: &str| {
                outside_strings(part, b',').map(|at| format!("{} {}", &part[..at], &part[at + 1..]))
            }),
            4 => either(&|part: &str| {
                outside_strings(part, b':').map(|at| format!("{}{}", &part[..at], &part[at + 1..]))
            }),
            5 => prefix.find('{').map(|at| {
                format!(
                    "{}/* the call */ {}{arguments}{suffix}",
                    &prefix[..=at],
                    &prefix[at + 1..]
                )
            }),
            6 => either(&|part: &str| {
                part.contains(&name)
                    .then(|| part.replacen(&name, &format!("\"{}\t\"", base.call.name), 1))
            }),
            _ => either(&|part: &str| {
                part.contains(&name)
                    .then(|| part.replacen(&name, &format!("\"\n{}\"", base.call.name), 1))
            }),
        };
        if let Some(text) = text {
            out.push(mutant(category, variant, text, Need::Exact));
        }
    }
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

/// Anthropic `tool_use` blocks: in a `content` array beside a text block,
/// in an assistant message, in a whole API response; without their type
/// (an id beginning `toolu_` is the evidence); and with `"arguments"` or
/// `"parameters"` written for `"input"`.
fn anthropic_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let text = &base.text;
    for (variant, wrapped) in [
        (
            "content-array",
            format!("[{{\"type\":\"text\",\"text\":\"Checking.\"}},{text}]"),
        ),
        (
            "message",
            format!("{{\"role\":\"assistant\",\"content\":[{text}]}}"),
        ),
        (
            "response",
            format!(
                "{{\"id\":\"msg_01\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{text}],\"stop_reason\":\"tool_use\"}}"
            ),
        ),
    ] {
        out.push(mutant("canonical", variant, wrapped, Need::Exact));
    }
    out.push(mutant(
        "canonical",
        "untyped-toolu-id",
        format!(
            "{{\"id\":\"toolu_01\",\"name\":{},\"input\":{}}}",
            json::quote(&base.call.name),
            base.arguments_text()
        ),
        Need::Exact,
    ));
    // The keys are read for "input" only after the block's evidence (its
    // "type": "tool_use").
    let split = base.split();
    if let Some(at) = split.prefix.rfind("\"input\"")
        && split.prefix[..at].contains("\"tool_use\"")
    {
        for key in ["\"arguments\"", "\"parameters\""] {
            out.push(mutant(
                "tool_call.markup.terminal-confusable",
                key.trim_matches('"').to_owned(),
                format!(
                    "{}{key}{}{}{}",
                    &split.prefix[..at],
                    &split.prefix[at + "\"input\"".len()..],
                    base.arguments_text(),
                    split.suffix
                ),
                Need::Exact,
            ));
        }
    }
}

/// The end of a Harmony call message.
const HARMONY_CALL: &str = "<|call|>";

/// Harmony: the call in its documented and renderer layouts, after an
/// analysis message and a commentary preamble, without its content type;
/// with `<|end|>` for `<|call|>`, a `<|channel>` spelling, the recipient
/// without `functions.`, text after the arguments, and the recipient after
/// `<|message|>`.
fn harmony_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let name = &base.call.name;
    let arguments = base.arguments_text();
    let documented = format!(
        "<|start|>assistant<|channel|>commentary to=functions.{name} <|constrain|>json<|message|>{arguments}<|call|>"
    );
    for (category, variant, text, need) in [
        ("canonical", "documented", documented.clone(), Need::Exact),
        (
            "canonical",
            "renderer",
            format!(
                "<|start|>assistant to=functions.{name}<|channel|>commentary <|constrain|>json<|message|>{arguments}<|call|>"
            ),
            Need::Exact,
        ),
        (
            "canonical",
            "analysis-first",
            format!("<|channel|>analysis<|message|>Need to call {name}.<|end|>{documented}"),
            Need::Exact,
        ),
        (
            "canonical",
            "preamble",
            format!("<|channel|>commentary<|message|>Calling {name} now.<|end|>{documented}"),
            Need::Exact,
        ),
        (
            "canonical",
            "no-content-type",
            format!("<|channel|>commentary to=functions.{name}<|message|>{arguments}<|call|>"),
            Need::Exact,
        ),
        (
            "tool_call.markup.closer-variant",
            "end",
            documented.replace("<|call|>", "<|end|>"),
            Need::Exact,
        ),
        (
            "tool_call.markup.terminal-confusable",
            "channel",
            documented.replace("<|channel|>", "<|channel>"),
            Need::Exact,
        ),
        (
            "tool_call.markup.missing-token",
            "namespace",
            documented.replace("to=functions.", "to="),
            Need::Exact,
        ),
        (
            "tool_call.extract.prose",
            "trailing",
            documented.replace("<|call|>", " done<|call|>"),
            Need::DataLoss,
        ),
        (
            "tool_call.extract.prose",
            "recipient-in-body",
            format!(
                "<|start|>assistant<|channel|>commentary<|message|>to=functions.{name} {arguments}<|call|>"
            ),
            Need::DataLoss,
        ),
    ] {
        out.push(mutant(category, variant, text, need));
    }
}

/// Bare JSON: the call object among prose, in a `json` code fence, as the
/// element of an array (with a second element that is not a call), with its
/// members in the other order, and with a trailing id.
fn bare_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let text = &base.text;
    let name = json::quote(&base.call.name);
    let arguments = base.arguments_text();
    for (category, variant, text, need) in [
        (
            "canonical",
            "in-prose",
            format!("I'll look that up.\n{text}\nOne moment."),
            Need::Exact,
        ),
        (
            "canonical",
            "array",
            format!("[{text}, {{\"name\": \"note\", \"value\": 1}}]"),
            Need::Exact,
        ),
        (
            "canonical",
            "arguments-first",
            format!("{{\"arguments\": {arguments}, \"name\": {name}}}"),
            Need::Exact,
        ),
        (
            "canonical",
            "trailing-id",
            format!("{{\"name\": {name}, \"arguments\": {arguments}, \"id\": \"call_7\"}}"),
            Need::Exact,
        ),
        (
            "tool_call.context.fenced-example",
            "json-fence",
            format!("Calling it:\n```json\n{text}\n```\n"),
            Need::Guess,
        ),
    ] {
        out.push(mutant(category, variant, text, need));
    }
}

/// The closer of a Kimi-K2 tool-calls section.
const KIMI_SECTION_END: &str = "<|tool_calls_section_end|>";
/// The opener of a Kimi-K2 tool-calls section.
const KIMI_SECTION_BEGIN: &str = "<|tool_calls_section_begin|>";

/// Kimi-K2: the section opener left out, the singular section-token
/// spellings, the id without its `functions.` prefix, and the id without
/// its `:IDX`.
fn kimi_mutations(base: &Base, out: &mut Vec<Mutant>) {
    let text = &base.text;
    if let Some(rest) = text.strip_prefix(KIMI_SECTION_BEGIN) {
        out.push(mutant(
            "tool_call.markup.missing-token",
            "section-begin",
            rest.trim_start().to_owned(),
            Need::Exact,
        ));
    }
    out.push(mutant(
        "tool_call.markup.terminal-confusable",
        "singular-section",
        text.replace("calls_section", "call_section"),
        Need::Exact,
    ));
    // A bare call counter for the id: the tool is read from the arguments,
    // a guess.
    if let (Some(open), Some(argument)) = (
        text.find("<|tool_call_begin|>"),
        text.find("<|tool_call_argument_begin|>"),
    ) {
        let open = open + "<|tool_call_begin|>".len();
        out.push(mutant(
            "tool_call.name.from-arguments",
            "counter-id",
            format!("{}3{}", &text[..open], &text[argument..]),
            Need::Guess,
        ));
    }
    let id = format!("functions.{}:", base.call.name);
    if let Some(at) = text.find(&id) {
        out.push(mutant(
            "canonical",
            "unprefixed-id",
            format!("{}{}", &text[..at], &text[at + "functions.".len()..]),
            Need::Exact,
        ));
        let index = at + id.len();
        let digits = text[index..].bytes().take_while(u8::is_ascii_digit).count();
        out.push(mutant(
            "tool_call.id.dedup",
            "missing-index",
            format!("{}{}", &text[..index - 1], &text[index + digits..]),
            Need::Exact,
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
        // Stage-3 pass 5 (F14): prose between the last string value's
        // closer and a second closer before the call's closer is trailing
        // text inside the wrapper, never read into the value (issue #3's
        // negative corpus: "trailing text inside the wrapper" must be
        // rejected or stay unresolved; "Never accept trailing free text as
        // an argument"): abstain.
        if let Some(&index) = strings.last()
            && index + 1 == elements.len()
        {
            let mut body = xml_body(&elements);
            let closer = nth_parameter_close(&body, index) + "</parameter>\n".len();
            body.insert_str(closer, "Done, the file is written.\n</parameter>\n");
            out.push(mutant(
                "tool_call.markup.stray-closer",
                "trailing-text",
                base.with_arguments(&body),
                Need::Never,
            ));
        }
        // Adversary pass 6 (F14), maintainer: "Newlines have zero meaning
        // here." The `text-between` and `trailing-text` shapes with the
        // value's closer on the value's own line or indented: the closer
        // is unpaired and text follows it, so the reading holding the text
        // (`tool_call.value.text-after-closer`) and the reading ending the
        // value at that closer and dropping the text (`tool_call.markup.
        // text-between-parameters`) tie and the call is Ambiguous: abstain,
        // as for the closer on a line of its own.
        let last = strings
            .last()
            .copied()
            .filter(|&index| index + 1 == elements.len());
        for (variant, index, text) in [
            ("same-line", strings.first().copied(), "more text"),
            ("indented", strings.first().copied(), "more text"),
            ("trailing-same-line", last, "Done, the file is written."),
            ("trailing-indented", last, "Done, the file is written."),
        ] {
            let Some(index) = index else {
                continue;
            };
            let mut body = xml_body(&elements);
            let closer = nth_parameter_close(&body, index);
            let after = closer + "</parameter>".len();
            if variant.ends_with("same-line") {
                body.insert_str(after, &format!(" {text}\n</parameter>"));
                body.remove(closer - 1);
            } else {
                body.insert_str(after + 1, &format!("{text}\n</parameter>\n"));
                body.insert_str(closer, "  ");
            }
            out.push(mutant(
                "tool_call.value.text-after-closer",
                variant,
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
    // Array-of-object parameters as repeated nested elements. The truth is
    // issue #8 section 1: each repetition of `<parameter=NAME>` holding
    // nested `<parameter=>` elements is one object item, collected in source
    // order (`tool_call.markup.nested-parameter` per item,
    // `tool_call.xml.array-item` per item, `tool_call.parameter.collect`).
    // The witness writes at most one item, so a second mutant repeats the
    // first item to put the collection under the evaluation too.
    for (index, member) in members.iter().enumerate() {
        let Value::Array(items) = &member.value.value else {
            continue;
        };
        let ValueShape::Array(item_shape) = &base.declared(&member.key).shape else {
            continue;
        };
        let ValueShape::Object(shape) = &**item_shape else {
            continue;
        };
        let nested = |item: &Node| -> Option<String> {
            let Value::Object(fields) = &item.value else {
                return None;
            };
            if fields.is_empty() {
                return None;
            }
            let elements: Vec<(String, String, bool)> = fields
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
            Some(
                xml_body(&elements)
                    .trim_start_matches('\n')
                    .trim_end_matches('\n')
                    .to_owned(),
            )
        };
        let Some(first) = items.first() else {
            continue;
        };
        for (variant, repeated) in [("items", 0), ("items-repeated", 1)] {
            let mut written: Vec<&Node> = items.iter().collect();
            written.extend(std::iter::repeat_n(first, repeated));
            let Some(bodies) = written
                .iter()
                .map(|item| nested(item))
                .collect::<Option<Vec<String>>>()
            else {
                continue;
            };
            let mut spread: Vec<(String, String, bool)> = Vec::new();
            for (position, element) in elements.iter().enumerate() {
                if position == index {
                    for body in &bodies {
                        spread.push((element.0.clone(), body.clone(), true));
                    }
                } else {
                    spread.push(element.clone());
                }
            }
            let category = if bodies.len() == 1 {
                "tool_call.xml.array-item"
            } else {
                "tool_call.parameter.collect"
            };
            let mut mutated = mutant(
                category,
                format!("{}-{variant}", member.key),
                base.with_arguments(&xml_body(&spread)),
                Need::Exact,
            );
            if repeated > 0 {
                let mut arguments = base.members().to_vec();
                arguments[index].value = Node::new(Value::Array(
                    written.iter().map(|item| (*item).clone()).collect(),
                ));
                mutated.call = Some(Call {
                    name: base.call.name.clone(),
                    arguments: Node::new(Value::Object(arguments)),
                });
            }
            out.push(mutated);
        }
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
    // The object without its "name" member: the tool is read from the
    // arguments, a guess.
    out.push(mutant(
        "tool_call.name.from-arguments",
        "nameless",
        format!(
            "{}{{\"arguments\": {}}}{}",
            &text[..open],
            base.arguments_text(),
            &text[close + 1..]
        ),
        Need::Guess,
    ));
}

// ---------------------------------------------------------------------
// Turns of two calls.
// ---------------------------------------------------------------------

/// Two calls of a section dialect (DeepSeek-V3.1, DeepSeek-V3, Kimi-K2) in
/// one section, and the same with the first call's closer left out.
fn section_pair(first: &Base, second: &Base, out: &mut Vec<Mutant>) {
    let (end, begin, call_end, sep) = match first.dialect {
        DialectId::DeepSeekV31 => (
            DEEPSEEK_SECTION_END,
            DEEPSEEK_SECTION_BEGIN,
            "<｜tool▁call▁end｜>",
            "",
        ),
        // DeepSeek-V3 separates the calls of a section with a newline.
        DialectId::DeepSeekV3 => (
            DEEPSEEK_SECTION_END,
            DEEPSEEK_SECTION_BEGIN,
            "<｜tool▁call▁end｜>",
            "\n",
        ),
        DialectId::KimiK2 => (
            KIMI_SECTION_END,
            KIMI_SECTION_BEGIN,
            "<|tool_call_end|>",
            "",
        ),
        _ => return,
    };
    let (Some(head), Some(tail)) = (
        first.text.strip_suffix(end),
        second.text.strip_prefix(begin),
    ) else {
        return;
    };
    let head = head.trim_end();
    let tail = tail.trim_start();
    out.push(mutant(
        "canonical",
        "one-section",
        format!("{head}{sep}{tail}"),
        Need::Exact,
    ));
    if let Some(open) = head.strip_suffix(call_end) {
        out.push(mutant(
            "tool_call.markup.missing-function-close",
            "call-end",
            format!("{}{sep}{tail}", open.trim_end()),
            Need::Exact,
        ));
    }
    // A malformed call between the two calls of the section: both calls
    // are recovered and only the malformed one is rejected.
    if let Some(end) = tail.find(call_end)
        && let Some(bad) = break_name(&tail[..end + call_end.len()], &second.call.name)
    {
        out.push(mutant(
            "tool_call.markup.group-split",
            "middle",
            format!("{head}{sep}{bad}{sep}{tail}"),
            Need::Exact,
        ));
        // The same malformed call first: the attempt resynchronizes past it
        // as past a later one.
        if let Some(rest) = head.strip_prefix(begin) {
            out.push(mutant(
                "tool_call.markup.group-split",
                "first",
                format!("{begin}{bad}{sep}{}{sep}{tail}", rest.trim_start()),
                Need::Exact,
            ));
        }
        // Without its closer: the next call's opener ends it.
        if let Some(open) = bad.strip_suffix(call_end) {
            out.push(mutant(
                "tool_call.markup.group-split",
                "middle-unclosed",
                format!("{head}{sep}{open}{sep}{tail}"),
                Need::Exact,
            ));
        }
    }
}

/// The `tool_call.markup.group-split` mutations of a Mistral v3 list of
/// two calls: a malformed element between them, first, and left open.
fn mistral_list_pair(first: &Base, second: &Base, out: &mut Vec<Mutant>) {
    if let (Some((head, first_inside, tail)), Some((_, second_inside, _))) =
        (mistral_array(first), mistral_array(second))
        && let Some(bad) = break_name(second_inside.trim(), &second.call.name)
    {
        // A malformed element between the two calls of one list.
        out.push(mutant(
            "tool_call.markup.group-split",
            "middle",
            format!(
                "{head}[{}, {bad}, {}]{tail}",
                first_inside.trim(),
                second_inside.trim()
            ),
            Need::Exact,
        ));
        // The same malformed element first.
        out.push(mutant(
            "tool_call.markup.group-split",
            "first",
            format!(
                "{head}[{bad}, {}, {}]{tail}",
                first_inside.trim(),
                second_inside.trim()
            ),
            Need::Exact,
        ));
        // Left open (no `}`): the next element's `{` ends it.
        if let Some(open) = bad.strip_suffix('}') {
            out.push(mutant(
                "tool_call.markup.group-split",
                "middle-unclosed",
                format!(
                    "{head}[{}, {open}, {}]{tail}",
                    first_inside.trim(),
                    second_inside.trim()
                ),
                Need::Exact,
            ));
        }
    }
}

/// Adversary pass 6 (F19): the first call's closers written inline after
/// its last value, then the second call, then stray closers: two calls,
/// never one call holding the other (as at 72f34f7), whatever the layout
/// ("Newlines have zero meaning here").
fn inline_closers_pair(first: &Base, second: &Base, out: &mut Vec<Mutant>) {
    let closers = "</parameter></function></tool_call>";
    if first.dialect == DialectId::Qwen3CoderXml
        && let Some(at) = first.text.rfind("</parameter>")
        && let Some(head) = first.text[..at].strip_suffix('\n')
        // A value ending in a newline loses it to the framing newline when
        // the closer follows inline (see `closer_in_value_mutations`).
        && (!head.ends_with('\n') || head.ends_with(">\n"))
        && first.text[at..]
            .chars()
            .filter(|character| !character.is_ascii_whitespace())
            .eq(closers.chars())
    {
        out.push(mutant(
            "tool_call.markup.stray-closer",
            "inline-closers-then-call",
            format!(
                "{head}{closers}\n{}\n</parameter>\n</function>\n</tool_call>",
                second.text
            ),
            Need::Exact,
        ));
    }
}

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
        section_pair(first, second, &mut out);
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
        mistral_list_pair(first, second, &mut out);
        inline_closers_pair(first, second, &mut out);
        if first.dialect == DialectId::MistralV11 {
            // A call whose arguments no repair reads between two calls.
            out.push(mutant(
                "tool_call.markup.group-split",
                "unread-arguments",
                format!(
                    "{}\n[TOOL_CALLS]{}[ARGS]{{\"x\": ::: }}\n{}",
                    first.text, second.call.name, second.text
                ),
                Need::Exact,
            ));
            // The same call first.
            out.push(mutant(
                "tool_call.markup.group-split",
                "unread-arguments-first",
                format!(
                    "[TOOL_CALLS]{}[ARGS]{{\"x\": ::: }}\n{}\n{}",
                    first.call.name, first.text, second.text
                ),
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
        if first.dialect == DialectId::BareJson {
            // Two call objects as the elements of one array.
            out.push(mutant(
                "canonical",
                "array-pair",
                format!("[{},\n{}]", first.text, second.text),
                Need::Exact,
            ));
        } else if first.dialect != DialectId::MistralV11 {
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

const ALL_DIALECTS: &str = "anthropic-tool-use.v1,bare-json.v1,deepseek-v3.1.v1,deepseek-v3.v1,harmony.v1,hermes.v1,kimi-k2.v1,llama3-json.v1,mistral-v11.v1,mistral-v3.v1,openai-tool-calls.v1,qwen3-coder-xml.v1";

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
        let call = mutant.call.clone().unwrap_or_else(|| base.call.clone());
        let truth = answerable(mutant.need, base.effects, policy).then(|| vec![call]);
        cases.push(Case {
            tool_set: None,
            invalid: mutant.invalid,
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
            tool_set: None,
            invalid: None,
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
                    tool_set: None,
                    invalid: None,
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
                tool_set: None,
                invalid: None,
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
    adapter_cases(&mut cases);
    let identities: BTreeSet<&str> = cases.iter().map(|case| case.id.as_str()).collect();
    assert_eq!(identities.len(), cases.len(), "case identities are unique");
    cases
}
