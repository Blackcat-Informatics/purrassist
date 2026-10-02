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
//! [`ToolCalls::usable_accepting_guesses`] select what may run.
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
//! function of the output, the tool set, and the request alone.
//! [`ToolRecoveryRequest::sized`] sizes a budget for an output length.
use super::adapter::consulted_by_repair;
use super::emit::{self, Emission, EmitTarget};
use super::recognizer::{edit_entries, escaped};
use super::resolve::loss_kind;
use super::{
    CallLedger, CallLoss, CallOutcome, CallReading, DialectId, DialectSet, Effects, FromNode,
    FromNodeError, GenerationStop, Policies, PolicySelection, ReasoningPolicy, ReasoningSegment,
    Recognizer, Rejection, RepairPolicy, ResolvedCall, ResolvedPiece, ResolvedSegment,
    ResolvedTurn, ToolSet, ToolSource, UnresolvedReason, resolve_forest, rules,
};
use crate::contract::saturating_size as size;
use crate::contract::{
    AlternativeReceipt, AssistResult, Budget, Effort, Evidence, Governor, Inference, InferenceKind,
    Loss, Outcome, Receipt, Resource, SourceSpan, StopReason, Validation, invalid_input,
};
use crate::digest::Digest;
use crate::grammar::cost::{RECOVER_SCRATCH_PER_BYTE, RECOVER_WORK_PER_BYTE};
use crate::grammar::edit::LossKind;
use crate::json::{Node, Value, pointer_segment};
use crate::provenance::{Derivation, Forest, NodeId, ProvenanceNode, permute};
use crate::wire::{
    self, WireValue, array, byte_range, exact_bytes, inferences, integer, object, string,
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

/// How recovered calls receive ids (R5.6).
///
/// - `Prefix { prefix, start }`: call `i` is `{prefix}{start + i}`, and
///   [`ToolCalls::next_id_start`] is `start + calls`; passing it back as the
///   next round's `start` makes ids of successive rounds never collide. A
///   counter that would overflow `u64` is an invalid request.
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
/// of calls. The default is `Prefix { prefix: "call_", start: 0 }`.
///
/// ```
/// use purrassist_core::tool_call::IdPolicy;
/// let IdPolicy::Prefix { prefix, start } = IdPolicy::default() else { unreachable!() };
/// assert_eq!((prefix.as_str(), start), ("call_", 0));
/// assert_eq!(format!("{:?}", IdPolicy::Source), "Source");
/// ```
#[derive(Clone)]
pub enum IdPolicy {
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

impl Default for IdPolicy {
    fn default() -> Self {
        Self::Prefix {
            prefix: "call_".into(),
            start: 0,
        }
    }
}

impl fmt::Debug for IdPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
///
/// A failed check makes the turn's [`Finish`] `Unresolved`.
///
/// ```
/// use purrassist_core::tool_call::ToolChoice;
/// assert_eq!(ToolChoice::default(), ToolChoice::Auto);
/// assert_eq!(ToolChoice::Forced("read".into()).constraint(), "tool_choice=forced:read");
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
    /// `{"type":"tool","name":NAME}`.
    ///
    /// # Errors
    /// [`ToolChoiceError`] for a non-object, an unknown `type`, a `name`
    /// missing for `"tool"` or present otherwise, or any other member
    /// (including `disable_parallel_tool_use`, which this choice model does
    /// not represent and so never silently drops).
    ///
    /// ```
    /// use purrassist_core::{json, tool_call::ToolChoice};
    /// let parse = |text: &str| ToolChoice::from_anthropic(&json::parse(text).unwrap());
    /// assert_eq!(parse(r#"{"type":"any"}"#), Ok(ToolChoice::Required));
    /// assert_eq!(parse(r#"{"type":"tool","name":"read"}"#), Ok(ToolChoice::Forced("read".into())));
    /// assert!(parse(r#"{"type":"auto","disable_parallel_tool_use":true}"#).is_err());
    /// ```
    pub fn from_anthropic(node: &Node) -> Result<Self, ToolChoiceError> {
        if node.as_str().is_some() {
            return Err(choice_error("expected an object with a `type` member"));
        }
        let kind = {
            choice_members(node, &["type", "name"])?;
            choice_string(node, "type")?
        };
        let named = node.get("name").is_some();
        match (kind, named) {
            ("tool", true) => Ok(Self::Forced(choice_string(node, "name")?.to_owned())),
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
    /// `tool_choice=required`, or `tool_choice=forced:NAME`.
    ///
    /// See [`ToolChoice`] for an example.
    #[must_use]
    pub fn constraint(&self) -> String {
        match self {
            Self::Auto => "tool_choice=auto".into(),
            Self::None => "tool_choice=none".into(),
            Self::Required => "tool_choice=required".into(),
            Self::Forced(name) => format!("tool_choice=forced:{name}"),
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

/// One tool-call recovery request (one field per concept).
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
    /// The model's dialect allow-list.
    pub dialects: DialectSet,
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
    /// A request at `effort` with `budget = effort.budget()`, every built-in
    /// dialect ([`DialectSet::built_in`]), separate reasoning, a
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
            dialects: DialectSet::built_in(),
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

    /// [`Self::new`] with a budget sized for `input_len` output bytes:
    /// `work = preset + RECOVER_WORK_PER_BYTE · input_len`, `scratch_bytes =
    /// preset + RECOVER_SCRATCH_PER_BYTE · input_len` (see
    /// [`crate::grammar::cost`]), and `input_bytes` at least `input_len`;
    /// every other limit is the preset's. Saturating and deterministic.
    ///
    /// ```
    /// use purrassist_core::{Effort, grammar::cost};
    /// use purrassist_core::tool_call::ToolRecoveryRequest;
    /// let request = ToolRecoveryRequest::sized(Effort::Quick, 1 << 20);
    /// let preset = Effort::Quick.budget();
    /// assert_eq!(request.budget.work, preset.work + cost::RECOVER_WORK_PER_BYTE * (1 << 20));
    /// assert_eq!(request.budget.scratch_bytes, preset.scratch_bytes + cost::RECOVER_SCRATCH_PER_BYTE * (1 << 20));
    /// assert_eq!(request.budget.candidates, preset.candidates);
    /// ```
    #[must_use]
    pub fn sized(effort: Effort, input_len: usize) -> Self {
        let mut request = Self::new(effort);
        let length = size(input_len);
        let budget = &mut request.budget;
        budget.work = budget
            .work
            .saturating_add(RECOVER_WORK_PER_BYTE.saturating_mul(length));
        budget.scratch_bytes = budget
            .scratch_bytes
            .saturating_add(RECOVER_SCRATCH_PER_BYTE.saturating_mul(length));
        budget.input_bytes = budget.input_bytes.max(length);
        request
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

    /// Set the dialect allow-list.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, tool_call::{DialectId, DialectSet, ToolRecoveryRequest}};
    /// let set = DialectSet::new([DialectId::Hermes], &mut Governor::new(Budget::default())).unwrap();
    /// assert_eq!(ToolRecoveryRequest::default().dialects(set).dialects.dialects(), [DialectId::Hermes]);
    /// ```
    #[must_use]
    pub fn dialects(mut self, dialects: DialectSet) -> Self {
        self.dialects = dialects;
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
/// - `Calls`: the turn made calls and nothing is wrong with any of them.
/// - `ContentOnly`: no call, nothing wrong.
/// - `TruncatedWithCalls` / `Truncated`: a [`GenerationStop::Length`] stop
///   (never a clean finish), with or without a call that is not itself
///   cut off.
/// - `Unresolved`: something is wrong, whatever the stop: a rejected call
///   attempt (other than one a length stop cut off), a call that is
///   `Unresolved` (other than by truncation) or `Ambiguous`, or a failed
///   tool choice check. Truncation never upgrades malformed output.
///
/// ```
/// use purrassist_core::tool_call::Finish;
/// assert_eq!(Finish::TruncatedWithCalls.openai_finish_reason(), "length");
/// assert_eq!(Finish::Calls.openai_finish_reason(), "tool_calls");
/// assert_eq!(Finish::Unresolved.openai_finish_reason(), "stop");
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

impl Finish {
    /// The `OpenAI` `finish_reason` (D15): `"length"` for the truncated
    /// finishes, `"tool_calls"` for `Calls`, and `"stop"` otherwise.
    ///
    /// `"stop"` hides rejected and unresolved calls: callers must read
    /// [`ToolCalls::finish`] and [`ToolCalls::rejected`], never only this.
    ///
    /// See [`Finish`] for an example.
    #[must_use]
    pub const fn openai_finish_reason(self) -> &'static str {
        match self {
            Self::TruncatedWithCalls | Self::Truncated => "length",
            Self::Calls => "tool_calls",
            Self::ContentOnly | Self::Unresolved => "stop",
        }
    }

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
/// `used` is whether repair consulted it: `$ref`, `anyOf` and the other
/// keywords that make a value opaque to repair, `$schema`, Draft-07
/// `dependencies`, `unevaluatedProperties`, and `patternProperties`. Every
/// other unmodeled keyword (`format`, `minLength`, `x-*`, ...) is ignored
/// by repair and enforced only by validation against the exact schema.
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
///     dialect: DialectId::Hermes,
///     candidate_dialects: vec![DialectId::Hermes],
///     raw: b"<tc".to_vec(),
///     reason: Rejection::Unterminated,
///     repairs: CallLedger::default(),
/// };
/// assert_eq!(rejected.text(), Some("<tc"));
/// ```
///
/// A complete call followed by a dangling `<tool_call>` under the default
/// allow-list: Qwen3-Coder XML and Hermes both attempted the dangling call,
/// the complete call stays usable, and the dangling remainder is one
/// rejected attempt naming both dialects:
///
/// ```
/// use purrassist_core::tool_call::{DialectId, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::{Budget, Outcome};
/// let tools = ToolSet::compile([ToolDefinition::new("read").param("path", Param::string().required())], Budget::default()).unwrap();
/// let output = b"<tool_call>{\"name\": \"read\", \"arguments\": {\"path\": \"a\"}}</tool_call>\n<tool_call>";
/// let Outcome::BestGuess(calls) = tools.recover(output, &ToolRecoveryRequest::default()).outcome else { unreachable!() };
/// assert_eq!(calls.usable().len(), 1);
/// assert_eq!(calls.rejected.len(), 1);
/// assert_eq!(calls.rejected[0].candidate_dialects, [DialectId::Hermes, DialectId::Qwen3CoderXml]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedCall {
    /// The attempt's source range.
    pub span: Range<usize>,
    /// The dialect whose opener started the attempt reported: of the
    /// dialects that attempted a call at this opener, the one whose grammar
    /// read furthest before failing.
    pub dialect: DialectId,
    /// Every dialect whose opener started a rejected attempt at this
    /// offset in a reading of the turn that recovers the same calls, in
    /// canonical [`DialectId`] order; always contains
    /// [`dialect`](Self::dialect).
    pub candidate_dialects: Vec<DialectId>,
    /// The attempt's exact bytes.
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
/// let result = tools.recover(br#"<tool_call>{"name": "ls", "arguments": {"depth": 2}}</tool_call>"#, &ToolRecoveryRequest::default());
/// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
/// let call = &calls.calls[0];
/// assert_eq!((call.id.as_str(), call.tool.as_deref()), ("call_0", Some("ls")));
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
    /// let result = tools.recover(br#"<tool_call>{"name": "read", "arguments": {"path": "a.rs", "limit": 5}}</tool_call>"#, &ToolRecoveryRequest::default());
    /// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
    /// let read: Read = calls.calls[0].arguments_as().unwrap();
    /// assert_eq!((read.path.as_str(), read.limit), ("a.rs", Some(5)));
    /// assert!(calls.calls[0].arguments_as::<u32>().is_err());
    /// ```
    pub fn arguments_as<T: FromNode>(&self) -> Result<T, FromNodeError> {
        T::from_node(&self.arguments)
    }

    /// Whether the call may run as is: its outcome is `Resolved`.
    ///
    /// See [`ToolCall`] for an example.
    #[must_use]
    pub fn is_usable(&self) -> bool {
        self.outcome == CallOutcome::Resolved
    }

    /// Whether the call may run when the caller accepts guesses: `Resolved`,
    /// or `BestGuess` on a tool whose effective effects are
    /// [`Effects::ReadOnly`]. A guess on a mutating tool never qualifies.
    ///
    /// See [`ToolCalls::usable_accepting_guesses`] for an example.
    #[must_use]
    pub fn is_usable_accepting_guesses(&self) -> bool {
        self.is_usable()
            || (self.outcome == CallOutcome::BestGuess && self.effects == Some(Effects::ReadOnly))
    }
}

/// Every call of one turn, with what surrounds them.
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
    /// let result = tools.recover(output, &ToolRecoveryRequest::default());
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
    /// The reasoning block ranges, markers included.
    pub reasoning: Vec<Range<usize>>,
    /// How the turn finished.
    pub finish: Finish,
    /// The tool choice check.
    pub tool_choice: ToolChoiceCheck,
    /// The id counter after this recovery (see [`IdPolicy`]).
    pub next_id_start: u64,
    /// The calls printed for the request's [`EmitTarget`], with every call
    /// and rejected attempt not printed; `None` for [`EmitTarget::None`].
    /// Ids are the calls' assigned [`ToolCall::id`]s.
    pub emitted: Option<Emission>,
}

impl ToolCalls {
    /// The calls that may run as is: per-call `Resolved` only.
    ///
    /// See [`ToolSet::recover`] for an example.
    #[must_use]
    pub fn usable(&self) -> Vec<&ToolCall> {
        self.calls.iter().filter(|call| call.is_usable()).collect()
    }

    /// The calls that may run when the caller accepts guesses: also every
    /// `BestGuess` call to a read-only tool, never a guess on a mutating
    /// tool.
    ///
    /// ```
    /// use purrassist_core::tool_call::{Effects, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, Outcome};
    /// let tools = ToolSet::compile([ToolDefinition::new("read_file").effects(Effects::ReadOnly)], Budget::default()).unwrap();
    /// let result = tools.recover(br#"<tool_call>{"name": "read_fiel", "arguments": {}}</tool_call>"#, &ToolRecoveryRequest::default());
    /// let Outcome::BestGuess(calls) = result.outcome else { unreachable!() };
    /// assert!(calls.usable().is_empty());
    /// assert_eq!(calls.usable_accepting_guesses()[0].tool.as_deref(), Some("read_file"));
    /// ```
    #[must_use]
    pub fn usable_accepting_guesses(&self) -> Vec<&ToolCall> {
        self.calls
            .iter()
            .filter(|call| call.is_usable_accepting_guesses())
            .collect()
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

const fn null() -> Node {
    Node::new(Value::Null)
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

fn rejection_node(reason: Rejection) -> Node {
    let (code, at) = match reason {
        Rejection::Malformed { at } => ("malformed", Some(at)),
        Rejection::InvalidUtf8 { at } => ("invalid_utf8", Some(at)),
        Rejection::Unterminated => ("unterminated", None),
        Rejection::Overlapped { at } => ("overlapped", Some(at)),
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
    /// `{calls, rejected: [{span, dialect, candidate_dialects, reason: {code,
    /// at}, raw: {text | bytes_hex}, repairs}], embedded, content: [{start, end}], reasoning:
    /// [{start, end}], finish, openai_finish_reason, tool_choice: {constraint,
    /// passed, diagnostics}, next_id_start}`, each call and embedded call
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
                        ("dialect", string(rejected.dialect.id())),
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
            ("finish", string(self.finish.code())),
            (
                "openai_finish_reason",
                string(self.finish.openai_finish_reason()),
            ),
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
    /// `request.budget`.
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
    /// ```
    /// use purrassist_core::tool_call::{CallOutcome, Finish, Param, ToolDefinition, ToolRecoveryRequest, ToolSet, UnresolvedReason};
    /// use purrassist_core::grammar::edit::LossKind;
    /// use purrassist_core::{Budget, Outcome};
    /// let output = b"<tool_call><function=f></parameters></parameters></function></tool_call>";
    /// let request = ToolRecoveryRequest::default();
    ///
    /// let tools = ToolSet::compile([ToolDefinition::new("f").param("path", Param::string())], Budget::default()).unwrap();
    /// let result = tools.recover(output, &request);
    /// assert_eq!(result.receipt.operation, "tool_call.recover.v1");
    /// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
    /// let call = &calls.calls[0];
    /// assert_eq!((call.tool.as_deref(), call.arguments.to_json().as_str()), (Some("f"), "{}"));
    /// assert_eq!(call.outcome, CallOutcome::Resolved);
    /// assert_eq!(call.repairs.losses.len(), 1);
    /// assert_eq!(call.repairs.losses[0].kind, LossKind::Markup);
    /// assert_eq!(result.receipt.losses, [call.repairs.losses[0].loss.clone()]);
    /// assert_eq!((calls.finish, calls.finish.openai_finish_reason()), (Finish::Calls, "tool_calls"));
    /// assert_eq!(calls.usable().len(), 1);
    ///
    /// let strict = ToolSet::compile([ToolDefinition::new("f").param("path", Param::string().required())], Budget::default()).unwrap();
    /// let result = strict.recover(output, &request);
    /// let Outcome::Resolved(calls) = result.outcome else { unreachable!() };
    /// assert!(matches!(calls.calls[0].outcome, CallOutcome::Unresolved { reason: UnresolvedReason::MissingRequired { .. } }));
    /// assert_eq!(calls.finish, Finish::Unresolved);
    /// assert!(calls.usable().is_empty());
    /// ```
    #[must_use]
    pub fn recover(&self, output: &[u8], request: &ToolRecoveryRequest) -> AssistResult<ToolCalls> {
        self.recover_governed(output, request, &mut Governor::new(request.budget))
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
    /// The receipt always names the output's digest, and retains the output
    /// whenever its retention charge succeeded.
    ///
    /// ```
    /// use purrassist_core::tool_call::{Effects, ToolDefinition, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::{Budget, CancellationToken, Governor, Outcome, StopReason};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)], Budget::default()).unwrap();
    /// let token = CancellationToken::new();
    /// let mut governor = Governor::with_cancellation(Budget::default(), token.clone());
    /// token.cancel();
    /// let result = tools.recover_governed(b"<tool_call>{\"name\": \"ls\", \"arguments\": {}}</tool_call>", &ToolRecoveryRequest::default(), &mut governor);
    /// assert_eq!(result.outcome, Outcome::Unresolved);
    /// assert_eq!(result.receipt.stopped, StopReason::Cancelled);
    /// assert!(result.receipt.sources.is_empty());
    ///
    /// let request = ToolRecoveryRequest::default().effects("rm", Effects::ReadOnly);
    /// let result = tools.recover_governed(b"", &request, &mut Governor::new(Budget::default()));
    /// assert!(matches!(result.receipt.stopped, StopReason::InvalidInput(_)));
    /// assert_eq!(result.receipt.sources.len(), 1);
    /// ```
    #[must_use]
    pub fn recover_governed(
        &self,
        output: &[u8],
        request: &ToolRecoveryRequest,
        governor: &mut Governor,
    ) -> AssistResult<ToolCalls> {
        let mut receipt = Receipt::new(
            output,
            RECOVER_OPERATION,
            request.effort,
            *governor.limits(),
        );
        let recovery = Recovery {
            tools: self,
            request,
            output,
            source: receipt.source,
            policies: Policies {
                selection: request.policy,
                effects: request.effects.clone(),
                exact: request.effort == Effort::Exact,
            },
        };
        let outcome = match receipt
            .retain_source_bytes(output, governor)
            .and_then(|()| recovery.run(governor))
        {
            Ok(done) => {
                receipt.evidence = done.evidence;
                receipt.losses = done.losses;
                receipt.inferences = done.inferences;
                receipt.validations = done.validations;
                receipt.alternatives = done.alternatives;
                receipt.stopped = done.stopped;
                done.outcome
            }
            Err(reason) => {
                receipt.stopped = reason;
                Outcome::Unresolved
            }
        };
        receipt.usage = governor.usage();
        AssistResult { outcome, receipt }
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
    /// their dialects read before failing.
    fn body_evidence(&self) -> usize {
        self.value.rejected.iter().map(reach).sum()
    }

    fn ambiguous_calls(&self) -> impl Iterator<Item = usize> + '_ {
        self.value
            .calls
            .iter()
            .filter(|call| call.outcome == CallOutcome::Ambiguous)
            .map(|call| call.index)
    }
}

/// The receipt's validation log, deduplicated per source range.
struct Log {
    entries: Vec<Validation>,
    index: BTreeMap<(usize, usize), Vec<usize>>,
}

impl Log {
    fn add(
        &mut self,
        key: (usize, usize),
        validation: &Validation,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let known = self.index.entry(key).or_default();
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
    output: &'a [u8],
    source: Digest,
    policies: Policies,
}

impl Recovery<'_> {
    fn run(&self, governor: &mut Governor) -> Result<Done, StopReason> {
        governor.check()?;
        self.policies.validate(self.tools)?;
        let forced = match &self.request.tool_choice {
            ToolChoice::Forced(name) => Some(
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
        let encoding = self.encoding(governor)?;
        let mut recognizer = Recognizer::with_tools(
            &self.request.dialects,
            self.tools,
            self.request.reasoning,
            governor,
        )?;
        recognizer.push(self.output, governor)?;
        let turn = recognizer.finish(self.request.stop.clone(), governor)?;
        let resolved = resolve_forest(self.tools, &turn, &self.policies, governor)?;
        drop(turn);
        let readings = self.readings(&resolved, &encoding, forced.as_deref(), governor)?;
        drop(resolved);
        self.conclude(readings, governor)
    }

    /// The `tool_call.input.invalid-utf8` losses of the output.
    fn encoding(&self, governor: &mut Governor) -> Result<CallLedger, StopReason> {
        governor.charge(Resource::Work, size(self.output.len()))?;
        let mut ledger = CallLedger::default();
        let mut at = 0;
        let mut run: Option<Range<usize>> = None;
        for chunk in self.output.utf8_chunks() {
            if !chunk.valid().is_empty()
                && let Some(range) = run.take()
            {
                self.invalid(range, &mut ledger, governor)?;
            }
            at += chunk.valid().len();
            if !chunk.invalid().is_empty() {
                let end = at + chunk.invalid().len();
                run = Some(run.map_or(at, |range| range.start)..end);
                at = end;
            }
        }
        if let Some(range) = run {
            self.invalid(range, &mut ledger, governor)?;
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
    fn readings(
        &self,
        turn: &ResolvedTurn,
        encoding: &CallLedger,
        forced: Option<&str>,
        governor: &mut Governor,
    ) -> Result<Vec<Reading>, StopReason> {
        let radices: Vec<usize> = turn
            .pieces
            .iter()
            .filter_map(|piece| match piece {
                ResolvedPiece::Choice(choice) => Some(choice.readings.len()),
                ResolvedPiece::Segment(_) => None,
            })
            .collect();
        if !radices.is_empty() {
            // Every reading is admitted before any is built: a product over
            // the budget is exhaustion, never a truncated list.
            let product = radices
                .iter()
                .fold(1_u64, |product, &radix| product.saturating_mul(size(radix)));
            governor.charge(Resource::Candidates, product)?;
        }
        let mut digits = vec![0_usize; radices.len()];
        let mut readings = Vec::new();
        loop {
            let mut segments = Vec::with_capacity(turn.pieces.len());
            let mut choice = 0;
            for piece in &turn.pieces {
                match piece {
                    ResolvedPiece::Segment(segment) => segments.push(segment),
                    ResolvedPiece::Choice(options) => {
                        segments.extend(&options.readings[digits[choice]]);
                        choice += 1;
                    }
                }
            }
            readings.push(self.reading(&segments, turn.cost, encoding, forced, governor)?);
            let mut position = 0;
            loop {
                if position == digits.len() {
                    return Ok(readings);
                }
                digits[position] += 1;
                if digits[position] < radices[position] {
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
                finish: Finish::ContentOnly,
                tool_choice: ToolChoiceCheck {
                    choice: self.request.tool_choice.clone(),
                    passed: true,
                    diagnostics: Vec::new(),
                },
                next_id_start: 0,
                emitted: None,
            },
            facts: Vec::new(),
            parts: vec![Part::Message(encoding.clone())],
            validations: Vec::new(),
            cost,
            inexact: Vec::new(),
        };
        for segment in segments {
            self.segment(&mut reading, segment, governor)?;
        }
        reading.value.next_id_start = self.ids(&mut reading.value.calls, governor)?;
        self.tool_choice(&mut reading.value, forced);
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
                reading.value.rejected.push(RejectedCall {
                    span: span.clone(),
                    dialect: dialect.clone(),
                    candidate_dialects: vec![dialect.clone()],
                    raw: self.output[span.clone()].to_vec(),
                    reason: *reason,
                    repairs: repairs.clone(),
                });
                reading.parts.push(Part::Rejected(index));
            }
        }
        Ok(())
    }

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
        let sources: Vec<(usize, Option<String>)> = calls
            .iter()
            .map(|call| (call.index, call.source_id.clone()))
            .collect();
        let (ids, next) = assign_ids(
            &self.request.ids,
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

    /// Enforce the tool choice on a reading's calls.
    fn tool_choice(&self, value: &mut ToolCalls, forced: Option<&str>) {
        let choice = &self.request.tool_choice;
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
            ToolChoice::Forced(_) => {
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
            }
        }
        value.tool_choice.passed = diagnostics.is_empty();
        value.tool_choice.diagnostics = diagnostics;
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
            if !on_path(pointer, &call.arguments, governor)? {
                continue;
            }
            let used = consulted_by_repair(keyword);
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
        if rejected || broken || !value.tool_choice.passed {
            Finish::Unresolved
        } else if truncation {
            if value.calls.iter().any(|call| !cut(call)) {
                Finish::TruncatedWithCalls
            } else {
                Finish::Truncated
            }
        } else if value.calls.is_empty() {
            Finish::ContentOnly
        } else {
            Finish::Calls
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
        let mut readings = same_calls(readings, governor)?;
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
                        readings = merge(readings, &groups, governor)?;
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

/// The id of every call under `policy` (see [`IdPolicy`]), by position,
/// and the id counter after them. `calls` holds each call's index and the
/// id it carries; `dedup(position, duplicate, id, governor)` is told of
/// every carried id renumbered under [`IdPolicy::Source`] (`duplicate` when
/// the call carried an id an earlier call holds). Recovery and
/// [`emit`](super::emit) share it, so one policy numbers calls the same way.
pub(crate) fn assign_ids(
    policy: &IdPolicy,
    calls: &[(usize, Option<String>)],
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
            for (index, _) in calls {
                let id = format!("{prefix}{}", start + size(*index));
                governor.charge(Resource::ScratchBytes, size(id.len()))?;
                ids.push(id);
            }
            Ok((ids, next))
        }
        IdPolicy::Generator(generator) => {
            let mut taken: BTreeMap<String, usize> = BTreeMap::new();
            for (index, _) in calls {
                let id = generator.id(*index);
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
                taken.insert(id.clone(), *index);
                ids.push(id);
            }
            Ok((ids, count))
        }
        IdPolicy::Source => {
            let carried: BTreeSet<&String> =
                calls.iter().filter_map(|(_, id)| id.as_ref()).collect();
            governor.charge(
                Resource::ScratchBytes,
                carried
                    .iter()
                    .map(|id| size(id.len()).saturating_add(ENTRY_BYTES))
                    .sum(),
            )?;
            let mut taken: BTreeSet<String> = BTreeSet::new();
            for (position, (index, source)) in calls.iter().enumerate() {
                governor.charge(Resource::Work, 1)?;
                let id = match source {
                    Some(id) if !taken.contains(id) => id.clone(),
                    source => {
                        let base = source.as_deref().unwrap_or("call");
                        let mut id = format!("{base}#{index}");
                        let mut suffix = 1_u64;
                        while taken.contains(&id) || carried.contains(&id) {
                            governor.charge(Resource::Work, 1)?;
                            id = format!("{base}#{index}#{suffix}");
                            suffix += 1;
                        }
                        dedup(position, source.is_some(), &id, governor)?;
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

/// How far a rejected attempt's dialect read before it failed: the bytes
/// from its opener to the byte it could not consume, or to the end of its
/// span when the output stopped inside it.
const fn reach(rejected: &RejectedCall) -> usize {
    let end = match rejected.reason {
        Rejection::Malformed { at }
        | Rejection::InvalidUtf8 { at }
        | Rejection::Overlapped { at } => at,
        Rejection::Unterminated => rejected.span.end,
    };
    end.saturating_sub(rejected.span.start)
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
/// reasoning, the reasoning ranges, and every message-level ledger other
/// than a rejected attempt's. Content ranges and rejected attempts, the
/// bytes outside every call, are not part of it.
fn calls_key(reading: &Reading, governor: &mut Governor) -> Result<Digest, StopReason> {
    let value = &reading.value;
    let json = object([
        ("calls", array(value.calls.iter().map(call_identity))),
        ("embedded", array(value.embedded.iter().map(call_identity))),
        ("reasoning", array(value.reasoning.iter().map(byte_range))),
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
fn same_calls(readings: Vec<Reading>, governor: &mut Governor) -> Result<Vec<Reading>, StopReason> {
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
    merge(readings, &groups, governor)
}

/// Each group of `readings` (indices, best first) as one reading: the
/// member whose rejected attempts' dialects read the most bytes before
/// failing (the body evidence), the earliest on a tie, with every rejected
/// attempt's [`RejectedCall::candidate_dialects`] extended to the dialects
/// that attempted a call at the same opener in any member, and every
/// call's (and embedded call's) [`ToolCall::candidate_dialects`] extended
/// to the dialects that read the same call in any member.
fn merge(
    readings: Vec<Reading>,
    groups: &[Vec<usize>],
    governor: &mut Governor,
) -> Result<Vec<Reading>, StopReason> {
    let mut slots: Vec<Option<Reading>> = readings.into_iter().map(Some).collect();
    let mut merged = Vec::with_capacity(groups.len());
    for group in groups {
        let mut best = group[0];
        for &member in &group[1..] {
            let evidence = |at: usize| slots[at].as_ref().map_or(0, Reading::body_evidence);
            if evidence(member) > evidence(best) {
                best = member;
            }
        }
        let mut attempts: BTreeMap<usize, BTreeSet<DialectId>> = BTreeMap::new();
        for &member in group {
            let reading = slots[member]
                .as_ref()
                .expect("each reading is in one group");
            for rejected in &reading.value.rejected {
                governor.charge(
                    Resource::Work,
                    size(rejected.candidate_dialects.len()).saturating_add(1),
                )?;
                attempts
                    .entry(rejected.span.start)
                    .or_default()
                    .extend(rejected.candidate_dialects.iter().cloned());
            }
        }
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
            let candidates = &attempts[&rejected.span.start];
            governor.charge(
                Resource::ScratchBytes,
                size(candidates.len()).saturating_mul(ENTRY_BYTES),
            )?;
            rejected.candidate_dialects = candidates.iter().cloned().collect();
        }
        merged.push(reading);
    }
    Ok(merged)
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
