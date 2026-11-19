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
//! Look-around uses core's bounded scalar product. Backreferences are left
//! to the schema check.

use std::{collections::BTreeMap, fmt::Write as _};

use purrassist_core::{
    Governor, StopReason,
    contract::{Resource, ScratchLease, saturating_size},
    pattern::{DfaState, LookaroundCompileError, Pattern, PatternCompileError, PatternError},
};

/// A deterministic automaton: state 0 is the start, every state reaches an
/// accepting state, and the edges of one state are disjoint and ordered.
#[derive(Debug)]
pub(super) struct Automaton {
    pub(super) states: Vec<State>,
    _scratch: ScratchLease,
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
}

/// Why a pattern could not be lowered.
#[derive(Debug)]
pub(super) enum Failure {
    /// The pattern is not valid under the Unicode reading.
    Syntax(PatternError),
    /// No string contains a match.
    Empty,
    /// The look-around automaton exceeded its structural size cap.
    LookaroundLimit,
    /// The governor stopped the work.
    Resource(StopReason),
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
    if let Some(automaton) = automaton(&pattern, governor)? {
        return Ok(Lowered::Automaton(automaton));
    }
    let machine = match pattern.lookaround_dfa(governor) {
        Ok(Some(machine)) => machine,
        Ok(None) => return Ok(Lowered::Backreference),
        Err(LookaroundCompileError::SizeLimit) => return Err(Failure::LookaroundLimit),
        Err(LookaroundCompileError::Resource(reason)) => return Err(Failure::Resource(reason)),
    };
    minimal(&machine.rows(), governor).map(Lowered::Automaton)
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

/// A state of the search automaton, with every matched state as one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Matched,
    Open(DfaState),
}

type Raw = Vec<(bool, Vec<(char, char, usize)>)>;

/// A state's class and its merged edges, the key of one refined class.
type Signature = (usize, Vec<(char, char, usize)>);

/// Index signatures by a fixed-width key, then compare the full runs on a hit.
#[derive(Default)]
struct Signatures {
    heads: BTreeMap<u64, usize>,
    entries: Vec<(Signature, Option<usize>)>,
}

impl Signatures {
    fn intern(&mut self, signature: Signature, governor: &mut Governor) -> Result<usize, Failure> {
        let cells = saturating_size(signature.1.len()).saturating_add(1);
        let lookup = u64::from(self.heads.len().max(1).ilog2() + 1) * 16;
        governor
            .charge(Resource::Work, lookup.saturating_add(cells))
            .map_err(Failure::Resource)?;
        let mut key = 0xcbf2_9ce4_8422_2325_u64;
        let mut feed = |value: u64| key = (key ^ value).wrapping_mul(0x0100_0000_01b3);
        feed(saturating_size(signature.0));
        for &(first, last, target) in &signature.1 {
            feed(u64::from(first));
            feed(u64::from(last));
            feed(saturating_size(target));
        }
        let head = self.heads.get(&key).copied();
        let mut candidate = head;
        while let Some(id) = candidate {
            governor
                .charge(Resource::Work, cells)
                .map_err(Failure::Resource)?;
            let (stored, previous) = &self.entries[id];
            if *stored == signature {
                return Ok(id);
            }
            candidate = *previous;
        }
        governor
            .charge(Resource::Work, lookup.saturating_mul(2))
            .map_err(Failure::Resource)?;
        let id = self.entries.len();
        self.heads.insert(key, id);
        self.entries.push((signature, head));
        Ok(id)
    }
}

/// The index of `key` among the states met so far, adding it when new.
fn intern(
    key: Key,
    index: &mut BTreeMap<Key, usize>,
    raw: &mut Raw,
    pending: &mut Vec<Key>,
) -> usize {
    *index.entry(key).or_insert_with(|| {
        raw.push((false, Vec::new()));
        pending.push(key);
        raw.len() - 1
    })
}

/// The automaton of a pattern the derivative engine compiles, or `None`.
fn automaton(pattern: &Pattern, governor: &mut Governor) -> Result<Option<Automaton>, Failure> {
    let Some(mut dfa) = pattern.byte_dfa(governor).map_err(Failure::Resource)? else {
        return Ok(None);
    };
    let mut index = BTreeMap::new();
    let mut raw: Raw = Vec::new();
    let mut pending = Vec::new();
    intern(Key::Open(dfa.start()), &mut index, &mut raw, &mut pending);
    let mut cursor = 0;
    while cursor < pending.len() {
        let key = pending[cursor];
        let this = cursor;
        cursor += 1;
        governor
            .charge(Resource::Work, 1)
            .map_err(Failure::Resource)?;
        let Key::Open(state) = key else {
            raw[this] = (
                true,
                vec![('\0', '\u{D7FF}', this), ('\u{E000}', char::MAX, this)],
            );
            continue;
        };
        let accepting = dfa.accepting(state, governor).map_err(Failure::Resource)?;
        let mut edges: Vec<(char, char, usize)> = Vec::new();
        for run in dfa
            .scalar_transitions(state, governor)
            .map_err(Failure::Resource)?
        {
            let key = if run.target.is_matched() {
                Key::Matched
            } else {
                Key::Open(run.target)
            };
            let target = intern(key, &mut index, &mut raw, &mut pending);
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
    minimal(
        &raw.iter()
            .map(|(accepting, edges)| (*accepting, edges.iter().copied())),
        governor,
    )
    .map(Some)
}

/// Drop the states that reach no accepting state, merge equivalent states and
/// number the rest in the order a walk from the start meets them.
fn minimal<E>(
    rows: &(impl ExactSizeIterator<Item = (bool, E)> + Clone),
    governor: &mut Governor,
) -> Result<Automaton, Failure>
where
    E: ExactSizeIterator<Item = (char, char, usize)>,
{
    let count = saturating_size(rows.len());
    governor
        .charge(Resource::Work, count)
        .map_err(Failure::Resource)?;
    let edges = rows.clone().fold(0_u64, |total, (_, row)| {
        total.saturating_add(saturating_size(row.len()))
    });
    let scan = count.saturating_add(edges);
    // Bound the live refinement maps, lists, and signature vectors. Dense
    // rows are borrowed. Only merged edge runs are copied into signatures
    // and the output, with room for geometric vector growth.
    let _temporary = governor
        .lease_scratch(
            count
                .saturating_mul(2048)
                .saturating_add(edges.saturating_mul(32)),
        )
        .map_err(Failure::Resource)?;
    let mut productive: Vec<bool> = rows.clone().map(|(accepting, _)| accepting).collect();
    loop {
        governor
            .charge(Resource::Work, scan)
            .map_err(Failure::Resource)?;
        let mut changed = false;
        for (state, (_, mut edges)) in rows.clone().enumerate() {
            if !productive[state] && edges.any(|(_, _, target)| productive[target]) {
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
    let (class, classes) = refine(rows, &productive, scan, governor)?;
    // Number the classes by a walk from the start, in edge order.
    let mut number = vec![None; classes];
    let mut order: Vec<usize> = Vec::new();
    let mut representative = vec![0; classes];
    governor
        .charge(Resource::Work, count)
        .map_err(Failure::Resource)?;
    for (state, &state_class) in class.iter().enumerate().rev() {
        if productive[state] {
            representative[state_class] = state;
        }
    }
    number[class[0]] = Some(0);
    order.push(class[0]);
    let scratch = governor
        .lease_scratch(
            count
                .saturating_mul(64)
                .saturating_add(edges.saturating_mul(32)),
        )
        .map_err(Failure::Resource)?;
    governor
        .charge(Resource::Work, scan)
        .map_err(Failure::Resource)?;
    let mut cursor = 0;
    let mut states = Vec::new();
    while cursor < order.len() {
        let member = representative[order[cursor]];
        cursor += 1;
        let mut edges: Vec<(char, char, usize)> = Vec::new();
        let (accepting, row) = rows.clone().nth(member).expect("representative row");
        for (first, last, target) in row {
            if !productive[target] {
                continue;
            }
            let target_class = class[target];
            let target_number = *number[target_class].get_or_insert_with(|| {
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
        states.push(State { accepting, edges });
    }
    Ok(Automaton {
        states,
        _scratch: scratch,
    })
}

/// Refine classes until equivalent states reach equivalent classes over
/// the same scalar values.
fn refine<E>(
    rows: &(impl ExactSizeIterator<Item = (bool, E)> + Clone),
    productive: &[bool],
    scan: u64,
    governor: &mut Governor,
) -> Result<(Vec<usize>, usize), Failure>
where
    E: ExactSizeIterator<Item = (char, char, usize)>,
{
    let mut class: Vec<usize> = rows
        .clone()
        .map(|(accepting, _)| usize::from(accepting))
        .collect();
    let mut classes = 2;
    loop {
        governor
            .charge(Resource::Work, scan)
            .map_err(Failure::Resource)?;
        let mut signatures = Signatures::default();
        let mut next = Vec::with_capacity(rows.len());
        for (state, (_, edges)) in rows.clone().enumerate() {
            let mut merged: Vec<(char, char, usize)> = Vec::new();
            for (first, last, target) in edges {
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
            next.push(signatures.intern((class[state], merged), governor)?);
        }
        let settled = signatures.entries.len() == classes;
        classes = signatures.entries.len();
        class = next;
        if settled {
            return Ok((class, classes));
        }
    }
}
