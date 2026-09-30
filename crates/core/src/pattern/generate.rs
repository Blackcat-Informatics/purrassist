// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! [`Pattern::generate`]: a shortest witness for the derivative automaton
//! intersected with the complement of every `avoid` pattern's own
//! unanchored-search language, found by a governed breadth-first search of
//! the product automaton. A match established before an actual next character
//! persists through every extension; an end-dependent match is checked only
//! when accepting a complete candidate. Start position and the preceding
//! word class are part of every frontier and cycle key. For a pattern
//! (or `avoid` pattern) that needs the backtracking engine, bounded
//! candidate enumeration from the syntax tree verified with
//! [`Pattern::is_match`], with exhaustion reported explicitly rather than as
//! a claim of emptiness.
use super::ast::Node;
use super::derive::{self, NullCtx};
use super::{Engine, GenerateOptions, GenerateOutcome, Pattern, is_word};
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

/// A lane's live search state and whether it already matched at a position
/// whose actual next character was available. Such a match remains true for
/// every extension, unlike a match depending on the current string ending.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct LaneState {
    live: u32,
    matched: bool,
}

/// Assertion context belongs to the product state: two equal derivative
/// terms reached after a word and a non-word character can accept differently
/// and have different successors. The initial position is distinct as well.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ProductState {
    lanes: Vec<LaneState>,
    at_start: bool,
    prev_word: bool,
}

impl ProductState {
    const fn context(&self, at_end: bool, next_word: bool) -> NullCtx {
        NullCtx {
            at_start: self.at_start,
            at_end,
            prev_word: self.prev_word,
            next_word,
        }
    }
}

fn state_matches(lane: &Lane<'_>, state: LaneState, ctx: NullCtx) -> bool {
    state.matched
        || lane
            .session
            .nullable(derive::classes(lane.compiled), state.live, ctx)
}

/// Resolve assertions at the position before consuming `code`, then advance
/// the unanchored search. Only this nonterminal context can establish a
/// permanent match: `$` is false and a boundary uses the actual next scalar.
fn step_lane(
    lane: &mut Lane<'_>,
    current: LaneState,
    code: u32,
    ctx: NullCtx,
    governor: &mut Governor,
) -> Result<LaneState, StopReason> {
    let root = derive::root(lane.compiled);
    if state_matches(lane, current, ctx) {
        return Ok(LaneState {
            live: root,
            matched: true,
        });
    }
    let stepped = lane.session.step(
        derive::classes(lane.compiled),
        current.live,
        code,
        derive::minterm_of(derive::cuts(lane.compiled), code).0,
        ctx,
        governor,
    )?;
    Ok(LaneState {
        live: lane.session.union(stepped, root)?,
        matched: false,
    })
}

/// Step every lane (the target pattern and every `avoid` pattern) by one
/// character. A permanently established exclusion enters a rejecting sink;
/// terminal-only exclusions remain live until complete-candidate acceptance.
fn step_lanes(
    target: &mut Lane<'_>,
    avoids: &mut [Lane<'_>],
    state: &ProductState,
    code: u32,
    governor: &mut Governor,
) -> Result<Option<ProductState>, StopReason> {
    let next_word = char::from_u32(code).is_some_and(is_word);
    let ctx = state.context(false, next_word);
    let mut lanes = Vec::with_capacity(state.lanes.len());
    lanes.push(step_lane(target, state.lanes[0], code, ctx, governor)?);
    for (lane, &current) in avoids.iter_mut().zip(&state.lanes[1..]) {
        let next = step_lane(lane, current, code, ctx, governor)?;
        if next.matched {
            return Ok(None);
        }
        lanes.push(next);
    }
    Ok(Some(ProductState {
        lanes,
        at_start: false,
        prev_word: next_word,
    }))
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

    // Breadth-first over string length; `frontier` holds, for each reachable
    // combination of lane states and assertion context, the witness string
    // that reaches it. Deduplicated *within one step* (so a
    // step's own work is bounded by the finite, interned product state
    // space, never by the alphabet), but never across steps: a state whose
    // nullability was not yet checked (because its length was still below
    // `options.min_len`) must stay reachable at every later length too, or a
    // self-looping accepting state (any `+`/`*`/`{n,}` repetition) would
    // wrongly read as an empty language the moment `min_len` forces the
    // search past the point where states start repeating.
    let mut frontier = vec![(
        ProductState {
            lanes: std::iter::once(target.state)
                .chain(avoids.iter().map(|lane| lane.state))
                .map(|live| LaneState {
                    live,
                    matched: false,
                })
                .collect(),
            at_start: true,
            prev_word: false,
        },
        String::new(),
    )];
    let mut length = 0usize;
    // Once every current-length nullability check has failed, remember the
    // *set* of live states so far seen at length >= `options.min_len`
    // (stepping is a deterministic function of that set, so a repeated set
    // reproduces the exact same set forever). Recognizing that cycle is what
    // still lets this search prove emptiness *within these bounds* even
    // though states below `options.min_len` are deliberately never pruned
    // (see the `frontier` doc comment above).
    let mut seen_state_sets: std::collections::BTreeSet<Vec<ProductState>> =
        std::collections::BTreeSet::new();
    loop {
        governor.charge(
            Resource::Work,
            u64::try_from(frontier.len()).unwrap_or(u64::MAX),
        )?;
        if length >= options.min_len {
            for (state, witness) in &frontier {
                let ctx = state.context(true, false);
                if state_matches(&target, state.lanes[0], ctx)
                    && avoids
                        .iter()
                        .zip(&state.lanes[1..])
                        .all(|(lane, &current)| !state_matches(lane, current, ctx))
                {
                    return Ok(GenerateOutcome::Witness(witness.clone()));
                }
            }
            let mut current_set: Vec<ProductState> =
                frontier.iter().map(|(state, _)| state.clone()).collect();
            current_set.sort_unstable();
            current_set.dedup();
            if !seen_state_sets.insert(current_set) {
                return Ok(no_candidate(
                    "the language is empty within these bounds: the reachable state set has cycled back to one already checked for acceptance without ever satisfying it, so no longer witness can ever satisfy it either",
                ));
            }
        }
        if options.max_len.is_some_and(|max| length >= max) {
            return Ok(no_candidate(format!(
                "no witness of length <= {} exists: the search of the product automaton (this pattern and its {} `avoid` pattern(s)) exhausted every reachable state without ever satisfying the acceptance condition",
                options.max_len.unwrap_or_default(),
                avoids.len()
            )));
        }
        let next_frontier = step_frontier(
            &frontier,
            &representatives,
            &mut target,
            &mut avoids,
            governor,
        )?;
        if next_frontier.is_empty() {
            return Ok(no_candidate(
                "the language is empty: the product automaton (this pattern and every `avoid` pattern) has no reachable accepting state",
            ));
        }
        frontier = next_frontier;
        length += 1;
    }
}

/// Step every frontier entry by one character over every representative
/// code point, deduplicating *within this one step* (never across steps:
/// see the `frontier` doc comment in [`generate_derivative`]).
fn step_frontier(
    frontier: &[(ProductState, String)],
    representatives: &[u32],
    target: &mut Lane<'_>,
    avoids: &mut [Lane<'_>],
    governor: &mut Governor,
) -> Result<Vec<(ProductState, String)>, StopReason> {
    let mut next_frontier = Vec::new();
    let mut seen_this_step: std::collections::BTreeSet<ProductState> =
        std::collections::BTreeSet::new();
    for (state, witness) in frontier {
        for &code in representatives {
            let Some(character) = char::from_u32(code) else {
                continue;
            };
            let Some(next_state) = step_lanes(target, avoids, state, code, governor)? else {
                continue;
            };
            if seen_this_step.insert(next_state.clone()) {
                let mut extended = witness.clone();
                extended.push(character);
                next_frontier.push((next_state, extended));
            }
        }
    }
    Ok(next_frontier)
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
