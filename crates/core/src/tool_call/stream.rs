// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Live recovery intake and owned event publication.
//!
//! The current event producer publishes the initial native content run only.
//! Content following structural segments, semantic call/reasoning events,
//! decoding queries and stream cloning are not implemented yet. Finalization
//! resolves the live recognizer without replaying the retained source.

use super::recover::RecoveryIntake;
use super::{DialectSet, GenerationStop, ToolCalls, ToolRecoveryRequest, ToolSet};
use crate::contract::{AssistResult, Governor, Resource, ScratchVec, StopReason, reserve_failure};
use core::fmt;
use core::ops::Range;

/// Portable admission per published event, including collector storage.
/// Four native slots cover even a fresh `Vec` receiving a single event.
const EVENT_BYTES: u64 = 256;
const _: () = assert!(4 * core::mem::size_of::<StreamEvent>() as u64 <= EVENT_BYTES);

/// An owned notification whose source interpretation is already stable.
///
/// Only initial content notifications are implemented so far. Events describe
/// source bytes; they do not grant permission to execute a tool.
#[derive(Debug, PartialEq, Eq)]
pub enum StreamEvent {
    /// Newly settled bytes of the initial content run, before any native call,
    /// rejected attempt or reasoning block. Adjacent events do not overlap.
    ContentDelta {
        /// The exact range in the original output.
        span: Range<usize>,
        /// Original bytes, including invalid UTF-8 without replacement.
        /// Boundaries never split a valid UTF-8 scalar.
        raw: Vec<u8>,
    },
}

impl StreamEvent {
    /// Publish identical ownership before either the batch discard sink or a
    /// caller receives the event. Consumer retention cannot change accounting.
    pub(super) fn content(
        span: Range<usize>,
        raw: ScratchVec<u8>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        // Amortized collector insertion and relocation work is admitted per
        // event on every path, including batch and externally supplied sinks.
        governor.charge(Resource::Work, 4)?;
        governor.charge(Resource::ScratchBytes, EVENT_BYTES)?;
        Ok(Self::ContentDelta {
            span,
            raw: raw.into_vec(),
        })
    }
}

/// A live tool-call recovery operation borrowing its tools and dialects.
///
/// It owns the request and the original governor. Every feed is a byte fold
/// through the same intake as batch recovery, including event allocation and
/// publication. Exhaustion and cancellation are reported by the final receipt;
/// bytes supplied after refusal still contribute to its complete source digest.
///
/// The current event API covers initial content only; see [`StreamEvent`].
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
    #[must_use]
    pub fn finish(mut self, stop: GenerationStop) -> AssistResult<ToolCalls> {
        self.request.stop = stop;
        self.intake.finish(&self.request, &mut self.governor)
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
