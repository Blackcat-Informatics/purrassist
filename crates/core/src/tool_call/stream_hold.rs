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

pub(super) struct CandidateCoverage {
    intervals: ScratchVec<Interval>,
    queries: [Option<(usize, usize)>; 2],
}

impl CandidateCoverage {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            intervals: ScratchVec::new(governor),
            queries: [None; 2],
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
        let mut covered = 0;
        if let Some(last) = self.intervals.as_mut_slice().last_mut() {
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
                    self.queries = [None; 2];
                }
                return Ok(());
            }
            covered = last.covered;
        }
        covered = covered
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
        self.queries = [None; 2];
        Ok(())
    }

    fn before(
        &mut self,
        at: usize,
        slot: usize,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        governor.charge(Resource::Work, 1)?;
        if let Some((known, value)) = self.queries[slot]
            && known == at
        {
            return Ok(value);
        }
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
        self.queries[slot] = Some((at, value));
        Ok(value)
    }

    pub(super) fn counts(
        &mut self,
        n: usize,
        candidate: usize,
        normalized: usize,
        governor: &mut Governor,
    ) -> Result<(usize, usize), StopReason> {
        governor.charge(Resource::Work, 4)?;
        if candidate > n || normalized > n {
            return Err(invalid_input("byte ownership exceeds interpreted source"));
        }
        let covered = self.before(candidate, 0, governor)?;
        let candidates = covered
            .checked_add(n - candidate)
            .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
        let held = if normalized >= candidate {
            0
        } else {
            let earlier = self.before(normalized, 1, governor)?;
            let overlap = covered
                .checked_sub(earlier)
                .ok_or_else(|| invalid_input("candidate coverage is not monotone"))?;
            (candidate - normalized)
                .checked_sub(overlap)
                .ok_or_else(|| invalid_input("candidate overlap exceeds held source"))?
        };
        if candidates > n {
            return Err(invalid_input("candidate coverage exceeds source length"));
        }
        Ok((held, candidates))
    }
}
