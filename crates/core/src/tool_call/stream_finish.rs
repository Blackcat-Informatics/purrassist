// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Borrowed EOF source order and the prefix shared by resident final readings.
use super::recover::ReasoningBlock;
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

/// A borrowed item; overlapping rejected wrappers remain separate source facts.
#[derive(Clone, Copy)]
pub(super) enum FinalItem<'a> {
    Content(&'a Range<usize>),
    Reasoning(&'a ReasoningBlock),
    Call(&'a ToolCall),
    Rejected(&'a RejectedCall),
}

impl FinalItem<'_> {
    pub(super) const fn span(&self) -> &Range<usize> {
        match self {
            Self::Content(span) => span,
            Self::Reasoning(block) => &block.span,
            Self::Call(call) => &call.span,
            Self::Rejected(rejected) => &rejected.span,
        }
    }

    const fn order(self) -> (usize, u8, usize) {
        let rank = match self {
            Self::Content(_) => 0,
            Self::Reasoning(_) => 1,
            Self::Call(_) => 2,
            Self::Rejected(_) => 3,
        };
        (self.span().start, rank, self.span().end)
    }
}

/// Four monotone indices into one existing final reading, without payload copies.
pub(super) struct FinalCursor<'a> {
    calls: &'a ToolCalls,
    next: [usize; 4],
}

impl ScratchCharge for FinalCursor<'_> {
    const CHARGE: usize = 40;
}
const _: () = assert!(core::mem::size_of::<FinalCursor<'_>>() <= FinalCursor::CHARGE);

impl<'a> FinalCursor<'a> {
    pub(super) const fn new(calls: &'a ToolCalls) -> Self {
        Self {
            calls,
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
            self.calls.calls.get(self.next[2]).map(FinalItem::Call),
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
/// disagreement inside its span. Complete differing calls stop at their start;
/// finer intra-call commitment still needs correlated native capture identities.
pub(super) fn common_prefix(
    readings: &[ToolCalls],
    length: usize,
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    let Some(first) = readings.first() else {
        return Err(invalid_input("completed recovery has no final reading"));
    };
    if readings.len() == 1 {
        return Ok(length);
    }
    let mut cursors = ScratchVec::new(governor);
    for reading in readings {
        push_scratch(&mut cursors, FinalCursor::new(reading), governor)?;
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
        for cursor in &cursors.as_slice()[1..] {
            governor.charge(Resource::Work, CURSOR_WORK)?;
            let other = cursor.peek().map(|(_, item)| item);
            if let Some(at) = difference(base, other, governor)? {
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
        for index in 0..first.embedded.len().max(reading.embedded.len()) {
            governor.charge(Resource::Work, 8)?;
            let left = first.embedded.get(index).map(FinalItem::Call);
            let right = reading.embedded.get(index).map(FinalItem::Call);
            if left
                .into_iter()
                .chain(right)
                .map(|item| item.span().start)
                .min()
                .is_none_or(|start| start >= limit)
            {
                break;
            }
            if let Some(at) = difference(left, right, governor)? {
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

fn difference(
    left: Option<FinalItem<'_>>,
    right: Option<FinalItem<'_>>,
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
        (FinalItem::Call(a), FinalItem::Call(b)) if same_call(a, b, governor)? => None,
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

fn same_call(
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
