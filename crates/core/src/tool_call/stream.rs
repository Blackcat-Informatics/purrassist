// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Live recovery intake and owned event publication.
//!
//! Live events follow the history shared by native continuations. Calls subject
//! to later prose normalization hold numeric call indexing. Finalization flushes
//! remaining source facts from a unique completed reading, without replaying
//! input. Completion events retain final certainty and typed values, while
//! native revisions retain their actual operator payloads. Longest semantic
//! prefix equivalence, ambiguous EOF intersection, semantic-search revisions,
//! decoding queries and stream cloning are not implemented yet.

use super::recover::RecoveryIntake;
use super::{
    CallOutcome, CallSegment, DialectId, DialectSet, GenerationStop, Piece, RecognizedTurn,
    Recognizer, Rejection, Segment, ToolCall, ToolCalls, ToolRecoveryRequest, ToolSet,
    UnresolvedReason,
};
use crate::contract::{
    AssistResult, Governor, Resource, ScratchCharge, ScratchLease, ScratchString, ScratchVec,
    StopReason, invalid_input, push_scratch, reserve_failure, saturating_size as size,
};
use crate::grammar::CaptureKind;
use crate::grammar::edit::{EditOperator, MarkupEdit, OPERATORS};
use crate::json::{Node, OwnedDocument, Value};
use core::fmt;
use core::ops::Range;

/// Portable admission for constructing and handing off one event.
/// Four native slots also admit the collector insertion per event.
const EVENT_BYTES: u64 = 512;
const _: () = assert!(4 * core::mem::size_of::<StreamEvent>() as u64 <= EVENT_BYTES);

/// An owned notification whose source interpretation is already stable.
///
/// Events describe source facts; they do not grant permission to execute a tool.
/// Use the final result's outcomes, losses and inferences to assess certainty.
#[derive(Debug, PartialEq, Eq)]
pub enum StreamEvent {
    /// Newly settled content bytes. Live notifications currently cover the
    /// initial native content run; EOF flushes later content of a unique final
    /// reading. Notifications do not overlap.
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
    pub(super) fn content_end(&self, governor: &mut Governor) -> Result<usize, StopReason> {
        match &self.event {
            StreamEvent::ContentDelta { span, .. } => Ok(span.end),
            _ => Err(StopReason::InvalidInput(
                crate::contract::format_output_governed(
                    format_args!("content producer prepared a different event"),
                    governor,
                )?,
            )),
        }
    }

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
        let levels = usize::BITS - dialects.len().leading_zeros() + 1;
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

fn copy_bytes(bytes: &[u8], governor: &mut Governor) -> Result<ScratchVec<u8>, StopReason> {
    governor.charge(Resource::Work, size(bytes.len()).saturating_add(1))?;
    let mut copied = ScratchVec::new(governor);
    copied.reserve(bytes.len())?;
    copied.extend(bytes.iter().copied())?;
    Ok(copied)
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

/// Owned transient copies of real operator payloads, never reconstructed from
/// a human explanation or guessed from the final value tree.
#[derive(Debug)]
struct EditFact {
    call_start: usize,
    call_end: usize,
    operator: EditOperator,
    at: usize,
    end: usize,
    before: ScratchVec<u8>,
    after: ScratchVec<u8>,
    parameter: Option<ScratchString>,
}

impl ScratchCharge for EditFact {
    const CHARGE: usize = 192;
}
const _: () = assert!(core::mem::size_of::<EditFact>() <= EditFact::CHARGE);

impl EditFact {
    fn key(&self) -> (usize, usize, EditOperator, usize, usize) {
        (
            self.call_start,
            self.call_end,
            self.operator,
            self.at,
            self.end,
        )
    }

    fn same_payload(&self, other: &Self, governor: &mut Governor) -> Result<bool, StopReason> {
        governor.charge(
            Resource::Work,
            size(self.before.len())
                .saturating_add(size(self.after.len()))
                .saturating_add(
                    self.parameter
                        .as_ref()
                        .map_or(0, |pointer| size(pointer.len())),
                )
                .saturating_add(1),
        )?;
        Ok(self.before == other.before && self.after == other.after)
    }
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

#[derive(Clone, Copy, Debug)]
struct ArgumentFact {
    start: usize,
    end: usize,
    capture: Option<(usize, usize)>,
}

impl ScratchCharge for ArgumentFact {
    const CHARGE: usize = 40;
}
const _: () = assert!(core::mem::size_of::<ArgumentFact>() <= ArgumentFact::CHARGE);

/// One monotone traversal of native shared history plus EOF capture facts.
/// Nothing here resolves a prefix, replays source or snapshots a governor.
pub(super) struct EventCursor {
    history: u32,
    /// A successful traversal depends on immutable native ancestry and the
    /// normalized end. Reasoning embedding can change, but its text and the
    /// stored call readings inspected here do not. Include end so a pending
    /// step beyond the previous cutoff is reconsidered when it advances.
    advance_key: Option<(u32, usize)>,
    indexed: bool,
    reasoning_end: usize,
    calls: ScratchVec<LiveCall>,
    arguments: ScratchVec<ArgumentFact>,
    edits: ScratchVec<EditFact>,
}

impl EventCursor {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            history: 0,
            advance_key: None,
            indexed: true,
            reasoning_end: 0,
            calls: ScratchVec::new(governor),
            arguments: ScratchVec::new(governor),
            edits: ScratchVec::new(governor),
        }
    }

    pub(super) fn advance(
        &mut self,
        recognizer: &Recognizer<'_>,
        source: &[u8],
        tools: &ToolSet,
        end: usize,
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        let key = (recognizer.settled_history_id(), end);
        governor.charge(Resource::Work, 1)?;
        if self.advance_key == Some(key) {
            return Ok(());
        }
        while let Some(step) = recognizer.settled_step(self.history, governor)? {
            if step.end > end {
                break;
            }
            match step.segment {
                Some(Segment::Reasoning(block)) => {
                    if !block.text.is_empty() {
                        StreamEvent::reasoning(block.text.clone(), source, governor)?
                            .publish(sink)?;
                    }
                    self.reasoning_end = block.text.end;
                }
                Some(Segment::Call(call)) if self.indexed => {
                    governor.charge(Resource::Work, size(call.readings.len()))?;
                    if call.readings.iter().any(|raw| {
                        raw.dialect.grammar().examples_content()
                            || (raw.bare && raw.dialect.grammar().declaration_content())
                    }) {
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
                _ => {}
            }
            self.history = step.history;
        }
        self.advance_key = Some(key);
        Ok(())
    }

    /// The final native turn already exists; retain only capture offsets before
    /// resolution consumes it. Alternative readings must agree on the capture.
    pub(super) fn remember_final(
        &mut self,
        turn: &RecognizedTurn,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        for piece in turn.pieces() {
            governor.charge(Resource::Work, 1)?;
            match piece {
                Piece::Segment(segment) => self.remember(segment, governor)?,
                Piece::Choice(choice) => {
                    for reading in &choice.readings {
                        for segment in reading {
                            self.remember(segment, governor)?;
                        }
                    }
                }
            }
        }
        let levels = usize::BITS - self.arguments.len().leading_zeros() + 1;
        governor.charge(
            Resource::Work,
            size(self.arguments.len()).saturating_mul(u64::from(levels)),
        )?;
        self.arguments
            .as_mut_slice()
            .sort_unstable_by_key(|fact| (fact.start, fact.end));
        let levels = usize::BITS - self.edits.len().leading_zeros() + 1;
        governor.charge(
            Resource::Work,
            size(self.edits.len())
                .saturating_mul(u64::from(levels))
                .saturating_mul(5),
        )?;
        self.edits
            .as_mut_slice()
            .sort_unstable_by_key(EditFact::key);
        Ok(())
    }

    fn remember(&mut self, segment: &Segment, governor: &mut Governor) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        if let Segment::Call(call) = segment {
            let capture = argument_span(call, governor)?.map(|span| (span.start, span.end));
            push_scratch(
                &mut self.arguments,
                ArgumentFact {
                    start: call.span.start,
                    end: call.span.end,
                    capture,
                },
                governor,
            )?;
            for raw in &call.readings {
                for edit in &raw.edits {
                    self.remember_edit(call, raw, edit, governor)?;
                }
            }
        }
        Ok(())
    }

    fn remember_edit(
        &mut self,
        call: &CallSegment,
        raw: &super::RawCall,
        edit: &MarkupEdit,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        if edit.original.is_empty() && edit.replacement.is_empty() {
            return Ok(());
        }
        let mut parameter = None;
        for leaf in &raw.arguments.leaves {
            governor.charge(Resource::Work, size(leaf.pointer.len()).saturating_add(1))?;
            if leaf.raw.start <= edit.span.start && edit.span.end <= leaf.raw.end {
                if parameter.is_some_and(|known| known != leaf.pointer.as_str()) {
                    parameter = None;
                    break;
                }
                parameter = Some(leaf.pointer.as_str());
            }
        }
        let fact = EditFact {
            call_start: call.span.start,
            call_end: call.span.end,
            operator: edit.operator,
            at: edit.span.start,
            end: edit.span.end,
            before: copy_bytes(&edit.original, governor)?,
            after: copy_bytes(&edit.replacement, governor)?,
            parameter: parameter
                .map(|pointer| ScratchString::copy_from(pointer, governor))
                .transpose()?,
        };
        push_scratch(&mut self.edits, fact, governor)
    }

    /// Flush only facts from a complete, unique whole-message reading. This is
    /// EOF completion of the live producer, not another recovery operation.
    pub(super) fn finish(
        &self,
        calls: &ToolCalls,
        content_end: usize,
        source: &[u8],
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        for range in &calls.content {
            governor.charge(Resource::Work, 1)?;
            let start = range.start.max(content_end);
            if start < range.end {
                let span = start..range.end;
                let raw = StreamEvent::bytes(span.clone(), source, governor)?;
                StreamEvent::content(span, raw, governor)?.publish(sink)?;
            }
        }
        for block in &calls.reasoning_blocks {
            governor.charge(Resource::Work, 1)?;
            let start = block.text.start.max(self.reasoning_end);
            if start < block.text.end {
                StreamEvent::reasoning(start..block.text.end, source, governor)?.publish(sink)?;
            }
        }
        for (index, call) in calls.calls.iter().enumerate() {
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
                    call.candidate_dialects.iter().cloned(),
                    governor,
                )?
                .publish(sink)?;
            }
            if live.is_none_or(|live| !live.named)
                && let Some(tool) = &call.tool
            {
                StreamEvent::name(index, tool, governor)?.publish(sink)?;
            }
            if live.is_none_or(|live| !live.arguments)
                && let Some(span) = self.final_arguments(&call.span, governor)?
            {
                StreamEvent::argument(index, span, source, governor)?.publish(sink)?;
            }
            self.revisions(index, call, governor, sink)?;
            StreamEvent::completed(index, &call.outcome, governor)?.publish(sink)?;
            // All typed parameter events follow completion, including mutating
            // calls. Ambiguous/unresolved values are not presented as settled.
            if matches!(call.outcome, CallOutcome::Resolved | CallOutcome::BestGuess) {
                parameters(index, &call.arguments, governor, sink)?;
            }
        }
        for rejected in &calls.rejected {
            governor.charge(Resource::Work, 1)?;
            StreamEvent::rejected(rejected.span.clone(), rejected.reason, governor)?
                .publish(sink)?;
        }
        governor.check()
    }

    fn revisions(
        &self,
        index: usize,
        call: &ToolCall,
        governor: &mut Governor,
        sink: &mut impl FnMut(StreamEvent) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        for evidence in &call.repairs.evidence {
            governor.charge(
                Resource::Work,
                size(OPERATORS.len()).saturating_mul(size(evidence.rule.len()).saturating_add(1)),
            )?;
            let Some(operator) = OPERATORS
                .iter()
                .find(|operator| operator.id == evidence.rule)
            else {
                continue;
            };
            let Some(span) = &evidence.span else {
                continue;
            };
            let key = (
                call.span.start,
                call.span.end,
                operator.operator,
                span.bytes.start,
                span.bytes.end,
            );
            let levels = usize::BITS - self.edits.len().leading_zeros() + 1;
            governor.charge(Resource::Work, u64::from(levels).saturating_mul(5))?;
            let first = self.edits.partition_point(|edit| edit.key() < key);
            let Some(edit) = self.edits.as_slice().get(first) else {
                continue;
            };
            if edit.key() != key {
                continue;
            }
            let mut shared = true;
            let mut parameter = edit.parameter.as_ref().map(ScratchString::as_str);
            for other in &self.edits.as_slice()[first + 1..] {
                governor.charge(Resource::Work, 5)?;
                if other.key() != key {
                    break;
                }
                if !edit.same_payload(other, governor)? {
                    shared = false;
                    break;
                }
                if parameter != other.parameter.as_ref().map(ScratchString::as_str) {
                    parameter = None;
                }
            }
            if shared {
                StreamEvent::revision(index, edit, parameter, governor)?.publish(sink)?;
            }
        }
        Ok(())
    }

    fn final_arguments(
        &self,
        span: &Range<usize>,
        governor: &mut Governor,
    ) -> Result<Option<Range<usize>>, StopReason> {
        let key = (span.start, span.end);
        let levels = usize::BITS - self.arguments.len().leading_zeros() + 1;
        governor.charge(Resource::Work, u64::from(levels))?;
        let first = self
            .arguments
            .partition_point(|fact| (fact.start, fact.end) < key);
        let Some(fact) = self.arguments.as_slice().get(first) else {
            return Ok(None);
        };
        if (fact.start, fact.end) != key {
            return Ok(None);
        }
        for other in &self.arguments.as_slice()[first + 1..] {
            governor.charge(Resource::Work, 1)?;
            if (other.start, other.end) != key {
                break;
            }
            if other.capture != fact.capture {
                return Ok(None);
            }
        }
        Ok(fact.capture.map(|(start, end)| start..end))
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
    let levels = usize::BITS - ordered.len().leading_zeros() + 1;
    governor.charge(
        Resource::Work,
        size(ordered.len())
            .saturating_mul(u64::from(levels))
            .saturating_mul(size(longest).saturating_add(1)),
    )?;
    ordered
        .as_mut_slice()
        .sort_unstable_by(|(_, left), (_, right)| left.cmp(right));
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
    let levels = usize::BITS - tools.name_index_len().leading_zeros() + 1;
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
/// See [`StreamEvent`] for source facts and EOF completion events. Semantic
/// search revisions and events common to ambiguous final readings remain
/// unfinished.
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
        self.governor.cancel();
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
