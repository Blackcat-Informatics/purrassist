// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Least-cost markup repair at failure configurations.
//!
//! When every configuration of a call attempt fails on a byte (or waits at a
//! literal when the input ends), the engine seeds a [`Search`] with those
//! configurations, rewound to the start of the literal they were matching.
//! Expanding a configuration applies each applicable [edit
//! operator](super::edit::EditOperator) and replays the bytes from the
//! literal's start to the current position through the ordinary
//! interpreter; a replay that consumes every byte (or completes the
//! production) is accepted, and one that fails again is a new failure
//! configuration to expand. The search explores in ascending
//! `(cost, tie, serial)` order, the tie being
//! `(offset, location, mode, node, status, entry, edit digest)`, and keeps
//! every accepted configuration of the least cost.
//!
//! Every explored configuration charges [`K_ERR`] `Work` on top of the
//! search's own per-candidate charges; repair chains are bounded by the
//! program length.
//!
//! [`Search`]: crate::search::Search
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); these items are
// shared across the grammar and tool-call modules, so `pub(crate)` (matching
// `unreachable_pub`) wins, as in `pattern::derive`.
#![allow(clippy::redundant_pub_crate)]
use super::automaton::ROOT;
use super::cost::{K_ERR, K_THREAD};
use super::edit::{EditOperator, MarkupEdit};
use super::engine::{Failure, Out, Step, Sub, Thread, extend_digest};
use super::{Inst, Program};
use crate::contract::{Evidence, Governor, Resource, ScratchVec, StopReason};
use crate::digest::Digest;
use crate::search::{Acceptance, Change, Proposals, SearchState};
use crate::tool_call::{GenerationStop, Rejection};
use core::marker::PhantomData;

/// Where a repair configuration stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Status {
    /// Failed mid-input; rewound to the start of its literal.
    Failed,
    /// Waiting at a literal at the end of input.
    Stuck,
    /// Consumed every byte up to the current position.
    Live,
    /// Completed its production.
    Completed,
}

/// One configuration of the repair search.
#[derive(Clone, Debug)]
pub(crate) struct RepairState<'a, 's> {
    pub(crate) thread: Thread,
    pub(crate) status: Status,
    digest: Digest,
    depth: u32,
    marker: PhantomData<fn() -> (&'a (), &'s ())>,
}

/// The interpreter and position a repair search replays through.
pub(crate) struct RepairContext<'a, 's> {
    pub(crate) step: Step<'a, 's>,
    pub(crate) until: usize,
    pub(crate) eof: Option<GenerationStop>,
    pub(crate) opened: bool,
}

impl<'a, 's> RepairState<'a, 's> {
    /// A seed for `failure`, when any operator can apply to it.
    pub(crate) fn seed(
        step: &Step<'a, 's>,
        failure: &Failure,
        eof: Option<&GenerationStop>,
        opened: bool,
    ) -> Option<Self> {
        let thread = failure.thread;
        let Inst::Lit { lit, .. } = step.program.insts[thread.pc as usize] else {
            return None;
        };
        let literal = &step.grammar.literals[lit as usize];
        let tables = &step.grammar.literal_tables[lit as usize];
        let make = |thread: Thread, status: Status| Self {
            thread,
            status,
            digest: Digest::of(b""),
            depth: 0,
            marker: PhantomData,
        };
        if let Some(stop) = eof {
            let Sub::Lit(node) = thread.sub else {
                return None;
            };
            let consumed = node == 0
                && matches!(stop, GenerationStop::StopSequence(sequence) if *sequence == literal.bytes);
            return (opened && (literal.closer.is_some() || consumed))
                .then(|| make(thread, Status::Stuck));
        }
        if matches!(failure.reason, Rejection::InvalidUtf8 { .. }) {
            return None;
        }
        let pending = step.bytes(thread.entry..failure.at + 1);
        let applies = (opened && (literal.closer.is_some() || step.grammar.stray.admits(pending)))
            || (!tables.variants.trie.is_empty() && tables.variants.trie.admits(pending))
            || (!tables.confusables.trie.is_empty() && tables.confusables.trie.admits(pending));
        let rewound = Thread {
            sub: Sub::Lit(0),
            pos: thread.entry,
            ..thread
        };
        applies.then(|| make(rewound, Status::Failed))
    }

    fn successor(&self, thread: Thread, status: Status, operator: EditOperator) -> Self {
        Self {
            thread,
            status,
            digest: extend_digest(self.digest, operator, self.thread.pos),
            depth: self.depth + 1,
            marker: PhantomData,
        }
    }

    /// Run the configurations on `seeds` (already moved by `operator`) to
    /// the current position and propose every outcome.
    fn replay(
        &self,
        context: &mut RepairContext<'a, 's>,
        proposals: &mut Proposals<'_, Self>,
        seeds: &mut ScratchVec<Thread>,
        operator: EditOperator,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut out = Out::new(governor);
        let eof = context.eof.is_some();
        context
            .step
            .advance(seeds, context.until, eof, &mut out, governor)?;
        for &thread in out.live.as_slice() {
            proposals.commit(
                governor,
                self.successor(thread, Status::Live, operator),
                change(operator),
            )?;
        }
        for &thread in out.completed.as_slice() {
            proposals.commit(
                governor,
                self.successor(thread, Status::Completed, operator),
                change(operator),
            )?;
        }
        for failure in out.failed.as_slice() {
            if let Some(next) =
                Self::seed(&context.step, failure, context.eof.as_ref(), context.opened)
            {
                proposals.commit(
                    governor,
                    self.successor(next.thread, next.status, operator),
                    change(operator),
                )?;
            }
        }
        Ok(())
    }

    /// Insert `replacement` (the expected literal, or its missing suffix)
    /// at this configuration's position, continue at `next`, and propose
    /// every outcome. With `complete == false` an insertion that completes
    /// the production at once is not proposed.
    #[allow(clippy::too_many_arguments)]
    fn insert(
        &self,
        context: &mut RepairContext<'a, 's>,
        proposals: &mut Proposals<'_, Self>,
        operator: EditOperator,
        replacement: Vec<u8>,
        next: u32,
        complete: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let at = self.thread.pos;
        let edit = MarkupEdit {
            operator,
            span: at..at,
            original: Vec::new(),
            replacement,
        };
        let hist = context.step.arena.edit(self.thread.hist, edit, governor)?;
        let mut seeds = ScratchVec::new(governor);
        let mut out = Out::new(governor);
        context.step.closure(
            Thread {
                hist,
                ..self.thread
            },
            next,
            at,
            &mut seeds,
            &mut out,
            governor,
        )?;
        // The inserted literal may complete the production at once.
        for &thread in out.completed.as_slice().iter().filter(|_| complete) {
            proposals.commit(
                governor,
                self.successor(thread, Status::Completed, operator),
                change(operator),
            )?;
        }
        self.replay(context, proposals, &mut seeds, operator, governor)
    }
}

/// Whether the production completes from `pc` without consuming input:
/// whether an inserted literal continuing at `pc` is the production's last.
fn completes(program: &Program, pc: u32, governor: &mut Governor) -> Result<bool, StopReason> {
    let mut visited = vec![false; program.insts.len()];
    let mut pending = vec![pc];
    while let Some(pc) = pending.pop() {
        let index = pc as usize;
        if core::mem::replace(&mut visited[index], true) {
            continue;
        }
        governor.charge(Resource::Work, K_THREAD)?;
        match program.insts[index] {
            Inst::Accept => return Ok(true),
            Inst::Split(first, second) => pending.extend([first, second]),
            Inst::Open { next, .. } | Inst::Close { next, .. } | Inst::Commit { next } => {
                pending.push(next);
            }
            Inst::Lit { .. } | Inst::Class { .. } | Inst::Until { .. } | Inst::Slot { .. } => {}
        }
    }
    Ok(false)
}

/// The search ledger entry of one operator application.
fn change(operator: EditOperator) -> Change {
    let rule = operator.rule();
    Change {
        evidence: Evidence {
            rule: rule.id.into(),
            explanation: String::new(),
            span: None,
            destination: String::new(),
        },
        cost: rule.cost,
        original: None,
        guess: None,
    }
}

impl<'a, 's: 'a> SearchState for RepairState<'a, 's> {
    type Tie = (usize, u32, u8, u32, Status, usize, Digest);
    type Context<'c> = RepairContext<'a, 's>;

    fn tie(&self) -> Self::Tie {
        let (mode, node) = self.thread.sub.key();
        (
            self.thread.pos,
            self.thread.pc,
            mode,
            node,
            self.status,
            self.thread.entry,
            self.digest,
        )
    }

    fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason> {
        let (mode, node) = self.thread.sub.key();
        key.push(self.status as u8)?;
        key.push(mode)?;
        key.extend((self.thread.pos as u64).to_be_bytes())?;
        key.extend(self.thread.pc.to_be_bytes())?;
        key.extend(node.to_be_bytes())?;
        key.extend((self.thread.entry as u64).to_be_bytes())?;
        key.extend(self.thread.attempt.to_be_bytes())?;
        key.extend(*self.digest.as_bytes())
    }

    fn accept(
        &self,
        _context: &mut RepairContext<'a, 's>,
        governor: &mut Governor,
    ) -> Result<Acceptance, StopReason> {
        governor.charge(Resource::Work, K_ERR)?;
        Ok(match self.status {
            Status::Live | Status::Completed => Acceptance::Accepted { validation: 0 },
            Status::Failed | Status::Stuck => Acceptance::Rejected,
        })
    }

    fn expand(
        &self,
        context: &mut RepairContext<'a, 's>,
        proposals: &mut Proposals<'_, Self>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let program = context.step.program;
        if self.depth as usize > program.insts.len() + 2 {
            return Ok(());
        }
        let Inst::Lit { lit, next } = program.insts[self.thread.pc as usize] else {
            return Ok(());
        };
        let grammar = context.step.grammar;
        let literal = &grammar.literals[lit as usize];
        let tables = &grammar.literal_tables[lit as usize];
        let at = self.thread.pos;
        match self.status {
            Status::Failed => {
                let pending = context.step.bytes(at..context.until);
                let modes = [
                    (
                        context.opened && grammar.stray.admits(pending),
                        Sub::Skip(ROOT),
                        EditOperator::DeleteStrayCloser,
                    ),
                    (
                        !tables.variants.trie.is_empty() && tables.variants.trie.admits(pending),
                        Sub::Variant(ROOT),
                        EditOperator::CloserVariant,
                    ),
                    (
                        !tables.confusables.trie.is_empty()
                            && tables.confusables.trie.admits(pending),
                        Sub::Confusable(ROOT),
                        EditOperator::TerminalConfusable,
                    ),
                ];
                if context.opened && literal.closer.is_some() {
                    let replacement = literal.bytes.clone();
                    // A closer missing mid-message: the wrapper's when it
                    // ends the production, an inner element's otherwise.
                    let wrapper = completes(program, next, governor)?;
                    let operator = if wrapper {
                        EditOperator::MissingWrapperClose
                    } else {
                        EditOperator::MissingFunctionClose
                    };
                    // Ending the call by inserting its last closer right
                    // before a closer spelling of this grammar would leave
                    // that closer behind as content: a repeated or stray
                    // closer belongs to the call, so only the deletion
                    // (`tool_call.markup.stray-closer`) reads it.
                    let complete = !(wrapper && grammar.stray.admits(pending));
                    self.insert(
                        context,
                        proposals,
                        operator,
                        replacement,
                        next,
                        complete,
                        governor,
                    )?;
                }
                for (applies, sub, operator) in modes {
                    if applies {
                        let mut seeds = ScratchVec::new(governor);
                        seeds.push(Thread { sub, ..self.thread })?;
                        self.replay(context, proposals, &mut seeds, operator, governor)?;
                    }
                }
            }
            Status::Stuck => {
                let Sub::Lit(node) = self.thread.sub else {
                    return Ok(());
                };
                let suffix = literal.bytes[node as usize..].to_vec();
                if node == 0
                    && matches!(&context.eof, Some(GenerationStop::StopSequence(sequence)) if *sequence == literal.bytes)
                {
                    let replacement = suffix.clone();
                    self.insert(
                        context,
                        proposals,
                        EditOperator::ConsumedCloser,
                        replacement,
                        next,
                        true,
                        governor,
                    )?;
                }
                if context.opened && literal.closer.is_some() {
                    self.insert(
                        context,
                        proposals,
                        EditOperator::MissingClosersAtEnd,
                        suffix,
                        next,
                        true,
                        governor,
                    )?;
                }
            }
            Status::Live | Status::Completed => {}
        }
        Ok(())
    }
}
