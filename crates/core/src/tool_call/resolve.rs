// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Resolution of recognized calls against a tool set (R3.3–R3.5, R1.4–R1.5,
//! C2).
//!
//! [`resolve_forest`] takes a [`RecognizedTurn`] and resolves every call of
//! every reading against a [`ToolSet`] under the request's [`Policies`]. It
//! mirrors the turn's factorized segmentation ([`ResolvedTurn::pieces`]):
//! content, reasoning (with its embedded calls resolved), tool results and
//! rejected attempts pass through, and every call becomes a
//! [`ResolvedCall`] with a per-call [`CallOutcome`], its own repairs
//! ([`CallLedger`]: evidence, losses and inferences, every one itemized),
//! the validations it passed or failed, and its ranked alternative readings
//! ([`CallReading`]).
//!
//! # Steps
//!
//! For each recognized reading of a call:
//!
//! 1. **Repairs.** The reading's markup edits (and any guess it inherits,
//!    such as a reasoning block closed at the call's opener) form the base
//!    ledger; its cost is the sum of their [`rules`] costs. A
//!    call cut off by a length stop is `Unresolved` with a
//!    `tool_call.truncated` data loss. A call completed by
//!    `tool_call.markup.missing-closers-at-end` must be otherwise complete
//!    (D10): its name parsed, no parameter value open at the end of output,
//!    its arguments complete, and every required parameter present;
//!    otherwise it is `Unresolved` as [`UnresolvedReason::Malformed`].
//! 2. **Tool name** (R3.4a). An exact name or an explicit alias
//!    (`tool_call.name.alias`) identifies the tool under every policy. Any
//!    other name is offered to
//!    [`matching::resolve_governed`] with
//!    the default [`SelectionPolicy`] over every tool's name and aliases:
//!    abstention is [`UnresolvedReason::UnknownTool`] (a hallucinated name is
//!    never coerced into a real tool); an accepted near-miss is a
//!    guess-flagged `tool_call.name.match` reading, and several are
//!    competing readings. A fuzzy match is admitted only when every tool it
//!    could name resolves under `Tolerant`: a `Structural` tool is reached by
//!    its exact name or alias only.
//! 3. **Parameter names** (R3.4b). Exact names and aliases bind directly.
//!    The rest are assigned by a best-first `Search` over a per-scope
//!    state: under `Tolerant`, renamed to a declared parameter
//!    (`json.key.match`, the shared schema-recovery name cost) or, for a
//!    missing required parameter, relocated out of a nested object
//!    (`json.key.relocate`); under any policy, dropped with a data loss
//!    (`DropWithLoss`) or kept (`PassThrough`) by the scope's
//!    [`UnknownParameters`], except that under `Structural` a near miss of
//!    a declared name (a name `json.key.match` would rename) is never
//!    dropped: that would silently discard the value the evidence assigns
//!    to a declared parameter. Under `Error`, and for such a near miss
//!    under `DropWithLoss`, an unassignable parameter is
//!    [`UnresolvedReason::UnknownParameter`]; a required parameter nothing
//!    supplies is [`UnresolvedReason::MissingRequired`] (never invented).
//! 4. **Repeats** (R3.4d). An array parameter collects every occurrence's
//!    items; a repeatable parameter collects its occurrences into an array
//!    (`tool_call.parameter.collect`). Any other parameter occurring more
//!    than once gives two readings, first occurrence and last occurrence
//!    (`tool_call.parameter.repeated`, each dropping the other occurrences
//!    as data losses): never last-wins. Evidence decides when it can: a
//!    reading whose kept value fails the declared type, enumeration or
//!    schema is blocked, and one that needs a repair costs more, so the
//!    other is the call's only reading (`BestGuess`). When every value is
//!    valid the call is `Ambiguous`. Either way the selected reading
//!    records `tool_call.parameter.repeated-choice`, a guess naming the
//!    occurrence its `arguments` keep and why (the evidence, or, for an
//!    `Ambiguous` call, that it is merely the first-ranked reading), citing
//!    the alternatives that keep the others.
//! 5. **Typing** (R3.4c). A declared string is never parsed and numbers keep
//!    their spelling. An enumeration value equal to exactly one literal
//!    after case folding and whitespace normalization takes that literal
//!    (`tool_call.value.enum-normalize`); under `Tolerant` an approximate
//!    value takes its closest literals (`json.enum.match`). XML text of a
//!    typed parameter whose tool was only known after name resolution is
//!    read by its declared type (`tool_call.value.decode-typed`). Nothing
//!    else is coerced: `json.required.invent`, `json.type.stringify-scalar`
//!    and `json.type.decode-string` are disabled ([`rules::DISABLED`]).
//! 6. **Validation.** The resolved arguments are validated against the
//!    tool's compiled schema (the exact source schema for an adapter tool);
//!    a failure is [`UnresolvedReason::SchemaValidation`] with the
//!    diagnostics recorded, never a schema-guided invention.
//!
//! The call's readings are then ranked by cost and a digest of their
//! content (a total order independent of tool and dialect declaration
//! order). Among readings with no blocker whose every rule the tool's policy
//! admits, the least-cost ones compete: one is classified by
//! [`CallOutcome::classify`], several are `Ambiguous` (with
//! [`ResolvedCall::distinguishing`] explaining the difference). Readings
//! with no blocker that the policy does not admit are reported as
//! `Ambiguous` when several tie, and `Unresolved` otherwise.
//!
//! # Policies
//!
//! A tool's effects are its declared effects unless [`Policies::effects`]
//! overrides them; its policy is [`PolicySelection::All`]'s, or (per tool)
//! its explicit policy, else the one its effects select
//! ([`CompiledTool::policy_for`]).
//!
//! # Resources
//!
//! Charges `Work` per member, option, alternative and validation step,
//! `ScratchBytes` for every value and ledger entry built, `EvidenceEntries`
//! per recorded evidence, loss and inference, `Depth` per nested object
//! scope, and the search's own `Candidates`; name matching and validation
//! charge as documented on them. Every charge is a function of the
//! recognized turn and the tool set alone.
use super::rules::{self, Rule};
use super::{
    ArgOrigin, CallSegment, Closure, CompiledTool, DialectId, Effects, GenerationStop, ObjectShape,
    ParameterShape, Piece, RawCall, ReasoningSegment, RecognizedTurn, Rejection, RepairPolicy,
    Segment, Spanned, ToolSet, UnknownParameters, ValueShape,
};
use crate::contract::saturating_size as size;
use crate::contract::{
    Evidence, Governor, Inference, InferenceKind, Loss, Outcome, Resource, ScratchVec, SourceSpan,
    StopReason, Validation,
};
use crate::digest::{Digest, DigestBuilder};
use crate::grammar::CaptureKind;
use crate::grammar::edit::{EditOperator, LossKind};
use crate::json::{Member, Node, Value, pointer_segment};
use crate::matching::{self, Candidate, MatchRequest, SelectionPolicy};
use crate::provenance::{Derivation, Forest, ProvenanceNode};
use crate::schema::recovery::{FOLDED_NAME_COST, name_cost};
use crate::search::{Acceptance, Change, Ledger, Proposals, Search, SearchState};
use core::fmt;
use core::marker::PhantomData;
use core::ops::Range;
use std::collections::BTreeMap;

/// Scratch bytes charged per ledger entry built by resolution.
const ENTRY_BYTES: u64 = 256;

/// Portable scratch charges of resolution's own structures: each one's size
/// on a 64-bit target, asserted below never to undercount the target
/// compiling this crate (see [`crate::contract::ScratchCharge`]). Every
/// structure resolution builds per call, reading, parameter or alternative
/// is charged at these sizes (and every value it copies at its
/// [`Node::estimated_bytes`]), so resolution's allocations are charged in
/// proportion to what it builds.
const READING_BYTES: u64 = 232;
/// See [`READING_BYTES`].
const CALL_BYTES: u64 = 456;
/// See [`READING_BYTES`].
const DRAFT_BYTES: u64 = 144;
/// See [`READING_BYTES`].
const TYPED_BYTES: u64 = 176;
/// See [`READING_BYTES`].
const OCCURRENCE_BYTES: u64 = 128;
/// See [`READING_BYTES`].
const SLOT_BYTES: u64 = 104;
/// See [`READING_BYTES`].
const TARGET_BYTES: u64 = 176;
/// See [`READING_BYTES`]: one reading's ranking record (its cost, two
/// digests and its position).
const RANKED_BYTES: u64 = 80;
/// The portable charge of one object member.
const MEMBER_BYTES: u64 = <Member as crate::contract::ScratchCharge>::CHARGE as u64;
const _: () = {
    use core::mem::size_of;
    assert!(size_of::<CallReading>() as u64 <= READING_BYTES);
    assert!(size_of::<Option<CallReading>>() as u64 <= READING_BYTES);
    assert!(size_of::<ResolvedCall>() as u64 <= CALL_BYTES);
    assert!(size_of::<Draft>() as u64 <= DRAFT_BYTES);
    assert!(size_of::<Typed>() as u64 <= TYPED_BYTES);
    assert!(size_of::<Occurrence>() as u64 <= OCCURRENCE_BYTES);
    assert!(size_of::<Slot>() as u64 <= SLOT_BYTES);
    assert!(size_of::<Target<'static>>() as u64 <= TARGET_BYTES);
    assert!(size_of::<(u64, Digest, Digest, usize)>() as u64 <= RANKED_BYTES);
};

/// How the repair policy of each tool is chosen for one request (C6).
///
/// ```
/// use purrassist_core::tool_call::{PolicySelection, RepairPolicy};
/// assert_eq!(PolicySelection::default(), PolicySelection::PerTool);
/// assert_ne!(PolicySelection::All(RepairPolicy::Structural), PolicySelection::PerTool);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PolicySelection {
    /// Each tool's own policy: explicit, else selected by its effects.
    #[default]
    PerTool,
    /// One policy for every tool, whatever it declares.
    All(RepairPolicy),
}

/// The per-request policy inputs of resolution.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Effects, Policies, PolicySelection, RepairPolicy, ToolDefinition, ToolSet}};
/// let tools = ToolSet::compile([ToolDefinition::new("rm")], Budget::default()).unwrap();
/// let rm = tools.tool("rm").unwrap();
/// let mut policies = Policies::default();
/// assert_eq!(policies.effective(rm), (Effects::Mutating, RepairPolicy::Structural));
/// policies.effects.insert("rm".into(), Effects::ReadOnly);
/// assert_eq!(policies.effective(rm), (Effects::ReadOnly, RepairPolicy::Tolerant));
/// policies.selection = PolicySelection::All(RepairPolicy::Structural);
/// assert_eq!(policies.effective(rm), (Effects::ReadOnly, RepairPolicy::Structural));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policies {
    /// How each tool's repair policy is chosen.
    pub selection: PolicySelection,
    /// Effects overrides by canonical tool name (adapter-built tools declare
    /// none and default to [`Effects::Mutating`]). Naming a tool the set
    /// does not declare is a request error ([`Policies::validate`]).
    pub effects: BTreeMap<String, Effects>,
    /// Canonical parsing only ([`crate::Effort::Exact`]): no repair rule
    /// ([`Rule::is_repair`]) is applied. Every reading that needs one is
    /// blocked by [`UnresolvedReason::RepairNotPermitted`] naming the first
    /// such rule, carries `null` arguments and no repair entries (only its
    /// parsing and record entries), and is never validated. Names are
    /// never matched: a name that is neither a declared name nor an alias
    /// is blocked by `RepairNotPermitted` naming `tool_call.name.match`
    /// (without matching, a near-miss and a hallucinated name are not told
    /// apart, and no matching work is charged).
    pub exact: bool,
}

impl Policies {
    /// One policy for every tool.
    ///
    /// ```
    /// use purrassist_core::tool_call::{Policies, PolicySelection, RepairPolicy};
    /// assert_eq!(Policies::all(RepairPolicy::Tolerant).selection, PolicySelection::All(RepairPolicy::Tolerant));
    /// ```
    #[must_use]
    pub const fn all(policy: RepairPolicy) -> Self {
        Self {
            selection: PolicySelection::All(policy),
            effects: BTreeMap::new(),
            exact: false,
        }
    }

    /// Check the request against `tools`.
    ///
    /// # Errors
    /// [`StopReason::InvalidInput`] naming the first effects override (in
    /// name order) whose key is not the canonical name of a tool in `tools`.
    ///
    /// ```
    /// use purrassist_core::{Budget, StopReason, tool_call::{Effects, Policies, ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("rm")], Budget::default()).unwrap();
    /// let mut policies = Policies::default();
    /// policies.effects.insert("mv".into(), Effects::ReadOnly);
    /// assert!(matches!(policies.validate(&tools), Err(StopReason::InvalidInput(reason)) if reason.contains("`mv`")));
    /// ```
    pub fn validate(&self, tools: &ToolSet) -> Result<(), StopReason> {
        self.effects
            .keys()
            .find(|name| tools.tool(name).is_none())
            .map_or(Ok(()), |name| {
                Err(StopReason::InvalidInput(format!(
                    "effects override names `{name}`, which is not a declared tool"
                )))
            })
    }

    /// The effects and repair policy `tool` resolves under.
    ///
    /// See [`Policies`] for an example.
    #[must_use]
    pub fn effective(&self, tool: &CompiledTool) -> (Effects, RepairPolicy) {
        let effects = self
            .effects
            .get(tool.name())
            .copied()
            .unwrap_or_else(|| tool.effects());
        let policy = match self.selection {
            PolicySelection::All(policy) => policy,
            PolicySelection::PerTool => tool.policy_for(effects),
        };
        (effects, policy)
    }
}

/// One loss of a call, with the kind of information it discards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallLoss {
    /// Whether the loss discards markup or possible data.
    pub kind: LossKind,
    /// The loss.
    pub loss: Loss,
}

/// Every repair of one call reading, itemized (C2).
///
/// ```
/// use purrassist_core::contract::{Evidence, Inference, InferenceKind, Loss};
/// use purrassist_core::grammar::edit::LossKind;
/// use purrassist_core::tool_call::{CallLedger, CallLoss};
/// let evidence = Evidence { rule: "tool_call.markup.stray-closer".into(), explanation: String::new(), span: None, destination: String::new() };
/// let mut ledger = CallLedger::default();
/// assert!(!ledger.guessed() && !ledger.data_loss());
/// ledger.losses.push(CallLoss { kind: LossKind::Markup, loss: Loss { evidence: evidence.clone(), original: "</p>".into() } });
/// assert!(!ledger.data_loss());
/// ledger.inferences.push(Inference { evidence, kind: InferenceKind::Guess, value: "f".into() });
/// assert!(ledger.guessed());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallLedger {
    /// Every rule applied, in order.
    pub evidence: Vec<Evidence>,
    /// Every loss, with its kind.
    pub losses: Vec<CallLoss>,
    /// Every inferred or invented value.
    pub inferences: Vec<Inference>,
}

impl CallLedger {
    /// Whether any inference is a guess.
    ///
    /// See [`CallLedger`] for an example.
    #[must_use]
    pub fn guessed(&self) -> bool {
        self.inferences
            .iter()
            .any(|inference| inference.kind == InferenceKind::Guess)
    }

    /// Whether any loss may carry data.
    ///
    /// See [`CallLedger`] for an example.
    #[must_use]
    pub fn data_loss(&self) -> bool {
        self.losses.iter().any(|loss| loss.kind == LossKind::Data)
    }

    /// The first recorded rule `policy` does not admit (an id that names
    /// no registered rule is never admitted).
    ///
    /// ```
    /// use purrassist_core::contract::Evidence;
    /// use purrassist_core::tool_call::{CallLedger, RepairPolicy};
    /// let mut ledger = CallLedger::default();
    /// ledger.evidence.push(Evidence { rule: "tool_call.name.match".into(), explanation: String::new(), span: None, destination: String::new() });
    /// assert_eq!(ledger.unadmitted(RepairPolicy::Structural), Some("tool_call.name.match"));
    /// assert_eq!(ledger.unadmitted(RepairPolicy::Tolerant), None);
    /// ```
    #[must_use]
    pub fn unadmitted(&self, policy: RepairPolicy) -> Option<&str> {
        self.evidence
            .iter()
            .find(|evidence| rules::rule(&evidence.rule).is_none_or(|rule| !rule.admits(policy)))
            .map(|evidence| evidence.rule.as_str())
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.evidence.extend(other.evidence);
        self.losses.extend(other.losses);
        self.inferences.extend(other.inferences);
    }

    pub(crate) fn entries(&self) -> u64 {
        size(self.evidence.len() + self.losses.len() + self.inferences.len())
    }

    /// Record one application of `rule`: its evidence, a loss of
    /// `original` when the rule loses information, and a guess inference
    /// of `value` when it guesses.
    pub(crate) fn record(
        &mut self,
        rule: &Rule,
        evidence: Evidence,
        original: String,
        value: String,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let entries = 1 + u64::from(rule.loss.is_some()) + u64::from(rule.guess);
        governor.charge(Resource::EvidenceEntries, entries)?;
        governor.charge(Resource::ScratchBytes, entries.saturating_mul(ENTRY_BYTES))?;
        if let Some(kind) = rule.loss {
            self.losses.push(CallLoss {
                kind,
                loss: Loss {
                    evidence: evidence.clone(),
                    original,
                },
            });
        }
        if rule.guess {
            self.inferences.push(Inference {
                evidence: evidence.clone(),
                kind: InferenceKind::Guess,
                value,
            });
        }
        self.evidence.push(evidence);
        Ok(())
    }

    /// The ledger of a search candidate, with each loss's kind from its
    /// rule.
    fn from_search(ledger: Ledger, governor: &mut Governor) -> Result<Self, StopReason> {
        let entries = size(ledger.evidence.len() + ledger.losses.len() + ledger.inferences.len());
        governor.charge(Resource::EvidenceEntries, entries)?;
        governor.charge(Resource::ScratchBytes, entries.saturating_mul(ENTRY_BYTES))?;
        Ok(Self {
            evidence: ledger.evidence,
            losses: ledger
                .losses
                .into_iter()
                .map(|loss| CallLoss {
                    kind: loss_kind(&loss.evidence.rule),
                    loss,
                })
                .collect(),
            inferences: ledger.inferences,
        })
    }
}

/// The loss kind a rule id records; data for an unregistered id.
pub(crate) fn loss_kind(rule: &str) -> LossKind {
    rules::rule(rule)
        .and_then(|rule| rule.loss)
        .unwrap_or(LossKind::Data)
}

/// Why a call reading cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnresolvedReason {
    /// The call names no tool.
    MissingName,
    /// No declared tool matches the name (a hallucinated name, or no
    /// near-miss acceptable to name matching).
    UnknownTool {
        /// The name as written.
        name: String,
    },
    /// A rule the reading needs is not admitted by its tool's policy.
    RuleNotAdmitted {
        /// The rule id.
        rule: String,
        /// The policy that refuses it.
        policy: RepairPolicy,
    },
    /// An argument value could not be decoded.
    InvalidArgument {
        /// JSON Pointer of the value within the arguments.
        pointer: String,
        /// Why it could not be decoded.
        fault: super::ArgumentFault,
    },
    /// The arguments are not a JSON object.
    ArgumentsNotObject,
    /// A parameter the tool does not declare, under
    /// [`UnknownParameters::Error`], or a near miss of a declared name
    /// under `Structural` and [`UnknownParameters::DropWithLoss`] (never
    /// dropped in place of the rename).
    UnknownParameter {
        /// JSON Pointer of the parameter as written.
        pointer: String,
    },
    /// A required parameter nothing supplies.
    MissingRequired {
        /// JSON Pointer of the parameter.
        pointer: String,
    },
    /// The arguments fail the tool's schema; see the reading's validations.
    SchemaValidation,
    /// The output stopped (not by length) inside a call that end-of-output
    /// closers cannot complete (D10).
    Malformed {
        /// What is incomplete.
        detail: String,
    },
    /// A length stop cut the call off (truncation).
    Truncated,
    /// The request's tool choice was `none`, so no call may be made
    /// (recorded by [`ToolSet::recover_governed`]).
    ToolChoiceNone,
    /// The request forced a call to another tool (recorded by
    /// [`ToolSet::recover_governed`]).
    ToolChoiceForced {
        /// The canonical name of the tool the request forces.
        expected: String,
    },
    /// Only canonical parsing is permitted ([`Policies::exact`]) and the
    /// reading needs this repair rule.
    RepairNotPermitted {
        /// The first repair rule the reading needs.
        rule: String,
    },
}

impl UnresolvedReason {
    /// A stable snake-case code naming the variant, used by wire forms.
    ///
    /// ```
    /// use purrassist_core::tool_call::UnresolvedReason;
    /// assert_eq!(UnresolvedReason::Truncated.code(), "truncated");
    /// assert_eq!(UnresolvedReason::ToolChoiceForced { expected: "f".into() }.code(), "tool_choice_forced");
    /// ```
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingName => "missing_name",
            Self::UnknownTool { .. } => "unknown_tool",
            Self::RuleNotAdmitted { .. } => "rule_not_admitted",
            Self::InvalidArgument { .. } => "invalid_argument",
            Self::ArgumentsNotObject => "arguments_not_object",
            Self::UnknownParameter { .. } => "unknown_parameter",
            Self::MissingRequired { .. } => "missing_required",
            Self::SchemaValidation => "schema_validation",
            Self::Malformed { .. } => "malformed",
            Self::Truncated => "truncated",
            Self::ToolChoiceNone => "tool_choice_none",
            Self::ToolChoiceForced { .. } => "tool_choice_forced",
            Self::RepairNotPermitted { .. } => "repair_not_permitted",
        }
    }
}

impl fmt::Display for UnresolvedReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingName => formatter.write_str("the call names no tool"),
            Self::UnknownTool { name } => write!(formatter, "no declared tool matches `{name}`"),
            Self::RuleNotAdmitted { rule, policy } => {
                write!(formatter, "rule `{rule}` is not admitted under {policy:?}")
            }
            Self::InvalidArgument { pointer, fault } => {
                if pointer.is_empty() {
                    formatter.write_str("the arguments value")?;
                } else {
                    write!(formatter, "argument `{pointer}`")?;
                }
                write!(formatter, " cannot be decoded: {fault}")
            }
            Self::ArgumentsNotObject => formatter.write_str("the arguments are not a JSON object"),
            Self::UnknownParameter { pointer } => {
                write!(formatter, "parameter `{pointer}` is not declared")
            }
            Self::MissingRequired { pointer } => {
                write!(formatter, "required parameter `{pointer}` is missing")
            }
            Self::SchemaValidation => formatter.write_str("the arguments fail the tool's schema"),
            Self::Malformed { detail } => write!(formatter, "malformed call: {detail}"),
            Self::Truncated => formatter.write_str("the call was cut off by a length stop"),
            Self::ToolChoiceNone => {
                formatter.write_str("the request's tool choice `none` forbids every call")
            }
            Self::ToolChoiceForced { expected } => {
                write!(
                    formatter,
                    "the request's tool choice forces tool `{expected}`"
                )
            }
            Self::RepairNotPermitted { rule } => write!(
                formatter,
                "repair `{rule}` is needed but only canonical parsing is permitted"
            ),
        }
    }
}

/// The facts [`CallOutcome::classify`] decides from.
#[derive(Clone, Copy, Debug)]
pub struct CallFacts<'a> {
    /// The tool's effective effects.
    pub effects: Effects,
    /// The tool's effective repair policy.
    pub policy: RepairPolicy,
    /// The selected reading's repairs.
    pub ledger: &'a CallLedger,
    /// How many distinct readings compete at the least cost.
    pub competing: usize,
    /// Why the selected reading cannot be used, if it cannot.
    pub blocker: Option<&'a UnresolvedReason>,
}

/// The per-call outcome of resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallOutcome {
    /// One reading, with no guess and no data loss.
    Resolved,
    /// One reading with a guess or a data loss.
    BestGuess,
    /// Several equally good readings; see the call's alternatives.
    Ambiguous,
    /// No usable reading.
    Unresolved {
        /// Why.
        reason: UnresolvedReason,
    },
}

impl CallOutcome {
    /// Classify a call: the single place enforcing that
    ///
    /// - a blocked reading is `Unresolved` with its reason;
    /// - several competing readings are `Ambiguous`;
    /// - a reading using a rule its policy does not admit is `Unresolved`
    ///   ([`UnresolvedReason::RuleNotAdmitted`]), so `Structural` admits
    ///   single-reading repairs only;
    /// - any guess inference makes the call at most `BestGuess`, and so a
    ///   `Mutating` tool's call is never `Resolved` through a guess (A13);
    /// - any data loss makes the call at most `BestGuess`;
    /// - markup losses never demote.
    ///
    /// ```
    /// use purrassist_core::contract::{Evidence, Inference, InferenceKind, Loss};
    /// use purrassist_core::grammar::edit::LossKind;
    /// use purrassist_core::tool_call::{CallFacts, CallLedger, CallLoss, CallOutcome, Effects, RepairPolicy};
    /// let evidence = |rule: &str| Evidence { rule: rule.into(), explanation: String::new(), span: None, destination: String::new() };
    /// let mut ledger = CallLedger::default();
    /// ledger.evidence.push(evidence("tool_call.markup.closers-only-empty"));
    /// ledger.losses.push(CallLoss { kind: LossKind::Markup, loss: Loss { evidence: evidence("tool_call.markup.closers-only-empty"), original: "</parameters>".into() } });
    /// fn facts(ledger: &CallLedger, policy: RepairPolicy) -> CallFacts<'_> {
    ///     CallFacts { effects: Effects::Mutating, policy, ledger, competing: 1, blocker: None }
    /// }
    /// // A markup loss never demotes, even on a mutating tool under Structural.
    /// assert_eq!(CallOutcome::classify(facts(&ledger, RepairPolicy::Structural)), CallOutcome::Resolved);
    /// ledger.evidence.push(evidence("tool_call.name.match"));
    /// ledger.inferences.push(Inference { evidence: evidence("tool_call.name.match"), kind: InferenceKind::Guess, value: "f".into() });
    /// // A guess is not admitted under Structural, and is at most a best guess.
    /// assert!(matches!(CallOutcome::classify(facts(&ledger, RepairPolicy::Structural)), CallOutcome::Unresolved { .. }));
    /// assert_eq!(CallOutcome::classify(facts(&ledger, RepairPolicy::Tolerant)), CallOutcome::BestGuess);
    /// ```
    #[must_use]
    pub fn classify(facts: CallFacts<'_>) -> Self {
        if let Some(reason) = facts.blocker {
            return Self::Unresolved {
                reason: reason.clone(),
            };
        }
        if facts.competing > 1 {
            return Self::Ambiguous;
        }
        if let Some(rule) = facts.ledger.unadmitted(facts.policy) {
            return Self::Unresolved {
                reason: UnresolvedReason::RuleNotAdmitted {
                    rule: rule.to_owned(),
                    policy: facts.policy,
                },
            };
        }
        let guessed = facts.ledger.guessed();
        // A guess is never resolved, whatever the effects; for a mutating
        // tool this is the A13 guarantee.
        if guessed || facts.ledger.data_loss() {
            return Self::BestGuess;
        }
        debug_assert!(!(facts.effects == Effects::Mutating && guessed));
        Self::Resolved
    }
}

/// One reading of a call after resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallReading {
    /// The canonical name of the tool the reading resolved to, if any.
    pub tool: Option<String>,
    /// The tool's effective effects, when a tool was identified.
    pub effects: Option<Effects>,
    /// The tool's effective repair policy, when a tool was identified.
    pub policy: Option<RepairPolicy>,
    /// The arguments: resolved and canonical when the reading resolved,
    /// otherwise the best tree resolution reached.
    pub arguments: Node,
    /// The ranking cost: the sum of every applied rule's cost.
    pub cost: u64,
    /// Every repair, itemized.
    pub repairs: CallLedger,
    /// The schema validations performed.
    pub validations: Vec<Validation>,
    /// Why the reading cannot be used, if it cannot.
    pub blocker: Option<UnresolvedReason>,
}

/// One call after resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCall {
    /// The call's source range.
    pub span: Range<usize>,
    /// The dialect that recognized it.
    pub dialect: DialectId,
    /// The call's position in its reading of the turn.
    pub order: usize,
    /// The captured call id.
    pub id: Option<Spanned<String>>,
    /// The tool name as written.
    pub name: Option<Spanned<String>>,
    /// How the call's markup was closed.
    pub closure: Closure,
    /// The per-call outcome.
    pub outcome: CallOutcome,
    /// The selected reading (the first of the least-cost readings).
    pub reading: CallReading,
    /// How many distinct readings compete at the least cost: the selected
    /// reading and the first `competing - 1` alternatives (1 unless the call
    /// is `Ambiguous`).
    pub competing: usize,
    /// Every other reading, best first (R5.2).
    pub alternatives: Vec<CallReading>,
    /// For an `Ambiguous` call, the provenance that distinguishes the two
    /// best readings ([`Forest::distinguishing`]).
    pub distinguishing: Vec<ProvenanceNode>,
}

impl ResolvedCall {
    /// The canonical name of the selected reading's tool.
    ///
    /// See [`resolve_forest`] for an example.
    #[must_use]
    pub fn tool(&self) -> Option<&str> {
        self.reading.tool.as_deref()
    }

    /// The selected reading's arguments.
    ///
    /// See [`resolve_forest`] for an example.
    #[must_use]
    pub const fn arguments(&self) -> &Node {
        &self.reading.arguments
    }

    /// The selected reading's repairs.
    ///
    /// See [`resolve_forest`] for an example.
    #[must_use]
    pub const fn repairs(&self) -> &CallLedger {
        &self.reading.repairs
    }
}

/// One segment of a resolved turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedSegment {
    /// Plain assistant content.
    Content(Range<usize>),
    /// A reasoning block and its embedded calls, resolved.
    Reasoning {
        /// The block.
        reasoning: ReasoningSegment,
        /// Its embedded calls (guess-flagged alternatives).
        embedded: Vec<ResolvedCall>,
    },
    /// A tool call (boxed: a resolved call is large).
    Call(Box<ResolvedCall>),
    /// A tool result.
    Result(CallSegment),
    /// A rejected call attempt.
    Rejected {
        /// The attempt's source range.
        span: Range<usize>,
        /// The dialect whose opener started it.
        dialect: DialectId,
        /// Why it was rejected.
        reason: Rejection,
        /// Message-level repairs: a call attempt still open at the end of
        /// output records a `tool_call.markup.dangling-opener` data loss.
        repairs: CallLedger,
    },
}

impl ResolvedSegment {
    /// The segment's source range.
    ///
    /// ```
    /// use purrassist_core::tool_call::ResolvedSegment;
    /// assert_eq!(ResolvedSegment::Content(2..5).span(), 2..5);
    /// ```
    #[must_use]
    pub fn span(&self) -> Range<usize> {
        match self {
            Self::Content(span) | Self::Rejected { span, .. } => span.clone(),
            Self::Reasoning { reasoning, .. } => reasoning.span.clone(),
            Self::Call(call) => call.span.clone(),
            Self::Result(result) => result.span.clone(),
        }
    }
}

/// A span with competing segmentations, each resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedChoice {
    /// The contested source range.
    pub span: Range<usize>,
    /// The competing segmentations, in the recognized turn's order.
    pub readings: Vec<Vec<ResolvedSegment>>,
    /// The recognized derivation of each segmentation.
    pub derivations: Vec<Derivation>,
}

/// One piece of a resolved turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedPiece {
    /// A segment every reading shares.
    Segment(ResolvedSegment),
    /// Competing segmentations of one span.
    Choice(ResolvedChoice),
}

/// A recognized turn with every call resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTurn {
    /// The SHA-256 digest of the output.
    pub source: Digest,
    /// The number of output bytes.
    pub len: usize,
    /// The generation stop the turn was recognized under.
    pub stop: GenerationStop,
    /// The least segmentation cost of the recognized turn.
    pub cost: u64,
    /// The factorized segmentation, mirroring
    /// [`RecognizedTurn::pieces`].
    pub pieces: Vec<ResolvedPiece>,
}

impl ResolvedTurn {
    /// The selected reading: every shared segment plus the first reading
    /// of every choice, in order.
    ///
    /// See [`resolve_forest`] for an example.
    #[must_use]
    pub fn selected(&self) -> Vec<&ResolvedSegment> {
        let mut segments = Vec::new();
        for piece in &self.pieces {
            match piece {
                ResolvedPiece::Segment(segment) => segments.push(segment),
                ResolvedPiece::Choice(choice) => segments.extend(&choice.readings[0]),
            }
        }
        segments
    }

    /// The calls of the selected reading, in order.
    ///
    /// See [`resolve_forest`] for an example.
    #[must_use]
    pub fn calls(&self) -> Vec<&ResolvedCall> {
        self.selected()
            .into_iter()
            .filter_map(|segment| match segment {
                ResolvedSegment::Call(call) => Some(&**call),
                _ => None,
            })
            .collect()
    }
}

/// Resolve every call of `turn` against `tools` under `policies`.
///
/// # Errors
/// [`StopReason::InvalidInput`] for an effects override naming a tool the
/// set does not declare ([`Policies::validate`]); otherwise only the
/// governor's exhaustion or cancellation, with nothing partial returned.
///
/// ```
/// use purrassist_core::tool_call::{
///     CallOutcome, DialectId, DialectSet, Effects, GenerationStop, Param, Policies,
///     ReasoningPolicy, Recognizer, ToolDefinition, ToolSet, resolve_forest,
/// };
/// use purrassist_core::{Budget, Governor};
/// let mut governor = Governor::new(Budget::default());
/// let tools = ToolSet::compile(
///     [ToolDefinition::new("read_file").effects(Effects::ReadOnly).param("path", Param::string().required())],
///     Budget::default(),
/// ).unwrap();
/// let set = DialectSet::new([DialectId::Hermes], &mut governor).unwrap();
/// let mut recognizer = Recognizer::with_tools(&set, &tools, ReasoningPolicy::default(), &mut governor)?;
/// recognizer.push(br#"<tool_call>{"name": "read_fiel", "arguments": {"path": "a.rs"}}</tool_call>"#, &mut governor)?;
/// let turn = recognizer.finish(GenerationStop::ToolCalls, &mut governor)?;
/// let resolved = resolve_forest(&tools, &turn, &Policies::default(), &mut governor)?;
/// let call = resolved.calls()[0];
/// assert_eq!(call.outcome, CallOutcome::BestGuess);
/// assert_eq!(call.tool(), Some("read_file"));
/// assert_eq!(call.arguments().to_json(), r#"{"path":"a.rs"}"#);
/// assert_eq!(call.repairs().evidence[0].rule, "tool_call.name.match");
/// assert_eq!(resolved.selected().len(), 1);
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
pub fn resolve_forest(
    tools: &ToolSet,
    turn: &RecognizedTurn,
    policies: &Policies,
    governor: &mut Governor,
) -> Result<ResolvedTurn, StopReason> {
    governor.check()?;
    policies.validate(tools)?;
    let mut candidates = Vec::with_capacity(tools.tools().len());
    for tool in tools.tools() {
        governor.charge(
            Resource::ScratchBytes,
            size(tool.name().len() * 2 + tool.aliases().iter().map(String::len).sum::<usize>())
                .saturating_add(ENTRY_BYTES),
        )?;
        candidates.push(Candidate {
            id: tool.name().to_owned(),
            text: tool.name().to_owned(),
            aliases: tool.aliases().to_vec(),
        });
    }
    let resolver = Resolver {
        tools,
        policies,
        turn,
        candidates,
    };
    let mut pieces = Vec::with_capacity(turn.pieces().len());
    for piece in turn.pieces() {
        governor.charge(Resource::Work, 1)?;
        pieces.push(match piece {
            Piece::Segment(segment) => ResolvedPiece::Segment(resolver.segment(segment, governor)?),
            Piece::Choice(choice) => {
                let mut readings = Vec::with_capacity(choice.readings.len());
                for reading in &choice.readings {
                    let mut segments = Vec::with_capacity(reading.len());
                    for segment in reading {
                        segments.push(resolver.segment(segment, governor)?);
                    }
                    readings.push(segments);
                }
                ResolvedPiece::Choice(ResolvedChoice {
                    span: choice.span.clone(),
                    readings,
                    derivations: choice.derivations.clone(),
                })
            }
        });
    }
    Ok(ResolvedTurn {
        source: turn.source(),
        len: turn.len(),
        stop: turn.stop().clone(),
        cost: turn.cost(),
        pieces,
    })
}

/// Whether a recognized call is content by the counter-evidence of a
/// `declaration-content` grammar
/// ([`CompiledGrammar::declaration_content`](crate::grammar::CompiledGrammar::declaration_content)):
/// its object is bare ([`RawCall::bare`]), its arguments as written have a
/// tool declaration's parameters shape (an object whose `"type"` is
/// `"object"` and whose `"properties"` is an object), and the declared tool
/// does not take them as they are (the call is not
/// [`CallOutcome::Resolved`]: the named tool is undeclared, or its arguments
/// schema needs a guess or a loss to accept them).
fn declaration_content(segment: &CallSegment, call: &ResolvedCall) -> bool {
    let raw = &segment.readings[0];
    if !raw.bare || !raw.dialect.grammar().declaration_content() {
        return false;
    }
    let root = &raw.arguments.root;
    let shaped = matches!(root.get("type").map(|node| &node.value), Some(Value::String(kind)) if kind == "object")
        && matches!(
            root.get("properties").map(|node| &node.value),
            Some(Value::Object(_))
        );
    shaped && call.outcome != CallOutcome::Resolved
}

/// A value with the changes that produced it.
#[derive(Clone, Debug)]
struct Typed {
    value: Node,
    ledger: CallLedger,
    cost: u64,
    blocker: Option<UnresolvedReason>,
}

impl Typed {
    const fn plain(value: Node) -> Self {
        Self {
            value,
            ledger: CallLedger {
                evidence: Vec::new(),
                losses: Vec::new(),
                inferences: Vec::new(),
            },
            cost: 0,
            blocker: None,
        }
    }
}

/// One reading of an object scope.
#[derive(Clone, Debug)]
struct Draft {
    members: Vec<Member>,
    ledger: CallLedger,
    cost: u64,
    blocker: Option<UnresolvedReason>,
}

/// Where a value sits: its pointer in the resolved arguments and in the
/// recognized arguments (the source of its leaves).
#[derive(Clone, Debug)]
struct Site {
    pointer: String,
    source: String,
}

impl Site {
    fn child(&self, key: &str, written: &str) -> Self {
        Self {
            pointer: format!("{}/{}", self.pointer, pointer_segment(key)),
            source: format!("{}/{}", self.source, pointer_segment(written)),
        }
    }

    fn item(&self, index: usize, source: usize) -> Self {
        Self {
            pointer: format!("{}/{index}", self.pointer),
            source: format!("{}/{source}", self.source),
        }
    }
}

/// One occurrence of a declared parameter in an object scope.
#[derive(Clone, Debug)]
struct Occurrence {
    key: String,
    key_span: Option<Range<usize>>,
    value: Node,
    source: String,
}

/// One slot of a resolved object scope, in output order.
#[derive(Clone, Debug)]
enum Slot {
    Parameter(usize, Vec<Occurrence>),
    Kept(Member),
}

/// What resolution of one call knows about its context.
struct Scope<'r> {
    raw: &'r RawCall,
    policy: RepairPolicy,
}

struct Resolver<'a> {
    tools: &'a ToolSet,
    policies: &'a Policies,
    turn: &'a RecognizedTurn,
    candidates: Vec<Candidate>,
}

impl<'a> Resolver<'a> {
    fn segment(
        &self,
        segment: &Segment,
        governor: &mut Governor,
    ) -> Result<ResolvedSegment, StopReason> {
        Ok(match segment {
            Segment::Content(span) => ResolvedSegment::Content(span.clone()),
            Segment::Reasoning(reasoning) => {
                governor.charge(
                    Resource::ScratchBytes,
                    size(reasoning.embedded.len()).saturating_mul(CALL_BYTES),
                )?;
                let mut embedded = Vec::with_capacity(reasoning.embedded.len());
                for call in &reasoning.embedded {
                    embedded.push(self.call(call, governor)?);
                }
                governor.charge(Resource::ScratchBytes, ENTRY_BYTES)?;
                ResolvedSegment::Reasoning {
                    reasoning: reasoning.clone(),
                    embedded,
                }
            }
            Segment::Call(segment) => {
                let call = self.call(segment, governor)?;
                if declaration_content(segment, &call) {
                    ResolvedSegment::Content(segment.span.clone())
                } else {
                    governor.charge(Resource::ScratchBytes, CALL_BYTES)?;
                    ResolvedSegment::Call(Box::new(call))
                }
            }
            Segment::Result(result) => {
                governor.charge(Resource::ScratchBytes, ENTRY_BYTES)?;
                ResolvedSegment::Result(result.clone())
            }
            Segment::Rejected {
                span,
                dialect,
                reason,
            } => {
                let mut repairs = CallLedger::default();
                if *reason == Rejection::Unterminated && span.end == self.turn.len() {
                    let evidence = self.evidence(
                        "tool_call.markup.dangling-opener",
                        "a call attempt was still open at the end of output; its partial markup is dropped".into(),
                        Some(span.clone()),
                        format!("/turn/{}..{}", span.start, span.end),
                    );
                    repairs.record(
                        rules::resolution_rule("tool_call.markup.dangling-opener"),
                        evidence,
                        format!("bytes {}..{}", span.start, span.end),
                        String::new(),
                        governor,
                    )?;
                }
                ResolvedSegment::Rejected {
                    span: span.clone(),
                    dialect: dialect.clone(),
                    reason: *reason,
                    repairs,
                }
            }
        })
    }

    fn evidence(
        &self,
        rule: &str,
        explanation: String,
        span: Option<Range<usize>>,
        destination: String,
    ) -> Evidence {
        Evidence {
            rule: rule.into(),
            explanation,
            span: span.map(|bytes| SourceSpan {
                source: self.turn.source(),
                bytes,
            }),
            destination,
        }
    }

    /// Resolve one call segment: every reading of every recognized reading,
    /// ranked and classified.
    fn call(
        &self,
        segment: &CallSegment,
        governor: &mut Governor,
    ) -> Result<ResolvedCall, StopReason> {
        let mut groups: Vec<Vec<CallReading>> = Vec::with_capacity(segment.readings.len());
        for (index, raw) in segment.readings.iter().enumerate() {
            let base = self.base(segment.derivations.get(index), governor)?;
            let mut readings = self.readings(raw, base, governor)?;
            if self.policies.exact {
                for reading in &mut readings {
                    exact(reading, governor)?;
                }
            }
            groups.push(readings);
        }
        let total: usize = groups.iter().map(Vec::len).sum();
        // The ranking records, the selection slots and the distinct
        // readings are transient: at most two reading slots and two
        // ranking-sized records per reading.
        let _ranking = governor
            .lease_scratch(size(total).saturating_mul(2 * (RANKED_BYTES + READING_BYTES)))?;
        let mut slots: Vec<Option<CallReading>> = Vec::with_capacity(total);
        let mut ranked: Vec<(u64, Digest, Digest, usize)> = Vec::with_capacity(total);
        for reading in groups.into_iter().flatten() {
            let (content, full) = reading_digests(&reading, governor)?;
            ranked.push((reading.cost, full, content, slots.len()));
            slots.push(Some(reading));
        }
        // A total order: least cost first, then by digest; one reading per
        // distinct call (tool, arguments, blocker), its best derivation.
        ranked.sort_by_key(|&(cost, full, _, position)| (cost, full, position));
        let mut unique: Vec<(Digest, usize)> = Vec::with_capacity(ranked.len());
        for &(_, _, content, position) in &ranked {
            if !unique.iter().any(|(known, _)| *known == content) {
                unique.push((content, position));
            }
        }
        let mut readings: Vec<Option<CallReading>> = unique
            .iter()
            .map(|&(_, position)| slots[position].take())
            .collect();
        drop(slots);
        let admitted = |reading: &CallReading| {
            reading.blocker.is_none()
                && reading
                    .policy
                    .is_some_and(|policy| reading.repairs.unadmitted(policy).is_none())
        };
        let viable = |reading: &CallReading| reading.blocker.is_none();
        let filter: &dyn Fn(&CallReading) -> bool = if readings.iter().flatten().any(admitted) {
            &admitted
        } else {
            &viable
        };
        let first = readings
            .iter()
            .position(|reading| reading.as_ref().is_some_and(filter))
            .unwrap_or(0);
        let cost = readings[first].as_ref().map_or(0, |reading| reading.cost);
        let tied: Vec<usize> = (0..readings.len())
            .filter(|&index| {
                index != first
                    && readings[index]
                        .as_ref()
                        .is_some_and(|reading| filter(reading) && reading.cost == cost)
            })
            .collect();
        let competing = 1 + tied.len();
        let reading = readings[first].take().expect("the selected reading");
        // The alternatives are retained.
        governor.charge(
            Resource::ScratchBytes,
            size(readings.len()).saturating_mul(READING_BYTES),
        )?;
        let mut ordered: Vec<CallReading> = Vec::with_capacity(readings.len());
        for index in tied {
            ordered.push(readings[index].take().expect("a tied reading"));
        }
        ordered.extend(readings.into_iter().flatten());
        let distinguishing = if competing > 1 {
            distinguishing(&reading, &ordered[0], governor)?
        } else {
            Vec::new()
        };
        let outcome = CallOutcome::classify(CallFacts {
            effects: reading.effects.unwrap_or(Effects::Mutating),
            policy: reading.policy.unwrap_or(RepairPolicy::Structural),
            ledger: &reading.repairs,
            competing,
            blocker: reading.blocker.as_ref(),
        });
        let mut reading = reading;
        self.label_repeated_choice(&mut reading, &ordered, competing, governor)?;
        let first = &segment.readings[0];
        Ok(ResolvedCall {
            span: segment.span.clone(),
            dialect: first.dialect.clone(),
            order: first.order,
            id: first.id.clone(),
            name: first.name.clone(),
            closure: first.closure,
            outcome,
            reading,
            competing,
            alternatives: ordered,
            distinguishing,
        })
    }

    /// Make the choice among the readings of a repeated parameter that may
    /// not repeat explicit (`tool_call.parameter.repeated-choice`, a guess
    /// recorded after classification, so it never changes the outcome).
    /// When the selected reading kept one occurrence and others competed
    /// equally (`competing > 1`, every value valid), the call is
    /// `Ambiguous` and nothing preferred a value: the inference says the
    /// primary `arguments` is the first-ranked reading (least cost, then
    /// content digest) and points at the alternatives. When the reading
    /// keeping another occurrence is blocked (its value fails the declared
    /// type, enumeration or schema), or costs more (it needs a repair),
    /// the choice is grounded in that evidence, which the inference cites.
    fn label_repeated_choice(
        &self,
        reading: &mut CallReading,
        alternatives: &[CallReading],
        competing: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        const REPEATED: &str = "tool_call.parameter.repeated";
        if reading.blocker.is_some() {
            return Ok(());
        }
        let kept: Vec<(String, String)> = reading
            .repairs
            .inferences
            .iter()
            .filter(|inference| inference.evidence.rule == REPEATED)
            .map(|inference| {
                (
                    inference.evidence.destination.clone(),
                    inference.value.clone(),
                )
            })
            .collect();
        let Some((destination, _)) = kept.first().cloned() else {
            return Ok(());
        };
        let mut kept_list: Vec<String> = Vec::new();
        for (place, value) in &kept {
            let item = format!("{value} at {place}");
            if !kept_list.contains(&item) {
                kept_list.push(item);
            }
        }
        let kept_text = kept_list.join(", ");
        let others = alternatives.iter().enumerate().filter(|(_, other)| {
            other
                .repairs
                .evidence
                .iter()
                .any(|entry| entry.rule == REPEATED)
        });
        let explanation = if competing > 1 {
            let tied: Vec<String> = (0..competing - 1).map(|index| index.to_string()).collect();
            format!(
                "a parameter that may not repeat occurs several times and every occurrence's value is valid, so no evidence prefers one: the call is Ambiguous, the primary arguments are the first-ranked reading (least cost, then content digest), keeping {kept_text}, and alternative(s) [{}] keep the other occurrences",
                tied.join(", ")
            )
        } else {
            let reasons: Vec<String> = others
                .map(|(index, other)| match &other.blocker {
                    Some(blocker) => format!("alternative {index} is blocked ({blocker})"),
                    None => format!(
                        "alternative {index} costs {} against {}",
                        other.cost, reading.cost
                    ),
                })
                .collect();
            if reasons.is_empty() {
                format!("the reading keeping {kept_text} is the only one")
            } else {
                format!("kept {kept_text} on evidence: {}", reasons.join("; "))
            }
        };
        let rule = rules::resolution_rule("tool_call.parameter.repeated-choice");
        let evidence = self.evidence(rule.id, explanation, None, destination);
        reading
            .repairs
            .record(rule, evidence, String::new(), kept_text, governor)
    }

    /// The ledger of a recognized reading's derivation: its edits and any
    /// guess it inherits.
    fn base(
        &self,
        derivation: Option<&Derivation>,
        governor: &mut Governor,
    ) -> Result<CallLedger, StopReason> {
        let mut ledger = CallLedger::default();
        let Some(derivation) = derivation else {
            return Ok(ledger);
        };
        governor.charge(
            Resource::ScratchBytes,
            size(derivation.nodes().len()).saturating_mul(ENTRY_BYTES),
        )?;
        for &id in derivation.nodes() {
            match self.turn.forest().node(id) {
                ProvenanceNode::Evidence(evidence) => ledger.evidence.push(evidence.clone()),
                ProvenanceNode::Loss(loss) => ledger.losses.push(CallLoss {
                    kind: loss_kind(&loss.evidence.rule),
                    loss: loss.clone(),
                }),
                ProvenanceNode::Inference(inference) => ledger.inferences.push(inference.clone()),
                ProvenanceNode::Validation(..) => {}
            }
        }
        Ok(ledger)
    }

    /// Every reading of one recognized reading.
    fn readings(
        &self,
        raw: &RawCall,
        base: CallLedger,
        governor: &mut Governor,
    ) -> Result<Vec<CallReading>, StopReason> {
        governor.charge(Resource::Work, size(raw.edits.len()).saturating_add(1))?;
        let cost = raw.edits.iter().fold(0_u64, |total, edit| {
            total.saturating_add(rules::of_operator(edit.operator).cost)
        });
        let policies = self.policies;
        let unresolved = |blocker: UnresolvedReason, tool: Option<&CompiledTool>, ledger| {
            let effective = tool.map(|tool| policies.effective(tool));
            CallReading {
                tool: tool.map(|tool| tool.name().to_owned()),
                effects: effective.map(|(effects, _)| effects),
                policy: effective.map(|(_, policy)| policy),
                arguments: raw.arguments.root.clone(),
                cost,
                repairs: ledger,
                validations: Vec::new(),
                blocker: Some(blocker),
            }
        };
        governor.charge(Resource::ScratchBytes, raw.arguments.root.estimated_bytes())?;
        let exact = raw
            .name
            .as_ref()
            .and_then(|name| self.tools.lookup_exact(&name.value));
        if raw.closure == Closure::Dangling {
            let mut ledger = base;
            let evidence = self.evidence(
                "tool_call.truncated",
                "a length stop cut the call off; what was captured is kept, unexecuted".into(),
                Some(raw.span.clone()),
                destination(raw, ""),
            );
            ledger.record(
                rules::resolution_rule("tool_call.truncated"),
                evidence,
                format!("bytes {}..{}", raw.span.start, raw.span.end),
                String::new(),
                governor,
            )?;
            governor.charge(Resource::ScratchBytes, READING_BYTES)?;
            return Ok(vec![unresolved(UnresolvedReason::Truncated, exact, ledger)]);
        }
        let incomplete = (raw.closure == Closure::Unterminated)
            .then(|| self.incomplete(raw))
            .flatten()
            .map(|detail| UnresolvedReason::Malformed { detail });
        let early = incomplete.or_else(|| {
            raw.arguments
                .unresolved
                .first()
                .map(|fault| UnresolvedReason::InvalidArgument {
                    pointer: fault.pointer.clone(),
                    fault: fault.fault,
                })
        });
        let targets = match &raw.name {
            Some(name) => self.targets(raw, name, governor)?,
            None if early.is_some() => vec![Target::Blocked(UnresolvedReason::MissingName)],
            None => self.targets_from_arguments(raw, governor)?,
        };
        governor.charge(
            Resource::ScratchBytes,
            size(targets.len()).saturating_mul(TARGET_BYTES.saturating_add(READING_BYTES)),
        )?;
        let mut readings = Vec::with_capacity(targets.len());
        for target in targets {
            let mut ledger = base.clone();
            governor.charge(
                Resource::ScratchBytes,
                ledger.entries().saturating_mul(ENTRY_BYTES),
            )?;
            let (tool, change) = match target {
                Target::Blocked(reason) => {
                    readings.push(unresolved(early.clone().unwrap_or(reason), None, ledger));
                    continue;
                }
                Target::Tool(tool, change) => (tool, change),
            };
            let mut cost = cost;
            if let Some((rule, evidence, value, applied)) = change {
                cost = cost.saturating_add(applied);
                ledger.record(rule, evidence, String::new(), value, governor)?;
            }
            if let Some(reason) = early.clone() {
                let mut reading = unresolved(reason, Some(tool), ledger);
                reading.cost = cost;
                readings.push(reading);
                continue;
            }
            let produced = self.arguments(raw, tool, ledger, cost, governor)?;
            // Room for the readings beyond the one reserved per target.
            governor.charge(
                Resource::ScratchBytes,
                size(produced.len().saturating_sub(1)).saturating_mul(READING_BYTES),
            )?;
            readings.extend(produced);
        }
        Ok(readings)
    }

    /// Why a call completed by end-of-output closers is not otherwise
    /// complete (D10), if it is not.
    fn incomplete(&self, raw: &RawCall) -> Option<String> {
        let end = self.turn.len();
        if raw.name.is_none() {
            return Some("the call's name was not parsed".into());
        }
        if raw
            .captures
            .iter()
            .any(|capture| capture.kind == CaptureKind::ParamValue && capture.span.end == end)
        {
            return Some("a parameter value is still open at the end of output".into());
        }
        let arguments_end = raw
            .captures
            .iter()
            .find(|capture| capture.kind == CaptureKind::Arguments)
            .map(|capture| capture.span.end);
        let completed_at_end = raw.edits.iter().any(|edit| {
            edit.rule().id.starts_with("json.")
                && edit.operator != EditOperator::JsonComment
                && edit.span.start == end
        });
        if arguments_end == Some(end) && completed_at_end {
            return Some("the arguments value is incomplete at the end of output".into());
        }
        None
    }

    /// The tools a call's name can denote.
    fn targets(
        &self,
        raw: &RawCall,
        name: &Spanned<String>,
        governor: &mut Governor,
    ) -> Result<Vec<Target<'a>>, StopReason> {
        if let Some(tool) = self.tools.lookup_exact(&name.value) {
            let change = (tool.name() != name.value).then(|| {
                let rule = rules::resolution_rule("tool_call.name.alias");
                (
                    rule,
                    self.evidence(
                        rule.id,
                        format!(
                            "`{}` is an explicit alias of tool `{}`",
                            name.value,
                            tool.name()
                        ),
                        Some(name.span.clone()),
                        destination(raw, ""),
                    ),
                    String::new(),
                    rule.cost,
                )
            });
            return Ok(vec![Target::Tool(tool, change)]);
        }
        if self.policies.exact {
            // Exact parsing never matches names: reading any other name as a
            // tool would need the guess `tool_call.name.match`.
            return Ok(vec![Target::Blocked(
                UnresolvedReason::RepairNotPermitted {
                    rule: "tool_call.name.match".into(),
                },
            )]);
        }
        let result = matching::resolve_governed(
            &name.value,
            &self.candidates,
            &MatchRequest::default(),
            SelectionPolicy::default(),
            governor,
        );
        let matches = match result.outcome {
            Outcome::Resolved(found) | Outcome::BestGuess(found) => vec![found],
            Outcome::Ambiguous(found) => found,
            Outcome::Unresolved => {
                return match result.receipt.stopped {
                    StopReason::NoCandidate | StopReason::Completed => {
                        Ok(vec![Target::Blocked(UnresolvedReason::UnknownTool {
                            name: name.value.clone(),
                        })])
                    }
                    reason => Err(reason),
                };
            }
        };
        let found: Vec<(&CompiledTool, u32)> = matches
            .iter()
            .filter_map(|found| {
                self.tools
                    .tool(&found.candidate.id)
                    .map(|tool| (tool, found.score))
            })
            .collect();
        if found
            .iter()
            .any(|(tool, _)| self.policies.effective(tool).1 == RepairPolicy::Structural)
        {
            // A structural tool is named exactly or by alias only, and a
            // near-miss that could name one is never redirected elsewhere.
            return Ok(vec![Target::Blocked(UnresolvedReason::RuleNotAdmitted {
                rule: "tool_call.name.match".into(),
                policy: RepairPolicy::Structural,
            })]);
        }
        // The shared name cost of each match; a name the matcher accepts by
        // a coarser signal (phonetics, say) costs as much as the farthest
        // edit the name cost admits. Matches name matching found ambiguous
        // share the least cost, so ranking never breaks their tie.
        let mut costs = Vec::with_capacity(found.len());
        for (tool, _) in &found {
            costs.push(
                name_cost(&name.value, tool.name(), governor)?
                    .unwrap_or(crate::schema::recovery::EDITED_NAME_COST + 2),
            );
        }
        let shared = costs.iter().copied().min().unwrap_or(0);
        let rule = rules::resolution_rule("tool_call.name.match");
        let mut targets = Vec::with_capacity(found.len());
        for ((tool, score), own) in found.into_iter().zip(costs) {
            let base = if matches.len() > 1 { shared } else { own };
            let evidence = self.evidence(
                rule.id,
                format!(
                    "tool name `{}` read as `{}` (match score {score})",
                    name.value,
                    tool.name(),
                ),
                Some(name.span.clone()),
                destination(raw, ""),
            );
            targets.push(Target::Tool(
                tool,
                Some((
                    rule,
                    evidence,
                    tool.name().to_owned(),
                    base.saturating_mul(rules::multiplier(rule.grounding)),
                )),
            ));
        }
        Ok(targets)
    }

    /// The tools a call that names no tool can denote: the declared tools
    /// its arguments fit (`tool_call.name.from-arguments`). A tool fits when
    /// every required parameter is present and every member is one of its
    /// declared parameters or aliases (an undeclared member is no evidence
    /// for a tool, even one that keeps it under `PassThrough`). No fitting
    /// tool leaves the call
    /// [`UnresolvedReason::MissingName`]; a fitting tool under `Structural`
    /// blocks the guess, as for `tool_call.name.match`.
    fn targets_from_arguments(
        &self,
        raw: &RawCall,
        governor: &mut Governor,
    ) -> Result<Vec<Target<'a>>, StopReason> {
        const RULE: &str = "tool_call.name.from-arguments";
        let Value::Object(members) = &raw.arguments.root.value else {
            return Ok(vec![Target::Blocked(UnresolvedReason::MissingName)]);
        };
        let mut found: Vec<&CompiledTool> = Vec::new();
        for tool in self.tools.tools() {
            governor.charge(Resource::Work, size(members.len()).saturating_add(1))?;
            let object = &tool.argument_shape().arguments;
            governor.charge(
                Resource::ScratchBytes,
                size(members.len()).saturating_mul(size(core::mem::size_of::<&str>())),
            )?;
            let mut present: Vec<&str> = Vec::with_capacity(members.len());
            let mut fits = true;
            for member in members {
                // A member the tool does not declare is no evidence that the
                // call names it, even where the tool would keep the member.
                match object.resolve_name(&member.key) {
                    Some(canonical) => present.push(canonical),
                    None => fits = false,
                }
            }
            fits &= object
                .required
                .iter()
                .all(|required| present.contains(&required.as_str()));
            if fits {
                found.push(tool);
            }
        }
        if found.is_empty() {
            return Ok(vec![Target::Blocked(UnresolvedReason::MissingName)]);
        }
        if self.policies.exact {
            return Ok(vec![Target::Blocked(
                UnresolvedReason::RepairNotPermitted { rule: RULE.into() },
            )]);
        }
        if found
            .iter()
            .any(|tool| self.policies.effective(tool).1 == RepairPolicy::Structural)
        {
            return Ok(vec![Target::Blocked(UnresolvedReason::RuleNotAdmitted {
                rule: RULE.into(),
                policy: RepairPolicy::Structural,
            })]);
        }
        let rule = rules::resolution_rule(RULE);
        let keys: Vec<String> = members
            .iter()
            .map(|member| format!("`{}`", member.key))
            .collect();
        let keys = if keys.is_empty() {
            "no members".to_owned()
        } else {
            keys.join(", ")
        };
        let count = found.len();
        let mut targets = Vec::with_capacity(count);
        for tool in found {
            let explanation = if count == 1 {
                format!(
                    "the call names no tool; its arguments ({keys}) fit tool `{}` and no other declared tool",
                    tool.name()
                )
            } else {
                format!(
                    "the call names no tool; its arguments ({keys}) fit tool `{}`, one of {count} declared tools they fit",
                    tool.name()
                )
            };
            let evidence = self.evidence(
                rule.id,
                explanation,
                Some(raw.span.clone()),
                destination(raw, ""),
            );
            targets.push(Target::Tool(
                tool,
                Some((rule, evidence, tool.name().to_owned(), rule.cost)),
            ));
        }
        Ok(targets)
    }

    /// Every reading of a call's arguments for `tool`.
    fn arguments(
        &self,
        raw: &RawCall,
        tool: &CompiledTool,
        ledger: CallLedger,
        cost: u64,
        governor: &mut Governor,
    ) -> Result<Vec<CallReading>, StopReason> {
        let (effects, policy) = self.policies.effective(tool);
        let make = |arguments: Node, ledger, cost, blocker| CallReading {
            tool: Some(tool.name().to_owned()),
            effects: Some(effects),
            policy: Some(policy),
            arguments,
            cost,
            repairs: ledger,
            validations: Vec::new(),
            blocker,
        };
        let root = &raw.arguments.root;
        let Value::Object(members) = &root.value else {
            governor.charge(
                Resource::ScratchBytes,
                READING_BYTES.saturating_add(root.estimated_bytes()),
            )?;
            return Ok(vec![make(
                root.clone(),
                ledger,
                cost,
                Some(UnresolvedReason::ArgumentsNotObject),
            )]);
        };
        let scope = Scope { raw, policy };
        let site = Site {
            pointer: String::new(),
            source: String::new(),
        };
        let drafts = self.object(
            &scope,
            members,
            &tool.argument_shape().arguments,
            &site,
            governor,
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(drafts.len())
                .saturating_mul(READING_BYTES.saturating_add(size(tool.name().len()))),
        )?;
        let mut readings = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let arguments = Node {
                value: Value::Object(draft.members),
                span: root.span.clone(),
            };
            let mut repairs = ledger.clone();
            governor.charge(
                Resource::ScratchBytes,
                repairs.entries().saturating_mul(ENTRY_BYTES),
            )?;
            repairs.extend(draft.ledger);
            let mut blocker = draft.blocker;
            if raw.closure == Closure::Unterminated
                && let Some(UnresolvedReason::MissingRequired { pointer }) = &blocker
            {
                blocker = Some(UnresolvedReason::Malformed {
                    detail: format!(
                        "required parameter `{pointer}` is missing at the end of output"
                    ),
                });
            }
            let mut validations = Vec::new();
            let mut arguments = arguments;
            if blocker.is_none() {
                let (typed, ledger) = self.schema_typed(raw, tool, arguments, governor)?;
                arguments = typed;
                repairs.extend(ledger);
                let report = tool.schema().validate_with(&arguments, governor)?;
                let validation = report.to_validation();
                governor.charge(
                    Resource::ScratchBytes,
                    size(
                        validation
                            .diagnostics
                            .iter()
                            .map(String::len)
                            .sum::<usize>(),
                    )
                    .saturating_add(ENTRY_BYTES),
                )?;
                if !validation.passed {
                    blocker = Some(UnresolvedReason::SchemaValidation);
                }
                validations.push(validation);
            }
            let mut reading = make(arguments, repairs, cost.saturating_add(draft.cost), blocker);
            reading.validations = validations;
            readings.push(reading);
        }
        Ok(readings)
    }

    /// Type the XML text inside a parameter whose schema the typed model
    /// does not express ([`ValueShape::Unmodeled`]: `anyOf`, `oneOf`,
    /// `$ref`, a type array, ...) by the tool's exact schema itself: each
    /// verbatim string is also read as the JSON value it spells (a number
    /// keeping its exact spelling, a boolean, `null`, an array or object
    /// written inline), and the exact validator chooses. The verbatim string
    /// is kept wherever it validates (a string branch keeps the text, as a
    /// declared string does), the JSON reading is taken where it removes
    /// the violations at or around that value, and the verbatim text stays
    /// where neither validates (its `schema_validation` diagnosis
    /// unchanged). Each decision is recorded as
    /// `tool_call.value.schema-typed`, a definition-grounded single reading
    /// like `tool_call.value.decode-typed` for a declared type, so not a
    /// guess. `$ref` (and `$defs`/`definitions`) are resolved by the
    /// governed validator against the whole schema, so a recursive `$ref`
    /// never loops. The values are decided in batched rounds (a handful of
    /// validations whatever their number), each validation charged to the
    /// governor: exhaustion is an explicit `BudgetExceeded`, never a quiet
    /// verbatim fallback.
    fn schema_typed(
        &self,
        raw: &RawCall,
        tool: &CompiledTool,
        mut arguments: Node,
        governor: &mut Governor,
    ) -> Result<(Node, CallLedger), StopReason> {
        let mut ledger = CallLedger::default();
        if !holds_unmodeled(&tool.argument_shape().arguments) {
            return Ok((arguments, ledger));
        }
        let mut sites = Vec::new();
        schema_sites(
            &tool.argument_shape().arguments,
            &arguments,
            "",
            "",
            raw,
            &mut sites,
            governor,
        )?;
        if sites.is_empty() {
            return Ok((arguments, ledger));
        }
        let rule = rules::resolution_rule("tool_call.value.schema-typed");
        let Some((typed, accepted, both)) =
            schema_rounds(tool, &sites, arguments.clone(), governor)?
        else {
            return Ok((arguments, ledger));
        };
        arguments = typed;
        for (index, site) in sites.iter().enumerate() {
            if !accepted[index] && !both[index] {
                continue;
            }
            let keywords: Vec<String> = tool
                .unmodeled_keywords()
                .iter()
                .filter(|(pointer, _)| *pointer == site.schema)
                .map(|(_, keyword)| format!("`{keyword}`"))
                .collect();
            let keywords = if keywords.is_empty() {
                String::new()
            } else {
                format!(" ({})", keywords.join(", "))
            };
            let reading = site.reading.to_json();
            let kind = json_kind(&site.reading.value);
            let explanation = if accepted[index] {
                format!(
                    "read XML text `{}` as the JSON {kind} `{reading}`: the exact schema at `{}`{keywords} admits it and not the verbatim string",
                    site.text, site.schema
                )
            } else {
                format!(
                    "kept XML text `{}` verbatim: the exact schema at `{}`{keywords} admits both the string and the JSON {kind} `{reading}`, and a string branch keeps the verbatim text as a declared string does",
                    site.text, site.schema
                )
            };
            let evidence = self.evidence(
                rule.id,
                explanation,
                Some(site.raw.clone()),
                destination(raw, &site.pointer),
            );
            ledger.record(rule, evidence, String::new(), String::new(), governor)?;
        }
        Ok((arguments, ledger))
    }

    /// Every reading of an object scope's members against `shape`.
    fn object(
        &self,
        scope: &Scope<'_>,
        members: &[Member],
        shape: &ObjectShape,
        site: &Site,
        governor: &mut Governor,
    ) -> Result<Vec<Draft>, StopReason> {
        governor.charge(Resource::Depth, size(site.pointer.matches('/').count() + 1))?;
        governor.charge(Resource::Work, size(members.len()).saturating_add(1))?;
        let assignments = match self.assign(scope, members, shape, site, governor)? {
            Ok(assignments) => assignments,
            Err(reason) => {
                governor.charge(
                    Resource::ScratchBytes,
                    members
                        .iter()
                        .map(|member| {
                            MEMBER_BYTES
                                .saturating_add(size(member.key.len()))
                                .saturating_add(member.value.estimated_bytes())
                        })
                        .sum::<u64>()
                        .saturating_add(DRAFT_BYTES),
                )?;
                return Ok(vec![Draft {
                    members: members.to_vec(),
                    ledger: CallLedger::default(),
                    cost: 0,
                    blocker: Some(reason),
                }]);
            }
        };
        let mut drafts = Vec::new();
        for (slots, ledger, cost) in assignments {
            governor.charge(
                Resource::ScratchBytes,
                DRAFT_BYTES.saturating_add(size(slots.len()).saturating_mul(MEMBER_BYTES)),
            )?;
            let mut partial = vec![Draft {
                members: Vec::with_capacity(slots.len()),
                ledger,
                cost,
                blocker: None,
            }];
            for slot in slots {
                let alternatives: Vec<(Member, Typed)> = match slot {
                    Slot::Kept(member) => {
                        governor.charge(Resource::ScratchBytes, TYPED_BYTES)?;
                        vec![(member, Typed::plain(Node::new(Value::Null)))]
                    }
                    Slot::Parameter(index, occurrences) => {
                        let parameter = &shape.parameters[index];
                        let key_span = occurrences[0].key_span.clone();
                        let typed =
                            self.parameter(scope, parameter, occurrences, site, governor)?;
                        governor.charge(
                            Resource::ScratchBytes,
                            size(typed.len()).saturating_mul(
                                MEMBER_BYTES.saturating_add(size(parameter.name.len())),
                            ),
                        )?;
                        typed
                            .into_iter()
                            .map(|mut typed| {
                                // The value moves into the member; only the
                                // changes that produced it stay with it.
                                let value =
                                    core::mem::replace(&mut typed.value, Node::new(Value::Null));
                                (
                                    Member {
                                        key: parameter.name.clone(),
                                        key_span: key_span.clone(),
                                        value,
                                    },
                                    typed,
                                )
                            })
                            .collect()
                    }
                };
                partial = product(partial, alternatives, governor)?;
            }
            if drafts.is_empty() {
                // The first assignment's drafts are the list itself.
                drafts = partial;
            } else {
                governor.charge(
                    Resource::ScratchBytes,
                    size(drafts.len().saturating_add(partial.len()).saturating_mul(2))
                        .saturating_mul(DRAFT_BYTES),
                )?;
                drafts.extend(partial);
            }
        }
        Ok(drafts)
    }

    /// The name assignments of an object scope: each as its slots in
    /// output order, its ledger and cost; or why none exists.
    #[allow(clippy::type_complexity)]
    fn assign(
        &self,
        scope: &Scope<'_>,
        members: &[Member],
        shape: &ObjectShape,
        site: &Site,
        governor: &mut Governor,
    ) -> Result<Result<Vec<(Vec<Slot>, CallLedger, u64)>, UnresolvedReason>, StopReason> {
        let tolerant = scope.policy == RepairPolicy::Tolerant;
        let mut exact: Vec<Option<usize>> = Vec::with_capacity(members.len());
        let mut undecided = Vec::new();
        let mut options = Vec::new();
        let mut aliases = CallLedger::default();
        for (position, member) in members.iter().enumerate() {
            governor.charge(Resource::Work, 1)?;
            let canonical = shape.resolve_name(&member.key);
            let index = canonical.and_then(|canonical| {
                shape
                    .parameters
                    .iter()
                    .position(|parameter| parameter.name == canonical)
            });
            exact.push(index);
            if let Some(index) = index {
                if member.key != shape.parameters[index].name {
                    let evidence = self.evidence(
                        "tool_call.parameter.alias",
                        format!(
                            "parameter `{}` is an explicit alias of `{}`",
                            member.key, shape.parameters[index].name
                        ),
                        member.key_span.clone(),
                        destination(
                            scope.raw,
                            &site
                                .child(&shape.parameters[index].name, &member.key)
                                .pointer,
                        ),
                    );
                    aliases.record(
                        rules::resolution_rule("tool_call.parameter.alias"),
                        evidence,
                        String::new(),
                        String::new(),
                        governor,
                    )?;
                }
                continue;
            }
            undecided.push(position);
            options.push(self::options(member, shape, tolerant, governor)?);
        }
        let names = Names {
            members,
            shape,
            tolerant,
            exact,
            undecided,
            options,
            resolver: self,
            raw: scope.raw,
            site,
        };
        let seed = Assignment::seed();
        let accepted: Vec<(Assignment<'_>, Ledger)> =
            if names.undecided.is_empty() && names.missing(&seed).is_none() {
                vec![(seed, Ledger::default())]
            } else {
                let mut search = Search::new(governor);
                search.seed(governor, seed, Ledger::default())?;
                let mut context = names;
                let accepted = search.run(&mut context, governor)?;
                let found = accepted.into_states();
                if found.is_empty() {
                    return Ok(Err(context.failure()));
                }
                let mut unique: Vec<(Assignment<'_>, Ledger)> = Vec::new();
                for (state, ledger) in found {
                    if !unique.iter().any(|(known, _)| known.slots_equal(&state)) {
                        unique.push((state, ledger));
                    }
                }
                return assignments(&context, unique, &aliases, governor).map(Ok);
            };
        assignments(&names, accepted, &aliases, governor).map(Ok)
    }
}

/// Slots, ledgers and costs of accepted assignments.
#[allow(clippy::type_complexity)]
fn assignments(
    names: &Names<'_>,
    accepted: Vec<(Assignment<'_>, Ledger)>,
    aliases: &CallLedger,
    governor: &mut Governor,
) -> Result<Vec<(Vec<Slot>, CallLedger, u64)>, StopReason> {
    let mut assignments = Vec::with_capacity(accepted.len());
    for (state, ledger) in accepted {
        let cost = ledger.cost;
        let mut repairs = aliases.clone();
        repairs.extend(CallLedger::from_search(ledger, governor)?);
        assignments.push((names.slots(&state, governor)?, repairs, cost));
    }
    Ok(assignments)
}

/// The admissible assignments of one undeclared member, cheapest first.
fn options(
    member: &Member,
    shape: &ObjectShape,
    tolerant: bool,
    governor: &mut Governor,
) -> Result<Vec<NameOption>, StopReason> {
    let mut options: Vec<NameOption> = Vec::new();
    // Whether the member is a near miss of a declared name (one
    // `json.key.match` would rename it): needed to rename it under
    // `Tolerant`, and to refuse dropping it otherwise.
    let mut near_miss = false;
    let consult = tolerant || shape.unknown_parameters == UnknownParameters::DropWithLoss;
    for literal in shape.names.iter().filter(|_| consult) {
        let Some(base) = name_cost(&member.key, &literal.literal, governor)? else {
            continue;
        };
        near_miss = true;
        if tolerant {
            let Some(index) = shape
                .parameters
                .iter()
                .position(|parameter| parameter.name == literal.canonical)
            else {
                continue;
            };
            let cost = base.saturating_mul(rules::multiplier(
                rules::resolution_rule("json.key.match").grounding,
            ));
            match options
                .iter_mut()
                .find(|option| option.action == Action::Rename(index))
            {
                Some(known) if known.cost <= cost => {}
                Some(known) => known.cost = cost,
                None => options.push(NameOption {
                    action: Action::Rename(index),
                    cost,
                }),
            }
        }
    }
    // Without the rename, a near miss is unassignable: dropping it would
    // silently discard the value the evidence assigns to a declared
    // parameter. (Keeping it, under `PassThrough`, is its literal reading.)
    match shape.unknown_parameters {
        UnknownParameters::DropWithLoss if tolerant || !near_miss => options.push(NameOption {
            action: Action::Drop,
            cost: rules::resolution_rule("tool_call.parameter.drop-unknown").cost,
        }),
        UnknownParameters::Error | UnknownParameters::DropWithLoss => {}
        UnknownParameters::PassThrough => options.push(NameOption {
            action: Action::Keep,
            cost: rules::resolution_rule("tool_call.parameter.pass-through").cost,
        }),
    }
    options.sort_by_key(|option| (option.cost, option.action));
    governor.charge(Resource::Work, size(options.len()).saturating_add(1))?;
    Ok(options)
}

impl Resolver<'_> {
    /// Every reading of one declared parameter's occurrences.
    fn parameter(
        &self,
        scope: &Scope<'_>,
        parameter: &ParameterShape,
        occurrences: Vec<Occurrence>,
        site: &Site,
        governor: &mut Governor,
    ) -> Result<Vec<Typed>, StopReason> {
        governor.charge(Resource::Work, size(occurrences.len()))?;
        governor.charge(
            Resource::ScratchBytes,
            size(
                2 * (site.pointer.len()
                    + site.source.len()
                    + 2
                    + 2 * (parameter.name.len() + occurrences[0].key.len())),
            ),
        )?;
        let here = Site {
            pointer: format!("{}/{}", site.pointer, pointer_segment(&parameter.name)),
            source: format!("{}/{}", site.source, pointer_segment(&occurrences[0].key)),
        };
        let item_shape = match &parameter.value {
            ValueShape::Array(item) if !parameter.repeatable => Some(&**item),
            _ if parameter.repeatable => Some(&parameter.value),
            _ => None,
        };
        if let Some(item) = item_shape
            && (parameter.repeatable || occurrences.len() > 1)
        {
            return self.collect(scope, parameter, item, occurrences, &here, governor);
        }
        if occurrences.len() == 1 {
            let occurrence = &occurrences[0];
            let site = Site {
                pointer: here.pointer,
                source: occurrence.source.clone(),
            };
            return self.typed(
                scope,
                occurrence.value.clone(),
                &parameter.value,
                &site,
                governor,
            );
        }
        // A repeated parameter that may not repeat: first and last readings.
        let rule = rules::resolution_rule("tool_call.parameter.repeated");
        let last = occurrences.len() - 1;
        let identical = occurrences[0].value.to_json() == occurrences[last].value.to_json();
        let kept: &[usize] = if identical { &[0] } else { &[0, last] };
        let mut readings = Vec::new();
        for &keep in kept {
            let occurrence = &occurrences[keep];
            let site = Site {
                pointer: here.pointer.clone(),
                source: occurrence.source.clone(),
            };
            let mut ledger = CallLedger::default();
            for (index, dropped) in occurrences.iter().enumerate() {
                if index == keep {
                    continue;
                }
                let which = if keep == 0 { "first" } else { "last" };
                let evidence = self.evidence(
                    rule.id,
                    format!(
                        "parameter `{}` occurs {} times but may not repeat; this reading keeps the {which} occurrence and drops occurrence {}",
                        parameter.name,
                        occurrences.len(),
                        index + 1
                    ),
                    dropped.key_span.clone(),
                    destination(scope.raw, &here.pointer),
                );
                ledger.record(
                    rule,
                    evidence,
                    format!("\"{}\":{}", dropped.key, dropped.value.to_json()),
                    occurrence.value.to_json(),
                    governor,
                )?;
            }
            for mut typed in self.typed(
                scope,
                occurrence.value.clone(),
                &parameter.value,
                &site,
                governor,
            )? {
                let mut combined = ledger.clone();
                combined.extend(typed.ledger);
                typed.ledger = combined;
                typed.cost = typed.cost.saturating_add(rule.cost);
                readings.push(typed);
            }
        }
        Ok(readings)
    }

    /// The readings of an array or repeatable parameter: every
    /// occurrence's items, in order.
    fn collect(
        &self,
        scope: &Scope<'_>,
        parameter: &ParameterShape,
        item: &ValueShape,
        occurrences: Vec<Occurrence>,
        here: &Site,
        governor: &mut Governor,
    ) -> Result<Vec<Typed>, StopReason> {
        let mut ledger = CallLedger::default();
        let gathered = occurrences.len() > 1
            || occurrences
                .first()
                .is_some_and(|occurrence| !matches!(occurrence.value.value, Value::Array(_)));
        if gathered {
            let rule = rules::resolution_rule("tool_call.parameter.collect");
            let evidence = self.evidence(
                rule.id,
                format!(
                    "collected {} occurrence(s) of parameter `{}` as array items",
                    occurrences.len(),
                    parameter.name
                ),
                occurrences[0].key_span.clone(),
                destination(scope.raw, &here.pointer),
            );
            ledger.record(rule, evidence, String::new(), String::new(), governor)?;
        }
        let span = occurrences[0].value.span.clone();
        let mut items: Vec<(Node, String)> = Vec::new();
        for occurrence in occurrences {
            match occurrence.value.value {
                Value::Array(values) => {
                    for (index, value) in values.into_iter().enumerate() {
                        items.push((value, format!("{}/{index}", occurrence.source)));
                    }
                }
                _ => items.push((occurrence.value, occurrence.source)),
            }
        }
        let mut partial = vec![Typed {
            value: Node {
                value: Value::Array(Vec::new()),
                span,
            },
            ledger,
            cost: 0,
            blocker: None,
        }];
        for (index, (value, source)) in items.into_iter().enumerate() {
            let site = Site {
                pointer: format!("{}/{index}", here.pointer),
                source,
            };
            let typed = self.typed(scope, value, item, &site, governor)?;
            let mut next = Vec::with_capacity(partial.len() * typed.len());
            for base in &partial {
                for alternative in &typed {
                    governor.charge(Resource::Work, 1)?;
                    governor.charge(Resource::ScratchBytes, combined_bytes(base, alternative))?;
                    let mut combined = base.clone();
                    if let Value::Array(values) = &mut combined.value.value {
                        values.push(alternative.value.clone());
                    }
                    combined.ledger.extend(alternative.ledger.clone());
                    combined.cost = combined.cost.saturating_add(alternative.cost);
                    combined.blocker = combined.blocker.or_else(|| alternative.blocker.clone());
                    next.push(combined);
                }
            }
            partial = next;
        }
        Ok(partial)
    }

    /// Every reading of `value` as declared by `shape`.
    fn typed(
        &self,
        scope: &Scope<'_>,
        value: Node,
        shape: &ValueShape,
        site: &Site,
        governor: &mut Governor,
    ) -> Result<Vec<Typed>, StopReason> {
        governor.charge(Resource::Work, 1)?;
        governor.charge(
            Resource::ScratchBytes,
            value.estimated_bytes().saturating_add(TYPED_BYTES),
        )?;
        match shape {
            ValueShape::String(_) | ValueShape::Unmodeled => Ok(vec![Typed::plain(value)]),
            ValueShape::Enumeration(literals) => {
                self.enumeration(scope, value, literals, site, governor)
            }
            ValueShape::Integer | ValueShape::Number | ValueShape::Boolean => {
                Ok(vec![self.decoded(scope, value, shape, site, governor)?])
            }
            ValueShape::Array(item) => {
                let decoded = self.decoded(scope, value, shape, site, governor)?;
                let Value::Array(values) = &decoded.value.value else {
                    return Ok(vec![decoded]);
                };
                let mut partial = vec![Typed {
                    value: Node {
                        value: Value::Array(Vec::new()),
                        span: decoded.value.span.clone(),
                    },
                    ledger: decoded.ledger.clone(),
                    cost: decoded.cost,
                    blocker: None,
                }];
                for (index, value) in values.iter().enumerate() {
                    let typed = self.typed(
                        scope,
                        value.clone(),
                        item,
                        &site.item(index, index),
                        governor,
                    )?;
                    let mut next = Vec::with_capacity(partial.len() * typed.len());
                    for base in &partial {
                        for alternative in &typed {
                            governor.charge(Resource::Work, 1)?;
                            governor.charge(
                                Resource::ScratchBytes,
                                combined_bytes(base, alternative),
                            )?;
                            let mut combined = base.clone();
                            if let Value::Array(values) = &mut combined.value.value {
                                values.push(alternative.value.clone());
                            }
                            combined.ledger.extend(alternative.ledger.clone());
                            combined.cost = combined.cost.saturating_add(alternative.cost);
                            combined.blocker =
                                combined.blocker.or_else(|| alternative.blocker.clone());
                            next.push(combined);
                        }
                    }
                    partial = next;
                }
                Ok(partial)
            }
            ValueShape::Object(inner) => {
                let decoded = self.decoded(scope, value, shape, site, governor)?;
                let Value::Object(members) = &decoded.value.value else {
                    return Ok(vec![decoded]);
                };
                let drafts = self.object(scope, members, inner, site, governor)?;
                Ok(drafts
                    .into_iter()
                    .map(|draft| {
                        let mut ledger = decoded.ledger.clone();
                        ledger.extend(draft.ledger);
                        Typed {
                            value: Node {
                                value: Value::Object(draft.members),
                                span: decoded.value.span.clone(),
                            },
                            ledger,
                            cost: decoded.cost.saturating_add(draft.cost),
                            blocker: draft.blocker,
                        }
                    })
                    .collect())
            }
        }
    }

    /// XML text of a typed parameter read by its declared type, when the
    /// codec could not type it (its tool was only known after name
    /// resolution); any other value unchanged.
    fn decoded(
        &self,
        scope: &Scope<'_>,
        value: Node,
        shape: &ValueShape,
        site: &Site,
        governor: &mut Governor,
    ) -> Result<Typed, StopReason> {
        let Value::String(text) = &value.value else {
            return Ok(Typed::plain(value));
        };
        let Some(leaf) = scope
            .raw
            .arguments
            .leaves
            .iter()
            .find(|leaf| leaf.pointer == site.source && leaf.origin == ArgOrigin::XmlText)
        else {
            return Ok(Typed::plain(value));
        };
        let mut decoded = match crate::json::parse_with(text, governor) {
            Ok(decoded) => decoded,
            Err(crate::json::ParseFailure::Resource(reason)) => return Err(reason),
            Err(crate::json::ParseFailure::Syntax(_)) => return Ok(Typed::plain(value)),
        };
        let fits = matches!(
            (shape, &decoded.value),
            (ValueShape::Integer | ValueShape::Number, Value::Number(_))
                | (ValueShape::Boolean, Value::Bool(_))
                | (ValueShape::Array(_), Value::Array(_))
                | (ValueShape::Object(_), Value::Object(_))
        );
        if !fits {
            return Ok(Typed::plain(value));
        }
        rebase(&mut decoded, leaf.raw.start);
        let rule = rules::resolution_rule("tool_call.value.decode-typed");
        let mut typed = Typed::plain(decoded);
        let evidence = self.evidence(
            rule.id,
            format!(
                "read XML text `{text}` as a declared {}",
                shape.type_name().unwrap_or("value")
            ),
            Some(leaf.raw.clone()),
            destination(scope.raw, &site.pointer),
        );
        typed
            .ledger
            .record(rule, evidence, String::new(), String::new(), governor)?;
        typed.cost = rule.cost;
        Ok(typed)
    }

    /// The readings of an enumeration value.
    fn enumeration(
        &self,
        scope: &Scope<'_>,
        value: Node,
        literals: &[String],
        site: &Site,
        governor: &mut Governor,
    ) -> Result<Vec<Typed>, StopReason> {
        let Value::String(text) = &value.value else {
            return Ok(vec![Typed::plain(value)]);
        };
        if literals.iter().any(|literal| literal == text) {
            return Ok(vec![Typed::plain(value)]);
        }
        governor.charge(
            Resource::Work,
            size(literals.iter().map(String::len).sum::<usize>() + text.len()),
        )?;
        let normal = normalized(text);
        let equal: Vec<&String> = literals
            .iter()
            .filter(|literal| normalized(literal) == normal)
            .collect();
        let (rule, found): (&Rule, Vec<(&String, u64)>) = if equal.len() == 1 {
            let base = name_cost(text, equal[0], governor)?.unwrap_or(FOLDED_NAME_COST);
            (
                rules::resolution_rule("tool_call.value.enum-normalize"),
                vec![(equal[0], base)],
            )
        } else if scope.policy == RepairPolicy::Tolerant {
            let mut costed = Vec::new();
            for literal in literals {
                if let Some(base) = name_cost(text, literal, governor)? {
                    costed.push((literal, base));
                }
            }
            let least = costed.iter().map(|(_, cost)| *cost).min();
            costed.retain(|(_, cost)| Some(*cost) == least);
            (rules::resolution_rule("json.enum.match"), costed)
        } else {
            (rules::resolution_rule("json.enum.match"), Vec::new())
        };
        if found.is_empty() {
            return Ok(vec![Typed::plain(value)]);
        }
        governor.charge(
            Resource::ScratchBytes,
            size(found.len().saturating_sub(1)).saturating_mul(TYPED_BYTES),
        )?;
        let mut readings = Vec::with_capacity(found.len());
        for (literal, base) in found {
            let mut typed = Typed::plain(Node {
                value: Value::String(literal.clone()),
                span: value.span.clone(),
            });
            let evidence = self.evidence(
                rule.id,
                format!("read enumeration value `{text}` as the declared literal `{literal}`"),
                value.span.clone(),
                destination(scope.raw, &site.pointer),
            );
            typed
                .ledger
                .record(rule, evidence, String::new(), literal.clone(), governor)?;
            typed.cost = base.saturating_mul(rules::multiplier(rule.grounding));
            readings.push(typed);
        }
        Ok(readings)
    }
}

/// A name change: its rule, evidence, guessed value and applied cost.
type NameChange = (&'static Rule, Evidence, String, u64);

/// What a call's name can denote.
enum Target<'t> {
    /// A tool, with the name change that reached it.
    Tool(&'t CompiledTool, Option<NameChange>),
    /// No usable tool.
    Blocked(UnresolvedReason),
}

/// One assignment action of an undeclared member.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Action {
    /// Rename to the parameter at this index (`json.key.match`).
    Rename(usize),
    /// Drop with a data loss (`DropWithLoss`).
    Drop,
    /// Keep unchanged (`PassThrough`).
    Keep,
}

/// One costed assignment option.
#[derive(Clone, Copy, Debug)]
struct NameOption {
    action: Action,
    cost: u64,
}

/// The search state of one object scope's name assignment: an option per
/// decided undeclared member, in order, then relocations of required
/// parameters out of nested objects.
#[derive(Clone, Debug)]
struct Assignment<'n> {
    choices: Vec<u32>,
    /// `(member, child, parameter)`: move `child` of `member`'s object
    /// value to the scope as `parameter`.
    moves: Vec<(u32, u32, u32)>,
    marker: PhantomData<fn() -> &'n ()>,
}

impl Assignment<'_> {
    const fn seed() -> Self {
        Self {
            choices: Vec::new(),
            moves: Vec::new(),
            marker: PhantomData,
        }
    }

    fn slots_equal(&self, other: &Self) -> bool {
        self.choices == other.choices && self.moves == other.moves
    }
}

/// The search context of one object scope's name assignment.
struct Names<'n> {
    members: &'n [Member],
    shape: &'n ObjectShape,
    tolerant: bool,
    exact: Vec<Option<usize>>,
    undecided: Vec<usize>,
    options: Vec<Vec<NameOption>>,
    resolver: &'n Resolver<'n>,
    raw: &'n RawCall,
    site: &'n Site,
}

impl Names<'_> {
    /// Which parameters an assignment binds.
    fn bound(&self, state: &Assignment<'_>) -> Vec<bool> {
        let mut bound = vec![false; self.shape.parameters.len()];
        for index in self.exact.iter().flatten() {
            bound[*index] = true;
        }
        for (position, &choice) in state.choices.iter().enumerate() {
            if let Action::Rename(index) = self.options[position][choice as usize].action {
                bound[index] = true;
            }
        }
        for &(_, _, index) in &state.moves {
            bound[index as usize] = true;
        }
        bound
    }

    /// The first required parameter an assignment leaves unbound.
    fn missing(&self, state: &Assignment<'_>) -> Option<usize> {
        let bound = self.bound(state);
        self.shape.required.iter().find_map(|name| {
            let index = self
                .shape
                .parameters
                .iter()
                .position(|parameter| &parameter.name == name)?;
            (!bound[index]).then_some(index)
        })
    }

    /// The action an assignment takes for member `position`, if it is
    /// undeclared and decided.
    fn action(&self, state: &Assignment<'_>, position: usize) -> Option<Action> {
        let slot = self
            .undecided
            .iter()
            .position(|&member| member == position)?;
        let choice = *state.choices.get(slot)?;
        Some(self.options[slot][choice as usize].action)
    }

    /// Why no assignment exists: the first undeclared member with no
    /// admissible option, else the first required parameter missing.
    fn failure(&self) -> UnresolvedReason {
        if let Some(slot) = self.options.iter().position(Vec::is_empty) {
            let member = &self.members[self.undecided[slot]];
            return UnresolvedReason::UnknownParameter {
                pointer: self.site.child(&member.key, &member.key).source,
            };
        }
        let index = self.missing(&Assignment::seed()).unwrap_or(0);
        let name = self
            .shape
            .parameters
            .get(index)
            .map_or("", |parameter| parameter.name.as_str());
        UnresolvedReason::MissingRequired {
            pointer: self.site.child(name, name).pointer,
        }
    }

    /// The search change of deciding member `position` by `option`.
    fn change(&self, position: usize, option: NameOption) -> Change {
        let member = &self.members[position];
        let (rule, explanation, original, guess, pointer) = match option.action {
            Action::Rename(index) => {
                let name = &self.shape.parameters[index].name;
                (
                    "json.key.match",
                    format!(
                        "parameter `{}` read as the declared parameter `{name}`",
                        member.key
                    ),
                    None,
                    Some(name.clone()),
                    self.site.child(name, &member.key).pointer,
                )
            }
            Action::Drop => (
                "tool_call.parameter.drop-unknown",
                format!(
                    "dropped parameter `{}`, which the tool does not declare",
                    member.key
                ),
                Some(format!("\"{}\":{}", member.key, member.value.to_json())),
                None,
                self.site.child(&member.key, &member.key).source,
            ),
            Action::Keep => (
                "tool_call.parameter.pass-through",
                format!(
                    "kept parameter `{}`, which the tool does not declare, unchanged",
                    member.key
                ),
                None,
                None,
                self.site.child(&member.key, &member.key).pointer,
            ),
        };
        Change {
            evidence: self.resolver.evidence(
                rule,
                explanation,
                member.key_span.clone(),
                destination(self.raw, &pointer),
            ),
            cost: option.cost,
            original,
            guess,
        }
    }

    /// The slots of an accepted assignment, in output order.
    fn slots(
        &self,
        state: &Assignment<'_>,
        governor: &mut Governor,
    ) -> Result<Vec<Slot>, StopReason> {
        let mut slots: Vec<Slot> = Vec::with_capacity(self.members.len() + state.moves.len());
        let mut members: Vec<Member> = self.members.to_vec();
        // The copied members, and per member its slot, occurrence and
        // source pointer.
        governor.charge(
            Resource::ScratchBytes,
            members
                .iter()
                .map(|member| {
                    MEMBER_BYTES
                        .saturating_add(SLOT_BYTES)
                        .saturating_add(OCCURRENCE_BYTES)
                        .saturating_add(size(member.key.len()))
                        .saturating_add(member.value.estimated_bytes())
                        .saturating_add(size(
                            2 * (self.site.source.len()
                                + self.site.pointer.len()
                                + 2
                                + 4 * member.key.len()),
                        ))
                })
                .sum(),
        )?;
        let mut moved: Vec<(usize, Occurrence)> = Vec::new();
        for &(member, child, index) in &state.moves {
            if let Value::Object(children) = &members[member as usize].value.value {
                let child_member = &children[child as usize];
                governor.charge(
                    Resource::ScratchBytes,
                    OCCURRENCE_BYTES
                        .saturating_add(SLOT_BYTES)
                        .saturating_add(size(child_member.key.len()))
                        .saturating_add(child_member.value.estimated_bytes()),
                )?;
                moved.push((
                    index as usize,
                    Occurrence {
                        key: child_member.key.clone(),
                        key_span: child_member.key_span.clone(),
                        value: child_member.value.clone(),
                        source: format!(
                            "{}/{}",
                            self.site
                                .child(&members[member as usize].key, &members[member as usize].key)
                                .source,
                            pointer_segment(&child_member.key)
                        ),
                    },
                ));
            }
        }
        // Remove moved children, highest index first.
        let mut removals: Vec<(u32, u32)> = state
            .moves
            .iter()
            .map(|&(member, child, _)| (member, child))
            .collect();
        removals.sort_unstable_by(|a, b| b.cmp(a));
        for (member, child) in removals {
            if let Value::Object(children) = &mut members[member as usize].value.value {
                children.remove(child as usize);
            }
        }
        let push = |slots: &mut Vec<Slot>, index: usize, occurrence: Occurrence| {
            if let Some(Slot::Parameter(_, occurrences)) = slots
                .iter_mut()
                .find(|slot| matches!(slot, Slot::Parameter(known, _) if *known == index))
            {
                occurrences.push(occurrence);
            } else {
                slots.push(Slot::Parameter(index, vec![occurrence]));
            }
        };
        for (position, member) in members.into_iter().enumerate() {
            let source = self.site.child(&member.key, &member.key).source;
            let index = match (self.exact[position], self.action(state, position)) {
                (Some(index), _) | (None, Some(Action::Rename(index))) => index,
                (None, Some(Action::Keep)) => {
                    slots.push(Slot::Kept(member));
                    continue;
                }
                (None, Some(Action::Drop) | None) => continue,
            };
            push(
                &mut slots,
                index,
                Occurrence {
                    key: member.key,
                    key_span: member.key_span,
                    value: member.value,
                    source,
                },
            );
        }
        for (index, occurrence) in moved {
            push(&mut slots, index, occurrence);
        }
        Ok(slots)
    }
}

impl<'n> SearchState for Assignment<'n> {
    type Tie = (Vec<u32>, Vec<(u32, u32, u32)>);
    type Context<'c> = Names<'n>;

    fn tie(&self) -> Self::Tie {
        (self.choices.clone(), self.moves.clone())
    }

    fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason> {
        for choice in &self.choices {
            key.extend(choice.to_be_bytes())?;
        }
        key.push(0xFF)?;
        for (member, child, parameter) in &self.moves {
            key.extend(member.to_be_bytes())?;
            key.extend(child.to_be_bytes())?;
            key.extend(parameter.to_be_bytes())?;
        }
        Ok(())
    }

    fn accept(
        &self,
        names: &mut Names<'n>,
        governor: &mut Governor,
    ) -> Result<Acceptance, StopReason> {
        governor.charge(Resource::Work, 1)?;
        Ok(
            if self.choices.len() == names.undecided.len() && names.missing(self).is_none() {
                Acceptance::Accepted { validation: 0 }
            } else {
                Acceptance::Rejected
            },
        )
    }

    fn expand(
        &self,
        names: &mut Names<'n>,
        proposals: &mut Proposals<'_, Self>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let next = self.choices.len();
        if next < names.undecided.len() {
            let bound = names.bound(self);
            for (index, option) in names.options[next].iter().enumerate() {
                if let Action::Rename(parameter) = option.action
                    && bound[parameter]
                {
                    continue;
                }
                let mut successor = self.clone();
                successor
                    .choices
                    .push(u32::try_from(index).unwrap_or(u32::MAX));
                let change = names.change(names.undecided[next], *option);
                proposals.commit(governor, successor, change)?;
            }
            return Ok(());
        }
        if !names.tolerant {
            return Ok(());
        }
        let Some(parameter) = names.missing(self) else {
            return Ok(());
        };
        let name = names.shape.parameters[parameter].name.clone();
        let rule = rules::resolution_rule("json.key.relocate");
        for (position, member) in names.members.iter().enumerate() {
            if names.exact[position].is_none()
                && !matches!(
                    names.action(self, position),
                    Some(Action::Keep | Action::Rename(_))
                )
            {
                continue;
            }
            let Value::Object(children) = &member.value.value else {
                continue;
            };
            for (child, entry) in children.iter().enumerate() {
                governor.charge(Resource::Work, 1)?;
                if names.shape.resolve_name(&entry.key) != Some(name.as_str()) {
                    continue;
                }
                let step = (
                    u32::try_from(position).unwrap_or(u32::MAX),
                    u32::try_from(child).unwrap_or(u32::MAX),
                    u32::try_from(parameter).unwrap_or(u32::MAX),
                );
                if self
                    .moves
                    .iter()
                    .any(|&(m, c, _)| (m, c) == (step.0, step.1))
                {
                    continue;
                }
                let mut successor = self.clone();
                successor.moves.push(step);
                let evidence = names.resolver.evidence(
                    rule.id,
                    format!(
                        "moved required parameter `{name}` out of `{}` to where it is required",
                        member.key
                    ),
                    entry.key_span.clone(),
                    destination(names.raw, &names.site.child(&name, &name).pointer),
                );
                proposals.commit(
                    governor,
                    successor,
                    Change {
                        evidence,
                        cost: rule.cost,
                        original: None,
                        guess: Some(name.clone()),
                    },
                )?;
            }
        }
        Ok(())
    }
}

/// The evidence destination of a call-level change, or of the argument at
/// `pointer`.
fn destination(raw: &RawCall, pointer: &str) -> String {
    if pointer.is_empty() {
        format!("/turn/{}..{}", raw.span.start, raw.span.end)
    } else {
        format!(
            "/turn/{}..{}/arguments{pointer}",
            raw.span.start, raw.span.end
        )
    }
}

/// Case-folded text with every whitespace run a single space and no
/// leading or trailing whitespace: the enumeration comparison form.
fn normalized(text: &str) -> String {
    crate::text::comparison_key(&text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Shift every span of `node` (parsed from text at `offset`) by `offset`.
/// One XML-text string inside an unmodeled value, with its JSON reading.
struct SchemaSite {
    /// The value's pointer in the arguments.
    pointer: String,
    /// The pointer of the unmodeled parameter holding it.
    root: String,
    /// The schema pointer of that parameter.
    schema: String,
    /// The verbatim text.
    text: String,
    /// The JSON value the text spells (not a string).
    reading: Node,
    /// The text's source range.
    raw: Range<usize>,
}

/// Collect every XML-text string inside the unmodeled values of an object
/// scope, walking the modeled scopes of `shape` down to them.
fn schema_sites(
    shape: &ObjectShape,
    node: &Node,
    pointer: &str,
    schema: &str,
    raw: &RawCall,
    out: &mut Vec<SchemaSite>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    governor.charge(Resource::Work, 1)?;
    let Value::Object(members) = &node.value else {
        return Ok(());
    };
    for member in members {
        let Some(parameter) = shape.parameter(&member.key) else {
            continue;
        };
        let here = format!("{pointer}/{}", pointer_segment(&member.key));
        let here_schema = format!("{schema}/properties/{}", pointer_segment(&member.key));
        let value_shape = match &parameter.value {
            ValueShape::Array(inner) => inner.as_ref(),
            other => other,
        };
        let mut visit = |node: &Node, at: &str, at_schema: &str| match value_shape {
            ValueShape::Unmodeled => {
                let root = (at, at_schema);
                unmodeled_sites(node, at, root, raw, out, governor)
            }
            ValueShape::Object(inner) => {
                schema_sites(inner, node, at, at_schema, raw, out, governor)
            }
            _ => Ok(()),
        };
        if matches!(parameter.value, ValueShape::Array(_)) || parameter.repeatable {
            if let Value::Array(items) = &member.value.value {
                let items_schema = format!("{here_schema}/items");
                for (index, element) in items.iter().enumerate() {
                    visit(element, &format!("{here}/{index}"), &items_schema)?;
                }
            }
        } else {
            visit(&member.value, &here, &here_schema)?;
        }
    }
    Ok(())
}

/// Collect every XML-text string inside an unmodeled value (`root` is the
/// value's pointer and schema pointer), with the JSON value it spells.
fn unmodeled_sites(
    node: &Node,
    pointer: &str,
    root: (&str, &str),
    raw: &RawCall,
    out: &mut Vec<SchemaSite>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    governor.charge(Resource::Work, 1)?;
    match &node.value {
        Value::String(text) => {
            let Some(leaf) = raw.arguments.leaves.iter().find(|leaf| {
                leaf.origin == ArgOrigin::XmlText && node.span.as_ref() == Some(&leaf.raw)
            }) else {
                return Ok(());
            };
            let mut reading = match crate::json::parse_with(text, governor) {
                Ok(reading) => reading,
                Err(crate::json::ParseFailure::Resource(reason)) => return Err(reason),
                Err(crate::json::ParseFailure::Syntax(_)) => return Ok(()),
            };
            if matches!(reading.value, Value::String(_)) {
                return Ok(());
            }
            rebase(&mut reading, leaf.raw.start);
            governor.charge(
                Resource::ScratchBytes,
                reading
                    .estimated_bytes()
                    .saturating_add(size(text.len() + pointer.len() + root.1.len())),
            )?;
            out.push(SchemaSite {
                pointer: pointer.to_owned(),
                root: root.0.to_owned(),
                schema: root.1.to_owned(),
                text: text.clone(),
                reading,
                raw: leaf.raw.clone(),
            });
        }
        Value::Object(members) => {
            for member in members {
                let here = format!("{pointer}/{}", pointer_segment(&member.key));
                unmodeled_sites(&member.value, &here, root, raw, out, governor)?;
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                unmodeled_sites(
                    item,
                    &format!("{pointer}/{index}"),
                    root,
                    raw,
                    out,
                    governor,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The schema-typing rounds of [`Resolver::schema_typed`] over `sites`:
/// the arguments with the accepted JSON readings, and per site whether its
/// JSON reading was taken and whether the schema admits both readings.
/// `None` when the validator cannot evaluate the schema.
#[allow(clippy::type_complexity)]
fn schema_rounds(
    tool: &CompiledTool,
    sites: &[SchemaSite],
    mut arguments: Node,
    governor: &mut Governor,
) -> Result<Option<(Node, Vec<bool>, Vec<bool>)>, StopReason> {
    // A schema the validator cannot evaluate (a `$ref` recurring without
    // instance progress) types nothing: the verbatim text stays, and the
    // arguments' own validation reports the schema as before.
    let validate = |instance: &Node, governor: &mut Governor| match tool
        .schema()
        .validate_with(instance, governor)
    {
        Ok(report) => Ok(Some(report)),
        Err(StopReason::InvalidInput(_)) => Ok(None),
        Err(reason) => Err(reason),
    };
    let Some(mut report) = validate(&arguments, governor)? else {
        return Ok(None);
    };
    let flip = |base: &Node, indices: &[usize], governor: &mut Governor| {
        let mut candidate = base.clone();
        governor.charge(Resource::ScratchBytes, candidate.estimated_bytes())?;
        for &index in indices {
            let site = &sites[index];
            replace_at(&mut candidate, &site.pointer, site.reading.clone());
        }
        Ok::<_, StopReason>(candidate)
    };
    // Rounds: flip every value the violations implicate at once and keep
    // the flips that reduced their own violations; when a round keeps
    // none (values interacting through a shared ancestor, an `anyOf`
    // over an object), try them one at a time. Every validation is
    // charged; exhaustion is the request's explicit `BudgetExceeded`.
    let mut accepted = vec![false; sites.len()];
    loop {
        let counts = related_counts(&report.violations, sites, governor)?;
        let implicated: Vec<usize> = (0..sites.len())
            .filter(|&index| !accepted[index] && counts[index] > 0)
            .collect();
        if implicated.is_empty() {
            break;
        }
        let candidate = flip(&arguments, &implicated, governor)?;
        let Some(tried) = validate(&candidate, governor)? else {
            break;
        };
        let after = related_counts(&tried.violations, sites, governor)?;
        let better: Vec<usize> = implicated
            .iter()
            .copied()
            .filter(|&index| after[index] < counts[index])
            .collect();
        if better.len() == implicated.len() {
            arguments = candidate;
            report = tried;
            for &index in &better {
                accepted[index] = true;
            }
            continue;
        }
        if !better.is_empty() {
            let next = flip(&arguments, &better, governor)?;
            if let Some(next_report) = validate(&next, governor)?
                && next_report.violations.len() < report.violations.len()
            {
                arguments = next;
                report = next_report;
                for &index in &better {
                    accepted[index] = true;
                }
                continue;
            }
        }
        let mut progressed = false;
        for &index in &implicated {
            let counts = related_counts(&report.violations, sites, governor)?;
            if counts[index] == 0 {
                continue;
            }
            let next = flip(&arguments, &[index], governor)?;
            let Some(next_report) = validate(&next, governor)? else {
                continue;
            };
            let after = related_counts(&next_report.violations, sites, governor)?;
            if after[index] < counts[index]
                && next_report.violations.len() < report.violations.len()
            {
                arguments = next;
                report = next_report;
                accepted[index] = true;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    // The values whose verbatim string validates: whether the schema
    // also admits their JSON reading (recorded as kept), in one batch.
    let counts = related_counts(&report.violations, sites, governor)?;
    let quiet: Vec<usize> = (0..sites.len())
        .filter(|&index| !accepted[index] && counts[index] == 0)
        .collect();
    let mut both = vec![false; sites.len()];
    if !quiet.is_empty() {
        let candidate = flip(&arguments, &quiet, governor)?;
        if let Some(tried) = validate(&candidate, governor)? {
            let after = related_counts(&tried.violations, sites, governor)?;
            for &index in &quiet {
                both[index] = after[index] == 0;
            }
        }
    }
    Ok(Some((arguments, accepted, both)))
}

/// Per site, how many of `violations` concern it: a violation at the
/// site's value or below it, or at an ancestor inside the site's unmodeled
/// parameter (an `anyOf` over the whole value). An index of the site
/// pointers makes it linear in the violations and the sites they reach.
fn related_counts(
    violations: &[crate::schema::Violation],
    sites: &[SchemaSite],
    governor: &mut Governor,
) -> Result<Vec<usize>, StopReason> {
    let index: BTreeMap<&str, usize> = sites
        .iter()
        .enumerate()
        .map(|(position, site)| (site.pointer.as_str(), position))
        .collect();
    let mut counts = vec![0; sites.len()];
    for violation in violations {
        governor.charge(Resource::Work, 1)?;
        let path = violation.instance_path.as_str();
        let mut prefix = path;
        loop {
            if let Some(&position) = index.get(prefix) {
                counts[position] += 1;
                break;
            }
            match prefix.rfind('/') {
                Some(cut) => prefix = &prefix[..cut],
                None => break,
            }
        }
        let below = format!("{path}/");
        for (pointer, &position) in index.range(below.as_str()..) {
            if !pointer.starts_with(&below) {
                break;
            }
            governor.charge(Resource::Work, 1)?;
            if path.starts_with(sites[position].root.as_str()) {
                counts[position] += 1;
            }
        }
    }
    Ok(counts)
}

/// Whether a scope holds an unmodeled parameter, at any depth: a walk of
/// the compiled shape (charged when the tool was compiled).
fn holds_unmodeled(shape: &ObjectShape) -> bool {
    fn value(shape: &ValueShape) -> bool {
        match shape {
            ValueShape::Unmodeled => true,
            ValueShape::Array(item) => value(item),
            ValueShape::Object(inner) => holds_unmodeled(inner),
            _ => false,
        }
    }
    shape
        .parameters
        .iter()
        .any(|parameter| value(&parameter.value))
}

/// Replace the value at `pointer` in `node`; whether it was found.
fn replace_at(node: &mut Node, pointer: &str, value: Node) -> bool {
    let mut current = node;
    for segment in pointer.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        current = match &mut current.value {
            Value::Object(members) => {
                match members.iter_mut().find(|member| member.key == segment) {
                    Some(member) => &mut member.value,
                    None => return false,
                }
            }
            Value::Array(items) => {
                match segment.parse::<usize>().ok().and_then(|i| items.get_mut(i)) {
                    Some(item) => item,
                    None => return false,
                }
            }
            _ => return false,
        };
    }
    *current = value;
    true
}

/// The JSON type name of a value.
const fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn rebase(node: &mut Node, offset: usize) {
    node.span = node
        .span
        .as_ref()
        .map(|span| span.start + offset..span.end + offset);
    match &mut node.value {
        Value::Array(items) => {
            for item in items {
                rebase(item, offset);
            }
        }
        Value::Object(members) => {
            for member in members {
                member.key_span = member
                    .key_span
                    .as_ref()
                    .map(|span| span.start + offset..span.end + offset);
                rebase(&mut member.value, offset);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Under exact parsing, block a reading that needs a repair: name the first
/// repair rule, drop every repair entry, and keep no repaired value.
fn exact(reading: &mut CallReading, governor: &mut Governor) -> Result<(), StopReason> {
    let is_repair = |id: &str| rules::rule(id).is_none_or(Rule::is_repair);
    let Some(rule) = reading
        .repairs
        .evidence
        .iter()
        .find(|evidence| is_repair(&evidence.rule))
        .map(|evidence| evidence.rule.clone())
    else {
        return Ok(());
    };
    governor.charge(Resource::Work, reading.repairs.entries().saturating_add(1))?;
    reading
        .repairs
        .evidence
        .retain(|evidence| !is_repair(&evidence.rule));
    reading
        .repairs
        .losses
        .retain(|loss| !is_repair(&loss.loss.evidence.rule));
    reading
        .repairs
        .inferences
        .retain(|inference| !is_repair(&inference.evidence.rule));
    reading.arguments = Node::new(Value::Null);
    reading.validations.clear();
    reading.blocker = Some(UnresolvedReason::RepairNotPermitted { rule });
    Ok(())
}

/// Extend each partial draft by each alternative of the next slot.
fn product(
    partial: Vec<Draft>,
    mut alternatives: Vec<(Member, Typed)>,
    governor: &mut Governor,
) -> Result<Vec<Draft>, StopReason> {
    let count = partial.len().saturating_mul(alternatives.len());
    governor.charge(
        Resource::ScratchBytes,
        size(count).saturating_mul(DRAFT_BYTES),
    )?;
    let mut next = Vec::with_capacity(count);
    let drafts = partial.len();
    for (index, draft) in partial.into_iter().enumerate() {
        // The last draft takes the alternatives; earlier ones copy them.
        let own = if index + 1 == drafts {
            core::mem::take(&mut alternatives)
        } else {
            copy_alternatives(&alternatives, governor)?
        };
        let choices = own.len();
        let mut draft = Some(draft);
        for (choice, (member, typed)) in own.into_iter().enumerate() {
            governor.charge(Resource::Work, 1)?;
            if !next.is_empty() {
                governor.charge(Resource::Candidates, 1)?;
            }
            // The last alternative takes the draft; earlier ones copy it.
            let mut combined = if choice + 1 == choices {
                draft.take().expect("the draft is taken once")
            } else {
                copy_draft(draft.as_ref().expect("the draft"), governor)?
            };
            combined.members.push(member);
            combined.ledger.extend(typed.ledger);
            combined.cost = combined.cost.saturating_add(typed.cost);
            combined.blocker = combined.blocker.or(typed.blocker);
            next.push(combined);
        }
    }
    Ok(next)
}

/// The scratch of an array reading extended by one item: a copy of `base`
/// (its value with room for the item, and its ledger) and of the item.
fn combined_bytes(base: &Typed, alternative: &Typed) -> u64 {
    let items = match &base.value.value {
        Value::Array(values) => size(values.len().saturating_add(1).saturating_mul(2)),
        _ => 0,
    };
    TYPED_BYTES
        .saturating_add(base.value.estimated_bytes())
        .saturating_add(
            items.saturating_mul(size(<Node as crate::contract::ScratchCharge>::CHARGE)),
        )
        .saturating_add(alternative.value.estimated_bytes())
        .saturating_add(
            base.ledger
                .entries()
                .saturating_add(alternative.ledger.entries())
                .saturating_mul(ENTRY_BYTES),
        )
}

/// A charged copy of `draft`, with room for its remaining members.
fn copy_draft(draft: &Draft, governor: &mut Governor) -> Result<Draft, StopReason> {
    governor.charge(
        Resource::ScratchBytes,
        size(draft.members.capacity())
            .saturating_mul(MEMBER_BYTES)
            .saturating_add(
                draft
                    .members
                    .iter()
                    .map(|member| {
                        size(member.key.len()).saturating_add(member.value.estimated_bytes())
                    })
                    .sum(),
            )
            .saturating_add(draft.ledger.entries().saturating_mul(ENTRY_BYTES)),
    )?;
    let mut members = Vec::with_capacity(draft.members.capacity());
    members.extend(draft.members.iter().cloned());
    Ok(Draft {
        members,
        ledger: draft.ledger.clone(),
        cost: draft.cost,
        blocker: draft.blocker.clone(),
    })
}

/// A charged copy of a slot's alternatives.
fn copy_alternatives(
    alternatives: &[(Member, Typed)],
    governor: &mut Governor,
) -> Result<Vec<(Member, Typed)>, StopReason> {
    governor.charge(
        Resource::ScratchBytes,
        alternatives
            .iter()
            .map(|(member, typed)| {
                MEMBER_BYTES
                    .saturating_add(TYPED_BYTES)
                    .saturating_add(size(member.key.len()))
                    .saturating_add(member.value.estimated_bytes())
                    .saturating_add(typed.ledger.entries().saturating_mul(ENTRY_BYTES))
            })
            .sum(),
    )?;
    Ok(alternatives.to_vec())
}

/// Two digests of a reading: its content (tool, arguments, blocker), the
/// identity of the call it denotes, and its full derivation (the content
/// plus every rule application), the ranking tie-break.
fn reading_digests(
    reading: &CallReading,
    governor: &mut Governor,
) -> Result<(Digest, Digest), StopReason> {
    // The arguments' JSON is the only transient copy: written into a
    // buffer sized (and leased) by its conservative estimate, so it is
    // allocated once; every other part streams into the digests.
    let estimate = reading.arguments.estimated_bytes();
    let lease = governor.lease_scratch(estimate)?;
    let mut json = String::with_capacity(usize::try_from(estimate).unwrap_or(usize::MAX));
    reading.arguments.write_json(&mut json);
    let blocker = reading
        .blocker
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    let mut builder = DigestBuilder::new();
    let mut encoded = 0_usize;
    let mut spans = 0_usize;
    let mut part = |builder: &mut DigestBuilder, bytes: &[u8]| {
        builder.update(&(bytes.len() as u64).to_be_bytes());
        builder.update(bytes);
        encoded += 8 + bytes.len();
    };
    part(
        &mut builder,
        reading.tool.as_deref().unwrap_or("").as_bytes(),
    );
    part(&mut builder, json.as_bytes());
    part(&mut builder, blocker.as_bytes());
    drop(json);
    drop(lease);
    let content = builder.clone().finish();
    for evidence in &reading.repairs.evidence {
        part(&mut builder, evidence.rule.as_bytes());
        part(&mut builder, evidence.explanation.as_bytes());
        if let Some(span) = &evidence.span {
            builder.update(&(span.bytes.start as u64).to_be_bytes());
            builder.update(&(span.bytes.end as u64).to_be_bytes());
            spans += 16;
        }
    }
    governor.charge(Resource::Work, size(encoded + spans))?;
    Ok((content, builder.finish()))
}

/// The provenance distinguishing two competing readings.
fn distinguishing(
    first: &CallReading,
    second: &CallReading,
    governor: &mut Governor,
) -> Result<Vec<ProvenanceNode>, StopReason> {
    let mut forest = Forest::new(governor);
    let mut derivations = Vec::with_capacity(2);
    for reading in [first, second] {
        let mut derivation = Derivation::new(reading_digests(reading, governor)?.1, reading.cost);
        for evidence in &reading.repairs.evidence {
            derivation.push(forest.evidence(governor, evidence.clone())?);
        }
        for loss in &reading.repairs.losses {
            derivation.push(forest.loss(governor, loss.loss.clone())?);
        }
        for inference in &reading.repairs.inferences {
            derivation.push(forest.inference(governor, inference.clone())?);
        }
        derivations.push(derivation);
    }
    let nodes = forest.distinguishing(&derivations[0], &derivations[1]);
    governor.charge(
        Resource::ScratchBytes,
        size(nodes.len()).saturating_mul(ENTRY_BYTES),
    )?;
    Ok(nodes.into_iter().cloned().collect())
}
