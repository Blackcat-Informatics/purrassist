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
use super::storage::{Buffer, Stack};
use super::unicode::interval_contains;
use super::{ast::Node, is_word};
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason};

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

struct State {
    position: usize,
    captures: Buffer<Option<(usize, usize)>>,
}

impl ScratchCharge for Option<(usize, usize)> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 24);
        24
    };
}

impl ScratchCharge for State {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 48);
        48
    };
}

impl State {
    fn try_clone(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(
            Resource::Work,
            u64::try_from(self.captures.values.len()).unwrap_or(u64::MAX),
        )?;
        let mut captures = ScratchVec::new(governor);
        captures.extend(self.captures.values.iter().copied())?;
        Ok(Self {
            position: self.position,
            captures: Buffer::from_scratch(captures, governor),
        })
    }
}

/// Unanchored search (JSON Schema `pattern` semantics: found anywhere).
pub(super) fn is_match(
    node: &Node,
    captures: usize,
    text: &[char],
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    for position in 0..=text.len() {
        governor.charge(
            Resource::Work,
            u64::try_from(captures.saturating_add(1)).unwrap_or(u64::MAX),
        )?;
        let mut empty = ScratchVec::new(governor);
        empty.extend(std::iter::repeat_n(None, captures.saturating_add(1)))?;
        let state = State {
            position,
            captures: Buffer::from_scratch(empty, governor),
        };
        if !evaluate(node, text, state, governor, 0)?.values.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn single(
    accept: bool,
    mut state: State,
    advance: usize,
    governor: &Governor,
) -> Result<Buffer<State>, StopReason> {
    let mut out = ScratchVec::new(governor);
    if accept {
        state.position += advance;
        out.push(state)?;
    }
    Ok(Buffer::from_scratch(out, governor))
}

fn evaluate(
    node: &Node,
    text: &[char],
    state: State,
    governor: &mut Governor,
    depth: u32,
) -> Result<Buffer<State>, StopReason> {
    governor.charge(Resource::Work, 1)?;
    governor.charge(Resource::Depth, u64::from(depth))?;
    let position = state.position;
    match node {
        Node::Empty => single(true, state, 0, governor),
        Node::NoMatch => single(false, state, 0, governor),
        Node::Class(intervals) => single(
            text.get(position)
                .is_some_and(|character| interval_contains(intervals, u32::from(*character))),
            state,
            1,
            governor,
        ),
        Node::Start => single(position == 0, state, 0, governor),
        Node::End => single(position == text.len(), state, 0, governor),
        Node::Boundary(positive) => {
            let before = position
                .checked_sub(1)
                .and_then(|index| text.get(index))
                .is_some_and(|character| is_word(*character));
            let after = text
                .get(position)
                .is_some_and(|character| is_word(*character));
            single((before != after) == *positive, state, 0, governor)
        }
        Node::Concat(items) => {
            let mut states = single(true, state, 0, governor)?;
            for item in items {
                let mut next = ScratchVec::new(governor);
                for state in states {
                    next.extend(evaluate(item, text, state, governor, depth + 1)?)?;
                }
                states = Buffer::from_scratch(next, governor);
                if states.values.is_empty() {
                    break;
                }
            }
            Ok(states)
        }
        Node::Alternate(items) => {
            let mut results = ScratchVec::new(governor);
            for item in items {
                let state = state.try_clone(governor)?;
                results.extend(evaluate(item, text, state, governor, depth + 1)?)?;
            }
            Ok(Buffer::from_scratch(results, governor))
        }
        Node::Group(index, inner) => {
            let mut results = evaluate(&inner[0], text, state, governor, depth + 1)?;
            for result in &mut results.values {
                result.captures.values[*index] = Some((position, result.position));
            }
            Ok(results)
        }
        Node::Backref(indices) => {
            governor.charge(
                Resource::Work,
                u64::try_from(indices.len()).unwrap_or(u64::MAX),
            )?;
            let Some((start, end)) = participating_capture(indices, &state.captures.values) else {
                // No candidate group has captured anything on this path: a
                // numeric backreference to a group not yet reached, or (for
                // a duplicated name) a group whose mutually exclusive
                // alternative never ran. ECMA-262: matches the empty
                // string, not a failure.
                return single(true, state, 0, governor);
            };
            let length = end - start;
            governor.charge(Resource::Work, u64::try_from(length).unwrap_or(u64::MAX))?;
            single(
                text.get(position..position.saturating_add(length)) == text.get(start..end),
                state,
                length,
                governor,
            )
        }
        Node::Look {
            inner,
            positive,
            behind,
        } => assertion(
            &inner[0],
            (*positive, *behind),
            text,
            state,
            governor,
            depth,
        ),
        Node::Repeat(owned) => repeat(&owned[0], text, state, governor, depth),
    }
}
fn assertion(
    inner: &Node,
    flags: (bool, bool),
    text: &[char],
    state: State,
    governor: &mut Governor,
    depth: u32,
) -> Result<Buffer<State>, StopReason> {
    let (positive, behind) = flags;
    let position = state.position;
    let starts = if behind {
        0..=position
    } else {
        position..=position
    };
    let mut found = None;
    for start in starts {
        let mut probe = state.try_clone(governor)?;
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
    let mut results = ScratchVec::new(governor);
    if positive {
        results.extend(found)?;
    } else if found.is_none() {
        results.push(state)?;
    }
    Ok(Buffer::from_scratch(results, governor))
}
fn repeat(
    repetition: &super::ast::Repetition,
    text: &[char],
    state: State,
    governor: &mut Governor,
    depth: u32,
) -> Result<Buffer<State>, StopReason> {
    enum Frame {
        Expand(State, usize),
        Accept(State),
    }
    impl ScratchCharge for Frame {
        const CHARGE: usize = {
            assert!(core::mem::size_of::<Self>() <= 64);
            64
        };
    }
    let bound = repetition
        .max
        .unwrap_or_else(|| text.len().saturating_add(repetition.min).saturating_add(1));
    let mut stack = Stack::new(governor);
    stack.push(Frame::Expand(state, 0))?;
    let mut results = ScratchVec::new(governor);
    while let Some(frame) = stack.pop() {
        governor.charge(Resource::Work, 1)?;
        let Frame::Expand(state, count) = frame else {
            if let Frame::Accept(state) = frame {
                results.push(state)?;
            }
            continue;
        };
        let accept = count >= repetition.min;
        if accept && repetition.greedy {
            stack.push(Frame::Accept(state.try_clone(governor)?))?;
        }
        if count < bound {
            let before = state.position;
            let probe = state.try_clone(governor)?;
            let advanced = evaluate(&repetition.inner, text, probe, governor, depth + 1)?;
            for next in advanced.into_iter().rev() {
                if next.position != before || count < repetition.min {
                    stack.push(Frame::Expand(next, count + 1))?;
                }
            }
        }
        if accept && !repetition.greedy {
            stack.push(Frame::Accept(state))?;
        }
    }
    Ok(Buffer::from_scratch(results, governor))
}
