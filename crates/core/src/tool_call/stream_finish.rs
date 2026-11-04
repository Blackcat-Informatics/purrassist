// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Borrowed EOF source order and the prefix shared by resident final readings.
use super::recover::ReasoningBlock;
use super::source_min;
use super::stream_capture::{FinalOrigins, NativeFact, NativeTrace, Origins};
use super::{
    CallLedger, CallOutcome, CallReading, RejectedCall, ToolCall, ToolCalls, UnresolvedReason,
};
use crate::contract::{
    Evidence, Governor, Resource, ScratchCharge, ScratchVec, StopReason, Validation, invalid_input,
    push_scratch, saturating_size as size,
};
use crate::json::{same_source_text as text, same_source_tree};
use crate::provenance::ProvenanceNode;
use core::ops::Range;

/// Four source-family lookups, ordering and a checked cursor update.
const CURSOR_WORK: u64 = 32;

/// Keep a publication cutoff outside any valid UTF-8 scalar crossing it.
/// Invalid byte sequences keep their exact original boundaries.
pub(super) fn source_boundary(
    source: &[u8],
    end: usize,
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    // One cutoff lookup, at most three backward probes, and scalar validation
    // over at most four bytes, shared with the existing live publication path.
    governor.charge(Resource::Work, 12)?;
    if end > source.len() {
        return Err(invalid_input("content frontier exceeds admitted source"));
    }
    if end == 0 || source.get(end).is_none_or(|byte| byte & 0xc0 != 0x80) {
        return Ok(end);
    }
    let mut start = end;
    for _ in 0..3 {
        start -= 1;
        let first = source[start];
        if first & 0xc0 != 0x80 {
            let width = match first {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => return Ok(end),
            };
            let Some(scalar) = source.get(start..start.saturating_add(width)) else {
                return Ok(end);
            };
            return Ok(
                if start.saturating_add(width) > end && core::str::from_utf8(scalar).is_ok() {
                    start
                } else {
                    end
                },
            );
        }
        if start == 0 {
            break;
        }
    }
    Ok(end)
}

/// Actual final values paired with their private native correlation row.
#[derive(Clone, Copy)]
pub(super) struct FinalView<'a> {
    pub(super) calls: &'a ToolCalls,
    pub(super) origins: &'a FinalOrigins,
}
impl ScratchCharge for FinalView<'_> {
    const CHARGE: usize = 16;
}
const _: () = assert!(core::mem::size_of::<FinalView<'_>>() <= FinalView::CHARGE);

/// A borrowed item; overlapping rejected wrappers remain separate source facts.
#[derive(Clone, Copy)]
pub(super) enum FinalItem<'a> {
    Content(&'a Range<usize>),
    Reasoning(&'a ReasoningBlock),
    Call(&'a ToolCall, &'a Origins),
    Rejected(&'a RejectedCall),
}

impl FinalItem<'_> {
    pub(super) const fn span(&self) -> &Range<usize> {
        match self {
            Self::Content(span) => span,
            Self::Reasoning(block) => &block.span,
            Self::Call(call, _) => &call.span,
            Self::Rejected(rejected) => &rejected.span,
        }
    }

    const fn order(self) -> (usize, u8, usize) {
        let rank = match self {
            Self::Content(_) => 0,
            Self::Reasoning(_) => 1,
            Self::Call(..) => 2,
            Self::Rejected(_) => 3,
        };
        (self.span().start, rank, self.span().end)
    }
}

/// Four monotone indices into one existing final reading, without payload copies.
pub(super) struct FinalCursor<'a> {
    calls: &'a ToolCalls,
    origins: &'a FinalOrigins,
    next: [usize; 4],
}

impl ScratchCharge for FinalCursor<'_> {
    const CHARGE: usize = 48;
}
const _: () = assert!(core::mem::size_of::<FinalCursor<'_>>() <= FinalCursor::CHARGE);

impl<'a> FinalCursor<'a> {
    pub(super) const fn new(view: FinalView<'a>) -> Self {
        Self {
            calls: view.calls,
            origins: view.origins,
            next: [0; 4],
        }
    }

    fn peek(&self) -> Option<(usize, FinalItem<'a>)> {
        [
            self.calls.content.get(self.next[0]).map(FinalItem::Content),
            self.calls
                .reasoning_blocks
                .get(self.next[1])
                .map(FinalItem::Reasoning),
            self.calls
                .calls
                .get(self.next[2])
                .map(|call| FinalItem::Call(call, &self.origins.calls[self.next[2]])),
            self.calls
                .rejected
                .get(self.next[3])
                .map(FinalItem::Rejected),
        ]
        .into_iter()
        .enumerate()
        .filter_map(|(slot, item)| item.map(|item| (slot, item)))
        .min_by_key(|(_, item)| item.order())
    }

    pub(super) fn next(
        &mut self,
        governor: &mut Governor,
    ) -> Result<Option<FinalItem<'a>>, StopReason> {
        governor.charge(Resource::Work, CURSOR_WORK)?;
        let Some((slot, item)) = self.peek() else {
            return Ok(None);
        };
        self.next[slot] = self.next[slot]
            .checked_add(1)
            .ok_or_else(|| governor.stop_resource(Resource::Work))?;
        Ok(Some(item))
    }
}

/// Plan before publishing, so an equal wrapper cannot hide a later-discovered
/// disagreement inside its span. Calls retain their correlated native source
/// facts so a differing call can still publish its earlier shared facts.
pub(super) fn common_prefix(
    readings: &[FinalView<'_>],
    native: &NativeTrace,
    length: usize,
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    let Some(first) = readings.first() else {
        return Err(invalid_input("completed recovery has no final reading"));
    };
    let mut cursors = ScratchVec::new(governor);
    for reading in readings {
        push_scratch(&mut cursors, FinalCursor::new(*reading), governor)?;
    }
    let mut limit = length;
    loop {
        governor.charge(
            Resource::Work,
            size(cursors.len()).saturating_mul(CURSOR_WORK),
        )?;
        let earliest = cursors
            .iter()
            .filter_map(|cursor| cursor.peek().map(|(_, item)| item.span().start))
            .min();
        if earliest.is_none_or(|start| start >= limit) {
            break;
        }
        governor.charge(Resource::Work, CURSOR_WORK)?;
        let base = cursors[0].peek().map(|(_, item)| item);
        for cursor in &cursors {
            if let Some((_, FinalItem::Call(call, origins))) = cursor.peek()
                && let Some(at) = call_limit(call, origins, native, governor)?
            {
                limit = limit.min(at);
            }
        }
        for cursor in &cursors.as_slice()[1..] {
            governor.charge(Resource::Work, CURSOR_WORK)?;
            let other = cursor.peek().map(|(_, item)| item);
            if let Some(at) = difference(base, other, native, governor)? {
                limit = limit.min(at);
            }
        }
        for cursor in cursors.as_mut_slice() {
            let _ = cursor.next(governor)?;
        }
    }
    // Embedded calls are semantic facts inside reasoning even though they do
    // not receive top-level call indices or independent completion events.
    for reading in &readings[1..] {
        for index in 0..first.calls.embedded.len().max(reading.calls.embedded.len()) {
            governor.charge(Resource::Work, 8)?;
            let left = first
                .calls
                .embedded
                .get(index)
                .map(|call| FinalItem::Call(call, &first.origins.embedded[index]));
            let right = reading
                .calls
                .embedded
                .get(index)
                .map(|call| FinalItem::Call(call, &reading.origins.embedded[index]));
            if left
                .into_iter()
                .chain(right)
                .map(|item| item.span().start)
                .min()
                .is_none_or(|start| start >= limit)
            {
                break;
            }
            if let Some(at) = difference(left, right, native, governor)? {
                limit = limit.min(at);
            }
        }
    }
    for reading in readings {
        for (call, origins) in reading
            .calls
            .embedded
            .iter()
            .zip(reading.origins.embedded.iter())
        {
            if let Some(at) = call_limit(call, origins, native, governor)? {
                limit = limit.min(at);
            }
        }
    }
    governor.check()?;
    Ok(limit)
}

fn range_difference(left: &Range<usize>, right: &Range<usize>) -> Option<usize> {
    if left.start != right.start {
        Some(left.start.min(right.start))
    } else if left.end != right.end {
        Some(left.end.min(right.end))
    } else {
        None
    }
}

pub(super) fn selected_fact<'a>(
    origins: &Origins,
    native: &'a NativeTrace,
    governor: &mut Governor,
) -> Result<&'a NativeFact, StopReason> {
    let row = native.call(origins.selected(governor)?)?;
    let selected = row
        .readings
        .first()
        .ok_or_else(|| invalid_input("native call has no selected semantic origin"))?;
    native.fact(*selected)
}

fn fact_difference(
    left: &NativeFact,
    right: &NativeFact,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    let mut limit = left.header_difference(right, governor)?;
    for index in 0..left.arguments.len().max(right.arguments.len()) {
        governor.charge(Resource::Work, 4)?;
        let at = match (left.arguments.get(index), right.arguments.get(index)) {
            (Some(&(a, b)), Some(&(c, d))) if a == c => (b != d).then_some(b.min(d)),
            (Some(&(a, _)), Some(&(b, _))) => Some(a.min(b)),
            (Some(&(at, _)), None) | (None, Some(&(at, _))) => Some(at),
            (None, None) => None,
        };
        limit = source_min(limit, at);
    }
    for index in 0..left.edits.len().max(right.edits.len()) {
        governor.charge(Resource::Work, 4)?;
        let a = left.edits.get(index);
        let b = right.edits.get(index);
        let equal = match (a, b) {
            (Some(a), Some(b)) => {
                governor.charge(
                    Resource::Work,
                    size(
                        a.parameter
                            .as_ref()
                            .map_or(0, crate::contract::ScratchString::len),
                    )
                    .saturating_add(size(
                        b.parameter
                            .as_ref()
                            .map_or(0, crate::contract::ScratchString::len),
                    ))
                    .saturating_add(1),
                )?;
                a.operator == b.operator
                    && a.at == b.at
                    && a.end == b.end
                    && a.same_payload(b, governor)?
                    && a.parameter == b.parameter
            }
            (None, None) => true,
            _ => false,
        };
        if !equal {
            limit = source_min(limit, a.into_iter().chain(b).map(|edit| edit.at).min());
        }
    }
    if left.closure != right.closure {
        limit = source_min(limit, Some(left.span.end.min(right.span.end)));
    }
    Ok(limit)
}

/// Include local semantic competitors even when there is one whole-turn row.
/// Higher-cost diagnostic alternatives retain their origins but are not live
/// competing interpretations of its selected call.
pub(super) fn call_limit(
    call: &ToolCall,
    origins: &Origins,
    native: &NativeTrace,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    let base = selected_fact(origins, native, governor)?;
    let mut limit = None;
    for origin in &origins.0 {
        governor.charge(Resource::Work, 5)?;
        let row = native.call(origin.id)?;
        if row.competing == 0
            || row.competing > row.readings.len()
            || (origin.competing
                && row.readings.len()
                    != call
                        .alternatives
                        .len()
                        .checked_add(1)
                        .ok_or_else(|| invalid_input("semantic capture count overflows"))?)
        {
            return Err(invalid_input(
                "final semantic readings and native origins disagree",
            ));
        }
        if !origin.competing {
            continue;
        }
        let header = native.fact(row.header)?;
        limit = source_min(limit, base.header_difference(header, governor)?);
        for (index, &raw) in row.readings.iter().take(row.competing).enumerate() {
            let fact = native.fact(raw)?;
            if fact.span != call.span {
                return Err(invalid_input(
                    "native capture span differs from its resolved call",
                ));
            }
            limit = source_min(limit, fact_difference(base, fact, governor)?);
            if index > 0 {
                let alternative = &call.alternatives[index - 1];
                let fallback = base.argument_start().min(fact.argument_start());
                limit = source_min(
                    limit,
                    tree_difference(&call.arguments, &alternative.arguments, fallback, governor)?,
                );
                if !optional_text(call.tool.as_deref(), alternative.tool.as_deref(), governor)? {
                    limit = source_min(limit, Some(base.name_start().min(fact.name_start())));
                }
                limit = source_min(limit, Some(call.span.end));
            }
        }
    }
    Ok(limit)
}

fn call_difference(
    left: &ToolCall,
    left_origins: &Origins,
    right: &ToolCall,
    right_origins: &Origins,
    native: &NativeTrace,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    if left.index != right.index || left.span.start != right.span.start {
        return Ok(Some(left.span.start.min(right.span.start)));
    }
    let a = selected_fact(left_origins, native, governor)?;
    let b = selected_fact(right_origins, native, governor)?;
    let mut limit = fact_difference(a, b, governor)?;
    limit = source_min(limit, range_difference(&left.span, &right.span));
    if !optional_text(left.tool.as_deref(), right.tool.as_deref(), governor)? {
        limit = source_min(limit, Some(a.name_start().min(b.name_start())));
    }
    limit = source_min(
        limit,
        tree_difference(
            &left.arguments,
            &right.arguments,
            a.argument_start().min(b.argument_start()),
            governor,
        )?,
    );
    // Native captures, ordered edits and semantic values anchor interior
    // facts above. Final numbering and provenance metadata can disagree
    // over the whole call without changing its shared name or raw arguments;
    // those differences still prevent completion at the call's end.
    if !same_call(left, right, governor)? {
        limit = source_min(limit, Some(left.span.end.min(right.span.end)));
    }
    Ok(limit)
}

struct TreePair<'a> {
    left: &'a crate::json::Node,
    right: &'a crate::json::Node,
    depth: u64,
}
impl ScratchCharge for TreePair<'_> {
    const CHARGE: usize = 24;
}
const _: () = assert!(core::mem::size_of::<TreePair<'_>>() <= TreePair::CHARGE);

fn tree_difference(
    left: &crate::json::Node,
    right: &crate::json::Node,
    fallback: usize,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    use crate::json::Value;
    let mut stack = ScratchVec::new(governor);
    push_scratch(
        &mut stack,
        TreePair {
            left,
            right,
            depth: 0,
        },
        governor,
    )?;
    let mut limit = None;
    while let Some(TreePair { left, right, depth }) = stack.pop() {
        governor.charge(Resource::Work, 4)?;
        governor.charge(Resource::Depth, depth)?;
        let at = left
            .span
            .as_ref()
            .into_iter()
            .chain(right.span.as_ref())
            .map(|span| span.start)
            .min()
            .unwrap_or(fallback);
        let span_difference = match (&left.span, &right.span) {
            (Some(a), Some(b)) => range_difference(a, b),
            (None, None) => None,
            _ => Some(at),
        };
        limit = source_min(limit, span_difference);
        match (&left.value, &right.value) {
            (Value::Null, Value::Null) => {}
            (Value::Bool(a), Value::Bool(b)) if a == b => {}
            (Value::String(a), Value::String(b)) if text(a, b, governor)? => {}
            (Value::Number(a), Value::Number(b)) if text(a.as_str(), b.as_str(), governor)? => {}
            (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
                for (left, right) in a.iter().zip(b) {
                    governor.charge(Resource::Depth, depth.saturating_add(1))?;
                    push_scratch(
                        &mut stack,
                        TreePair {
                            left,
                            right,
                            depth: depth.saturating_add(1),
                        },
                        governor,
                    )?;
                }
            }
            (Value::Object(a), Value::Object(b)) if a.len() == b.len() => {
                for (left, right) in a.iter().zip(b) {
                    if left.key_span != right.key_span || !text(&left.key, &right.key, governor)? {
                        let key = left
                            .key_span
                            .as_ref()
                            .into_iter()
                            .chain(right.key_span.as_ref())
                            .map(|span| span.start)
                            .min()
                            .unwrap_or(at);
                        limit = source_min(limit, Some(key));
                    }
                    governor.charge(Resource::Depth, depth.saturating_add(1))?;
                    push_scratch(
                        &mut stack,
                        TreePair {
                            left: &left.value,
                            right: &right.value,
                            depth: depth.saturating_add(1),
                        },
                        governor,
                    )?;
                }
            }
            _ => limit = source_min(limit, Some(at)),
        }
    }
    Ok(limit)
}

fn difference(
    left: Option<FinalItem<'_>>,
    right: Option<FinalItem<'_>>,
    native: &NativeTrace,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    governor.charge(Resource::Work, 16)?;
    let (left, right) = match (left, right) {
        (Some(left), Some(right)) => (left, right),
        (Some(item), None) | (None, Some(item)) => return Ok(Some(item.span().start)),
        (None, None) => return Ok(None),
    };
    let start = left.span().start.min(right.span().start);
    if left.span().start != right.span().start {
        return Ok(Some(start));
    }
    Ok(match (left, right) {
        (FinalItem::Content(a), FinalItem::Content(b)) => range_difference(a, b),
        (FinalItem::Reasoning(a), FinalItem::Reasoning(b)) => range_difference(&a.span, &b.span)
            .into_iter()
            .chain(range_difference(&a.text, &b.text))
            .min(),
        (FinalItem::Call(a, left), FinalItem::Call(b, right)) => {
            call_difference(a, left, b, right, native, governor)?
        }
        (FinalItem::Rejected(a), FinalItem::Rejected(b)) if same_rejected(a, b, governor)? => None,
        _ => Some(start),
    })
}

fn optional_text(
    left: Option<&str>,
    right: Option<&str>,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 1)?;
    match (left, right) {
        (Some(a), Some(b)) => text(a, b, governor),
        (None, None) => Ok(true),
        _ => Ok(false),
    }
}

fn dialects(
    left: &[super::DialectId],
    right: &[super::DialectId],
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.iter().zip(right) {
        if !text(left.name(), right.name(), governor)? || left != right {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn same_call(
    left: &ToolCall,
    right: &ToolCall,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 32)?;
    if left.index != right.index
        || left.span != right.span
        || left.effects != right.effects
        || left.policy != right.policy
        || !text(left.dialect.name(), right.dialect.name(), governor)?
        || left.dialect != right.dialect
        || !dialects(
            &left.candidate_dialects,
            &right.candidate_dialects,
            governor,
        )?
        || !text(&left.id, &right.id, governor)?
        || !optional_text(
            left.source_id.as_deref(),
            right.source_id.as_deref(),
            governor,
        )?
        || !optional_text(left.tool.as_deref(), right.tool.as_deref(), governor)?
        || !optional_text(left.name.as_deref(), right.name.as_deref(), governor)?
        || !outcome(&left.outcome, &right.outcome, governor)?
        || !same_source_tree(&left.arguments, &right.arguments, governor)?
        || !ledger(&left.repairs, &right.repairs, governor)?
        || left.alternatives.len() != right.alternatives.len()
        || left.keywords.len() != right.keywords.len()
        || !provenance(&left.distinguishing, &right.distinguishing, governor)?
    {
        return Ok(false);
    }
    for (left, right) in left.alternatives.iter().zip(&right.alternatives) {
        if !reading(left, right, governor)? {
            return Ok(false);
        }
    }
    for (left, right) in left.keywords.iter().zip(&right.keywords) {
        governor.charge(Resource::Work, 1)?;
        if left.used != right.used
            || !text(&left.pointer, &right.pointer, governor)?
            || !text(&left.keyword, &right.keyword, governor)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn reading(
    left: &CallReading,
    right: &CallReading,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 16)?;
    for value in [left, right] {
        if let Some(reason) = &value.blocker {
            admit_reason(reason, governor)?;
        }
    }
    if left.cost != right.cost
        || left.effects != right.effects
        || left.policy != right.policy
        || left.blocker != right.blocker
        || !optional_text(left.tool.as_deref(), right.tool.as_deref(), governor)?
        || !same_source_tree(&left.arguments, &right.arguments, governor)?
        || !ledger(&left.repairs, &right.repairs, governor)?
    {
        return Ok(false);
    }
    for value in [left, right] {
        for validation in &value.validations {
            admit_validation(validation, governor)?;
        }
    }
    Ok(left.validations == right.validations)
}

fn same_rejected(
    left: &RejectedCall,
    right: &RejectedCall,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(
        Resource::Work,
        16_u64
            .saturating_add(size(left.raw.len()))
            .saturating_add(size(right.raw.len())),
    )?;
    for dialect in left.dialect.iter().chain(right.dialect.iter()) {
        governor.charge(Resource::Work, size(dialect.name().len()).saturating_add(1))?;
    }
    Ok(left.span == right.span
        && left.dialect == right.dialect
        && left.reason == right.reason
        && left.raw == right.raw
        && dialects(
            &left.candidate_dialects,
            &right.candidate_dialects,
            governor,
        )?
        && ledger(&left.repairs, &right.repairs, governor)?)
}

fn admit_evidence(value: &Evidence, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(
        Resource::Work,
        8_u64
            .saturating_add(size(value.rule.len()))
            .saturating_add(size(value.explanation.len()))
            .saturating_add(size(value.destination.len())),
    )
}

fn ledger(
    left: &CallLedger,
    right: &CallLedger,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    for value in [left, right] {
        governor.charge(Resource::Work, 3)?;
        for value in &value.evidence {
            admit_evidence(value, governor)?;
        }
        for value in &value.losses {
            admit_evidence(&value.loss.evidence, governor)?;
            governor.charge(
                Resource::Work,
                size(value.loss.original.len()).saturating_add(1),
            )?;
        }
        for value in &value.inferences {
            admit_evidence(&value.evidence, governor)?;
            governor.charge(Resource::Work, size(value.value.len()).saturating_add(1))?;
        }
    }
    Ok(left == right)
}

fn admit_validation(value: &Validation, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(
        Resource::Work,
        4_u64
            .saturating_add(size(value.validator.len()))
            .saturating_add(size(value.constraint.len())),
    )?;
    for text in &value.diagnostics {
        governor.charge(Resource::Work, size(text.len()).saturating_add(1))?;
    }
    Ok(())
}

fn admit_reason(value: &UnresolvedReason, governor: &mut Governor) -> Result<(), StopReason> {
    let value = match value {
        UnresolvedReason::UnknownTool { name } => name.as_str(),
        UnresolvedReason::RuleNotAdmitted { rule, .. }
        | UnresolvedReason::RepairNotPermitted { rule } => rule.as_str(),
        UnresolvedReason::InvalidArgument { pointer, .. }
        | UnresolvedReason::UnknownParameter { pointer }
        | UnresolvedReason::MissingRequired { pointer } => pointer.as_str(),
        UnresolvedReason::Malformed { detail } => detail.as_str(),
        UnresolvedReason::ToolChoiceForced { expected } => expected.as_str(),
        UnresolvedReason::MissingName
        | UnresolvedReason::ArgumentsNotObject
        | UnresolvedReason::SchemaValidation
        | UnresolvedReason::Truncated
        | UnresolvedReason::ToolChoiceNone
        | UnresolvedReason::ToolChoiceCardinality => "",
    };
    governor.charge(Resource::Work, size(value.len()).saturating_add(4))
}

fn outcome(
    left: &CallOutcome,
    right: &CallOutcome,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 1)?;
    for value in [left, right] {
        if let CallOutcome::Unresolved { reason } = value {
            admit_reason(reason, governor)?;
        }
    }
    Ok(left == right)
}

fn provenance(
    left: &[ProvenanceNode],
    right: &[ProvenanceNode],
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    for values in [left, right] {
        for value in values {
            governor.charge(Resource::Work, 1)?;
            match value {
                ProvenanceNode::Evidence(value) => admit_evidence(value, governor)?,
                ProvenanceNode::Loss(value) => {
                    admit_evidence(&value.evidence, governor)?;
                    governor.charge(Resource::Work, size(value.original.len()))?;
                }
                ProvenanceNode::Inference(value) => {
                    admit_evidence(&value.evidence, governor)?;
                    governor.charge(Resource::Work, size(value.value.len()))?;
                }
                ProvenanceNode::Validation(..) => {}
            }
        }
    }
    Ok(left == right)
}
