// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Anthropic Messages streaming event logs reassembled into tool calls.
//!
//! An event log is the server-sent-event text of one streamed message:
//! `message_start`, then per content block a `content_block_start`, its
//! `content_block_delta` events and a `content_block_stop`, then
//! `message_delta` (carrying the stop reason) and `message_stop`.
//! [`AnthropicEventStream`] reads the log in any number of chunks and rebuilds
//! the complete message those events describe, as compact JSON:
//!
//! ```text
//! {"role":"assistant","content":[BLOCK,BLOCK,...]}
//! ```
//!
//! A `tool_use` block is `{"type":"tool_use","id":ID,"name":NAME,"input":INPUT}`
//! where `INPUT` is the concatenation of the block's `input_json_delta`
//! fragments, byte for byte, or `{}` when there are none; when the fragments
//! leave a string or container open, whether the block is stopped or cut, the
//! closers they owe are appended, which is a fault and withholds its call. A `text` block is
//! `{"type":"text","text":"..."}` and a `thinking` block is
//! `{"type":"thinking","thinking":"...","signature":"..."}`; the strings keep
//! their original JSON escapes. The message bytes are fed, event by event,
//! through the same [`ToolCallStream`] as raw output, so the recovered calls,
//! repairs, losses and receipt are those of [`ToolSet::recover`] on that
//! message with the stop reason the log states. Each fragment depends only on
//! the sequence of events, never on how the log was chunked, so every chunking
//! gives the same result.
//!
//! Everything the log does wrong is an [`EventFault`], never a silent drop:
//! a faulted event contributes nothing to the message, and a call whose block
//! is faulted (or any call, when the fault belongs to the whole log) is never
//! offered by [`EventLogRecovery::usable`], whatever the call's own outcome.
//!
//! Cost is linear in the log: each byte is charged to the governor once for
//! intake and once for retention, each event once for its size, and the
//! message bytes are charged by the inner [`ToolCallStream`].

use super::provider::{DiscardEvents, FaultScope, provider_input};
use super::stream::{debug_provider_stream, usable_provider_calls};
use super::{
    DialectSet, GenerationStop, StreamEvent, ToolCall, ToolCallStream, ToolCalls,
    ToolRecoveryRequest, ToolSet,
};
use crate::contract::{
    AssistResult, Resource, ScratchCharge, ScratchVec, StopReason, push_scratch,
    saturating_size as size,
};
use crate::json::{Node, OwnedDocument, OwnedParseFailure, Value};
use core::fmt;
use core::ops::Range;

/// The message prefix every reassembled message starts with.
const MESSAGE_OPEN: &[u8] = br#"{"role":"assistant","content":["#;
/// The message suffix every reassembled message ends with.
const MESSAGE_CLOSE: &[u8] = b"]}";

/// The transport class of a [`EventFaultKind`].
pub use super::provider::ProviderFaultClass as EventFaultClass;

/// What was wrong with one event, or with the log as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventFaultKind {
    /// The event data is not UTF-8. The event is skipped.
    NotUtf8,
    /// The event data is not JSON. The event is skipped.
    NotJson,
    /// A required member is missing or has the wrong type. The event is
    /// skipped, except that a `tool_use` block with no `id` or `name` is
    /// still assembled without it.
    MissingMember(&'static str),
    /// An event or delta type this reader does not assemble. It is skipped.
    UnsupportedEvent,
    /// A content block type other than `tool_use`, `text` and `thinking`.
    /// The block and its deltas contribute nothing to the message.
    UnsupportedBlock,
    /// A delta whose type does not fit its block, or a `thinking_delta` after
    /// the block's signature began. The delta is skipped.
    MismatchedDelta,
    /// A `message_delta` whose `stop_reason` is not one this reader knows.
    /// The stop is read as unknown and no call is usable.
    UnrecognizedStopReason,
    /// A `tool_use` start whose `input` is not `{}`. The deltas alone build
    /// the input, as the API defines.
    NonEmptyStartInput,
    /// The log ends inside a line or an event that was not closed by a blank
    /// line. The event is discarded, as server-sent events require.
    PartialEvent,
    /// A content block was never stopped (the log ended, or `message_stop`
    /// arrived). On this forced close the message gets a closing quote for an
    /// unfinished string (and a second backslash for a trailing one), the
    /// closers of the input's open containers, then the block's own closer,
    /// so later blocks stay readable. The block's call is withheld.
    OpenBlock,
    /// A `content_block_stop` arrived while the tool input's fragments still
    /// left a string or container open. The message gets the closing quote and
    /// the owed closers, as for [`Self::OpenBlock`], so later blocks stay
    /// readable, and the call is withheld: a repair of cut-off input is not a
    /// resolved call.
    UnfinishedInput,
    /// A tool input's fragments are not one well-formed JSON object: a first
    /// non-whitespace byte other than `{`, a closer with no container open, a closer of the other kind than the innermost
    /// opener, or non-whitespace after the top-level value closed. Reported
    /// once per block, which is withheld, so fragment text cannot close its own
    /// block or form further message structure.
    MalformedInput,
    /// An event block that has field lines (such as `event:`) but no `data`
    /// line, so nothing says what the event carried. It is skipped, and the
    /// fault belongs to the whole log: a dropped delta would silently change a
    /// call's arguments. Comment-only blocks (keep-alives) are not events.
    MissingData,
    /// The log ended without `message_stop`.
    MissingMessageStop,
    /// The log never stated a stop reason: no `message_delta` with a
    /// `stop_reason`. The turn is read with an unknown stop, so a cut by
    /// `max_tokens` cannot be told from a normal end; no call is usable.
    MissingMessageDelta,
    /// The first event is not `message_start`.
    MessageStartMissing,
    /// A second `message_start`.
    MessageStartRepeated,
    /// An event after `message_stop`. It is skipped.
    EventAfterMessageStop,
    /// A `content_block_start` whose index is not the next one.
    BlockIndexSequence,
    /// A `content_block_start` while the previous block is still open. The
    /// previous block is closed first.
    BlockStartWhileOpen,
    /// A delta for a block that never started. It is skipped.
    DeltaWithoutStart,
    /// A delta for a block that was already stopped. It is skipped, even if
    /// its bytes would complete the block's input.
    DeltaAfterStop,
    /// A `content_block_stop` for a block that is not open. It is skipped.
    StopWithoutOpenBlock,
    /// An `error` event.
    ProviderError,
    /// The provider stopped the turn at the generation limit.
    LengthStop,
}

impl EventFaultKind {
    pub(super) const fn wire_code(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::NotUtf8 => ("not_utf8", None),
            Self::NotJson => ("not_json", None),
            Self::MissingMember(member) => ("missing_member", Some(member)),
            Self::UnsupportedEvent => ("unsupported_event", None),
            Self::UnsupportedBlock => ("unsupported_block", None),
            Self::MismatchedDelta => ("mismatched_delta", None),
            Self::UnrecognizedStopReason => ("unrecognized_stop_reason", None),
            Self::NonEmptyStartInput => ("non_empty_start_input", None),
            Self::PartialEvent => ("partial_event", None),
            Self::OpenBlock => ("open_block", None),
            Self::UnfinishedInput => ("unfinished_input", None),
            Self::MalformedInput => ("malformed_input", None),
            Self::MissingData => ("missing_data", None),
            Self::MissingMessageStop => ("missing_message_stop", None),
            Self::MissingMessageDelta => ("missing_message_delta", None),
            Self::MessageStartMissing => ("message_start_missing", None),
            Self::MessageStartRepeated => ("message_start_repeated", None),
            Self::EventAfterMessageStop => ("event_after_message_stop", None),
            Self::BlockIndexSequence => ("block_index_sequence", None),
            Self::BlockStartWhileOpen => ("block_start_while_open", None),
            Self::DeltaWithoutStart => ("delta_without_start", None),
            Self::DeltaAfterStop => ("delta_after_stop", None),
            Self::StopWithoutOpenBlock => ("stop_without_open_block", None),
            Self::ProviderError => ("provider_error", None),
            Self::LengthStop => ("length_stop", None),
        }
    }

    /// The class of this fault.
    ///
    /// ```
    /// use purrassist_core::tool_call::{EventFaultClass, EventFaultKind};
    /// assert_eq!(EventFaultKind::DeltaAfterStop.class(), EventFaultClass::OutOfOrder);
    /// assert_eq!(EventFaultKind::NotJson.class(), EventFaultClass::Malformed);
    /// ```
    #[must_use]
    pub const fn class(self) -> EventFaultClass {
        match self {
            Self::NotUtf8
            | Self::NotJson
            | Self::MissingMember(_)
            | Self::UnsupportedEvent
            | Self::UnsupportedBlock
            | Self::MismatchedDelta
            | Self::UnrecognizedStopReason
            | Self::NonEmptyStartInput
            | Self::MalformedInput
            | Self::MissingData => EventFaultClass::Malformed,
            Self::PartialEvent
            | Self::OpenBlock
            | Self::UnfinishedInput
            | Self::MissingMessageStop
            | Self::MissingMessageDelta
            | Self::LengthStop => EventFaultClass::Truncated,
            Self::MessageStartMissing
            | Self::MessageStartRepeated
            | Self::EventAfterMessageStop
            | Self::BlockIndexSequence
            | Self::BlockStartWhileOpen
            | Self::DeltaWithoutStart
            | Self::DeltaAfterStop
            | Self::StopWithoutOpenBlock => EventFaultClass::OutOfOrder,
            Self::ProviderError => EventFaultClass::Provider,
        }
    }
}

/// One fault of an event log, in the order found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventFault {
    /// What was wrong.
    pub kind: EventFaultKind,
    /// The position of the event among the log's events (blank-line
    /// terminated), counting from zero; the event count for a fault found at
    /// the end of the log.
    pub event: usize,
    /// The offset in the log where the fault was found: the end of the
    /// event's last line, or the log length for a fault found at the end.
    pub at: usize,
    /// The content block the fault belongs to. `None` means the whole log:
    /// no call of such a log is offered by [`EventLogRecovery::usable`].
    pub block: Option<u64>,
}

impl ScratchCharge for EventFault {
    const CHARGE: usize = 64;
}
const _: () = assert!(core::mem::size_of::<EventFault>() <= EventFault::CHARGE);

/// The range of the reassembled message that one content block occupies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockSpan {
    /// The block's index in the log.
    pub block: u64,
    /// The block's bytes in the reassembled message (the receipt's source).
    pub span: Range<usize>,
    tool_use: bool,
}

impl ScratchCharge for BlockSpan {
    const CHARGE: usize = 32;
}
const _: () = assert!(core::mem::size_of::<BlockSpan>() <= BlockSpan::CHARGE);

/// The result of reading one event log.
///
/// `recovery` is the ordinary recovery result of the reassembled message;
/// its receipt retains that message as its source. Do not select calls from
/// `recovery` with [`ToolCalls::usable`] when `faults` is not empty: use
/// [`Self::usable`], which honors the faults.
#[derive(Debug)]
pub struct EventLogRecovery {
    /// The recovery of the reassembled message.
    pub recovery: AssistResult<ToolCalls>,
    /// Every fault of the log, in the order found. Empty for a clean log.
    pub faults: Vec<EventFault>,
    /// The blocks that were assembled into the message, in order.
    pub blocks: Vec<BlockSpan>,
    /// The original event log, exactly as supplied up to the end of what was
    /// admitted: cancellation and a refusal raised after admission keep every
    /// admitted byte. Shorter than the supplied log (possibly empty) only when
    /// the governor refused the log's own retention or bytes arrived after a
    /// halt; the receipt's `stopped` then says why.
    pub log: Vec<u8>,
    pub(super) whole_fault: bool,
}

impl EventLogRecovery {
    /// The content block index of the block `call` was read from.
    #[must_use]
    pub fn block_of(&self, call: &ToolCall) -> Option<u64> {
        block_at(&self.blocks, call).map(|block| block.block)
    }

    /// Whether the log had no fault.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.faults.is_empty()
    }

    /// The calls that may run as is: [`ToolCalls::usable`] without the calls
    /// of any faulted block, and none at all when a fault belongs to the whole
    /// log (a log cut short, a provider error, a missing `message_start`).
    #[must_use]
    pub fn usable(&self) -> Vec<&ToolCall> {
        usable_provider_calls(&self.recovery.outcome, ToolCalls::usable)
    }

    /// Like [`Self::usable`], built on [`ToolCalls::usable_accepting_guesses`].
    #[must_use]
    pub fn usable_accepting_guesses(&self) -> Vec<&ToolCall> {
        usable_provider_calls(&self.recovery.outcome, ToolCalls::usable_accepting_guesses)
    }
}

fn block_at<'a>(blocks: &'a [BlockSpan], call: &ToolCall) -> Option<&'a BlockSpan> {
    let position = blocks.partition_point(|block| block.span.end <= call.span.start);
    blocks
        .get(position)
        .filter(|block| block.span.contains(&call.span.start))
}

fn tool_block_index(blocks: &[BlockSpan], call: &ToolCall) -> Option<u64> {
    block_at(blocks, call)
        .filter(|block| block.tool_use)
        .map(|block| block.block)
}

/// The kind of the open block, which fixes the deltas it accepts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    ToolUse,
    Text,
    Thinking,
    /// A block type that is not assembled; its deltas are consumed unread.
    Skipped,
}

/// The one open block. Anthropic streams blocks one after another.
struct Open {
    index: u64,
    kind: Kind,
    /// Whether a non-empty input fragment was written.
    wrote: bool,
    /// Whether the signature string of a thinking block began.
    signature: bool,
    /// Where the block starts in the message.
    start: usize,
}

/// Where the lexer of a tool input's fragments stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lex {
    Plain,
    InString,
    Escape,
}

/// How far a tool input's top-level object has come.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Input {
    /// No non-whitespace byte yet.
    Unstarted,
    Open,
    /// The top-level container closed; only whitespace may follow.
    Closed,
}

/// What an event block has so far; a comment line adds nothing.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lines {
    Empty,
    /// Field lines other than `data`.
    Fields,
    Data,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Before,
    Streaming,
    Ended,
}

/// An incremental reader of one Anthropic Messages streaming event log.
///
/// Feed the log's bytes in any chunks, then [`Self::finish`]. Refusal by the
/// governor, or [`Self::cancel`], stops interpretation; the result is then
/// `Unresolved` with the reason in its receipt, as for [`ToolCallStream`].
///
/// ```
/// use purrassist_core::tool_call::{DialectSet, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::Budget;
/// let tools = ToolSet::compile(
///     [ToolDefinition::new("ls").param("depth", Param::integer())],
///     Budget::default(),
/// ).unwrap();
/// let log = concat!(
///     "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
///     "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,",
///     "\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"ls\",\"input\":{}}}\n\n",
///     "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,",
///     "\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"depth\\\": 2}\"}}\n\n",
///     "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
///     "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
///     "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
/// );
/// let mut stream = tools.stream_anthropic_events(ToolRecoveryRequest::default(), DialectSet::built_in());
/// let mut events = Vec::new();
/// let (first, rest) = log.as_bytes().split_at(40); // any split gives the same result
/// stream.feed(first, &mut events);
/// stream.feed(rest, &mut events);
/// let result = stream.finish(&mut events);
/// assert!(result.is_clean());
/// let calls = result.usable();
/// assert_eq!(calls[0].tool.as_deref(), Some("ls"));
/// assert_eq!(calls[0].arguments.to_json(), r#"{"depth":2}"#);
/// ```
pub struct AnthropicEventStream<'s> {
    stream: ToolCallStream<'s>,
    log: ScratchVec<u8>,
    data: ScratchVec<u8>,
    faults: ScratchVec<EventFault>,
    blocks: ScratchVec<BlockSpan>,
    /// The closers owed by the open tool input's unclosed containers.
    closers: ScratchVec<u8>,
    lex: Lex,
    /// How far the open tool input's top-level object has come.
    input: Input,
    /// Whether the open tool input was already reported as malformed.
    input_faulted: bool,
    /// What the event block being read has so far.
    lines: Lines,
    line_start: usize,
    skip_lf: bool,
    /// The offset of the line terminator being handled.
    at: usize,
    events: usize,
    current: usize,
    phase: Phase,
    open: Option<Open>,
    next_index: u64,
    emitted: usize,
    message_len: usize,
    stop: Option<GenerationStop>,
    halted: bool,
}

impl fmt::Debug for AnthropicEventStream<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_provider_stream(formatter, "AnthropicEventStream", self.events, self.halted)
    }
}

/// The delta payload strings of a block, raw inside their quotes.
fn quoted<'t>(node: &Node, key: &str, text: &'t str) -> Option<&'t str> {
    let member = node.get(key)?;
    member.as_str()?;
    text.get(member.span.clone()?)
}

/// The raw bytes of a JSON string member between its quotes.
fn inner<'t>(node: &Node, key: &str, text: &'t str) -> Option<&'t str> {
    let whole = quoted(node, key, text)?;
    whole.get(1..whole.len().checked_sub(1)?)
}

fn index_of(node: &Node) -> Option<u64> {
    match &node.get("index")?.value {
        Value::Number(number) => number.as_str().parse().ok(),
        _ => None,
    }
}

/// The stop a recognized `stop_reason` states; `None` for any other value.
fn stop_of(node: &Node, reason: &str) -> Option<GenerationStop> {
    Some(match reason {
        "end_turn" => GenerationStop::Stop,
        "tool_use" => GenerationStop::ToolCalls,
        "max_tokens" => GenerationStop::Length,
        "refusal" => GenerationStop::ContentFilter,
        "stop_sequence" => GenerationStop::StopSequence(
            node.pointer("/delta/stop_sequence")
                .and_then(Node::as_str)
                .map_or_else(Vec::new, |sequence| sequence.as_bytes().to_vec()),
        ),
        "pause_turn" => GenerationStop::Unknown,
        _ => return None,
    })
}

impl<'s> AnthropicEventStream<'s> {
    pub(super) fn with_stream(stream: ToolCallStream<'s>) -> Self {
        let governor = stream.governor();
        Self {
            log: ScratchVec::new(governor),
            data: ScratchVec::new(governor),
            faults: ScratchVec::new(governor),
            blocks: ScratchVec::new(governor),
            closers: ScratchVec::new(governor),
            lex: Lex::Plain,
            input: Input::Unstarted,
            input_faulted: false,
            stream,
            lines: Lines::Empty,
            line_start: 0,
            skip_lf: false,
            at: 0,
            events: 0,
            current: 0,
            phase: Phase::Before,
            open: None,
            next_index: 0,
            emitted: 0,
            message_len: 0,
            stop: None,
            halted: false,
        }
    }

    provider_input!();

    /// Cancel interpretation. The first governor refusal is preserved.
    pub fn cancel(&mut self) {
        self.halt(StopReason::Cancelled);
    }

    fn halt(&mut self, reason: StopReason) {
        if !self.halted {
            self.halted = true;
            self.stream.stop_producer(reason);
        }
    }

    /// One complete line of the log, `log[start..end]` without terminator.
    fn line(
        &mut self,
        start: usize,
        end: usize,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        if start == end {
            return if self.lines == Lines::Data {
                self.dispatch(events)
            } else if core::mem::replace(&mut self.lines, Lines::Empty) == Lines::Fields {
                self.current = self.events;
                self.events += 1;
                self.fault(EventFaultKind::MissingData, None)
            } else {
                Ok(())
            };
        }
        let line = &self.log.as_slice()[start..end];
        let colon = line.iter().position(|&byte| byte == b':');
        if colon == Some(0) {
            return Ok(()); // a comment line, such as a keep-alive
        }
        let name_end = colon.map_or(end, |colon| start + colon);
        let is_data = &self.log.as_slice()[start..name_end] == b"data";
        let mut value = colon.map_or(end, |colon| start + colon + 1);
        if value < end && self.log.as_slice()[value] == b' ' {
            value += 1;
        }
        if !is_data {
            self.lines = self.lines.max(Lines::Fields);
            return Ok(()); // `event`, `id`, `retry`: the data's own `type` governs
        }
        if self.lines == Lines::Data {
            self.data.push(b'\n')?;
        }
        self.lines = Lines::Data;
        self.data
            .extend(self.log.as_slice()[value..end].iter().copied())
    }

    fn dispatch(&mut self, events: &mut impl Extend<StreamEvent>) -> Result<(), StopReason> {
        let mut data = core::mem::replace(&mut self.data, ScratchVec::new(self.stream.governor()));
        self.lines = Lines::Empty;
        self.current = self.events;
        self.events += 1;
        let result = self.event(data.as_slice(), events);
        data.clear();
        self.data = data;
        result
    }

    fn event(
        &mut self,
        data: &[u8],
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        self.stream
            .governor_mut()
            .charge(Resource::Work, size(data.len()).saturating_add(1))?;
        let Ok(text) = core::str::from_utf8(data) else {
            return self.fault(EventFaultKind::NotUtf8, None);
        };
        let document = match OwnedDocument::parse(text, self.stream.governor_mut()) {
            Ok(document) => document,
            Err(OwnedParseFailure::Resource(reason)) => return Err(reason),
            Err(OwnedParseFailure::Syntax(_)) => return self.fault(EventFaultKind::NotJson, None),
        };
        self.apply(document.root(), text, events)
    }

    fn fault(&mut self, kind: EventFaultKind, block: Option<u64>) -> Result<(), StopReason> {
        let fault = EventFault {
            kind,
            event: self.current,
            at: self.at,
            block,
        };
        push_scratch(&mut self.faults, fault, self.stream.governor_mut())
    }

    fn apply(
        &mut self,
        root: &Node,
        text: &str,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        let Some(kind) = root.get("type").and_then(Node::as_str) else {
            return self.fault(EventFaultKind::MissingMember("type"), None);
        };
        if self.phase == Phase::Ended {
            return self.fault(EventFaultKind::EventAfterMessageStop, None);
        }
        if self.phase == Phase::Before && !matches!(kind, "message_start" | "ping" | "error") {
            self.fault(EventFaultKind::MessageStartMissing, None)?;
            self.phase = Phase::Streaming;
        }
        match kind {
            "ping" => Ok(()),
            "message_start" => {
                if self.phase == Phase::Before {
                    self.phase = Phase::Streaming;
                    Ok(())
                } else {
                    self.fault(EventFaultKind::MessageStartRepeated, None)
                }
            }
            "content_block_start" => self.block_start(root, text, events),
            "content_block_delta" => self.block_delta(root, text, events),
            "content_block_stop" => self.block_stop(root, events),
            "message_delta" => {
                let reason = root.pointer("/delta/stop_reason").and_then(Node::as_str);
                match reason {
                    None => self.fault(EventFaultKind::MissingMember("delta.stop_reason"), None),
                    Some(reason) => {
                        if let Some(stop) = stop_of(root, reason) {
                            if stop == GenerationStop::Length {
                                self.fault(EventFaultKind::LengthStop, None)?;
                            }
                            self.stop = Some(stop);
                            Ok(())
                        } else {
                            self.stop = Some(GenerationStop::Unknown);
                            self.fault(EventFaultKind::UnrecognizedStopReason, None)
                        }
                    }
                }
            }
            "message_stop" => {
                if let Some(index) = self.open.as_ref().map(|open| open.index) {
                    self.fault(EventFaultKind::OpenBlock, Some(index))?;
                    self.close(events)?;
                }
                self.phase = Phase::Ended;
                Ok(())
            }
            "error" => self.fault(EventFaultKind::ProviderError, None),
            _ => self.fault(EventFaultKind::UnsupportedEvent, None),
        }
    }

    /// Send message bytes to the recognizer, opening the message first.
    fn put(&mut self, bytes: &[u8], events: &mut impl Extend<StreamEvent>) {
        self.begin(events);
        self.message_len += bytes.len();
        self.stream.feed(bytes, events);
    }

    fn begin(&mut self, events: &mut impl Extend<StreamEvent>) {
        if self.message_len == 0 {
            self.message_len += MESSAGE_OPEN.len();
            self.stream.feed(MESSAGE_OPEN, events);
        }
    }

    fn block_start(
        &mut self,
        root: &Node,
        text: &str,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        let Some(index) = index_of(root) else {
            return self.fault(EventFaultKind::MissingMember("index"), None);
        };
        let Some(block) = root.get("content_block") else {
            return self.fault(EventFaultKind::MissingMember("content_block"), Some(index));
        };
        if let Some(previous) = self.open.as_ref().map(|open| open.index) {
            self.fault(EventFaultKind::BlockStartWhileOpen, Some(previous))?;
            self.close(events)?;
        }
        if index != self.next_index {
            self.fault(EventFaultKind::BlockIndexSequence, Some(index))?;
        }
        self.next_index = index.saturating_add(1);
        self.closers.clear();
        self.lex = Lex::Plain;
        self.input = Input::Unstarted;
        self.input_faulted = false;
        let kind = match block.get("type").and_then(Node::as_str) {
            Some("tool_use") => Kind::ToolUse,
            Some("text") => Kind::Text,
            Some("thinking") => Kind::Thinking,
            Some(_) => {
                self.fault(EventFaultKind::UnsupportedBlock, Some(index))?;
                Kind::Skipped
            }
            None => {
                self.fault(
                    EventFaultKind::MissingMember("content_block.type"),
                    Some(index),
                )?;
                Kind::Skipped
            }
        };
        self.begin(events);
        let start = self.message_len + usize::from(self.emitted > 0);
        if kind != Kind::Skipped {
            if self.emitted > 0 {
                self.put(b",", events);
            }
            self.emitted += 1;
        }
        match kind {
            Kind::ToolUse => {
                self.put(br#"{"type":"tool_use""#, events);
                for (member, key) in [("id", "content_block.id"), ("name", "content_block.name")] {
                    if let Some(raw) = quoted(block, member, text) {
                        self.put(format!(",\"{member}\":").as_bytes(), events);
                        self.put(raw.as_bytes(), events);
                    } else {
                        self.fault(EventFaultKind::MissingMember(key), Some(index))?;
                    }
                }
                self.put(br#","input":"#, events);
                let empty = match block.get("input").map(|input| &input.value) {
                    None => true,
                    Some(Value::Object(members)) => members.is_empty(),
                    Some(_) => false,
                };
                if !empty {
                    self.fault(EventFaultKind::NonEmptyStartInput, Some(index))?;
                }
            }
            Kind::Text | Kind::Thinking => {
                let (head, key): (&[u8], _) = if kind == Kind::Text {
                    (br#"{"type":"text","text":""#, "text")
                } else {
                    (br#"{"type":"thinking","thinking":""#, "thinking")
                };
                self.put(head, events);
                if let Some(raw) = inner(block, key, text) {
                    self.put(raw.as_bytes(), events);
                }
            }
            Kind::Skipped => {}
        }
        self.open = Some(Open {
            index,
            kind,
            wrote: false,
            signature: false,
            start,
        });
        Ok(())
    }

    fn block_delta(
        &mut self,
        root: &Node,
        text: &str,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        let Some(index) = index_of(root) else {
            return self.fault(EventFaultKind::MissingMember("index"), None);
        };
        let Some(delta) = root.get("delta") else {
            return self.fault(EventFaultKind::MissingMember("delta"), Some(index));
        };
        let Some(kind) = delta.get("type").and_then(Node::as_str) else {
            return self.fault(EventFaultKind::MissingMember("delta.type"), Some(index));
        };
        let Some((block, signature)) = self
            .open
            .as_ref()
            .filter(|open| open.index == index)
            .map(|open| (open.kind, open.signature))
        else {
            let stray = if index < self.next_index {
                EventFaultKind::DeltaAfterStop
            } else {
                EventFaultKind::DeltaWithoutStart
            };
            return self.fault(stray, Some(index));
        };
        match (kind, block) {
            (_, Kind::Skipped) => Ok(()),
            ("input_json_delta", Kind::ToolUse) => {
                let Some(fragment) = delta.get("partial_json").and_then(Node::as_str) else {
                    return self.fault(
                        EventFaultKind::MissingMember("delta.partial_json"),
                        Some(index),
                    );
                };
                if !fragment.is_empty() {
                    self.scan(fragment.as_bytes(), index)?;
                    self.put(fragment.as_bytes(), events);
                    self.mark_wrote();
                }
                Ok(())
            }
            ("text_delta", Kind::Text) | ("thinking_delta", Kind::Thinking) if !signature => {
                let key = if kind == "text_delta" {
                    "text"
                } else {
                    "thinking"
                };
                match inner(delta, key, text) {
                    Some(raw) => {
                        self.put(raw.as_bytes(), events);
                        Ok(())
                    }
                    None => self.fault(
                        EventFaultKind::MissingMember(if key == "text" {
                            "delta.text"
                        } else {
                            "delta.thinking"
                        }),
                        Some(index),
                    ),
                }
            }
            ("signature_delta", Kind::Thinking) => {
                let Some(raw) = inner(delta, "signature", text) else {
                    return self.fault(
                        EventFaultKind::MissingMember("delta.signature"),
                        Some(index),
                    );
                };
                if !signature {
                    self.put(br#"","signature":""#, events);
                    if let Some(open) = self.open.as_mut() {
                        open.signature = true;
                    }
                }
                self.put(raw.as_bytes(), events);
                Ok(())
            }
            ("input_json_delta" | "text_delta" | "thinking_delta" | "signature_delta", _) => {
                self.fault(EventFaultKind::MismatchedDelta, Some(index))
            }
            _ => self.fault(EventFaultKind::UnsupportedEvent, Some(index)),
        }
    }

    /// Track the containers and string a tool input's fragments leave open, so
    /// a block that ends early can still be closed in place, and fault input
    /// that is not one well-formed value (see [`EventFaultKind::MalformedInput`]).
    fn scan(&mut self, fragment: &[u8], index: u64) -> Result<(), StopReason> {
        self.stream
            .governor_mut()
            .charge(Resource::Work, size(fragment.len()))?;
        let mut malformed = false;
        for &byte in fragment {
            let space = matches!(byte, b' ' | b'\t' | b'\n' | b'\r');
            if self.input == Input::Unstarted && !space {
                self.input = Input::Open;
                // A tool input is always a JSON object.
                malformed |= byte != b'{';
            }
            if self.input == Input::Closed && self.lex == Lex::Plain && !space {
                malformed = true;
            }
            self.lex = match (self.lex, byte) {
                (Lex::InString, b'\\') => Lex::Escape,
                (Lex::InString, b'"') => Lex::Plain,
                (Lex::Escape | Lex::InString, _) | (Lex::Plain, b'"') => Lex::InString,
                (Lex::Plain, b'{') => {
                    self.closers.push(b'}')?;
                    Lex::Plain
                }
                (Lex::Plain, b'[') => {
                    self.closers.push(b']')?;
                    Lex::Plain
                }
                (Lex::Plain, b'}' | b']') => {
                    // Nothing open, or the wrong kind of closer; the owed closer
                    // stays, so an in-place close is still complete.
                    if self.closers.as_slice().last() == Some(&byte) {
                        self.closers.pop();
                        if self.closers.is_empty() {
                            self.input = Input::Closed;
                        }
                    } else {
                        malformed = true;
                    }
                    Lex::Plain
                }
                (Lex::Plain, _) => Lex::Plain,
            };
        }
        if malformed && !core::mem::replace(&mut self.input_faulted, true) {
            self.fault(EventFaultKind::MalformedInput, Some(index))?;
        }
        Ok(())
    }

    const fn mark_wrote(&mut self) {
        if let Some(open) = self.open.as_mut() {
            open.wrote = true;
        }
    }

    fn block_stop(
        &mut self,
        root: &Node,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        let Some(index) = index_of(root) else {
            return self.fault(EventFaultKind::MissingMember("index"), None);
        };
        if let Some(open) = self.open.as_ref().filter(|open| open.index == index) {
            if open.kind == Kind::ToolUse
                && open.wrote
                && (self.lex != Lex::Plain || !self.closers.is_empty())
            {
                self.fault(EventFaultKind::UnfinishedInput, Some(index))?;
            }
            self.close(events)
        } else {
            self.fault(EventFaultKind::StopWithoutOpenBlock, Some(index))
        }
    }

    /// Close the open block with its own closers and record its span.
    fn close(&mut self, events: &mut impl Extend<StreamEvent>) -> Result<(), StopReason> {
        let Some(open) = self.open.take() else {
            return Ok(());
        };
        match open.kind {
            Kind::Skipped => return Ok(()),
            Kind::ToolUse => {
                if open.wrote {
                    // Append what the fragments owe. Only a forced close or an
                    // unfinished input owes anything, and both are faulted
                    // (clean stops of finished input append nothing).
                    match self.lex {
                        Lex::Plain => {}
                        Lex::InString => self.put(b"\"", events),
                        Lex::Escape => self.put(b"\\\"", events),
                    }
                    while let Some(closer) = self.closers.pop() {
                        self.put(&[closer], events);
                    }
                } else {
                    self.put(b"{}", events);
                }
                self.put(b"}", events);
            }
            Kind::Text => self.put(br#""}"#, events),
            Kind::Thinking => {
                if !open.signature {
                    self.put(br#"","signature":""#, events);
                }
                self.put(br#""}"#, events);
            }
        }
        let span = BlockSpan {
            block: open.index,
            span: open.start..self.message_len,
            tool_use: open.kind == Kind::ToolUse,
        };
        push_scratch(&mut self.blocks, span, self.stream.governor_mut())
    }

    /// Report what the end of the log leaves undone and close the message.
    fn seal(&mut self, events: &mut impl Extend<StreamEvent>) -> Result<(), StopReason> {
        self.current = self.events;
        self.at = self.log.len();
        if self.line_start < self.log.len() || self.lines != Lines::Empty {
            self.fault(EventFaultKind::PartialEvent, None)?;
        }
        if let Some(index) = self.open.as_ref().map(|open| open.index) {
            self.fault(EventFaultKind::OpenBlock, Some(index))?;
            self.close(events)?;
        }
        if self.phase != Phase::Ended {
            self.fault(EventFaultKind::MissingMessageStop, None)?;
        }
        if self.stop.is_none() {
            self.fault(EventFaultKind::MissingMessageDelta, None)?;
        }
        self.put(MESSAGE_CLOSE, events);
        Ok(())
    }

    /// Finish the log. A log that ends early is reported by its faults, and
    /// the message is closed over whatever was assembled; the stop reason is
    /// the one `message_delta` stated. A log that never stated a recognized one
    /// is read with an unknown stop and reports [`EventFaultKind::MissingMessageDelta`]
    /// or [`EventFaultKind::UnrecognizedStopReason`], so none of its calls is usable.
    ///
    /// A governor refusal or cancellation, including one raised while the
    /// message is closed, returns `Unresolved` with the reason in the receipt,
    /// with no calls, whichever stage raised it, including the final recovery.
    /// The `log` keeps every admitted byte, as the source of a halted
    /// [`ToolCallStream`] is kept.
    #[must_use]
    pub fn finish(mut self, events: &mut impl Extend<StreamEvent>) -> EventLogRecovery {
        let scope = if self.halted {
            FaultScope::Whole
        } else {
            match self.seal(events).and_then(|()| {
                self.stream.governor().check()?;
                FaultScope::prepare(
                    self.faults.as_slice().iter().map(|fault| fault.block),
                    self.stream.governor_mut(),
                )
            }) {
                Ok(scope) => scope,
                Err(reason) => {
                    self.halt(reason);
                    FaultScope::Whole
                }
            }
        };
        let Self {
            stream,
            log,
            faults,
            blocks,
            stop,
            data,
            closers,
            open,
            ..
        } = self;
        drop((data, closers, open));
        let index = |call: &ToolCall| tool_block_index(&blocks, call);
        let transport = scope.transport(&index, blocks.len());
        let recovery =
            stream.finish_provider(stop.unwrap_or(GenerationStop::Unknown), events, transport);
        EventLogRecovery {
            recovery,
            faults: faults.into_vec(),
            blocks: blocks.into_vec(),
            log: log.into_vec(),
            whole_fault: matches!(scope, FaultScope::Whole),
        }
    }
}

impl ToolSet {
    /// Start reading a streamed Anthropic Messages event log over `dialects`
    /// under `request.budget`. See [`AnthropicEventStream`].
    #[must_use]
    pub fn stream_anthropic_events<'s>(
        &'s self,
        request: ToolRecoveryRequest,
        dialects: &'s DialectSet,
    ) -> AnthropicEventStream<'s> {
        AnthropicEventStream::with_stream(self.stream_with_dialects(request, dialects))
    }

    /// Read a complete Anthropic Messages event log in one call: the result
    /// of [`Self::stream_anthropic_events`] fed `log` as one chunk, with the
    /// recognizer's events discarded. The request's own stop reason is
    /// replaced by the log's.
    ///
    /// ```
    /// use purrassist_core::tool_call::{DialectSet, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::Budget;
    /// let tools = ToolSet::compile([], Budget::default()).unwrap();
    /// let result = tools.recover_anthropic_events(b"", ToolRecoveryRequest::default(), DialectSet::built_in());
    /// // An empty log is a truncated one, never a clean empty turn.
    /// assert!(!result.is_clean());
    /// assert!(result.usable().is_empty());
    /// ```
    #[must_use]
    pub fn recover_anthropic_events(
        &self,
        log: &[u8],
        request: ToolRecoveryRequest,
        dialects: &DialectSet,
    ) -> EventLogRecovery {
        let mut stream = self.stream_anthropic_events(request, dialects);
        let mut events = DiscardEvents;
        stream.feed(log, &mut events);
        stream.finish(&mut events)
    }
}
