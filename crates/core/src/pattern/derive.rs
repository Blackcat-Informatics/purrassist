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
use super::storage::{Buffer, Stack};
use super::{ast::Node, is_word};
use crate::contract::{Governor, Resource, ScratchMap, ScratchVec, StopReason};

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

/// Caller-retained, already-admitted compile arena. Its sorted index uses
/// the same pairs as the temporary `ScratchMap`, so ownership transfer never
/// copies or allocates and no source-length estimate bounds term growth.
#[derive(Clone, Debug)]
struct CompileArena {
    items: Vec<Term>,
    index: Vec<(Term, u32)>,
}

/// Every state and transition discovered while matching or generating one
/// call is charged against the caller's governor here; seeded from the
/// pattern's compiled base arena (a copy proportional to pattern size).
pub(super) struct MatchArena {
    items: ScratchVec<Term>,
    index: ScratchMap<Term, u32>,
}
impl MatchArena {
    fn new(governor: &Governor) -> Result<Self, StopReason> {
        let mut items = ScratchVec::new(governor);
        items.extend([Term::Dead, Term::Empty])?;
        Ok(Self {
            items,
            index: ScratchMap::new(governor),
        })
    }

    fn into_retained(self) -> CompileArena {
        CompileArena {
            items: self.items.into_vec(),
            index: self.index.into_pairs(),
        }
    }

    fn seeded(base: &CompileArena, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(
            Resource::Work,
            u64::try_from(base.items.len().saturating_add(base.index.len())).unwrap_or(u64::MAX),
        )?;
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

/// Iteratively merge two canonical, sorted chains. Every visited head and
/// reconstructed term consumes work; temporary heads are admitted before
/// allocation and remain owned until reconstruction finishes. A shared suffix
/// can be reused directly. No input width consumes the Rust call stack.
/// Only this function creates `Or` terms: their left child is the lowest
/// leaf id and their right child is the remaining sorted, distinct chain.
fn mk_or<A: Arena>(
    mut a: u32,
    mut b: u32,
    arena: &mut A,
    governor: &mut Governor,
) -> Result<u32, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let mut heads = ScratchVec::new(governor);
    while a != b && a != DEAD_ID && b != DEAD_ID {
        governor.charge(Resource::Work, 2)?;
        let (a_head, a_tail) = match arena.term(a) {
            Term::Or(head, tail) => (head, tail),
            _ => (a, DEAD_ID),
        };
        let (b_head, b_tail) = match arena.term(b) {
            Term::Or(head, tail) => (head, tail),
            _ => (b, DEAD_ID),
        };
        match a_head.cmp(&b_head) {
            std::cmp::Ordering::Less => {
                heads.push(a_head)?;
                a = a_tail;
            }
            std::cmp::Ordering::Equal => {
                heads.push(a_head)?;
                a = a_tail;
                b = b_tail;
            }
            std::cmp::Ordering::Greater => {
                heads.push(b_head)?;
                b = b_tail;
            }
        }
    }
    let mut chain = if a == DEAD_ID { b } else { a };
    for &head in heads.as_slice().iter().rev() {
        governor.charge(Resource::Work, 1)?;
        chain = if chain == DEAD_ID {
            head
        } else {
            arena.intern(Term::Or(head, chain))?
        };
    }
    Ok(chain)
}

fn intern_class(
    intervals: &[(u32, u32)],
    classes: &mut ScratchVec<Buffer<(u32, u32)>>,
    governor: &mut Governor,
) -> Result<u32, StopReason> {
    for (index, class) in classes.as_slice().iter().enumerate() {
        governor.charge(
            Resource::Work,
            u64::try_from(class.values.len().min(intervals.len()).saturating_add(1))
                .unwrap_or(u64::MAX),
        )?;
        if class.values == intervals {
            return Ok(u32::try_from(index).unwrap_or(u32::MAX));
        }
    }
    governor.charge(
        Resource::Work,
        u64::try_from(intervals.len()).unwrap_or(u64::MAX),
    )?;
    let mut copied = ScratchVec::new(governor);
    copied.extend(intervals.iter().copied())?;
    classes.push(Buffer::from_scratch(copied, governor))?;
    Ok(u32::try_from(classes.len() - 1).unwrap_or(u32::MAX))
}

fn translate<A: Arena>(
    node: &Node,
    arena: &mut A,
    classes: &mut ScratchVec<Buffer<(u32, u32)>>,
    governor: &mut Governor,
) -> Result<u32, StopReason> {
    governor.charge(Resource::Work, 1)?;
    Ok(match node {
        Node::Empty => EMPTY_ID,
        // Neither `Backref` nor `Look` ever reaches this engine:
        // `Pattern::compile_governed` routes any pattern containing one to
        // `backtrack.rs` instead, so translating one to `DEAD_ID` is dead
        // code paired with `NoMatch`'s only for a defensive, total match.
        Node::NoMatch | Node::Backref(_) | Node::Look { .. } => DEAD_ID,
        Node::Class(intervals) => {
            let class_id = intern_class(intervals, classes, governor)?;
            arena.intern(Term::Class(class_id))?
        }
        Node::Concat(items) => {
            let mut id = EMPTY_ID;
            for item in items {
                let next = translate(item, arena, classes, governor)?;
                id = mk_concat(id, next, arena)?;
            }
            id
        }
        Node::Alternate(items) => {
            let mut id = DEAD_ID;
            for item in items {
                let next = translate(item, arena, classes, governor)?;
                id = mk_or(id, next, arena, governor)?;
            }
            id
        }
        Node::Repeat(repetition) => {
            let inner = translate(&repetition.inner, arena, classes, governor)?;
            let max_code = repetition.max.map_or(0, |max| {
                u64::try_from(max).unwrap_or(u64::MAX - 1).saturating_add(1)
            });
            let min = u64::try_from(repetition.min).unwrap_or(u64::MAX);
            arena.intern(Term::Repeat(inner, min, max_code, repetition.greedy))?
        }
        Node::Group(_, inner) => translate(inner, arena, classes, governor)?,
        Node::Start => arena.intern(Term::Start)?,
        Node::End => arena.intern(Term::End)?,
        Node::Boundary(positive) => arena.intern(Term::Boundary(*positive))?,
    })
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

fn collect_cuts(
    classes: &[Buffer<(u32, u32)>],
    cuts: &mut ScratchVec<u32>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    // Each unique predicate contributes once. Repeated identical AST atoms
    // need neither duplicate cut storage nor sorting proportional to width.
    for class in classes {
        for &(start, end) in &class.values {
            governor.charge(Resource::Work, 2)?;
            cuts.push(start)?;
            if let Some(next) = end.checked_add(1) {
                cuts.push(next)?;
            }
        }
    }
    Ok(())
}

impl crate::contract::ScratchCharge for Vec<(u32, u32)> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 24);
        24
    };
}

/// Build the pattern's derivative automaton structure. Translation and
/// canonical union share the caller's work/cancellation budget; any failure
/// propagates rather than substituting an empty-language term.
pub(super) fn compile(node: &Node, governor: &mut Governor) -> Result<Compiled, StopReason> {
    let mut base = MatchArena::new(governor)?;
    let mut classes = ScratchVec::new(governor);
    let root = translate(node, &mut base, &mut classes, governor)?;
    let mut cuts = ScratchVec::new(governor);
    collect_cuts(classes.as_slice(), &mut cuts, governor)?;
    governor.charge(Resource::Work, 2)?;
    let has_boundary = base.index.get(&Term::Boundary(true)).is_some()
        || base.index.get(&Term::Boundary(false)).is_some();
    if has_boundary {
        // Cut points for the ASCII word class (`is_ascii_alphanumeric` or
        // `_`), so a minterm never straddles a word/non-word boundary.
        cuts.extend([0x30, 0x3A, 0x41, 0x5B, 0x5F, 0x60, 0x61, 0x7B])?;
    }
    let mut cuts = Buffer::from_scratch(cuts, governor);
    governor.charge(
        Resource::Work,
        u64::try_from(cuts.values.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::from(
                cuts.values.len().checked_ilog2().unwrap_or(0) + 1,
            )),
    )?;
    cuts.values.sort_unstable();
    cuts.values.dedup();
    // Reserve the final outer vector before retaining any inner storage.
    // After the last check ownership transfer has no allocation or failure.
    let mut retained_classes = ScratchVec::new(governor);
    retained_classes.reserve(classes.len())?;
    governor.charge(
        Resource::Work,
        u64::try_from(classes.len()).unwrap_or(u64::MAX),
    )?;
    let mut retained_classes = Buffer::from_scratch(retained_classes, governor);
    for class in Buffer::from_scratch(classes, governor) {
        retained_classes.values.push(class.into_retained());
    }
    Ok(Compiled {
        base: base.into_retained(),
        root,
        classes: retained_classes.into_retained(),
        cuts: cuts.into_retained(),
    })
}

fn minterm_representative(cuts: &[u32], code: u32) -> (usize, u32) {
    let bucket = cuts.partition_point(|&cut| cut <= code);
    let representative = if bucket == 0 { 0 } else { cuts[bucket - 1] };
    (bucket, representative)
}

/// Short-circuit continuation for an iterative nullable traversal. The
/// memo records each completed term under this one immutable position context.
enum NullFrame {
    And(u32, u32),
    Or(u32, u32),
    Finish(u32),
}

/// A term's nullable value depends on all four assertion flags. Retaining
/// this per-session cache avoids traversing the same context at every text
/// position; both entries and misses use admitted scratch owners.
type NullableCache = ScratchMap<(u32, u32), bool>;

fn cached_nullable(
    arena: &dyn Arena,
    id: u32,
    ctx: Ctx,
    cache: &mut NullableCache,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let flags = u32::from(ctx.at_start)
        | (u32::from(ctx.at_end) << 1)
        | (u32::from(ctx.prev_word) << 2)
        | (u32::from(ctx.next_word) << 3);
    let key = (id, flags);
    if let Some(&value) = cache.get(&key) {
        return Ok(value);
    }
    let value = nullable(arena, id, ctx, governor)?;
    cache.insert(key, value)?;
    governor.check()?;
    Ok(value)
}
impl crate::contract::ScratchCharge for NullFrame {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 12);
        12
    };
}

fn nullable(
    arena: &dyn Arena,
    id: u32,
    ctx: Ctx,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 1)?;
    if let Some(value) = immediate_nullable(arena, arena.term(id), ctx, governor)? {
        return Ok(value);
    }
    let mut memo = ScratchMap::new(governor);
    nullable_cached(arena, id, ctx, &mut memo, governor)
}

/// A scalar/zero-width term needs no traversal storage. Composite terms use
/// the admitted continuation/memo path; both paths charge before inspecting.
const fn scalar_nullable(term: Term, ctx: Ctx) -> Option<bool> {
    match term {
        Term::Empty | Term::Repeat(_, 0, _, _) => Some(true),
        Term::Dead | Term::Class(_) => Some(false),
        Term::Start => Some(ctx.at_start),
        Term::End => Some(ctx.at_end),
        Term::Boundary(want) => Some((ctx.prev_word != ctx.next_word) == want),
        Term::Concat(_, _) | Term::Or(_, _) | Term::Repeat(_, _, _, _) => None,
    }
}

/// Inspect at most the immediate children before constructing traversal
/// storage. Nullability is a pure Boolean predicate: a false Concat child or
/// true Or child decides its parent in either order. This avoids allocating
/// frames/memo entries at every position of an end-anchored repetition, while
/// composites that require traversal still use the admitted iterative path.
fn immediate_nullable(
    arena: &dyn Arena,
    term: Term,
    ctx: Ctx,
    governor: &mut Governor,
) -> Result<Option<bool>, StopReason> {
    if let Some(value) = scalar_nullable(term, ctx) {
        return Ok(Some(value));
    }
    match term {
        Term::Concat(left, right) | Term::Or(left, right) => {
            let is_or = matches!(term, Term::Or(_, _));
            governor.charge(Resource::Work, 1)?;
            let left = scalar_nullable(arena.term(left), ctx);
            if left == Some(is_or) {
                return Ok(left);
            }
            governor.charge(Resource::Work, 1)?;
            let right = scalar_nullable(arena.term(right), ctx);
            if right == Some(is_or) {
                return Ok(right);
            }
            Ok(left.and(right))
        }
        Term::Repeat(inner, _, _, _) => {
            governor.charge(Resource::Work, 1)?;
            Ok(scalar_nullable(arena.term(inner), ctx))
        }
        _ => unreachable!("every remaining term is composite"),
    }
}

fn nullable_cached(
    arena: &dyn Arena,
    id: u32,
    ctx: Ctx,
    memo: &mut ScratchMap<u32, bool>,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    let mut stack = Stack::new(governor);
    let mut next = Some(id);
    let mut value = false;
    loop {
        if let Some(id) = next.take() {
            governor.charge(Resource::Work, 1)?;
            if let Some(&cached) = memo.get(&id) {
                value = cached;
            } else {
                let term = arena.term(id);
                if let Some(scalar) = immediate_nullable(arena, term, ctx, governor)? {
                    value = scalar;
                } else {
                    match term {
                        Term::Concat(a, b) => {
                            stack.push(NullFrame::And(id, b))?;
                            next = Some(a);
                            continue;
                        }
                        Term::Or(a, b) => {
                            stack.push(NullFrame::Or(id, b))?;
                            next = Some(a);
                            continue;
                        }
                        Term::Repeat(inner, min, _, _) => {
                            debug_assert!(min > 0);
                            stack.push(NullFrame::Finish(id))?;
                            next = Some(inner);
                            continue;
                        }
                        _ => unreachable!("every remaining term is composite"),
                    }
                }
            }
        }
        let Some(frame) = stack.pop() else {
            return Ok(value);
        };
        governor.charge(Resource::Work, 1)?;
        match frame {
            NullFrame::And(parent, right) if value => {
                stack.push(NullFrame::Finish(parent))?;
                next = Some(right);
            }
            NullFrame::Or(parent, right) if !value => {
                stack.push(NullFrame::Finish(parent))?;
                next = Some(right);
            }
            NullFrame::And(parent, _) | NullFrame::Or(parent, _) | NullFrame::Finish(parent) => {
                memo.insert(parent, value)?;
            }
        }
    }
}

/// Derivative continuations retain only arena ids and bounded repetition
/// metadata. Flat Concat/Or chains use admitted heap frames, never Rust frames.
enum StepFrame {
    Concat(u32, u32),
    Alternate(u32),
    Merge(u32),
    Repeat(u32, u64, u64, bool),
}
impl crate::contract::ScratchCharge for StepFrame {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 32);
        32
    };
}

fn step<A: Arena>(
    arena: &mut A,
    classes: &[Vec<(u32, u32)>],
    id: u32,
    code: u32,
    ctx: Ctx,
    governor: &mut Governor,
) -> Result<u32, StopReason> {
    let mut stack = Stack::new(governor);
    let mut nullability = ScratchMap::new(governor);
    let mut next = Some(id);
    let mut derived = DEAD_ID;
    loop {
        if let Some(id) = next.take() {
            governor.charge(Resource::Work, 1)?;
            match arena.term(id) {
                Term::Empty | Term::Dead | Term::Start | Term::End | Term::Boundary(_) => {
                    derived = DEAD_ID;
                }
                Term::Class(class_id) => {
                    derived =
                        if super::unicode::interval_contains(&classes[class_id as usize], code) {
                            EMPTY_ID
                        } else {
                            DEAD_ID
                        };
                }
                Term::Concat(a, b) => {
                    stack.push(StepFrame::Concat(a, b))?;
                    next = Some(a);
                    continue;
                }
                Term::Or(a, b) => {
                    stack.push(StepFrame::Alternate(b))?;
                    next = Some(a);
                    continue;
                }
                Term::Repeat(inner, min, max_code, greedy) if max_code != 1 => {
                    stack.push(StepFrame::Repeat(inner, min, max_code, greedy))?;
                    next = Some(inner);
                    continue;
                }
                Term::Repeat(_, _, _, _) => derived = DEAD_ID,
            }
        }
        let Some(frame) = stack.pop() else {
            return Ok(derived);
        };
        governor.charge(Resource::Work, 1)?;
        match frame {
            StepFrame::Concat(left, right) => {
                let consumed_left = mk_concat(derived, right, arena)?;
                if nullable_cached(arena, left, ctx, &mut nullability, governor)? {
                    stack.push(StepFrame::Merge(consumed_left))?;
                    next = Some(right);
                } else {
                    derived = mk_or(consumed_left, DEAD_ID, arena, governor)?;
                }
            }
            StepFrame::Alternate(right) => {
                stack.push(StepFrame::Merge(derived))?;
                next = Some(right);
            }
            StepFrame::Merge(left) => derived = mk_or(left, derived, arena, governor)?,
            StepFrame::Repeat(inner, min, max_code, greedy) => {
                if derived != DEAD_ID {
                    let next_min = min.saturating_sub(1);
                    let next_max = if max_code == 0 { 0 } else { max_code - 1 };
                    let tail = arena.intern(Term::Repeat(inner, next_min, next_max, greedy))?;
                    derived = mk_concat(derived, tail, arena)?;
                }
            }
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
    governor.charge(Resource::Work, 1)?;
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
    let mut nullability = NullableCache::new(governor);
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
        if cached_nullable(&arena, live, ctx, &mut nullability, governor)? {
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
        live = mk_or(stepped, compiled.root, &mut arena, governor)?;
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
    nullability: NullableCache,
}
impl Session {
    pub(super) fn new(compiled: &Compiled, governor: &mut Governor) -> Result<Self, StopReason> {
        Ok(Self {
            arena: MatchArena::seeded(&compiled.base, governor)?,
            cache: ScratchMap::new(governor),
            nullability: NullableCache::new(governor),
        })
    }
    pub(super) fn nullable(
        &mut self,
        id: u32,
        ctx: NullCtx,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        cached_nullable(&self.arena, id, ctx.into(), &mut self.nullability, governor)
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
    pub(super) fn union(
        &mut self,
        a: u32,
        b: u32,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        mk_or(a, b, &mut self.arena, governor)
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

#[cfg(test)]
mod tests {
    use super::{Arena, Compiled, DEAD_ID, MatchArena, Session, Term};
    use crate::contract::{Budget, CancellationToken, Governor, Resource, StopReason};

    fn interleaved_chains(width: usize) -> (Compiled, u32, u32) {
        // Construct valid canonical input chains once without repeatedly
        // merging growing prefixes; the tested union receives caller-owned
        // compiled input just as a generation session does.
        let governor = Governor::new(Budget::default());
        let mut base = MatchArena::new(&governor).unwrap();
        let leaves: Vec<_> = (0..width * 2)
            .map(|index| {
                base.intern(Term::Class(u32::try_from(index).unwrap()))
                    .unwrap()
            })
            .collect();
        let mut chain = |parity| {
            let mut tail = DEAD_ID;
            for &leaf in leaves.iter().skip(parity).step_by(2).rev() {
                tail = if tail == DEAD_ID {
                    leaf
                } else {
                    base.intern(Term::Or(leaf, tail)).unwrap()
                };
            }
            tail
        };
        let a = chain(0);
        let b = chain(1);
        (
            Compiled {
                base: base.into_retained(),
                root: a,
                classes: Vec::new(),
                cuts: Vec::new(),
            },
            a,
            b,
        )
    }

    #[test]
    fn wide_canonical_union_is_iterative_and_intern_hits_still_consume_work() {
        let width = 8192;
        let (compiled, a, b) = interleaved_chains(width);
        let mut governor = Governor::new(Budget {
            depth: 0,
            ..Budget::default()
        });
        {
            let mut session = Session::new(&compiled, &mut governor).unwrap();
            let before = governor.usage().work;
            let merged = session.union(a, b, &mut governor).unwrap();
            let work = governor.usage().work - before;
            assert!(work >= u64::try_from(width * 2).unwrap());
            let mut tail = merged;
            let mut expected = 0;
            while let Term::Or(head, rest) = session.arena.term(tail) {
                assert_eq!(session.arena.term(head), Term::Class(expected));
                expected += 1;
                tail = rest;
            }
            assert_eq!(session.arena.term(tail), Term::Class(expected));
            assert_eq!(usize::try_from(expected).unwrap() + 1, width * 2);
            let items = session.arena.items.len();
            let before = governor.usage().work;
            assert_eq!(session.union(a, b, &mut governor), Ok(merged));
            assert_eq!(governor.usage().work - before, work);
            assert_eq!(session.arena.items.len(), items);
            let remaining = governor.limits().work - governor.usage().work;
            governor.charge(Resource::Work, remaining - 1001).unwrap();
            let live = governor.scratch_meter().live();
            assert_eq!(
                session.union(a, b, &mut governor),
                Err(StopReason::BudgetExceeded(Resource::Work))
            );
            assert_eq!(governor.usage().work, governor.limits().work);
            assert_eq!(
                governor.scratch_meter().live(),
                live,
                "partial head storage must be released"
            );
            assert_eq!(session.arena.items.len(), items);
        }
        assert_eq!(governor.scratch_meter().live(), 0);
    }

    #[test]
    fn cancelled_interned_union_does_not_return_a_cached_success() {
        let (compiled, a, b) = interleaved_chains(1024);
        let token = CancellationToken::new();
        let mut governor = Governor::with_cancellation(Budget::default(), token.clone());
        {
            let mut session = Session::new(&compiled, &mut governor).unwrap();
            session.union(a, b, &mut governor).unwrap();
            let live = governor.scratch_meter().live();
            token.cancel();
            assert_eq!(
                session.union(a, b, &mut governor),
                Err(StopReason::Cancelled)
            );
            assert_eq!(governor.scratch_meter().live(), live);
        }
        assert_eq!(governor.scratch_meter().live(), 0);
    }

    #[test]
    fn nullable_cache_preserves_each_assertion_flag_and_polls_cache_hits() {
        for source in ["^", "$", r"\b", r"\B"] {
            let pattern = super::super::Pattern::compile(source).unwrap();
            let compiled = pattern.compiled.as_ref().unwrap();
            let mut governor = Governor::new(Budget::default());
            {
                let mut session = Session::new(compiled, &mut governor).unwrap();
                for flags in 0..16 {
                    let ctx = super::NullCtx {
                        at_start: flags & 1 != 0,
                        at_end: flags & 2 != 0,
                        prev_word: flags & 4 != 0,
                        next_word: flags & 8 != 0,
                    };
                    let expected = match source {
                        "^" => ctx.at_start,
                        "$" => ctx.at_end,
                        r"\b" => ctx.prev_word != ctx.next_word,
                        _ => ctx.prev_word == ctx.next_word,
                    };
                    assert_eq!(
                        session.nullable(compiled.root, ctx, &mut governor),
                        Ok(expected)
                    );
                    let before = governor.usage().work;
                    assert_eq!(
                        session.nullable(compiled.root, ctx, &mut governor),
                        Ok(expected)
                    );
                    assert_eq!(governor.usage().work - before, 1);
                    if flags == 15 {
                        governor.cancel();
                        assert_eq!(
                            session.nullable(compiled.root, ctx, &mut governor),
                            Err(StopReason::Cancelled)
                        );
                    }
                }
            }
            assert_eq!(governor.scratch_meter().live(), 0);
        }
    }
}
