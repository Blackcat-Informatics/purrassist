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
//!    (a missing closer inserted before the next opener) are always seen;
//!    a call ended by a wrapper closer inserted before text holds every
//!    offset after it until the text is decided (another call's opener, a
//!    closer, or the end of output; see
//!    `tool_call.markup.missing-wrapper-close.before-text`).
//!    Content may not contain a complete opener; an attempt whose opener
//!    completed ends as a call (or result), or as a rejected segment
//!    costing [`cost::REJECT`](crate::grammar::cost::REJECT). Equal-cost
//!    segmentations of the same span are packed and reported as a local
//!    [`Choice`]; independent choices are never multiplied out. A choice is
//!    recognition-level: dialects sharing an opener each reject their own
//!    attempt at it, so the recognized turn keeps one segmentation per
//!    dialect. Recovery ([`ToolSet::recover`](super::ToolSet::recover))
//!    reads those that recover the same calls as one reading, keeping the
//!    attempt the body evidences.
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
//! inside a reasoning block. A block opened in the output ends at its own
//! pair's closer; a block the turn starts in ends at the first closer of a
//! pair that is not `explicit`
//! ([`CompiledGrammar::reasoning_explicit`](crate::grammar::CompiledGrammar::reasoning_explicit)):
//! an explicit pair's closer, such as Anthropic's `"}`, is ordinary JSON
//! inside a call and never ends it. Under [`ReasoningPolicy::AsContent`]
//! markers are plain content.
//!
//! # End of output
//!
//! [`GenerationStop::Length`] cuts open calls off: each becomes a
//! [`Closure::Dangling`] reading of what was captured (truncation). Every
//! other stop treats an open call as malformed: it is completed only by
//! inserting missing closers at the end ([`Closure::Unterminated`],
//! `tool_call.markup.missing-closers-at-end`, left to the completeness rule
//! of [`resolve_forest`](super::resolve_forest)) or by reinserting the
//! closer the stop sequence consumed ([`Closure::Repaired`],
//! `tool_call.stop.consumed-closer`), and is otherwise
//! [`Rejection::Unterminated`].
//!
//! # Examples in context
//!
//! A call recognized inside fenced code, an inline code span, or a block
//! quote carries the guess-flagged edit `tool_call.context.fenced-example`:
//! it may be an example rather than a call. Fences are decided over the
//! fence lines outside every recognized call and tool result, so a fence
//! inside an argument value never affects other calls.
use super::context::ContextTracker;
use super::dialect::{DialectId, DialectSet};
use super::ir::{
    ArgOrigin, ArgTree, ArgumentFault, CallSegment, Choice, Closure, Piece, ProductionKind,
    RawCall, ReasoningClosure, ReasoningSegment, Rejection, Segment,
};
use super::toolset::ToolSet;
use crate::contract::saturating_size as size;
use crate::contract::{
    Evidence, Governor, Inference, InferenceKind, Loss, Resource, ScratchMeter, SourceSpan,
    StopReason,
};
use crate::digest::{Digest, DigestBuilder};
use crate::grammar::cost::{K_CAPTURE, K_EVENT, K_LEX, QUEUED_REPORT, REASONING_UNCLOSED, REJECT};
use crate::grammar::edit::{EditOperator, MarkupEdit};
use crate::grammar::engine::{Engine, Event, Part, Salvaged};
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

/// The scratch held by queued reports ([`QUEUED_REPORT`] each), released
/// as they are processed; what is still held when the recognizer is dropped
/// unprocessed is released then.
#[derive(Debug)]
struct Queued {
    meter: ScratchMeter,
    bytes: u64,
}

impl Drop for Queued {
    fn drop(&mut self) {
        let _ = self.meter.release(self.bytes);
    }
}

// The portable per-report charge never undercounts a report's own size.
const _: () = assert!(core::mem::size_of::<Pending>() as u64 <= QUEUED_REPORT);

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
    /// Each attempt with an interpretation, until its end is reported:
    /// where it began, and its interpretations.
    interps: BTreeMap<u64, (usize, Vec<Interp>)>,
    /// The base of the last opener start that ended a content run.
    last_kill: Option<(usize, Base)>,
    /// Every stored reasoning segment: its end and store index, by its
    /// block's start. A call an attempt began inside the block may complete
    /// after the segment is stored (an attempt holding several calls ends
    /// after its last one); its parts that end inside the block are added
    /// to the segment then.
    /// Keyed by block start; each entry's scratch is covered by its stored
    /// segment's charge.
    blocks: BTreeMap<usize, Vec<(usize, Option<usize>, u32)>>,
    /// The opening reasoning markers found (start, marker pair) and the
    /// closing ones (start, end, pair), while an attempt that may end
    /// rejected across them, or read over them, is live (see
    /// [`Recognizer::ended`]).
    blocked: Vec<(usize, u32)>,
    blocked_close: Vec<(usize, usize, u32)>,
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

    /// The scratch charged for the stored segments no segmentation ending
    /// at `last` uses (a call another dialect's attempt covered, an
    /// alternative that lost on cost): it is released when the turn is
    /// built, since the recognized turn keeps only the segments its paths
    /// reach. One `Work` unit per history node visited.
    fn unused_bytes(&self, last: u32, governor: &mut Governor) -> Result<u64, StopReason> {
        let _marks = governor.lease_scratch(size(self.nodes.len() + self.store.len()))?;
        let mut seen = vec![false; self.nodes.len()];
        let mut used = vec![false; self.store.len()];
        let mut stack = vec![last];
        while let Some(node) = stack.pop() {
            if core::mem::replace(&mut seen[node as usize], true) {
                continue;
            }
            governor.charge(Resource::Work, 1)?;
            let TurnNode { prev, kind, .. } = self.nodes[node as usize];
            match kind {
                TurnKind::Root => {}
                TurnKind::Pack(other) => stack.extend([prev, other]),
                TurnKind::Content(_) => stack.push(prev),
                TurnKind::Stored(index, _) => {
                    used[index as usize] = true;
                    stack.push(prev);
                }
            }
        }
        Ok(self
            .store
            .iter()
            .zip(used)
            .filter(|(_, used)| !used)
            .map(|(segment, _)| segment_bytes(segment))
            .fold(0, u64::saturating_add))
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

    /// The reasoning segment of `think` ending at `end`; `marker` is where
    /// its closing marker begins, when a marker closed it. A call that ran
    /// into the marker is cut at it (`tool_call.reasoning.marker-cut`): the
    /// marker's bytes belong to the block.
    fn reasoning(
        &mut self,
        think: &Think,
        end: usize,
        marker: Option<usize>,
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
            let Some(mut segment) = cut_at_marker(segment, marker) else {
                continue;
            };
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
        // A call another embedded call contains (the bare function element
        // inside a wrapped Qwen3-Coder XML call, say) is part of that call,
        // as it would be outside reasoning.
        governor.charge(
            Resource::Work,
            size(embedded.len()).saturating_mul(size(embedded.len())),
        )?;
        let spans: Vec<Range<usize>> = embedded
            .iter()
            .map(|segment| segment.span.clone())
            .collect();
        embedded.retain(|segment| {
            !spans.iter().any(|outer| {
                *outer != segment.span
                    && outer.start <= segment.span.start
                    && segment.span.end <= outer.end
            })
        });
        // One call per span, in span order: two attempts that read the same
        // call (a call of an attempt reading several, and the attempt its
        // own opener began) offer it once, the first one completed.
        embedded.sort_by_key(|segment| (segment.span.start, segment.span.end));
        embedded.dedup_by(|later, first| later.span == first.span);
        let index = self.store(
            Segment::Reasoning(ReasoningSegment {
                span: think.start..end,
                closure,
                embedded,
                edits,
            }),
            governor,
        )?;
        self.blocks
            .entry(think.start)
            .or_default()
            .push((end, marker, index));
        Ok(index)
    }

    /// Add the call or result stored at `part`, which an attempt begun in
    /// the reasoning block starting at `start` completed after some of the
    /// block's segments were stored, to each of them it ends inside, as
    /// [`Self::reasoning`] would have, had it completed before.
    fn embed_late(
        &mut self,
        start: usize,
        part: u32,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let (Segment::Call(call) | Segment::Result(call)) = &self.store[part as usize] else {
            return Ok(());
        };
        let blocks: Vec<(Option<usize>, u32)> = self
            .blocks
            .get(&start)
            .into_iter()
            .flatten()
            .filter(|(end, _, _)| call.span.end <= *end)
            .map(|&(_, marker, index)| (marker, index))
            .collect();
        if blocks.is_empty() {
            return Ok(());
        }
        let whole = call.clone();
        for (marker, index) in blocks {
            governor.charge(Resource::Work, 1)?;
            let Some(call) = cut_at_marker(&whole, marker) else {
                continue;
            };
            let Segment::Reasoning(reasoning) = &mut self.store[index as usize] else {
                continue;
            };
            governor.charge(Resource::Work, size(reasoning.embedded.len()))?;
            // A call another embedded call contains is part of that call,
            // and a call already offered (another attempt read it) is not
            // offered again.
            if reasoning
                .embedded
                .iter()
                .any(|outer| outer.span.start <= call.span.start && call.span.end <= outer.span.end)
            {
                continue;
            }
            let before = segment_bytes(&self.store[index as usize]);
            let Segment::Reasoning(reasoning) = &mut self.store[index as usize] else {
                continue;
            };
            reasoning.embedded.retain(|inner| {
                !(call.span.start <= inner.span.start && inner.span.end <= call.span.end)
            });
            let mut segment = call.clone();
            for reading in &mut segment.readings {
                reading.edits.push(MarkupEdit {
                    operator: EditOperator::ReasoningEmbeddedCall,
                    span: segment.span.clone(),
                    original: Vec::new(),
                    replacement: Vec::new(),
                });
            }
            let at = reasoning.embedded.partition_point(|other| {
                (other.span.start, other.span.end) < (segment.span.start, segment.span.end)
            });
            reasoning.embedded.insert(at, segment);
            let after = segment_bytes(&self.store[index as usize]);
            governor.charge(Resource::ScratchBytes, after.saturating_sub(before))?;
        }
        Ok(())
    }
}

/// `segment` cut at a reasoning block's closing marker at `marker`
/// (`tool_call.reasoning.marker-cut`): unchanged when it ends before the
/// marker (or no marker closed the block), `None` when it begins inside it.
/// A cut reading keeps what it read before the marker: a name, id or
/// capture reaching into the marker is dropped, and arguments read from the
/// marker's bytes are not arguments (the reading's arguments are then
/// unresolved at the marker), so a call whose arguments came from the
/// marker is never usable.
/// The turn-level segment of one part of a completed attempt of `dialect`.
fn part_segment(kind: ProductionKind, dialect: &DialectId, part: Part) -> Segment {
    match part {
        Part::Call(segment) => match kind {
            ProductionKind::Call => Segment::Call(segment),
            ProductionKind::Result => Segment::Result(segment),
        },
        Part::Rejected { span, reason } => Segment::Rejected {
            span,
            dialect: dialect.clone(),
            reason,
        },
    }
}

/// An attempt that ended, as the engine reported it at `at`.
#[derive(Clone, Copy)]
struct Ended {
    table: u32,
    attempt: u64,
    start: usize,
    at: usize,
    completed: bool,
    reason: super::Rejection,
    outread: bool,
}

fn cut_at_marker(segment: &CallSegment, marker: Option<usize>) -> Option<CallSegment> {
    let Some(marker) = marker.filter(|marker| segment.span.end > *marker) else {
        return Some(segment.clone());
    };
    if segment.span.start >= marker {
        return None;
    }
    let mut cut = segment.clone();
    cut.span = segment.span.start..marker;
    for reading in &mut cut.readings {
        let end = reading.span.end;
        reading.span = reading.span.start..marker;
        if reading
            .name
            .as_ref()
            .is_some_and(|name| name.span.end > marker)
        {
            reading.name = None;
        }
        if reading.id.as_ref().is_some_and(|id| id.span.end > marker) {
            reading.id = None;
        }
        let lost = reading
            .captures
            .iter()
            .any(|capture| capture.span.end > marker)
            || reading
                .arguments
                .root
                .span
                .as_ref()
                .is_some_and(|span| span.end > marker);
        reading
            .captures
            .retain(|capture| capture.span.end <= marker);
        if lost {
            reading.arguments = ArgTree::unresolved(
                ArgumentFault::InvalidJson { at: marker },
                marker..end.max(marker),
            );
        }
        reading.edits.retain(|edit| edit.span.end <= marker);
        reading.edits.push(MarkupEdit {
            operator: EditOperator::ReasoningMarkerCut,
            span: marker..end.max(marker),
            original: Vec::new(),
            replacement: Vec::new(),
        });
        if reading.closure == Closure::Closed {
            reading.closure = Closure::Repaired;
        }
    }
    Some(cut)
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
    queued: Queued,
    turn: Turn,
    context: ContextTracker,
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
            blocks: BTreeMap::new(),
            blocked: Vec::new(),
            blocked_close: Vec::new(),
        };
        Ok(Self {
            set,
            policy,
            engine,
            digest: DigestBuilder::new(),
            marker: 0,
            pending: Vec::new(),
            queued: Queued {
                meter: governor.scratch_meter(),
                bytes: 0,
            },
            turn,
            context: ContextTracker::new(),
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
        self.context.step(byte, at, governor)?;
        self.engine.push(byte, governor)?;
        if let (ReasoningPolicy::Separate { .. }, Some(matcher)) =
            (self.policy, &self.set.marker_matcher)
        {
            self.marker = matcher.step(self.marker, byte);
            if let Some((literal, length)) = matcher.found(self.marker) {
                let (start, pair, open) = (at + 1 - length as usize, literal / 2, literal % 2 == 0);
                // Every marker, as it is found: an attempt reading over it
                // may end rejected across it before its report is applied
                // (see `Self::ended`).
                governor.charge(Resource::ScratchBytes, BLOCKED_BYTES)?;
                if open {
                    self.turn.blocked.push((start, pair));
                    // A value read on past a closer never runs past it.
                    self.engine.stop_past_at(start, governor)?;
                } else {
                    self.turn.blocked_close.push((start, at + 1, pair));
                }
                let marker = Pending::Marker {
                    start,
                    end: at + 1,
                    pair,
                    open,
                };
                self.queue(marker, governor)?;
            }
        }
        self.queue_events(governor)?;
        if let Some(limit) = (at + 1).checked_sub(self.set.lookbehind) {
            // A call whose inserted wrapper closer is still being decided
            // holds every report after its end.
            let limit = self
                .engine
                .held_floor()
                .map_or(limit, |floor| limit.min(floor.saturating_sub(1)));
            self.settle(limit, governor)?;
        }
        Ok(())
    }

    /// Queue one report, charging [`QUEUED_REPORT`] scratch bytes until it
    /// is processed: reports wait at most the lookbehind, or behind a call
    /// whose inserted wrapper closer is still being decided, and a queue too
    /// long for the budget is `BudgetExceeded(ScratchBytes)`.
    fn queue(&mut self, pending: Pending, governor: &mut Governor) -> Result<(), StopReason> {
        governor.charge(Resource::ScratchBytes, QUEUED_REPORT)?;
        self.queued.bytes += QUEUED_REPORT;
        self.pending.push(pending);
        Ok(())
    }

    /// Queue every report the engine produced.
    fn queue_events(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        let events = core::mem::take(&mut self.engine.events);
        for event in events {
            self.queue(Pending::Engine(event), governor)?;
        }
        Ok(())
    }

    /// Process every pending report at an offset up to `limit`, releasing
    /// each one's queue charge.
    fn settle(&mut self, limit: usize, governor: &mut Governor) -> Result<(), StopReason> {
        if !self.pending.iter().any(|pending| pending.at() <= limit) {
            return Ok(());
        }
        let (ready, rest): (Vec<Pending>, Vec<Pending>) = core::mem::take(&mut self.pending)
            .into_iter()
            .partition(|pending| pending.at() <= limit);
        self.pending = rest;
        let released = size(ready.len()).saturating_mul(QUEUED_REPORT);
        governor.release_scratch(released)?;
        self.queued.bytes -= released;
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
                    parts,
                    charged,
                    ..
                }) => {
                    let kind = self.engine.kind(table);
                    let dialect = self.engine.dialect(table).clone();
                    let parts = parts
                        .into_iter()
                        .map(|(part, cost)| (part_segment(kind, &dialect, part), cost))
                        .collect();
                    self.completed(&mut frame, attempt, parts, charged, governor)?;
                }
                Pending::Engine(Event::Ended {
                    table,
                    attempt,
                    start,
                    completed,
                    reason,
                    salvage,
                    outread,
                    ..
                }) => {
                    let ended = Ended {
                        table,
                        attempt,
                        start,
                        at,
                        completed,
                        reason,
                        outread,
                    };
                    self.ended(&mut frame, ended, salvage, governor)?;
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
        if interps.is_empty() {
            // No reading of the turn can take this attempt: it opened
            // inside another attempt's committed span. Its completion or
            // rejection would be dropped unread, so the engine stops
            // reading it.
            self.engine.abandon(attempt, governor)?;
        } else {
            self.turn.interps.insert(attempt, (start, interps));
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
            None,
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

    /// The segments of `attempt` ended: one call (or result), or each
    /// call of an attempt whose production separates calls with `next`,
    /// in order, each with its cost.
    fn completed(
        &mut self,
        frame: &mut Frame,
        attempt: u64,
        parts: Vec<(Segment, u64)>,
        charged: u64,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Some((_, interps)) = self.turn.interps.get(&attempt).cloned() else {
            // An attempt with no interpretation (it began inside another
            // attempt's committed span): its parts are dropped unread.
            drop(parts);
            return governor.release_scratch(charged);
        };
        // An opening reasoning marker a call read over as markup, not as
        // the data of a string value, cuts it (see `marker_cut`).
        let cut = self.marker_cut(&parts);
        let mut stored = Vec::with_capacity(parts.len());
        let mut cut_part = None;
        for (index, (segment, cost)) in parts.into_iter().enumerate() {
            if let Some((part, marker, _)) = cut
                && part == index
                && let Segment::Call(call) | Segment::Result(call) = &segment
                && let Some(trimmed) = cut_at_marker(call, Some(marker))
            {
                let trimmed = match segment {
                    Segment::Result(_) => Segment::Result(trimmed),
                    _ => Segment::Call(trimmed),
                };
                cut_part = Some((self.turn.store(trimmed, governor)?, cost, marker));
            }
            let end = segment.span().end;
            stored.push((self.turn.store(segment, governor)?, cost, end));
        }
        for interp in interps {
            match interp {
                Interp::Top(base) => {
                    let mut chain = base;
                    let (kept, open) = match (cut, cut_part) {
                        (Some((part, marker, pair)), Some(trimmed)) => {
                            (&stored[..part], Some((trimmed, marker, pair)))
                        }
                        (Some((part, marker, pair)), None) => {
                            (&stored[..part], Some(((u32::MAX, 0, marker), marker, pair)))
                        }
                        _ => (&stored[..], None),
                    };
                    for &(index, cost, end) in kept {
                        let (cost, hist) = self.turn.append(chain, index, cost, end, governor)?;
                        chain = Base { cost, hist };
                    }
                    let Some(((index, cost, end), marker, pair)) = open else {
                        frame.candidates.push((chain.cost, chain.hist));
                        continue;
                    };
                    if index != u32::MAX {
                        let (cost, hist) = self.turn.append(chain, index, cost, end, governor)?;
                        chain = Base { cost, hist };
                    }
                    let think = Think {
                        cost: chain.cost,
                        hist: chain.hist,
                        start: marker,
                        pair: Some(pair),
                        promoted: None,
                        embedded: Vec::new(),
                    };
                    self.offer_think(frame, think, governor)?;
                }
                Interp::Embedded(think_start) => {
                    if let Some(think) = self.turn.think.as_mut()
                        && think.start == think_start
                    {
                        think
                            .embedded
                            .extend(stored.iter().map(|&(index, _, _)| index));
                    }
                    // Segments of the block stored before the attempt
                    // ended (the block closed while it read on).
                    for &(index, _, _) in &stored {
                        self.turn.embed_late(think_start, index, governor)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Attempt `ended.attempt` has no configuration left: a rejected one is
    /// a rejected segment on every interpretation (after the calls it read
    /// whole before the call it failed in, when it `salvage`d them).
    fn ended(
        &mut self,
        frame: &mut Frame,
        ended: Ended,
        salvage: Option<Salvaged>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let removed = self.turn.interps.remove(&ended.attempt);
        // An opening reasoning marker the attempt read over cuts it: the
        // marker's bytes belong to the block, which opens where the
        // rejected attempt now ends (as a block's closing marker cuts a
        // call inside it).
        let cut = self
            .turn
            .blocked
            .iter()
            .copied()
            .filter(|&(marker, _)| ended.start < marker && marker <= ended.at)
            .min();
        // The block's own closing marker, when it came before the end.
        let close = cut.and_then(|(marker, pair)| {
            self.turn
                .blocked_close
                .iter()
                .copied()
                .filter(|&(start, end, closer)| closer == pair && start > marker && end <= ended.at)
                .min()
        });
        self.prune_blocked(governor)?;
        let Some((_, interps)) = removed else {
            if let Some(salvage) = salvage {
                governor.release_scratch(salvage.charged)?;
            }
            return Ok(());
        };
        if ended.completed {
            return Ok(());
        }
        // Body evidence: another dialect's attempt that began at the same
        // opener read past where this one failed and read a whole call (see
        // `Engine::defer`), so the bytes after the opener are that dialect's
        // markup. This rejection is recorded (it names the dialect among the
        // opener's candidates) but is not a reading of the opener: a reading
        // that rejected the opener here and read the bytes after it as other
        // calls would rest on strictly less grammar evidence than the
        // attempt that read further.
        let outread = ended.outread;
        let dialect = self.engine.dialect(ended.table).clone();
        if let Some(salvage) = salvage {
            let kind = self.engine.kind(ended.table);
            return self.salvaged(
                frame,
                &interps,
                kind,
                salvage,
                ended.at,
                &dialect,
                ended.reason,
                governor,
            );
        }
        if let Some((marker, pair)) = cut
            && !outread
        {
            return self.cut_rejected(
                frame,
                &ended,
                &interps,
                dialect,
                (marker, pair),
                close,
                governor,
            );
        }
        let segment = Segment::Rejected {
            span: ended.start..ended.at,
            dialect,
            reason: ended.reason,
        };
        let index = self.turn.store(segment, governor)?;
        if outread {
            return Ok(());
        }
        for interp in interps {
            if let Interp::Top(base) = interp {
                frame
                    .candidates
                    .push(self.turn.append(base, index, REJECT, ended.at, governor)?);
            }
        }
        Ok(())
    }

    /// The first part of a completed attempt that read over an opening
    /// reasoning marker (seen outside reasoning while it was reading) as
    /// data it should not hold: the marker lies inside a capture or the
    /// arguments the part read (not in markup its grammar spells, as a
    /// Harmony header's channel is), but not inside the raw bytes of a
    /// string value (a JSON string, a verbatim parameter): the part, the
    /// marker's start and its pair. The marker's
    /// bytes belong to the block it opens, as a block's closing marker cuts
    /// a call inside the block (`tool_call.reasoning.marker-cut`).
    fn marker_cut(&self, parts: &[(Segment, u64)]) -> Option<(usize, usize, u32)> {
        if self.turn.blocked.is_empty() {
            return None;
        }
        for (index, (segment, _)) in parts.iter().enumerate() {
            let (Segment::Call(call) | Segment::Result(call)) = segment else {
                continue;
            };
            let found = self
                .turn
                .blocked
                .iter()
                .copied()
                .filter(|&(marker, _)| call.span.start < marker && marker < call.span.end)
                .filter(|&(marker, _)| {
                    // Read as data: inside a capture or the arguments' bytes
                    // (not markup its grammar spells, which is the call's),
                    // and not inside a string value.
                    let in_data = |reading: &RawCall| {
                        reading.captures.iter().any(|capture| {
                            capture.span.start <= marker && marker < capture.span.end
                        }) || reading
                            .arguments
                            .root
                            .span
                            .as_ref()
                            .is_some_and(|span| span.start <= marker && marker < span.end)
                    };
                    call.readings.iter().any(in_data)
                        && !call.readings.iter().all(|reading| {
                            reading.arguments.leaves.iter().any(|leaf| {
                                leaf.raw.start <= marker
                                    && marker < leaf.raw.end
                                    && match leaf.origin {
                                        ArgOrigin::Raw | ArgOrigin::XmlText => true,
                                        ArgOrigin::Json | ArgOrigin::StringifiedJson => matches!(
                                            reading
                                                .arguments
                                                .root
                                                .pointer(&leaf.pointer)
                                                .map(|node| &node.value),
                                            Some(crate::json::Value::String(_))
                                        ),
                                    }
                            })
                        })
                })
                .min();
            if let Some((marker, pair)) = found {
                return Some((index, marker, pair));
            }
        }
        None
    }

    /// The rejected attempt `ended`, which ran over the opening reasoning
    /// marker `opener` (its start and pair), cut at it on every
    /// interpretation: the block it opens follows, closed at `close` (the
    /// block's closing marker's start and end) when that came before the
    /// attempt's end, else left open (see [`Self::ended`]).
    #[allow(clippy::too_many_arguments)]
    fn cut_rejected(
        &mut self,
        frame: &mut Frame,
        ended: &Ended,
        interps: &[Interp],
        dialect: DialectId,
        opener: (usize, u32),
        close: Option<(usize, usize, u32)>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let (marker, pair) = opener;
        let reason = match ended.reason {
            Rejection::Malformed { at } => Rejection::Malformed { at: at.min(marker) },
            Rejection::InvalidUtf8 { at } => Rejection::InvalidUtf8 { at: at.min(marker) },
            Rejection::Overlapped { at } => Rejection::Overlapped { at: at.min(marker) },
            other => other,
        };
        let segment = Segment::Rejected {
            span: ended.start..marker,
            dialect,
            reason,
        };
        let index = self.turn.store(segment, governor)?;
        for &interp in interps {
            if let Interp::Top(base) = interp {
                let (cost, hist) = self.turn.append(base, index, REJECT, marker, governor)?;
                let think = Think {
                    cost,
                    hist,
                    start: marker,
                    pair: Some(pair),
                    promoted: None,
                    embedded: Vec::new(),
                };
                match close {
                    Some((start, end, _)) => {
                        let segment = self.turn.reasoning(
                            &think,
                            end,
                            Some(start),
                            ReasoningClosure::Closed,
                            Vec::new(),
                            governor,
                        )?;
                        let (cost, mut hist) =
                            self.turn
                                .append(Base { cost, hist }, segment, 0, end, governor)?;
                        if end < ended.at {
                            hist =
                                self.turn
                                    .push(hist, ended.at, TurnKind::Content(end), governor)?;
                        }
                        frame.candidates.push((cost, hist));
                    }
                    None => self.offer_think(frame, think, governor)?,
                }
            }
        }
        Ok(())
    }

    /// Drop the blocked reasoning markers no live attempt began before.
    fn prune_blocked(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        let earliest = self.turn.interps.values().map(|(start, _)| *start).min();
        let before = self.turn.blocked.len() + self.turn.blocked_close.len();
        let live = |marker: usize| earliest.is_some_and(|start| start < marker);
        self.turn.blocked.retain(|&(marker, _)| live(marker));
        self.turn
            .blocked_close
            .retain(|&(marker, _, _)| live(marker));
        let after = self.turn.blocked.len() + self.turn.blocked_close.len();
        governor.release_scratch(size(before - after).saturating_mul(BLOCKED_BYTES))
    }

    /// Offer `think`, a block opened where a rejected attempt was cut, as
    /// the open block: the cheaper one stays, and an equally cheap one at
    /// the same start is packed with it.
    fn offer_think(
        &mut self,
        frame: &mut Frame,
        think: Think,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let current = frame.think.clone().or_else(|| self.turn.think.clone());
        match current {
            Some(existing) if existing.cost < think.cost => {}
            Some(existing) if existing.cost == think.cost && existing.start == think.start => {
                let hist = self.turn.push(
                    existing.hist,
                    think.start,
                    TurnKind::Pack(think.hist),
                    governor,
                )?;
                frame.think = Some(Think { hist, ..existing });
            }
            _ => frame.think = Some(think),
        }
        Ok(())
    }

    /// A rejected attempt of a production with `next` that read whole
    /// calls before the call it failed in (`tool_call.markup.group-split`):
    /// each segmentation of those calls, then the rejected attempt from the
    /// failing call's start to `at` (none when it failed at its first
    /// byte), on every interpretation of the attempt. Inside reasoning the
    /// calls are offered as embedded calls.
    #[allow(clippy::too_many_arguments)]
    fn salvaged(
        &mut self,
        frame: &mut Frame,
        interps: &[Interp],
        kind: ProductionKind,
        salvage: Salvaged,
        at: usize,
        dialect: &DialectId,
        reason: super::Rejection,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let rejected = if salvage.at < at {
            Some(self.turn.store(
                Segment::Rejected {
                    span: salvage.at..at,
                    dialect: dialect.clone(),
                    reason,
                },
                governor,
            )?)
        } else {
            None
        };
        for parts in salvage.segmentations {
            let mut stored = Vec::with_capacity(parts.len());
            for (part, cost) in parts {
                let segment = part_segment(kind, dialect, part);
                let end = segment.span().end;
                stored.push((self.turn.store(segment, governor)?, cost, end));
            }
            for interp in interps {
                match *interp {
                    Interp::Top(base) => {
                        let mut chain = base;
                        for &(index, cost, end) in &stored {
                            let (cost, hist) =
                                self.turn.append(chain, index, cost, end, governor)?;
                            chain = Base { cost, hist };
                        }
                        if let Some(index) = rejected {
                            let (cost, hist) =
                                self.turn.append(chain, index, REJECT, at, governor)?;
                            chain = Base { cost, hist };
                        }
                        frame.candidates.push((chain.cost, chain.hist));
                    }
                    Interp::Embedded(think_start) => {
                        if let Some(think) = self.turn.think.as_mut()
                            && think.start == think_start
                        {
                            think
                                .embedded
                                .extend(stored.iter().map(|&(index, _, _)| index));
                        }
                        for &(index, _, _) in &stored {
                            self.turn.embed_late(think_start, index, governor)?;
                        }
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
        // An opened block ends at its own pair's closer; a block the turn
        // starts in (no opener) at the closer of a pair a prompt template
        // can open, never at an `explicit` pair's (Anthropic's `"}` is also
        // ordinary JSON inside a call).
        let closes = match think.pair {
            Some(open) => open == pair,
            None => !self
                .set
                .explicit
                .get(pair as usize)
                .copied()
                .unwrap_or(false),
        };
        if !closes || think.start > start {
            return Ok(());
        }
        let segment = self.turn.reasoning(
            &think,
            at,
            Some(start),
            ReasoningClosure::Closed,
            Vec::new(),
            governor,
        )?;
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
            None,
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
        self,
        stop: GenerationStop,
        governor: &mut Governor,
    ) -> Result<RecognizedTurn, StopReason> {
        self.finish_over(stop, None, governor)
    }

    /// [`Self::finish`], given the whole `output` when the caller holds it:
    /// a rejected attempt's markup then extends as far as recovery reads it
    /// (`RejectedCall::span`) when fences inside it are kept out of the
    /// prose context; without it, the span its grammar read is used.
    pub(crate) fn finish_over(
        mut self,
        stop: GenerationStop,
        output: Option<&[u8]>,
        governor: &mut Governor,
    ) -> Result<RecognizedTurn, StopReason> {
        if let Some(reason) = self.failed.take() {
            return Err(reason);
        }
        let end = self.engine.fed();
        self.context.finish(end, governor)?;
        self.engine.finish(&stop, governor)?;
        self.queue_events(governor)?;
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
            context: &self.context,
            len: end,
            output,
        };
        let pieces = builder.pieces(last, governor)?;
        let Builder { forest, .. } = builder;
        let unused = self.turn.unused_bytes(last, governor)?;
        governor.release_scratch(unused)?;
        // Every dialect whose attempt at an offset was rejected, in any
        // segmentation (a costlier one included).
        let mut attempted: Vec<(usize, DialectId)> = Vec::new();
        for segment in &self.turn.store {
            governor.charge(Resource::Work, 1)?;
            if let Segment::Rejected { span, dialect, .. } = segment
                && !attempted
                    .iter()
                    .any(|(start, known)| *start == span.start && known == dialect)
            {
                governor.charge(Resource::ScratchBytes, ATTEMPT_BYTES)?;
                attempted.push((span.start, dialect.clone()));
            }
        }
        attempted.sort();
        Ok(RecognizedTurn {
            source,
            len: end,
            stop,
            policy: self.policy,
            pieces,
            forest,
            cost: top.cost,
            attempted,
        })
    }
}

/// Builds the factorized pieces and their derivations.
struct Builder<'t> {
    turn: &'t Turn,
    forest: Forest,
    source: Digest,
    context: &'t ContextTracker,
    len: usize,
    /// The whole output, when the caller holds it.
    output: Option<&'t [u8]>,
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
        self.mark_examples(&mut pieces, governor)?;
        self.annotate(&mut pieces, governor)?;
        Ok(pieces)
    }

    /// Flag every call recognized inside fenced code, an inline code span or
    /// a block quote with the guess-flagged edit
    /// `tool_call.context.fenced-example` (see [`ContextTracker`]).
    fn mark_examples(
        &self,
        pieces: &mut [Piece],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        // The selected reading's segments, in order.
        let mut reading: Vec<&Segment> = Vec::new();
        for piece in pieces.iter() {
            match piece {
                Piece::Segment(segment) => reading.push(segment),
                Piece::Choice(choice) => reading.extend(&choice.readings[0]),
            }
        }
        let mut excluded = Vec::new();
        for (index, segment) in reading.iter().enumerate() {
            match segment {
                Segment::Call(call) | Segment::Result(call) => excluded.push(call.span.clone()),
                // A rejected attempt's bytes are call markup, not prose: a
                // fence inside it opens no fenced block. With the whole
                // output, its markup extends as recovery reads it.
                Segment::Rejected {
                    span,
                    dialect,
                    reason,
                } => {
                    let end = match self.output {
                        Some(output) => {
                            let boundaries: Vec<(Range<usize>, bool)> = reading[index + 1..]
                                .iter()
                                .filter_map(|later| match later {
                                    Segment::Call(call) | Segment::Result(call) => {
                                        Some((call.span.clone(), true))
                                    }
                                    Segment::Rejected { span, .. } => Some((span.clone(), true)),
                                    Segment::Reasoning(reasoning) => {
                                        Some((reasoning.span.clone(), false))
                                    }
                                    Segment::Content(_) => None,
                                })
                                .filter(|(range, _)| range.start >= span.end && !range.is_empty())
                                .collect();
                            super::recover::rejected_end(
                                output,
                                span,
                                dialect,
                                *reason,
                                &boundaries,
                                governor,
                            )?
                        }
                        None => span.end,
                    };
                    excluded.push(span.start..end.max(span.end));
                }
                Segment::Content(_) | Segment::Reasoning(_) => {}
            }
        }
        // A rejected attempt's markup may cover calls inside it: one range
        // per covered stretch, sorted by start.
        excluded.sort_by_key(|range| range.start);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(excluded.len());
        for range in excluded {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        let excluded = merged;
        let mut calls: Vec<&mut CallSegment> = Vec::new();
        for piece in pieces.iter_mut() {
            match piece {
                Piece::Segment(Segment::Call(call)) => calls.push(call),
                Piece::Choice(choice) => {
                    for reading in &mut choice.readings {
                        for segment in reading {
                            if let Segment::Call(call) = segment {
                                calls.push(call);
                            }
                        }
                    }
                }
                Piece::Segment(_) => {}
            }
        }
        let spans: Vec<Range<usize>> = calls.iter().map(|call| call.span.clone()).collect();
        let contexts = self
            .context
            .examples(&spans, &excluded, self.len, governor)?;
        for (call, context) in calls.into_iter().zip(contexts) {
            let Some(region) = context else {
                continue;
            };
            for reading in &mut call.readings {
                let edit = MarkupEdit {
                    operator: EditOperator::ContextFencedExample,
                    span: region.clone(),
                    original: Vec::new(),
                    replacement: Vec::new(),
                };
                governor.charge(Resource::ScratchBytes, edit.estimated_bytes())?;
                let at = reading
                    .edits
                    .iter()
                    .position(|other| other.span.start > region.start)
                    .unwrap_or(reading.edits.len());
                reading.edits.insert(at, edit);
            }
        }
        Ok(())
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
        let (evidence, loss, inference) = edit_entries(edit, within, self.source);
        let mut nodes = vec![self.forest.evidence(governor, evidence)?];
        if let Some(loss) = loss {
            nodes.push(self.forest.loss(governor, loss)?);
        }
        if let Some(inference) = inference {
            nodes.push(self.forest.inference(governor, inference)?);
        }
        Ok(nodes)
    }
}

/// The ledger entries of one markup edit applied within the segment at
/// `within` of the output whose digest is `source`: its evidence, a loss of
/// the original bytes when the edit's rule loses information, and a guess
/// inference of the replacement when the rule guesses.
pub(super) fn edit_entries(
    edit: &MarkupEdit,
    within: &Range<usize>,
    source: Digest,
) -> (Evidence, Option<Loss>, Option<Inference>) {
    let rule = edit.rule();
    let evidence = Evidence {
        rule: rule.id.into(),
        explanation: explain(edit),
        span: Some(SourceSpan {
            source,
            bytes: edit.span.clone(),
        }),
        destination: format!("/turn/{}..{}", within.start, within.end),
    };
    let loss = rule.loss.is_some().then(|| Loss {
        evidence: evidence.clone(),
        original: escaped(&edit.original),
    });
    let inference = rule.guess.then(|| Inference {
        evidence: evidence.clone(),
        kind: InferenceKind::Guess,
        value: escaped(&edit.replacement),
    });
    (evidence, loss, inference)
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
pub(super) fn escaped(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        for byte in chunk.invalid() {
            write!(text, "\\x{byte:02X}").expect("writing to a String is infallible");
        }
    }
    text
}

/// The explanation of an edit that keeps a call recoverable: an attempt
/// split before the call it could not read, a misspelled closer, or a call
/// cut at a reasoning marker.
fn explain_recovery(edit: &MarkupEdit, original: &str, replacement: &str) -> String {
    match edit.operator {
        EditOperator::GroupSplit => format!(
            "split the attempt reading several calls at byte {}, beside a call it could not read; the calls it read whole are recovered, and the failing call is a rejected attempt of its own",
            edit.span.start
        ),
        EditOperator::CloserMisspelled => format!(
            "read misspelled closer `{original}` as `{replacement}`, the only closer of the grammar within edit distance one"
        ),
        EditOperator::TokenMisspelled => format!(
            "read misspelled token `{original}` as `{replacement}`, the only token of the grammar within edit distance one"
        ),
        EditOperator::CloserInValue => format!(
            "read the closer spelling at byte {} as text of the value, which ends at byte {}: the reading that ends the value at byte {} was weighed against this one by the repairs each needs; when it needs no more, it is reported beside this one",
            edit.span.start, edit.span.end, edit.span.start
        ),
        _ => format!(
            "cut the call at the reasoning block's marker at byte {}: the marker belongs to the block, so what the call read from bytes {}..{} is dropped",
            edit.span.start, edit.span.start, edit.span.end
        ),
    }
}

fn explain(edit: &MarkupEdit) -> String {
    let replacement = escaped(&edit.replacement);
    let original = escaped(&edit.original);
    match edit.operator {
        EditOperator::MissingFunctionClose => {
            format!("inserted missing closer `{replacement}` before the markup that follows it")
        }
        EditOperator::MissingWrapperClose => {
            format!("inserted the call's missing final closer `{replacement}`")
        }
        EditOperator::MissingToken => {
            format!("read the omitted token `{replacement}` as inserted here")
        }
        EditOperator::InvalidUtf8Value => format!(
            "read bytes `{original}`, which are not well-formed UTF-8, as `{replacement}` (U+FFFD per malformed sequence); the original bytes are retained in this edit and in the receipt's source"
        ),
        EditOperator::MissingWrapperCloseBeforeText => format!(
            "inserted the call's missing final closer `{replacement}` before text that no closer follows; the text may instead be inside a wrapper that never closes"
        ),
        EditOperator::MissingClosersAtEnd => format!(
            "inserted missing closer `{replacement}` at the end of output; usable only when the call is otherwise complete"
        ),
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
            "ended the open parameter value before the opener of a parameter the tool declares (inserted `{replacement}`); the alternative reading, that the value contains that opener as text, is not offered because a declared parameter's opener always ends an XML parameter value"
        ),
        EditOperator::ParameterClosedByElement => format!(
            "ended the open parameter value where its enclosing element closes (inserted `{replacement}`)"
        ),
        EditOperator::UndeclaredParameterBoundary => format!(
            "ended the open parameter value at a parameter opener of a call that names no declared tool (inserted `{replacement}`); the opener may also be text of the value"
        ),
        EditOperator::ClosersOnlyEmpty => format!(
            "read a function body of only closers `{original}` as no arguments; valid only when the tool requires no parameter"
        ),
        EditOperator::NestedParameter => format!(
            "read nested parameter elements after `{original}` as the members of an object parameter"
        ),
        EditOperator::JsonBody => {
            "read a whole JSON object in the function body as the arguments".into()
        }
        EditOperator::ArrayItem => {
            format!("read array parameter value `{original}` as one item of the array")
        }
        EditOperator::TrimNonverbatim => format!(
            "trimmed whitespace around the value `{original}` of a non-verbatim declared type to `{replacement}`"
        ),
        EditOperator::FramingNewline => {
            "consumed one framing newline of an XML parameter value".into()
        }
        EditOperator::StringifiedArguments => {
            format!("decoded stringified arguments `{original}` as JSON `{replacement}`")
        }
        EditOperator::ExtractProse => {
            format!("dropped prose `{original}` inside the call's markup")
        }
        EditOperator::ExtractFence => {
            format!("dropped code-fence marker `{original}` inside the call's markup")
        }
        EditOperator::GroupSplit
        | EditOperator::CloserMisspelled
        | EditOperator::TokenMisspelled
        | EditOperator::CloserInValue
        | EditOperator::ReasoningMarkerCut => explain_recovery(edit, &original, &replacement),
        EditOperator::ContextFencedExample => {
            "call recognized inside fenced code, an inline code span or a block quote; it may be an example rather than a call".into()
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
    attempted: Vec<(usize, DialectId)>,
}

/// Scratch bytes charged per recorded rejected attempt.
const ATTEMPT_BYTES: u64 = 32;

/// Scratch bytes per blocked opening reasoning marker.
const BLOCKED_BYTES: u64 = 16;

impl PartialEq for RecognizedTurn {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.len == other.len
            && self.stop == other.stop
            && self.policy == other.policy
            && self.pieces == other.pieces
            && self.cost == other.cost
            && self.attempted == other.attempted
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

    /// Every `(offset, dialect)` whose attempt starting at that offset was
    /// rejected in some segmentation of the turn, a costlier one included,
    /// in order. Recovery names these dialects in a reported rejected
    /// attempt's `candidate_dialects`: a dialect that attempted the opener
    /// is named even when another segmentation explains the bytes more
    /// cheaply.
    ///
    /// ```
    /// use purrassist_core::tool_call::{DialectId, DialectSet, GenerationStop, ReasoningPolicy, Recognizer};
    /// use purrassist_core::{Budget, Governor};
    /// let mut governor = Governor::new(Budget::default());
    /// let set = DialectSet::new([DialectId::Hermes, DialectId::Qwen3CoderXml], &mut governor).unwrap();
    /// let mut recognizer = Recognizer::new(&set, ReasoningPolicy::default(), &mut governor)?;
    /// recognizer.push(b"<tool_call>\n<function=f>\n<parameter=p>\nx", &mut governor)?;
    /// let turn = recognizer.finish(GenerationStop::Stop, &mut governor)?;
    /// // Both dialects attempted `<tool_call>`; after Hermes's rejection the
    /// // bare `<function=f>` element is a Qwen3-Coder XML attempt of its own.
    /// assert_eq!(
    ///     turn.rejected_attempts(),
    ///     [(0, DialectId::Hermes), (0, DialectId::Qwen3CoderXml), (12, DialectId::Qwen3CoderXml)].as_slice()
    /// );
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub fn rejected_attempts(&self) -> &[(usize, DialectId)] {
        &self.attempted
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

    /// Whether some span has competing segmentations. Competing
    /// segmentations that recover the same calls (for example, one rejected
    /// attempt per dialect sharing an opener) are one reading of the
    /// recovered turn ([`ToolSet::recover`](super::ToolSet::recover)), so a
    /// recognized ambiguity need not make recovery `Ambiguous`.
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
