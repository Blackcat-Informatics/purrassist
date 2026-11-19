// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! `OpenAI` chat-completions streaming logs reassembled into tool calls.
//!
//! A chat log is the server-sent-event text of one streamed completion: a
//! series of `data:` chunks whose `choices[0].delta` carries `content` and
//! `tool_calls[i]` fragments, a chunk with a `finish_reason`, and the
//! terminating `data: [DONE]`. [`OpenAiChatStream`] reads the log in any number
//! of chunks and rebuilds the complete assistant message those deltas describe,
//! as compact JSON:
//!
//! ```text
//! {"role":"assistant","content":"TEXT","tool_calls":[CALL,CALL,...]}
//! ```
//!
//! A call is `{"id":ID,"type":"function","function":{"name":"NAME","arguments":"ARGS"}}`.
//! `ID`, `NAME` and `ARGS` concatenate the call's `id`, `function.name` and
//! `function.arguments` fragments, byte for byte inside their JSON string
//! quotes, so their escapes are those the provider wrote. The `id` member is
//! left out when the call never carried one, and `content` when there was no
//! text. The assembled message bytes are fed through the same
//! [`ToolCallStream`] as raw output, so the recovered calls, repairs, losses
//! and receipt are those of [`ToolSet::recover`] on that message with the stop
//! reason the log states. Each fragment depends only on the sequence of
//! chunks, never on how the log was cut, so every chunking gives the same
//! result.
//!
//! Calls are read strictly in index order: a call is open from its first
//! fragment until the next call starts. Its fields can arrive independently.
//! A malformed or out-of-order event records a [`ChatFault`].
//! A faulted call (or any call, when the fault belongs to the whole
//! log, such as a missing finish reason or a missing `[DONE]`) is never offered
//! by [`ChatLogRecovery::usable`], whatever the call's own outcome. A length
//! stop withholds all calls. An unfinished argument payload withholds its call
//! while preserving the arguments for batch-equivalent recovery.
//!
//! Reassembly admits source scanning, buffer growth, event parsing, and
//! argument completeness checks through the shared governor. The inner
//! [`ToolCallStream`] admits its own work on the assembled message.

use super::stream::{debug_provider_stream, usable_provider_calls};
use super::{
    DialectSet, GenerationStop, StreamEvent, ToolCall, ToolCallStream, ToolCalls,
    ToolRecoveryRequest, ToolSet,
};
use crate::contract::{
    AssistResult, Resource, ScratchCharge, ScratchString, ScratchVec, StopReason, push_scratch,
    saturating_size as size,
};
use crate::json::{Node, OwnedDocument, OwnedParseFailure, Value};
use core::fmt;
use core::ops::Range;

/// Which kind of problem a [`ChatFaultKind`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatFaultClass {
    /// A chunk that is not valid or not understood.
    Malformed,
    /// The log ended before the completion did.
    Truncated,
    /// A valid chunk in a place the protocol does not allow.
    OutOfOrder,
    /// The provider reported an error chunk.
    Provider,
}

/// What was wrong with one chunk, or with the log as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatFaultKind {
    /// The chunk data is not UTF-8. The chunk is skipped.
    NotUtf8,
    /// The chunk data is not JSON. The chunk is skipped.
    NotJson,
    /// A required member is missing or has the wrong type. The member, and the
    /// tool call fragment or chunk that holds it when it is required to place
    /// the fragment, is skipped.
    MissingMember(&'static str),
    /// An envelope member occurs more than once in the same object.
    DuplicateMember(&'static str),
    /// A choice other than index 0. The choice is skipped.
    UnsupportedChoice,
    /// A delta carrying the legacy `function_call` member. It is skipped.
    UnsupportedDelta,
    /// A tool call whose `type` is present and is not `function`. The call is
    /// still assembled, as a function call.
    UnsupportedCallType,
    /// A chunk has fields but no `data` line.
    MissingData,
    /// A chunk lists the single supported choice more than once.
    RepeatedChoice,
    /// A finish reason is not recognized by this reader.
    UnrecognizedFinishReason,
    /// A tool call ended without any `function.name` fragment. The call is
    /// still assembled, without a name.
    MissingName,
    /// The log ended inside a line or a chunk that was not closed by a blank
    /// line. The chunk is discarded, as server-sent events require.
    PartialEvent,
    /// The log ended without a chunk carrying a `finish_reason`.
    MissingFinishReason,
    /// The log ended without the terminating `data: [DONE]`.
    MissingDone,
    /// The provider ended the response at its generation limit.
    LengthStop,
    /// A call's argument JSON ended while still expecting input.
    UnfinishedArguments,
    /// A chunk after `[DONE]`. It is skipped.
    EventAfterDone,
    /// Content or tool call fragments after the chunk with the `finish_reason`.
    /// They are skipped.
    DeltaAfterFinish,
    /// A second chunk with a `finish_reason`. It is skipped.
    FinishRepeated,
    /// A call started at an index that is not the next one.
    IndexSequence,
    /// A fragment for a call that is no longer open: a later call has started.
    /// It is skipped, even if its bytes would complete the call's arguments.
    DeltaForClosedCall,
    /// A `content` fragment after the first tool call began. It is skipped.
    ContentAfterToolCalls,
    /// An `error` chunk.
    ProviderError,
}

impl ChatFaultKind {
    /// The class of this fault.
    ///
    /// ```
    /// use purrassist_core::tool_call::{ChatFaultClass, ChatFaultKind};
    /// assert_eq!(ChatFaultKind::DeltaForClosedCall.class(), ChatFaultClass::OutOfOrder);
    /// assert_eq!(ChatFaultKind::NotJson.class(), ChatFaultClass::Malformed);
    /// assert_eq!(ChatFaultKind::MissingDone.class(), ChatFaultClass::Truncated);
    /// ```
    #[must_use]
    pub const fn class(self) -> ChatFaultClass {
        match self {
            Self::NotUtf8
            | Self::NotJson
            | Self::MissingMember(_)
            | Self::DuplicateMember(_)
            | Self::UnsupportedChoice
            | Self::UnsupportedDelta
            | Self::UnsupportedCallType
            | Self::MissingData
            | Self::RepeatedChoice
            | Self::UnrecognizedFinishReason
            | Self::MissingName => ChatFaultClass::Malformed,
            Self::PartialEvent
            | Self::MissingFinishReason
            | Self::MissingDone
            | Self::LengthStop
            | Self::UnfinishedArguments => ChatFaultClass::Truncated,
            Self::EventAfterDone
            | Self::DeltaAfterFinish
            | Self::FinishRepeated
            | Self::IndexSequence
            | Self::DeltaForClosedCall
            | Self::ContentAfterToolCalls => ChatFaultClass::OutOfOrder,
            Self::ProviderError => ChatFaultClass::Provider,
        }
    }
}

/// One fault of a chat log, in the order found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChatFault {
    /// What was wrong.
    pub kind: ChatFaultKind,
    /// The position of the fault's chunk among the log's chunks
    /// (blank-line terminated), counting from zero; the chunk count for a
    /// fault found at the end of the log.
    pub event: usize,
    /// The offset in the log where the fault was found: the end of the
    /// chunk's last line, or the log length for a fault found at the end.
    pub at: usize,
    /// The `tool_calls` index the fault belongs to. `None` means the whole log:
    /// no call of such a log is offered by [`ChatLogRecovery::usable`].
    pub call: Option<u64>,
}

impl ScratchCharge for ChatFault {
    const CHARGE: usize = 64;
}
const _: () = assert!(core::mem::size_of::<ChatFault>() <= ChatFault::CHARGE);

/// The range of the reassembled message that one tool call occupies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatCallSpan {
    /// The call's `tool_calls` index in the log.
    pub call: u64,
    /// The call's bytes in the reassembled message (the receipt's source).
    pub span: Range<usize>,
}

impl ScratchCharge for ChatCallSpan {
    const CHARGE: usize = 24;
}
const _: () = assert!(core::mem::size_of::<ChatCallSpan>() <= ChatCallSpan::CHARGE);

/// The result of reading one chat-completions log.
///
/// `recovery` is the ordinary recovery result of the reassembled message; its
/// receipt retains that message as its source. Do not select calls from
/// `recovery` with [`ToolCalls::usable`] when `faults` is not empty: use
/// [`Self::usable`], which honors the faults. A call is never reported as
/// resolved because its fragments were repaired or guessed: the call's own
/// outcome is unchanged.
#[derive(Debug)]
pub struct ChatLogRecovery {
    /// The recovery of the reassembled message.
    pub recovery: AssistResult<ToolCalls>,
    /// Every fault of the log, in the order found. Empty for a clean log.
    pub faults: Vec<ChatFault>,
    /// The tool calls that were assembled into the message, in order.
    pub calls: Vec<ChatCallSpan>,
    /// The original log, up to the last byte admitted before refusal or
    /// cancellation. The receipt's `stopped` reports why admission ended.
    pub log: Vec<u8>,
}

impl ChatLogRecovery {
    /// The `tool_calls` index of the log's call `call` was read from.
    #[must_use]
    pub fn index_of(&self, call: &ToolCall) -> Option<u64> {
        let position = self
            .calls
            .partition_point(|entry| entry.span.end <= call.span.start);
        self.calls
            .get(position)
            .filter(|entry| entry.span.contains(&call.span.start))
            .map(|entry| entry.call)
    }

    /// Whether the log had no fault.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.faults.is_empty()
    }

    fn permitted<'a>(&self, calls: Vec<&'a ToolCall>) -> Vec<&'a ToolCall> {
        let Some(faulted) = self
            .faults
            .iter()
            .map(|fault| fault.call)
            .collect::<Option<std::collections::BTreeSet<_>>>()
        else {
            return Vec::new();
        };
        calls
            .into_iter()
            .filter(|call| {
                self.index_of(call)
                    .is_some_and(|index| !faulted.contains(&index))
            })
            .collect()
    }

    /// The calls that may run as is: [`ToolCalls::usable`] without the calls
    /// of any faulted index, and none at all when a fault belongs to the whole
    /// log (a log cut short, a provider error, an unreadable chunk).
    #[must_use]
    pub fn usable(&self) -> Vec<&ToolCall> {
        usable_provider_calls(&self.recovery.outcome, ToolCalls::usable, |calls| {
            self.permitted(calls)
        })
    }

    /// Like [`Self::usable`], built on [`ToolCalls::usable_accepting_guesses`].
    #[must_use]
    pub fn usable_accepting_guesses(&self) -> Vec<&ToolCall> {
        usable_provider_calls(
            &self.recovery.outcome,
            ToolCalls::usable_accepting_guesses,
            |calls| self.permitted(calls),
        )
    }
}

/// How much of the message has been written, which fixes what may follow.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    /// Nothing yet.
    Fresh,
    /// Inside the `content` string.
    Content,
    /// Inside the `tool_calls` array.
    Calls,
}

/// Fragments are retained until the call can be emitted in canonical order.
/// The provider can send more ID bytes after the arguments begin.
struct Open {
    index: u64,
    id: Option<ScratchVec<u8>>,
    name: ScratchVec<u8>,
    arguments: ScratchVec<u8>,
    decoded_arguments: ScratchString,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lines {
    Empty,
    Fields,
    Data,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Streaming,
    Ended,
}

/// An incremental reader of one `OpenAI` chat-completions streaming log.
///
/// Feed the log's bytes in any chunks, then [`Self::finish`]. Refusal by the
/// governor, or [`Self::cancel`], stops interpretation; the result is then
/// `Unresolved` with the reason in its receipt and the admitted log bytes.
///
/// ```
/// use purrassist_core::tool_call::{DialectSet, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
/// use purrassist_core::Budget;
/// let tools = ToolSet::compile(
///     [ToolDefinition::new("ls").param("depth", Param::integer())],
///     Budget::default(),
/// ).unwrap();
/// let log = concat!(
///     "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
///     "\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"ls\",\"arguments\":\"\"}}]}}]}\n\n",
///     "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
///     "\"function\":{\"arguments\":\"{\\\"depth\\\": 2}\"}}]}}]}\n\n",
///     "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
///     "data: [DONE]\n\n",
/// );
/// let mut stream = tools.stream_openai_chat(ToolRecoveryRequest::default(), DialectSet::built_in());
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
pub struct OpenAiChatStream<'s> {
    stream: ToolCallStream<'s>,
    log: ScratchVec<u8>,
    data: ScratchVec<u8>,
    faults: ScratchVec<ChatFault>,
    spans: ScratchVec<ChatCallSpan>,
    lines: Lines,
    line_start: usize,
    skip_lf: bool,
    /// The offset of the line terminator being handled.
    at: usize,
    events: usize,
    current: usize,
    phase: Phase,
    part: Part,
    open: Option<Open>,
    next_index: u64,
    message_len: usize,
    finish: Option<GenerationStop>,
    halted: bool,
}

impl fmt::Debug for OpenAiChatStream<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_provider_stream(formatter, "OpenAiChatStream", self.events, self.halted)
    }
}

/// The raw bytes of a JSON string member between its quotes.
fn inner<'t>(node: &Node, key: &str, text: &'t str) -> Option<&'t str> {
    let member = node.get(key)?;
    member.as_str()?;
    let whole = text.get(member.span.clone()?)?;
    whole.get(1..whole.len().checked_sub(1)?)
}

/// A member that is present and not `null`.
fn present<'n>(node: &'n Node, key: &str) -> Option<&'n Node> {
    node.get(key)
        .filter(|member| !matches!(member.value, Value::Null))
}

/// Check recognized envelope fields before looking them up. The JSON tree's
/// unique-member lookup otherwise treats a duplicate as an absent field.
fn duplicate_member(node: &Node, keys: &[&'static str]) -> Option<&'static str> {
    let Value::Object(members) = &node.value else {
        return None;
    };
    keys.iter().copied().find(|key| {
        members
            .iter()
            .filter(|member| member.key == *key)
            .nth(1)
            .is_some()
    })
}

fn number_of(node: &Node, key: &str) -> Option<u64> {
    match &node.get(key)?.value {
        Value::Number(number) => number.as_str().parse().ok(),
        _ => None,
    }
}

fn stop_of(reason: &str) -> Option<GenerationStop> {
    Some(match reason {
        "stop" => GenerationStop::Stop,
        "tool_calls" | "function_call" => GenerationStop::ToolCalls,
        "length" => GenerationStop::Length,
        "content_filter" => GenerationStop::ContentFilter,
        _ => return None,
    })
}

impl<'s> OpenAiChatStream<'s> {
    fn new(tools: &'s ToolSet, request: ToolRecoveryRequest, dialects: &'s DialectSet) -> Self {
        let stream = tools.stream_with_dialects(request, dialects);
        let governor = stream.governor();
        Self {
            log: ScratchVec::new(governor),
            data: ScratchVec::new(governor),
            faults: ScratchVec::new(governor),
            spans: ScratchVec::new(governor),
            stream,
            lines: Lines::Empty,
            line_start: 0,
            skip_lf: false,
            at: 0,
            events: 0,
            current: 0,
            phase: Phase::Streaming,
            part: Part::Fresh,
            open: None,
            next_index: 0,
            message_len: 0,
            finish: None,
            halted: false,
        }
    }

    /// Feed log bytes, which may end anywhere (inside a line, a multi-byte
    /// character, or a CR LF pair), and append the recognizer's newly settled
    /// [`StreamEvent`]s to `events`. Their spans are offsets in the
    /// reassembled message, not in the log. Empty feeds charge nothing.
    ///
    /// Governor refusal and cancellation stop interpretation; they are
    /// reported by [`Self::finish`], never by panic or a partial result.
    pub fn feed(&mut self, bytes: &[u8], events: &mut impl Extend<StreamEvent>) {
        if self.halted || bytes.is_empty() {
            return;
        }
        if let Err(reason) = self.take(bytes, events) {
            self.halt(reason);
        }
    }

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

    fn take(
        &mut self,
        bytes: &[u8],
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        for &byte in bytes {
            let at = self.log.len();
            let governor = self.stream.governor_mut();
            governor.charge(Resource::InputBytes, size(at).saturating_add(1))?;
            governor.charge(
                Resource::Work,
                self.log.compact_relocation_work().saturating_add(2),
            )?;
            self.log.reserve_compact(1)?;
            self.log.push(byte)?;
            if core::mem::take(&mut self.skip_lf) && byte == b'\n' {
                self.line_start = at + 1;
                continue;
            }
            if byte == b'\n' || byte == b'\r' {
                self.skip_lf = byte == b'\r';
                let start = core::mem::replace(&mut self.line_start, at + 1);
                self.at = at;
                self.line(start, at, events)?;
            }
        }
        Ok(())
    }

    /// One complete line of the log, `log[start..end]` without terminator.
    fn line(
        &mut self,
        start: usize,
        end: usize,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        if start == end {
            return match core::mem::replace(&mut self.lines, Lines::Empty) {
                Lines::Empty => Ok(()),
                Lines::Fields => {
                    self.current = self.events;
                    self.events += 1;
                    self.fault(ChatFaultKind::MissingData, None)
                }
                Lines::Data => self.dispatch(events),
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
            if self.lines == Lines::Empty {
                self.lines = Lines::Fields;
            }
            return Ok(());
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
        // Validation scans the event and the retained fragments can incur up
        // to six additional byte moves, including decoded text and relocation.
        self.stream.governor_mut().charge(
            Resource::Work,
            size(data.len()).saturating_mul(7).saturating_add(1),
        )?;
        if self.phase == Phase::Ended {
            return self.fault(ChatFaultKind::EventAfterDone, None);
        }
        if data == b"[DONE]" {
            self.phase = Phase::Ended;
            return Ok(());
        }
        let Ok(text) = core::str::from_utf8(data) else {
            return self.fault(ChatFaultKind::NotUtf8, None);
        };
        let document = match OwnedDocument::parse(text, self.stream.governor_mut()) {
            Ok(document) => document,
            Err(OwnedParseFailure::Resource(reason)) => return Err(reason),
            Err(OwnedParseFailure::Syntax(_)) => return self.fault(ChatFaultKind::NotJson, None),
        };
        self.chunk(document.root(), text, events)
    }

    fn fault(&mut self, kind: ChatFaultKind, call: Option<u64>) -> Result<(), StopReason> {
        let fault = ChatFault {
            kind,
            event: self.current,
            at: self.at,
            call,
        };
        push_scratch(&mut self.faults, fault, self.stream.governor_mut())
    }

    fn chunk(
        &mut self,
        root: &Node,
        text: &str,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        if let Some(key) = duplicate_member(root, &["error", "choices"]) {
            return self.fault(ChatFaultKind::DuplicateMember(key), None);
        }
        if present(root, "error").is_some() {
            return self.fault(ChatFaultKind::ProviderError, None);
        }
        let Some(Value::Array(choices)) = root.get("choices").map(|node| &node.value) else {
            return self.fault(ChatFaultKind::MissingMember("choices"), None);
        };
        let mut seen_choice = false;
        for choice in choices {
            self.choice(choice, text, &mut seen_choice, events)?;
        }
        Ok(())
    }

    fn choice(
        &mut self,
        choice: &Node,
        text: &str,
        seen_choice: &mut bool,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        if let Some(key) = duplicate_member(choice, &["index", "delta", "finish_reason"]) {
            return self.fault(ChatFaultKind::DuplicateMember(key), None);
        }
        match number_of(choice, "index") {
            Some(0) => {}
            Some(_) => return self.fault(ChatFaultKind::UnsupportedChoice, None),
            None => return self.fault(ChatFaultKind::MissingMember("choices.index"), None),
        }
        if core::mem::replace(seen_choice, true) {
            return self.fault(ChatFaultKind::RepeatedChoice, None);
        }
        let finish = present(choice, "finish_reason");
        match present(choice, "delta") {
            Some(delta) => self.delta(delta, text, events)?,
            None if finish.is_none() => {
                self.fault(ChatFaultKind::MissingMember("choices.delta"), None)?;
            }
            None => {}
        }
        if let Some(reason) = finish {
            self.finish_reason(reason)?;
        }
        Ok(())
    }

    fn finish_reason(&mut self, reason: &Node) -> Result<(), StopReason> {
        let Some(reason) = reason.as_str() else {
            return self.fault(ChatFaultKind::MissingMember("choices.finish_reason"), None);
        };
        if self.finish.is_some() {
            return self.fault(ChatFaultKind::FinishRepeated, None);
        }
        if let Some(stop) = stop_of(reason) {
            if stop == GenerationStop::Length {
                self.fault(ChatFaultKind::LengthStop, None)?;
            }
            self.finish = Some(stop);
            Ok(())
        } else {
            self.fault(ChatFaultKind::UnrecognizedFinishReason, None)
        }
    }

    fn delta(
        &mut self,
        delta: &Node,
        text: &str,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        if !matches!(delta.value, Value::Object(_)) {
            return self.fault(ChatFaultKind::MissingMember("choices.delta"), None);
        }
        if let Some(key) = duplicate_member(delta, &["content", "function_call", "tool_calls"]) {
            return self.fault(ChatFaultKind::DuplicateMember(key), None);
        }
        if present(delta, "content").is_some() {
            match inner(delta, "content", text) {
                None => self.fault(ChatFaultKind::MissingMember("delta.content"), None)?,
                Some("") => {}
                Some(raw) => self.content(raw, events)?,
            }
        }
        if present(delta, "function_call").is_some() {
            self.fault(ChatFaultKind::UnsupportedDelta, None)?;
        }
        if let Some(calls) = present(delta, "tool_calls") {
            let Value::Array(elements) = &calls.value else {
                return self.fault(ChatFaultKind::MissingMember("delta.tool_calls"), None);
            };
            for element in elements {
                self.tool_call(element, text, events)?;
            }
        }
        Ok(())
    }

    /// Send message bytes to the recognizer.
    fn put(
        &mut self,
        bytes: &[u8],
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        self.stream.feed(bytes, events);
        self.stream.governor().check()?;
        self.message_len += bytes.len();
        Ok(())
    }

    fn content(
        &mut self,
        raw: &str,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        if self.finish.is_some() {
            return self.fault(ChatFaultKind::DeltaAfterFinish, None);
        }
        if self.open.is_some() {
            return self.fault(ChatFaultKind::ContentAfterToolCalls, None);
        }
        match self.part {
            Part::Fresh => {
                self.put(br#"{"role":"assistant","content":""#, events)?;
                self.part = Part::Content;
            }
            Part::Content => {}
            Part::Calls => return self.fault(ChatFaultKind::ContentAfterToolCalls, None),
        }
        self.put(raw.as_bytes(), events)
    }

    fn tool_call(
        &mut self,
        element: &Node,
        text: &str,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        if self.finish.is_some() {
            return self.fault(ChatFaultKind::DeltaAfterFinish, None);
        }
        let Some(index) = number_of(element, "index") else {
            let kind = if duplicate_member(element, &["index"]).is_some() {
                ChatFaultKind::DuplicateMember("index")
            } else {
                ChatFaultKind::MissingMember("tool_calls.index")
            };
            return self.fault(kind, None);
        };
        if let Some(key) = duplicate_member(element, &["id", "type", "function"]) {
            return self.fault(ChatFaultKind::DuplicateMember(key), Some(index));
        }
        let function = present(element, "function");
        if let Some(key) =
            function.and_then(|function| duplicate_member(function, &["name", "arguments"]))
        {
            return self.fault(ChatFaultKind::DuplicateMember(key), Some(index));
        }
        if self.open.as_ref().is_none_or(|open| open.index != index) {
            if index < self.next_index {
                return self.fault(ChatFaultKind::DeltaForClosedCall, Some(index));
            }
            self.begin(index, events)?;
        }
        self.fields(index, element, function, text)
    }

    fn begin(
        &mut self,
        index: u64,
        events: &mut impl Extend<StreamEvent>,
    ) -> Result<(), StopReason> {
        self.close(events)?;
        if index != self.next_index {
            self.fault(ChatFaultKind::IndexSequence, Some(index))?;
        }
        self.next_index = index.saturating_add(1);
        let governor = self.stream.governor();
        self.open = Some(Open {
            index,
            id: None,
            name: ScratchVec::new(governor),
            arguments: ScratchVec::new(governor),
            decoded_arguments: ScratchString::new(governor),
        });
        Ok(())
    }

    /// Apply one fragment's members to the open call.
    fn fields(
        &mut self,
        index: u64,
        element: &Node,
        function: Option<&Node>,
        text: &str,
    ) -> Result<(), StopReason> {
        if present(element, "id").is_some() {
            if let Some(raw) = inner(element, "id", text) {
                let governor = self.stream.governor();
                let open = self.open.as_mut().expect("call fragment has an open call");
                open.id
                    .get_or_insert_with(|| ScratchVec::new(governor))
                    .extend(raw.bytes())?;
            } else {
                self.fault(ChatFaultKind::MissingMember("tool_calls.id"), Some(index))?;
            }
        }
        if let Some(kind) = present(element, "type")
            && kind.as_str() != Some("function")
        {
            self.fault(ChatFaultKind::UnsupportedCallType, Some(index))?;
        }
        let Some(function) = function else {
            return Ok(());
        };
        if !matches!(function.value, Value::Object(_)) {
            return self.fault(ChatFaultKind::MissingMember("function"), Some(index));
        }
        if present(function, "name").is_some() {
            match inner(function, "name", text) {
                None => self.fault(ChatFaultKind::MissingMember("function.name"), Some(index))?,
                Some(raw) => self
                    .open
                    .as_mut()
                    .expect("call fragment has an open call")
                    .name
                    .extend(raw.bytes())?,
            }
        }
        if present(function, "arguments").is_some() {
            if let Some(raw) = inner(function, "arguments", text) {
                let decoded = function
                    .get("arguments")
                    .and_then(Node::as_str)
                    .expect("a source string fragment is a parsed string");
                let open = self.open.as_mut().expect("call fragment has an open call");
                open.arguments.extend(raw.bytes())?;
                open.decoded_arguments.push_str(decoded)?;
            } else {
                self.fault(
                    ChatFaultKind::MissingMember("function.arguments"),
                    Some(index),
                )?;
            }
        }
        Ok(())
    }

    /// Emit the finished call in the same member order as the batch message.
    fn close(&mut self, events: &mut impl Extend<StreamEvent>) -> Result<(), StopReason> {
        let Some(open) = self.open.take() else {
            return Ok(());
        };
        if open.name.is_empty() {
            self.fault(ChatFaultKind::MissingName, Some(open.index))?;
        }
        match OwnedDocument::parse(open.decoded_arguments.as_str(), self.stream.governor_mut()) {
            Err(OwnedParseFailure::Syntax(error)) if error.at_end => {
                self.fault(ChatFaultKind::UnfinishedArguments, Some(open.index))?;
            }
            Err(OwnedParseFailure::Resource(reason)) => return Err(reason),
            _ => {}
        }
        drop(open.decoded_arguments);
        match self.part {
            Part::Fresh => self.put(br#"{"role":"assistant","tool_calls":["#, events)?,
            Part::Content => self.put(br#"","tool_calls":["#, events)?,
            Part::Calls => self.put(b",", events)?,
        }
        self.part = Part::Calls;
        let start = self.message_len;
        self.put(b"{", events)?;
        if let Some(id) = open.id {
            self.put(br#""id":""#, events)?;
            self.put(id.as_slice(), events)?;
            self.put(br#"","#, events)?;
        }
        self.put(br#""type":"function","function":{"name":""#, events)?;
        self.put(open.name.as_slice(), events)?;
        self.put(br#"","arguments":""#, events)?;
        self.put(open.arguments.as_slice(), events)?;
        self.put(br#""}}"#, events)?;
        let span = ChatCallSpan {
            call: open.index,
            span: start..self.message_len,
        };
        push_scratch(&mut self.spans, span, self.stream.governor_mut())
    }

    /// Report what the end of the log leaves undone and close the message.
    fn seal(&mut self, events: &mut impl Extend<StreamEvent>) -> Result<(), StopReason> {
        self.current = self.events;
        self.at = self.log.len();
        if self.line_start < self.log.len() || self.lines != Lines::Empty {
            self.fault(ChatFaultKind::PartialEvent, None)?;
        }
        if self.phase != Phase::Ended {
            self.fault(ChatFaultKind::MissingDone, None)?;
        }
        if self.finish.is_none() {
            self.fault(ChatFaultKind::MissingFinishReason, None)?;
        }
        self.close(events)?;
        match self.part {
            Part::Fresh => self.put(br#"{"role":"assistant"}"#, events),
            Part::Content => self.put(br#""}"#, events),
            Part::Calls => self.put(b"]}", events),
        }
    }

    /// Finish the log. A log that ends early is reported by its faults, and
    /// the message is closed over whatever was assembled; the stop reason is
    /// the one the `finish_reason` chunk stated, or unknown when none arrived.
    ///
    /// A governor refusal or cancellation returns `Unresolved` with the reason
    /// in the receipt and no usable calls. The admitted prefix remains in `log`.
    #[must_use]
    pub fn finish(mut self, events: &mut impl Extend<StreamEvent>) -> ChatLogRecovery {
        if !self.halted
            && let Err(reason) = self.seal(events)
        {
            self.halt(reason);
        }
        let Self {
            stream,
            log,
            faults,
            spans,
            finish,
            data,
            open,
            ..
        } = self;
        drop(data);
        drop(open);
        let recovery = stream.finish_with_events(finish.unwrap_or(GenerationStop::Unknown), events);
        ChatLogRecovery {
            recovery,
            faults: faults.into_vec(),
            calls: spans.into_vec(),
            log: log.into_vec(),
        }
    }
}

impl ToolSet {
    /// Start reading a streamed `OpenAI` chat-completions log over `dialects`
    /// under `request.budget`. See [`OpenAiChatStream`].
    #[must_use]
    pub fn stream_openai_chat<'s>(
        &'s self,
        request: ToolRecoveryRequest,
        dialects: &'s DialectSet,
    ) -> OpenAiChatStream<'s> {
        OpenAiChatStream::new(self, request, dialects)
    }

    /// Read a complete `OpenAI` chat-completions log in one call: the result of
    /// [`Self::stream_openai_chat`] fed `log` as one chunk, with the
    /// recognizer's events discarded. The request's own stop reason is
    /// replaced by the log's.
    ///
    /// ```
    /// use purrassist_core::tool_call::{DialectSet, ToolRecoveryRequest, ToolSet};
    /// use purrassist_core::Budget;
    /// let tools = ToolSet::compile([], Budget::default()).unwrap();
    /// let result = tools.recover_openai_chat(b"", ToolRecoveryRequest::default(), DialectSet::built_in());
    /// // An empty log is a truncated one, never a clean empty turn.
    /// assert!(!result.is_clean());
    /// assert!(result.usable().is_empty());
    /// ```
    #[must_use]
    pub fn recover_openai_chat(
        &self,
        log: &[u8],
        request: ToolRecoveryRequest,
        dialects: &DialectSet,
    ) -> ChatLogRecovery {
        let mut stream = self.stream_openai_chat(request, dialects);
        let mut events = DiscardEvents;
        stream.feed(log, &mut events);
        stream.finish(&mut events)
    }
}

/// Discard batch events as they are handed off by the recognizer.
struct DiscardEvents;

impl Extend<StreamEvent> for DiscardEvents {
    fn extend<T: IntoIterator<Item = StreamEvent>>(&mut self, events: T) {
        for _ in events {}
    }
}
