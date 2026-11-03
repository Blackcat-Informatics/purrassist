// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Append-only source proofs addressed by immutable call and reading indices.

use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchVec, StopReason, invalid_input, push_scratch,
};
use crate::grammar::engine::SourceEnvelope;

#[derive(Clone, Copy)]
struct Entry {
    key: (u32, usize),
    source: SourceEnvelope,
}

impl ScratchCharge for Entry {
    const CHARGE: usize = 48;
}

const _: () = assert!(core::mem::size_of::<Entry>() <= Entry::CHARGE);

/// Completed parts are stored once, in increasing store and reading order.
/// This owner preserves that order without sorted insertion or payload copies.
pub(super) struct SourceEnvelopes {
    entries: ScratchVec<Entry>,
}

impl SourceEnvelopes {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            entries: ScratchVec::new(governor),
        }
    }

    pub(super) fn push(
        &mut self,
        stored: u32,
        reading: usize,
        source: SourceEnvelope,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        // The last-key lookup and its two scalar comparisons precede mutation.
        governor.charge(Resource::Work, 3)?;
        let key = (stored, reading);
        if self.entries.last().is_some_and(|entry| entry.key >= key) {
            return Err(invalid_input(
                "completed call envelope keys are not strictly increasing",
            ));
        }
        push_scratch(&mut self.entries, Entry { key, source }, governor)
    }

    pub(super) fn get(
        &self,
        stored: u32,
        reading: usize,
        governor: &mut Governor,
    ) -> Result<Option<&SourceEnvelope>, StopReason> {
        governor.charge(
            Resource::Work,
            u64::from(self.entries.len().bit_width() + 1).saturating_mul(2),
        )?;
        Ok(self
            .entries
            .binary_search_by_key(&(stored, reading), |entry| entry.key)
            .ok()
            .map(|index| &self.entries[index].source))
    }
}
