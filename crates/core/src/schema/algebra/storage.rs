// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Fallible preparation lists; copied elements keep their original ownership.
use super::super::reference::work;
use crate::contract::{Governor, ScratchCharge, ScratchVec, StopReason};

pub(super) use super::super::index::push;
pub(super) trait CopyIn: Sized {
    fn copy_in(&self, governor: &mut Governor) -> Result<Self, StopReason>;
}
pub(super) fn copy<T: CopyIn + ScratchCharge>(
    items: &[T],
    governor: &mut Governor,
) -> Result<ScratchVec<T>, StopReason> {
    let mut result = ScratchVec::new(governor);
    result.reserve(items.len())?;
    extend(&mut result, items, governor)?;
    Ok(result)
}
pub(super) fn extend<T: CopyIn + ScratchCharge>(
    target: &mut ScratchVec<T>,
    items: &[T],
    governor: &mut Governor,
) -> Result<(), StopReason> {
    for item in items {
        let item = item.copy_in(governor)?;
        push(target, item, governor)?;
    }
    Ok(())
}
impl<T: CopyIn + ScratchCharge> CopyIn for ScratchVec<T> {
    fn copy_in(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        copy(self, governor)
    }
}
impl ScratchCharge for &crate::json::Node {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 8);
        8
    };
}
impl ScratchCharge for &crate::json::Number {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 8);
        8
    };
}

impl<T: ?Sized> CopyIn for &T {
    fn copy_in(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        work(governor, 1)?;
        Ok(*self)
    }
}
macro_rules! copy_value {
    ($($ty:ty),+ $(,)?) => {$ (
        impl CopyIn for $ty {
            fn copy_in(&self, governor: &mut Governor) -> Result<Self, StopReason> {
                work(governor, 1)?;
                Ok(*self)
            }
        }
    )+};
}
copy_value!(
    bool,
    usize,
    u64,
    Option<u64>,
    super::canon::TypeSet,
    super::canon::PatternRef<'_>
);
impl<A: CopyIn, B: CopyIn> CopyIn for (A, B) {
    fn copy_in(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        Ok((self.0.copy_in(governor)?, self.1.copy_in(governor)?))
    }
}
impl<A: CopyIn, B: CopyIn, C: CopyIn> CopyIn for (A, B, C) {
    fn copy_in(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        Ok((
            self.0.copy_in(governor)?,
            self.1.copy_in(governor)?,
            self.2.copy_in(governor)?,
        ))
    }
}
pub(super) fn singleton<T: ScratchCharge>(
    value: T,
    governor: &mut Governor,
) -> Result<ScratchVec<T>, StopReason> {
    let mut output = ScratchVec::new(governor);
    push(&mut output, value, governor)?;
    Ok(output)
}
pub(super) fn equivalent_any(
    values: &[&crate::json::Node],
    value: &crate::json::Node,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    for other in values {
        work(governor, 1)?;
        if other.equivalent_governed(value, governor)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn list<T: ScratchCharge, const N: usize>(
    values: [T; N],
    governor: &mut Governor,
) -> Result<ScratchVec<T>, StopReason> {
    let mut result = ScratchVec::new(governor);
    result.reserve(N)?;
    for value in values {
        push(&mut result, value, governor)?;
    }
    Ok(result)
}

impl ScratchCharge for &[crate::json::Node] {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 16);
        16
    };
}
pub(super) fn depth(governor: &mut Governor, depth: u32) -> Result<(), StopReason> {
    governor.charge(crate::contract::Resource::Depth, u64::from(depth))?;
    if depth > 128 {
        return Err(governor.stop_resource(crate::contract::Resource::Depth));
    }
    Ok(())
}

impl ScratchCharge for Option<u64> {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 16);
        16
    };
}
impl<A: ScratchCharge, B: ScratchCharge, C: ScratchCharge> ScratchCharge for (A, B, C) {
    const CHARGE: usize = {
        let bytes = (A::CHARGE.div_ceil(8) + B::CHARGE.div_ceil(8) + C::CHARGE.div_ceil(8)) * 8;
        assert!(size_of::<Self>() <= bytes);
        bytes
    };
}
