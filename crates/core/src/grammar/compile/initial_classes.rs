// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Prove grouped Class locations belong exclusively to the initial call.

use super::{Inst, flags, successors, work_for};
use crate::contract::{Governor, Resource, ScratchLease, ScratchVec, StopReason, push_scratch};
use crate::grammar::GrammarError;

/// The descriptor and its lease are admitted separately from boolean capacity.
#[derive(Debug)]
pub(crate) struct InitialClasses {
    mask: ScratchVec<bool>,
    _descriptor: ScratchLease,
}

const _: () = assert!(core::mem::size_of::<InitialClasses>() <= 64);
const _: () =
    assert!(core::mem::size_of::<(ScratchVec<bool>, ScratchVec<u32>, ScratchLease)>() <= 96);

impl PartialEq for InitialClasses {
    fn eq(&self, other: &Self) -> bool {
        self.mask == other.mask
    }
}

impl Eq for InitialClasses {}

impl InitialClasses {
    pub(crate) fn compile(
        insts: &[Inst],
        start: u32,
        enabled: bool,
        governor: &mut Governor,
    ) -> Result<Self, GrammarError> {
        governor.charge(Resource::Work, 1)?;
        let descriptor = governor.lease_scratch(64)?;
        let mut result = Self {
            mask: ScratchVec::new(governor),
            _descriptor: descriptor,
        };
        if !enabled {
            return Ok(result);
        }
        // Cover both temporary vector handles and this aggregate lease.
        governor.charge(Resource::Work, 2)?;
        let _temporaries = governor.lease_scratch(96)?;
        result.mask = flags(insts.len(), governor)?;
        let mut after = flags(insts.len(), governor)?;
        let mut stack = ScratchVec::new(governor);
        stack.reserve(insts.len())?;
        enqueue(start, &mut result.mask, &mut stack, governor)?;
        walk(insts, &mut result.mask, &mut stack, true, governor)?;
        // All Next targets share one walk, including unreachable Next seeds.
        work_for(insts.len(), 1, 1, governor)?;
        for inst in insts {
            if let Inst::Next { next } = *inst {
                enqueue(next, &mut after, &mut stack, governor)?;
            }
        }
        walk(insts, &mut after, &mut stack, false, governor)?;
        work_for(insts.len(), 4, 1, governor)?;
        let mut any = false;
        for (pc, inst) in insts.iter().enumerate() {
            let keep = result.mask[pc] && !after[pc] && matches!(inst, Inst::Class { .. });
            result.mask.as_mut_slice()[pc] = keep;
            any |= keep;
        }
        if !any {
            result.mask = ScratchVec::new(governor);
        }
        governor.check()?;
        Ok(result)
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.mask.is_empty()
    }

    /// A nonempty proof has exactly one cell for every program instruction.
    pub(crate) fn contains(&self, pc: u32) -> bool {
        self.mask[pc as usize]
    }
}

/// Mark before enqueueing, so the reserved stack never exceeds the PC count.
fn enqueue(
    pc: u32,
    reached: &mut ScratchVec<bool>,
    stack: &mut ScratchVec<u32>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    governor.charge(Resource::Work, 1)?;
    if !core::mem::replace(&mut reached.as_mut_slice()[pc as usize], true) {
        push_scratch(stack, pc, governor)?;
    }
    Ok(())
}

/// Traverse consuming and zero-width continuations through one authority.
fn walk(
    insts: &[Inst],
    reached: &mut ScratchVec<bool>,
    stack: &mut ScratchVec<u32>,
    stop_at_next: bool,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    while let Some(pc) = stack.pop() {
        governor.charge(Resource::Work, 1)?;
        let inst = &insts[pc as usize];
        if stop_at_next && matches!(inst, Inst::Next { .. }) {
            continue;
        }
        for next in successors(inst) {
            enqueue(next, reached, stack, governor)?;
        }
    }
    Ok(())
}
