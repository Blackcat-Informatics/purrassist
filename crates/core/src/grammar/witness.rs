// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Well-formed witnesses of a dialect grammar's `call` production.
//!
//! [`generate`] prints one call per [`ArgumentShape`] by interpreting the
//! grammar's `call` expression forward:
//!
//! - literals are emitted verbatim and closer classes as their canonical
//!   spelling;
//! - a `name:` capture prints the tool's canonical name, an `id:` capture a
//!   generated call id, and a `param-name:` capture the parameter's name;
//!   each must be accepted by the class it captures, or generation fails;
//! - any other class prints a string [`Pattern::generate`] finds for the
//!   class (a shortest member of a seeded minimum length);
//! - an `arguments:` slot prints shape-conforming arguments in the slot
//!   codec's encoding (a JSON object, a stringified JSON object, or
//!   `xml-params` elements with string values verbatim between one framing
//!   newline on each side and every other value as JSON); a `param-value:`
//!   slot prints one parameter value;
//! - an `until` body is empty;
//! - an alternation picks, by the seed, among the alternatives holding the
//!   most call captures (a name first), and a repetition holding captures is
//!   emitted once (once per parameter when it holds `param-name:`), a
//!   repetition holding a `dropped:` capture its minimum (dropped text is
//!   never part of a well-formed call), and other repetitions their minimum
//!   plus a seeded optional extra.
//!
//! Every choice comes from a `SplitMix64` sequence seeded by
//! [`WitnessOptions::seed`], so equal inputs give equal witnesses. Each
//! [`WitnessCall`] records the call the text denotes when recognized with a
//! [`ToolSet`](crate::tool_call::ToolSet) declaring the shapes: its name, id,
//! and the arguments value as the recognizer reads it (a repeatable
//! parameter printed as `xml-params` elements is one duplicate member per
//! occurrence, as that codec records repeats; a `raw` parameter value is the
//! string of its printed text).
//!
//! # Resources
//!
//! Charges [`K_WITNESS`] `Work` per expression visited and per byte printed,
//! the printed text and recorded calls as `ScratchBytes`, and every class
//! pattern compilation, match and generation under the same governor.
//!
//! ```
//! use purrassist_core::grammar::witness::{self, WitnessOptions};
//! use purrassist_core::tool_call::{DialectId, DialectSet, GenerationStop, Param, ReasoningPolicy, Recognizer, ToolDefinition, ToolSet};
//! use purrassist_core::{Budget, Governor};
//! let tools = ToolSet::compile(
//!     [ToolDefinition::new("read").param("path", Param::string().required())],
//!     Budget::default(),
//! ).unwrap();
//! let mut governor = Governor::new(Budget::default());
//! let shapes = [tools.argument_shape("read").unwrap()];
//! let grammar = DialectId::Qwen3CoderXml.grammar();
//! let witness = witness::generate(grammar, &shapes, WitnessOptions::default(), &mut governor)?;
//! assert!(witness.text.starts_with("<tool_call>"));
//! let set = DialectSet::new([DialectId::Qwen3CoderXml], &mut governor).unwrap();
//! let mut recognizer = Recognizer::with_tools(&set, &tools, ReasoningPolicy::default(), &mut governor)?;
//! recognizer.push(witness.text.as_bytes(), &mut governor)?;
//! let turn = recognizer.finish(GenerationStop::Stop, &mut governor)?;
//! let call = &turn.calls()[0].readings[0];
//! assert_eq!(call.name.as_ref().unwrap().value, "read");
//! assert!(call.arguments.root.equivalent(&witness.calls[0].arguments));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
use super::codec::CodecKind;
use super::cost::K_WITNESS;
use super::{CaptureKind, ClassId, CompiledGrammar, DelimiterSetId, Expr};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, StopReason};
use crate::json::{Member, Node, Number, Value, quote};
use crate::pattern::{GenerateOptions, GenerateOutcome, Pattern, PatternCompileError};
use crate::tool_call::{ArgumentShape, ObjectShape, ParameterShape, ValueShape};
use core::fmt;

/// How a `json` codec slot prints a call's arguments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum JsonArguments {
    /// A JSON object: `{"path":"a"}`.
    #[default]
    Object,
    /// A JSON string holding the object, as `OpenAI` `tool_calls` carry
    /// arguments: `"{\"path\":\"a\"}"`. The recognizer decodes it and flags
    /// the arguments as stringified.
    Stringified,
}

/// Parameters of [`generate`].
///
/// ```
/// use purrassist_core::grammar::witness::{JsonArguments, WitnessOptions};
/// let options = WitnessOptions::default();
/// assert_eq!((options.seed, options.json_arguments), (0, JsonArguments::Object));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WitnessOptions {
    /// The seed of every choice.
    pub seed: u64,
    /// How `json` slots print arguments.
    pub json_arguments: JsonArguments,
}

/// One call a witness denotes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WitnessCall {
    /// The tool's canonical name.
    pub name: String,
    /// The call id printed for an `id:` capture, when the production
    /// captured one.
    pub id: Option<String>,
    /// The arguments as the recognizer reads them (see the module
    /// documentation for repeated and `raw` parameters).
    pub arguments: Node,
}

/// A generated text and the calls it denotes, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Witness {
    /// The printed calls, separated by one newline.
    pub text: String,
    /// One call per requested shape, in order.
    pub calls: Vec<WitnessCall>,
}

/// Why no witness was generated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WitnessError {
    /// The `call` production has a form the generator cannot fill: a
    /// `name:`, `id:` or `param-name:` capture that is not a class or a
    /// literal, a slot outside an `arguments:` or `param-value:` capture, a
    /// codec that cannot carry the value, or too few parameters for a
    /// repetition's minimum.
    Unsupported {
        /// What could not be filled.
        reason: String,
    },
    /// A name the shape requires is not accepted by the class capturing it,
    /// or a printed value contains a delimiter ending its slot.
    Unrepresentable {
        /// The text that cannot be printed in place.
        text: String,
    },
    /// A class has no member within the generation bounds.
    NoCandidate {
        /// The class pattern source.
        class: String,
    },
    /// The governor's budget was exhausted, or the caller cancelled.
    Resource(StopReason),
}

impl fmt::Display for WitnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(formatter, "unsupported production: {reason}"),
            Self::Unrepresentable { text } => {
                write!(formatter, "`{text}` cannot be printed in place")
            }
            Self::NoCandidate { class } => write!(formatter, "class /{class}/ has no candidate"),
            Self::Resource(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for WitnessError {}

impl From<StopReason> for WitnessError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// Print one well-formed call per shape with `grammar`'s `call` production.
///
/// # Errors
/// [`WitnessError::Unsupported`], [`WitnessError::Unrepresentable`] and
/// [`WitnessError::NoCandidate`] when the production cannot carry a shape's
/// call (see the variants), and [`WitnessError::Resource`] when the
/// governor's budget is exhausted or the caller cancelled.
///
/// ```
/// use purrassist_core::grammar::witness::{self, JsonArguments, WitnessError, WitnessOptions};
/// use purrassist_core::tool_call::{DialectId, Param, ToolDefinition, ToolSet};
/// use purrassist_core::{Budget, Governor, StopReason, contract::Resource};
/// let tools = ToolSet::compile([ToolDefinition::new("ls").param("depth", Param::integer())], Budget::default()).unwrap();
/// let shapes = [tools.argument_shape("ls").unwrap()];
/// let options = WitnessOptions { seed: 7, json_arguments: JsonArguments::Stringified };
/// let grammar = DialectId::OpenAiToolCalls.grammar();
/// let first = witness::generate(grammar, &shapes, options, &mut Governor::new(Budget::default()))?;
/// let again = witness::generate(grammar, &shapes, options, &mut Governor::new(Budget::default()))?;
/// assert_eq!(first, again);
/// // Stringified arguments: a JSON string holding the object.
/// assert!(first.text.contains(r#""{"#));
/// assert!(first.calls[0].id.is_some());
/// let mut starved = Governor::new(Budget { work: 3, ..Budget::default() });
/// assert_eq!(
///     witness::generate(grammar, &shapes, options, &mut starved).unwrap_err(),
///     WitnessError::Resource(StopReason::BudgetExceeded(Resource::Work))
/// );
/// # Ok::<(), WitnessError>(())
/// ```
pub fn generate(
    grammar: &CompiledGrammar,
    shapes: &[&ArgumentShape],
    options: WitnessOptions,
    governor: &mut Governor,
) -> Result<Witness, WitnessError> {
    let mut printer = Printer {
        grammar,
        options,
        rng: SplitMix64(options.seed),
        patterns: vec![None; grammar.classes.len()],
        out: Vec::new(),
        call: None,
        param: None,
    };
    let carrier = Carrier {
        codec: printer.codec_of_call(),
        pairs: printer.holds(grammar.call()).parameters,
    };
    let mut calls = Vec::with_capacity(shapes.len());
    for (index, shape) in shapes.iter().enumerate() {
        if index > 0 {
            printer.emit(b"\n", governor)?;
        }
        let parameters = printer.object_parameters(&shape.arguments, governor)?;
        printer.call = Some(Plan {
            name: shape.tool.clone(),
            id: None,
            parameters,
        });
        printer.walk(grammar.call(), None, governor)?;
        let Some(plan) = printer.call.take() else {
            unreachable!("the plan lives for the call")
        };
        let arguments = plan.expected(carrier);
        governor.charge(
            Resource::ScratchBytes,
            arguments
                .estimated_bytes()
                .saturating_add(size(plan.name.len()))
                .saturating_add(size(plan.id.as_ref().map_or(0, String::len))),
        )?;
        calls.push(WitnessCall {
            name: plan.name,
            id: plan.id,
            arguments,
        });
    }
    let text = String::from_utf8(printer.out).map_err(|error| WitnessError::Unrepresentable {
        text: String::from_utf8_lossy(error.as_bytes()).into_owned(),
    })?;
    Ok(Witness { text, calls })
}

/// The `SplitMix64` sequence: a fixed, portable, seedable choice source.
struct SplitMix64(u64);

impl SplitMix64 {
    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A choice in `0..bound` (`bound > 0`).
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % size(bound.max(1))).unwrap_or(0)
    }

    const fn coin(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

/// One generated parameter: its name, value, and declared shape.
struct Generated<'a> {
    name: String,
    value: Node,
    shape: &'a ParameterShape,
}

/// The call being printed.
struct Plan<'a> {
    name: String,
    id: Option<String>,
    parameters: Vec<Generated<'a>>,
}

/// How a call production carries its arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Carrier {
    /// The codec of the `arguments:` or `param-value:` slot.
    codec: Option<CodecKind>,
    /// Whether arguments are `param-name`/`param-value` pairs.
    pairs: bool,
}

impl Plan<'_> {
    /// The arguments the recognizer reads from the printed call: a
    /// repeatable parameter printed once per item becomes one duplicate
    /// member per occurrence, and a `raw` value is its printed text.
    fn expected(&self, carrier: Carrier) -> Node {
        let split = carrier.pairs || carrier.codec == Some(CodecKind::XmlParams);
        let raw = carrier.codec == Some(CodecKind::Raw);
        let mut members = Vec::new();
        for parameter in &self.parameters {
            let occurrences: Vec<&Node> = match &parameter.value.value {
                Value::Array(items) if split && parameter.shape.repeatable => {
                    items.iter().collect()
                }
                _ => vec![&parameter.value],
            };
            for value in occurrences {
                let value = if raw {
                    node(Value::String(raw_text(value, &parameter.shape.value)))
                } else {
                    value.clone()
                };
                members.push(member(&parameter.name, value));
            }
        }
        node(Value::Object(members))
    }
}

const fn node(value: Value) -> Node {
    Node { value, span: None }
}

fn member(key: &str, value: Node) -> Member {
    Member {
        key: key.to_owned(),
        key_span: None,
        value,
    }
}

/// Whether `shape` is read verbatim (not as JSON) by the `xml-params` codec.
const fn verbatim(shape: &ValueShape) -> bool {
    matches!(
        shape,
        ValueShape::String(_) | ValueShape::Enumeration(_) | ValueShape::Unmodeled
    )
}

/// A value's printed text where it is read verbatim: a string's own text,
/// any other value's JSON spelling.
fn raw_text(value: &Node, shape: &ValueShape) -> String {
    match &value.value {
        Value::String(text) if verbatim(shape) => text.clone(),
        _ => value.to_json(),
    }
}

/// Words of generated strings: significant leading, inner and trailing
/// whitespace included, nothing that could begin markup.
const WORDS: [&str; 8] = [
    "alpha",
    "beta gamma",
    "  indented",
    "line one\nline two",
    "tab\tseparated",
    "trailing  ",
    "src/lib.rs",
    "\n  block\n",
];

struct Printer<'g> {
    grammar: &'g CompiledGrammar,
    options: WitnessOptions,
    rng: SplitMix64,
    patterns: Vec<Option<Pattern>>,
    out: Vec<u8>,
    call: Option<Plan<'g>>,
    param: Option<(usize, usize)>,
}

/// Which call captures an expression holds.
#[derive(Clone, Copy, Debug, Default)]
struct Holds {
    name: bool,
    count: usize,
    parameters: bool,
    dropped: bool,
}

impl<'g> Printer<'g> {
    fn emit(&mut self, bytes: &[u8], governor: &mut Governor) -> Result<(), StopReason> {
        governor.charge(Resource::Work, size(bytes.len()).saturating_mul(K_WITNESS))?;
        governor.charge(Resource::ScratchBytes, size(bytes.len()))?;
        self.out.extend_from_slice(bytes);
        Ok(())
    }

    /// The codec of the call production's arguments or parameter slot.
    fn codec_of_call(&self) -> Option<CodecKind> {
        fn find(grammar: &CompiledGrammar, expr: &Expr) -> Option<CodecKind> {
            match expr {
                Expr::Capture {
                    kind: CaptureKind::Arguments | CaptureKind::ParamValue,
                    inner,
                } => match **inner {
                    Expr::Slot { codec, .. } => Some(codec),
                    _ => find(grammar, inner),
                },
                Expr::Capture { inner, .. } | Expr::Repeat { inner, .. } => find(grammar, inner),
                Expr::Sequence(items) | Expr::Choice(items) => {
                    items.iter().find_map(|item| find(grammar, item))
                }
                Expr::Reference(id) => find(grammar, grammar.production_expr(*id)),
                _ => None,
            }
        }
        find(self.grammar, self.grammar.call())
    }

    fn holds(&self, expr: &Expr) -> Holds {
        let mut holds = Holds::default();
        self.collect_holds(expr, &mut holds);
        holds
    }

    fn collect_holds(&self, expr: &Expr, holds: &mut Holds) {
        match expr {
            Expr::Capture { kind, inner } => {
                match kind {
                    CaptureKind::Name => holds.name = true,
                    CaptureKind::ParamName => holds.parameters = true,
                    CaptureKind::Dropped => holds.dropped = true,
                    _ => {}
                }
                if !matches!(
                    kind,
                    CaptureKind::Content | CaptureKind::Reasoning | CaptureKind::Dropped
                ) {
                    holds.count += 1;
                }
                self.collect_holds(inner, holds);
            }
            Expr::Repeat { inner, .. } => self.collect_holds(inner, holds),
            Expr::Sequence(items) | Expr::Choice(items) => {
                for item in items {
                    self.collect_holds(item, holds);
                }
            }
            Expr::Reference(id) => self.collect_holds(self.grammar.production_expr(*id), holds),
            _ => {}
        }
    }

    fn walk(
        &mut self,
        expr: &Expr,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<(), WitnessError> {
        governor.charge(Resource::Work, K_WITNESS)?;
        let grammar = self.grammar;
        match expr {
            Expr::Literal(id) => {
                let bytes = grammar.literal(*id);
                if matches!(capture, Some(CaptureKind::Name)) {
                    let plan = self.call.as_ref().expect("printing a call");
                    if bytes != plan.name.as_bytes() {
                        return Err(WitnessError::Unrepresentable {
                            text: plan.name.clone(),
                        });
                    }
                }
                self.emit(bytes, governor)?;
            }
            Expr::Closer(id) => self.emit(&grammar.closer_spellings(*id)[0], governor)?,
            Expr::Class(id) => self.class(*id, capture, governor)?,
            Expr::Until(_) => {}
            Expr::Slot { codec, delimiters } => {
                self.slot(*codec, *delimiters, capture, governor)?;
            }
            Expr::Capture { kind, inner } => self.walk(inner, Some(*kind), governor)?,
            Expr::Sequence(items) => {
                for item in items {
                    self.walk(item, capture, governor)?;
                }
            }
            Expr::Choice(items) => {
                let holds: Vec<Holds> = items.iter().map(|item| self.holds(item)).collect();
                let best = holds
                    .iter()
                    .map(|hold| (hold.name, hold.count))
                    .max()
                    .unwrap_or((false, 0));
                let candidates: Vec<usize> = (0..items.len())
                    .filter(|&index| (holds[index].name, holds[index].count) == best)
                    .collect();
                let pick = candidates[self.rng.below(candidates.len())];
                self.walk(&items[pick], capture, governor)?;
            }
            Expr::Repeat { inner, min, max } => {
                self.repeat(inner, *min, *max, capture, governor)?;
            }
            Expr::Reference(id) => self.walk(grammar.production_expr(*id), capture, governor)?,
        }
        Ok(())
    }

    fn repeat(
        &mut self,
        inner: &Expr,
        min: u32,
        max: Option<u32>,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<(), WitnessError> {
        let holds = self.holds(inner);
        let fits = |count: usize| {
            size(count) >= u64::from(min) && max.is_none_or(|max| size(count) <= u64::from(max))
        };
        if holds.parameters {
            let plan = self.call.as_ref().expect("printing a call");
            let occurrences: Vec<usize> = plan
                .parameters
                .iter()
                .enumerate()
                .flat_map(|(index, parameter)| {
                    let count = match &parameter.value.value {
                        Value::Array(items) if parameter.shape.repeatable => items.len(),
                        _ => 1,
                    };
                    core::iter::repeat_n(index, count)
                })
                .collect();
            if !fits(occurrences.len()) {
                return Err(WitnessError::Unsupported {
                    reason: format!(
                        "{} parameter occurrences do not fit a repetition of {min}..{max:?}",
                        occurrences.len()
                    ),
                });
            }
            let mut seen: Vec<usize> = Vec::new();
            for index in occurrences {
                let occurrence = seen.iter().filter(|&&other| other == index).count();
                seen.push(index);
                self.param = Some((index, occurrence));
                self.walk(inner, capture, governor)?;
            }
            self.param = None;
            return Ok(());
        }
        let count = if holds.dropped {
            // Text a call drops is never part of a well-formed call.
            min
        } else if holds.count > 0 {
            min.max(1)
        } else {
            let extra = u32::from(max.is_none_or(|max| max > min) && self.rng.coin());
            min + extra
        };
        if !fits(count as usize) {
            return Err(WitnessError::Unsupported {
                reason: format!("a repetition of {min}..{max:?} cannot hold its captures"),
            });
        }
        for _ in 0..count {
            self.walk(inner, capture, governor)?;
        }
        Ok(())
    }

    /// The current parameter of a parameter-pair repetition, and which
    /// occurrence of it is printed.
    fn current(&self) -> Result<(&Generated<'g>, usize), WitnessError> {
        let plan = self.call.as_ref().expect("printing a call");
        let (index, occurrence) = self.param.ok_or_else(|| WitnessError::Unsupported {
            reason: "a parameter capture outside a repetition".into(),
        })?;
        Ok((&plan.parameters[index], occurrence))
    }

    fn class(
        &mut self,
        id: ClassId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<(), WitnessError> {
        let text = match capture {
            Some(CaptureKind::Name) => self.call.as_ref().expect("printing a call").name.clone(),
            Some(CaptureKind::ParamName) => self.current()?.0.name.clone(),
            Some(CaptureKind::Id) => {
                let candidate = format!("call_{:08x}", self.rng.next() & 0xFFFF_FFFF);
                let text = if self.accepts(id, candidate.as_bytes(), governor)? {
                    candidate
                } else {
                    let min_len = 4 + self.rng.below(4);
                    self.generate_class(id, min_len, governor)?
                };
                self.call.as_mut().expect("printing a call").id = Some(text.clone());
                text
            }
            _ => {
                let min_len = self.rng.below(3);
                self.generate_class(id, min_len, governor)?
            }
        };
        if !self.accepts(id, text.as_bytes(), governor)? {
            return Err(WitnessError::Unrepresentable { text });
        }
        self.emit(text.as_bytes(), governor)?;
        Ok(())
    }

    /// Whether class `id` matches all of `text`.
    fn accepts(
        &self,
        id: ClassId,
        text: &[u8],
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        if text.is_empty() {
            return Ok(false);
        }
        let mut dfa = self.grammar.classes[id.index()].automaton.dfa(governor)?;
        let mut state = dfa.start();
        for &byte in text {
            state = dfa.step(state, byte, governor)?;
        }
        dfa.accepting(state, governor)
    }

    /// A member of class `id` of at least `min_len` characters (or the
    /// shortest member, when there is none that long).
    fn generate_class(
        &mut self,
        id: ClassId,
        min_len: usize,
        governor: &mut Governor,
    ) -> Result<String, WitnessError> {
        let source = self.grammar.class_source(id);
        if self.patterns[id.index()].is_none() {
            let anchored = format!("^(?:{source})$");
            let pattern =
                Pattern::compile_governed(&anchored, governor).map_err(|error| match error {
                    PatternCompileError::Resource(reason) => WitnessError::Resource(reason),
                    PatternCompileError::Syntax(error) => WitnessError::Unsupported {
                        reason: format!("class /{source}/: {error}"),
                    },
                })?;
            self.patterns[id.index()] = Some(pattern);
        }
        let pattern = self.patterns[id.index()].as_ref().expect("compiled above");
        for min_len in [min_len, 0] {
            let outcome = pattern.generate(
                governor,
                GenerateOptions {
                    min_len,
                    max_len: Some(min_len + 16),
                    avoid: &[],
                },
            )?;
            if let GenerateOutcome::Witness(text) = outcome
                && !text.is_empty()
            {
                return Ok(text);
            }
        }
        Err(WitnessError::NoCandidate {
            class: source.to_owned(),
        })
    }

    fn slot(
        &mut self,
        codec: CodecKind,
        delimiters: DelimiterSetId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<(), WitnessError> {
        let text = match (capture, codec) {
            (Some(CaptureKind::Arguments), CodecKind::Json) => {
                let plan = self.call.as_ref().expect("printing a call");
                let object = plan
                    .expected(Carrier {
                        codec: Some(CodecKind::Json),
                        pairs: false,
                    })
                    .to_json();
                match self.options.json_arguments {
                    JsonArguments::Object => object,
                    JsonArguments::Stringified => quote(&object),
                }
            }
            (Some(CaptureKind::Arguments), CodecKind::XmlParams) => self.xml_parameters()?,
            (Some(CaptureKind::ParamValue), CodecKind::Raw) => {
                let (parameter, occurrence) = self.current()?;
                let value = occurrence_value(parameter, occurrence);
                raw_text(value, &parameter.shape.value)
            }
            (Some(CaptureKind::ParamValue), CodecKind::Json) => {
                let (parameter, occurrence) = self.current()?;
                occurrence_value(parameter, occurrence).to_json()
            }
            _ => {
                return Err(WitnessError::Unsupported {
                    reason: format!(
                        "a {} slot under {:?} cannot carry call arguments",
                        codec.name(),
                        capture.map(CaptureKind::name)
                    ),
                });
            }
        };
        governor.charge(Resource::Work, size(text.len()).saturating_mul(K_WITNESS))?;
        let bytes = text.as_bytes();
        let cut = self.grammar.delimiters(delimiters).iter().any(|delimiter| {
            bytes
                .windows(delimiter.len())
                .any(|window| window == delimiter)
        });
        if cut {
            return Err(WitnessError::Unrepresentable { text });
        }
        self.emit(bytes, governor)?;
        Ok(())
    }

    /// The `xml-params` body of the current call.
    fn xml_parameters(&self) -> Result<String, WitnessError> {
        let plan = self.call.as_ref().expect("printing a call");
        let mut body = String::from("\n");
        for parameter in &plan.parameters {
            let items: Vec<&Node> = match &parameter.value.value {
                Value::Array(items) if parameter.shape.repeatable => items.iter().collect(),
                _ => vec![&parameter.value],
            };
            for item in items {
                let text = raw_text(item, &parameter.shape.value);
                if text.contains("<parameter=") || text.contains("</parameter") {
                    return Err(WitnessError::Unrepresentable { text });
                }
                body.push_str("<parameter=");
                body.push_str(&parameter.name);
                body.push_str(">\n");
                body.push_str(&text);
                body.push_str("\n</parameter>\n");
            }
        }
        Ok(body)
    }

    /// Shape-conforming values for an object scope: every required
    /// parameter, and each optional one by the seed.
    fn object_parameters(
        &mut self,
        object: &'g ObjectShape,
        governor: &mut Governor,
    ) -> Result<Vec<Generated<'g>>, WitnessError> {
        let mut parameters = Vec::new();
        for parameter in &object.parameters {
            if !parameter.required && !self.rng.coin() {
                continue;
            }
            let value = if parameter.repeatable {
                let count = 1 + self.rng.below(2);
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.value(&parameter.value, governor)?);
                }
                node(Value::Array(items))
            } else {
                self.value(&parameter.value, governor)?
            };
            parameters.push(Generated {
                name: parameter.name.clone(),
                value,
                shape: parameter,
            });
        }
        Ok(parameters)
    }

    fn value(
        &mut self,
        shape: &'g ValueShape,
        governor: &mut Governor,
    ) -> Result<Node, WitnessError> {
        governor.charge(Resource::Work, K_WITNESS)?;
        Ok(node(match shape {
            ValueShape::String(Some(constraint)) => {
                let min_len = self.rng.below(4);
                match constraint.pattern.generate(
                    governor,
                    GenerateOptions {
                        min_len,
                        max_len: Some(min_len + 32),
                        avoid: &[],
                    },
                )? {
                    GenerateOutcome::Witness(text) => Value::String(text),
                    GenerateOutcome::NoCandidate(_) => {
                        return Err(WitnessError::NoCandidate {
                            class: constraint.source.clone(),
                        });
                    }
                }
            }
            ValueShape::String(None) | ValueShape::Unmodeled => {
                let count = 1 + self.rng.below(2);
                let mut text = String::new();
                for index in 0..count {
                    if index > 0 {
                        text.push(' ');
                    }
                    text.push_str(WORDS[self.rng.below(WORDS.len())]);
                }
                Value::String(text)
            }
            ValueShape::Enumeration(literals) if !literals.is_empty() => {
                Value::String(literals[self.rng.below(literals.len())].clone())
            }
            ValueShape::Enumeration(_) => {
                return Err(WitnessError::Unsupported {
                    reason: "an enumeration with no literal".into(),
                });
            }
            ValueShape::Integer => {
                let magnitude = self.rng.below(1000);
                let spelled = if self.rng.coin() {
                    format!("-{magnitude}")
                } else {
                    magnitude.to_string()
                };
                Value::Number(number(&spelled))
            }
            ValueShape::Number => {
                let spelled = format!("{}.{:02}", self.rng.below(100), self.rng.below(100));
                Value::Number(number(&spelled))
            }
            ValueShape::Boolean => Value::Bool(self.rng.coin()),
            ValueShape::Array(item) => {
                let count = self.rng.below(3);
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.value(item, governor)?);
                }
                Value::Array(items)
            }
            ValueShape::Object(object) => {
                let members = self
                    .object_parameters(object, governor)?
                    .into_iter()
                    .map(|parameter| member(&parameter.name, parameter.value))
                    .collect();
                Value::Object(members)
            }
        }))
    }
}

/// The value of one occurrence of a (possibly repeatable) parameter.
fn occurrence_value<'n>(parameter: &'n Generated<'_>, occurrence: usize) -> &'n Node {
    match &parameter.value.value {
        Value::Array(items) if parameter.shape.repeatable => &items[occurrence],
        _ => &parameter.value,
    }
}

fn number(spelled: &str) -> Number {
    spelled
        .parse()
        .expect("generated number spellings are valid JSON numbers")
}
