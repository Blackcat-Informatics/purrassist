// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Shared ordinary and admitted storage and scans for first-party algorithms.
use crate::contract::{
    Buffer, Operation, Resource, ScratchCharge, ScratchString, ScratchVec, StopReason, Storage,
    Text, push_scratch, saturating_size,
};
use core::cmp::Ordering;

impl Operation<'_> {
    /// Bound binary table lookup comparisons before invoking the shared lookup.
    pub(super) fn lookup(&mut self, entries: usize) -> Result<(), StopReason> {
        self.work(entries.checked_ilog2().map_or(0, |bits| bits as usize + 1))
    }
    pub(super) fn buffer<T: ScratchCharge>(&self) -> Buffer<T> {
        Buffer(self.0.as_ref().map_or_else(
            || Storage::Plain(Vec::new()),
            |governor| Storage::Admitted(ScratchVec::new(governor)),
        ))
    }
    pub(super) fn push_buffer<T: ScratchCharge>(
        &mut self,
        buffer: &mut Buffer<T>,
        value: T,
    ) -> Result<(), StopReason> {
        match (&mut buffer.0, &mut self.0) {
            (Storage::Plain(values), None) => {
                values.push(value);
                Ok(())
            }
            (Storage::Admitted(values), Some(governor)) => push_scratch(values, value, governor),
            _ => unreachable!("INVARIANT: buffer and operation admission agree"),
        }
    }
    pub(super) fn work(&mut self, bytes: usize) -> Result<(), StopReason> {
        if let Some(governor) = &mut self.0 {
            governor.charge(Resource::Work, saturating_size(bytes).saturating_add(1))?;
        }
        Ok(())
    }
    pub(super) fn depth(&mut self, depth: u64) -> Result<(), StopReason> {
        if let Some(governor) = &mut self.0 {
            governor.charge(Resource::Depth, depth)?;
        }
        Ok(())
    }
    pub(super) fn check(&self) -> Result<(), StopReason> {
        if let Some(governor) = &self.0 {
            governor.check()?;
        }
        Ok(())
    }
    pub(super) fn text(&self) -> Text {
        self.0.as_ref().map_or_else(
            || Text::Plain(String::new()),
            |governor| Text::Admitted(ScratchString::new(governor)),
        )
    }
    pub(super) fn copy(&mut self, value: &str) -> Result<Text, StopReason> {
        self.work(value.len())?;
        let mut text = self.text();
        text.push_str(value)?;
        Ok(text)
    }
    /// Admit relocation before an arithmetic append grows its existing buffer.
    pub(super) fn push(&mut self, text: &mut Text, value: char) -> Result<(), StopReason> {
        let capacity = match text {
            Text::Plain(text) => text.capacity(),
            Text::Admitted(text) => text.capacity(),
        };
        if value.len_utf8() > capacity - text.as_str().len() {
            self.work(text.as_str().len())?;
        }
        text.push(value)
    }
    pub(super) fn count(&mut self, count: usize) -> Result<Text, StopReason> {
        self.work(40)?;
        self.0.as_mut().map_or_else(
            || Ok(Text::Plain(count.to_string())),
            |governor| {
                crate::contract::format_scratch_governed(format_args!("{count}"), governor)
                    .map(Text::Admitted)
            },
        )
    }
    pub(super) fn compare(&mut self, left: &str, right: &str) -> Result<Ordering, StopReason> {
        self.work(left.len().min(right.len()))?;
        Ok(left.cmp(right))
    }
}

impl<P, A> Storage<P, A> {
    pub(super) fn into_admitted(self) -> A {
        match self {
            Self::Admitted(owner) => owner,
            Self::Plain(_) => {
                unreachable!("INVARIANT: governed operation returns admitted storage")
            }
        }
    }
    pub(super) fn append<V>(
        &mut self,
        value: V,
        plain: impl FnOnce(&mut P, V),
        admitted: impl FnOnce(&mut A, V) -> Result<(), StopReason>,
    ) -> Result<(), StopReason> {
        match self {
            Self::Plain(storage) => {
                plain(storage, value);
                Ok(())
            }
            Self::Admitted(storage) => admitted(storage, value),
        }
    }
}
impl<T: ScratchCharge> Buffer<T> {
    pub(super) fn into_admitted(self) -> ScratchVec<T> {
        self.0.into_admitted()
    }
    pub(super) fn as_slice(&self) -> &[T] {
        match &self.0 {
            Storage::Plain(values) => values,
            Storage::Admitted(values) => values.as_slice(),
        }
    }
    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        match &mut self.0 {
            Storage::Plain(values) => values,
            Storage::Admitted(values) => values.as_mut_slice(),
        }
    }
    pub(super) fn pop(&mut self) -> Option<T> {
        match &mut self.0 {
            Storage::Plain(values) => values.pop(),
            Storage::Admitted(values) => values.pop(),
        }
    }
    pub(super) fn clear(&mut self) {
        match &mut self.0 {
            Storage::Plain(values) => values.clear(),
            Storage::Admitted(values) => values.clear(),
        }
    }
}
impl Text {
    pub(super) fn as_str(&self) -> &str {
        match self {
            Self::Plain(text) => text,
            Self::Admitted(text) => text.as_str(),
        }
    }
    pub(super) fn push_str(&mut self, value: &str) -> Result<(), StopReason> {
        match self {
            Self::Plain(text) => {
                text.push_str(value);
                Ok(())
            }
            Self::Admitted(text) => text.push_str(value),
        }
    }
    pub(super) fn push(&mut self, value: char) -> Result<(), StopReason> {
        self.append(value, String::push, ScratchString::push)
    }
}
