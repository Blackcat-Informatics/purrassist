// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Printing recovered calls, tool results, and tool declarations (D2, C6,
//! C7, R1.8, R2.6, R7.3).
//!
//! Printing is the inverse of recognition: every dialect printing here runs
//! the dialect grammar's own productions through the shared production
//! printer ([`crate::grammar::print`]), so recognizing what is printed gives
//! back the same calls (identity, `Node::equivalent` arguments, order) with
//! no new loss.
//!
//! # Which calls are emitted
//!
//! Exactly the calls [`ToolCalls::emitted_calls`] selects are emitted: when
//! the turn passes the request's tool-choice check, every `Resolved` call
//! and a `BestGuess` call only on a tool whose effective effects are
//! [`Effects::ReadOnly`](super::Effects::ReadOnly)
//! ([`ToolCalls::usable_accepting_guesses`]); when it fails the check,
//! none ([`SkipReason::ToolChoiceFailed`]). The same selection decides
//! [`ToolCalls::verdict`] and [`ToolCalls::openai_finish_reason`], so the
//! emitted `finish_reason` is `"tool_calls"` exactly when the emission
//! holds a call, except under a length stop, which is always `"length"`.
//! Emitted output (an `OpenAI` `tool_calls` array in particular) carries no
//! outcome, and a downstream executor may run whatever it holds, so a guess
//! on a mutating tool is never emitted. Nothing is dropped silently; every
//! call or attempt that is not printed is listed in [`Emitted::skipped`], in
//! output order:
//!
//! - an `Unresolved` call ([`SkipReason::Unresolved`], with its reason), an
//!   `Ambiguous` call ([`SkipReason::Ambiguous`]: it has no single reading
//!   to print), and a `BestGuess` call on a tool that is not read-only
//!   ([`SkipReason::GuessOnMutatingTool`]);
//! - every call of a turn that fails the tool-choice check
//!   ([`SkipReason::ToolChoiceFailed`] for a call that would otherwise
//!   run);
//! - a call the target dialect cannot carry ([`SkipReason::Unrepresentable`]:
//!   an id or name the dialect's class does not accept, or a value that
//!   cannot be written without ending its slot early);
//! - every rejected call attempt ([`Skipped::Rejected`]): it was never a
//!   call.
//!
//! # Canonical forms
//!
//! Each emitted call is printed by the target grammar's `call` production
//! with the printer's canonical choices (first alternative carrying the
//! most values; optional whitespace printed as the grammar's `layout` text
//! where it declares one, omitted otherwise); calls are separated by one
//! newline. A `call` production that separates calls with `next`
//! (Mistral v3's array) prints every emitted call in one instance of it:
//! each call is first printed alone, so a call the dialect cannot carry is
//! skipped without failing the others. Ids come from an [`IdPolicy`] applied exactly as recovery
//! applies it, so passing the request's policy reproduces
//! [`ToolCall::id`], except in a composite id (Kimi-K2's
//! `functions.NAME:IDX`, which must hold the tool name): that is printed
//! from the call's name and its position among the printed calls.
//!
//! | Dialect | Canonical call |
//! | --- | --- |
//! | `bare-json.v1` | `{"name":"NAME","arguments":ARGS}` (compact), one object per call, the calls separated by a newline |
//! | `anthropic-tool-use.v1` | `{"type":"tool_use","id":"ID","name":"NAME","input":ARGS}` (the API's member order; `ARGS` is compact) |
//! | `qwen3-coder-xml.v1` | `<tool_call>\n<function=NAME>` + body + `</function>\n</tool_call>` (the chat template's layout) |
//! | `deepseek-v3.1.v1` | `<｜tool▁calls▁begin｜>` + `<｜tool▁call▁begin｜>NAME<｜tool▁sep｜>ARGS<｜tool▁call▁end｜>` per call + `<｜tool▁calls▁end｜>`: every printed call in one section (the template's layout; `ARGS` is compact) |
//! | `deepseek-v3.v1` | `<｜tool▁calls▁begin｜>` + `<｜tool▁call▁begin｜>function<｜tool▁sep｜>NAME\n```json\nARGS\n```<｜tool▁call▁end｜>` per call, the calls separated by a newline, + `<｜tool▁calls▁end｜>`: every printed call in one section (the template's layout; `ARGS` is compact) |
//! | `kimi-k2.v1` | `<\|tool_calls_section_begin\|>` + `<\|tool_call_begin\|>functions.NAME:IDX<\|tool_call_argument_begin\|>ARGS<\|tool_call_end\|>` per call + `<\|tool_calls_section_end\|>`: every printed call in one section (the template's layout; `IDX` is the call's position in the section, `ARGS` is compact) |
//! | `harmony.v1` | `<\|start\|>assistant<\|channel\|>commentary to=functions.NAME <\|constrain\|>json<\|message\|>ARGS<\|call\|>` (the documented call layout; `ARGS` is compact), one message per call |
//! | `hermes.v1` | `<tool_call>\n{"name":"NAME","arguments":ARGS}\n</tool_call>` (the template's wrapper layout; the object is compact) |
//! | `openai-tool-calls.v1` | `{"id":"ID","type":"function","function":{"name":"NAME","arguments":"ARGS"}}` |
//! | `llama3-json.v1` | `{"name": "NAME", "parameters": ARGS}` (the template's layout; `ARGS` is compact) |
//! | `mistral-v11.v1` | `[TOOL_CALLS]NAME[CALL_ID]ID[ARGS]ARGS` per call (the v11 layout; `ARGS` is compact) |
//! | `mistral-v3.v1` | `[TOOL_CALLS] [{"name": "NAME", "arguments": ARGS, "id": "ID"}, ...]`: every printed call in one array (the template's layout; `ARGS` is compact) |
//!
//! `ARGS` is the compact JSON of the argument tree with every number's
//! exact spelling ([`Node::to_json`]); the `OpenAI` dialect carries it
//! stringified, as the API does, and every other `json` slot as an object.
//! The `xml-params` body (Qwen3-Coder) is one `<parameter=NAME>` element
//! per top-level member: a string value of a parameter the codec reads
//! verbatim (declared string, enumeration or unmodeled, or undeclared) is
//! written verbatim between one framing newline on each side; every other
//! value, nested objects included, is inline compact JSON; a repeatable
//! parameter is one element per item. The body is checked by decoding it
//! with the codec under the tool's shape: when that does not give back the
//! same tree with framing newlines as the only edits (an unmodeled
//! parameter holding a non-string, a value containing parameter markup),
//! the body is instead one whole JSON object, which the codec reads
//! independently of the shape. JSON text that would contain a slot
//! delimiter or parameter markup writes every `<` as `\u003c`.
//!
//! [`openai_tool_calls`] returns the `OpenAI` `tool_calls` array itself as
//! a value: exactly `{"id", "type": "function", "function": {"name",
//! "arguments"}}` per emitted call, where `arguments` is the compact JSON
//! string of the argument tree; no `role` and no message envelope. Its
//! [`Node::to_json`] elements are byte-identical to the `OpenAI` dialect
//! printing of the same calls.
//!
//! # Minimal-diff re-emission
//!
//! [`ToolCalls::reemit_with`] applies [`CallEdit`]s to the original output
//! bytes: only the bytes of values that actually change are regenerated,
//! each in its source call's own encoding; everything else is copied
//! verbatim (see its documentation for the granularity).
//!
//! # Resources
//!
//! Printing charges the governor as documented on
//! [`crate::grammar::print`]; codec checks, id assignment, and `OpenAI`
//! values charge `Work` per byte and `ScratchBytes` for what they retain.
//! Exhaustion and cancellation are [`EmitError::Resource`], never a partial
//! output.
//!
//! ```
//! use purrassist_core::tool_call::{emit, DialectId, IdPolicy, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
//! use purrassist_core::{Budget, Outcome};
//! let tools = ToolSet::compile([ToolDefinition::new("read").param("path", Param::string().required())], Budget::default()).unwrap();
//! let output = b"<tool_call>\n<function=read>\n<parameter=path>\nsrc/a b.rs\n</parameter>\n</function>\n</tool_call>";
//! let Outcome::Resolved(calls) = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in()).outcome else { unreachable!() };
//! let hermes = emit::emit(&tools, &calls, &DialectId::Hermes, &IdPolicy::default())?;
//! assert_eq!(hermes.output, b"<tool_call>\n{\"name\":\"read\",\"arguments\":{\"path\":\"src/a b.rs\"}}\n</tool_call>");
//! assert_eq!((hermes.calls.as_slice(), hermes.skipped.len()), (&[0][..], 0));
//! let array = emit::openai_tool_calls(&calls, &IdPolicy::default())?;
//! assert_eq!(
//!     array.output.to_json(),
//!     r#"[{"id":"call_03cf7be79285_0","type":"function","function":{"name":"read","arguments":"{\"path\":\"src/a b.rs\"}"}}]"#,
//! );
//! # Ok::<(), emit::EmitError>(())
//! ```
use super::recover::{IdSource, assign_ids};
use super::{
    ArgumentShape, CallOutcome, CompiledTool, DialectId, IdPolicy, ObjectShape, Rejection,
    ToolCall, ToolCalls, ToolSet, UnresolvedReason, ValueShape,
};
use crate::contract::saturating_size as size;
use crate::contract::{
    Budget, Governor, Resource, ScratchCharge, ScratchString, ScratchVec, StopReason,
};
use crate::grammar::codec::{
    Codec, CodecContext, CodecEnd, CodecKind, KvPairsCodec, XmlParamsCodec,
};
use crate::grammar::edit::EditOperator;
use crate::grammar::print::{
    Canonical, PrintError, Printer, Root, ScratchPrintError, ScratchPrinted, Values,
    canonical_scratch, contains_any, guard_json, unsupported,
};
use crate::grammar::{CaptureKind, DelimiterSetId};
use crate::json::{self, Member, Node, Value};
use crate::wire::{array, byte_range, exact_bytes, integer, object, string};
use core::fmt;
use core::ops::Range;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

/// The parameter markup of the `xml-params` codec a value must not contain.
const PARAMETER_MARKUP: [&[u8]; 2] = [b"<parameter=", b"</parameter"];

/// The effective parameters schema of a declaration that has none.
pub(crate) const NO_PARAMETERS: &str = r#"{"type":"object"}"#;

/// What [`ToolSet::recover`] prints for the recovered calls
/// ([`ToolCalls::emitted`]).
///
/// Either target emits exactly the calls [`ToolCalls::emitted_calls`]
/// selects: a `BestGuess` call on a tool that is not read-only is skipped
/// ([`SkipReason::GuessOnMutatingTool`]), never emitted, and nothing of a
/// turn that fails the tool-choice check is
/// ([`SkipReason::ToolChoiceFailed`]).
///
/// ```
/// use purrassist_core::tool_call::{DialectId, EmitTarget, ToolRecoveryRequest};
/// assert_eq!(EmitTarget::default(), EmitTarget::None);
/// let request = ToolRecoveryRequest::default().emit(EmitTarget::Dialect(DialectId::Hermes));
/// assert_eq!(request.emit, EmitTarget::Dialect(DialectId::Hermes));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum EmitTarget {
    /// Nothing ([`ToolCalls::emitted`] is `None`).
    #[default]
    None,
    /// Canonical markup of this dialect (see [`emit`]).
    Dialect(DialectId),
    /// The `OpenAI` `tool_calls` array (see [`openai_tool_calls`]).
    OpenAiToolCalls,
}

/// Why a call was not emitted.
///
/// ```
/// use purrassist_core::tool_call::{SkipReason, UnresolvedReason};
/// assert_eq!(SkipReason::Ambiguous.code(), "ambiguous");
/// let skip = SkipReason::Unresolved { reason: UnresolvedReason::Truncated };
/// assert_eq!(skip.code(), "unresolved");
/// assert!(skip.to_string().contains("truncated"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The call's outcome is `Unresolved`.
    Unresolved {
        /// Why.
        reason: UnresolvedReason,
    },
    /// The call's outcome is `Ambiguous`: it has no single reading.
    Ambiguous,
    /// The call's outcome is `BestGuess` and its tool is not read-only:
    /// emitted output carries no outcome, so a guessed mutation is never
    /// emitted (see [`ToolCall::is_usable_accepting_guesses`]).
    GuessOnMutatingTool,
    /// The target cannot carry the call.
    Unrepresentable {
        /// What could not be printed.
        detail: String,
    },
    /// The call would run on its own, but the turn fails the request's
    /// tool-choice check: nothing of the turn may run
    /// ([`Verdict::NothingUsable`](super::Verdict::NothingUsable)), so
    /// nothing is emitted (see [`ToolCalls::emitted_calls`]).
    ToolChoiceFailed,
}

impl SkipReason {
    /// The wire code: `unresolved`, `ambiguous`, `guess_on_mutating_tool`,
    /// `unrepresentable`, or `tool_choice_failed`.
    ///
    /// See [`SkipReason`] for an example.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unresolved { .. } => "unresolved",
            Self::Ambiguous => "ambiguous",
            Self::GuessOnMutatingTool => "guess_on_mutating_tool",
            Self::Unrepresentable { .. } => "unrepresentable",
            Self::ToolChoiceFailed => "tool_choice_failed",
        }
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unresolved { reason } => {
                write!(formatter, "unresolved ({}): {reason}", reason.code())
            }
            Self::Ambiguous => formatter.write_str("ambiguous: no single reading"),
            Self::GuessOnMutatingTool => {
                formatter.write_str("a best guess on a tool that is not read-only")
            }
            Self::Unrepresentable { detail } => write!(formatter, "unrepresentable: {detail}"),
            Self::ToolChoiceFailed => {
                formatter.write_str("the turn fails the request's tool-choice check")
            }
        }
    }
}

/// One call or attempt an emission did not print.
///
/// ```
/// use purrassist_core::tool_call::{Rejection, SkipReason, Skipped};
/// let skipped = Skipped::Call { index: 1, reason: SkipReason::Ambiguous };
/// assert_ne!(skipped, Skipped::Rejected { index: 0, span: 0..4, reason: Rejection::Unterminated });
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skipped {
    /// The call at [`ToolCall::index`] `index`.
    Call {
        /// The call's index.
        index: usize,
        /// Why it was not printed.
        reason: SkipReason,
    },
    /// The rejected attempt at position `index` of [`ToolCalls::rejected`].
    Rejected {
        /// The attempt's position.
        index: usize,
        /// Its source range.
        span: Range<usize>,
        /// Why the recognizer rejected it.
        reason: Rejection,
    },
}

/// An emission: the printed output, which calls it holds, and what it does
/// not.
///
/// See the [module documentation](self) for an example.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emitted<T> {
    /// The printed output.
    pub output: T,
    /// The [`ToolCall::index`] of every printed call, in output order.
    pub calls: Vec<usize>,
    /// Every call and rejected attempt not printed, in output order.
    pub skipped: Vec<Skipped>,
}

/// The emission recovery made for its [`EmitTarget`].
///
/// ```
/// use purrassist_core::tool_call::{DialectId, Emission, EmitTarget, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::{Budget, Outcome};
/// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
/// let request = ToolRecoveryRequest::default().emit(EmitTarget::Dialect(DialectId::Hermes));
/// let result = tools.recover(br#"{"id":"c1","type":"function","function":{"name":"ls","arguments":"{}"}}"#, &request, purrassist_core::tool_call::DialectSet::built_in());
/// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
/// let Some(Emission::Dialect { dialect, emitted }) = &calls.emitted else { unreachable!() };
/// assert_eq!(dialect, &DialectId::Hermes);
/// assert_eq!(emitted.output, b"<tool_call>\n{\"name\":\"ls\",\"arguments\":{}}\n</tool_call>");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Emission {
    /// Canonical markup of `dialect` (see [`emit`]).
    Dialect {
        /// The target dialect.
        dialect: DialectId,
        /// The printed markup.
        emitted: Emitted<Vec<u8>>,
    },
    /// The `OpenAI` `tool_calls` array (see [`openai_tool_calls`]).
    OpenAiToolCalls(Emitted<Node>),
}

impl Emission {
    /// The calls and attempts the emission did not print.
    ///
    /// ```
    /// use purrassist_core::json::{Node, Value};
    /// use purrassist_core::tool_call::{Emission, Emitted};
    /// let emission = Emission::OpenAiToolCalls(Emitted { output: Node::new(Value::Array(Vec::new())), calls: Vec::new(), skipped: Vec::new() });
    /// assert!(emission.skipped().is_empty());
    /// ```
    #[must_use]
    pub fn skipped(&self) -> &[Skipped] {
        match self {
            Self::Dialect { emitted, .. } => &emitted.skipped,
            Self::OpenAiToolCalls(emitted) => &emitted.skipped,
        }
    }

    /// The wire form: `{target: "dialect", dialect, output: {text |
    /// bytes_hex}, calls, skipped}` or `{target: "openai_tool_calls",
    /// output: [...], calls, skipped}`; each skipped entry is `{kind:
    /// "call", index, reason: {code, detail}}` or `{kind: "rejected", index,
    /// span: {start, end}, reason: {code, detail}}`.
    pub(crate) fn to_node(&self) -> Node {
        let tail = |calls: &[usize], skipped: &[Skipped]| {
            [
                (
                    "calls",
                    array(calls.iter().map(|&index| integer(size(index)))),
                ),
                ("skipped", array(skipped.iter().map(skipped_node))),
            ]
        };
        match self {
            Self::Dialect { dialect, emitted } => {
                let [calls, skipped] = tail(&emitted.calls, &emitted.skipped);
                object([
                    ("target", string("dialect")),
                    ("dialect", string(dialect.id())),
                    ("output", object([exact_bytes(&emitted.output)])),
                    calls,
                    skipped,
                ])
            }
            Self::OpenAiToolCalls(emitted) => {
                let [calls, skipped] = tail(&emitted.calls, &emitted.skipped);
                object([
                    ("target", string("openai_tool_calls")),
                    ("output", emitted.output.clone()),
                    calls,
                    skipped,
                ])
            }
        }
    }
}

fn skipped_node(skipped: &Skipped) -> Node {
    match skipped {
        Skipped::Call { index, reason } => object([
            ("kind", string("call")),
            ("index", integer(size(*index))),
            (
                "reason",
                object([
                    ("code", string(reason.code())),
                    ("detail", string(reason.to_string())),
                ]),
            ),
        ]),
        Skipped::Rejected {
            index,
            span,
            reason,
        } => object([
            ("kind", string("rejected")),
            ("index", integer(size(*index))),
            ("span", byte_range(span)),
            (
                "reason",
                object([
                    ("code", string("rejected")),
                    ("detail", string(format!("{reason:?}"))),
                ]),
            ),
        ]),
    }
}

/// Why printing failed.
///
/// ```
/// use purrassist_core::tool_call::emit::EmitError;
/// use purrassist_core::{StopReason, contract::Resource};
/// let error = EmitError::from(StopReason::Cancelled);
/// assert_eq!(error, EmitError::Resource(StopReason::Cancelled));
/// assert!(EmitError::UnknownTool { name: "rm".into() }.to_string().contains("rm"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmitError {
    /// The production cannot carry what is printed (see
    /// [`PrintError::Unsupported`]).
    Unsupported {
        /// What could not be filled.
        reason: String,
    },
    /// A value cannot be printed in place (see
    /// [`PrintError::Unrepresentable`]).
    Unrepresentable {
        /// The text that cannot be printed.
        text: String,
    },
    /// A class has no member within the generation bounds.
    NoCandidate {
        /// The class pattern source.
        class: String,
    },
    /// The id policy cannot number the calls: a [`super::IdGenerator`]
    /// returned one id twice, or a prefix counter overflows.
    InvalidIds {
        /// Why.
        reason: String,
    },
    /// A [`CallEdit`] does not apply: an unknown call index, a pointer that
    /// names no single value, or source bytes that do not hold the
    /// recovered calls.
    InvalidEdit {
        /// Why.
        reason: String,
    },
    /// No declared tool has this canonical name.
    UnknownTool {
        /// The name asked for.
        name: String,
    },
    /// The governor's budget was exhausted, or the caller cancelled.
    Resource(StopReason),
}

impl fmt::Display for EmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(formatter, "unsupported production: {reason}"),
            Self::Unrepresentable { text } => {
                write!(formatter, "`{text}` cannot be printed in place")
            }
            Self::NoCandidate { class } => write!(formatter, "class /{class}/ has no candidate"),
            Self::InvalidIds { reason } => write!(formatter, "invalid id policy: {reason}"),
            Self::InvalidEdit { reason } => write!(formatter, "invalid edit: {reason}"),
            Self::UnknownTool { name } => write!(formatter, "no declared tool `{name}`"),
            Self::Resource(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for EmitError {}

impl From<StopReason> for EmitError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

impl From<ScratchPrintError> for EmitError {
    fn from(error: ScratchPrintError) -> Self {
        Self::from(PrintError::from(error))
    }
}

impl From<PrintError> for EmitError {
    fn from(error: PrintError) -> Self {
        match error {
            PrintError::Unsupported { reason } => Self::Unsupported { reason },
            PrintError::Unrepresentable { text } => Self::Unrepresentable { text },
            PrintError::NoCandidate { class } => Self::NoCandidate { class },
            PrintError::Resource(reason) => Self::Resource(reason),
        }
    }
}

fn invalid_edit(reason: impl Into<String>) -> EmitError {
    EmitError::InvalidEdit {
        reason: reason.into(),
    }
}

/// Print `calls` as canonical markup of `dialect` under a fresh governor
/// with the default [`Budget`].
///
/// Emits exactly the calls [`ToolCalls::emitted_calls`] selects (every
/// `Resolved` call, and a `BestGuess` call only on a read-only tool, when
/// the turn passes the tool-choice check); every other call, a guess on a
/// mutating tool included ([`SkipReason::GuessOnMutatingTool`]), and every
/// rejected attempt is listed in [`Emitted::skipped`].
///
/// # Errors
/// See [`emit_governed`].
///
/// See the [module documentation](self) for an example.
pub fn emit(
    tools: &ToolSet,
    calls: &ToolCalls,
    dialect: &DialectId,
    ids: &IdPolicy,
) -> Result<Emitted<Vec<u8>>, EmitError> {
    emit_governed(
        tools,
        calls,
        dialect,
        ids,
        &mut Governor::new(Budget::default()),
    )
}

/// Print `calls` as canonical markup of `dialect` against a caller-owned
/// governor.
///
/// `tools` supplies the argument shapes the `xml-params` codec reads values
/// by; see the [module documentation](self) for the canonical forms.
/// Emits exactly the calls [`ToolCalls::emitted_calls`] selects (every
/// `Resolved` call, and a `BestGuess` call only on a read-only tool, when
/// the turn passes the tool-choice check); every other call, a guess on a
/// mutating tool included ([`SkipReason::GuessOnMutatingTool`]), and every
/// rejected attempt is listed in [`Emitted::skipped`].
///
/// # Errors
/// [`EmitError::InvalidIds`] when `ids` cannot number the calls, and
/// [`EmitError::Resource`] on exhaustion or cancellation. A call the
/// dialect cannot carry is never an error: it is listed in
/// [`Emitted::skipped`].
///
/// ```
/// use purrassist_core::tool_call::{emit, DialectId, IdPolicy, SkipReason, Skipped, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::{Budget, Governor, Outcome};
/// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
/// let output = br#"<tool_call>{"name": "ls", "arguments": {}}</tool_call><tool_call>{"name": "rm", "arguments": {}}</tool_call>"#;
/// let calls = match tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in()).outcome {
///     Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
///     _ => unreachable!(),
/// };
/// let ids = IdPolicy::Prefix { prefix: "t".into(), start: 7 };
/// let emitted = emit::emit_governed(&tools, &calls, &DialectId::OpenAiToolCalls, &ids, &mut Governor::new(Budget::default()))?;
/// assert_eq!(emitted.output, br#"{"id":"t7","type":"function","function":{"name":"ls","arguments":"{}"}}"#);
/// assert!(matches!(emitted.skipped[..], [Skipped::Call { index: 1, reason: SkipReason::Unresolved { .. } }]));
/// # Ok::<(), emit::EmitError>(())
/// ```
pub fn emit_governed(
    tools: &ToolSet,
    calls: &ToolCalls,
    dialect: &DialectId,
    ids: &IdPolicy,
    governor: &mut Governor,
) -> Result<Emitted<Vec<u8>>, EmitError> {
    let ids = numbered(calls, ids, governor)?;
    Ok(dialect_markup(Some(tools), calls, dialect, &ids, governor)?)
}

/// The `OpenAI` `tool_calls` array of `calls` under a fresh governor with
/// the default [`Budget`].
///
/// Emits exactly the calls [`ToolCalls::emitted_calls`] selects (every
/// `Resolved` call, and a `BestGuess` call only on a read-only tool, when
/// the turn passes the tool-choice check); every other call, a guess on a
/// mutating tool included ([`SkipReason::GuessOnMutatingTool`]), and every
/// rejected attempt is listed in [`Emitted::skipped`].
///
/// # Errors
/// See [`openai_tool_calls_governed`].
///
/// See the [module documentation](self) for an example.
pub fn openai_tool_calls(calls: &ToolCalls, ids: &IdPolicy) -> Result<Emitted<Node>, EmitError> {
    openai_tool_calls_governed(calls, ids, &mut Governor::new(Budget::default()))
}

/// The `OpenAI` `tool_calls` array of `calls` against a caller-owned
/// governor (C7).
///
/// The array is exactly `[{"id", "type": "function", "function":
/// {"name", "arguments"}}, ...]`, one element per emitted call, with the
/// id from `ids`, the canonical tool name, and `arguments` the compact JSON
/// string of the argument tree (exact number spellings). Pair it with
/// [`ToolCalls::openai_finish_reason`] for the message's `finish_reason`:
/// under a length stop it is `"length"` (the array still holds the calls
/// that may run), otherwise `"tool_calls"` exactly when this array holds a
/// call, because both select calls with [`ToolCalls::emitted_calls`].
/// Emits exactly the calls [`ToolCalls::emitted_calls`] selects (every
/// `Resolved` call, and a `BestGuess` call only on a read-only tool, when
/// the turn passes the tool-choice check); every other call, a guess on a
/// mutating tool included ([`SkipReason::GuessOnMutatingTool`]), and every
/// rejected attempt is listed in [`Emitted::skipped`].
///
/// # Errors
/// [`EmitError::InvalidIds`] when `ids` cannot number the calls, and
/// [`EmitError::Resource`] on exhaustion or cancellation.
///
/// ```
/// use purrassist_core::tool_call::{emit, IdGenerator, IdPolicy, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::{Budget, Governor, Outcome};
/// use std::sync::Arc;
/// struct Ids;
/// impl IdGenerator for Ids { fn id(&self, index: usize) -> String { format!("toolu_{index}") } }
/// let tools = ToolSet::compile([ToolDefinition::new("pay").param("amount", Param::number())], Budget::default()).unwrap();
/// let output = br#"<tool_call>{"name": "pay", "arguments": {"amount": 1.50}}</tool_call>"#;
/// let Outcome::Resolved(calls) = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in()).outcome else { unreachable!() };
/// let array = emit::openai_tool_calls_governed(&calls, &IdPolicy::Generator(Arc::new(Ids)), &mut Governor::new(Budget::default()))?;
/// assert_eq!(
///     array.output.to_json(),
///     r#"[{"id":"toolu_0","type":"function","function":{"name":"pay","arguments":"{\"amount\":1.50}"}}]"#,
/// );
/// # Ok::<(), emit::EmitError>(())
/// ```
pub fn openai_tool_calls_governed(
    calls: &ToolCalls,
    ids: &IdPolicy,
    governor: &mut Governor,
) -> Result<Emitted<Node>, EmitError> {
    let ids = numbered(calls, ids, governor)?;
    Ok(openai_array(calls, &ids, governor)?)
}

/// The emission recovery makes for `target`, numbering calls by their
/// assigned [`ToolCall::id`]s.
pub(crate) fn emission(
    tools: &ToolSet,
    calls: &ToolCalls,
    target: &EmitTarget,
    governor: &mut Governor,
) -> Result<Option<Emission>, StopReason> {
    let ids: Vec<&str> = calls.calls.iter().map(|call| call.id.as_str()).collect();
    Ok(match target {
        EmitTarget::None => None,
        EmitTarget::Dialect(dialect) => Some(Emission::Dialect {
            dialect: dialect.clone(),
            emitted: dialect_markup(Some(tools), calls, dialect, &ids, governor)?,
        }),
        EmitTarget::OpenAiToolCalls => Some(Emission::OpenAiToolCalls(openai_array(
            calls, &ids, governor,
        )?)),
    })
}

/// Every call's id under `policy`, as recovery assigns them.
fn numbered(
    calls: &ToolCalls,
    policy: &IdPolicy,
    governor: &mut Governor,
) -> Result<Vec<String>, EmitError> {
    let sources: Vec<IdSource> = calls.calls.iter().map(IdSource::of).collect();
    let (ids, _) = assign_ids(policy, &calls.turn, &sources, governor, |_, _, _, _| Ok(()))
        .map_err(|reason| match reason {
            StopReason::InvalidInput(reason) => EmitError::InvalidIds { reason },
            other => EmitError::Resource(other),
        })?;
    Ok(ids)
}

/// The calls an emission prints, and every call and attempt it skips (by
/// outcome), keyed by source start.
fn selection(calls: &ToolCalls) -> (Vec<&ToolCall>, Vec<(usize, Skipped)>) {
    let mut printed = Vec::new();
    let mut skipped = Vec::new();
    for call in &calls.calls {
        // The one selection rule (`ToolCalls::emitted_calls`):
        // `openai_finish_reason` and `verdict` use it too.
        if call.is_usable_accepting_guesses() && calls.tool_choice.passed {
            printed.push(call);
            continue;
        }
        let reason = match &call.outcome {
            _ if call.is_usable_accepting_guesses() => SkipReason::ToolChoiceFailed,
            CallOutcome::Unresolved { reason } => SkipReason::Unresolved {
                reason: reason.clone(),
            },
            CallOutcome::Ambiguous => SkipReason::Ambiguous,
            CallOutcome::BestGuess => SkipReason::GuessOnMutatingTool,
            CallOutcome::Resolved => SkipReason::Unrepresentable {
                detail: "the call names no declared tool".into(),
            },
        };
        skipped.push((
            call.span.start,
            Skipped::Call {
                index: call.index,
                reason,
            },
        ));
    }
    for (index, rejected) in calls.rejected.iter().enumerate() {
        skipped.push((
            rejected.span.start,
            Skipped::Rejected {
                index,
                span: rejected.span.clone(),
                reason: rejected.reason,
            },
        ));
    }
    (printed, skipped)
}

/// The skipped entries in output order.
fn ordered(mut skipped: Vec<(usize, Skipped)>) -> Vec<Skipped> {
    skipped.sort_by_key(|(start, _)| *start);
    skipped.into_iter().map(|(_, skipped)| skipped).collect()
}

// Newly governed skip details stay transient until the whole envelope succeeds.
struct PendingDiagnostic {
    position: usize,
    detail: ScratchString,
}
impl ScratchCharge for PendingDiagnostic {
    // 64-bit position (8), String (24), meter handle (8), reservation (8).
    const CHARGE: usize = 48;
}
const _: () = assert!(core::mem::size_of::<PendingDiagnostic>() <= PendingDiagnostic::CHARGE);

fn dialect_markup<S: AsRef<str>>(
    tools: Option<&ToolSet>,
    calls: &ToolCalls,
    dialect: &DialectId,
    ids: &[S],
    governor: &mut Governor,
) -> Result<Emitted<Vec<u8>>, StopReason> {
    let (selected, mut skipped) = selection(calls);
    let mut pending = ScratchVec::new(governor);
    let mut printer = Printer::new(dialect.grammar(), governor);
    let mut emitted = Vec::with_capacity(selected.len());
    let grouped = printer.holds(dialect.grammar().call(), governor)?.next;
    let mut group: Vec<(&ToolCall, Prepared<'_>)> = Vec::new();
    for call in selected {
        let mark = printer.out.len();
        if mark > 0 && !grouped {
            printer.emit(b"\n", governor)?;
        }
        let tool = tools
            .zip(call.tool.as_deref())
            .and_then(|(tools, name)| tools.lookup_exact(name));
        let id = ids.get(call.index).map(AsRef::as_ref);
        // A production that separates calls with `next` prints the whole
        // group in one instance; each call is first printed alone, so a
        // call it cannot carry is skipped rather than failing the group.
        let outcome = prepare(&printer, call, id, tool, dialect, governor)
            .map_err(ScratchPrintError::from)
            .and_then(|prepared| {
                let rendered = canonical_scratch(
                    printer.grammar,
                    Root::Call,
                    prepared.values(),
                    &[],
                    governor,
                )?;
                if grouped {
                    group.push((call, prepared));
                } else {
                    printer.emit(rendered.bytes.as_slice(), governor)?;
                }
                Ok(())
            });
        match outcome {
            Ok(()) => {
                if !grouped {
                    emitted.push(call.index);
                }
            }
            Err(ScratchPrintError::Resource(reason)) => return Err(reason),
            Err(error) => {
                while printer.out.len() > mark {
                    printer.out.pop();
                }
                pending.push(PendingDiagnostic {
                    position: skipped.len(),
                    detail: error.into_detail(governor)?,
                })?;
                skipped.push((
                    call.span.start,
                    Skipped::Call {
                        index: call.index,
                        reason: SkipReason::Unrepresentable {
                            detail: String::new(),
                        },
                    },
                ));
            }
        }
    }
    print_group(
        &mut printer,
        &group,
        &mut emitted,
        &mut skipped,
        &mut pending,
        governor,
    )?;
    governor.check()?;
    while let Some(PendingDiagnostic { position, detail }) = pending.pop() {
        let (
            _,
            Skipped::Call {
                reason: SkipReason::Unrepresentable { detail: target },
                ..
            },
        ) = &mut skipped[position]
        else {
            unreachable!("pending details refer to their canonical skip entries")
        };
        *target = detail.into_string();
    }
    Ok(Emitted {
        output: printer.out.into_vec(),
        calls: emitted,
        skipped: ordered(skipped),
    })
}

fn print_group(
    printer: &mut Printer<'_>,
    group: &[(&ToolCall, Prepared<'_>)],
    emitted: &mut Vec<usize>,
    skipped: &mut Vec<(usize, Skipped)>,
    pending: &mut ScratchVec<PendingDiagnostic>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    if let Some(((_, first), rest)) = group.split_first() {
        let mut values = ScratchVec::new(governor);
        governor.charge(Resource::Work, size(rest.len()))?;
        values.extend(rest.iter().map(|(_, prepared)| prepared.values()))?;
        match canonical_scratch(
            printer.grammar,
            Root::Call,
            first.values(),
            values.as_slice(),
            governor,
        ) {
            Ok(rendered) => {
                printer.emit(rendered.bytes.as_slice(), governor)?;
                emitted.extend(group.iter().map(|(call, _)| call.index));
            }
            Err(ScratchPrintError::Resource(reason)) => return Err(reason),
            Err(error) => {
                let detail = match error {
                    ScratchPrintError::Unsupported { reason }
                        if reason.as_str()
                            == "the production prints fewer calls than the group holds" =>
                    {
                        reason
                    }
                    error => error.into_detail(governor)?,
                };
                printer.out.clear();
                let mut detail = Some(detail);
                for (position, (call, _)) in group.iter().enumerate() {
                    let item = if position + 1 == group.len() {
                        detail.take().expect("final grouped diagnostic")
                    } else {
                        let original = detail.as_ref().expect("grouped diagnostic");
                        governor.charge(Resource::Work, size(original.as_str().len()))?;
                        let mut copy = ScratchString::new(governor);
                        copy.push_str(original.as_str())?;
                        copy
                    };
                    pending.push(PendingDiagnostic {
                        position: skipped.len(),
                        detail: item,
                    })?;
                    skipped.push((
                        call.span.start,
                        Skipped::Call {
                            index: call.index,
                            reason: SkipReason::Unrepresentable {
                                detail: String::new(),
                            },
                        },
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Whether `dialect`'s `json` argument slot carries stringified arguments
/// (the `OpenAI` wire form).
fn stringifies(dialect: &DialectId) -> bool {
    *dialect == DialectId::OpenAiToolCalls
}

/// The values one call fills into the dialect's `call` production.
struct Prepared<'c> {
    name: &'c str,
    id: Option<&'c str>,
    arguments: Option<String>,
    parameters: Option<&'c [Member]>,
}

impl Prepared<'_> {
    fn values(&self) -> Values<'_> {
        Values {
            name: Some(self.name),
            id: self.id,
            arguments: self.arguments.as_deref(),
            parameters: self.parameters,
            ..Values::default()
        }
    }
}

/// The values of `call` for the dialect's `call` production.
fn prepare<'c>(
    printer: &Printer<'_>,
    call: &'c ToolCall,
    id: Option<&'c str>,
    tool: Option<&CompiledTool>,
    dialect: &DialectId,
    governor: &mut Governor,
) -> Result<Prepared<'c>, PrintError> {
    let production = printer.grammar.call();
    let name = call
        .tool
        .as_deref()
        .ok_or_else(|| unsupported("the call names no declared tool"))?;
    let mut prepared = Prepared {
        name,
        // An `optional-id` dialect reads ids a model adds but prints none.
        id: id.filter(|_| printer.grammar.call_carries_ids()),
        arguments: None,
        parameters: None,
    };
    if printer.holds(production, governor)?.parameters {
        let Value::Object(members) = &call.arguments.value else {
            return Err(unsupported(
                "parameter pairs carry only an arguments object",
            ));
        };
        prepared.parameters = Some(members);
    } else if let Some((codec, delimiters)) = printer.arguments_slot(production, governor)? {
        prepared.arguments = Some(arguments_text(
            printer,
            codec,
            delimiters,
            &call.arguments,
            tool,
            dialect,
            governor,
        )?);
    }
    Ok(prepared)
}

/// The text of an `arguments:` slot of `codec` holding `arguments`.
fn arguments_text(
    printer: &Printer<'_>,
    codec: CodecKind,
    delimiters: DelimiterSetId,
    arguments: &Node,
    tool: Option<&CompiledTool>,
    dialect: &DialectId,
    governor: &mut Governor,
) -> Result<String, PrintError> {
    let shape = tool.map(CompiledTool::argument_shape);
    let delimiters = printer.grammar.delimiters(delimiters);
    let json = arguments.to_json();
    governor.charge(Resource::Work, size(json.len()))?;
    match codec {
        CodecKind::Json | CodecKind::JsonArgs if stringifies(dialect) => {
            Ok(json::quote(&guard_json(json, delimiters)))
        }
        CodecKind::Json | CodecKind::JsonArgs => Ok(guard_json(json, delimiters)),
        CodecKind::XmlParams => xml_body(arguments, json, delimiters, shape, governor),
        CodecKind::KvPairs => kv_body(arguments, delimiters, tool, governor),
        CodecKind::Raw => Err(unsupported("a raw slot cannot carry an arguments object")),
    }
}

/// The public GLM template writes strings verbatim and other values as JSON.
/// Read back the complete body before publication; no escaping convention is
/// invented for text that collides with tag or call delimiters.
fn kv_body(
    arguments: &Node,
    delimiters: &[ScratchVec<u8>],
    tool: Option<&CompiledTool>,
    governor: &mut Governor,
) -> Result<String, PrintError> {
    let shape = tool.map(CompiledTool::argument_shape);
    let Value::Object(members) = &arguments.value else {
        return Err(unsupported("key/value pairs carry an arguments object"));
    };
    let mut body = String::new();
    let mut expected = Vec::new();
    let mut risky: Vec<&[u8]> = delimiters.iter().map(ScratchVec::as_slice).collect();
    risky.extend([
        b"<arg_key>".as_slice(),
        b"</arg_key>",
        b"<arg_value>",
        b"</arg_value>",
    ]);
    for member in members {
        let repeatable = shape
            .and_then(|shape| shape.arguments.parameter(&member.key))
            .is_some_and(|parameter| parameter.repeatable);
        let items = match &member.value.value {
            Value::Array(items) if repeatable => items.as_slice(),
            _ => core::slice::from_ref(&member.value),
        };
        for item in items {
            let text = match &item.value {
                Value::String(text) => text.clone(),
                _ => guard_json(item.to_json(), &risky),
            };
            governor.charge(
                Resource::Work,
                size(member.key.len()).saturating_add(size(text.len())),
            )?;
            body.push_str("<arg_key>");
            body.push_str(&member.key);
            body.push_str("</arg_key>\n<arg_value>");
            body.push_str(&text);
            body.push_str("</arg_value>\n");
            expected.push(Member {
                key: member.key.clone(),
                key_span: None,
                value: item.clone(),
            });
        }
    }
    if contains_any(body.as_bytes(), delimiters) {
        return Err(PrintError::Unrepresentable { text: body });
    }
    let context = CodecContext {
        shape,
        ..CodecContext::default()
    };
    let decoded = KvPairsCodec::begin(0, governor)?.finish(
        body.as_bytes(),
        0..body.len(),
        CodecEnd::Delimited,
        context,
        governor,
    )?;
    let expected = Node::new(Value::Object(expected));
    let matches = match decoded {
        Ok(value) => {
            let exact = value.tree.is_resolved()
                && value.alternative.is_none()
                && value
                    .edits
                    .iter()
                    .all(|edit| edit.operator == EditOperator::JsonString);
            if !exact {
                false
            } else if value.tree.root.equivalent_governed(&expected, governor)? {
                true
            } else if let Some(tool) = tool {
                match super::resolve::schema_readback(tool, &value.tree, governor)? {
                    Some(typed) => typed.equivalent_governed(&expected, governor)?,
                    None => false,
                }
            } else {
                false
            }
        }
        Err(_) => false,
    };
    if !matches {
        return Err(PrintError::Unrepresentable { text: body });
    }
    governor.check()?;
    Ok(body)
}

/// The canonical `xml-params` body of `arguments` (see the module
/// documentation).
fn xml_body(
    arguments: &Node,
    json: String,
    delimiters: &[ScratchVec<u8>],
    shape: Option<&ArgumentShape>,
    governor: &mut Governor,
) -> Result<String, PrintError> {
    let mut risky: Vec<&[u8]> = delimiters.iter().map(ScratchVec::as_slice).collect();
    risky.extend(PARAMETER_MARKUP);
    if let (Some(shape), Value::Object(members)) = (shape, &arguments.value) {
        let (body, expected) = parameter_body(members, &shape.arguments, &risky);
        if !contains_any(body.as_bytes(), delimiters)
            && reads_back(
                &body,
                &expected,
                shape,
                EditOperator::FramingNewline,
                governor,
            )?
        {
            return Ok(body);
        }
    }
    let body = guard_json(json, &risky);
    match shape {
        Some(shape) if !reads_back(&body, arguments, shape, EditOperator::JsonBody, governor)? => {
            Err(PrintError::Unrepresentable { text: body })
        }
        _ => Ok(body),
    }
}

/// Whether the `xml-params` codec of a parameter reads a string verbatim.
fn verbatim(shape: &ObjectShape, name: &str) -> bool {
    shape.parameter(name).is_none_or(|parameter| {
        matches!(
            parameter.value,
            ValueShape::String(_) | ValueShape::Enumeration(_) | ValueShape::Unmodeled
        )
    })
}

/// The parameter-element body of `members`, and the tree the codec should
/// read from it (a repeatable parameter's items as duplicate members).
fn parameter_body(members: &[Member], shape: &ObjectShape, risky: &[&[u8]]) -> (String, Node) {
    let mut body = String::from("\n");
    let mut expected = Vec::with_capacity(members.len());
    for member in members {
        let repeatable = shape
            .parameter(&member.key)
            .is_some_and(|parameter| parameter.repeatable);
        let items: Vec<&Node> = match &member.value.value {
            Value::Array(items) if repeatable => items.iter().collect(),
            _ => vec![&member.value],
        };
        for item in items {
            let text = match &item.value {
                Value::String(text) if verbatim(shape, &member.key) => text.clone(),
                _ => guard_json(item.to_json(), risky),
            };
            body.push_str("<parameter=");
            body.push_str(&member.key);
            body.push_str(">\n");
            body.push_str(&text);
            body.push_str("\n</parameter>\n");
            expected.push(Member {
                key: member.key.clone(),
                key_span: None,
                value: item.clone(),
            });
        }
    }
    (body, Node::new(Value::Object(expected)))
}

/// Whether the `xml-params` codec reads `body` under `shape` back as
/// `expected`, resolved, with no edit but `allowed` and those that only
/// record how the printed text spells the same value: closer text a string
/// value holds (`tool_call.value.closer-in-value`, a value written with
/// `</parameter>` inside it) and a JSON string escape (`json.syntax.string`,
/// as `\u003c`), and with no second reading (a value holding text after
/// an unpaired closer reads back `Ambiguous`). The body is printed only
/// when it reads back as the call (a round trip); otherwise the call is
/// skipped as unrepresentable.
fn reads_back(
    body: &str,
    expected: &Node,
    shape: &ArgumentShape,
    allowed: EditOperator,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    let bytes = body.as_bytes();
    let mut codec = XmlParamsCodec::begin(0, governor)?;
    for (at, &byte) in bytes.iter().enumerate() {
        codec.feed(byte, at, governor)?;
    }
    let grammar = DialectId::Qwen3CoderXml.grammar();
    let context = CodecContext {
        shape: Some(shape),
        openers: &grammar.call_markup[0],
        closers: &grammar.call_markup[1],
    };
    Ok(
        match codec.finish(
            bytes,
            0..bytes.len(),
            CodecEnd::Delimited,
            context,
            governor,
        )? {
            Ok(value) => {
                value.tree.is_resolved()
                    && value.alternative.is_none()
                    && value.tree.root.equivalent_governed(expected, governor)?
                    && value.edits.iter().all(|edit| {
                        edit.operator == allowed
                            || matches!(
                                edit.operator,
                                EditOperator::CloserInValue | EditOperator::JsonString
                            )
                    })
            }
            Err(_) => false,
        },
    )
}

fn openai_array<S: AsRef<str>>(
    calls: &ToolCalls,
    ids: &[S],
    governor: &mut Governor,
) -> Result<Emitted<Node>, StopReason> {
    let (printed, skipped) = selection(calls);
    let mut elements = Vec::with_capacity(printed.len());
    let mut emitted = Vec::with_capacity(printed.len());
    for call in printed {
        let arguments = call.arguments.to_json();
        governor.charge(Resource::Work, size(arguments.len()))?;
        let element = object([
            ("id", string(ids.get(call.index).map_or("", AsRef::as_ref))),
            ("type", string("function")),
            (
                "function",
                object([
                    ("name", string(call.tool.as_deref().unwrap_or_default())),
                    ("arguments", string(arguments)),
                ]),
            ),
        ]);
        governor.charge(Resource::ScratchBytes, element.estimated_bytes())?;
        elements.push(element);
        emitted.push(call.index);
    }
    Ok(Emitted {
        output: Node::new(Value::Array(elements)),
        calls: emitted,
        skipped: ordered(skipped),
    })
}

/// One tool result to print: the content and, where the dialect carries
/// them, the call id and tool name it answers.
///
/// ```
/// use purrassist_core::tool_call::ToolResult;
/// let result = ToolResult { id: Some("call_0"), name: None, content: "22 C" };
/// assert_eq!(result.content, "22 C");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolResult<'a> {
    /// The id of the call this result answers.
    pub id: Option<&'a str>,
    /// The name of the tool that produced it.
    pub name: Option<&'a str>,
    /// The result content, as text.
    pub content: &'a str,
}

/// A printed tool result.
///
/// See [`render_tool_result`] for an example.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedResult {
    /// The printed markup.
    pub bytes: Vec<u8>,
    /// The given fields (`id`, `name`) the dialect's `result` production
    /// does not carry, so the markup omits them: Qwen3-Coder and Hermes
    /// tool responses carry neither (the chat template pairs results with
    /// calls by order).
    pub omitted: Vec<CaptureKind>,
}

/// Print a tool result with `dialect`'s `result` production under a fresh
/// governor with the default [`Budget`].
///
/// A `content:` capture of an `until` body (Qwen3-Coder, Hermes) holds the
/// content verbatim; of a class (the `OpenAI` tool message's string
/// `content`), the content as a JSON string literal.
///
/// # Errors
/// See [`render_tool_result_governed`].
///
/// ```
/// use purrassist_core::grammar::CaptureKind;
/// use purrassist_core::tool_call::{emit, DialectId, ToolResult};
/// let result = ToolResult { id: Some("call_0"), name: None, content: "22 \"C\"" };
/// let openai = emit::render_tool_result(&DialectId::OpenAiToolCalls, &result)?;
/// assert_eq!(openai.bytes, br#"{"role":"tool","tool_call_id":"call_0","content":"22 \"C\""}"#);
/// let hermes = emit::render_tool_result(&DialectId::Hermes, &result)?;
/// assert_eq!(hermes.bytes, b"<tool_response>22 \"C\"</tool_response>");
/// assert_eq!(hermes.omitted, [CaptureKind::Id]);
/// # Ok::<(), emit::EmitError>(())
/// ```
pub fn render_tool_result(
    dialect: &DialectId,
    result: &ToolResult<'_>,
) -> Result<RenderedResult, EmitError> {
    render_tool_result_governed(dialect, result, &mut Governor::new(Budget::default()))
}

/// Print a tool result with `dialect`'s `result` production against a
/// caller-owned governor.
///
/// Recognizing the bytes gives back a result segment with the same content
/// (and id, where the dialect carries one).
///
/// # Errors
/// [`EmitError::Unsupported`] when the production needs a value the result
/// lacks (an `OpenAI` tool message needs an id),
/// [`EmitError::Unrepresentable`] when the content would end its body early
/// (Qwen3-Coder and Hermes content containing `</tool_response>`) or an id
/// is not accepted by the dialect, and [`EmitError::Resource`] on
/// exhaustion or cancellation.
///
/// ```
/// use purrassist_core::tool_call::{emit::{self, EmitError}, DialectId, ToolResult};
/// use purrassist_core::{Budget, Governor};
/// let unnumbered = ToolResult { id: None, name: None, content: "ok" };
/// let error = emit::render_tool_result_governed(&DialectId::OpenAiToolCalls, &unnumbered, &mut Governor::new(Budget::default()));
/// assert!(matches!(error, Err(EmitError::Unsupported { .. })));
/// let closing = ToolResult { content: "a</tool_response>", ..unnumbered };
/// assert!(matches!(emit::render_tool_result(&DialectId::Hermes, &closing), Err(EmitError::Unrepresentable { .. })));
/// ```
pub fn render_tool_result_governed(
    dialect: &DialectId,
    result: &ToolResult<'_>,
    governor: &mut Governor,
) -> Result<RenderedResult, EmitError> {
    let grammar = dialect.grammar();
    let rendered = canonical_scratch(
        grammar,
        Root::Result,
        Values {
            name: result.name,
            id: result.id,
            content: Some(result.content),
            ..Values::default()
        },
        &[],
        governor,
    )?;
    let omitted = [
        (result.id, CaptureKind::Id),
        (result.name, CaptureKind::Name),
    ]
    .into_iter()
    .filter(|(value, kind)| value.is_some() && !rendered.printed(*kind))
    .map(|(_, kind)| kind)
    .collect();
    Ok(RenderedResult {
        bytes: rendered.bytes.into_vec(),
        omitted,
    })
}

/// Print one tool declaration with `dialect`'s `declaration` production: the
/// name, the description when present, and the exact parameters schema text
/// verbatim (`{"type":"object"}` when the tool has none and the production
/// requires one).
pub(crate) fn declaration(
    dialect: &DialectId,
    name: &str,
    description: Option<&str>,
    parameters: Option<&str>,
    governor: &mut Governor,
) -> Result<String, EmitError> {
    let grammar = dialect.grammar();
    let printer = Printer::new(grammar, governor);
    let mut values = Values {
        name: Some(name),
        content: description,
        arguments: parameters,
        ..Values::default()
    };
    if parameters.is_none()
        && !Canonical::new(values).printable(&printer, grammar.declaration(), governor)?
    {
        values.arguments = Some(NO_PARAMETERS);
    }
    let rendered = canonical_scratch(grammar, Root::Declaration, values, &[], governor)?;
    for (given, kind, what) in [
        (description.is_some(), CaptureKind::Content, "description"),
        (
            parameters.is_some(),
            CaptureKind::Arguments,
            "parameters schema",
        ),
    ] {
        if given && !rendered.printed(kind) {
            return Err(unsupported(format!(
                "the `{}` declaration production cannot carry the {what}",
                grammar.id()
            ))
            .into());
        }
    }
    rendered.into_string(governor).map_err(EmitError::from)
}

/// One edit [`ToolCalls::reemit_with`] applies.
///
/// ```
/// use purrassist_core::json::{Node, Value};
/// use purrassist_core::tool_call::CallEdit;
/// let edit = CallEdit::Value { call: 1, pointer: "/path".into(), value: Node::new(Value::String("b.rs".into())) };
/// assert!(matches!(edit, CallEdit::Value { call: 1, .. }));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallEdit {
    /// Replace the whole arguments of the call at [`ToolCall::index`] `call`.
    Arguments {
        /// The call's index.
        call: usize,
        /// The new arguments.
        arguments: Node,
    },
    /// Replace the single argument value at RFC 6901 `pointer` (which must
    /// name an existing value; `""` is the whole arguments).
    Value {
        /// The call's index.
        call: usize,
        /// Where the value is.
        pointer: String,
        /// The new value.
        value: Node,
    },
}

impl CallEdit {
    const fn call(&self) -> usize {
        match self {
            Self::Arguments { call, .. } | Self::Value { call, .. } => *call,
        }
    }
}

/// How one call's arguments are spelled in the source.
struct Spelling<'s> {
    source: &'s [u8],
    /// The arguments are a JSON string holding JSON (the stringified form).
    stringified: bool,
    /// The arguments are an `xml-params` body.
    xml: bool,
    /// What spliced text must not contain: the slot's delimiters, and
    /// parameter markup in an `xml-params` body.
    risky: Vec<&'s [u8]>,
}

impl ToolCalls {
    /// Apply `edits` to the output these calls were recovered from, under a
    /// fresh governor with the default [`Budget`].
    ///
    /// # Errors
    /// See [`Self::reemit_with_governed`].
    ///
    /// ```
    /// use purrassist_core::json::{Node, Value};
    /// use purrassist_core::tool_call::{CallEdit, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, Outcome};
    /// let tools = ToolSet::compile([ToolDefinition::new("read").param("path", Param::string()).param("limit", Param::integer())], Budget::default()).unwrap();
    /// let output = b"Reading.\n<tool_call>\n<function=read>\n<parameter=path>\na.rs\n</parameter>\n<parameter=limit>\n5\n</parameter>\n</function>\n</tool_call>";
    /// let Outcome::Resolved(calls) = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in()).outcome else { unreachable!() };
    /// let edit = CallEdit::Value { call: 0, pointer: "/path".into(), value: Node::new(Value::String("src/b.rs".into())) };
    /// let edited = calls.reemit_with(output, &[edit])?;
    /// assert_eq!(edited, b"Reading.\n<tool_call>\n<function=read>\n<parameter=path>\nsrc/b.rs\n</parameter>\n<parameter=limit>\n5\n</parameter>\n</function>\n</tool_call>");
    /// # Ok::<(), purrassist_core::tool_call::emit::EmitError>(())
    /// ```
    pub fn reemit_with(&self, source: &[u8], edits: &[CallEdit]) -> Result<Vec<u8>, EmitError> {
        self.reemit_with_governed(source, edits, &mut Governor::new(Budget::default()))
    }

    /// Apply `edits` to `source`, the exact output these calls were
    /// recovered from (the `put` of the print/parse lens), against a
    /// caller-owned governor.
    ///
    /// The edits of each call are applied in order to a copy of its
    /// arguments; the result is compared with the recovered arguments
    /// value by value, and only values that differ are regenerated, each in
    /// place in its call's source encoding: a JSON value as compact JSON
    /// (escaped inside stringified arguments), a verbatim `xml-params`
    /// string verbatim between its existing framing, a nested-parameter
    /// object as inline JSON. Objects with the same member names in the same
    /// order and arrays of the same length are compared member by member and
    /// item by item; any other change replaces the whole value. A value that
    /// cannot be written in place (it has no source span, or its new text
    /// would end its slot or parameter early, or a verbatim parameter
    /// would receive a non-string) is regenerated with its enclosing value,
    /// up to the whole arguments, which an `xml-params` body then holds as
    /// one JSON object; a call whose arguments have no source span is
    /// reprinted whole with its dialect's canonical form. Content, reasoning,
    /// other calls, and every untouched byte of an edited call are copied
    /// verbatim.
    ///
    /// # Errors
    /// [`EmitError::InvalidEdit`] for an edit naming no call, a pointer that
    /// names no single value, or a `source` that does not hold the calls'
    /// spans; [`EmitError::Unrepresentable`] or [`EmitError::Unsupported`]
    /// when a call must be reprinted whole and its dialect cannot carry it;
    /// [`EmitError::Resource`] on exhaustion or cancellation.
    ///
    /// ```
    /// use purrassist_core::json::{Node, Value};
    /// use purrassist_core::tool_call::{CallEdit, ToolDefinition, ToolRecoveryRequest, ToolSet, emit::EmitError};
    /// use purrassist_core::{Budget, Governor, Outcome};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
    /// let output = br#"<tool_call>{"name": "ls", "arguments": {}}</tool_call>"#;
    /// let Outcome::Resolved(calls) = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in()).outcome else { unreachable!() };
    /// let missing = CallEdit::Value { call: 0, pointer: "/depth".into(), value: Node::new(Value::Null) };
    /// let error = calls.reemit_with_governed(output, &[missing], &mut Governor::new(Budget::default()));
    /// assert!(matches!(error, Err(EmitError::InvalidEdit { .. })));
    /// ```
    pub fn reemit_with_governed(
        &self,
        source: &[u8],
        edits: &[CallEdit],
        governor: &mut Governor,
    ) -> Result<Vec<u8>, EmitError> {
        let mut edited: BTreeMap<usize, Node> = BTreeMap::new();
        for edit in edits {
            governor.charge(Resource::Work, 1)?;
            let index = edit.call();
            let call = self
                .calls
                .get(index)
                .ok_or_else(|| invalid_edit(format!("no call has index {index}")))?;
            if source.get(call.span.clone()).is_none() {
                return Err(invalid_edit(format!(
                    "the source does not hold call {index}'s span {}..{}",
                    call.span.start, call.span.end
                )));
            }
            let arguments = match edited.entry(index) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    governor.charge(Resource::ScratchBytes, call.arguments.estimated_bytes())?;
                    entry.insert(call.arguments.clone())
                }
            };
            let (target, value) = match edit {
                CallEdit::Arguments {
                    arguments: value, ..
                } => (Some(&mut *arguments), value),
                CallEdit::Value { pointer, value, .. } => (arguments.pointer_mut(pointer), value),
            };
            let target = target.ok_or_else(|| {
                invalid_edit(format!("the pointer names no single value of call {index}"))
            })?;
            governor.charge(Resource::ScratchBytes, value.estimated_bytes())?;
            *target = value.clone();
        }
        let mut splices: Vec<(Range<usize>, SpliceText)> = Vec::new();
        for (index, arguments) in &edited {
            splice_call(
                &self.calls[*index],
                source,
                arguments,
                &mut splices,
                governor,
            )?;
        }
        splices.sort_by_key(|(range, _)| range.start);
        governor.charge(Resource::Work, size(source.len()))?;
        governor.charge(
            Resource::ScratchBytes,
            size(source.len()).saturating_add(
                splices
                    .iter()
                    .map(|(_, text)| size(text.as_bytes().len()))
                    .sum::<u64>(),
            ),
        )?;
        let mut output = Vec::with_capacity(source.len());
        let mut at = 0;
        for (range, text) in splices {
            if range.start < at {
                return Err(invalid_edit("two edited values overlap in the source"));
            }
            output.extend_from_slice(&source[at..range.start]);
            output.extend_from_slice(text.as_bytes());
            at = range.end;
        }
        output.extend_from_slice(&source[at..]);
        Ok(output)
    }
}

// Existing lens preparation and newly governed canonical chunks have distinct
// ownership until the complete source edit is assembled.
enum SpliceText {
    Prepared(String),
    Canonical(ScratchPrinted),
}
impl SpliceText {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Prepared(text) => text.as_bytes(),
            Self::Canonical(rendered) => rendered.bytes.as_slice(),
        }
    }
}

/// The splices that turn `call`'s recovered arguments into `arguments`.
fn splice_call(
    call: &ToolCall,
    source: &[u8],
    arguments: &Node,
    splices: &mut Vec<(Range<usize>, SpliceText)>,
    governor: &mut Governor,
) -> Result<(), EmitError> {
    let grammar = call.dialect.grammar();
    let printer = Printer::new(grammar, governor);
    let slot = printer.arguments_slot(grammar.call(), governor)?;
    let xml =
        slot.is_some_and(|(codec, _)| matches!(codec, CodecKind::XmlParams | CodecKind::KvPairs));
    let mut risky: Vec<&[u8]> = slot
        .map_or(&[][..], |(_, delimiters)| grammar.delimiters(delimiters))
        .iter()
        .map(ScratchVec::as_slice)
        .collect();
    if slot.is_some_and(|(codec, _)| codec == CodecKind::KvPairs) {
        risky.extend([
            b"<arg_key>".as_slice(),
            b"</arg_key>",
            b"<arg_value>",
            b"</arg_value>",
        ]);
    } else if xml {
        risky.extend(PARAMETER_MARKUP);
    }
    let decoded = EditOperator::StringifiedArguments.rule().id;
    let spelling = Spelling {
        source,
        stringified: call
            .repairs
            .evidence
            .iter()
            .any(|evidence| evidence.rule == decoded),
        xml,
        risky,
    };
    let mut own = Vec::new();
    if spelling.diff(&call.arguments, arguments, &mut own, governor)? {
        splices.extend(
            own.into_iter()
                .map(|(range, text)| (range, SpliceText::Prepared(text))),
        );
        return Ok(());
    }
    if let Some(span) = call.arguments.span.clone().filter(|_| xml) {
        // The whole body, as one JSON object.
        let text = guard_json(arguments.to_json(), &spelling.risky);
        splices.push((span, SpliceText::Prepared(text)));
        return Ok(());
    }
    if printer.holds(grammar.call(), governor)?.next {
        // A call that shares its production instance with other calls is
        // never reprinted whole (its span holds the group's framing): its
        // arguments value is replaced in place.
        let span = call
            .arguments
            .span
            .clone()
            .ok_or_else(|| EmitError::Unrepresentable {
                text: arguments.to_json(),
            })?;
        let json = guard_json(arguments.to_json(), &spelling.risky);
        let text = if spelling.stringified {
            json::quote(&json)
        } else {
            json
        };
        splices.push((span, SpliceText::Prepared(text)));
        return Ok(());
    }
    // Reprint the whole call in its dialect's canonical form.
    let printer = Printer::new(grammar, governor);
    let reprinted = ToolCall {
        arguments: arguments.clone(),
        ..call.clone()
    };
    let id = call.source_id.as_deref().unwrap_or(&call.id);
    let prepared = prepare(
        &printer,
        &reprinted,
        Some(id),
        None,
        &call.dialect,
        governor,
    )?;
    let rendered = canonical_scratch(grammar, Root::Call, prepared.values(), &[], governor)?;
    rendered.check_utf8(governor)?;
    splices.push((call.span.clone(), SpliceText::Canonical(rendered)));
    Ok(())
}

/// Whether two values are identical: equal, in order, with equal number
/// spellings (spans ignored).
fn identical(left: &Node, right: &Node) -> bool {
    match (&left.value, &right.value) {
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| identical(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().zip(right).all(|(left, right)| {
                    left.key == right.key && identical(&left.value, &right.value)
                })
        }
        (left, right) => left == right,
    }
}

impl Spelling<'_> {
    /// Splice `old` into `new` value by value; `false` when `old` cannot be
    /// replaced in place (its parent must be).
    fn diff(
        &self,
        old: &Node,
        new: &Node,
        splices: &mut Vec<(Range<usize>, String)>,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        governor.charge(Resource::Work, 1)?;
        if identical(old, new) {
            return Ok(true);
        }
        let pairs: Option<Vec<(&Node, &Node)>> = match (&old.value, &new.value) {
            (Value::Object(left), Value::Object(right))
                if left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| left.key == right.key) =>
            {
                Some(
                    left.iter()
                        .zip(right)
                        .map(|(left, right)| (&left.value, &right.value))
                        .collect(),
                )
            }
            (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
                Some(left.iter().zip(right).collect())
            }
            _ => None,
        };
        if let Some(pairs) = pairs {
            let mut children = Vec::new();
            let mut whole = true;
            for (left, right) in pairs {
                if !self.diff(left, right, &mut children, governor)? {
                    whole = false;
                    break;
                }
            }
            if whole {
                splices.extend(children);
                return Ok(true);
            }
        }
        let splice = self.replacement(old, new, governor)?;
        let found = splice.is_some();
        splices.extend(splice);
        Ok(found)
    }

    /// The in-place replacement of `old` by `new`, when `old`'s source
    /// spelling is known and `new` can be written there.
    fn replacement(
        &self,
        old: &Node,
        new: &Node,
        governor: &mut Governor,
    ) -> Result<Option<(Range<usize>, String)>, StopReason> {
        let Some(span) = old.span.clone() else {
            return Ok(None);
        };
        let Some(text) = self
            .source
            .get(span.clone())
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
        else {
            return Ok(None);
        };
        governor.charge(Resource::Work, size(text.len()))?;
        let json = || guard_json(new.to_json(), &self.risky);
        if self.stringified {
            // Escaped JSON inside the arguments string.
            let decoded = parse_spelling(&format!("\"{text}\""), governor)?;
            let spelled = if let Some(inner) = decoded.as_ref().and_then(Node::as_str) {
                parse_spelling(inner, governor)?
            } else {
                None
            };
            if let Some(parsed) = spelled
                && parsed.equivalent_governed(old, governor)?
            {
                let quoted = json::quote(&json());
                return Ok(Some((span, quoted[1..quoted.len() - 1].to_owned())));
            }
            return Ok(None);
        }
        if let Some(parsed) = parse_spelling(text, governor)?
            && parsed.equivalent_governed(old, governor)?
        {
            return Ok(Some((span, json())));
        }
        match (&old.value, &new.value) {
            (Value::String(before), Value::String(after)) if before == text => {
                // A verbatim string: framing newlines stay outside its span.
                let framed = |at: Option<&u8>, edge: Option<char>| {
                    at == Some(&b'\n') || !matches!(edge, Some('\n' | '\r'))
                };
                let fits = !contains_any(after.as_bytes(), &self.risky)
                    && framed(
                        span.start.checked_sub(1).and_then(|at| self.source.get(at)),
                        after.chars().next(),
                    )
                    && framed(self.source.get(span.end), after.chars().next_back());
                Ok(fits.then(|| (span, after.clone())))
            }
            (Value::Object(_), _) if self.xml && text.trim_start().starts_with("<parameter=") => {
                // Nested parameter elements of an object parameter: inline
                // JSON reads to the same tree.
                Ok(Some((span, format!("\n{}\n", json()))))
            }
            _ => Ok(None),
        }
    }
}

// Invalid source spelling may use the existing replacement path; native stops
// must leave that path immediately rather than looking like a spelling mismatch.
fn parse_spelling(text: &str, governor: &mut Governor) -> Result<Option<Node>, StopReason> {
    match json::parse_with(text, governor) {
        Ok(node) => Ok(Some(node)),
        Err(json::ParseFailure::Syntax(_)) => Ok(None),
        Err(json::ParseFailure::Resource(reason)) => Err(reason),
    }
}
