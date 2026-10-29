// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The segment-candidate layer of the recognizer: one interpreter over the
//! compiled programs of every dialect production in a set.
//!
//! Each `(dialect, production)` pair has a *table* of live configurations
//! ([`Thread`]s). A byte that can begin a production's opener spawns a new
//! *attempt* there; every live configuration of every table consumes every
//! byte, in canonical table order. Configurations at the same program
//! location (and literal position) are merged: the earliest attempt wins,
//! then the least cost, then the earliest terminal entry; exact ties are
//! *packed* (their histories become one node with two predecessors), which
//! keeps the configuration count per table bounded by
//! [`CompiledGrammar::configuration_bound`] and so the work per byte
//! constant.
//!
//! A configuration may lag behind the input after a delimiter (an
//! `until`/`slot` body ends where its delimiter *begins*, so the
//! continuation replays the delimiter's bytes) or a repair; it catches up
//! within the same byte step from the capture window, which holds the bytes
//! of every live attempt. Zero-width cycles are cut by a stall bound.
//!
//! When every configuration of an attempt fails on a byte, the repair search
//! (`super::repair`) runs over the failed configurations. A completed
//! configuration is held while the bytes after it spell closers of its
//! grammar, separated by bounded whitespace, which it absorbs as stray
//! closers (a `Trail`); it is reported once the following bytes cannot
//! continue a gap and a closer spelling, within the recognizer's lookbehind.
//! A completion that ended its call by inserting the wrapper's closer
//! mid-message is then held until the bytes after it decide the insertion
//! (another call's opener or the end of output: kept; text and then a
//! closer: the attempt is rejected; text and no closer: a guess), and the
//! turn layer settles nothing after the call's end meanwhile. An
//! attempt that completes, is rejected, or is absorbed emits [`Event`]s that
//! the turn-level segmentation consumes. This layer never depends on the turn
//! level, so its events are a pure function of the bytes.
//!
//! An attempt opens (is reported, and may be rejected) when its opener
//! literal completes, or, in a program with `commit` instructions, only when
//! one of its configurations passes a commit or it completes; until then its
//! failure is silent and the turn level holds every report from its start.
//!
//! A history records each `next` it passed with its offset and cost, so one
//! completed attempt of a production with `next` materializes as several
//! call segments ([`Parts`]), one per call.
//! A completion whose history is one path (or packs alternatives only within
//! a call) is segmented incrementally: each `next` node closes the call
//! before it once, into a chain shared by every completion that extends it,
//! so a chain of calls reads each call twice at most however many follow
//! (see [`Step::segment_chained`]); the completion is materialized only if
//! no later completion of its attempt dominates it. Any other history is
//! segmented whole ([`Step::segment`]).
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); these items are
// shared across the grammar and tool-call modules, so `pub(crate)` (matching
// `unreachable_pub`) wins, as in `pattern::derive`.
#![allow(clippy::redundant_pub_crate)]
use super::codec::{
    CodecContext, CodecEnd, CodecFault, CodecKind, CodecState, CodecStep, CodecValue,
};
use super::compile::JsonSpelling;
use super::cost::{K_CAPTURE, K_CODEC, K_THREAD, STRAY_CLOSER_GAP, UNDECLARED_NAME};
use super::edit::{EditOperator, MarkupEdit};
use super::repair::{RepairContext, RepairState, Status};
use super::{CaptureKind, CompiledGrammar, Inst, Program, index_u32};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchMeter, ScratchVec, StopReason};
use crate::digest::Digest;
use crate::pattern::{DerivativeDfa, DfaState};
use crate::search::Ledger;
use crate::search::Search;
use crate::tool_call::{
    ArgTree, ArgumentFault, CallSegment, CaptureSpan, Closure, CompiledTool, DialectId,
    GenerationStop, ProductionKind, RawCall, Rejection, Spanned, ToolSet,
};
use core::cmp::Reverse;
use core::ops::Range;
use std::collections::{BTreeMap, BTreeSet};

/// Set in a body configuration's matcher state once the body read on past
/// a repaired delimiter (a closer variant or confusable spelling), beside
/// the reading that ended there (see `Step::step_body`).
const PAST_REPAIRED: u32 = 1 << 31;

/// Set beside [`PAST_REPAIRED`] once such a body read text (anything but
/// whitespace, outside any delimiter) after the delimiter it read on past:
/// a verbatim value holding closers followed only by closers and whitespace
/// holds no value text, only stray markup (issue #3 §3: "repeated or stray
/// closers are all one class"), so it never ends there (see
/// `Step::end_early`).
const TEXTED: u32 = 1 << 30;

/// The root of every call history.
pub(crate) const ROOT: u32 = 0;

/// What a configuration is doing at its terminal instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sub {
    /// Matching the canonical spelling; the number of bytes matched.
    Lit(u32),
    /// Matching a declared closer variant; the trie node.
    Variant(u32),
    /// Matching a confusable spelling; the trie node.
    Confusable(u32),
    /// Deleting a stray closer before returning to this literal; the node.
    Skip(u32),
    /// Matching a JSON-syntax spelling of a `json-envelope` grammar; the
    /// table ([`JsonSpelling`](super::compile::JsonSpelling) as `u8`) and
    /// the trie node.
    Json(u8, u32),
    /// Deleting a trailing comma or a comment before this literal in a
    /// `json-envelope` grammar; the scanner state (see
    /// [`Step::step_json_skip`]).
    JsonSkip(u32),
    /// Deleting a comment inside this literal, at a JSON token boundary of
    /// it, in a `json-envelope` grammar; the scanner state and the
    /// canonical bytes matched before the comment (see
    /// [`Step::step_json_inner`]). The configuration's `entry` is where the
    /// comment began.
    JsonInner(u8, u32),
    /// Inside a class run.
    Class(DfaState),
    /// Inside an `until`/`slot` body: matcher state and, for a slot, the
    /// arena slot of its codec's state.
    Body(u32, Option<u32>),
    /// Reading a misspelled closer (`tool_call.markup.closer-misspelled`):
    /// the slip was made, and the rest of the canonical spelling is read
    /// exactly from the first index; the second is how many canonical
    /// bytes matched before the slip.
    Misspelled(u32, u32),
    /// Reading a misspelled closer whose slip swapped the canonical bytes
    /// at the first index and the next: the byte at that index comes next,
    /// then the canonical spelling resumes after the pair; the second
    /// index is how many canonical bytes matched before the slip.
    Transposed(u32, u32),
    /// Resynchronizing after a call the attempt could not read (see
    /// [`Engine::resync`]): scanning for this call-closer literal; the
    /// scanner state (bracket depth in the low 16 bits, then whether inside
    /// a JSON string and after a backslash in one) and how many of the
    /// literal's bytes matched so far.
    Resync(u32, u32),
}

impl Sub {
    /// The same literal mode at trie node `node`.
    const fn with_node(self, node: u32) -> Self {
        match self {
            Self::Variant(_) => Self::Variant(node),
            Self::Confusable(_) => Self::Confusable(node),
            Self::Skip(_) => Self::Skip(node),
            Self::Json(table, _) => Self::Json(table, node),
            other => other,
        }
    }

    /// Whether a configuration in this mode resynchronizes after a call it
    /// could not read (see `Engine::resync`).
    pub(crate) const fn resyncing(&self) -> bool {
        matches!(self, Self::Resync(..))
    }

    /// Whether a configuration in this mode reads on only beside a
    /// fallback: resynchronizing, or a body that read past a repaired
    /// delimiter (see [`PAST_REPAIRED`]). Such a configuration does not
    /// keep its attempt from being stuck, so the repairs of the readings
    /// that failed are still proposed.
    pub(crate) const fn speculative(&self) -> bool {
        match self {
            Self::Resync(..) => true,
            Self::Body(state, _) => *state & PAST_REPAIRED != 0,
            _ => false,
        }
    }

    pub(crate) const fn key(&self) -> (u8, u32) {
        match self {
            Self::Lit(node) => (0, *node),
            Self::Variant(node) => (1, *node),
            Self::Confusable(node) => (2, *node),
            Self::Skip(node) => (3, *node),
            Self::Class(_) => (4, 0),
            Self::Body(..) => (5, 0),
            Self::Json(table, node) => (6 + *table, *node),
            Self::JsonSkip(state) => (9, *state),
            Self::JsonInner(state, node) => (10 + *state, *node),
            Self::Misspelled(index, _) => (20, *index),
            Self::Transposed(index, _) => (21, *index),
            Self::Resync(state, matched) => (22, state.wrapping_mul(64).wrapping_add(*matched)),
        }
    }
}

/// One live configuration of one attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Thread {
    pub(crate) pc: u32,
    pub(crate) sub: Sub,
    pub(crate) attempt: u64,
    pub(crate) start: usize,
    /// Where the current terminal began.
    pub(crate) entry: usize,
    /// The next byte to consume.
    pub(crate) pos: usize,
    pub(crate) cost: u64,
    pub(crate) hist: u32,
    pub(crate) opened: bool,
    pub(crate) stall: u32,
    pub(crate) reach: usize,
    /// Whether the configuration descends from a body that read on past a
    /// delimiter (see [`PAST_REPAIRED`]): a reading beside the one ending
    /// there (see [`Self::beside`]).
    pub(crate) past: bool,
    /// For a reading that ended a body beside another without finishing
    /// its codec yet (see `Step::end_early`): the deferred body's index
    /// plus one, else zero. Its value is read, and its cost added, when the
    /// reading completes or begins another slot.
    pub(crate) pending: u32,
}

impl ScratchCharge for Thread {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 120);
        120
    };
}

impl Thread {
    /// Whether the configuration reads on beside a reading that ended at a
    /// delimiter: a body that read past one, or a configuration descending
    /// from it (see [`Self::past`]).
    pub(crate) const fn beside(&self) -> bool {
        self.past || matches!(self.sub, Sub::Body(state, _) if state & PAST_REPAIRED != 0)
    }

    /// Whether the configuration reads on only beside a fallback (see
    /// [`Sub::speculative`]), or beside a reading that ended at a delimiter
    /// ([`Self::beside`]): it does not keep its attempt from being stuck,
    /// so the repairs of the readings as written that failed are proposed.
    pub(crate) const fn speculative(&self) -> bool {
        self.beside() || self.sub.speculative()
    }

    /// The merge key: program location, terminal mode, literal node, the
    /// deferred body a reading carries (its cost is not known yet), and for
    /// a reading beside another its attempt (one more than its id; zero
    /// otherwise): a value read on past a closer or another call's opener
    /// is a reading of its own attempt alone, never one of a later attempt
    /// reading the same bytes as markup.
    pub(crate) const fn key(&self) -> (u32, u8, u32, u32, u64) {
        let (mode, node) = self.sub.key();
        let beside = if self.beside() { self.attempt + 1 } else { 0 };
        (self.pc, mode, node, self.pending, beside)
    }

    /// The precedence among configurations sharing a key: the earliest
    /// attempt, then the least cost, then for an `until` body the earliest
    /// entry (the longest body; it continues the same way whatever its
    /// entry) and for a slot body the latest entry (the reading whose
    /// grammar read the most bytes as markup before the value began, rather
    /// than leaving them to the codec).
    const fn rank(&self) -> (usize, u64, usize) {
        let entry = match self.sub {
            Sub::Body(_, Some(_)) => usize::MAX - self.entry,
            _ => self.entry,
        };
        (self.start, self.cost, entry)
    }
}

/// One step of a call history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistKind {
    Root,
    Open(CaptureKind, usize),
    Close(CaptureKind, usize),
    Value(u32),
    Edit(u32),
    /// Two equal-cost histories: `prev` or this node's operand.
    Pack(u32),
    /// A `next`: the call that began before ends and the next one begins at
    /// this offset, with the configuration's cost so far.
    Next(usize, u64),
    /// The call that began at the last `next` (or the attempt's start) could
    /// not be read: it failed at this offset, and the attempt
    /// resynchronized at its call closer (see `Engine::resync`). The part
    /// is a rejected attempt of its own.
    Skipped(usize),
    /// No step: the history continues at `prev`. A deferred body's value
    /// (see `Step::end_early`) stands here; once read, `prev` is the value.
    Link,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HistNode {
    pub(crate) prev: u32,
    pub(crate) kind: HistKind,
}

impl ScratchCharge for HistNode {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 32);
        32
    };
}

/// Persistent call histories, codec values, and edits of the live attempts.
/// Reset whenever no configuration is live.
pub(crate) struct Arena {
    pub(crate) nodes: ScratchVec<HistNode>,
    pub(crate) values: Vec<ArgTree>,
    pub(crate) edits: Vec<MarkupEdit>,
    /// Live codec states, out of line so a configuration stays `Copy`
    /// whatever state a codec keeps; each body configuration owns its slot.
    codecs: Vec<Option<CodecState>>,
    /// Bodies ended beside another reading whose codec has not finished
    /// yet (see `Step::end_early`).
    pub(crate) deferred: ScratchVec<Deferred>,
    /// The scratch charged for `values`, `edits` and `codecs`, released by
    /// [`Self::reset`] or, for what is still held, when the arena drops.
    charged: u64,
    meter: ScratchMeter,
    /// The closed parts of grouped attempts' histories, shared by every
    /// completion that extends them (see [`Step::segment_chained`]).
    chains: Vec<ChainNode>,
    /// The tokens read as omitted at the start of an attempt whose first
    /// call was skipped, once per such chain.
    leads: Vec<Lead>,
    /// What each `next` node of a history closes: its chain of parts, the
    /// rejection that ended it, or that it must be segmented whole.
    memo: BTreeMap<u32, Closed>,
}

impl Drop for Arena {
    fn drop(&mut self) {
        let _ = self.meter.release(self.charged);
    }
}

/// Scratch bytes charged per codec slot (a codec charges any further heap
/// state it keeps itself).
const CODEC_SLOT: u64 = 64;
const _: () = assert!(core::mem::size_of::<Option<CodecState>>() as u64 <= CODEC_SLOT);

impl Arena {
    fn new(governor: &Governor) -> Result<Self, StopReason> {
        let mut nodes = ScratchVec::new(governor);
        nodes.push(HistNode {
            prev: ROOT,
            kind: HistKind::Root,
        })?;
        Ok(Self {
            nodes,
            values: Vec::new(),
            edits: Vec::new(),
            codecs: Vec::new(),
            deferred: ScratchVec::new(governor),
            charged: 0,
            meter: governor.scratch_meter(),
            chains: Vec::new(),
            leads: Vec::new(),
            memo: BTreeMap::new(),
        })
    }

    pub(crate) fn push(&mut self, prev: u32, kind: HistKind) -> Result<u32, StopReason> {
        self.nodes.push(HistNode { prev, kind })?;
        Ok(index_u32(self.nodes.len() - 1))
    }

    pub(crate) fn pack(&mut self, first: u32, second: u32) -> Result<u32, StopReason> {
        if first == second {
            return Ok(first);
        }
        self.push(first, HistKind::Pack(second))
    }

    pub(crate) fn edit(
        &mut self,
        prev: u32,
        edit: MarkupEdit,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        governor.charge(Resource::ScratchBytes, edit.estimated_bytes())?;
        self.adopt_edit(prev, edit)
    }

    /// Record an edit whose retained size its creator already charged.
    fn adopt_edit(&mut self, prev: u32, edit: MarkupEdit) -> Result<u32, StopReason> {
        self.charged = self.charged.saturating_add(edit.estimated_bytes());
        self.edits.push(edit);
        let index = index_u32(self.edits.len() - 1);
        self.push(prev, HistKind::Edit(index))
    }

    /// Record a finished codec value and its edits after `prev`; the new
    /// history and the edits' total cost. A value with an equally cheap
    /// alternative reading (see [`CodecValue::alternative`]) records both,
    /// packed side by side, so the call has both readings; were one
    /// cheaper, it alone is kept.
    fn codec_value(&mut self, prev: u32, value: CodecValue) -> Result<(u32, u64), StopReason> {
        let CodecValue {
            tree,
            edits,
            alternative,
        } = value;
        let mut hist = self.value(prev, tree)?;
        let mut cost = 0_u64;
        for edit in edits {
            cost = cost.saturating_add(edit.rule().cost);
            hist = self.adopt_edit(hist, edit)?;
        }
        let Some(alternative) = alternative else {
            return Ok((hist, cost));
        };
        let (other, other_cost) = self.codec_value(prev, *alternative)?;
        Ok(match other_cost.cmp(&cost) {
            core::cmp::Ordering::Equal => (self.pack(hist, other)?, cost),
            core::cmp::Ordering::Less => (other, other_cost),
            core::cmp::Ordering::Greater => (hist, cost),
        })
    }

    fn value(&mut self, prev: u32, tree: ArgTree) -> Result<u32, StopReason> {
        // The codec charged exactly `tree.estimated_bytes()` when it built
        // the tree; that charge is released with the arena.
        self.charged = self.charged.saturating_add(tree.estimated_bytes());
        self.values.push(tree);
        let index = index_u32(self.values.len() - 1);
        self.push(prev, HistKind::Value(index))
    }

    /// A new codec slot holding `state`.
    fn begin_codec(
        &mut self,
        state: CodecState,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        let bytes = CODEC_SLOT.saturating_add(state.boxed_bytes());
        governor.charge(Resource::ScratchBytes, bytes)?;
        self.charged = self.charged.saturating_add(bytes);
        self.codecs.push(Some(state));
        Ok(index_u32(self.codecs.len() - 1))
    }

    /// The codec state in `slot`, which its configuration finishes.
    fn take_codec(&mut self, slot: u32) -> CodecState {
        self.codecs[slot as usize]
            .take()
            .expect("a codec slot is finished once")
    }

    fn reset(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        self.nodes.clear();
        self.nodes.push(HistNode {
            prev: ROOT,
            kind: HistKind::Root,
        })?;
        self.values.clear();
        self.edits.clear();
        self.codecs.clear();
        self.deferred.clear();
        self.chains.clear();
        self.leads.clear();
        self.memo.clear();
        governor.release_scratch(core::mem::take(&mut self.charged))
    }
}

/// A body a reading ended beside another (a fork of a body that read on
/// past a closer; see `Step::end_early`) without finishing its codec: it
/// is finished only if the reading completes or begins another slot, so a
/// reading that dies at the bytes after the closer costs nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Deferred {
    /// The [`HistKind::Link`] node standing for the value.
    node: u32,
    /// The codec's arena slot, its delimiter set, and the body.
    slot: u32,
    set: u32,
    entry: usize,
    end: usize,
    /// Whether the value was read yet, and how.
    outcome: Reading,
}

/// How a deferred body's value was read (see [`Deferred`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reading {
    /// Not yet.
    Pending,
    /// Read; its edits add this cost.
    Read(u64),
    /// No repair reads it: the reading is dropped.
    Unread,
}

impl ScratchCharge for Deferred {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 64);
        64
    };
}

/// A configuration that could not consume the byte at `at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    pub(crate) thread: Thread,
    pub(crate) at: usize,
    pub(crate) reason: Rejection,
    /// Kept from an earlier step while a sibling configuration lived on:
    /// only a respelling (a closer variant or a confusable spelling) may
    /// repair it, as every other repair of the attempt is proposed at the
    /// step where it runs out of configurations.
    pub(crate) respell: bool,
}

impl ScratchCharge for Failure {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 160);
        160
    };
}

/// The outcome of advancing a set of configurations.
pub(crate) struct Out {
    pub(crate) live: ScratchVec<Thread>,
    pub(crate) failed: ScratchVec<Failure>,
    pub(crate) completed: ScratchVec<Thread>,
    pub(crate) opened: ScratchVec<(u64, usize)>,
}

impl Out {
    pub(crate) fn new(governor: &Governor) -> Self {
        Self {
            live: ScratchVec::new(governor),
            failed: ScratchVec::new(governor),
            completed: ScratchVec::new(governor),
            opened: ScratchVec::new(governor),
        }
    }

    fn clear(&mut self) {
        self.live.clear();
        self.failed.clear();
        self.completed.clear();
        self.opened.clear();
    }
}

/// The interpreter over one table's program, with split borrows of the
/// engine state it needs.
pub(crate) struct Step<'a, 's> {
    pub(crate) grammar: &'s CompiledGrammar,
    pub(crate) program: &'s Program,
    /// What the program reads (a call's verbatim argument body may hold
    /// closer text; see `Step::repaired_delimiter`).
    pub(crate) kind: ProductionKind,
    pub(crate) dfas: &'a mut [DerivativeDfa<'s>],
    pub(crate) arena: &'a mut Arena,
    pub(crate) buffer: &'a [u8],
    pub(crate) base: usize,
    pub(crate) stall_cap: u32,
    pub(crate) visited: &'a mut Vec<u64>,
    pub(crate) pending: &'a mut Vec<(u32, u32, u64)>,
    pub(crate) tools: Option<&'s ToolSet>,
    /// The latest start of any attempt that opened, in any table: a
    /// resynchronizing configuration gives up at another call's opener.
    pub(crate) newest_open: usize,
    /// The skipped calls a resynchronizing configuration has ended, by
    /// attempt and the offset its call failed at: its siblings scanning for
    /// a later closer give up (a skipped call ends at its first call
    /// closer).
    pub(crate) resynced: &'a mut Vec<(u64, usize, usize)>,
    /// The openers (by attempt start) a resynchronizing attempt met inside
    /// a JSON string of the call it skips: text of the string, never a
    /// reason to give up (see `step_resync`).
    pub(crate) quoted_openers: &'a mut Vec<(u64, usize)>,
    /// The table's live attempts (see `Step::opens_call`).
    pub(crate) attempts: &'a [Attempt],
}

impl<'s> Step<'_, 's> {
    pub(crate) const fn byte(&self, pos: usize) -> u8 {
        self.buffer[pos - self.base]
    }

    pub(crate) fn bytes(&self, range: Range<usize>) -> &[u8] {
        &self.buffer[range.start - self.base..range.end - self.base]
    }

    /// Advance every configuration on `stack` to `until`. With `eof`, the
    /// configurations reaching `until` take their end-of-input transitions,
    /// and those left waiting at a literal are reported as failures with
    /// [`Rejection::Unterminated`].
    pub(crate) fn advance(
        &mut self,
        stack: &mut ScratchVec<Thread>,
        until: usize,
        eof: bool,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        while let Some(thread) = stack.pop() {
            governor.charge(Resource::Work, K_THREAD)?;
            if thread.pos >= until {
                if eof {
                    self.end(thread, stack, out, governor)?;
                } else {
                    out.live.push(thread)?;
                }
                continue;
            }
            let byte = self.byte(thread.pos);
            self.step(thread, byte, stack, out, governor)?;
        }
        Ok(())
    }

    fn fail(out: &mut Out, thread: Thread, at: usize, reason: Rejection) -> Result<(), StopReason> {
        out.failed.push(Failure {
            thread,
            at,
            reason,
            respell: false,
        })
    }

    /// Queue `thread`, which has moved to `thread.pos`.
    fn queue(&self, mut thread: Thread, stack: &mut ScratchVec<Thread>) -> Result<(), StopReason> {
        if thread.pos > thread.reach {
            thread.reach = thread.pos;
            thread.stall = 0;
        } else {
            thread.stall += 1;
            if thread.stall > self.stall_cap {
                return Ok(()); // A zero-width cycle: this lineage adds nothing.
            }
        }
        stack.push(thread)
    }

    fn step(
        &mut self,
        thread: Thread,
        byte: u8,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if self.writes_inserted(&thread, byte) {
            // The token this reading inserted is written right here: the
            // reading that reads it as written is the reading.
            return Ok(());
        }
        match self.program.insts[thread.pc as usize] {
            Inst::Lit { lit, next } => {
                self.step_literal(thread, lit, next, byte, stack, out, governor)
            }
            Inst::Class { class, next } => {
                self.step_class(thread, class, next, byte, stack, out, governor)
            }
            Inst::Until { set, next } | Inst::Slot { set, next, .. } => {
                self.step_body(thread, set, next, byte, stack, out, governor)
            }
            Inst::Split(..)
            | Inst::Open { .. }
            | Inst::Close { .. }
            | Inst::Commit { .. }
            | Inst::Next { .. }
            | Inst::Insert { .. }
            | Inst::Accept => {
                unreachable!("configurations wait only at terminals")
            }
        }
    }

    /// Whether `byte` completes, at `thread`'s position, a token written
    /// where its reading inserted one (`tool_call.markup.missing-token`
    /// read just before its current terminal, past only zero-width
    /// captures): the inserted token itself, or, for a terminal that is not
    /// a literal (a value), a literal the grammar reads at the insertion's
    /// position instead (an optional segment's token, such as Mistral v11's
    /// `[CALL_ID]` before `[ARGS]`). A token is never read as omitted where
    /// the call's markup is written, so that reading ends. Only a terminal
    /// that began within the longest literal of the position is checked.
    fn writes_inserted(&self, thread: &Thread, byte: u8) -> bool {
        let pos = thread.pos;
        if pos < thread.entry || pos - thread.entry >= 32 {
            return false;
        }
        let nodes = self.arena.nodes.as_slice();
        let mut node = thread.hist;
        for _ in 0..8 {
            let HistNode { prev, kind } = nodes[node as usize];
            match kind {
                HistKind::Open(_, at) | HistKind::Close(_, at) | HistKind::Next(at, _)
                    if at == thread.entry =>
                {
                    node = prev;
                }
                HistKind::Edit(index) => {
                    let edit = &self.arena.edits[index as usize];
                    let start = edit.span.start;
                    if edit.operator != EditOperator::MissingToken
                        || !edit.span.is_empty()
                        || start != thread.entry
                    {
                        return false;
                    }
                    let spells = |token: &[u8]| {
                        pos + 1 - start == token.len()
                            && token[token.len() - 1] == byte
                            && self.bytes(start..pos) == &token[..token.len() - 1]
                    };
                    let token = &edit.replacement;
                    if spells(token) {
                        return true;
                    }
                    let value = !matches!(self.program.insts[thread.pc as usize], Inst::Lit { .. });
                    let literals = &self.grammar.literals;
                    return value
                        && self
                            .program
                            .insert_alternatives
                            .iter()
                            .filter(|(lit, _)| literals[*lit as usize].bytes.as_slice() == token)
                            .flat_map(|(_, alternatives)| alternatives)
                            .any(|lit| spells(&literals[*lit as usize].bytes));
                }
                _ => return false,
            }
        }
        false
    }

    /// Whether a misspelling of `canonical`, a literal other than a closer,
    /// is read where `thread` expects it (`tool_call.markup.token-misspelled`,
    /// the opener-side mirror of a misspelled closer): a markup token (a
    /// multi-byte spelling beginning with `<` or `[`), an attempt's own
    /// opener included. A misspelling is only ever a fallback: it is read
    /// where the canonical spelling failed at this byte, the token must
    /// misspell exactly one literal of the grammar and spell none exactly
    /// (see [`Self::misspell`]), and its inner word must be long enough
    /// ([`super::misspelling::MIN_INNER`]), so a short or ambiguous token is
    /// never respelled.
    const fn respellable_token(canonical: &[u8]) -> bool {
        canonical.len() > 1 && matches!(canonical[0], b'<' | b'[')
    }

    /// One byte of a literal: its canonical spelling, or (after a repair) a
    /// variant, confusable, or stray-closer spelling.
    #[allow(clippy::too_many_arguments)]
    fn step_literal(
        &mut self,
        mut thread: Thread,
        lit: u32,
        next: u32,
        byte: u8,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let grammar = self.grammar;
        let canonical = &grammar.literals[lit as usize].bytes;
        let tables = &grammar.literal_tables[lit as usize];
        let pos = thread.pos;
        let (trie, operator, node) = match thread.sub {
            Sub::Lit(depth) => {
                if canonical[depth as usize] != byte {
                    if grammar.literals[lit as usize].closer.is_some()
                        || Self::respellable_token(canonical)
                    {
                        self.misspell(
                            thread,
                            lit,
                            next,
                            depth as usize,
                            byte,
                            stack,
                            out,
                            governor,
                        )?;
                    }
                    return Self::fail(out, thread, pos, Rejection::Malformed { at: pos });
                }
                if depth as usize + 1 == canonical.len() {
                    return self.finish_literal(thread, next, pos + 1, stack, out, governor);
                }
                thread.sub = Sub::Lit(depth + 1);
                thread.pos = pos + 1;
                return self.queue(thread, stack);
            }
            Sub::Variant(node) => (&tables.variants.trie, EditOperator::CloserVariant, node),
            Sub::Confusable(node) => (
                &tables.confusables.trie,
                EditOperator::TerminalConfusable,
                node,
            ),
            Sub::Skip(node) => (&grammar.stray, EditOperator::DeleteStrayCloser, node),
            Sub::Json(table, node) => (
                &tables.json[table as usize].trie,
                JsonSpelling::ALL[table as usize].operator(),
                node,
            ),
            Sub::JsonSkip(state) => {
                return self.step_json_skip(thread, lit, next, byte, state, stack, out, governor);
            }
            Sub::JsonInner(state, node) => {
                return self
                    .step_json_inner(thread, lit, next, byte, state, node, stack, out, governor);
            }
            Sub::Misspelled(..) | Sub::Transposed(..) => {
                return self.step_misspelled(thread, lit, next, byte, stack, out, governor);
            }
            Sub::Resync(state, matched) => {
                return self.step_resync(
                    thread, lit, next, byte, state, matched, stack, out, governor,
                );
            }
            Sub::Class(_) | Sub::Body(..) => unreachable!("literal configurations only"),
        };
        let Some(child) = trie.step(node, byte) else {
            return Self::fail(out, thread, pos, Rejection::Malformed { at: pos });
        };
        if trie.has_children(child) {
            let longer = Thread {
                sub: thread.sub.with_node(child),
                pos: pos + 1,
                ..thread
            };
            self.queue(longer, stack)?;
        }
        if trie.accept(child).is_none() {
            return Ok(());
        }
        let deleted = operator == EditOperator::DeleteStrayCloser;
        let edit = MarkupEdit {
            operator,
            span: thread.entry..pos + 1,
            original: self.bytes(thread.entry..pos + 1).to_vec(),
            replacement: if deleted {
                Vec::new()
            } else {
                canonical.as_slice().to_vec()
            },
        };
        thread.hist = self.arena.edit(thread.hist, edit, governor)?;
        if deleted {
            // The stray closer is gone: expect this literal again.
            thread.sub = Sub::Lit(0);
            thread.entry = pos + 1;
            thread.pos = pos + 1;
            return self.queue(thread, stack);
        }
        self.finish_literal(thread, next, pos + 1, stack, out, governor)
    }

    /// One byte of a configuration resynchronizing at literal `lit` (see
    /// [`Engine::resync`]): a call closer ends the skipped call and the
    /// attempt reads on after it; the next call's opener ends the skipped
    /// call where the opener begins, and the attempt reads that call. Only
    /// outside JSON strings: a single-byte closer (a JSON element's `}`) at
    /// the call's bracket level, a multi-byte literal at any depth. It
    /// gives up (silently: the attempt's failure stands) at another
    /// attempt's opener.
    #[allow(clippy::too_many_arguments)]
    fn step_resync(
        &mut self,
        mut thread: Thread,
        lit: u32,
        next: u32,
        byte: u8,
        state: u32,
        matched: u32,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let canonical = &self.grammar.literals[lit as usize].bytes;
        let pos = thread.pos;
        let mut matched = matched as usize;
        // Another call's opener ends the search, unless it is the opener
        // this configuration is reading (where the next call begins).
        let opener = self.program.call_openers.contains(&thread.pc);
        let newest = self.newest_open;
        let reading_it = opener && matched > 0 && newest == pos - matched;
        // Another attempt opened after the skipped call failed.
        let later = newest > thread.entry;
        let mut scan = JsonScan::unpack(state);
        // An opener inside a JSON string of the skipped call is text of the
        // string, not another call (its attempt opened inside this one's
        // markup and has no reading of its own).
        let ignored = self
            .quoted_openers
            .iter()
            .any(|&(attempt, opener)| attempt == thread.attempt && opener == newest);
        if later && !reading_it && scan.quoted && !ignored {
            governor.charge(Resource::ScratchBytes, 16)?;
            self.quoted_openers.push((thread.attempt, newest));
        }
        if (later && !reading_it && !scan.quoted && !ignored)
            || self
                .resynced
                .iter()
                .any(|&(attempt, failed, _)| attempt == thread.attempt && failed == thread.entry)
        {
            return Ok(());
        }
        // Never inside a JSON string (a string's text is no markup). A
        // closing-bracket closer (a JSON element's own `}`) closes the
        // bracket the call opened; a multi-byte literal (a special token)
        // is read at any bracket depth (a value left open).
        let multi = canonical.len() > 1;
        let level = !scan.quoted
            && (multi
                || if opener {
                    // A single-byte opener (a JSON element's `{`) inside the
                    // skipped element, at its member level where no value is
                    // due: no member begins with it, so the element was left
                    // open and the next one begins.
                    scan.depth == 1 && !scan.colon
                } else {
                    scan.depth == 0 || (scan.depth == 1 && matches!(canonical[0], b'}' | b']'))
                });
        if level {
            matched = if canonical[matched] == byte {
                matched + 1
            } else {
                usize::from(canonical[0] == byte)
            };
            if matched == canonical.len() {
                governor.charge(Resource::ScratchBytes, 24)?;
                let at = pos + 1 - matched;
                // The skipped call's markup runs to its closer's end, or to
                // where the next call's opener begins; a single byte (a JSON
                // element's bracket, which prose writes too) shows no markup
                // read beyond the call's own failure (see `Engine::retire`).
                let read = match (multi, opener) {
                    (false, _) => 0,
                    (true, true) => at,
                    (true, false) => pos + 1,
                };
                self.resynced.push((thread.attempt, thread.entry, read));
                thread.entry = at;
                thread.sub = Sub::Lit(0);
                if opener {
                    // The skipped call ends where the next one begins.
                    thread.hist = self
                        .arena
                        .push(thread.hist, HistKind::Next(at, thread.cost))?;
                }
                return self.finish_literal(thread, next, pos + 1, stack, out, governor);
            }
        } else {
            matched = 0;
        }
        scan.step(byte);
        if scan.quoted {
            matched = 0;
        }
        let state = scan.pack();
        thread.sub = Sub::Resync(state, index_u32(matched));
        thread.pos = pos + 1;
        self.queue(thread, stack)
    }

    /// One byte of a misspelled closer of literal `lit` after its slip: the
    /// rest of the canonical spelling, read exactly.
    #[allow(clippy::too_many_arguments)]
    fn step_misspelled(
        &mut self,
        mut thread: Thread,
        lit: u32,
        next: u32,
        byte: u8,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let canonical = &self.grammar.literals[lit as usize].bytes;
        let pos = thread.pos;
        let (index, slip, step) = match thread.sub {
            Sub::Misspelled(index, slip) => (index as usize, slip, 1),
            Sub::Transposed(index, slip) => (index as usize, slip, 2),
            _ => unreachable!("misspelled closer configurations only"),
        };
        if canonical[index] != byte {
            return Self::unspell(out, thread, slip);
        }
        let resume = index + step;
        if resume == canonical.len() {
            return self.finish_misspelled(thread, lit, next, slip, pos + 1, stack, out, governor);
        }
        thread.sub = Sub::Misspelled(index_u32(resume), slip);
        thread.pos = pos + 1;
        self.queue(thread, stack)
    }

    /// The canonical spelling of closer literal `lit` failed on `byte` after
    /// its first `depth` bytes: queue each reading of `byte` as the one slip
    /// of a misspelled closer (`tool_call.markup.closer-misspelled`) that
    /// keeps the closer's delimiters: substituted for the expected byte,
    /// inserted before it, the expected byte deleted (`byte` is the one
    /// after it), or the expected byte and the next swapped. Each carries
    /// the repair's cost; [`Self::finish_misspelled`] admits the token only
    /// when it misspells this closer and no other.
    #[allow(clippy::too_many_arguments)]
    fn misspell(
        &mut self,
        thread: Thread,
        lit: u32,
        next: u32,
        depth: usize,
        byte: u8,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let canonical = &self.grammar.literals[lit as usize].bytes;
        let tables = &self.grammar.literal_tables[lit as usize];
        let (lead, trail) = super::misspelling::delimiters(canonical);
        let inner_end = canonical.len() - trail;
        if inner_end < lead + super::misspelling::MIN_INNER || depth < lead || depth > inner_end {
            return Ok(());
        }
        // A spelling the grammar declares (a closer variant, a confusable)
        // is read by its own repair, never as a misspelling.
        let pending = self.bytes(thread.entry..thread.pos + 1);
        if tables.variants.trie.admits(pending) || tables.confusables.trie.admits(pending) {
            return Ok(());
        }
        // Nor is a token another literal of the grammar may still spell
        // exactly (a section opener where a call opener was due): that
        // literal reads it, or the reading fails as written.
        if self
            .grammar
            .literals
            .iter()
            .any(|other| other.bytes.starts_with(pending))
        {
            return Ok(());
        }
        let slip = index_u32(depth);
        let cost = thread.cost.saturating_add(super::cost::CLOSER_MISSPELLED);
        let pos = thread.pos;
        let at = |sub| Thread {
            sub,
            cost,
            pos: pos + 1,
            ..thread
        };
        // A substituted or inserted byte is a word byte of the token.
        let word = super::misspelling::word(byte);
        // Substituted, or the expected byte deleted with `byte` the next.
        for resume in [depth + 1, depth + 2] {
            let fits = depth < inner_end
                && if resume == depth + 1 {
                    word
                } else {
                    depth + 1 < canonical.len() && canonical[depth + 1] == byte
                };
            if !fits {
                continue;
            }
            if resume == canonical.len() {
                self.finish_misspelled(
                    at(Sub::Lit(0)),
                    lit,
                    next,
                    slip,
                    pos + 1,
                    stack,
                    out,
                    governor,
                )?;
            } else {
                self.queue(at(Sub::Misspelled(index_u32(resume), slip)), stack)?;
            }
        }
        // Inserted before the expected byte.
        if word {
            self.queue(at(Sub::Misspelled(slip, slip)), stack)?;
        }
        // The expected byte and the next swapped.
        if depth + 1 < inner_end && canonical[depth + 1] == byte && canonical[depth] != byte {
            self.queue(at(Sub::Transposed(slip, slip)), stack)?;
        }
        Ok(())
    }

    /// A misspelling reading of a closer died: report the canonical
    /// spelling's failure at the slip (after `slip` matched bytes) again,
    /// as the configuration that failed there, so the attempt's repairs
    /// (a stray closer deleted, a missing closer inserted) are proposed
    /// from it as if no misspelling had been read.
    fn unspell(out: &mut Out, thread: Thread, slip: u32) -> Result<(), StopReason> {
        let at = thread.entry + slip as usize;
        Self::fail(
            out,
            Thread {
                sub: Sub::Lit(slip),
                pos: at,
                cost: thread.cost.saturating_sub(super::cost::CLOSER_MISSPELLED),
                ..thread
            },
            at,
            Rejection::Malformed { at },
        )
    }

    /// A misspelled closer token of literal `lit` ended at `end`: admit it
    /// when it misspells this closer's class and no other closer of the
    /// grammar, recording `tool_call.markup.closer-misspelled` with the
    /// original bytes.
    #[allow(clippy::too_many_arguments)]
    fn finish_misspelled(
        &mut self,
        mut thread: Thread,
        lit: u32,
        next: u32,
        slip: u32,
        end: usize,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let grammar = self.grammar;
        let literal = &grammar.literals[lit as usize];
        let token = self.bytes(thread.entry..end);
        let Some(class) = literal.closer else {
            // A token other than a closer: it must misspell this literal and
            // no other literal of the grammar.
            governor.charge(
                Resource::Work,
                size(token.len()).saturating_mul(size(grammar.literals.len())),
            )?;
            let found = super::misspelling::misspelled_class(
                token,
                grammar
                    .literals
                    .iter()
                    .map(|other| core::slice::from_ref(&other.bytes)),
            );
            if found != Some(lit as usize) {
                return Self::unspell(out, thread, slip);
            }
            let edit = MarkupEdit {
                operator: EditOperator::TokenMisspelled,
                span: thread.entry..end,
                original: token.to_vec(),
                replacement: literal.bytes.as_slice().to_vec(),
            };
            thread.hist = self.arena.edit(thread.hist, edit, governor)?;
            return self.finish_literal(thread, next, end, stack, out, governor);
        };
        governor.charge(
            Resource::Work,
            size(token.len()).saturating_mul(size(grammar.closers.len())),
        )?;
        let found = super::misspelling::misspelled_class(
            token,
            grammar
                .closers
                .iter()
                .map(|closer| closer.spellings.as_slice()),
        );
        if found != Some(class.index()) {
            return Self::unspell(out, thread, slip);
        }
        let edit = MarkupEdit {
            operator: EditOperator::CloserMisspelled,
            span: thread.entry..end,
            original: token.to_vec(),
            replacement: literal.bytes.as_slice().to_vec(),
        };
        thread.hist = self.arena.edit(thread.hist, edit, governor)?;
        self.finish_literal(thread, next, end, stack, out, governor)
    }

    /// One byte of a JSON-syntax deletion before literal `lit` (a
    /// `json-envelope` grammar): a trailing `,` before the literal's `}` or
    /// `]`, a `/* ... */` or `// ...` comment, each with the whitespace
    /// after it, or a run of raw control characters before a closing `"`.
    /// States: 0 the first byte; 1 whitespace after the comma; 2 after
    /// `/`; 3 inside a block comment; 4 after its `*`; 5 inside a line
    /// comment; 6 whitespace after a comment; 7 inside the control run. At
    /// the first other byte the deletion is recorded
    /// (`json.syntax.trailing-comma`, `json.syntax.comment` or
    /// `json.syntax.string`) and the literal is read from that byte.
    #[allow(clippy::too_many_arguments)]
    fn step_json_skip(
        &mut self,
        mut thread: Thread,
        lit: u32,
        next: u32,
        byte: u8,
        state: u32,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let pos = thread.pos;
        let bytes = &self.grammar.literals[lit as usize].bytes;
        let closing = matches!(bytes.first(), Some(b'}' | b']'));
        let quote = bytes.as_slice() == b"\"";
        let next_state = match (state, byte) {
            (0, b',') if closing => Some(1),
            (0 | 7, 0..=0x1F) if quote => Some(7),
            (0 | 6, b'/') => Some(2),
            (1 | 6, b' ' | b'\t' | b'\r' | b'\n') => Some(state),
            (3 | 4, b'*') => Some(4),
            (4, b'/') | (5, b'\n') => Some(6),
            (2, b'*') | (3 | 4, _) => Some(3),
            (2, b'/') | (5, _) => Some(5),
            _ => None,
        };
        if let Some(state) = next_state {
            thread.sub = Sub::JsonSkip(state);
            thread.pos = pos + 1;
            return self.queue(thread, stack);
        }
        let operator = match state {
            1 => EditOperator::JsonTrailingComma,
            6 => EditOperator::JsonComment,
            7 => EditOperator::JsonString,
            _ => return Self::fail(out, thread, pos, Rejection::Malformed { at: pos }),
        };
        let edit = MarkupEdit {
            operator,
            span: thread.entry..pos,
            original: self.bytes(thread.entry..pos).to_vec(),
            replacement: Vec::new(),
        };
        thread.hist = self.arena.edit(thread.hist, edit, governor)?;
        thread.sub = Sub::Lit(0);
        thread.entry = pos;
        self.step_literal(thread, lit, next, byte, stack, out, governor)
    }

    /// One byte of a comment deleted inside literal `lit` (a `json-envelope`
    /// grammar), after its first `node` canonical bytes, which end at a
    /// JSON token boundary (`OpenAI`'s `{"type": "function"` opener spelling
    /// spans several JSON tokens). States: 0 the first byte; 1 whitespace
    /// before the comment; 2 after `/`; 3 inside a block comment; 4 after
    /// its `*`; 5 inside a line comment; 6 whitespace after a comment. At
    /// the first other byte after a comment, the comment and the whitespace
    /// around it stand for the literal's whitespace at that point (JSON
    /// whitespace is insignificant between tokens): the deletion is
    /// recorded (`json.syntax.comment`, the literal's whitespace as its
    /// replacement) and the literal is read on from its next non-space
    /// byte.
    #[allow(clippy::too_many_arguments)]
    fn step_json_inner(
        &mut self,
        mut thread: Thread,
        lit: u32,
        next: u32,
        byte: u8,
        state: u8,
        node: u32,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let pos = thread.pos;
        let space = matches!(byte, b' ' | b'\t' | b'\r' | b'\n');
        let next_state = match (state, byte) {
            (0 | 1 | 6, b'/') => Some(2),
            (0 | 1, _) if space => Some(1),
            (6, _) if space => Some(6),
            (3 | 4, b'*') => Some(4),
            (4, b'/') | (5, b'\n') => Some(6),
            (2, b'*') | (3 | 4, _) => Some(3),
            (2, b'/') | (5, _) => Some(5),
            _ => None,
        };
        if let Some(state) = next_state {
            thread.sub = Sub::JsonInner(state, node);
            thread.pos = pos + 1;
            return self.queue(thread, stack);
        }
        if state != 6 {
            return Self::fail(out, thread, pos, Rejection::Malformed { at: pos });
        }
        let canonical = &self.grammar.literals[lit as usize].bytes;
        let resume = canonical[node as usize..]
            .iter()
            .position(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            .map_or(canonical.len(), |skipped| node as usize + skipped);
        let edit = MarkupEdit {
            operator: EditOperator::JsonComment,
            span: thread.entry..pos,
            original: self.bytes(thread.entry..pos).to_vec(),
            replacement: canonical[node as usize..resume].to_vec(),
        };
        thread.hist = self.arena.edit(thread.hist, edit, governor)?;
        if resume == canonical.len() {
            // The literal ended in the whitespace the comment stood for:
            // its continuation reads this byte.
            return self.finish_literal(thread, next, pos, stack, out, governor);
        }
        thread.sub = Sub::Lit(index_u32(resume));
        self.step_literal(thread, lit, next, byte, stack, out, governor)
    }

    /// One byte of a maximal class run.
    #[allow(clippy::too_many_arguments)]
    fn step_class(
        &mut self,
        mut thread: Thread,
        class: u32,
        next: u32,
        byte: u8,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Sub::Class(state) = thread.sub else {
            unreachable!("class configurations only")
        };
        let pos = thread.pos;
        let dfa = &mut self.dfas[class as usize];
        let stepped = dfa.step(state, byte, governor)?;
        if !dfa.is_dead(stepped, governor)? {
            thread.sub = Sub::Class(stepped);
            thread.pos = pos + 1;
            return self.queue(thread, stack);
        }
        if thread.entry < pos && dfa.accepting(state, governor)? {
            // The run ended before this byte, which the continuation consumes.
            return self.closure(thread, next, pos, stack, out, governor);
        }
        // Classes read whole characters: a run that dies inside a character
        // could not consume that character, so the run ends (when it can)
        // at the character's lead byte, and the continuation replays the
        // character from its first byte — a multi-byte confusable or closer
        // spelling right after a class is then read exactly like an ASCII
        // one.
        let boundary = pos - state.partial_bytes();
        if let Some(start) = state.character_start()
            && thread.entry < boundary
            && dfa.accepting(start, governor)?
        {
            thread.sub = Sub::Class(start);
            return self.closure(thread, next, boundary, stack, out, governor);
        }
        // Otherwise a run that dies on a continuation byte fails at the
        // lead byte of the character it could not consume, which keeps the
        // rejected span from splitting a well-formed character.
        let at = if byte & 0xC0 == 0x80 { boundary } else { pos };
        Self::fail(out, thread, at, Rejection::Malformed { at })
    }

    /// One byte of an `until`/`slot` body: hold back a possible delimiter
    /// prefix, release the rest to the codec, and end at a delimiter.
    #[allow(clippy::too_many_arguments)]
    fn step_body(
        &mut self,
        mut thread: Thread,
        set: u32,
        next: u32,
        byte: u8,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Sub::Body(raw, codec) = thread.sub else {
            unreachable!("body configurations only")
        };
        let (state, mut past) = (raw & !(PAST_REPAIRED | TEXTED), raw & PAST_REPAIRED != 0);
        let mut texted = raw & TEXTED != 0;
        let pos = thread.pos;
        self.misspelled_delimiter(thread, set, next, byte, stack, out, governor)?;
        let matcher = &self.grammar.delimiters[set as usize].matcher;
        let released = pos - matcher.depth(state) as usize;
        let stepped = matcher.step(state, byte);
        let found = matcher.found(stepped);
        let to = match found {
            Some((_, length)) => pos + 1 - length as usize,
            None => pos + 1 - matcher.depth(stepped) as usize,
        };
        // The bytes no delimiter can hold any more (not those of a
        // delimiter just found): text, unless whitespace.
        let text_from = if matcher.found(state).is_some() {
            pos
        } else {
            released
        };
        if past && !texted && text_from < to {
            texted = self
                .bytes(text_from..to)
                .iter()
                .any(|byte| !byte.is_ascii_whitespace());
            if texted {
                thread.sub = Sub::Body(raw | TEXTED, codec);
            }
        }
        let mut end = None;
        if let Some(slot) = codec {
            match self.feed(slot, released..to, governor)? {
                Ok(Some(complete)) => end = Some(complete),
                Ok(None) => {}
                Err(fault) => return Self::fail(out, thread, pos, rejection(fault)),
            }
        }
        if end.is_none()
            && let Some((index, _)) = found
        {
            if self.repaired_delimiter(&thread, set, index) {
                // A delimiter that ends the body only by a repair (a closer
                // variant or confusable spelling, or one written inside a
                // self-delimiting value not yet complete): a reading ending
                // the body there is queued beside it as a fallback, and the
                // body reads on, so a reading that needs no repair (running
                // on to a canonical delimiter, or to the value's own end) is
                // never overridden.
                self.end_early(thread, to, next, stack, out, governor)?;
                past = true;
            } else {
                end = Some(to);
            }
        }
        let Some(end) = end else {
            let flags = match (past, texted) {
                (true, true) => PAST_REPAIRED | TEXTED,
                (true, false) => PAST_REPAIRED,
                _ => 0,
            };
            thread.sub = Sub::Body(stepped | flags, codec);
            thread.pos = pos + 1;
            return self.queue(thread, stack);
        };
        thread.hist = self.closer_in_value(&thread, set, end, governor)?;
        self.end_body(
            thread,
            codec,
            end,
            CodecEnd::Delimited,
            next,
            stack,
            out,
            governor,
        )
    }

    /// A body that `byte` may end a misspelled closer delimiter of
    /// (`</functio>` for `</function>`): when the bytes ending at it
    /// misspell exactly one closer of the grammar, and that closer is one of
    /// the body's delimiters, a configuration that ends the body where the
    /// token begins is queued beside the body, which reads on; the closer
    /// literal after the body then reads the token as misspelled
    /// ([`Self::finish_misspelled`]). Only a body whose codec reads its
    /// bytes when it finishes (`raw`, `xml-params`, or none) is ended
    /// early: a `json` codec has read past the token already and ends its
    /// value by itself.
    #[allow(clippy::too_many_arguments)]
    fn misspelled_delimiter(
        &mut self,
        thread: Thread,
        set: u32,
        next: u32,
        byte: u8,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let kind = match self.program.insts[thread.pc as usize] {
            Inst::Slot { codec, .. } => Some(codec),
            _ => None,
        };
        if kind.is_some_and(|kind| {
            !matches!(
                kind,
                CodecKind::Raw | CodecKind::XmlParams | CodecKind::KvPairs
            )
        }) {
            return Ok(());
        }
        let grammar = self.grammar;
        let pos = thread.pos;
        let declared = &grammar.delimiters[set as usize].declared;
        // The closer classes among the body's delimiters that `byte` may
        // end, and the token widths a misspelling of one may have.
        let mut classes: Vec<usize> = Vec::new();
        let mut candidates: Vec<usize> = Vec::new();
        for delimiter in declared
            .iter()
            .filter(|delimiter| delimiter.last() == Some(&byte))
        {
            let Some(class) = grammar
                .closers
                .iter()
                .position(|closer| closer.spellings.contains(delimiter))
            else {
                continue;
            };
            if !classes.contains(&class) {
                classes.push(class);
            }
            for width in delimiter.len().saturating_sub(1)..=delimiter.len() + 1 {
                let Some(start) = (pos + 1).checked_sub(width) else {
                    continue;
                };
                // The token keeps the closer's first byte (its delimiters
                // are never misspelled): only then is it compared.
                if start > thread.entry
                    && start >= self.base
                    && self.byte(start) == delimiter[0]
                    && !candidates.contains(&start)
                {
                    candidates.push(start);
                }
            }
        }
        let mut starts: Vec<usize> = Vec::new();
        for start in candidates {
            governor.charge(
                Resource::Work,
                size(pos + 1 - start).saturating_mul(size(grammar.closers.len())),
            )?;
            let token = self.bytes(start..pos + 1);
            let found = super::misspelling::misspelled_class(
                token,
                grammar
                    .closers
                    .iter()
                    .map(|closer| closer.spellings.as_slice()),
            );
            if found.is_some_and(|class| classes.contains(&class)) {
                starts.push(start);
            }
        }
        for start in starts {
            self.end_early(thread, start, next, stack, out, governor)?;
        }
        Ok(())
    }

    /// Whether the delimiter `index` of set `set` that `thread`'s body
    /// found may be text of the body rather than its end, so the body also
    /// reads on past it (see `Step::step_body`): a closer delimiter (any
    /// spelling of a closer class, or a confusable one) of a call's verbatim
    /// argument body (`raw`, `xml-params`), whose text may hold the exact
    /// spelling of a closer (`tool_call.value.closer-in-value`:
    /// the reading ending there and the one reading on are weighed by the
    /// repairs each needs); in a self-delimiting (`json`) value, one written
    /// inside a string not yet complete; otherwise a confusable spelling or
    /// a declared closer variant (not its class's canonical spelling).
    fn repaired_delimiter(&self, thread: &Thread, set: u32, index: u32) -> bool {
        if self.verbatim(thread) {
            // Closer text: a closer spelling (or a confusable one). A
            // delimiter that opens markup (the next parameter's opener)
            // ends the body: a value never holds it.
            let declared = &self.grammar.delimiters[set as usize].declared;
            return declared.get(index as usize).is_none_or(|spelling| {
                self.grammar
                    .closers
                    .iter()
                    .any(|closer| closer.spellings.contains(spelling))
            });
        }
        if let Inst::Slot { codec, .. } = self.program.insts[thread.pc as usize]
            && !matches!(
                codec,
                CodecKind::Raw | CodecKind::XmlParams | CodecKind::KvPairs
            )
        {
            // A delimiter written inside a string of a self-delimiting value:
            // ending the value here repairs it (an unterminated string), so
            // the body also reads on, and the value ending by itself later
            // needs no repair.
            let Sub::Body(_, Some(slot)) = thread.sub else {
                return false;
            };
            return self.arena.codecs[slot as usize]
                .as_ref()
                .is_some_and(CodecState::in_string);
        }
        let declared = &self.grammar.delimiters[set as usize].declared;
        let Some(spelling) = declared.get(index as usize) else {
            return true;
        };
        self.grammar.closers.iter().any(|closer| {
            closer.spellings.first() != Some(spelling) && closer.spellings.contains(spelling)
        })
    }

    /// Whether `thread` reads a call's verbatim argument body: a `raw` or
    /// `xml-params` slot of a call production.
    fn verbatim(&self, thread: &Thread) -> bool {
        verbatim_slot(self.program, self.kind, thread)
    }

    /// For a verbatim argument body that read on past a delimiter (see
    /// `Step::repaired_delimiter`) and ends at `end`: the history recording
    /// the closer text it holds (`tool_call.value.closer-in-value`), from
    /// its first exact closer spelling (a declared delimiter) to `end`.
    /// Charges one `Work` unit per body byte scanned.
    fn closer_in_value(
        &mut self,
        thread: &Thread,
        set: u32,
        end: usize,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        let Sub::Body(raw, _) = thread.sub else {
            return Ok(thread.hist);
        };
        if raw & PAST_REPAIRED == 0 || !self.verbatim(thread) {
            return Ok(thread.hist);
        }
        self.closer_note(thread.hist, thread.entry, set, end, governor)
    }

    /// `hist` with the closer text the body `entry..end` of delimiter set
    /// `set` holds recorded (see [`Self::closer_in_value`]), if it holds a
    /// declared delimiter.
    /// Whether a body read on past a closer (`thread`), ending at `end`,
    /// would end inside a call it holds: another attempt of the table opened
    /// inside it and has not completed (or closed a call awaiting its trail). A value holding call markup holds
    /// whole calls, as a value documenting the format does (the reading
    /// that holds a call whose closers come before the value's end competes
    /// with the calls read apart); two calls in a row are never one call
    /// whose value ends inside the second. `Work` per live attempt.
    fn opens_call(
        &self,
        thread: &Thread,
        end: usize,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        governor.charge(Resource::Work, size(self.attempts.len()))?;
        Ok(self.attempts.iter().any(|record| {
            record.id != thread.attempt
                && record.opened
                && !record.completed
                && record.closed.is_none()
                && record.start > thread.entry
                && record.start < end
        }))
    }

    fn closer_note(
        &mut self,
        hist: u32,
        entry: usize,
        set: u32,
        end: usize,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        let delimiters = &self.grammar.delimiters[set as usize];
        let mut state = super::automaton::ROOT;
        let mut first = None;
        for at in entry..end {
            state = delimiters.matcher.step(state, self.byte(at));
            if let Some((index, length)) = delimiters.matcher.found(state)
                && (index as usize) < delimiters.declared.len()
            {
                first = Some(at + 1 - length as usize);
                break;
            }
        }
        governor.charge(
            Resource::Work,
            size(first.map_or(end, |first| first + 1) - entry),
        )?;
        let Some(first) = first else {
            return Ok(hist);
        };
        let edit = MarkupEdit {
            operator: EditOperator::CloserInValue,
            span: first..end,
            original: self.bytes(first..end).to_vec(),
            replacement: Vec::new(),
        };
        self.arena.edit(hist, edit, governor)
    }

    /// The cost a configuration's deferred body (`pending`, zero for none)
    /// adds once read, or `None` when no repair reads its value.
    fn settle_pending(
        &mut self,
        pending: u32,
        governor: &mut Governor,
    ) -> Result<Option<u64>, StopReason> {
        if pending == 0 {
            return Ok(Some(0));
        }
        self.resolve(pending, governor)
    }

    /// Finish the deferred body `pending` (an index plus one; see
    /// [`Deferred`]) once: record the closer text it holds and its value
    /// after the history it ended on, link its node there, and return the
    /// cost its edits add, or `None` when no repair reads the value.
    fn resolve(
        &mut self,
        pending: u32,
        governor: &mut Governor,
    ) -> Result<Option<u64>, StopReason> {
        let index = pending as usize - 1;
        let deferred = self.arena.deferred.as_slice()[index];
        match deferred.outcome {
            Reading::Read(cost) => return Ok(Some(cost)),
            Reading::Unread => return Ok(None),
            Reading::Pending => {}
        }
        let prev = self.arena.nodes.as_slice()[deferred.node as usize].prev;
        let hist = self.closer_note(prev, deferred.entry, deferred.set, deferred.end, governor)?;
        let outcome = match self.finish_codec(
            deferred.slot,
            hist,
            deferred.entry..deferred.end,
            CodecEnd::Delimited,
            governor,
        )? {
            Ok((top, cost)) => {
                self.arena.nodes.as_mut_slice()[deferred.node as usize].prev = top;
                Reading::Read(cost)
            }
            Err(_) => Reading::Unread,
        };
        self.arena.deferred.as_mut_slice()[index].outcome = outcome;
        Ok(match outcome {
            Reading::Read(cost) => Some(cost),
            _ => None,
        })
    }

    /// A fork of body `thread` ending at `end` (a fallback reading beside
    /// the body, which reads on): its codec, when it has one, reads the
    /// body's bytes afresh when it finishes.
    fn end_early(
        &mut self,
        thread: Thread,
        end: usize,
        next: u32,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let kind = match self.program.insts[thread.pc as usize] {
            Inst::Slot { codec, .. } => Some(codec),
            _ => None,
        };
        let codec = match kind {
            Some(kind) => Some(
                self.arena
                    .begin_codec(kind.begin(thread.entry, governor)?, governor)?,
            ),
            None => None,
        };
        let past = thread.beside();
        if past && self.opens_call(&thread, end, governor)? {
            return Ok(());
        }
        // A verbatim body that read on past a closer and since read only
        // closers and whitespace holds no value text (see [`TEXTED`]).
        if let Sub::Body(raw, _) = thread.sub
            && raw & PAST_REPAIRED != 0
            && raw & TEXTED == 0
            && self.verbatim(&thread)
        {
            return Ok(());
        }
        if past
            && !self.program.grouped
            && let (Some(slot), Inst::Slot { set, .. }) =
                (codec, self.program.insts[thread.pc as usize])
        {
            // A fork of a body that read on past a closer: its value is read
            // only if the reading completes (most die at the bytes after
            // the closer), so a value holding many closers is read once per
            // reading that completes, not once per closer.
            let node = self.arena.push(thread.hist, HistKind::Link)?;
            self.arena.deferred.push(Deferred {
                node,
                slot,
                set,
                entry: thread.entry,
                end,
                outcome: Reading::Pending,
            })?;
            let fork = Thread {
                sub: Sub::Body(0, codec),
                hist: node,
                past: true,
                pending: index_u32(self.arena.deferred.len()),
                ..thread
            };
            return self.closure(fork, next, end, stack, out, governor);
        }
        let hist = match self.program.insts[thread.pc as usize] {
            Inst::Slot { set, .. } => self.closer_in_value(&thread, set, end, governor)?,
            _ => thread.hist,
        };
        let fork = Thread {
            sub: Sub::Body(0, codec),
            hist,
            past,
            ..thread
        };
        self.end_body(
            fork,
            codec,
            end,
            CodecEnd::Delimited,
            next,
            stack,
            out,
            governor,
        )
    }

    /// Offer `range`'s bytes to `codec`; the offset where a self-delimiting
    /// codec's value ended, if it did.
    fn feed(
        &mut self,
        slot: u32,
        range: Range<usize>,
        governor: &mut Governor,
    ) -> Result<Result<Option<usize>, CodecFault>, StopReason> {
        let codec = self.arena.codecs[slot as usize]
            .as_mut()
            .expect("a live body owns its codec slot");
        for at in range {
            match codec.feed(self.buffer[at - self.base], at, governor)? {
                CodecStep::Consumed => {}
                CodecStep::Complete { end } => return Ok(Ok(Some(end))),
                CodecStep::Fault(fault) => return Ok(Err(fault)),
            }
        }
        Ok(Ok(None))
    }

    /// Finish an `until`/`slot` body at `end` and continue at `next`.
    #[allow(clippy::too_many_arguments)]
    fn end_body(
        &mut self,
        mut thread: Thread,
        codec: Option<u32>,
        end: usize,
        how: CodecEnd,
        next: u32,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        // A verbatim value read on past a closer: what follows reads beside
        // the reading that ended there. A JSON value is self-delimiting, so
        // a closer inside it was never one: its reading is as written.
        if let Sub::Body(raw, _) = thread.sub
            && raw & PAST_REPAIRED != 0
            && self.verbatim(&thread)
        {
            // Never inside a call it holds (see `Self::opens_call`).
            if self.opens_call(&thread, end, governor)? {
                return Ok(());
            }
            thread.past = true;
        }
        if let Some(slot) = codec {
            match self.finish_codec(slot, thread.hist, thread.entry..end, how, governor)? {
                Ok((hist, cost)) => {
                    thread.hist = hist;
                    thread.cost = thread.cost.saturating_add(cost);
                }
                Err(Finished::Unread { at }) => {
                    // An arguments value no repair reads: the attempt fails
                    // where the value stopped decoding (not at the end of its
                    // body), so what follows is read as the attempt's markup
                    // only as far as its closer or bracket shows it is.
                    return Self::fail(out, thread, at, Rejection::Malformed { at });
                }
                Err(Finished::Fault(fault)) => {
                    return Self::fail(out, thread, thread.pos, rejection(fault));
                }
            }
        }
        self.closure(thread, next, end, stack, out, governor)
    }

    /// Finish the codec in `slot` over `body` and record its value and
    /// edits after `hist`: the new history and the edits' cost.
    fn finish_codec(
        &mut self,
        slot: u32,
        hist: u32,
        body: Range<usize>,
        how: CodecEnd,
        governor: &mut Governor,
    ) -> Result<Result<(u32, u64), Finished>, StopReason> {
        let codec = self.arena.take_codec(slot);
        let context = self.context(codec.kind(), hist, governor)?;
        let buffer = self.buffer;
        let bytes = &buffer[body.start - self.base..body.end - self.base];
        let body_end = body.end;
        match codec.finish(bytes, body, how, context, governor)? {
            // A JSON value no repair reads is not a call's arguments: the
            // attempt fails where its decoding stopped (see
            // `Step::end_body`), a rejected attempt rather than a call.
            Ok(value)
                if how != CodecEnd::Truncated
                    && value.tree.unresolved.iter().any(|fault| {
                        fault.pointer.is_empty()
                            && matches!(fault.fault, ArgumentFault::InvalidJson { .. })
                    }) =>
            {
                let at = value
                    .tree
                    .unresolved
                    .iter()
                    .find_map(|fault| match fault.fault {
                        ArgumentFault::InvalidJson { at } if fault.pointer.is_empty() => Some(at),
                        _ => None,
                    })
                    .unwrap_or(body_end);
                Ok(Err(Finished::Unread { at }))
            }
            Ok(value) => Ok(Ok(self.arena.codec_value(hist, value)?)),
            Err(fault) => Ok(Err(Finished::Fault(fault))),
        }
    }

    /// What a codec of `kind` finishing after history `hist` may know: the
    /// argument shape of the declared tool named by the reading's first
    /// `name:` capture, when the recognizer has a tool set.
    fn context(
        &self,
        kind: CodecKind,
        hist: u32,
        governor: &mut Governor,
    ) -> Result<CodecContext<'s>, StopReason> {
        if !kind.uses_context() {
            return Ok(CodecContext::default());
        }
        let markup = CodecContext {
            shape: None,
            openers: &self.grammar.call_markup[0],
            closers: &self.grammar.call_markup[1],
        };
        let Some(tools) = self.tools else {
            return Ok(markup);
        };
        let nodes = self.arena.nodes.as_slice();
        let (mut node, mut close, mut name) = (hist, None, None);
        loop {
            governor.charge(Resource::Work, K_CODEC)?;
            let HistNode { prev, kind } = nodes[node as usize];
            match kind {
                // The name of the call this slot belongs to: no earlier
                // than the call's own beginning.
                HistKind::Root | HistKind::Next(..) => break,
                HistKind::Close(CaptureKind::Name, at) => close = Some(at),
                HistKind::Open(CaptureKind::Name, start) => {
                    if let Some(end) = close {
                        name = Some(start..end);
                    }
                }
                _ => {}
            }
            node = prev;
        }
        let Some(name) = name else {
            return Ok(markup);
        };
        governor.charge(Resource::Work, size(name.len()).saturating_mul(K_CODEC))?;
        let shape = core::str::from_utf8(self.bytes(name))
            .ok()
            .and_then(|name| tools.lookup_exact(name))
            .map(CompiledTool::argument_shape);
        Ok(CodecContext { shape, ..markup })
    }

    fn finish_literal(
        &mut self,
        mut thread: Thread,
        next: u32,
        at: usize,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if !thread.opened && !self.program.deferred() && self.program.openers.contains(&thread.pc) {
            thread.opened = true;
            out.opened.push((thread.attempt, at))?;
        }
        self.closure(thread, next, at, stack, out, governor)
    }

    /// Follow every input-free transition from `pc` at offset `at`.
    pub(crate) fn closure(
        &mut self,
        thread: Thread,
        pc: u32,
        at: usize,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let words = self.program.insts.len().div_ceil(64);
        self.visited.clear();
        self.visited.resize(words, 0);
        self.pending.clear();
        self.pending.push((pc, thread.hist, thread.cost));
        while let Some((pc, hist, cost)) = self.pending.pop() {
            let (word, bit) = (pc as usize / 64, 1_u64 << (pc % 64));
            if self.visited[word] & bit != 0 {
                continue;
            }
            self.visited[word] |= bit;
            governor.charge(Resource::Work, K_THREAD)?;
            let sub = match self.program.insts[pc as usize] {
                Inst::Split(first, second) => {
                    self.pending.push((second, hist, cost));
                    self.pending.push((first, hist, cost));
                    continue;
                }
                Inst::Open { kind, next } => {
                    let node = self.arena.push(hist, HistKind::Open(kind, at))?;
                    self.pending.push((next, node, cost));
                    continue;
                }
                Inst::Close { kind, next } => {
                    let node = self.arena.push(hist, HistKind::Close(kind, at))?;
                    self.pending.push((next, node, cost));
                    continue;
                }
                // The attempt opens when a configuration past the commit
                // reaches the turn level (see `Engine::commit`).
                Inst::Commit { next } => {
                    self.pending.push((next, hist, cost));
                    continue;
                }
                Inst::Next { next } => {
                    let node = self.arena.push(hist, HistKind::Next(at, cost))?;
                    self.pending.push((next, node, cost));
                    continue;
                }
                Inst::Insert { lit, next } => {
                    let edit = MarkupEdit {
                        operator: EditOperator::MissingToken,
                        span: at..at,
                        original: Vec::new(),
                        replacement: self.grammar.literals[lit as usize]
                            .bytes
                            .as_slice()
                            .to_vec(),
                    };
                    let node = self.arena.edit(hist, edit, governor)?;
                    let cost = cost.saturating_add(EditOperator::MissingToken.rule().cost);
                    self.pending.push((next, node, cost));
                    continue;
                }
                // A deferred body's value is read when the completion is
                // reported (see `Engine::emit`): one the bytes after it
                // reject never reads it.
                Inst::Accept => {
                    out.completed.push(Thread {
                        pc,
                        hist,
                        cost,
                        pos: at,
                        ..thread
                    })?;
                    continue;
                }
                Inst::Lit { .. } => Sub::Lit(0),
                Inst::Class { class, .. } => Sub::Class(self.dfas[class as usize].start()),
                Inst::Until { .. } => Sub::Body(super::automaton::ROOT, None),
                Inst::Slot { codec, .. } => {
                    // A deferred body's value is read before the next one.
                    let Some(added) = self.settle_pending(thread.pending, governor)? else {
                        continue;
                    };
                    let slot = self
                        .arena
                        .begin_codec(codec.begin(at, governor)?, governor)?;
                    let next = Thread {
                        pc,
                        sub: Sub::Body(super::automaton::ROOT, Some(slot)),
                        entry: at,
                        pos: at,
                        hist,
                        cost: cost.saturating_add(added),
                        pending: 0,
                        ..thread
                    };
                    self.queue(next, stack)?;
                    continue;
                }
            };
            let next = Thread {
                pc,
                sub,
                entry: at,
                pos: at,
                hist,
                cost,
                ..thread
            };
            self.queue(next, stack)?;
        }
        Ok(())
    }

    /// End-of-input transitions of a configuration at the end of input.
    fn end(
        &mut self,
        thread: Thread,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let pos = thread.pos;
        match (self.program.insts[thread.pc as usize], thread.sub) {
            (Inst::Lit { .. }, Sub::Lit(_)) => {
                Self::fail(out, thread, pos, Rejection::Unterminated)
            }
            (Inst::Class { class, next }, Sub::Class(state)) => {
                let dfa = &mut self.dfas[class as usize];
                if thread.entry < pos && dfa.accepting(state, governor)? {
                    return self.closure(thread, next, pos, stack, out, governor);
                }
                // Input that ends inside a character: as in `step_class`,
                // the run ends at the character's lead byte when it can, and
                // the continuation replays the truncated character.
                let boundary = pos - state.partial_bytes();
                if let Some(start) = state.character_start()
                    && thread.entry < boundary
                    && dfa.accepting(start, governor)?
                {
                    let thread = Thread {
                        sub: Sub::Class(start),
                        ..thread
                    };
                    return self.closure(thread, next, boundary, stack, out, governor);
                }
                Ok(())
            }
            // A body that read on past a closer beside the reading that
            // ended there reads no call at the end of output: the value it
            // holds is never ended by a closer that completes the call.
            (Inst::Until { .. } | Inst::Slot { .. }, Sub::Body(raw, _))
                if raw & PAST_REPAIRED != 0 =>
            {
                Ok(())
            }
            (Inst::Until { set, next } | Inst::Slot { set, next, .. }, Sub::Body(raw, codec)) => {
                let state = raw & !PAST_REPAIRED;
                let matcher = &self.grammar.delimiters[set as usize].matcher;
                let released = pos - matcher.depth(state) as usize;
                let mut end = pos;
                if let Some(slot) = codec {
                    match self.feed(slot, released..pos, governor)? {
                        Ok(Some(complete)) => end = complete,
                        Ok(None) => {}
                        Err(_) => return Ok(()),
                    }
                }
                self.end_body(
                    thread,
                    codec,
                    end,
                    CodecEnd::EndOfInput,
                    next,
                    stack,
                    out,
                    governor,
                )
            }
            _ => Ok(()),
        }
    }

    /// A configuration at a length stop. When it is in a self-delimiting
    /// slot whose value is already complete, and the production completes
    /// from the slot without further input (nothing but optional markup or
    /// an `insert` after it), the call is whole: the codec finishes as at a
    /// delimiter, the completions go to `out`, and `None` is returned.
    /// Otherwise the history (and added cost) of the configuration cut off:
    /// an open slot keeps the complete part of its body as its value.
    fn cut_thread(
        &mut self,
        thread: Thread,
        stack: &mut ScratchVec<Thread>,
        out: &mut Out,
        governor: &mut Governor,
    ) -> Result<Option<(u32, u64)>, StopReason> {
        if let Sub::Body(raw, _) = thread.sub
            && raw & PAST_REPAIRED != 0
        {
            // A body that read on past a repaired delimiter beside the
            // reading that ended there: cut off by the stop, it reads no
            // call of its own (a cut-off reading needs no repair, so it
            // would override that reading).
            return Ok(None);
        }
        if thread.pending != 0 {
            // A reading that ended a body beside another: its value is read
            // first.
            let Some(added) = self.resolve(thread.pending, governor)? else {
                return Ok(None);
            };
            let thread = Thread {
                cost: thread.cost.saturating_add(added),
                pending: 0,
                ..thread
            };
            return Ok(self
                .cut_thread(thread, stack, out, governor)?
                .map(|(hist, more)| (hist, more.saturating_add(added))));
        }
        let (Inst::Slot { set, next, .. }, Sub::Body(state, Some(slot))) =
            (self.program.insts[thread.pc as usize], thread.sub)
        else {
            return Ok(Some((thread.hist, 0)));
        };
        let matcher = &self.grammar.delimiters[set as usize].matcher;
        let released = thread.pos - matcher.depth(state) as usize;
        let Ok(complete) = self.feed(slot, released..thread.pos, governor)? else {
            return Ok(Some((thread.hist, 0)));
        };
        let whole = complete.or_else(|| {
            self.arena.codecs[slot as usize]
                .as_ref()
                .and_then(CodecState::complete_end)
        });
        if let Some(end) = whole
            && super::repair::completes(self.program, next, governor)?
        {
            self.end_body(
                thread,
                Some(slot),
                end,
                CodecEnd::EndOfInput,
                next,
                stack,
                out,
                governor,
            )?;
            return Ok(None);
        }
        let end = complete.unwrap_or(thread.pos);
        Ok(Some(
            self.finish_codec(
                slot,
                thread.hist,
                thread.entry..end,
                CodecEnd::Truncated,
                governor,
            )?
            .unwrap_or((thread.hist, 0)),
        ))
    }

    /// Every equal-cost history ending at `hist`, each as a forward list of
    /// steps, deduplicated.
    fn histories(
        &self,
        hist: u32,
        governor: &mut Governor,
    ) -> Result<Vec<Vec<HistKind>>, StopReason> {
        let mut readings = Vec::new();
        let mut path: Vec<HistKind> = Vec::new();
        let mut stack = vec![(hist, 0_usize)];
        while let Some((node, depth)) = stack.pop() {
            governor.charge(Resource::Work, K_CAPTURE)?;
            path.truncate(depth);
            let HistNode { prev, kind } = self.arena.nodes.as_slice()[node as usize];
            match kind {
                HistKind::Root => {
                    let forward: Vec<HistKind> = path.iter().rev().copied().collect();
                    if !readings.contains(&forward) {
                        readings.push(forward);
                    }
                }
                HistKind::Pack(other) => {
                    stack.push((other, depth));
                    stack.push((prev, depth));
                }
                kind => {
                    path.push(kind);
                    stack.push((prev, depth + 1));
                }
            }
        }
        Ok(readings)
    }

    /// The text of a name, id or parameter-name capture: bytes that are
    /// not well-formed UTF-8 are read as U+FFFD, each run recorded as a
    /// `tool_call.value.invalid-utf8` edit (pushed onto `edits`).
    fn text(
        &self,
        span: Range<usize>,
        edits: &mut Vec<MarkupEdit>,
        governor: &mut Governor,
    ) -> Result<String, StopReason> {
        governor.charge(Resource::Work, size(span.len()).saturating_mul(K_CAPTURE))?;
        governor.charge(Resource::ScratchBytes, size(span.len()).saturating_add(24))?;
        let bytes = self.bytes(span.clone());
        if let Ok(text) = core::str::from_utf8(bytes) {
            return Ok(text.to_owned());
        }
        let (text, invalid) = super::codec::lossy(bytes, span.start, governor)?;
        edits.extend(invalid);
        Ok(text)
    }

    /// The captures (each with its codec value) and edits of a forward
    /// history, captures ordered by span; captures still open (a cut-off
    /// reading) close at `end`.
    fn trace(
        &self,
        path: &[HistKind],
        end: usize,
        governor: &mut Governor,
    ) -> Result<Trace, StopReason> {
        let mut captures: Vec<(CaptureSpan, Option<u32>)> = Vec::new();
        let mut open: Vec<(CaptureKind, usize, Option<u32>)> = Vec::new();
        let mut edits = Vec::new();
        for step in path {
            match *step {
                HistKind::Open(kind, at) => open.push((kind, at, None)),
                HistKind::Close(kind, at) => {
                    let (opened, start, value) = open.pop().expect("captures nest");
                    debug_assert_eq!(opened, kind);
                    captures.push((
                        CaptureSpan {
                            kind,
                            span: start..at,
                        },
                        value,
                    ));
                }
                HistKind::Value(index) => {
                    if let Some(last) = open.last_mut() {
                        last.2 = Some(index);
                    }
                }
                HistKind::Edit(index) => {
                    let edit = &self.arena.edits[index as usize];
                    governor.charge(Resource::ScratchBytes, edit.estimated_bytes())?;
                    edits.push(edit.clone());
                }
                HistKind::Root
                | HistKind::Pack(_)
                | HistKind::Next(..)
                | HistKind::Skipped(_)
                | HistKind::Link => {}
            }
        }
        while let Some((kind, start, value)) = open.pop() {
            captures.push((
                CaptureSpan {
                    kind,
                    span: start..end,
                },
                value,
            ));
        }
        captures.sort_by_key(|(capture, _)| (capture.span.start, capture.span.end));
        Ok((captures, edits))
    }

    /// The name, id and arguments the captures denote.
    fn fields(
        &self,
        captures: &[(CaptureSpan, Option<u32>)],
        governor: &mut Governor,
    ) -> Result<Result<Fields, Rejection>, StopReason> {
        let mut fields = Fields {
            name: None,
            id: None,
            arguments: None,
            invalid: Vec::new(),
        };
        let mut parameters: Vec<(Spanned<String>, ArgTree)> = Vec::new();
        let mut pending_name: Option<Spanned<String>> = None;
        for (capture, value) in captures {
            match capture.kind {
                CaptureKind::Name | CaptureKind::Id | CaptureKind::ParamName => {
                    let text = self.text(capture.span.clone(), &mut fields.invalid, governor)?;
                    let spanned = Spanned {
                        value: text,
                        span: capture.span.clone(),
                    };
                    match capture.kind {
                        CaptureKind::Name => {
                            fields.name.get_or_insert(spanned);
                        }
                        CaptureKind::Id => {
                            fields.id.get_or_insert(spanned);
                        }
                        _ => pending_name = Some(spanned),
                    }
                }
                CaptureKind::ParamValue => {
                    if let (Some(key), Some(value)) = (pending_name.take(), value) {
                        parameters.push((key, self.value(*value, governor)?));
                    }
                }
                CaptureKind::Arguments => {
                    if let (None, Some(value)) = (&fields.arguments, value) {
                        fields.arguments = Some(self.value(*value, governor)?);
                    }
                }
                CaptureKind::Reasoning | CaptureKind::Content | CaptureKind::Dropped => {}
            }
        }
        if fields.arguments.is_none() && !parameters.is_empty() {
            fields.arguments = Some(ArgTree::from_parameters(parameters));
        }
        Ok(Ok(fields))
    }

    /// A charged copy of codec value `index`.
    fn value(&self, index: u32, governor: &mut Governor) -> Result<ArgTree, StopReason> {
        let tree = &self.arena.values[index as usize];
        governor.charge(Resource::ScratchBytes, tree.estimated_bytes())?;
        Ok(tree.clone())
    }

    /// Build one reading from a forward history.
    fn reading(
        &self,
        path: &[HistKind],
        shape: &Shape<'_>,
        governor: &mut Governor,
    ) -> Result<Result<RawCall, Rejection>, StopReason> {
        let span = shape.span.clone();
        let (captures, mut edits) = self.trace(path, shape.open_end, governor)?;
        let mut fields = match self.fields(&captures, governor)? {
            Ok(fields) => fields,
            Err(rejection) => return Ok(Err(rejection)),
        };
        let mut cost = shape.cost;
        for edit in core::mem::take(&mut fields.invalid) {
            cost = cost.saturating_add(edit.operator.rule().cost);
            let at = edits
                .iter()
                .position(|other| other.span.start > edit.span.start)
                .unwrap_or(edits.len());
            edits.insert(at, edit);
        }
        for (capture, _) in &captures {
            if capture.kind != CaptureKind::Dropped || capture.span.is_empty() {
                continue;
            }
            let original = self.bytes(capture.span.clone()).to_vec();
            governor.charge(
                Resource::Work,
                size(original.len()).saturating_mul(K_CAPTURE),
            )?;
            let operator = if fence_marker(&original) {
                EditOperator::ExtractFence
            } else {
                EditOperator::ExtractProse
            };
            let edit = MarkupEdit {
                operator,
                span: capture.span.clone(),
                original,
                replacement: Vec::new(),
            };
            governor.charge(Resource::ScratchBytes, edit.estimated_bytes())?;
            cost = cost.saturating_add(operator.rule().cost);
            let at = edits
                .iter()
                .position(|other| other.span.start > edit.span.start)
                .unwrap_or(edits.len());
            edits.insert(at, edit);
        }
        let closure = if shape.dangling {
            Closure::Dangling
        } else if edits
            .iter()
            .any(|edit: &MarkupEdit| edit.operator == EditOperator::MissingClosersAtEnd)
        {
            Closure::Unterminated
        } else if edits.iter().any(|edit| edit.rule().repair) {
            Closure::Repaired
        } else {
            Closure::Closed
        };
        let bare = !span.is_empty() && self.bytes(span.start..span.start + 1) == b"{";
        Ok(Ok(RawCall {
            bare,
            span,
            dialect: shape.dialect.clone(),
            kind: shape.kind,
            order: 0,
            name: fields.name,
            id: fields.id,
            arguments: fields.arguments.unwrap_or_else(ArgTree::empty_object),
            captures: captures.into_iter().map(|(capture, _)| capture).collect(),
            closure,
            edits,
            cost,
        }))
    }

    /// Materialize every reading of a completed (or cut-off) history.
    ///
    /// A history without `next` is one call. A history with `next` is one
    /// call per part: the first spans from the attempt's start to the first
    /// boundary, each further one from its boundary to the next, the last
    /// to the attempt's end (and alone keeps a cut-off's dangling closure).
    /// Readings are grouped by where their calls begin and by the cost each
    /// call carries; each group is one sequence of call segments, with the
    /// cost of each, and distinct groups are competing segmentations of the
    /// attempt's span.
    pub(crate) fn segment(
        &self,
        hist: u32,
        shape: &Shape<'_>,
        governor: &mut Governor,
    ) -> Result<Result<Vec<Parts>, Rejection>, StopReason> {
        // (boundaries with their costs, readings of each call)
        let mut groups: Vec<Grouped> = Vec::new();
        let mut rejection = None;
        'paths: for path in self.histories(hist, governor)? {
            let cuts: Vec<(usize, usize, u64)> = path
                .iter()
                .enumerate()
                .filter_map(|(index, step)| match *step {
                    HistKind::Next(at, cost) => Some((index, at, cost)),
                    _ => None,
                })
                .collect();
            let Some(calls) = self.part_readings(&path, &cuts, shape, &mut rejection, governor)?
            else {
                continue 'paths;
            };
            let boundaries: Vec<(usize, u64)> =
                cuts.iter().map(|&(_, at, cost)| (at, cost)).collect();
            let shape_of = |parts: &[PartReadings]| -> Vec<bool> {
                parts
                    .iter()
                    .map(|part| matches!(part, PartReadings::Rejected(..)))
                    .collect()
            };
            let found = groups.iter().position(|(known, parts)| {
                *known == boundaries && shape_of(parts) == shape_of(&calls)
            });
            let Some(index) = found else {
                groups.push((boundaries, calls));
                continue;
            };
            let group = &mut groups[index].1;
            for (readings, part) in group.iter_mut().zip(calls) {
                let (PartReadings::Call(readings), PartReadings::Call(mut call)) = (readings, part)
                else {
                    continue;
                };
                let call = call.remove(0);
                if !readings.contains(&call) {
                    // Every reading beyond the first competes.
                    if !readings.is_empty() {
                        governor.charge(Resource::Candidates, 1)?;
                    }
                    readings.push(call);
                }
            }
        }
        // A segmentation that read no call is no reading of the attempt: it
        // is rejected whole, as one attempt.
        groups.retain(|(_, parts)| {
            parts
                .iter()
                .any(|part| matches!(part, PartReadings::Call(..)))
        });
        if groups.is_empty() {
            return Ok(Err(
                rejection.unwrap_or(Rejection::Malformed { at: shape.span.end })
            ));
        }
        let mut leads: Vec<Vec<MarkupEdit>> = Vec::with_capacity(groups.len());
        let mut segmentations: Vec<Parts> = groups
            .into_iter()
            .map(|(boundaries, mut calls)| {
                leads.push(match calls.first_mut() {
                    Some(PartReadings::Rejected(_, _, lead)) => core::mem::take(lead),
                    _ => Vec::new(),
                });
                calls
                    .into_iter()
                    .enumerate()
                    .map(|(part, readings)| {
                        let before = if part == 0 { 0 } else { boundaries[part - 1].1 };
                        let through = boundaries.get(part).map_or(shape.cost, |&(_, cost)| cost);
                        let cost = through.saturating_sub(before);
                        match readings {
                            PartReadings::Rejected(span, reason, _) => {
                                (Part::Rejected { span, reason }, cost)
                            }
                            PartReadings::Call(readings) => {
                                let span = readings[0].span.clone();
                                let weight = self.undeclared(shape.kind, &readings);
                                (
                                    Part::Call(CallSegment {
                                        span,
                                        readings,
                                        derivations: Vec::new(),
                                    }),
                                    cost.saturating_add(weight),
                                )
                            }
                        }
                    })
                    .collect()
            })
            .collect();
        for (parts, lead) in segmentations.iter_mut().zip(leads) {
            lead_split(parts, &lead);
        }
        Ok(Ok(segmentations))
    }
}

/// When `parts` begins with rejected parts (the attempt's first call could
/// not be read and it resynchronized past it), the first call read after
/// them records the split (`tool_call.markup.group-split`) at its start, as
/// the last call before a later skipped call does at its end, and the
/// tokens `lead` read as omitted at the attempt's start (which the first
/// call carries when it is read); their cost moves to it from the rejected
/// part before it.
fn lead_split(parts: &mut Parts, lead: &[MarkupEdit]) {
    let Some(first) = parts
        .iter()
        .position(|(part, _)| matches!(part, Part::Call(..)))
    else {
        return;
    };
    if first == 0 {
        return;
    }
    let moved = lead
        .iter()
        .map(|edit| edit.operator.rule().cost)
        .fold(EditOperator::GroupSplit.rule().cost, u64::saturating_add);
    parts[first - 1].1 = parts[first - 1].1.saturating_sub(moved);
    let (Part::Call(segment), cost) = &mut parts[first] else {
        return;
    };
    *cost = cost.saturating_add(moved);
    let at = segment.span.start;
    for reading in &mut segment.readings {
        let mut edits = lead.to_vec();
        edits.push(MarkupEdit {
            operator: EditOperator::GroupSplit,
            span: at..at,
            original: Vec::new(),
            replacement: Vec::new(),
        });
        edits.append(&mut reading.edits);
        reading.edits = edits;
        reading.cost = reading.cost.saturating_add(moved);
        if reading.closure == Closure::Closed {
            reading.closure = Closure::Repaired;
        }
    }
}

/// No chain: the parts closed so far are none.
const NO_CHAIN: u32 = u32::MAX;

/// Scratch bytes charged for each closed part's node and memo entry, on top
/// of what reading the part charged.
const CHAIN_BYTES: u64 = 192;

/// One closed call of a grouped attempt's history: the call between two
/// `next` nodes (or the attempt's start and the first). Every completion
/// that extends the history shares the node, so a call is read once however
/// many calls follow it.
struct ChainNode {
    parent: u32,
    /// The part, as the segmentation of any completion holds it.
    part: Part,
    /// The cost the part carries, before [`lead_split`] moves any.
    cost: u64,
    /// The costs of the parts up to and including this one.
    cum: u64,
    /// The number of parts up to and including this one.
    depth: u32,
    /// The index of the first call among the parts up to this one.
    first_call: u32,
    /// The cost of the part before that first call, when it is not the
    /// first part (which [`lead_split`] takes cost from).
    before: u64,
    /// The attempt's lead (see [`Lead`]) when its first part was skipped.
    lead: u32,
    /// The scratch the parts up to and including this one charged.
    bytes: u64,
}

/// The tokens read as omitted at the start of an attempt whose first call
/// was skipped, and the cost [`lead_split`] moves to its first read call.
struct Lead {
    edits: Vec<MarkupEdit>,
    moved: u64,
}

/// What a `next` node closes.
#[derive(Clone, Copy)]
enum Closed {
    /// The parts before it, as a chain.
    Chain(u32),
    /// A part before it had no reading.
    Failed(Rejection),
    /// The history before it packs several readings: it is segmented whole.
    Whole,
}

/// A completion of a grouped attempt whose history is one path: the closed
/// parts it shares with the completions before it, and its last part.
/// [`Arena::parts`] materializes it.
pub(crate) struct Chained {
    pub(crate) table: u32,
    pub(crate) attempt: u64,
    pub(crate) at: usize,
    closed: u32,
    last: Part,
    last_cost: u64,
    last_bytes: u64,
    lead: u32,
    first_call: u32,
}

impl Chained {
    /// The scratch the last part charged.
    pub(crate) const fn last_bytes(&self) -> u64 {
        self.last_bytes
    }

    /// Whether the last part began its arguments.
    pub(crate) fn arguments(&self) -> bool {
        began_arguments(&self.last)
    }
}

/// Whether `part` is a call with a reading that began its arguments (the
/// model was still writing it, should the output stop there).
fn began_arguments(part: &Part) -> bool {
    let Part::Call(segment) = part else {
        return false;
    };
    segment.readings.iter().any(|reading| {
        reading.captures.iter().any(|capture| {
            matches!(
                capture.kind,
                CaptureKind::Arguments | CaptureKind::ParamName
            )
        })
    })
}

/// What reading a part of a history gave.
enum Built {
    /// The part, its cost, and the lead of a skipped first part.
    Part(Part, u64, Option<Vec<MarkupEdit>>),
    /// A call of the part had no reading.
    Failed(Rejection),
    /// Its paths do not read alike: the segmentation has several groups.
    Mixed,
}

/// The history of a completion up to its nearest `next` node.
struct Tail {
    /// Every equal-cost path (see [`Step::histories`]), forward: more than
    /// one where packed alternatives lie between the cut and the end.
    paths: Vec<Vec<HistKind>>,
    /// The `next` node, with where it cuts and the cost there; none for the
    /// attempt's start.
    cut: Option<(u32, usize, u64)>,
}

impl Arena {
    /// The parts of `chained` and the scratch holding them charges.
    pub(crate) fn parts(
        &self,
        chained: &Chained,
        governor: &mut Governor,
    ) -> Result<(Parts, u64), StopReason> {
        let mut closed = Vec::new();
        let mut at = chained.closed;
        let mut bytes = chained.last_bytes;
        if at != NO_CHAIN {
            bytes = bytes.saturating_add(self.chains[at as usize].bytes);
        }
        while at != NO_CHAIN {
            governor.charge(Resource::Work, K_CAPTURE)?;
            let node = &self.chains[at as usize];
            closed.push(at);
            at = node.parent;
        }
        governor.charge(Resource::Work, K_CAPTURE)?;
        governor.charge(Resource::ScratchBytes, bytes)?;
        let mut parts: Parts = closed
            .iter()
            .rev()
            .map(|&id| {
                let node = &self.chains[id as usize];
                (node.part.clone(), node.cost)
            })
            .collect();
        parts.push((chained.last.clone(), chained.last_cost));
        let no_lead = [];
        let lead = match chained.lead {
            NO_CHAIN => &no_lead[..],
            lead => &self.leads[lead as usize].edits,
        };
        lead_split(&mut parts, lead);
        Ok((parts, bytes))
    }

    /// How many parts `chained` closed before its last.
    pub(crate) fn depth_of(&self, chained: &Chained) -> usize {
        match chained.closed {
            NO_CHAIN => 0,
            id => self.chains[id as usize].depth as usize,
        }
    }

    /// The cost `lead_split` moves between the parts of `chained`, if any:
    /// the cost the part before its first call gives up, and the cost that
    /// call gains.
    fn moved(&self, lead: u32, first_call: u32, before: u64) -> (u64, u64) {
        if lead == NO_CHAIN || first_call == 0 || first_call == u32::MAX {
            return (0, 0);
        }
        let moved = self.leads[lead as usize].moved;
        (before.min(moved), moved)
    }

    /// What the costs of `chained`'s calls sum to, after `lead_split`.
    pub(crate) fn total(&self, chained: &Chained) -> u64 {
        let (depth, closed, tip, held) = match chained.closed {
            NO_CHAIN => (0, 0, 0, 0),
            id => {
                let node = &self.chains[id as usize];
                (node.depth, node.cum, node.cost, node.before)
            }
        };
        let before = if chained.first_call == depth {
            tip
        } else {
            held
        };
        let (down, up) = self.moved(chained.lead, chained.first_call, before);
        closed
            .saturating_add(chained.last_cost)
            .saturating_sub(down)
            .saturating_add(up)
    }

    /// The costs of the first `0..=closed` parts of `chained` summed (index
    /// `n`: the first `n` parts), after `lead_split`.
    pub(crate) fn table(
        &self,
        chained: &Chained,
        governor: &mut Governor,
    ) -> Result<Vec<u64>, StopReason> {
        let depth = match chained.closed {
            NO_CHAIN => 0,
            id => self.chains[id as usize].depth as usize,
        };
        governor.charge(Resource::Work, size(depth).saturating_add(1))?;
        governor.charge(
            Resource::ScratchBytes,
            size(depth + 1).saturating_mul(size(core::mem::size_of::<u64>())),
        )?;
        let mut costs = vec![0_u64; depth];
        let mut at = chained.closed;
        while at != NO_CHAIN {
            let node = &self.chains[at as usize];
            costs[node.depth as usize - 1] = node.cost;
            at = node.parent;
        }
        let first = chained.first_call;
        if chained.lead != NO_CHAIN && first != 0 && first != u32::MAX {
            let moved = self.leads[chained.lead as usize].moved;
            let first = first as usize;
            costs[first - 1] = costs[first - 1].saturating_sub(moved);
            if let Some(cost) = costs.get_mut(first) {
                *cost = cost.saturating_add(moved);
            }
        }
        let mut sums = Vec::with_capacity(depth + 1);
        let mut sum = 0_u64;
        sums.push(0);
        for cost in costs {
            sum = sum.saturating_add(cost);
            sums.push(sum);
        }
        Ok(sums)
    }
}

impl Step<'_, '_> {
    /// The paths of `hist` back to its nearest `next` node (or the start),
    /// each forward, deduplicated as [`Self::histories`] does; `None` when
    /// they do not all reach the same cut.
    fn tail(&self, hist: u32, governor: &mut Governor) -> Result<Option<Tail>, StopReason> {
        let mut paths: Vec<Vec<HistKind>> = Vec::new();
        let mut path: Vec<HistKind> = Vec::new();
        let mut cut: Option<Option<(u32, usize, u64)>> = None;
        let mut stack = vec![(hist, 0_usize)];
        while let Some((node, depth)) = stack.pop() {
            governor.charge(Resource::Work, K_CAPTURE)?;
            path.truncate(depth);
            let HistNode { prev, kind } = self.arena.nodes.as_slice()[node as usize];
            let reached = match kind {
                HistKind::Root => Some(None),
                HistKind::Next(position, cost) => Some(Some((node, position, cost))),
                HistKind::Pack(other) => {
                    stack.push((other, depth));
                    stack.push((prev, depth));
                    None
                }
                kind => {
                    path.push(kind);
                    stack.push((prev, depth + 1));
                    None
                }
            };
            let Some(reached) = reached else {
                continue;
            };
            if cut.is_some_and(|known| known != reached) {
                return Ok(None);
            }
            cut = Some(reached);
            let forward: Vec<HistKind> = path.iter().rev().copied().collect();
            if !paths.contains(&forward) {
                paths.push(forward);
            }
        }
        Ok(Some(Tail {
            paths,
            cut: cut.expect("a history reaches its start"),
        }))
    }

    /// One part of a history: its `paths` between the cut before it and the
    /// span's `end`. The part, its cost (the cost `span` carries less the
    /// cost at the cut before it, and the undeclared weight of a call), and
    /// the tokens read as omitted at the start when it is the first part and
    /// was skipped; or the rejection of a call with no reading. `None` when
    /// its paths do not all read alike (some skipped, or one without a
    /// reading): the segmentation then has several groups.
    fn part(
        &self,
        paths: &[Vec<HistKind>],
        first: bool,
        shape: &Shape<'_>,
        governor: &mut Governor,
    ) -> Result<Built, StopReason> {
        let skips: Vec<Option<usize>> = paths
            .iter()
            .map(|steps| {
                steps.iter().find_map(|step| match *step {
                    HistKind::Skipped(at) => Some(at),
                    _ => None,
                })
            })
            .collect();
        if skips.iter().any(Option::is_some) {
            let Some(at) = skips[0].filter(|_| skips.iter().all(Option::is_some)) else {
                return Ok(Built::Mixed);
            };
            // A token inserted where the attempt began (a section or list
            // opener the model left out) opens every call of the attempt,
            // not the skipped one alone.
            let mut lead = Vec::new();
            if first {
                for step in &paths[0] {
                    let HistKind::Edit(index) = *step else {
                        continue;
                    };
                    let edit = &self.arena.edits[index as usize];
                    if edit.operator == EditOperator::MissingToken
                        && edit.span == (shape.span.start..shape.span.start)
                    {
                        governor.charge(Resource::ScratchBytes, edit.estimated_bytes())?;
                        lead.push(edit.clone());
                    }
                }
            }
            let part = Part::Rejected {
                span: shape.span.clone(),
                reason: Rejection::Malformed { at },
            };
            return Ok(Built::Part(part, shape.cost, first.then_some(lead)));
        }
        let mut readings: Vec<RawCall> = Vec::new();
        for steps in paths {
            let call = match self.reading(steps, shape, governor)? {
                Ok(call) => call,
                Err(reason) if paths.len() == 1 => return Ok(Built::Failed(reason)),
                Err(_) => return Ok(Built::Mixed),
            };
            if !readings.contains(&call) {
                // Every reading beyond the first competes.
                if !readings.is_empty() {
                    governor.charge(Resource::Candidates, 1)?;
                }
                readings.push(call);
            }
        }
        let weight = self.undeclared(shape.kind, &readings);
        let span = readings[0].span.clone();
        let part = Part::Call(CallSegment {
            span,
            readings,
            derivations: Vec::new(),
        });
        Ok(Built::Part(part, shape.cost.saturating_add(weight), None))
    }

    /// What the `next` node `node` closes, built (with the nodes before it
    /// that are not yet) from the history before each.
    fn close(
        &mut self,
        node: u32,
        shape: &Shape<'_>,
        governor: &mut Governor,
    ) -> Result<Closed, StopReason> {
        // The unbuilt nodes, newest first, with where each cuts and the
        // steps of the part before it.
        let mut pending: Vec<(u32, usize, u64, Tail)> = Vec::new();
        let mut at = node;
        let mut state = loop {
            if let Some(&known) = self.arena.memo.get(&at) {
                break known;
            }
            let HistNode { prev, kind } = self.arena.nodes.as_slice()[at as usize];
            let HistKind::Next(position, cost) = kind else {
                unreachable!("a cut is a next node")
            };
            let Some(tail) = self.tail(prev, governor)? else {
                self.remember(at, Closed::Whole, governor)?;
                break Closed::Whole;
            };
            let before = tail.cut;
            governor.charge(Resource::ScratchBytes, CLOSE_PENDING_BYTES)?;
            pending.push((at, position, cost, tail));
            match before {
                Some((cut, ..)) => at = cut,
                None => break Closed::Chain(NO_CHAIN),
            }
        };
        while let Some((at, position, cost, tail)) = pending.pop() {
            state = match state {
                Closed::Chain(parent) => {
                    let (start, before) = match tail.cut {
                        Some((_, start, before)) => (start, before),
                        None => (shape.span.start, 0),
                    };
                    let part_shape = Shape {
                        span: start..position,
                        dialect: shape.dialect,
                        kind: shape.kind,
                        dangling: false,
                        open_end: position,
                        cost: cost.saturating_sub(before),
                    };
                    self.extend(parent, &tail.paths, &part_shape, governor)?
                }
                other => other,
            };
            self.remember(at, state, governor)?;
            governor.release_scratch(CLOSE_PENDING_BYTES)?;
        }
        Ok(state)
    }

    /// Record what `node` closes.
    fn remember(
        &mut self,
        node: u32,
        closed: Closed,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::ScratchBytes, CHAIN_BYTES)?;
        self.arena.charged = self.arena.charged.saturating_add(CHAIN_BYTES);
        self.arena.memo.insert(node, closed);
        Ok(())
    }

    /// The chain after `parent` with the closed part `steps` read as `shape`.
    fn extend(
        &mut self,
        parent: u32,
        paths: &[Vec<HistKind>],
        shape: &Shape<'_>,
        governor: &mut Governor,
    ) -> Result<Closed, StopReason> {
        let first = parent == NO_CHAIN;
        let charged = governor.scratch_meter().total_charged();
        let (part, cost, lead) = match self.part(paths, first, shape, governor)? {
            Built::Part(part, cost, lead) => (part, cost, lead),
            Built::Failed(reason) => return Ok(Closed::Failed(reason)),
            Built::Mixed => return Ok(Closed::Whole),
        };
        let bytes = governor
            .scratch_meter()
            .total_charged()
            .saturating_sub(charged);
        let (depth, cum, first_call, before, held, total) = if first {
            (0, 0, u32::MAX, 0, NO_CHAIN, 0)
        } else {
            let node = &self.arena.chains[parent as usize];
            (
                node.depth,
                node.cum,
                node.first_call,
                node.before,
                node.lead,
                node.bytes,
            )
        };
        let lead = match lead {
            Some(edits) => {
                let moved = edits
                    .iter()
                    .map(|edit| edit.operator.rule().cost)
                    .fold(EditOperator::GroupSplit.rule().cost, u64::saturating_add);
                self.arena.leads.push(Lead { edits, moved });
                index_u32(self.arena.leads.len() - 1)
            }
            None => held,
        };
        let (first_call, before) = match (&part, first_call) {
            (Part::Call(..), u32::MAX) => (depth, if first { 0 } else { self.tip_cost(parent) }),
            _ => (first_call, before),
        };
        governor.charge(Resource::ScratchBytes, CHAIN_BYTES)?;
        self.arena.charged = self
            .arena
            .charged
            .saturating_add(bytes)
            .saturating_add(CHAIN_BYTES);
        self.arena.chains.push(ChainNode {
            parent,
            part,
            cost,
            cum: cum.saturating_add(cost),
            depth: depth + 1,
            first_call,
            before,
            lead,
            bytes: total.saturating_add(bytes),
        });
        Ok(Closed::Chain(index_u32(self.arena.chains.len() - 1)))
    }

    fn tip_cost(&self, chain: u32) -> u64 {
        self.arena.chains[chain as usize].cost
    }

    /// Segment the completion `hist` as `shape` over a chain of the parts
    /// closed before its last one, when its history is one path; `None` when
    /// it packs alternatives (see [`Self::segment`]). Every part is read
    /// once however many completions extend it, so a chain of calls costs
    /// work and scratch linear in the calls.
    pub(crate) fn segment_chained(
        &mut self,
        hist: u32,
        shape: &Shape<'_>,
        governor: &mut Governor,
    ) -> Result<Option<Result<Chained, Rejection>>, StopReason> {
        let Some(tail) = self.tail(hist, governor)? else {
            return Ok(None);
        };
        let (closed, start, before) = match tail.cut {
            None => (NO_CHAIN, shape.span.start, 0),
            Some((node, start, before)) => match self.close(node, shape, governor)? {
                Closed::Chain(chain) => (chain, start, before),
                Closed::Failed(reason) => return Ok(Some(Err(reason))),
                Closed::Whole => return Ok(None),
            },
        };
        let last = closed == NO_CHAIN;
        let part_shape = Shape {
            span: start..shape.span.end,
            dialect: shape.dialect,
            kind: shape.kind,
            dangling: shape.dangling,
            open_end: shape.open_end,
            cost: shape.cost.saturating_sub(before),
        };
        let charged = governor.scratch_meter().total_charged();
        let (part, cost, lead) = match self.part(&tail.paths, last, &part_shape, governor)? {
            Built::Part(part, cost, lead) => (part, cost, lead),
            Built::Failed(reason) => return Ok(Some(Err(reason))),
            Built::Mixed => return Ok(None),
        };
        let last_bytes = governor
            .scratch_meter()
            .total_charged()
            .saturating_sub(charged);
        let (depth, first_call, held) = match closed {
            NO_CHAIN => (0, u32::MAX, NO_CHAIN),
            id => {
                let node = &self.arena.chains[id as usize];
                (node.depth, node.first_call, node.lead)
            }
        };
        let first_call = match (&part, first_call) {
            (Part::Call(..), u32::MAX) => depth,
            _ => first_call,
        };
        if first_call == u32::MAX {
            // No call was read: the attempt is rejected whole.
            return Ok(Some(Err(Rejection::Malformed { at: shape.span.end })));
        }
        let lead = match lead {
            Some(edits) => {
                let moved = edits
                    .iter()
                    .map(|edit| edit.operator.rule().cost)
                    .fold(EditOperator::GroupSplit.rule().cost, u64::saturating_add);
                self.arena.leads.push(Lead { edits, moved });
                governor.charge(Resource::ScratchBytes, CHAIN_BYTES)?;
                self.arena.charged = self.arena.charged.saturating_add(CHAIN_BYTES);
                index_u32(self.arena.leads.len() - 1)
            }
            None => held,
        };
        Ok(Some(Ok(Chained {
            table: 0,
            attempt: 0,
            at: 0,
            closed,
            last: part,
            last_cost: cost,
            last_bytes,
            lead,
            first_call,
        })))
    }
}

impl Step<'_, '_> {
    /// The readings of each part of history `path` (its `next` cuts at
    /// `cuts`), or `None` when a call part has no reading (its rejection
    /// noted in `rejection`).
    fn part_readings(
        &self,
        path: &[HistKind],
        cuts: &[(usize, usize, u64)],
        shape: &Shape<'_>,
        rejection: &mut Option<Rejection>,
        governor: &mut Governor,
    ) -> Result<Option<Vec<PartReadings>>, StopReason> {
        let mut calls: Vec<PartReadings> = Vec::with_capacity(cuts.len() + 1);
        let mut from = 0;
        for part in 0..=cuts.len() {
            let last = part == cuts.len();
            let start = if part == 0 {
                shape.span.start
            } else {
                cuts[part - 1].1
            };
            let (to, end) = if last {
                (path.len(), shape.span.end)
            } else {
                (cuts[part].0, cuts[part].1)
            };
            let before = if part == 0 { 0 } else { cuts[part - 1].2 };
            let through = if last { shape.cost } else { cuts[part].2 };
            let part_shape = Shape {
                span: start..end,
                dialect: shape.dialect,
                kind: shape.kind,
                dangling: last && shape.dangling,
                open_end: if last { shape.open_end } else { end },
                cost: through.saturating_sub(before),
            };
            let skipped = path[from..to].iter().find_map(|step| match *step {
                HistKind::Skipped(at) => Some(at),
                _ => None,
            });
            if let Some(at) = skipped {
                // A token inserted where the attempt began (a section or
                // list opener the model left out) opens every call of the
                // attempt, not the skipped one alone.
                let mut lead = Vec::new();
                if part == 0 {
                    for step in &path[from..to] {
                        let HistKind::Edit(index) = *step else {
                            continue;
                        };
                        let edit = &self.arena.edits[index as usize];
                        if edit.operator == EditOperator::MissingToken
                            && edit.span == (start..start)
                        {
                            governor.charge(Resource::ScratchBytes, edit.estimated_bytes())?;
                            lead.push(edit.clone());
                        }
                    }
                }
                calls.push(PartReadings::Rejected(
                    start..end,
                    Rejection::Malformed { at },
                    lead,
                ));
            } else {
                match self.reading(&path[from..to], &part_shape, governor)? {
                    Ok(call) => calls.push(PartReadings::Call(vec![call])),
                    Err(reason) => {
                        rejection.get_or_insert(reason);
                        return Ok(None);
                    }
                }
            }
            from = to + 1;
        }
        Ok(Some(calls))
    }

    /// The selection weight of a call none of whose readings names a
    /// declared tool ([`UNDECLARED_NAME`]), when the tool set is
    /// known; zero for a result, or without a tool set.
    fn undeclared(&self, kind: ProductionKind, readings: &[RawCall]) -> u64 {
        let Some(tools) = self.tools else {
            return 0;
        };
        let declared = readings.iter().any(|reading| {
            reading
                .name
                .as_ref()
                .is_some_and(|name| tools.lookup_exact(&name.value).is_some())
        });
        if kind == ProductionKind::Call && !declared {
            UNDECLARED_NAME
        } else {
            0
        }
    }
}

/// The call-closer literals of `program` the failing configuration at `pc`
/// could still reach without passing a `next` (where its call can end),
/// and the opener literals (multi-byte, or a JSON element's `{`) of the
/// call after each `next` it reaches (where the next call begins, should
/// its call have no closer):
/// see `Engine::resync`. One `Work` unit per location visited.
fn resync_targets(
    program: &Program,
    grammar: &CompiledGrammar,
    pc: u32,
    governor: &mut Governor,
) -> Result<Vec<u32>, StopReason> {
    let mut targets: Vec<u32> = Vec::new();
    let mut seen = vec![false; program.insts.len()];
    let mut stack = vec![pc];
    let opener = |pc: u32| match program.insts[pc as usize] {
        Inst::Lit { lit, .. } => {
            let bytes = grammar.literals[lit as usize].bytes.as_slice();
            program.call_openers.contains(&pc) && (bytes.len() > 1 || bytes == b"{")
        }
        _ => false,
    };
    while let Some(pc) = stack.pop() {
        if core::mem::replace(&mut seen[pc as usize], true) {
            continue;
        }
        governor.charge(Resource::Work, 1)?;
        let inst = program.insts[pc as usize];
        if let Inst::Next { next } = inst {
            // The next call's openers, reached without input.
            let mut inner = vec![next];
            while let Some(at) = inner.pop() {
                if core::mem::replace(&mut seen[at as usize], true) {
                    continue;
                }
                governor.charge(Resource::Work, 1)?;
                match program.insts[at as usize] {
                    Inst::Lit { .. } => {
                        if opener(at) && !targets.contains(&at) {
                            targets.push(at);
                        }
                    }
                    Inst::Split(first, second) => inner.extend([first, second]),
                    Inst::Open { next, .. } | Inst::Close { next, .. } | Inst::Commit { next } => {
                        inner.push(next);
                    }
                    _ => {}
                }
            }
            continue;
        }
        if program.call_closers.contains(&pc) {
            targets.push(pc);
        }
        // Past a closer too: the next call begins after it.
        stack.extend(super::compile::successors(&inst));
    }
    Ok(targets)
}

/// A scan of JSON-shaped bytes: bracket depth outside strings, and whether
/// inside a string and after a backslash in one.
#[derive(Clone, Copy, Debug, Default)]
struct JsonScan {
    depth: u32,
    quoted: bool,
    escaped: bool,
    /// Whether the last byte outside strings and whitespace was a `:` (a
    /// value comes next).
    colon: bool,
}

impl JsonScan {
    fn step(&mut self, byte: u8) {
        if self.quoted {
            match byte {
                _ if self.escaped => self.escaped = false,
                b'\\' => self.escaped = true,
                b'"' => self.quoted = false,
                _ => {}
            }
            return;
        }
        if byte.is_ascii_whitespace() {
            return;
        }
        self.colon = byte == b':';
        match byte {
            b'"' => self.quoted = true,
            b'{' | b'[' => self.depth = (self.depth + 1).min(0xFFFF),
            b'}' | b']' => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
    }

    /// The scan as a [`Sub::Resync`] state: the depth in the low 16 bits,
    /// then the three flags.
    const fn pack(self) -> u32 {
        self.depth
            | if self.quoted { 0x1_0000 } else { 0 }
            | if self.escaped { 0x2_0000 } else { 0 }
            | if self.colon { 0x4_0000 } else { 0 }
    }

    const fn unpack(state: u32) -> Self {
        Self {
            depth: state & 0xFFFF,
            quoted: state & 0x1_0000 != 0,
            escaped: state & 0x2_0000 != 0,
            colon: state & 0x4_0000 != 0,
        }
    }
}

/// How a slot's codec failed to finish (see `Step::finish_codec`).
pub(crate) enum Finished {
    /// The codec faulted on its body.
    Fault(CodecFault),
    /// A JSON value no repair reads, which stopped decoding at `at`.
    Unread { at: usize },
}

/// One part of a completed attempt's segmentation.
#[derive(Clone, Debug)]
pub(crate) enum Part {
    /// A call (or result) the attempt read.
    Call(CallSegment),
    /// A call the attempt could not read and resynchronized past
    /// ([`HistKind::Skipped`]): a rejected attempt of its own.
    Rejected {
        span: Range<usize>,
        reason: Rejection,
    },
}

/// One segmentation of a completed attempt: each part, in order, with the
/// cost it carries.
pub(crate) type Parts = Vec<(Part, u64)>;

/// The readings of one part while they are collected.
#[derive(Clone, Debug, PartialEq)]
enum PartReadings {
    Call(Vec<RawCall>),
    /// A skipped call: its span, why it was rejected, and (for the
    /// attempt's first call) the tokens read as omitted at the attempt's
    /// start, which belong to the calls the attempt went on to read.
    Rejected(Range<usize>, Rejection, Vec<MarkupEdit>),
}

/// The readings of one segmentation while they are collected: where each
/// later call begins (with the cost so far), and each part's readings.
type Grouped = (Vec<(usize, u64)>, Vec<PartReadings>);

/// Whether dropped text is only a code-fence marker: a run of at least
/// three backticks or tildes, optionally followed by an info string on the
/// same line, with surrounding ASCII whitespace.
fn fence_marker(text: &[u8]) -> bool {
    let text = text.trim_ascii();
    let Some(&fence) = text.first().filter(|byte| matches!(byte, b'`' | b'~')) else {
        return false;
    };
    let run = text.iter().take_while(|&&byte| byte == fence).count();
    let info = &text[run..];
    run >= 3 && !info.contains(&b'\n') && !(fence == b'`' && info.contains(&b'`'))
}

/// The rejection of a call attempt whose codec faulted.
const fn rejection(fault: CodecFault) -> Rejection {
    match fault {
        CodecFault::InvalidUtf8 { at } => Rejection::InvalidUtf8 { at },
        CodecFault::Malformed { at } => Rejection::Malformed { at },
        CodecFault::PythonicPositionalRejected { at } => {
            Rejection::PythonicPositionalRejected { at }
        }
    }
}

/// A reading's captures (with their codec values) and edits.
type Trace = (Vec<(CaptureSpan, Option<u32>)>, Vec<MarkupEdit>);

/// The name, id and arguments of one reading.
struct Fields {
    name: Option<Spanned<String>>,
    id: Option<Spanned<String>>,
    arguments: Option<ArgTree>,
    /// `tool_call.value.invalid-utf8` edits of the name, id and parameter
    /// names.
    invalid: Vec<MarkupEdit>,
}

/// What every reading of one completion shares.
pub(crate) struct Shape<'d> {
    pub(crate) span: Range<usize>,
    pub(crate) dialect: &'d DialectId,
    pub(crate) kind: ProductionKind,
    pub(crate) dangling: bool,
    /// Where the captures still open at the end of `span` close: before
    /// the character a cut-off class run had only begun, else `span.end`.
    pub(crate) open_end: usize,
    pub(crate) cost: u64,
}

/// Merge configurations sharing a key: the earliest attempt, then the least
/// cost, then the entry (see `Thread::rank`) wins; exact ties pack their
/// histories.
pub(crate) fn merge(
    threads: &mut ScratchVec<Thread>,
    arena: &mut Arena,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    merge_noting(threads, arena, governor, |_| {})
}

/// [`merge`], calling `note` on every configuration kept, in the same
/// charged pass.
pub(crate) fn merge_noting(
    threads: &mut ScratchVec<Thread>,
    arena: &mut Arena,
    governor: &mut Governor,
    mut note: impl FnMut(&Thread),
) -> Result<(), StopReason> {
    governor.charge(Resource::Work, size(threads.len()).saturating_mul(K_THREAD))?;
    let slice = threads.as_mut_slice();
    // In place: configurations equal in key, rank and history are equal
    // but for `stall` (checked below in debug builds), which merging
    // minimizes, so an unstable sort merges exactly as a stable one would,
    // and allocates nothing uncharged.
    slice.sort_unstable_by(|a, b| {
        a.key()
            .cmp(&b.key())
            .then(a.rank().cmp(&b.rank()))
            .then(a.hist.cmp(&b.hist))
    });
    debug_assert!(slice.windows(2).all(|pair| {
        let (mut first, mut second) = (pair[0], pair[1]);
        (first.key(), first.rank(), first.hist) != (second.key(), second.rank(), second.hist) || {
            first.stall = 0;
            second.stall = 0;
            first == second
        }
    }));
    let mut write = 0;
    for read in 0..slice.len() {
        let current = slice[read];
        if write > 0 && slice[write - 1].key() == current.key() {
            let winner = &mut slice[write - 1];
            if winner.rank() == current.rank() {
                winner.hist = arena.pack(winner.hist, current.hist)?;
                winner.stall = winner.stall.min(current.stall);
            }
            continue;
        }
        slice[write] = current;
        write += 1;
    }
    while threads.len() > write {
        threads.pop();
    }
    for thread in threads.as_slice() {
        note(thread);
    }
    Ok(())
}

/// A configuration the repair search accepted.
struct Repaired {
    thread: Thread,
    status: Status,
}

/// One call attempt of one table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Attempt {
    pub(crate) id: u64,
    pub(crate) start: usize,
    pub(crate) opened: bool,
    pub(crate) completed: bool,
    pub(crate) failure: Option<Rejection>,
    /// Where a rejected span ends.
    pub(crate) end: Option<usize>,
    /// Where the attempt's span ends when it ends rejected at the end of
    /// output (see [`FailureEnd`]).
    pub(crate) failure_end: FailureEnd,
    /// Whether `failure` was noted before the end of output: every reading
    /// of the attempt as written ran out there, and only readings beside
    /// one that ended at a delimiter (see [`Thread::beside`]) kept it live.
    /// Should none of those complete, the attempt ends rejected as it would
    /// have there, not as cut off by the end of output.
    pub(crate) stuck: bool,
    /// Where a completion of it that waits on its trail (stray closers
    /// after it, an inserted wrapper closer) ends: it closed a call there,
    /// which a value holding it holds whole (see `Step::opens_call`).
    pub(crate) closed: Option<usize>,
}

/// Where a rejected attempt's span ends at the end of output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FailureEnd {
    /// At the end of output.
    Stop,
    /// Where it failed: it failed in a slot value no repair reads that ran
    /// to the end of output (see `Step::end_body`).
    Failure,
    /// Where it failed: it resynchronized past a call it could not read
    /// (see `Engine::resync`), so its first failure stands for the attempt
    /// (a later call's failure is that call's own).
    Resynced,
}

impl FailureEnd {
    /// Whether the span ends where the attempt failed.
    const fn at_failure(self) -> bool {
        !matches!(self, Self::Stop)
    }
}

impl ScratchCharge for Attempt {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 80);
        80
    };
}

/// A notification to the turn-level segmentation.
#[derive(Clone, Debug)]
pub(crate) enum Event {
    /// Attempt `attempt` (opener at `start`) finished its opener at `at`.
    Opened {
        attempt: u64,
        start: usize,
        at: usize,
    },
    /// Attempt `attempt` matched `parts` (one call segment, or one per
    /// call of a production with `next`, each with its cost), ending at
    /// `at`. `charged` is the scratch materializing `parts` charged (zero
    /// when one completion has several segmentations): the turn level
    /// releases it when it drops the parts unused.
    Completed {
        table: u32,
        attempt: u64,
        at: usize,
        parts: Parts,
        charged: u64,
    },
    /// Attempt `attempt` has no configuration left at `at`.
    Ended {
        table: u32,
        attempt: u64,
        start: usize,
        at: usize,
        completed: bool,
        reason: Rejection,
        /// For a rejected attempt of a production with `next` that read
        /// whole calls before the one it failed in: those calls (see
        /// [`Salvaged`]); the rejected span then begins where the failing
        /// call does.
        salvage: Option<Salvaged>,
        /// For a rejected attempt: another dialect's attempt that began at
        /// the same opener read on past where this one failed and read a
        /// whole call (see [`Engine::defer`]).
        outread: bool,
    },
}

/// The calls a rejected attempt of a production with `next` read whole
/// before the call it failed in (`tool_call.markup.group-split`): each
/// segmentation of the span from the attempt's start to `at`, where the
/// failing call begins, and the scratch materializing them charged (as for
/// [`Event::Completed`]).
#[derive(Clone, Debug)]
pub(crate) struct Salvaged {
    pub(crate) at: usize,
    pub(crate) segmentations: Vec<Parts>,
    pub(crate) charged: u64,
}

/// Where a stuck attempt of a production with `next` last began a call:
/// the boundary's offset, the history before it, and the cost there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Boundary {
    attempt: u64,
    at: usize,
    hist: u32,
    cost: u64,
    /// The failing configuration's location, where it failed, and where
    /// its terminal began.
    pc: u32,
    failed: usize,
    reason: Rejection,
    entry: usize,
}

impl ScratchCharge for Boundary {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 72);
        72
    };
}

impl Event {
    pub(crate) const fn at(&self) -> usize {
        match self {
            Self::Opened { at, .. } | Self::Completed { at, .. } | Self::Ended { at, .. } => *at,
        }
    }
}

struct Table<'s> {
    grammar: usize,
    kind: ProductionKind,
    program: &'s Program,
    threads: ScratchVec<Thread>,
    attempts: ScratchVec<Attempt>,
    trails: Vec<Trail>,
    /// Completions of a grouped program's live attempts (see
    /// `Program::grouped`), released when the attempt ends.
    held: HeldSet,
    /// Completions of readings beside another (a body that read on past a
    /// closer; see [`Thread::beside`]) of live attempts, by attempt, oldest
    /// first: when the attempt ends, the newest whose value is read is
    /// reported (see `Engine::release_past`).
    past: Vec<Thread>,
    /// The completions as written reported for live attempts: attempt, end
    /// and cost. A held completion beside another reading of the same span
    /// that already costs more (its value not read yet: reading it only
    /// adds) can never be chosen, so it is dropped unread (see
    /// `Engine::release_past`).
    written: Vec<(u64, usize, u64)>,
    /// The newest failure of a reading beside another of each live attempt
    /// of a program without `next`, repaired once the attempt's body that
    /// reads on past closers is gone (see `Engine::settle`): of the readings
    /// that end the value at successive closers only the last can be
    /// reported, so only the last one's failure is worth a repair search.
    stalled: ScratchVec<Failure>,
    /// Literal failures of live attempts kept as repair seeds while a
    /// configuration of the same attempt that began its terminal at or
    /// before the failed literal's start is still live (see `settle`).
    failures: HeldFailures,
    /// For a grouped program, the last call boundary of each attempt that
    /// got stuck after one (see [`Salvaged`]).
    boundaries: ScratchVec<Boundary>,
}

/// The scratch one held failure is charged: the failure itself
/// ([`Failure::CHARGE`]), plus its share of the ordered maps that hold it and
/// of the key that finds it by `entry`, including B-tree node slack. A
/// counting allocator measured 274 bytes for a keyed failure and 240 for a
/// trailing one, with a 120-byte `Failure`. The allowance keeps the charge at
/// or above that whatever the size of `Failure`.
const HELD_FAILURE_INDEX_BYTES: usize = 128;
const HELD_FAILURE_CHARGE: u64 = {
    assert!(core::mem::size_of::<Failure>() <= Failure::CHARGE);
    (Failure::CHARGE + HELD_FAILURE_INDEX_BYTES) as u64
};

/// The literal failures of live attempts a table keeps between steps (see
/// `Engine::hold_failures`), grouped by attempt, each group in the order its
/// failures were held.
///
/// A held failure survives a step while its attempt has a configuration
/// that is not speculative and, unless the failure is a `trailing` one (a
/// JSON closer that met a comma, held while any configuration of its
/// attempt lives), began its terminal at or before the failure's. Every
/// other failure is therefore keyed by `entry` in an ordered set: a step
/// examines only the failures whose `entry` lies below the attempt's
/// earliest live `entry` (the ones it drops) and the groups of attempts
/// with no live configuration, never the failures that stay held.
struct HeldFailures {
    groups: BTreeMap<u64, HeldGroup>,
    meter: ScratchMeter,
    /// The scratch charged for the held failures.
    reserved: u64,
    /// The `floors` of the last [`Self::retain`] that examined the groups:
    /// while they stand, no held failure became droppable, so the groups
    /// are not examined again.
    settled: ScratchVec<(u64, usize)>,
}

/// One attempt's held failures.
#[derive(Default)]
struct HeldGroup {
    /// The sequence number the next held failure takes.
    next: u64,
    /// The failures, by the order they were held.
    items: BTreeMap<u64, Failure>,
    /// The failures that are not `trailing`: `(entry, sequence)`.
    keyed: BTreeSet<(usize, u64)>,
}

impl HeldFailures {
    fn new(governor: &Governor) -> Self {
        Self {
            groups: BTreeMap::new(),
            meter: governor.scratch_meter(),
            reserved: 0,
            settled: ScratchVec::new(governor),
        }
    }

    /// The failures held for `attempt`, oldest first.
    fn of(&self, attempt: u64) -> impl Iterator<Item = &Failure> {
        self.groups
            .get(&attempt)
            .into_iter()
            .flat_map(|group| group.items.values())
    }

    /// How many failures are held for `attempt`.
    fn count(&self, attempt: u64) -> usize {
        self.groups
            .get(&attempt)
            .map_or(0, |group| group.items.len())
    }

    fn release(&mut self, failures: usize) {
        let amount = size(failures).saturating_mul(HELD_FAILURE_CHARGE);
        self.reserved = self.reserved.saturating_sub(amount);
        let _ = self.meter.release(amount);
    }

    /// Drop every failure held for `attempt`; the number dropped.
    fn remove_attempt(&mut self, attempt: u64) -> usize {
        let dropped = self
            .groups
            .remove(&attempt)
            .map_or(0, |group| group.items.len());
        self.release(dropped);
        dropped
    }

    /// Drop every failure.
    fn clear(&mut self) {
        let held: usize = self.groups.values().map(|group| group.items.len()).sum();
        self.groups.clear();
        self.release(held);
    }

    /// Drop the failures `floors` shows are no longer repair seeds, then
    /// hold the `fresh` ones that are. `floors` lists the earliest `entry`
    /// of a non-speculative live configuration of each attempt that has one,
    /// by attempt. The groups are examined only when `floors` differ from
    /// the last examination's: `Work` per group, and per failure dropped.
    fn retain(
        &mut self,
        floors: &[(u64, usize)],
        fresh: &[(Failure, bool)],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let floor_of = |attempt: u64| {
            floors
                .binary_search_by_key(&attempt, |&(owner, _)| owner)
                .ok()
                .map(|index| floors[index].1)
        };
        // A failure is droppable only when its attempt's floor rose or its
        // last configuration went, and a failure held under these floors
        // was not droppable then: with the floors unchanged only the fresh
        // failures need a look.
        let examine = self.settled.as_slice() != floors;
        if examine {
            self.settled.clear();
            self.settled.extend(floors.iter().copied())?;
            self.drop_below(&floor_of, governor)?;
        }
        self.hold(&floor_of, fresh)?;
        Ok(())
    }

    /// Drop every held failure `floor_of` shows is no longer a repair seed.
    /// `Work` per group and per failure dropped.
    fn drop_below(
        &mut self,
        floor_of: &impl Fn(u64) -> Option<usize>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, size(self.groups.len()))?;
        let _emptied = governor.lease_scratch(size(self.groups.len()).saturating_mul(8))?;
        let mut dropped = 0;
        let mut emptied = Vec::new();
        for (&attempt, group) in &mut self.groups {
            let Some(floor) = floor_of(attempt) else {
                governor.charge(Resource::Work, size(group.items.len()))?;
                dropped += group.items.len();
                emptied.push(attempt);
                continue;
            };
            while let Some(&(entry, order)) = group.keyed.first() {
                if entry >= floor {
                    break;
                }
                governor.charge(Resource::Work, 1)?;
                group.keyed.pop_first();
                group.items.remove(&order);
                dropped += 1;
            }
        }
        for attempt in emptied {
            self.groups.remove(&attempt);
        }
        self.release(dropped);
        Ok(())
    }

    /// Hold the `fresh` failures `floor_of` shows are repair seeds.
    fn hold(
        &mut self,
        floor_of: &impl Fn(u64) -> Option<usize>,
        fresh: &[(Failure, bool)],
    ) -> Result<(), StopReason> {
        for &(failure, trailing) in fresh {
            let Some(floor) = floor_of(failure.thread.attempt) else {
                continue;
            };
            if !(trailing || floor <= failure.thread.entry) {
                continue;
            }
            self.meter.charge(HELD_FAILURE_CHARGE)?;
            self.reserved = self.reserved.saturating_add(HELD_FAILURE_CHARGE);
            let group = self.groups.entry(failure.thread.attempt).or_default();
            let order = group.next;
            group.next += 1;
            if !trailing {
                group.keyed.insert((failure.thread.entry, order));
            }
            group.items.insert(order, failure);
        }
        Ok(())
    }
}

impl Drop for HeldFailures {
    fn drop(&mut self) {
        let _ = self.meter.release(self.reserved);
    }
}

/// A completion of a grouped program's attempt, held until the attempt
/// ends.
struct Held {
    attempt: u64,
    /// Where the completion ends.
    at: usize,
    body: HeldBody,
    /// How many calls the completion reads, and what their costs sum to.
    calls: usize,
    total: u64,
    dangling: bool,
    /// Whether the last call of a cut-off reading began its arguments: only
    /// then is it evidence that the attempt went on past an earlier
    /// completion (a name alone, or a separator, may be text after it).
    arguments: bool,
}

/// The held completions of a table, by attempt, and the earliest end among
/// them (kept, so asking for it costs nothing per byte).
#[derive(Default)]
struct HeldSet {
    /// Each attempt's completions with the earliest end among them.
    by_attempt: BTreeMap<u64, (usize, Vec<Held>)>,
    /// The earliest end of any held completion.
    floor: Option<usize>,
}

impl HeldSet {
    fn push(&mut self, entry: Held) {
        self.floor = Some(self.floor.map_or(entry.at, |floor| floor.min(entry.at)));
        let (earliest, entries) = self
            .by_attempt
            .entry(entry.attempt)
            .or_insert_with(|| (entry.at, Vec::new()));
        *earliest = (*earliest).min(entry.at);
        entries.push(entry);
    }

    /// Take the completions of `attempt`, oldest first.
    fn take(&mut self, attempt: u64) -> Vec<Held> {
        let Some((_, taken)) = self.by_attempt.remove(&attempt) else {
            return Vec::new();
        };
        self.floor = self
            .by_attempt
            .values()
            .map(|&(earliest, _)| earliest)
            .min();
        taken
    }
}

/// What a held completion stands for.
enum HeldBody {
    /// Its event, and the cost each call carries, in order.
    Event(Event, Vec<u64>),
    /// A completion whose history is one path, sharing its parts with the
    /// completions before it; materialized only if no later completion
    /// dominates it.
    Chain(Chained),
}

/// A completed configuration held while closer spellings of its grammar
/// that follow it are absorbed into it as stray closers
/// (`tool_call.markup.stray-closer`), so a repeated or stray closer after a
/// call never leaks into content. Closers may be separated from the call
/// and from one another by ASCII whitespace (space, tab, newline, carriage
/// return) of at most [`STRAY_CLOSER_GAP`] bytes; each absorbed span runs
/// from the call's current end through the closer, whitespace included,
/// and whitespace after the last absorbed closer stays content. The
/// completion is reported once the bytes after it cannot continue a gap and
/// a closer spelling: within `STRAY_CLOSER_GAP` plus the longest literal
/// bytes of its current end, which the recognizer's lookbehind covers
/// ([`DialectSet::lookbehind`](crate::tool_call::DialectSet::lookbehind)).
#[derive(Clone, Copy, Debug)]
struct Trail {
    /// The completed configuration; `pos` is the call's current end.
    thread: Thread,
    /// The stray-closer trie node of the spelling being matched.
    node: u32,
    /// The next byte to examine.
    at: usize,
    /// The end of the longest complete spelling matched so far.
    accepted: Option<usize>,
    /// For a completion that inserted the call's wrapper closer
    /// mid-message, once the stray closers after it are absorbed: the
    /// lookahead deciding that insertion (see [`Engine::scan_trail`]).
    scan: Option<Scan>,
}

/// The lookahead after a call ended by an inserted wrapper closer.
#[derive(Clone, Copy, Debug)]
struct Scan {
    /// The first byte after the call that is not ASCII whitespace.
    text: Option<usize>,
}

/// What the bytes after a held completion decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    /// Not yet decided.
    Held,
    /// Report the completion as it is.
    Report,
    /// Report it, its inserted wrapper closer a guess
    /// (`tool_call.markup.missing-wrapper-close.before-text`).
    Guess,
    /// Not a call: the text after it is inside its wrapper.
    Reject,
}

struct GrammarRuntime<'s> {
    grammar: &'s CompiledGrammar,
    dialect: DialectId,
    dfas: Vec<DerivativeDfa<'s>>,
}

/// The segment-candidate layer over every table of a dialect set.
pub(crate) struct Engine<'s> {
    tables: Vec<Table<'s>>,
    grammars: Vec<GrammarRuntime<'s>>,
    buffer: ScratchVec<u8>,
    base: usize,
    fed: usize,
    /// Whitespace-only prefix of the current source line, independent of buffering.
    line_blank: bool,
    arena: Arena,
    next_attempt: u64,
    stack: ScratchVec<Thread>,
    out: Out,
    visited: Vec<u64>,
    pending: Vec<(u32, u32, u64)>,
    tools: Option<&'s ToolSet>,
    pub(crate) events: Vec<Event>,
    /// The latest start of any attempt that opened (see
    /// [`Step::newest_open`]).
    newest_open: usize,
    /// See [`Step::resynced`]; an entry leaves when its attempt ends.
    resynced: Vec<(u64, usize, usize)>,
    /// See [`Step::quoted_openers`]; an entry leaves when its attempt ends.
    quoted_openers: Vec<(u64, usize)>,
    /// The failures each attempt resynchronized from (see
    /// [`Self::resync`]), so a resynchronization that gave up is not
    /// started again; an entry leaves when its attempt ends.
    spawned: Vec<(u64, usize)>,
    /// Ended reports held by [`Self::defer`], with their attempts' start.
    deferred: Vec<(usize, Event)>,
    /// The openers where an attempt read a whole call, while a held report
    /// or a live attempt at them remains (see [`Self::defer`]).
    whole: Vec<usize>,
}

/// Scratch bytes charged per completion as written recorded (see
/// [`Table::written`]).
const WRITTEN_BYTES: u64 = 24;

/// The scratch one unbuilt `next` node of [`Step::close`] holds while its
/// chain is built (its cut, cost and the tail's own vectors, which are
/// charged where they are built).
const CLOSE_PENDING_BYTES: u64 = {
    assert!(core::mem::size_of::<(u32, usize, u64, Tail)>() <= 96);
    96
};

/// The scratch one held completion takes in the working vectors of
/// [`Engine::release`]: its position in the order (8), its prefix-cost
/// table slot (24) and its keep flag (1), rounded up.
const RELEASE_VECTOR_BYTES: u64 = 40;

/// Scratch bytes charged per held ended report or recorded opener.
const DEFERRED_BYTES: u64 = 64;

impl<'s> Engine<'s> {
    pub(crate) fn new(
        dialects: &'s [DialectId],
        tools: Option<&'s ToolSet>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let mut grammars = Vec::with_capacity(dialects.len());
        let mut tables = Vec::with_capacity(dialects.len() * 2);
        for (index, dialect) in dialects.iter().enumerate() {
            let grammar = dialect.grammar();
            let mut dfas = Vec::with_capacity(grammar.classes.len());
            for class in &grammar.classes {
                dfas.push(class.automaton.dfa(governor)?);
            }
            grammars.push(GrammarRuntime {
                grammar,
                dialect: dialect.clone(),
                dfas,
            });
            for (kind, program) in [ProductionKind::Call, ProductionKind::Result]
                .into_iter()
                .zip(&grammar.programs)
            {
                tables.push(Table {
                    grammar: index,
                    kind,
                    program,
                    threads: ScratchVec::new(governor),
                    attempts: ScratchVec::new(governor),
                    trails: Vec::new(),
                    held: HeldSet::default(),
                    past: Vec::new(),
                    written: Vec::new(),
                    stalled: ScratchVec::new(governor),
                    failures: HeldFailures::new(governor),
                    boundaries: ScratchVec::new(governor),
                });
            }
        }
        Ok(Self {
            tables,
            grammars,
            buffer: ScratchVec::new(governor),
            base: 0,
            fed: 0,
            line_blank: true,
            arena: Arena::new(governor)?,
            next_attempt: 0,
            stack: ScratchVec::new(governor),
            out: Out::new(governor),
            visited: Vec::new(),
            pending: Vec::new(),
            newest_open: 0,
            resynced: Vec::new(),
            quoted_openers: Vec::new(),
            spawned: Vec::new(),
            deferred: Vec::new(),
            whole: Vec::new(),
            tools,
            events: Vec::new(),
        })
    }

    /// The number of bytes consumed.
    pub(crate) const fn fed(&self) -> usize {
        self.fed
    }

    pub(crate) fn dialect(&self, table: u32) -> &DialectId {
        &self.grammars[self.tables[table as usize].grammar].dialect
    }

    pub(crate) fn kind(&self, table: u32) -> ProductionKind {
        self.tables[table as usize].kind
    }

    fn stall_cap(&self, table: usize) -> u32 {
        let grammar = self.grammars[self.tables[table].grammar].grammar;
        let program = self.tables[table].program;
        index_u32((program.insts.len() + 1) * (grammar.longest + 2))
    }

    fn split(&mut self, table: usize) -> (Step<'_, 's>, &mut ScratchVec<Thread>, &mut Out) {
        let stall_cap = self.stall_cap(table);
        let runtime = &mut self.grammars[self.tables[table].grammar];
        (
            Step {
                grammar: runtime.grammar,
                program: self.tables[table].program,
                kind: self.tables[table].kind,
                dfas: &mut runtime.dfas,
                arena: &mut self.arena,
                buffer: self.buffer.as_slice(),
                base: self.base,
                stall_cap,
                visited: &mut self.visited,
                pending: &mut self.pending,
                tools: self.tools,
                newest_open: self.newest_open,
                resynced: &mut self.resynced,
                quoted_openers: &mut self.quoted_openers,
                attempts: self.tables[table].attempts.as_slice(),
            },
            &mut self.stack,
            &mut self.out,
        )
    }

    /// Consume one byte.
    pub(crate) fn push(&mut self, byte: u8, governor: &mut Governor) -> Result<(), StopReason> {
        let at = self.fed;
        if self.buffer.is_empty() {
            self.base = at;
        }
        self.buffer.push(byte)?;
        self.fed += 1;
        for table in 0..self.tables.len() {
            self.step_table(table, byte, at, governor)?;
        }
        self.line_blank = match byte {
            b'\r' | b'\n' => true,
            b' ' | b'\t' => self.line_blank,
            _ => false,
        };
        self.collect(governor)
    }

    fn step_table(
        &mut self,
        table: usize,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let program = self.tables[table].program;
        let mut spawned = None;
        let line_blank = self.line_blank;
        let grammar = self.grammars[self.tables[table].grammar].grammar;
        let opener_allowed = |pc: &u32| {
            if !line_blank && program.line_start_openers.contains(pc) {
                return false;
            }
            let Inst::Lit { lit, .. } = program.insts[*pc as usize] else {
                return false;
            };
            let repairs = &grammar.literal_tables[lit as usize];
            grammar.literals[lit as usize].bytes.first() == Some(&byte)
                || repairs
                    .variants
                    .trie
                    .first_bytes()
                    .chain(repairs.confusables.trie.first_bytes())
                    .chain(
                        repairs
                            .json
                            .iter()
                            .flat_map(|table| table.trie.first_bytes()),
                    )
                    .any(|first| first == byte)
        };
        if program.starts_with(byte) && program.openers.iter().any(opener_allowed) {
            let id = self.next_attempt;
            self.next_attempt += 1;
            self.tables[table].attempts.push(Attempt {
                id,
                start: at,
                opened: false,
                completed: false,
                failure: None,
                end: None,
                failure_end: FailureEnd::Stop,
                stuck: false,
                closed: None,
            })?;
            let thread = Thread {
                pc: program.start,
                sub: Sub::Lit(0),
                attempt: id,
                start: at,
                entry: at,
                pos: at,
                cost: 0,
                hist: ROOT,
                opened: false,
                stall: 0,
                reach: at,
                past: false,
                pending: 0,
            };
            if program.start_closure {
                // Zero-width instructions with effects (an `insert`) lie
                // before the opener: follow them from the start.
                spawned = Some(thread);
            } else {
                for &pc in &program.openers {
                    if opener_allowed(&pc) {
                        self.tables[table].threads.push(Thread { pc, ..thread })?;
                    }
                }
            }
        }
        // An attempt whose configurations were stopped outside a step (see
        // `Self::stop_past_at`) is still settled: its held repair and its
        // end are due.
        if spawned.is_none()
            && self.tables[table].threads.is_empty()
            && self.tables[table].trails.is_empty()
            && self.tables[table].attempts.is_empty()
        {
            return Ok(());
        }
        {
            let threads = &mut self.tables[table].threads;
            self.stack.clear();
            while let Some(thread) = threads.pop() {
                self.stack.push(thread)?;
            }
        }
        self.out.clear();
        let (mut step, stack, out) = self.split(table);
        if let Some(thread) = spawned {
            step.closure(thread, thread.pc, at, stack, out, governor)?;
        }
        step.advance(stack, at + 1, false, out, governor)?;
        self.settle(table, at, None, governor)
    }

    /// Resolve one table's step: merge, complete, repair, and retire
    /// attempts. `eof` is the stop reason at the end of input.
    fn settle(
        &mut self,
        table: usize,
        at: usize,
        eof: Option<&GenerationStop>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        self.advance_trails(table, self.fed, eof.is_some(), governor)?;
        let mut live = core::mem::replace(&mut self.out.live, ScratchVec::new(governor));
        merge(&mut live, &mut self.arena, governor)?;
        let mut completed = core::mem::replace(&mut self.out.completed, ScratchVec::new(governor));
        let mut failed = core::mem::replace(&mut self.out.failed, ScratchVec::new(governor));
        self.stall(table, &mut failed, governor)?;
        // A literal that failed while a sibling configuration of its
        // attempt, begun at the same place or earlier, lived on (the
        // canonical `<|tool_call_begin|>` beside a variant spelling of a
        // section closer that shares its first bytes) is still a repair
        // seed, by respelling only, when the attempt later runs out of
        // configurations. The table keeps such failures between steps.
        self.commit(
            table,
            live.as_slice()
                .iter()
                .chain(completed.as_slice())
                .chain(failed.as_slice().iter().map(|failure| &failure.thread)),
            governor,
        )?;
        let opened = self.open(table, governor)?;
        let keep = self.last_written(table, opened, governor)?;
        Self::stop_past(&mut live, opened, &keep, governor)?;
        // Repairs for attempts left with nothing.
        let _records = governor.lease_scratch(
            size(self.tables[table].attempts.len())
                .saturating_mul(size(<Attempt as ScratchCharge>::CHARGE)),
        )?;
        let records: Vec<Attempt> = self.tables[table].attempts.as_slice().to_vec();
        for record in &records {
            // A configuration resynchronizing past an earlier failure, or
            // reading beside one that ended at a delimiter, does not keep
            // the attempt from being stuck: its repairs are proposed as if
            // it were not there.
            let touches = |threads: &ScratchVec<Thread>| {
                threads
                    .as_slice()
                    .iter()
                    .any(|thread| thread.attempt == record.id && !thread.speculative())
            };
            if touches(&live) || touches(&completed) || record.completed {
                continue;
            }
            let mine = |failure: &&Failure| failure.thread.attempt == record.id;
            // The failures held from earlier steps (none at the end of
            // input), one `Work` unit each.
            let held_count = if eof.is_none() {
                self.tables[table].failures.count(record.id)
            } else {
                0
            };
            governor.charge(Resource::Work, size(held_count))?;
            let _failures = governor.lease_scratch(
                size(failed.as_slice().iter().filter(mine).count() + held_count)
                    .saturating_mul(size(<Failure as ScratchCharge>::CHARGE)),
            )?;
            let held = self.tables[table]
                .failures
                .of(record.id)
                .filter(|_| eof.is_none())
                .map(|failure| Failure {
                    respell: true,
                    ..*failure
                });
            let failures: Vec<Failure> = failed
                .as_slice()
                .iter()
                .filter(mine)
                .copied()
                .chain(held)
                .collect();
            self.note_failure(table, record.id, &failures, eof.is_none());
            self.note_boundary(table, record.id, &failures, governor)?;
            let until = if eof.is_some() { at } else { at + 1 };
            for state in self.repair(table, record, &failures, until, eof, governor)? {
                match state.status {
                    Status::Live => live.push(state.thread)?,
                    Status::Completed => completed.push(state.thread)?,
                    Status::Failed | Status::Respelled | Status::Stuck => {}
                }
            }
            // The attempt also resynchronizes past the failing call; a
            // repair that reads it costs less and wins where it completes.
            if eof.is_none() {
                self.resync(table, record, until, &mut live, governor)?;
            }
        }
        self.hold_failures(table, &failed, &live, governor)?;
        self.merge_settled(table, at, eof, &mut live, &mut completed, governor)?;
        self.unstick(table, &live, governor)?;
        // A repair may carry an attempt past its commit, and a completion
        // always commits its attempt.
        if self.tables[table].program.deferred() {
            self.commit(
                table,
                live.as_slice().iter().chain(completed.as_slice()),
                governor,
            )?;
            let opened = self.open(table, governor)?;
            let keep = self.last_written(table, opened, governor)?;
            Self::stop_past(&mut live, opened, &keep, governor)?;
        }
        self.complete(table, &mut completed, eof, governor)?;
        self.retire(table, &live, at, eof, governor)?;
        self.tables[table].threads = live;
        Ok(())
    }

    /// Merge `live`, then repair the stalled failures its merge shows are
    /// final (see [`Self::repair_stalled`]), merging again if any were.
    fn merge_settled(
        &mut self,
        table: usize,
        at: usize,
        eof: Option<&GenerationStop>,
        live: &mut ScratchVec<Thread>,
        completed: &mut ScratchVec<Thread>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        // The attempts whose body reads on past closers, noted in the merge.
        let mut reading: Vec<u64> = Vec::new();
        merge_noting(live, &mut self.arena, governor, |thread| {
            if matches!(thread.sub, Sub::Body(state, _) if state & PAST_REPAIRED != 0) {
                reading.push(thread.attempt);
            }
        })?;
        if self.repair_stalled(table, at, eof, &reading, live, completed, governor)? {
            merge(live, &mut self.arena, governor)?;
        }
        Ok(())
    }

    /// Move the failures of readings beside another of a verbatim value
    /// out of `failed` into [`Table::stalled`], keeping each attempt's
    /// newest, for a program without `next` (a grouped program repairs them
    /// as they come). One `Work` unit per failure.
    fn stall(
        &mut self,
        table: usize,
        failed: &mut ScratchVec<Failure>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let (program, kind) = (self.tables[table].program, self.tables[table].kind);
        if program.grouped
            || !failed
                .as_slice()
                .iter()
                .any(|failure| failure.thread.beside())
        {
            return Ok(());
        }
        governor.charge(
            Resource::Work,
            size(failed.len() + self.tables[table].stalled.len()),
        )?;
        let slice = failed.as_mut_slice();
        let mut write = 0;
        for read in 0..slice.len() {
            let failure = slice[read];
            // A verbatim value's reading beside another (a JSON value's
            // failure is noted as written, see `Step::end_body`).
            let waits = failure.thread.past
                || (failure.thread.beside() && verbatim_slot(program, kind, &failure.thread));
            if !waits {
                slice[write] = failure;
                write += 1;
                continue;
            }
            let stalled = &mut self.tables[table].stalled;
            match stalled
                .as_slice()
                .iter()
                .position(|held| held.thread.attempt == failure.thread.attempt)
            {
                Some(index) => {
                    if stalled.as_slice()[index].at <= failure.at {
                        stalled.as_mut_slice()[index] = failure;
                    }
                }
                None => stalled.push(failure)?,
            }
        }
        while failed.len() > write {
            failed.pop();
        }
        Ok(())
    }

    /// Repair the stalled failure (see [`Table::stalled`]) of every attempt
    /// of `table` whose body reading on past closers is gone (stopped at
    /// another call's opener or a reasoning marker, failed, or ended with
    /// the output): its last reading beside another is final. It is
    /// repaired as the mid-output failure it was; at the end of output the
    /// repaired configurations then take their end-of-input transitions.
    #[allow(clippy::too_many_arguments)]
    fn repair_stalled(
        &mut self,
        table: usize,
        at: usize,
        eof: Option<&GenerationStop>,
        reading: &[u64],
        live: &mut ScratchVec<Thread>,
        completed: &mut ScratchVec<Thread>,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        if self.tables[table].stalled.is_empty() {
            return Ok(false);
        }
        // One unit per stalled failure examined: `reading` was noted in the
        // merge pass, which charged its configurations.
        governor.charge(Resource::Work, size(self.tables[table].stalled.len()))?;
        let mut repaired = false;
        let mut index = 0;
        while index < self.tables[table].stalled.len() {
            let failure = self.tables[table].stalled.as_slice()[index];
            if eof.is_none() && reading.contains(&failure.thread.attempt) {
                index += 1;
                continue;
            }
            let stalled = self.tables[table].stalled.as_mut_slice();
            stalled.copy_within(index + 1.., index);
            self.tables[table].stalled.pop();
            // A reading that already completed at or past the failing
            // configuration (beside another, or as written) is the one the
            // repair would make, or holds less: the failing one holds only
            // closer text more, which needs a repair besides (no repair is
            // proposed for an attempt that completed).
            let attempt = failure.thread.attempt;
            let entry = failure.thread.entry;
            if self.tables[table]
                .past
                .iter()
                .any(|held| held.attempt == attempt && held.pos >= entry)
                || self.tables[table]
                    .written
                    .iter()
                    .any(|&(owner, end, _)| owner == attempt && end >= entry)
            {
                continue;
            }
            repaired = true;
            let Some(record) = self.tables[table]
                .attempts
                .as_slice()
                .iter()
                .find(|record| record.id == failure.thread.attempt)
                .copied()
            else {
                continue;
            };
            let until = if eof.is_some() { at } else { at + 1 };
            // A failure at the end of output is repaired as one; any other
            // as the mid-output failure it was.
            let at_end = eof.filter(|_| failure.at >= self.fed);
            let mut ending = ScratchVec::new(governor);
            for state in self.repair(table, &record, &[failure], until, at_end, governor)? {
                match state.status {
                    Status::Live if eof.is_some() => ending.push(state.thread)?,
                    Status::Live => live.push(state.thread)?,
                    Status::Completed => completed.push(state.thread)?,
                    Status::Failed | Status::Respelled | Status::Stuck => {}
                }
            }
            if !ending.is_empty() {
                // The output has ended: they take their end-of-input
                // transitions now.
                self.out.clear();
                let (mut step, _, out) = self.split(table);
                step.advance(&mut ending, at, true, out, governor)?;
                for thread in self.out.completed.as_slice() {
                    completed.push(*thread)?;
                }
                self.out.clear();
            }
        }
        Ok(repaired)
    }

    /// Clear [`Attempt::stuck`] for every attempt of `table` a repair or a
    /// resynchronization carries on (a configuration of `live` read as
    /// written): it is no longer kept live only beside a reading that ended
    /// at a delimiter. One `Work` unit per configuration examined.
    fn unstick(
        &mut self,
        table: usize,
        live: &ScratchVec<Thread>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let attempts = self.tables[table].attempts.as_mut_slice();
        if !attempts.iter().any(|record| record.stuck) {
            return Ok(());
        }
        governor.charge(Resource::Work, size(live.len()))?;
        for record in attempts {
            if record.stuck
                && live
                    .as_slice()
                    .iter()
                    .any(|thread| thread.attempt == record.id && !thread.beside())
            {
                record.stuck = false;
            }
        }
        Ok(())
    }

    /// Keep the literal failures of `table` a still-live sibling
    /// configuration of the same attempt began beside (at the failed
    /// literal's start or earlier), when the literal has a respelling: the
    /// held ones (`earlier`) compacted in place, then this step's
    /// (`failed`). They seed respelling repairs if the attempt later runs
    /// out of configurations (see `settle`).
    fn hold_failures(
        &mut self,
        table: usize,
        failed: &ScratchVec<Failure>,
        live: &ScratchVec<Thread>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if self.tables[table].failures.groups.is_empty() && failed.is_empty() {
            return Ok(());
        }
        let grammar = self.grammars[self.tables[table].grammar].grammar;
        let program = self.tables[table].program;
        // The earliest `entry` of a non-speculative configuration of each
        // attempt, by attempt. `live` and `failed` hold configurations this
        // step advanced, each charged `K_THREAD` where it was stepped, so
        // passing over them adds no unit; the held failures, which this
        // step did not advance, are charged in [`HeldFailures::retain`].
        let _floors = governor.lease_scratch(
            size(live.len()).saturating_mul(size(<(u64, usize) as ScratchCharge>::CHARGE)),
        )?;
        let mut floors: Vec<(u64, usize)> = live
            .as_slice()
            .iter()
            .filter(|thread| !thread.speculative())
            .map(|thread| (thread.attempt, thread.entry))
            .collect();
        floors.sort_unstable();
        floors.dedup_by_key(|&mut (attempt, _)| attempt);
        // Only a literal with a respelling (or, in a `json-envelope`
        // grammar, a JSON separator insertion or deletion) can be repaired
        // later.
        let candidate = |failure: &Failure| {
            let respellable = match program.insts[failure.thread.pc as usize] {
                Inst::Lit { lit, .. } => {
                    let tables = &grammar.literal_tables[lit as usize];
                    let bytes = &grammar.literals[lit as usize].bytes;
                    !tables.variants.trie.is_empty()
                        || !tables.confusables.trie.is_empty()
                        || tables.json.iter().any(|json| !json.trie.is_empty())
                        || (grammar.json_envelope
                            && (matches!(bytes.first(), Some(b'}' | b']'))
                                || bytes.as_slice() == b","
                                || bytes.as_slice() == b":"))
                }
                _ => false,
            };
            // A JSON closer that met a `,` (a trailing comma, or the
            // separator before an optional member a sibling took) is held
            // while any configuration of its attempt lives: the sibling
            // has moved past the comma by the time the attempt runs out.
            // The byte stays in the window while its attempt lives.
            let trailing = grammar.json_envelope
                && matches!(failure.thread.sub, Sub::Lit(0))
                && matches!(
                    program.insts[failure.thread.pc as usize],
                    Inst::Lit { lit, .. }
                        if matches!(grammar.literals[lit as usize].bytes.first(), Some(b'}' | b']'))
                )
                && failure
                    .at
                    .checked_sub(self.base)
                    .and_then(|at| self.buffer.as_slice().get(at))
                    == Some(&b',');
            (respellable && matches!(failure.thread.sub, Sub::Lit(_)))
                .then_some((*failure, trailing))
        };
        let _fresh = governor.lease_scratch(
            size(failed.len()).saturating_mul(size(<Failure as ScratchCharge>::CHARGE) + 8),
        )?;
        let fresh: Vec<(Failure, bool)> = failed.as_slice().iter().filter_map(candidate).collect();
        self.tables[table]
            .failures
            .retain(&floors, &fresh, governor)
    }

    /// Queue the opening of every attempt of a deferred program (one with
    /// `commit` instructions) that one of `threads` shows committed: a
    /// configuration at a location only a committed attempt reaches, or a
    /// completion. The attempt is reported opened one byte after its start,
    /// so it orders before every report inside it, which the turn level
    /// holds back while the attempt is uncommitted ([`Self::held_floor`]).
    fn commit<'t>(
        &mut self,
        table: usize,
        threads: impl Iterator<Item = &'t Thread>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let program = self.tables[table].program;
        if !program.deferred() {
            return Ok(());
        }
        for thread in threads {
            governor.charge(Resource::Work, K_THREAD)?;
            let accepted = matches!(program.insts[thread.pc as usize], Inst::Accept);
            if (accepted || program.commits(thread.pc))
                && self.tables[table]
                    .attempts
                    .as_slice()
                    .iter()
                    .any(|record| record.id == thread.attempt && !record.opened)
            {
                self.out.opened.push((thread.attempt, thread.start + 1))?;
            }
        }
        Ok(())
    }

    /// Report every attempt that finished its opener in this step.
    fn open(&mut self, table: usize, governor: &mut Governor) -> Result<Option<usize>, StopReason> {
        let _opened = governor.lease_scratch(
            size(self.out.opened.len())
                .saturating_mul(size(<(u64, usize) as ScratchCharge>::CHARGE)),
        )?;
        let mut opened: Vec<(u64, usize)> = self.out.opened.as_slice().to_vec();
        opened.sort_unstable();
        let mut latest = None;
        for (attempt, position) in opened {
            let attempts = self.tables[table].attempts.as_mut_slice();
            if let Some(record) = attempts.iter_mut().find(|record| record.id == attempt)
                && !record.opened
            {
                record.opened = true;
                self.newest_open = self.newest_open.max(record.start);
                latest = latest.max(Some(record.start));
                self.events.push(Event::Opened {
                    attempt,
                    start: record.start,
                    at: position,
                });
            }
        }
        Ok(latest)
    }

    /// The attempts of `table` whose completion as written ends last (the
    /// call just before an opener at `opened`; none without one). `Work`
    /// per completion examined.
    fn last_written(
        &self,
        table: usize,
        opened: Option<usize>,
        governor: &mut Governor,
    ) -> Result<Vec<u64>, StopReason> {
        if opened.is_none() {
            return Ok(Vec::new());
        }
        let written = &self.tables[table].written;
        governor.charge(Resource::Work, size(written.len()).saturating_mul(2))?;
        let latest = written.iter().map(|&(_, end, _)| end).max();
        let mut keep: Vec<u64> = written
            .iter()
            .filter(|&&(_, end, _)| Some(end) == latest)
            .map(|&(attempt, _, _)| attempt)
            .collect();
        // The earliest attempt read as written only with a repair: its
        // value may hold every call after it (a value documenting the
        // format with several calls), the clean reading.
        let repaired = written
            .iter()
            .map(|&(attempt, _, _)| attempt)
            .filter(|&attempt| {
                written
                    .iter()
                    .filter(|&&(owner, _, _)| owner == attempt)
                    .all(|&(_, _, cost)| cost > 0)
            })
            .min();
        if let Some(attempt) = repaired
            && !keep.contains(&attempt)
        {
            keep.push(attempt);
        }
        Ok(keep)
    }

    /// Drop every body of `live` that read on past a closer (see
    /// [`PAST_REPAIRED`]) of an attempt begun before `start`, where another
    /// attempt of the table opened, but those of the attempts in `keep`,
    /// whose completions as written end last (the call just before the
    /// opener, see `Self::last_written`): such a value may hold its own
    /// closers and the call that opens here, whole (see `Step::opens_call`),
    /// competing with the two calls read apart. Any other value never runs
    /// on into a later call, so each value reads on over at most the one
    /// call after it. One `Work` unit per configuration examined.
    fn stop_past(
        live: &mut ScratchVec<Thread>,
        start: Option<usize>,
        keep: &[u64],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Some(start) = start else {
            return Ok(());
        };
        governor.charge(Resource::Work, size(live.len()))?;
        let slice = live.as_mut_slice();
        let mut write = 0;
        for read in 0..slice.len() {
            let thread = slice[read];
            let past = matches!(thread.sub, Sub::Body(state, _) if state & PAST_REPAIRED != 0);
            if past && thread.start < start && !keep.contains(&thread.attempt) {
                continue;
            }
            slice[write] = thread;
            write += 1;
        }
        while live.len() > write {
            live.pop();
        }
        Ok(())
    }

    /// Record why (and where) an attempt left with no configuration failed.
    fn note_failure(&mut self, table: usize, attempt: u64, failures: &[Failure], midway: bool) {
        // Where the attempt failed is where its own configurations ran out,
        // not where an earlier sibling's literal did (a respelling seed).
        let current = || failures.iter().filter(|failure| !failure.respell);
        let Some(first) = current().min_by_key(|failure| (failure.at, failure.reason)) else {
            return;
        };
        // The rejected span ends where the furthest failing token began: a
        // partial literal may start the next opener.
        let program = self.tables[table].program;
        let end = current()
            .map(|failure| match program.insts[failure.thread.pc as usize] {
                Inst::Lit { .. } => failure.thread.entry,
                _ => failure.at,
            })
            .max()
            .unwrap_or(first.at);
        let record = self.tables[table]
            .attempts
            .as_mut_slice()
            .iter_mut()
            .find(|other| other.id == attempt)
            .expect("attempt is live");
        if record.failure_end == FailureEnd::Resynced {
            if record.failure.is_some() {
                return;
            }
        } else {
            // A slot configuration that failed before its own position
            // failed in its value (`Step::end_body`), not at a token it was
            // reading.
            record.failure_end =
                if matches!(program.insts[first.thread.pc as usize], Inst::Slot { .. })
                    && first.at < first.thread.pos
                {
                    FailureEnd::Failure
                } else {
                    FailureEnd::Stop
                };
        }
        record.failure = Some(first.reason);
        record.end = Some(end);
        record.stuck = midway;
    }

    /// For a grouped program, record where the failing configurations of
    /// stuck `attempt` last began a call (a `next` they passed): the
    /// furthest such boundary, the least cost there, and the history before
    /// it. The calls before it were read whole; should the attempt end
    /// rejected, they are recovered and the rejection begins there. One
    /// `Work` unit per history node visited.
    fn note_boundary(
        &mut self,
        table: usize,
        attempt: u64,
        failures: &[Failure],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if !self.tables[table].program.grouped {
            return Ok(());
        }
        // A first call is only a call of an attempt that read its opener.
        let opened = self.tables[table]
            .attempts
            .as_slice()
            .iter()
            .any(|record| record.id == attempt && record.opened);
        let mut best: Option<Boundary> = None;
        for failure in failures.iter().filter(|failure| !failure.respell) {
            let mut node = failure.thread.hist;
            loop {
                governor.charge(Resource::Work, 1)?;
                let HistNode { prev, kind } = self.arena.nodes.as_slice()[node as usize];
                let boundary = match kind {
                    HistKind::Next(at, cost) => Some((at, prev, cost)),
                    // The attempt's first call: it began at the attempt's
                    // start, and the failing configuration's history and
                    // cost stand for what was read before it.
                    HistKind::Root if opened => Some((
                        failure.thread.start,
                        failure.thread.hist,
                        failure.thread.cost,
                    )),
                    HistKind::Root => break,
                    _ => None,
                };
                match boundary {
                    None => node = prev,
                    Some((at, hist, cost)) => {
                        let candidate = Boundary {
                            attempt,
                            at,
                            hist,
                            cost,
                            pc: failure.thread.pc,
                            failed: failure.at,
                            reason: failure.reason,
                            entry: failure.thread.entry,
                        };
                        let better = best.is_none_or(|known| {
                            (Reverse(candidate.at), candidate.cost, candidate.hist)
                                < (Reverse(known.at), known.cost, known.hist)
                        });
                        if better {
                            best = Some(candidate);
                        }
                        break;
                    }
                }
            }
        }
        // A later stuck step whose failures passed no boundary (a repair
        // carried the attempt on) keeps the one recorded before.
        let Some(best) = best else {
            return Ok(());
        };
        let boundaries = &mut self.tables[table].boundaries;
        if let Some(known) = boundaries
            .as_mut_slice()
            .iter_mut()
            .find(|known| known.attempt == attempt)
        {
            let first = failures
                .iter()
                .any(|failure| !failure.respell && failure.thread.start == best.at);
            if !first {
                *known = best;
            } else if best.at == known.at {
                // The first call failed again (a repair carried it on): what
                // was read before it stands; where it failed moves on.
                known.pc = best.pc;
                known.failed = best.failed;
                known.reason = best.reason;
                known.entry = best.entry;
            }
        } else {
            boundaries.push(best)?;
        }
        Ok(())
    }

    /// Resynchronize stuck `record`, an attempt of a grouped program, past
    /// the call it could not read (its recorded [`Boundary`]: the `next`
    /// the call began at, or the attempt's start for its first call): a
    /// configuration per target literal (see `resync_targets`: a call
    /// closer, or the next call's opener) scans on from `until` for that
    /// literal (a single-byte one at the call's bracket level outside JSON
    /// strings, a multi-byte one wherever it is written), then reads on
    /// from it, so later calls of the attempt are read; the skipped call
    /// becomes a rejected part ([`HistKind::Skipped`]) costing
    /// [`REJECT`](super::cost::REJECT), and the split
    /// (`tool_call.markup.group-split`) is recorded on the last call before
    /// it, or for a first call on the first call after it (see
    /// `lead_split`). It gives up at another attempt's opener, and the
    /// attempt then ends as before.
    fn resync(
        &mut self,
        table: usize,
        record: &Attempt,
        until: usize,
        live: &mut ScratchVec<Thread>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let program = self.tables[table].program;
        if program.call_closers.is_empty() {
            return Ok(());
        }
        let Some(boundary) = self.tables[table]
            .boundaries
            .as_slice()
            .iter()
            .find(|known| known.attempt == record.id)
            .copied()
        else {
            return Ok(());
        };
        // Once per failure: a resynchronization already under way from it
        // (or already ended at its closer) is not started again.
        if boundary.at < self.base
            || boundary.at >= until
            || live.as_slice().iter().any(|thread| {
                thread.attempt == record.id
                    && thread.sub.resyncing()
                    && thread.entry == boundary.failed
            })
            || self
                .spawned
                .iter()
                .any(|&(attempt, failed)| attempt == record.id && failed == boundary.failed)
        {
            return Ok(());
        }
        governor.charge(Resource::ScratchBytes, 24)?;
        self.spawned.push((record.id, boundary.failed));
        let grammar = self.grammars[self.tables[table].grammar].grammar;
        let targets = resync_targets(program, grammar, boundary.pc, governor)?;
        if targets.is_empty() {
            return Ok(());
        }
        let first = boundary.at == record.start;
        let Some(state) = self.resync_state(
            &boundary, first, &targets, grammar, program, until, governor,
        )?
        else {
            return Ok(());
        };
        if let Some(stuck) = self.tables[table]
            .attempts
            .as_mut_slice()
            .iter_mut()
            .find(|other| other.id == record.id)
        {
            stuck.failure_end = FailureEnd::Resynced;
        }
        // The calls before the failing one were read whole: the last of them
        // records the split (`tool_call.markup.group-split`), as when the
        // attempt ends there. A failing first call has none before it: the
        // first call read after it records the split (see
        // `Step::segment`).
        let split = EditOperator::GroupSplit.rule().cost;
        let hist = if first {
            self.arena
                .push(boundary.hist, HistKind::Skipped(boundary.failed))?
        } else {
            let edited = self.arena.edit(
                boundary.hist,
                MarkupEdit {
                    operator: EditOperator::GroupSplit,
                    span: boundary.at..boundary.at,
                    original: Vec::new(),
                    replacement: Vec::new(),
                },
                governor,
            )?;
            let next = self.arena.push(
                edited,
                HistKind::Next(boundary.at, boundary.cost.saturating_add(split)),
            )?;
            self.arena.push(next, HistKind::Skipped(boundary.failed))?
        };
        let cost = boundary
            .cost
            .saturating_add(split)
            .saturating_add(super::cost::REJECT);
        for pc in targets {
            live.push(Thread {
                pc,
                sub: Sub::Resync(state, 0),
                attempt: record.id,
                start: record.start,
                entry: boundary.failed,
                pos: until,
                cost,
                hist,
                opened: true,
                stall: 0,
                reach: until,
                past: false,
                pending: 0,
            })?;
        }
        Ok(())
    }

    /// The scanner state a resynchronization of the call at `boundary`
    /// starts from at `until` (see [`Self::resync`]), or `None` when a
    /// first call opened no bracket its closer could close.
    #[allow(clippy::too_many_arguments)]
    fn resync_state(
        &self,
        boundary: &Boundary,
        first: bool,
        targets: &[u32],
        grammar: &CompiledGrammar,
        program: &Program,
        until: usize,
        governor: &mut Governor,
    ) -> Result<Option<u32>, StopReason> {
        // The scanner state after the call's bytes read so far, its depth
        // counted from the call's own level. A later call begins at its
        // boundary; the first begins inside the attempt's opener, at the
        // depth where the failing terminal began, less the bracket a
        // bracket-closed call (a JSON element) opened.
        governor.charge(Resource::Work, size(until - boundary.at))?;
        let mut scan = JsonScan::default();
        let mut level = 0;
        for (offset, &byte) in self.buffer.as_slice()[boundary.at - self.base..until - self.base]
            .iter()
            .enumerate()
        {
            if first && boundary.at + offset == boundary.entry {
                level = scan.depth;
            }
            scan.step(byte);
        }
        if first {
            let bracketed = targets.iter().any(|&pc| {
                let Inst::Lit { lit, .. } = program.insts[pc as usize] else {
                    return false;
                };
                matches!(grammar.literals[lit as usize].bytes.as_slice(), b"}" | b"]")
            });
            if bracketed {
                if level == 0 {
                    return Ok(None);
                }
                level -= 1;
            }
            scan.depth = scan.depth.saturating_sub(level);
        }
        Ok(Some(scan.pack()))
    }

    /// Drop the resynchronization records of `attempt`, which ended.
    fn forget_resync(&mut self, attempt: u64, governor: &mut Governor) -> Result<(), StopReason> {
        let before = self.resynced.len() + self.spawned.len();
        self.resynced.retain(|&(owner, _, _)| owner != attempt);
        self.spawned.retain(|&(owner, _)| owner != attempt);
        let quoted = self.quoted_openers.len();
        self.quoted_openers.retain(|&(owner, _)| owner != attempt);
        governor.release_scratch(
            size(before - self.resynced.len() - self.spawned.len())
                .saturating_mul(24)
                .saturating_add(size(quoted - self.quoted_openers.len()).saturating_mul(16)),
        )
    }

    /// Take the boundary recorded for `attempt`, if any.
    fn take_boundary(&mut self, table: usize, attempt: u64) -> Option<Boundary> {
        let boundaries = &mut self.tables[table].boundaries;
        let index = boundaries
            .as_slice()
            .iter()
            .position(|known| known.attempt == attempt)?;
        let found = boundaries.as_slice()[index];
        let last = boundaries.len() - 1;
        boundaries.as_mut_slice().swap(index, last);
        boundaries.pop();
        Some(found)
    }

    /// The calls rejected `record` read whole before the boundary recorded
    /// for it, when the rejection (ending at `end`) lies past it: each
    /// segmentation of the span up to the boundary, its last call carrying
    /// a `tool_call.markup.group-split` edit at the boundary.
    fn salvage(
        &mut self,
        table: usize,
        record: &Attempt,
        end: usize,
        governor: &mut Governor,
    ) -> Result<Option<(Salvaged, Rejection)>, StopReason> {
        let Some(boundary) = self.take_boundary(table, record.id) else {
            return Ok(None);
        };
        if record.completed || boundary.at <= record.start || boundary.at > end {
            return Ok(None);
        }
        let kind = self.tables[table].kind;
        let dialect = self.grammars[self.tables[table].grammar].dialect.clone();
        let (step, _, _) = self.split(table);
        let shape = Shape {
            span: record.start..boundary.at,
            dialect: &dialect,
            kind,
            dangling: false,
            open_end: boundary.at,
            cost: boundary.cost,
        };
        let before = governor.scratch_meter().total_charged();
        let Ok(mut segmentations) = step.segment(boundary.hist, &shape, governor)? else {
            return Ok(None);
        };
        let rule = EditOperator::GroupSplit.rule();
        for parts in &mut segmentations {
            if let Some((Part::Call(segment), cost)) = parts.last_mut() {
                *cost = cost.saturating_add(rule.cost);
                for reading in &mut segment.readings {
                    reading.edits.push(MarkupEdit {
                        operator: EditOperator::GroupSplit,
                        span: boundary.at..boundary.at,
                        original: Vec::new(),
                        replacement: Vec::new(),
                    });
                    reading.cost = reading.cost.saturating_add(rule.cost);
                    if reading.closure == Closure::Closed {
                        reading.closure = Closure::Repaired;
                    }
                }
            }
        }
        let charged = governor
            .scratch_meter()
            .total_charged()
            .saturating_sub(before);
        Ok(Some((
            Salvaged {
                at: boundary.at,
                charged: if segmentations.len() == 1 { charged } else { 0 },
                segmentations,
            },
            boundary.reason,
        )))
    }

    /// Retire the attempts `live` no longer holds a configuration of.
    fn retire(
        &mut self,
        table: usize,
        live: &ScratchVec<Thread>,
        at: usize,
        eof: Option<&GenerationStop>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut index = 0;
        let mut retired = false;
        while index < self.tables[table].attempts.len() {
            let record = self.tables[table].attempts.as_slice()[index];
            if live
                .as_slice()
                .iter()
                .any(|thread| thread.attempt == record.id)
                || self.tables[table]
                    .trails
                    .iter()
                    .any(|trail| trail.thread.attempt == record.id)
            {
                index += 1;
                continue;
            }
            // Its reading beside another, held until now, may complete it.
            self.release_past(table, record.id, governor)?;
            self.forget_written(table, record.id, governor)?;
            let record = self.tables[table].attempts.as_slice()[index];
            self.release(table, record.id, governor)?;
            // How far the attempt read the calls it skipped, when it skipped
            // more than one: an attempt that ends rejected whole read their
            // markup too (one skipped call's markup is its own rejection's,
            // run on to its closer as any rejected attempt's is).
            let skips = self
                .resynced
                .iter()
                .filter(|&&(owner, _, _)| owner == record.id);
            let skipped = if skips.clone().count() > 1 {
                skips.map(|&(_, _, read)| read).max()
            } else {
                None
            };
            self.forget_resync(record.id, governor)?;
            if record.opened {
                // A rejected span ends before the token that could not be
                // consumed; a completed attempt's last report comes after
                // all of its completions.
                // At the end of output a value no repair reads ends its
                // span where it stopped decoding, as mid-input.
                let malformed = record.failure_end.at_failure() || record.stuck;
                let end = match (eof, record.completed) {
                    (Some(_), false) if malformed => record.end.unwrap_or(at),
                    (Some(_), _) => at,
                    (None, true) => at + 1,
                    (None, false) => record.end.unwrap_or(at),
                };
                let end = match skipped {
                    Some(read) if !record.completed => end.max(read),
                    _ => end,
                };
                let salvaged = self.salvage(table, &record, end, governor)?;
                let tail_reason = salvaged.as_ref().map(|(_, reason)| *reason);
                let salvage = salvaged.map(|(parts, _)| parts);
                let whole = record.completed || salvage.is_some();
                let ended = Event::Ended {
                    table: index_u32(table),
                    attempt: record.id,
                    start: record.start,
                    at: end,
                    completed: record.completed,
                    reason: tail_reason.unwrap_or_else(|| match (eof, record.failure) {
                        (
                            _,
                            Some(
                                reason @ (Rejection::InvalidUtf8 { .. }
                                | Rejection::PythonicPositionalRejected { .. }),
                            ),
                        )
                        | (None, Some(reason)) => reason,
                        (Some(_), Some(reason @ Rejection::Malformed { .. }))
                            if !record.completed && record.failure_end.at_failure() =>
                        {
                            reason
                        }
                        (Some(_), Some(reason)) if !record.completed && record.stuck => reason,
                        (Some(_), _) => Rejection::Unterminated,
                        (None, None) => Rejection::Overlapped { at },
                    }),
                    salvage,
                    outread: false,
                };
                self.defer(table, &record, whole, ended, governor)?;
            } else {
                self.take_boundary(table, record.id);
            }
            let attempts = self.tables[table].attempts.as_mut_slice();
            attempts.copy_within(index + 1.., index);
            self.tables[table].attempts.pop();
            retired = true;
        }
        // Only an attempt that ended can release a held report.
        if retired {
            self.release_deferred(governor)?;
        }
        Ok(())
    }

    /// Report the end of `record` (of `table`), `whole` when it read a whole
    /// call (completed, or recovered calls before the one it failed in). A
    /// rejected attempt with no whole call, while another table's attempt
    /// that began at the same opener still reads on (it has completed no
    /// call yet: one that has ends whole, see `Self::complete`), is held: when every
    /// attempt at that opener has ended, it is reported, `outread` when one
    /// of them read a whole call. The turn layer then records it but does
    /// not offer it as a reading of the opener: the bytes after the opener
    /// are the other dialect's markup (body evidence), and a reading that
    /// rejected the opener here and read those bytes as other calls would
    /// rest on strictly less grammar evidence. Held reports hold the turn
    /// layer ([`Self::held_floor`]).
    fn defer(
        &mut self,
        table: usize,
        record: &Attempt,
        whole: bool,
        ended: Event,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if whole && !self.whole.contains(&record.start) {
            governor.charge(Resource::ScratchBytes, DEFERRED_BYTES)?;
            self.whole.push(record.start);
        }
        let reads_on =
            !whole
                && self.tables.iter().enumerate().any(|(other, entry)| {
                    other != table
                        && entry.attempts.as_slice().iter().any(|live| {
                            live.opened && !live.completed && live.start == record.start
                        })
                });
        if reads_on {
            governor.charge(Resource::ScratchBytes, DEFERRED_BYTES)?;
            self.deferred.push((record.start, ended));
        } else {
            // Another attempt at the opener may have read a whole call and
            // ended first (this one read on, resynchronizing): the same
            // body evidence stands.
            let mut ended = ended;
            if let Event::Ended { outread, .. } = &mut ended {
                *outread = !whole && self.whole.contains(&record.start);
            }
            self.events.push(ended);
        }
        Ok(())
    }

    /// Report every held end (see [`Self::defer`]) whose opener no attempt
    /// reads on from any more.
    fn release_deferred(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        if self.deferred.is_empty() && self.whole.is_empty() {
            return Ok(());
        }
        governor.charge(
            Resource::Work,
            size(self.deferred.len() + self.whole.len()).saturating_mul(size(self.tables.len())),
        )?;
        let reading = |tables: &[Table<'_>], start: usize| {
            tables.iter().any(|entry| {
                entry
                    .attempts
                    .as_slice()
                    .iter()
                    .any(|live| live.opened && !live.completed && live.start == start)
            })
        };
        let mut index = 0;
        while index < self.deferred.len() {
            let start = self.deferred[index].0;
            if reading(&self.tables, start) {
                index += 1;
                continue;
            }
            let (_, mut ended) = self.deferred.remove(index);
            if let Event::Ended { outread, .. } = &mut ended {
                *outread = self.whole.contains(&start);
            }
            self.events.push(ended);
            governor.release_scratch(DEFERRED_BYTES)?;
        }
        let before = self.whole.len();
        let (tables, deferred) = (&self.tables, &self.deferred);
        self.whole.retain(|&start| {
            reading(tables, start) || deferred.iter().any(|(held, _)| *held == start)
        });
        governor.release_scratch(size(before - self.whole.len()).saturating_mul(DEFERRED_BYTES))
    }

    /// Report the held completions of `attempt`, a grouped program's, that
    /// no other completion of it dominates. A completion is dominated by
    /// one that ends later and reads more calls, its first calls costing no
    /// more than the dominated completion's calls (it reads the same calls
    /// no worse, and further ones: the attempt reads every call it can, so
    /// a call-shaped object after a call is never left as content, whatever
    /// repair reading it needs), or by the cut-off reading of a length stop
    /// that ends later, reads more calls and began its last call's
    /// arguments (the attempt was still being written past it). A cut-off
    /// reading is dominated by a completion that reaches as far, or reads as
    /// many calls no worse, or by any completion when its last call never
    /// began its arguments. Charges one `Work` unit per pair compared.
    fn release(
        &mut self,
        table: usize,
        attempt: u64,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mine = self.tables[table].held.take(attempt);
        if mine.is_empty() {
            return Ok(());
        }
        let count = mine.len();
        let _vectors = governor.lease_scratch(size(count).saturating_mul(RELEASE_VECTOR_BYTES))?;
        // Latest end first: the completion that dominates the rest is almost
        // always the one that read the most.
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by_key(|&index| Reverse((mine[index].at, mine[index].calls)));
        governor.charge(
            Resource::Work,
            size(count).saturating_mul(size(usize::BITS as usize - count.leading_zeros() as usize)),
        )?;
        // The cost of the first calls of a completion, built when asked.
        let mut tables: Vec<Option<Vec<u64>>> = vec![None; count];
        let mut table_bytes = 0_u64;
        let mut keep = vec![true; count];
        for (index, entry) in mine.iter().enumerate() {
            for &other in &order {
                if other == index {
                    continue;
                }
                governor.charge(Resource::Work, 1)?;
                let other_entry = &mine[other];
                let later = other_entry.at > entry.at;
                let more = other_entry.calls > entry.calls;
                let dominated = if other_entry.dangling {
                    // Cut off while writing a further call's arguments.
                    later && more && other_entry.arguments
                } else if entry.dangling {
                    // A cut-off reading of no more calls than a completion
                    // reads, or one whose last call never began its
                    // arguments, or one that reaches no further.
                    !entry.arguments
                        || other_entry.at >= entry.at
                        || (other_entry.calls >= entry.calls
                            && self.prefix(
                                other_entry,
                                &mut tables[other],
                                &mut table_bytes,
                                entry.calls,
                                governor,
                            )? <= entry.total)
                } else {
                    // The same calls read no worse, and further ones.
                    later
                        && more
                        && self.prefix(
                            other_entry,
                            &mut tables[other],
                            &mut table_bytes,
                            entry.calls,
                            governor,
                        )? <= entry.total
                };
                if dominated {
                    keep[index] = false;
                    break;
                }
            }
        }
        governor.release_scratch(table_bytes)?;
        for (entry, keep) in mine.into_iter().zip(keep) {
            if !keep {
                if let HeldBody::Chain(chained) = &entry.body {
                    governor.release_scratch(chained.last_bytes())?;
                }
                continue;
            }
            match entry.body {
                HeldBody::Event(event, _) => self.events.push(event),
                HeldBody::Chain(chained) => {
                    let (parts, charged) = self.arena.parts(&chained, governor)?;
                    // The shared last part's charge is the copy's now.
                    governor.release_scratch(chained.last_bytes())?;
                    self.events.push(Event::Completed {
                        table: chained.table,
                        attempt: chained.attempt,
                        at: chained.at,
                        parts,
                        charged,
                    });
                }
            }
        }
        Ok(())
    }

    /// What the first `calls` calls of `held` cost together, building and
    /// keeping its table of prefix costs (counted in `bytes`) when asked
    /// for the first time.
    fn prefix(
        &self,
        held: &Held,
        table: &mut Option<Vec<u64>>,
        bytes: &mut u64,
        calls: usize,
        governor: &mut Governor,
    ) -> Result<u64, StopReason> {
        if calls >= held.calls {
            return Ok(held.total);
        }
        if table.is_none() {
            let built = match &held.body {
                HeldBody::Chain(chained) => self.arena.table(chained, governor)?,
                HeldBody::Event(_, costs) => {
                    governor.charge(Resource::Work, size(costs.len()))?;
                    governor.charge(
                        Resource::ScratchBytes,
                        size(costs.len() + 1).saturating_mul(size(core::mem::size_of::<u64>())),
                    )?;
                    let mut sums = Vec::with_capacity(costs.len() + 1);
                    let mut sum = 0_u64;
                    sums.push(0);
                    for cost in costs {
                        sum += cost;
                        sums.push(sum);
                    }
                    sums
                }
            };
            *bytes = bytes.saturating_add(size(built.len()).saturating_mul(8));
            *table = Some(built);
        }
        Ok(table.as_ref().expect("built")[calls])
    }

    /// Group completions by attempt and offset, pack equal-cost histories,
    /// and emit each as one segment.
    fn complete(
        &mut self,
        table: usize,
        completed: &mut ScratchVec<Thread>,
        eof: Option<&GenerationStop>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let slice = completed.as_mut_slice();
        slice.sort_by(|a, b| {
            (a.attempt, a.pos, a.pending, a.cost, a.hist)
                .cmp(&(b.attempt, b.pos, b.pending, b.cost, b.hist))
        });
        let mut groups: Vec<Thread> = Vec::new();
        // Completions whose deferred values differ (their costs are not
        // known yet) are never grouped.
        for thread in completed.as_slice() {
            match groups.last_mut() {
                Some(last)
                    if last.attempt == thread.attempt
                        && last.pos == thread.pos
                        && last.pending == thread.pending =>
                {
                    if last.cost == thread.cost {
                        last.hist = self.arena.pack(last.hist, thread.hist)?;
                    }
                }
                _ => groups.push(*thread),
            }
        }
        let dangling = matches!(eof, Some(GenerationStop::Length));
        for group in groups {
            // A completion before the end of output (a stalled failure's
            // repair) is decided by the bytes after it, as mid-output.
            if eof.is_some() && group.pos >= self.fed {
                self.emit(table, group, dangling, governor)?;
            } else {
                if let Some(record) = self.tables[table]
                    .attempts
                    .as_mut_slice()
                    .iter_mut()
                    .find(|record| record.id == group.attempt && !group.beside())
                {
                    record.closed = Some(group.pos);
                }
                self.tables[table].trails.push(Trail {
                    thread: group,
                    node: ROOT,
                    at: group.pos,
                    accepted: None,
                    scan: None,
                });
            }
        }
        self.advance_trails(table, self.fed, eof.is_some(), governor)
    }

    /// Advance every held completion of `table` over the bytes up to
    /// `until`, absorbing complete stray-closer spellings, and report each
    /// completion the following bytes decide (every one when `end`).
    fn advance_trails(
        &mut self,
        table: usize,
        until: usize,
        end: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut index = 0;
        while index < self.tables[table].trails.len() {
            let mut trail = self.tables[table].trails[index];
            let mut verdict = Verdict::Held;
            if trail.scan.is_none() && self.step_trail(table, &mut trail, until, end, governor)? {
                verdict = Verdict::Report;
                if self.ended_by_insertion(trail.thread.hist, governor)? {
                    trail.scan = Some(Scan { text: None });
                    trail.at = trail.thread.pos;
                }
            }
            if trail.scan.is_some() {
                verdict = self.scan_trail(table, &mut trail, until, end, governor)?;
            }
            match verdict {
                Verdict::Held => {
                    self.tables[table].trails[index] = trail;
                    index += 1;
                    continue;
                }
                Verdict::Report => {}
                Verdict::Guess => self.guess_insertion(&mut trail.thread, governor)?,
                Verdict::Reject => {
                    self.tables[table].trails.remove(index);
                    self.drop_completion(table, &trail);
                    continue;
                }
            }
            self.tables[table].trails.remove(index);
            self.emit(table, trail.thread, false, governor)?;
        }
        Ok(())
    }

    /// Step one held completion up to `until`; whether it is decided.
    fn step_trail(
        &mut self,
        table: usize,
        trail: &mut Trail,
        until: usize,
        end: bool,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let grammar = self.grammars[self.tables[table].grammar].grammar;
        let stray = &grammar.stray;
        loop {
            if trail.at >= until {
                if !end {
                    return Ok(false);
                }
                let Some(stop) = trail.accepted else {
                    return Ok(true);
                };
                self.absorb(trail, stop, governor)?;
                continue;
            }
            governor.charge(Resource::Work, K_THREAD)?;
            let byte = self.buffer.as_slice()[trail.at - self.base];
            if trail.node == ROOT && matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                // Whitespace between closers: held, up to the bounded gap;
                // a longer run stays content.
                if trail.at - trail.thread.pos >= STRAY_CLOSER_GAP {
                    return Ok(true);
                }
                trail.at += 1;
                continue;
            }
            if let Some(child) = stray.step(trail.node, byte) {
                trail.at += 1;
                trail.node = child;
                if stray.accept(child).is_some() {
                    trail.accepted = Some(trail.at);
                }
                if stray.has_children(child) {
                    continue;
                }
            }
            // The spelling ended (a leaf) or cannot continue: absorb the
            // longest complete spelling, or decide the completion.
            let Some(stop) = trail.accepted else {
                return Ok(true);
            };
            self.absorb(trail, stop, governor)?;
        }
    }

    /// Decide a call ended by inserting its wrapper's closer mid-message
    /// (`tool_call.markup.missing-wrapper-close`, possibly followed by
    /// absorbed stray closers) from the bytes after it, read up to `until`:
    ///
    /// - only whitespace, then the opener of another attempt of the same
    ///   production or the end of output: the insertion is the only
    ///   reading ([`Verdict::Report`]);
    /// - text, then a closer spelling of the grammar before any such
    ///   opener: the text is inside the wrapper, so the attempt is not a
    ///   call ([`Verdict::Reject`]; trailing text inside a call is never
    ///   an argument);
    /// - text, then such an opener or the end of output with no closer
    ///   before it: the text is content after the call or inside a wrapper
    ///   that never closes, so the insertion is a guess ([`Verdict::Guess`]).
    ///
    /// Until then the completion is held, and with it its attempt (so the
    /// capture window keeps its bytes, charged as scratch) and every turn
    /// report after the call's end ([`Self::held_floor`]): the decision
    /// never depends on how the output is chunked. Each byte read charges
    /// `Work` per literal compared.
    fn scan_trail(
        &self,
        table: usize,
        trail: &mut Trail,
        until: usize,
        end: bool,
        governor: &mut Governor,
    ) -> Result<Verdict, StopReason> {
        let grammar = self.grammars[self.tables[table].grammar].grammar;
        let program = self.tables[table].program;
        let openers: Vec<&[u8]> = program
            .openers
            .iter()
            .filter_map(|&pc| match program.insts[pc as usize] {
                Inst::Lit { lit, .. } => Some(grammar.literals[lit as usize].bytes.as_slice()),
                _ => None,
            })
            .collect();
        let closers: Vec<&[u8]> = grammar
            .closers
            .iter()
            .flat_map(|class| {
                class
                    .spellings
                    .iter()
                    .map(crate::contract::ScratchVec::as_slice)
            })
            .collect();
        // A reasoning block's opening marker: the block's bytes belong to
        // the block, never to the wrapper (see `Recognizer::ended`).
        let markers: Vec<&[u8]> = grammar
            .reasoning
            .iter()
            .map(|(open, _)| open.as_slice())
            .collect();
        let compared = size(openers.len() + closers.len() + markers.len()).saturating_mul(K_THREAD);
        let Some(mut scan) = trail.scan else {
            return Ok(Verdict::Report);
        };
        while trail.at < until {
            governor.charge(Resource::Work, compared)?;
            let at = trail.at;
            let byte = self.buffer.as_slice()[at - self.base];
            trail.at += 1;
            if scan.text.is_none() && !matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                scan.text = Some(at);
            }
            let read = &self.buffer.as_slice()[trail.thread.pos - self.base..trail.at - self.base];
            if let Some(opener) = openers
                .iter()
                .chain(&markers)
                .find(|opener| read.ends_with(opener))
            {
                // Another call, or a reasoning block, after the call: the
                // inserted closer is the only reading when only whitespace
                // came between, and text before it is a guess as before.
                let start = trail.at - opener.len();
                return Ok(if scan.text.is_some_and(|text| text < start) {
                    Verdict::Guess
                } else {
                    Verdict::Report
                });
            }
            if let Some(closer) = closers.iter().find(|closer| read.ends_with(closer)) {
                // A closer after only whitespace is the wrapper's own,
                // beyond the stray-closer gap: the call as it was read.
                let start = trail.at - closer.len();
                return Ok(if scan.text.is_some_and(|text| text < start) {
                    Verdict::Reject
                } else {
                    Verdict::Report
                });
            }
        }
        trail.scan = Some(scan);
        Ok(match (end, scan.text) {
            (false, _) => Verdict::Held,
            (true, None) => Verdict::Report,
            (true, Some(_)) => Verdict::Guess,
        })
    }

    /// The earliest end of a call whose inserted wrapper closer is still
    /// being decided ([`Self::scan_trail`]), or start of an attempt of a
    /// program with `commit` that has not committed yet ([`Self::commit`]):
    /// the turn layer settles no report at or after it until the decision
    /// is made or the attempt commits or ends.
    pub(crate) fn held_floor(&self) -> Option<usize> {
        let trails = self
            .tables
            .iter()
            .flat_map(|table| table.trails.iter())
            .filter(|trail| trail.scan.is_some())
            .map(|trail| trail.thread.pos);
        // An uncommitted attempt of a deferred program may still open at
        // its start: nothing at or after its start is settled until it
        // commits or ends.
        let held = self.tables.iter().filter_map(|table| table.held.floor);
        let uncommitted = self
            .tables
            .iter()
            .filter(|table| table.program.deferred())
            .flat_map(|table| table.attempts.as_slice().iter())
            .filter(|record| !record.opened)
            .map(|record| record.start);
        let deferred = self.deferred.iter().map(|(_, ended)| ended.at());
        trails.chain(held).chain(uncommitted).chain(deferred).min()
    }

    /// Record every inserted wrapper closer ending `thread`'s history as
    /// the guess `tool_call.markup.missing-wrapper-close.before-text`,
    /// adjusting its cost.
    fn guess_insertion(
        &mut self,
        thread: &mut Thread,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut pending = vec![thread.hist];
        let mut changed = false;
        while let Some(at) = pending.pop() {
            governor.charge(Resource::Work, K_THREAD)?;
            let HistNode { prev, kind } = self.arena.nodes.as_slice()[at as usize];
            match kind {
                HistKind::Edit(index) => {
                    let edit = &mut self.arena.edits[index as usize];
                    match edit.operator {
                        EditOperator::MissingWrapperClose => {
                            edit.operator = EditOperator::MissingWrapperCloseBeforeText;
                            changed = true;
                        }
                        EditOperator::DeleteStrayCloser => pending.push(prev),
                        _ => {}
                    }
                }
                HistKind::Pack(other) => pending.extend([prev, other]),
                _ => {}
            }
        }
        if changed {
            thread.cost = thread
                .cost
                .saturating_sub(EditOperator::MissingWrapperClose.rule().cost)
                .saturating_add(EditOperator::MissingWrapperCloseBeforeText.rule().cost);
        }
        Ok(())
    }

    /// Whether every history packed in `hist` ends with an inserted wrapper
    /// closer, possibly followed by absorbed stray closers.
    fn ended_by_insertion(&self, hist: u32, governor: &mut Governor) -> Result<bool, StopReason> {
        let nodes = self.arena.nodes.as_slice();
        let mut pending = vec![hist];
        while let Some(at) = pending.pop() {
            governor.charge(Resource::Work, K_THREAD)?;
            let HistNode { prev, kind } = nodes[at as usize];
            match kind {
                HistKind::Edit(index) => match self.arena.edits[index as usize].operator {
                    EditOperator::MissingWrapperClose => {}
                    EditOperator::DeleteStrayCloser => pending.push(prev),
                    _ => return Ok(false),
                },
                HistKind::Pack(other) => pending.extend([prev, other]),
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    /// Drop a held completion [`Self::scan_trail`] rejected: the attempt
    /// keeps the failure that led to the insertion, so it is rejected, its
    /// raw span and reason kept, unless another reading of it completes.
    fn drop_completion(&mut self, table: usize, trail: &Trail) {
        if let Some(record) = self.tables[table]
            .attempts
            .as_mut_slice()
            .iter_mut()
            .find(|record| record.id == trail.thread.attempt)
        {
            record.failure.get_or_insert(Rejection::Malformed {
                at: trail.thread.pos,
            });
            record.end.get_or_insert(trail.thread.pos);
        }
    }

    /// Absorb the stray closer from a held completion's end to `stop`.
    fn absorb(
        &mut self,
        trail: &mut Trail,
        stop: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let start = trail.thread.pos;
        let original = self.buffer.as_slice()[start - self.base..stop - self.base].to_vec();
        governor.charge(Resource::Work, size(original.len()))?;
        let HistNode { prev, kind } = self.arena.nodes.as_slice()[trail.thread.hist as usize];
        let inserted = match kind {
            HistKind::Edit(index) => {
                let edit = &self.arena.edits[index as usize];
                (edit.operator == EditOperator::MissingWrapperClose
                    && edit.span.start == start
                    && original.trim_ascii_start() == edit.replacement.as_slice())
                .then_some(prev)
            }
            _ => None,
        };
        if let Some(prev) = inserted {
            // The call's final closer was inserted where whitespace stood,
            // and that very closer follows the whitespace: it was never
            // missing. Undo the insertion and read the closer as written.
            trail.thread.hist = prev;
            trail.thread.cost = trail
                .thread
                .cost
                .saturating_sub(EditOperator::MissingWrapperClose.rule().cost);
        } else {
            let edit = MarkupEdit {
                operator: EditOperator::DeleteStrayCloser,
                span: start..stop,
                original,
                replacement: Vec::new(),
            };
            trail.thread.hist = self.arena.edit(trail.thread.hist, edit, governor)?;
            trail.thread.cost = trail
                .thread
                .cost
                .saturating_add(EditOperator::DeleteStrayCloser.rule().cost);
        }
        trail.thread.pos = stop;
        trail.at = stop;
        trail.node = ROOT;
        trail.accepted = None;
        Ok(())
    }

    fn emit(
        &mut self,
        table: usize,
        thread: Thread,
        dangling: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if thread.beside() && !dangling && !self.tables[table].program.grouped {
            // A reading beside another (its value holds closer text): held
            // until its attempt ends, when only the newest is reported (the
            // call ends at the last closer that completes it).
            governor.charge(
                Resource::ScratchBytes,
                size(<Thread as ScratchCharge>::CHARGE),
            )?;
            self.tables[table].past.push(thread);
            return Ok(());
        }
        self.report(table, thread, dangling, governor)
    }

    /// Report the held completions beside another reading of `attempt`
    /// (see [`Table::past`]) that end latest and whose value is read: the
    /// reading that ends the call at the last closer completing it. At that
    /// end, the least costly ones are reported (equally cheap ones side by
    /// side); a completion whose cost before its value is read (a lower
    /// bound) already exceeds the least found, or a completion as written
    /// of the same span, is dropped unread, as are the older ones and any
    /// whose value no repair reads (so a value holding many closers is read
    /// once or twice, not once per closer). `Work` per held completion and
    /// completion as written compared.
    fn release_past(
        &mut self,
        table: usize,
        attempt: u64,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if !self.tables[table]
            .past
            .iter()
            .any(|thread| thread.attempt == attempt)
        {
            return Ok(());
        }
        let (mine, rest): (Vec<Thread>, Vec<Thread>) =
            core::mem::take(&mut self.tables[table].past)
                .into_iter()
                .partition(|thread| thread.attempt == attempt);
        // The partition reads every held completion; each of
        // the attempt's is compared with its completions as written.
        governor.charge(
            Resource::Work,
            size(mine.len() + rest.len()).saturating_add(
                size(mine.len()).saturating_mul(size(self.tables[table].written.len()).max(1)),
            ),
        )?;
        self.tables[table].past = rest;
        governor.release_scratch(
            size(mine.len()).saturating_mul(size(<Thread as ScratchCharge>::CHARGE)),
        )?;
        // The latest end first; within one end, the least cost before the
        // value is read first (reading a value only adds cost).
        let mut mine = mine;
        mine.sort_by_key(|thread| (Reverse(thread.pos), thread.cost));
        let mut index = 0;
        while index < mine.len() {
            let end = mine[index].pos;
            let group = mine[index..]
                .iter()
                .take_while(|thread| thread.pos == end)
                .count();
            // The same span already reported as written at less cost.
            let written = self.tables[table]
                .written
                .iter()
                .filter(|&&(owner, at, _)| owner == attempt && at == end)
                .map(|&(_, _, cost)| cost)
                .min();
            let mut best: Option<u64> = None;
            let mut chosen: Vec<Thread> = Vec::new();
            for &candidate in &mine[index..index + group] {
                let bound = written.into_iter().chain(best).min();
                if bound.is_some_and(|bound| candidate.cost > bound) {
                    continue;
                }
                let mut thread = candidate;
                if thread.pending != 0 {
                    let (mut step, _, _) = self.split(table);
                    let Some(added) = step.resolve(thread.pending, governor)? else {
                        continue;
                    };
                    thread.cost = thread.cost.saturating_add(added);
                    thread.pending = 0;
                }
                if written.is_some_and(|written| thread.cost > written) {
                    continue;
                }
                match best {
                    Some(least) if thread.cost > least => {}
                    Some(least) if thread.cost == least => chosen.push(thread),
                    _ => {
                        best = Some(thread.cost);
                        chosen.clear();
                        chosen.push(thread);
                    }
                }
            }
            if !chosen.is_empty() {
                for thread in chosen {
                    self.report(table, thread, false, governor)?;
                }
                return Ok(());
            }
            index += group;
        }
        Ok(())
    }

    /// Drop the completions as written recorded for `attempt` (see
    /// [`Table::written`]), which has ended.
    fn forget_written(
        &mut self,
        table: usize,
        attempt: u64,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let written = &mut self.tables[table].written;
        let before = written.len();
        written.retain(|&(owner, _, _)| owner != attempt);
        governor.release_scratch(size(before - written.len()).saturating_mul(WRITTEN_BYTES))
    }

    /// Hold the completion `thread` reported, segmented as a chain (see
    /// [`Step::segment_chained`]), or record why its attempt could not read
    /// it.
    fn hold_chained(
        &mut self,
        table: usize,
        thread: &Thread,
        dangling: bool,
        chained: Result<Chained, Rejection>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let record = self.tables[table]
            .attempts
            .as_mut_slice()
            .iter_mut()
            .find(|record| record.id == thread.attempt)
            .expect("completing attempt is live");
        let mut chained = match chained {
            Ok(chained) => chained,
            Err(reason) => {
                record.failure.get_or_insert(reason);
                return Ok(());
            }
        };
        record.completed = true;
        let start = record.start;
        chained.table = index_u32(table);
        chained.attempt = thread.attempt;
        chained.at = thread.pos;
        // It ends whole: a held report at its opener is outread now.
        if !self.whole.contains(&start) {
            governor.charge(Resource::ScratchBytes, DEFERRED_BYTES)?;
            self.whole.push(start);
        }
        governor.charge(Resource::Work, super::cost::K_EVENT)?;
        let calls = self.arena.depth_of(&chained) + 1;
        let total = self.arena.total(&chained);
        self.tables[table].held.push(Held {
            attempt: thread.attempt,
            at: thread.pos,
            calls,
            total,
            dangling,
            arguments: chained.arguments(),
            body: HeldBody::Chain(chained),
        });
        Ok(())
    }

    fn report(
        &mut self,
        table: usize,
        mut thread: Thread,
        dangling: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if !thread.beside() && !self.tables[table].program.grouped {
            governor.charge(Resource::ScratchBytes, WRITTEN_BYTES)?;
            self.tables[table]
                .written
                .push((thread.attempt, thread.pos, thread.cost));
        }
        if thread.pending != 0 {
            // A deferred body's value is read now: a value no repair reads
            // leaves no completion (this reading beside another is dropped).
            let (mut step, _, _) = self.split(table);
            let Some(added) = step.resolve(thread.pending, governor)? else {
                return Ok(());
            };
            thread.cost = thread.cost.saturating_add(added);
            thread.pending = 0;
        }
        let kind = self.tables[table].kind;
        let dialect = self.grammars[self.tables[table].grammar].dialect.clone();
        let (mut step, _, _) = self.split(table);
        // A length stop inside a character a class run had begun: the
        // capture keeps the whole characters (as a cut-off slot does).
        let open_end = match (step.program.insts[thread.pc as usize], thread.sub) {
            (Inst::Class { .. }, Sub::Class(state)) if dangling => {
                thread.pos - state.partial_bytes()
            }
            _ => thread.pos,
        };
        let shape = Shape {
            span: thread.start..thread.pos,
            dialect: &dialect,
            kind,
            dangling,
            open_end,
            cost: thread.cost,
        };
        if step.program.grouped
            && let Some(chained) = step.segment_chained(thread.hist, &shape, governor)?
        {
            return self.hold_chained(table, &thread, dangling, chained, governor);
        }
        let (step, _, _) = self.split(table);
        let before = governor.scratch_meter().total_charged();
        let segmentations = step.segment(thread.hist, &shape, governor)?;
        // Materializing charges only retained scratch (no lease), so the
        // growth of the cumulative total is what the parts hold.
        let charged = governor
            .scratch_meter()
            .total_charged()
            .saturating_sub(before);
        let attempts = self.tables[table].attempts.as_mut_slice();
        let record = attempts
            .iter_mut()
            .find(|record| record.id == thread.attempt)
            .expect("completing attempt is live");
        match segmentations {
            Ok(segmentations) => {
                record.completed = true;
                // It ends whole: a held report at its opener is outread now,
                // and no longer waits for it (see `Self::defer`).
                if !self.whole.contains(&record.start) {
                    governor.charge(Resource::ScratchBytes, DEFERRED_BYTES)?;
                    self.whole.push(record.start);
                }
                let grouped = self.tables[table].program.grouped;
                let charged = if segmentations.len() == 1 { charged } else { 0 };
                for parts in segmentations {
                    governor.charge(Resource::Work, super::cost::K_EVENT)?;
                    let event = Event::Completed {
                        table: index_u32(table),
                        attempt: thread.attempt,
                        at: thread.pos,
                        parts,
                        charged,
                    };
                    if grouped {
                        let (costs, arguments) = match &event {
                            Event::Completed { parts, .. } => (
                                parts.iter().map(|(_, cost)| *cost).collect(),
                                parts.last().is_some_and(|(part, _)| began_arguments(part)),
                            ),
                            _ => (Vec::new(), false),
                        };
                        self.tables[table].held.push(Held {
                            attempt: thread.attempt,
                            at: event.at(),
                            calls: costs.len(),
                            total: costs.iter().sum(),
                            dangling,
                            arguments,
                            body: HeldBody::Event(event, costs),
                        });
                    } else {
                        self.events.push(event);
                    }
                }
            }
            Err(reason) => {
                record.failure.get_or_insert(reason);
            }
        }
        Ok(())
    }

    /// Run the repair search for one stuck attempt.
    fn repair(
        &mut self,
        table: usize,
        record: &Attempt,
        failures: &[Failure],
        until: usize,
        eof: Option<&GenerationStop>,
        governor: &mut Governor,
    ) -> Result<Vec<Repaired>, StopReason> {
        let (step, _, _) = self.split(table);
        let seeds: Vec<RepairState<'_, 's>> = failures
            .iter()
            .filter_map(|failure| RepairState::seed(&step, failure, eof, record.opened))
            .collect();
        if seeds.is_empty() {
            return Ok(Vec::new());
        }
        // Repairs explored here are not competing answers: only the readings
        // of a completed segment are (see `segment`), so the queue charges
        // `Work` and `ScratchBytes` but no `Candidates`.
        let mut search = Search::uncounted(governor);
        for seed in seeds {
            let ledger = Ledger {
                cost: seed.thread.cost,
                ..Ledger::default()
            };
            search.seed(governor, seed, ledger)?;
        }
        let mut context = RepairContext {
            step,
            until,
            eof: eof.cloned(),
            opened: record.opened,
        };
        let accepted = search.run(&mut context, governor)?;
        Ok(accepted
            .into_states()
            .into_iter()
            .map(|(state, ledger)| Repaired {
                thread: Thread {
                    cost: ledger.cost,
                    ..state.thread
                },
                status: state.status,
            })
            .collect())
    }

    /// Drop histories and window bytes no live configuration needs.
    fn collect(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        let earliest = self
            .tables
            .iter()
            .flat_map(|table| {
                table
                    .attempts
                    .as_slice()
                    .iter()
                    .map(|attempt| attempt.start)
            })
            .min();
        let Some(earliest) = earliest else {
            self.arena.reset(governor)?;
            self.buffer.clear();
            self.base = self.fed;
            return Ok(());
        };
        let drop = earliest - self.base;
        if drop > 64 && drop * 2 > self.buffer.len() {
            governor.charge(Resource::Work, size(self.buffer.len() - drop))?;
            let length = self.buffer.len();
            self.buffer.as_mut_slice().copy_within(drop..length, 0);
            for _ in 0..drop {
                self.buffer.pop();
            }
            self.base = earliest;
        }
        Ok(())
    }

    /// Stop every body that read on past a closer (see [`PAST_REPAIRED`]) of
    /// an attempt begun before `start`, where a reasoning block opens: a
    /// call is cut at the block's marker (`tool_call.reasoning.marker-cut`),
    /// so a value read on past it is never read. One `Work` unit per
    /// configuration examined.
    pub(crate) fn stop_past_at(
        &mut self,
        start: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        for table in &mut self.tables {
            Self::stop_past(&mut table.threads, Some(start), &[], governor)?;
        }
        Ok(())
    }

    /// Drop attempt `attempt` of every table: its configurations, trails,
    /// held completions and failures, and its record, with no event. The
    /// turn level abandons an attempt that opened where no reading of the
    /// turn can take it (inside another attempt's committed span): its
    /// completion or rejection would be dropped unread, so reading it to
    /// its end is wasted work. One `Work` unit per entry examined.
    pub(crate) fn abandon(
        &mut self,
        attempt: u64,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        fn keep<T: Copy + ScratchCharge>(entries: &mut ScratchVec<T>, kept: impl Fn(&T) -> bool) {
            let mut write = 0;
            for read in 0..entries.len() {
                let entry = entries.as_slice()[read];
                if kept(&entry) {
                    entries.as_mut_slice()[write] = entry;
                    write += 1;
                }
            }
            while entries.len() > write {
                entries.pop();
            }
        }
        for table in &mut self.tables {
            governor.charge(
                Resource::Work,
                size(table.threads.len() + table.attempts.len() + table.trails.len()),
            )?;
            keep(&mut table.threads, |thread| thread.attempt != attempt);
            keep(&mut table.attempts, |record| record.id != attempt);
            governor.charge(
                Resource::Work,
                size(table.failures.count(attempt)).saturating_add(1),
            )?;
            table.failures.remove_attempt(attempt);
            keep(&mut table.boundaries, |known| known.attempt != attempt);
            keep(&mut table.stalled, |failure| {
                failure.thread.attempt != attempt
            });
            table.trails.retain(|trail| trail.thread.attempt != attempt);
            let mut freed = 0_u64;
            for entry in table.held.take(attempt) {
                if let HeldBody::Chain(chained) = &entry.body {
                    freed = freed.saturating_add(chained.last_bytes());
                }
            }
            governor.release_scratch(freed)?;
            let dropped = table
                .past
                .iter()
                .filter(|thread| thread.attempt == attempt)
                .count();
            table.past.retain(|thread| thread.attempt != attempt);
            governor.release_scratch(
                size(dropped).saturating_mul(size(<Thread as ScratchCharge>::CHARGE)),
            )?;
            let written = table.written.len();
            table.written.retain(|&(owner, _, _)| owner != attempt);
            governor.release_scratch(
                size(written - table.written.len()).saturating_mul(WRITTEN_BYTES),
            )?;
        }
        self.forget_resync(attempt, governor)?;
        self.release_deferred(governor)
    }

    /// End of input: complete, cut off, or reject every open attempt.
    pub(crate) fn finish(
        &mut self,
        generation: &GenerationStop,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let end = self.fed;
        for table in 0..self.tables.len() {
            if self.tables[table].threads.is_empty() && self.tables[table].attempts.is_empty() {
                continue;
            }
            self.out.clear();
            if *generation == GenerationStop::Length {
                self.advance_trails(table, end, true, governor)?;
                self.cut(table, governor)?;
            } else {
                {
                    let threads = &mut self.tables[table].threads;
                    self.stack.clear();
                    while let Some(thread) = threads.pop() {
                        self.stack.push(thread)?;
                    }
                }
                let (mut step, stack, out) = self.split(table);
                step.advance(stack, end, true, out, governor)?;
                // At the end every configuration has resolved: nothing stays live.
                self.out.live.clear();
                self.settle(table, end, Some(generation), governor)?;
            }
            self.tables[table].threads.clear();
            self.tables[table].attempts.clear();
            self.tables[table].failures.clear();
            self.tables[table].stalled.clear();
            let written = core::mem::take(&mut self.tables[table].written);
            governor.release_scratch(size(written.len()).saturating_mul(WRITTEN_BYTES))?;
            self.tables[table].trails.clear();
        }
        // Every attempt has ended: every held report is due.
        self.release_deferred(governor)
    }

    /// Report the completions of whole calls found at a length stop (see
    /// `Step::cut_thread`), equal-cost histories at one end packed; whether
    /// there were any.
    fn whole_calls(
        &mut self,
        table: usize,
        completed: &mut ScratchVec<Thread>,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let slice = completed.as_mut_slice();
        slice.sort_by_key(|thread| (thread.pos, thread.pending, thread.cost, thread.hist));
        let mut groups: Vec<Thread> = Vec::new();
        for thread in completed.as_slice() {
            match groups.last_mut() {
                Some(last)
                    if last.pos == thread.pos
                        && last.pending == thread.pending
                        && last.cost == thread.cost =>
                {
                    last.hist = self.arena.pack(last.hist, thread.hist)?;
                }
                _ => groups.push(*thread),
            }
        }
        for thread in &groups {
            self.emit(table, *thread, false, governor)?;
        }
        Ok(!groups.is_empty())
    }

    /// A length stop: every open attempt becomes a cut-off reading of its
    /// least-cost configurations, except that a call whose arguments value
    /// was complete at the cut completes.
    fn cut(&mut self, table: usize, governor: &mut Governor) -> Result<(), StopReason> {
        let records: Vec<Attempt> = self.tables[table].attempts.as_slice().to_vec();
        let threads: Vec<Thread> = self.tables[table].threads.as_slice().to_vec();
        let grouped = self.tables[table].program.grouped;
        for record in records {
            // Its reading beside another, held until now, may complete it.
            self.release_past(table, record.id, governor)?;
            let record = self.tables[table]
                .attempts
                .as_slice()
                .iter()
                .find(|live| live.id == record.id)
                .copied()
                .unwrap_or(record);
            // A grouped attempt that completed a call may still be writing
            // the next one: its cut-off reading competes too.
            if record.opened && (!record.completed || grouped) {
                let mut best: Option<Thread> = None;
                let mut stack = ScratchVec::new(governor);
                let mut whole = Out::new(governor);
                for thread in threads.iter().filter(|thread| thread.attempt == record.id) {
                    let (mut step, _, _) = self.split(table);
                    let Some((hist, added)) =
                        step.cut_thread(*thread, &mut stack, &mut whole, governor)?
                    else {
                        continue;
                    };
                    let candidate = Thread {
                        hist,
                        cost: thread.cost.saturating_add(added),
                        ..*thread
                    };
                    best = Some(match best {
                        None => candidate,
                        Some(current) if candidate.cost < current.cost => candidate,
                        Some(mut current) if candidate.cost == current.cost => {
                            current.hist = self.arena.pack(current.hist, candidate.hist)?;
                            current
                        }
                        Some(current) => current,
                    });
                }
                // A call whose arguments value was complete at the cut is
                // whole: the stop cut the output after it, not the call.
                let completed = self.whole_calls(table, &mut whole.completed, governor)?;
                self.release_past(table, record.id, governor)?;
                if let Some(thread) = best.filter(|_| grouped || !completed) {
                    self.emit(
                        table,
                        Thread {
                            pos: self.fed,
                            ..thread
                        },
                        true,
                        governor,
                    )?;
                }
            }
            self.forget_written(table, record.id, governor)?;
            self.release(table, record.id, governor)?;
            self.forget_resync(record.id, governor)?;
            self.take_boundary(table, record.id);
            if record.opened {
                // The cut-off reading may itself be rejected (a capture that
                // is not well-formed UTF-8): the attempt then ends rejected.
                let live = self.tables[table]
                    .attempts
                    .as_slice()
                    .iter()
                    .find(|live| live.id == record.id)
                    .copied()
                    .expect("the attempt is live until it ends");
                // A completed or cut-off reading read a whole call: a
                // rejection held at its opener is outread (see `defer`).
                if live.completed && !self.whole.contains(&record.start) {
                    governor.charge(Resource::ScratchBytes, DEFERRED_BYTES)?;
                    self.whole.push(record.start);
                }
                self.events.push(Event::Ended {
                    table: index_u32(table),
                    attempt: record.id,
                    start: record.start,
                    at: self.fed,
                    completed: live.completed,
                    reason: match live.failure {
                        Some(
                            reason @ (Rejection::InvalidUtf8 { .. }
                            | Rejection::PythonicPositionalRejected { .. }),
                        ) => reason,
                        _ => Rejection::Unterminated,
                    },
                    // A length stop keeps the cut-off reading instead.
                    salvage: None,
                    outread: false,
                });
            }
        }
        Ok(())
    }
}

/// Whether `thread` reads a call's verbatim argument body: a `raw` or
/// `xml-params` slot of a call production (`kind`) of `program`.
fn verbatim_slot(program: &Program, kind: ProductionKind, thread: &Thread) -> bool {
    kind == ProductionKind::Call
        && matches!(
            program.insts[thread.pc as usize],
            Inst::Slot {
                codec: CodecKind::Raw | CodecKind::XmlParams | CodecKind::KvPairs,
                ..
            }
        )
}

/// The digest of a reading's edit sequence after one more edit.
pub(crate) fn extend_digest(digest: Digest, operator: EditOperator, at: usize) -> Digest {
    let mut builder = crate::digest::DigestBuilder::new();
    builder.update(digest.as_bytes());
    builder.update(&[operator as u8]);
    builder.update(&(at as u64).to_be_bytes());
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Budget;

    fn failure(attempt: u64, entry: usize) -> Failure {
        Failure {
            thread: Thread {
                pc: 0,
                sub: Sub::Lit(0),
                attempt,
                start: 0,
                entry,
                pos: entry,
                cost: 0,
                hist: 0,
                opened: false,
                stall: 0,
                reach: 0,
                past: false,
                pending: 0,
            },
            at: entry,
            reason: Rejection::Malformed { at: entry },
            respell: false,
        }
    }

    fn work(governor: &Governor) -> u64 {
        governor.usage().work
    }

    /// A step examines the failures it can drop, not the ones that stay: with
    /// every live configuration at or before the held failures' entries, a
    /// step over 100 held failures and a step over 20,000 charge the same;
    /// a step whose floors changed examines each group once and charges one
    /// unit more for each of the `d` failures it drops.
    #[test]
    fn a_step_over_held_failures_costs_what_it_changes() {
        let mut governor = Governor::new(Budget::default());
        let mut held = HeldFailures::new(&governor);
        let floors = [(1_u64, 0_usize)];
        let mut cost_at = Vec::new();
        for count in [100_usize, 20_000] {
            while held.count(1) < count {
                // Half the failures are `trailing` ones, keyed by nothing.
                let next = held.count(1);
                let fresh = [(failure(1, next + 10), next.is_multiple_of(2))];
                held.retain(&floors, &fresh, &mut governor).unwrap();
            }
            let before = work(&governor);
            held.retain(&floors, &[], &mut governor).unwrap();
            cost_at.push(work(&governor) - before);
            assert_eq!(held.count(1), count);
        }
        assert_eq!(cost_at[0], cost_at[1], "{cost_at:?}");
        assert_eq!(cost_at[1], 0, "{cost_at:?}");
        // The floor rises past the first 5,010 entries: only the keyed
        // (non-trailing) ones below it go, one `Work` unit each, and the
        // others keep their order.
        let before = work(&governor);
        held.retain(&[(1, 5_010)], &[], &mut governor).unwrap();
        let dropped = 20_000 - held.count(1);
        assert_eq!(dropped, 2_500);
        assert_eq!(work(&governor) - before, 1 + dropped as u64);
        let entries: Vec<usize> = held.of(1).map(|failure| failure.thread.entry).collect();
        assert!(entries.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(entries.iter().take(2_500).all(|&entry| entry < 5_010));
        // Without a live configuration the attempt's failures all go.
        held.retain(&[], &[], &mut governor).unwrap();
        assert_eq!(held.count(1), 0);
        assert_eq!(held.reserved, 0);
    }
}
