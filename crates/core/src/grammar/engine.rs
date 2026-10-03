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
    ArgTree, CallSegment, CaptureSpan, Closure, CompiledTool, DialectId, GenerationStop,
    ProductionKind, RawCall, Rejection, Spanned, ToolSet,
};
use core::cmp::Reverse;
use core::ops::Range;

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
}

impl ScratchCharge for Thread {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 120);
        120
    };
}

impl Thread {
    /// The merge key: program location, terminal mode, and literal node.
    pub(crate) const fn key(&self) -> (u32, u8, u32) {
        let (mode, node) = self.sub.key();
        (self.pc, mode, node)
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
    /// The scratch charged for `values`, `edits` and `codecs`, released by
    /// [`Self::reset`] or, for what is still held, when the arena drops.
    charged: u64,
    meter: ScratchMeter,
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
            charged: 0,
            meter: governor.scratch_meter(),
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
    /// history and the edits' total cost.
    fn codec_value(&mut self, prev: u32, value: CodecValue) -> Result<(u32, u64), StopReason> {
        let mut hist = self.value(prev, value.tree)?;
        let mut cost = 0_u64;
        for edit in value.edits {
            cost = cost.saturating_add(edit.rule().cost);
            hist = self.adopt_edit(hist, edit)?;
        }
        Ok((hist, cost))
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
        governor.release_scratch(core::mem::take(&mut self.charged))
    }
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
    pub(crate) dfas: &'a mut [DerivativeDfa<'s>],
    pub(crate) arena: &'a mut Arena,
    pub(crate) buffer: &'a [u8],
    pub(crate) base: usize,
    pub(crate) stall_cap: u32,
    pub(crate) visited: &'a mut Vec<u64>,
    pub(crate) pending: &'a mut Vec<(u32, u32, u64)>,
    pub(crate) tools: Option<&'s ToolSet>,
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
                    if grammar.literals[lit as usize].closer.is_some() {
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
                canonical.clone()
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
        let Some(class) = literal.closer else {
            return Ok(());
        };
        let token = self.bytes(thread.entry..end);
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
            replacement: literal.bytes.clone(),
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
        let Sub::Body(state, codec) = thread.sub else {
            unreachable!("body configurations only")
        };
        let pos = thread.pos;
        self.misspelled_delimiter(thread, set, next, byte, stack, out, governor)?;
        let matcher = &self.grammar.delimiters[set as usize].matcher;
        let released = pos - matcher.depth(state) as usize;
        let stepped = matcher.step(state, byte);
        let (to, delimited) = match matcher.found(stepped) {
            Some((_, length)) => (pos + 1 - length as usize, true),
            None => (pos + 1 - matcher.depth(stepped) as usize, false),
        };
        let mut end = delimited.then_some(to);
        if let Some(slot) = codec {
            match self.feed(slot, released..to, governor)? {
                Ok(Some(complete)) => end = Some(complete),
                Ok(None) => {}
                Err(fault) => return Self::fail(out, thread, pos, rejection(fault)),
            }
        }
        let Some(end) = end else {
            thread.sub = Sub::Body(stepped, codec);
            thread.pos = pos + 1;
            return self.queue(thread, stack);
        };
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
        if kind.is_some_and(|kind| !matches!(kind, CodecKind::Raw | CodecKind::XmlParams)) {
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
            let codec = match kind {
                Some(kind) => Some(
                    self.arena
                        .begin_codec(kind.begin(thread.entry, governor)?, governor)?,
                ),
                None => None,
            };
            let fork = Thread {
                sub: Sub::Body(0, codec),
                ..thread
            };
            self.end_body(
                fork,
                codec,
                start,
                CodecEnd::Delimited,
                next,
                stack,
                out,
                governor,
            )?;
        }
        Ok(())
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
        if let Some(slot) = codec {
            match self.finish_codec(slot, thread.hist, thread.entry..end, how, governor)? {
                Ok((hist, cost)) => {
                    thread.hist = hist;
                    thread.cost = thread.cost.saturating_add(cost);
                }
                Err(fault) => return Self::fail(out, thread, thread.pos, rejection(fault)),
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
    ) -> Result<Result<(u32, u64), CodecFault>, StopReason> {
        let codec = self.arena.take_codec(slot);
        let context = self.context(codec.kind(), hist, governor)?;
        let buffer = self.buffer;
        let bytes = &buffer[body.start - self.base..body.end - self.base];
        match codec.finish(bytes, body, how, context, governor)? {
            Ok(value) => Ok(Ok(self.arena.codec_value(hist, value)?)),
            Err(fault) => Ok(Err(fault)),
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
        let Some(tools) = self.tools.filter(|_| kind.uses_context()) else {
            return Ok(CodecContext::default());
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
            return Ok(CodecContext::default());
        };
        governor.charge(Resource::Work, size(name.len()).saturating_mul(K_CODEC))?;
        let shape = core::str::from_utf8(self.bytes(name))
            .ok()
            .and_then(|name| tools.lookup_exact(name))
            .map(CompiledTool::argument_shape);
        Ok(CodecContext { shape })
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
                        replacement: self.grammar.literals[lit as usize].bytes.clone(),
                    };
                    let node = self.arena.edit(hist, edit, governor)?;
                    let cost = cost.saturating_add(EditOperator::MissingToken.rule().cost);
                    self.pending.push((next, node, cost));
                    continue;
                }
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
                    let slot = self
                        .arena
                        .begin_codec(codec.begin(at, governor)?, governor)?;
                    Sub::Body(super::automaton::ROOT, Some(slot))
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
            (Inst::Until { set, next } | Inst::Slot { set, next, .. }, Sub::Body(state, codec)) => {
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
                HistKind::Root | HistKind::Pack(_) | HistKind::Next(..) => {}
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
            let mut calls = Vec::with_capacity(cuts.len() + 1);
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
                match self.reading(&path[from..to], &part_shape, governor)? {
                    Ok(call) => calls.push(call),
                    Err(reason) => {
                        rejection.get_or_insert(reason);
                        continue 'paths;
                    }
                }
                from = to + 1;
            }
            let boundaries: Vec<(usize, u64)> =
                cuts.iter().map(|&(_, at, cost)| (at, cost)).collect();
            let found = groups.iter().position(|(known, _)| *known == boundaries);
            let index = found.unwrap_or_else(|| {
                groups.push((boundaries, vec![Vec::new(); calls.len()]));
                groups.len() - 1
            });
            let group = &mut groups[index].1;
            for (readings, call) in group.iter_mut().zip(calls) {
                if !readings.contains(&call) {
                    // Every reading beyond the first competes.
                    if !readings.is_empty() {
                        governor.charge(Resource::Candidates, 1)?;
                    }
                    readings.push(call);
                }
            }
        }
        if groups.is_empty() {
            return Ok(Err(
                rejection.unwrap_or(Rejection::Malformed { at: shape.span.end })
            ));
        }
        Ok(Ok(groups
            .into_iter()
            .map(|(boundaries, calls)| {
                calls
                    .into_iter()
                    .enumerate()
                    .map(|(part, readings)| {
                        let before = if part == 0 { 0 } else { boundaries[part - 1].1 };
                        let through = boundaries.get(part).map_or(shape.cost, |&(_, cost)| cost);
                        let span = readings[0].span.clone();
                        let weight = self.undeclared(shape.kind, &readings);
                        (
                            CallSegment {
                                span,
                                readings,
                                derivations: Vec::new(),
                            },
                            through.saturating_sub(before).saturating_add(weight),
                        )
                    })
                    .collect()
            })
            .collect()))
    }
}

impl Step<'_, '_> {
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

/// One segmentation of a completed attempt: each call's segment, in order,
/// with the cost it carries.
pub(crate) type Parts = Vec<(CallSegment, u64)>;

/// The readings of one segmentation while they are collected: where each
/// later call begins (with the cost so far), and each call's readings.
type Grouped = (Vec<(usize, u64)>, Vec<Vec<RawCall>>);

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
}

impl ScratchCharge for Boundary {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 32);
        32
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
    held: Vec<Held>,
    /// Literal failures of live attempts kept as repair seeds while a
    /// configuration of the same attempt that began its terminal at or
    /// before the failed literal's start is still live (see `settle`).
    failures: ScratchVec<Failure>,
    /// For a grouped program, the last call boundary of each attempt that
    /// got stuck after one (see [`Salvaged`]).
    boundaries: ScratchVec<Boundary>,
}

/// A completion of a grouped program's attempt, held until the attempt
/// ends.
struct Held {
    attempt: u64,
    event: Event,
    /// The cost each call of the completion carries, in order.
    costs: Vec<u64>,
    dangling: bool,
    /// Whether the last call of a cut-off reading began its arguments: only
    /// then is it evidence that the attempt went on past an earlier
    /// completion (a name alone, or a separator, may be text after it).
    arguments: bool,
}

impl Held {
    /// What the completion's first `calls` calls cost together.
    fn prefix(&self, calls: usize) -> u64 {
        self.costs.iter().take(calls).sum()
    }
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
    arena: Arena,
    next_attempt: u64,
    stack: ScratchVec<Thread>,
    out: Out,
    visited: Vec<u64>,
    pending: Vec<(u32, u32, u64)>,
    tools: Option<&'s ToolSet>,
    pub(crate) events: Vec<Event>,
}

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
                    held: Vec::new(),
                    failures: ScratchVec::new(governor),
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
            arena: Arena::new(governor)?,
            next_attempt: 0,
            stack: ScratchVec::new(governor),
            out: Out::new(governor),
            visited: Vec::new(),
            pending: Vec::new(),
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
                dfas: &mut runtime.dfas,
                arena: &mut self.arena,
                buffer: self.buffer.as_slice(),
                base: self.base,
                stall_cap,
                visited: &mut self.visited,
                pending: &mut self.pending,
                tools: self.tools,
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
        if program.starts_with(byte) {
            let id = self.next_attempt;
            self.next_attempt += 1;
            self.tables[table].attempts.push(Attempt {
                id,
                start: at,
                opened: false,
                completed: false,
                failure: None,
                end: None,
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
            };
            if program.start_closure {
                // Zero-width instructions with effects (an `insert`) lie
                // before the opener: follow them from the start.
                spawned = Some(thread);
            } else {
                for &pc in &program.openers {
                    self.tables[table].threads.push(Thread { pc, ..thread })?;
                }
            }
        }
        if spawned.is_none()
            && self.tables[table].threads.is_empty()
            && self.tables[table].trails.is_empty()
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
        let failed = core::mem::replace(&mut self.out.failed, ScratchVec::new(governor));
        // A literal that failed while a sibling configuration of its
        // attempt, begun at the same place or earlier, lived on (the
        // canonical `<|tool_call_begin|>` beside a variant spelling of a
        // section closer that shares its first bytes) is still a repair
        // seed, by respelling only, when the attempt later runs out of
        // configurations. The table keeps such failures between steps.
        let earlier =
            core::mem::replace(&mut self.tables[table].failures, ScratchVec::new(governor));
        self.commit(
            table,
            live.as_slice()
                .iter()
                .chain(completed.as_slice())
                .chain(failed.as_slice().iter().map(|failure| &failure.thread)),
            governor,
        )?;
        self.open(table, governor)?;
        // Repairs for attempts left with nothing.
        let _records = governor.lease_scratch(
            size(self.tables[table].attempts.len())
                .saturating_mul(size(<Attempt as ScratchCharge>::CHARGE)),
        )?;
        let records: Vec<Attempt> = self.tables[table].attempts.as_slice().to_vec();
        for record in &records {
            let touches = |threads: &ScratchVec<Thread>| {
                threads
                    .as_slice()
                    .iter()
                    .any(|thread| thread.attempt == record.id)
            };
            if touches(&live) || touches(&completed) || record.completed {
                continue;
            }
            let mine = |failure: &&Failure| failure.thread.attempt == record.id;
            let held = earlier
                .as_slice()
                .iter()
                .filter(|_| eof.is_none())
                .filter(mine)
                .map(|failure| Failure {
                    respell: true,
                    ..*failure
                });
            let _failures = governor.lease_scratch(
                size(failed.as_slice().iter().filter(mine).count() + held.clone().count())
                    .saturating_mul(size(<Failure as ScratchCharge>::CHARGE)),
            )?;
            let failures: Vec<Failure> = failed
                .as_slice()
                .iter()
                .filter(mine)
                .copied()
                .chain(held)
                .collect();
            self.note_failure(table, record.id, &failures);
            self.note_boundary(table, record.id, &failures, governor)?;
            let until = if eof.is_some() { at } else { at + 1 };
            for state in self.repair(table, record, &failures, until, eof, governor)? {
                match state.status {
                    Status::Live => live.push(state.thread)?,
                    Status::Completed => completed.push(state.thread)?,
                    Status::Failed | Status::Respelled | Status::Stuck => {}
                }
            }
        }
        self.hold_failures(table, earlier, &failed, &live)?;
        merge(&mut live, &mut self.arena, governor)?;
        // A repair may carry an attempt past its commit, and a completion
        // always commits its attempt.
        if self.tables[table].program.deferred() {
            self.commit(
                table,
                live.as_slice().iter().chain(completed.as_slice()),
                governor,
            )?;
            self.open(table, governor)?;
        }
        self.complete(table, &mut completed, eof, governor)?;
        self.retire(table, &live, at, eof, governor)?;
        self.tables[table].threads = live;
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
        mut earlier: ScratchVec<Failure>,
        failed: &ScratchVec<Failure>,
        live: &ScratchVec<Thread>,
    ) -> Result<(), StopReason> {
        let grammar = self.grammars[self.tables[table].grammar].grammar;
        let program = self.tables[table].program;
        let sibling = |failure: &Failure| {
            // Only a literal with a respelling (or, in a `json-envelope`
            // grammar, a JSON separator insertion or deletion) can be
            // repaired later.
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
            respellable
                && matches!(failure.thread.sub, Sub::Lit(_))
                && live.as_slice().iter().any(|thread| {
                    thread.attempt == failure.thread.attempt
                        && (trailing || thread.entry <= failure.thread.entry)
                })
        };
        let mut write = 0;
        for read in 0..earlier.len() {
            let failure = earlier.as_slice()[read];
            if sibling(&failure) {
                earlier.as_mut_slice()[write] = failure;
                write += 1;
            }
        }
        while earlier.len() > write {
            earlier.pop();
        }
        for failure in failed.as_slice() {
            if sibling(failure) {
                earlier.push(*failure)?;
            }
        }
        self.tables[table].failures = earlier;
        Ok(())
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
    fn open(&mut self, table: usize, governor: &mut Governor) -> Result<(), StopReason> {
        let _opened = governor.lease_scratch(
            size(self.out.opened.len())
                .saturating_mul(size(<(u64, usize) as ScratchCharge>::CHARGE)),
        )?;
        let mut opened: Vec<(u64, usize)> = self.out.opened.as_slice().to_vec();
        opened.sort_unstable();
        for (attempt, position) in opened {
            let attempts = self.tables[table].attempts.as_mut_slice();
            if let Some(record) = attempts.iter_mut().find(|record| record.id == attempt)
                && !record.opened
            {
                record.opened = true;
                self.events.push(Event::Opened {
                    attempt,
                    start: record.start,
                    at: position,
                });
            }
        }
        Ok(())
    }

    /// Record why (and where) an attempt left with no configuration failed.
    fn note_failure(&mut self, table: usize, attempt: u64, failures: &[Failure]) {
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
        record.failure = Some(first.reason);
        record.end = Some(end);
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
        let mut best: Option<Boundary> = None;
        for failure in failures.iter().filter(|failure| !failure.respell) {
            let mut node = failure.thread.hist;
            loop {
                governor.charge(Resource::Work, 1)?;
                let HistNode { prev, kind } = self.arena.nodes.as_slice()[node as usize];
                match kind {
                    HistKind::Root => break,
                    HistKind::Next(at, cost) => {
                        let candidate = Boundary {
                            attempt,
                            at,
                            hist: prev,
                            cost,
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
                    _ => node = prev,
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
            *known = best;
        } else {
            boundaries.push(best)?;
        }
        Ok(())
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
    ) -> Result<Option<Salvaged>, StopReason> {
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
            if let Some((segment, cost)) = parts.last_mut() {
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
        Ok(Some(Salvaged {
            at: boundary.at,
            charged: if segmentations.len() == 1 { charged } else { 0 },
            segmentations,
        }))
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
            self.release(table, record.id, governor)?;
            if record.opened {
                // A rejected span ends before the token that could not be
                // consumed; a completed attempt's last report comes after
                // all of its completions.
                let end = match (eof, record.completed) {
                    (Some(_), _) => at,
                    (None, true) => at + 1,
                    (None, false) => record.end.unwrap_or(at),
                };
                let salvage = self.salvage(table, &record, end, governor)?;
                self.events.push(Event::Ended {
                    table: index_u32(table),
                    attempt: record.id,
                    start: record.start,
                    at: end,
                    completed: record.completed,
                    reason: match (eof, record.failure) {
                        (_, Some(reason @ Rejection::InvalidUtf8 { .. }))
                        | (None, Some(reason)) => reason,
                        (Some(_), _) => Rejection::Unterminated,
                        (None, None) => Rejection::Overlapped { at },
                    },
                    salvage,
                });
            } else {
                self.take_boundary(table, record.id);
            }
            let attempts = self.tables[table].attempts.as_mut_slice();
            attempts.copy_within(index + 1.., index);
            self.tables[table].attempts.pop();
        }
        Ok(())
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
        let held = &mut self.tables[table].held;
        if !held.iter().any(|entry| entry.attempt == attempt) {
            return Ok(());
        }
        let (mine, rest): (Vec<Held>, Vec<Held>) = core::mem::take(held)
            .into_iter()
            .partition(|entry| entry.attempt == attempt);
        self.tables[table].held = rest;
        governor.charge(
            Resource::Work,
            size(mine.len()).saturating_mul(size(mine.len())),
        )?;
        let dominated = |entry: &Held| {
            mine.iter().any(|other| {
                let later = other.event.at() > entry.event.at();
                let more = other.costs.len() > entry.costs.len();
                if other.dangling {
                    // Cut off while writing a further call's arguments.
                    later && more && other.arguments
                } else if entry.dangling {
                    // A cut-off reading of no more calls than a completion
                    // reads, or one whose last call never began its
                    // arguments, or one that reaches no further.
                    !entry.arguments
                        || other.event.at() >= entry.event.at()
                        || (other.costs.len() >= entry.costs.len()
                            && other.prefix(entry.costs.len()) <= entry.prefix(entry.costs.len()))
                } else {
                    // The same calls read no worse, and further ones.
                    later
                        && more
                        && other.prefix(entry.costs.len()) <= entry.prefix(entry.costs.len())
                }
            })
        };
        let keep: Vec<bool> = mine.iter().map(|entry| !dominated(entry)).collect();
        for (entry, keep) in mine.into_iter().zip(keep) {
            if keep {
                self.events.push(entry.event);
            }
        }
        Ok(())
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
            (a.attempt, a.pos, a.cost, a.hist).cmp(&(b.attempt, b.pos, b.cost, b.hist))
        });
        let mut groups: Vec<Thread> = Vec::new();
        for thread in completed.as_slice() {
            match groups.last_mut() {
                Some(last) if last.attempt == thread.attempt && last.pos == thread.pos => {
                    if last.cost == thread.cost {
                        last.hist = self.arena.pack(last.hist, thread.hist)?;
                    }
                }
                _ => groups.push(*thread),
            }
        }
        let dangling = matches!(eof, Some(GenerationStop::Length));
        for group in groups {
            if eof.is_some() {
                self.emit(table, group, dangling, governor)?;
            } else {
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
            .flat_map(|class| class.spellings.iter().map(Vec::as_slice))
            .collect();
        let compared = size(openers.len() + closers.len()).saturating_mul(K_THREAD);
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
            if let Some(opener) = openers.iter().find(|opener| read.ends_with(opener)) {
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
        let held = self
            .tables
            .iter()
            .flat_map(|table| table.held.iter())
            .map(|entry| entry.event.at());
        let uncommitted = self
            .tables
            .iter()
            .filter(|table| table.program.deferred())
            .flat_map(|table| table.attempts.as_slice().iter())
            .filter(|record| !record.opened)
            .map(|record| record.start);
        trails.chain(held).chain(uncommitted).min()
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
        let kind = self.tables[table].kind;
        let dialect = self.grammars[self.tables[table].grammar].dialect.clone();
        let (step, _, _) = self.split(table);
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
                                parts.last().is_some_and(|(segment, _)| {
                                    segment.readings.iter().any(|reading| {
                                        reading.captures.iter().any(|capture| {
                                            matches!(
                                                capture.kind,
                                                CaptureKind::Arguments | CaptureKind::ParamName
                                            )
                                        })
                                    })
                                }),
                            ),
                            _ => (Vec::new(), false),
                        };
                        self.tables[table].held.push(Held {
                            attempt: thread.attempt,
                            event,
                            costs,
                            dangling,
                            arguments,
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
            keep(&mut table.failures, |failure| {
                failure.thread.attempt != attempt
            });
            keep(&mut table.boundaries, |known| known.attempt != attempt);
            table.trails.retain(|trail| trail.thread.attempt != attempt);
            table.held.retain(|entry| entry.attempt != attempt);
        }
        Ok(())
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
            self.tables[table].trails.clear();
        }
        Ok(())
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
        slice.sort_by_key(|thread| (thread.pos, thread.cost, thread.hist));
        let mut groups: Vec<Thread> = Vec::new();
        for thread in completed.as_slice() {
            match groups.last_mut() {
                Some(last) if last.pos == thread.pos && last.cost == thread.cost => {
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
            self.release(table, record.id, governor)?;
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
                self.events.push(Event::Ended {
                    table: index_u32(table),
                    attempt: record.id,
                    start: record.start,
                    at: self.fed,
                    completed: live.completed,
                    reason: match live.failure {
                        Some(reason @ Rejection::InvalidUtf8 { .. }) => reason,
                        _ => Rejection::Unterminated,
                    },
                    // A length stop keeps the cut-off reading instead.
                    salvage: None,
                });
            }
        }
        Ok(())
    }
}

/// The digest of a reading's edit sequence after one more edit.
pub(crate) fn extend_digest(digest: Digest, operator: EditOperator, at: usize) -> Digest {
    let mut builder = crate::digest::DigestBuilder::new();
    builder.update(digest.as_bytes());
    builder.update(&[operator as u8]);
    builder.update(&(at as u64).to_be_bytes());
    builder.finish()
}
