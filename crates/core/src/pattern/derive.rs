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
    /// Ask "can this term still become nullable in some future context?"
    /// instead of "is it nullable here?" (see [`DerivativeDfa::is_dead`]).
    viable: bool,
}

/// A resizable, governed [`Term`] store shared by the initial (compile-time)
/// translation and every state discovered while matching or generating.
trait Arena {
    fn term(&self, id: u32) -> Term;
    fn intern(&mut self, term: Term, governor: &mut Governor) -> Result<u32, StopReason>;
    fn metered(&self) -> bool;
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
impl MatchArena {
    fn intern_mode(
        &mut self,
        term: Term,
        governor: &mut Governor,
        metered: bool,
    ) -> Result<u32, StopReason> {
        if metered {
            map_work(self.index.len(), false, governor)?;
        }
        if let Some(id) = self.index.get(&term) {
            return Ok(*id);
        }
        let id = u32::try_from(self.items.len())
            .map_err(|_| StopReason::BudgetExceeded(Resource::ScratchBytes))?;
        if metered {
            map_work(self.index.len(), true, governor)?;
        }
        self.items.push(term)?;
        self.index.insert(term, id)?;
        Ok(id)
    }
}
impl Arena for MatchArena {
    fn term(&self, id: u32) -> Term {
        self.items.as_slice()[id as usize]
    }
    fn intern(&mut self, term: Term, governor: &mut Governor) -> Result<u32, StopReason> {
        self.intern_mode(term, governor, false)
    }
    fn metered(&self) -> bool {
        false
    }
}

/// A temporary accounting policy, never stored in retained/session layouts.
struct MeteredArena<'a>(&'a mut MatchArena);
impl Arena for MeteredArena<'_> {
    fn term(&self, id: u32) -> Term {
        self.0.term(id)
    }
    fn intern(&mut self, term: Term, governor: &mut Governor) -> Result<u32, StopReason> {
        self.0.intern_mode(term, governor, true)
    }
    fn metered(&self) -> bool {
        true
    }
}

fn mk_concat<A: Arena>(
    a: u32,
    b: u32,
    arena: &mut A,
    governor: &mut Governor,
) -> Result<u32, StopReason> {
    if a == EMPTY_ID {
        return Ok(b);
    }
    if b == EMPTY_ID {
        return Ok(a);
    }
    if a == DEAD_ID || b == DEAD_ID {
        return Ok(DEAD_ID);
    }
    arena.intern(Term::Concat(a, b), governor)
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
            arena.intern(Term::Or(head, chain), governor)?
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
            return u32::try_from(index)
                .map_err(|_| StopReason::BudgetExceeded(Resource::ScratchBytes));
        }
    }
    governor.charge(
        Resource::Work,
        u64::try_from(intervals.len()).unwrap_or(u64::MAX),
    )?;
    let index = u32::try_from(classes.len())
        .map_err(|_| StopReason::BudgetExceeded(Resource::ScratchBytes))?;
    let mut copied = ScratchVec::new(governor);
    copied.extend(intervals.iter().copied())?;
    classes.push(Buffer::from_scratch(copied, governor))?;
    Ok(index)
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
            arena.intern(Term::Class(class_id), governor)?
        }
        Node::Concat(items) => {
            let mut id = EMPTY_ID;
            for item in items {
                let next = translate(item, arena, classes, governor)?;
                id = mk_concat(id, next, arena, governor)?;
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
            arena.intern(
                Term::Repeat(inner, min, max_code, repetition.greedy),
                governor,
            )?
        }
        Node::Group(_, inner) => translate(inner, arena, classes, governor)?,
        Node::Start => arena.intern(Term::Start, governor)?,
        Node::End => arena.intern(Term::End, governor)?,
        Node::Boundary(positive) => arena.intern(Term::Boundary(*positive), governor)?,
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
    retain_compiled(base, classes, root, false, governor)
}

fn retain_compiled(
    base: MatchArena,
    classes: ScratchVec<Buffer<(u32, u32)>>,
    root: u32,
    byte_program: bool,
    governor: &mut Governor,
) -> Result<Compiled, StopReason> {
    let mut cuts = ScratchVec::new(governor);
    collect_cuts(classes.as_slice(), &mut cuts, governor)?;
    governor.charge(Resource::Work, 2)?;
    // Direct byte programs cannot construct assertion terms. Unicode programs
    // retain their established lookup and Work schedule.
    let has_boundary = !byte_program
        && (base.index.get(&Term::Boundary(true)).is_some()
            || base.index.get(&Term::Boundary(false)).is_some());
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

/// Direct postfix construction shares every term/class/storage authority with
/// Unicode compilation, without constructing a textual regex or syntax AST.
pub(super) fn compile_bytes(
    instructions: &[super::ByteInstruction],
    governor: &mut Governor,
) -> Result<Compiled, super::ByteCompileError> {
    use super::{ByteCompileError, ByteInstruction};
    governor.charge(
        Resource::InputBytes,
        crate::contract::saturating_size(instructions.len()).saturating_mul(40),
    )?;
    let mut base = MatchArena::new(governor)?;
    let mut arena = MeteredArena(&mut base);
    let mut classes = ScratchVec::new(governor);
    let mut stack: ScratchVec<(u32, u32)> = ScratchVec::new(governor);
    for (index, instruction) in instructions.iter().enumerate() {
        governor.charge(Resource::Work, 1)?;
        let (root, depth) = match *instruction {
            ByteInstruction::Empty => (EMPTY_ID, 0),
            ByteInstruction::Dead => (DEAD_ID, 0),
            ByteInstruction::Literal(byte) => {
                let class = intern_class(
                    &[(u32::from(byte), u32::from(byte))],
                    &mut classes,
                    governor,
                )?;
                (arena.intern(Term::Class(class), governor)?, 0)
            }
            ByteInstruction::Class(bits) => {
                let class = byte_class(bits, &mut classes, governor)?;
                (arena.intern(Term::Class(class), governor)?, 0)
            }
            ByteInstruction::Star => {
                let Some((inner, depth)) = stack.pop() else {
                    return Err(ByteCompileError::Arity {
                        instruction: index,
                        required: 1,
                        available: 0,
                    });
                };
                let depth = depth
                    .checked_add(1)
                    .ok_or(StopReason::BudgetExceeded(Resource::Depth))?;
                governor.charge(Resource::Depth, u64::from(depth))?;
                (
                    arena.intern(Term::Repeat(inner, 0, 0, true), governor)?,
                    depth,
                )
            }
            ByteInstruction::Concat(count) | ByteInstruction::Alternate(count) => {
                if count == 0 || stack.len() < count {
                    return Err(ByteCompileError::Arity {
                        instruction: index,
                        required: count,
                        available: stack.len(),
                    });
                }
                governor.charge(
                    Resource::Work,
                    crate::contract::saturating_size(count).saturating_mul(3),
                )?;
                let start = stack.len() - count;
                let mut depth = 0;
                for &(_, child_depth) in &stack.as_slice()[start..] {
                    depth = depth.max(child_depth);
                }
                let depth = depth
                    .checked_add(1)
                    .ok_or(StopReason::BudgetExceeded(Resource::Depth))?;
                governor.charge(Resource::Depth, u64::from(depth))?;
                let alternate = matches!(instruction, ByteInstruction::Alternate(_));
                let mut root = if alternate { DEAD_ID } else { EMPTY_ID };
                for &(child, _) in &stack.as_slice()[start..] {
                    root = if alternate {
                        mk_or(root, child, &mut arena, governor)?
                    } else {
                        mk_concat(root, child, &mut arena, governor)?
                    };
                }
                for _ in 0..count {
                    stack.pop();
                }
                (root, depth)
            }
        };
        governor.charge(Resource::Depth, u64::from(depth))?;
        stack.push((root, depth))?;
    }
    if stack.len() != 1 {
        return Err(ByteCompileError::Roots {
            remaining: stack.len(),
        });
    }
    let root = stack.as_slice()[0].0;
    governor.check()?;
    Ok(retain_compiled(base, classes, root, true, governor)?)
}

fn byte_class(
    bits: [u64; 4],
    classes: &mut ScratchVec<Buffer<(u32, u32)>>,
    governor: &mut Governor,
) -> Result<u32, StopReason> {
    let mut ranges = ScratchVec::new(governor);
    let mut first = None;
    for code in 0..=256_u32 {
        governor.charge(Resource::Work, 1)?;
        let present = code < 256 && bits[(code / 64) as usize] & (1 << (code % 64)) != 0;
        match (first, present) {
            (None, true) => first = Some(code),
            (Some(start), false) => {
                ranges.push((start, code - 1))?;
                first = None;
            }
            _ => {}
        }
    }
    intern_class(ranges.as_slice(), classes, governor)
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
        | (u32::from(ctx.next_word) << 3)
        | (u32::from(ctx.viable) << 4);
    let key = (id, flags);
    if arena.metered() {
        map_work(cache.len(), false, governor)?;
    }
    if let Some(&value) = cache.get(&key) {
        return Ok(value);
    }
    let value = nullable(arena, id, ctx, governor)?;
    if arena.metered() {
        map_work(cache.len(), true, governor)?;
    }
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
    if ctx.viable {
        // Viability over-approximates every future context: any class may
        // still consume a character, and `$`/`\b`/`\B` may still hold at
        // some later position. Only `^` is position-bound.
        return match term {
            Term::Empty
            | Term::Repeat(_, 0, _, _)
            | Term::Class(_)
            | Term::End
            | Term::Boundary(_) => Some(true),
            Term::Dead => Some(false),
            Term::Start => Some(ctx.at_start),
            Term::Concat(_, _) | Term::Or(_, _) | Term::Repeat(_, _, _, _) => None,
        };
    }
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
    let metered = arena.metered();
    let mut stack = Stack::new(governor);
    let mut next = Some(id);
    let mut value = false;
    loop {
        if let Some(id) = next.take() {
            governor.charge(Resource::Work, 1)?;
            if arena.metered() {
                map_work(memo.len(), false, governor)?;
            }
            if let Some(&cached) = memo.get(&id) {
                value = cached;
            } else {
                let term = arena.term(id);
                if let Some(scalar) = immediate_nullable(arena, term, ctx, governor)? {
                    value = scalar;
                } else {
                    match term {
                        Term::Concat(a, b) => {
                            push_frame(&mut stack, NullFrame::And(id, b), metered, governor)?;
                            next = Some(a);
                            continue;
                        }
                        Term::Or(a, b) => {
                            push_frame(&mut stack, NullFrame::Or(id, b), metered, governor)?;
                            next = Some(a);
                            continue;
                        }
                        Term::Repeat(inner, min, _, _) => {
                            debug_assert!(min > 0);
                            push_frame(&mut stack, NullFrame::Finish(id), metered, governor)?;
                            next = Some(inner);
                            continue;
                        }
                        _ => unreachable!("every remaining term is composite"),
                    }
                }
            }
        }
        let Some(frame) = pop_frame(&mut stack, metered, governor)? else {
            return Ok(value);
        };
        governor.charge(Resource::Work, 1)?;
        match frame {
            NullFrame::And(parent, right) if value => {
                push_frame(&mut stack, NullFrame::Finish(parent), metered, governor)?;
                next = Some(right);
            }
            NullFrame::Or(parent, right) if !value => {
                push_frame(&mut stack, NullFrame::Finish(parent), metered, governor)?;
                next = Some(right);
            }
            NullFrame::And(parent, _) | NullFrame::Or(parent, _) | NullFrame::Finish(parent) => {
                if arena.metered() {
                    map_work(memo.len(), true, governor)?;
                }
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
    let metered = arena.metered();
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
                    if arena.metered() {
                        map_work(classes[class_id as usize].len(), false, governor)?;
                    }
                    derived =
                        if super::unicode::interval_contains(&classes[class_id as usize], code) {
                            EMPTY_ID
                        } else {
                            DEAD_ID
                        };
                }
                Term::Concat(a, b) => {
                    push_frame(&mut stack, StepFrame::Concat(a, b), metered, governor)?;
                    next = Some(a);
                    continue;
                }
                Term::Or(a, b) => {
                    push_frame(&mut stack, StepFrame::Alternate(b), metered, governor)?;
                    next = Some(a);
                    continue;
                }
                Term::Repeat(inner, min, max_code, greedy) if max_code != 1 => {
                    push_frame(
                        &mut stack,
                        StepFrame::Repeat(inner, min, max_code, greedy),
                        metered,
                        governor,
                    )?;
                    next = Some(inner);
                    continue;
                }
                Term::Repeat(_, _, _, _) => derived = DEAD_ID,
            }
        }
        let Some(frame) = pop_frame(&mut stack, metered, governor)? else {
            return Ok(derived);
        };
        governor.charge(Resource::Work, 1)?;
        match frame {
            StepFrame::Concat(left, right) => {
                let consumed_left = mk_concat(derived, right, arena, governor)?;
                if nullable_cached(arena, left, ctx, &mut nullability, governor)? {
                    push_frame(
                        &mut stack,
                        StepFrame::Merge(consumed_left),
                        metered,
                        governor,
                    )?;
                    next = Some(right);
                } else {
                    derived = mk_or(consumed_left, DEAD_ID, arena, governor)?;
                }
            }
            StepFrame::Alternate(right) => {
                push_frame(&mut stack, StepFrame::Merge(derived), metered, governor)?;
                next = Some(right);
            }
            StepFrame::Merge(left) => derived = mk_or(left, derived, arena, governor)?,
            StepFrame::Repeat(inner, min, max_code, greedy) => {
                if derived != DEAD_ID {
                    let next_min = min.saturating_sub(1);
                    let next_max = if max_code == 0 { 0 } else { max_code - 1 };
                    let tail =
                        arena.intern(Term::Repeat(inner, next_min, next_max, greedy), governor)?;
                    derived = mk_concat(derived, tail, arena, governor)?;
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

fn cached_step<A: Arena>(
    arena: &mut A,
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
    if arena.metered() {
        map_work(cache.len(), false, governor)?;
    }
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
    if arena.metered() {
        map_work(cache.len(), true, governor)?;
    }
    cache.insert(key, next)?;
    Ok(next)
}

/// Unanchored search over `text`, per JSON Schema `pattern` semantics.
///
/// This is the only derivative matching loop: it drives [`DerivativeDfa`]
/// byte by byte over `text`'s UTF-8 encoding and returns as soon as a match
/// is established, exactly like a caller stepping the automaton itself.
pub(super) fn is_match(
    compiled: &Compiled,
    text: &str,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    let mut dfa = DerivativeDfa::new(compiled, governor)?;
    let mut state = dfa.start();
    for &byte in text.as_bytes() {
        state = dfa.step(state, byte, governor)?;
        if state.matched {
            return Ok(true);
        }
    }
    dfa.accepting(state, governor)
}

/// The UTF-8 decoding position of a [`DfaState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Utf8 {
    /// At a character boundary.
    Boundary,
    /// Inside a character: `bits` are the payload bits decoded so far,
    /// `remaining` continuation bytes are still expected and `length` is the
    /// full encoded length. The prefix denotes the contiguous code-point
    /// interval `[bits << 6r, (bits << 6r) | (2^6r - 1)]` for `r = remaining`
    /// (UTF-8 preserves code-point order).
    Partial {
        bits: u32,
        remaining: u8,
        length: u8,
    },
    /// The bytes are not well-formed UTF-8; no continuation is accepted.
    Invalid,
}

/// One explicit state of a [`DerivativeDfa`].
///
/// It records the live derivative term, the
/// zero-width context of the current position, whether a match has already
/// been found (search semantics make that sticky), and the UTF-8 decoding
/// position, so a state between two bytes of one character is an ordinary
/// value that can be stored and resumed later.
///
/// ```
/// use purrassist_core::{Budget, Governor, pattern::Pattern};
/// let pattern = Pattern::compile("^é$").unwrap();
/// let mut governor = Governor::new(Budget::default());
/// let mut dfa = pattern.byte_dfa(&mut governor)?.expect("derivative pattern");
/// let start = dfa.start();
/// let middle = dfa.step(start, 0xC3, &mut governor)?; // first byte of 'é'
/// assert!(!dfa.accepting(middle, &mut governor)?);
/// let saved = middle; // states are plain `Copy` values
/// let end = dfa.step(saved, 0xA9, &mut governor)?;
/// assert!(dfa.accepting(end, &mut governor)?);
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DfaState {
    live: u32,
    matched: bool,
    at_start: bool,
    prev_word: bool,
    utf8: Utf8,
}

/// A resumable, byte-level automaton for one derivative-engine pattern,
/// with JSON Schema `pattern` (unanchored search) semantics.
///
/// The automaton consumes UTF-8 bytes. Character classes act on whole code
/// points, so a partially decoded character is an explicit [`DfaState`]
/// whose transitions narrow the code-point interval its prefix denotes; the
/// derivative is taken once the character completes, through the same
/// cached, governed transition [`Pattern::generate`](super::Pattern::generate)
/// uses. States, transitions and nullability results are discovered lazily
/// and admitted against the governor passed to each call; the caches live as
/// long as the automaton and release their scratch charge when it drops.
///
/// [`Pattern::is_match`](super::Pattern::is_match) is implemented by stepping
/// this automaton over the whole text, so the two always agree.
///
/// ```
/// use purrassist_core::{Budget, Governor, pattern::Pattern};
/// let pattern = Pattern::compile(r"\bcat\b").unwrap();
/// let mut governor = Governor::new(Budget::default());
/// let mut dfa = pattern.byte_dfa(&mut governor)?.expect("derivative pattern");
/// let mut state = dfa.start();
/// for chunk in [&b"a ca"[..], b"t!"] {
///     for &byte in chunk {
///         state = dfa.step(state, byte, &mut governor)?;
///     }
/// }
/// assert!(dfa.accepting(state, &mut governor)?);
/// assert!(!dfa.is_dead(state, &mut governor)?);
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
pub struct DerivativeDfa<'p> {
    compiled: &'p Compiled,
    session: Session,
}

impl core::fmt::Debug for DerivativeDfa<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("DerivativeDfa")
            .field("root", &self.compiled.root)
            .field("terms", &self.session.arena.items.len())
            .finish_non_exhaustive()
    }
}

impl<'p> DerivativeDfa<'p> {
    pub(super) fn new(compiled: &'p Compiled, governor: &mut Governor) -> Result<Self, StopReason> {
        Ok(Self {
            compiled,
            session: Session::new(compiled, governor)?,
        })
    }

    /// The state before any byte has been consumed.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::Pattern};
    /// let mut governor = Governor::new(Budget::default());
    /// let pattern = Pattern::compile("^$").unwrap();
    /// let mut dfa = pattern.byte_dfa(&mut governor)?.expect("derivative pattern");
    /// let start = dfa.start();
    /// assert!(dfa.accepting(start, &mut governor)?); // the empty text matches
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub const fn start(&self) -> DfaState {
        DfaState {
            live: self.compiled.root,
            matched: false,
            at_start: true,
            prev_word: false,
            utf8: Utf8::Boundary,
        }
    }

    /// Consume one UTF-8 byte.
    ///
    /// Charges one `Work` unit per byte: a character's lead byte charges all
    /// of the character's bytes (as its length announces), so continuation
    /// bytes make no governor call. At a character boundary the match
    /// test for the current position is evaluated (once a match is found it
    /// is sticky and later bytes only track UTF-8 well-formedness); when a
    /// character completes, its derivative is computed through the lazy,
    /// governed transition cache, which may intern new terms (`ScratchBytes`)
    /// and traverse terms (`Work`). Malformed UTF-8 (a stray continuation
    /// byte, an overlong form, a surrogate, a value above U+10FFFF, or a
    /// missing continuation byte) moves to a dead, never-accepting state.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` or `Cancelled` from the governor; the input
    /// `state` remains valid and can be stepped again under a fresh budget.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::Pattern};
    /// let mut governor = Governor::new(Budget::default());
    /// let pattern = Pattern::compile("^a").unwrap();
    /// let mut dfa = pattern.byte_dfa(&mut governor)?.expect("derivative pattern");
    /// let state = dfa.step(dfa.start(), b'a', &mut governor)?;
    /// assert!(dfa.accepting(state, &mut governor)?);
    /// let invalid = dfa.step(dfa.start(), 0xFF, &mut governor)?;
    /// assert!(dfa.is_dead(invalid, &mut governor)?);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[inline]
    pub fn step(
        &mut self,
        state: DfaState,
        byte: u8,
        governor: &mut Governor,
    ) -> Result<DfaState, StopReason> {
        // Fast path: a continuation byte that does not complete its
        // character only narrows the pending code-point interval.
        if let Utf8::Partial {
            bits,
            remaining: remaining @ 2..,
            length,
        } = state.utf8
        {
            let mut next = state;
            next.utf8 = match continue_character(bits, remaining, length, byte) {
                Some((bits, remaining)) => Utf8::Partial {
                    bits,
                    remaining,
                    length,
                },
                None => Utf8::Invalid,
            };
            return Ok(next);
        }
        self.step_slow(state, byte, governor)
    }

    /// [`Self::step`] at a character boundary, on a character's final
    /// byte, or in the invalid state.
    #[inline]
    fn step_slow(
        &mut self,
        state: DfaState,
        byte: u8,
        governor: &mut Governor,
    ) -> Result<DfaState, StopReason> {
        let mut next = state;
        let (bits, remaining, length) = match state.utf8 {
            Utf8::Invalid => {
                governor.charge(Resource::Work, 1)?;
                return Ok(state);
            }
            Utf8::Boundary => {
                let (bits, length) = match byte {
                    0x00..=0x7F => (u32::from(byte), 1),
                    0xC2..=0xDF => (u32::from(byte & 0x1F), 2),
                    0xE0..=0xEF => (u32::from(byte & 0x0F), 3),
                    0xF0..=0xF4 => (u32::from(byte & 0x07), 4),
                    _ => {
                        governor.charge(Resource::Work, 1)?;
                        return Ok(next.invalid());
                    }
                };
                // The lead byte charges the whole character's bytes, so a
                // continuation byte needs no governor call of its own.
                governor.charge(Resource::Work, u64::from(length))?;
                if !state.matched {
                    // A multi-byte lead byte never starts an ASCII word
                    // character, so the next character's word status is
                    // known from its first byte.
                    let ctx = state.context(false, length == 1 && is_word(char::from(byte)));
                    next.matched = cached_nullable(
                        &self.session.arena,
                        state.live,
                        ctx,
                        &mut self.session.nullability,
                        governor,
                    )?;
                }
                (bits, length - 1, length)
            }
            Utf8::Partial {
                bits,
                remaining,
                length,
            } => {
                let Some((bits, remaining)) = continue_character(bits, remaining, length, byte)
                else {
                    return Ok(next.invalid());
                };
                (bits, remaining, length)
            }
        };
        if remaining > 0 {
            next.utf8 = Utf8::Partial {
                bits,
                remaining,
                length,
            };
            return Ok(next);
        }
        next.utf8 = Utf8::Boundary;
        let Some(character) = char::from_u32(bits) else {
            return Ok(next.invalid());
        };
        if !next.matched {
            let (minterm, code) = minterm_representative(&self.compiled.cuts, bits);
            let ctx = state.context(false, is_word(character));
            let stepped = cached_step(
                &mut self.session.arena,
                &self.compiled.classes,
                &mut self.session.cache,
                &Transition {
                    state: state.live,
                    code,
                    minterm,
                    ctx,
                },
                governor,
            )?;
            next.live = mk_or(
                stepped,
                self.compiled.root,
                &mut self.session.arena,
                governor,
            )?;
        }
        next.at_start = false;
        next.prev_word = is_word(character);
        Ok(next)
    }

    /// Whether the bytes consumed so far form complete UTF-8 text that
    /// contains a match (evaluating `$` and `\b` against the end of text).
    /// A state inside a character, or after malformed UTF-8, never accepts.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` or `Cancelled` from the governor while
    /// evaluating (and caching) the end-of-text nullability check.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::Pattern};
    /// let mut governor = Governor::new(Budget::default());
    /// let pattern = Pattern::compile("b$").unwrap();
    /// let mut dfa = pattern.byte_dfa(&mut governor)?.expect("derivative pattern");
    /// let state = dfa.step(dfa.start(), b'b', &mut governor)?;
    /// assert!(dfa.accepting(state, &mut governor)?);
    /// let state = dfa.step(state, b'c', &mut governor)?;
    /// assert!(!dfa.accepting(state, &mut governor)?);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    pub fn accepting(
        &mut self,
        state: DfaState,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        if state.utf8 != Utf8::Boundary {
            return Ok(false);
        }
        if state.matched {
            return Ok(true);
        }
        let ctx = state.context(true, false);
        cached_nullable(
            &self.session.arena,
            state.live,
            ctx,
            &mut self.session.nullability,
            governor,
        )
    }

    /// Whether no continuation of the consumed bytes can ever be accepted.
    ///
    /// Sound but deliberately incomplete: `true` is a proof that every
    /// extension is rejected; `false` means only that this cheap structural
    /// test could not rule acceptance out. Malformed UTF-8 is dead and a
    /// found match never is. Otherwise the state is dead when neither its
    /// live term nor the pattern root (re-entered at every later position,
    /// where `^` can no longer hold) is *viable*. Viability is nullability in
    /// a context that over-approximates every future one: classes, `$`,
    /// `\b` and `\B` count as satisfiable and only `^` depends on the
    /// position. A term that is not viable is not nullable, and each of its
    /// derivatives is again not viable (`Concat`/`Or`/`Repeat` derivatives
    /// are built from derivatives of non-viable parts or nullable non-viable
    /// left operands, which do not exist), so no future position can match.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` or `Cancelled` from the governor while
    /// evaluating (and caching) the viability traversals.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::Pattern};
    /// let mut governor = Governor::new(Budget::default());
    /// let pattern = Pattern::compile("^[a-z]+$").unwrap();
    /// let mut dfa = pattern.byte_dfa(&mut governor)?.expect("derivative pattern");
    /// let letters = dfa.step(dfa.start(), b'q', &mut governor)?;
    /// assert!(!dfa.is_dead(letters, &mut governor)?);
    /// let digit = dfa.step(letters, b'7', &mut governor)?;
    /// assert!(dfa.is_dead(digit, &mut governor)?);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    pub fn is_dead(
        &mut self,
        state: DfaState,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        if state.utf8 == Utf8::Invalid {
            return Ok(true);
        }
        if state.matched {
            return Ok(false);
        }
        for (term, at_start) in [(state.live, state.at_start), (self.compiled.root, false)] {
            let ctx = Ctx {
                at_start,
                at_end: false,
                prev_word: false,
                next_word: false,
                viable: true,
            };
            if cached_nullable(
                &self.session.arena,
                term,
                ctx,
                &mut self.session.nullability,
                governor,
            )? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl DfaState {
    const fn context(self, at_end: bool, next_word: bool) -> Ctx {
        Ctx {
            at_start: self.at_start,
            at_end,
            prev_word: self.prev_word,
            next_word,
            viable: false,
        }
    }

    const fn invalid(mut self) -> Self {
        self.utf8 = Utf8::Invalid;
        self
    }

    /// The number of bytes of a partially decoded character this state has
    /// consumed: zero at a character boundary and after invalid UTF-8.
    pub(crate) const fn partial_bytes(self) -> usize {
        match self.utf8 {
            Utf8::Partial {
                remaining, length, ..
            } => (length - remaining) as usize,
            Utf8::Boundary | Utf8::Invalid => 0,
        }
    }

    /// The state at the start of the partially decoded character this state
    /// is inside, `None` at a character boundary or after invalid UTF-8.
    ///
    /// Inside a character only the decoding state and the sticky match flag
    /// differ from the state before its lead byte, and the lead byte sets
    /// that flag only when the text before the character contains a match
    /// with `$` false (and the next character not a word character): terms
    /// have no negation, so that match also holds at the end of text. The
    /// result is therefore [`DerivativeDfa::accepting`] exactly when the
    /// bytes before the character's lead byte are.
    pub(crate) const fn character_start(self) -> Option<Self> {
        match self.utf8 {
            Utf8::Partial { .. } => Some(Self {
                utf8: Utf8::Boundary,
                ..self
            }),
            Utf8::Boundary | Utf8::Invalid => None,
        }
    }
}

/// After the second byte of a multi-byte character, the decoded prefix
/// interval lies either wholly inside or wholly outside the scalar values
/// encodable at that length (the boundaries are aligned to the interval
/// width), so this one check rejects overlong forms, surrogates, and values
/// above U+10FFFF before the character completes.
/// Add one continuation byte to a partial character with `remaining`
/// continuation bytes expected; `None` when the byte is not a continuation
/// byte or the prefix can no longer encode a scalar value (checked once the
/// first continuation byte fixes the interval).
#[inline]
const fn continue_character(bits: u32, remaining: u8, length: u8, byte: u8) -> Option<(u32, u8)> {
    if byte & 0xC0 != 0x80 {
        return None;
    }
    let bits = (bits << 6) | (byte & 0x3F) as u32;
    let remaining = remaining - 1;
    if remaining + 2 == length && !valid_prefix(bits, remaining, length) {
        return None;
    }
    Some((bits, remaining))
}

const fn valid_prefix(bits: u32, remaining: u8, length: u8) -> bool {
    let shift = 6 * remaining as u32;
    let low = bits << shift;
    let high = low | ((1 << shift) - 1);
    let minimum = match length {
        2 => 0x80,
        3 => 0x800,
        _ => 0x1_0000,
    };
    low >= minimum && high <= 0x10_FFFF && !(low >= 0xD800 && high <= 0xDFFF)
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
    pub(super) fn new_bytes(
        compiled: &Compiled,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        // The base index is already sorted: each insertion appends, but its
        // binary lookup still compares keys. Unicode sessions retain their
        // established Work law; byte sessions opt into these additional costs.
        let count = compiled.base.index.len();
        governor.charge(
            Resource::Work,
            crate::contract::saturating_size(count)
                .saturating_mul(u64::from(count.checked_ilog2().unwrap_or(0) + 2))
                .saturating_mul(4),
        )?;
        Self::new(compiled, governor)
    }
    pub(super) fn new(compiled: &Compiled, governor: &mut Governor) -> Result<Self, StopReason> {
        Ok(Self {
            arena: MatchArena::seeded(&compiled.base, governor)?,
            cache: ScratchMap::new(governor),
            nullability: NullableCache::new(governor),
        })
    }
    pub(super) fn nullable_bytes(
        &mut self,
        id: u32,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        cached_nullable(
            &MeteredArena(&mut self.arena),
            id,
            NullCtx {
                at_start: false,
                at_end: false,
                prev_word: false,
                next_word: false,
            }
            .into(),
            &mut self.nullability,
            governor,
        )
    }
    pub(super) fn step_bytes(
        &mut self,
        classes: &[Vec<(u32, u32)>],
        state: u32,
        code: u32,
        minterm: usize,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        cached_step(
            &mut MeteredArena(&mut self.arena),
            classes,
            &mut self.cache,
            &Transition {
                state,
                code,
                minterm,
                ctx: NullCtx {
                    at_start: false,
                    at_end: false,
                    prev_word: false,
                    next_word: false,
                }
                .into(),
            },
            governor,
        )
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
            viable: false,
        }
    }
}

pub(super) fn minterm_of(cuts: &[u32], code: u32) -> (usize, u32) {
    minterm_representative(cuts, code)
}

/// Before-work bound for fixed-size map-key comparisons and shifted entries.
/// Byte programs opt into this accounting without changing Unicode schedules.
pub(super) fn map_work(
    len: usize,
    insertion: bool,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let comparisons = u64::from(len.checked_ilog2().unwrap_or(0) + 2).saturating_mul(4);
    governor.charge(
        Resource::Work,
        comparisons.saturating_add(if insertion {
            crate::contract::saturating_size(len)
        } else {
            0
        }),
    )
}

fn push_frame<T: crate::contract::ScratchCharge>(
    stack: &mut Stack<T>,
    value: T,
    metered: bool,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    // Stack keys are increasing: insertion appends without shifting entries.
    if metered {
        map_work(stack.len(), false, governor)?;
    }
    stack.push(value)
}
fn pop_frame<T: crate::contract::ScratchCharge>(
    stack: &mut Stack<T>,
    metered: bool,
    governor: &mut Governor,
) -> Result<Option<T>, StopReason> {
    // Removal always targets the largest key and shifts no entries.
    if metered {
        map_work(stack.len(), false, governor)?;
    }
    Ok(stack.pop())
}
pub(super) const fn dead(state: u32) -> bool {
    state == DEAD_ID
}

#[cfg(test)]
mod tests {
    use super::{Arena, Compiled, DEAD_ID, MatchArena, Session, Term};
    use crate::contract::{Budget, CancellationToken, Governor, Resource, StopReason};

    fn interleaved_chains(width: usize) -> (Compiled, u32, u32) {
        // Construct valid canonical input chains once without repeatedly
        // merging growing prefixes; the tested union receives caller-owned
        // compiled input just as a generation session does.
        let mut governor = Governor::new(Budget::default());
        let mut base = MatchArena::new(&governor).unwrap();
        let leaves: Vec<_> = (0..width * 2)
            .map(|index| {
                base.intern(Term::Class(u32::try_from(index).unwrap()), &mut governor)
                    .unwrap()
            })
            .collect();
        let mut chain = |parity| {
            let mut tail = DEAD_ID;
            for &leaf in leaves.iter().skip(parity).step_by(2).rev() {
                tail = if tail == DEAD_ID {
                    leaf
                } else {
                    base.intern(Term::Or(leaf, tail), &mut governor).unwrap()
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

    /// The pre-byte-engine matcher, kept verbatim (over `char`s) as the
    /// reference oracle for the byte-level automaton.
    fn char_reference_is_match(compiled: &Compiled, text: &str) -> bool {
        use super::{Ctx, cached_nullable, cached_step, is_word, minterm_representative, mk_or};
        let governor = &mut Governor::new(Budget::default());
        let mut session = Session::new(compiled, governor).unwrap();
        let text: Vec<char> = text.chars().collect();
        let mut live = compiled.root;
        let mut prev_word = false;
        for i in 0..=text.len() {
            let next_char = text.get(i).copied();
            let ctx = Ctx {
                at_start: i == 0,
                at_end: next_char.is_none(),
                prev_word,
                next_word: next_char.is_some_and(is_word),
                viable: false,
            };
            if cached_nullable(
                &session.arena,
                live,
                ctx,
                &mut session.nullability,
                governor,
            )
            .unwrap()
            {
                return true;
            }
            let Some(character) = next_char else { break };
            let (minterm, code) = minterm_representative(&compiled.cuts, u32::from(character));
            let stepped = cached_step(
                &mut session.arena,
                &compiled.classes,
                &mut session.cache,
                &super::Transition {
                    state: live,
                    code,
                    minterm,
                    ctx,
                },
                governor,
            )
            .unwrap();
            live = mk_or(stepped, compiled.root, &mut session.arena, governor).unwrap();
            prev_word = is_word(character);
        }
        false
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn byte_step_equals_char_match() {
        use crate::pattern::Pattern;
        let patterns = [
            "^[a-z]+[0-9]{2}$",
            "foo|bar",
            r"^\d+$",
            r"^\s$",
            ".",
            "[^]",
            "^(a?)*$",
            "^a$",
            r"\ba\b",
            r"\B_\B",
            r"\bé",
            "(a+)+b",
            "(a+)+$",
            "[a-z]+[0-9]*",
            r"^\p{Script=Greek}+$",
            r"^\P{L}+$",
            "^é$",
            "猫+",
            "^😀{2}$",
            "[α-ω]{2,3}",
            "^.$",
            "$",
            "^",
            "^$",
            r"[^\s\S]",
            "abc",
            "^[^a]",
            r"\u{1F600}|é",
        ];
        let texts = [
            "",
            "a",
            "ab12",
            "ab1",
            "xxbarxx",
            "\u{ff11}\u{ff12}",
            "12",
            "\u{a0}",
            "\u{2028}",
            "\n",
            "a\n",
            "a a",
            "_a",
            "a9",
            "éa猫",
            "a_9",
            "é_猫",
            "aaaaab",
            "aaaa!",
            "Ελλάδα",
            "Hello",
            "é",
            "eé",
            "猫猫",
            "😀😀",
            "😀",
            "αβ",
            "x😀y",
            "abc",
            "ÿa",
        ];
        let governor = &mut Governor::new(Budget::default());
        for source in patterns {
            let pattern = Pattern::compile(source).unwrap();
            let compiled = pattern.compiled.as_ref().unwrap();
            let mut dfa = pattern.byte_dfa(governor).unwrap().unwrap();
            for text in texts {
                let expected = char_reference_is_match(compiled, text);
                assert_eq!(
                    pattern.is_match(text, governor),
                    Ok(expected),
                    "{source:?} {text:?}"
                );
                assert_eq!(
                    pattern.is_match_backtracking(text, governor),
                    Ok(expected),
                    "backtracking {source:?} {text:?}"
                );
                // Step every byte, saving the state at every split point
                // (including between the bytes of one character), perturbing
                // the shared caches with an unrelated text, then resuming.
                let bytes = text.as_bytes();
                let mut state = dfa.start();
                for (index, &byte) in bytes.iter().enumerate() {
                    let saved = state;
                    let mut other = dfa.start();
                    for &noise in "zé9 😀".as_bytes() {
                        other = dfa.step(other, noise, governor).unwrap();
                    }
                    assert_eq!(saved, state);
                    state = dfa.step(saved, byte, governor).unwrap();
                    let boundary = text.is_char_boundary(index + 1);
                    if !boundary {
                        assert!(!dfa.accepting(state, governor).unwrap());
                    }
                    let prefix_accepts =
                        boundary && char_reference_is_match(compiled, &text[..=index]);
                    if boundary {
                        assert_eq!(
                            dfa.accepting(state, governor).unwrap(),
                            prefix_accepts,
                            "{source:?} prefix of {text:?}"
                        );
                    }
                    if dfa.is_dead(state, governor).unwrap() {
                        // Soundness: no tested continuation of a dead state accepts.
                        for suffix in texts {
                            let mut continued = state;
                            for &byte in suffix.as_bytes() {
                                continued = dfa.step(continued, byte, governor).unwrap();
                            }
                            assert!(
                                !dfa.accepting(continued, governor).unwrap(),
                                "{source:?} {text:?} dead at {index} but accepts {suffix:?}"
                            );
                            let mut combined = bytes[..=index].to_vec();
                            combined.extend_from_slice(suffix.as_bytes());
                            if let Ok(combined) = core::str::from_utf8(&combined) {
                                assert!(!char_reference_is_match(compiled, combined));
                            }
                        }
                    }
                }
                assert_eq!(
                    dfa.accepting(state, governor),
                    Ok(expected),
                    "stepped {source:?} {text:?}"
                );
            }
            // Malformed UTF-8 is dead and never accepts, including a lone
            // continuation byte, an overlong form, a surrogate and a
            // truncated sequence.
            for malformed in [
                &[0x80_u8][..],
                &[0xC0, 0xAF],
                &[0xED, 0xA0, 0x80],
                &[0xF4, 0x90],
            ] {
                let mut state = dfa.start();
                for &byte in malformed {
                    state = dfa.step(state, byte, governor).unwrap();
                }
                assert!(dfa.is_dead(state, governor).unwrap(), "{malformed:?}");
                assert!(!dfa.accepting(state, governor).unwrap());
            }
            let truncated = dfa.step(dfa.start(), 0xE7, governor).unwrap();
            assert!(!dfa.accepting(truncated, governor).unwrap());
            let broken = dfa.step(truncated, b'a', governor).unwrap();
            assert!(dfa.is_dead(broken, governor).unwrap());
        }
        // `^`-anchored patterns become dead once the anchor can never hold.
        let anchored = Pattern::compile("^[a-z]+$").unwrap();
        let mut dfa = anchored.byte_dfa(governor).unwrap().unwrap();
        let mut state = dfa.start();
        for &byte in b"ab3" {
            state = dfa.step(state, byte, governor).unwrap();
        }
        assert!(dfa.is_dead(state, governor).unwrap());
        let open = Pattern::compile("[a-z]$").unwrap();
        let mut dfa = open.byte_dfa(governor).unwrap().unwrap();
        let state = dfa.step(dfa.start(), b'3', governor).unwrap();
        assert!(!dfa.is_dead(state, governor).unwrap());
    }
}
