// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Reuse an unchanged, uncommitted ASCII class step after ordinary settlement.

use super::{Attempt, Engine, Event, FailureEnd, HistoryEvidence, Inst, Sub, Table};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason, invalid_input};
use crate::tool_call::ProductionKind;

/// The table's current settled vector already proves this exact self-loop.
/// Every mutator consumes the proof before changing that vector or its owner.
#[derive(Clone, Copy)]
pub(super) struct ClassRun {
    byte: u8,
    next_at: usize,
}

impl ScratchCharge for Option<ClassRun> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 32);
        32
    };
}

/// Initialize and retain a charged slot for every table, including empty slots.
pub(super) fn slots(
    count: usize,
    governor: &mut Governor,
) -> Result<ScratchVec<Option<ClassRun>>, StopReason> {
    governor.charge(Resource::Work, size(count))?;
    let mut slots = ScratchVec::new(governor);
    slots.reserve(count)?;
    slots.extend(core::iter::repeat_n(None, count))?;
    Ok(slots)
}

/// Immutable dispatch excludes tables that cannot own this kind of proof.
#[must_use]
pub(super) fn eligible(table: &Table<'_>) -> bool {
    table.program.argument_anchor
        && (!table.program.grouped || !table.program.initial_classes.is_empty())
        && table.kind == ProductionKind::Call
}

/// Separate fields let opener admission retain its borrowed grammar metadata.
pub(super) fn invalidate(
    slots: &mut ScratchVec<Option<ClassRun>>,
    occupied: &mut usize,
    table: usize,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    if *occupied == 0 {
        return Ok(());
    }
    governor.charge(Resource::Work, 1)?;
    if slots[table].is_some() {
        governor.charge(Resource::Work, 2)?;
        slots.as_mut_slice()[table] = None;
        *occupied -= 1;
    }
    Ok(())
}

impl Engine<'_> {
    /// Empty occupancy and immutable eligibility avoid unnecessary slot probes.
    pub(super) fn invalidate_class_run(
        &mut self,
        table: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if !eligible(&self.tables[table]) {
            return Ok(());
        }
        invalidate(
            &mut self.class_runs,
            &mut self.class_run_count,
            table,
            governor,
        )
    }

    /// External stops and arena retirement invalidate all table subjects.
    pub(super) fn clear_class_runs(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        if self.class_run_count == 0 {
            return Ok(());
        }
        governor.charge(
            Resource::Work,
            size(self.class_runs.len()).saturating_add(1),
        )?;
        self.class_runs.as_mut_slice().fill(None);
        self.class_run_count = 0;
        Ok(())
    }

    /// The proof owns no cache row or source reference. DFA terms are append-only
    /// and the relevant history suffix has no mutable deferred Link.
    pub(super) fn reuse_class_run(
        &mut self,
        table: usize,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        if self.class_run_count == 0 {
            return Ok(false);
        }
        governor.charge(Resource::Work, 1)?;
        let slot = &mut self.class_runs.as_mut_slice()[table];
        let Some(retained) = slot else {
            return Ok(false);
        };
        governor.charge(Resource::Work, 2)?;
        if retained.byte != byte || retained.next_at != at {
            // This lookup already established occupancy; do not probe again.
            governor.charge(Resource::Work, 2)?;
            *slot = None;
            self.class_run_count -= 1;
            return Ok(false);
        }
        // The ordinary queue would advance pos/reach and keep stall at zero.
        // No outgoing state was produced: the next VM use clears its own Out.
        // Admit both thread stores, next-position computation and cursor write
        // before advancing anything; retain the proof slot in place.
        governor.charge(
            Resource::Work,
            size(self.tables[table].threads.len())
                .saturating_mul(2)
                .saturating_add(2),
        )?;
        let next = at + 1;
        for thread in self.tables[table].threads.as_mut_slice() {
            thread.pos = next;
            thread.reach = next;
        }
        retained.next_at = next;
        Ok(true)
    }

    /// Certify only the actual settled state, using successful native DFA
    /// queries already performed by the ordinary production step.
    pub(super) fn remember_class_run(
        &mut self,
        table: usize,
        byte: u8,
        next_at: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        let current = &self.tables[table];
        if !byte.is_ascii() {
            return Ok(());
        }
        // Most class runs are still short or have just changed state. Refuse
        // those before inspecting side owners and immutable capture evidence.
        governor.charge(Resource::Work, 5)?;
        let Some(first) = current.threads.first() else {
            return Ok(());
        };
        if !matches!(first.sub, Sub::Class(_))
            || first.pos != next_at
            || first
                .pos
                .checked_sub(first.entry)
                .is_none_or(|length| length < 32)
        {
            return Ok(());
        }
        if current.program.grouped {
            governor.charge(Resource::Work, 1)?;
            if !current.program.initial_classes.contains(first.pc) {
                return Ok(());
            }
        }
        if !self.repeated_class_byte(byte, next_at, governor)? {
            return Ok(());
        }
        let Some(owner) = self.class_run_owner(table, governor)? else {
            return Ok(());
        };
        if self.class_run_queued(owner, governor)?
            || !self.class_run_threads(table, owner, byte, next_at, governor)?
        {
            return Ok(());
        }
        governor.charge(Resource::Work, 2)?;
        self.class_runs.as_mut_slice()[table] = Some(ClassRun { byte, next_at });
        self.class_run_count += 1;
        Ok(())
    }

    /// Eight written copies qualify a run without retaining a streak counter.
    /// A missing older optional suffix declines proof; the current input byte
    /// and its absolute window position remain required engine invariants.
    fn repeated_class_byte(
        &self,
        byte: u8,
        next_at: usize,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        // Current-position/subtraction/bounds checks and optional suffix bound.
        governor.charge(Resource::Work, 5)?;
        let Some(end) = next_at.checked_sub(self.base) else {
            return Err(invalid_input(
                "settled class position precedes its input window",
            ));
        };
        if next_at != self.fed || end == 0 || end != self.buffer.len() {
            return Err(invalid_input(
                "settled class position is outside its current input window",
            ));
        }
        governor.charge(Resource::Work, 1)?;
        if self.buffer[end - 1] != byte {
            return Err(invalid_input(
                "settled class byte differs from its current input",
            ));
        }
        let Some(start) = end.checked_sub(8) else {
            return Ok(false);
        };
        for &previous in self.buffer.as_slice()[start..end - 1].iter().rev() {
            governor.charge(Resource::Work, 1)?;
            if previous != byte {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// A pristine single attempt has no settled side work to repeat.
    fn class_run_owner(
        &self,
        table: usize,
        governor: &mut Governor,
    ) -> Result<Option<&Attempt>, StopReason> {
        governor.charge(Resource::Work, 24)?;
        let current = &self.tables[table];
        let [owner] = current.attempts.as_slice() else {
            return Ok(None);
        };
        if owner.opened
            || owner.completed
            || owner.stuck
            || owner.failure.is_some()
            || owner.end.is_some()
            || owner.failure_end != FailureEnd::Stop
            || owner.closed.is_some()
            || !current.trails.is_empty()
            || !current.held.by_attempt.is_empty()
            || current.held.floor.is_some()
            || !current.past.is_empty()
            || !current.written.is_empty()
            || !current.stalled.is_empty()
            || !current.failures.groups.is_empty()
            || !current.boundaries.is_empty()
            || !self.stack.is_empty()
            || !self.pending.is_empty()
            || !self.out.live.is_empty()
            || !self.out.failed.is_empty()
            || !self.out.completed.is_empty()
            || !self.out.opened.is_empty()
            || !self.out.headers.is_empty()
            || !self.out.native_frames.is_empty()
        {
            return Ok(None);
        }
        Ok(Some(owner))
    }

    /// Global queues may hold other tables' reports, but none for this owner.
    fn class_run_queued(
        &self,
        owner: &Attempt,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        // Global queues can contain another table's reports. The subject's
        // owner must have none; later changes for it require a table mutation,
        // which consumes the certificate before touching those queues.
        governor.charge(
            Resource::Work,
            size(
                self.events.len()
                    + self.deferred.len()
                    + self.resynced.len()
                    + self.quoted_openers.len()
                    + self.spawned.len()
                    + self.whole.len(),
            )
            .saturating_mul(2),
        )?;
        let owned_event = |event: &Event| match event {
            Event::Opened { attempt, .. }
            | Event::Completed { attempt, .. }
            | Event::Ended { attempt, .. } => *attempt == owner.id,
        };
        Ok(self.events.iter().any(owned_event)
            || self.deferred.iter().any(|(_, event)| owned_event(event))
            || self
                .resynced
                .iter()
                .any(|&(attempt, _, _)| attempt == owner.id)
            || self
                .quoted_openers
                .iter()
                .any(|&(attempt, _)| attempt == owner.id)
            || self.spawned.iter().any(|&(attempt, _)| attempt == owner.id)
            || self.whole.contains(&owner.start))
    }

    /// Settled keys remain unique, every DFA step is the same self-loop, and
    /// immutable history evidence proves commit false with no active codec.
    fn class_run_threads(
        &self,
        table: usize,
        owner: &Attempt,
        byte: u8,
        next_at: usize,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let current = &self.tables[table];
        let runtime = &self.grammars[current.grammar];
        let owner_id = owner.id;
        let owner_start = owner.start;
        let mut previous = None;
        for thread in &current.threads {
            // Full state equality twice, thread/owner guards, merge ordering,
            // name-follower lookup and immutable commit-evidence flags.
            governor.charge(Resource::Work, 40)?;
            let (Inst::Class { class, .. }, Sub::Class(state)) =
                (current.program.insts[thread.pc as usize], thread.sub)
            else {
                return Ok(false);
            };
            if current.program.grouped {
                governor.charge(Resource::Work, 1)?;
                if !current.program.initial_classes.contains(thread.pc) {
                    return Ok(false);
                }
            }
            if thread.attempt != owner_id
                || thread.start != owner_start
                || thread.opened
                || thread.past
                || thread.pending != 0
                || thread.stall != 0
                || thread.pos != next_at
                || thread.reach != next_at
                || thread
                    .pos
                    .checked_sub(thread.entry)
                    .is_none_or(|length| length < 32)
                || state.partial_bytes() != 0
                || previous.is_some_and(|key| key >= thread.key())
            {
                return Ok(false);
            }
            previous = Some(thread.key());
            let Some(cached) = runtime.class_steps[class as usize] else {
                return Ok(false);
            };
            if cached.byte != byte
                || cached.dead
                || cached.state != state
                || cached.stepped != state
            {
                return Ok(false);
            }
            if !current.program.name_follower_index.is_empty() {
                let row = current.program.name_follower_index[thread.pc as usize];
                if row != u32::MAX
                    && current.program.name_follower_bytes[row as usize + usize::from(byte >> 6)]
                        & (1 << (byte & 63))
                        != 0
                {
                    return Ok(false);
                }
            }
            let evidence = self.arena.nodes[thread.hist as usize].evidence;
            if evidence.has(HistoryEvidence::LINKED)
                || (evidence.has(HistoryEvidence::NAMED) && evidence.has(HistoryEvidence::VALUE))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
