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
//! - any other class prints a string [`Pattern::generate`](crate::pattern::Pattern::generate) finds for the
//!   class (a shortest member of a seeded minimum length);
//! - an `arguments:` slot prints shape-conforming arguments in the slot
//!   codec's encoding (a JSON object, a stringified JSON object, or
//!   `xml-params` elements with string values verbatim between one framing
//!   newline on each side and every other value as JSON); a `param-value:`
//!   slot prints one parameter value;
//! - an `until` body is empty;
//! - an alternation picks, by the seed, among the alternatives holding the
//!   most call captures (a name first), never a repaired form (one holding
//!   `insert(...)`) when another alternative exists, and a repetition holding captures is
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
//! The text is printed by the shared production walker of [`super::print`],
//! driven by seeded choices: it charges [`K_PRINT`](super::cost::K_PRINT)
//! `Work` per expression visited and per byte printed and the printed text
//! as `ScratchBytes`; generation charges [`K_WITNESS`] `Work` per value and
//! the recorded calls as `ScratchBytes`; every class pattern compilation,
//! match and generation runs under the same governor.
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
use super::print::{Fill, PrintError, Printer, Repetition, fits, unsupported};
use super::{CaptureKind, ClassId, CompiledGrammar, DelimiterSetId, Expr};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource};
use crate::json::{Member, Node, Number, Value, quote};
use crate::pattern::{GenerateOptions, GenerateOutcome};
use crate::tool_call::{ArgumentShape, ObjectShape, ParameterShape, ValueShape};

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

/// Why no witness was generated: the shared [`PrintError`] of the
/// production printer this generator drives.
///
/// - [`PrintError::Unsupported`]: the `call` production has a form the
///   generator cannot fill: a `name:`, `id:` or `param-name:` capture that
///   is not a class or a literal, a slot outside an `arguments:` or
///   `param-value:` capture, a codec that cannot carry the value, or too few
///   parameters for a repetition's minimum.
/// - [`PrintError::Unrepresentable`]: a name the shape requires is not
///   accepted by the class capturing it, or a printed value contains a
///   delimiter ending its slot.
/// - [`PrintError::NoCandidate`]: a class has no member within the
///   generation bounds.
/// - [`PrintError::Resource`]: the governor's budget was exhausted, or the
///   caller cancelled.
pub type WitnessError = PrintError;

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
    let mut printer = Printer::new(grammar);
    let mut fill = Seeded {
        options,
        rng: SplitMix64(options.seed),
        call: None,
        occurrences: Vec::new(),
        param: None,
        group: Vec::new(),
        printed: Vec::new(),
    };
    let carrier = Carrier {
        codec: printer
            .arguments_slot(grammar.call())
            .map(|(codec, _)| codec),
        pairs: printer.holds(grammar.call()).parameters,
    };
    let mut calls = Vec::with_capacity(shapes.len());
    let grouped = printer.holds(grammar.call()).next;
    if grouped {
        // The production separates calls with `next`: every call is printed
        // in one instance of it.
        let Some((first, rest)) = shapes.split_first() else {
            return Ok(Witness {
                text: String::new(),
                calls,
            });
        };
        fill.group = rest.iter().rev().copied().collect();
        let parameters = fill.object_parameters(&first.arguments, governor)?;
        fill.call = Some(Plan {
            name: first.tool.clone(),
            id: None,
            parameters,
        });
        printer.walk(&mut fill, grammar.call(), None, governor)?;
        if !fill.group.is_empty() {
            return Err(unsupported("the production printed fewer calls than given"));
        }
        let last = fill
            .call
            .take()
            .ok_or_else(|| unsupported("the production printed no call"))?;
        fill.printed.push(last);
        for plan in core::mem::take(&mut fill.printed) {
            calls.push(plan.finish(carrier, governor)?);
        }
    }
    for (index, shape) in shapes.iter().enumerate().filter(|_| !grouped) {
        if index > 0 {
            printer.emit(b"\n", governor)?;
        }
        let parameters = fill.object_parameters(&shape.arguments, governor)?;
        fill.call = Some(Plan {
            name: shape.tool.clone(),
            id: None,
            parameters,
        });
        printer.walk(&mut fill, grammar.call(), None, governor)?;
        let Some(plan) = fill.call.take() else {
            unreachable!("the plan lives for the call")
        };
        calls.push(plan.finish(carrier, governor)?);
    }
    let text = String::from_utf8(printer.out).map_err(|error| PrintError::Unrepresentable {
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
    /// The witness call this plan printed, charged.
    fn finish(
        self,
        carrier: Carrier,
        governor: &mut Governor,
    ) -> Result<WitnessCall, WitnessError> {
        let arguments = self.expected(carrier);
        governor.charge(
            Resource::ScratchBytes,
            arguments
                .estimated_bytes()
                .saturating_add(size(self.name.len()))
                .saturating_add(size(self.id.as_ref().map_or(0, String::len))),
        )?;
        Ok(WitnessCall {
            name: self.name,
            id: self.id,
            arguments,
        })
    }

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

/// The seeded choices and generated values of a witness.
struct Seeded<'g> {
    options: WitnessOptions,
    rng: SplitMix64,
    call: Option<Plan<'g>>,
    /// The parameter index of each occurrence of the current parameter-pair
    /// repetition.
    occurrences: Vec<usize>,
    param: Option<(usize, usize)>,
    /// The shapes of the group's calls still to print after a `next`, last
    /// first.
    group: Vec<&'g ArgumentShape>,
    /// The group's calls printed before the current one.
    printed: Vec<Plan<'g>>,
}

impl<'g> Seeded<'g> {
    const fn plan(&self) -> &Plan<'g> {
        self.call.as_ref().expect("printing a call")
    }

    /// The current parameter of a parameter-pair repetition, and which
    /// occurrence of it is printed.
    fn current(&self) -> Result<(&Generated<'g>, usize), WitnessError> {
        let (index, occurrence) = self
            .param
            .ok_or_else(|| unsupported("a parameter capture outside a repetition"))?;
        Ok((&self.plan().parameters[index], occurrence))
    }

    /// The `xml-params` body of the current call.
    fn xml_parameters(&self) -> Result<String, WitnessError> {
        let mut body = String::from("\n");
        for parameter in &self.plan().parameters {
            let items: Vec<&Node> = match &parameter.value.value {
                Value::Array(items) if parameter.shape.repeatable => items.iter().collect(),
                _ => vec![&parameter.value],
            };
            for item in items {
                let text = raw_text(item, &parameter.shape.value);
                if text.contains("<parameter=") || text.contains("</parameter") {
                    return Err(PrintError::Unrepresentable { text });
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
                        return Err(PrintError::NoCandidate {
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
                return Err(unsupported("an enumeration with no literal"));
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

impl Fill for Seeded<'_> {
    fn name(&self) -> Option<&str> {
        self.call.as_ref().map(|plan| plan.name.as_str())
    }

    /// By the seed, among the alternatives holding the most call captures
    /// (a name first).
    fn choose(&mut self, printer: &Printer<'_>, alternatives: &[Expr]) -> usize {
        let holds: Vec<_> = alternatives
            .iter()
            .map(|item| printer.holds(item))
            .collect();
        // A repaired form (one that always holds `insert`) is never a
        // witness's choice when a well-formed alternative exists.
        let repaired: Vec<bool> = alternatives
            .iter()
            .map(|item| printer.repaired(item))
            .collect();
        let clean = repaired.iter().any(|repaired| !repaired);
        let allowed = |index: usize| !(clean && repaired[index]);
        let best = (0..alternatives.len())
            .filter(|&index| allowed(index))
            .map(|index| (holds[index].name, holds[index].count))
            .max()
            .unwrap_or((false, 0));
        let candidates: Vec<usize> = (0..alternatives.len())
            .filter(|&index| allowed(index) && (holds[index].name, holds[index].count) == best)
            .collect();
        candidates[self.rng.below(candidates.len())]
    }

    fn repetition(
        &mut self,
        printer: &Printer<'_>,
        inner: &Expr,
        min: u32,
        max: Option<u32>,
    ) -> Result<Repetition, WitnessError> {
        let holds = printer.holds(inner);
        if holds.next {
            // Once per further call of the group.
            let count = self.group.len();
            if !fits(count, min, max) {
                return Err(unsupported(format!(
                    "{count} further calls do not fit a repetition of {min}..{max:?}"
                )));
            }
            return Ok(Repetition::Times(u32::try_from(count).unwrap_or(u32::MAX)));
        }
        if holds.parameters {
            let occurrences: Vec<usize> = self
                .plan()
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
            if !fits(occurrences.len(), min, max) {
                return Err(unsupported(format!(
                    "{} parameter occurrences do not fit a repetition of {min}..{max:?}",
                    occurrences.len()
                )));
            }
            let count = occurrences.len();
            self.occurrences = occurrences;
            return Ok(Repetition::Parameters(count));
        }
        let count = if holds.dropped || printer.bare_slot(inner) {
            // Text a call drops is never part of a well-formed call, and a
            // slot no capture holds has no value to print.
            min
        } else if holds.count > 0 {
            min.max(1)
        } else {
            let extra = u32::from(max.is_none_or(|max| max > min) && self.rng.coin());
            min + extra
        };
        if !fits(count as usize, min, max) {
            return Err(unsupported(format!(
                "a repetition of {min}..{max:?} cannot hold its captures"
            )));
        }
        Ok(Repetition::Times(count))
    }

    fn next_call(&mut self, governor: &mut Governor) -> Result<(), WitnessError> {
        let shape = self
            .group
            .pop()
            .ok_or_else(|| unsupported("a `next` with no further call to print"))?;
        let done = self.call.take().expect("printing a call");
        self.printed.push(done);
        let parameters = self.object_parameters(&shape.arguments, governor)?;
        self.call = Some(Plan {
            name: shape.tool.clone(),
            id: None,
            parameters,
        });
        self.occurrences.clear();
        self.param = None;
        Ok(())
    }

    fn ordinal(&mut self) -> String {
        self.printed.len().to_string()
    }

    fn composite_id(&mut self, text: &str) {
        self.call.as_mut().expect("printing a call").id = Some(text.to_owned());
    }

    fn occurrence(&mut self, occurrence: Option<usize>) {
        self.param = occurrence.map(|position| {
            let index = self.occurrences[position];
            let earlier = self.occurrences[..position]
                .iter()
                .filter(|&&other| other == index)
                .count();
            (index, earlier)
        });
    }

    fn literal(&mut self, capture: CaptureKind, bytes: &[u8]) -> Result<(), WitnessError> {
        let plan = self.plan();
        if capture == CaptureKind::Name && bytes != plan.name.as_bytes() {
            return Err(PrintError::Unrepresentable {
                text: plan.name.clone(),
            });
        }
        Ok(())
    }

    fn class(
        &mut self,
        printer: &mut Printer<'_>,
        id: ClassId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<String, WitnessError> {
        Ok(match capture {
            Some(CaptureKind::Name) => self.plan().name.clone(),
            Some(CaptureKind::ParamName) => self.current()?.0.name.clone(),
            Some(CaptureKind::Id) => {
                let candidate = format!("call_{:08x}", self.rng.next() & 0xFFFF_FFFF);
                let text = if printer.accepts(id, candidate.as_bytes(), governor)? {
                    candidate
                } else {
                    let min_len = 4 + self.rng.below(4);
                    printer.generate_class(id, min_len, governor)?
                };
                self.call.as_mut().expect("printing a call").id = Some(text.clone());
                text
            }
            _ => {
                let min_len = self.rng.below(3);
                printer.generate_class(id, min_len, governor)?
            }
        })
    }

    fn slot(
        &mut self,
        _printer: &Printer<'_>,
        codec: CodecKind,
        _delimiters: DelimiterSetId,
        capture: Option<CaptureKind>,
    ) -> Result<String, WitnessError> {
        match (capture, codec) {
            (Some(CaptureKind::Arguments), CodecKind::Json | CodecKind::JsonArgs) => {
                let object = self
                    .plan()
                    .expected(Carrier {
                        codec: Some(CodecKind::Json),
                        pairs: false,
                    })
                    .to_json();
                Ok(match self.options.json_arguments {
                    JsonArguments::Object => object,
                    JsonArguments::Stringified => quote(&object),
                })
            }
            (Some(CaptureKind::Arguments), CodecKind::XmlParams) => self.xml_parameters(),
            (Some(CaptureKind::ParamValue), CodecKind::Raw) => {
                let (parameter, occurrence) = self.current()?;
                let value = occurrence_value(parameter, occurrence);
                Ok(raw_text(value, &parameter.shape.value))
            }
            (Some(CaptureKind::ParamValue), CodecKind::Json | CodecKind::JsonArgs) => {
                let (parameter, occurrence) = self.current()?;
                Ok(occurrence_value(parameter, occurrence).to_json())
            }
            _ => Err(unsupported(format!(
                "a {} slot under {:?} cannot carry call arguments",
                codec.name(),
                capture.map(CaptureKind::name)
            ))),
        }
    }

    fn until(&mut self, _capture: Option<CaptureKind>) -> Result<String, WitnessError> {
        Ok(String::new())
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
