// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The tool-call recovery operation, `tool_call.recover.v1` (R4, R5, R7.1,
//! C2, C3, C6).
//!
//! [`ToolSet::recover`] (and [`ToolSet::recover_governed`]) reads one model
//! turn — the raw output bytes, which need not be UTF-8 — against a tool set
//! under a [`ToolRecoveryRequest`], and returns an
//! [`AssistResult<ToolCalls>`]: the calls, the rejected call attempts, the
//! content and reasoning ranges, how the turn finished ([`Finish`]), the tool
//! choice check, and an itemized receipt.
//!
//! # Steps
//!
//! 1. The output bytes are retained in the receipt
//!    ([`Receipt::retain_source_bytes`]) before anything else is charged. If
//!    that charge fails, it is the reported exhaustion; the receipt still
//!    names the output's digest.
//! 2. The request is checked: an effects override or a forced tool choice
//!    naming a tool the set does not declare is
//!    [`StopReason::InvalidInput`].
//! 3. Every maximal run of bytes that are not well-formed UTF-8 is recorded
//!    as a `tool_call.input.invalid-utf8` data loss with its exact byte
//!    range. The bytes are never replaced: content, reasoning, and rejected
//!    attempts are byte ranges of the retained output.
//! 4. The [`Recognizer`] segments the turn over the request's dialects and
//!    reasoning policy and [`resolve_forest`] resolves every call under the
//!    request's policies (see their documentation for every rule).
//! 5. Every whole-message reading is built: the product of the recognized
//!    turn's competing segmentations, all charged as
//!    [`Resource::Candidates`] (one per reading) before the first is built.
//!    The segmentations of each contested span that recover the same calls
//!    are folded into one first (the selection described next, made per
//!    span), so such spans add readings instead of multiplying them.
//!    Exceeding the budget is a [`StopReason::BudgetExceeded`] naming
//!    candidates, never a truncated list. Readings that recover the same
//!    calls (each call's span, tool, arguments, outcome, repairs, and
//!    alternatives, the calls embedded in reasoning, and every reasoning and
//!    encoding ledger) and differ only in which dialect read each call and
//!    how the bytes outside every call divide into rejected attempts and
//!    content are one reading. This is the case where dialects sharing an
//!    opener (Qwen3-Coder XML and Hermes both open with `<tool_call>`) each
//!    reject an attempt no dialect reads, or several dialects read the same
//!    call; each call's [`ToolCall::candidate_dialects`] names every dialect
//!    that read it.
//!    The reading kept is the one whose rejected attempts' dialects read the
//!    most bytes before failing: the body is the evidence, so a `<function=`
//!    body keeps the Qwen3-Coder XML attempt and a JSON body the Hermes one.
//!    Each kept attempt's [`RejectedCall::candidate_dialects`] names every
//!    dialect that attempted a call at its opener. Readings that recover
//!    different calls but whose receipt derivations depend on exactly the
//!    same evidence, losses, inferences, and validations are told apart by
//!    the dialect that read each call: a passing `dialect=ID` validation of
//!    the call (validator [`TOOL_CALL_VALIDATOR`]). Readings still
//!    indistinguishable after that are one reading.
//!    In each reading, the call markup of the content that no dialect of
//!    the request reads ([`detect_unparsed`]: a family not built in yet,
//!    or a built-in dialect the allow-list leaves out) is reported as a
//!    rejected attempt with [`Rejection::UnsupportedDialect`] and a
//!    `tool_call.markup.unparsed-dialect` evidence entry; its bytes stay
//!    content, so such markup is never silent content and the finish is
//!    [`Finish::Unresolved`].
//! 6. Each reading's calls receive ids ([`IdPolicy`]), the tool choice is
//!    enforced ([`ToolChoice`]), and every unmodeled keyword of an adapter
//!    tool's schema on the call's path is reported ([`KeywordUse`]).
//! 7. When the request names an [`EmitTarget`], each reading's calls are
//!    printed for it ([`ToolCalls::emitted`]; see [`emit`]),
//!    charged to the same governor.
//!
//! # Outcome
//!
//! - `Unresolved` only for exhaustion, cancellation, an invalid request or
//!   configuration (including an [`IdGenerator`] returning one id twice),
//!   [`Effort::Exact`] meeting a message whose segmentation itself needs a
//!   repair (a reasoning block closed at a call opener: the receipt stops
//!   with [`StopReason::NoCandidate`] and a failed `effort=exact`
//!   validation naming the rule, and carries no repair),
//!   or [`AmbiguityPolicy::Reject`] meeting any ambiguity: competing
//!   whole-message readings or any per-call `Ambiguous` call. The receipt
//!   then stops with [`StopReason::NoCandidate`], records a failed
//!   `tool_call.ambiguity` validation, and keeps every alternative in
//!   [`Receipt::alternatives`].
//! - `Ambiguous` only for competing whole-message readings that recover
//!   different calls and that evidence distinguishes, in
//!   [`Receipt::alternatives`] order: the receipt's `distinguishing`
//!   annotation is never empty for a ranked-adjacent pair.
//! - `BestGuess` when the selected reading records any guess inference or
//!   any data loss (in a call, in a rejected attempt, in reasoning, or in
//!   the input's encoding).
//! - `Resolved` otherwise.
//!
//! Under [`Effort::Exact`] only canonical parsing is applied (see
//! [`ToolRecoveryRequest::effort`]): a call that needs a repair is
//! `Unresolved` with [`UnresolvedReason::RepairNotPermitted`] naming it.
//!
//! A call written inside a closed reasoning block stays reasoning text: it
//! is part of a [`ToolCalls::reasoning`] range, never a recovered call. It
//! is offered only as a guess-flagged alternative in
//! [`ToolCalls::embedded`] (`tool_call.reasoning.embedded-call`), itemized
//! on that [`ToolCall`] like a per-call alternative: it is never
//! `Resolved`, never selected by [`ToolCalls::usable`] or
//! [`ToolCalls::usable_accepting_guesses`], receives no id, and does not
//! affect the finish, the tool choice check, or the operation's outcome.
//!
//! A rejected attempt or a call to an undeclared tool does not make the
//! operation `Unresolved`: it is a recorded fact (a [`RejectedCall`], or a
//! [`ToolCall`] whose own [`CallOutcome`] is `Unresolved`), and the turn's
//! [`Finish`] is [`Finish::Unresolved`]. Per-call outcomes come from
//! [`CallOutcome::classify`]; [`ToolCalls::usable`] and
//! [`ToolCalls::usable_accepting_guesses`] select what may run, and
//! [`ToolCalls::verdict`] is the one usability verdict ([`Verdict`]). The
//! operation's outcome is the certainty of the reading of the turn, as in
//! every operation: a turn with nothing to run is still `Resolved` when it
//! has one reading.
//!
//! # Receipt
//!
//! The receipt's operation is [`RECOVER_OPERATION`]. Its evidence, losses, and
//! inferences are the selected reading's, in output order: encoding losses,
//! then each segment's (a reasoning block's edits, a call's repairs
//! including its id inference, a rejected attempt's repairs). Every
//! per-call repair also stays on its [`ToolCall`] (C2). Its validations are
//! the tool choice check, each call's schema validation, each call's
//! passing `dialect=ID` attestation when it tells apart whole-message
//! readings whose ledgers are otherwise identical, one validation per
//! reported unmodeled keyword, and, under [`AmbiguityPolicy::Reject`], the
//! `tool_call.ambiguity` check.
//!
//! # Resources
//!
//! Everything is charged to the governor with portable amounts: retention as
//! [`Receipt::retain_source_bytes`] documents, one `Work` per output byte
//! for the encoding scan, recognition and resolution as documented on
//! them, and `Work`, `ScratchBytes`, and `EvidenceEntries` for every value,
//! id, check, and ledger entry this operation builds. Every charge is a
//! function of the output, the tool set, and the request alone. Each call's
//! schema validation ([`crate::schema::Schema::validate_with`]) charges its
//! working set to the same governor before allocating it.
//! [`ToolRecoveryRequest::sized`] sizes a budget for an output: its length
//! and its call openers.
use super::StreamEvent;
use super::adapter::{consulted_by_repair, reshaping};
use super::emit::{self, Emission, EmitTarget};
use super::recognizer::{ContinuationFrontier, edit_entries, escaped};
use super::resolve::loss_kind;
use super::stream::{EventCursor, PreparedEvent};
use super::{
    CallLedger, CallLoss, CallOutcome, CallReading, DialectId, DialectSet, Effects, FromNode,
    FromNodeError, GenerationStop, Policies, PolicySelection, ReasoningPolicy, ReasoningSegment,
    Recognizer, Rejection, RepairPolicy, ResolvedCall, ResolvedPiece, ResolvedSegment,
    ResolvedTurn, ToolSet, ToolSource, UnresolvedReason, detect_unparsed, resolve_forest, rules,
};
use crate::contract::saturating_size as size;
use crate::contract::{
    AlternativeReceipt, AssistResult, Budget, Effort, Evidence, Governor, Inference, InferenceKind,
    Loss, Outcome, Receipt, Resource, ScratchLease, ScratchVec, SourceDocument, SourceSpan,
    StopReason, Validation, invalid_input, push_scratch, reserve_failure,
};
use crate::digest::{Digest, DigestBuilder};
use crate::grammar::CallEnd;
use crate::grammar::cost::{
    K_EVENT, RECOVER_EVIDENCE_BYTES_PER_ENTRY, RECOVER_OPENER_CANDIDATE_DIVISOR,
    RECOVER_SCRATCH_PER_BYTE, RECOVER_WORK_PER_BYTE,
};
use crate::grammar::edit::LossKind;
use crate::json::{Node, Value, pointer_segment};
use crate::provenance::{Derivation, Forest, NodeId, ProvenanceNode, permute};
use crate::wire::{
    self, WireValue, array, byte_range, exact_bytes, inferences, integer, null, object, string,
};
use core::fmt;
use core::ops::Range;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// The operation id every recovery receipt records.
pub const RECOVER_OPERATION: &str = "tool_call.recover.v1";

/// The validator id of every validation this operation performs itself
/// (tool choice, unmodeled keywords, ambiguity).
pub const TOOL_CALL_VALIDATOR: &str = "purrassist.tool_call.v1";

/// Scratch bytes charged per ledger entry or record built.
const ENTRY_BYTES: u64 = 256;

/// The validation-log key of a message-level validation.
const MESSAGE: (usize, usize) = (usize::MAX, usize::MAX);

/// Produces call ids for [`IdPolicy::Generator`].
///
/// Implementations must be deterministic for the result to be (C3). Ids
/// must be distinct within one result: a duplicate makes the recovery
/// `Unresolved` with [`StopReason::InvalidInput`]; it is never renumbered.
///
/// ```
/// use purrassist_core::tool_call::IdGenerator;
/// struct Tagged;
/// impl IdGenerator for Tagged {
///     fn id(&self, call_index: usize) -> String {
///         format!("toolu_{call_index:04}")
///     }
/// }
/// assert_eq!(Tagged.id(7), "toolu_0007");
/// ```
pub trait IdGenerator: Send + Sync {
    /// The id of the call at `call_index` (zero-based, in output order)
    /// of one result.
    fn id(&self, call_index: usize) -> String;
}

/// The text that starts every [`IdPolicy::Prefix`] id unless the caller
/// names another prefix, and every id [`IdPolicy::Turn`] assigns.
pub const DEFAULT_ID_PREFIX: &str = "call_";

/// The number of hexadecimal digits of the turn digest in an
/// [`IdPolicy::Turn`] id: 12 digits, 48 bits.
pub const TURN_TAG_DIGITS: usize = 12;

/// The text before the call index of every id [`IdPolicy::Turn`] assigns.
///
/// For the turn whose output bytes have digest `turn` it is `call_{tag}_`,
/// where `tag` is the first [`TURN_TAG_DIGITS`] lowercase hexadecimal
/// digits of the SHA-256 digest of the output (the digest the receipt names
/// as its source, and [`ToolCalls::turn`]). Infallible.
///
/// ```
/// use purrassist_core::digest::Digest;
/// use purrassist_core::tool_call::turn_id_prefix;
/// let prefix = turn_id_prefix(&Digest::of(b"abc"));
/// // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223...
/// assert_eq!(prefix, "call_ba7816bf8f01_");
/// ```
#[must_use]
pub fn turn_id_prefix(turn: &Digest) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut prefix = String::with_capacity(DEFAULT_ID_PREFIX.len() + TURN_TAG_DIGITS + 1);
    prefix.push_str(DEFAULT_ID_PREFIX);
    for byte in &turn.as_bytes()[..TURN_TAG_DIGITS / 2] {
        prefix.push(char::from(HEX[usize::from(byte >> 4)]));
        prefix.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    prefix.push('_');
    prefix
}

/// How recovered calls receive ids (R5.6).
///
/// Ids are caller-scoped: within one result they are always distinct, and
/// every policy gives a way to keep them from colliding across the rounds
/// of a conversation.
///
/// - `Turn` (the default; zero configuration): a call keeps the id the
///   model wrote in its markup (an `OpenAI` `tool_calls[].id`), reported as
///   both [`ToolCall::id`] and [`ToolCall::source_id`]. Every other call is
///   numbered `call_{tag}_{i}` ([`turn_id_prefix`]), where `tag` is the first
///   12 hexadecimal digits of the SHA-256 digest of the turn's output bytes
///   and `i` is the call's index. If that id is itself taken (by an id
///   assigned earlier or carried by any call of the result), `_{k}` is
///   appended for the least `k = 1, 2, ...` that is free. A call whose
///   dialect provides for an id ([`CompiledGrammar::call_carries_ids`](crate::grammar::CompiledGrammar::call_carries_ids))
///   but which has none, or whose id an earlier call already holds, is
///   recorded as a derived `tool_call.id.dedup` inference naming the
///   assigned id; a call of a dialect whose markup carries no id
///   (DeepSeek-V3.1, DeepSeek-V3, Hermes, Llama 3 JSON, Qwen3-Coder XML)
///   is numbered without one, as under `Prefix` (Hermes and Llama 3 JSON
///   keep an id a model adds, as a source id).
///   [`ToolCalls::next_id_start`] is the number of calls.
///
///   Different turns get disjoint numbered ids without any caller state.
///   The same output bytes always get the same ids (C3): recovering one
///   turn twice is the same turn. Two different outputs share a tag with
///   probability `2^-48`; among `n` distinct turns of one conversation the
///   chance that any two share a tag is at most `n(n-1)/2^49` (the birthday
///   bound): about `1.8e-11` for 100 turns, `1.8e-7` for 10,000, and
///   `1.8e-5` for 100,000. A conversation that may repeat an identical turn
///   and must still number it apart chains `Prefix` through
///   `next_id_start`.
/// - `Prefix { prefix, start }`: call `i` is `{prefix}{start + i}`, whatever
///   id the output carries, and [`ToolCalls::next_id_start`] is
///   `start + calls`; passing it back as the next round's `start` makes ids
///   of successive rounds never collide. A counter that would overflow
///   `u64` is an invalid request.
/// - `Generator(generator)`: call `i` is `generator.id(i)`; a duplicate is an
///   invalid request ([`StopReason::InvalidInput`]).
/// - `Source`: the id the call carries in the output. A missing id becomes
///   `call#{i}` and an id an earlier call already holds becomes
///   `{source}#{i}`, where `i` is the call's index; if that is itself taken
///   (by an id assigned earlier or carried by any call of the result),
///   `#{k}` is appended for the least `k = 1, 2, ...` that is free. Each
///   such id is recorded as a derived `tool_call.id.dedup` inference.
///
/// For `Generator` and `Source`, [`ToolCalls::next_id_start`] is the number
/// of calls.
///
/// ```
/// use purrassist_core::tool_call::{Effects, IdPolicy, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::{Budget, Outcome};
/// assert!(matches!(IdPolicy::default(), IdPolicy::Turn));
/// let tools = ToolSet::compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)], Budget::default()).unwrap();
/// let ids = |output: &[u8]| {
///     let Outcome::Resolved(calls) = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in()).outcome else { unreachable!() };
///     calls.calls.into_iter().map(|call| call.id).collect::<Vec<_>>()
/// };
/// let first = ids(br#"<tool_call>{"name": "ls", "arguments": {}}</tool_call>"#);
/// let second = ids(br#"<tool_call>{"name": "ls", "arguments": {} }</tool_call>"#);
/// assert!(first[0].starts_with("call_") && first[0].ends_with("_0"));
/// assert_ne!(first, second);
/// assert_eq!(first, ids(br#"<tool_call>{"name": "ls", "arguments": {}}</tool_call>"#));
/// // The model's own id is kept.
/// let openai = br#"{"id": "call_abc", "type": "function", "function": {"name": "ls", "arguments": "{}"}}"#;
/// assert_eq!(ids(openai), ["call_abc"]);
/// ```
#[derive(Clone, Default)]
pub enum IdPolicy {
    /// The model's ids, and ids numbered from the turn's digest for every
    /// call without one of its own.
    #[default]
    Turn,
    /// Sequential ids with a fixed prefix.
    Prefix {
        /// The text before the number.
        prefix: String,
        /// The number of the first call.
        start: u64,
    },
    /// Ids from a caller generator.
    Generator(Arc<dyn IdGenerator>),
    /// The ids the output carries, deduplicated.
    Source,
}

impl fmt::Debug for IdPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Turn => formatter.write_str("Turn"),
            Self::Prefix { prefix, start } => formatter
                .debug_struct("Prefix")
                .field("prefix", prefix)
                .field("start", start)
                .finish(),
            Self::Generator(_) => formatter.write_str("Generator(..)"),
            Self::Source => formatter.write_str("Source"),
        }
    }
}

/// Which calls the request allows (D14).
///
/// Enforced as a validation `{validator: "purrassist.tool_call.v1",
/// constraint: "tool_choice=…"}` (see [`ToolChoice::constraint`]):
///
/// - `Auto`: anything.
/// - `None`: no call. Calls are still parsed; each that is not already
///   `Unresolved` becomes `Unresolved` with
///   [`UnresolvedReason::ToolChoiceNone`].
/// - `Required`: at least one call (a recognized call of any outcome).
/// - `Forced(name)`: at least one call, and only calls to the tool `name`
///   (its canonical name or an alias; undeclared is a request error). Each
///   call to anything else that is not already `Unresolved` becomes
///   `Unresolved` with [`UnresolvedReason::ToolChoiceForced`].
/// - `ForcedStrict(strict)`: the `Forced` checks, plus the options of
///   [`StrictForced`]: exactly one call (each call after the first becomes
///   `Unresolved` with [`UnresolvedReason::ToolChoiceCardinality`]; every
///   rejected call attempt, of any reason, counts against it too), no
///   content outside the call, and optionally no reasoning. `Forced(name)`
///   is the lenient form: a `ForcedStrict` with every option lenient checks
///   exactly what `Forced` checks and renders the same constraint.
///
/// A failed check makes the turn's [`Finish`] `Unresolved`.
///
/// ```
/// use purrassist_core::tool_call::{StrictForced, ToolChoice};
/// assert_eq!(ToolChoice::default(), ToolChoice::Auto);
/// assert_eq!(ToolChoice::Forced("read".into()).constraint(), "tool_choice=forced:read");
/// let strict = ToolChoice::ForcedStrict(StrictForced::exactly_one("read").allow_content(false));
/// assert_eq!(strict.constraint(), "tool_choice=forced[exactly-one,no-content]:read");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ToolChoice {
    /// Any number of calls, or none.
    #[default]
    Auto,
    /// No call.
    None,
    /// At least one call.
    Required,
    /// Only calls to this tool, at least one.
    Forced(String),
    /// [`Self::Forced`] plus the cardinality, content and reasoning options
    /// of [`StrictForced`].
    ForcedStrict(StrictForced),
}

/// A forced tool choice with options: the strict form of
/// [`ToolChoice::Forced`].
///
/// It enforces everything `Forced` does (at least one call, every call to
/// `name`, an alias counting as its canonical name; an undeclared `name` is
/// a request error, `invalid_input`) and then the three options:
///
/// - `exactly_one`: the turn holds exactly one call. "Calls" are the
///   entries of [`ToolCalls::calls`] (recognized calls of any outcome, as
///   [`ToolChoice::Required`] counts them); each call after the first
///   becomes `Unresolved` with [`UnresolvedReason::ToolChoiceCardinality`]
///   unless it already was unresolved, and every entry of
///   [`ToolCalls::rejected`] (a call attempt of any [`Rejection`] reason,
///   an unsupported dialect's included) is one more call beyond the one.
///   The check states the count of both in one diagnostic. Calls embedded in reasoning are not
///   counted (see [`ToolCalls::embedded`]); `allow_reasoning` governs them.
/// - `allow_content: false`: no content range holds a byte that is not
///   ASCII whitespace (`u8::is_ascii_whitespace`: space, tab, line feed,
///   carriage return, form feed), and no rejected call attempt: the bytes
///   of an attempt that is not a call are outside every call. Framing
///   whitespace between calls passes; non-ASCII whitespace such as U+00A0
///   is content, and so are tool results the model wrote.
/// - `allow_reasoning: false`: no reasoning block at all. Under
///   [`ReasoningPolicy::AsContent`] reasoning is never recognized, so the
///   combination is a request error (`invalid_input`) rather than a check
///   that passes vacuously.
///
/// A failed option is a diagnostic on the tool choice check; the check then
/// fails and the turn follows the whole-turn contract of
/// [`ToolChoice`] (`Finish::Unresolved`, nothing emitted). The constraint
/// string lists the active options, the name last:
/// `tool_choice=forced[exactly-one,no-content,no-reasoning]:NAME`. With no
/// option active it is exactly `tool_choice=forced:NAME`.
///
/// ```
/// use purrassist_core::{Budget, Outcome};
/// use purrassist_core::tool_call::{
///     Effects, Finish, Param, StrictForced, ToolChoice, ToolDefinition, ToolRecoveryRequest, ToolSet,
/// };
/// let tools = ToolSet::compile(
///     [ToolDefinition::new("ls").effects(Effects::ReadOnly).param("depth", Param::integer())],
///     Budget::default(),
/// ).unwrap();
/// let strict = StrictForced::exactly_one("ls");
/// assert_eq!(
///     ToolChoice::ForcedStrict(strict.clone()).constraint(),
///     "tool_choice=forced[exactly-one,no-content]:ls",
/// );
/// let request = ToolRecoveryRequest::default().tool_choice(ToolChoice::ForcedStrict(strict));
/// let call = r#"<tool_call>{"name": "ls", "arguments": {}}</tool_call>"#;
/// // Two calls: the second is unresolved and the turn fails.
/// let twice = format!("{call}\n{call}");
/// let Outcome::Resolved(calls) = tools.recover(twice.as_bytes(), &request, purrassist_core::tool_call::DialectSet::built_in()).outcome else { unreachable!() };
/// assert!(!calls.tool_choice.passed);
/// assert_eq!(calls.finish, Finish::Unresolved);
/// // One call passes.
/// let Outcome::Resolved(calls) = tools.recover(call.as_bytes(), &request, purrassist_core::tool_call::DialectSet::built_in()).outcome else { unreachable!() };
/// assert!(calls.tool_choice.passed);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrictForced {
    /// The tool the request forces (canonical name or alias).
    pub name: String,
    /// Exactly one call, not just at least one.
    pub exactly_one: bool,
    /// Whether content other than ASCII whitespace may surround the call.
    pub allow_content: bool,
    /// Whether reasoning blocks may appear.
    pub allow_reasoning: bool,
}

impl StrictForced {
    /// Exactly one call to `name`, with no content outside it and reasoning
    /// allowed: `exactly_one` set, `allow_content` unset, `allow_reasoning`
    /// set.
    ///
    /// ```
    /// use purrassist_core::tool_call::StrictForced;
    /// let strict = StrictForced::exactly_one("read");
    /// assert!(strict.exactly_one && !strict.allow_content && strict.allow_reasoning);
    /// ```
    #[must_use]
    pub fn exactly_one(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            exactly_one: true,
            allow_content: false,
            allow_reasoning: true,
        }
    }

    /// Set whether content other than ASCII whitespace is allowed.
    ///
    /// ```
    /// use purrassist_core::tool_call::StrictForced;
    /// assert!(StrictForced::exactly_one("read").allow_content(true).allow_content);
    /// ```
    #[must_use]
    pub const fn allow_content(mut self, allow: bool) -> Self {
        self.allow_content = allow;
        self
    }

    /// Set whether reasoning blocks are allowed.
    ///
    /// ```
    /// use purrassist_core::tool_call::StrictForced;
    /// assert!(!StrictForced::exactly_one("read").allow_reasoning(false).allow_reasoning);
    /// ```
    #[must_use]
    pub const fn allow_reasoning(mut self, allow: bool) -> Self {
        self.allow_reasoning = allow;
        self
    }

    /// Set whether more than one call is allowed (`false` is exactly one).
    ///
    /// ```
    /// use purrassist_core::tool_call::StrictForced;
    /// assert!(!StrictForced::exactly_one("read").with_exactly_one(false).exactly_one);
    /// ```
    #[must_use]
    pub const fn with_exactly_one(mut self, exactly_one: bool) -> Self {
        self.exactly_one = exactly_one;
        self
    }
}

/// A tool choice that is not one of the accepted wire forms.
///
/// ```
/// use purrassist_core::{json, tool_call::ToolChoice};
/// let error = ToolChoice::from_openai(&json::parse(r#""sometimes""#).unwrap()).unwrap_err();
/// assert!(error.to_string().contains("sometimes"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolChoiceError {
    /// What is wrong with the value.
    pub reason: String,
}

impl fmt::Display for ToolChoiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid tool choice: {}", self.reason)
    }
}

impl std::error::Error for ToolChoiceError {}

fn choice_error(reason: impl Into<String>) -> ToolChoiceError {
    ToolChoiceError {
        reason: reason.into(),
    }
}

/// The members of a tool-choice object, checked against the allowed names.
fn choice_members<'n>(
    node: &'n Node,
    allowed: &[&str],
) -> Result<&'n [crate::json::Member], ToolChoiceError> {
    let Value::Object(members) = &node.value else {
        return Err(choice_error("expected a string or an object"));
    };
    for (position, member) in members.iter().enumerate() {
        if !allowed.contains(&member.key.as_str()) {
            return Err(choice_error(format!(
                "member `{}` is not accepted (accepted: {})",
                member.key,
                allowed.join(", ")
            )));
        }
        if members[..position]
            .iter()
            .any(|other| other.key == member.key)
        {
            return Err(choice_error(format!("member `{}` repeats", member.key)));
        }
    }
    Ok(members)
}

/// A required string member.
fn choice_string<'n>(node: &'n Node, key: &str) -> Result<&'n str, ToolChoiceError> {
    node.get(key)
        .and_then(Node::as_str)
        .ok_or_else(|| choice_error(format!("`{key}` must be a string")))
}

impl ToolChoice {
    /// Read an `OpenAI` chat-completions `tool_choice`: `"auto"`, `"none"`,
    /// `"required"`, or `{"type":"function","function":{"name":NAME}}`.
    ///
    /// # Errors
    /// [`ToolChoiceError`] for any other string, a non-string non-object,
    /// an object with a missing, repeated, or unknown member, a `type` other
    /// than `"function"`, or a `function` that is not an object holding
    /// exactly a string `name`.
    ///
    /// ```
    /// use purrassist_core::{json, tool_call::ToolChoice};
    /// let parse = |text: &str| ToolChoice::from_openai(&json::parse(text).unwrap());
    /// assert_eq!(parse(r#""required""#), Ok(ToolChoice::Required));
    /// assert_eq!(parse(r#"{"type":"function","function":{"name":"read"}}"#), Ok(ToolChoice::Forced("read".into())));
    /// assert!(parse(r#"{"type":"tool","name":"read"}"#).is_err());
    /// ```
    pub fn from_openai(node: &Node) -> Result<Self, ToolChoiceError> {
        if let Some(text) = node.as_str() {
            return match text {
                "auto" => Ok(Self::Auto),
                "none" => Ok(Self::None),
                "required" => Ok(Self::Required),
                other => Err(choice_error(format!(
                    "`{other}` is not \"auto\", \"none\" or \"required\""
                ))),
            };
        }
        choice_members(node, &["type", "function"])?;
        if choice_string(node, "type")? != "function" {
            return Err(choice_error("`type` must be \"function\""));
        }
        let function = node
            .get("function")
            .ok_or_else(|| choice_error("`function` is missing"))?;
        choice_members(function, &["name"])
            .map_err(|error| choice_error(format!("in `function`: {}", error.reason)))?;
        Ok(Self::Forced(choice_string(function, "name")?.to_owned()))
    }

    /// Read an Anthropic Messages `tool_choice`: `{"type":"auto"}`,
    /// `{"type":"any"}` (read as `Required`), `{"type":"none"}`, or
    /// `{"type":"tool","name":NAME}`. For `"tool"`, `disable_parallel_tool_use`
    /// is read: `true` is [`ToolChoice::ForcedStrict`] with exactly one call
    /// and content and reasoning allowed (`StrictForced { exactly_one: true,
    /// allow_content: true, allow_reasoning: true }`), `false` is
    /// [`ToolChoice::Forced`].
    ///
    /// # Errors
    /// [`ToolChoiceError`] for a non-object, an unknown `type`, a `name`
    /// missing for `"tool"` or present otherwise, a `disable_parallel_tool_use`
    /// that is not a boolean, or any other member (including
    /// `disable_parallel_tool_use` on a type other than `"tool"`, which this
    /// choice model does not represent and so never silently drops).
    ///
    /// ```
    /// use purrassist_core::{json, tool_call::ToolChoice};
    /// let parse = |text: &str| ToolChoice::from_anthropic(&json::parse(text).unwrap());
    /// assert_eq!(parse(r#"{"type":"any"}"#), Ok(ToolChoice::Required));
    /// assert_eq!(parse(r#"{"type":"tool","name":"read"}"#), Ok(ToolChoice::Forced("read".into())));
    /// let strict = parse(r#"{"type":"tool","name":"read","disable_parallel_tool_use":true}"#).unwrap();
    /// assert_eq!(strict.constraint(), "tool_choice=forced[exactly-one]:read");
    /// assert_eq!(
    ///     parse(r#"{"type":"tool","name":"read","disable_parallel_tool_use":false}"#),
    ///     Ok(ToolChoice::Forced("read".into())),
    /// );
    /// assert!(parse(r#"{"type":"auto","disable_parallel_tool_use":true}"#).is_err());
    /// ```
    pub fn from_anthropic(node: &Node) -> Result<Self, ToolChoiceError> {
        if node.as_str().is_some() {
            return Err(choice_error("expected an object with a `type` member"));
        }
        let kind = {
            choice_members(node, &["type", "name", "disable_parallel_tool_use"])?;
            choice_string(node, "type")?
        };
        let named = node.get("name").is_some();
        let parallel = node.get("disable_parallel_tool_use");
        if parallel.is_some() && kind != "tool" {
            return Err(choice_error(format!(
                "`disable_parallel_tool_use` is accepted only for type \"tool\", not \"{kind}\""
            )));
        }
        match (kind, named) {
            ("tool", true) => {
                let name = choice_string(node, "name")?.to_owned();
                let disabled = match parallel {
                    None => false,
                    Some(flag) => match flag.value {
                        Value::Bool(flag) => flag,
                        _ => {
                            return Err(choice_error(
                                "`disable_parallel_tool_use` must be a boolean",
                            ));
                        }
                    },
                };
                Ok(if disabled {
                    Self::ForcedStrict(StrictForced {
                        name,
                        exactly_one: true,
                        allow_content: true,
                        allow_reasoning: true,
                    })
                } else {
                    Self::Forced(name)
                })
            }
            ("tool", false) => Err(choice_error("`name` is required for type \"tool\"")),
            ("auto" | "any" | "none", true) => Err(choice_error(format!(
                "`name` is accepted only for type \"tool\", not \"{kind}\""
            ))),
            ("auto", false) => Ok(Self::Auto),
            ("any", false) => Ok(Self::Required),
            ("none", false) => Ok(Self::None),
            (other, _) => Err(choice_error(format!(
                "`type` \"{other}\" is not \"auto\", \"any\", \"none\" or \"tool\""
            ))),
        }
    }

    /// The validation constraint: `tool_choice=auto`, `tool_choice=none`,
    /// `tool_choice=required`, or `tool_choice=forced:NAME`. A
    /// [`Self::ForcedStrict`] lists its active options in brackets before
    /// the name, which comes last so a name holding `:`, `;` or `,` stays
    /// unambiguous: `tool_choice=forced[exactly-one,no-content,no-reasoning]:NAME`.
    /// With every option lenient it is `tool_choice=forced:NAME`.
    ///
    /// See [`ToolChoice`] for an example.
    #[must_use]
    pub fn constraint(&self) -> String {
        match self {
            Self::Auto => "tool_choice=auto".into(),
            Self::None => "tool_choice=none".into(),
            Self::Required => "tool_choice=required".into(),
            Self::Forced(name) => format!("tool_choice=forced:{name}"),
            Self::ForcedStrict(strict) => {
                let options: Vec<&str> = [
                    (strict.exactly_one, "exactly-one"),
                    (!strict.allow_content, "no-content"),
                    (!strict.allow_reasoning, "no-reasoning"),
                ]
                .into_iter()
                .filter_map(|(active, text)| active.then_some(text))
                .collect();
                if options.is_empty() {
                    format!("tool_choice=forced:{}", strict.name)
                } else {
                    format!("tool_choice=forced[{}]:{}", options.join(","), strict.name)
                }
            }
        }
    }
}

/// What ambiguity does to the operation's outcome.
///
/// ```
/// use purrassist_core::tool_call::AmbiguityPolicy;
/// assert_eq!(AmbiguityPolicy::default(), AmbiguityPolicy::Report);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AmbiguityPolicy {
    /// Report it: competing whole-message readings are `Ambiguous`, and an
    /// ambiguous call is a per-call `Ambiguous` outcome.
    #[default]
    Report,
    /// Reject it: any ambiguity, whole-message or per-call, makes the
    /// operation `Unresolved` ([`StopReason::NoCandidate`]) with a failed
    /// `tool_call.ambiguity` validation and every alternative kept in the
    /// receipt.
    Reject,
}

/// Owned tool-call recovery settings (one field per concept).
///
/// Prepared dialect sets are passed separately to recovery and stay caller-owned.
/// Cloning these settings never clones a matcher or selects a different set.
///
/// ```
/// use purrassist_core::{Effort, tool_call::{AmbiguityPolicy, Effects, GenerationStop, ToolChoice, ToolRecoveryRequest}};
/// let request = ToolRecoveryRequest::new(Effort::Deep)
///     .stop(GenerationStop::ToolCalls)
///     .tool_choice(ToolChoice::Required)
///     .effects("read_file", Effects::ReadOnly)
///     .ambiguity(AmbiguityPolicy::Reject);
/// assert_eq!(request.budget, Effort::Deep.budget());
/// assert_eq!(request.effects["read_file"], Effects::ReadOnly);
/// ```
#[derive(Clone, Debug)]
pub struct ToolRecoveryRequest {
    /// The requested effort, recorded in the receipt. Besides naming the
    /// budget preset ([`Self::new`]), [`Effort::Exact`] permits canonical
    /// parsing only: no repair rule ([`Rule::is_repair`](super::Rule::is_repair):
    /// any rule that costs something or guesses; see [`super::rules`] for the
    /// classification) is applied, so a call that needs one is `Unresolved`
    /// with [`UnresolvedReason::RepairNotPermitted`] naming it, its span kept
    /// and no repaired value reported. Zero-cost parsing (framing newlines,
    /// nested parameter elements, stringified arguments, typed values,
    /// aliases) and records (truncation, ids, invalid UTF-8) stay. Every
    /// other effort only selects the budget.
    pub effort: Effort,
    /// The limits [`ToolSet::recover`] runs under
    /// ([`ToolSet::recover_governed`] uses its governor's instead).
    pub budget: Budget,
    /// How reasoning markers are treated.
    pub reasoning: ReasoningPolicy,
    /// Why the model stopped generating.
    pub stop: GenerationStop,
    /// Which calls are allowed.
    pub tool_choice: ToolChoice,
    /// How calls receive ids.
    pub ids: IdPolicy,
    /// How each tool's repair policy is chosen.
    pub policy: PolicySelection,
    /// Effects overrides by canonical tool name; naming an undeclared tool
    /// is a request error.
    pub effects: BTreeMap<String, Effects>,
    /// What ambiguity does to the outcome.
    pub ambiguity: AmbiguityPolicy,
    /// What to print for the recovered calls ([`ToolCalls::emitted`]);
    /// [`EmitTarget::None`] prints nothing.
    pub emit: EmitTarget,
}

impl Default for ToolRecoveryRequest {
    fn default() -> Self {
        Self::new(Effort::default())
    }
}

impl ToolRecoveryRequest {
    /// Owned settings at `effort` with `budget = effort.budget()`, separate reasoning, a
    /// [`GenerationStop::Stop`], `Auto` tool choice, the default
    /// [`IdPolicy`], per-tool policies, no effects overrides,
    /// [`AmbiguityPolicy::Report`], and no emission.
    ///
    /// See [`ToolRecoveryRequest`] for an example.
    #[must_use]
    pub fn new(effort: Effort) -> Self {
        Self {
            effort,
            budget: effort.budget(),
            reasoning: ReasoningPolicy::default(),
            stop: GenerationStop::Stop,
            tool_choice: ToolChoice::Auto,
            ids: IdPolicy::default(),
            policy: PolicySelection::PerTool,
            effects: BTreeMap::new(),
            ambiguity: AmbiguityPolicy::Report,
            emit: EmitTarget::None,
        }
    }

    /// [`Self::new`] with a budget sized for `output` over the built-in
    /// dialects: [`Self::sized_for`] with [`DialectSet::built_in`]. The
    /// budget covers dense markup (many small calls, many repeated
    /// parameters) up to the measured maximum plus a margin, see
    /// [`Self::sized_budget`].
    ///
    /// ```
    /// use purrassist_core::{Effort, tool_call::{DialectSet, ToolRecoveryRequest}};
    /// let output = b"<tool_call>\n{\"name\": \"ls\", \"arguments\": {}}\n</tool_call>";
    /// let request = ToolRecoveryRequest::sized(Effort::Standard, output);
    /// // Three `<tool_call>` openers (Qwen3-Coder XML, Hermes and GLM) and two `{`
    /// // where a Llama 3 JSON, an Anthropic `tool_use` and a bare JSON
    /// // attempt may open.
    /// assert_eq!(request.budget, ToolRecoveryRequest::sized_budget(Effort::Standard, output.len(), 9));
    /// ```
    #[must_use]
    pub fn sized(effort: Effort, output: &[u8]) -> Self {
        Self::sized_for(effort, output, DialectSet::built_in())
    }

    /// [`Self::new`] over `dialects` with a budget sized for `output`: the
    /// output's length and its call openers under `dialects`
    /// ([`DialectSet::call_openers`]) give [`Self::sized_budget`]. Counting
    /// the openers is a linear, uncharged pre-scan; recovery then runs under
    /// the sized budget. Pass this same prepared set explicitly to recovery.
    /// Use this, not [`Self::sized`], for a restricted or extended allow-list, so the candidate budget
    /// counts the openers of the dialects that actually read the output.
    ///
    /// ```
    /// use purrassist_core::{Budget, Effort, Governor, tool_call::{DialectId, DialectSet, ToolRecoveryRequest}};
    /// let set = DialectSet::new([DialectId::Hermes], &mut Governor::new(Budget::default())).unwrap();
    /// let output = b"<tool_call>\n{\"name\": \"ls\", \"arguments\": {}}\n</tool_call>";
    /// let request = ToolRecoveryRequest::sized_for(Effort::Standard, output, &set);
    /// assert_eq!(request.budget, ToolRecoveryRequest::sized_budget(Effort::Standard, output.len(), 1));
    /// ```
    #[must_use]
    pub fn sized_for(effort: Effort, output: &[u8], dialects: &DialectSet) -> Self {
        let openers = dialects.call_openers(output);
        Self::new(effort).budget(Self::sized_budget(effort, output.len(), openers))
    }

    /// The budget for an output of `input_len` bytes with `openers` call
    /// openers ([`DialectSet::call_openers`]) at `effort`, from the
    /// preset `effort.budget()` and the constants of
    /// [`crate::grammar::cost`]:
    ///
    /// ```text
    /// work          = preset.work          + RECOVER_WORK_PER_BYTE    · input_len
    /// scratch_bytes = preset.scratch_bytes + RECOVER_SCRATCH_PER_BYTE · input_len
    /// input_bytes   = max(preset.input_bytes, input_len)
    /// candidates    = preset.candidates
    ///               + ⌈preset.candidates / RECOVER_OPENER_CANDIDATE_DIVISOR⌉ · openers
    /// evidence_entries = preset.evidence_entries
    ///               + ⌈input_len / RECOVER_EVIDENCE_BYTES_PER_ENTRY⌉
    /// ```
    ///
    /// Every other limit is the preset's. Saturating, finite, and a function
    /// of its arguments alone. Openers never exceed `input_len`, so every
    /// limit is linear in the output length. The per-byte constants cover
    /// the densest markup measured up to 1 MiB under the built-in dialects
    /// (see `docs/RESOURCES.md`, "Dense markup"), so output of that kind
    /// recovers under this budget as ordinary output does. The shapes that
    /// section lists as not covered still end in an explicit
    /// [`StopReason::BudgetExceeded`].
    ///
    /// ```
    /// use purrassist_core::{Effort, grammar::cost};
    /// use purrassist_core::tool_call::ToolRecoveryRequest;
    /// let budget = ToolRecoveryRequest::sized_budget(Effort::Quick, 1 << 20, 64);
    /// let preset = Effort::Quick.budget();
    /// assert_eq!(budget.work, preset.work + cost::RECOVER_WORK_PER_BYTE * (1 << 20));
    /// assert_eq!(budget.scratch_bytes, preset.scratch_bytes + cost::RECOVER_SCRATCH_PER_BYTE * (1 << 20));
    /// assert_eq!(budget.candidates, 16 + 2 * 64);
    /// assert_eq!(budget.evidence_entries, preset.evidence_entries + (1u64 << 20).div_ceil(cost::RECOVER_EVIDENCE_BYTES_PER_ENTRY));
    /// assert_eq!(ToolRecoveryRequest::sized_budget(Effort::Standard, 0, 0), Effort::Standard.budget());
    /// ```
    #[must_use]
    pub fn sized_budget(effort: Effort, input_len: usize, openers: u64) -> Budget {
        let mut budget = effort.budget();
        let length = size(input_len);
        budget.work = budget
            .work
            .saturating_add(RECOVER_WORK_PER_BYTE.saturating_mul(length));
        budget.scratch_bytes = budget
            .scratch_bytes
            .saturating_add(RECOVER_SCRATCH_PER_BYTE.saturating_mul(length));
        budget.input_bytes = budget.input_bytes.max(length);
        budget.evidence_entries = budget
            .evidence_entries
            .saturating_add(length.div_ceil(RECOVER_EVIDENCE_BYTES_PER_ENTRY));
        let per_opener = budget.candidates.div_ceil(RECOVER_OPENER_CANDIDATE_DIVISOR);
        budget.candidates = budget
            .candidates
            .saturating_add(per_opener.saturating_mul(openers));
        budget
    }

    /// Set the budget.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::ToolRecoveryRequest};
    /// let budget = Budget { work: 10, ..Budget::default() };
    /// assert_eq!(ToolRecoveryRequest::default().budget(budget).budget, budget);
    /// ```
    #[must_use]
    pub const fn budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Set the reasoning policy.
    ///
    /// ```
    /// use purrassist_core::tool_call::{ReasoningPolicy, ToolRecoveryRequest};
    /// let request = ToolRecoveryRequest::default().reasoning(ReasoningPolicy::AsContent);
    /// assert_eq!(request.reasoning, ReasoningPolicy::AsContent);
    /// ```
    #[must_use]
    pub const fn reasoning(mut self, reasoning: ReasoningPolicy) -> Self {
        self.reasoning = reasoning;
        self
    }

    /// Set the generation stop.
    ///
    /// ```
    /// use purrassist_core::tool_call::{GenerationStop, ToolRecoveryRequest};
    /// assert_eq!(ToolRecoveryRequest::default().stop(GenerationStop::Length).stop, GenerationStop::Length);
    /// ```
    #[must_use]
    pub fn stop(mut self, stop: GenerationStop) -> Self {
        self.stop = stop;
        self
    }

    /// Set the tool choice.
    ///
    /// ```
    /// use purrassist_core::tool_call::{ToolChoice, ToolRecoveryRequest};
    /// assert_eq!(ToolRecoveryRequest::default().tool_choice(ToolChoice::None).tool_choice, ToolChoice::None);
    /// ```
    #[must_use]
    pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = tool_choice;
        self
    }

    /// Set the id policy.
    ///
    /// ```
    /// use purrassist_core::tool_call::{IdPolicy, ToolRecoveryRequest};
    /// assert!(matches!(ToolRecoveryRequest::default().ids(IdPolicy::Source).ids, IdPolicy::Source));
    /// ```
    #[must_use]
    pub fn ids(mut self, ids: IdPolicy) -> Self {
        self.ids = ids;
        self
    }

    /// Set how each tool's repair policy is chosen.
    ///
    /// ```
    /// use purrassist_core::tool_call::{PolicySelection, RepairPolicy, ToolRecoveryRequest};
    /// let request = ToolRecoveryRequest::default().policy(PolicySelection::All(RepairPolicy::Tolerant));
    /// assert_eq!(request.policy, PolicySelection::All(RepairPolicy::Tolerant));
    /// ```
    #[must_use]
    pub const fn policy(mut self, policy: PolicySelection) -> Self {
        self.policy = policy;
        self
    }

    /// Override the effects of the tool named `tool` (a canonical name).
    ///
    /// ```
    /// use purrassist_core::tool_call::{Effects, ToolRecoveryRequest};
    /// let request = ToolRecoveryRequest::default().effects("ls", Effects::ReadOnly);
    /// assert_eq!(request.effects.len(), 1);
    /// ```
    #[must_use]
    pub fn effects(mut self, tool: impl Into<String>, effects: Effects) -> Self {
        self.effects.insert(tool.into(), effects);
        self
    }

    /// Set the ambiguity policy.
    ///
    /// ```
    /// use purrassist_core::tool_call::{AmbiguityPolicy, ToolRecoveryRequest};
    /// let request = ToolRecoveryRequest::default().ambiguity(AmbiguityPolicy::Reject);
    /// assert_eq!(request.ambiguity, AmbiguityPolicy::Reject);
    /// ```
    #[must_use]
    pub const fn ambiguity(mut self, ambiguity: AmbiguityPolicy) -> Self {
        self.ambiguity = ambiguity;
        self
    }

    /// Set what to print for the recovered calls.
    ///
    /// ```
    /// use purrassist_core::tool_call::{EmitTarget, ToolRecoveryRequest};
    /// let request = ToolRecoveryRequest::default().emit(EmitTarget::OpenAiToolCalls);
    /// assert_eq!(request.emit, EmitTarget::OpenAiToolCalls);
    /// ```
    #[must_use]
    pub fn emit(mut self, emit: EmitTarget) -> Self {
        self.emit = emit;
        self
    }
}

/// How a turn finished (D10).
///
/// - `Calls`: the turn made calls, nothing is wrong with any of them, and
///   at least one may run (usable accepting read-only guesses, the rule
///   [`ToolCalls::verdict`] and the emitters use).
/// - `ContentOnly`: no call, nothing wrong.
/// - `TruncatedWithCalls` / `Truncated`: a [`GenerationStop::Length`] stop
///   (never a clean finish), with or without a call that may run (a call
///   the stop cut off never may).
/// - `Unresolved`: something is wrong, whatever the stop: a rejected call
///   attempt (other than one a length stop cut off), a call that is
///   `Unresolved` (other than by truncation) or `Ambiguous`, a failed tool
///   choice check (even beside calls that would run: the turn breaks the
///   request's contract), or, under any other stop, calls of which none
///   may run (each a guess on a mutating tool). Truncation never upgrades
///   malformed output.
///
/// So `Calls` and `TruncatedWithCalls` imply [`Verdict::Run`]; the converse
/// fails only where something else is wrong beside the calls that run (a
/// rejected or unresolved attempt), which finishes `Unresolved`. The
/// `OpenAI` `finish_reason` does not depend on the finish
/// ([`openai_finish_reason`]).
///
/// ```
/// use purrassist_core::tool_call::Finish;
/// assert_eq!(Finish::TruncatedWithCalls.code(), "truncated_with_calls");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    /// Calls, all well formed.
    Calls,
    /// Content only.
    ContentOnly,
    /// A length stop after at least one call that is not cut off.
    TruncatedWithCalls,
    /// A length stop with no call that is not cut off.
    Truncated,
    /// Something is wrong with a call, an attempt, or the tool choice.
    Unresolved,
}

/// The `OpenAI` `finish_reason` (D15) of a turn: the one mapping.
///
/// It is `"length"` whenever the caller's stop reason is
/// [`GenerationStop::Length`] (a length
/// stop is never reported as a clean finish, whatever the finish: the
/// usable calls are still emitted), otherwise `"tool_calls"` exactly when
/// the `OpenAI` emission prints a call (`emits_calls`,
/// [`ToolCalls::emitted_calls`] not empty), and `"stop"` otherwise.
/// [`ToolCalls::openai_finish_reason`] (the wire `openai_finish_reason`)
/// applies it to the turn's own stop and emission.
///
/// `"stop"`, `"tool_calls"` and `"length"` all hide rejected and unresolved
/// calls: callers must read [`ToolCalls::finish`], [`ToolCalls::verdict`]
/// and [`ToolCalls::rejected`], never only this.
///
/// ```
/// use purrassist_core::tool_call::openai_finish_reason;
/// // A length stop is reported as such, with or without calls.
/// assert_eq!(openai_finish_reason(true, true), "length");
/// assert_eq!(openai_finish_reason(true, false), "length");
/// assert_eq!(openai_finish_reason(false, true), "tool_calls");
/// assert_eq!(openai_finish_reason(false, false), "stop");
/// ```
#[must_use]
pub const fn openai_finish_reason(length_stop: bool, emits_calls: bool) -> &'static str {
    if length_stop {
        "length"
    } else if emits_calls {
        "tool_calls"
    } else {
        "stop"
    }
}

impl Finish {
    /// The wire code: `calls`, `content_only`, `truncated_with_calls`,
    /// `truncated`, or `unresolved`.
    ///
    /// ```
    /// use purrassist_core::tool_call::Finish;
    /// assert_eq!(Finish::ContentOnly.code(), "content_only");
    /// ```
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Calls => "calls",
            Self::ContentOnly => "content_only",
            Self::TruncatedWithCalls => "truncated_with_calls",
            Self::Truncated => "truncated",
            Self::Unresolved => "unresolved",
        }
    }
}

/// What a caller should do with a recovered reading: the usability verdict
/// of [`ToolCalls`] ([`ToolCalls::verdict`], wire `value.verdict`).
///
/// The operation's `status` ([`Outcome`]) is the certainty
/// of the *reading of the turn* (one reading, a best guess among several,
/// competing readings, or none), shared with every other operation; it is
/// not a verdict on the calls. A turn read with certainty may still hold
/// nothing to run (a hallucinated tool name, a missing required argument, a
/// violated tool choice): this verdict says so.
///
/// | Verdict | Meaning | Caller |
/// | --- | --- | --- |
/// | `Run` | the tool-choice check passed and at least one call is usable when guesses are accepted ([`ToolCalls::usable_accepting_guesses`]) | run [`ToolCalls::usable`] (strict) or [`ToolCalls::usable_accepting_guesses`] |
/// | `NoCalls` | the tool-choice check passed and the turn is clean content (finish `content_only`) | use the content; nothing to run |
/// | `Ambiguous` | the tool-choice check passed, nothing is usable, and a call is `Ambiguous` | choose among the call's `alternatives`, or re-ask |
/// | `NothingUsable` | nothing to run: no usable call, or the tool-choice check failed (even beside calls that would be usable: [`ToolCalls::usable`] and [`ToolCalls::usable_accepting_guesses`] are then empty, and nothing is emitted) | re-ask or report; never run a call |
///
/// The CLI's exit code is [`Self::exit_code`] of this verdict.
///
/// ```
/// use purrassist_core::tool_call::{ToolDefinition, ToolRecoveryRequest, ToolSet, Verdict};
/// use purrassist_core::{Budget, Outcome};
/// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
/// let request = ToolRecoveryRequest::default();
/// // The turn is read with certainty, but the only call names no tool.
/// let result = tools.recover(br#"<tool_call>{"name": "rm", "arguments": {}}</tool_call>"#, &request, purrassist_core::tool_call::DialectSet::built_in());
/// let Outcome::Resolved(calls) = &result.outcome else { panic!() };
/// assert_eq!(calls.verdict(), Verdict::NothingUsable);
/// assert_eq!(calls.verdict().exit_code(), 4);
/// let result = tools.recover(br#"<tool_call>{"name": "ls", "arguments": {}}</tool_call>"#, &request, purrassist_core::tool_call::DialectSet::built_in());
/// let Outcome::Resolved(calls) = &result.outcome else { panic!() };
/// assert_eq!((calls.verdict(), calls.verdict().code()), (Verdict::Run, "run"));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// At least one call may run, and the tool choice is satisfied.
    Run,
    /// Clean content: no call, nothing wrong.
    NoCalls,
    /// Nothing usable, and a call is ambiguous.
    Ambiguous,
    /// Nothing to run, or the tool-choice check failed.
    NothingUsable,
}

impl Verdict {
    /// The wire code: `run`, `no_calls`, `ambiguous`, or `nothing_usable`.
    ///
    /// See [`Verdict`] for an example.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::NoCalls => "no_calls",
            Self::Ambiguous => "ambiguous",
            Self::NothingUsable => "nothing_usable",
        }
    }

    /// The `recover-tool-calls` CLI's exit code for a recovered reading:
    /// 0 for `Run` and `NoCalls`, 3 for `Ambiguous`, 4 for `NothingUsable`.
    /// (Without a reading, an `Ambiguous` operation exits 3 and an
    /// `Unresolved` one 4; see [`exit_code`].)
    ///
    /// See [`Verdict`] for an example.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Run | Self::NoCalls => 0,
            Self::Ambiguous => 3,
            Self::NothingUsable => 4,
        }
    }
}

/// The `recover-tool-calls` CLI's exit code for a recovery outcome.
///
/// It is the selected reading's [`Verdict::exit_code`], 3 for an
/// `Ambiguous` operation (competing readings, no selection) and 4 for an
/// `Unresolved` one (no reading: an exhausted budget, invalid input).
///
/// ```
/// use purrassist_core::Outcome;
/// use purrassist_core::tool_call::{ToolCalls, exit_code};
/// assert_eq!(exit_code(&Outcome::<ToolCalls>::Unresolved), 4);
/// ```
#[must_use]
pub fn exit_code(outcome: &crate::Outcome<ToolCalls>) -> u8 {
    match outcome {
        crate::Outcome::Resolved(calls) | crate::Outcome::BestGuess(calls) => {
            calls.verdict().exit_code()
        }
        crate::Outcome::Ambiguous(_) => 3,
        crate::Outcome::Unresolved => 4,
    }
}

/// The result of enforcing the request's [`ToolChoice`].
///
/// ```
/// use purrassist_core::tool_call::{ToolChoice, ToolChoiceCheck};
/// let check = ToolChoiceCheck { choice: ToolChoice::Auto, passed: true, diagnostics: Vec::new() };
/// assert!(check.passed);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolChoiceCheck {
    /// The enforced choice.
    pub choice: ToolChoice,
    /// Whether the turn satisfies it.
    pub passed: bool,
    /// One line per violation.
    pub diagnostics: Vec<String>,
}

/// One unmodeled keyword of an adapter tool's schema on a call's path.
///
/// `used` is whether repair consulted it. A keyword that makes a value
/// unmodeled (`$ref`, `anyOf`, `oneOf`, ..., and an unmodeled `type` such as
/// a type array) is used when the exact schema typed XML text under it
/// (`tool_call.value.schema-typed`: the JSON reading taken, or the verbatim
/// string kept where both validate); JSON arguments, already typed, never
/// consult it. `$schema`, Draft-07 `dependencies`,
/// `unevaluatedProperties`, and `patternProperties` decide how repair
/// treats their scope and are always used. Every other unmodeled keyword
/// (`format`, `minLength`, `x-*`, ...) is ignored by repair and enforced
/// only by validation against the exact schema.
///
/// ```
/// use purrassist_core::tool_call::KeywordUse;
/// let keyword = KeywordUse { pointer: "/properties/a".into(), keyword: "format".into(), used: false };
/// assert!(!keyword.used);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeywordUse {
    /// RFC 6901 pointer of the schema object holding the keyword, relative
    /// to the parameters schema.
    pub pointer: String,
    /// The keyword.
    pub keyword: String,
    /// Whether repair consulted it.
    pub used: bool,
}

/// A call attempt the recognizer rejected.
///
/// Dialects that share an opener (Qwen3-Coder XML and Hermes both open with
/// `<tool_call>`) each start an attempt there. When none of them reads a
/// call, the readings of the turn that differ only in which dialect's
/// attempt was rejected recover the same calls and are one reading: the
/// attempt reported is the one whose dialect read furthest into the body
/// before failing (the body is the evidence: a `<function=` body is read by
/// Qwen3-Coder XML, a JSON object body by Hermes), and
/// [`candidate_dialects`](Self::candidate_dialects) lists every dialect
/// that attempted a call at the same opener.
///
/// ```
/// use purrassist_core::tool_call::{CallLedger, DialectId, RejectedCall, Rejection};
/// let rejected = RejectedCall {
///     span: 0..3,
///     dialect: Some(DialectId::Hermes),
///     candidate_dialects: vec![DialectId::Hermes],
///     raw: b"<tc".to_vec(),
///     reason: Rejection::Unterminated,
///     repairs: CallLedger::default(),
/// };
/// assert_eq!(rejected.text(), Some("<tc"));
/// ```
///
/// A complete call followed by a dangling `<tool_call>` under the default
/// allow-list: GLM, Qwen3-Coder XML and Hermes attempted the dangling call,
/// the complete call stays usable, and the dangling remainder is one
/// rejected attempt naming all three dialects:
///
/// ```
/// use purrassist_core::tool_call::{DialectId, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::{Budget, Outcome};
/// let tools = ToolSet::compile([ToolDefinition::new("read").param("path", Param::string().required())], Budget::default()).unwrap();
/// let output = b"<tool_call>{\"name\": \"read\", \"arguments\": {\"path\": \"a\"}}</tool_call>\n<tool_call>";
/// let Outcome::BestGuess(calls) = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in()).outcome else { unreachable!() };
/// assert_eq!(calls.usable().len(), 1);
/// assert_eq!(calls.rejected.len(), 1);
/// assert_eq!(calls.rejected[0].candidate_dialects, [DialectId::Glm45, DialectId::Hermes, DialectId::Qwen3CoderXml]);
/// ```
///
/// A malformed call keeps its whole markup, closers included, and the
/// prose after it stays content:
///
/// ```
/// use purrassist_core::tool_call::{DialectId, DialectSet, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::{Budget, Governor, Outcome};
/// let tools = ToolSet::compile([ToolDefinition::new("f").param("a", Param::string())], Budget::default()).unwrap();
/// let call = "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\nthat is all</tool_call>";
/// let output = format!("{call} done");
/// let dialects = DialectSet::new([DialectId::Qwen3CoderXml], &mut Governor::new(Budget::default())).unwrap();
/// let request = ToolRecoveryRequest::default();
/// let (Outcome::Resolved(calls) | Outcome::BestGuess(calls)) = tools.recover(output.as_bytes(), &request, &dialects).outcome else { unreachable!() };
/// assert_eq!(calls.rejected[0].text(), Some(call));
/// assert_eq!(calls.content, [call.len()..output.len()]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedCall {
    /// The attempt's source range: the call markup it began, and nothing
    /// that is not shown to be its markup. It is decided in three steps:
    ///
    /// 1. Where the grammar stopped reading: the end of the last token it
    ///    read whole, which is where the furthest token it could not read
    ///    begins. A literal it matched only in part is left out (it may be
    ///    prose, or begin the next opener), so the reason's
    ///    [`Malformed { at }`](Rejection::Malformed) (the first byte no
    ///    reading could consume) lies inside the span, at its end, or past
    ///    it only inside such a partial literal (read up to `at`, perhaps
    ///    through one repaired byte): `<function=f<oops` is rejected over
    ///    `<function=f` (`0..11`) with `at` 12, past the `<` that began a
    ///    literal.
    /// 2. Never inside a literal: when the bytes there complete a literal
    ///    spelling of the dialect that began inside the span (a closer
    ///    variant read as a prefix of the canonical `</function>`), the
    ///    span runs to its end.
    /// 3. To its closer: an attempt a parser made runs on to the end
    ///    literal of the `call` alternative it opened (nesting by that
    ///    alternative's opener unless the alternative chains calls that
    ///    each open with it, or by bracket for a JSON call) and any repeated
    ///    closers right after it, found before the next call reopening that
    ///    alternative, a reasoning block, or reported unparsed markup; a
    ///    call or attempt inside the broken wrapper is kept, and this range
    ///    covers it. Without that closer, it runs to the end of the last
    ///    literal before the next segment that ends a call of its dialect
    ///    (a chained call's own `<|tool_call_end|>`, another alternative's
    ///    `</function>`), when there is one; else, when it left a JSON value
    ///    open (more brackets opened than closed, outside strings), to the
    ///    bracket that balances it.
    ///
    /// Otherwise the span ends at step 2 and the bytes after it stay
    /// content (prose after a broken opener is the answer). Content never
    /// holds the span's bytes. Unparsed markup
    /// ([`Rejection::UnsupportedDialect`]) covers only the detected markup,
    /// whose bytes also stay content.
    pub span: Range<usize>,
    /// The dialect whose opener started the attempt reported: of the
    /// dialects that attempted a call at this opener, the one whose grammar
    /// read furthest before failing. For call markup no allowed dialect
    /// reads ([`Rejection::UnsupportedDialect`]), the built-in dialect of
    /// the detected family when it has one (the allow-list left it out),
    /// and `None` for a family no built-in dialect parses yet.
    pub dialect: Option<DialectId>,
    /// Every dialect whose opener started a rejected attempt at this
    /// offset in a reading of the turn that recovers the same calls, in
    /// canonical [`DialectId`] order; it contains
    /// [`dialect`](Self::dialect) for every attempt a parser made, and is
    /// empty for unparsed markup ([`Rejection::UnsupportedDialect`]),
    /// which no allowed dialect attempted.
    pub candidate_dialects: Vec<DialectId>,
    /// The attempt's exact bytes: `output[span]`, closing markup included.
    pub raw: Vec<u8>,
    /// Why it was rejected.
    pub reason: Rejection,
    /// Its message-level repairs (a `tool_call.markup.dangling-opener` data
    /// loss when it was still open at the end of output).
    pub repairs: CallLedger,
}

impl RejectedCall {
    /// The attempt as text, or `None` when its bytes are not UTF-8.
    ///
    /// See [`RejectedCall`] for an example.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        core::str::from_utf8(&self.raw).ok()
    }
}

/// One recovered call.
///
/// ```
/// use purrassist_core::tool_call::{Effects, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::{Budget, Outcome};
/// let tools = ToolSet::compile([ToolDefinition::new("ls").effects(Effects::ReadOnly).param("depth", Param::integer())], Budget::default()).unwrap();
/// let result = tools.recover(br#"<tool_call>{"name": "ls", "arguments": {"depth": 2}}</tool_call>"#, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in());
/// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
/// let call = &calls.calls[0];
/// // `call_` + the first 12 hex digits of the output's SHA-256 + `_` + index.
/// assert_eq!((call.id.as_str(), call.tool.as_deref()), ("call_473f1a043a7d_0", Some("ls")));
/// assert_eq!(call.arguments.to_json(), r#"{"depth":2}"#);
/// assert!(call.is_usable());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    /// The call's position among the result's calls.
    pub index: usize,
    /// The id assigned by the request's [`IdPolicy`].
    pub id: String,
    /// The id the output carries, if any.
    pub source_id: Option<String>,
    /// The canonical name of the tool, when one was identified.
    pub tool: Option<String>,
    /// The tool name as written, if any.
    pub name: Option<String>,
    /// The arguments: resolved and canonical when the call resolved,
    /// otherwise the best tree resolution reached.
    pub arguments: Node,
    /// The call's source range.
    pub span: Range<usize>,
    /// The dialect that recognized it.
    pub dialect: DialectId,
    /// Every dialect that recognized this same call (same span, tool,
    /// arguments, outcome, and repairs) in a reading of the turn, in
    /// canonical [`DialectId`] order; always contains
    /// [`dialect`](Self::dialect). Several dialects that read the same
    /// bytes into the same call are one reading of the turn, never an
    /// ambiguity, and each of them is named here.
    pub candidate_dialects: Vec<DialectId>,
    /// The tool's effective effects, when a tool was identified.
    pub effects: Option<Effects>,
    /// The tool's effective repair policy, when a tool was identified.
    pub policy: Option<RepairPolicy>,
    /// The per-call outcome.
    pub outcome: CallOutcome,
    /// Every repair of the selected reading, itemized (C2), including a
    /// `tool_call.id.dedup` inference for a renumbered source id.
    pub repairs: CallLedger,
    /// Every other reading, best first; an `Ambiguous` call's competing
    /// readings come first.
    pub alternatives: Vec<CallReading>,
    /// For an `Ambiguous` call, the provenance that distinguishes its two
    /// best readings.
    pub distinguishing: Vec<ProvenanceNode>,
    /// The unmodeled keywords of an adapter tool's schema on this call's
    /// path, in schema order.
    pub keywords: Vec<KeywordUse>,
}

impl ToolCall {
    /// Convert the arguments exactly into `T`.
    ///
    /// # Errors
    /// [`FromNodeError`] locating the first value whose JSON type,
    /// integrality, or range does not fit `T`.
    ///
    /// ```
    /// use purrassist_core::json::Node;
    /// use purrassist_core::tool_call::{Effects, FromNode, FromNodeError, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, Outcome};
    /// struct Read { path: String, limit: Option<u32> }
    /// impl FromNode for Read {
    ///     fn from_node(node: &Node) -> Result<Self, FromNodeError> {
    ///         let field = |key: &str| node.get(key).cloned().unwrap_or(Node::new(purrassist_core::json::Value::Null));
    ///         Ok(Self { path: String::from_node(&field("path"))?, limit: Option::from_node(&field("limit"))? })
    ///     }
    /// }
    /// let tools = ToolSet::compile([ToolDefinition::new("read").effects(Effects::ReadOnly)
    ///     .param("path", Param::string().required()).param("limit", Param::integer())], Budget::default()).unwrap();
    /// let result = tools.recover(br#"<tool_call>{"name": "read", "arguments": {"path": "a.rs", "limit": 5}}</tool_call>"#, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in());
    /// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
    /// let read: Read = calls.calls[0].arguments_as().unwrap();
    /// assert_eq!((read.path.as_str(), read.limit), ("a.rs", Some(5)));
    /// assert!(calls.calls[0].arguments_as::<u32>().is_err());
    /// ```
    pub fn arguments_as<T: FromNode>(&self) -> Result<T, FromNodeError> {
        T::from_node(&self.arguments)
    }

    /// Whether the call may run as is: its outcome is `Resolved` and it
    /// names a declared tool.
    ///
    /// See [`ToolCall`] for an example.
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.outcome == CallOutcome::Resolved && self.tool.is_some()
    }

    /// Whether the call may run when the caller accepts guesses: `Resolved`,
    /// or `BestGuess` on a tool whose effective effects are
    /// [`Effects::ReadOnly`]. A guess on a mutating tool never qualifies.
    ///
    /// See [`ToolCalls::usable_accepting_guesses`] for an example.
    #[must_use]
    pub fn is_usable_accepting_guesses(&self) -> bool {
        self.is_usable()
            || (self.outcome == CallOutcome::BestGuess
                && self.effects == Some(Effects::ReadOnly)
                && self.tool.is_some())
    }
}

/// One reasoning block of a turn: its source range with the markers, and
/// its text without them.
///
/// `text` begins where the opening marker ends (at `0` under
/// [`ReasoningPolicy::Separate`] with `starts_open`, where the turn begins
/// inside a block and no opener is stripped) and ends where the closing
/// marker begins; a block closed at a call opener ends at the opener, and an
/// unterminated block at the end of output. It is a range of the output's
/// bytes, verbatim and never unescaped: the text of an Anthropic `thinking`
/// block stays JSON-escaped. The text of an Anthropic `redacted_thinking`
/// block is not reasoning prose: it is the opaque encrypted `data` payload,
/// verbatim, which a consumer must not interpret or display. `text` is
/// never a reversed range: a closing marker is recognized only at or after
/// the start of the text, so one that overlaps the opening marker does not
/// close the block.
///
/// ```
/// use purrassist_core::tool_call::{Effects, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::{Budget, Outcome};
/// let tools = ToolSet::compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)], Budget::default()).unwrap();
/// let result = tools.recover(b"<think>abc</think>Done.", &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in());
/// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
/// let block = &calls.reasoning_blocks[0];
/// assert_eq!(block.span, 0..18);
/// assert_eq!(block.text, 7..10);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReasoningBlock {
    /// The block's range, markers included.
    pub span: Range<usize>,
    /// The block's text range, markers excluded; inside [`Self::span`].
    pub text: Range<usize>,
}

/// Every call of one turn, with what surrounds them.
///
/// Whether anything can run is [`Self::verdict`] (wire `verdict`, with the
/// `usable_calls` and `usable_calls_accepting_guesses` counts), never the
/// operation's status, which is the certainty of the reading of the turn.
///
/// See [`ToolSet::recover`] for an example.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCalls {
    /// The calls, in output order.
    pub calls: Vec<ToolCall>,
    /// The rejected call attempts, in output order. An attempt over an
    /// opener several dialects share is reported once (see
    /// [`RejectedCall`]).
    pub rejected: Vec<RejectedCall>,
    /// The calls written inside closed reasoning blocks, in output order:
    /// reasoning text offered only as guess-flagged alternatives
    /// (`tool_call.reasoning.embedded-call`), so never `Resolved`. They are
    /// not calls of the turn: [`Self::usable`] and
    /// [`Self::usable_accepting_guesses`] never select them, their
    /// [`ToolCall::id`] is empty and their [`ToolCall::index`] is their
    /// position here, and they affect neither [`Self::finish`], nor
    /// [`Self::tool_choice`], nor the operation's outcome. Each keeps its
    /// own itemized repairs; the receipt itemizes the selected reading
    /// only.
    ///
    /// ```
    /// use purrassist_core::tool_call::{CallOutcome, Effects, Finish, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, Outcome};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").effects(Effects::ReadOnly).param("depth", Param::integer())], Budget::default()).unwrap();
    /// let output = br#"<think>maybe <tool_call>{"name": "ls", "arguments": {"depth": 1}}</tool_call></think>No call needed."#;
    /// let result = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in());
    /// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
    /// assert!(calls.calls.is_empty());
    /// assert_eq!(calls.finish, Finish::ContentOnly);
    /// let embedded = &calls.embedded[0];
    /// assert_eq!((embedded.tool.as_deref(), embedded.outcome.clone()), (Some("ls"), CallOutcome::BestGuess));
    /// assert!(embedded.repairs.evidence.iter().any(|evidence| evidence.rule == "tool_call.reasoning.embedded-call"));
    /// assert!(calls.usable_accepting_guesses().is_empty());
    /// ```
    pub embedded: Vec<ToolCall>,
    /// The content ranges (tool results the model wrote count as content),
    /// adjacent ranges merged.
    pub content: Vec<Range<usize>>,
    /// The reasoning ranges, markers included, adjacent ranges merged:
    /// `<think>a</think><think>b</think>` is one range, `0..32`. See
    /// [`Self::reasoning_blocks`] for the blocks one by one, and for their
    /// marker-free text.
    pub reasoning: Vec<Range<usize>>,
    /// The reasoning blocks, one entry per block in source order and never
    /// merged, each with its marker-free text range
    /// ([`ReasoningBlock::text`]). Every block's `span` lies inside one
    /// [`Self::reasoning`] range, and the spans together cover exactly the
    /// bytes those ranges cover.
    ///
    /// ```
    /// use purrassist_core::tool_call::{Effects, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, Outcome};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)], Budget::default()).unwrap();
    /// let output = b"<think>a</think><think>b</think>";
    /// let result = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in());
    /// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
    /// // One merged outer range, two blocks whose text excludes the markers.
    /// assert_eq!(calls.reasoning, vec![0..32]);
    /// assert_eq!(calls.reasoning_blocks[0].span, 0..16);
    /// assert_eq!(calls.reasoning_blocks[0].text, 7..8);
    /// assert_eq!(calls.reasoning_blocks[1].text, 23..24);
    /// assert_eq!(&output[calls.reasoning_blocks[1].text.clone()], b"b");
    /// ```
    pub reasoning_blocks: Vec<ReasoningBlock>,
    /// How the turn finished.
    pub finish: Finish,
    /// The tool choice check.
    pub tool_choice: ToolChoiceCheck,
    /// The id counter after this recovery (see [`IdPolicy`]).
    pub next_id_start: u64,
    /// The SHA-256 digest of the turn's output bytes: the receipt's source
    /// digest, from which [`IdPolicy::Turn`] numbers calls
    /// ([`turn_id_prefix`]), so emitting these calls under it reproduces
    /// recovery's ids. Not part of the wire form, which names the digest in
    /// its receipt.
    pub turn: Digest,
    /// Whether the caller's stop reason was [`GenerationStop::Length`]: the
    /// turn's `OpenAI` `finish_reason` is then `"length"` whatever its
    /// finish ([`Self::openai_finish_reason`]). Not part of the wire form,
    /// which states the `finish_reason` itself.
    pub length_stop: bool,
    /// The calls printed for the request's [`EmitTarget`], with every call
    /// and rejected attempt not printed; `None` for [`EmitTarget::None`].
    /// Ids are the calls' assigned [`ToolCall::id`]s.
    pub emitted: Option<Emission>,
}

impl ToolCalls {
    /// The calls that may run as is: per-call `Resolved` only, and none
    /// when the turn fails the request's tool-choice check (a violated tool
    /// choice is a contract failure, so nothing of the turn runs, and a
    /// count of these calls always agrees with the empty emission).
    ///
    /// ```
    /// use purrassist_core::tool_call::{ToolChoice, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, Outcome};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls"), ToolDefinition::new("cat")], Budget::default()).unwrap();
    /// let output = br#"<tool_call>{"name": "ls", "arguments": {}}</tool_call>"#;
    /// let result = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in());
    /// let Outcome::Resolved(calls) = &result.outcome else { panic!() };
    /// assert_eq!(calls.usable().len(), 1);
    /// // Forced to `cat`, the check fails: the call is still `Resolved`, but nothing may run.
    /// let forced = ToolRecoveryRequest::default().tool_choice(ToolChoice::Forced("cat".into()));
    /// let result = tools.recover(output, &forced, purrassist_core::tool_call::DialectSet::built_in());
    /// let Outcome::Resolved(calls) = &result.outcome else { panic!() };
    /// assert!(!calls.tool_choice.passed && calls.usable().is_empty());
    /// ```
    #[must_use]
    pub fn usable(&self) -> Vec<&ToolCall> {
        if !self.tool_choice.passed {
            return Vec::new();
        }
        self.calls.iter().filter(|call| call.is_usable()).collect()
    }

    /// The calls that may run when the caller accepts guesses: also every
    /// `BestGuess` call to a read-only tool, never a guess on a mutating
    /// tool. As with [`Self::usable`], none when the turn fails the
    /// request's tool-choice check.
    ///
    /// ```
    /// use purrassist_core::tool_call::{Effects, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, Outcome};
    /// let tools = ToolSet::compile([ToolDefinition::new("read_file").effects(Effects::ReadOnly)], Budget::default()).unwrap();
    /// let result = tools.recover(br#"<tool_call>{"name": "read_fiel", "arguments": {}}</tool_call>"#, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in());
    /// let Outcome::BestGuess(calls) = result.outcome else { unreachable!() };
    /// assert!(calls.usable().is_empty());
    /// assert_eq!(calls.usable_accepting_guesses()[0].tool.as_deref(), Some("read_file"));
    /// ```
    #[must_use]
    pub fn usable_accepting_guesses(&self) -> Vec<&ToolCall> {
        if !self.tool_choice.passed {
            return Vec::new();
        }
        self.calls
            .iter()
            .filter(|call| call.is_usable_accepting_guesses())
            .collect()
    }

    /// The `OpenAI` `finish_reason` of this turn: [`openai_finish_reason`]
    /// of whether the stop was a length stop ([`Self::length_stop`]) and
    /// whether the `OpenAI` emission prints a call ([`Self::emitted_calls`]
    /// is what every emitter prints). The wire `openai_finish_reason`.
    ///
    /// See [`openai_finish_reason`] for the mapping.
    #[must_use]
    pub fn openai_finish_reason(&self) -> &'static str {
        openai_finish_reason(self.length_stop, !self.emitted_calls().is_empty())
    }

    /// The calls every emitter prints, and the calls a caller may run:
    /// [`Self::usable_accepting_guesses`], which is empty when the turn
    /// fails the request's tool-choice check (a violated tool choice is a
    /// contract failure, so nothing of the turn runs). The one selection [`Self::verdict`] (`Run` exactly when it is not empty),
    /// [`Self::openai_finish_reason`] and the emitters use.
    ///
    /// ```
    /// use purrassist_core::tool_call::{ToolChoice, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, Outcome};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls"), ToolDefinition::new("cat")], Budget::default()).unwrap();
    /// let output = br#"<tool_call>{"name": "ls", "arguments": {}}</tool_call>"#;
    /// let result = tools.recover(output, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in());
    /// let Outcome::Resolved(calls) = &result.outcome else { panic!() };
    /// assert_eq!(calls.emitted_calls().len(), 1);
    /// // Forced to `cat`, the turn fails its tool choice: nothing runs.
    /// let forced = ToolRecoveryRequest::default().tool_choice(ToolChoice::Forced("cat".into()));
    /// let result = tools.recover(output, &forced, purrassist_core::tool_call::DialectSet::built_in());
    /// let Outcome::Resolved(calls) = &result.outcome else { panic!() };
    /// assert!(calls.emitted_calls().is_empty());
    /// assert_eq!(calls.openai_finish_reason(), "stop");
    /// ```
    #[must_use]
    pub fn emitted_calls(&self) -> Vec<&ToolCall> {
        self.usable_accepting_guesses()
    }

    /// What a caller should do with this reading (see [`Verdict`]): the
    /// one usability verdict, from the tool-choice check, the usable calls,
    /// the finish and the calls' outcomes.
    ///
    /// See [`Verdict`] for an example.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        if !self.emitted_calls().is_empty() {
            Verdict::Run
        } else if !self.tool_choice.passed {
            Verdict::NothingUsable
        } else if self.finish == Finish::ContentOnly {
            Verdict::NoCalls
        } else if self
            .calls
            .iter()
            .any(|call| call.outcome == CallOutcome::Ambiguous)
        {
            Verdict::Ambiguous
        } else {
            Verdict::NothingUsable
        }
    }
}

fn effects_code(effects: Option<Effects>) -> Node {
    effects.map_or_else(null, |effects| {
        string(match effects {
            Effects::ReadOnly => "read_only",
            Effects::Mutating => "mutating",
        })
    })
}

fn policy_code(policy: Option<RepairPolicy>) -> Node {
    policy.map_or_else(null, |policy| {
        string(match policy {
            RepairPolicy::Structural => "structural",
            RepairPolicy::Tolerant => "tolerant",
        })
    })
}

fn optional_string(value: Option<&str>) -> Node {
    value.map_or_else(null, string)
}

fn reason_node(reason: Option<&UnresolvedReason>) -> Node {
    reason.map_or_else(null, |reason| {
        object([
            ("code", string(reason.code())),
            ("detail", string(reason.to_string())),
        ])
    })
}

fn outcome_node(outcome: &CallOutcome) -> Node {
    let (status, reason) = match outcome {
        CallOutcome::Resolved => ("resolved", None),
        CallOutcome::BestGuess => ("best_guess", None),
        CallOutcome::Ambiguous => ("ambiguous", None),
        CallOutcome::Unresolved { reason } => ("unresolved", Some(reason)),
    };
    object([("status", string(status)), ("reason", reason_node(reason))])
}

fn ledger_node(ledger: &CallLedger) -> Node {
    object([
        (
            "evidence",
            array(ledger.evidence.iter().map(wire::evidence)),
        ),
        (
            "losses",
            array(ledger.losses.iter().map(|loss| {
                object([
                    (
                        "kind",
                        string(match loss.kind {
                            LossKind::Markup => "markup",
                            LossKind::Data => "data",
                        }),
                    ),
                    ("evidence", wire::evidence(&loss.loss.evidence)),
                    ("original", string(&loss.loss.original)),
                ])
            })),
        ),
        ("inferences", inferences(&ledger.inferences)),
    ])
}

fn reading_node(reading: &CallReading) -> Node {
    object([
        ("tool", optional_string(reading.tool.as_deref())),
        ("effects", effects_code(reading.effects)),
        ("policy", policy_code(reading.policy)),
        ("arguments", reading.arguments.clone()),
        ("cost", integer(reading.cost)),
        ("repairs", ledger_node(&reading.repairs)),
        (
            "validations",
            array(reading.validations.iter().map(wire::validation)),
        ),
        ("blocker", reason_node(reading.blocker.as_ref())),
    ])
}

fn provenance_node(node: &ProvenanceNode) -> Node {
    let (kind, rule) = match node {
        ProvenanceNode::Evidence(evidence) => ("evidence", evidence.rule.clone()),
        ProvenanceNode::Loss(loss) => ("loss", loss.evidence.rule.clone()),
        ProvenanceNode::Inference(inference) => ("inference", inference.evidence.rule.clone()),
        ProvenanceNode::Validation(index, _) => ("validation", format!("validation:{index}")),
    };
    object([("kind", string(kind)), ("rule", string(rule))])
}

/// The schema pointer a `tool_call.value.schema-typed` explanation names
/// (the backquoted pointer after "the exact schema at").
fn typed_schema(explanation: &str) -> Option<&str> {
    let start = explanation.find("the exact schema at `")? + "the exact schema at `".len();
    let length = explanation[start..].find('`')?;
    Some(&explanation[start..start + length])
}

/// The nesting state of a rejected attempt's markup scan.
/// The bytes a rejected attempt's markup is read over.
struct MarkupScan<'o> {
    output: &'o [u8],
}

/// Where the markup of `dialect`'s rejected attempt at `span` over `output`
/// ends (see [`Recovery::close_rejections`]): `boundaries` are the segments
/// after it, in order, each marked whether the scan may pass over it (a call
/// or attempt inside the broken wrapper) or stops at it (a reasoning block,
/// reported unparsed markup). `reason` says where its grammar stopped reading
/// (a resync may have read on past it, to a literal). The recognizer reads
/// the same end to keep fences inside rejected markup out of the prose
/// context.
pub(super) fn rejected_end(
    output: &[u8],
    span: &Range<usize>,
    dialect: &DialectId,
    reason: Rejection,
    boundaries: &[(Range<usize>, bool)],
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    let ends = dialect.grammar().call_ends();
    let at = |spellings: &[ScratchVec<u8>], position: usize| {
        spellings
            .iter()
            .filter(|spelling| output[position..].starts_with(spelling))
            .map(ScratchVec::len)
            .max()
    };
    // The alternatives the attempt opened (its longest opener spelling;
    // every alternative sharing that opener, as an Anthropic block and a
    // streaming event share `{`), or every alternative when no spelling
    // matches.
    let opened: Vec<&CallEnd> = {
        let matched: Vec<(usize, &CallEnd)> = ends
            .iter()
            .filter_map(|end| at(end.openers(), span.start).map(|length| (length, end)))
            .collect();
        let best = matched.iter().map(|(length, _)| *length).max();
        best.map_or_else(
            || ends.iter().collect(),
            |best| {
                matched
                    .into_iter()
                    .filter(|(length, _)| *length == best)
                    .map(|(_, end)| end)
                    .collect()
            },
        )
    };
    let markup = Markup {
        openers: opened
            .iter()
            .flat_map(|end| end.openers().iter().map(|bytes| bytes.as_slice().to_vec()))
            .collect(),
        closers: opened
            .iter()
            .flat_map(|end| end.closers().iter().map(|bytes| bytes.as_slice().to_vec()))
            .collect(),
        // A repeated opener nests a level, unless the alternative chains
        // several calls that each open with it.
        nests: !opened.iter().any(|end| end.chained),
        // Every literal that ends a call of the dialect, in any alternative,
        // a section's or a chained call's own; a single byte (a JSON
        // bracket, a separator) is prose too.
        call_closers: {
            let mut closers: Vec<Vec<u8>> = ends
                .iter()
                .flat_map(|end| end.call_closers().iter())
                .filter(|spelling| spelling.len() > 1)
                .map(|bytes| bytes.as_slice().to_vec())
                .collect();
            closers.sort();
            closers.dedup();
            closers
        },
        literals: dialect.grammar().call_literals(),
    };
    // Where the grammar stopped reading the attempt's own syntax.
    let read = match reason {
        Rejection::Malformed { at }
        | Rejection::PythonicPositionalRejected { at }
        | Rejection::InvalidUtf8 { at }
        | Rejection::Overlapped { at } => at.clamp(span.start, span.end),
        Rejection::Unterminated | Rejection::UnsupportedDialect { .. } => span.end,
    };
    MarkupScan { output }.markup_end(span, read, &markup, boundaries, governor)
}

impl MarkupScan<'_> {
    /// Where the markup of a rejected attempt at `span` ends (see
    /// [`Recovery::close_rejections`]); `boundaries` are the segments after it
    /// in order, each marked whether the scan may pass over it.
    fn markup_end(
        &self,
        span: &Range<usize>,
        read: usize,
        markup: &Markup<'_>,
        boundaries: &[(Range<usize>, bool)],
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let (openers, closers, nests) = (&markup.openers, &markup.closers, markup.nests);
        let len = self.output.len();
        let at = |spellings: &[Vec<u8>], position: usize| {
            spellings
                .iter()
                .filter(|spelling| self.output[position..].starts_with(spelling))
                .map(Vec::len)
                .max()
        };
        // An alternative whose end literal is a bracket (a JSON object
        // or array call) nests by that bracket, outside JSON strings;
        // any other nests by its opener spellings, brackets paired by kind
        // (see `Scan`). The walk starts at the attempt's own start, so the
        // depth at its failure is known.
        let brackets: Vec<(u8, u8)> = [(b'{', b'}'), (b'[', b']')]
            .into_iter()
            .filter(|(_, close)| closers.iter().any(|spelling| spelling == &[*close]))
            .collect();
        let mut walk = Scan {
            state: Nesting::default(),
            open: Brackets::new(governor),
            body: false,
            json_end: span.end,
        };
        let mut position = span.start;
        let mut next = boundaries.iter();
        let mut found = None;
        'scan: loop {
            let (limit, boundary) = match next.next() {
                Some((range, call)) => (range.start, Some((range, *call))),
                None => (len, None),
            };
            while position < limit {
                governor.charge(Resource::Work, 1)?;
                if !brackets.is_empty() {
                    match self.bracket_step(
                        &mut walk,
                        &mut position,
                        span,
                        &brackets,
                        limit,
                        governor,
                    )? {
                        Flow::Next => continue,
                        Flow::Found(end) => found = Some(end),
                        Flow::Stop => {}
                    }
                    break 'scan;
                }
                if let Some(length) = at(closers, position)
                    .or_else(|| self.misspelled(closers, markup, position, limit))
                {
                    walk.state.depth = walk.state.depth.saturating_sub(1);
                    position += length;
                    if walk.state.depth == 0 && position > span.end {
                        found = Some(position.min(limit));
                        break 'scan;
                    }
                } else if let Some(length) = at(openers, position) {
                    if nests || walk.state.depth == 0 {
                        walk.state.depth += 1;
                    }
                    position += length;
                } else {
                    position += 1;
                }
            }
            match boundary {
                // A call or attempt inside the broken wrapper that does
                // not reopen its alternative.
                Some((range, true)) if at(openers, range.start).is_none() => {
                    // Its brackets nest with the attempt's: when it closes
                    // the bracket the attempt left open (a group's last
                    // call holds the array's `]`), no later bracket closes
                    // the attempt, which ends unclosed.
                    if !brackets.is_empty()
                        && self.closes(
                            &mut walk.state,
                            &mut walk.open,
                            &brackets,
                            range,
                            governor,
                        )?
                    {
                        break 'scan;
                    }
                    position = range.end;
                }
                // A call body the attempt left open runs to where the next
                // segment (or the output's end) cuts it: its markup, no
                // prose.
                _ if walk.body && walk.state.depth > 0 => {
                    found = Some(limit);
                    break;
                }
                _ => break,
            }
        }
        let Some(end) = found.filter(|end| *end > span.end) else {
            let end = self.unclosed_end(span, read, markup, boundaries, governor)?;
            return self.call_body_end(end, boundaries, governor);
        };
        // Repeated closers right after the end (whitespace between) are
        // the attempt's stray closers, as after a completed call.
        let end = if brackets.is_empty() {
            self.stray_closers(end, markup, boundaries)
        } else {
            end
        };
        // A call body after the attempt's markup, before prose, is its
        // markup too (F16).
        self.call_body_end(end, boundaries, governor)
    }

    /// `end`, moved past the call bodies after it, which are the rejected
    /// attempt's markup (F16: a call body is never content): each begins at
    /// a call's member (`"name"`, then `:`), whether its object's `{` is
    /// there or was lost (a cut special token, a slip that broke a string
    /// before it), and runs to the bracket closing its object, to its last
    /// JSON byte before prose, or to where the next segment (`boundaries`)
    /// or the output's end cuts it. Only markup debris comes before a body:
    /// whitespace, separators, the `:` after a member's key the attempt
    /// completed (the `"function"` a cut `OpenAI` arguments string runs
    /// into), stray punctuation (the `>` of a cut tag), and JSON text
    /// holding no call's member, read from `end` with strings opening and
    /// closing there, never prose (see [`Self::bare_word`]); debris after
    /// the last body stays where it is. `Work` per byte read.
    fn call_body_end(
        &self,
        end: usize,
        boundaries: &[(Range<usize>, bool)],
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let limit = boundaries
            .iter()
            .map(|(range, _)| range.start)
            .find(|start| *start >= end)
            .unwrap_or(self.output.len());
        let brackets = [(b'{', b'}'), (b'[', b']')];
        let (mut end, mut from) = (end, end);
        loop {
            // The debris before the next call member: no prose outside its
            // strings. A member's spelling begins a body wherever it is
            // written: a slip that broke a string before it leaves no
            // telling which quote opens a string.
            let (mut quoted, mut escaped) = (false, false);
            while from < limit {
                governor.charge(Resource::Work, 1)?;
                let byte = self.output[from];
                if byte == b'"' && !escaped && self.call_member(from) {
                    governor.charge(Resource::Work, 8)?;
                    break;
                }
                if quoted {
                    match byte {
                        _ if escaped => escaped = false,
                        b'\\' => escaped = true,
                        b'"' => quoted = false,
                        _ => {}
                    }
                } else if byte == b'"' {
                    quoted = true;
                } else if (byte.is_ascii_alphabetic() || byte == b'_')
                    && (from == 0 || !is_word_byte(self.output[from - 1]))
                {
                    let (word, prose) = self.bare_word(from, limit);
                    governor.charge(Resource::Work, size(word))?;
                    if prose {
                        return Ok(end);
                    }
                    from += word;
                    continue;
                }
                from += 1;
            }
            if from >= limit {
                return Ok(end);
            }
            // The body: the walk opens its object at the member (see
            // `bracket_step`) and closes it by its `}`.
            let mut walk = Scan {
                state: Nesting::default(),
                open: Brackets::new(governor),
                body: false,
                json_end: from,
            };
            let (start, mut position) = (from..from, from);
            loop {
                if position >= limit {
                    return Ok(if walk.body && walk.state.depth > 0 {
                        limit
                    } else {
                        end
                    });
                }
                governor.charge(Resource::Work, 1)?;
                match self.bracket_step(
                    &mut walk,
                    &mut position,
                    &start,
                    &brackets,
                    limit,
                    governor,
                )? {
                    Flow::Next => {}
                    Flow::Found(at) => {
                        end = at.max(end);
                        from = at.max(from + 1);
                        break;
                    }
                    Flow::Stop => return Ok(end),
                }
            }
        }
    }

    /// Read the byte at `position` of a JSON-object attempt's markup (see
    /// [`Self::markup_end`]), outside strings by its `brackets`, paired by
    /// kind: on to the next byte, the attempt's end (the bracket closing
    /// what it opened, or the end of the call body it left open where prose
    /// begins), or no end (prose, or a bracket closing a structure around
    /// it).
    fn bracket_step(
        &self,
        walk: &mut Scan,
        position: &mut usize,
        span: &Range<usize>,
        brackets: &[(u8, u8)],
        limit: usize,
        governor: &mut Governor,
    ) -> Result<Flow, StopReason> {
        let at = *position;
        let byte = self.output[at];
        // Past where the grammar stopped, prose (a bare word followed by
        // another word or sentence punctuation, outside a string) is no
        // JSON structure: a bracket after it closes nothing the attempt
        // read, and the attempt ends as one left unclosed, or, when it
        // left a call body open (`<|python_tag|>t_begin|>"name": ...`),
        // after that body's last JSON byte.
        if !walk.state.quoted
            && at >= span.end
            && (byte.is_ascii_alphabetic() || byte == b'_')
            && (at == 0 || !is_word_byte(self.output[at - 1]))
        {
            let (word, prose) = self.bare_word(at, limit);
            governor.charge(Resource::Work, size(word))?;
            if prose {
                return Ok(if walk.body && walk.state.depth > 0 {
                    Flow::Found(walk.json_end)
                } else {
                    Flow::Stop
                });
            }
        }
        *position += 1;
        if walk.state.quoted {
            match byte {
                _ if walk.state.escaped => walk.state.escaped = false,
                b'\\' => walk.state.escaped = true,
                b'"' => {
                    walk.state.quoted = false;
                    walk.json_end = *position;
                }
                _ => {}
            }
            return Ok(Flow::Next);
        }
        if matches!(
            byte,
            b'{' | b'}' | b'[' | b']' | b':' | b',' | b'-' | b'+' | b'.'
        ) || byte.is_ascii_alphanumeric()
        {
            walk.json_end = *position;
        }
        if byte == b'"' {
            walk.state.quoted = true;
            // Past the failure, a call's member: the attempt's call body.
            // Outside any bracket, its object's `{` was lost (a cut special
            // token before it), and the walk closes it by its `}`.
            if *position > span.end
                && brackets.iter().any(|(opener, _)| *opener == b'{')
                && self.call_member(at)
            {
                governor.charge(Resource::Work, 8)?;
                walk.body = true;
                if walk.state.depth == 0 {
                    walk.state.depth = walk.open.push(b'{')?;
                }
            }
        } else if brackets.iter().any(|(opener, _)| *opener == byte) {
            walk.state.depth = walk.open.push(byte)?;
        } else if let Some(&(kind, _)) = brackets.iter().find(|(_, close)| *close == byte) {
            // A bracket closing nothing the walk opened closes a structure
            // around the attempt (it began inside one, or inside a string):
            // the attempt's markup ends before it, unclosed.
            if walk.state.depth == 0 {
                return Ok(Flow::Stop);
            }
            walk.state.depth = walk.open.close(kind, governor)?;
            if walk.state.depth == 0 && *position > span.end {
                return Ok(Flow::Found(*position));
            }
        }
        Ok(Flow::Next)
    }

    /// Whether the bytes of `range` (a call inside a rejected attempt's
    /// markup), nesting by `brackets` outside strings from `state` (its
    /// open brackets by kind in `open`, paired by kind), close the
    /// attempt's last open bracket.
    fn closes(
        &self,
        state: &mut Nesting,
        open: &mut Brackets,
        brackets: &[(u8, u8)],
        range: &Range<usize>,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        governor.charge(Resource::Work, size(range.len()))?;
        for &byte in &self.output[range.clone()] {
            if state.quoted {
                match byte {
                    _ if state.escaped => state.escaped = false,
                    b'\\' => state.escaped = true,
                    b'"' => state.quoted = false,
                    _ => {}
                }
            } else if byte == b'"' {
                state.quoted = true;
            } else if brackets.iter().any(|(opener, _)| *opener == byte) {
                state.depth = open.push(byte)?;
            } else if let Some(&(kind, _)) = brackets.iter().find(|(_, close)| *close == byte)
                && state.depth > 0
            {
                state.depth = open.close(kind, governor)?;
                if state.depth == 0 {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// `end`, moved past the closers of `markup`'s alternative repeated
    /// right after it (whitespace between, each possibly with one slip),
    /// before the next segment in `boundaries`.
    fn stray_closers(
        &self,
        mut end: usize,
        markup: &Markup<'_>,
        boundaries: &[(Range<usize>, bool)],
    ) -> usize {
        let closers = &markup.closers;
        let limit = boundaries
            .iter()
            .map(|(range, _)| range.start)
            .find(|start| *start >= end)
            .unwrap_or(self.output.len());
        let mut probe = end;
        loop {
            while probe < limit && self.output[probe].is_ascii_whitespace() {
                probe += 1;
            }
            let exact = closers
                .iter()
                .filter(|spelling| self.output[probe..].starts_with(spelling))
                .map(Vec::len)
                .max();
            match exact.or_else(|| self.misspelled(closers, markup, probe, limit)) {
                Some(length) if probe + length <= limit => {
                    probe += length;
                    end = probe;
                }
                _ => break,
            }
        }
        end
    }

    /// The length of a token at `position` (before `limit`) that misspells
    /// one of `spellings` and no other closer of the dialect (one edit, as
    /// `tool_call.markup.closer-misspelled` reads a closer of a call): a
    /// rejected attempt's markup ends at its misspelled closer as a call's
    /// does.
    fn misspelled(
        &self,
        spellings: &[Vec<u8>],
        markup: &Markup<'_>,
        position: usize,
        limit: usize,
    ) -> Option<usize> {
        let rest = &self.output[position..limit.min(self.output.len())];
        let classes: Vec<&[Vec<u8>]> = markup
            .closers
            .iter()
            .chain(&markup.call_closers)
            .map(core::slice::from_ref)
            .collect();
        let classes = &classes;
        spellings
            .iter()
            .filter(|spelling| spelling.len() > 1 && rest.first() == spelling.first())
            .flat_map(|spelling| {
                (spelling.len().saturating_sub(1)..=spelling.len() + 1)
                    .filter(|&length| length <= rest.len())
                    .filter(move |&length| {
                        let token = &rest[..length];
                        crate::grammar::misspelling::misspells(token, spelling)
                            && classes
                                .iter()
                                .filter(|class| {
                                    class.iter().any(|other| {
                                        other.as_slice() == token
                                            || crate::grammar::misspelling::misspells(token, other)
                                    })
                                })
                                .map(|class| class[0].as_slice())
                                .collect::<Vec<_>>()
                                .windows(2)
                                .all(|pair| pair[0] == pair[1])
                    })
            })
            .max()
    }

    /// Where a rejected attempt at `span` without its alternative's closer
    /// before the stop ends: at the end of the last literal that ends a
    /// call of its dialect (a chained call's own closer, another
    /// alternative's) before the next segment, when there is one, so no
    /// closer of the attempt is left as content; otherwise where its
    /// grammar stopped reading, never inside a literal it began. What
    /// follows is not shown to be its markup and stays content (prose
    /// after a broken opener is the answer).
    fn unclosed_end(
        &self,
        span: &Range<usize>,
        read: usize,
        markup: &Markup<'_>,
        boundaries: &[(Range<usize>, bool)],
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let limit = boundaries
            .iter()
            .map(|(range, _)| range.start)
            .find(|start| *start >= span.end)
            .unwrap_or(self.output.len());
        let base = self.complete_literal(span, markup.literals, limit, governor)?;
        let mut last = None;
        let mut position = base;
        while position < limit {
            governor.charge(Resource::Work, 1)?;
            let length = markup
                .call_closers
                .iter()
                .filter(|spelling| self.output[position..limit].starts_with(spelling))
                .map(Vec::len)
                .max()
                .or_else(|| self.misspelled(&markup.call_closers, markup, position, limit));
            if let Some(length) = length {
                position += length;
                last = Some(position);
            } else {
                position += 1;
            }
        }
        if let Some(last) = last {
            return Ok(last);
        }
        self.balance_brackets(span.start, read, base, limit, governor)
    }

    /// `end`, moved past a JSON value the markup from `start` left open: when
    /// the bytes `start..read` its grammar read (to where it stopped; a
    /// resync may read on to `end` over bytes it does not parse) open more
    /// brackets (`{`, `[`) than they close, outside JSON strings and paired
    /// by kind (a `}` closes the innermost open `{` and any bracket a slip
    /// left open inside it; it never closes a `[` alone), the end of the
    /// bracket that balances them before `limit` (an arguments object
    /// whose value no repair read, with no closer of the dialect after it),
    /// when it lies past `end` and no prose comes before it outside a
    /// string: a bare word (other than `true`, `false`, `null`) not followed
    /// by JSON punctuation (`,` `:` `}` `]` `"`) but by another word or by
    /// sentence punctuation is prose, which is no JSON structure. Otherwise
    /// `end`.
    fn balance_brackets(
        &self,
        start: usize,
        read: usize,
        end: usize,
        limit: usize,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let mut state = Nesting::default();
        let mut open = Brackets::new(governor);
        governor.charge(Resource::Work, size(read - start))?;
        for &byte in &self.output[start..read] {
            state.step(byte, &mut open, governor)?;
        }
        if state.depth == 0 {
            return Ok(end);
        }
        let mut position = read;
        while position < limit {
            governor.charge(Resource::Work, 1)?;
            let byte = self.output[position];
            // Only bytes JSON reads as structure, strings or scalars: a bare
            // word outside a string (prose after the value) is no part of
            // the value, so the bracket after it balances nothing the
            // attempt opened.
            if !state.quoted
                && (byte.is_ascii_alphabetic() || byte == b'_')
                && (position == 0 || !is_word_byte(self.output[position - 1]))
            {
                let (word, prose) = self.bare_word(position, limit);
                if prose {
                    return Ok(end);
                }
                governor.charge(Resource::Work, size(word))?;
                position += word;
                continue;
            }
            state.step(byte, &mut open, governor)?;
            position += 1;
            if state.depth == 0 {
                return Ok(position.max(end));
            }
        }
        Ok(end)
    }

    /// The length of the bare word (outside a JSON string) at `position`,
    /// before `limit`, and whether it is prose. A word begins at a letter
    /// or `_` (`_dir`) that no letter, digit or `_` precedes, and runs over
    /// letters, digits and `_`. A slip in JSON (`"f" oops,
    /// "arguments": ...`) is a word where a JSON token is due: what follows
    /// it is JSON punctuation (`,` `:` `}` `]` `"`, after spaces) or the
    /// end. A word (other than `true`, `false`, `null`) followed by another
    /// word or by sentence punctuation (`oops. Sorry`) is prose, which is no
    /// JSON structure.
    fn bare_word(&self, position: usize, limit: usize) -> (usize, bool) {
        let word = self.output[position..limit]
            .iter()
            .take_while(|byte| is_word_byte(**byte))
            .count();
        let after = self.output[position + word..limit]
            .iter()
            .find(|byte| !matches!(byte, b' ' | b'\t'));
        let literal = matches!(
            &self.output[position..position + word],
            b"true" | b"false" | b"null"
        );
        // The tail of a special token (`t_begin|>` of a cut
        // `<|tool_call_begin|>`, a DeepSeek `｜`) is markup, not prose.
        let rest = &self.output[position + word..limit];
        let fragment = rest.starts_with(b"|") || rest.starts_with("｜".as_bytes());
        let prose = !literal
            && !fragment
            && !matches!(after, None | Some(b',' | b':' | b'}' | b']' | b'"'));
        (word, prose)
    }

    /// Whether a JSON member naming a call (`"name"`, then `:`) begins at
    /// `at`: a call object's body, which the scan reads as markup of the
    /// attempt even when its opening `{` was lost (a cut special token
    /// before it).
    fn call_member(&self, at: usize) -> bool {
        let rest = &self.output[at..];
        rest.starts_with(b"\"name\"")
            && rest[6..]
                .iter()
                .find(|byte| !byte.is_ascii_whitespace())
                .is_some_and(|byte| *byte == b':')
    }

    /// `span`'s end, moved to the end of the longest literal spelling that
    /// began inside the span and runs on past its end (within `limit`): the
    /// grammar may stop reading inside a literal (a closer variant is a
    /// prefix of the canonical spelling), and a rejected span never splits
    /// one.
    fn complete_literal(
        &self,
        span: &Range<usize>,
        literals: &[ScratchVec<u8>],
        limit: usize,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let mut end = span.end;
        for spelling in literals {
            governor.charge(Resource::Work, size(spelling.len()))?;
            for inside in 1..spelling.len() {
                let Some(start) = span.end.checked_sub(inside) else {
                    break;
                };
                if start < span.start {
                    break;
                }
                let stop = start + spelling.len();
                if stop <= limit && self.output[start..].starts_with(spelling) {
                    end = end.max(stop);
                }
            }
        }
        Ok(end)
    }
}

/// What ends a rejected attempt's markup (see
/// [`Recovery::close_rejections`]).
struct Markup<'g> {
    /// The opener spellings of the alternative it opened.
    openers: Vec<Vec<u8>>,
    /// That alternative's end literals.
    closers: Vec<Vec<u8>>,
    /// Whether a repeated opener nests a level.
    nests: bool,
    /// Every multi-byte literal that ends a call of the dialect.
    call_closers: Vec<Vec<u8>>,
    /// Every literal spelling of the dialect's `call` production.
    literals: &'g [ScratchVec<u8>],
}

/// Whether `byte` continues a bare word (an ASCII letter, digit or `_`).
const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Where a JSON-object attempt's markup scan stands (see
/// `MarkupScan::markup_end`): its nesting, the brackets it opened by kind,
/// whether it read a call's member past the failure (the attempt's call
/// body), and where the last byte JSON reads ends.
struct Scan {
    state: Nesting,
    open: Brackets,
    body: bool,
    json_end: usize,
}

/// What one byte of a markup scan decides (see `MarkupScan::bracket_step`).
enum Flow {
    /// Read on.
    Next,
    /// The attempt's markup ends here.
    Found(usize),
    /// The scan stops with no end found.
    Stop,
}

/// The open brackets of a rejected attempt's markup scan by kind (`{` or
/// `[`), innermost last, and how many of each kind are open.
struct Brackets {
    open: ScratchVec<u8>,
    objects: usize,
    lists: usize,
}

impl Brackets {
    fn new(governor: &Governor) -> Self {
        Self {
            open: ScratchVec::new(governor),
            objects: 0,
            lists: 0,
        }
    }

    /// The count of open brackets of `kind`'s kind.
    const fn count(&mut self, kind: u8) -> &mut usize {
        if kind == b'{' {
            &mut self.objects
        } else {
            &mut self.lists
        }
    }

    /// Open a bracket of `kind` (`{` or `[`), returning how many are open.
    fn push(&mut self, kind: u8) -> Result<usize, StopReason> {
        self.open.push(kind)?;
        *self.count(kind) += 1;
        Ok(self.open.len())
    }

    /// Close the innermost open bracket of `kind` (and every bracket opened
    /// inside it, which a slip left unclosed: `{"a": [1}` closes the
    /// object), returning how many stay open. With none of its kind open,
    /// the closer is a slip inside the markup (a stray `}` in a list never
    /// closes the list's `[`) and closes nothing. `Work` per bracket
    /// closed, each paid once for its opening.
    fn close(&mut self, kind: u8, governor: &mut Governor) -> Result<usize, StopReason> {
        if *self.count(kind) == 0 {
            return Ok(self.open.len());
        }
        while let Some(opener) = self.open.pop() {
            governor.charge(Resource::Work, 1)?;
            *self.count(opener) -= 1;
            if opener == kind {
                break;
            }
        }
        Ok(self.open.len())
    }
}

/// Where a scan of JSON-like bytes stands: its bracket depth and whether
/// it is inside a string (just after an escape).
#[derive(Default)]
struct Nesting {
    depth: usize,
    quoted: bool,
    escaped: bool,
}

impl Nesting {
    /// Read one more byte, its brackets paired by kind in `open` (see
    /// [`Brackets::close`]: a `}` never closes a `[`), `depth` of them open.
    fn step(
        &mut self,
        byte: u8,
        open: &mut Brackets,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if self.quoted {
            match byte {
                _ if self.escaped => self.escaped = false,
                b'\\' => self.escaped = true,
                b'"' => self.quoted = false,
                _ => {}
            }
        } else {
            match byte {
                b'"' => self.quoted = true,
                b'{' | b'[' => self.depth = open.push(byte)?,
                b'}' => self.depth = open.close(b'{', governor)?,
                b']' => self.depth = open.close(b'[', governor)?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// Remove `cut` from the sorted, disjoint `ranges`.
fn remove_range(ranges: &mut Vec<Range<usize>>, cut: &Range<usize>) {
    let mut kept = Vec::with_capacity(ranges.len() + 1);
    for range in ranges.drain(..) {
        if range.end <= cut.start || range.start >= cut.end {
            kept.push(range);
            continue;
        }
        if range.start < cut.start {
            kept.push(range.start..cut.start);
        }
        if range.end > cut.end {
            kept.push(cut.end..range.end);
        }
    }
    *ranges = kept;
}

/// `reason`, made precise: an attempt that failed at a byte beginning a
/// malformed UTF-8 sequence (a tool name holding such bytes, which no
/// built-in name grammar admits) failed because of it.
fn precise(reason: Rejection, output: &[u8]) -> Rejection {
    let Rejection::Malformed { at } = reason else {
        return reason;
    };
    let rest = output.get(at..).unwrap_or_default();
    let head = &rest[..rest.len().min(4)];
    match core::str::from_utf8(head) {
        Err(error)
            if error.valid_up_to() == 0
                && (error.error_len().is_some() || head.len() == rest.len()) =>
        {
            Rejection::InvalidUtf8 { at }
        }
        _ => reason,
    }
}

fn rejection_node(reason: Rejection) -> Node {
    let (code, at) = match reason {
        Rejection::Malformed { at } => ("malformed", Some(at)),
        Rejection::PythonicPositionalRejected { at } => ("pythonic_positional_rejected", Some(at)),
        Rejection::InvalidUtf8 { at } => ("invalid_utf8", Some(at)),
        Rejection::Unterminated => ("unterminated", None),
        Rejection::Overlapped { at } => ("overlapped", Some(at)),
        Rejection::UnsupportedDialect { family } => {
            return object([
                ("code", string("unsupported_dialect")),
                ("at", null()),
                ("family", string(family.id())),
                ("built_in", Node::new(Value::Bool(family.built_in()))),
                ("detail", string(family.message())),
            ]);
        }
    };
    object([
        ("code", string(code)),
        (
            "at",
            at.map_or_else(null, |at| integer(u64::try_from(at).unwrap_or(u64::MAX))),
        ),
    ])
}

impl WireValue for ToolCall {
    /// `{index, id, source_id, tool, name, arguments, span: {start, end},
    /// dialect, candidate_dialects, effects, policy, outcome: {status, reason}, repairs:
    /// {evidence, losses (each with its `kind`), inferences}, alternatives,
    /// distinguishing: [{kind, rule}], keywords: [{pointer, keyword,
    /// used}]}`; absent values are `null`.
    fn to_node(&self) -> Node {
        object([
            ("index", integer(size(self.index))),
            ("id", string(&self.id)),
            ("source_id", optional_string(self.source_id.as_deref())),
            ("tool", optional_string(self.tool.as_deref())),
            ("name", optional_string(self.name.as_deref())),
            ("arguments", self.arguments.clone()),
            ("span", byte_range(&self.span)),
            ("dialect", string(self.dialect.id())),
            (
                "candidate_dialects",
                array(
                    self.candidate_dialects
                        .iter()
                        .map(|dialect| string(dialect.id())),
                ),
            ),
            ("effects", effects_code(self.effects)),
            ("policy", policy_code(self.policy)),
            ("outcome", outcome_node(&self.outcome)),
            ("repairs", ledger_node(&self.repairs)),
            (
                "alternatives",
                array(self.alternatives.iter().map(reading_node)),
            ),
            (
                "distinguishing",
                array(self.distinguishing.iter().map(provenance_node)),
            ),
            (
                "keywords",
                array(self.keywords.iter().map(|keyword| {
                    object([
                        ("pointer", string(&keyword.pointer)),
                        ("keyword", string(&keyword.keyword)),
                        ("used", Node::new(Value::Bool(keyword.used))),
                    ])
                })),
            ),
        ])
    }
}

impl WireValue for ToolCalls {
    /// `{calls, rejected: [{span, dialect (null for a family no built-in
    /// dialect parses), candidate_dialects, reason: {code, at, and for
    /// unsupported_dialect also family, built_in, detail}, raw: {text |
    /// bytes_hex}, repairs}], embedded, content: [{start, end}], reasoning:
    /// [{start, end}], reasoning_blocks: [{span: {start, end}, text: {start,
    /// end}}] (one per block, never merged), finish, openai_finish_reason, tool_choice: {constraint,
    /// passed, diagnostics}, next_id_start, verdict, usable_calls,
    /// usable_calls_accepting_guesses}` (the [`Verdict`] code and the
    /// counts of [`ToolCalls::usable`] and
    /// [`ToolCalls::usable_accepting_guesses`]), each call and embedded call
    /// as [`ToolCall`]'s wire form, plus an `emitted` member (see
    /// [`Emission`]) only when an emission was requested, so results of
    /// requests without one keep their wire form. Deterministic and
    /// infallible.
    fn to_node(&self) -> Node {
        let ranges = |ranges: &[Range<usize>]| array(ranges.iter().map(byte_range));
        let mut node = object([
            ("calls", array(self.calls.iter().map(WireValue::to_node))),
            (
                "rejected",
                array(self.rejected.iter().map(|rejected| {
                    object([
                        ("span", byte_range(&rejected.span)),
                        (
                            "dialect",
                            rejected
                                .dialect
                                .as_ref()
                                .map_or_else(null, |dialect| string(dialect.id())),
                        ),
                        (
                            "candidate_dialects",
                            array(
                                rejected
                                    .candidate_dialects
                                    .iter()
                                    .map(|dialect| string(dialect.id())),
                            ),
                        ),
                        ("reason", rejection_node(rejected.reason)),
                        ("raw", object([exact_bytes(&rejected.raw)])),
                        ("repairs", ledger_node(&rejected.repairs)),
                    ])
                })),
            ),
            (
                "embedded",
                array(self.embedded.iter().map(WireValue::to_node)),
            ),
            ("content", ranges(&self.content)),
            ("reasoning", ranges(&self.reasoning)),
            (
                "reasoning_blocks",
                array(self.reasoning_blocks.iter().map(|block| {
                    object([
                        ("span", byte_range(&block.span)),
                        ("text", byte_range(&block.text)),
                    ])
                })),
            ),
            ("finish", string(self.finish.code())),
            ("openai_finish_reason", string(self.openai_finish_reason())),
            (
                "tool_choice",
                object([
                    ("constraint", string(self.tool_choice.choice.constraint())),
                    ("passed", Node::new(Value::Bool(self.tool_choice.passed))),
                    (
                        "diagnostics",
                        array(self.tool_choice.diagnostics.iter().map(string)),
                    ),
                ]),
            ),
            ("next_id_start", integer(self.next_id_start)),
            ("verdict", string(self.verdict().code())),
            ("usable_calls", integer(size(self.usable().len()))),
            (
                "usable_calls_accepting_guesses",
                integer(size(self.usable_accepting_guesses().len())),
            ),
        ]);
        if let (Some(emitted), Value::Object(members)) = (&self.emitted, &mut node.value) {
            members.push(crate::json::Member {
                key: "emitted".into(),
                key_span: None,
                value: emitted.to_node(),
            });
        }
        node
    }
}

impl ToolSet {
    /// Recover the tool calls of one model turn under a fresh governor with
    /// `request.budget`, borrowing the explicit prepared `dialects` set.
    /// Use [`DialectSet::built_in`] for the default built-in selection; custom
    /// sets remain owned by their caller and may be reused across settings.
    ///
    /// See the [module documentation](super::recover) for every step and the
    /// outcome rules; [`Self::recover_governed`] has the error semantics.
    ///
    /// The motivating case: a Qwen3-Coder call whose body is only stray
    /// closers. With no required parameter the closers are markup, dropped
    /// with one itemized markup loss, and the call is `Resolved` as `f {}`;
    /// with a required parameter nothing may be invented, so the call is
    /// `Unresolved` and the turn's finish says so.
    ///
    /// The outcome's status is the certainty of the *reading of the turn*,
    /// as in every operation, not a verdict on the calls: the second turn
    /// below is read with certainty (`Resolved`) and has nothing to run.
    /// [`ToolCalls::verdict`] (wire `value.verdict`) is the usability
    /// verdict; see [`Verdict`] for what a caller does with each.
    ///
    /// ```
    /// use purrassist_core::tool_call::{CallOutcome, Finish, Param, ToolDefinition, ToolRecoveryRequest, ToolSet, UnresolvedReason};
    /// use purrassist_core::grammar::edit::LossKind;
    /// use purrassist_core::{Budget, Outcome};
    /// let output = b"<tool_call><function=f></parameters></parameters></function></tool_call>";
    /// let request = ToolRecoveryRequest::default();
    ///
    /// let tools = ToolSet::compile([ToolDefinition::new("f").param("path", Param::string())], Budget::default()).unwrap();
    /// let result = tools.recover(output, &request, purrassist_core::tool_call::DialectSet::built_in());
    /// assert_eq!(result.receipt.operation, "tool_call.recover.v1");
    /// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
    /// let call = &calls.calls[0];
    /// assert_eq!((call.tool.as_deref(), call.arguments.to_json().as_str()), (Some("f"), "{}"));
    /// assert_eq!(call.outcome, CallOutcome::Resolved);
    /// assert_eq!(call.repairs.losses.len(), 1);
    /// assert_eq!(call.repairs.losses[0].kind, LossKind::Markup);
    /// assert_eq!(result.receipt.losses, [call.repairs.losses[0].loss.clone()]);
    /// assert_eq!((calls.finish, calls.openai_finish_reason()), (Finish::Calls, "tool_calls"));
    /// assert_eq!(calls.usable().len(), 1);
    ///
    /// let strict = ToolSet::compile([ToolDefinition::new("f").param("path", Param::string().required())], Budget::default()).unwrap();
    /// let result = strict.recover(output, &request, purrassist_core::tool_call::DialectSet::built_in());
    /// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
    /// assert!(matches!(calls.calls[0].outcome, CallOutcome::Unresolved { reason: UnresolvedReason::MissingRequired { .. } }));
    /// assert_eq!(calls.finish, Finish::Unresolved);
    /// assert!(calls.usable().is_empty());
    /// assert_eq!(calls.verdict(), purrassist_core::tool_call::Verdict::NothingUsable);
    /// ```
    #[must_use]
    pub fn recover(
        &self,
        output: &[u8],
        request: &ToolRecoveryRequest,
        dialects: &DialectSet,
    ) -> AssistResult<ToolCalls> {
        self.recover_governed(
            output,
            request,
            dialects,
            &mut Governor::new(request.budget),
        )
    }

    /// Recover the tool calls of one model turn against a caller-owned
    /// governor, whose limits are authoritative (`request.budget` is not
    /// consulted; the receipt records the governor's limits and cumulative
    /// usage).
    ///
    /// Never panics and never returns a partial value: exhaustion,
    /// cancellation, and an invalid request are an `Unresolved` outcome with
    /// [`Receipt::stopped`] set to [`StopReason::BudgetExceeded`],
    /// [`StopReason::Cancelled`], or [`StopReason::InvalidInput`] (an
    /// effects override or a forced tool choice naming an undeclared tool,
    /// or an [`IdGenerator`] returning one id twice), with empty ledgers.
    /// The receipt always names the complete supplied output's digest.
    /// Source metadata is admitted before request validation and recognizer
    /// construction. Each byte then admits input, source copying, encoding
    /// classification and recognition in that order. A semantic request
    /// refusal stops interpretation while source capture continues under the
    /// same healthy governor. A governor or capture refusal stops copying and
    /// admission; remaining bytes contribute only to the identity. The receipt
    /// retains either the complete admitted source or no source, including
    /// when a later stage refuses after full capture.
    ///
    /// ```
    /// use purrassist_core::tool_call::{Effects, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, CancellationToken, Governor, Outcome, StopReason};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)], Budget::default()).unwrap();
    /// let token = CancellationToken::new();
    /// let mut governor = Governor::with_cancellation(Budget::default(), token.clone());
    /// token.cancel();
    /// let result = tools.recover_governed(b"<tool_call>{\"name\": \"ls\", \"arguments\": {}}</tool_call>", &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in(), &mut governor);
    /// assert_eq!(result.outcome, Outcome::Unresolved);
    /// assert_eq!(result.receipt.stopped, StopReason::Cancelled);
    /// assert!(result.receipt.sources.is_empty());
    ///
    /// let request = ToolRecoveryRequest::default().effects("rm", Effects::ReadOnly);
    /// let input = b"original output retained despite invalid request";
    /// let result = tools.recover_governed(input, &request, purrassist_core::tool_call::DialectSet::built_in(), &mut Governor::new(Budget::default()));
    /// assert!(matches!(result.receipt.stopped, StopReason::InvalidInput(_)));
    /// assert_eq!(result.receipt.sources[0].bytes.as_slice(), input.as_slice());
    /// ```
    #[must_use]
    pub fn recover_governed(
        &self,
        output: &[u8],
        request: &ToolRecoveryRequest,
        dialects: &DialectSet,
        governor: &mut Governor,
    ) -> AssistResult<ToolCalls> {
        let mut intake = RecoveryIntake::new(self, request, dialects, governor);
        intake.feed(output, governor);
        intake.finish(request, governor)
    }
}

/// One admitted source owner. Its publication vector is reserved before intake,
/// so publishing a complete source after a later refusal needs no allocation.
struct SourceCapture {
    bytes: ScratchVec<u8>,
    documents: Vec<SourceDocument>,
    metadata: ScratchLease,
}

impl SourceCapture {
    fn new(governor: &mut Governor) -> Result<Self, StopReason> {
        let metadata = governor.lease_scratch(128)?;
        let mut documents = Vec::new();
        documents
            .try_reserve_exact(1)
            .map_err(|error| reserve_failure(&error))?;
        Ok(Self {
            bytes: ScratchVec::new(governor),
            documents,
            metadata,
        })
    }

    fn publish(mut self, digest: Digest) -> Vec<SourceDocument> {
        self.documents.push(SourceDocument {
            digest,
            bytes: self.bytes.into_vec(),
        });
        self.metadata.retain();
        self.documents
    }
}

/// Incremental equivalent of `utf8_chunks`, retaining only invalid-run ranges.
/// At most three incomplete scalar bytes survive a byte step.
struct EncodingCursor {
    pending: [u8; 4],
    len: usize,
    start: usize,
    bad: Option<Range<usize>>,
    invalid: ScratchVec<(usize, usize)>,
}

impl EncodingCursor {
    fn new(governor: &Governor) -> Self {
        Self {
            pending: [0; 4],
            len: 0,
            start: 0,
            bad: None,
            invalid: ScratchVec::new(governor),
        }
    }

    fn flush_bad(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        if let Some(range) = self.bad.take() {
            push_scratch(&mut self.invalid, (range.start, range.end), governor)?;
        }
        Ok(())
    }

    fn mark_bad(&mut self, end: usize) {
        let start = self.bad.as_ref().map_or(self.start, |range| range.start);
        self.bad = Some(start..end);
    }

    fn step(&mut self, byte: u8, at: usize, governor: &mut Governor) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        if self.len == 0 {
            self.start = at;
        }
        self.pending[self.len] = byte;
        self.len += 1;
        loop {
            match core::str::from_utf8(&self.pending[..self.len]) {
                Ok(_) => {
                    self.flush_bad(governor)?;
                    self.len = 0;
                    return Ok(());
                }
                Err(error) => {
                    let Some(width) = error.error_len() else {
                        return Ok(());
                    };
                    // The cursor starts at one scalar: a valid scalar was
                    // flushed on its final byte, so no valid prefix precedes it.
                    debug_assert_eq!(error.valid_up_to(), 0);
                    self.mark_bad(self.start + width);
                    self.pending.copy_within(width..self.len, 0);
                    self.len -= width;
                    self.start += width;
                    if self.len == 0 {
                        return Ok(());
                    }
                }
            }
        }
    }

    fn finish(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        if self.len != 0 {
            self.mark_bad(self.start + self.len);
            self.len = 0;
        }
        self.flush_bad(governor)
    }
}

struct IntakeSetup<'s> {
    recognizer: Recognizer<'s>,
    policies: Policies,
    forced: Option<String>,
}

struct RecognizedInput {
    turn: super::RecognizedTurn,
    encoding: CallLedger,
    policies: Policies,
    forced: Option<String>,
}

/// The two independently normalized source cutoffs.
#[derive(Clone, Copy)]
enum BoundaryKind {
    Prefix,
    Content,
}

/// Shared byte intake for batch recovery and the live stream facade.
/// It owns source storage without lending it to its live recognizer. Every
/// method uses the original governor. Interpretation failure does not discard
/// admissible source; capture refusal stops admission but never source identity.
pub(super) struct RecoveryIntake<'s> {
    tools: &'s ToolSet,
    dialects: &'s DialectSet,
    receipt: Receipt,
    digest: DigestBuilder,
    received: usize,
    capture: Option<SourceCapture>,
    encoding: EncodingCursor,
    setup: Option<IntakeSetup<'s>>,
    failed: Option<StopReason>,
    progress: ContinuationFrontier,
    /// Successful normalization of each UTF-8-capped cutoff. Source appends
    /// cannot change a scalar boundary already behind the encoding cursor.
    prefix_boundary: Option<(usize, usize)>,
    content_boundary: Option<(usize, usize)>,
    /// Only the contiguous native content prefix is published so far. Later
    /// content can still be removed by rejection/stray-closer normalization.
    content_end: usize,
    events: EventCursor,
}

impl<'s> RecoveryIntake<'s> {
    pub(super) fn new(
        tools: &'s ToolSet,
        request: &ToolRecoveryRequest,
        dialects: &'s DialectSet,
        governor: &mut Governor,
    ) -> Self {
        let mut intake = Self {
            tools,
            dialects,
            receipt: Receipt::new(&[], RECOVER_OPERATION, request.effort, *governor.limits()),
            digest: DigestBuilder::new(),
            received: 0,
            capture: None,
            encoding: EncodingCursor::new(governor),
            setup: None,
            failed: None,
            progress: ContinuationFrontier::default(),
            prefix_boundary: None,
            content_boundary: None,
            content_end: 0,
            events: EventCursor::new(governor),
        };
        let result = (|| {
            intake.capture = Some(SourceCapture::new(governor)?);
            let recovery = Recovery {
                tools,
                request,
                dialects,
                output: &[],
                source: intake.receipt.source,
                policies: Policies {
                    selection: request.policy,
                    effects: request.effects.clone(),
                    exact: request.effort == Effort::Exact,
                },
            };
            let forced = recovery.preconditions(governor)?;
            intake.setup = Some(IntakeSetup {
                recognizer: Recognizer::with_tools(dialects, tools, request.reasoning, governor)?,
                policies: recovery.policies,
                forced,
            });
            Ok(())
        })();
        if let Err(reason) = result {
            intake.failed = Some(reason);
        }
        intake
    }

    /// Hash all supplied bytes, even after refusal. An interpretation error
    /// leaves source capture independent; capture or governor failure stops
    /// admission without retrying an allocation or publishing a retained prefix.
    pub(super) fn feed(&mut self, bytes: &[u8], governor: &mut Governor) {
        self.feed_with(bytes, governor, &mut |_| {}, &mut |_| Ok(()));
    }

    /// Native frontier notifications use the same byte fold and event charges
    /// as batch's discard sink. Live content currently covers only the initial
    /// native run; shared native segments also produce call and reasoning facts.
    /// The source range and copied raw payload have byte-derived boundaries,
    /// independent of the caller's feed boundaries. The payload is admitted and
    /// built under temporary ownership through sink handoff, including batch's
    /// discard sink. Producer reservations release after the sink returns;
    /// caller retention cannot affect recovery resource usage.
    pub(super) fn feed_with(
        &mut self,
        bytes: &[u8],
        governor: &mut Governor,
        progress: &mut impl FnMut(ContinuationFrontier),
        content: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) {
        self.digest.update(bytes);
        for &byte in bytes {
            if self.capture.is_none() {
                continue;
            }
            if self
                .failed
                .as_ref()
                .is_some_and(|reason| !matches!(reason, StopReason::InvalidInput(_)))
            {
                self.capture = None;
                continue;
            }
            if let Err(reason) = self.capture_byte(byte, governor) {
                // Source refusal takes precedence over a semantic request
                // error: the complete input could not be retained.
                self.capture = None;
                self.setup = None;
                self.failed = Some(reason);
                continue;
            }
            if self.failed.is_none()
                && let Err(reason) = self
                    .interpret_byte(byte, governor)
                    .and_then(|()| self.report_progress(governor, progress, content))
            {
                self.setup = None;
                self.failed = Some(reason);
            }
        }
    }

    fn report_progress(
        &mut self,
        governor: &mut Governor,
        progress: &mut impl FnMut(ContinuationFrontier),
        content: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        let mut frontier = self
            .setup
            .as_ref()
            .expect("live interpretation owns a recognizer")
            .recognizer
            .continuation_frontier();
        let utf8 = if self.encoding.len == 0 {
            self.received
        } else {
            self.encoding.start
        };
        let prefix = frontier.prefix_end.min(utf8);
        frontier.prefix_end =
            self.cached_source_boundary(prefix, BoundaryKind::Prefix, governor)?;
        frontier.content = match frontier.content {
            Some((start, end)) => {
                let end = end.min(utf8);
                let end = if end == prefix {
                    frontier.prefix_end
                } else {
                    self.cached_source_boundary(end, BoundaryKind::Content, governor)?
                };
                (start < end).then_some((start, end))
            }
            None => None,
        };
        if frontier != self.progress {
            governor.charge(Resource::Work, K_EVENT)?;
            let delta = self.content_delta(frontier, governor)?;
            self.progress = frontier;
            progress(frontier);
            if let Some(delta) = delta {
                self.content_end = delta.content_end(governor)?;
                delta.publish(content)?;
            }
        }
        let source = self
            .capture
            .as_ref()
            .expect("live event production owns admitted source")
            .bytes
            .as_slice();
        self.events.advance(
            &self
                .setup
                .as_ref()
                .expect("live event production owns its recognizer")
                .recognizer,
            source,
            self.tools,
            frontier.prefix_end,
            governor,
            content,
        )?;
        Ok(())
    }

    /// Cache only cutoffs already capped before a viable incomplete scalar.
    /// Future input cannot complete a scalar across such a cutoff: malformed
    /// subparts stay malformed, and complete source scalars are append-only.
    fn cached_source_boundary(
        &mut self,
        end: usize,
        kind: BoundaryKind,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        governor.charge(Resource::Work, 1)?;
        let cached = match kind {
            BoundaryKind::Prefix => self.prefix_boundary,
            BoundaryKind::Content => self.content_boundary,
        };
        if let Some((known, normalized)) = cached
            && known == end
        {
            return Ok(normalized);
        }
        let normalized = self.source_boundary(end, governor)?;
        let cached = Some((end, normalized));
        match kind {
            BoundaryKind::Prefix => self.prefix_boundary = cached,
            BoundaryKind::Content => self.content_boundary = cached,
        }
        Ok(normalized)
    }

    /// A lexical holdback can cut an earlier, already complete scalar even
    /// when the encoding cursor has no pending bytes. Inspect only the scalar
    /// crossing this cutoff; invalid sequences retain their original boundary.
    fn source_boundary(&self, end: usize, governor: &mut Governor) -> Result<usize, StopReason> {
        // A constant bound covers the cutoff lookup, at most three backward
        // byte probes, leading-byte classification and validation of four bytes.
        governor.charge(Resource::Work, 12)?;
        let source = self
            .capture
            .as_ref()
            .map(|capture| capture.bytes.as_slice())
            .ok_or_else(|| invalid_input("content frontier has no admitted source"))?;
        if end > source.len() {
            return Err(invalid_input("content frontier exceeds admitted source"));
        }
        if end == 0 || source.get(end).is_none_or(|byte| byte & 0xc0 != 0x80) {
            return Ok(end);
        }
        let mut start = end;
        for _ in 0..3 {
            start -= 1;
            let first = source[start];
            if first & 0xc0 != 0x80 {
                let width = match first {
                    0xc2..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf4 => 4,
                    _ => return Ok(end),
                };
                let Some(scalar) = source.get(start..start.saturating_add(width)) else {
                    return Ok(end);
                };
                return Ok(
                    if start.saturating_add(width) > end && core::str::from_utf8(scalar).is_ok() {
                        start
                    } else {
                        end
                    },
                );
            }
            if start == 0 {
                break;
            }
        }
        Ok(end)
    }

    /// Initial content precedes every native call, attempt and reasoning block.
    /// Final recovery can only add unsupported-markup reports over these bytes:
    /// those reports keep their bytes as content. Rejection extension and stray
    /// closer removal start after a structural segment, so later native content
    /// is deliberately withheld until those owners have an incremental form.
    fn content_delta(
        &self,
        frontier: ContinuationFrontier,
        governor: &mut Governor,
    ) -> Result<Option<PreparedEvent>, StopReason> {
        let Some((0, end)) = frontier.content else {
            return Ok(None);
        };
        if end <= self.content_end {
            return Ok(None);
        }
        let span = self.content_end..end;
        let source = self
            .capture
            .as_ref()
            .and_then(|capture| capture.bytes.as_slice().get(span.clone()))
            .ok_or_else(|| invalid_input("content frontier exceeds admitted source"))?;
        governor.charge(Resource::Work, K_EVENT.saturating_add(size(source.len())))?;
        let mut raw = ScratchVec::new(governor);
        raw.reserve(source.len())?;
        raw.extend(source.iter().copied())?;
        StreamEvent::content(span, raw, governor).map(Some)
    }

    fn capture_byte(&mut self, byte: u8, governor: &mut Governor) -> Result<(), StopReason> {
        // A sticky governor refusal is observed before any further charge or
        // copy, including when interpretation failed on the preceding byte.
        governor.check()?;
        let Some(received) = self.received.checked_add(1) else {
            self.capture = None;
            return Err(invalid_input(
                "tool-call source length exceeds the native offset domain",
            ));
        };
        self.received = received;
        governor.charge(Resource::InputBytes, size(self.received))?;
        let capture = self
            .capture
            .as_mut()
            .expect("capture admission requires an existing source owner");
        push_scratch(&mut capture.bytes, byte, governor)
    }

    fn interpret_byte(&mut self, byte: u8, governor: &mut Governor) -> Result<(), StopReason> {
        self.encoding.step(byte, self.received - 1, governor)?;
        self.setup
            .as_mut()
            .expect("healthy intake owns a recognizer")
            .recognizer
            .push(core::slice::from_ref(&byte), governor)
    }

    fn recognize(
        &mut self,
        request: &ToolRecoveryRequest,
        governor: &mut Governor,
    ) -> Result<RecognizedInput, StopReason> {
        if let Some(reason) = &self.failed {
            return Err(reason.clone());
        }
        governor.check()?;
        self.encoding.finish(governor)?;
        let IntakeSetup {
            recognizer,
            policies,
            forced,
        } = self.setup.take().expect("healthy intake owns a recognizer");
        let output = self
            .capture
            .as_ref()
            .expect("healthy intake owns source storage")
            .bytes
            .as_slice();
        let recovery = Recovery {
            tools: self.tools,
            request,
            dialects: self.dialects,
            output,
            source: self.receipt.source,
            policies,
        };
        let encoding = recovery.encoding(self.encoding.invalid.as_slice(), governor)?;
        let turn = recognizer.finish_over(request.stop.clone(), Some(output), governor)?;
        Ok(RecognizedInput {
            turn,
            encoding,
            policies: recovery.policies,
            forced,
        })
    }

    fn complete(
        &mut self,
        request: &ToolRecoveryRequest,
        governor: &mut Governor,
    ) -> Result<Done, StopReason> {
        let RecognizedInput {
            turn,
            encoding,
            policies,
            forced,
        } = self.recognize(request, governor)?;
        self.events.remember_final(&turn, governor)?;
        let recovery = Recovery {
            tools: self.tools,
            request,
            dialects: self.dialects,
            output: self
                .capture
                .as_ref()
                .expect("complete intake owns source storage")
                .bytes
                .as_slice(),
            source: self.receipt.source,
            policies,
        };
        let readings = recovery.resolve_turn(turn, &encoding, forced.as_deref(), governor)?;
        recovery.conclude(readings, governor)
    }

    pub(super) fn finish(
        self,
        request: &ToolRecoveryRequest,
        governor: &mut Governor,
    ) -> AssistResult<ToolCalls> {
        self.finish_with_events(request, governor, &mut |_| Ok(()))
    }

    pub(super) fn finish_with_events(
        mut self,
        request: &ToolRecoveryRequest,
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> AssistResult<ToolCalls> {
        self.receipt.source = core::mem::take(&mut self.digest).finish();
        let completed = self.complete(request, governor).and_then(|done| {
            if let Outcome::Resolved(calls) | Outcome::BestGuess(calls) = &done.outcome {
                let source = self
                    .capture
                    .as_ref()
                    .expect("completed recovery owns admitted source")
                    .bytes
                    .as_slice();
                self.events
                    .finish(calls, self.content_end, source, governor, sink)?;
            }
            Ok(done)
        });
        let outcome = match completed {
            Ok(done) => {
                self.receipt.evidence = done.evidence;
                self.receipt.losses = done.losses;
                self.receipt.inferences = done.inferences;
                self.receipt.validations = done.validations;
                self.receipt.alternatives = done.alternatives;
                self.receipt.stopped = done.stopped;
                done.outcome
            }
            Err(reason) => {
                self.receipt.stopped = reason;
                Outcome::Unresolved
            }
        };
        if let Some(capture) = self.capture.take() {
            self.receipt.sources = capture.publish(self.receipt.source);
        }
        self.receipt.usage = governor.usage();
        AssistResult {
            outcome,
            receipt: self.receipt,
        }
    }
}

/// A finished recovery's outcome and receipt ledgers.
struct Done {
    outcome: Outcome<ToolCalls>,
    evidence: Vec<Evidence>,
    losses: Vec<Loss>,
    inferences: Vec<Inference>,
    validations: Vec<Validation>,
    alternatives: Vec<AlternativeReceipt>,
    stopped: StopReason,
}

/// One part of a reading's ledger, in output order.
enum Part {
    /// Message-level repairs (encoding, reasoning).
    Message(CallLedger),
    /// The repairs of the call at this index.
    Call(usize),
    /// The repairs of the rejected attempt at this index.
    Rejected(usize),
}

/// What a reading keeps about one call beyond its value.
struct CallFacts {
    /// The selected reading's cost.
    cost: u64,
    /// The selected reading's schema validations.
    validations: Vec<Validation>,
    /// How many readings compete at the least cost.
    competing: usize,
}

/// One whole-message reading.
struct Reading {
    value: ToolCalls,
    facts: Vec<CallFacts>,
    parts: Vec<Part>,
    /// Validations keyed by the source range they belong to.
    validations: Vec<((usize, usize), Validation)>,
    cost: u64,
    /// Under [`Effort::Exact`], every message-level repair the segmentation
    /// needed (a reasoning block closed at a call opener), as diagnostics.
    inexact: Vec<String>,
    /// Every dialect that attempted a rejected call, by the attempt's
    /// opener offset: the reading's own rejected attempts and those of the
    /// options folded into its choices ([`Fold`]).
    attempts: BTreeMap<usize, BTreeSet<DialectId>>,
    /// What the strict cardinality check counted (see [`Extras`]).
    extras: Extras,
    /// The index of the tool-choice validation in [`Self::validations`].
    choice_validation: usize,
}

/// What a strict `exactly_one` check counted beyond the one call: calls
/// after the first, and rejected call attempts of any reason. Readings
/// that recover the same calls can differ in the attempts (a dialect that
/// rejects the bytes another reads as content), so [`merge`] keeps the
/// largest count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Extras {
    calls: usize,
    attempts: usize,
}

impl Extras {
    /// The one cardinality diagnostic, stating both counts; `None` when
    /// there is nothing beyond the one call.
    fn diagnostic(self, constraint: &str) -> Option<String> {
        let plural = |count: usize| if count == 1 { "" } else { "s" };
        let mut parts = Vec::new();
        if self.calls > 0 {
            parts.push(format!("{} extra call{}", self.calls, plural(self.calls)));
        }
        if self.attempts > 0 {
            parts.push(format!(
                "{} rejected call attempt{}",
                self.attempts,
                plural(self.attempts)
            ));
        }
        (!parts.is_empty()).then(|| {
            format!(
                "{} beyond the one call allowed under {constraint}",
                parts.join(" and ")
            )
        })
    }
}

/// The count of further entries a capped diagnostic leaves out.
fn more(rest: usize) -> String {
    if rest == 0 {
        String::new()
    } else {
        format!(" (and {rest} more)")
    }
}

/// One option of a contested span after equivalent segmentations are
/// folded together: the segments of the kept segmentation, and the dialects
/// every folded segmentation contributed.
struct Fold<'t> {
    segments: &'t [ResolvedSegment],
    /// The dialects that read each call, by the call's source range.
    calls: BTreeMap<(usize, usize), BTreeSet<DialectId>>,
    /// The dialects that attempted each rejected call, by its opener offset.
    attempts: BTreeMap<usize, BTreeSet<DialectId>>,
}

impl<'t> Fold<'t> {
    fn new(segments: &'t [ResolvedSegment], governor: &mut Governor) -> Result<Self, StopReason> {
        let mut fold = Self {
            segments,
            calls: BTreeMap::new(),
            attempts: BTreeMap::new(),
        };
        fold.absorb(segments, governor)?;
        Ok(fold)
    }

    /// Add the dialects of `segments`, a segmentation equivalent to the
    /// kept one ([`equivalent_options`]).
    fn absorb(
        &mut self,
        segments: &[ResolvedSegment],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        for segment in segments {
            let (entry, dialect) = match segment {
                ResolvedSegment::Call(call) => (
                    self.calls
                        .entry((call.span.start, call.span.end))
                        .or_default(),
                    &call.dialect,
                ),
                ResolvedSegment::Rejected { span, dialect, .. } => {
                    (self.attempts.entry(span.start).or_default(), dialect)
                }
                ResolvedSegment::Content(_)
                | ResolvedSegment::Result(_)
                | ResolvedSegment::Reasoning { .. } => continue,
            };
            governor.charge(Resource::Work, size(entry.len()).saturating_add(1))?;
            if entry.insert(dialect.clone()) {
                governor.charge(Resource::ScratchBytes, ENTRY_BYTES)?;
            }
        }
        Ok(())
    }

    /// The body evidence of the segmentation's rejected attempts over
    /// `output` (see [`Reading::body_evidence`]).
    fn body_evidence(segments: &[ResolvedSegment], output: &[u8]) -> BodyEvidence {
        evidence(segments.iter().filter_map(|segment| match segment {
            ResolvedSegment::Rejected {
                span,
                reason,
                dialect,
                ..
            } => Some((
                reach_of(span, *reason),
                dialect,
                output.get(span.clone()).unwrap_or_default(),
            )),
            _ => None,
        }))
    }
}

/// Whether two segmentations of one contested span recover the same calls
/// (each call's span, name, id, closure, outcome, readings, and
/// alternatives, whichever dialect read it) and the same reasoning: they
/// differ at most in which dialect read each call and in how the bytes
/// outside every call divide into rejected attempts and content. Whole
/// readings that differ only in such a choice are one reading
/// ([`same_calls`]), so folding them per span before the product keeps the
/// product linear in the number of such spans instead of exponential.
fn equivalent_options(
    left: &[ResolvedSegment],
    right: &[ResolvedSegment],
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    let kept = |segment: &&ResolvedSegment| {
        matches!(
            segment,
            ResolvedSegment::Call(_) | ResolvedSegment::Reasoning { .. }
        )
    };
    let mut left = left.iter().filter(kept);
    let mut right = right.iter().filter(kept);
    loop {
        governor.charge(Resource::Work, 1)?;
        match (left.next(), right.next()) {
            (None, None) => return Ok(true),
            (Some(ResolvedSegment::Call(a)), Some(ResolvedSegment::Call(b))) => {
                if !same_call(a, b) {
                    return Ok(false);
                }
            }
            (Some(a @ ResolvedSegment::Reasoning { .. }), Some(b)) => {
                if a != b {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
    }
}

/// Whether two resolved calls are the same call, whichever dialect read it.
fn same_call(left: &ResolvedCall, right: &ResolvedCall) -> bool {
    let ResolvedCall {
        span,
        dialect: _,
        order,
        id,
        name,
        closure,
        outcome,
        reading,
        competing,
        alternatives,
        distinguishing,
    } = left;
    *span == right.span
        && *order == right.order
        && *id == right.id
        && *name == right.name
        && *closure == right.closure
        && *outcome == right.outcome
        && *reading == right.reading
        && *competing == right.competing
        && *alternatives == right.alternatives
        && *distinguishing == right.distinguishing
}

impl Reading {
    /// Every ledger of the reading, in output order.
    fn ledgers(&self) -> impl Iterator<Item = &CallLedger> {
        self.parts.iter().map(|part| match part {
            Part::Message(ledger) => ledger,
            Part::Call(index) => &self.value.calls[*index].repairs,
            Part::Rejected(index) => &self.value.rejected[*index].repairs,
        })
    }

    /// The body evidence of the reading's rejected attempts: how many bytes
    /// their dialects read before failing; on a tie, how many of them begin
    /// with their dialect's canonical opener spelling (an opener read
    /// through a confusable spelling is weaker evidence of the dialect).
    fn body_evidence(&self) -> BodyEvidence {
        evidence(self.value.rejected.iter().filter_map(|rejected| {
            rejected
                .dialect
                .as_ref()
                .map(|dialect| (reach(rejected), dialect, rejected.raw.as_slice()))
        }))
    }

    fn ambiguous_calls(&self) -> impl Iterator<Item = usize> + '_ {
        self.value
            .calls
            .iter()
            .filter(|call| call.outcome == CallOutcome::Ambiguous)
            .map(|call| call.index)
    }
}

/// The receipt's validation log, deduplicated per source range and
/// constraint: a validation can only equal another with the same
/// constraint, so only those are compared (a call with thousands of
/// distinct keyword validations costs linear, not quadratic, work).
struct Log {
    entries: Vec<Validation>,
    index: BTreeMap<((usize, usize), String), Vec<usize>>,
}

impl Log {
    fn add(
        &mut self,
        key: (usize, usize),
        validation: &Validation,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let known = self
            .index
            .entry((key, validation.constraint.clone()))
            .or_default();
        governor.charge(Resource::Work, size(known.len()).saturating_add(1))?;
        if let Some(&found) = known
            .iter()
            .find(|&&index| self.entries[index] == *validation)
        {
            return Ok(found);
        }
        governor.charge(Resource::ScratchBytes, validation_bytes(validation))?;
        known.push(self.entries.len());
        self.entries.push(validation.clone());
        Ok(self.entries.len() - 1)
    }
}

fn validation_bytes(validation: &Validation) -> u64 {
    size(
        validation.validator.len()
            + validation.constraint.len()
            + validation
                .diagnostics
                .iter()
                .map(String::len)
                .sum::<usize>(),
    )
    .saturating_add(ENTRY_BYTES)
}

fn ledger_bytes(ledger: &CallLedger) -> u64 {
    let evidence = |evidence: &Evidence| {
        size(evidence.rule.len() + evidence.explanation.len() + evidence.destination.len())
            .saturating_add(ENTRY_BYTES)
    };
    let mut total = 0_u64;
    for entry in &ledger.evidence {
        total = total.saturating_add(evidence(entry));
    }
    for loss in &ledger.losses {
        total = total
            .saturating_add(evidence(&loss.loss.evidence))
            .saturating_add(size(loss.loss.original.len()));
    }
    for inference in &ledger.inferences {
        total = total
            .saturating_add(evidence(&inference.evidence))
            .saturating_add(size(inference.value.len()));
    }
    total
}

fn reading_bytes(reading: &CallReading) -> u64 {
    reading
        .arguments
        .estimated_bytes()
        .saturating_add(ledger_bytes(&reading.repairs))
        .saturating_add(reading.validations.iter().map(validation_bytes).sum())
        .saturating_add(ENTRY_BYTES)
}

/// Append `range` to `ranges`, merging it into an adjacent last range.
fn push_range(ranges: &mut Vec<Range<usize>>, range: &Range<usize>) {
    match ranges.last_mut() {
        Some(last) if last.end == range.start => last.end = range.end,
        _ => ranges.push(range.clone()),
    }
}

/// Whether the schema object at `pointer` (an unmodeled-keyword pointer:
/// `""` or `/properties/NAME` and `/items` steps) describes a value
/// `arguments` contains.
fn on_path(pointer: &str, arguments: &Node, governor: &mut Governor) -> Result<bool, StopReason> {
    let mut nodes = vec![arguments];
    let mut segments = pointer.split('/').skip(1);
    while let Some(segment) = segments.next() {
        let mut next = Vec::new();
        match segment {
            "properties" => {
                let Some(name) = segments.next() else {
                    return Ok(false);
                };
                for node in &nodes {
                    if let Value::Object(members) = &node.value {
                        governor.charge(Resource::Work, size(members.len()).saturating_add(1))?;
                        next.extend(
                            members
                                .iter()
                                .filter(|member| pointer_segment(&member.key) == name)
                                .map(|member| &member.value),
                        );
                    }
                }
            }
            "items" => {
                for node in &nodes {
                    if let Value::Array(items) = &node.value {
                        governor.charge(Resource::Work, size(items.len()).saturating_add(1))?;
                        next.extend(items.iter());
                    }
                }
            }
            _ => return Ok(false),
        }
        if next.is_empty() {
            return Ok(false);
        }
        nodes = next;
    }
    Ok(true)
}

/// A label for a call in diagnostics.
fn label(call: &ToolCall) -> String {
    format!(
        "call {} (`{}`)",
        call.index,
        call.tool
            .as_deref()
            .or(call.name.as_deref())
            .unwrap_or("<unnamed>")
    )
}

struct Recovery<'a> {
    tools: &'a ToolSet,
    request: &'a ToolRecoveryRequest,
    dialects: &'a DialectSet,
    output: &'a [u8],
    source: Digest,
    policies: Policies,
}

impl Recovery<'_> {
    /// Every whole-message reading of the output, before they are compared
    /// ([`Self::conclude`]).
    #[cfg(test)]
    fn recognized(&self, governor: &mut Governor) -> Result<Vec<Reading>, StopReason> {
        let mut intake = RecoveryIntake::new(self.tools, self.request, self.dialects, governor);
        intake.feed(self.output, governor);
        intake.receipt.source = self.source;
        let RecognizedInput {
            turn,
            encoding,
            forced,
            ..
        } = intake.recognize(self.request, governor)?;
        self.resolve_turn(turn, &encoding, forced.as_deref(), governor)
    }

    /// Validate the request once before constructing or advancing a recognizer.
    fn preconditions(&self, governor: &Governor) -> Result<Option<String>, StopReason> {
        governor.check()?;
        self.policies.validate(self.tools)?;
        let forced = match &self.request.tool_choice {
            ToolChoice::Forced(name) | ToolChoice::ForcedStrict(StrictForced { name, .. }) => Some(
                self.tools
                    .lookup_exact(name)
                    .ok_or_else(|| {
                        invalid_input(format!(
                            "tool choice forces `{name}`, which is not a declared tool"
                        ))
                    })?
                    .name()
                    .to_owned(),
            ),
            _ => None,
        };
        if let ToolChoice::ForcedStrict(StrictForced {
            allow_reasoning: false,
            ..
        }) = &self.request.tool_choice
            && self.request.reasoning == ReasoningPolicy::AsContent
        {
            // Reasoning is never recognized, so the option could not be
            // enforced: the request is contradictory, not satisfied.
            return Err(invalid_input(
                "tool choice forbids reasoning (`allow_reasoning: false`), but the reasoning policy \
                 treats reasoning markers as content, so reasoning is never recognized"
                    .to_owned(),
            ));
        }
        Ok(forced)
    }

    /// Resolve an already finished recognizer without reading the input again.
    /// Callers retain the original source and its encoding ledger throughout.
    fn resolve_turn(
        &self,
        turn: super::RecognizedTurn,
        encoding: &CallLedger,
        forced: Option<&str>,
        governor: &mut Governor,
    ) -> Result<Vec<Reading>, StopReason> {
        let resolved = resolve_forest(self.tools, &turn, &self.policies, governor)?;
        let attempted = turn.rejected_attempts().to_vec();
        drop(turn);
        let readings = self.readings(&resolved, encoding, &attempted, forced, governor)?;
        drop(resolved);
        Ok(readings)
    }

    /// Publish the invalid runs found by the shared incremental UTF-8 cursor.
    fn encoding(
        &self,
        invalid: &[(usize, usize)],
        governor: &mut Governor,
    ) -> Result<CallLedger, StopReason> {
        let mut ledger = CallLedger::default();
        for &(start, end) in invalid {
            self.invalid(start..end, &mut ledger, governor)?;
        }
        Ok(ledger)
    }

    fn invalid(
        &self,
        range: Range<usize>,
        ledger: &mut CallLedger,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        // `\xHH` per byte.
        governor.charge(Resource::ScratchBytes, size(range.len()).saturating_mul(4))?;
        let evidence = Evidence {
            rule: "tool_call.input.invalid-utf8".into(),
            explanation: format!(
                "bytes {}..{} are not well-formed UTF-8; they are retained exactly in the receipt's source",
                range.start, range.end
            ),
            span: Some(SourceSpan {
                source: self.source,
                bytes: range.clone(),
            }),
            destination: format!("/turn/{}..{}", range.start, range.end),
        };
        ledger.record(
            rules::resolution_rule("tool_call.input.invalid-utf8"),
            evidence,
            escaped(&self.output[range]),
            String::new(),
            governor,
        )
    }

    /// Every whole-message reading of `turn`; when the turn has competing
    /// segmentations, their product is charged as candidates first.
    ///
    /// The segmentations of each contested span that recover the same calls
    /// ([`equivalent_options`]) are folded into one option first: the one
    /// whose rejected attempts' dialects read the most bytes (the earliest
    /// on a tie), carrying every folded segmentation's dialects. This is the
    /// selection [`same_calls`] makes over whole readings, made per span, so
    /// the readings are the same, but a turn with many contested spans of
    /// this kind (a `<tool_call>` attempt that Qwen3-Coder XML and Hermes
    /// both reject, repeated over many calls) builds one reading instead of
    /// one per combination.
    fn readings(
        &self,
        turn: &ResolvedTurn,
        encoding: &CallLedger,
        attempted: &[(usize, DialectId)],
        forced: Option<&str>,
        governor: &mut Governor,
    ) -> Result<Vec<Reading>, StopReason> {
        let mut choices: Vec<Vec<Fold<'_>>> = Vec::new();
        for piece in &turn.pieces {
            if let ResolvedPiece::Choice(choice) = piece {
                choices.push(fold_options(&choice.readings, self.output, governor)?);
            }
        }
        if !choices.is_empty() {
            // Every reading is admitted before any is built: a product over
            // the budget is exhaustion, never a truncated list.
            let product = choices.iter().fold(1_u64, |product, folds| {
                product.saturating_mul(size(folds.len()))
            });
            governor.charge(Resource::Candidates, product)?;
        }
        let mut digits = vec![0_usize; choices.len()];
        let mut readings = Vec::new();
        loop {
            let mut segments = Vec::with_capacity(turn.pieces.len());
            let mut folds = Vec::with_capacity(choices.len());
            let mut choice = 0;
            for piece in &turn.pieces {
                match piece {
                    ResolvedPiece::Segment(segment) => segments.push(segment),
                    ResolvedPiece::Choice(_) => {
                        let fold = &choices[choice][digits[choice]];
                        segments.extend(fold.segments);
                        folds.push(fold);
                        choice += 1;
                    }
                }
            }
            let mut reading =
                self.reading(&segments, turn.cost, encoding, attempted, forced, governor)?;
            apply_folds(&mut reading, &folds, governor)?;
            readings.push(reading);
            let mut position = 0;
            loop {
                if position == digits.len() {
                    return Ok(readings);
                }
                digits[position] += 1;
                if digits[position] < choices[position].len() {
                    break;
                }
                digits[position] = 0;
                position += 1;
            }
        }
    }

    /// One whole-message reading over its segments.
    fn reading(
        &self,
        segments: &[&ResolvedSegment],
        cost: u64,
        encoding: &CallLedger,
        attempted: &[(usize, DialectId)],
        forced: Option<&str>,
        governor: &mut Governor,
    ) -> Result<Reading, StopReason> {
        governor.charge(Resource::ScratchBytes, ledger_bytes(encoding))?;
        let mut reading = Reading {
            value: ToolCalls {
                calls: Vec::new(),
                rejected: Vec::new(),
                embedded: Vec::new(),
                content: Vec::new(),
                reasoning: Vec::new(),
                reasoning_blocks: Vec::new(),
                finish: Finish::ContentOnly,
                tool_choice: ToolChoiceCheck {
                    choice: self.request.tool_choice.clone(),
                    passed: true,
                    diagnostics: Vec::new(),
                },
                next_id_start: 0,
                turn: self.source,
                length_stop: self.request.stop.is_truncation(),
                emitted: None,
            },
            facts: Vec::new(),
            parts: vec![Part::Message(encoding.clone())],
            validations: Vec::new(),
            cost,
            inexact: Vec::new(),
            attempts: BTreeMap::new(),
            extras: Extras::default(),
            choice_validation: 0,
        };
        for segment in segments {
            self.segment(&mut reading, segment, governor)?;
        }
        // Name every dialect that attempted a reported attempt's opener,
        // in any segmentation.
        for (start, dialect) in attempted {
            governor.charge(Resource::Work, 1)?;
            if reading
                .attempts
                .get(start)
                .is_some_and(|known| !known.contains(dialect))
            {
                governor.charge(Resource::ScratchBytes, ENTRY_BYTES)?;
                reading
                    .attempts
                    .entry(*start)
                    .or_default()
                    .insert(dialect.clone());
            }
        }
        // A report never starts inside a literal a rejected attempt began.
        let owned = self.owned_literals(&reading.value, governor)?;
        self.unparsed(&mut reading, &owned, governor)?;
        // After unparsed markup is reported: its reports are boundaries.
        self.close_rejections(&mut reading.value, governor)?;
        self.closers_after_cut(&mut reading, governor)?;
        reading.value.next_id_start = self.ids(&mut reading.value.calls, governor)?;
        reading.extras = self.tool_choice(&mut reading.value, forced, governor)?;
        reading.choice_validation = reading.validations.len();
        let check = &reading.value.tool_choice;
        reading.validations.push((
            MESSAGE,
            Validation {
                validator: TOOL_CALL_VALIDATOR.into(),
                constraint: check.choice.constraint(),
                passed: check.passed,
                diagnostics: check.diagnostics.clone(),
            },
        ));
        for index in 0..reading.value.calls.len() {
            let passed = reading.facts[index]
                .validations
                .last()
                .is_some_and(|validation| validation.passed);
            let call = &mut reading.value.calls[index];
            for validation in self.keywords(call, passed, governor)? {
                reading
                    .validations
                    .push(((call.span.start, call.span.end), validation));
            }
        }
        reading.value.finish = self.finish(&reading.value);
        reading.value.emitted =
            emit::emission(self.tools, &reading.value, &self.request.emit, governor)?;
        Ok(reading)
    }

    /// The bytes after each rejected attempt that complete the literal it
    /// stopped inside: an attempt that stopped reading inside a literal
    /// spelling of its dialect's call production (a key, a wrapper token)
    /// owns the rest of that spelling, before the next segment. Unparsed
    /// markup is scanned without them, so no report starts inside the
    /// attempt's literal; [`Self::close_rejections`] then extends the
    /// attempt over them (and possibly further).
    fn owned_literals(
        &self,
        value: &ToolCalls,
        governor: &mut Governor,
    ) -> Result<Vec<Range<usize>>, StopReason> {
        let mut owned = Vec::new();
        for (index, rejected) in value.rejected.iter().enumerate() {
            let Some(dialect) = &rejected.dialect else {
                continue;
            };
            let span = &rejected.span;
            // The next segment after the attempt bounds the literal.
            let limit = value
                .calls
                .iter()
                .map(|call| &call.span)
                .chain(value.reasoning.iter())
                .chain(
                    value
                        .rejected
                        .iter()
                        .enumerate()
                        .filter(|(other, _)| *other != index)
                        .map(|(_, rejected)| &rejected.span),
                )
                .filter(|range| range.start >= span.end && !range.is_empty())
                .map(|range| range.start)
                .min()
                .unwrap_or(self.output.len());
            let end = MarkupScan {
                output: self.output,
            }
            .complete_literal(
                span,
                dialect.grammar().call_literals(),
                limit,
                governor,
            )?;
            if end > span.end {
                governor.charge(Resource::ScratchBytes, ENTRY_BYTES)?;
                owned.push(span.end..end);
            }
        }
        Ok(owned)
    }

    /// Extend every rejected attempt over its own markup (see
    /// [`RejectedCall::span`]): a rejected attempt ends where its grammar
    /// stopped reading, but the call markup it began runs on to the end
    /// literal of the alternative it opened (its wrapper or section closer,
    /// any declared spelling, at nesting depth one: another opener of the
    /// same alternative opens a level, unless the alternative chains calls
    /// that each open with it), so its raw text is the whole call and no
    /// closing markup is left as content. The search reads the content
    /// after the attempt; a call or rejected attempt inside the broken
    /// wrapper (a bare JSON object recovered from it, an inner element's
    /// own failure) is kept and passed over, so the rejection's span covers
    /// it, and the search stops at a reasoning block, reported unparsed
    /// markup (whose bytes stay content under its own report), or a call or
    /// attempt opening the same alternative again (a sibling). Repeated
    /// closers right after the end literal are absorbed as stray closers.
    /// Without an end literal before such a boundary the attempt ends at
    /// the last literal ending a call of its dialect before the next
    /// segment, or else where its grammar stopped reading (never inside a
    /// literal); everything after it stays content, since nothing shows it
    /// is markup (a broken opener followed by the answer in prose).
    fn close_rejections(
        &self,
        value: &mut ToolCalls,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        for index in 0..value.rejected.len() {
            // Unparsed markup (no allowed dialect read it) stays content.
            if matches!(
                value.rejected[index].reason,
                Rejection::UnsupportedDialect { .. }
            ) {
                continue;
            }
            let Some(dialect) = value.rejected[index].dialect.clone() else {
                continue;
            };
            let span = value.rejected[index].span.clone();
            // The segments after the attempt, in order.
            let mut boundaries: Vec<(Range<usize>, bool)> = value
                .calls
                .iter()
                .map(|call| (call.span.clone(), true))
                .chain(value.reasoning.iter().map(|range| (range.clone(), false)))
                .chain(
                    value
                        .rejected
                        .iter()
                        .enumerate()
                        .filter(|(other, _)| *other != index)
                        .map(|(_, rejected)| {
                            // An inner attempt is passed over like an inner
                            // call; reported unparsed markup stays content
                            // and bounds the scan.
                            let inner =
                                !matches!(rejected.reason, Rejection::UnsupportedDialect { .. });
                            (rejected.span.clone(), inner)
                        }),
                )
                .filter(|(range, _)| range.start >= span.end && !range.is_empty())
                .collect();
            boundaries.sort_by_key(|(range, _)| range.start);
            let reason = value.rejected[index].reason;
            let end = rejected_end(self.output, &span, &dialect, reason, &boundaries, governor)?;
            if end <= span.end {
                continue;
            }
            governor.charge(Resource::ScratchBytes, size(end - span.end))?;
            remove_range(&mut value.content, &(span.end..end));
            let rejected = &mut value.rejected[index];
            rejected.span.end = end;
            rejected.raw = self.output[rejected.span.clone()].to_vec();
        }
        Ok(())
    }

    /// Add one segment to a reading.
    fn segment(
        &self,
        reading: &mut Reading,
        segment: &ResolvedSegment,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        governor.charge(Resource::ScratchBytes, ENTRY_BYTES)?;
        match segment {
            ResolvedSegment::Content(span) => push_range(&mut reading.value.content, span),
            ResolvedSegment::Result(result) => {
                push_range(&mut reading.value.content, &result.span);
            }
            ResolvedSegment::Reasoning {
                reasoning,
                embedded,
            } => {
                push_range(&mut reading.value.reasoning, &reasoning.span);
                reading.value.reasoning_blocks.push(ReasoningBlock {
                    span: reasoning.span.clone(),
                    text: reasoning.text.clone(),
                });
                for call in embedded {
                    let index = reading.value.embedded.len();
                    let (built, _) = tool_call(call, index, governor)?;
                    reading.value.embedded.push(built);
                }
                let ledger = self.reasoning_ledger(reasoning, governor)?;
                if self.policies.exact {
                    for evidence in &ledger.evidence {
                        if rules::rule(&evidence.rule).is_none_or(super::Rule::is_repair) {
                            reading.inexact.push(format!(
                                "the segmentation needs repair `{}` ({})",
                                evidence.rule, evidence.destination
                            ));
                        }
                    }
                }
                reading.parts.push(Part::Message(ledger));
            }
            ResolvedSegment::Call(call) => {
                let index = reading.value.calls.len();
                let (built, facts) = tool_call(call, index, governor)?;
                reading.cost = reading.cost.saturating_add(facts.cost);
                for validation in &facts.validations {
                    reading
                        .validations
                        .push(((call.span.start, call.span.end), validation.clone()));
                }
                reading.value.calls.push(built);
                reading.facts.push(facts);
                reading.parts.push(Part::Call(index));
            }
            ResolvedSegment::Rejected {
                span,
                dialect,
                reason,
                repairs,
            } => {
                governor.charge(
                    Resource::ScratchBytes,
                    size(span.len()).saturating_add(ledger_bytes(repairs)),
                )?;
                let index = reading.value.rejected.len();
                if reading
                    .attempts
                    .entry(span.start)
                    .or_default()
                    .insert(dialect.clone())
                {
                    governor.charge(Resource::ScratchBytes, ENTRY_BYTES)?;
                }
                reading.value.rejected.push(RejectedCall {
                    span: span.clone(),
                    dialect: Some(dialect.clone()),
                    candidate_dialects: vec![dialect.clone()],
                    raw: self.output[span.clone()].to_vec(),
                    reason: precise(*reason, self.output),
                    repairs: repairs.clone(),
                });
                reading.parts.push(Part::Rejected(index));
            }
        }
        Ok(())
    }

    /// Report the call markup of the reading's content that no allowed
    /// dialect reads ([`detect_unparsed`]) as rejected attempts with
    /// [`Rejection::UnsupportedDialect`], each recording the
    /// `tool_call.markup.unparsed-dialect` evidence; the bytes stay
    /// content. The `owned` ranges (the rest of a literal a rejected
    /// attempt stopped inside) are not scanned. The rejected attempts stay
    /// ordered by span.
    fn unparsed(
        &self,
        reading: &mut Reading,
        owned: &[Range<usize>],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut content = reading.value.content.clone();
        for range in owned {
            remove_range(&mut content, range);
        }
        let found = detect_unparsed(self.output, &content, self.dialects, governor)?;
        if found.is_empty() {
            return Ok(());
        }
        for detection in found {
            let span = detection.span;
            let family = detection.family;
            governor.charge(
                Resource::ScratchBytes,
                size(span.len()).saturating_add(ENTRY_BYTES),
            )?;
            let mut repairs = CallLedger::default();
            let evidence = Evidence {
                rule: "tool_call.markup.unparsed-dialect".into(),
                explanation: format!(
                    "bytes {}..{} are {}",
                    span.start,
                    span.end,
                    family.message()
                ),
                span: Some(SourceSpan {
                    source: self.source,
                    bytes: span.clone(),
                }),
                destination: format!("/turn/{}..{}", span.start, span.end),
            };
            repairs.record(
                rules::resolution_rule("tool_call.markup.unparsed-dialect"),
                evidence,
                String::new(),
                String::new(),
                governor,
            )?;
            let index = reading.value.rejected.len();
            reading.value.rejected.push(RejectedCall {
                raw: self.output[span.clone()].to_vec(),
                span,
                dialect: family.dialect(),
                candidate_dialects: Vec::new(),
                reason: Rejection::UnsupportedDialect { family },
                repairs,
            });
            reading.parts.push(Part::Rejected(index));
        }
        sort_rejected(reading);
        Ok(())
    }

    /// The closers a call's or attempt's markup left after a reasoning block
    /// it ran into, so none is left as content: see
    /// [`Self::closers_of_rejected`], [`Self::closers_of_calls`] and
    /// [`Self::closers_after_blocks`]. Each found closer of an attempt is
    /// reported as a rejected attempt of its dialect over the closer alone
    /// (the block's bytes stay reasoning).
    fn closers_after_cut(
        &self,
        reading: &mut Reading,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut found = self.closers_of_rejected(reading, governor)?;
        self.closers_of_calls(reading, governor)?;
        self.closers_after_blocks(reading, &mut found, governor)?;
        for (span, dialect, candidates) in found {
            governor.charge(
                Resource::ScratchBytes,
                size(span.len()).saturating_add(ENTRY_BYTES),
            )?;
            remove_range(&mut reading.value.content, &span);
            let index = reading.value.rejected.len();
            reading.value.rejected.push(RejectedCall {
                raw: self.output[span.clone()].to_vec(),
                reason: Rejection::Malformed { at: span.start },
                span,
                dialect: Some(dialect),
                candidate_dialects: candidates,
                repairs: CallLedger::default(),
            });
            reading.parts.push(Part::Rejected(index));
        }
        sort_rejected(reading);
        Ok(())
    }

    /// The first byte after `block` and the whitespace after it.
    const fn after(&self, block: &Range<usize>) -> usize {
        let mut at = block.end;
        while at < self.output.len() && self.output[at].is_ascii_whitespace() {
            at += 1;
        }
        at
    }

    /// The length of one of `closers` (multi-byte, exactly or with one
    /// slip, as [`MarkupScan`] reads them) written at `at`.
    fn closer_at(&self, closers: &[impl AsRef<[u8]>], at: usize) -> Option<usize> {
        let markup = Markup {
            openers: Vec::new(),
            closers: closers
                .iter()
                .map(|bytes| bytes.as_ref().to_vec())
                .collect(),
            nests: false,
            call_closers: Vec::new(),
            literals: &[],
        };
        let closers = &markup.closers;
        closers
            .iter()
            .filter(|closer| closer.len() > 1 && self.output[at..].starts_with(closer))
            .map(Vec::len)
            .max()
            .or_else(|| {
                MarkupScan {
                    output: self.output,
                }
                .misspelled(closers, &markup, at, self.output.len())
            })
    }

    /// The closers of the alternatives of `dialect` whose opener begins
    /// `raw`.
    fn closers_opened(dialect: &DialectId, raw: &[u8]) -> Vec<Vec<u8>> {
        dialect
            .grammar()
            .call_ends()
            .iter()
            .filter(|end| end.openers().iter().any(|opener| raw.starts_with(opener)))
            .flat_map(|end| end.closers().iter().map(|bytes| bytes.as_slice().to_vec()))
            .filter(|closer| closer.len() > 1)
            .collect()
    }

    /// Whether `span` lies in one content range of `reading`.
    fn in_content(reading: &Reading, span: &Range<usize>) -> bool {
        reading
            .value
            .content
            .iter()
            .any(|range| range.start <= span.start && span.end <= range.end)
    }

    /// The first reasoning block at or after `from` with no content but
    /// whitespace before it.
    fn next_block<'r>(&self, reading: &'r Reading, from: usize) -> Option<&'r Range<usize>> {
        let block = reading
            .value
            .reasoning
            .iter()
            .filter(|range| range.start >= from)
            .min_by_key(|range| range.start)?;
        let prose = reading.value.content.iter().any(|span| {
            let (start, end) = (span.start.max(from), span.end.min(block.start));
            start < end && !self.output[start..end].iter().all(u8::is_ascii_whitespace)
        });
        (!prose).then_some(block)
    }

    /// A rejected attempt whose markup ran into a reasoning block (cut at
    /// its opening marker, see `Recognizer::ended`, or a broken wrapper
    /// whose inner call ends there) with no closer of its own may have its
    /// closer written right after the block: that closer is the attempt's
    /// markup.
    fn closers_of_rejected(
        &self,
        reading: &Reading,
        governor: &mut Governor,
    ) -> Result<Vec<StrayCloser>, StopReason> {
        let mut found = Vec::new();
        for rejected in &reading.value.rejected {
            let (Some(dialect), Rejection::Malformed { .. } | Rejection::Unterminated) =
                (&rejected.dialect, rejected.reason)
            else {
                continue;
            };
            let Some(block) = self.next_block(reading, rejected.span.end) else {
                continue;
            };
            governor.charge(Resource::Work, 1)?;
            let closers = Self::closers_opened(dialect, &rejected.raw);
            if closers.iter().any(|closer| rejected.raw.ends_with(closer)) {
                continue;
            }
            let at = self.after(block);
            if let Some(length) = self.closer_at(&closers, at)
                && Self::in_content(reading, &(at..at + length))
            {
                found.push((
                    at..at + length,
                    dialect.clone(),
                    rejected.candidate_dialects.clone(),
                ));
            }
        }
        Ok(found)
    }

    /// A call read with its wrapper closer inserted before a reasoning
    /// block (the block is no wrapper text), whose closer is written right
    /// after the block: the call's own closer, moved past the block,
    /// recorded on the call as a stray closer
    /// (`tool_call.markup.stray-closer`) and no longer content.
    fn closers_of_calls(
        &self,
        reading: &mut Reading,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Some(rule) = rules::rule("tool_call.markup.stray-closer") else {
            return Ok(());
        };
        for index in 0..reading.value.calls.len() {
            let call = &reading.value.calls[index];
            let inserted = call.repairs.evidence.iter().any(|evidence| {
                evidence.rule == "tool_call.markup.missing-wrapper-close"
                    || evidence.rule == "tool_call.markup.missing-wrapper-close.before-text"
            });
            let Some(block) = self
                .next_block(reading, call.span.end)
                .filter(|block| inserted && block.start >= call.span.end)
            else {
                continue;
            };
            if !self.output[call.span.end..block.start]
                .iter()
                .all(u8::is_ascii_whitespace)
            {
                continue;
            }
            governor.charge(Resource::Work, 1)?;
            let at = self.after(block);
            let closers = Self::closers_opened(&call.dialect, &self.output[call.span.clone()]);
            let Some(length) = self.closer_at(&closers, at) else {
                continue;
            };
            let stray = at..at + length;
            if !Self::in_content(reading, &stray) {
                continue;
            }
            let evidence = Evidence {
                rule: rule.id.into(),
                explanation: format!(
                    "the call's closer `{}` is written after the reasoning block at bytes {}..{}; the block is no part of the call, so the closer was read as inserted before it and this one as stray",
                    escaped(&self.output[stray.clone()]),
                    block.start,
                    block.end
                ),
                span: Some(SourceSpan {
                    source: self.source,
                    bytes: stray.clone(),
                }),
                destination: format!("/turn/{}..{}", call.span.start, call.span.end),
            };
            let original = escaped(&self.output[stray.clone()]);
            governor.charge(Resource::ScratchBytes, size(stray.len()))?;
            remove_range(&mut reading.value.content, &stray);
            reading.value.calls[index].repairs.record(
                rule,
                evidence,
                original,
                String::new(),
                governor,
            )?;
        }
        Ok(())
    }

    /// The mirror inside reasoning: a block holding a call opener with no
    /// closer of its alternative after it, whose closer is written right
    /// after the block (a call begun inside the block that ran into its
    /// closing marker), named by every allowed dialect with such an
    /// opener.
    fn closers_after_blocks(
        &self,
        reading: &Reading,
        found: &mut Vec<StrayCloser>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        for block in &reading.value.reasoning {
            let at = self.after(block);
            let inside = &self.output[block.clone()];
            let mut dialects: Vec<(DialectId, usize)> = Vec::new();
            for dialect in self.dialects.dialects() {
                for end in dialect.grammar().call_ends() {
                    governor.charge(Resource::Work, size(inside.len()))?;
                    let open =
                        end.openers()
                            .iter()
                            .filter(|opener| opener.len() > 1)
                            .any(|opener| {
                                inside
                                    .windows(opener.len())
                                    .rposition(|window| window == opener.as_slice())
                                    .is_some_and(|last| {
                                        !end.closers()
                                            .iter()
                                            .filter(|closer| !closer.is_empty())
                                            .any(|closer| {
                                                inside[last..]
                                                    .windows(closer.len())
                                                    .any(|window| window == closer.as_slice())
                                            })
                                    })
                            });
                    if let Some(length) = open.then(|| self.closer_at(end.closers(), at)).flatten()
                    {
                        dialects.push((dialect.clone(), length));
                    }
                }
            }
            let Some(length) = dialects.iter().map(|(_, length)| *length).max() else {
                continue;
            };
            let span = at..at + length;
            if !Self::in_content(reading, &span) || found.iter().any(|(known, _, _)| *known == span)
            {
                continue;
            }
            let mut candidates: Vec<DialectId> = dialects
                .into_iter()
                .filter(|(_, read)| *read == length)
                .map(|(dialect, _)| dialect)
                .collect();
            candidates.sort_by(|left, right| left.id().cmp(right.id()));
            candidates.dedup();
            found.push((span, candidates[0].clone(), candidates));
        }
        Ok(())
    }
}

/// A closer found after a reasoning block (see
/// [`Recovery::closers_after_cut`]): its span, the dialect it is reported
/// under, and every dialect it may close.
type StrayCloser = (Range<usize>, DialectId, Vec<DialectId>);

/// Keep `reading`'s rejected attempts in span order, its parts following.
fn sort_rejected(reading: &mut Reading) {
    {
        let rejected = core::mem::take(&mut reading.value.rejected);
        let mut order: Vec<usize> = (0..rejected.len()).collect();
        order.sort_by_key(|&index| (rejected[index].span.start, rejected[index].span.end));
        let mut moved = vec![0; rejected.len()];
        for (new, &old) in order.iter().enumerate() {
            moved[old] = new;
        }
        let mut slots: Vec<Option<RejectedCall>> = rejected.into_iter().map(Some).collect();
        reading.value.rejected = order.iter().filter_map(|&old| slots[old].take()).collect();
        for part in &mut reading.parts {
            if let Part::Rejected(index) = part {
                *index = moved[*index];
            }
        }
    }
}

impl Recovery<'_> {
    /// The ledger of a reasoning block's edits.
    fn reasoning_ledger(
        &self,
        reasoning: &ReasoningSegment,
        governor: &mut Governor,
    ) -> Result<CallLedger, StopReason> {
        let mut ledger = CallLedger::default();
        for edit in &reasoning.edits {
            let (evidence, loss, inference) = edit_entries(edit, &reasoning.span, self.source);
            governor.charge(Resource::EvidenceEntries, 1)?;
            if let Some(loss) = loss {
                governor.charge(Resource::EvidenceEntries, 1)?;
                ledger.losses.push(CallLoss {
                    kind: loss_kind(&loss.evidence.rule),
                    loss,
                });
            }
            if let Some(inference) = inference {
                governor.charge(Resource::EvidenceEntries, 1)?;
                ledger.inferences.push(inference);
            }
            ledger.evidence.push(evidence);
        }
        governor.charge(Resource::ScratchBytes, ledger_bytes(&ledger))?;
        Ok(ledger)
    }

    /// Assign every call's id; returns the id counter after them.
    fn ids(&self, calls: &mut [ToolCall], governor: &mut Governor) -> Result<u64, StopReason> {
        let sources: Vec<IdSource> = calls.iter().map(IdSource::of).collect();
        let (ids, next) = assign_ids(
            &self.request.ids,
            &self.source,
            &sources,
            governor,
            |position, duplicate, id, governor| {
                self.dedup(&mut calls[position], duplicate, id, governor)
            },
        )?;
        for (call, id) in calls.iter_mut().zip(ids) {
            call.id = id;
        }
        Ok(next)
    }

    /// Record a renumbered source id as a derived `tool_call.id.dedup`
    /// inference on the call.
    fn dedup(
        &self,
        call: &mut ToolCall,
        duplicate: bool,
        id: &str,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let explanation = match &call.source_id {
            Some(source) if duplicate => {
                format!("source call id `{source}` repeats an earlier call's id; numbered `{id}`")
            }
            _ if matches!(self.request.ids, IdPolicy::Turn) => format!(
                "the call carries no id, though its dialect's markup provides for one; numbered `{id}`"
            ),
            _ => format!("the call carries no source id; numbered `{id}`"),
        };
        let evidence = Evidence {
            rule: "tool_call.id.dedup".into(),
            explanation,
            span: Some(SourceSpan {
                source: self.source,
                bytes: call.span.clone(),
            }),
            destination: format!("/turn/{}..{}/id", call.span.start, call.span.end),
        };
        call.repairs.record(
            rules::resolution_rule("tool_call.id.dedup"),
            evidence.clone(),
            String::new(),
            String::new(),
            governor,
        )?;
        governor.charge(Resource::EvidenceEntries, 1)?;
        governor.charge(
            Resource::ScratchBytes,
            size(id.len()).saturating_add(ENTRY_BYTES),
        )?;
        call.repairs.inferences.push(Inference {
            evidence,
            kind: InferenceKind::Derived,
            value: id.to_owned(),
        });
        Ok(())
    }

    /// Enforce the tool choice on a reading's calls; the extras a strict
    /// `exactly_one` counted (see [`Extras`]) for [`merge`].
    fn tool_choice(
        &self,
        value: &mut ToolCalls,
        forced: Option<&str>,
        governor: &mut Governor,
    ) -> Result<Extras, StopReason> {
        let choice = &self.request.tool_choice;
        let mut extras = Extras::default();
        let constraint = choice.constraint();
        let mut diagnostics = Vec::new();
        let unresolved = |call: &ToolCall| matches!(call.outcome, CallOutcome::Unresolved { .. });
        match choice {
            ToolChoice::Auto => {}
            ToolChoice::None => {
                for call in &mut value.calls {
                    diagnostics.push(format!("{} is not allowed under {constraint}", label(call)));
                    if !unresolved(call) {
                        call.outcome = CallOutcome::Unresolved {
                            reason: UnresolvedReason::ToolChoiceNone,
                        };
                    }
                }
            }
            ToolChoice::Required => {
                if value.calls.is_empty() {
                    diagnostics.push(format!("no call was made under {constraint}"));
                }
            }
            ToolChoice::Forced(_) | ToolChoice::ForcedStrict(_) => {
                let expected = forced.unwrap_or_default();
                if value.calls.is_empty() {
                    diagnostics.push(format!("no call was made under {constraint}"));
                }
                for call in &mut value.calls {
                    if call.tool.as_deref() == Some(expected) {
                        continue;
                    }
                    diagnostics.push(format!(
                        "{} does not call `{expected}` under {constraint}",
                        label(call)
                    ));
                    if !unresolved(call) {
                        call.outcome = CallOutcome::Unresolved {
                            reason: UnresolvedReason::ToolChoiceForced {
                                expected: expected.to_owned(),
                            },
                        };
                    }
                }
                if let ToolChoice::ForcedStrict(strict) = choice {
                    extras = self.strict(strict, value, &constraint, &mut diagnostics, governor)?;
                }
            }
        }
        for diagnostic in &diagnostics {
            governor.charge(
                Resource::ScratchBytes,
                size(diagnostic.len()).saturating_add(ENTRY_BYTES),
            )?;
        }
        value.tool_choice.passed = diagnostics.is_empty();
        value.tool_choice.diagnostics = diagnostics;
        Ok(extras)
    }

    /// The options of a strict forced choice, after the `Forced` checks.
    ///
    /// Each failed option adds at most one diagnostic, however many entries
    /// fail it (the first, and a count of the rest); every entry visited is
    /// charged as work.
    fn strict(
        &self,
        strict: &StrictForced,
        value: &mut ToolCalls,
        constraint: &str,
        diagnostics: &mut Vec<String>,
        governor: &mut Governor,
    ) -> Result<Extras, StopReason> {
        let mut extras = Extras::default();
        if strict.exactly_one {
            // Every rejected attempt, whatever its reason, is a call
            // attempt beyond the one call.
            governor.charge(
                Resource::Work,
                size(value.calls.len().saturating_add(value.rejected.len())),
            )?;
            extras = Extras {
                calls: value.calls.len().saturating_sub(1),
                attempts: value.rejected.len(),
            };
            for call in value.calls.iter_mut().skip(1) {
                if !matches!(call.outcome, CallOutcome::Unresolved { .. }) {
                    call.outcome = CallOutcome::Unresolved {
                        reason: UnresolvedReason::ToolChoiceCardinality,
                    };
                }
            }
            if let Some(diagnostic) = extras.diagnostic(constraint) {
                diagnostics.push(diagnostic);
            }
        }
        if !strict.allow_content {
            governor.charge(
                Resource::Work,
                size(value.content.len().saturating_add(value.rejected.len())),
            )?;
            let mut offending = value.content.iter().filter_map(|range| {
                let text = self.output.get(range.clone()).unwrap_or_default();
                text.iter()
                    .position(|byte| !byte.is_ascii_whitespace())
                    .map(|offset| range.start + offset)
            });
            if let Some(first) = offending.next() {
                diagnostics.push(format!(
                    "content at byte {first} is not allowed under {constraint}{}",
                    more(offending.count())
                ));
            }
            // A rejected attempt's bytes are outside every call: content.
            if let Some(first) = value.rejected.first() {
                diagnostics.push(format!(
                    "rejected call attempt at bytes {}..{} is not allowed under {constraint}{}",
                    first.span.start,
                    first.span.end,
                    more(value.rejected.len() - 1)
                ));
            }
        }
        if !strict.allow_reasoning {
            governor.charge(Resource::Work, size(value.reasoning.len()))?;
            if let Some(first) = value.reasoning.first() {
                diagnostics.push(format!(
                    "reasoning at bytes {}..{} is not allowed under {constraint}{}",
                    first.start,
                    first.end,
                    more(value.reasoning.len() - 1)
                ));
            }
        }
        Ok(extras)
    }

    /// The unmodeled keywords of an adapter tool's schema on the call's
    /// path, recorded on the call, and their validations.
    fn keywords(
        &self,
        call: &mut ToolCall,
        passed: bool,
        governor: &mut Governor,
    ) -> Result<Vec<Validation>, StopReason> {
        let Some(tool) = call.tool.as_deref().and_then(|name| self.tools.tool(name)) else {
            return Ok(Vec::new());
        };
        if !matches!(tool.source(), ToolSource::Declaration(_)) {
            return Ok(Vec::new());
        }
        let mut validations = Vec::new();
        for (pointer, keyword) in tool.unmodeled_keywords() {
            governor.charge(Resource::Work, size(pointer.len()).saturating_add(1))?;
            if !on_path(tool.argument_pointer(pointer), &call.arguments, governor)? {
                continue;
            }
            // A reshaping keyword (`$ref`, `anyOf`, ...) or an unmodeled
            // `type` (a type array) is used when the exact schema typed a
            // value under it (`schema-typed` names the parameter's schema
            // pointer); the other consulted keywords decide how repair
            // treats their scope.
            let used = if reshaping(keyword) || keyword == "type" {
                call.repairs.evidence.iter().any(|evidence| {
                    evidence.rule == "tool_call.value.schema-typed"
                        && typed_schema(&evidence.explanation).is_some_and(|typed| {
                            pointer == typed
                                || pointer.starts_with(&format!("{typed}/"))
                                || typed.starts_with(&format!("{pointer}/"))
                        })
                })
            } else {
                consulted_by_repair(keyword)
            };
            let validation = Validation {
                validator: TOOL_CALL_VALIDATOR.into(),
                constraint: format!("keyword {pointer}/{}", pointer_segment(keyword)),
                passed,
                diagnostics: vec![
                    if used {
                        "used"
                    } else {
                        "ignored-by-repair; enforced-by-validation"
                    }
                    .into(),
                ],
            };
            governor.charge(
                Resource::ScratchBytes,
                validation_bytes(&validation)
                    .saturating_add(size(pointer.len() + keyword.len()))
                    .saturating_add(ENTRY_BYTES),
            )?;
            call.keywords.push(KeywordUse {
                pointer: pointer.clone(),
                keyword: keyword.clone(),
                used,
            });
            validations.push(validation);
        }
        Ok(validations)
    }

    /// How the turn finished (see [`Finish`]).
    fn finish(&self, value: &ToolCalls) -> Finish {
        let truncation = self.request.stop.is_truncation();
        let cut = |call: &ToolCall| {
            matches!(
                call.outcome,
                CallOutcome::Unresolved {
                    reason: UnresolvedReason::Truncated
                }
            )
        };
        let output_end = self.output.len();
        let rejected = value.rejected.iter().any(|rejected| {
            !(truncation
                && rejected.reason == Rejection::Unterminated
                && rejected.span.end == output_end)
        });
        let broken = value.calls.iter().any(|call| {
            !cut(call)
                && matches!(
                    call.outcome,
                    CallOutcome::Unresolved { .. } | CallOutcome::Ambiguous
                )
        });
        // The finishes with calls need a call that may run (the rule
        // `verdict` and the emitters use): calls that are all guesses on
        // mutating tools are a turn with nothing to run.
        let usable = value
            .calls
            .iter()
            .any(ToolCall::is_usable_accepting_guesses);
        if rejected || broken || !value.tool_choice.passed {
            Finish::Unresolved
        } else if truncation {
            if usable {
                Finish::TruncatedWithCalls
            } else {
                Finish::Truncated
            }
        } else if value.calls.is_empty() {
            Finish::ContentOnly
        } else if usable {
            Finish::Calls
        } else {
            Finish::Unresolved
        }
    }

    /// Publish the readings and decide the outcome.
    fn conclude(
        &self,
        readings: Vec<Reading>,
        governor: &mut Governor,
    ) -> Result<Done, StopReason> {
        if let Some(reading) = readings.iter().find(|reading| !reading.inexact.is_empty()) {
            // No exact reading of the message exists: nothing is reported
            // as a value, and no repair reaches the receipt.
            return Ok(Done {
                outcome: Outcome::Unresolved,
                evidence: Vec::new(),
                losses: Vec::new(),
                inferences: Vec::new(),
                validations: vec![Validation {
                    validator: TOOL_CALL_VALIDATOR.into(),
                    constraint: "effort=exact".into(),
                    passed: false,
                    diagnostics: reading.inexact.clone(),
                }],
                alternatives: Vec::new(),
                stopped: StopReason::NoCandidate,
            });
        }
        let mut readings = same_calls(self, readings, governor)?;
        let reject = self.request.ambiguity == AmbiguityPolicy::Reject;
        let mut log = Log {
            entries: Vec::new(),
            index: BTreeMap::new(),
        };
        let mut forest = Forest::new(governor);
        let per_call = readings.len() == 1 && readings[0].ambiguous_calls().next().is_some();
        let derivations = if reject && per_call {
            call_derivations(&readings[0], &mut forest, &mut log, governor)?
        } else {
            let mut derivations = whole_derivations(&readings, &mut forest, &mut log, governor)?;
            if readings.len() > 1 {
                let groups = indistinguishable(&derivations, governor)?;
                if groups.len() < readings.len() {
                    // Different calls no ledger tells apart: the dialect
                    // that read each call is the evidence, attested as a
                    // validation of the call.
                    for group in groups.iter().filter(|group| group.len() > 1) {
                        for &member in group {
                            attest_dialects(&mut readings[member], governor)?;
                        }
                    }
                    derivations = whole_derivations(&readings, &mut forest, &mut log, governor)?;
                    let groups = indistinguishable(&derivations, governor)?;
                    if groups.len() < readings.len() {
                        // Still nothing tells them apart: one reading.
                        readings = merge(self, readings, &groups, governor)?;
                        derivations =
                            whole_derivations(&readings, &mut forest, &mut log, governor)?;
                    }
                }
            }
            derivations
        };
        let published = forest.publish(&derivations, governor)?;
        debug_assert!(
            published.order.windows(2).all(|pair| !forest
                .distinguishing(&derivations[pair[0]], &derivations[pair[1]])
                .is_empty()),
            "an ambiguity without distinguishing evidence"
        );
        let ambiguous = readings.len() > 1 || per_call;
        if reject {
            log.add(
                MESSAGE,
                &Validation {
                    validator: TOOL_CALL_VALIDATOR.into(),
                    constraint: "tool_call.ambiguity".into(),
                    passed: !ambiguous,
                    diagnostics: ambiguity_diagnostics(&readings),
                },
                governor,
            )?;
        }
        let (outcome, stopped) = if reject && ambiguous {
            (Outcome::Unresolved, StopReason::NoCandidate)
        } else if readings.len() > 1 {
            let values: Vec<ToolCalls> =
                readings.into_iter().map(|reading| reading.value).collect();
            (
                Outcome::Ambiguous(permute(&values, &published.order)),
                StopReason::Completed,
            )
        } else {
            let reading = readings.pop().expect("a turn has at least one reading");
            let doubtful = reading
                .ledgers()
                .any(|ledger| ledger.guessed() || ledger.data_loss());
            (
                if doubtful {
                    Outcome::BestGuess(reading.value)
                } else {
                    Outcome::Resolved(reading.value)
                },
                StopReason::Completed,
            )
        };
        Ok(Done {
            outcome,
            evidence: published.evidence,
            losses: published.losses,
            inferences: published.inferences,
            validations: log.entries,
            alternatives: published.alternatives,
            stopped,
        })
    }
}

/// What id assignment needs of one call: its index, the id it carries,
/// and whether its dialect's markup provides for an id.
pub(crate) struct IdSource {
    pub(crate) index: usize,
    pub(crate) carried: Option<String>,
    pub(crate) slot: bool,
}

impl IdSource {
    pub(crate) fn of(call: &ToolCall) -> Self {
        Self {
            index: call.index,
            carried: call.source_id.clone(),
            slot: call.dialect.grammar().call_carries_ids(),
        }
    }
}

/// The id of every call under `policy` (see [`IdPolicy`]), by position,
/// and the id counter after them. `turn` is the digest of the turn's
/// output ([`IdPolicy::Turn`]); `dedup(position, duplicate, id, governor)`
/// is told of every id assigned in place of a missing or repeated carried
/// one under [`IdPolicy::Source`] and [`IdPolicy::Turn`] (`duplicate` when
/// the call carried an id an earlier call holds). Recovery and
/// [`emit`](super::emit) share it, so one policy numbers calls the same way.
pub(crate) fn assign_ids(
    policy: &IdPolicy,
    turn: &Digest,
    calls: &[IdSource],
    governor: &mut Governor,
    mut dedup: impl FnMut(usize, bool, &str, &mut Governor) -> Result<(), StopReason>,
) -> Result<(Vec<String>, u64), StopReason> {
    let count = size(calls.len());
    let mut ids = Vec::with_capacity(calls.len());
    match policy {
        IdPolicy::Prefix { prefix, start } => {
            let next = start.checked_add(count).ok_or_else(|| {
                invalid_input(format!(
                    "id counter {start} overflows numbering {count} calls"
                ))
            })?;
            for call in calls {
                let id = format!("{prefix}{}", start + size(call.index));
                governor.charge(Resource::ScratchBytes, size(id.len()))?;
                ids.push(id);
            }
            Ok((ids, next))
        }
        IdPolicy::Generator(generator) => {
            let mut taken: BTreeMap<String, usize> = BTreeMap::new();
            for call in calls {
                let index = call.index;
                let id = generator.id(index);
                governor.charge(Resource::Work, size(id.len()).saturating_add(1))?;
                governor.charge(
                    Resource::ScratchBytes,
                    size(id.len().saturating_mul(2)).saturating_add(ENTRY_BYTES),
                )?;
                if let Some(first) = taken.get(&id) {
                    return Err(invalid_input(format!(
                        "the id generator returned `{id}` for calls {first} and {index}"
                    )));
                }
                taken.insert(id.clone(), index);
                ids.push(id);
            }
            Ok((ids, count))
        }
        IdPolicy::Source | IdPolicy::Turn => {
            let turn = matches!(policy, IdPolicy::Turn).then(|| turn_id_prefix(turn));
            let carried: BTreeSet<&String> = calls
                .iter()
                .filter_map(|call| call.carried.as_ref())
                .collect();
            governor.charge(
                Resource::ScratchBytes,
                carried
                    .iter()
                    .map(|id| size(id.len()).saturating_add(ENTRY_BYTES))
                    .sum::<u64>()
                    .saturating_add(turn.as_ref().map_or(0, |tag| size(tag.len()))),
            )?;
            let mut taken: BTreeSet<String> = BTreeSet::new();
            for (position, call) in calls.iter().enumerate() {
                governor.charge(Resource::Work, 1)?;
                let index = call.index;
                let id = match &call.carried {
                    Some(id) if !taken.contains(id) => id.clone(),
                    source => {
                        let (base, separator) = turn.as_ref().map_or_else(
                            || {
                                let carried = source.as_deref().unwrap_or("call");
                                (format!("{carried}#{index}"), "#")
                            },
                            |tag| (format!("{tag}{index}"), "_"),
                        );
                        let mut id = base.clone();
                        let mut suffix = 1_u64;
                        while taken.contains(&id) || carried.contains(&id) {
                            governor.charge(Resource::Work, 1)?;
                            id = format!("{base}{separator}{suffix}");
                            suffix += 1;
                        }
                        // Under `Turn`, numbering a call whose dialect
                        // carries no id is the policy, not a repair.
                        if turn.is_none() || source.is_some() || call.slot {
                            dedup(position, source.is_some(), &id, governor)?;
                        }
                        id
                    }
                };
                governor.charge(
                    Resource::ScratchBytes,
                    size(id.len().saturating_mul(2)).saturating_add(ENTRY_BYTES),
                )?;
                taken.insert(id.clone());
                ids.push(id);
            }
            Ok((ids, count))
        }
    }
}

/// The options of one contested span with equivalent segmentations
/// ([`equivalent_options`]) folded together, in the order of their kept
/// segmentations. The kept segmentation of a group is its member with the
/// most body evidence (see [`Reading::body_evidence`]), the earliest on a tie: the member [`merge`] keeps
/// among whole readings that differ only in this span.
fn fold_options<'t>(
    options: &'t [Vec<ResolvedSegment>],
    output: &[u8],
    governor: &mut Governor,
) -> Result<Vec<Fold<'t>>, StopReason> {
    // Each group: its member indices, in option order.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, option) in options.iter().enumerate() {
        let mut found = None;
        for (group, members) in groups.iter().enumerate() {
            if equivalent_options(&options[members[0]], option, governor)? {
                found = Some(group);
                break;
            }
        }
        governor.charge(Resource::ScratchBytes, ENTRY_BYTES)?;
        match found {
            Some(group) => groups[group].push(index),
            None => groups.push(vec![index]),
        }
    }
    let mut folds = Vec::with_capacity(groups.len());
    for members in &groups {
        let mut kept = members[0];
        for &member in &members[1..] {
            if Fold::body_evidence(&options[member], output)
                > Fold::body_evidence(&options[kept], output)
            {
                kept = member;
            }
        }
        let mut fold = Fold::new(&options[kept], governor)?;
        for &member in members.iter().filter(|&&member| member != kept) {
            fold.absorb(&options[member], governor)?;
        }
        folds.push((kept, fold));
    }
    // The readings' order is the order of the kept segmentations.
    folds.sort_unstable_by_key(|(kept, _)| *kept);
    Ok(folds.into_iter().map(|(_, fold)| fold).collect())
}

/// Record on a reading built from folded options the dialects of every
/// folded segmentation: each call's [`ToolCall::candidate_dialects`], each
/// rejected attempt's [`RejectedCall::candidate_dialects`], and the
/// reading's attempts.
fn apply_folds(
    reading: &mut Reading,
    folds: &[&Fold<'_>],
    governor: &mut Governor,
) -> Result<(), StopReason> {
    // Contested spans are disjoint, so each call's range is in one fold.
    let mut calls: BTreeMap<(usize, usize), &BTreeSet<DialectId>> = BTreeMap::new();
    for fold in folds {
        for (&start, dialects) in &fold.attempts {
            #[expect(
                clippy::mutable_key_type,
                reason = "DialectId Eq/Ord use only immutable grammar name and digest; scratch-meter atomics never participate in key identity"
            )]
            let known = reading.attempts.entry(start).or_default();
            governor.charge(Resource::Work, size(dialects.len()).saturating_add(1))?;
            for dialect in dialects {
                if known.insert(dialect.clone()) {
                    governor.charge(Resource::ScratchBytes, ENTRY_BYTES)?;
                }
            }
        }
        for (&range, dialects) in &fold.calls {
            governor.charge(Resource::Work, 1)?;
            if dialects.len() > 1 {
                calls.insert(range, dialects);
            }
        }
    }
    if !calls.is_empty() {
        for call in &mut reading.value.calls {
            governor.charge(Resource::Work, 1)?;
            if let Some(dialects) = calls.get(&(call.span.start, call.span.end)) {
                governor.charge(
                    Resource::ScratchBytes,
                    size(dialects.len()).saturating_mul(ENTRY_BYTES),
                )?;
                call.candidate_dialects = dialects.iter().cloned().collect();
            }
        }
    }
    for rejected in &mut reading.value.rejected {
        if let Some(dialects) = reading.attempts.get(&rejected.span.start)
            && dialects.len() > rejected.candidate_dialects.len()
        {
            governor.charge(
                Resource::ScratchBytes,
                size(dialects.len()).saturating_mul(ENTRY_BYTES),
            )?;
            rejected.candidate_dialects = dialects.iter().cloned().collect();
        }
    }
    Ok(())
}

/// How far a rejected attempt's dialect read before it failed: the bytes
/// from its opener to the byte it could not consume, or to the end of its
/// span when the output stopped inside it.
const fn reach(rejected: &RejectedCall) -> usize {
    reach_of(&rejected.span, rejected.reason)
}

/// The body evidence of a reading's rejected attempts (see
/// [`Reading::body_evidence`]), compared greatest first: bytes read, then
/// openers read in their canonical spelling.
type BodyEvidence = (usize, usize);

/// The [`BodyEvidence`] of rejected attempts given as (reach, dialect, bytes).
fn evidence<'a>(rejected: impl Iterator<Item = (usize, &'a DialectId, &'a [u8])>) -> BodyEvidence {
    let (mut reach, mut canonical) = (0, 0);
    for (bytes_read, dialect, bytes) in rejected {
        reach += bytes_read;
        if dialect.grammar().call_ends().iter().any(|end| {
            end.openers()
                .first()
                .is_some_and(|opener| bytes.starts_with(opener))
        }) {
            canonical += 1;
        }
    }
    (reach, canonical)
}

/// [`reach`] of an attempt over `span` rejected for `reason`.
const fn reach_of(span: &Range<usize>, reason: Rejection) -> usize {
    let end = match reason {
        Rejection::Malformed { at }
        | Rejection::PythonicPositionalRejected { at }
        | Rejection::InvalidUtf8 { at }
        | Rejection::Overlapped { at } => at,
        Rejection::Unterminated | Rejection::UnsupportedDialect { .. } => span.end,
    };
    end.saturating_sub(span.start)
}

/// A call's wire form without its dialect labels: what the call is, not
/// which dialect read it.
fn call_identity(call: &ToolCall) -> Node {
    let mut node = call.to_node();
    if let Value::Object(members) = &mut node.value {
        members.retain(|member| member.key != "dialect" && member.key != "candidate_dialects");
    }
    node
}

/// Extend each call's [`ToolCall::candidate_dialects`] in `calls` with the
/// dialects of the call at the same position in `others` when that call is
/// the same call ([`call_identity`]).
fn union_call_dialects(
    calls: &mut [ToolCall],
    others: &[&[ToolCall]],
    governor: &mut Governor,
) -> Result<(), StopReason> {
    for (index, call) in calls.iter_mut().enumerate() {
        let identity = call_identity(call).to_json();
        #[expect(
            clippy::mutable_key_type,
            reason = "DialectId Eq/Ord use only immutable grammar name and digest; scratch-meter atomics never participate in key identity"
        )]
        let mut dialects: BTreeSet<DialectId> = call.candidate_dialects.iter().cloned().collect();
        for other in others.iter().filter_map(|calls| calls.get(index)) {
            let same = call_identity(other).to_json();
            governor.charge(
                Resource::Work,
                size(identity.len().saturating_add(same.len())),
            )?;
            if same == identity {
                dialects.extend(other.candidate_dialects.iter().cloned());
            }
        }
        governor.charge(
            Resource::ScratchBytes,
            size(dialects.len()).saturating_mul(ENTRY_BYTES),
        )?;
        call.candidate_dialects = dialects.into_iter().collect();
    }
    Ok(())
}

/// The digest of everything a reading recovers: its calls (each with its
/// outcome, repairs, and alternatives, whichever dialect read it), the calls embedded in
/// reasoning, the reasoning blocks (each block's span and marker-free text,
/// so readings that divide the same bytes into different blocks stay
/// distinct), and every message-level ledger other
/// than a rejected attempt's. Content ranges and rejected attempts, the
/// bytes outside every call, are not part of it.
///
/// The block split is a defensive part of the key: no recognizer input is
/// known to yield two readings that differ only in it, because every
/// dialect of a set shares one reasoning-marker matcher and the recognizer
/// keeps one cheapest block path, so contested spans differ in calls, never
/// in blocks (an exhaustive search of about 600,000 outputs over
/// overlapping marker pairs found none, and [`equivalent_options`] already
/// compares whole reasoning segments). The key keeps the split so a future
/// recognizer that does produce such readings cannot have them merged,
/// which would drop a block boundary the caller can observe;
/// `calls_key_distinguishes_reasoning_block_splits` pins it.
fn calls_key(reading: &Reading, governor: &mut Governor) -> Result<Digest, StopReason> {
    let value = &reading.value;
    let json = object([
        ("calls", array(value.calls.iter().map(call_identity))),
        ("embedded", array(value.embedded.iter().map(call_identity))),
        (
            "reasoning",
            array(value.reasoning_blocks.iter().map(|block| {
                object([
                    ("span", byte_range(&block.span)),
                    ("text", byte_range(&block.text)),
                ])
            })),
        ),
        (
            "messages",
            array(reading.parts.iter().filter_map(|part| match part {
                Part::Message(ledger) => Some(ledger_node(ledger)),
                Part::Call(_) | Part::Rejected(_) => None,
            })),
        ),
    ])
    .to_json();
    governor.charge(Resource::Work, size(json.len()))?;
    let _lease = governor.lease_scratch(size(json.len()))?;
    Ok(Digest::of(json.as_bytes()))
}

/// The readings with distinct recovered calls ([`calls_key`]). Readings
/// that recover the same calls and differ only in which dialect read each
/// call and how the bytes outside every call divide into rejected attempts
/// and content (which dialect's attempt was rejected, and where it failed)
/// are one reading; see [`merge`].
fn same_calls(
    recovery: &Recovery<'_>,
    readings: Vec<Reading>,
    governor: &mut Governor,
) -> Result<Vec<Reading>, StopReason> {
    if readings.len() < 2 {
        return Ok(readings);
    }
    let mut keys: Vec<Digest> = Vec::with_capacity(readings.len());
    let mut groups: Vec<Vec<usize>> = Vec::with_capacity(readings.len());
    for (position, reading) in readings.iter().enumerate() {
        let key = calls_key(reading, governor)?;
        governor.charge(Resource::Work, size(keys.len()).saturating_add(1))?;
        if let Some(group) = keys.iter().position(|known| *known == key) {
            groups[group].push(position);
        } else {
            keys.push(key);
            groups.push(vec![position]);
        }
    }
    if groups.len() == readings.len() {
        return Ok(readings);
    }
    merge(recovery, readings, &groups, governor)
}

/// Each group of `readings` (indices, best first) as one reading: the
/// member whose rejected attempts' dialects read the most bytes before
/// failing (the body evidence, then canonical openers; see
/// [`Reading::body_evidence`]), the earliest on a tie, with every rejected
/// attempt's [`RejectedCall::candidate_dialects`] extended to the dialects
/// that attempted a call at the same opener in any member, and every
/// call's (and embedded call's) [`ToolCall::candidate_dialects`] extended
/// to the dialects that read the same call in any member.
///
/// The merge fails closed: members can differ in how they divide the bytes
/// outside every call (a rejected attempt in one is content in another), and
/// a strict tool choice counts them differently, so the merged reading's
/// tool-choice check fails when any member's does, whichever member is kept.
/// Its diagnostics are then the deduplicated, sorted union of the members'
/// (one cardinality diagnostic, with the largest count). The calls are the
/// same in every member (they are the key), so the cardinality outcomes the
/// failing member gave them are the kept member's. The keep order already
/// favours failing: a rejected attempt is body evidence and content is not,
/// so the member that reads the bytes as an attempt is kept. Failing closed
/// does not rely on that order; it holds for any kept member.
fn merge(
    recovery: &Recovery<'_>,
    readings: Vec<Reading>,
    groups: &[Vec<usize>],
    governor: &mut Governor,
) -> Result<Vec<Reading>, StopReason> {
    let mut slots: Vec<Option<Reading>> = readings.into_iter().map(Some).collect();
    let mut merged = Vec::with_capacity(groups.len());
    for group in groups {
        let mut best = group[0];
        for &member in &group[1..] {
            let evidence = |at: usize| slots[at].as_ref().map(Reading::body_evidence);
            if evidence(member) > evidence(best) {
                best = member;
            }
        }
        let mut attempts: BTreeMap<usize, BTreeSet<DialectId>> = BTreeMap::new();
        for &member in group {
            let reading = slots[member]
                .as_ref()
                .expect("each reading is in one group");
            for (&start, dialects) in &reading.attempts {
                governor.charge(Resource::Work, size(dialects.len()).saturating_add(1))?;
                attempts
                    .entry(start)
                    .or_default()
                    .extend(dialects.iter().cloned());
            }
        }
        let checks: Vec<(bool, Vec<String>, Extras)> = group
            .iter()
            .filter_map(|&member| slots[member].as_ref())
            .map(|other| {
                (
                    other.value.tool_choice.passed,
                    other.value.tool_choice.diagnostics.clone(),
                    other.extras,
                )
            })
            .collect();
        let mut reading = slots[best].take().expect("each reading is in one group");
        let members: Vec<&Reading> = group
            .iter()
            .filter_map(|&member| slots[member].as_ref())
            .collect();
        let calls: Vec<&[ToolCall]> = members.iter().map(|other| &other.value.calls[..]).collect();
        union_call_dialects(&mut reading.value.calls, &calls, governor)?;
        let embedded: Vec<&[ToolCall]> = members
            .iter()
            .map(|other| &other.value.embedded[..])
            .collect();
        union_call_dialects(&mut reading.value.embedded, &embedded, governor)?;
        for rejected in &mut reading.value.rejected {
            #[expect(
                clippy::mutable_key_type,
                reason = "DialectId Eq/Ord use only immutable grammar name and digest; scratch-meter atomics never participate in key identity"
            )]
            let candidates = &attempts[&rejected.span.start];
            governor.charge(
                Resource::ScratchBytes,
                size(candidates.len()).saturating_mul(ENTRY_BYTES),
            )?;
            rejected.candidate_dialects = candidates.iter().cloned().collect();
        }
        reading.attempts = attempts;
        fail_closed(recovery, &mut reading, &checks, governor)?;
        merged.push(reading);
    }
    Ok(merged)
}

/// Make `reading`, the kept member of a merged group, carry the tool-choice
/// check of every member (`checks`: each member's `passed`, diagnostics and
/// extras, the kept member included): failed if any failed, with the
/// deduplicated union of the diagnostics, sorted so the order of the
/// readings cannot show.
fn fail_closed(
    recovery: &Recovery<'_>,
    reading: &mut Reading,
    checks: &[(bool, Vec<String>, Extras)],
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let agree = checks.iter().all(|(passed, diagnostics, _)| {
        *passed == reading.value.tool_choice.passed
            && *diagnostics == reading.value.tool_choice.diagnostics
    });
    if agree {
        return Ok(());
    }
    let constraint = reading.value.tool_choice.choice.constraint();
    let mut extras = Extras::default();
    let mut union: BTreeSet<String> = BTreeSet::new();
    let mut passed = true;
    for (member_passed, diagnostics, member_extras) in checks {
        passed &= *member_passed;
        extras.attempts = extras.attempts.max(member_extras.attempts);
        extras.calls = extras.calls.max(member_extras.calls);
        let cardinality = member_extras.diagnostic(&constraint);
        for diagnostic in diagnostics {
            governor.charge(Resource::Work, size(diagnostic.len()).saturating_add(1))?;
            if cardinality.as_ref() != Some(diagnostic) {
                union.insert(diagnostic.clone());
            }
        }
    }
    if let Some(diagnostic) = extras.diagnostic(&constraint) {
        union.insert(diagnostic);
    }
    let diagnostics: Vec<String> = union.into_iter().collect();
    for diagnostic in &diagnostics {
        governor.charge(
            Resource::ScratchBytes,
            size(diagnostic.len()).saturating_add(ENTRY_BYTES),
        )?;
    }
    let newly_failed = reading.value.tool_choice.passed && !passed;
    reading.extras = extras;
    reading.value.tool_choice.passed = passed;
    reading
        .value
        .tool_choice
        .diagnostics
        .clone_from(&diagnostics);
    if let Some((_, validation)) = reading.validations.get_mut(reading.choice_validation) {
        validation.passed = passed;
        validation.diagnostics = diagnostics;
    }
    if newly_failed {
        // A failed choice makes the finish `Unresolved` and nothing emitted.
        reading.value.finish = recovery.finish(&reading.value);
        reading.value.emitted = emit::emission(
            recovery.tools,
            &reading.value,
            &recovery.request.emit,
            governor,
        )?;
    }
    Ok(())
}

/// Record, as a passing `dialect=ID` validation of each call, the dialect
/// that read it: the evidence that tells apart whole-message readings whose
/// calls differ but whose ledgers do not.
fn attest_dialects(reading: &mut Reading, governor: &mut Governor) -> Result<(), StopReason> {
    for call in &reading.value.calls {
        let constraint = format!("dialect={}", call.dialect.id());
        governor.charge(
            Resource::ScratchBytes,
            size(constraint.len()).saturating_add(ENTRY_BYTES),
        )?;
        reading.validations.push((
            (call.span.start, call.span.end),
            Validation {
                validator: TOOL_CALL_VALIDATOR.into(),
                constraint,
                passed: true,
                diagnostics: Vec::new(),
            },
        ));
    }
    Ok(())
}

/// One derivation per whole-message reading: its ledgers and validations,
/// identified by the digest of the reading's wire form (no digest for a
/// single reading).
fn whole_derivations(
    readings: &[Reading],
    forest: &mut Forest,
    log: &mut Log,
    governor: &mut Governor,
) -> Result<Vec<Derivation>, StopReason> {
    let mut derivations = Vec::with_capacity(readings.len());
    for reading in readings {
        let output = if readings.len() < 2 {
            Digest::of(b"")
        } else {
            let json = reading.value.to_node().to_json();
            governor.charge(Resource::Work, size(json.len()))?;
            let _lease = governor.lease_scratch(size(json.len()))?;
            Digest::of(json.as_bytes())
        };
        let mut derivation = Derivation::new(output, reading.cost);
        for ledger in reading.ledgers() {
            push_ledger(forest, &mut derivation, ledger, governor)?;
        }
        for (key, validation) in &reading.validations {
            let at = log.add(*key, validation, governor)?;
            derivation.push(forest.validation(governor, at, validation.passed)?);
        }
        derivations.push(derivation);
    }
    Ok(derivations)
}

/// The derivations grouped by the provenance they depend on, each group
/// in [`Forest::rank`] order: derivations in one group have no
/// distinguishing evidence ([`Forest::distinguishing`] is empty).
fn indistinguishable(
    derivations: &[Derivation],
    governor: &mut Governor,
) -> Result<Vec<Vec<usize>>, StopReason> {
    let mut groups: Vec<(Vec<NodeId>, Vec<usize>)> = Vec::new();
    for position in Forest::rank(derivations) {
        let nodes = derivations[position].nodes();
        governor.charge(
            Resource::Work,
            size(nodes.len()).saturating_mul(size(groups.len()).saturating_add(1)),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(nodes.len()).saturating_mul(ENTRY_BYTES),
        )?;
        let mut nodes = nodes.to_vec();
        nodes.sort_unstable();
        match groups.iter_mut().find(|(known, _)| *known == nodes) {
            Some((_, members)) => members.push(position),
            None => groups.push((nodes, vec![position])),
        }
    }
    Ok(groups.into_iter().map(|(_, members)| members).collect())
}

/// One derivation per competing reading of each ambiguous call of
/// `reading`: the reading's other ledgers and validations with that call's
/// replaced by the competing reading's.
fn call_derivations(
    reading: &Reading,
    forest: &mut Forest,
    log: &mut Log,
    governor: &mut Governor,
) -> Result<Vec<Derivation>, StopReason> {
    let mut derivations = Vec::new();
    for index in reading.ambiguous_calls() {
        let call = &reading.value.calls[index];
        let facts = &reading.facts[index];
        let key = (call.span.start, call.span.end);
        let selected = CallReading {
            tool: call.tool.clone(),
            effects: call.effects,
            policy: call.policy,
            arguments: call.arguments.clone(),
            cost: facts.cost,
            repairs: call.repairs.clone(),
            validations: facts.validations.clone(),
            blocker: None,
        };
        governor.charge(Resource::ScratchBytes, reading_bytes(&selected))?;
        let tied = facts
            .competing
            .saturating_sub(1)
            .min(call.alternatives.len());
        for candidate in core::iter::once(&selected).chain(&call.alternatives[..tied]) {
            let json = object([
                ("call", integer(size(index))),
                ("reading", reading_node(candidate)),
            ])
            .to_json();
            governor.charge(Resource::Work, size(json.len()))?;
            let mut derivation = Derivation::new(Digest::of(json.as_bytes()), candidate.cost);
            for (part, ledger) in reading.parts.iter().zip(reading.ledgers()) {
                let ledger = match part {
                    Part::Call(other) if *other == index => &candidate.repairs,
                    _ => ledger,
                };
                push_ledger(forest, &mut derivation, ledger, governor)?;
            }
            let others = reading
                .validations
                .iter()
                .filter(|(other, _)| *other != key)
                .map(|(other, validation)| (*other, validation));
            let own = candidate
                .validations
                .iter()
                .map(|validation| (key, validation));
            for (at, validation) in others.chain(own) {
                let index = log.add(at, validation, governor)?;
                derivation.push(forest.validation(governor, index, validation.passed)?);
            }
            derivations.push(derivation);
        }
    }
    Ok(derivations)
}

/// The diagnostics of the `tool_call.ambiguity` validation.
fn ambiguity_diagnostics(readings: &[Reading]) -> Vec<String> {
    if readings.len() > 1 {
        return vec![format!(
            "{} competing whole-message readings",
            readings.len()
        )];
    }
    readings
        .iter()
        .flat_map(|reading| {
            reading.ambiguous_calls().map(|index| {
                format!(
                    "{} has {} competing readings",
                    label(&reading.value.calls[index]),
                    reading.facts[index].competing
                )
            })
        })
        .collect()
}

/// A resolved call as a [`ToolCall`] (its id assigned later).
fn tool_call(
    resolved: &ResolvedCall,
    index: usize,
    governor: &mut Governor,
) -> Result<(ToolCall, CallFacts), StopReason> {
    let reading = &resolved.reading;
    let mut bytes = reading_bytes(reading);
    for alternative in &resolved.alternatives {
        bytes = bytes.saturating_add(reading_bytes(alternative));
    }
    governor.charge(
        Resource::ScratchBytes,
        bytes.saturating_add(size(resolved.distinguishing.len()).saturating_mul(ENTRY_BYTES)),
    )?;
    Ok((
        ToolCall {
            index,
            id: String::new(),
            source_id: resolved.id.as_ref().map(|id| id.value.clone()),
            tool: reading.tool.clone(),
            name: resolved.name.as_ref().map(|name| name.value.clone()),
            arguments: reading.arguments.clone(),
            span: resolved.span.clone(),
            dialect: resolved.dialect.clone(),
            candidate_dialects: vec![resolved.dialect.clone()],
            effects: reading.effects,
            policy: reading.policy,
            outcome: resolved.outcome.clone(),
            repairs: reading.repairs.clone(),
            alternatives: resolved.alternatives.clone(),
            distinguishing: resolved.distinguishing.clone(),
            keywords: Vec::new(),
        },
        CallFacts {
            cost: reading.cost,
            validations: reading.validations.clone(),
            competing: resolved.competing,
        },
    ))
}

/// Intern every entry of `ledger` into `forest` and record it on
/// `derivation`.
fn push_ledger(
    forest: &mut Forest,
    derivation: &mut Derivation,
    ledger: &CallLedger,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    for evidence in &ledger.evidence {
        derivation.push(forest.evidence(governor, evidence.clone())?);
    }
    for loss in &ledger.losses {
        derivation.push(forest.loss(governor, loss.loss.clone())?);
    }
    for inference in &ledger.inferences {
        derivation.push(forest.inference(governor, inference.clone())?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_call::{DialectSet, EmitTarget, Param, ToolDefinition};
    use crate::{Budget, Governor};

    const OUTPUT: &[u8] = b"<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a\"}}\n</tool_call>\n<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"b\"}} junk junk {{{\n</tool_call>";

    fn printed(emitted: Option<&crate::tool_call::Emission>) -> Option<usize> {
        match emitted {
            Some(crate::tool_call::Emission::OpenAiToolCalls(emitted)) => Some(emitted.calls.len()),
            _ => None,
        }
    }

    /// The two readings of [`OUTPUT`] a merge would join if two dialects
    /// contested the second body: `failing` reads it as a rejected attempt
    /// (the real Hermes reading), `passing` is the same reading with the
    /// attempt counted as content, as a dialect that did not attempt it
    /// would leave it (the strict check passes with content allowed).
    fn members(
        tools: &ToolSet,
        request: &ToolRecoveryRequest,
        dialects: &DialectSet,
        governor: &mut Governor,
    ) -> (Reading, Reading) {
        let recovery = Recovery {
            tools,
            request,
            dialects,
            output: OUTPUT,
            source: Digest::of(OUTPUT),
            policies: Policies {
                selection: request.policy,
                effects: request.effects.clone(),
                exact: false,
            },
        };
        let mut failing = recovery.recognized(governor).unwrap();
        let mut passing = recovery.recognized(governor).unwrap();
        let (failing, mut passing) = (failing.remove(0), passing.remove(0));
        assert!(!failing.value.tool_choice.passed);
        assert_eq!(
            failing.extras,
            Extras {
                calls: 0,
                attempts: 1
            }
        );
        passing.extras = Extras::default();
        passing.value.tool_choice.passed = true;
        passing.value.tool_choice.diagnostics = Vec::new();
        let index = passing.choice_validation;
        passing.validations[index].1.passed = true;
        passing.validations[index].1.diagnostics = Vec::new();
        passing.value.finish = recovery.finish(&passing.value);
        passing.value.emitted =
            emit::emission(tools, &passing.value, &request.emit, governor).unwrap();
        (failing, passing)
    }

    /// A merge of readings whose strict checks disagree fails closed
    /// whichever member it keeps and whatever the readings' order: the
    /// merged check fails with the same diagnostics, the finish is
    /// `Unresolved`, and nothing is emitted (R4, G3).
    #[test]
    fn merge_fails_closed_in_either_order() {
        let tools = ToolSet::compile(
            [ToolDefinition::new("read_file").param("path", Param::string().required())],
            Budget::default(),
        )
        .unwrap();
        let mut governor = Governor::new(Budget::default());
        let dialects = DialectSet::new([DialectId::Hermes], &mut governor).unwrap();
        let request = ToolRecoveryRequest::default()
            .emit(EmitTarget::OpenAiToolCalls)
            .tool_choice(ToolChoice::ForcedStrict(
                StrictForced::exactly_one("read_file").allow_content(true),
            ));
        let mut merged = Vec::new();
        for failing_first in [true, false] {
            let (failing, passing) = members(&tools, &request, &dialects, &mut governor);
            assert!(passing.value.tool_choice.passed);
            assert_eq!(printed(passing.value.emitted.as_ref()), Some(1));
            let recovery = Recovery {
                tools: &tools,
                request: &request,
                dialects: &dialects,
                output: OUTPUT,
                source: Digest::of(OUTPUT),
                policies: Policies {
                    selection: request.policy,
                    effects: request.effects.clone(),
                    exact: false,
                },
            };
            let readings = if failing_first {
                vec![failing, passing]
            } else {
                vec![passing, failing]
            };
            let readings = same_calls(&recovery, readings, &mut governor).unwrap();
            assert_eq!(readings.len(), 1, "the members recover the same calls");
            merged.push(readings.into_iter().next().unwrap());
        }
        for reading in &merged {
            let check = &reading.value.tool_choice;
            assert!(!check.passed, "{check:?}");
            assert_eq!(check.diagnostics.len(), 1, "{check:?}");
            assert!(check.diagnostics[0].starts_with("1 rejected call attempt beyond"));
            assert_eq!(reading.value.finish, Finish::Unresolved);
            assert_eq!(reading.value.emitted_calls().len(), 0);
            assert_eq!(printed(reading.value.emitted.as_ref()), Some(0));
            let (_, validation) = &reading.validations[reading.choice_validation];
            assert!(!validation.passed);
            assert_eq!(validation.diagnostics, check.diagnostics);
        }
        assert_eq!(
            merged[0].value.tool_choice.diagnostics,
            merged[1].value.tool_choice.diagnostics
        );
    }

    /// The reading of `output` (one reading, no contested span) under the
    /// default request.
    fn reading_of(output: &[u8], governor: &mut Governor) -> Reading {
        let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
        let request = ToolRecoveryRequest::default();
        let recovery = Recovery {
            tools: &tools,
            request: &request,
            dialects: DialectSet::built_in(),
            output,
            source: Digest::of(output),
            policies: Policies {
                selection: request.policy,
                effects: request.effects.clone(),
                exact: false,
            },
        };
        let mut readings = recovery.recognized(governor).unwrap();
        assert_eq!(readings.len(), 1);
        readings.remove(0)
    }

    /// Readings with the same calls and the same merged `reasoning` but
    /// different `reasoning_blocks` have different keys: the key hashes
    /// each block's span and marker-free text, not the merged ranges
    /// (R8, G8). Identical blocks give identical keys.
    #[test]
    fn calls_key_distinguishes_reasoning_block_splits() {
        let output = b"<think>a</think><think>b</think>";
        let mut governor = Governor::new(Budget::default());
        let two = reading_of(output, &mut governor);
        assert_eq!(two.value.reasoning, vec![0..32]);
        assert_eq!(two.value.reasoning_blocks.len(), 2);
        let key =
            |reading: &Reading, governor: &mut Governor| calls_key(reading, governor).unwrap();

        // Control: the same bytes read the same way share a key.
        let again = reading_of(output, &mut governor);
        assert_eq!(key(&two, &mut governor), key(&again, &mut governor));

        // One block over the same bytes: the merged range, calls, and
        // message ledgers are unchanged; only the block split differs.
        let mut one = reading_of(output, &mut governor);
        one.value.reasoning_blocks = vec![ReasoningBlock {
            span: 0..32,
            text: 7..24,
        }];
        assert_eq!(one.value.reasoning, two.value.reasoning);
        assert_eq!(one.value.calls.len(), two.value.calls.len());
        assert_ne!(
            key(&two, &mut governor),
            key(&one, &mut governor),
            "a different block split over the same merged reasoning must be a different reading"
        );

        // The same spans with different marker-free text also differ.
        let mut shifted = reading_of(output, &mut governor);
        shifted.value.reasoning_blocks[1].text = 22..24;
        assert_ne!(key(&two, &mut governor), key(&shifted, &mut governor));
    }
}
