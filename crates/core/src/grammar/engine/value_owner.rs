// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Table-local ownership of finished codec trees with stable history indices.

use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchLease, ScratchMeter, ScratchVec, StopReason,
    invalid_input, push_scratch, saturating_size as size,
};
use crate::grammar::codec::CodecValue;
use crate::tool_call::ArgTree;

pub(super) struct OwnedValue {
    tree: ArgTree,
    _storage: ScratchLease,
}

struct Slot {
    owner: usize,
    next: Option<u32>,
    value: Option<OwnedValue>,
}

impl ScratchCharge for Slot {
    const CHARGE: usize = 320;
}
const _: () = assert!(core::mem::size_of::<Slot>() <= Slot::CHARGE);

#[derive(Clone, Copy)]
struct Head(Option<u32>);
impl ScratchCharge for Head {
    const CHARGE: usize = 16;
}
const _: () = assert!(core::mem::size_of::<Head>() <= Head::CHARGE);

/// Empty slots preserve every history ID until the whole arena resets.
pub(super) struct Values {
    slots: ScratchVec<Slot>,
    heads: ScratchVec<Head>,
    _metadata: ScratchLease,
}

impl Values {
    pub(super) fn new(count: usize, governor: &mut Governor) -> Result<Self, StopReason> {
        const METADATA_BYTES: u64 = 192;
        const _: () = assert!(
            core::mem::size_of::<(Values, ScratchLease, usize, Option<u32>,)>() as u64
                <= METADATA_BYTES
        );
        let metadata = governor.lease_scratch(METADATA_BYTES)?;
        governor.charge(Resource::Work, size(count))?;
        let mut heads = ScratchVec::new(governor);
        heads.reserve(count)?;
        heads.extend(core::iter::repeat_n(Head(None), count))?;
        Ok(Self {
            slots: ScratchVec::new(governor),
            heads,
            _metadata: metadata,
        })
    }

    pub(super) fn insert(
        &mut self,
        owner: usize,
        value: OwnedValue,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        governor.charge(Resource::Work, 2)?;
        let head = self
            .heads
            .get(owner)
            .ok_or_else(|| invalid_input("codec value has no table owner"))?
            .0;
        let index = u32::try_from(self.slots.len())
            .map_err(|_| governor.stop_resource(Resource::ScratchBytes))?;
        push_scratch(
            &mut self.slots,
            Slot {
                owner,
                next: head,
                value: Some(value),
            },
            governor,
        )?;
        self.heads.as_mut_slice()[owner].0 = Some(index);
        Ok(index)
    }

    pub(super) fn get(&self, owner: usize, index: u32) -> Result<&ArgTree, StopReason> {
        let slot = self
            .slots
            .get(index as usize)
            .ok_or_else(|| invalid_input("codec value index exceeds its arena"))?;
        if slot.owner != owner {
            return Err(invalid_input("codec value belongs to another table"));
        }
        slot.value
            .as_ref()
            .map(|value| &value.tree)
            .ok_or_else(|| invalid_input("codec value was read after its table retired"))
    }

    /// A vacant head certifies that this table has no retained tree payload.
    pub(super) fn has_values(&self, owner: usize) -> Result<bool, StopReason> {
        self.heads
            .get(owner)
            .map(|head| head.0.is_some())
            .ok_or_else(|| invalid_input("codec value query has no table owner"))
    }

    /// Every node in this intrusive list is visited and destroyed only once.
    pub(super) fn retire(
        &mut self,
        owner: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        let head = self
            .heads
            .as_mut_slice()
            .get_mut(owner)
            .ok_or_else(|| invalid_input("retired values have no table owner"))?;
        while let Some(index) = head.0 {
            governor.charge(Resource::Work, 3)?;
            let slot = self
                .slots
                .as_mut_slice()
                .get_mut(index as usize)
                .ok_or_else(|| invalid_input("retired value index exceeds its arena"))?;
            if slot.owner != owner || slot.value.is_none() {
                return Err(invalid_input("retired value list lost its unique owner"));
            }
            head.0 = slot.next;
            slot.next = None;
            drop(slot.value.take());
        }
        Ok(())
    }

    pub(super) fn reset(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        if self.slots.is_empty() {
            return Ok(());
        }
        governor.charge(Resource::Work, size(self.heads.len()))?;
        self.clear();
        self.heads.as_mut_slice().fill(Head(None));
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        self.slots.clear();
    }
}

/// Adopt the codec's existing manual grants before any fallible transfer.
/// A refused initial census still disposes of the complete unadopted value.
pub(super) struct Pending {
    pub(super) value: Option<CodecValue>,
    remaining: Option<u64>,
    meter: ScratchMeter,
}

impl Pending {
    pub(super) fn new(value: CodecValue, governor: &mut Governor) -> Result<Self, StopReason> {
        let mut pending = Self {
            value: Some(value),
            remaining: None,
            meter: governor.scratch_meter(),
        };
        let mut bytes = 0_u64;
        let mut next = pending.value.as_ref();
        while let Some(value) = next {
            governor.charge(Resource::Work, size(value.edits.len()).saturating_add(1))?;
            bytes = bytes.saturating_add(value.tree.estimated_bytes());
            for edit in &value.edits {
                bytes = bytes.saturating_add(edit.estimated_bytes());
            }
            next = value.alternative.as_deref();
        }
        pending.remaining = Some(bytes);
        Ok(pending)
    }

    pub(super) fn tree(
        &mut self,
        tree: ArgTree,
        governor: &Governor,
    ) -> Result<OwnedValue, StopReason> {
        let bytes = tree.estimated_bytes();
        self.transfer(bytes)?;
        Ok(OwnedValue {
            tree,
            _storage: governor.scratch_lease_for(bytes),
        })
    }

    pub(super) fn transfer(&mut self, bytes: u64) -> Result<(), StopReason> {
        self.remaining = Some(
            self.remaining
                .and_then(|remaining| remaining.checked_sub(bytes))
                .ok_or_else(|| invalid_input("codec transfer exceeds its unadopted grant"))?,
        );
        Ok(())
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        let bytes = self.remaining.unwrap_or_else(|| {
            // Cleanup after a refused census cannot ask the governor for work.
            let mut bytes = 0_u64;
            let mut next = self.value.as_ref();
            while let Some(value) = next {
                bytes = bytes.saturating_add(value.tree.estimated_bytes());
                for edit in &value.edits {
                    bytes = bytes.saturating_add(edit.estimated_bytes());
                }
                next = value.alternative.as_deref();
            }
            bytes
        });
        drop(self.value.take());
        let _ = self.meter.release(bytes);
    }
}
