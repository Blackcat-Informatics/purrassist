// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Reuse a certified ASCII byte step after ordinary native settlement.

use super::{Attempt, Engine, Event, FailureEnd, HistoryEvidence, Inst, Sub, Table, Thread};
use crate::contract::saturating_size as size;
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchLease, ScratchVec, StopReason, invalid_input,
};
use crate::grammar::automaton::ROOT;
use crate::grammar::codec::{CodecKind, CodecState};
use crate::tool_call::ProductionKind;

/// The table's current settled vector already proves this exact self-loop.
/// Every mutator consumes the proof before changing that vector or its owner.
/// Every successful byte fold visits the cohort once. Its logical pos/reach
/// is the engine's shared cursor at byte boundaries; a pre-byte consumer
/// supplies that byte's explicit old cursor before changing the subject.
#[derive(Clone, Copy)]
pub(super) struct RunProof {
    byte: u8,
}

impl ScratchCharge for Option<RunProof> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 32);
        // Slot capacity remains admitted during its optional cursor handoff
        // and consumption, including the transient proof/cursor carriers.
        assert!(core::mem::size_of::<(Self, Option<usize>, RunProof, usize)>() <= 32);
        32
    };
}

/// Initialize and retain a charged slot for every table, including empty slots.
pub(super) fn slots(
    count: usize,
    governor: &mut Governor,
) -> Result<ScratchVec<Option<RunProof>>, StopReason> {
    governor.charge(Resource::Work, size(count))?;
    let mut slots = ScratchVec::new(governor);
    slots.reserve(count)?;
    slots.extend(core::iter::repeat_n(None, count))?;
    Ok(slots)
}

/// Keep the original class proof's compiler qualification separate.
#[must_use]
fn class_eligible(table: &Table<'_>) -> bool {
    table.program.argument_anchor
        && (!table.program.grouped || !table.program.initial_classes.is_empty())
        && table.kind == ProductionKind::Call
}

/// Native bodies are qualified dynamically; class proofs retain their guards.
#[must_use]
pub(super) fn eligible(table: &Table<'_>) -> bool {
    class_eligible(table)
        || (table.kind == ProductionKind::Call
            && !table.program.origin.is_projected()
            && !table.program.grouped)
}

/// Qualification borrows all payloads; only bounded control records coexist.
const BODY_METADATA_BYTES: usize = 192;
const _: () = assert!(
    core::mem::size_of::<(
        ScratchLease,
        Option<(u32, u8, u32, u32, u64)>,
        Option<u32>,
        u32,
        Option<CodecKind>,
        usize,
        &'static Thread,
        &'static Attempt,
        Option<&'static CodecState>,
        Result<bool, StopReason>,
    )>() <= BODY_METADATA_BYTES
);

/// Separate fields let opener admission retain its borrowed grammar metadata.
pub(super) fn invalidate(
    slots: &mut ScratchVec<Option<RunProof>>,
    occupied: &mut usize,
    table: usize,
    threads: &mut ScratchVec<Thread>,
    cursor: Option<usize>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    if *occupied == 0 {
        return Ok(());
    }
    // One bounded occupancy/slot metadata validation and probe.
    governor.charge(Resource::Work, 1)?;
    if *occupied > slots.len() {
        return Err(invalid_input("byte run occupancy exceeds its slot count"));
    }
    let slot = slots
        .as_mut_slice()
        .get_mut(table)
        .ok_or_else(|| invalid_input("byte run table has no proof slot"))?;
    if slot.is_some() {
        governor.charge(Resource::Work, 1)?;
        let cursor = cursor.ok_or_else(|| {
            invalid_input("byte run reached native processing without its pre-byte cursor")
        })?;
        consume(slot, occupied, threads, cursor, governor)?;
    }
    Ok(())
}

/// The caller admits the metadata census before inspecting this subject.
fn store_work(threads: &[Thread], governor: &mut Governor) -> Result<u64, StopReason> {
    if threads.is_empty() {
        return Err(invalid_input("byte run proof has an empty subject"));
    }
    u64::try_from(threads.len())
        .ok()
        .and_then(|count| count.checked_mul(2))
        .ok_or_else(|| governor.stop_resource(Resource::Work))
}

/// One infallible store pass, reached only after complete Work admission.
fn materialize(threads: &mut [Thread], cursor: usize) {
    for thread in threads {
        thread.pos = cursor;
        thread.reach = cursor;
    }
}

/// A known occupied slot is consumed without a second cache probe.
fn consume(
    slot: &mut Option<RunProof>,
    occupied: &mut usize,
    threads: &mut ScratchVec<Thread>,
    cursor: usize,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    // Subject/count validation and fee arithmetic precede all mutation.
    governor.charge(Resource::Work, 2)?;
    if slot.is_none() {
        return Err(invalid_input("byte run consumption lost its proof"));
    }
    if *occupied == 0 {
        return Err(invalid_input("byte run consumption has zero occupancy"));
    }
    let work = store_work(threads, governor)?
        .checked_add(2)
        .ok_or_else(|| governor.stop_resource(Resource::Work))?;
    governor.charge(Resource::Work, work)?;
    materialize(threads.as_mut_slice(), cursor);
    *slot = None;
    *occupied -= 1;
    Ok(())
}

impl Engine<'_> {
    /// Empty occupancy and immutable eligibility avoid unnecessary slot probes.
    pub(super) fn invalidate_byte_run(
        &mut self,
        table: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if !eligible(&self.tables[table]) {
            return Ok(());
        }
        invalidate(
            &mut self.byte_runs,
            &mut self.byte_run_count,
            table,
            &mut self.tables[table].threads,
            None,
            governor,
        )
    }

    /// External stops and arena retirement invalidate all table subjects.
    pub(super) fn clear_byte_runs(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        if self.byte_run_count == 0 {
            return Ok(());
        }
        // One shape check, then one metadata census. No cohort is changed
        // until the complete second pass has also been admitted.
        governor.charge(Resource::Work, 1)?;
        let count = self.byte_runs.len();
        let occupied = self.byte_run_count;
        if count != self.tables.len() || occupied > count {
            return Err(invalid_input(
                "byte run slot/occupancy metadata is inconsistent",
            ));
        }
        let census = size(occupied)
            .checked_mul(3)
            .and_then(|work| work.checked_add(size(count)))
            .ok_or_else(|| governor.stop_resource(Resource::Work))?;
        governor.charge(Resource::Work, census)?;
        let mut seen = 0;
        let mut stores = 0_u64;
        for (slot, table) in self.byte_runs.iter().zip(&self.tables) {
            if slot.is_some() {
                if seen == occupied {
                    return Err(invalid_input("byte run occupancy omits a proof"));
                }
                stores = stores
                    .checked_add(store_work(&table.threads, governor)?)
                    .ok_or_else(|| governor.stop_resource(Resource::Work))?;
                seen = seen
                    .checked_add(1)
                    .ok_or_else(|| invalid_input("byte run proof count overflow"))?;
            }
        }
        if seen != occupied {
            return Err(invalid_input("byte run occupancy counts a missing proof"));
        }
        let mutation = size(count)
            .checked_add(stores)
            .and_then(|work| work.checked_add(size(occupied)))
            .and_then(|work| work.checked_add(1))
            .ok_or_else(|| governor.stop_resource(Resource::Work))?;
        governor.charge(Resource::Work, mutation)?;
        for (slot, table) in self
            .byte_runs
            .as_mut_slice()
            .iter_mut()
            .zip(&mut self.tables)
        {
            if slot.is_some() {
                // Every table completed the last successful byte fold before
                // this boundary operation can consume any retained cohort.
                materialize(table.threads.as_mut_slice(), self.fed);
                *slot = None;
            }
        }
        self.byte_run_count = 0;
        Ok(())
    }

    /// The proof owns no cache row or source reference. DFA terms are append-only
    /// and the relevant history suffix has no mutable deferred Link.
    pub(super) fn reuse_byte_run(
        &mut self,
        table: usize,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        if self.byte_run_count == 0 {
            return Ok(false);
        }
        // One bounded occupancy/slot metadata validation and probe.
        governor.charge(Resource::Work, 1)?;
        if self.byte_run_count > self.byte_runs.len() {
            return Err(invalid_input("byte run occupancy exceeds its slot count"));
        }
        let slot = &mut self.byte_runs.as_mut_slice()[table];
        let Some(retained) = slot else {
            return Ok(false);
        };
        governor.charge(Resource::Work, 1)?;
        if retained.byte != byte {
            // step_table moves threads before split: restore them here.
            consume(
                slot,
                &mut self.byte_run_count,
                &mut self.tables[table].threads,
                at,
                governor,
            )?;
            return Ok(false);
        }
        // The byte fold advances every surviving cohort exactly once. No
        // per-table cursor write is needed: boundary consumption uses fed,
        // while a pre-byte miss above supplies at before native processing.
        Ok(true)
    }

    /// Certify the actual settled cohort through its native classifiers.
    /// Class and body proofs retain their separate qualification guards.
    pub(super) fn remember_byte_run(
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
        if !class_eligible(current) {
            return self.remember_body_run(table, byte, next_at, governor);
        }
        // Most byte runs are still short or have just changed state. Refuse
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
        if !self.repeated_byte(byte, next_at, governor)? {
            return Ok(());
        }
        let Some(owner) = self.byte_run_owner(table, governor)? else {
            return Ok(());
        };
        if self.byte_run_queued(owner, governor)?
            || !self.byte_run_threads(table, owner, byte, next_at, governor)?
        {
            return Ok(());
        }
        governor.charge(Resource::Work, 2)?;
        self.byte_runs.as_mut_slice()[table] = Some(RunProof { byte });
        self.byte_run_count += 1;
        Ok(())
    }

    /// A non-incremental codec observes its retained source only at body end.
    /// Every owner and thread stays native; this skips only a proved identity.
    fn remember_body_run(
        &mut self,
        table: usize,
        byte: u8,
        next_at: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let current = &self.tables[table];
        let Some(first) = current.threads.first() else {
            return Ok(());
        };
        if !matches!(first.sub, Sub::Body(..)) {
            return Ok(());
        }
        governor.charge(Resource::Work, 2)?;
        if !matches!(
            current.program.insts[first.pc as usize],
            Inst::Until { .. }
                | Inst::Slot {
                    codec: CodecKind::Raw | CodecKind::XmlParams | CodecKind::KvPairs,
                    ..
                }
        ) {
            return Ok(());
        }
        governor.charge(Resource::Work, 5)?;
        if first.pos != next_at
            || first
                .pos
                .checked_sub(first.entry)
                .is_none_or(|length| length < 32)
            || !self.repeated_byte(byte, next_at, governor)?
        {
            return Ok(());
        }
        let _metadata = governor.lease_scratch(size(BODY_METADATA_BYTES))?;
        if !self.body_run_owners(table, governor)?
            || !self.body_run_threads(table, byte, next_at, governor)?
        {
            return Ok(());
        }
        governor.charge(Resource::Work, 2)?;
        self.byte_runs.as_mut_slice()[table] = Some(RunProof { byte });
        self.byte_run_count += 1;
        Ok(())
    }

    /// Every pristine opened owner has a surviving body, with no suffix work.
    fn body_run_owners(&self, table: usize, governor: &mut Governor) -> Result<bool, StopReason> {
        let current = &self.tables[table];
        governor.charge(Resource::Work, 24)?;
        if current.attempts.is_empty()
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
            return Ok(false);
        }
        for owner in &current.attempts {
            governor.charge(
                Resource::Work,
                size(current.threads.len())
                    .saturating_mul(2)
                    .saturating_add(10),
            )?;
            if !owner.opened
                || owner.completed
                || owner.stuck
                || owner.failure.is_some()
                || owner.end.is_some()
                || owner.failure_end != FailureEnd::Stop
                || owner.closed.is_some()
                || !current
                    .threads
                    .iter()
                    .any(|thread| thread.attempt == owner.id && thread.start == owner.start)
                || self.byte_run_queued(owner, governor)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The actual native step changes only the subject's logical cursor.
    fn body_run_threads(
        &self,
        table: usize,
        byte: u8,
        next_at: usize,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let current = &self.tables[table];
        let grammar = self.grammars[current.grammar].grammar;
        let mut previous = None;
        for thread in &current.threads {
            governor.charge(Resource::Work, 32)?;
            let Sub::Body(raw, slot) = thread.sub else {
                return Ok(false);
            };
            if raw != ROOT
                || thread.ownership != 2
                || thread.past
                || thread.pending != 0
                || thread.stall != 0
                || thread.pos != next_at
                || thread.reach != next_at
                || thread
                    .pos
                    .checked_sub(thread.entry)
                    .is_none_or(|length| length < 32)
                || previous.is_some_and(|key| key >= thread.key())
                || self.arena.nodes[thread.hist as usize]
                    .evidence
                    .has(HistoryEvidence::LINKED)
            {
                return Ok(false);
            }
            previous = Some(thread.key());
            governor.charge(
                Resource::Work,
                size(current.attempts.len()).saturating_mul(2),
            )?;
            if !current
                .attempts
                .iter()
                .any(|owner| owner.id == thread.attempt && owner.start == thread.start)
            {
                return Err(invalid_input("byte run body has no matching native owner"));
            }
            let (set, expected) = match current.program.insts[thread.pc as usize] {
                Inst::Until { set, .. } => (set, None),
                Inst::Slot { set, codec, .. } => (set, Some(codec)),
                _ => return Err(invalid_input("byte run body has no body instruction")),
            };
            if !self.body_run_codec(slot, expected)? {
                return Ok(false);
            }
            let delimiters = &grammar.delimiters[set as usize];
            governor.charge(
                Resource::Work,
                size(delimiters.declared.len()).saturating_add(3),
            )?;
            if delimiters.matcher.step(ROOT, byte) != ROOT
                || delimiters.matcher.found(ROOT).is_some()
                || delimiters
                    .declared
                    .iter()
                    .any(|token| token.last() == Some(&byte))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Refuse incremental codecs; an absent or mismatched slot is an invariant.
    fn body_run_codec(
        &self,
        slot: Option<u32>,
        expected: Option<CodecKind>,
    ) -> Result<bool, StopReason> {
        match (slot, expected) {
            (None, None) => Ok(true),
            (Some(slot), Some(expected)) => {
                let actual = self
                    .arena
                    .codecs
                    .get(slot as usize)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| invalid_input("byte run body lost its codec slot"))?;
                if actual.kind() != expected {
                    return Err(invalid_input(
                        "byte run body codec differs from its instruction",
                    ));
                }
                Ok(matches!(
                    expected,
                    CodecKind::Raw | CodecKind::XmlParams | CodecKind::KvPairs
                ))
            }
            _ => Err(invalid_input(
                "byte run body codec presence differs from its instruction",
            )),
        }
    }

    /// Eight written copies qualify a run without retaining a streak counter.
    /// A missing older optional suffix declines proof; the current input byte
    /// and its absolute window position remain required engine invariants.
    fn repeated_byte(
        &self,
        byte: u8,
        next_at: usize,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        // Current-position/subtraction/bounds checks and optional suffix bound.
        governor.charge(Resource::Work, 5)?;
        let Some(end) = next_at.checked_sub(self.base) else {
            return Err(invalid_input(
                "settled byte position precedes its input window",
            ));
        };
        if next_at != self.fed || end == 0 || end != self.buffer.len() {
            return Err(invalid_input(
                "settled byte position is outside its current input window",
            ));
        }
        governor.charge(Resource::Work, 1)?;
        if self.buffer[end - 1] != byte {
            return Err(invalid_input("settled byte differs from its current input"));
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
    fn byte_run_owner(
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
    fn byte_run_queued(
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
    fn byte_run_threads(
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
