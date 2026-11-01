// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Live recovery intake and owned event publication.
//!
//! Live events follow the history shared by native continuations. Calls subject
//! to later prose normalization hold numeric call indexing. Finalization flushes
//! remaining source facts from a unique completed reading, without replaying
//! input. Longest semantic-prefix equivalence, ambiguous EOF intersection,
//! settled parameter/completion/revision events, decoding queries and stream
//! cloning are not implemented yet.

use super::recover::RecoveryIntake;
use super::{
    CallSegment, DialectId, DialectSet, GenerationStop, Piece, RecognizedTurn, Recognizer, Segment,
    ToolCalls, ToolRecoveryRequest, ToolSet,
};
use crate::contract::{
    AssistResult, Governor, Resource, ScratchCharge, ScratchString, ScratchVec, StopReason,
    invalid_input, push_scratch, reserve_failure, saturating_size as size,
};
use crate::grammar::CaptureKind;
use core::fmt;
use core::ops::Range;

/// Portable admission per published event, including collector storage.
/// Four native slots cover even a fresh `Vec` receiving a single event.
const EVENT_BYTES: u64 = 256;
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
}

impl StreamEvent {
    fn admit(governor: &mut Governor) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 4)?;
        governor.charge(Resource::ScratchBytes, EVENT_BYTES)
    }

    /// Publish identical ownership before either the batch discard sink or a
    /// caller receives the event. Consumer retention cannot change accounting.
    pub(super) fn content(
        span: Range<usize>,
        raw: ScratchVec<u8>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        // Amortized collector insertion and relocation work is admitted per
        // event on every path, including batch and externally supplied sinks.
        Self::admit(governor)?;
        Ok(Self::ContentDelta {
            span,
            raw: raw.into_vec(),
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
    ) -> Result<Self, StopReason> {
        let raw = Self::bytes(span.clone(), source, governor)?;
        Self::admit(governor)?;
        Ok(Self::ReasoningDelta {
            span,
            raw: raw.into_vec(),
        })
    }

    fn started(
        index: usize,
        offset: usize,
        candidates: impl IntoIterator<Item = DialectId>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
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
        Self::admit(governor)?;
        Ok(Self::CallStarted {
            index,
            offset,
            dialect_candidates: dialects.into_vec(),
        })
    }

    fn name(index: usize, tool: &str, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(Resource::Work, size(tool.len()).saturating_add(1))?;
        let mut name = ScratchString::new(governor);
        name.push_str(tool)?;
        Self::admit(governor)?;
        Ok(Self::CallName {
            index,
            tool: name.into_string(),
        })
    }

    fn argument(
        index: usize,
        span: Range<usize>,
        source: &[u8],
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let raw = Self::bytes(span, source, governor)?;
        Self::admit(governor)?;
        Ok(Self::ArgumentDelta {
            index,
            raw: raw.into_vec(),
        })
    }
}

/// Metadata only: payload ownership has already been permanently published.
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
    indexed: bool,
    reasoning_end: usize,
    calls: ScratchVec<LiveCall>,
    arguments: ScratchVec<ArgumentFact>,
}

impl EventCursor {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            history: 0,
            indexed: true,
            reasoning_end: 0,
            calls: ScratchVec::new(governor),
            arguments: ScratchVec::new(governor),
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
        while let Some(step) = recognizer.settled_step(self.history, governor)? {
            if step.end > end {
                break;
            }
            match step.segment {
                Some(Segment::Reasoning(block)) => {
                    if !block.text.is_empty() {
                        sink(StreamEvent::reasoning(
                            block.text.clone(),
                            source,
                            governor,
                        )?)?;
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
                        sink(StreamEvent::started(
                            index,
                            call.span.start,
                            call.readings.iter().map(|raw| raw.dialect.clone()),
                            governor,
                        )?)?;
                        let name = unique_name(call, tools, governor)?;
                        if let Some(name) = name {
                            sink(StreamEvent::name(index, name, governor)?)?;
                        }
                        let arguments = argument_span(call, governor)?;
                        if let Some(span) = &arguments {
                            sink(StreamEvent::argument(
                                index,
                                span.clone(),
                                source,
                                governor,
                            )?)?;
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
        }
        Ok(())
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
                sink(StreamEvent::content(span, raw, governor)?)?;
            }
        }
        for block in &calls.reasoning_blocks {
            governor.charge(Resource::Work, 1)?;
            let start = block.text.start.max(self.reasoning_end);
            if start < block.text.end {
                sink(StreamEvent::reasoning(
                    start..block.text.end,
                    source,
                    governor,
                )?)?;
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
                sink(StreamEvent::started(
                    index,
                    call.span.start,
                    call.candidate_dialects.iter().cloned(),
                    governor,
                )?)?;
            }
            if live.is_none_or(|live| !live.named)
                && let Some(tool) = &call.tool
            {
                sink(StreamEvent::name(index, tool, governor)?)?;
            }
            if live.is_none_or(|live| !live.arguments)
                && let Some(span) = self.final_arguments(&call.span, governor)?
            {
                sink(StreamEvent::argument(index, span, source, governor)?)?;
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
/// See [`StreamEvent`] for the implemented source facts. Settled parameter,
/// completion and revision events remain unimplemented.
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
    /// to the caller; each event admits its portable collector storage before
    /// the sink receives it, identically to batch's discard path.
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
    /// The same per-event admission runs with any sink and any chunking;
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
    /// the explicit reason in the receipt, retaining no partial source.
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
    /// Collector allocation is fallible within the same per-event admission;
    /// a refusal is recorded in the returned result, preserving earlier events.
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
