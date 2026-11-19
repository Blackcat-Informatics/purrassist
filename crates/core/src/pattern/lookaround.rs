// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Finite compilation for regular expressions with look-around. A look
//! assertion's machine reads the whole subject with one boundary marker. Its
//! prefix state and the right-context acceptance set decide the assertion
//! without changing the current subject position.

use super::{
    ast::{Intervals, Node, Repetition},
    is_word,
    unicode::interval_contains,
};
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchOrdMap, ScratchVec, StopReason, push_scratch,
    saturating_size,
};

const STATE_LIMIT: usize = 4096;
const CELL_LIMIT: usize = 65_536;
const ACCEPT: usize = 0;
const SCALAR_END: u32 = 0x11_0000;

/// Why a regular look-around pattern could not be compiled.
#[derive(Debug)]
pub enum LookaroundCompileError {
    /// An intermediate or final automaton exceeded the bounded product size.
    SizeLimit,
    /// The governor refused the compilation.
    Resource(StopReason),
}

impl From<StopReason> for LookaroundCompileError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

type Result<T> = core::result::Result<T, LookaroundCompileError>;

/// Shared by the outer program and every nested guard. Cells bound dense
/// rows and subsets as well as the number of states in each machine.
struct Limits {
    states: usize,
    cells: usize,
}

impl Limits {
    const fn new() -> Self {
        Self {
            states: STATE_LIMIT,
            cells: CELL_LIMIT,
        }
    }

    fn state(&mut self, cells: usize) -> Result<()> {
        self.states = self
            .states
            .checked_sub(1)
            .ok_or(LookaroundCompileError::SizeLimit)?;
        self.cells(cells)
    }

    fn cells(&mut self, count: usize) -> Result<()> {
        self.cells = self
            .cells
            .checked_sub(count)
            .ok_or(LookaroundCompileError::SizeLimit)?;
        Ok(())
    }
}

/// All machines use the same scalar partition. Every AST character class and
/// the word-boundary class is constant on each inclusive interval.
type Alphabet = ScratchVec<(char, char)>;

fn alphabet(node: &Node, limits: &mut Limits, governor: &mut Governor) -> Result<Alphabet> {
    let mut cuts = ScratchVec::new(governor);
    for cut in [
        0,
        0xD800,
        0xE000,
        SCALAR_END,
        u32::from(b'0'),
        u32::from(b'9') + 1,
        u32::from(b'A'),
        u32::from(b'Z') + 1,
        u32::from(b'_'),
        u32::from(b'_') + 1,
        u32::from(b'a'),
        u32::from(b'z') + 1,
    ] {
        push_scratch(&mut cuts, cut, governor)?;
    }
    class_cuts(node, &mut cuts, limits, governor)?;
    let count = saturating_size(cuts.len());
    governor.charge(Resource::Work, count * (1 + u64::from(count.ilog2())))?;
    cuts.as_mut_slice().sort_unstable();
    let mut ranges = ScratchVec::new(governor);
    for pair in cuts.windows(2) {
        if pair[0] == pair[1] {
            continue;
        }
        // The sentinel cuts exclude the surrogate interval from the alphabet.
        if let (Some(first), Some(last)) = (char::from_u32(pair[0]), char::from_u32(pair[1] - 1)) {
            limits.cells(1)?;
            push_scratch(&mut ranges, (first, last), governor)?;
        }
    }
    Ok(ranges)
}

fn class_cuts(
    node: &Node,
    cuts: &mut ScratchVec<u32>,
    limits: &mut Limits,
    governor: &mut Governor,
) -> Result<()> {
    governor.charge(Resource::Work, 1)?;
    match node {
        Node::Class(intervals) => {
            limits.cells(intervals.len().saturating_mul(2))?;
            for &(first, last) in intervals {
                push_scratch(cuts, first, governor)?;
                push_scratch(cuts, last + 1, governor)?;
            }
        }
        Node::Concat(nodes)
        | Node::Alternate(nodes)
        | Node::Group(_, nodes)
        | Node::Look { inner: nodes, .. } => {
            for child in nodes {
                class_cuts(child, cuts, limits, governor)?;
            }
        }
        Node::Repeat(repeat) => class_cuts(&repeat[0].inner, cuts, limits, governor)?,
        Node::Empty
        | Node::NoMatch
        | Node::Start
        | Node::End
        | Node::Boundary(_)
        | Node::Backref(_) => {}
    }
    Ok(())
}

fn has_backref(node: &Node, governor: &mut Governor) -> Result<bool> {
    governor.charge(Resource::Work, 1)?;
    match node {
        Node::Backref(_) => Ok(true),
        Node::Concat(nodes)
        | Node::Alternate(nodes)
        | Node::Group(_, nodes)
        | Node::Look { inner: nodes, .. } => {
            for child in nodes {
                if has_backref(child, governor)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Node::Repeat(repeat) => has_backref(&repeat[0].inner, governor),
        _ => Ok(false),
    }
}

#[derive(Clone, Copy)]
enum Condition {
    Start,
    End,
    WordBoundary(bool),
    Look { guard: usize, positive: bool },
}

#[derive(Clone, Copy)]
enum Instruction<'a> {
    Fail,
    Accept,
    Jump(usize),
    Split(usize, usize),
    Test(Condition, usize),
    Scalar(Option<&'a Intervals>, usize),
    Marker(usize),
}

impl ScratchCharge for Instruction<'_> {
    const CHARGE: usize = 40;
}
const _: () = assert!(core::mem::size_of::<Instruction<'_>>() <= 40);

#[derive(Clone, Copy)]
enum Entry {
    Search,
    Ahead,
    Behind,
}

struct Program<'a> {
    code: ScratchVec<Instruction<'a>>,
    guards: ScratchVec<Guard<'a>>,
    start: usize,
    word_boundary: bool,
}

struct Guard<'a> {
    source: &'a Node,
    behind: bool,
    machine: Machine,
    offset: usize,
}

impl ScratchCharge for Guard<'_> {
    const CHARGE: usize = 64;
}
const _: () = assert!(core::mem::size_of::<Guard<'_>>() <= 64);

impl<'a> Program<'a> {
    fn push(
        &mut self,
        instruction: Instruction<'a>,
        limits: &mut Limits,
        governor: &mut Governor,
    ) -> Result<usize> {
        limits.state(1)?;
        let id = self.code.len();
        push_scratch(&mut self.code, instruction, governor)?;
        Ok(id)
    }

    /// Compile a Thompson fragment with the given continuation. An assertion
    /// is an epsilon test. Finite repeats share child guard machines even
    /// when they expand the child's consuming instructions.
    fn fragment(
        &mut self,
        node: &'a Node,
        next: usize,
        alphabet: &Alphabet,
        limits: &mut Limits,
        governor: &mut Governor,
    ) -> Result<usize> {
        governor.charge(Resource::Work, 1)?;
        let instruction = match node {
            Node::NoMatch => Instruction::Fail,
            Node::Empty => Instruction::Jump(next),
            Node::Class(intervals) => Instruction::Scalar(Some(intervals), next),
            Node::Start => Instruction::Test(Condition::Start, next),
            Node::End => Instruction::Test(Condition::End, next),
            Node::Boundary(positive) => {
                self.word_boundary = true;
                Instruction::Test(Condition::WordBoundary(*positive), next)
            }
            Node::Group(_, child) => {
                return self.fragment(&child[0], next, alphabet, limits, governor);
            }
            Node::Concat(nodes) => {
                return nodes.iter().rev().try_fold(next, |next, child| {
                    self.fragment(child, next, alphabet, limits, governor)
                });
            }
            Node::Alternate(nodes) => {
                let mut branches = self.push(Instruction::Fail, limits, governor)?;
                for child in nodes {
                    let branch = self.fragment(child, next, alphabet, limits, governor)?;
                    branches = self.push(Instruction::Split(branch, branches), limits, governor)?;
                }
                return Ok(branches);
            }
            Node::Repeat(repeat) => {
                return self.repetition(&repeat[0], next, alphabet, limits, governor);
            }
            Node::Look {
                inner,
                positive,
                behind,
            } => {
                governor.charge(Resource::Work, saturating_size(self.guards.len()))?;
                let child = &inner[0];
                let guard =
                    if let Some(id) = self.guards.iter().position(|guard| {
                        core::ptr::eq(guard.source, child) && guard.behind == *behind
                    }) {
                        id
                    } else {
                        let entry = if *behind { Entry::Behind } else { Entry::Ahead };
                        let machine = compile_machine(child, entry, alphabet, limits, governor)?;
                        let id = self.guards.len();
                        let offset = self
                            .guards
                            .last()
                            .map_or(0, |guard| guard.offset + guard.machine.rows.len());
                        push_scratch(
                            &mut self.guards,
                            Guard {
                                source: child,
                                behind: *behind,
                                machine,
                                offset,
                            },
                            governor,
                        )?;
                        id
                    };
                Instruction::Test(
                    Condition::Look {
                        guard,
                        positive: *positive,
                    },
                    next,
                )
            }
            Node::Backref(_) => unreachable!("backreferences are checked before compilation"),
        };
        self.push(instruction, limits, governor)
    }

    fn repetition(
        &mut self,
        repetition: &'a Repetition,
        next: usize,
        alphabet: &Alphabet,
        limits: &mut Limits,
        governor: &mut Governor,
    ) -> Result<usize> {
        let mut target = next;
        if let Some(max) = repetition.max {
            for _ in repetition.min..max {
                let optional =
                    self.fragment(&repetition.inner, target, alphabet, limits, governor)?;
                target = self.push(Instruction::Split(optional, target), limits, governor)?;
            }
        } else {
            let loop_start = self.push(Instruction::Fail, limits, governor)?;
            let repeated =
                self.fragment(&repetition.inner, loop_start, alphabet, limits, governor)?;
            self.code.as_mut_slice()[loop_start] = Instruction::Split(repeated, target);
            target = loop_start;
        }
        for _ in 0..repetition.min {
            target = self.fragment(&repetition.inner, target, alphabet, limits, governor)?;
        }
        if target == next {
            return self.push(Instruction::Jump(next), limits, governor);
        }
        Ok(target)
    }
}

/// Wrap the expression with unanchored prefix/suffix search. For an ahead
/// guard only the marker can begin the body. For a behind guard only a body
/// ending at the marker can enter the accepting suffix state.
fn program<'a>(
    node: &'a Node,
    entry: Entry,
    alphabet: &Alphabet,
    limits: &mut Limits,
    governor: &mut Governor,
) -> Result<Program<'a>> {
    let mut program = Program {
        code: ScratchVec::new(governor),
        guards: ScratchVec::new(governor),
        start: 0,
        word_boundary: false,
    };
    program.push(Instruction::Accept, limits, governor)?;
    let next = match entry {
        Entry::Behind => program.push(Instruction::Marker(ACCEPT), limits, governor)?,
        Entry::Search | Entry::Ahead => ACCEPT,
    };
    let body = program.fragment(node, next, alphabet, limits, governor)?;
    let begin = match entry {
        Entry::Ahead => program.push(Instruction::Marker(body), limits, governor)?,
        Entry::Search | Entry::Behind => body,
    };
    let prefix = program.push(Instruction::Fail, limits, governor)?;
    let skip = program.push(Instruction::Scalar(None, prefix), limits, governor)?;
    program.code.as_mut_slice()[prefix] = Instruction::Split(begin, skip);
    program.start = prefix;
    Ok(program)
}

#[derive(Debug)]
struct Machine {
    rows: ScratchVec<Row>,
}

#[derive(Debug)]
struct Row {
    accepting: bool,
    scalar: ScratchVec<usize>,
    marker: usize,
}

impl ScratchCharge for Row {
    const CHARGE: usize = 56;
}
const _: () = assert!(core::mem::size_of::<Row>() <= 56);

/// The combined right-context state says which child states would accept
/// the remaining suffix. Row zero describes the empty suffix; prefixing one
/// scalar applies the preimage of that scalar's transition in every child.
struct Suffix {
    accepts: ScratchVec<bool>,
    prefixed: ScratchVec<usize>,
}

impl ScratchCharge for Suffix {
    const CHARGE: usize = 80;
}
const _: () = assert!(core::mem::size_of::<Suffix>() <= 80);

fn suffixes(
    guards: &[Guard<'_>],
    width: usize,
    limits: &mut Limits,
    governor: &mut Governor,
) -> Result<ScratchVec<Suffix>> {
    let child_rows = guards
        .last()
        .map_or(0, |guard| guard.offset + guard.machine.rows.len());
    limits.state(child_rows + width)?;
    let mut empty = ScratchVec::new(governor);
    governor.charge(Resource::Work, saturating_size(child_rows))?;
    empty.extend(
        guards
            .iter()
            .flat_map(|guard| guard.machine.rows.iter().map(|row| row.accepting)),
    )?;
    let mut suffixes = ScratchVec::new(governor);
    push_scratch(
        &mut suffixes,
        Suffix {
            accepts: empty,
            prefixed: ScratchVec::new(governor),
        },
        governor,
    )?;
    let mut cursor = 0;
    while cursor < suffixes.len() {
        let mut prefixed = ScratchVec::new(governor);
        for symbol in 0..width {
            governor.charge(Resource::Work, saturating_size(child_rows))?;
            let mut preimage = ScratchVec::new(governor);
            for guard in guards {
                preimage.extend(
                    guard
                        .machine
                        .rows
                        .iter()
                        .map(|row| suffixes[cursor].accepts[guard.offset + row.scalar[symbol]]),
                )?;
            }
            governor.charge(
                Resource::Work,
                saturating_size(child_rows + 1).saturating_mul(saturating_size(suffixes.len())),
            )?;
            let target = if let Some(id) = suffixes
                .iter()
                .position(|suffix| suffix.accepts == preimage)
            {
                id
            } else {
                limits.state(child_rows + width)?;
                let id = suffixes.len();
                push_scratch(
                    &mut suffixes,
                    Suffix {
                        accepts: preimage,
                        prefixed: ScratchVec::new(governor),
                    },
                    governor,
                )?;
                id
            };
            push_scratch(&mut prefixed, target, governor)?;
        }
        suffixes.as_mut_slice()[cursor].prefixed = prefixed;
        cursor += 1;
    }
    Ok(suffixes)
}

#[derive(PartialEq, Eq)]
struct Trial {
    suffix: usize,
    active: ScratchVec<bool>,
}

impl ScratchCharge for Trial {
    const CHARGE: usize = 48;
}
const _: () = assert!(core::mem::size_of::<Trial>() <= 48);

/// All trials for a deterministic state share the prefix and boundary facts.
/// The trials represent different remaining suffixes and are sorted by ID.
#[derive(PartialEq, Eq)]
struct Search {
    prefix: ScratchVec<usize>,
    trials: ScratchVec<Trial>,
    beginning: bool,
    previous_word: bool,
    pending_marker: bool,
}

impl ScratchCharge for Search {
    const CHARGE: usize = 88;
}
const _: () = assert!(core::mem::size_of::<Search>() <= 88);

impl Search {
    fn empty(governor: &Governor) -> Self {
        Self {
            prefix: ScratchVec::new(governor),
            trials: ScratchVec::new(governor),
            beginning: false,
            previous_word: false,
            pending_marker: false,
        }
    }

    fn marker(&self, governor: &mut Governor) -> Result<Self> {
        if self.pending_marker || self.trials.is_empty() {
            return Ok(Self::empty(governor));
        }
        let mut marked = Self {
            beginning: self.beginning,
            previous_word: self.previous_word,
            pending_marker: true,
            ..Self::empty(governor)
        };
        governor.charge(Resource::Work, saturating_size(self.prefix.len()))?;
        marked.prefix.extend(self.prefix.iter().copied())?;
        for trial in &self.trials {
            governor.charge(Resource::Work, saturating_size(trial.active.len()))?;
            let mut active = ScratchVec::new(governor);
            active.extend(trial.active.iter().copied())?;
            push_scratch(
                &mut marked.trials,
                Trial {
                    suffix: trial.suffix,
                    active,
                },
                governor,
            )?;
        }
        Ok(marked)
    }
}

struct Boundary<'a> {
    before_word: bool,
    following: Option<char>,
    beginning: bool,
    prefix: &'a [usize],
    suffix: &'a [bool],
}

impl Boundary<'_> {
    fn allows(&self, condition: Condition, guards: &[Guard<'_>]) -> bool {
        match condition {
            Condition::Start => self.beginning,
            Condition::End => self.following.is_none(),
            Condition::WordBoundary(positive) => {
                (self.before_word != self.following.is_some_and(is_word)) == positive
            }
            Condition::Look { guard, positive } => {
                let machine = &guards[guard];
                let marked = machine.machine.rows[self.prefix[guard]].marker;
                self.suffix[machine.offset + marked] == positive
            }
        }
    }
}

fn bits(count: usize, governor: &mut Governor) -> Result<ScratchVec<bool>> {
    governor.charge(Resource::Work, saturating_size(count))?;
    let mut set = ScratchVec::new(governor);
    set.extend(core::iter::repeat_n(false, count))?;
    Ok(set)
}

fn closure(
    program: &Program<'_>,
    active: &mut ScratchVec<bool>,
    boundary: &Boundary<'_>,
    governor: &mut Governor,
) -> Result<()> {
    governor.charge(Resource::Work, saturating_size(active.len()))?;
    let mut pending = ScratchVec::new(governor);
    for (id, selected) in active.iter().enumerate() {
        if *selected {
            push_scratch(&mut pending, id, governor)?;
        }
    }
    while let Some(id) = pending.pop() {
        governor.charge(Resource::Work, 1)?;
        let targets = match program.code[id] {
            Instruction::Jump(next) => [Some(next), None],
            Instruction::Split(left, right) => [Some(left), Some(right)],
            Instruction::Test(condition, next) if boundary.allows(condition, &program.guards) => {
                [Some(next), None]
            }
            _ => continue,
        };
        for next in targets.into_iter().flatten() {
            if !active[next] {
                active.as_mut_slice()[next] = true;
                push_scratch(&mut pending, next, governor)?;
            }
        }
    }
    Ok(())
}

fn boundary_states(
    program: &Program<'_>,
    search: &Search,
    trial: &Trial,
    suffixes: &[Suffix],
    following: Option<char>,
    governor: &mut Governor,
) -> Result<ScratchVec<bool>> {
    let boundary = Boundary {
        before_word: search.previous_word,
        following,
        beginning: search.beginning,
        prefix: &search.prefix,
        suffix: &suffixes[trial.suffix].accepts,
    };
    governor.charge(Resource::Work, saturating_size(trial.active.len()))?;
    let mut states = ScratchVec::new(governor);
    states.extend(trial.active.iter().copied())?;
    closure(program, &mut states, &boundary, governor)?;
    if search.pending_marker {
        // A marker is not a scalar. Its actual right neighbor must be known
        // before evaluating epsilon tests on either side of the marker.
        let mut marked = bits(states.len(), governor)?;
        for (&instruction, &active) in program.code.iter().zip(states.iter()) {
            if let (true, Instruction::Marker(next)) = (active, instruction) {
                marked.as_mut_slice()[next] = true;
            }
        }
        closure(program, &mut marked, &boundary, governor)?;
        states = marked;
    }
    Ok(states)
}

fn advance(
    program: &Program<'_>,
    search: &Search,
    suffixes: &[Suffix],
    symbol: usize,
    scalar: char,
    governor: &mut Governor,
) -> Result<Search> {
    let mut next = Search::empty(governor);
    if search.trials.is_empty() {
        return Ok(next);
    }
    governor.charge(Resource::Work, saturating_size(suffixes.len()))?;
    let trial_lookup = u64::from(search.trials.len().ilog2() + 1);
    for (id, suffix) in suffixes.iter().enumerate() {
        // A forward step reverses the deterministic prefixing relation.
        // The predecessor suffix is unique for each next suffix and scalar.
        let predecessor = suffix.prefixed[symbol];
        governor.charge(Resource::Work, trial_lookup)?;
        let Ok(trial) = search
            .trials
            .binary_search_by_key(&predecessor, |trial| trial.suffix)
        else {
            continue;
        };
        let active = boundary_states(
            program,
            search,
            &search.trials[trial],
            suffixes,
            Some(scalar),
            governor,
        )?;
        let mut consumed = bits(active.len(), governor)?;
        for (&instruction, &selected) in program.code.iter().zip(active.iter()) {
            if !selected {
                continue;
            }
            let target = match instruction {
                Instruction::Accept => ACCEPT,
                Instruction::Scalar(None, target) => target,
                Instruction::Scalar(Some(intervals), target) => {
                    governor.charge(
                        Resource::Work,
                        u64::from(intervals.len().max(1).ilog2() + 1),
                    )?;
                    if !interval_contains(intervals, u32::from(scalar)) {
                        continue;
                    }
                    target
                }
                _ => continue,
            };
            consumed.as_mut_slice()[target] = true;
        }
        governor.charge(Resource::Work, saturating_size(consumed.len()))?;
        if consumed.iter().any(|&active| active) {
            push_scratch(
                &mut next.trials,
                Trial {
                    suffix: id,
                    active: consumed,
                },
                governor,
            )?;
        }
    }
    if !next.trials.is_empty() {
        governor.charge(Resource::Work, saturating_size(program.guards.len()))?;
        next.prefix.extend(
            program
                .guards
                .iter()
                .zip(search.prefix.iter())
                .map(|(guard, &state)| guard.machine.rows[state].scalar[symbol]),
        )?;
        next.previous_word = program.word_boundary && is_word(scalar);
    }
    Ok(next)
}

/// Look up a complete state without scanning every previously interned key.
/// Fingerprints only select candidates; full state equality resolves collisions.
struct StateIndex {
    heads: ScratchOrdMap<u64, usize>,
    previous: ScratchVec<usize>,
}

impl StateIndex {
    fn new(governor: &Governor) -> Self {
        Self {
            heads: ScratchOrdMap::new(governor),
            previous: ScratchVec::new(governor),
        }
    }

    fn key(search: &Search, governor: &mut Governor) -> Result<(u64, usize)> {
        governor.charge(Resource::Work, saturating_size(search.trials.len()))?;
        let cells = search.prefix.len()
            + search
                .trials
                .iter()
                .map(|trial| trial.active.len() + 1)
                .sum::<usize>();
        governor.charge(Resource::Work, saturating_size(cells + 4))?;
        let mut key = 0xcbf2_9ce4_8422_2325_u64;
        let mut feed = |value: u64| key = (key ^ value).wrapping_mul(0x0100_0000_01b3);
        feed(
            u64::from(search.beginning)
                | (u64::from(search.previous_word) << 1)
                | (u64::from(search.pending_marker) << 2),
        );
        feed(saturating_size(search.prefix.len()));
        for &state in &search.prefix {
            feed(saturating_size(state));
        }
        feed(saturating_size(search.trials.len()));
        for trial in &search.trials {
            feed(saturating_size(trial.suffix));
            for &active in &trial.active {
                feed(u64::from(active));
            }
        }
        Ok((key, cells))
    }

    fn add(&mut self, key: u64, id: usize, governor: &mut Governor) -> Result<()> {
        // Bound the fixed-width comparisons in the ordered index.
        let lookup = u64::from(id.max(1).ilog2() + 1) * 16;
        governor.charge(Resource::Work, lookup.saturating_mul(2))?;
        let previous = self.heads.insert(key, id)?.unwrap_or(usize::MAX);
        push_scratch(&mut self.previous, previous, governor)?;
        Ok(())
    }

    fn intern(
        &mut self,
        search: Search,
        states: &mut ScratchVec<Search>,
        width: usize,
        limits: &mut Limits,
        governor: &mut Governor,
    ) -> Result<usize> {
        let (key, cells) = Self::key(&search, governor)?;
        let id = states.len();
        governor.charge(Resource::Work, u64::from(id.max(1).ilog2() + 1) * 16)?;
        let mut candidate = self.heads.get(&key).copied().unwrap_or(usize::MAX);
        while candidate != usize::MAX {
            governor.charge(Resource::Work, saturating_size(cells + 1))?;
            if states[candidate] == search {
                return Ok(candidate);
            }
            candidate = self.previous[candidate];
        }
        limits.state(cells + width + 2)?;
        self.add(key, id, governor)?;
        push_scratch(states, search, governor)?;
        Ok(id)
    }
}

fn compile_machine(
    node: &Node,
    entry: Entry,
    alphabet: &Alphabet,
    limits: &mut Limits,
    governor: &mut Governor,
) -> Result<Machine> {
    let program = program(node, entry, alphabet, limits, governor)?;
    let suffixes = suffixes(&program.guards, alphabet.len(), limits, governor)?;
    let mut initial = Search {
        beginning: true,
        ..Search::empty(governor)
    };
    limits.state(
        program.guards.len() + suffixes.len() * (program.code.len() + 1) + alphabet.len() + 2,
    )?;
    governor.charge(Resource::Work, saturating_size(program.guards.len()))?;
    initial
        .prefix
        .extend(core::iter::repeat_n(0, program.guards.len()))?;
    for suffix in 0..suffixes.len() {
        let mut active = bits(program.code.len(), governor)?;
        active.as_mut_slice()[program.start] = true;
        push_scratch(&mut initial.trials, Trial { suffix, active }, governor)?;
    }
    let mut index = StateIndex::new(governor);
    let (key, _) = StateIndex::key(&initial, governor)?;
    index.add(key, 0, governor)?;
    let mut states = ScratchVec::new(governor);
    push_scratch(&mut states, initial, governor)?;
    let mut rows = ScratchVec::new(governor);
    while rows.len() < states.len() {
        let id = rows.len();
        // Only a trial whose suffix actually ended here can accept. Even
        // a match that finished before EOF can depend on its right context.
        let accepting = match states[id].trials.first() {
            Some(trial) if trial.suffix == 0 => {
                boundary_states(&program, &states[id], trial, &suffixes, None, governor)?[ACCEPT]
            }
            _ => false,
        };
        let mut scalar = ScratchVec::new(governor);
        for (symbol, &(first, _)) in alphabet.iter().enumerate() {
            let next = advance(&program, &states[id], &suffixes, symbol, first, governor)?;
            let target = index.intern(next, &mut states, alphabet.len(), limits, governor)?;
            push_scratch(&mut scalar, target, governor)?;
        }
        let marker = match entry {
            Entry::Search => id,
            Entry::Ahead | Entry::Behind => {
                let next = states[id].marker(governor)?;
                index.intern(next, &mut states, alphabet.len(), limits, governor)?
            }
        };
        push_scratch(
            &mut rows,
            Row {
                accepting,
                scalar,
                marker,
            },
            governor,
        )?;
    }
    Ok(Machine { rows })
}

/// A deterministic scalar automaton for a regular look-around pattern.
/// The initial state is row zero; all other rows use their index as state ID.
#[derive(Debug)]
pub struct LookaroundDfa {
    alphabet: Alphabet,
    machine: Machine,
}

impl LookaroundDfa {
    /// Visit rows in their state order and each row's inclusive scalar ranges.
    /// Each range names the destination row.
    #[must_use = "iterate the rows to inspect the automaton"]
    pub fn rows(
        &self,
    ) -> impl ExactSizeIterator<
        Item = (
            bool,
            impl ExactSizeIterator<Item = (char, char, usize)> + '_,
        ),
    > + Clone
    + '_ {
        self.machine.rows.iter().map(|row| {
            let edges = self
                .alphabet
                .iter()
                .zip(row.scalar.iter())
                .map(|(&(first, last), &target)| (first, last, target));
            (row.accepting, edges)
        })
    }
}

pub(super) fn compile(node: &Node, governor: &mut Governor) -> Result<Option<LookaroundDfa>> {
    if has_backref(node, governor)? {
        return Ok(None);
    }
    let mut limits = Limits::new();
    let alphabet = alphabet(node, &mut limits, governor)?;
    let machine = compile_machine(node, Entry::Search, &alphabet, &mut limits, governor)?;
    Ok(Some(LookaroundDfa { alphabet, machine }))
}
