// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Call-local membership of native revisions in actual final semantic ledgers.

use super::{EditFact, FinalPublication, revision_operator};
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchVec, StopReason, invalid_input, push_scratch,
};
use crate::grammar::edit::EditOperator;
use core::cmp::Ordering;

/// The source key precedes the actual ledger identity in numeric sort order.
#[derive(Clone, Copy)]
struct WitnessKey {
    operator: EditOperator,
    at: usize,
    end: usize,
    view: usize,
    semantic: usize,
}

impl ScratchCharge for WitnessKey {
    const CHARGE: usize = 40;
}
const _: () = assert!(core::mem::size_of::<WitnessKey>() <= WitnessKey::CHARGE);

impl WitnessKey {
    const fn prefix(self) -> (EditOperator, usize, usize) {
        (self.operator, self.at, self.end)
    }

    fn compare(self, other: Self) -> Ordering {
        (self.prefix(), self.view, self.semantic).cmp(&(other.prefix(), other.view, other.semantic))
    }
}

/// One index per call, shared by equivalent native origins of each ledger.
pub(super) struct RevisionWitnesses {
    keys: ScratchVec<WitnessKey>,
    ledgers: usize,
}

impl RevisionWitnesses {
    pub(super) fn build(
        publication: &FinalPublication<'_>,
        index: usize,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let mut witnesses = Self {
            keys: ScratchVec::new(governor),
            ledgers: 0,
        };
        for (view_slot, view) in publication.readings.iter().enumerate() {
            governor.charge(Resource::Work, 1)?;
            let call = view
                .calls
                .calls
                .get(index)
                .ok_or_else(|| invalid_input("revision has no final call counterpart"))?;
            let origins = view
                .origins
                .calls
                .get(index)
                .ok_or_else(|| invalid_input("revision has no final native correlation"))?;
            let semantic_count = call
                .alternatives
                .len()
                .checked_add(1)
                .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
            let mut competing = 0;
            for origin in &origins.0 {
                governor.charge(Resource::Work, 2)?;
                let row = publication.native.call(origin.id)?;
                if row.competing == 0
                    || row.competing > row.readings.len()
                    || (origin.competing && row.readings.len() != semantic_count)
                {
                    return Err(invalid_input(
                        "revision semantic readings and native origins disagree",
                    ));
                }
                if !origin.competing {
                    continue;
                }
                competing = competing.max(row.competing);
            }
            if competing == 0 {
                return Err(invalid_input("revision has no competing native origin"));
            }
            // Semantic slots start at zero in every correlated native row, so
            // their union is this prefix, even when several origins share it.
            for semantic in 0..competing {
                governor.charge(Resource::Work, 1)?;
                let ledger = if semantic == 0 {
                    &call.repairs
                } else {
                    &call
                        .alternatives
                        .get(semantic - 1)
                        .ok_or_else(|| {
                            invalid_input("revision semantic competitor has no retained reading")
                        })?
                        .repairs
                };
                witnesses.ledgers = witnesses
                    .ledgers
                    .checked_add(1)
                    .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
                for evidence in &ledger.evidence {
                    governor.charge(Resource::Work, 1)?;
                    let Some(span) = &evidence.span else {
                        continue;
                    };
                    let Some(operator) = revision_operator(&evidence.rule, governor)? else {
                        continue;
                    };
                    push_scratch(
                        &mut witnesses.keys,
                        WitnessKey {
                            operator,
                            at: span.bytes.start,
                            end: span.bytes.end,
                            view: view_slot,
                            semantic,
                        },
                        governor,
                    )?;
                }
            }
        }
        if witnesses.ledgers == 0 {
            return Err(invalid_input("revision has no final semantic ledger"));
        }
        witnesses.sort_unique(governor)?;
        Ok(witnesses)
    }

    pub(super) fn contains_all(
        &self,
        edit: &EditFact,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let prefix = (edit.operator, edit.at, edit.end);
        let first = self.prefix_bound(prefix, false, governor)?;
        let end = self.prefix_bound(prefix, true, governor)?;
        let matches = end
            .checked_sub(first)
            .ok_or_else(|| invalid_input("revision witness bounds are inconsistent"))?;
        // Full-key deduplication leaves at most one match per actual ledger;
        // there are no keys from diagnostic alternatives or unrelated calls.
        Ok(matches == self.ledgers)
    }

    fn prefix_bound(
        &self,
        prefix: (EditOperator, usize, usize),
        upper: bool,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        let (mut low, mut high) = (0, self.keys.len());
        loop {
            governor.charge(Resource::Work, 1)?;
            if low == high {
                return Ok(low);
            }
            let middle = low + (high - low) / 2;
            let order = self.keys[middle].prefix().cmp(&prefix);
            if order.is_lt() || (upper && order.is_eq()) {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
    }

    fn sort_unique(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        let keys = self.keys.as_mut_slice();
        let length = keys.len();
        let mut root = length / 2;
        loop {
            if root == 0 {
                break;
            }
            root -= 1;
            sift(keys, root, length, governor)?;
        }
        let mut end = keys.len();
        loop {
            if end < 2 {
                break;
            }
            end -= 1;
            keys.swap(0, end);
            sift(keys, 0, end, governor)?;
        }
        let mut write = 0;
        for read in 0..keys.len() {
            governor.charge(Resource::Work, 1)?;
            if write == 0 || keys[write - 1].compare(keys[read]) != Ordering::Equal {
                keys[write] = keys[read];
                write += 1;
            }
        }
        while self.keys.len() > write {
            governor.charge(Resource::Work, 1)?;
            self.keys.pop();
        }
        Ok(())
    }
}

/// Restore a numeric max heap; each visited loop, comparison and swap is admitted.
fn sift(
    keys: &mut [WitnessKey],
    mut root: usize,
    end: usize,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    loop {
        governor.charge(Resource::Work, 1)?;
        if root >= end / 2 {
            return Ok(());
        }
        // The parent guard proves both child calculations cannot overflow.
        let mut child = root * 2 + 1;
        let right = child + 1;
        if right < end && keys[child].compare(keys[right]).is_lt() {
            child = right;
        }
        if !keys[root].compare(keys[child]).is_lt() {
            return Ok(());
        }
        keys.swap(root, child);
        root = child;
    }
}
