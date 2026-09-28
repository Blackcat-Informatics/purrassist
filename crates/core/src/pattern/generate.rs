// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! [`Pattern::generate`]: a shortest witness for the derivative automaton
//! intersected with the complement of every `avoid` pattern's own
//! unanchored-search language, found by a governed breadth-first search of
//! the product automaton (pruning a path the instant any `avoid` pattern's
//! search state becomes nullable, since every extension of a string
//! containing a forbidden substring still contains it); or, for a pattern
//! (or `avoid` pattern) that needs the backtracking engine, bounded
//! candidate enumeration from the syntax tree verified with
//! [`Pattern::is_match`], with exhaustion reported explicitly rather than as
//! a claim of emptiness.
use super::ast::Node;
use super::derive::{self, NullCtx};
use super::{Engine, GenerateOptions, GenerateOutcome, Pattern};
use crate::contract::{Evidence, Governor, Resource, StopReason};

fn no_candidate(explanation: impl Into<String>) -> GenerateOutcome {
    GenerateOutcome::NoCandidate(Evidence {
        rule: "pattern.generate.no-candidate-v1".into(),
        explanation: explanation.into(),
        span: None,
        destination: String::new(),
    })
}

pub(super) fn generate(
    pattern: &Pattern,
    governor: &mut Governor,
    options: GenerateOptions<'_>,
) -> Result<GenerateOutcome, StopReason> {
    let backtracking = matches!(pattern.engine, Engine::Backtrack)
        || options
            .avoid
            .iter()
            .any(|p| matches!(p.engine, Engine::Backtrack));
    if backtracking {
        return generate_backtrack(pattern, governor, options);
    }
    generate_derivative(pattern, governor, options)
}

/// One automaton participating in the product search: its compiled
/// structure, a private stepping session, and its own unified-alphabet
/// unanchored-search state.
struct Lane<'c> {
    compiled: &'c derive::Compiled,
    session: derive::Session,
    state: u32,
}

fn lane_of<'c>(pattern: &'c Pattern, governor: &Governor) -> Result<Lane<'c>, StopReason> {
    let compiled = pattern
        .compiled
        .as_ref()
        .expect("Engine::Derivative has a compiled automaton");
    Ok(Lane {
        compiled,
        session: derive::Session::new(compiled, governor)?,
        state: derive::root(compiled),
    })
}

/// `true` if `lane` (an `avoid` pattern's own search state) is already
/// nullable: an unanchored search finds that empty match at every position,
/// so no extension of any witness could ever avoid it.
fn already_matches(lane: &Lane<'_>, length: usize) -> bool {
    state_matches(lane, lane.state, length)
}
/// Like `already_matches`, but for a state other than `lane`'s own current
/// one (the state reached by stepping `lane`, before it is committed).
fn state_matches(lane: &Lane<'_>, state: u32, length: usize) -> bool {
    lane.session.nullable(
        derive::classes(lane.compiled),
        state,
        end_ctx(length, false),
    )
}

/// Step every lane (the target pattern and every `avoid` pattern) by one
/// character, pruning (returning `None`) the instant an `avoid` pattern's
/// restarted search state becomes nullable.
fn step_lanes(
    target: &mut Lane<'_>,
    avoids: &mut [Lane<'_>],
    states: &[u32],
    code: u32,
    ctx: NullCtx,
    length: usize,
    governor: &mut Governor,
) -> Result<Option<Vec<u32>>, StopReason> {
    let target_next = target.session.step(
        derive::classes(target.compiled),
        states[0],
        code,
        derive::minterm_of(derive::cuts(target.compiled), code).0,
        ctx,
        governor,
    )?;
    let mut next_states = Vec::with_capacity(states.len());
    next_states.push(target_next);
    for (lane, &current) in avoids.iter_mut().zip(&states[1..]) {
        let stepped = lane.session.step(
            derive::classes(lane.compiled),
            current,
            code,
            derive::minterm_of(derive::cuts(lane.compiled), code).0,
            ctx,
            governor,
        )?;
        // An `avoid` pattern's search is unanchored: a fresh attempt
        // restarts at every position.
        let restarted = lane.session.union(stepped, derive::root(lane.compiled))?;
        if state_matches(lane, restarted, length + 1) {
            return Ok(None);
        }
        next_states.push(restarted);
    }
    Ok(Some(next_states))
}

fn generate_derivative(
    pattern: &Pattern,
    governor: &mut Governor,
    options: GenerateOptions<'_>,
) -> Result<GenerateOutcome, StopReason> {
    let mut target = lane_of(pattern, governor)?;
    let mut avoids: Vec<Lane<'_>> = options
        .avoid
        .iter()
        .map(|avoid| lane_of(avoid, governor))
        .collect::<Result<_, StopReason>>()?;
    let mut cuts: Vec<u32> = derive::cuts(target.compiled).to_vec();
    for lane in &avoids {
        cuts.extend_from_slice(derive::cuts(lane.compiled));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let representatives = representatives(&cuts);

    if avoids.iter().any(|lane| already_matches(lane, 0)) {
        return Ok(no_candidate(
            "at least one `avoid` pattern matches the empty string, which an unanchored search finds at every position, so no witness for this pattern can ever avoid it",
        ));
    }

    // Breadth-first over string length; `frontier` holds, for each reachable
    // combination of (target state, every avoid pattern's search state),
    // the witness string that reaches it. Deduplicated by that combination
    // so the search is bounded by the (finite, interned) product state
    // space, never by string length alone.
    let mut frontier: Vec<(Vec<u32>, String)> = vec![(
        std::iter::once(target.state)
            .chain(avoids.iter().map(|lane| lane.state))
            .collect(),
        String::new(),
    )];
    let mut visited: std::collections::BTreeSet<Vec<u32>> =
        frontier.iter().map(|(key, _)| key.clone()).collect();
    let mut length = 0usize;
    loop {
        governor.charge(
            Resource::Work,
            u64::try_from(frontier.len()).unwrap_or(u64::MAX),
        )?;
        if length >= options.min_len {
            for (states, witness) in &frontier {
                let target_ctx = end_ctx(length, false);
                if target
                    .session
                    .nullable(derive::classes(target.compiled), states[0], target_ctx)
                {
                    return Ok(GenerateOutcome::Witness(witness.clone()));
                }
            }
        }
        if options.max_len.is_some_and(|max| length >= max) {
            return Ok(no_candidate(format!(
                "no witness of length <= {} exists: the search of the product automaton (this pattern and its {} `avoid` pattern(s)) exhausted every reachable state without ever satisfying the acceptance condition",
                options.max_len.unwrap_or_default(),
                avoids.len()
            )));
        }
        let mut next_frontier = Vec::new();
        for (states, witness) in &frontier {
            for &code in &representatives {
                let Some(character) = char::from_u32(code) else {
                    continue;
                };
                let prev_word = witness.chars().last().is_some_and(derive::is_word);
                let ctx = NullCtx {
                    at_start: length == 0,
                    at_end: false,
                    prev_word,
                    next_word: derive::is_word(character),
                };
                let Some(next_states) = step_lanes(
                    &mut target,
                    &mut avoids,
                    states,
                    code,
                    ctx,
                    length,
                    governor,
                )?
                else {
                    continue;
                };
                if visited.insert(next_states.clone()) {
                    let mut extended = witness.clone();
                    extended.push(character);
                    next_frontier.push((next_states, extended));
                }
            }
        }
        if next_frontier.is_empty() {
            return Ok(no_candidate(
                "the language is empty: the product automaton (this pattern and every `avoid` pattern) has no reachable accepting state",
            ));
        }
        frontier = next_frontier;
        length += 1;
    }
}

const fn end_ctx(length: usize, prev_word: bool) -> NullCtx {
    NullCtx {
        at_start: length == 0,
        at_end: true,
        prev_word,
        next_word: false,
    }
}

/// One representative scalar value per unified minterm bucket across every
/// automaton in the search (see the module doc); buckets entirely inside
/// the surrogate range (which no `char` can hold) are skipped.
fn representatives(cuts: &[u32]) -> Vec<u32> {
    let mut boundaries = vec![0u32];
    boundaries.extend_from_slice(cuts);
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
        .into_iter()
        .filter(|&code| char::from_u32(code).is_some())
        .collect()
}

/// One structurally generated candidate: its text so far, and the substring
/// each capturing group produced along *this* path (so a later
/// `Backref` can reuse its participating group's actual text instead of
/// guessing).
#[derive(Clone)]
struct Candidate {
    text: String,
    captures: std::collections::BTreeMap<usize, String>,
}

/// Bounded structural enumeration from the syntax tree, verified with
/// `Pattern::is_match` (which alone knows backreference/lookaround
/// semantics); exhaustion is reported as such, never as proven emptiness.
fn generate_backtrack(
    pattern: &Pattern,
    governor: &mut Governor,
    options: GenerateOptions<'_>,
) -> Result<GenerateOutcome, StopReason> {
    let start = Candidate {
        text: String::new(),
        captures: std::collections::BTreeMap::new(),
    };
    let mut budget = 20_000usize;
    let mut candidates = collect_candidates(&pattern.ast, start, &mut budget, governor)?;
    candidates.sort_by_key(|candidate| candidate.text.len());
    let mut tried = std::collections::BTreeSet::new();
    for candidate in candidates {
        if !tried.insert(candidate.text.clone()) {
            continue;
        }
        governor.charge(Resource::Work, 1)?;
        let text = candidate.text;
        if text.chars().count() < options.min_len {
            continue;
        }
        if options
            .max_len
            .is_some_and(|max| text.chars().count() > max)
        {
            continue;
        }
        if !pattern.is_match(&text, governor)? {
            continue;
        }
        let mut forbidden = false;
        for avoid in options.avoid {
            if avoid.is_match(&text, governor)? {
                forbidden = true;
                break;
            }
        }
        if !forbidden {
            return Ok(GenerateOutcome::Witness(text));
        }
    }
    Ok(no_candidate(
        "the bounded backtracking candidate search (this pattern or an `avoid` pattern needs backreference/lookaround semantics that only structural enumeration plus verification can check) found no witness matching this pattern and none of the `avoid` patterns; this is exhaustion of a bounded search, not a proof that no witness exists",
    ))
}

fn collect_candidates(
    node: &Node,
    prefix: Candidate,
    budget: &mut usize,
    governor: &mut Governor,
) -> Result<Vec<Candidate>, StopReason> {
    governor.charge(Resource::Work, 1)?;
    *budget = budget.saturating_sub(1);
    if *budget == 0 {
        return Ok(vec![prefix]);
    }
    Ok(match node {
        // Zero-width: none of these ever contributes text of its own. A
        // lookaround's own content never becomes part of the matched text
        // either, so it is treated the same way here; `Pattern::is_match`
        // alone verifies the assertion actually holds for the candidate.
        Node::Empty | Node::Start | Node::End | Node::Boundary(_) | Node::Look { .. } => {
            vec![prefix]
        }
        Node::NoMatch => Vec::new(),
        Node::Class(intervals) => intervals
            .first()
            .and_then(|&(start, _)| char::from_u32(start))
            .map(|character| {
                let mut next = prefix;
                next.text.push(character);
                vec![next]
            })
            .unwrap_or_default(),
        Node::Concat(items) => {
            let mut current = vec![prefix];
            for item in items {
                let mut next = Vec::new();
                for base in current {
                    next.extend(collect_candidates(item, base, budget, governor)?);
                    if next.len() > 16 {
                        next.truncate(16);
                        break;
                    }
                }
                current = next;
                if current.is_empty() {
                    break;
                }
            }
            current
        }
        Node::Alternate(items) => {
            let mut out = Vec::new();
            for item in items {
                out.extend(collect_candidates(item, prefix.clone(), budget, governor)?);
                if out.len() > 16 {
                    out.truncate(16);
                    break;
                }
            }
            out
        }
        Node::Repeat(repetition) => {
            let mut out = Vec::new();
            for count in [repetition.min, repetition.min.saturating_add(1)] {
                if repetition.max.is_some_and(|max| count > max) {
                    continue;
                }
                let mut current = vec![prefix.clone()];
                for _ in 0..count {
                    let mut next = Vec::new();
                    for base in current {
                        next.extend(collect_candidates(
                            &repetition.inner,
                            base,
                            budget,
                            governor,
                        )?);
                        if next.len() > 4 {
                            next.truncate(4);
                            break;
                        }
                    }
                    current = next;
                    if current.is_empty() {
                        break;
                    }
                }
                out.extend(current);
            }
            out
        }
        Node::Group(index, inner) => {
            let start_len = prefix.text.len();
            collect_candidates(inner, prefix, budget, governor)?
                .into_iter()
                .map(|mut candidate| {
                    let captured = candidate.text[start_len..].to_string();
                    candidate.captures.insert(*index, captured);
                    candidate
                })
                .collect()
        }
        Node::Backref(indices) => {
            let mut next = prefix.clone();
            // A single generated candidate only ever took one branch of
            // any `Alternate`, so at most one index sharing a duplicated
            // name can have a capture on this path already (see
            // `ast::Node::Backref`'s doc comment).
            if let Some(text) = indices.iter().find_map(|index| prefix.captures.get(index)) {
                next.text.push_str(text);
            }
            vec![next]
        }
    })
}
