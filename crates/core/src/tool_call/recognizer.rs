// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The push-based recognizer of a model turn.
//!
//! A [`Recognizer`] runs every dialect of a [`DialectSet`] concurrently over
//! the output bytes and assembles the turn
//! `Turn := (Content | Reasoning | Call_d | Result_d)*`. It works in two
//! layers:
//!
//! 1. The segment-candidate layer interprets every dialect's compiled
//!    `call` and `result` programs over each byte, repairs failed attempts,
//!    and reports openers, completed segments, and rejected attempts. It is
//!    a pure function of the bytes.
//! 2. The turn layer is a least-cost (tropical) segmentation over those
//!    reports. It settles each offset once the input is
//!    [`DialectSet::lookbehind`] bytes past it, so repairs that reach back
//!    (a missing closer inserted before the next opener) are always seen.
//!    Content may not contain a complete opener; an attempt whose opener
//!    completed ends as a call (or result), or as a rejected segment
//!    costing [`cost::REJECT`](crate::grammar::cost::REJECT). Equal-cost
//!    segmentations of the same span are packed and reported as a local
//!    [`Choice`]; independent choices are never multiplied out.
//!
//! [`Recognizer::push`] is a fold of a per-byte step, so the result and
//! every governor charge are identical however the bytes are split.
//!
//! # Reasoning
//!
//! Under [`ReasoningPolicy::Separate`], the set's reasoning markers delimit
//! reasoning blocks. A call inside a closed block stays reasoning text and
//! is also offered as a guess-flagged alternative
//! (`tool_call.reasoning.embedded-call`). An unclosed block that contains a
//! call opener is closed at the first opener by the guess-flagged rule
//! `tool_call.reasoning.unclosed` (so later calls depend on that guess);
//! without an opener it runs to the end of output with a data loss
//! (`tool_call.reasoning.unterminated`). With `starts_open`, the turn begins
//! inside a reasoning block. Under [`ReasoningPolicy::AsContent`] markers are
//! plain content.
//!
//! # End of output
//!
//! [`GenerationStop::Length`] cuts open calls off: each becomes a
//! [`Closure::Dangling`] reading of what was captured (truncation). Every
//! other stop treats an open call as malformed: it is completed only by
//! inserting missing closers at the end ([`Closure::Unterminated`], left to
//! the caller's completeness rule) or by reinserting the closer the stop
//! sequence consumed ([`Closure::Repaired`], `tool_call.stop.consumed-closer`),
//! and is otherwise [`Rejection::Unterminated`].
use super::dialect::DialectSet;
use super::ir::{
    CallSegment, Choice, Piece, ProductionKind, RawCall, ReasoningClosure, ReasoningSegment,
    Segment,
};
use super::toolset::ToolSet;
use crate::contract::saturating_size as size;
use crate::contract::{
    Evidence, Governor, Inference, InferenceKind, Loss, Resource, SourceSpan, StopReason,
};
use crate::digest::{Digest, DigestBuilder};
use crate::grammar::cost::{K_CAPTURE, K_EVENT, K_LEX, REASONING_UNCLOSED, REJECT};
use crate::grammar::edit::{EditOperator, MarkupEdit};
use crate::grammar::engine::{Engine, Event};
use crate::provenance::{Derivation, Forest, NodeId, permute};
use core::fmt;
use core::fmt::Write as _;
use core::ops::Range;
use std::collections::{BTreeMap, BTreeSet};

/// Why the model stopped generating: the recognizer's end-of-output input.
///
/// [`StopReason`] names why an *engine* stopped; this names why the *model*
/// stopped, which decides whether an open call at the end is truncated or
/// malformed.
///
/// ```
/// use purrassist_core::tool_call::GenerationStop;
/// assert!(GenerationStop::Length.is_truncation());
/// assert!(!GenerationStop::Unknown.is_truncation());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GenerationStop {
    /// A natural stop token.
    Stop,
    /// A caller stop sequence matched and was removed from the output.
    StopSequence(Vec<u8>),
    /// The token budget ran out.
    Length,
    /// The model ended its turn to call tools.
    ToolCalls,
    /// A content filter stopped generation.
    ContentFilter,
    /// Unknown; treated like [`GenerationStop::Stop`] (an open call is
    /// malformed, never truncated). Serving-layer repetition or degenerate
    /// output stops map here.
    Unknown,
}

impl GenerationStop {
    /// Whether an open call at the end of output is truncation (only for
    /// [`GenerationStop::Length`]).
    ///
    /// See [`GenerationStop`] for an example.
    #[must_use]
    pub const fn is_truncation(&self) -> bool {
        matches!(self, Self::Length)
    }
}

/// How reasoning markers are treated.
///
/// ```
/// use purrassist_core::tool_call::ReasoningPolicy;
/// assert_eq!(ReasoningPolicy::default(), ReasoningPolicy::Separate { starts_open: false });
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReasoningPolicy {
    /// Reasoning blocks are separate segments.
    Separate {
        /// The output begins inside a reasoning block (the chat template
        /// placed the opening marker in the prompt).
        starts_open: bool,
    },
    /// Reasoning markers are plain content.
    AsContent,
}

impl Default for ReasoningPolicy {
    fn default() -> Self {
        Self::Separate { starts_open: false }
    }
}

/// One step of the turn-level segmentation history.
#[derive(Clone, Copy, Debug)]
enum TurnKind {
    Root,
    /// Content from this offset to the node's end.
    Content(usize),
    /// A stored segment and its cost.
    Stored(u32, u64),
    /// Two equal-cost segmentations: `prev` or this operand.
    Pack(u32),
}

#[derive(Clone, Copy, Debug)]
struct TurnNode {
    prev: u32,
    end: usize,
    kind: TurnKind,
}

#[derive(Clone, Copy, Debug)]
struct Top {
    cost: u64,
    hist: u32,
    boundary: usize,
}

#[derive(Clone, Debug)]
struct Think {
    cost: u64,
    hist: u32,
    start: usize,
    pair: Option<u32>,
    /// The opener start and promoted base, once the block was closed at its
    /// first opener.
    promoted: Option<(usize, Base)>,
    embedded: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Base {
    cost: u64,
    hist: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Interp {
    Top(Base),
    Embedded(usize),
}

#[derive(Clone, Debug)]
enum Pending {
    Engine(Event),
    Marker {
        start: usize,
        end: usize,
        pair: u32,
        open: bool,
    },
}

impl Pending {
    const fn at(&self) -> usize {
        match self {
            Self::Engine(event) => event.at(),
            Self::Marker { end, .. } => *end,
        }
    }

    /// Order within one offset: openers before revivals.
    const fn phase(&self) -> u8 {
        match self {
            Self::Engine(Event::Opened { .. }) => 0,
            Self::Marker { open: true, .. } => 1,
            Self::Engine(Event::Completed { .. }) => 2,
            Self::Engine(Event::Ended { .. }) => 3,
            Self::Marker { open: false, .. } => 4,
        }
    }
}

/// The changes one offset's reports make, applied together.
struct Frame {
    /// The content state before this offset.
    top: Option<Top>,
    /// An opener ended a content run.
    kill: bool,
    /// Segments ending here, with their total costs.
    candidates: Vec<(u64, u32)>,
    /// A reasoning block opened here.
    think: Option<Think>,
    /// The reasoning block closed here.
    closed: bool,
}

/// The turn-level segmentation state.
struct Turn {
    nodes: Vec<TurnNode>,
    store: Vec<Segment>,
    top: Option<Top>,
    think: Option<Think>,
    interps: BTreeMap<u64, Vec<Interp>>,
    /// The base of the last opener start that ended a content run.
    last_kill: Option<(usize, Base)>,
}

impl Turn {
    fn push(
        &mut self,
        prev: u32,
        end: usize,
        kind: TurnKind,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        governor.charge(Resource::ScratchBytes, 32)?;
        self.nodes.push(TurnNode { prev, end, kind });
        Ok(u32::try_from(self.nodes.len() - 1).unwrap_or(u32::MAX))
    }

    fn store(&mut self, segment: Segment, governor: &mut Governor) -> Result<u32, StopReason> {
        governor.charge(Resource::ScratchBytes, segment_bytes(&segment))?;
        self.store.push(segment);
        Ok(u32::try_from(self.store.len() - 1).unwrap_or(u32::MAX))
    }

    /// The history of `top` with its content run closed at `at`.
    fn close(&mut self, top: Top, at: usize, governor: &mut Governor) -> Result<u32, StopReason> {
        if top.boundary < at {
            self.push(top.hist, at, TurnKind::Content(top.boundary), governor)
        } else {
            Ok(top.hist)
        }
    }

    fn append(
        &mut self,
        base: Base,
        segment: u32,
        cost: u64,
        end: usize,
        governor: &mut Governor,
    ) -> Result<(u64, u32), StopReason> {
        let hist = self.push(base.hist, end, TurnKind::Stored(segment, cost), governor)?;
        Ok((base.cost.saturating_add(cost), hist))
    }

    /// Least-cost merge of `candidates` into the content state at `at`;
    /// equal costs pack.
    fn revive(
        &mut self,
        mut candidates: Vec<(u64, u32)>,
        at: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Some(best) = candidates.iter().map(|(cost, _)| *cost).min() else {
            return Ok(());
        };
        if let Some(top) = self.top {
            if top.cost < best {
                return Ok(());
            }
            if top.cost == best {
                let closed = self.close(top, at, governor)?;
                candidates.push((best, closed));
            }
        }
        let mut hists: Vec<u32> = candidates
            .into_iter()
            .filter(|(cost, _)| *cost == best)
            .map(|(_, hist)| hist)
            .collect();
        hists.sort_unstable();
        hists.dedup();
        let mut hist = hists[0];
        for &other in &hists[1..] {
            hist = self.push(hist, at, TurnKind::Pack(other), governor)?;
        }
        self.top = Some(Top {
            cost: best,
            hist,
            boundary: at,
        });
        Ok(())
    }

    /// The reasoning segment of `think` ending at `end`.
    fn reasoning(
        &mut self,
        think: &Think,
        end: usize,
        closure: ReasoningClosure,
        edits: Vec<MarkupEdit>,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        let mut embedded = Vec::new();
        for &index in &think.embedded {
            let (Segment::Call(segment) | Segment::Result(segment)) = &self.store[index as usize]
            else {
                continue;
            };
            if segment.span.end > end {
                continue;
            }
            let mut segment = segment.clone();
            for reading in &mut segment.readings {
                reading.edits.push(MarkupEdit {
                    operator: EditOperator::ReasoningEmbeddedCall,
                    span: segment.span.clone(),
                    original: Vec::new(),
                    replacement: Vec::new(),
                });
            }
            embedded.push(segment);
        }
        self.store(
            Segment::Reasoning(ReasoningSegment {
                span: think.start..end,
                closure,
                embedded,
                edits,
            }),
            governor,
        )
    }
}

fn segment_bytes(segment: &Segment) -> u64 {
    let call = |segment: &CallSegment| -> u64 {
        segment
            .readings
            .iter()
            .map(|reading| {
                reading
                    .arguments
                    .root
                    .estimated_bytes()
                    .saturating_add(size(reading.edits.len()).saturating_mul(128))
                    .saturating_add(256)
            })
            .sum()
    };
    match segment {
        Segment::Call(segment) | Segment::Result(segment) => call(segment),
        Segment::Reasoning(reasoning) => reasoning
            .embedded
            .iter()
            .map(call)
            .sum::<u64>()
            .saturating_add(128),
        Segment::Content(_) | Segment::Rejected { .. } => 96,
    }
}

/// A push-based recognizer of one model turn over a [`DialectSet`].
///
/// ```
/// use purrassist_core::grammar::Grammar;
/// use purrassist_core::tool_call::{DialectId, DialectSet, GenerationStop, Piece, ReasoningPolicy, Recognizer, Segment};
/// use purrassist_core::{Budget, Governor};
/// let mut governor = Governor::new(Budget::default());
/// let grammar = Grammar::compile_governed(
///     r#"grammar "tiny.v1";
///        call = "<c>" name:/[a-z]+/ "</c>";
///        result = "<r>" content:until("</r>") "</r>";
///        declaration = "<d>";"#,
///     &mut governor,
/// )
/// .unwrap();
/// let set = DialectSet::new([DialectId::custom(grammar)], &mut governor).unwrap();
/// let mut recognizer = Recognizer::new(&set, ReasoningPolicy::default(), &mut governor)?;
/// recognizer.push(b"hi <c>wea", &mut governor)?;
/// recognizer.push(b"ther</c> bye", &mut governor)?;
/// let turn = recognizer.finish(GenerationStop::Stop, &mut governor)?;
/// let segments = turn.selected();
/// assert_eq!(segments.len(), 3);
/// let Segment::Call(call) = segments[1] else { unreachable!() };
/// assert_eq!(call.span, 3..17);
/// assert_eq!(call.readings[0].name.as_ref().unwrap().value, "weather");
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
pub struct Recognizer<'s> {
    set: &'s DialectSet,
    policy: ReasoningPolicy,
    engine: Engine<'s>,
    digest: DigestBuilder,
    marker: u32,
    pending: Vec<Pending>,
    turn: Turn,
    failed: Option<StopReason>,
}

impl fmt::Debug for Recognizer<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Recognizer")
            .field("dialects", &self.set.dialects())
            .field("policy", &self.policy)
            .field("fed", &self.engine.fed())
            .finish_non_exhaustive()
    }
}

impl<'s> Recognizer<'s> {
    /// A recognizer for one turn over `set`.
    ///
    /// Builds a working copy of every class terminal's automaton, charged to
    /// `governor`.
    ///
    /// # Errors
    /// Returns the governor's exhaustion or cancellation reason.
    ///
    /// See [`Recognizer`] for an example.
    pub fn new(
        set: &'s DialectSet,
        policy: ReasoningPolicy,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        Self::build(set, None, policy, governor)
    }

    /// A recognizer for one turn over `set` whose argument codecs know the
    /// declared tools: when a call's `name:` capture precedes its arguments
    /// slot and names a tool of `tools` (exactly, or by alias), the slot's
    /// codec receives that tool's [`ArgumentShape`](super::ArgumentShape)
    /// (see [`crate::grammar::codec`]). The `xml-params` codec types values
    /// and decides parameter boundaries by it; the `json` codec records the
    /// tool's canonical name ([`ArgTree::tool`](super::ArgTree::tool)).
    ///
    /// # Errors
    /// Returns the governor's exhaustion or cancellation reason.
    ///
    /// ```
    /// use purrassist_core::grammar::Grammar;
    /// use purrassist_core::tool_call::{DialectId, DialectSet, GenerationStop, Param, ReasoningPolicy, Recognizer, ToolDefinition, ToolSet};
    /// use purrassist_core::{Budget, Governor};
    /// let mut governor = Governor::new(Budget::default());
    /// let grammar = Grammar::compile_governed(
    ///     r#"grammar "xml.v1";
    ///        call = "<f=" name:/[a-z]+/ ">" arguments:slot(xml-params, "</f>") "</f>";
    ///        result = "<r>" content:until("</r>") "</r>";
    ///        declaration = "<d>";"#,
    ///     &mut governor,
    /// )
    /// .unwrap();
    /// let set = DialectSet::new([DialectId::custom(grammar)], &mut governor).unwrap();
    /// let tools = ToolSet::compile([ToolDefinition::new("sum").param("n", Param::integer())], Budget::default()).unwrap();
    /// let mut recognizer = Recognizer::with_tools(&set, &tools, ReasoningPolicy::default(), &mut governor)?;
    /// recognizer.push(b"<f=sum><parameter=n>\n1.50\n</parameter></f>", &mut governor)?;
    /// let turn = recognizer.finish(GenerationStop::Stop, &mut governor)?;
    /// let call = &turn.calls()[0].readings[0];
    /// assert_eq!(call.arguments.root.to_json(), r#"{"n":1.50}"#);
    /// assert_eq!(call.arguments.tool.as_deref(), Some("sum"));
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    pub fn with_tools(
        set: &'s DialectSet,
        tools: &'s ToolSet,
        policy: ReasoningPolicy,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        Self::build(set, Some(tools), policy, governor)
    }

    fn build(
        set: &'s DialectSet,
        tools: Option<&'s ToolSet>,
        policy: ReasoningPolicy,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let engine = Engine::new(&set.dialects, tools, governor)?;
        let starts_open = matches!(policy, ReasoningPolicy::Separate { starts_open: true });
        let root = TurnNode {
            prev: 0,
            end: 0,
            kind: TurnKind::Root,
        };
        let turn = Turn {
            nodes: vec![root],
            store: Vec::new(),
            top: (!starts_open).then_some(Top {
                cost: 0,
                hist: 0,
                boundary: 0,
            }),
            think: starts_open.then(|| Think {
                cost: 0,
                hist: 0,
                start: 0,
                pair: None,
                promoted: None,
                embedded: Vec::new(),
            }),
            interps: BTreeMap::new(),
            last_kill: None,
        };
        Ok(Self {
            set,
            policy,
            engine,
            digest: DigestBuilder::new(),
            marker: 0,
            pending: Vec::new(),
            turn,
            failed: None,
        })
    }

    /// The number of bytes pushed so far.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::tool_call::{DialectId, DialectSet, ReasoningPolicy, Recognizer};
    /// # use purrassist_core::{Budget, Governor};
    /// # let mut governor = Governor::new(Budget::default());
    /// # let text = "grammar \"g\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// # let set = DialectSet::new([DialectId::custom(Grammar::compile_governed(text, &mut governor).unwrap())], &mut governor).unwrap();
    /// let mut recognizer = Recognizer::new(&set, ReasoningPolicy::AsContent, &mut governor)?;
    /// recognizer.push(b"abc", &mut governor)?;
    /// assert_eq!(recognizer.fed(), 3);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub const fn fed(&self) -> usize {
        self.engine.fed()
    }

    /// Consume the next chunk of output.
    ///
    /// Each byte charges [`K_LEX`] `Work` and is admitted against
    /// `InputBytes` cumulatively; every dialect configuration it advances
    /// charges as documented in [`crate::grammar::cost`]. A chunk may end
    /// anywhere, including inside a UTF-8 character or a dialect literal.
    ///
    /// # Errors
    /// Returns the governor's exhaustion or cancellation reason. Errors are
    /// sticky: later calls (and [`Recognizer::finish`]) return them again.
    ///
    /// See [`Recognizer`] for an example.
    pub fn push(&mut self, bytes: &[u8], governor: &mut Governor) -> Result<(), StopReason> {
        if let Some(reason) = &self.failed {
            return Err(reason.clone());
        }
        let result = bytes.iter().try_for_each(|&byte| self.step(byte, governor));
        match result {
            Ok(()) => {
                self.digest.update(bytes);
                Ok(())
            }
            Err(reason) => {
                self.failed = Some(reason.clone());
                Err(reason)
            }
        }
    }

    fn step(&mut self, byte: u8, governor: &mut Governor) -> Result<(), StopReason> {
        let at = self.engine.fed();
        governor.charge(Resource::InputBytes, size(at + 1))?;
        governor.charge(Resource::Work, K_LEX)?;
        self.engine.push(byte, governor)?;
        if let (ReasoningPolicy::Separate { .. }, Some(matcher)) =
            (self.policy, &self.set.marker_matcher)
        {
            self.marker = matcher.step(self.marker, byte);
            if let Some((literal, length)) = matcher.found(self.marker) {
                self.pending.push(Pending::Marker {
                    start: at + 1 - length as usize,
                    end: at + 1,
                    pair: literal / 2,
                    open: literal % 2 == 0,
                });
            }
        }
        self.pending
            .extend(self.engine.events.drain(..).map(Pending::Engine));
        if let Some(limit) = (at + 1).checked_sub(self.set.lookbehind) {
            self.settle(limit, governor)?;
        }
        Ok(())
    }

    /// Process every pending report at an offset up to `limit`.
    fn settle(&mut self, limit: usize, governor: &mut Governor) -> Result<(), StopReason> {
        if !self.pending.iter().any(|pending| pending.at() <= limit) {
            return Ok(());
        }
        let (ready, rest): (Vec<Pending>, Vec<Pending>) = core::mem::take(&mut self.pending)
            .into_iter()
            .partition(|pending| pending.at() <= limit);
        self.pending = rest;
        // Stable order: offset, then phase, then arrival.
        let mut ready: Vec<(usize, usize, Pending)> = ready
            .into_iter()
            .enumerate()
            .map(|(sequence, pending)| (sequence, pending.at(), pending))
            .collect();
        ready.sort_by_key(|(sequence, at, pending)| (*at, pending.phase(), *sequence));
        let mut start = 0;
        while start < ready.len() {
            let at = ready[start].1;
            let end = ready[start..]
                .iter()
                .position(|(_, other, _)| *other != at)
                .map_or(ready.len(), |offset| start + offset);
            let group: Vec<Pending> = ready[start..end]
                .iter()
                .map(|(_, _, pending)| pending.clone())
                .collect();
            self.position(at, group, governor)?;
            start = end;
        }
        Ok(())
    }

    /// Apply every report at offset `at`: openers and opening markers
    /// first (against the content state before this offset), then the
    /// segments ending here, merged least-cost.
    fn position(
        &mut self,
        at: usize,
        group: Vec<Pending>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, size(group.len()).saturating_mul(K_EVENT))?;
        let mut frame = Frame {
            top: self.turn.top,
            kill: false,
            candidates: Vec::new(),
            think: None,
            closed: false,
        };
        for pending in group {
            match pending {
                Pending::Engine(Event::Opened { attempt, start, .. }) => {
                    self.opened(&mut frame, attempt, start, governor)?;
                }
                Pending::Marker {
                    start,
                    pair,
                    open: true,
                    ..
                } => self.think_opened(&mut frame, start, pair, governor)?,
                Pending::Engine(Event::Completed {
                    table,
                    attempt,
                    segment,
                    cost,
                    ..
                }) => {
                    let segment = match self.engine.kind(table) {
                        ProductionKind::Call => Segment::Call(segment),
                        ProductionKind::Result => Segment::Result(segment),
                    };
                    self.completed(&mut frame, at, attempt, segment, cost, governor)?;
                }
                Pending::Engine(Event::Ended {
                    table,
                    attempt,
                    start,
                    completed,
                    reason,
                    ..
                }) => {
                    let Some(interps) = self.turn.interps.remove(&attempt) else {
                        continue;
                    };
                    if !completed {
                        let segment = Segment::Rejected {
                            span: start..at,
                            dialect: self.engine.dialect(table).clone(),
                            reason,
                        };
                        let index = self.turn.store(segment, governor)?;
                        for interp in interps {
                            if let Interp::Top(base) = interp {
                                frame
                                    .candidates
                                    .push(self.turn.append(base, index, REJECT, at, governor)?);
                            }
                        }
                    }
                }
                Pending::Marker {
                    start,
                    pair,
                    open: false,
                    ..
                } => self.think_closed(&mut frame, at, start, pair, governor)?,
            }
        }
        if frame.kill {
            self.turn.top = None;
        }
        if frame.closed {
            self.turn.think = None;
        }
        if let Some(think) = frame.think
            && self
                .turn
                .think
                .as_ref()
                .is_none_or(|existing| think.cost < existing.cost)
        {
            self.turn.think = Some(think);
        }
        self.turn.revive(frame.candidates, at, governor)
    }

    /// An attempt's opener completed: give it a base on every state that
    /// was live where it started.
    fn opened(
        &mut self,
        frame: &mut Frame,
        attempt: u64,
        start: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut interps = Vec::new();
        match (frame.top, self.turn.last_kill) {
            (Some(top), last) if top.boundary <= start => {
                let base = match last {
                    Some((kill_start, base)) if kill_start == start => base,
                    _ => {
                        let base = Base {
                            cost: top.cost,
                            hist: self.turn.close(top, start, governor)?,
                        };
                        self.turn.last_kill = Some((start, base));
                        base
                    }
                };
                interps.push(Interp::Top(base));
                frame.kill = true;
            }
            (_, Some((kill_start, base))) if kill_start == start => interps.push(Interp::Top(base)),
            _ => {}
        }
        if let Some(think) = self.turn.think.clone()
            && think.start <= start
        {
            interps.push(Interp::Embedded(think.start));
            if let Some(base) = self.promote(&think, start, governor)? {
                interps.push(Interp::Top(base));
            }
        }
        if !interps.is_empty() {
            self.turn.interps.insert(attempt, interps);
        }
        Ok(())
    }

    /// The base of a call that closes the unclosed reasoning block `think`
    /// at its first opener (`start`), by the guess-flagged rule.
    fn promote(
        &mut self,
        think: &Think,
        start: usize,
        governor: &mut Governor,
    ) -> Result<Option<Base>, StopReason> {
        if let Some((promoted_start, base)) = think.promoted {
            return Ok((promoted_start == start).then_some(base));
        }
        let edit = MarkupEdit {
            operator: EditOperator::ReasoningUnclosed,
            span: start..start,
            original: Vec::new(),
            replacement: self
                .set
                .markers
                .get(think.pair.unwrap_or(0) as usize)
                .map(|(_, close)| close.clone())
                .unwrap_or_default(),
        };
        let segment = self.turn.reasoning(
            think,
            start,
            ReasoningClosure::ClosedAtOpener,
            vec![edit],
            governor,
        )?;
        let prior = Base {
            cost: think.cost,
            hist: think.hist,
        };
        let (cost, hist) = self
            .turn
            .append(prior, segment, REASONING_UNCLOSED, start, governor)?;
        let base = Base { cost, hist };
        if let Some(think) = self.turn.think.as_mut() {
            think.promoted = Some((start, base));
        }
        Ok(Some(base))
    }

    /// A reasoning opener completed at `start`.
    fn think_opened(
        &mut self,
        frame: &mut Frame,
        start: usize,
        pair: u32,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Some(top) = frame.top.filter(|top| top.boundary <= start) else {
            return Ok(());
        };
        frame.kill = true;
        let think = Think {
            cost: top.cost,
            hist: self.turn.close(top, start, governor)?,
            start,
            pair: Some(pair),
            promoted: None,
            embedded: Vec::new(),
        };
        let current = self.turn.think.as_ref().or(frame.think.as_ref());
        if current.is_none_or(|existing| think.cost < existing.cost) {
            frame.think = Some(think);
        }
        Ok(())
    }

    /// A segment of `attempt` ended at `at`.
    fn completed(
        &mut self,
        frame: &mut Frame,
        at: usize,
        attempt: u64,
        segment: Segment,
        cost: u64,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Some(interps) = self.turn.interps.get(&attempt).cloned() else {
            return Ok(());
        };
        let index = self.turn.store(segment, governor)?;
        for interp in interps {
            match interp {
                Interp::Top(base) => frame
                    .candidates
                    .push(self.turn.append(base, index, cost, at, governor)?),
                Interp::Embedded(think_start) => {
                    if let Some(think) = self.turn.think.as_mut()
                        && think.start == think_start
                    {
                        think.embedded.push(index);
                    }
                }
            }
        }
        Ok(())
    }

    /// A reasoning closer ended at `at`.
    fn think_closed(
        &mut self,
        frame: &mut Frame,
        at: usize,
        start: usize,
        pair: u32,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Some(think) = self.turn.think.clone() else {
            return Ok(());
        };
        if think.pair.is_some_and(|open| open != pair) || think.start > start {
            return Ok(());
        }
        let segment =
            self.turn
                .reasoning(&think, at, ReasoningClosure::Closed, Vec::new(), governor)?;
        let prior = Base {
            cost: think.cost,
            hist: think.hist,
        };
        frame
            .candidates
            .push(self.turn.append(prior, segment, 0, at, governor)?);
        frame.closed = true;
        Ok(())
    }

    /// Keep an unclosed reasoning block without a call opener as reasoning
    /// to the end of output, recording the data loss.
    fn unterminated(
        &mut self,
        think: &Think,
        end: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let edit = MarkupEdit {
            operator: EditOperator::ReasoningUnterminated,
            span: think.start..end,
            original: Vec::new(),
            replacement: Vec::new(),
        };
        let segment = self.turn.reasoning(
            think,
            end,
            ReasoningClosure::Unterminated,
            vec![edit],
            governor,
        )?;
        let prior = Base {
            cost: think.cost,
            hist: think.hist,
        };
        let candidate = self.turn.append(prior, segment, 0, end, governor)?;
        self.turn.revive(vec![candidate], end, governor)
    }

    /// End the turn and return its packed segmentation.
    ///
    /// # Errors
    /// Returns a sticky error from [`Recognizer::push`], or the governor's
    /// exhaustion or cancellation reason; nothing partial is returned.
    ///
    /// See [`Recognizer`] for an example.
    pub fn finish(
        mut self,
        stop: GenerationStop,
        governor: &mut Governor,
    ) -> Result<RecognizedTurn, StopReason> {
        if let Some(reason) = self.failed.take() {
            return Err(reason);
        }
        let end = self.engine.fed();
        self.engine.finish(&stop, governor)?;
        self.pending
            .extend(self.engine.events.drain(..).map(Pending::Engine));
        self.settle(usize::MAX, governor)?;
        if let Some(think) = self.turn.think.take()
            && think.promoted.is_none()
        {
            if think.start == end {
                // An output that ends where its open block starts (an empty
                // output under `starts_open`) holds no reasoning to classify.
                let empty = (think.cost, think.hist);
                self.turn.revive(vec![empty], end, governor)?;
            } else {
                self.unterminated(&think, end, governor)?;
            }
        }
        let Some(top) = self.turn.top else {
            return Err(StopReason::InvalidInput(
                "recognizer reached the end of output without a segmentation".into(),
            ));
        };
        let last = self.turn.close(top, end, governor)?;
        let source = self.digest.finish();
        let mut builder = Builder {
            turn: &self.turn,
            forest: Forest::new(governor),
            source,
        };
        let pieces = builder.pieces(last, governor)?;
        let Builder { forest, .. } = builder;
        Ok(RecognizedTurn {
            source,
            len: end,
            stop,
            policy: self.policy,
            pieces,
            forest,
            cost: top.cost,
        })
    }
}

/// Builds the factorized pieces and their derivations.
struct Builder<'t> {
    turn: &'t Turn,
    forest: Forest,
    source: Digest,
}

impl Builder<'_> {
    fn predecessors(&self, node: u32) -> [Option<u32>; 2] {
        let TurnNode { prev, kind, .. } = self.turn.nodes[node as usize];
        match kind {
            TurnKind::Root => [None, None],
            TurnKind::Pack(other) => [Some(prev), Some(other)],
            TurnKind::Content(_) | TurnKind::Stored(..) => [Some(prev), None],
        }
    }

    /// The nearest node every path from `node` back to the root passes.
    fn dominator(&self, node: u32, governor: &mut Governor) -> Result<u32, StopReason> {
        let mut frontier: BTreeSet<u32> = self.predecessors(node).into_iter().flatten().collect();
        while frontier.len() > 1 {
            governor.charge(Resource::Work, K_CAPTURE)?;
            let latest = frontier.pop_last().expect("non-empty frontier");
            frontier.extend(self.predecessors(latest).into_iter().flatten());
        }
        Ok(frontier.pop_first().expect("paths meet at the root"))
    }

    fn segment(&self, node: u32) -> (Segment, u64) {
        let TurnNode { end, kind, .. } = self.turn.nodes[node as usize];
        match kind {
            TurnKind::Content(start) => (Segment::Content(start..end), 0),
            TurnKind::Stored(index, cost) => (self.turn.store[index as usize].clone(), cost),
            TurnKind::Root | TurnKind::Pack(_) => unreachable!("not a segment"),
        }
    }

    /// Every distinct segmentation from `stop` to `node`.
    fn paths(
        &self,
        node: u32,
        stop: u32,
        governor: &mut Governor,
    ) -> Result<Vec<(Vec<Segment>, u64)>, StopReason> {
        let mut readings: Vec<(Vec<Segment>, u64)> = Vec::new();
        let mut stack = vec![(node, Vec::<u32>::new())];
        while let Some((current, mut path)) = stack.pop() {
            governor.charge(Resource::Work, K_CAPTURE)?;
            if current == stop {
                let mut segments = Vec::with_capacity(path.len());
                let mut cost = 0_u64;
                for &step in path.iter().rev() {
                    let (segment, segment_cost) = self.segment(step);
                    cost = cost.saturating_add(segment_cost);
                    push_merged(&mut segments, segment);
                }
                if !readings.iter().any(|(known, _)| *known == segments) {
                    governor.charge(Resource::Candidates, 1)?;
                    readings.push((segments, cost));
                }
                continue;
            }
            match self.turn.nodes[current as usize].kind {
                TurnKind::Pack(other) => {
                    stack.push((other, path.clone()));
                    stack.push((self.turn.nodes[current as usize].prev, path));
                }
                TurnKind::Content(_) | TurnKind::Stored(..) => {
                    path.push(current);
                    stack.push((self.turn.nodes[current as usize].prev, path));
                }
                TurnKind::Root => {}
            }
        }
        Ok(readings)
    }

    fn pieces(&mut self, last: u32, governor: &mut Governor) -> Result<Vec<Piece>, StopReason> {
        let mut reversed: Vec<Piece> = Vec::new();
        let mut node = last;
        while node != 0 {
            match self.turn.nodes[node as usize].kind {
                TurnKind::Pack(_) => {
                    let stop = self.dominator(node, governor)?;
                    let mut readings = self.paths(node, stop, governor)?;
                    if readings.len() == 1 {
                        let (segments, _) = readings.pop().expect("one reading");
                        reversed.extend(segments.into_iter().rev().map(Piece::Segment));
                    } else {
                        let span =
                            self.turn.nodes[stop as usize].end..self.turn.nodes[node as usize].end;
                        reversed.push(Piece::Choice(Choice {
                            span,
                            derivations: readings
                                .iter()
                                .map(|(_, cost)| Derivation::new(Digest::of(b""), *cost))
                                .collect(),
                            readings: readings.into_iter().map(|(segments, _)| segments).collect(),
                        }));
                    }
                    node = stop;
                }
                TurnKind::Content(_) | TurnKind::Stored(..) => {
                    reversed.push(Piece::Segment(self.segment(node).0));
                    node = self.turn.nodes[node as usize].prev;
                }
                TurnKind::Root => break,
            }
        }
        let mut pieces: Vec<Piece> = Vec::with_capacity(reversed.len());
        for piece in reversed.into_iter().rev() {
            match (pieces.last_mut(), piece) {
                (
                    Some(Piece::Segment(Segment::Content(last))),
                    Piece::Segment(Segment::Content(next)),
                ) if last.end == next.start => {
                    last.end = next.end;
                }
                (_, piece) => pieces.push(piece),
            }
        }
        self.annotate(&mut pieces, governor)?;
        Ok(pieces)
    }

    /// Number the calls, build every derivation, and rank every
    /// alternative list.
    fn annotate(
        &mut self,
        pieces: &mut [Piece],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut order = 0;
        let mut inherited: Vec<NodeId> = Vec::new();
        for piece in pieces {
            match piece {
                Piece::Segment(segment) => {
                    self.segment_derivations(segment, &mut order, &mut inherited, governor)?;
                }
                Piece::Choice(choice) => {
                    let mut derivations = Vec::with_capacity(choice.readings.len());
                    let mut outcomes = Vec::with_capacity(choice.readings.len());
                    for (reading, prior) in choice.readings.iter_mut().zip(&choice.derivations) {
                        let mut local_order = order;
                        let mut local_inherited = inherited.clone();
                        let mut nodes = Vec::new();
                        for segment in reading.iter_mut() {
                            nodes.extend(self.segment_derivations(
                                segment,
                                &mut local_order,
                                &mut local_inherited,
                                governor,
                            )?);
                        }
                        let mut encoded = Vec::new();
                        for segment in reading.iter() {
                            encode_segment(segment, &mut encoded);
                        }
                        governor.charge(Resource::Work, size(encoded.len()))?;
                        let mut derivation = Derivation::new(Digest::of(&encoded), prior.cost());
                        for node in nodes.into_iter().chain(local_inherited.iter().copied()) {
                            derivation.push(node);
                        }
                        derivations.push(derivation);
                        outcomes.push((local_order, local_inherited));
                    }
                    let ranked = Forest::rank(&derivations);
                    choice.readings = permute(&choice.readings, &ranked);
                    choice.derivations = permute(&derivations, &ranked);
                    let (selected_order, selected_inherited) = outcomes.swap_remove(ranked[0]);
                    order = selected_order;
                    inherited = selected_inherited;
                }
            }
        }
        Ok(())
    }

    /// Derivations of one segment's readings; returns the nodes of its
    /// selected reading (and its reasoning edits) for an enclosing choice.
    fn segment_derivations(
        &mut self,
        segment: &mut Segment,
        order: &mut usize,
        inherited: &mut Vec<NodeId>,
        governor: &mut Governor,
    ) -> Result<Vec<NodeId>, StopReason> {
        match segment {
            Segment::Call(call) => {
                let nodes = self.call_derivations(call, *order, inherited, governor)?;
                *order += 1;
                Ok(nodes)
            }
            Segment::Result(call) => self.call_derivations(call, *order, inherited, governor),
            Segment::Reasoning(reasoning) => {
                let mut nodes = Vec::new();
                for embedded in &mut reasoning.embedded {
                    self.call_derivations(embedded, *order, &[], governor)?;
                }
                for edit in &reasoning.edits {
                    let created = self.intern(edit, &reasoning.span, governor)?;
                    if edit.rule().guess {
                        inherited.extend(created.iter().copied());
                    }
                    nodes.extend(created);
                }
                Ok(nodes)
            }
            Segment::Content(_) | Segment::Rejected { .. } => Ok(Vec::new()),
        }
    }

    fn call_derivations(
        &mut self,
        call: &mut CallSegment,
        order: usize,
        inherited: &[NodeId],
        governor: &mut Governor,
    ) -> Result<Vec<NodeId>, StopReason> {
        let mut derivations = Vec::with_capacity(call.readings.len());
        let mut all = Vec::with_capacity(call.readings.len());
        for reading in &mut call.readings {
            reading.order = order;
            let mut encoded = Vec::new();
            encode_call(reading, &mut encoded);
            governor.charge(Resource::Work, size(encoded.len()))?;
            let mut derivation = Derivation::new(Digest::of(&encoded), reading.cost);
            let mut nodes = Vec::new();
            for edit in &reading.edits {
                nodes.extend(self.intern(edit, &reading.span, governor)?);
            }
            for &node in nodes.iter().chain(inherited) {
                derivation.push(node);
            }
            derivations.push(derivation);
            all.push(nodes);
        }
        let ranked = Forest::rank(&derivations);
        call.readings = permute(&call.readings, &ranked);
        call.derivations = permute(&derivations, &ranked);
        Ok(all.swap_remove(ranked[0]))
    }

    fn intern(
        &mut self,
        edit: &MarkupEdit,
        within: &Range<usize>,
        governor: &mut Governor,
    ) -> Result<Vec<NodeId>, StopReason> {
        let rule = edit.rule();
        let evidence = Evidence {
            rule: rule.id.into(),
            explanation: explain(edit),
            span: Some(SourceSpan {
                source: self.source,
                bytes: edit.span.clone(),
            }),
            destination: format!("/turn/{}..{}", within.start, within.end),
        };
        let mut nodes = vec![self.forest.evidence(governor, evidence.clone())?];
        if rule.loss.is_some() {
            nodes.push(self.forest.loss(
                governor,
                Loss {
                    evidence: evidence.clone(),
                    original: escaped(&edit.original),
                },
            )?);
        }
        if rule.guess {
            nodes.push(self.forest.inference(
                governor,
                Inference {
                    evidence,
                    kind: InferenceKind::Guess,
                    value: escaped(&edit.replacement),
                },
            )?);
        }
        Ok(nodes)
    }
}

fn push_merged(segments: &mut Vec<Segment>, segment: Segment) {
    if let (Some(Segment::Content(last)), Segment::Content(next)) = (segments.last_mut(), &segment)
        && last.end == next.start
    {
        last.end = next.end;
        return;
    }
    segments.push(segment);
}

/// Bytes as text, with every byte outside well-formed UTF-8 written `\xHH`.
fn escaped(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        for byte in chunk.invalid() {
            write!(text, "\\x{byte:02X}").expect("writing to a String is infallible");
        }
    }
    text
}

fn explain(edit: &MarkupEdit) -> String {
    let replacement = escaped(&edit.replacement);
    let original = escaped(&edit.original);
    match edit.operator {
        EditOperator::InsertCloser => format!("inserted missing closer `{replacement}`"),
        EditOperator::DeleteStrayCloser => format!("deleted stray closer `{original}`"),
        EditOperator::CloserVariant => {
            format!("read closer variant `{original}` as `{replacement}`")
        }
        EditOperator::TerminalConfusable => {
            format!("read confusable spelling `{original}` as `{replacement}`")
        }
        EditOperator::ConsumedCloser => {
            format!("reinserted closer `{replacement}` consumed by the stop sequence")
        }
        EditOperator::ReasoningUnclosed => {
            format!("closed unclosed reasoning block with `{replacement}` at the first call opener")
        }
        EditOperator::ReasoningEmbeddedCall => {
            "call recognized inside a closed reasoning block; it remains reasoning text".into()
        }
        EditOperator::ReasoningUnterminated => {
            "reasoning block never closed; kept as reasoning to the end of output".into()
        }
        EditOperator::MissingParameterClose => format!(
            "ended the open parameter value before a declared parameter opener (inserted `{replacement}`)"
        ),
        EditOperator::FramingNewline => {
            "consumed one framing newline of an XML parameter value".into()
        }
        EditOperator::StringifiedArguments => {
            format!("decoded stringified arguments `{original}` as JSON `{replacement}`")
        }
        EditOperator::JsonComment
        | EditOperator::JsonMissingValue
        | EditOperator::JsonLiteralCase
        | EditOperator::JsonBareString
        | EditOperator::JsonBareKey
        | EditOperator::JsonString
        | EditOperator::JsonUnterminatedString
        | EditOperator::JsonLiteralEscape
        | EditOperator::JsonMissingColon
        | EditOperator::JsonMissingComma
        | EditOperator::JsonTrailingComma
        | EditOperator::JsonCloseContainer => format!(
            "JSON argument recovery `{}`: read `{original}` as `{replacement}`",
            edit.rule().id
        ),
    }
}

fn encode_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    out.extend((bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn encode_range(range: &Range<usize>, out: &mut Vec<u8>) {
    out.extend((range.start as u64).to_be_bytes());
    out.extend((range.end as u64).to_be_bytes());
}

/// A canonical, order-independent-of-position encoding of one reading (its
/// `order` is excluded): the identity a derivation's output digest names.
fn encode_call(call: &RawCall, out: &mut Vec<u8>) {
    encode_range(&call.span, out);
    encode_bytes(call.dialect.name().as_bytes(), out);
    out.extend(call.dialect.grammar().digest().as_bytes());
    out.push(call.kind as u8);
    for field in [&call.name, &call.id] {
        match field {
            Some(spanned) => {
                out.push(1);
                encode_range(&spanned.span, out);
                encode_bytes(spanned.value.as_bytes(), out);
            }
            None => out.push(0),
        }
    }
    encode_bytes(call.arguments.root.to_json().as_bytes(), out);
    for leaf in &call.arguments.leaves {
        encode_bytes(leaf.pointer.as_bytes(), out);
        out.push(leaf.origin as u8);
        encode_range(&leaf.raw, out);
    }
    for unresolved in &call.arguments.unresolved {
        encode_bytes(unresolved.pointer.as_bytes(), out);
        out.extend(format!("{:?}", unresolved.fault).as_bytes());
        encode_range(&unresolved.raw, out);
    }
    out.push(u8::from(call.arguments.arguments_were_stringified));
    encode_bytes(
        call.arguments
            .tool
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
        out,
    );
    for capture in &call.captures {
        out.push(capture.kind as u8);
        encode_range(&capture.span, out);
    }
    out.push(call.closure as u8);
    for edit in &call.edits {
        out.push(edit.operator as u8);
        encode_range(&edit.span, out);
        encode_bytes(&edit.replacement, out);
    }
    out.extend(call.cost.to_be_bytes());
}

fn encode_segment(segment: &Segment, out: &mut Vec<u8>) {
    match segment {
        Segment::Content(span) => {
            out.push(0);
            encode_range(span, out);
        }
        Segment::Reasoning(reasoning) => {
            out.push(1);
            encode_range(&reasoning.span, out);
            out.push(reasoning.closure as u8);
        }
        Segment::Call(call) | Segment::Result(call) => {
            out.push(2);
            for reading in &call.readings {
                encode_call(reading, out);
            }
        }
        Segment::Rejected {
            span,
            dialect,
            reason,
        } => {
            out.push(3);
            encode_range(span, out);
            encode_bytes(dialect.name().as_bytes(), out);
            out.extend(format!("{reason:?}").as_bytes());
        }
    }
}

/// A recognized turn: a factorized segmentation of the output bytes and the
/// provenance forest every reading's derivation points into.
///
/// Each reading of the turn partitions `0..len` into segments. The pieces
/// list the segments every reading shares and, where segmentations
/// compete, a [`Choice`]; a call with several equal-cost readings is one
/// [`CallSegment`] (a per-call ambiguity), never a turn-level choice. Calls
/// are numbered (`RawCall::order`) along the selected reading; every
/// reading of a choice is numbered from the position the choice starts at.
#[derive(Debug)]
pub struct RecognizedTurn {
    source: Digest,
    len: usize,
    stop: GenerationStop,
    policy: ReasoningPolicy,
    pieces: Vec<Piece>,
    forest: Forest,
    cost: u64,
}

impl PartialEq for RecognizedTurn {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.len == other.len
            && self.stop == other.stop
            && self.policy == other.policy
            && self.pieces == other.pieces
            && self.cost == other.cost
            && self.forest.nodes() == other.forest.nodes()
    }
}

impl Eq for RecognizedTurn {}

impl RecognizedTurn {
    /// The SHA-256 digest of the recognized output.
    ///
    /// See [`RecognizedTurn::selected`] for an example.
    #[must_use]
    pub const fn source(&self) -> Digest {
        self.source
    }

    /// The number of output bytes.
    ///
    /// See [`RecognizedTurn::selected`] for an example.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the output was empty.
    ///
    /// See [`RecognizedTurn::selected`] for an example.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The generation stop the turn was finished with.
    ///
    /// See [`RecognizedTurn::selected`] for an example.
    #[must_use]
    pub const fn stop(&self) -> &GenerationStop {
        &self.stop
    }

    /// The reasoning policy the turn was recognized under.
    ///
    /// See [`RecognizedTurn::selected`] for an example.
    #[must_use]
    pub const fn policy(&self) -> ReasoningPolicy {
        self.policy
    }

    /// The factorized segmentation.
    ///
    /// See [`RecognizedTurn::selected`] for an example.
    #[must_use]
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// The provenance forest of every edit any reading depends on.
    ///
    /// See [`RecognizedTurn::selected`] for an example.
    #[must_use]
    pub const fn forest(&self) -> &Forest {
        &self.forest
    }

    /// The least segmentation cost.
    ///
    /// See [`RecognizedTurn::selected`] for an example.
    #[must_use]
    pub const fn cost(&self) -> u64 {
        self.cost
    }

    /// The selected (best-ranked) reading: every shared segment plus the
    /// first reading of every choice, in order.
    ///
    /// ```
    /// use purrassist_core::grammar::Grammar;
    /// use purrassist_core::tool_call::{DialectId, DialectSet, GenerationStop, ReasoningPolicy, Recognizer, Segment};
    /// use purrassist_core::{Budget, Governor};
    /// let mut governor = Governor::new(Budget::default());
    /// let text = "grammar \"g\"; call = \"<c>\" name:/[a-z]+/ \"</c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let set = DialectSet::new([DialectId::custom(Grammar::compile_governed(text, &mut governor).unwrap())], &mut governor).unwrap();
    /// let mut recognizer = Recognizer::new(&set, ReasoningPolicy::default(), &mut governor)?;
    /// recognizer.push(b"<c>f</c>", &mut governor)?;
    /// let turn = recognizer.finish(GenerationStop::ToolCalls, &mut governor)?;
    /// assert_eq!((turn.len(), turn.cost(), turn.is_empty()), (8, 0, false));
    /// assert_eq!(turn.stop(), &GenerationStop::ToolCalls);
    /// assert_eq!(turn.policy(), ReasoningPolicy::default());
    /// assert_eq!(turn.pieces().len(), 1);
    /// assert!(turn.forest().nodes().is_empty());
    /// assert_eq!(turn.source(), purrassist_core::digest::Digest::of(b"<c>f</c>"));
    /// assert!(matches!(turn.selected()[..], [Segment::Call(_)]));
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub fn selected(&self) -> Vec<&Segment> {
        let mut segments = Vec::new();
        for piece in &self.pieces {
            match piece {
                Piece::Segment(segment) => segments.push(segment),
                Piece::Choice(choice) => segments.extend(&choice.readings[0]),
            }
        }
        segments
    }

    /// The calls of the selected reading, in order.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::tool_call::{DialectId, DialectSet, GenerationStop, ReasoningPolicy, Recognizer};
    /// # use purrassist_core::{Budget, Governor};
    /// # let mut governor = Governor::new(Budget::default());
    /// # let text = "grammar \"g\"; call = \"<c>\" name:/[a-z]+/ \"</c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// # let set = DialectSet::new([DialectId::custom(Grammar::compile_governed(text, &mut governor).unwrap())], &mut governor).unwrap();
    /// let mut recognizer = Recognizer::new(&set, ReasoningPolicy::default(), &mut governor)?;
    /// recognizer.push(b"<c>a</c> and <c>b</c>", &mut governor)?;
    /// let turn = recognizer.finish(GenerationStop::Stop, &mut governor)?;
    /// let orders: Vec<usize> = turn.calls().iter().map(|call| call.readings[0].order).collect();
    /// assert_eq!(orders, [0, 1]);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub fn calls(&self) -> Vec<&CallSegment> {
        self.selected()
            .into_iter()
            .filter_map(|segment| match segment {
                Segment::Call(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    /// Whether some span has competing segmentations.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::tool_call::{DialectId, DialectSet, GenerationStop, ReasoningPolicy, Recognizer};
    /// # use purrassist_core::{Budget, Governor};
    /// # let mut governor = Governor::new(Budget::default());
    /// # let text = "grammar \"g\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// # let set = DialectSet::new([DialectId::custom(Grammar::compile_governed(text, &mut governor).unwrap())], &mut governor).unwrap();
    /// let mut recognizer = Recognizer::new(&set, ReasoningPolicy::default(), &mut governor)?;
    /// recognizer.push(b"plain text", &mut governor)?;
    /// assert!(!recognizer.finish(GenerationStop::Stop, &mut governor)?.is_ambiguous());
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub fn is_ambiguous(&self) -> bool {
        self.pieces
            .iter()
            .any(|piece| matches!(piece, Piece::Choice(_)))
    }

    /// The number of whole-turn readings: the product of every choice's
    /// reading count (saturating). Per-call readings do not multiply it.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::tool_call::{DialectId, DialectSet, GenerationStop, ReasoningPolicy, Recognizer};
    /// # use purrassist_core::{Budget, Governor};
    /// # let mut governor = Governor::new(Budget::default());
    /// # let text = "grammar \"g\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// # let set = DialectSet::new([DialectId::custom(Grammar::compile_governed(text, &mut governor).unwrap())], &mut governor).unwrap();
    /// let mut recognizer = Recognizer::new(&set, ReasoningPolicy::default(), &mut governor)?;
    /// recognizer.push(b"<c> <c>", &mut governor)?;
    /// assert_eq!(recognizer.finish(GenerationStop::Stop, &mut governor)?.readings(), 1);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub fn readings(&self) -> u128 {
        self.pieces
            .iter()
            .fold(1_u128, |product, piece| match piece {
                Piece::Choice(choice) => product.saturating_mul(choice.readings.len() as u128),
                Piece::Segment(_) => product,
            })
    }
}
