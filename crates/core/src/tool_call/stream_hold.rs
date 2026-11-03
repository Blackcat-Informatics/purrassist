// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Source coverage retained by settled frames awaiting final semantics.
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchVec, StopReason, invalid_input, push_scratch,
};
use core::ops::Range;

#[derive(Clone, Copy)]
struct Interval {
    start: usize,
    end: usize,
    covered: usize,
}
impl ScratchCharge for Interval {
    const CHARGE: usize = 24;
}
const _: () = assert!(core::mem::size_of::<Interval>() <= Interval::CHARGE);

/// A checked affine result for one unchanged interval union and source cutoffs.
#[derive(Clone, Copy)]
struct Basis {
    candidate: usize,
    normalized: usize,
    discarded: usize,
    before_normalized: usize,
    held: usize,
}
const _: () = assert!(core::mem::size_of::<Option<Basis>>() <= 48);

pub(super) struct CandidateCoverage {
    intervals: ScratchVec<Interval>,
    basis: Option<Basis>,
}

impl CandidateCoverage {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            intervals: ScratchVec::new(governor),
            basis: None,
        }
    }

    pub(super) fn append(
        &mut self,
        span: &Range<usize>,
        n: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        if span.start > span.end || span.end > n {
            return Err(invalid_input(
                "candidate interval exceeds interpreted source",
            ));
        }
        if span.is_empty() {
            return Ok(());
        }
        let covered = if let Some(last) = self.intervals.as_mut_slice().last_mut() {
            if span.start < last.start {
                return Err(invalid_input("candidate intervals are not in source order"));
            }
            if span.start <= last.end {
                if span.end > last.end {
                    last.covered = last
                        .covered
                        .checked_add(span.end - last.end)
                        .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
                    last.end = span.end;
                    self.basis = None;
                }
                return Ok(());
            }
            last.covered
        } else {
            0
        };
        let covered = covered
            .checked_add(span.end - span.start)
            .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
        push_scratch(
            &mut self.intervals,
            Interval {
                start: span.start,
                end: span.end,
                covered,
            },
            governor,
        )?;
        self.basis = None;
        Ok(())
    }

    fn before(&self, at: usize, governor: &mut Governor) -> Result<usize, StopReason> {
        governor.charge(Resource::Work, 1)?;
        let value = if self.intervals.last().is_none_or(|last| last.end <= at) {
            self.intervals.last().map_or(0, |last| last.covered)
        } else {
            let (mut low, mut high) = (0, self.intervals.len());
            while low < high {
                governor.charge(Resource::Work, 2)?;
                let mid = low + (high - low) / 2;
                if self.intervals[mid].start < at {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }
            if low == 0 {
                0
            } else {
                let last = self.intervals[low - 1];
                last.covered
                    .checked_sub(last.end - at.min(last.end))
                    .ok_or_else(|| invalid_input("candidate coverage prefix is inconsistent"))?
            }
        };
        Ok(value)
    }

    /// Prepare a fresh basis, reusing only exact coordinates of this union.
    fn prepare_basis(
        &self,
        candidate: usize,
        normalized: usize,
        governor: &mut Governor,
    ) -> Result<Basis, StopReason> {
        // Checked discarded/overlap/held records and final basis publication.
        governor.charge(Resource::Work, 4)?;
        let covered = if let Some(basis) = self.basis.filter(|basis| basis.candidate == candidate) {
            governor.charge(Resource::Work, 1)?;
            candidate
                .checked_sub(basis.discarded)
                .ok_or_else(|| invalid_input("cached candidate prefix is inconsistent"))?
        } else {
            self.before(candidate, governor)?
        };
        let earlier = if normalized == candidate {
            covered
        } else if let Some(basis) = self.basis.filter(|basis| basis.normalized == normalized) {
            basis.before_normalized
        } else {
            self.before(normalized, governor)?
        };
        let discarded = candidate
            .checked_sub(covered)
            .ok_or_else(|| invalid_input("candidate coverage exceeds its source prefix"))?;
        if earlier > normalized {
            return Err(invalid_input(
                "candidate coverage exceeds its normalized prefix",
            ));
        }
        let overlap = covered
            .checked_sub(earlier)
            .ok_or_else(|| invalid_input("candidate coverage is not monotone"))?;
        let held = (candidate - normalized)
            .checked_sub(overlap)
            .ok_or_else(|| invalid_input("candidate overlap exceeds held source"))?;
        Ok(Basis {
            candidate,
            normalized,
            discarded,
            before_normalized: earlier,
            held,
        })
    }

    pub(super) fn counts(
        &mut self,
        n: usize,
        candidate: usize,
        normalized: usize,
        governor: &mut Governor,
    ) -> Result<(usize, usize), StopReason> {
        // Coordinate/union bounds and exact basis-key selection.
        governor.charge(Resource::Work, 2)?;
        let last = self.intervals.last().copied();
        if candidate > n || normalized > n || last.is_some_and(|last| last.end > n) {
            return Err(invalid_input("byte ownership exceeds interpreted source"));
        }
        let normalized = normalized.min(candidate);
        if candidate == n && normalized == n {
            governor.charge(Resource::Work, 1)?;
            let candidates = last.map_or(0, |last| last.covered);
            if candidates > n {
                return Err(invalid_input("candidate coverage exceeds source length"));
            }
            return Ok((0, candidates));
        }
        if let Some(basis) = self
            .basis
            .filter(|basis| basis.candidate == candidate && basis.normalized == normalized)
        {
            governor.charge(Resource::Work, 1)?;
            let candidates = n
                .checked_sub(basis.discarded)
                .ok_or_else(|| invalid_input("cached candidate coverage exceeds source length"))?;
            return Ok((basis.held, candidates));
        }
        let basis = self.prepare_basis(candidate, normalized, governor)?;
        let candidates = n
            .checked_sub(basis.discarded)
            .ok_or_else(|| invalid_input("candidate coverage exceeds source length"))?;
        self.basis = Some(basis);
        Ok((basis.held, candidates))
    }
}
