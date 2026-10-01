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
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); these items are
// shared across the grammar and tool-call modules, so `pub(crate)` (matching
// `unreachable_pub`) wins, as in `pattern::derive`.
#![allow(clippy::redundant_pub_crate)]
use super::codec::{
    CodecContext, CodecEnd, CodecFault, CodecKind, CodecState, CodecStep, CodecValue,
};
use super::cost::{K_CAPTURE, K_CODEC, K_THREAD, STRAY_CLOSER_GAP};
use super::edit::{EditOperator, MarkupEdit};
use super::repair::{RepairContext, RepairState, Status};
use super::{CaptureKind, CompiledGrammar, Inst, Program, index_u32};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason};
use crate::digest::Digest;
use crate::pattern::{DerivativeDfa, DfaState};
use crate::search::Ledger;
use crate::search::Search;
use crate::tool_call::{
    ArgTree, CallSegment, CaptureSpan, Closure, CompiledTool, DialectId, GenerationStop,
    ProductionKind, RawCall, Rejection, Spanned, ToolSet,
};
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
    /// Inside a class run.
    Class(DfaState),
    /// Inside an `until`/`slot` body: matcher state and, for a slot, the
    /// arena slot of its codec's state.
    Body(u32, Option<u32>),
}

impl Sub {
    /// The same literal mode at trie node `node`.
    const fn with_node(self, node: u32) -> Self {
        match self {
            Self::Variant(_) => Self::Variant(node),
            Self::Confusable(_) => Self::Confusable(node),
            Self::Skip(_) => Self::Skip(node),
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

    /// The precedence among configurations sharing a key.
    const fn rank(&self) -> (usize, u64, usize) {
        (self.start, self.cost, self.entry)
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
    charged: u64,
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
    pub(crate) pending: &'a mut Vec<(u32, u32)>,
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
        out.failed.push(Failure { thread, at, reason })
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
            Inst::Split(..) | Inst::Open { .. } | Inst::Close { .. } | Inst::Accept => {
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
                HistKind::Root => break,
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
        if !thread.opened && self.program.openers.contains(&thread.pc) {
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
        self.pending.push((pc, thread.hist));
        while let Some((pc, hist)) = self.pending.pop() {
            let (word, bit) = (pc as usize / 64, 1_u64 << (pc % 64));
            if self.visited[word] & bit != 0 {
                continue;
            }
            self.visited[word] |= bit;
            governor.charge(Resource::Work, K_THREAD)?;
            let sub = match self.program.insts[pc as usize] {
                Inst::Split(first, second) => {
                    self.pending.push((second, hist));
                    self.pending.push((first, hist));
                    continue;
                }
                Inst::Open { kind, next } => {
                    let node = self.arena.push(hist, HistKind::Open(kind, at))?;
                    self.pending.push((next, node));
                    continue;
                }
                Inst::Close { kind, next } => {
                    let node = self.arena.push(hist, HistKind::Close(kind, at))?;
                    self.pending.push((next, node));
                    continue;
                }
                Inst::Accept => {
                    out.completed.push(Thread {
                        pc,
                        hist,
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

    /// The history (and added cost) of a configuration cut off by a length
    /// stop: an open slot keeps the complete part of its body as its value.
    fn truncate(
        &mut self,
        thread: Thread,
        governor: &mut Governor,
    ) -> Result<(u32, u64), StopReason> {
        let (Inst::Slot { set, .. }, Sub::Body(state, Some(slot))) =
            (self.program.insts[thread.pc as usize], thread.sub)
        else {
            return Ok((thread.hist, 0));
        };
        let matcher = &self.grammar.delimiters[set as usize].matcher;
        let released = thread.pos - matcher.depth(state) as usize;
        let end = match self.feed(slot, released..thread.pos, governor)? {
            Ok(Some(complete)) => complete,
            Ok(None) => thread.pos,
            Err(_) => return Ok((thread.hist, 0)),
        };
        Ok(self
            .finish_codec(
                slot,
                thread.hist,
                thread.entry..end,
                CodecEnd::Truncated,
                governor,
            )?
            .unwrap_or((thread.hist, 0)))
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

    fn text(
        &self,
        span: Range<usize>,
        governor: &mut Governor,
    ) -> Result<Result<String, Rejection>, StopReason> {
        governor.charge(Resource::Work, size(span.len()).saturating_mul(K_CAPTURE))?;
        governor.charge(Resource::ScratchBytes, size(span.len()).saturating_add(24))?;
        let bytes = self.bytes(span.clone());
        Ok(match core::str::from_utf8(bytes) {
            Ok(text) => Ok(text.to_owned()),
            Err(error) => Err(Rejection::InvalidUtf8 {
                at: span.start + error.valid_up_to(),
            }),
        })
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
                HistKind::Root | HistKind::Pack(_) => {}
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
        };
        let mut parameters: Vec<(Spanned<String>, ArgTree)> = Vec::new();
        let mut pending_name: Option<Spanned<String>> = None;
        for (capture, value) in captures {
            match capture.kind {
                CaptureKind::Name | CaptureKind::Id | CaptureKind::ParamName => {
                    let text = match self.text(capture.span.clone(), governor)? {
                        Ok(text) => text,
                        Err(rejection) => return Ok(Err(rejection)),
                    };
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
        let fields = match self.fields(&captures, governor)? {
            Ok(fields) => fields,
            Err(rejection) => return Ok(Err(rejection)),
        };
        let mut cost = shape.cost;
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
        Ok(Ok(RawCall {
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
    pub(crate) fn segment(
        &self,
        hist: u32,
        shape: &Shape<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CallSegment, Rejection>, StopReason> {
        let mut readings: Vec<RawCall> = Vec::new();
        let mut rejection = None;
        for path in self.histories(hist, governor)? {
            match self.reading(&path, shape, governor)? {
                Ok(call) => {
                    if !readings.contains(&call) {
                        // Every reading beyond the first competes.
                        if !readings.is_empty() {
                            governor.charge(Resource::Candidates, 1)?;
                        }
                        readings.push(call);
                    }
                }
                Err(reason) => {
                    rejection.get_or_insert(reason);
                }
            }
        }
        if readings.is_empty() {
            return Ok(Err(
                rejection.unwrap_or(Rejection::Malformed { at: shape.span.end })
            ));
        }
        Ok(Ok(CallSegment {
            span: shape.span.clone(),
            readings,
            derivations: Vec::new(),
        }))
    }
}

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
/// cost, then the earliest entry wins; exact ties pack their histories.
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
    /// Attempt `attempt` matched `segment`, ending at `at`.
    Completed {
        table: u32,
        attempt: u64,
        at: usize,
        segment: CallSegment,
        cost: u64,
    },
    /// Attempt `attempt` has no configuration left at `at`.
    Ended {
        table: u32,
        attempt: u64,
        start: usize,
        at: usize,
        completed: bool,
        reason: Rejection,
    },
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
    pending: Vec<(u32, u32)>,
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
            for &pc in &program.openers {
                self.tables[table].threads.push(Thread {
                    pc,
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
                })?;
            }
        }
        if self.tables[table].threads.is_empty() && self.tables[table].trails.is_empty() {
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
            let _failures = governor.lease_scratch(
                size(failed.as_slice().iter().filter(mine).count())
                    .saturating_mul(size(<Failure as ScratchCharge>::CHARGE)),
            )?;
            let failures: Vec<Failure> = failed.as_slice().iter().filter(mine).copied().collect();
            self.note_failure(table, record.id, &failures);
            let until = if eof.is_some() { at } else { at + 1 };
            for state in self.repair(table, record, &failures, until, eof, governor)? {
                match state.status {
                    Status::Live => live.push(state.thread)?,
                    Status::Completed => completed.push(state.thread)?,
                    Status::Failed | Status::Stuck => {}
                }
            }
        }
        merge(&mut live, &mut self.arena, governor)?;
        self.complete(table, &mut completed, eof, governor)?;
        self.retire(table, &live, at, eof);
        self.tables[table].threads = live;
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
        let Some(first) = failures
            .iter()
            .min_by_key(|failure| (failure.at, failure.reason))
        else {
            return;
        };
        // The rejected span ends where the furthest failing token began: a
        // partial literal may start the next opener.
        let program = self.tables[table].program;
        let end = failures
            .iter()
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

    /// Retire the attempts `live` no longer holds a configuration of.
    fn retire(
        &mut self,
        table: usize,
        live: &ScratchVec<Thread>,
        at: usize,
        eof: Option<&GenerationStop>,
    ) {
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
            if record.opened {
                self.events.push(Event::Ended {
                    table: index_u32(table),
                    attempt: record.id,
                    start: record.start,
                    // A rejected span ends before the token that could not
                    // be consumed; a completed attempt's last report comes
                    // after all of its completions.
                    at: match (eof, record.completed) {
                        (Some(_), _) => at,
                        (None, true) => at + 1,
                        (None, false) => record.end.unwrap_or(at),
                    },
                    completed: record.completed,
                    reason: match (eof, record.failure) {
                        (_, Some(reason @ Rejection::InvalidUtf8 { .. }))
                        | (None, Some(reason)) => reason,
                        (Some(_), _) => Rejection::Unterminated,
                        (None, None) => Rejection::Overlapped { at },
                    },
                });
            }
            let attempts = self.tables[table].attempts.as_mut_slice();
            attempts.copy_within(index + 1.., index);
            self.tables[table].attempts.pop();
        }
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
    /// being decided ([`Self::scan_trail`]): the turn layer settles no
    /// report at or after it until the decision is made.
    pub(crate) fn held_floor(&self) -> Option<usize> {
        self.tables
            .iter()
            .flat_map(|table| table.trails.iter())
            .filter(|trail| trail.scan.is_some())
            .map(|trail| trail.thread.pos)
            .min()
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
        let segment = step.segment(thread.hist, &shape, governor)?;
        let attempts = self.tables[table].attempts.as_mut_slice();
        let record = attempts
            .iter_mut()
            .find(|record| record.id == thread.attempt)
            .expect("completing attempt is live");
        match segment {
            Ok(segment) => {
                record.completed = true;
                governor.charge(Resource::Work, super::cost::K_EVENT)?;
                self.events.push(Event::Completed {
                    table: index_u32(table),
                    attempt: thread.attempt,
                    at: thread.pos,
                    segment,
                    cost: thread.cost,
                });
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
            self.tables[table].trails.clear();
        }
        Ok(())
    }

    /// A length stop: every open attempt becomes a cut-off reading of its
    /// least-cost configurations.
    fn cut(&mut self, table: usize, governor: &mut Governor) -> Result<(), StopReason> {
        let records: Vec<Attempt> = self.tables[table].attempts.as_slice().to_vec();
        let threads: Vec<Thread> = self.tables[table].threads.as_slice().to_vec();
        for record in records {
            if record.opened && !record.completed {
                let mut best: Option<Thread> = None;
                for thread in threads.iter().filter(|thread| thread.attempt == record.id) {
                    let (mut step, _, _) = self.split(table);
                    let (hist, added) = step.truncate(*thread, governor)?;
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
                if let Some(thread) = best {
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
