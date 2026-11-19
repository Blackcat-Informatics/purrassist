// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Transport admission and batch publication for provider event logs.

use super::{
    AnthropicEventStream, ChatLogRecovery, DialectSet, EventLogRecovery, OpenAiChatStream,
    StreamEvent, ToolCall, ToolCalls, ToolRecoveryRequest, ToolSet, Verdict,
};
use crate::{
    Governor, Outcome, StopReason,
    contract::{Resource, ScratchVec, saturating_size as size},
    digest::Digest,
    json::{Member, Node, Value},
    wire::{array, byte_range, exact_bytes, integer, null, object, string},
};

/// Which provider's event log is supplied to batch recovery.
///
/// ```
/// use purrassist_core::tool_call::ProviderLogFormat;
/// assert_eq!(ProviderLogFormat::OpenAiChat.code(), "openai-chat-events");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderLogFormat {
    /// Anthropic Messages server-sent events.
    Anthropic,
    /// `OpenAI` chat-completions server-sent events.
    OpenAiChat,
}
impl ProviderLogFormat {
    /// The explicit batch input format name.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic-events",
            Self::OpenAiChat => "openai-chat-events",
        }
    }
}

/// The class of a provider event log fault.
///
/// ```
/// use purrassist_core::tool_call::{ChatFaultKind, ProviderFaultClass};
/// assert_eq!(ChatFaultKind::NotJson.class(), ProviderFaultClass::Malformed);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderFaultClass {
    /// An event is invalid or unsupported.
    Malformed,
    /// The provider turn or an event ended early.
    Truncated,
    /// An event arrived in a place the protocol does not allow.
    OutOfOrder,
    /// The provider reported an error.
    Provider,
}
impl ProviderFaultClass {
    const fn code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::Truncated => "truncated",
            Self::OutOfOrder => "out_of_order",
            Self::Provider => "provider",
        }
    }
}

#[derive(Default, Debug)]
pub(super) enum FaultScope {
    Whole,
    #[default]
    Clean,
    Calls(Vec<u64>),
}
impl FaultScope {
    pub(super) fn prepare(
        faults: impl ExactSizeIterator<Item = Option<u64>>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let count = faults.len();
        if count == 0 {
            return Ok(Self::Clean);
        }
        // Index repeated and out-of-order faults without rescanning the log for
        // every recovered call. Admit the fixed-width sort before construction.
        governor.charge(
            Resource::Work,
            size(count).saturating_mul(3_u64.saturating_add(u64::from(size(count).ilog2()))),
        )?;
        let mut indices = ScratchVec::new(governor);
        indices.reserve(count)?;
        for index in faults {
            let Some(index) = index else {
                return Ok(Self::Whole);
            };
            indices.push(index)?;
        }
        let mut indices = indices.into_vec();
        indices.sort_unstable();
        indices.dedup();
        Ok(Self::Calls(indices))
    }

    pub(super) fn transport<'a>(
        &'a self,
        index: &'a dyn Fn(&ToolCall) -> Option<u64>,
        spans: usize,
    ) -> Transport<'a> {
        Transport {
            scope: self,
            index,
            spans,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Transport<'a> {
    scope: &'a FaultScope,
    index: &'a dyn Fn(&ToolCall) -> Option<u64>,
    spans: usize,
}

impl Transport<'_> {
    pub(super) const fn faulted(self) -> bool {
        !matches!(self.scope, FaultScope::Clean)
    }

    pub(super) fn allows(
        self,
        call: &ToolCall,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let index_work = u64::from(self.spans.max(1).ilog2()) + 2;
        match self.scope {
            FaultScope::Whole => {
                governor.charge(Resource::Work, 1)?;
                Ok(false)
            }
            FaultScope::Clean => {
                governor.charge(Resource::Work, index_work)?;
                Ok((self.index)(call).is_some())
            }
            FaultScope::Calls(faulted) => {
                let fault_work = u64::from(faulted.len().max(1).ilog2()) + 2;
                governor.charge(Resource::Work, index_work + fault_work)?;
                Ok((self.index)(call).is_some_and(|index| faulted.binary_search(&index).is_err()))
            }
        }
    }
}

/// Batch recovery of a provider log, with transport-aware publication.
///
/// The provider result keeps its admitted log and faults. The receipt still
/// describes the reconstructed assistant message. A setup refusal can arrive
/// before any provider bytes were admitted.
///
/// ```
/// use purrassist_core::{Budget, Governor, tool_call::{DialectSet, ProviderLogFormat, ToolRecoveryRequest, ToolSet}};
/// let tools = ToolSet::compile([], Budget::default()).unwrap();
/// let result = tools.recover_provider_log(
///     ProviderLogFormat::OpenAiChat, b"", ToolRecoveryRequest::default(),
///     DialectSet::built_in(), Governor::new(Budget::default()),
/// );
/// assert_eq!(result.exit_code(), 4);
/// assert!(result.to_node().pointer("/provider/faults/0").is_some());
/// ```
#[derive(Debug)]
pub enum ProviderLogRecovery {
    /// Anthropic Messages recovery and transport evidence.
    Anthropic(EventLogRecovery),
    /// `OpenAI` chat-completions recovery and transport evidence.
    OpenAiChat(ChatLogRecovery),
    /// Recovery preparation stopped before the provider reader could run.
    NotStarted {
        /// The requested input format.
        format: ProviderLogFormat,
        /// The original preparation stop and its receipt.
        receipt: crate::Receipt,
    },
}

impl ProviderLogRecovery {
    /// Encode recovery and provider evidence, applying the provider's
    /// eligibility decision to every published reading.
    #[must_use]
    pub fn to_node(&self) -> Node {
        match self {
            Self::Anthropic(result) => provider_node(
                result
                    .recovery
                    .to_node_with(|calls| calls.to_node_with(true)),
                ProviderLogFormat::Anthropic,
                &result.log,
                provider_verdict(&result.recovery.outcome, result.whole_fault),
                array(result.faults.iter().map(|fault| {
                    let (code, member) = fault.kind.wire_code();
                    fault_node(
                        code,
                        member,
                        fault.kind.class().code(),
                        fault.event,
                        fault.at,
                        fault.block,
                    )
                })),
                array(
                    result
                        .blocks
                        .iter()
                        .map(|block| origin(block.block, &block.span)),
                ),
            ),
            Self::OpenAiChat(result) => provider_node(
                result
                    .recovery
                    .to_node_with(|calls| calls.to_node_with(true)),
                ProviderLogFormat::OpenAiChat,
                &result.log,
                provider_verdict(&result.recovery.outcome, result.whole_fault),
                array(result.faults.iter().map(|fault| {
                    let (code, member) = fault.kind.wire_code();
                    fault_node(
                        code,
                        member,
                        fault.kind.class().code(),
                        fault.event,
                        fault.at,
                        fault.call,
                    )
                })),
                array(
                    result
                        .calls
                        .iter()
                        .map(|call| origin(call.call, &call.span)),
                ),
            ),
            Self::NotStarted { format, receipt } => provider_node(
                object([
                    ("status", string("unresolved")),
                    ("value", null()),
                    ("alternatives", array([])),
                    ("receipt", receipt.to_node()),
                ]),
                *format,
                &[],
                Verdict::NothingUsable,
                array([]),
                array([]),
            ),
        }
    }

    /// Return the recovery CLI exit code after transport withholding.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Anthropic(result) => {
                provider_verdict(&result.recovery.outcome, result.whole_fault)
            }
            Self::OpenAiChat(result) => {
                provider_verdict(&result.recovery.outcome, result.whole_fault)
            }
            Self::NotStarted { .. } => Verdict::NothingUsable,
        }
        .exit_code()
    }
}

fn provider_verdict(outcome: &Outcome<ToolCalls>, whole_fault: bool) -> Verdict {
    match outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls.verdict(),
        Outcome::Ambiguous(_) if !whole_fault => Verdict::Ambiguous,
        Outcome::Ambiguous(_) | Outcome::Unresolved => Verdict::NothingUsable,
    }
}

fn provider_node(
    mut recovery: Node,
    format: ProviderLogFormat,
    log: &[u8],
    verdict: Verdict,
    faults: Node,
    spans: Node,
) -> Node {
    let Value::Object(members) = &mut recovery.value else {
        unreachable!("recovery wire is an object");
    };
    members.push(Member {
        key: "provider".into(),
        key_span: None,
        value: object([
            ("format", string(format.code())),
            (
                "source",
                object([
                    ("digest", string(Digest::of(log).to_string())),
                    exact_bytes(log),
                ]),
            ),
            ("verdict", string(verdict.code())),
            ("faults", faults),
            ("message_spans", spans),
        ]),
    });
    recovery
}

fn fault_node(
    code: &str,
    member: Option<&str>,
    class: &str,
    event: usize,
    offset: usize,
    index: Option<u64>,
) -> Node {
    object([
        ("code", string(code)),
        ("member", member.map_or_else(null, string)),
        ("class", string(class)),
        ("event", integer(size(event))),
        ("log_offset", integer(size(offset))),
        ("index", index.map_or_else(null, integer)),
    ])
}

fn origin(index: u64, span: &core::ops::Range<usize>) -> Node {
    object([
        ("index", integer(index)),
        ("message_span", byte_range(span)),
    ])
}

impl ToolSet {
    /// Recover an event log with the caller's original governor. Its limits
    /// and accumulated usage are authoritative, including tool declaration and
    /// dialect preparation. The provider log supplies the stop reason.
    ///
    /// See [`ProviderLogRecovery`] for an example.
    #[must_use]
    pub fn recover_provider_log(
        &self,
        format: ProviderLogFormat,
        log: &[u8],
        request: ToolRecoveryRequest,
        dialects: &DialectSet,
        governor: Governor,
    ) -> ProviderLogRecovery {
        let stream = self.stream_with_dialects_governed(request, dialects, governor);
        let mut events = DiscardEvents;
        match format {
            ProviderLogFormat::Anthropic => {
                let mut provider = AnthropicEventStream::with_stream(stream);
                provider.feed(log, &mut events);
                ProviderLogRecovery::Anthropic(provider.finish(&mut events))
            }
            ProviderLogFormat::OpenAiChat => {
                let mut provider = OpenAiChatStream::with_stream(stream);
                provider.feed(log, &mut events);
                ProviderLogRecovery::OpenAiChat(provider.finish(&mut events))
            }
        }
    }
}

// Both readers admit and retain each byte before interpreting its line. The
// callback runs before another byte is admitted, so dispatch and a refusal
// occur at the same offset regardless of the caller's chunk boundaries.
macro_rules! provider_input {
    () => {
        /// Feed event-log bytes, which may end anywhere (inside a line, a
        /// multi-byte character, or a CR LF pair), and append the recognizer's
        /// newly settled [`StreamEvent`]s to `events`. Their spans are offsets in
        /// the reassembled message, not in the log. Empty feeds charge nothing.
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
    };
}
pub(super) use provider_input;

/// Discard batch event payloads after their admission and handoff.
pub(super) struct DiscardEvents;
impl Extend<StreamEvent> for DiscardEvents {
    fn extend<T: IntoIterator<Item = StreamEvent>>(&mut self, events: T) {
        for _ in events {}
    }
}
