// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The backtracking evaluator: the only engine that understands
//! backreferences and lookaround. Backreferences can describe nonregular
//! languages; lookaround over regular components remains regular, but this
//! implementation evaluates it through bounded exploration. Every other
//! supported construct is also
//! matched here when a pattern contains either, so the two engines never
//! partially cooperate on one search. Charges `Resource::Work` per
//! exploration step, matching the pre-split engine's semantics exactly (see
//! `docs/QUALIFICATION.md`).
use super::unicode::interval_contains;
use super::{ast::Node, is_word};
use crate::contract::{Governor, Resource, StopReason};

/// Which of `indices` (a numeric backreference's single index, or every
/// index sharing a duplicated name) has actually captured something on
/// this path, per ECMA-262's duplicate-named-group resolution: at most one
/// *should* ever have, since a duplicated name's groups are checked at
/// parse time to be mutually exclusive (different branches of a shared
/// alternation). A group under repetition can still leave more than one of
/// a name's indices set from different iterations; picking the capture
/// ending latest (ties broken by starting latest) matches ordinary
/// same-index backreference behavior under repetition, where each
/// iteration simply overwrites the one index it has.
fn participating_capture(
    indices: &[usize],
    captures: &[Option<(usize, usize)>],
) -> Option<(usize, usize)> {
    indices
        .iter()
        .filter_map(|&index| captures.get(index).copied().flatten())
        .max_by_key(|&(start, end)| (end, start))
}

#[derive(Clone)]
struct State {
    position: usize,
    captures: Vec<Option<(usize, usize)>>,
}

/// Unanchored search (JSON Schema `pattern` semantics: found anywhere).
pub(super) fn is_match(
    node: &Node,
    captures: usize,
    text: &[char],
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    // Transient to this one search; released on every return path (the
    // lease's `Drop`), so a repeated search on the same governor never
    // accumulates live scratch.
    let _lease = governor.lease_scratch(
        u64::try_from(text.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(4),
    )?;
    for position in 0..=text.len() {
        let state = State {
            position,
            captures: vec![None; captures + 1],
        };
        if !evaluate(node, text, state, governor, 0)?.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn evaluate(
    node: &Node,
    text: &[char],
    state: State,
    governor: &mut Governor,
    depth: u32,
) -> Result<Vec<State>, StopReason> {
    governor.charge(Resource::Work, 1)?;
    governor.charge(Resource::Depth, u64::from(depth))?;
    // Released on every exit from this call, so a bounded but deep
    // backtracking search never leaves scratch charged once it unwinds.
    let _lease = governor.lease_scratch(
        u64::try_from(state.captures.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(32)
            .saturating_add(64),
    )?;
    let position = state.position;
    let single = |accept: bool, mut state: State, advance: usize| {
        if accept {
            state.position += advance;
            vec![state]
        } else {
            Vec::new()
        }
    };
    match node {
        Node::Empty => Ok(vec![state]),
        Node::NoMatch => Ok(Vec::new()),
        Node::Class(intervals) => Ok(single(
            text.get(position)
                .is_some_and(|character| interval_contains(intervals, u32::from(*character))),
            state,
            1,
        )),
        Node::Start => Ok(single(position == 0, state, 0)),
        Node::End => Ok(single(position == text.len(), state, 0)),
        Node::Boundary(positive) => {
            let before = position
                .checked_sub(1)
                .and_then(|index| text.get(index))
                .is_some_and(|character| is_word(*character));
            let after = text
                .get(position)
                .is_some_and(|character| is_word(*character));
            Ok(single((before != after) == *positive, state, 0))
        }
        Node::Concat(items) => {
            let mut states = vec![state];
            for item in items {
                let mut next = Vec::new();
                for state in states {
                    next.extend(evaluate(item, text, state, governor, depth + 1)?);
                }
                states = next;
                if states.is_empty() {
                    break;
                }
            }
            Ok(states)
        }
        Node::Alternate(items) => {
            let mut results = Vec::new();
            for item in items {
                results.extend(evaluate(item, text, state.clone(), governor, depth + 1)?);
            }
            Ok(results)
        }
        Node::Group(index, inner) => {
            let mut results = evaluate(inner, text, state, governor, depth + 1)?;
            for result in &mut results {
                result.captures[*index] = Some((position, result.position));
            }
            Ok(results)
        }
        Node::Backref(indices) => {
            let Some((start, end)) = participating_capture(indices, &state.captures) else {
                // No candidate group has captured anything on this path: a
                // numeric backreference to a group not yet reached, or (for
                // a duplicated name) a group whose mutually exclusive
                // alternative never ran. ECMA-262: matches the empty
                // string, not a failure.
                return Ok(vec![state]);
            };
            let length = end - start;
            Ok(single(
                text.get(position..position.saturating_add(length)) == text.get(start..end),
                state,
                length,
            ))
        }
        Node::Look {
            inner,
            positive,
            behind,
        } => assertion(inner, (*positive, *behind), text, state, governor, depth),
        Node::Repeat(repetition) => repeat(repetition, text, state, governor, depth),
    }
}
fn assertion(
    inner: &Node,
    flags: (bool, bool),
    text: &[char],
    state: State,
    governor: &mut Governor,
    depth: u32,
) -> Result<Vec<State>, StopReason> {
    let (positive, behind) = flags;
    let position = state.position;
    let starts = if behind {
        0..=position
    } else {
        position..=position
    };
    let mut found = None;
    for start in starts {
        let mut probe = state.clone();
        probe.position = start;
        for mut result in evaluate(inner, text, probe, governor, depth + 1)? {
            if !behind || result.position == position {
                result.position = position;
                found = Some(result);
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    if positive {
        Ok(found.into_iter().collect())
    } else {
        Ok(if found.is_none() {
            vec![state]
        } else {
            Vec::new()
        })
    }
}
fn repeat(
    repetition: &super::ast::Repetition,
    text: &[char],
    state: State,
    governor: &mut Governor,
    depth: u32,
) -> Result<Vec<State>, StopReason> {
    enum Frame {
        Expand(State, usize),
        Accept(State),
    }
    let bound = repetition
        .max
        .unwrap_or_else(|| text.len().saturating_add(repetition.min).saturating_add(1));
    // Every iteration below can clone `state` (once to keep exploring, once
    // to record an `Accept` frame) before `evaluate`'s own per-call lease
    // charges for the clone it receives; bracket the whole backtracking
    // search's `stack`/`results` growth and those extra clones in one lease,
    // released when this call returns, so a long repeated match never leaves
    // scratch charged once `is_match` returns.
    let clone_bytes = u64::try_from(state.captures.len().saturating_add(1))
        .unwrap_or(u64::MAX)
        .saturating_mul(32);
    let _lease = governor.lease_scratch(
        u64::try_from(bound)
            .unwrap_or(u64::MAX)
            .saturating_mul(clone_bytes.saturating_mul(3).saturating_add(64)),
    )?;
    let mut stack = vec![Frame::Expand(state, 0)];
    let mut results = Vec::new();
    while let Some(frame) = stack.pop() {
        governor.charge(Resource::Work, 1)?;
        let Frame::Expand(state, count) = frame else {
            if let Frame::Accept(state) = frame {
                results.push(state);
            }
            continue;
        };
        let accept = count >= repetition.min;
        if accept && repetition.greedy {
            stack.push(Frame::Accept(state.clone()));
        }
        if count < bound {
            let before = state.position;
            let advanced = evaluate(&repetition.inner, text, state.clone(), governor, depth + 1)?;
            for next in advanced.into_iter().rev() {
                if next.position != before || count < repetition.min {
                    stack.push(Frame::Expand(next, count + 1));
                }
            }
        }
        if accept && !repetition.greedy {
            stack.push(Frame::Accept(state));
        }
    }
    Ok(results)
}
