// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Live recovery intake and owned event publication.
//!
//! Live events follow the history shared by native continuations. Calls subject
//! to later prose normalization hold numeric call indexing. Finalization flushes
//! source facts from the common interpretation prefix of completed readings,
//! without replaying input. Completion events retain final certainty and typed
//! values, while native revisions retain their actual operator payloads.
//! EOF capture identities follow semantic selection and equivalent branch
//! folding, permitting shared source facts inside a differing call. Full live
//! semantic-prefix equivalence, semantic-search revisions, decoding queries
//! and stream cloning are not implemented yet.

use super::recover::RecoveryIntake;
use super::stream_capture::{EditFact, NativeTrace, Origins, copy_bytes};
use super::stream_finish::{
    FinalCursor, FinalItem, FinalView, call_limit, common_prefix, same_call, selected_fact,
};
use super::{
    CallOutcome, CallSegment, Closure, DialectId, DialectSet, GenerationStop, Recognizer,
    Rejection, Segment, ToolCall, ToolCalls, ToolRecoveryRequest, ToolSet, UnresolvedReason,
};
use crate::contract::{
    AssistResult, Governor, Resource, ScratchCharge, ScratchLease, ScratchString, ScratchVec,
    StopReason, invalid_input, push_scratch, reserve_failure, saturating_size as size,
};
use crate::grammar::CaptureKind;
use crate::grammar::cost::K_EVENT;
use crate::grammar::edit::{EditOperator, OPERATOR_COUNT, OPERATOR_TABLE, OPERATORS};
use crate::json::{Node, OwnedDocument, Value};
use core::fmt;
use core::ops::Range;

#[path = "stream_revision.rs"]
mod revision;
use revision::RevisionWitnesses;
#[path = "stream_hold.rs"]
mod hold;
use hold::CandidateCoverage;
const _: () =
    assert!(core::mem::size_of::<(CandidateCoverage, usize, Option<usize>, ScratchLease)>() <= 128);

/// Portable admission for constructing and handing off one event.
/// Four native slots also admit the collector insertion per event.
const EVENT_BYTES: u64 = 512;
const _: () = assert!(4 * core::mem::size_of::<StreamEvent>() as u64 <= EVENT_BYTES);

/// Byte length, then lexical byte ordering for the compile-time operator index.
const fn operator_id_precedes(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return left.len() < right.len();
    }
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut at = 0;
    while at < left.len() && at < right.len() {
        if left[at] != right[at] {
            return left[at] < right[at];
        }
        at += 1;
    }
    left.len() < right.len()
}

/// Stable ordinal ordering preserves the registry's first match for equal ids.
/// The index derives from the registry without duplicating its identifiers.
static OPERATOR_ID_ORDER: [usize; OPERATOR_COUNT] = {
    let mut order = [0; OPERATOR_COUNT];
    let mut next = 0;
    while next < OPERATOR_COUNT {
        let mut at = next;
        while at > 0
            && operator_id_precedes(OPERATOR_TABLE[next].id, OPERATOR_TABLE[order[at - 1]].id)
        {
            order[at] = order[at - 1];
            at -= 1;
        }
        order[at] = next;
        next += 1;
    }
    order
};

/// Admit each comparison before finding the first equal registry identifier.
fn revision_operator(
    id: &str,
    governor: &mut Governor,
) -> Result<Option<EditOperator>, StopReason> {
    const CONTROL_BYTES: u64 = 96;
    const _: () = assert!(
        core::mem::size_of::<(
            ScratchLease,
            &'static str,
            &'static crate::grammar::edit::OperatorRule,
            [usize; 4],
            u64,
            core::cmp::Ordering,
            bool,
        )>() as u64
            <= CONTROL_BYTES
    );
    let _controls = governor.lease_scratch(CONTROL_BYTES)?;
    let comparison = size(id.len()).saturating_add(1);
    let (mut low, mut high) = (0, OPERATOR_ID_ORDER.len());
    while low < high {
        let middle = low + (high - low) / 2;
        let candidate = OPERATORS[OPERATOR_ID_ORDER[middle]].id;
        governor.charge(Resource::Work, 1)?;
        let precedes = match candidate.len().cmp(&id.len()) {
            core::cmp::Ordering::Less => true,
            core::cmp::Ordering::Greater => false,
            core::cmp::Ordering::Equal => {
                governor.charge(Resource::Work, comparison)?;
                candidate < id
            }
        };
        if precedes {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    let Some(&ordinal) = OPERATOR_ID_ORDER.get(low) else {
        return Ok(None);
    };
    let operator = &OPERATORS[ordinal];
    governor.charge(Resource::Work, 1)?;
    if operator.id.len() != id.len() {
        return Ok(None);
    }
    governor.charge(Resource::Work, comparison)?;
    Ok((operator.id == id).then_some(operator.operator))
}

/// An owned notification whose source interpretation is already stable.
///
/// Events describe source facts; they do not grant permission to execute a tool.
/// Use the final result's outcomes, losses and inferences to assess certainty.
#[derive(Debug, PartialEq, Eq)]
pub enum StreamEvent {
    /// Newly settled content bytes, including runs after closed native frames
    /// whose earlier history has no unresolved normalization owner. Other
    /// content waits for agreement in final readings. Notifications do not overlap.
    ContentDelta {
        /// The exact range in the original output.
        span: Range<usize>,
        /// Original bytes, including invalid UTF-8 without replacement.
        /// Boundaries never split a valid UTF-8 scalar.
        raw: Vec<u8>,
    },
    /// Settled reasoning text, excluding the block's markers. These are raw
    /// bytes; encrypted redacted-thinking data remains opaque to consumers.
    ReasoningDelta {
        /// The original marker-free text range.
        span: Range<usize>,
        /// Exact source bytes, without unescaping or replacement.
        raw: Vec<u8>,
    },
    /// A call whose existence and source position are shared by every live
    /// continuation. This is a source fact, not an execution verdict.
    CallStarted {
        /// Position among the turn's calls.
        index: usize,
        /// The first source byte of the call.
        offset: usize,
        /// Native dialect readings of this call, in canonical order.
        dialect_candidates: Vec<DialectId>,
    },
    /// A canonical tool identity shared by the call's readings.
    CallName {
        /// Position among the turn's calls.
        index: usize,
        /// The canonical tool name. Live names require canonical agreement;
        /// EOF names follow the final result, including explicitly recorded
        /// guesses. This event makes no resolution or usability claim.
        tool: String,
    },
    /// Stable original bytes of an arguments capture. This does not assert
    /// schema validity, settled parameter values or permission to execute.
    ArgumentDelta {
        /// Position among the turn's calls.
        index: usize,
        /// Original captured bytes, including markup or invalid UTF-8.
        raw: Vec<u8>,
    },
    /// A parameter's final typed value in a resolved or best-guess call.
    /// The preceding `CallComplete` carries its certainty. No parameter event
    /// is emitted for an ambiguous or unresolved call, or duplicate member name.
    ParameterComplete {
        /// Position among the final result's calls.
        index: usize,
        /// The canonical top-level parameter name.
        name: String,
        /// The exact final typed value, including lexical numbers and spans.
        value: Node,
    },
    /// The call's final outcome, preserving guesses and explicit blockers.
    /// This does not override the final result's tool-choice usability verdict.
    CallComplete {
        /// Position among the final result's calls.
        index: usize,
        /// The unchanged final per-call outcome.
        outcome: CallOutcome,
    },
    /// A rejected attempt, identified by its final source span.
    CallRejected {
        /// A previously started identity, or `None` when no `CallStarted` was
        /// emitted for this attempt. Rejected-list positions are never used as
        /// call indices. The current EOF producer reports unstarted attempts.
        index: Option<usize>,
        /// The complete final rejected span, including owned closing markup.
        span: Range<usize>,
        /// The final precise rejection reason.
        reason: Rejection,
    },
    /// An authentic grammar edit retained from native recognition and present
    /// in the selected final ledger. No replacement is inferred from prose.
    Revision {
        /// Position among the final result's calls.
        index: usize,
        /// Original argument-leaf JSON Pointer when the containing leaves agree,
        /// otherwise `None` for markup or an uncertain parameter owner.
        parameter: Option<String>,
        /// The grammar operator's stable rule id.
        rule: &'static str,
        /// Original source range affected by the edit.
        span: Range<usize>,
        /// Exact original bytes stored by the edit operator.
        before: Vec<u8>,
        /// Exact replacement bytes stored by the edit operator.
        after: Vec<u8>,
        /// Whether that operator records a guess inference in the receipt.
        guess: bool,
    },
}

/// An event and every producer reservation needed through sink handoff.
/// The event drops before its reservations if publication never occurs.
pub(super) struct PreparedEvent {
    event: StreamEvent,
    storage: ScratchLease,
}

impl PreparedEvent {
    /// Caller sinks and collectors own their memory from inception. Release
    /// producer storage after handoff, including a refused collector insertion.
    pub(super) fn publish(
        self,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        let Self { event, storage } = self;
        let result = sink(event);
        drop(storage);
        result
    }
}

impl StreamEvent {
    fn admit(governor: &mut Governor) -> Result<ScratchLease, StopReason> {
        governor.charge(Resource::Work, 4)?;
        governor.lease_scratch(EVENT_BYTES)
    }

    /// Prepare identical ownership for batch discard and caller handoff.
    pub(super) fn content(
        span: Range<usize>,
        raw: ScratchVec<u8>,
        governor: &mut Governor,
    ) -> Result<PreparedEvent, StopReason> {
        // Amortized collector insertion and relocation work is admitted per
        // event on every path, including batch and externally supplied sinks.
        let mut storage = Self::admit(governor)?;
        let (raw, owned) = raw.into_owned_parts();
        storage.absorb(owned);
        Ok(PreparedEvent {
            event: Self::ContentDelta { span, raw },
            storage,
        })
    }

    fn bytes(
        span: Range<usize>,
        source: &[u8],
        governor: &mut Governor,
    ) -> Result<ScratchVec<u8>, StopReason> {
        let bytes = source
            .get(span)
            .ok_or_else(|| invalid_input("stream event exceeds admitted source"))?;
        governor.charge(Resource::Work, size(bytes.len()).saturating_add(1))?;
        let mut raw = ScratchVec::new(governor);
        raw.reserve(bytes.len())?;
        raw.extend(bytes.iter().copied())?;
        Ok(raw)
    }

    fn reasoning(
        span: Range<usize>,
        source: &[u8],
        governor: &mut Governor,
    ) -> Result<PreparedEvent, StopReason> {
        let raw = Self::bytes(span.clone(), source, governor)?;
        let mut storage = Self::admit(governor)?;
        let (raw, owned) = raw.into_owned_parts();
        storage.absorb(owned);
        Ok(PreparedEvent {
            event: Self::ReasoningDelta { span, raw },
            storage,
        })
    }

    fn started(
        index: usize,
        offset: usize,
        candidates: impl IntoIterator<Item = DialectId>,
        governor: &mut Governor,
    ) -> Result<PreparedEvent, StopReason> {
        let mut dialects = ScratchVec::new(governor);
        let mut longest = 0;
        for dialect in candidates {
            longest = longest.max(dialect.id().len());
            governor.charge(
                Resource::Work,
                size(dialects.len())
                    .saturating_add(1)
                    .saturating_mul(size(dialect.id().len()).saturating_add(64)),
            )?;
            if !dialects.iter().any(|known| known == &dialect) {
                push_scratch(&mut dialects, dialect, governor)?;
            }
        }
        let levels = dialects.len().bit_width() + 1;
        governor.charge(
            Resource::Work,
            size(dialects.len())
                .saturating_mul(u64::from(levels))
                .saturating_mul(size(longest).saturating_add(64)),
        )?;
        dialects.as_mut_slice().sort_unstable();
        let mut storage = Self::admit(governor)?;
        let (dialect_candidates, owned) = dialects.into_owned_parts();
        storage.absorb(owned);
        Ok(PreparedEvent {
            event: Self::CallStarted {
                index,
                offset,
                dialect_candidates,
            },
            storage,
        })
    }

    fn name(
        index: usize,
        tool: &str,
        governor: &mut Governor,
    ) -> Result<PreparedEvent, StopReason> {
        let name = ScratchString::copy_from(tool, governor)?;
        let mut storage = Self::admit(governor)?;
        let (tool, owned) = name.into_owned_parts();
        storage.absorb(owned);
        Ok(PreparedEvent {
            event: Self::CallName { index, tool },
            storage,
        })
    }

    fn argument(
        index: usize,
        span: Range<usize>,
        source: &[u8],
        governor: &mut Governor,
    ) -> Result<PreparedEvent, StopReason> {
        let raw = Self::bytes(span, source, governor)?;
        let mut storage = Self::admit(governor)?;
        let (raw, owned) = raw.into_owned_parts();
        storage.absorb(owned);
        Ok(PreparedEvent {
            event: Self::ArgumentDelta { index, raw },
            storage,
        })
    }

    fn completed(
        index: usize,
        outcome: &CallOutcome,
        governor: &mut Governor,
    ) -> Result<PreparedEvent, StopReason> {
        let copied = copy_outcome(outcome, governor)?;
        let mut storage = Self::admit(governor)?;
        let OwnedOutcome {
            value: outcome,
            lease,
        } = copied;
        storage.absorb(lease);
        Ok(PreparedEvent {
            event: Self::CallComplete { index, outcome },
            storage,
        })
    }

    fn parameter(
        index: usize,
        name: &str,
        value: &Node,
        governor: &mut Governor,
    ) -> Result<PreparedEvent, StopReason> {
        let name = ScratchString::copy_from(name, governor)?;
        let value = OwnedDocument::copy_from(value, governor)?;
        let mut storage = Self::admit(governor)?;
        let (name, owned) = name.into_owned_parts();
        storage.absorb(owned);
        let (value, owned) = value.into_owned_parts();
        storage.absorb(owned);
        Ok(PreparedEvent {
            event: Self::ParameterComplete { index, name, value },
            storage,
        })
    }

    fn rejected(
        span: Range<usize>,
        reason: Rejection,
        governor: &mut Governor,
    ) -> Result<PreparedEvent, StopReason> {
        let storage = Self::admit(governor)?;
        Ok(PreparedEvent {
            event: Self::CallRejected {
                index: None,
                span,
                reason,
            },
            storage,
        })
    }

    fn revision(
        index: usize,
        edit: &EditFact,
        parameter: Option<&str>,
        governor: &mut Governor,
    ) -> Result<PreparedEvent, StopReason> {
        let before = copy_bytes(&edit.before, governor)?;
        let after = copy_bytes(&edit.after, governor)?;
        let parameter = parameter
            .map(|pointer| ScratchString::copy_from(pointer, governor))
            .transpose()?;
        let mut storage = Self::admit(governor)?;
        let (before, owned) = before.into_owned_parts();
        storage.absorb(owned);
        let (after, owned) = after.into_owned_parts();
        storage.absorb(owned);
        let parameter = parameter.map(|pointer| {
            let (pointer, owned) = pointer.into_owned_parts();
            storage.absorb(owned);
            pointer
        });
        Ok(PreparedEvent {
            event: Self::Revision {
                index,
                parameter,
                rule: edit.operator.rule().id,
                span: edit.at..edit.end,
                before,
                after,
                guess: edit.operator.rule().guess,
            },
            storage,
        })
    }
}

/// Blocker text remains transient until the whole event has been admitted.
/// Field order drops the copied value before refunding its storage lease.
struct OwnedOutcome {
    value: CallOutcome,
    lease: ScratchLease,
}

fn copy_outcome(
    outcome: &CallOutcome,
    governor: &mut Governor,
) -> Result<OwnedOutcome, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let mut lease = ScratchLease::empty(governor);
    let value = match outcome {
        CallOutcome::Resolved => CallOutcome::Resolved,
        CallOutcome::BestGuess => CallOutcome::BestGuess,
        CallOutcome::Ambiguous => CallOutcome::Ambiguous,
        CallOutcome::Unresolved { reason } => CallOutcome::Unresolved {
            reason: copy_reason(reason, governor, &mut lease)?,
        },
    };
    Ok(OwnedOutcome { value, lease })
}

fn copy_reason(
    reason: &UnresolvedReason,
    governor: &mut Governor,
    lease: &mut ScratchLease,
) -> Result<UnresolvedReason, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let mut text = |value: &str, governor: &mut Governor| {
        let (copied, storage) = ScratchString::copy_from(value, governor)?.into_owned_parts();
        lease.absorb(storage);
        Ok::<_, StopReason>(copied)
    };
    Ok(match reason {
        UnresolvedReason::MissingName => UnresolvedReason::MissingName,
        UnresolvedReason::UnknownTool { name } => UnresolvedReason::UnknownTool {
            name: text(name, governor)?,
        },
        UnresolvedReason::RuleNotAdmitted { rule, policy } => UnresolvedReason::RuleNotAdmitted {
            rule: text(rule, governor)?,
            policy: *policy,
        },
        UnresolvedReason::InvalidArgument { pointer, fault } => UnresolvedReason::InvalidArgument {
            pointer: text(pointer, governor)?,
            fault: *fault,
        },
        UnresolvedReason::ArgumentsNotObject => UnresolvedReason::ArgumentsNotObject,
        UnresolvedReason::UnknownParameter { pointer } => UnresolvedReason::UnknownParameter {
            pointer: text(pointer, governor)?,
        },
        UnresolvedReason::MissingRequired { pointer } => UnresolvedReason::MissingRequired {
            pointer: text(pointer, governor)?,
        },
        UnresolvedReason::SchemaValidation => UnresolvedReason::SchemaValidation,
        UnresolvedReason::Malformed { detail } => UnresolvedReason::Malformed {
            detail: text(detail, governor)?,
        },
        UnresolvedReason::Truncated => UnresolvedReason::Truncated,
        UnresolvedReason::ToolChoiceNone => UnresolvedReason::ToolChoiceNone,
        UnresolvedReason::ToolChoiceForced { expected } => UnresolvedReason::ToolChoiceForced {
            expected: text(expected, governor)?,
        },
        UnresolvedReason::ToolChoiceCardinality => UnresolvedReason::ToolChoiceCardinality,
        UnresolvedReason::RepairNotPermitted { rule } => UnresolvedReason::RepairNotPermitted {
            rule: text(rule, governor)?,
        },
    })
}

/// Metadata only: payload ownership has already transferred to the caller.
#[derive(Debug)]
struct LiveCall {
    start: usize,
    end: usize,
    named: bool,
    arguments: bool,
}

impl ScratchCharge for LiveCall {
    const CHARGE: usize = 24;
}
const _: () = assert!(core::mem::size_of::<LiveCall>() <= LiveCall::CHARGE);

/// An earlier segment whose content normalization still needs the final turn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NormalizationOwner {
    Rejection,
    Reasoning,
    SuppressibleFrame,
    UnsettledFrame,
}

impl NormalizationOwner {
    fn of(segment: &Segment, governor: &mut Governor) -> Result<Option<Self>, StopReason> {
        Ok(match segment {
            Segment::Call(call) | Segment::Result(call) => {
                governor.charge(Resource::Work, size(call.readings.len()).saturating_mul(2))?;
                if call.readings.iter().any(|raw| {
                    raw.dialect.grammar().examples_content()
                        || (raw.bare && raw.dialect.grammar().declaration_content())
                }) {
                    Some(Self::SuppressibleFrame)
                } else if call
                    .readings
                    .iter()
                    .any(|raw| raw.closure != Closure::Closed)
                {
                    Some(Self::UnsettledFrame)
                } else {
                    None
                }
            }
            Segment::Rejected { .. } => Some(Self::Rejection),
            Segment::Reasoning(_) => Some(Self::Reasoning),
            Segment::Content(_) => None,
        })
    }
}

/// Complete inputs of the last successful native event traversal.
#[derive(Clone, Copy, PartialEq, Eq)]
struct AdvanceKey {
    history: u32,
    end: usize,
    content: Option<(usize, usize)>,
}

/// One monotone traversal of native shared history plus EOF capture facts.
/// Nothing here resolves a prefix, replays source or snapshots a governor.
pub(super) struct EventCursor {
    history: u32,
    pub(super) visited_end: usize,
    normalization_from: Option<usize>,
    coverage: CandidateCoverage,
    ownership_metadata: ScratchLease,
    /// A successful traversal depends on immutable native ancestry and the
    /// normalized end. Reasoning embedding can change, but its text and the
    /// stored call readings inspected here do not. Include end so a pending
    /// step beyond the previous cutoff is reconsidered when it advances.
    advance_key: Option<AdvanceKey>,
    /// Earlier content is fully published before this source watermark moves.
    content_end: usize,
    /// Preserve the first outstanding owner and its source origin. Rejection
    /// extension, reasoning-adjacent closers and frame suppression still use
    /// final normalization; none of their later content is published early.
    normalization_owner: Option<(usize, NormalizationOwner)>,
    indexed: bool,
    reasoning_end: usize,
    calls: ScratchVec<LiveCall>,
}

impl EventCursor {
    pub(super) fn prepare_ownership(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        self.ownership_metadata = governor.lease_scratch(128)?;
        Ok(())
    }
    pub(super) const fn history_checkpoint(&self) -> u32 {
        self.history
    }

    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            history: 0,
            visited_end: 0,
            normalization_from: None,
            coverage: CandidateCoverage::new(governor),
            ownership_metadata: ScratchLease::empty(governor),
            advance_key: None,
            content_end: 0,
            normalization_owner: None,
            indexed: true,
            reasoning_end: 0,
            calls: ScratchVec::new(governor),
        }
    }

    pub(super) fn advance(
        &mut self,
        recognizer: &Recognizer<'_>,
        source: &[u8],
        tools: &ToolSet,
        frontier: super::recognizer::ContinuationFrontier,
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        let end = frontier.prefix_end;
        let key = AdvanceKey {
            history: recognizer.settled_history_id(),
            end,
            content: frontier.content,
        };
        governor.charge(Resource::Work, 1)?;
        if self.advance_key == Some(key) {
            return Ok(());
        }
        while let Some(step) = recognizer.settled_step(self.history, governor)? {
            if step.end > end {
                break;
            }
            if step.end < self.visited_end || step.end > source.len() {
                return Err(invalid_input(
                    "native ownership checkpoint is out of source order",
                ));
            }
            if let Some(span) = step.content {
                self.live_content(&span, source, governor, sink)?;
            }
            if let Some(segment) = step.segment {
                self.segment(segment, source, tools, governor, sink)?;
            }
            self.history = step.history;
            self.visited_end = step.end;
        }
        if let Some((start, end)) = frontier.content {
            self.live_content(&(start..end), source, governor, sink)?;
        }
        self.advance_key = Some(key);
        Ok(())
    }

    fn segment(
        &mut self,
        segment: &Segment,
        source: &[u8],
        tools: &ToolSet,
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        match segment {
            Segment::Call(call) => self.coverage.append(&call.span, source.len(), governor)?,
            Segment::Rejected { span, .. } => self.coverage.append(span, source.len(), governor)?,
            Segment::Reasoning(block) => {
                for call in &block.embedded {
                    governor.charge(Resource::Work, 1)?;
                    if call.span.start < block.span.start || call.span.end > block.span.end {
                        return Err(invalid_input(
                            "embedded candidate exceeds its reasoning block",
                        ));
                    }
                    self.coverage.append(&call.span, source.len(), governor)?;
                }
            }
            Segment::Content(_) | Segment::Result(_) => {}
        }
        let owner = NormalizationOwner::of(segment, governor)?;
        if let Some(owner) = owner
            && self.normalization_owner.is_none()
        {
            self.normalization_owner = Some((segment.span().start, owner));
        }
        match segment {
            Segment::Reasoning(block) => {
                if !block.text.is_empty() {
                    StreamEvent::reasoning(block.text.clone(), source, governor)?.publish(sink)?;
                }
                self.reasoning_end = block.text.end;
            }
            Segment::Call(call) if self.indexed => {
                if owner == Some(NormalizationOwner::SuppressibleFrame) {
                    // Final context/schema normalization may turn this
                    // call into content, changing every later call index.
                    self.indexed = false;
                } else {
                    let index = self.calls.len();
                    StreamEvent::started(
                        index,
                        call.span.start,
                        call.readings.iter().map(|raw| raw.dialect.clone()),
                        governor,
                    )?
                    .publish(sink)?;
                    let name = unique_name(call, tools, governor)?;
                    if let Some(name) = name {
                        StreamEvent::name(index, name, governor)?.publish(sink)?;
                    }
                    let arguments = argument_span(call, governor)?;
                    if let Some(span) = &arguments {
                        StreamEvent::argument(index, span.clone(), source, governor)?
                            .publish(sink)?;
                    }
                    push_scratch(
                        &mut self.calls,
                        LiveCall {
                            start: call.span.start,
                            end: call.span.end,
                            named: name.is_some(),
                            arguments: arguments.is_some(),
                        },
                        governor,
                    )?;
                }
            }
            Segment::Result(result) => {
                // Final recovery maps the exact result span to content.
                // Publish it before any following content watermark moves.
                self.live_content(&result.span, source, governor, sink)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The same source-order publisher handles initial, historical, trailing
    /// and final content. A live range is admitted only while no earlier
    /// segment can still remove content or insert unreported content before it.
    fn live_content(
        &mut self,
        span: &Range<usize>,
        source: &[u8],
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        if self.normalization_owner.is_none() {
            self.content(span, source, governor, sink)?;
        } else {
            let start = span.start.max(self.content_end);
            if start < span.end {
                if span.end > source.len() {
                    return Err(invalid_input("normalization ownership exceeds source"));
                }
                self.normalization_from =
                    Some(self.normalization_from.map_or(start, |old| old.min(start)));
            }
        }
        Ok(())
    }

    pub(super) fn ownership_counts(
        &mut self,
        n: usize,
        native: Option<usize>,
        normalized: usize,
        governor: &mut Governor,
    ) -> Result<(usize, usize), StopReason> {
        let candidate = native
            .into_iter()
            .chain(self.normalization_from)
            .min()
            .unwrap_or(n);
        self.coverage.counts(n, candidate, normalized, governor)
    }

    fn content(
        &mut self,
        span: &Range<usize>,
        source: &[u8],
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        let start = span.start.max(self.content_end);
        if start < span.end {
            governor.charge(Resource::Work, K_EVENT)?;
            let end = span.end;
            let span = start..end;
            let raw = StreamEvent::bytes(span.clone(), source, governor)?;
            StreamEvent::content(span, raw, governor)?.publish(sink)?;
            self.content_end = end;
        }
        Ok(())
    }

    /// Flush the source-ordered interpretation prefix shared by final readings.
    /// Planning visits overlapping owners before any EOF event is published.
    pub(super) fn finish(
        &mut self,
        readings: &[FinalView<'_>],
        native: &NativeTrace,
        source: &[u8],
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        let limit = common_prefix(readings, native, source.len(), governor)?;
        let limit = super::stream_finish::source_boundary(source, limit, governor)?;
        let first = readings
            .first()
            .ok_or_else(|| invalid_input("completed recovery has no final reading"))?;
        let publication = FinalPublication {
            readings,
            native,
            source,
            limit,
        };
        let mut cursor = FinalCursor::new(*first);
        while let Some(item) = cursor.next(governor)? {
            let span = item.span();
            if span.start > span.end || span.end > source.len() {
                return Err(invalid_input("final event owner exceeds admitted source"));
            }
            if span.start > limit {
                break;
            }
            match item {
                FinalItem::Content(range) => {
                    self.content(&(range.start..range.end.min(limit)), source, governor, sink)?;
                }
                FinalItem::Reasoning(block) => {
                    let start = block.text.start.max(self.reasoning_end);
                    let end = block.text.end.min(limit);
                    if start < end {
                        StreamEvent::reasoning(start..end, source, governor)?.publish(sink)?;
                        self.reasoning_end = end;
                    }
                }
                FinalItem::Call(call, origins) if publication.shared_start(call, governor)? => {
                    self.finish_call(call, origins, &publication, governor, sink)?;
                }
                FinalItem::Rejected(rejected) if rejected.span.end <= limit => {
                    StreamEvent::rejected(rejected.span.clone(), rejected.reason, governor)?
                        .publish(sink)?;
                }
                FinalItem::Call(..) | FinalItem::Rejected(_) => {}
            }
        }
        governor.check()
    }

    fn finish_call(
        &self,
        call: &ToolCall,
        origins: &Origins,
        publication: &FinalPublication<'_>,
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        let index = call.index;
        governor.charge(Resource::Work, 1)?;
        let live = self.calls.as_slice().get(index);
        if let Some(live) = live {
            if live.start != call.span.start || live.end != call.span.end {
                return Err(invalid_input(
                    "final call differs from published native call",
                ));
            }
        } else {
            StreamEvent::started(
                index,
                call.span.start,
                publication
                    .readings
                    .iter()
                    .flat_map(|row| row.calls.calls[index].candidate_dialects.iter().cloned()),
                governor,
            )?
            .publish(sink)?;
        }
        let first = origins.selected(governor)?;
        let header = publication
            .native
            .fact(publication.native.call(first)?.header)?;
        if live.is_none_or(|live| !live.named)
            && header.name_end() <= publication.limit
            && let Some(tool) = &call.tool
        {
            StreamEvent::name(index, tool, governor)?.publish(sink)?;
        }
        if live.is_none_or(|live| !live.arguments) {
            let fact = selected_fact(origins, publication.native, governor)?;
            for &(start, end) in &fact.arguments {
                let end = end.min(publication.limit);
                if start < end {
                    StreamEvent::argument(index, start..end, publication.source, governor)?
                        .publish(sink)?;
                }
            }
        }
        Self::revisions(index, origins, publication, governor, sink)?;
        if publication.complete(call, origins, governor)? {
            StreamEvent::completed(index, &call.outcome, governor)?.publish(sink)?;
            if matches!(call.outcome, CallOutcome::Resolved | CallOutcome::BestGuess) {
                parameters(index, &call.arguments, governor, sink)?;
            }
        }
        Ok(())
    }

    fn revisions(
        index: usize,
        origins: &Origins,
        publication: &FinalPublication<'_>,
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        let fact = selected_fact(origins, publication.native, governor)?;
        let mut witnesses = None;
        for (position, edit) in fact.edits.iter().enumerate() {
            if edit.end > publication.limit {
                break;
            }
            let Some(agreement) = publication.revision(index, position, edit, governor)? else {
                break;
            };
            if witnesses.is_none() {
                witnesses = Some(RevisionWitnesses::build(publication, index, governor)?);
            }
            // The only None branch above either prepares Some or returns.
            if !witnesses
                .as_ref()
                .expect("a shared revision prepared its ledger witnesses")
                .contains_all(edit, governor)?
            {
                break;
            }
            if edit.before.is_empty() && edit.after.is_empty() {
                continue;
            }
            StreamEvent::revision(index, edit, agreement.parameter, governor)?.publish(sink)?;
        }
        Ok(())
    }
}

struct FinalPublication<'a> {
    readings: &'a [FinalView<'a>],
    native: &'a NativeTrace,
    source: &'a [u8],
    limit: usize,
}

struct RevisionAgreement<'a> {
    parameter: Option<&'a str>,
}

impl FinalPublication<'_> {
    /// Edits form an ordered common prefix. The caller stops at its first
    /// native disagreement, including conflicting insertions at the cutoff
    /// itself, before admitting the shared ledger witness index.
    fn revision<'a>(
        &self,
        index: usize,
        position: usize,
        edit: &'a EditFact,
        governor: &mut Governor,
    ) -> Result<Option<RevisionAgreement<'a>>, StopReason> {
        let mut parameter = edit.parameter.as_ref().map(ScratchString::as_str);
        for view in self.readings {
            governor.charge(Resource::Work, 1)?;
            if view.calls.calls.get(index).is_none() {
                return Err(invalid_input("revision has no final call counterpart"));
            }
            let origins = view
                .origins
                .calls
                .get(index)
                .ok_or_else(|| invalid_input("revision has no final native correlation"))?;
            for origin in &origins.0 {
                governor.charge(Resource::Work, 4)?;
                let row = self.native.call(origin.id)?;
                if row.competing == 0 || row.competing > row.readings.len() {
                    return Err(invalid_input(
                        "revision native competition exceeds its readings",
                    ));
                }
                if !origin.competing {
                    continue;
                }
                let competing = row.readings.get(..row.competing).ok_or_else(|| {
                    invalid_input("revision native competition exceeds its readings")
                })?;
                for &origin in competing {
                    governor.charge(Resource::Work, 4)?;
                    let fact = self.native.fact(origin)?;
                    let Some(other) = fact.edits.get(position) else {
                        return Ok(None);
                    };
                    if edit.operator != other.operator
                        || edit.at != other.at
                        || edit.end != other.end
                        || !edit.same_payload(other, governor)?
                    {
                        return Ok(None);
                    }
                    governor.charge(
                        Resource::Work,
                        size(parameter.map_or(0, str::len)).saturating_add(size(
                            other.parameter.as_ref().map_or(0, ScratchString::len),
                        )),
                    )?;
                    if parameter != other.parameter.as_ref().map(ScratchString::as_str) {
                        parameter = None;
                    }
                }
            }
        }
        Ok(Some(RevisionAgreement { parameter }))
    }

    fn shared_start(&self, call: &ToolCall, governor: &mut Governor) -> Result<bool, StopReason> {
        for row in self.readings {
            governor.charge(Resource::Work, 4)?;
            if row.calls.calls.get(call.index).is_none_or(|other| {
                other.index != call.index || other.span.start != call.span.start
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn complete(
        &self,
        call: &ToolCall,
        origins: &Origins,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        if call.span.end > self.limit || call_limit(call, origins, self.native, governor)?.is_some()
        {
            return Ok(false);
        }
        for row in self.readings.iter().skip(1) {
            let other = row
                .calls
                .calls
                .get(call.index)
                .ok_or_else(|| invalid_input("shared call has no final counterpart"))?;
            if !same_call(call, other, governor)?
                || call_limit(other, &row.origins.calls[call.index], self.native, governor)?
                    .is_some()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

fn parameters(
    index: usize,
    arguments: &Node,
    governor: &mut Governor,
    sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
) -> Result<(), StopReason> {
    let Value::Object(members) = &arguments.value else {
        return Ok(());
    };
    let mut ordered = ScratchVec::new(governor);
    let mut longest = 0;
    for (position, member) in members.iter().enumerate() {
        longest = longest.max(member.key.len());
        push_scratch(&mut ordered, (position, member.key.as_str()), governor)?;
    }
    let levels = ordered.len().bit_width() + 1;
    governor.charge(
        Resource::Work,
        size(ordered.len())
            .saturating_mul(u64::from(levels))
            .saturating_mul(size(longest).saturating_add(1)),
    )?;
    ordered
        .as_mut_slice()
        .sort_unstable_by_key(|(_, left)| *left);
    for (at, &(position, name)) in ordered.iter().enumerate() {
        governor.charge(
            Resource::Work,
            size(name.len()).saturating_add(1).saturating_mul(2),
        )?;
        if at
            .checked_sub(1)
            .is_some_and(|previous| ordered[previous].1 == name)
            || ordered.get(at + 1).is_some_and(|next| next.1 == name)
        {
            continue;
        }
        StreamEvent::parameter(index, name, &members[position].value, governor)?.publish(sink)?;
    }
    Ok(())
}

fn argument_span(
    call: &CallSegment,
    governor: &mut Governor,
) -> Result<Option<Range<usize>>, StopReason> {
    let mut common = None;
    for (index, raw) in call.readings.iter().enumerate() {
        governor.charge(Resource::Work, size(raw.captures.len()).saturating_add(1))?;
        let mut captures = raw
            .captures
            .iter()
            .filter(|capture| capture.kind == CaptureKind::Arguments);
        let span = captures.next().map(|capture| capture.span.clone());
        if captures.next().is_some() || (index != 0 && common != span) {
            return Ok(None);
        }
        common = span;
    }
    Ok(common)
}

fn unique_name<'t>(
    call: &CallSegment,
    tools: &'t ToolSet,
    governor: &mut Governor,
) -> Result<Option<&'t str>, StopReason> {
    let mut common = None;
    let levels = tools.name_index_len().bit_width() + 1;
    for raw in &call.readings {
        let Some(name) = &raw.name else {
            return Ok(None);
        };
        governor.charge(
            Resource::Work,
            size(name.value.len())
                .saturating_add(1)
                .saturating_mul(u64::from(levels)),
        )?;
        let Some(tool) = tools.lookup_exact(&name.value) else {
            return Ok(None);
        };
        // An explicit alias may be shorter than the canonical name compared
        // for agreement, so admit that comparison independently of lookup.
        governor.charge(Resource::Work, size(tool.name().len()).saturating_add(1))?;
        if common.is_some_and(|known| known != tool.name()) {
            return Ok(None);
        }
        common = Some(tool.name());
    }
    Ok(common)
}

/// A live tool-call recovery operation borrowing its tools and dialects.
///
/// It owns the request and the original governor. Every feed is a byte fold
/// through the same intake as batch recovery, including event allocation and
/// publication. Exhaustion and cancellation are reported by the final receipt;
/// bytes supplied after refusal still contribute to its complete source digest.
///
/// See [`StreamEvent`] for source facts and EOF completion events. Final
/// readings publish their shared source prefix, including correlated names and
/// raw argument captures inside a differing call. Full live semantic-prefix
/// equivalence and semantic-search revisions remain unfinished.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{GenerationStop, ToolSet, ToolRecoveryRequest}};
/// let tools = ToolSet::compile([], Budget::default()).unwrap();
/// let mut stream = tools.stream(ToolRecoveryRequest::default());
/// let events = stream.feed_collect(b"Some plain content.");
/// let result = stream.finish(GenerationStop::Stop);
/// assert_eq!(result.receipt.sources[0].bytes.as_slice(), b"Some plain content.");
/// # let _ = events;
/// ```
pub struct ToolCallStream<'s> {
    intake: RecoveryIntake<'s>,
    request: ToolRecoveryRequest,
    governor: Governor,
}

impl fmt::Debug for ToolCallStream<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolCallStream")
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}

impl<'s> ToolCallStream<'s> {
    fn new(
        tools: &'s ToolSet,
        request: ToolRecoveryRequest,
        dialects: &'s DialectSet,
        mut governor: Governor,
    ) -> Self {
        let intake = RecoveryIntake::new(tools, &request, dialects, &mut governor);
        Self {
            intake,
            request,
            governor,
        }
    }

    /// Feed raw output bytes and append each newly settled event to `events`.
    /// Empty feeds do not allocate events or charge work. Sink memory belongs
    /// to the caller from inception; each event admits its portable record and
    /// payload storage through handoff, identically to batch's discard path.
    /// Producer reservations release after the sink receives each event.
    ///
    /// Refusal stops interpretation, never truncates a successful final result.
    /// Call [`Self::finish`] to obtain its explicit receipt and source identity.
    pub fn feed(&mut self, bytes: &[u8], events: &mut impl Extend<StreamEvent>) {
        self.intake
            .feed_with(bytes, &mut self.governor, &mut |_| {}, &mut |event| {
                events.extend(core::iter::once(event));
                Ok(())
            });
    }

    /// Feed bytes and return the newly settled events in a fresh vector.
    /// The collector is caller-owned storage from inception. The same per-event
    /// construction and handoff admission runs with any sink and any chunking;
    /// creating an empty collector has no resource charge.
    #[must_use]
    pub fn feed_collect(&mut self, bytes: &[u8]) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        self.intake
            .feed_with(bytes, &mut self.governor, &mut |_| {}, &mut |event| {
                events
                    .try_reserve(1)
                    .map_err(|error| reserve_failure(&error))?;
                events.push(event);
                Ok(())
            });
        events
    }

    /// Cancel future interpretation. The first governor refusal is preserved.
    /// Subsequent feeds still contribute their bytes to the source digest.
    pub fn cancel(&mut self) {
        self.intake.cancel(&mut self.governor);
    }

    /// Bytes still owned by lexical prefixes or UTF-8 boundaries, excluding
    /// candidate coverage. Reading this cached count allocates and charges nothing.
    /// A shared cancellation request is observed at the next mutable operation.
    /// This literal-leading dialect holds an incomplete scalar as encoding carry:
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_core::tool_call::{DialectId, DialectSet, ToolSet, ToolRecoveryRequest};
    /// let tools = ToolSet::compile([], Budget::default()).unwrap();
    /// let dialects = DialectSet::new([DialectId::Qwen3CoderXml],
    ///     &mut Governor::new(Budget::default())).unwrap();
    /// let mut stream = tools.stream_with_dialects(ToolRecoveryRequest::default(), &dialects);
    /// let _ = stream.feed_collect(&[0xf0, 0x9f, 0x98]);
    /// assert_eq!(stream.held_bytes(), 3);
    /// ```
    #[must_use]
    pub const fn held_bytes(&self) -> usize {
        self.intake.held_bytes()
    }

    /// Unique source bytes owned by live candidates or pending interpretation.
    /// Closed frames awaiting final semantics remain counted; receipt storage
    /// alone is excluded. Late discriminators and open reasoning can grow this
    /// count without a fixed bound. Refusal or explicit cancellation disposes
    /// these owners. Reading the cache allocates and charges nothing.
    /// Class-leading projections can own plain text before their discriminator;
    /// a literal-leading dialect leaves text without an opener outside candidates:
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_core::tool_call::{DialectId, DialectSet, ToolSet, ToolRecoveryRequest};
    /// let tools = ToolSet::compile([], Budget::default()).unwrap();
    /// let dialects = DialectSet::new([DialectId::Qwen3CoderXml],
    ///     &mut Governor::new(Budget::default())).unwrap();
    /// let mut stream = tools.stream_with_dialects(ToolRecoveryRequest::default(), &dialects);
    /// let _ = stream.feed_collect(b"plain content");
    /// assert_eq!(stream.candidate_bytes(), 0);
    /// ```
    #[must_use]
    pub const fn candidate_bytes(&self) -> usize {
        self.intake.candidate_bytes()
    }

    /// Maximum lexical and UTF-8 boundary holdback for this request's dialects.
    /// The compiled bound includes at least three UTF-8 carry bytes and scalar
    /// backoff before custom byte openers. Candidate bytes are separate. The
    /// bound remains available when stream preparation refuses.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolSet, ToolRecoveryRequest}};
    /// let tools = ToolSet::compile([], Budget::default()).unwrap();
    /// let stream = tools.stream(ToolRecoveryRequest::default());
    /// assert!(stream.hold_back_bound() >= 3);
    /// ```
    #[must_use]
    pub const fn hold_back_bound(&self) -> usize {
        self.intake.hold_back_bound()
    }

    /// Finish the existing recognizer using the model's actual stop reason.
    /// No input replay or new governor is used. The result and usage follow the
    /// same batch path for the complete supplied source and final stop reason.
    /// Exhaustion, cancellation and invalid requests return `Unresolved` with
    /// the explicit reason in the receipt and the complete admitted source
    /// or none. A semantic request refusal still permits complete source
    /// capture while the governor remains healthy.
    /// EOF events are constructed with identical admission and discarded;
    /// use [`Self::finish_with_events`] or [`Self::finish_collect`] to keep them.
    #[must_use]
    pub fn finish(self, stop: GenerationStop) -> AssistResult<ToolCalls> {
        self.finish_sink(stop, &mut |_| Ok(()))
    }

    /// Finish while appending EOF events to a caller's sink. This releases
    /// lexical holdback and completes the same event producer used by `feed`;
    /// the result and resource admissions are identical to [`Self::finish`].
    #[must_use]
    pub fn finish_with_events(
        self,
        stop: GenerationStop,
        events: &mut impl Extend<StreamEvent>,
    ) -> AssistResult<ToolCalls> {
        self.finish_sink(stop, &mut |event| {
            events.extend(core::iter::once(event));
            Ok(())
        })
    }

    /// Finish and return EOF events followed by the final recovery result.
    /// The collector is caller-owned storage from inception. Allocation is
    /// fallible during the same per-event handoff admission; a refusal is
    /// recorded in the returned result, preserving earlier events.
    #[must_use]
    pub fn finish_collect(
        self,
        stop: GenerationStop,
    ) -> (Vec<StreamEvent>, AssistResult<ToolCalls>) {
        let mut events = Vec::new();
        let result = self.finish_sink(stop, &mut |event| {
            events
                .try_reserve(1)
                .map_err(|error| reserve_failure(&error))?;
            events.push(event);
            Ok(())
        });
        (events, result)
    }

    fn finish_sink(
        mut self,
        stop: GenerationStop,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> AssistResult<ToolCalls> {
        self.request.stop = stop;
        self.intake
            .finish_with_events(&self.request, &mut self.governor, sink)
    }
}

impl ToolSet {
    /// Start streaming with built-in dialects and the request's budget.
    /// See [`ToolCallStream`] for an example and current event coverage.
    #[must_use]
    pub fn stream(&self, request: ToolRecoveryRequest) -> ToolCallStream<'_> {
        let governor = Governor::new(request.budget);
        self.stream_governed(request, governor)
    }

    /// Start streaming with built-in dialects and an owned original governor.
    /// Its limits and cumulative usage are authoritative; `request.budget` is
    /// ignored. Exhaustion is returned by [`ToolCallStream::finish`].
    #[must_use]
    pub fn stream_governed(
        &self,
        request: ToolRecoveryRequest,
        governor: Governor,
    ) -> ToolCallStream<'_> {
        self.stream_with_dialects_governed(request, DialectSet::built_in(), governor)
    }

    /// Start streaming over an explicit borrowed prepared dialect set.
    /// The request's budget applies; neither tools nor dialects are cloned.
    #[must_use]
    pub fn stream_with_dialects<'s>(
        &'s self,
        request: ToolRecoveryRequest,
        dialects: &'s DialectSet,
    ) -> ToolCallStream<'s> {
        let governor = Governor::new(request.budget);
        self.stream_with_dialects_governed(request, dialects, governor)
    }

    /// Start streaming over borrowed dialects with an owned original governor.
    /// Its limits and cumulative usage are authoritative. The stream owns no
    /// self-references and does not clone the prepared set or request.
    #[must_use]
    pub fn stream_with_dialects_governed<'s>(
        &'s self,
        request: ToolRecoveryRequest,
        dialects: &'s DialectSet,
        governor: Governor,
    ) -> ToolCallStream<'s> {
        ToolCallStream::new(self, request, dialects, governor)
    }
}
