// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Transactional, identity-indexed minima for reports and their candidate subset.
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason, invalid_input};

const NONE: usize = usize::MAX;
// Source/domain or removal identity (2), arena/free-row access (2), checked
// planning (1), row/carrier handoff (1), row/free publication (2), projected
// lane/minimum consistency (1), and publishing the exact floor pair (1).
const UPDATE_WORK: u64 = 10;
// Length/reservation planning, compact plan handoff, final incoming ID placement,
// and its row-position publication. Removal instead pays for endpoint identity,
// tail pop and clearing the removed position, plus separate replacement stores.
const INSERT_WORK: u64 = 4;
const REMOVE_WORK: u64 = 5;

/// A queue handle; its arrival identity is checked before every removal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SourceId(usize);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    source: usize,
    arrival: u64,
}

#[derive(Clone, Copy)]
struct Row {
    key: Key,
    positions: [usize; 2],
}

impl Row {
    const fn free(next: usize) -> Self {
        Self {
            key: Key {
                source: next,
                arrival: 0,
            },
            positions: [NONE; 2],
        }
    }
}

impl ScratchCharge for Row {
    // The live row and the opaque handle beside its sole queued Pending.
    const CHARGE: usize = 40;
}
const _: () =
    assert!(core::mem::size_of::<Row>() + core::mem::size_of::<SourceId>() <= Row::CHARGE);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Move {
    Inactive,
    Insert,
    Up,
    Down,
    Tail,
}

/// Endpoints determine the unique binary-heap path; no path array is retained.
struct LanePlan {
    len: usize,
    hole: usize,
    target: usize,
    replacement: SourceId,
    shift: u32,
    movement: Move,
}

impl LanePlan {
    const fn inactive(len: usize) -> Self {
        Self {
            len,
            hole: NONE,
            target: NONE,
            replacement: SourceId(NONE),
            shift: 0,
            movement: Move::Inactive,
        }
    }

    const fn after_len(&self) -> usize {
        match self.movement {
            Move::Inactive => self.len,
            Move::Insert => self.len + 1,
            Move::Up | Move::Down | Move::Tail => self.len - 1,
        }
    }
}

/// Actual simultaneous preparation scratch, included in the metadata lease.
pub(super) struct PreparationCursor {
    key: Key,
    selected: Key,
    row: Row,
    id: SourceId,
    hole: usize,
    selected_position: usize,
    parent: usize,
    left: usize,
    right: usize,
    length: usize,
    begin: usize,
    end: usize,
    last: usize,
}

impl PreparationCursor {
    const fn new(key: Key) -> Self {
        Self {
            key,
            selected: key,
            row: Row::free(NONE),
            id: SourceId(NONE),
            hole: 0,
            selected_position: 0,
            parent: 0,
            left: 0,
            right: 0,
            length: 0,
            begin: 0,
            end: 0,
            last: 0,
        }
    }
}

/// Actual checked replay coordinates, included alongside preparation metadata.
pub(super) struct ReplayCursor {
    hole: usize,
    next: usize,
    id: usize,
    shift: u32,
}

/// Both exact sift plans and all fallible admission precede queue mutation.
pub(super) struct SourceUpdate {
    id: SourceId,
    row: Row,
    next_free: usize,
    plans: [LanePlan; 2],
    floors: [Option<usize>; 2],
    adding: bool,
}

impl SourceUpdate {
    pub(super) const fn handle(&self) -> SourceId {
        self.id
    }

    /// Replay checked ID moves and backlinks without allocation or admission.
    pub(super) fn apply(self, sources: &mut PendingSources) {
        if self.adding {
            if self.id.0 == sources.arena.len() {
                sources
                    .arena
                    .push(self.row)
                    .expect("arena capacity prepared");
            } else {
                sources.arena.as_mut_slice()[self.id.0] = self.row;
            }
        }
        for (lane, plan) in self.plans.iter().enumerate() {
            sources.apply_lane(lane, plan);
        }
        if !self.adding {
            sources.arena.as_mut_slice()[self.id.0] = self.row;
        }
        sources.free_head = self.next_free;
        sources.floors = self.floors;
    }
}

/// Reusable identities, with one mandatory and one candidate min-heap.
pub(super) struct PendingSources {
    arena: ScratchVec<Row>,
    heaps: [ScratchVec<usize>; 2],
    free_head: usize,
    floors: [Option<usize>; 2],
}

impl PendingSources {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            arena: ScratchVec::new(governor),
            heaps: core::array::from_fn(|_| ScratchVec::new(governor)),
            free_head: NONE,
            floors: [None; 2],
        }
    }

    pub(super) const fn floor(&self) -> Option<usize> {
        self.floors[0]
    }

    pub(super) const fn candidate_floor(&self) -> Option<usize> {
        self.floors[1]
    }

    fn reserve_one<T: ScratchCharge>(
        values: &mut ScratchVec<T>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if values.len() == values.capacity() {
            governor.charge(Resource::Work, size(values.len()))?;
        }
        values.reserve(1)
    }

    fn validate_floors(
        lengths: &[usize; 2],
        floors: &[Option<usize>; 2],
    ) -> Result<(), StopReason> {
        if lengths[1] > lengths[0]
            || (lengths[0] == 0) != floors[0].is_none()
            || (lengths[1] == 0) != floors[1].is_none()
            || (floors[1].is_some() && floors[0].is_none())
            || matches!((&floors[0], &floors[1]), (Some(report), Some(candidate)) if report > candidate)
        {
            return Err(invalid_input(
                "pending heap minima or candidate subset disagree",
            ));
        }
        Ok(())
    }

    fn record(
        &self,
        id: SourceId,
        candidate: bool,
        cursor: &mut PreparationCursor,
    ) -> Result<(), StopReason> {
        cursor.row = self
            .arena
            .as_slice()
            .get(id.0)
            .copied()
            .ok_or_else(|| invalid_input("queued source handle has no arena row"))?;
        if cursor.key.arrival == 0
            || cursor.row.key != cursor.key
            || cursor.row.positions[0] == NONE
            || (cursor.row.positions[1] != NONE) != candidate
        {
            return Err(invalid_input(
                "queued source handle identity or candidate membership changed",
            ));
        }
        Ok(())
    }

    fn endpoint(&self, lane: usize, position: usize, id: SourceId) -> Result<(), StopReason> {
        if self.heaps[lane].as_slice().get(position) != Some(&id.0) {
            return Err(invalid_input(
                "pending heap endpoint has a different source handle",
            ));
        }
        Ok(())
    }

    /// Admit a checked key/backlink read, plus a candidate's joint endpoint.
    fn read(
        &self,
        lane: usize,
        position: usize,
        cursor: &mut PreparationCursor,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        cursor.id = SourceId(*self.heaps[lane].as_slice().get(position).ok_or_else(|| {
            invalid_input("pending heap key position is outside its live length")
        })?);
        cursor.row = *self
            .arena
            .as_slice()
            .get(cursor.id.0)
            .ok_or_else(|| invalid_input("pending heap key has no source row"))?;
        if cursor.row.key.arrival == 0 || cursor.row.positions[lane] != position {
            return Err(invalid_input(
                "pending heap key has a stale source backlink",
            ));
        }
        if lane == 1 {
            governor.charge(Resource::Work, 1)?;
            self.endpoint(0, cursor.row.positions[0], cursor.id)?;
        }
        Ok(())
    }

    fn ascend(
        &self,
        lane: usize,
        plan: &mut LanePlan,
        floor: &mut Option<usize>,
        cursor: &mut PreparationCursor,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        cursor.hole = plan.hole;
        while cursor.hole > 0 {
            cursor.parent = (cursor.hole - 1) / 2;
            self.read(lane, cursor.parent, cursor, governor)?;
            governor.charge(Resource::Work, 1)?;
            if cursor.key >= cursor.row.key {
                break;
            }
            governor.charge(Resource::Work, 2)?;
            cursor.hole = cursor.parent;
        }
        plan.target = cursor.hole;
        if plan.target == 0 {
            *floor = Some(cursor.key.source);
        }
        Ok(())
    }

    fn descend(
        &self,
        lane: usize,
        plan: &mut LanePlan,
        floor: &mut Option<usize>,
        cursor: &mut PreparationCursor,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        cursor.hole = plan.hole;
        cursor.length = plan.len - 1;
        while cursor.hole < cursor.length / 2 {
            cursor.left = cursor
                .hole
                .checked_mul(2)
                .and_then(|at| at.checked_add(1))
                .ok_or_else(|| invalid_input("pending heap child exceeds its address domain"))?;
            cursor.right = cursor
                .left
                .checked_add(1)
                .ok_or_else(|| invalid_input("pending heap sibling exceeds its address domain"))?;
            self.read(lane, cursor.left, cursor, governor)?;
            cursor.selected = cursor.row.key;
            cursor.selected_position = cursor.left;
            if cursor.right < cursor.length {
                self.read(lane, cursor.right, cursor, governor)?;
                governor.charge(Resource::Work, 1)?;
                if cursor.row.key < cursor.selected {
                    cursor.selected = cursor.row.key;
                    cursor.selected_position = cursor.right;
                }
            }
            governor.charge(Resource::Work, 1)?;
            if cursor.selected >= cursor.key {
                break;
            }
            governor.charge(Resource::Work, 2)?;
            if cursor.hole == 0 {
                *floor = Some(cursor.selected.source);
            }
            cursor.hole = cursor.selected_position;
        }
        plan.target = cursor.hole;
        cursor.begin = plan
            .hole
            .checked_add(1)
            .ok_or_else(|| invalid_input("pending heap start exceeds its address domain"))?;
        cursor.end = plan
            .target
            .checked_add(1)
            .ok_or_else(|| invalid_input("pending heap target exceeds its address domain"))?;
        plan.shift = cursor
            .begin
            .leading_zeros()
            .checked_sub(cursor.end.leading_zeros())
            .ok_or_else(|| invalid_input("pending heap target precedes its starting depth"))?;
        if cursor.end >> plan.shift != cursor.begin {
            return Err(invalid_input("pending heap target is not a descendant"));
        }
        Ok(())
    }

    fn remove_plan(
        &self,
        lane: usize,
        plan: &mut LanePlan,
        floor: &mut Option<usize>,
        cursor: &mut PreparationCursor,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        cursor.last = plan
            .len
            .checked_sub(1)
            .ok_or_else(|| invalid_input("pending source removal has no live heap"))?;
        plan.movement = Move::Tail;
        plan.target = plan.hole;
        if cursor.last == 0 {
            *floor = None;
        }
        if plan.hole == cursor.last {
            return Ok(());
        }
        self.read(lane, cursor.last, cursor, governor)?;
        cursor.key = cursor.row.key;
        plan.replacement = cursor.id;
        // Final replacement stores are distinct from intermediate ID shifts.
        governor.charge(Resource::Work, 2)?;
        self.ascend(lane, plan, floor, cursor, governor)?;
        if plan.target == plan.hole {
            plan.movement = Move::Down;
            self.descend(lane, plan, floor, cursor, governor)?;
        } else {
            plan.movement = Move::Up;
        }
        Ok(())
    }

    pub(super) fn prepare_add(
        &mut self,
        source: usize,
        arrival: u64,
        candidate: bool,
        fed: usize,
        governor: &mut Governor,
    ) -> Result<SourceUpdate, StopReason> {
        governor.charge(Resource::Work, UPDATE_WORK)?;
        if source > fed || u64::try_from(source).is_err() || arrival == 0 {
            return Err(invalid_input(
                "queued source origin or arrival exceeds its admitted domain",
            ));
        }
        let mut update = SourceUpdate {
            id: SourceId(self.free_head),
            row: Row {
                key: Key { source, arrival },
                positions: [NONE; 2],
            },
            next_free: NONE,
            plans: core::array::from_fn(|lane| LanePlan::inactive(self.heaps[lane].len())),
            floors: self.floors,
            adding: true,
        };
        let mut cursor = PreparationCursor::new(update.row.key);
        if self.free_head == NONE {
            update.id = SourceId(self.arena.len());
            self.arena
                .len()
                .checked_add(1)
                .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
            Self::reserve_one(&mut self.arena, governor)?;
        } else {
            cursor.row = *self
                .arena
                .as_slice()
                .get(self.free_head)
                .ok_or_else(|| invalid_input("pending free source row is missing"))?;
            if cursor.row.key.arrival != 0
                || cursor.row.positions != [NONE; 2]
                || (cursor.row.key.source != NONE && cursor.row.key.source >= self.arena.len())
            {
                return Err(invalid_input(
                    "pending free source row is still live or has an invalid successor",
                ));
            }
            update.next_free = cursor.row.key.source;
        }
        for heap in self.heaps.iter_mut().take(1 + usize::from(candidate)) {
            governor.charge(Resource::Work, INSERT_WORK)?;
            heap.len()
                .checked_add(1)
                .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
            Self::reserve_one(heap, governor)?;
        }
        for (lane, (plan, floor)) in update
            .plans
            .iter_mut()
            .zip(&mut update.floors)
            .enumerate()
            .take(1 + usize::from(candidate))
        {
            plan.hole = plan.len;
            plan.replacement = update.id;
            plan.movement = Move::Insert;
            self.ascend(lane, plan, floor, &mut cursor, governor)?;
            update.row.positions[lane] = plan.target;
        }
        Self::validate_floors(
            &core::array::from_fn(|lane| update.plans[lane].after_len()),
            &update.floors,
        )?;
        Ok(update)
    }

    pub(super) fn prepare_remove(
        &self,
        id: SourceId,
        source: usize,
        arrival: u64,
        candidate: bool,
        governor: &mut Governor,
    ) -> Result<SourceUpdate, StopReason> {
        governor.charge(Resource::Work, UPDATE_WORK)?;
        let mut cursor = PreparationCursor::new(Key { source, arrival });
        self.record(id, candidate, &mut cursor)?;
        let mut update = SourceUpdate {
            id,
            row: Row::free(self.free_head),
            next_free: id.0,
            plans: core::array::from_fn(|lane| LanePlan::inactive(self.heaps[lane].len())),
            floors: self.floors,
            adding: false,
        };
        // Copy both holes before the cursor is reused for key reads.
        for (plan, &position) in update.plans.iter_mut().zip(&cursor.row.positions) {
            plan.hole = position;
        }
        for (lane, (plan, floor)) in update
            .plans
            .iter_mut()
            .zip(&mut update.floors)
            .enumerate()
            .take(1 + usize::from(candidate))
        {
            governor.charge(Resource::Work, REMOVE_WORK)?;
            self.endpoint(lane, plan.hole, id)?;
            self.remove_plan(lane, plan, floor, &mut cursor, governor)?;
        }
        Self::validate_floors(
            &core::array::from_fn(|lane| update.plans[lane].after_len()),
            &update.floors,
        )?;
        Ok(update)
    }

    fn apply_lane(&mut self, lane: usize, plan: &LanePlan) {
        if plan.movement == Move::Inactive {
            return;
        }
        let heap = &mut self.heaps[lane];
        if plan.movement != Move::Insert {
            heap.pop().expect("heap tail prepared");
            if plan.movement == Move::Tail {
                return;
            }
        }
        let mut cursor = ReplayCursor {
            hole: plan.hole,
            next: 0,
            id: 0,
            shift: plan.shift,
        };
        while cursor.hole != plan.target {
            cursor.next = if plan.movement == Move::Down {
                cursor.shift -= 1;
                2 * cursor.hole + 1 + (((plan.target + 1) >> cursor.shift) & 1)
            } else {
                (cursor.hole - 1) / 2
            };
            cursor.id = heap[cursor.next];
            if cursor.hole == heap.len() {
                heap.push(cursor.id).expect("heap append capacity prepared");
            } else {
                heap.as_mut_slice()[cursor.hole] = cursor.id;
            }
            self.arena.as_mut_slice()[cursor.id].positions[lane] = cursor.hole;
            cursor.hole = cursor.next;
        }
        if cursor.hole == heap.len() {
            heap.push(plan.replacement.0)
                .expect("heap append capacity prepared");
        } else {
            heap.as_mut_slice()[cursor.hole] = plan.replacement.0;
        }
        if plan.movement != Move::Insert {
            self.arena.as_mut_slice()[plan.replacement.0].positions[lane] = cursor.hole;
        }
    }

    /// Admit terminal mode and the eventual constant-time owner retirements.
    /// The owners themselves remain through every queued-handle lifetime.
    pub(super) fn prepare_terminal(
        &self,
        pending: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 6)?;
        if self.heaps[0].len() != pending {
            return Err(invalid_input(
                "terminal report heap and dispatch queue lengths disagree",
            ));
        }
        Self::validate_floors(
            &core::array::from_fn(|lane| self.heaps[lane].len()),
            &self.floors,
        )
    }

    /// Validate without updating either heap; only consuming EOF may use this.
    pub(super) fn terminal_entry(
        &self,
        id: SourceId,
        source: usize,
        arrival: u64,
        candidate: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 4)?;
        let mut cursor = PreparationCursor::new(Key { source, arrival });
        self.record(id, candidate, &mut cursor)?;
        self.endpoint(0, cursor.row.positions[0], id)?;
        if candidate {
            governor.charge(Resource::Work, 1)?;
            self.endpoint(1, cursor.row.positions[1], id)?;
        }
        Ok(())
    }

    /// Called only after successful terminal dispatch has stripped every handle.
    pub(super) fn retire(&mut self, governor: &Governor) {
        *self = Self::new(governor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Budget;

    /// Routing and candidate identities may share origins; refusal must not
    /// publish a prepared handle, reuse a free row or change either minimum.
    #[test]
    fn typed_minima_survive_mixed_reports_and_unapplied_queue_admission() {
        let mut governor = Governor::new(Budget::default());
        let mut sources = PendingSources::new(&governor);
        let mut handles = Vec::new();
        for (arrival, (source, candidate)) in
            (1_u64..).zip([(63, false), (4096, true), (63, true), (64, false)])
        {
            let update = sources
                .prepare_add(source, arrival, candidate, 4096, &mut governor)
                .expect("queued report");
            handles.push(update.handle());
            update.apply(&mut sources);
        }
        assert_eq!(
            (sources.floor(), sources.candidate_floor()),
            (Some(63), Some(63))
        );
        sources
            .prepare_remove(handles[0], 63, 1, false, &mut governor)
            .expect("routing report leaves its candidate")
            .apply(&mut sources);
        assert_eq!(
            (sources.floor(), sources.candidate_floor()),
            (Some(63), Some(63))
        );
        sources
            .prepare_remove(handles[2], 63, 3, true, &mut governor)
            .expect("report and candidate have different successors")
            .apply(&mut sources);
        assert_eq!(
            (sources.floor(), sources.candidate_floor()),
            (Some(64), Some(4096))
        );
        sources
            .prepare_remove(handles[3], 64, 4, false, &mut governor)
            .expect("routing successor leaves")
            .apply(&mut sources);
        assert_eq!(
            (sources.floor(), sources.candidate_floor()),
            (Some(4096), Some(4096))
        );
        let free = sources.free_head;
        let _update = sources
            .prepare_add(128, 5, true, 4096, &mut governor)
            .expect("prepared reusable source row");
        assert_eq!(
            governor.charge(Resource::ScratchBytes, u64::MAX),
            Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
        );
        assert_eq!(
            (sources.floor(), sources.candidate_floor()),
            (Some(4096), Some(4096))
        );
        assert_eq!(sources.free_head, free);
        assert_eq!(sources.arena[free].key.arrival, 0);
        assert_eq!(sources.arena[handles[1].0].key.arrival, 2);
    }
}
