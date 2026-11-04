// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Compact native capture identities retained across semantic resolution.
use super::{ArgLeaf, CallSegment, Closure, RawCall, ResolvedSegment, Spanned};
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchLease, ScratchMap, ScratchString, ScratchVec,
    StopReason, invalid_input, push_scratch, saturating_size as size,
};
use crate::grammar::{CaptureKind, edit::EditOperator};
use core::ops::Range;

/// Structural coordinates, never a source-span join: piece, branch, segment,
/// and local embedded position. Zero denotes an ordinary segment/call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CallSite(pub(super) [usize; 4]);

impl CallSite {
    pub(super) fn embedded(self, index: usize) -> Result<Self, StopReason> {
        let mut site = self;
        site.0[3] = index
            .checked_add(1)
            .ok_or_else(|| invalid_input("embedded capture identity overflows"))?;
        Ok(site)
    }
}

struct TextFact {
    span: Range<usize>,
    value: ScratchString,
}

impl TextFact {
    fn copy(value: &Spanned<String>, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(Resource::Work, 1)?;
        Ok(Self {
            span: value.span.clone(),
            value: ScratchString::copy_from(&value.value, governor)?,
        })
    }
}

/// One real raw reading. Argument trees and native forests are never retained.
pub(super) struct NativeFact {
    pub(super) span: Range<usize>,
    name: Option<TextFact>,
    id: Option<TextFact>,
    pub(super) arguments: ScratchVec<(usize, usize)>,
    pub(super) edits: ScratchVec<EditFact>,
    pub(super) closure: Closure,
}

impl ScratchCharge for NativeFact {
    const CHARGE: usize = 256;
}
const _: () = assert!(core::mem::size_of::<NativeFact>() <= NativeFact::CHARGE);

impl NativeFact {
    fn copy(raw: &RawCall, governor: &mut Governor) -> Result<Self, StopReason> {
        let mut arguments = ScratchVec::new(governor);
        for capture in &raw.captures {
            governor.charge(Resource::Work, 1)?;
            if capture.kind == CaptureKind::Arguments {
                push_scratch(
                    &mut arguments,
                    (capture.span.start, capture.span.end),
                    governor,
                )?;
            }
        }
        let mut edits = ScratchVec::new(governor);
        if !raw.edits.is_empty() {
            let leaves = LeafPointers::new(&raw.arguments.leaves, governor)?;
            for edit in &raw.edits {
                governor.charge(Resource::Work, 1)?;
                let parameter = leaves.parameter(&edit.span, governor)?;
                push_scratch(
                    &mut edits,
                    EditFact {
                        operator: edit.operator,
                        at: edit.span.start,
                        end: edit.span.end,
                        before: copy_bytes(&edit.original, governor)?,
                        after: copy_bytes(&edit.replacement, governor)?,
                        parameter: parameter
                            .map(|value| ScratchString::copy_from(value, governor))
                            .transpose()?,
                    },
                    governor,
                )?;
            }
        }
        Ok(Self {
            span: raw.span.clone(),
            name: raw
                .name
                .as_ref()
                .map(|value| TextFact::copy(value, governor))
                .transpose()?,
            id: raw
                .id
                .as_ref()
                .map(|value| TextFact::copy(value, governor))
                .transpose()?,
            arguments,
            edits,
            closure: raw.closure,
        })
    }

    pub(super) fn name_start(&self) -> usize {
        self.name
            .as_ref()
            .map_or(self.span.start, |value| value.span.start)
    }
    pub(super) fn name_end(&self) -> usize {
        self.name
            .as_ref()
            .map_or(self.span.end, |value| value.span.end)
    }
    pub(super) fn argument_start(&self) -> usize {
        self.arguments
            .first()
            .map_or(self.span.end, |&(start, _)| start)
    }

    pub(super) fn header_difference(
        &self,
        other: &Self,
        governor: &mut Governor,
    ) -> Result<Option<usize>, StopReason> {
        let mut limit = None;
        for (left, right) in [(&self.name, &other.name), (&self.id, &other.id)] {
            governor.charge(Resource::Work, 1)?;
            let equal = match (left, right) {
                (Some(a), Some(b)) => {
                    governor.charge(
                        Resource::Work,
                        size(a.value.len()).saturating_add(size(b.value.len())),
                    )?;
                    a.span == b.span && a.value == b.value
                }
                (None, None) => true,
                _ => false,
            };
            if !equal {
                let at = left
                    .as_ref()
                    .into_iter()
                    .chain(right.as_ref())
                    .map(|value| value.span.start)
                    .min()
                    .unwrap_or(self.span.start);
                limit = Some(limit.map_or(at, |known: usize| known.min(at)));
            }
        }
        Ok(limit)
    }
}

/// A call's public header still comes from raw zero. Semantic origins follow
/// the exact winning derivation and ordered alternatives independently.
pub(super) struct CallTrace {
    pub(super) site: CallSite,
    pub(super) header: usize,
    pub(super) readings: ScratchVec<usize>,
    pub(super) competing: usize,
    raw_end: usize,
}
impl ScratchCharge for CallTrace {
    const CHARGE: usize = 96;
}
const _: () = assert!(core::mem::size_of::<CallTrace>() <= CallTrace::CHARGE);

pub(super) struct NativeTrace {
    calls: ScratchVec<CallTrace>,
    facts: ScratchVec<NativeFact>,
}

const TRACE_MAP_METADATA_BYTES: u64 = 192;
const _: () = assert!(
    core::mem::size_of::<(
        ScratchMap<usize, usize>,
        ScratchVec<usize>,
        ScratchLease,
        Option<usize>,
        usize,
        usize,
        usize,
        core::slice::Iter<'static, usize>,
        &'static RawCall,
        &'static RawCall,
    )>() as u64
        <= TRACE_MAP_METADATA_BYTES
);

impl NativeTrace {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            calls: ScratchVec::new(governor),
            facts: ScratchVec::new(governor),
        }
    }

    pub(super) fn record(
        &mut self,
        site: CallSite,
        call: &CallSegment,
        order: &[usize],
        competing: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if self.calls.last().is_some_and(|last| last.site >= site)
            || order.is_empty()
            || competing == 0
            || competing > order.len()
        {
            return Err(invalid_input(
                "native call correlation order is inconsistent",
            ));
        }
        let _mapping_metadata = governor.lease_scratch(TRACE_MAP_METADATA_BYTES)?;
        let mut identities = ScratchMap::new(governor);
        let header = self.facts.len();
        let raw_header = call
            .readings
            .first()
            .ok_or_else(|| invalid_input("native call has no header reading"))?;
        let fact = NativeFact::copy(raw_header, governor)?;
        push_scratch(&mut self.facts, fact, governor)?;
        governor.charge(Resource::Work, 4)?;
        identities.insert(0, header)?;
        let mut readings = ScratchVec::new(governor);
        for &raw in order {
            governor.charge(
                Resource::Work,
                u64::from(identities.len().bit_width()).saturating_add(3),
            )?;
            let origin = if let Some(&origin) = identities.get(&raw) {
                origin
            } else {
                let reading = call
                    .readings
                    .get(raw)
                    .ok_or_else(|| invalid_input("semantic reading has no native origin"))?;
                let origin = self.facts.len();
                let fact = NativeFact::copy(reading, governor)?;
                push_scratch(&mut self.facts, fact, governor)?;
                governor.charge(
                    Resource::Work,
                    size(identities.len())
                        .saturating_mul(2)
                        .saturating_add(u64::from(identities.len().bit_width()))
                        .saturating_add(4),
                )?;
                identities.insert(raw, origin)?;
                origin
            };
            push_scratch(&mut readings, origin, governor)?;
        }
        push_scratch(
            &mut self.calls,
            CallTrace {
                site,
                header,
                readings,
                competing,
                raw_end: self.facts.len(),
            },
            governor,
        )
    }

    pub(super) fn discard(
        &mut self,
        site: CallSite,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let last = self
            .calls
            .last()
            .ok_or_else(|| invalid_input("suppressed call has no native correlation"))?;
        governor.charge(
            Resource::Work,
            size(self.facts.len().saturating_sub(last.header)).saturating_add(1),
        )?;
        let row = self
            .calls
            .pop()
            .ok_or_else(|| invalid_input("suppressed call has no native correlation"))?;
        if row.site != site || row.raw_end != self.facts.len() {
            return Err(invalid_input(
                "suppressed call correlation is not the latest owner",
            ));
        }
        while self.facts.len() > row.header {
            let _ = self.facts.pop();
        }
        Ok(())
    }

    pub(super) fn lookup(
        &self,
        site: CallSite,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        governor.charge(
            Resource::Work,
            u64::from(self.calls.len().bit_width())
                .saturating_add(1)
                .saturating_mul(4),
        )?;
        let index = self.calls.partition_point(|row| row.site < site);
        if self.calls.get(index).is_none_or(|row| row.site != site) {
            return Err(invalid_input(
                "resolved call has no structural capture identity",
            ));
        }
        Ok(index)
    }

    pub(super) fn call(&self, id: usize) -> Result<&CallTrace, StopReason> {
        self.calls
            .get(id)
            .ok_or_else(|| invalid_input("native call identity is out of range"))
    }
    pub(super) fn fact(&self, id: usize) -> Result<&NativeFact, StopReason> {
        self.facts
            .get(id)
            .ok_or_else(|| invalid_input("native raw identity is out of range"))
    }
}

/// One retained raw source identity and its role after whole-option selection.
#[derive(Clone, Copy)]
pub(super) struct Origin {
    pub(super) id: usize,
    /// False only for a witness suppressed by complete source-frame dominance.
    pub(super) competing: bool,
}

impl ScratchCharge for Origin {
    const CHARGE: usize = 16;
}
const _: () = assert!(core::mem::size_of::<Origin>() <= Origin::CHARGE);

// Added insertion carriers and role controls remain admitted through refusal.
const ORIGIN_ROLE_METADATA_BYTES: u64 = 48;
const _: () = assert!(
    core::mem::size_of::<(ScratchLease, Origin, bool, bool, Option<&mut Origin>)>() as u64
        <= ORIGIN_ROLE_METADATA_BYTES
);

/// Only correspondence proven by the existing fold/merge admits this union.
pub(super) struct Origins(pub(super) ScratchVec<Origin>);
impl ScratchCharge for Origins {
    const CHARGE: usize = 40;
}
const _: () = assert!(core::mem::size_of::<Origins>() <= Origins::CHARGE);

impl Origins {
    pub(super) fn one(id: usize, governor: &mut Governor) -> Result<Self, StopReason> {
        let _metadata = governor.lease_scratch(ORIGIN_ROLE_METADATA_BYTES)?;
        let mut values = ScratchVec::new(governor);
        push_scratch(
            &mut values,
            Origin {
                id,
                competing: true,
            },
            governor,
        )?;
        Ok(Self(values))
    }

    /// The first row is the selected provenance; never substitute a witness.
    pub(super) fn selected(&self, governor: &mut Governor) -> Result<usize, StopReason> {
        governor.charge(Resource::Work, 1)?;
        let first = self
            .0
            .first()
            .ok_or_else(|| invalid_input("final call has no native origin"))?;
        if !first.competing {
            return Err(invalid_input(
                "selected native origin is a dominated witness",
            ));
        }
        Ok(first.id)
    }

    /// Ordinary unions preserve incoming roles. A dominance union marks only
    /// incoming witnesses; an already genuine competitor is never demoted.
    pub(super) fn union(
        &mut self,
        other: &Self,
        competing: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let _metadata = governor.lease_scratch(ORIGIN_ROLE_METADATA_BYTES)?;
        for origin in &other.0 {
            governor.charge(Resource::Work, size(self.0.len()).saturating_add(4))?;
            let active = competing && origin.competing;
            if let Some(known) = self
                .0
                .as_mut_slice()
                .iter_mut()
                .find(|known| known.id == origin.id)
            {
                known.competing |= active;
            } else {
                push_scratch(
                    &mut self.0,
                    Origin {
                        id: origin.id,
                        competing: active,
                    },
                    governor,
                )?;
            }
        }
        Ok(())
    }
    pub(super) fn copy(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        let mut copied = Self(ScratchVec::new(governor));
        copied.union(self, true, governor)?;
        Ok(copied)
    }
}

pub(super) struct OriginRow {
    pub(super) site: CallSite,
    pub(super) origins: Origins,
}
impl ScratchCharge for OriginRow {
    const CHARGE: usize = 72;
}
const _: () = assert!(core::mem::size_of::<OriginRow>() <= OriginRow::CHARGE);

pub(super) struct FinalOrigins {
    pub(super) calls: ScratchVec<Origins>,
    pub(super) embedded: ScratchVec<Origins>,
}
impl ScratchCharge for FinalOrigins {
    const CHARGE: usize = 80;
}
const _: () = assert!(core::mem::size_of::<FinalOrigins>() <= FinalOrigins::CHARGE);

pub(super) fn option_origins(
    segments: &[ResolvedSegment],
    base: CallSite,
    trace: &NativeTrace,
    governor: &mut Governor,
) -> Result<ScratchVec<OriginRow>, StopReason> {
    let mut rows = ScratchVec::new(governor);
    for (index, segment) in segments.iter().enumerate() {
        let mut site = base;
        site.0[2] = index;
        match segment {
            ResolvedSegment::Call(_) => add_origin(&mut rows, site, trace, governor)?,
            ResolvedSegment::Reasoning { embedded, .. } => {
                for index in 0..embedded.len() {
                    add_origin(&mut rows, site.embedded(index)?, trace, governor)?;
                }
            }
            _ => {
                governor.charge(Resource::Work, 1)?;
            }
        }
    }
    Ok(rows)
}

fn add_origin(
    rows: &mut ScratchVec<OriginRow>,
    site: CallSite,
    trace: &NativeTrace,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let id = trace.lookup(site, governor)?;
    let origins = Origins::one(id, governor)?;
    push_scratch(rows, OriginRow { site, origins }, governor)
}

pub(super) fn copy_bytes(
    bytes: &[u8],
    governor: &mut Governor,
) -> Result<ScratchVec<u8>, StopReason> {
    governor.charge(Resource::Work, size(bytes.len()).saturating_add(1))?;
    let mut copied = ScratchVec::new(governor);
    copied.reserve(bytes.len())?;
    copied.extend(bytes.iter().copied())?;
    Ok(copied)
}

/// One original leaf position and the greatest end in its balanced subtree.
#[derive(Clone, Copy)]
struct LeafInterval {
    original: usize,
    max_end: usize,
}

impl ScratchCharge for LeafInterval {
    const CHARGE: usize = 16;
}
const _: () = assert!(core::mem::size_of::<LeafInterval>() <= LeafInterval::CHARGE);

/// Borrowed source intervals prepared once per raw reading with edit payloads.
/// Every enclosing leaf must agree; overlapping different pointers stay unknown.
struct LeafPointers<'a> {
    leaves: &'a [ArgLeaf],
    intervals: ScratchVec<LeafInterval>,
}

impl<'a> LeafPointers<'a> {
    fn new(leaves: &'a [ArgLeaf], governor: &mut Governor) -> Result<Self, StopReason> {
        let mut intervals = ScratchVec::new(governor);
        intervals.reserve(leaves.len())?;
        for (original, leaf) in leaves.iter().enumerate() {
            push_scratch(
                &mut intervals,
                LeafInterval {
                    original,
                    max_end: leaf.raw.end,
                },
                governor,
            )?;
        }
        let levels = intervals.len().bit_width() + 1;
        governor.charge(
            Resource::Work,
            size(intervals.len())
                .saturating_mul(u64::from(levels))
                .saturating_mul(3),
        )?;
        intervals.as_mut_slice().sort_unstable_by_key(|interval| {
            let leaf = &leaves[interval.original];
            (leaf.raw.start, leaf.raw.end, interval.original)
        });
        let mut index = Self { leaves, intervals };
        index.prepare(0, index.intervals.len(), governor)?;
        Ok(index)
    }

    /// Halving the index range bounds native recursion by the index's bit width.
    fn prepare(
        &mut self,
        low: usize,
        high: usize,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        governor.charge(Resource::Work, 1)?;
        if low == high {
            return Ok(0);
        }
        let middle = low + (high - low) / 2;
        let end = self.intervals[middle].max_end;
        let left = self.prepare(low, middle, governor)?;
        let right = self.prepare(middle + 1, high, governor)?;
        let end = end.max(left).max(right);
        self.intervals.as_mut_slice()[middle].max_end = end;
        Ok(end)
    }

    fn parameter(
        &self,
        span: &Range<usize>,
        governor: &mut Governor,
    ) -> Result<Option<&'a str>, StopReason> {
        let mut found = None;
        let unanimous = self.enclosing(0, self.intervals.len(), span, &mut found, governor)?;
        Ok(if unanimous { found } else { None })
    }

    fn enclosing(
        &self,
        low: usize,
        high: usize,
        span: &Range<usize>,
        found: &mut Option<&'a str>,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        governor.charge(Resource::Work, 1)?;
        if low == high {
            return Ok(true);
        }
        let middle = low + (high - low) / 2;
        let interval = self.intervals[middle];
        let first = &self.leaves[self.intervals[low].original];
        // Strict comparisons preserve inclusive empty-span endpoints: leaves
        // ending and beginning at an insertion can both enclose that insertion.
        if interval.max_end < span.end || first.raw.start > span.start {
            return Ok(true);
        }
        if !self.enclosing(low, middle, span, found, governor)? {
            return Ok(false);
        }
        let leaf = &self.leaves[interval.original];
        if leaf.raw.start <= span.start && span.end <= leaf.raw.end {
            if let Some(known) = *found {
                governor.charge(
                    Resource::Work,
                    size(known.len().min(leaf.pointer.len())).saturating_add(1),
                )?;
                if known != leaf.pointer.as_str() {
                    return Ok(false);
                }
            }
            *found = Some(leaf.pointer.as_str());
        }
        self.enclosing(middle + 1, high, span, found, governor)
    }
}

/// Every native edit in its original order, including silent operators.
/// Payloads are never reconstructed from explanations or final value trees;
/// empty payload owners allocate no storage.
#[derive(Debug)]
pub(super) struct EditFact {
    pub(super) operator: EditOperator,
    pub(super) at: usize,
    pub(super) end: usize,
    pub(super) before: ScratchVec<u8>,
    pub(super) after: ScratchVec<u8>,
    pub(super) parameter: Option<ScratchString>,
}

impl ScratchCharge for EditFact {
    const CHARGE: usize = 192;
}
const _: () = assert!(core::mem::size_of::<EditFact>() <= EditFact::CHARGE);

impl EditFact {
    pub(super) fn same_payload(
        &self,
        other: &Self,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        governor.charge(
            Resource::Work,
            size(self.before.len())
                .saturating_add(size(self.after.len()))
                .saturating_add(
                    self.parameter
                        .as_ref()
                        .map_or(0, |pointer| size(pointer.len())),
                )
                .saturating_add(1),
        )?;
        Ok(self.before == other.before && self.after == other.after)
    }
}
