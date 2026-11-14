// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! A `pattern` as a finite automaton over Unicode scalar values.
//!
//! The pattern is compiled by the core pattern engine, in the ECMAScript
//! dialect with the Unicode (`u`) reading, as JSON Schema 2020-12 Core section
//! 6.4 (Regular Expressions) asks of `pattern`. The automaton is that engine's
//! own unanchored-search automaton, read one scalar value at a time, so it
//! accepts exactly the strings [`Pattern::is_match`] accepts: a pattern with no
//! anchor accepts every string that contains a match, and `^` and `$` pin an end
//! only where they are written, including inside an alternative. Quantifiers,
//! classes, `\p{...}`, `\b` and `\B` need no translation of their own.
//!
//! A pattern the derivative engine cannot compile has a back-reference or a
//! look-around. Neither is regular in the engine's sense, so each is named and
//! left to the caller.

use std::{collections::BTreeMap, fmt::Write as _};

use purrassist_core::{
    Governor, StopReason,
    contract::Resource,
    pattern::{DfaState, Pattern, PatternCompileError, PatternError},
};

/// A deterministic automaton: state 0 is the start, every state reaches an
/// accepting state, and the edges of one state are disjoint and ordered.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Automaton {
    pub(super) states: Vec<State>,
}

/// One state: whether the text so far is accepted, and its outgoing runs of
/// scalar values as `(first, last, target)`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct State {
    pub(super) accepting: bool,
    pub(super) edges: Vec<(char, char, usize)>,
}

/// What a pattern becomes.
#[derive(Debug)]
pub(super) enum Lowered {
    /// The pattern's language as an automaton.
    Automaton(Automaton),
    /// The pattern repeats an earlier match, which no context-free grammar states.
    Backreference,
    /// The pattern tests the text around a position; its exact translation is not
    /// implemented.
    Lookaround,
}

/// Why a pattern could not be lowered.
#[derive(Debug)]
pub(super) enum Failure {
    /// The pattern is not valid under the Unicode reading.
    Syntax(PatternError),
    /// No string contains a match.
    Empty,
    /// The engine needs backtracking for a construct that is neither a
    /// back-reference nor a look-around.
    Unnamed,
    /// The governor stopped the work.
    Resource(StopReason),
}

impl From<StopReason> for Failure {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// Lower `source` to an automaton, or name the construct that prevents it.
pub(super) fn lower(source: &str, governor: &mut Governor) -> Result<Lowered, Failure> {
    let pattern = match Pattern::compile_governed(source, governor) {
        Ok(pattern) => pattern,
        Err(PatternCompileError::Resource(reason)) => return Err(Failure::Resource(reason)),
        Err(PatternCompileError::Syntax(error)) => {
            // An identity escape of a character that is not a syntax character is
            // invalid under the Unicode reading and has one meaning without it:
            // that character. It is read so.
            let rewritten = literal_identity_escapes(source);
            if rewritten == source {
                return Err(Failure::Syntax(error));
            }
            match Pattern::compile_governed(&rewritten, governor) {
                Ok(pattern) => pattern,
                Err(PatternCompileError::Resource(reason)) => {
                    return Err(Failure::Resource(reason));
                }
                Err(PatternCompileError::Syntax(_)) => return Err(Failure::Syntax(error)),
            }
        }
    };
    automaton(&pattern, governor)?.map_or_else(
        || classify(source),
        |automaton| Ok(Lowered::Automaton(automaton)),
    )
}

/// `source` with every `\` followed by an ASCII punctuation character that the
/// Unicode reading does not allow as an identity escape replaced by `\u{...}`.
fn literal_identity_escapes(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut chars = source.chars();
    let mut in_class = false;
    while let Some(character) = chars.next() {
        match character {
            '\\' => match chars.next() {
                Some(next)
                    if next.is_ascii_punctuation()
                        && !"^$\\.*+?()[]{}|/".contains(next)
                        && !(in_class && next == '-') =>
                {
                    let _ = write!(output, "\\u{{{:X}}}", u32::from(next));
                }
                Some(next) => {
                    output.push('\\');
                    output.push(next);
                }
                None => output.push('\\'),
            },
            '[' => {
                in_class = true;
                output.push(character);
            }
            ']' => {
                in_class = false;
                output.push(character);
            }
            other => output.push(other),
        }
    }
    output
}

/// Which construct keeps a pattern off the derivative engine. A look-around
/// wins over a back-reference: only the back-reference is left to the schema
/// check.
fn classify(source: &str) -> Result<Lowered, Failure> {
    let characters: Vec<char> = source.chars().collect();
    let (mut backreference, mut lookaround) = (false, false);
    let (mut index, mut in_class) = (0, false);
    while index < characters.len() {
        match characters[index] {
            '\\' => {
                if !in_class
                    && characters
                        .get(index + 1)
                        .is_some_and(|next| matches!(next, '1'..='9' | 'k'))
                {
                    backreference = true;
                }
                index += 1;
            }
            '[' => in_class = true,
            ']' => in_class = false,
            '(' if !in_class && characters.get(index + 1) == Some(&'?') => {
                lookaround |= match characters.get(index + 2) {
                    Some('=' | '!') => true,
                    Some('<') => matches!(characters.get(index + 3), Some('=' | '!')),
                    _ => false,
                };
            }
            _ => {}
        }
        index += 1;
    }
    match (lookaround, backreference) {
        (true, _) => Ok(Lowered::Lookaround),
        (false, true) => Ok(Lowered::Backreference),
        (false, false) => Err(Failure::Unnamed),
    }
}

/// A state of the search automaton, with every matched state as one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Matched,
    Open(DfaState),
}

type Raw = Vec<(bool, Vec<(char, char, usize)>)>;

/// A state's class and its merged edges, the key of one refined class.
type Signature = (usize, Vec<(char, char, usize)>);

/// The index of `key` among the states met so far, adding it when new. `pending`
/// holds the keys in index order; the state type has no order to key a map by.
fn intern(key: Key, raw: &mut Raw, pending: &mut Vec<Key>) -> usize {
    pending
        .iter()
        .position(|known| *known == key)
        .unwrap_or_else(|| {
            raw.push((false, Vec::new()));
            pending.push(key);
            raw.len() - 1
        })
}

/// The automaton of a pattern the derivative engine compiles, or `None`.
fn automaton(pattern: &Pattern, governor: &mut Governor) -> Result<Option<Automaton>, Failure> {
    let Some(mut dfa) = pattern.byte_dfa(governor)? else {
        return Ok(None);
    };
    let mut raw: Raw = Vec::new();
    let mut pending = Vec::new();
    intern(Key::Open(dfa.start()), &mut raw, &mut pending);
    let mut cursor = 0;
    while cursor < pending.len() {
        let key = pending[cursor];
        let this = cursor;
        cursor += 1;
        governor.charge(Resource::Work, 1)?;
        let Key::Open(state) = key else {
            raw[this] = (
                true,
                vec![('\0', '\u{D7FF}', this), ('\u{E000}', char::MAX, this)],
            );
            continue;
        };
        let accepting = dfa.accepting(state, governor)?;
        let mut edges: Vec<(char, char, usize)> = Vec::new();
        for run in dfa.scalar_transitions(state, governor)? {
            let key = if run.target.is_matched() {
                Key::Matched
            } else {
                Key::Open(run.target)
            };
            let target = intern(key, &mut raw, &mut pending);
            match edges.last_mut() {
                Some(previous)
                    if previous.2 == target
                        && u32::from(previous.1) + 1 == u32::from(run.first) =>
                {
                    previous.1 = run.last;
                }
                _ => edges.push((run.first, run.last, target)),
            }
        }
        raw[this] = (accepting, edges);
    }
    minimal(&raw, governor).map(Some)
}

/// Drop the states that reach no accepting state, merge equivalent states and
/// number the rest in the order a walk from the start meets them.
fn minimal(raw: &Raw, governor: &mut Governor) -> Result<Automaton, Failure> {
    let count = raw.len();
    // Productive states: those from which an accepting state is reachable.
    let mut productive: Vec<bool> = raw.iter().map(|(accepting, _)| *accepting).collect();
    loop {
        governor.charge(Resource::Work, u64::try_from(count).unwrap_or(u64::MAX))?;
        let mut changed = false;
        for (state, (_, edges)) in raw.iter().enumerate() {
            if !productive[state] && edges.iter().any(|&(_, _, target)| productive[target]) {
                productive[state] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    if !productive[0] {
        return Err(Failure::Empty);
    }
    // Moore refinement: states are equivalent when they accept alike and reach
    // equivalent states over the same scalar values.
    let mut class: Vec<usize> = raw
        .iter()
        .map(|(accepting, _)| usize::from(*accepting))
        .collect();
    let mut classes = 2;
    loop {
        governor.charge(Resource::Work, u64::try_from(count).unwrap_or(u64::MAX))?;
        let mut signatures: BTreeMap<Signature, usize> = BTreeMap::new();
        let mut next = Vec::with_capacity(count);
        for (state, (_, edges)) in raw.iter().enumerate() {
            let mut merged: Vec<(char, char, usize)> = Vec::new();
            for &(first, last, target) in edges {
                if !productive[target] {
                    continue;
                }
                match merged.last_mut() {
                    Some(previous)
                        if previous.2 == class[target]
                            && u32::from(previous.1) + 1 == u32::from(first) =>
                    {
                        previous.1 = last;
                    }
                    _ => merged.push((first, last, class[target])),
                }
            }
            let fresh = signatures.len();
            next.push(*signatures.entry((class[state], merged)).or_insert(fresh));
        }
        let settled = signatures.len() == classes;
        classes = signatures.len();
        class = next;
        if settled {
            break;
        }
    }
    // Number the classes by a walk from the start, in edge order.
    let mut number: BTreeMap<usize, usize> = BTreeMap::new();
    let mut order: Vec<usize> = Vec::new();
    let mut representative: BTreeMap<usize, usize> = BTreeMap::new();
    for (state, &state_class) in class.iter().enumerate() {
        if productive[state] {
            representative.entry(state_class).or_insert(state);
        }
    }
    number.insert(class[0], 0);
    order.push(class[0]);
    let mut cursor = 0;
    let mut states = Vec::new();
    while cursor < order.len() {
        let member = representative[&order[cursor]];
        cursor += 1;
        let mut edges: Vec<(char, char, usize)> = Vec::new();
        for &(first, last, target) in &raw[member].1 {
            if !productive[target] {
                continue;
            }
            let target_class = class[target];
            let target_number = *number.entry(target_class).or_insert_with(|| {
                order.push(target_class);
                order.len() - 1
            });
            match edges.last_mut() {
                Some(previous)
                    if previous.2 == target_number
                        && u32::from(previous.1) + 1 == u32::from(first) =>
                {
                    previous.1 = last;
                }
                _ => edges.push((first, last, target_number)),
            }
        }
        states.push(State {
            accepting: raw[member].0,
            edges,
        });
    }
    Ok(Automaton { states })
}
