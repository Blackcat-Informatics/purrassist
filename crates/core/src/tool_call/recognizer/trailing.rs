// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Correlated trailing-loss interpretations of erased call endings.

use super::{
    CallSegment, Closure, EditOperator, Governor, MarkupEdit, Range, RawCall, Resource, ScratchMap,
    ScratchVec, Segment, StopReason, Turn, TurnKind, same_embedded_binding, same_source_text,
    same_source_tree, size,
};
use crate::contract::push_scratch;
use crate::grammar::CaptureKind;

pub(super) fn possible(
    turn: &Turn,
    hist: u32,
    at: usize,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 1)?;
    match turn.nodes[hist as usize].kind {
        TurnKind::Pack(_) => Ok(true),
        TurnKind::Stored(index, _) => {
            let Segment::Call(call) = &turn.store[index as usize] else {
                return Ok(false);
            };
            governor.charge(Resource::Work, size(call.readings.len()))?;
            Ok(call.span.end == at
                && call.derivations.is_empty()
                && !call.readings.is_empty()
                && call.readings.iter().all(|raw| {
                    raw.span == call.span
                        && raw.dialect.grammar().all_call_endings_erased
                        && tail(raw).is_some()
                }))
        }
        _ => Ok(false),
    }
}

pub(super) fn preserve(
    turn: &Turn,
    hist: u32,
    at: usize,
    minimums: usize,
    selected: &mut ScratchVec<u32>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    if !possible(turn, hist, at, governor)? {
        return Ok(());
    }
    let mut pending = ScratchVec::new(governor);
    let mut seen = ScratchMap::new(governor);
    push_scratch(&mut pending, hist, governor)?;
    while let Some(node) = pending.pop() {
        if !visit(&mut seen, (node, false), governor)? {
            continue;
        }
        let current = turn.nodes[node as usize];
        match current.kind {
            TurnKind::Pack(other) => {
                push_scratch(&mut pending, current.prev, governor)?;
                push_scratch(&mut pending, other, governor)?;
            }
            TurnKind::Stored(index, cost) => {
                let Segment::Call(call) = &turn.store[index as usize] else {
                    continue;
                };
                if !possible(turn, node, at, governor)? {
                    continue;
                }
                for index in 0..minimums {
                    let minimum = selected[index];
                    if matches(turn, minimum, current.prev, call, cost, governor)? {
                        governor.charge(Resource::Work, size(selected.len()))?;
                        if !selected.contains(&node) {
                            push_scratch(selected, node, governor)?;
                        }
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Memoized DAG traversal, rather than enumerating independent Pack products.
fn visit(
    seen: &mut ScratchMap<(u32, bool), ()>,
    key: (u32, bool),
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(
        Resource::Work,
        u64::from(seen.len().bit_width() + 1)
            .saturating_mul(2)
            .saturating_add(1),
    )?;
    if seen.get(&key).is_some() {
        return Ok(false);
    }
    // Binary insertion can move the admitted keys and relocate the buffer.
    governor.charge(
        Resource::Work,
        size(seen.len()).saturating_mul(2).saturating_add(1),
    )?;
    let _ = seen.insert(key, ())?;
    Ok(true)
}

fn tail(raw: &RawCall) -> Option<Range<usize>> {
    let capture = raw.captures.last()?;
    (capture.kind == CaptureKind::Dropped
        && !capture.span.is_empty()
        && capture.span.end == raw.span.end
        && capture.span.start > raw.span.start)
        .then(|| capture.span.clone())
}

fn matches(
    turn: &Turn,
    hist: u32,
    prefix: u32,
    extracted: &CallSegment,
    cost: u64,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    let Some(tail) = tail(&extracted.readings[0]) else {
        return Ok(false);
    };
    let mut pending = ScratchVec::new(governor);
    let mut seen = ScratchMap::new(governor);
    push_scratch(&mut pending, (hist, false), governor)?;
    while let Some((node, later)) = pending.pop() {
        if node <= prefix || !visit(&mut seen, (node, later), governor)? {
            continue;
        }
        let current = turn.nodes[node as usize];
        match current.kind {
            TurnKind::Pack(other) => {
                push_scratch(&mut pending, (current.prev, later), governor)?;
                push_scratch(&mut pending, (other, later), governor)?;
            }
            TurnKind::Stored(index, carried) => {
                let Segment::Call(call) = &turn.store[index as usize] else {
                    continue;
                };
                if tail.start <= call.span.start
                    && call.span.end <= tail.end
                    && canonical(call, extracted, carried, governor)?
                {
                    push_scratch(&mut pending, (current.prev, true), governor)?;
                } else if later
                    && current.prev == prefix
                    && call.span.start == extracted.span.start
                    && call.span.end == tail.start
                    && same_invocation(call, extracted, &tail, carried, cost, governor)?
                {
                    return Ok(true);
                }
            }
            TurnKind::Content(start) if tail.start <= start && current.end <= tail.end => {
                push_scratch(&mut pending, (current.prev, later), governor)?;
            }
            _ => {}
        }
    }
    Ok(false)
}

fn canonical(
    call: &CallSegment,
    extracted: &CallSegment,
    cost: u64,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(
        Resource::Work,
        size(call.readings.len())
            .saturating_mul(8)
            .saturating_add(1),
    )?;
    if cost != 0 || call.readings.is_empty() || !call.derivations.is_empty() {
        return Ok(false);
    }
    for raw in &call.readings {
        if !same_dialect(&raw.dialect, &extracted.readings[0].dialect, governor)?
            || raw.kind != super::ProductionKind::Call
            || raw.span != call.span
            || raw.closure != Closure::Closed
            || raw.cost != 0
            || !raw.edits.is_empty()
            || raw.name.is_none()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn same_dialect(
    left: &super::DialectId,
    right: &super::DialectId,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 8)?;
    Ok(same_source_text(left.name(), right.name(), governor)? && left == right)
}

fn same_invocation(
    first: &CallSegment,
    extracted: &CallSegment,
    tail: &Range<usize>,
    first_cost: u64,
    extracted_cost: u64,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 3)?;
    let extra = EditOperator::ExtractProse.rule().cost;
    if first_cost.checked_add(extra) != Some(extracted_cost)
        || first.readings.len() != extracted.readings.len()
        || !first.derivations.is_empty()
    {
        return Ok(false);
    }
    for (first, extracted) in first.readings.iter().zip(&extracted.readings) {
        if !same_raw(first, extracted, tail, extra, governor)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn same_raw(
    first: &RawCall,
    extracted: &RawCall,
    tail: &Range<usize>,
    extra: u64,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 18)?;
    if !same_dialect(&first.dialect, &extracted.dialect, governor)?
        || first.kind != super::ProductionKind::Call
        || first.kind != extracted.kind
        || first.span.start != extracted.span.start
        || first.span.end != tail.start
        || first.order != extracted.order
        || first.bare != extracted.bare
        || !matches!(first.closure, Closure::Closed | Closure::Repaired)
        || extracted.closure != Closure::Repaired
        || first.cost.checked_add(extra) != Some(extracted.cost)
        || first.captures.len().checked_add(1) != Some(extracted.captures.len())
        || first.edits.len().checked_add(1) != Some(extracted.edits.len())
        || self::tail(extracted).as_ref() != Some(tail)
        || !same_embedded_binding(first.name.as_ref(), extracted.name.as_ref(), governor)?
        || !same_embedded_binding(first.id.as_ref(), extracted.id.as_ref(), governor)?
    {
        return Ok(false);
    }
    governor.charge(Resource::Work, size(first.captures.len()).saturating_mul(3))?;
    if first.captures.as_slice() != &extracted.captures[..extracted.captures.len() - 1]
        || !same_arguments(&first.arguments, &extracted.arguments, governor)?
    {
        return Ok(false);
    }
    let mut original = first.edits.iter();
    let mut found = false;
    for edit in &extracted.edits {
        governor.charge(Resource::Work, 6)?;
        if edit.operator == EditOperator::ExtractProse && edit.span == *tail {
            if found || !edit.replacement.is_empty() || edit.original.len() != tail.len() {
                return Ok(false);
            }
            found = true;
        } else {
            let Some(prior) = original.next() else {
                return Ok(false);
            };
            if !same_edit(prior, edit, governor)? {
                return Ok(false);
            }
        }
    }
    Ok(found && original.next().is_none())
}

fn same_edit(
    left: &MarkupEdit,
    right: &MarkupEdit,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(
        Resource::Work,
        size(left.original.len())
            .saturating_add(size(right.original.len()))
            .saturating_add(size(left.replacement.len()))
            .saturating_add(size(right.replacement.len()))
            .saturating_add(4),
    )?;
    Ok(left == right)
}

fn same_arguments(
    left: &super::ArgTree,
    right: &super::ArgTree,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 5)?;
    if left.arguments_were_stringified != right.arguments_were_stringified
        || left.leaves.len() != right.leaves.len()
        || left.unresolved.len() != right.unresolved.len()
        || left.collected.len() != right.collected.len()
        || !left.same_adoption_state(right, governor)?
    {
        return Ok(false);
    }
    match (&left.tool, &right.tool) {
        (None, None) => {}
        (Some(a), Some(b)) if same_source_text(a, b, governor)? => {}
        _ => return Ok(false),
    }
    for (a, b) in left.leaves.iter().zip(&right.leaves) {
        governor.charge(Resource::Work, 4)?;
        if a.raw != b.raw
            || a.origin != b.origin
            || !same_source_text(&a.pointer, &b.pointer, governor)?
        {
            return Ok(false);
        }
    }
    for (a, b) in left.unresolved.iter().zip(&right.unresolved) {
        governor.charge(Resource::Work, 4)?;
        if a.raw != b.raw
            || a.fault != b.fault
            || !same_source_text(&a.pointer, &b.pointer, governor)?
        {
            return Ok(false);
        }
    }
    for (a, b) in left.collected.iter().zip(&right.collected) {
        governor.charge(Resource::Work, 4)?;
        if a.key_span != b.key_span
            || a.occurrences != b.occurrences
            || !same_source_text(&a.pointer, &b.pointer, governor)?
        {
            return Ok(false);
        }
    }
    same_source_tree(&left.root, &right.root, governor)
}
