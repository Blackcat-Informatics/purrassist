// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Pattern-engine ownership of shared admitted collections.
use crate::contract::{Governor, ScratchCharge, ScratchLease, ScratchMap, ScratchVec, StopReason};
use std::cmp::Ordering;

/// Mutable/consuming access to storage already admitted by `ScratchVec`.
/// Growth belongs exclusively to `ScratchVec`; this owner only performs
/// operations that cannot allocate. Its lease outlives the actual vector,
/// including an iterator that has not yet consumed all its elements.
#[derive(Debug)]
pub(super) struct Buffer<T> {
    pub(super) values: Vec<T>,
    lease: ScratchLease,
}

impl<T: ScratchCharge> Buffer<T> {
    pub(super) fn from_scratch(values: ScratchVec<T>, governor: &Governor) -> Self {
        let values = values.into_vec();
        let bytes = u64::try_from(values.capacity())
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::try_from(T::CHARGE.max(1)).unwrap_or(u64::MAX));
        Self {
            values,
            lease: governor.scratch_lease_for(bytes),
        }
    }
}

impl<T> ScratchCharge for Buffer<T> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 40);
        40
    };
}

impl<T: PartialEq> PartialEq for Buffer<T> {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}

impl<T: Eq> Eq for Buffer<T> {}

impl<T: Ord> PartialOrd for Buffer<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Ord> Ord for Buffer<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.values.cmp(&other.values)
    }
}

pub(super) struct BufferIter<T> {
    values: std::vec::IntoIter<T>,
    _lease: ScratchLease,
}

impl<T> Iterator for BufferIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.values.next()
    }
}

impl<T> DoubleEndedIterator for BufferIter<T> {
    fn next_back(&mut self) -> Option<T> {
        self.values.next_back()
    }
}

impl<T> IntoIterator for Buffer<T> {
    type Item = T;
    type IntoIter = BufferIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        BufferIter {
            values: self.values.into_iter(),
            _lease: self.lease,
        }
    }
}

/// A LIFO exploration stack whose backing association list admits every
/// frame before allocation. Consecutive integer keys keep insertions/removals
/// at the end of the shared `ScratchMap` storage, without shifting frames.
pub(super) struct Stack<T> {
    values: ScratchMap<usize, T>,
}

impl<T: ScratchCharge> Stack<T> {
    pub(super) const fn len(&self) -> usize {
        self.values.len()
    }
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            values: ScratchMap::new(governor),
        }
    }

    pub(super) fn push(&mut self, value: T) -> Result<(), StopReason> {
        self.values.insert(self.values.len(), value)?;
        Ok(())
    }

    pub(super) fn pop(&mut self) -> Option<T> {
        self.values
            .len()
            .checked_sub(1)
            .and_then(|last| self.values.remove(&last))
    }
}
