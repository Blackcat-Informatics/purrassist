// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The primary engine: Antimirov/Brzozowski derivatives over Unicode
//! interval-set predicates, with minterm partitioning and a governed,
//! lazily-built state cache.
//!
//! A "state" is a single [`Term`] id in a shared arena: rather than
//! Brzozowski's original construction (one big term per state, prone to
//! unbounded growth from repeated `Concat`/`Alt` nesting), every state that
//! is a *set* of simultaneously-live derivations (Antimirov's formulation)
//! is folded into one canonical, ACI-simplified chain of binary [`Term::Or`]
//! nodes built by [`mk_or`]: flattened, sorted, and deduplicated, so
//! syntactically distinct constructions of the same live set always intern
//! to the same id. Bounded repetition (`Term::Repeat`) keeps its own body as
//! a single shared subterm id rather than unrolling it, so counted
//! repetition never grows the term itself — this, combined with
//! interning, is what keeps `(a+)+b` linear-time instead of exponential.
//!
//! Unanchored search (JSON Schema `pattern` semantics) is the classic
//! "rejoin the start state every position" subset construction: `^`/`$`/
//! `\b`/`\B` are resolved against a per-position [`Ctx`] (shared by every
//! live derivation at that position, since they depend only on absolute
//! text position) rather than folded permanently into the term.
use super::{ast::Node, is_word};
use crate::contract::{Governor, Resource, ScratchMap, ScratchVec, StopReason};
use std::collections::BTreeMap;

/// One arena node. Fixed-size (registered as `ScratchCharge` in
/// `contract.rs`) so interning never needs `size_of` on a recursive type:
/// children are referenced by `u32` arena index, never owned recursively.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for an item in a `pub(crate) mod` nested inside a public one
// (https://github.com/rust-lang/rust-clippy/issues/6188): `pub` trips
// `unreachable_pub` (nothing outside the crate can reach `pattern::derive`
// at all), `pub(crate)` trips `redundant_pub_crate`. `contract.rs`'s
// `scratch_charge!(crate::pattern::derive::Term, ...)` needs it visible
// outside this module, so `pub(crate)` (matching `unreachable_pub`) wins.
#[allow(clippy::redundant_pub_crate)]
pub(crate) enum Term {
    /// Matches the empty string (and nothing else).
    Empty,
    /// Matches nothing at all.
    Dead,
    /// Matches exactly one scalar value in `classes[.0]`.
    Class(u32),
    Concat(u32, u32),
    /// A canonicalized (sorted, deduplicated, flattened) binary alternation;
    /// built only through [`mk_or`], never directly.
    Or(u32, u32),
    /// `inner`, `min`, `max_code` (`0` = unbounded, else `max + 1`), `greedy`.
    Repeat(u32, u64, u64, bool),
    Start,
    End,
    /// `true` for `\b`, `false` for `\B`.
    Boundary(bool),
}
const DEAD_ID: u32 = 0;
const EMPTY_ID: u32 = 1;

/// The zero-width context at one absolute text position, shared by every
/// simultaneously-live derivation there (see the module doc).
#[derive(Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent zero-width assertion (Start/End/word-boundary side); a state machine would need 2^4 variants for no clarity gain"
)]
struct Ctx {
    at_start: bool,
    at_end: bool,
    prev_word: bool,
    next_word: bool,
}

/// A resizable, governed [`Term`] store shared by the initial (compile-time)
/// translation and every state discovered while matching or generating.
trait Arena {
    fn term(&self, id: u32) -> Term;
    fn intern(&mut self, term: Term) -> Result<u32, StopReason>;
}

/// Built once, without a caller governor, from the pattern's own (already
/// governed) parse; proportional to pattern size, exactly like the AST it
/// replaces.
#[derive(Clone, Debug)]
struct CompileArena {
    items: Vec<Term>,
    index: BTreeMap<Term, u32>,
}
impl CompileArena {
    fn new() -> Self {
        Self {
            items: vec![Term::Dead, Term::Empty],
            index: BTreeMap::new(),
        }
    }
}
impl Arena for CompileArena {
    fn term(&self, id: u32) -> Term {
        self.items[id as usize]
    }
    fn intern(&mut self, term: Term) -> Result<u32, StopReason> {
        if let Some(id) = self.index.get(&term) {
            return Ok(*id);
        }
        let id = u32::try_from(self.items.len()).unwrap_or(u32::MAX);
        self.items.push(term);
        self.index.insert(term, id);
        Ok(id)
    }
}

/// Every state and transition discovered while matching or generating one
/// call is charged against the caller's governor here; seeded from the
/// pattern's compiled base arena (a copy proportional to pattern size).
pub(super) struct MatchArena {
    items: ScratchVec<Term>,
    index: ScratchMap<Term, u32>,
}
impl MatchArena {
    fn seeded(base: &CompileArena, governor: &Governor) -> Result<Self, StopReason> {
        let mut items = ScratchVec::new(governor);
        items.extend(base.items.iter().copied())?;
        let mut index = ScratchMap::new(governor);
        for (term, id) in &base.index {
            index.insert(*term, *id)?;
        }
        Ok(Self { items, index })
    }
}
impl Arena for MatchArena {
    fn term(&self, id: u32) -> Term {
        self.items.as_slice()[id as usize]
    }
    fn intern(&mut self, term: Term) -> Result<u32, StopReason> {
        if let Some(id) = self.index.get(&term) {
            return Ok(*id);
        }
        let id = u32::try_from(self.items.len()).unwrap_or(u32::MAX);
        self.items.push(term)?;
        self.index.insert(term, id)?;
        Ok(id)
    }
}

fn mk_concat<A: Arena>(a: u32, b: u32, arena: &mut A) -> Result<u32, StopReason> {
    if a == EMPTY_ID {
        return Ok(b);
    }
    if b == EMPTY_ID {
        return Ok(a);
    }
    if a == DEAD_ID || b == DEAD_ID {
        return Ok(DEAD_ID);
    }
    arena.intern(Term::Concat(a, b))
}

fn or_leaves<A: Arena>(arena: &A, id: u32, out: &mut Vec<u32>) {
    match arena.term(id) {
        Term::Or(a, b) => {
            or_leaves(arena, a, out);
            or_leaves(arena, b, out);
        }
        Term::Dead => {}
        _ => out.push(id),
    }
}

/// Flatten, sort, and deduplicate `a | b` so any two constructions of the
/// same live set intern to the same id (the ACI simplification a bounded
/// derivative-based automaton needs).
fn mk_or<A: Arena>(a: u32, b: u32, arena: &mut A) -> Result<u32, StopReason> {
    if a == b {
        return Ok(a);
    }
    let mut leaves = Vec::new();
    or_leaves(arena, a, &mut leaves);
    or_leaves(arena, b, &mut leaves);
    leaves.sort_unstable();
    leaves.dedup();
    let Some(mut chain) = leaves.pop() else {
        return Ok(DEAD_ID);
    };
    while let Some(leaf) = leaves.pop() {
        chain = arena.intern(Term::Or(leaf, chain))?;
    }
    Ok(chain)
}

fn translate<A: Arena>(
    node: &Node,
    arena: &mut A,
    classes: &mut Vec<Vec<(u32, u32)>>,
    class_index: &mut BTreeMap<Vec<(u32, u32)>, u32>,
) -> u32 {
    match node {
        Node::Empty => EMPTY_ID,
        // Neither `Backref` nor `Look` ever reaches this engine:
        // `Pattern::compile_governed` routes any pattern containing one to
        // `backtrack.rs` instead, so translating one to `DEAD_ID` is dead
        // code paired with `NoMatch`'s only for a defensive, total match.
        Node::NoMatch | Node::Backref(_) | Node::Look { .. } => DEAD_ID,
        Node::Class(intervals) => {
            let key = intervals.as_ref().clone();
            let class_id = *class_index.entry(key.clone()).or_insert_with(|| {
                classes.push(key);
                u32::try_from(classes.len() - 1).unwrap_or(u32::MAX)
            });
            arena.intern(Term::Class(class_id)).unwrap_or(DEAD_ID)
        }
        Node::Concat(items) => {
            let mut id = EMPTY_ID;
            for item in items {
                let next = translate(item, arena, classes, class_index);
                id = mk_concat(id, next, arena).unwrap_or(DEAD_ID);
            }
            id
        }
        Node::Alternate(items) => {
            let mut id = DEAD_ID;
            for item in items {
                let next = translate(item, arena, classes, class_index);
                id = mk_or(id, next, arena).unwrap_or(DEAD_ID);
            }
            id
        }
        Node::Repeat(repetition) => {
            let inner = translate(&repetition.inner, arena, classes, class_index);
            let max_code = repetition.max.map_or(0, |max| {
                u64::try_from(max).unwrap_or(u64::MAX - 1).saturating_add(1)
            });
            let min = u64::try_from(repetition.min).unwrap_or(u64::MAX);
            arena
                .intern(Term::Repeat(inner, min, max_code, repetition.greedy))
                .unwrap_or(DEAD_ID)
        }
        Node::Group(_, inner) => translate(inner, arena, classes, class_index),
        Node::Start => arena.intern(Term::Start).unwrap_or(DEAD_ID),
        Node::End => arena.intern(Term::End).unwrap_or(DEAD_ID),
        Node::Boundary(positive) => arena.intern(Term::Boundary(*positive)).unwrap_or(DEAD_ID),
    }
}

/// The pattern's static structure, built once at compile time and reused,
/// read-only, by every `is_match`/`generate` call.
#[derive(Clone, Debug)]
pub(super) struct Compiled {
    base: CompileArena,
    root: u32,
    classes: Vec<Vec<(u32, u32)>>,
    cuts: Vec<u32>,
}

fn collect_cuts(node: &Node, cuts: &mut Vec<u32>, has_boundary: &mut bool) {
    match node {
        Node::Class(intervals) => {
            for &(start, end) in intervals.iter() {
                cuts.push(start);
                if let Some(next) = end.checked_add(1) {
                    cuts.push(next);
                }
            }
        }
        Node::Concat(items) | Node::Alternate(items) => {
            for item in items {
                collect_cuts(item, cuts, has_boundary);
            }
        }
        Node::Repeat(repetition) => collect_cuts(&repetition.inner, cuts, has_boundary),
        Node::Group(_, inner) => collect_cuts(inner, cuts, has_boundary),
        Node::Boundary(_) => *has_boundary = true,
        Node::Empty
        | Node::NoMatch
        | Node::Start
        | Node::End
        | Node::Backref(_)
        | Node::Look { .. } => {}
    }
}

/// Build the pattern's derivative automaton structure. Infallible: cost is
/// proportional to the already-parsed, already-governed AST, exactly like
/// the estimated-AST charge `Pattern::compile_governed` already makes.
pub(super) fn compile(node: &Node) -> Compiled {
    let mut base = CompileArena::new();
    let mut classes = Vec::new();
    let mut class_index = BTreeMap::new();
    let root = translate(node, &mut base, &mut classes, &mut class_index);
    let mut cuts = Vec::new();
    let mut has_boundary = false;
    collect_cuts(node, &mut cuts, &mut has_boundary);
    if has_boundary {
        // Cut points for the ASCII word class (`is_ascii_alphanumeric` or
        // `_`), so a minterm never straddles a word/non-word boundary.
        cuts.extend([0x30, 0x3A, 0x41, 0x5B, 0x5F, 0x60, 0x61, 0x7B]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    Compiled {
        base,
        root,
        classes,
        cuts,
    }
}

fn minterm_representative(cuts: &[u32], code: u32) -> (usize, u32) {
    let bucket = cuts.partition_point(|&cut| cut <= code);
    let representative = if bucket == 0 { 0 } else { cuts[bucket - 1] };
    (bucket, representative)
}

fn nullable(arena: &dyn Arena, classes: &[Vec<(u32, u32)>], id: u32, ctx: Ctx) -> bool {
    let _ = classes;
    match arena.term(id) {
        Term::Empty => true,
        Term::Dead | Term::Class(_) => false,
        Term::Concat(a, b) => nullable(arena, classes, a, ctx) && nullable(arena, classes, b, ctx),
        Term::Or(a, b) => nullable(arena, classes, a, ctx) || nullable(arena, classes, b, ctx),
        Term::Repeat(inner, min, _max_code, _) => min == 0 || nullable(arena, classes, inner, ctx),
        Term::Start => ctx.at_start,
        Term::End => ctx.at_end,
        Term::Boundary(want) => (ctx.prev_word != ctx.next_word) == want,
    }
}

fn step<A: Arena>(
    arena: &mut A,
    classes: &[Vec<(u32, u32)>],
    id: u32,
    code: u32,
    ctx: Ctx,
    governor: &mut Governor,
) -> Result<u32, StopReason> {
    governor.charge(Resource::Work, 1)?;
    match arena.term(id) {
        Term::Empty | Term::Dead | Term::Start | Term::End | Term::Boundary(_) => Ok(DEAD_ID),
        Term::Class(class_id) => Ok(
            if super::unicode::interval_contains(&classes[class_id as usize], code) {
                EMPTY_ID
            } else {
                DEAD_ID
            },
        ),
        Term::Concat(a, b) => {
            let via_a = step(arena, classes, a, code, ctx, governor)?;
            let left = mk_concat(via_a, b, arena)?;
            let right = if nullable(arena, classes, a, ctx) {
                step(arena, classes, b, code, ctx, governor)?
            } else {
                DEAD_ID
            };
            mk_or(left, right, arena)
        }
        Term::Or(a, b) => {
            let sa = step(arena, classes, a, code, ctx, governor)?;
            let sb = step(arena, classes, b, code, ctx, governor)?;
            mk_or(sa, sb, arena)
        }
        Term::Repeat(inner, min, max_code, greedy) => {
            if max_code == 1 {
                return Ok(DEAD_ID);
            }
            let consumed = step(arena, classes, inner, code, ctx, governor)?;
            if consumed == DEAD_ID {
                return Ok(DEAD_ID);
            }
            let next_min = min.saturating_sub(1);
            let next_max_code = if max_code == 0 { 0 } else { max_code - 1 };
            let tail = arena.intern(Term::Repeat(inner, next_min, next_max_code, greedy))?;
            mk_concat(consumed, tail, arena)
        }
    }
}

/// A cached transition: `(state, minterm, previous-character-is-a-word-char,
/// at-start) -> next state`. The start assertion must distinguish the first
/// position from later positions even when state and character repeat.
/// `prev_word` is only ever meaningfully `true` for a
/// pattern containing `\b`/`\B`; harmless (just an unused extra key bit)
/// otherwise.
type TransitionCache = ScratchMap<((u32, u32), (bool, bool)), u32>;

/// One requested transition: the state to step from, the character actually
/// consumed (`code`, the current minterm's representative), which minterm it
/// falls in (for the cache key; a cheaper proxy than `code` itself), and the
/// zero-width context at this position.
struct Transition {
    state: u32,
    code: u32,
    minterm: usize,
    ctx: Ctx,
}

fn cached_step(
    arena: &mut MatchArena,
    classes: &[Vec<(u32, u32)>],
    cache: &mut TransitionCache,
    transition: &Transition,
    governor: &mut Governor,
) -> Result<u32, StopReason> {
    let minterm = u32::try_from(transition.minterm).unwrap_or(u32::MAX);
    let key = (
        (transition.state, minterm),
        (transition.ctx.prev_word, transition.ctx.at_start),
    );
    if let Some(next) = cache.get(&key) {
        return Ok(*next);
    }
    let next = step(
        arena,
        classes,
        transition.state,
        transition.code,
        transition.ctx,
        governor,
    )?;
    cache.insert(key, next)?;
    Ok(next)
}

/// Unanchored search over `text`, per JSON Schema `pattern` semantics.
pub(super) fn is_match(
    compiled: &Compiled,
    text: &[char],
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    let mut arena = MatchArena::seeded(&compiled.base, governor)?;
    let mut cache: TransitionCache = ScratchMap::new(governor);
    let mut live = compiled.root;
    let len = text.len();
    let mut prev_word = false;
    for i in 0..=len {
        governor.charge(Resource::Work, 1)?;
        let next_char = text.get(i).copied();
        let ctx = Ctx {
            at_start: i == 0,
            at_end: next_char.is_none(),
            prev_word,
            next_word: next_char.is_some_and(is_word),
        };
        if nullable(&arena, &compiled.classes, live, ctx) {
            return Ok(true);
        }
        let Some(character) = next_char else { break };
        let code = u32::from(character);
        let (minterm, representative) = minterm_representative(&compiled.cuts, code);
        let stepped = cached_step(
            &mut arena,
            &compiled.classes,
            &mut cache,
            &Transition {
                state: live,
                code: representative,
                minterm,
                ctx,
            },
            governor,
        )?;
        live = mk_or(stepped, compiled.root, &mut arena)?;
        prev_word = is_word(character);
    }
    Ok(false)
}

/// Governed emptiness check for `compiled`'s *full* (start-to-end) language,
/// used by `generate.rs`'s BFS: `compiled` alone (no `avoid`) is empty iff
/// its root is never nullable under any reachable state, which the BFS
/// there already establishes by exhausting the frontier.
pub(super) const fn root(compiled: &Compiled) -> u32 {
    compiled.root
}
pub(super) fn classes(compiled: &Compiled) -> &[Vec<(u32, u32)>] {
    &compiled.classes
}
pub(super) fn cuts(compiled: &Compiled) -> &[u32] {
    &compiled.cuts
}

/// A single generation/matching session's arena and step cache, exposed to
/// `generate.rs`'s product-automaton BFS so it can drive `step`/`nullable`
/// directly without re-deriving this module's internals.
pub(super) struct Session {
    pub(super) arena: MatchArena,
    cache: TransitionCache,
}
impl Session {
    pub(super) fn new(compiled: &Compiled, governor: &Governor) -> Result<Self, StopReason> {
        Ok(Self {
            arena: MatchArena::seeded(&compiled.base, governor)?,
            cache: ScratchMap::new(governor),
        })
    }
    pub(super) fn nullable(&self, classes: &[Vec<(u32, u32)>], id: u32, ctx: NullCtx) -> bool {
        nullable(&self.arena, classes, id, ctx.into())
    }
    pub(super) fn step(
        &mut self,
        classes: &[Vec<(u32, u32)>],
        state: u32,
        code: u32,
        minterm: usize,
        ctx: NullCtx,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        cached_step(
            &mut self.arena,
            classes,
            &mut self.cache,
            &Transition {
                state,
                code,
                minterm,
                ctx: ctx.into(),
            },
            governor,
        )
    }
    pub(super) fn union(&mut self, a: u32, b: u32) -> Result<u32, StopReason> {
        mk_or(a, b, &mut self.arena)
    }
}

/// The public zero-width context, in terms `generate.rs` can construct
/// without reaching into this module's private [`Ctx`].
#[derive(Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent zero-width assertion (Start/End/word-boundary side); a state machine would need 2^4 variants for no clarity gain"
)]
pub(super) struct NullCtx {
    pub(super) at_start: bool,
    pub(super) at_end: bool,
    pub(super) prev_word: bool,
    pub(super) next_word: bool,
}
impl From<NullCtx> for Ctx {
    fn from(value: NullCtx) -> Self {
        Self {
            at_start: value.at_start,
            at_end: value.at_end,
            prev_word: value.prev_word,
            next_word: value.next_word,
        }
    }
}
pub(super) fn minterm_of(cuts: &[u32], code: u32) -> (usize, u32) {
    minterm_representative(cuts, code)
}
