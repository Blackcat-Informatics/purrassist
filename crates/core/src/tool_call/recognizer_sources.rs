// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Transactional source minima for pending reports and their candidate subset.
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason, invalid_input};

const LEVELS: usize = 11;
// Shared prepare/apply record operations: source addressing/height (2), root and
// active/minimum agreement (3), count-page/count-pair read and validation (3),
// paired arithmetic/subset checks (3), leaf read/membership (2), active/floor
// planning (2), compact carrier construction/handoff (2), and applying the page,
// count pair and cached metadata (3). Floor validation, changed rows, promotions,
// minimum descent, and backing initialization/relocation are admitted separately.
const PREPARE_WORK: u64 = 20;
// Plan coordinate/mask, read/overlay pair, validate/update both lanes and record
// prefix lengths; apply recomputes that coordinate/mask and writes the pair.
const PATH_WORK: u64 = 5;
// Validate the newly exposed row, construct the old-root child and apply it.
const PROMOTE_WORK: u64 = 3;
// Minimum descent reads one pair with promotion/path overlays; each actual lane
// then separately admits its bit scan and checked coordinate advance.
const MINIMUM_READ_WORK: u64 = 3;

#[derive(Clone, Copy, Default)]
struct Counts([usize; 2]);
impl ScratchCharge for Counts {
    const CHARGE: usize = 16;
}
const _: () = assert!(core::mem::size_of::<Counts>() <= Counts::CHARGE);

#[derive(Clone, Copy, Default)]
struct Bits([u64; 2]);
impl ScratchCharge for Bits {
    const CHARGE: usize = 16;
}
const _: () = assert!(core::mem::size_of::<Bits>() <= Bits::CHARGE);

impl Bits {
    fn validate(self) -> Result<Self, StopReason> {
        if self.0[1] & !self.0[0] != 0 {
            return Err(invalid_input("candidate occupancy is not a report subset"));
        }
        Ok(self)
    }
}

/// Exact changed-row prefixes, avoiding fixed maximum-height carrier arrays.
#[derive(Clone, Copy)]
struct Path {
    levels: [usize; 2],
    adding: bool,
}

/// All fallible admission precedes the dispatch queue mutation.
pub(super) struct SourceUpdate {
    source: usize,
    count: Counts,
    active: [usize; 2],
    floors: [Option<usize>; 2],
    height: usize,
    path: Path,
    page: Option<ScratchVec<Counts>>,
}

impl SourceUpdate {
    /// Replay the fully checked compact deltas without allocation or admission.
    pub(super) fn apply(self, sources: &mut PendingSources) {
        if let Some(page) = self.page {
            sources.counts.as_mut_slice()[self.source / 64] = page;
        }
        sources.counts.as_mut_slice()[self.source / 64].as_mut_slice()[self.source % 64] =
            self.count;
        if sources.active[0] != 0 {
            let child = sources.promoted();
            for level in sources.height..self.height {
                sources.occupancy[level].as_mut_slice()[0] = child;
            }
        }
        let mut coordinate = self.source;
        for (level, words) in sources
            .occupancy
            .iter_mut()
            .enumerate()
            .take(self.path.levels[0].max(self.path.levels[1]))
        {
            let index = coordinate / 64;
            let mask = 1_u64 << (coordinate % 64);
            let pair = &mut words.as_mut_slice()[index];
            for lane in 0..2 {
                if level < self.path.levels[lane] {
                    pair.0[lane] = if self.path.adding {
                        pair.0[lane] | mask
                    } else {
                        pair.0[lane] & !mask
                    };
                }
            }
            coordinate = index;
        }
        sources.height = self.height;
        sources.active = self.active;
        sources.floors = self.floors;
    }
}

/// One counted source address with report and candidate occupancy lanes.
pub(super) struct PendingSources {
    counts: ScratchVec<ScratchVec<Counts>>,
    occupancy: [ScratchVec<Bits>; LEVELS],
    height: usize,
    active: [usize; 2],
    floors: [Option<usize>; 2],
}

impl PendingSources {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            counts: ScratchVec::new(governor),
            occupancy: core::array::from_fn(|_| ScratchVec::new(governor)),
            height: 0,
            active: [0; 2],
            floors: [None; 2],
        }
    }

    pub(super) const fn floor(&self) -> Option<usize> {
        self.floors[0]
    }
    pub(super) const fn candidate_floor(&self) -> Option<usize> {
        self.floors[1]
    }

    fn source_height(source: usize) -> Result<usize, StopReason> {
        let portable = u64::try_from(source)
            .map_err(|_| invalid_input("pending source exceeds the portable offset domain"))?;
        Ok((64 - portable.leading_zeros()).max(1).div_ceil(6) as usize)
    }

    fn count_at(&self, source: usize) -> Result<Counts, StopReason> {
        let page = self
            .counts
            .as_slice()
            .get(source / 64)
            .ok_or_else(|| invalid_input("pending source has no count page"))?;
        let count = match page.len() {
            0 => Counts::default(),
            64 => page[source % 64],
            _ => return Err(invalid_input("pending source count page is not complete")),
        };
        if count.0[1] > count.0[0] {
            return Err(invalid_input("candidate count exceeds report count"));
        }
        Ok(count)
    }

    fn validate(
        &self,
        source: usize,
        count: Counts,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if self.height > LEVELS || self.active[1] > self.active[0] {
            return Err(invalid_input(
                "pending source height or active subset is invalid",
            ));
        }
        let root = if self.height == 0 {
            Bits::default()
        } else {
            self.word(self.height - 1, 0)?.validate()?
        };
        let mut cached = None;
        for lane in 0..2 {
            if (self.active[lane] == 0) != (root.0[lane] == 0)
                || (self.active[lane] == 0) != self.floors[lane].is_none()
            {
                return Err(invalid_input("pending source root and minimum disagree"));
            }
            if let Some(floor) = self.floors[lane] {
                let actual = if floor == source {
                    count
                } else if let Some((_, value)) = cached.filter(|(known, _)| *known == floor) {
                    value
                } else {
                    governor.charge(Resource::Work, 2)?;
                    let value = self.count_at(floor)?;
                    cached = Some((floor, value));
                    value
                };
                if actual.0[lane] == 0 || (count.0[lane] != 0 && floor > source) {
                    return Err(invalid_input(
                        "pending source minimum has no matching live count",
                    ));
                }
            }
        }
        if self.floors[1]
            .is_some_and(|candidate| self.floors[0].is_none_or(|report| report > candidate))
        {
            return Err(invalid_input("candidate minimum precedes report minimum"));
        }
        Ok(())
    }

    fn word(&self, level: usize, index: usize) -> Result<Bits, StopReason> {
        self.occupancy[level]
            .as_slice()
            .get(index)
            .copied()
            .ok_or_else(|| invalid_input("pending source occupancy word is missing"))
    }

    fn promoted(&self) -> Bits {
        Bits([
            u64::from(self.active[0] != 0),
            u64::from(self.active[1] != 0),
        ])
    }

    fn before_word(&self, level: usize, index: usize) -> Result<Bits, StopReason> {
        let stored = self.word(level, index)?.validate()?;
        if level >= self.height && index == 0 && self.active[0] != 0 {
            Ok(self.promoted())
        } else {
            Ok(stored)
        }
    }

    fn after_word(
        &self,
        level: usize,
        index: usize,
        update: &SourceUpdate,
    ) -> Result<Bits, StopReason> {
        let mut pair = self.before_word(level, index)?;
        let coordinate = (update.source as u64) >> (6 * level);
        if coordinate / 64 == index as u64 {
            let mask = 1_u64 << (coordinate % 64);
            for lane in 0..2 {
                if level < update.path.levels[lane] {
                    pair.0[lane] = if update.path.adding {
                        pair.0[lane] | mask
                    } else {
                        pair.0[lane] & !mask
                    };
                }
            }
        }
        pair.validate()
    }

    fn reserve_slots<T: ScratchCharge>(
        values: &mut ScratchVec<T>,
        needed: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if needed > values.len() {
            let relocated = if needed > values.capacity() {
                values.len()
            } else {
                0
            };
            let missing = needed - values.len();
            governor.charge(
                Resource::Work,
                size(missing).saturating_add(size(relocated)),
            )?;
            values.reserve(missing)?;
        }
        Ok(())
    }

    /// Backing growth appends only zeros/empty owners; logical height and every
    /// nonzero row remain unchanged until the prepared update is applied.
    fn grow(
        &mut self,
        source: usize,
        height: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let needed = source / 64 + 1;
        if needed <= self.counts.len() && height <= self.height {
            return Ok(());
        }
        Self::reserve_slots(&mut self.counts, needed, governor)?;
        let mut coordinate = source;
        for words in self.occupancy.iter_mut().take(height) {
            // Reserve and later initialize this level in two real traversals.
            governor.charge(Resource::Work, 2)?;
            coordinate /= 64;
            Self::reserve_slots(words, coordinate + 1, governor)?;
        }
        for _ in self.counts.len()..needed {
            self.counts.push(ScratchVec::new(governor))?;
        }
        coordinate = source;
        for words in self.occupancy.iter_mut().take(height) {
            coordinate /= 64;
            let missing = (coordinate + 1).saturating_sub(words.len());
            words.extend(core::iter::repeat_n(Bits::default(), missing))?;
        }
        Ok(())
    }

    fn plan_path(
        &self,
        source: usize,
        changed: [bool; 2],
        adding: bool,
        height: usize,
        governor: &mut Governor,
    ) -> Result<Path, StopReason> {
        let mut path = Path {
            levels: [0; 2],
            adding,
        };
        let mut pending = changed;
        let mut coordinate = source;
        for level in 0..height {
            if !pending[0] && !pending[1] {
                break;
            }
            governor.charge(Resource::Work, PATH_WORK)?;
            let index = coordinate / 64;
            let mask = 1_u64 << (coordinate % 64);
            let old = self.before_word(level, index)?;
            let mut new = old;
            for lane in 0..2 {
                if pending[lane] {
                    if (old.0[lane] & mask != 0) == adding {
                        return Err(invalid_input(
                            "pending source occupancy transition is inconsistent",
                        ));
                    }
                    new.0[lane] = if adding {
                        old.0[lane] | mask
                    } else {
                        old.0[lane] & !mask
                    };
                    path.levels[lane] = level + 1;
                    pending[lane] = if adding {
                        old.0[lane] == 0
                    } else {
                        new.0[lane] == 0
                    };
                }
            }
            new.validate()?;
            coordinate = index;
        }
        Ok(path)
    }

    fn minimum_after(
        &self,
        update: &mut SourceUpdate,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let needed: [bool; 2] = core::array::from_fn(|lane| {
            !update.path.adding
                && update.path.levels[lane] != 0
                && self.floors[lane] == Some(update.source)
        });
        let mut descending = needed;
        let mut coordinates = [0_usize; 2];
        for level in (0..update.height).rev() {
            if !descending[0] && !descending[1] {
                break;
            }
            let mut cached = None;
            for lane in 0..2 {
                if !descending[lane] {
                    continue;
                }
                let pair = if let Some((_, pair)) =
                    cached.filter(|(known, _)| *known == coordinates[lane])
                {
                    pair
                } else {
                    governor.charge(Resource::Work, MINIMUM_READ_WORK)?;
                    let pair = self.after_word(level, coordinates[lane], update)?;
                    cached = Some((coordinates[lane], pair));
                    pair
                };
                governor.charge(Resource::Work, 1)?;
                let word = pair.0[lane];
                if update.active[lane] == 0 {
                    if word != 0 {
                        return Err(invalid_input(
                            "empty pending source index has a nonempty root",
                        ));
                    }
                    update.floors[lane] = None;
                    descending[lane] = false;
                } else {
                    if word == 0 {
                        return Err(invalid_input(
                            "nonempty pending source path has an empty word",
                        ));
                    }
                    coordinates[lane] = coordinates[lane]
                        .checked_mul(64)
                        .and_then(|base| base.checked_add(word.trailing_zeros() as usize))
                        .ok_or_else(|| {
                            invalid_input("pending source minimum exceeds the offset domain")
                        })?;
                }
            }
        }
        let mut cached = None;
        for lane in 0..2 {
            if needed[lane] && update.active[lane] != 0 {
                let at = coordinates[lane];
                let count = if at == update.source {
                    update.count
                } else if let Some((_, count)) = cached.filter(|(known, _)| *known == at) {
                    count
                } else {
                    governor.charge(Resource::Work, 2)?;
                    let count = self.count_at(at)?;
                    cached = Some((at, count));
                    count
                };
                if count.0[lane] == 0 {
                    return Err(invalid_input(
                        "pending source minimum points to an empty count",
                    ));
                }
                update.floors[lane] = Some(at);
            }
        }
        Ok(())
    }

    fn changed_count(
        old: Counts,
        candidate: bool,
        adding: bool,
        governor: &mut Governor,
    ) -> Result<Counts, StopReason> {
        let mut count = old;
        for lane in 0..2 {
            if lane == 0 || candidate {
                count.0[lane] = if adding {
                    old.0[lane]
                        .checked_add(1)
                        .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?
                } else {
                    old.0[lane]
                        .checked_sub(1)
                        .ok_or_else(|| invalid_input("queued source origin has no live count"))?
                };
            }
        }
        if count.0[1] > count.0[0] {
            return Err(invalid_input("candidate count exceeds report count"));
        }
        Ok(count)
    }

    fn prepare(
        &mut self,
        source: usize,
        candidate: bool,
        adding: bool,
        fed: Option<usize>,
        governor: &mut Governor,
    ) -> Result<SourceUpdate, StopReason> {
        governor.charge(Resource::Work, PREPARE_WORK)?;
        if fed.is_some_and(|fed| source > fed) {
            return Err(invalid_input("queued source origin exceeds admitted input"));
        }
        let old = if source / 64 < self.counts.len() {
            self.count_at(source)?
        } else {
            Counts::default()
        };
        self.validate(source, old, governor)?;
        let count = Self::changed_count(old, candidate, adding, governor)?;
        let height = if adding {
            self.height.max(Self::source_height(source)?)
        } else {
            self.height
        };
        if adding {
            self.grow(source, height, governor)?;
        }
        let leaf = self.word(0, source / 64)?.validate()?;
        let mask = 1_u64 << (source % 64);
        for lane in 0..2 {
            if (leaf.0[lane] & mask != 0) != (old.0[lane] != 0) {
                return Err(invalid_input("pending source count and occupancy disagree"));
            }
        }
        if self.active[0] != 0 {
            for level in self.height..height {
                governor.charge(Resource::Work, PROMOTE_WORK)?;
                if self.word(level, 0)?.0 != [0; 2] {
                    return Err(invalid_input(
                        "pending source promotion overwrites a live word",
                    ));
                }
            }
        }
        let changed = core::array::from_fn(|lane| (old.0[lane] == 0) != (count.0[lane] == 0));
        let path = self.plan_path(source, changed, adding, height, governor)?;
        let mut update = SourceUpdate {
            source,
            count,
            active: self.active,
            floors: self.floors,
            height,
            path,
            page: None,
        };
        for lane in 0..2 {
            if changed[lane] {
                update.active[lane] = if adding {
                    self.active[lane]
                        .checked_add(1)
                        .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?
                } else {
                    self.active[lane]
                        .checked_sub(1)
                        .ok_or_else(|| invalid_input("pending source origin count underflows"))?
                };
                if adding {
                    update.floors[lane] =
                        Some(self.floors[lane].map_or(source, |floor| floor.min(source)));
                }
            }
        }
        if self.counts[source / 64].is_empty() {
            if leaf.0 != [0; 2] {
                return Err(invalid_input(
                    "absent pending source page has live occupancy",
                ));
            }
            let mut page = ScratchVec::new(governor);
            Self::reserve_slots(&mut page, 64, governor)?;
            page.extend(core::iter::repeat_n(Counts::default(), 64))?;
            update.page = Some(page);
        }
        self.minimum_after(&mut update, governor)?;
        Ok(update)
    }

    pub(super) fn prepare_add(
        &mut self,
        source: usize,
        candidate: bool,
        fed: usize,
        governor: &mut Governor,
    ) -> Result<SourceUpdate, StopReason> {
        self.prepare(source, candidate, true, Some(fed), governor)
    }

    pub(super) fn prepare_remove(
        &mut self,
        source: usize,
        candidate: bool,
        governor: &mut Governor,
    ) -> Result<SourceUpdate, StopReason> {
        self.prepare(source, candidate, false, None, governor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Budget;

    /// Routing reports and candidate reports at one origin share multiplicity,
    /// but their minima diverge; queue refusal must not publish a prepared page.
    #[test]
    fn typed_minima_survive_mixed_reports_and_unapplied_queue_admission() {
        let mut governor = Governor::new(Budget::default());
        let mut sources = PendingSources::new(&governor);
        for (source, candidate) in [(63, false), (4096, true), (63, true)] {
            sources
                .prepare_add(source, candidate, 4096, &mut governor)
                .expect("queued report")
                .apply(&mut sources);
        }
        assert_eq!(
            (sources.floor(), sources.candidate_floor()),
            (Some(63), Some(63))
        );
        sources
            .prepare_remove(63, true, &mut governor)
            .expect("candidate leaves")
            .apply(&mut sources);
        assert_eq!(
            (sources.floor(), sources.candidate_floor()),
            (Some(63), Some(4096))
        );
        sources
            .prepare_remove(63, false, &mut governor)
            .expect("routing report leaves")
            .apply(&mut sources);
        assert_eq!(
            (sources.floor(), sources.candidate_floor()),
            (Some(4096), Some(4096))
        );

        let update = sources
            .prepare_add(128, true, 4096, &mut governor)
            .expect("prepared new page");
        assert_eq!(
            governor.charge(Resource::ScratchBytes, u64::MAX),
            Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
        );
        drop(update);
        assert_eq!(
            (sources.floor(), sources.candidate_floor()),
            (Some(4096), Some(4096))
        );
        assert_eq!(sources.count_at(128).expect("unpublished page").0, [0; 2]);
        assert_eq!(sources.count_at(4096).expect("resident report").0, [1; 2]);
    }
}
