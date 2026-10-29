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
use super::storage::Buffer;
use super::{
    Engine, GenerateOptions, GenerateOutcome, OwnedGenerateOutcome as Generated, Pattern, is_word,
};
use crate::contract::{
    Evidence, Governor, Resource, ScratchCharge, ScratchMap, ScratchString, ScratchVec, StopReason,
};
use std::fmt;

#[derive(Debug)]
struct Text(ScratchString);

impl ScratchCharge for Text {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 40);
        40
    };
}

/// Frontier entries share admitted prefix storage by index. Keeping one
/// scalar per accepted transition avoids cloning an arbitrarily long string
/// for every breadth-first step; the selected path is materialized only once.
struct Prefix {
    parent: usize,
    character: char,
    bytes: usize,
}

impl ScratchCharge for Prefix {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 24);
        24
    };
}

const EMPTY_PREFIX: usize = usize::MAX;

fn materialize_prefix(
    mut index: usize,
    prefixes: &ScratchVec<Prefix>,
    governor: &mut Governor,
) -> Result<ScratchString, StopReason> {
    let bytes = if index == EMPTY_PREFIX {
        0
    } else {
        prefixes.as_slice()[index].bytes
    };
    let mut reversed = ScratchVec::new(governor);
    while index != EMPTY_PREFIX {
        governor.charge(Resource::Work, 1)?;
        let prefix = &prefixes.as_slice()[index];
        reversed.push(prefix.character)?;
        index = prefix.parent;
    }
    governor.charge(Resource::Work, u64::try_from(bytes).unwrap_or(u64::MAX))?;
    let mut text = ScratchString::new(governor);
    text.reserve(bytes)?;
    for &character in reversed.as_slice().iter().rev() {
        text.push(character)?;
    }
    governor.check()?;
    Ok(text)
}

fn copy_text(text: &str, governor: &mut Governor) -> Result<Text, StopReason> {
    governor.charge(
        Resource::Work,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )?;
    let mut copied = ScratchString::new(governor);
    copied.push_str(text)?;
    Ok(Text(copied))
}

/// `fmt::Arguments` writes directly into governed storage: constructing a
/// formatted explanation must not allocate an intermediate plain string.
struct EvidenceWriter<'g> {
    text: ScratchString,
    failure: Option<StopReason>,
    governor: &'g mut Governor,
}

impl fmt::Write for EvidenceWriter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.governor
            .charge(
                Resource::Work,
                u64::try_from(text.len()).unwrap_or(u64::MAX),
            )
            .and_then(|()| self.text.push_str(text))
            .map_err(|reason| {
                self.failure = Some(reason);
                fmt::Error
            })
    }
}

impl Generated {
    fn into_public(self) -> GenerateOutcome {
        match self {
            Self::Witness(text) => GenerateOutcome::Witness(text.into_string()),
            Self::NoCandidate { rule, explanation } => GenerateOutcome::NoCandidate(Evidence {
                rule: rule.into_string(),
                explanation: explanation.into_string(),
                span: None,
                destination: String::new(),
            }),
        }
    }
}

fn no_candidate_scratch(
    explanation: fmt::Arguments<'_>,
    governor: &mut Governor,
) -> Result<Generated, StopReason> {
    governor.check()?;
    let rule = copy_text("pattern.generate.no-candidate-v1", governor)?;
    let mut writer = EvidenceWriter {
        text: ScratchString::new(governor),
        failure: None,
        governor,
    };
    if fmt::write(&mut writer, explanation).is_err() {
        return Err(writer
            .failure
            .expect("only a scratch failure stops this writer"));
    }
    writer.governor.check()?;
    Ok(Generated::NoCandidate {
        rule: rule.0,
        explanation: writer.text,
    })
}

pub(super) fn generate(
    pattern: &Pattern,
    governor: &mut Governor,
    options: GenerateOptions<'_>,
) -> Result<GenerateOutcome, StopReason> {
    let generated = generate_owned(pattern, governor, options)?;
    governor.check()?;
    Ok(generated.into_public())
}

pub(super) fn generate_owned(
    pattern: &Pattern,
    governor: &mut Governor,
    options: GenerateOptions<'_>,
) -> Result<Generated, StopReason> {
    governor.check()?;
    let backtracking = matches!(pattern.engine, Engine::Backtrack)
        || options
            .avoid
            .iter()
            .any(|p| matches!(p.engine, Engine::Backtrack));
    if backtracking {
        return generate_backtrack(pattern, governor, options);
    }
    generate_compiled(
        pattern
            .compiled
            .as_ref()
            .expect("derivative pattern has an automaton"),
        governor,
        options,
    )
}

/// One automaton participating in the product search: its compiled
/// structure, a private stepping session, and its own unified-alphabet
/// unanchored-search state.
struct Lane<'c> {
    compiled: &'c derive::Compiled,
    session: derive::Session,
    state: u32,
}

impl ScratchCharge for Lane<'_> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 176);
        176
    };
}

fn lane_of<'c>(pattern: &'c Pattern, governor: &mut Governor) -> Result<Lane<'c>, StopReason> {
    let compiled = pattern
        .compiled
        .as_ref()
        .expect("Engine::Derivative has a compiled automaton");
    lane_of_compiled(compiled, governor)
}

fn lane_of_compiled<'c>(
    compiled: &'c derive::Compiled,
    governor: &mut Governor,
) -> Result<Lane<'c>, StopReason> {
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

impl ScratchCharge for LaneState {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 8);
        8
    };
}

/// Assertion context belongs to the product state: two equal derivative
/// terms reached after a word and a non-word character can accept differently
/// and have different successors. The initial position is distinct as well.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ProductState {
    lanes: Buffer<LaneState>,
    at_start: bool,
    prev_word: bool,
}

impl ScratchCharge for ProductState {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 48);
        48
    };
}

impl ProductState {
    fn try_clone(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(
            Resource::Work,
            u64::try_from(self.lanes.values.len()).unwrap_or(u64::MAX),
        )?;
        let mut lanes = ScratchVec::new(governor);
        lanes.extend(self.lanes.values.iter().copied())?;
        Ok(Self {
            lanes: Buffer::from_scratch(lanes, governor),
            at_start: self.at_start,
            prev_word: self.prev_word,
        })
    }

    const fn context(&self, at_end: bool, next_word: bool) -> NullCtx {
        NullCtx {
            at_start: self.at_start,
            at_end,
            prev_word: self.prev_word,
            next_word,
        }
    }
}

fn state_matches(
    lane: &mut Lane<'_>,
    state: LaneState,
    ctx: NullCtx,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.check()?;
    if state.matched {
        Ok(true)
    } else {
        lane.session.nullable(state.live, ctx, governor)
    }
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
    if state_matches(lane, current, ctx, governor)? {
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
        live: lane.session.union(stepped, root, governor)?,
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
    let mut lanes = ScratchVec::new(governor);
    lanes.push(step_lane(
        target,
        state.lanes.values[0],
        code,
        ctx,
        governor,
    )?)?;
    for (lane, &current) in avoids.iter_mut().zip(&state.lanes.values[1..]) {
        let next = step_lane(lane, current, code, ctx, governor)?;
        if next.matched {
            return Ok(None);
        }
        lanes.push(next)?;
    }
    Ok(Some(ProductState {
        lanes: Buffer::from_scratch(lanes, governor),
        at_start: false,
        prev_word: next_word,
    }))
}

pub(super) fn generate_compiled(
    compiled: &derive::Compiled,
    governor: &mut Governor,
    options: GenerateOptions<'_>,
) -> Result<Generated, StopReason> {
    governor.check()?;
    let mut target = lane_of_compiled(compiled, governor)?;
    let mut avoids = ScratchVec::new(governor);
    for &avoid in options.avoid {
        avoids.push(lane_of(avoid, governor)?)?;
    }
    let mut avoids = Buffer::from_scratch(avoids, governor);
    let representatives = representatives(&target, &avoids.values, governor)?;

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
    let mut lanes = ScratchVec::new(governor);
    lanes.extend(
        std::iter::once(target.state)
            .chain(avoids.values.iter().map(|lane| lane.state))
            .map(|live| LaneState {
                live,
                matched: false,
            }),
    )?;
    let mut frontier = ScratchVec::new(governor);
    frontier.push((
        ProductState {
            lanes: Buffer::from_scratch(lanes, governor),
            at_start: true,
            prev_word: false,
        },
        EMPTY_PREFIX,
    ))?;
    let mut frontier = Buffer::from_scratch(frontier, governor);
    let mut prefixes = ScratchVec::new(governor);
    let mut length = 0usize;
    // Once every current-length nullability check has failed, remember the
    // *set* of live states so far seen at length >= `options.min_len`
    // (stepping is a deterministic function of that set, so a repeated set
    // reproduces the exact same set forever). Recognizing that cycle is what
    // still lets this search prove emptiness *within these bounds* even
    // though states below `options.min_len` are deliberately never pruned
    // (see the `frontier` doc comment above).
    let mut seen_state_sets: ScratchMap<Buffer<ProductState>, ()> = ScratchMap::new(governor);
    loop {
        governor.charge(
            Resource::Work,
            u64::try_from(frontier.values.len()).unwrap_or(u64::MAX),
        )?;
        if length >= options.min_len {
            for (state, witness) in &frontier.values {
                let ctx = state.context(true, false);
                if !state_matches(&mut target, state.lanes.values[0], ctx, governor)? {
                    continue;
                }
                let mut forbidden = false;
                for (lane, &current) in avoids.values.iter_mut().zip(&state.lanes.values[1..]) {
                    if state_matches(lane, current, ctx, governor)? {
                        forbidden = true;
                        break;
                    }
                }
                if !forbidden {
                    return Ok(Generated::Witness(materialize_prefix(
                        *witness, &prefixes, governor,
                    )?));
                }
            }
            let mut current_set = ScratchVec::new(governor);
            for (state, _) in &frontier.values {
                current_set.push(state.try_clone(governor)?)?;
            }
            let mut current_set = Buffer::from_scratch(current_set, governor);
            current_set.values.sort_unstable();
            current_set.values.dedup();
            if seen_state_sets.get(&current_set).is_some() {
                return no_candidate_scratch(
                    format_args!(
                        "the language is empty within these bounds: the reachable state set has cycled back to one already checked for acceptance without ever satisfying it, so no longer witness can ever satisfy it either"
                    ),
                    governor,
                );
            }
            seen_state_sets.insert(current_set, ())?;
        }
        if options.max_len.is_some_and(|max| length >= max) {
            return no_candidate_scratch(
                format_args!(
                    "no witness of length <= {} exists: the search of the product automaton (this pattern and its {} `avoid` pattern(s)) exhausted every reachable state without ever satisfying the acceptance condition",
                    options.max_len.unwrap_or_default(),
                    avoids.values.len()
                ),
                governor,
            );
        }
        let next_frontier = step_frontier(
            &frontier.values,
            &representatives.values,
            &mut target,
            &mut avoids.values,
            &mut prefixes,
            governor,
        )?;
        if next_frontier.values.is_empty() {
            return no_candidate_scratch(
                format_args!(
                    "the language is empty: the product automaton (this pattern and every `avoid` pattern) has no reachable accepting state"
                ),
                governor,
            );
        }
        frontier = next_frontier;
        length += 1;
    }
}

/// One representative scalar per unified minterm bucket. Reuse the admitted
/// boundary storage after sorting; surrogate buckets are skipped. Charge
/// work for copied boundaries and their sorting/scanning before those passes.
fn representatives(
    target: &Lane<'_>,
    avoids: &[Lane<'_>],
    governor: &mut Governor,
) -> Result<Buffer<u32>, StopReason> {
    let mut cuts = ScratchVec::new(governor);
    cuts.push(0)?;
    for lane in std::iter::once(target).chain(avoids) {
        let boundaries = derive::cuts(lane.compiled);
        governor.charge(
            Resource::Work,
            u64::try_from(boundaries.len()).unwrap_or(u64::MAX),
        )?;
        cuts.extend(boundaries.iter().copied())?;
    }
    let length = u64::try_from(cuts.len()).unwrap_or(u64::MAX);
    governor.charge(
        Resource::Work,
        length.saturating_mul(u64::from(length.ilog2()) + 2),
    )?;
    let mut representatives = Buffer::from_scratch(cuts, governor);
    representatives.values.sort_unstable();
    representatives.values.dedup();
    representatives
        .values
        .retain(|&code| char::from_u32(code).is_some());
    Ok(representatives)
}

/// Step every frontier entry by one character over every representative
/// code point, deduplicating *within this one step* (never across steps:
/// see the `frontier` doc comment in [`generate_derivative`]).
fn step_frontier(
    frontier: &[(ProductState, usize)],
    representatives: &[u32],
    target: &mut Lane<'_>,
    avoids: &mut [Lane<'_>],
    prefixes: &mut ScratchVec<Prefix>,
    governor: &mut Governor,
) -> Result<Buffer<(ProductState, usize)>, StopReason> {
    let mut next_frontier = ScratchVec::new(governor);
    let mut seen_this_step: ScratchMap<ProductState, ()> = ScratchMap::new(governor);
    for (state, witness) in frontier {
        for &code in representatives {
            let Some(character) = char::from_u32(code) else {
                continue;
            };
            let Some(next_state) = step_lanes(target, avoids, state, code, governor)? else {
                continue;
            };
            if seen_this_step.get(&next_state).is_none() {
                seen_this_step.insert(next_state.try_clone(governor)?, ())?;
                governor.charge(Resource::Work, 1)?;
                let bytes = if *witness == EMPTY_PREFIX {
                    0
                } else {
                    prefixes.as_slice()[*witness].bytes
                }
                .saturating_add(character.len_utf8());
                let extended = prefixes.len();
                prefixes.push(Prefix {
                    parent: *witness,
                    character,
                    bytes,
                })?;
                next_frontier.push((next_state, extended))?;
            }
        }
    }
    Ok(Buffer::from_scratch(next_frontier, governor))
}

/// One structurally generated candidate: its text so far, and the substring
/// each capturing group produced along *this* path (so a later
/// `Backref` can reuse its participating group's actual text instead of
/// guessing).
struct Candidate {
    text: Text,
    captures: ScratchMap<usize, Text>,
}

impl ScratchCharge for Candidate {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 80);
        80
    };
}

impl Candidate {
    fn try_clone(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        let text = copy_text(self.text.0.as_str(), governor)?;
        let mut captures = ScratchMap::new(governor);
        for (index, captured) in self.captures.iter() {
            captures.insert(*index, copy_text(captured.0.as_str(), governor)?)?;
        }
        Ok(Self { text, captures })
    }
}

/// Bounded structural enumeration from the syntax tree, verified with
/// `Pattern::is_match` (which alone knows backreference/lookaround
/// semantics); exhaustion is reported as such, never as proven emptiness.
fn generate_backtrack(
    pattern: &Pattern,
    governor: &mut Governor,
    options: GenerateOptions<'_>,
) -> Result<Generated, StopReason> {
    let start = Candidate {
        text: Text(ScratchString::new(governor)),
        captures: ScratchMap::new(governor),
    };
    let mut budget = 20_000usize;
    let mut candidates = collect_candidates(&pattern.ast, start, &mut budget, governor)?;
    // Stable insertion ordering preserves enumeration's tie order without
    // the auxiliary allocation of a stable standard-library sort.
    for index in 1..candidates.values.len() {
        let length = candidates.values[index].text.0.len();
        let start = candidates.values[..index]
            .partition_point(|candidate| candidate.text.0.len() <= length);
        governor.charge(
            Resource::Work,
            u64::try_from(index - start + 1).unwrap_or(u64::MAX),
        )?;
        candidates.values[start..=index].rotate_right(1);
    }
    let mut tried = ScratchVec::new(governor);
    for index in 0..candidates.values.len() {
        let text = candidates.values[index].text.0.as_str();
        let mut repeated = false;
        for &previous in tried.as_slice() {
            let prior: &Candidate = &candidates.values[previous];
            governor.charge(
                Resource::Work,
                u64::try_from(prior.text.0.len().min(text.len())).unwrap_or(u64::MAX),
            )?;
            if prior.text.0.as_str() == text {
                repeated = true;
                break;
            }
        }
        if repeated {
            continue;
        }
        tried.push(index)?;
        governor.charge(Resource::Work, 1)?;
        governor.charge(
            Resource::Work,
            u64::try_from(text.len()).unwrap_or(u64::MAX),
        )?;
        let length = text.chars().count();
        if length < options.min_len {
            continue;
        }
        if options.max_len.is_some_and(|max| length > max) {
            continue;
        }
        if !pattern.is_match(text, governor)? {
            continue;
        }
        let mut forbidden = false;
        for &avoid in options.avoid {
            if avoid.is_match(text, governor)? {
                forbidden = true;
                break;
            }
        }
        if !forbidden {
            governor.check()?;
            let text = std::mem::replace(
                &mut candidates.values[index].text.0,
                ScratchString::new(governor),
            );
            return Ok(Generated::Witness(text));
        }
    }
    no_candidate_scratch(
        format_args!(
            "the bounded backtracking candidate search (this pattern or an `avoid` pattern needs backreference/lookaround semantics that only structural enumeration plus verification can check) found no witness matching this pattern and none of the `avoid` patterns; this is exhaustion of a bounded search, not a proof that no witness exists"
        ),
        governor,
    )
}

fn collect_candidates(
    node: &Node,
    prefix: Candidate,
    budget: &mut usize,
    governor: &mut Governor,
) -> Result<Buffer<Candidate>, StopReason> {
    governor.charge(Resource::Work, 1)?;
    *budget = budget.saturating_sub(1);
    if *budget == 0 {
        let mut out = ScratchVec::new(governor);
        out.push(prefix)?;
        return Ok(Buffer::from_scratch(out, governor));
    }
    let mut out = ScratchVec::new(governor);
    match node {
        // Zero-width: none of these ever contributes text of its own. A
        // lookaround's own content never becomes part of the matched text
        // either, so it is treated the same way here; `Pattern::is_match`
        // alone verifies the assertion actually holds for the candidate.
        Node::Empty | Node::Start | Node::End | Node::Boundary(_) | Node::Look { .. } => {
            out.push(prefix)?;
        }
        Node::NoMatch => {}
        Node::Class(intervals) => {
            if let Some(character) = intervals
                .first()
                .and_then(|&(start, _)| char::from_u32(start))
            {
                let mut next = prefix;
                next.text.0.push(character)?;
                out.push(next)?;
            }
        }
        Node::Concat(items) => {
            out.push(prefix)?;
            let mut current = Buffer::from_scratch(out, governor);
            for item in items {
                current = advance_candidates(item, current, 16, budget, governor)?;
                if current.values.is_empty() {
                    break;
                }
            }
            return Ok(current);
        }
        Node::Alternate(items) => {
            for item in items {
                let branch = prefix.try_clone(governor)?;
                out.extend(collect_candidates(item, branch, budget, governor)?)?;
                if out.len() > 16 {
                    break;
                }
            }
            let mut out = Buffer::from_scratch(out, governor);
            out.values.truncate(16);
            return Ok(out);
        }
        Node::Repeat(owned) => {
            let repetition = &owned[0];
            for count in [repetition.min, repetition.min.saturating_add(1)] {
                if repetition.max.is_some_and(|max| count > max) {
                    continue;
                }
                let mut current = ScratchVec::new(governor);
                current.push(prefix.try_clone(governor)?)?;
                let mut current = Buffer::from_scratch(current, governor);
                for _ in 0..count {
                    current = advance_candidates(&repetition.inner, current, 4, budget, governor)?;
                    if current.values.is_empty() {
                        break;
                    }
                }
                out.extend(current)?;
            }
        }
        Node::Group(index, inner) => {
            let start_len = prefix.text.0.len();
            for mut candidate in collect_candidates(&inner[0], prefix, budget, governor)? {
                let captured = copy_text(&candidate.text.0.as_str()[start_len..], governor)?;
                candidate.captures.insert(*index, captured)?;
                out.push(candidate)?;
            }
        }
        Node::Backref(indices) => {
            let mut next = prefix;
            // A single generated candidate only ever took one branch of
            // any `Alternate`, so at most one index sharing a duplicated
            // name can have a capture on this path already (see
            // `ast::Node::Backref`'s doc comment).
            if let Some(text) = indices.iter().find_map(|index| next.captures.get(index)) {
                governor.charge(
                    Resource::Work,
                    u64::try_from(text.0.len()).unwrap_or(u64::MAX),
                )?;
                next.text.0.push_str(text.0.as_str())?;
            }
            out.push(next)?;
        }
    }
    Ok(Buffer::from_scratch(out, governor))
}

/// The same bounded branch expansion is used by concatenation and repetition.
/// Consuming the input retains its buffer lease until the iterator is dropped,
/// including an early stop after the enumeration limit is reached.
fn advance_candidates(
    node: &Node,
    current: Buffer<Candidate>,
    limit: usize,
    budget: &mut usize,
    governor: &mut Governor,
) -> Result<Buffer<Candidate>, StopReason> {
    let mut next = ScratchVec::new(governor);
    for base in current {
        next.extend(collect_candidates(node, base, budget, governor)?)?;
        if next.len() > limit {
            break;
        }
    }
    let mut next = Buffer::from_scratch(next, governor);
    next.values.truncate(limit);
    Ok(next)
}
