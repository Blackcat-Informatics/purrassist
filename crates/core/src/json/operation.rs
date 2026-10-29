// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Shared ordinary and admitted storage/scans for exact JSON comparison.
use crate::contract::{Governor, Resource, ScratchString, StopReason, saturating_size};
use core::cmp::Ordering;

pub(super) struct Operation<'g>(pub(super) Option<&'g mut Governor>);

impl Operation<'_> {
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

// The same append authority serves decimal text and the comparison stack.
pub(super) enum Storage<P, A> {
    Plain(P),
    Admitted(A),
}
impl<P, A> Storage<P, A> {
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
pub(super) type Text = Storage<String, ScratchString>;
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

pub(super) fn ordinary<T>(result: Result<T, StopReason>) -> T {
    // Ordinary operations have neither an admission gate nor fallible storage.
    result.expect("INVARIANT: ordinary JSON comparison cannot produce a resource refusal")
}
