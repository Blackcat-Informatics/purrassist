// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Exact temporary ownership for evaluator records and their publication.
use super::{index, reference};
use crate::contract::{Evidence, ScratchCharge, ScratchLease, Validation};
use crate::{Governor, ScratchString, ScratchVec, StopReason};

pub(super) struct Owned<T> {
    pub(super) value: T,
    pub(super) lease: ScratchLease,
}
impl<T: ScratchCharge> ScratchCharge for Owned<T> {
    const CHARGE: usize = {
        let charge = T::CHARGE + 16;
        assert!(core::mem::size_of::<Self>() <= charge);
        charge
    };
}
impl Owned<String> {
    pub(super) fn string(text: ScratchString) -> Self {
        let (value, lease) = text.into_owned_parts();
        Self { value, lease }
    }
}
impl Owned<Evidence> {
    pub(super) fn evidence(
        rule: ScratchString,
        explanation: ScratchString,
        destination: ScratchString,
    ) -> Self {
        let (rule, mut lease) = rule.into_owned_parts();
        let (explanation, explanation_lease) = explanation.into_owned_parts();
        let (destination, destination_lease) = destination.into_owned_parts();
        lease.absorb(explanation_lease);
        lease.absorb(destination_lease);
        Self {
            value: Evidence {
                rule,
                explanation,
                destination,
                span: None,
            },
            lease,
        }
    }
}
impl Owned<Validation> {
    pub(super) fn validation(
        validator: ScratchString,
        constraint: ScratchString,
        passed: bool,
        diagnostics: Records<String>,
    ) -> Self {
        let (validator, mut lease) = validator.into_owned_parts();
        let (constraint, constraint_lease) = constraint.into_owned_parts();
        let diagnostics = diagnostics.into_owned();
        lease.absorb(constraint_lease);
        lease.absorb(diagnostics.lease);
        Self {
            value: Validation {
                validator,
                constraint,
                passed,
                diagnostics: diagnostics.value,
            },
            lease,
        }
    }
}

/// Entries share a lease only when they are transferred or discarded together.
pub(super) struct Records<T> {
    pub(super) values: ScratchVec<T>,
    lease: ScratchLease,
}
impl<T: ScratchCharge> Records<T> {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            values: ScratchVec::new(governor),
            lease: ScratchLease::empty(governor),
        }
    }
    pub(super) fn push(
        &mut self,
        entry: Owned<T>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        index::push(&mut self.values, entry.value, governor)?;
        self.lease.absorb(entry.lease);
        Ok(())
    }
    pub(super) fn append(
        &mut self,
        other: &mut Self,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        append(&mut self.values, &mut other.values, governor)?;
        self.lease.absorb(core::mem::replace(
            &mut other.lease,
            ScratchLease::empty(governor),
        ));
        Ok(())
    }
    pub(super) fn into_owned(self) -> Owned<Vec<T>> {
        let (value, mut lease) = self.values.into_owned_parts();
        lease.absorb(self.lease);
        Owned { value, lease }
    }
    pub(super) fn publish(self) -> Vec<T> {
        let owned = self.into_owned();
        owned.lease.retain();
        owned.value
    }
}
impl<T: ScratchCharge> core::ops::Deref for Records<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        self.values.as_slice()
    }
}

pub(super) fn append<T: ScratchCharge>(
    target: &mut ScratchVec<T>,
    source: &mut ScratchVec<T>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    reference::work(governor, source.len())?;
    if source.len() > target.capacity() - target.len() {
        reference::work(governor, target.len())?;
    }
    target.append(source)
}

pub(super) fn compare(
    left: &str,
    right: &str,
    governor: &mut Governor,
) -> Result<core::cmp::Ordering, StopReason> {
    reference::work(governor, left.len().min(right.len()).saturating_add(1))?;
    Ok(left.cmp(right))
}

/// In-place heapsort admits comparisons and swaps without extra storage.
pub(super) fn sort_by<T>(
    values: &mut [T],
    governor: &mut Governor,
    mut compare: impl FnMut(&T, &T, &mut Governor) -> Result<core::cmp::Ordering, StopReason>,
) -> Result<(), StopReason> {
    fn sift<T>(
        values: &mut [T],
        mut root: usize,
        end: usize,
        governor: &mut Governor,
        compare: &mut impl FnMut(&T, &T, &mut Governor) -> Result<core::cmp::Ordering, StopReason>,
    ) -> Result<(), StopReason> {
        while root < end / 2 {
            let mut child = root * 2 + 1;
            if child + 1 < end && compare(&values[child], &values[child + 1], governor)?.is_lt() {
                child += 1;
            }
            if !compare(&values[root], &values[child], governor)?.is_lt() {
                break;
            }
            reference::work(governor, 1)?;
            values.swap(root, child);
            root = child;
        }
        Ok(())
    }
    for root in (0..values.len() / 2).rev() {
        sift(values, root, values.len(), governor, &mut compare)?;
    }
    for end in (1..values.len()).rev() {
        reference::work(governor, 1)?;
        values.swap(0, end);
        sift(values, 0, end, governor, &mut compare)?;
    }
    Ok(())
}

pub(super) fn search_by<T>(
    values: &[T],
    governor: &mut Governor,
    mut compare: impl FnMut(&T, &mut Governor) -> Result<core::cmp::Ordering, StopReason>,
) -> Result<Result<usize, usize>, StopReason> {
    let mut low = 0;
    let mut high = values.len();
    while low < high {
        reference::work(governor, 1)?;
        let middle = low + (high - low) / 2;
        match compare(&values[middle], governor)? {
            core::cmp::Ordering::Less => low = middle + 1,
            core::cmp::Ordering::Equal => return Ok(Ok(middle)),
            core::cmp::Ordering::Greater => high = middle,
        }
    }
    Ok(Err(low))
}

/// Committed observations retain individual payload owners and reserve their
/// final plain-vector slots before commit, so stopped recovery can publish them
/// without allocating or asking an already stopped governor for more work.
pub(super) struct Observations {
    prefix: ScratchVec<Owned<Evidence>>,
    patterns: ScratchVec<Owned<Evidence>>,
    output: ScratchVec<Evidence>,
}
impl Observations {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            prefix: ScratchVec::new(governor),
            patterns: ScratchVec::new(governor),
            output: ScratchVec::new(governor),
        }
    }
    fn reserve_entry(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        reference::work(governor, 1)?; // eventual output movement
        self.output.reserve(
            self.prefix
                .len()
                .saturating_add(self.patterns.len())
                .saturating_add(1),
        )
    }
    pub(super) fn prefix(
        &mut self,
        entry: Owned<Evidence>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        self.reserve_entry(governor)?;
        index::push(&mut self.prefix, entry, governor)
    }
    pub(super) fn pattern(
        &mut self,
        entry: Owned<Evidence>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let position = search_by(self.patterns.as_slice(), governor, |known, governor| {
            compare(&known.value.destination, &entry.value.destination, governor)
        })?;
        let Err(position) = position else {
            return Ok(());
        };
        governor.charge(crate::contract::Resource::EvidenceEntries, 1)?;
        self.reserve_entry(governor)?;
        reference::work(governor, self.patterns.len() - position)?;
        index::push(&mut self.patterns, entry, governor)?;
        self.patterns.as_mut_slice()[position..].rotate_right(1);
        Ok(())
    }
    /// Prepare additional search-result evidence while committed observations
    /// remain untouched. Failure leaves enough capacity for those observations.
    pub(super) fn prepare(
        &mut self,
        additional: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        reference::work(governor, additional)?;
        self.output.reserve(
            self.prefix
                .len()
                .saturating_add(self.patterns.len())
                .saturating_add(additional),
        )?;
        governor.check()
    }
    pub(super) fn publish(self, additional: Vec<Evidence>, include_prefix: bool) -> Vec<Evidence> {
        fn move_entries(output: &mut Vec<Evidence>, entries: ScratchVec<Owned<Evidence>>) {
            let (entries, entries_lease) = entries.into_owned_parts();
            for entry in entries {
                output.push(entry.value);
                entry.lease.retain();
            }
            drop(entries_lease);
        }
        let (mut output, output_lease) = self.output.into_owned_parts();
        assert!(
            output.capacity() >= self.prefix.len() + self.patterns.len() + additional.len(),
            "INVARIANT: committed evidence publication slots were admitted"
        );
        if include_prefix {
            move_entries(&mut output, self.prefix);
        } else {
            drop(self.prefix);
        }
        output.extend(additional);
        move_entries(&mut output, self.patterns);
        output_lease.retain();
        output
    }
}
