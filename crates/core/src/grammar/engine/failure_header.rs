// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Selected failed-body facts, completed before histories and source retire.

use super::{Attempt, Engine, Failure, FailureOrigin, HistKind, Inst, Sub};
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchMap, ScratchVec, StopReason, invalid_input,
    push_scratch, saturating_size as size,
};
use crate::grammar::{CaptureKind, codec::CodecKind};
use crate::tool_call::{DialectId, ProductionKind, Rejection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeaderSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeaderFacts {
    pub(crate) name: HeaderSpan,
    pub(crate) id: Option<HeaderSpan>,
    pub(crate) arguments: usize,
    pub(crate) first_retained: usize,
    pub(crate) dropped: Option<HeaderSpan>,
    pub(crate) consumed_to: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FailureWitness {
    pub(crate) failed_at: usize,
    pub(crate) reason: Rejection,
    pub(crate) end: usize,
    pub(crate) header: HeaderFacts,
    pub(crate) native: Option<HeaderSpan>,
}

#[derive(Clone, Copy, Debug)]
struct Selected {
    at: usize,
    reason: Rejection,
    end: usize,
    consumed_to: usize,
    header: Option<HeaderFacts>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct State {
    native: Option<HeaderSpan>,
    selected: Option<Selected>,
}

impl ScratchCharge for State {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 192);
        192
    };
}

/// Sorted-map queries and worst-case insertion/removal movement are admitted.
fn map_work(length: usize, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(
        Resource::Work,
        size(length)
            .saturating_mul(32)
            .saturating_add(u64::from(length.bit_width() + 1).saturating_mul(4))
            .saturating_add(64),
    )
}

pub(super) fn native(
    states: &mut ScratchMap<u64, State>,
    attempt: u64,
    span: HeaderSpan,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    map_work(states.len(), governor)?;
    let mut state = states.get(&attempt).copied().unwrap_or_default();
    state.native = Some(span);
    states.insert(attempt, state)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prefix {
    name: Option<HeaderSpan>,
    id: Option<HeaderSpan>,
    dropped: Option<HeaderSpan>,
    active: Option<(CaptureKind, usize)>,
    first: usize,
}

impl Default for Prefix {
    fn default() -> Self {
        Self {
            name: None,
            id: None,
            dropped: None,
            active: None,
            first: usize::MAX,
        }
    }
}

impl ScratchCharge for Option<Prefix> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 160);
        160
    };
}

impl Prefix {
    fn apply(mut self, kind: HistKind, start: usize) -> Option<Self> {
        match kind {
            HistKind::Open(kind, at) => {
                if self.active.is_some() {
                    return None;
                }
                match kind {
                    CaptureKind::Dropped
                        if self.dropped.is_none() && self.first == usize::MAX && at == start => {}
                    CaptureKind::Name if self.name.is_none() => self.first = self.first.min(at),
                    CaptureKind::Id if self.id.is_none() => self.first = self.first.min(at),
                    CaptureKind::Arguments if self.name.is_some() => {
                        self.first = self.first.min(at);
                    }
                    _ => return None,
                }
                self.active = Some((kind, at));
            }
            HistKind::Close(kind, end) => {
                let (opened, start) = self.active?;
                if opened != kind || start >= end {
                    return None;
                }
                let span = HeaderSpan { start, end };
                match kind {
                    CaptureKind::Dropped => self.dropped = Some(span),
                    CaptureKind::Name => self.name = Some(span),
                    CaptureKind::Id => self.id = Some(span),
                    _ => return None,
                }
                self.active = None;
            }
            _ => return None,
        }
        Some(self)
    }
}

impl Engine<'_> {
    /// A refused qualification drops materialized salvage before its charge.
    pub(super) fn finalize_failure_end(
        &mut self,
        event: super::Event,
        extended: bool,
        governor: &mut Governor,
    ) -> Result<super::Event, StopReason> {
        if let Err(reason) = self.qualify_failure_end(&event, extended, governor) {
            let charged = match &event {
                super::Event::Ended {
                    salvage: Some(salvage),
                    ..
                } => salvage.charged,
                _ => 0,
            };
            drop(event);
            let _ = governor.release_scratch(charged);
            return Err(reason);
        }
        Ok(event)
    }

    fn qualify_failure_end(
        &mut self,
        event: &super::Event,
        extended: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if self.failure_headers.is_empty() {
            return Ok(());
        }
        let super::Event::Ended {
            attempt,
            start,
            at,
            completed,
            reason,
            salvage,
            ..
        } = event
        else {
            return Err(invalid_input(
                "failure qualification requires an Ended report",
            ));
        };
        map_work(self.failure_headers.len(), governor)?;
        let Some(state) = self.failure_headers.get(attempt) else {
            return Ok(());
        };
        governor.charge(Resource::Work, 8)?;
        let valid = !*completed
            && salvage.is_none()
            && !extended
            && state.selected.is_some_and(|selected| {
                selected.header.is_some() && selected.end == *at && selected.reason == *reason
            })
            && state.native.is_none_or(|frame| frame.start == *start);
        if !valid {
            self.forget_failure_header(*attempt, governor)?;
        }
        Ok(())
    }

    /// A complete sparse owner survives queued Ended reports by attempt ID.
    pub(crate) fn forget_failure_header(
        &mut self,
        attempt: u64,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if self.failure_headers.is_empty() {
            return Ok(());
        }
        map_work(self.failure_headers.len(), governor)?;
        self.failure_headers.remove(&attempt);
        Ok(())
    }

    pub(crate) fn discard_failure_headers(&mut self, governor: &Governor) {
        let old = core::mem::replace(&mut self.failure_headers, ScratchMap::new(governor));
        drop(old);
    }

    pub(crate) fn take_failure_header(
        &mut self,
        attempt: u64,
        governor: &mut Governor,
    ) -> Result<Option<FailureWitness>, StopReason> {
        if self.failure_headers.is_empty() {
            return Ok(None);
        }
        map_work(self.failure_headers.len(), governor)?;
        let Some(state) = self.failure_headers.remove(&attempt) else {
            return Ok(None);
        };
        Ok(state.selected.and_then(|selected| {
            selected.header.map(|header| FailureWitness {
                failed_at: selected.at,
                reason: selected.reason,
                end: selected.end,
                header,
                native: state.native,
            })
        }))
    }

    /// This runs only when the actual selected failure changes, never per byte.
    pub(super) fn note_failure_header<'f>(
        &mut self,
        table: usize,
        record: &Attempt,
        failures: &[Failure],
        first: &'f Failure,
        end: usize,
        governor: &mut Governor,
    ) -> Result<Option<(&'f Failure, usize)>, StopReason> {
        if self.tables[table].kind != ProductionKind::Call
            || !matches!(
                self.dialect(super::index_u32(table)),
                DialectId::MistralV3 | DialectId::HermesStripped
            )
        {
            return Ok(None);
        }
        if !record.opened {
            // No unopened contributor can certify; decide retention only
            // after actual repair and resynchronization have finished.
            governor.charge(Resource::Work, 1)?;
            return Ok(Some((first, end)));
        }
        let mut header = None;
        let mut seen = false;
        for failure in failures.iter().filter(|failure| !failure.respell) {
            governor.charge(Resource::Work, 3)?;
            if (failure.at, failure.reason) != (first.at, first.reason) {
                continue;
            }
            let next = self.certify_failure(table, failure, end, governor)?;
            if seen {
                header = intersect(header, next, governor)?;
            } else {
                header = next;
                seen = true;
            }
        }
        self.retain_failure_header(record.id, first, end, header, governor)?;
        Ok(None)
    }

    /// One persistence path for proved headers and surviving negative selections.
    pub(super) fn retain_failure_header(
        &mut self,
        attempt: u64,
        first: &Failure,
        end: usize,
        header: Option<HeaderFacts>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut selected = Selected {
            at: first.at,
            reason: first.reason,
            end,
            consumed_to: first.thread.pos,
            header,
        };
        map_work(self.failure_headers.len(), governor)?;
        let mut state = self
            .failure_headers
            .get(&attempt)
            .copied()
            .unwrap_or_default();
        if let Some(previous) = state.selected
            && (previous.at, previous.reason) == (selected.at, selected.reason)
        {
            governor.charge(Resource::Work, 3)?;
            selected.header = if previous.end == end && previous.consumed_to == selected.consumed_to
            {
                intersect(previous.header, selected.header, governor)?
            } else {
                None
            };
        }
        state.selected = Some(selected);
        self.failure_headers.insert(attempt, state)?;
        Ok(())
    }

    /// The producer tag identifies the consumed-codec Unread route precisely.
    fn certify_failure(
        &self,
        table: usize,
        failure: &Failure,
        end: usize,
        governor: &mut Governor,
    ) -> Result<Option<HeaderFacts>, StopReason> {
        governor.charge(
            Resource::Work,
            size(self.tables[table].attempts.len()).saturating_add(12),
        )?;
        let thread = failure.thread;
        let opened = self.tables[table]
            .attempts
            .iter()
            .any(|attempt| attempt.id == thread.attempt && attempt.opened);
        if failure.origin != FailureOrigin::UnreadBody
            || !opened
            || thread.pending != 0
            || thread.speculative()
            || end != failure.at
            || failure.at >= thread.pos
            || failure.reason != (Rejection::Malformed { at: failure.at })
            || !matches!(
                self.tables[table].program.insts[thread.pc as usize],
                Inst::Slot {
                    codec: CodecKind::Json | CodecKind::JsonArgs,
                    ..
                }
            )
            || !matches!(thread.sub, Sub::Body(_, Some(_)))
        {
            return Ok(None);
        }
        governor.charge(Resource::Work, 1)?;
        let byte = thread
            .entry
            .checked_sub(self.base)
            .and_then(|offset| self.buffer.as_slice().get(offset))
            .ok_or_else(|| invalid_input("failed argument source retired before header proof"))?;
        if *byte != b'{' {
            return Ok(None);
        }
        let Some(prefix) = self.failure_prefix(thread.hist, thread.start, governor)? else {
            return Ok(None);
        };
        let (Some(name), Some((CaptureKind::Arguments, arguments))) = (prefix.name, prefix.active)
        else {
            return Ok(None);
        };
        if arguments != thread.entry
            || name.end > arguments
            || prefix.id.is_some_and(|id| id.end > arguments)
            || prefix
                .dropped
                .is_some_and(|dropped| dropped.end > prefix.first)
        {
            return Ok(None);
        }
        Ok(Some(HeaderFacts {
            name,
            id: prefix.id,
            arguments,
            first_retained: prefix.first,
            dropped: prefix.dropped,
            consumed_to: thread.pos,
        }))
    }

    /// Memoized prefix summaries prove all Pack branches without path expansion.
    /// Map shifts are charged, including the conservative quadratic worst case.
    fn failure_prefix(
        &self,
        root: u32,
        start: usize,
        governor: &mut Governor,
    ) -> Result<Option<Prefix>, StopReason> {
        let mut memo: ScratchMap<u32, Option<Prefix>> = ScratchMap::new(governor);
        let mut stack = ScratchVec::new(governor);
        push_scratch(&mut stack, (root, false), governor)?;
        while let Some((node, finish)) = stack.pop() {
            map_work(memo.len(), governor)?;
            if memo.get(&node).is_some() {
                continue;
            }
            governor.charge(Resource::Work, 1)?;
            let history = self
                .arena
                .nodes
                .as_slice()
                .get(node as usize)
                .ok_or_else(|| invalid_input("failed header history is outside its arena"))?;
            if matches!(
                history.kind,
                HistKind::Value(_)
                    | HistKind::Edit(_)
                    | HistKind::Link
                    | HistKind::Next(..)
                    | HistKind::Skipped(_)
            ) {
                map_work(memo.len(), governor)?;
                memo.insert(node, None)?;
                continue;
            }
            if !finish && !matches!(history.kind, HistKind::Root) {
                push_scratch(&mut stack, (node, true), governor)?;
                if let HistKind::Pack(other) = history.kind {
                    push_scratch(&mut stack, (other, false), governor)?;
                }
                push_scratch(&mut stack, (history.prev, false), governor)?;
                continue;
            }
            let summary = Self::failure_prefix_node(history, &memo, start, governor)?;
            map_work(memo.len(), governor)?;
            memo.insert(node, summary)?;
        }
        map_work(memo.len(), governor)?;
        memo.get(&root)
            .copied()
            .ok_or_else(|| invalid_input("failed header proof did not reach its root"))
    }

    fn failure_prefix_node(
        history: &super::HistNode,
        memo: &ScratchMap<u32, Option<Prefix>>,
        start: usize,
        governor: &mut Governor,
    ) -> Result<Option<Prefix>, StopReason> {
        if matches!(history.kind, HistKind::Root) {
            return Ok(Some(Prefix::default()));
        }
        map_work(memo.len(), governor)?;
        let previous = memo
            .get(&history.prev)
            .copied()
            .ok_or_else(|| invalid_input("failed header predecessor was not proved"))?;
        if let HistKind::Pack(other) = history.kind {
            map_work(memo.len(), governor)?;
            let branch = memo
                .get(&other)
                .copied()
                .ok_or_else(|| invalid_input("failed header packed branch was not proved"))?;
            governor.charge(Resource::Work, 16)?;
            return Ok(if previous == branch { previous } else { None });
        }
        governor.charge(Resource::Work, 8)?;
        Ok(previous.and_then(|prefix| prefix.apply(history.kind, start)))
    }
}

fn intersect(
    left: Option<HeaderFacts>,
    right: Option<HeaderFacts>,
    governor: &mut Governor,
) -> Result<Option<HeaderFacts>, StopReason> {
    governor.charge(Resource::Work, 12)?;
    Ok(match (left, right) {
        (Some(left), Some(right)) if left == right => Some(left),
        _ => None,
    })
}
