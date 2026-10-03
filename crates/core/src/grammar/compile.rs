// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Resolution, checking, and table construction for parsed grammars.
//!
//! Names are resolved against one namespace of productions and closer
//! classes; captures, codecs and classes are checked; recursion is refused
//! (the framing layer is regular, so context-free structure belongs in a
//! codec); and the `call` and `result` productions are compiled, with every
//! reference inlined, into programs of terminal instructions. Every table
//! entry charges [`K_COMPILE`] `Work` and its retained `ScratchBytes`.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); these items are
// shared across the grammar and tool-call modules, so `pub(crate)` (matching
// `unreachable_pub`) wins, as in `pattern::derive`.
#![allow(clippy::redundant_pub_crate)]
use super::automaton::{Matcher, Trie};
use super::codec::CodecKind;
use super::cost::K_COMPILE;
use super::syntax::{Document, PExpr, Statement};
use super::{
    CallEnd, CaptureKind, ClassId, ClassTerminal, CloserClass, CloserId, CompiledGrammar,
    DelimiterSet, DelimiterSetId, Expr, GrammarError, Literal, LiteralId, Production, ProductionId,
    index_u32,
};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, StopReason};
use crate::digest::Digest;
use crate::pattern::{Pattern, PatternCompileError};

/// One instruction of a compiled production program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Inst {
    /// Match a literal (canonical spelling; repairs use its tables).
    Lit { lit: u32, next: u32 },
    /// Match a maximal class run.
    Class { class: u32, next: u32 },
    /// Skip a verbatim body up to a delimiter.
    Until { set: u32, next: u32 },
    /// Decode a slot body up to a delimiter.
    Slot {
        codec: CodecKind,
        set: u32,
        next: u32,
    },
    /// Continue at both targets.
    Split(u32, u32),
    /// Open a capture.
    Open { kind: CaptureKind, next: u32 },
    /// Close the innermost capture.
    Close { kind: CaptureKind, next: u32 },
    /// The attempt is committed: from here on it is a call (or result)
    /// attempt, reported when it is rejected.
    Commit { next: u32 },
    /// The next call of the attempt begins here.
    Next { next: u32 },
    /// Read an omitted token as inserted here.
    Insert { lit: u32, next: u32 },
    /// The production matched.
    Accept,
}

impl ScratchCharge for Inst {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 16);
        16
    };
}

/// The recognizer program of one production.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Program {
    pub(crate) insts: Vec<Inst>,
    /// The program's entry location.
    pub(crate) start: u32,
    /// The literal instructions reachable from the start without input.
    pub(crate) openers: Vec<u32>,
    /// Bytes that can begin an opener (any spelling), as a 256-bit set.
    pub(crate) first: [u64; 4],
    /// The configuration bound (see `CompiledGrammar::configuration_bound`).
    pub(crate) bound: u64,
    /// For a program with `commit` instructions, which locations only a
    /// committed attempt reaches (every path from the start to them passes
    /// a `commit`); empty for a program without one, whose attempts open
    /// at their opener literal.
    pub(crate) committed: Vec<bool>,
    /// Whether a zero-width instruction other than a split (an `insert`, a
    /// capture, a commit, a `next`) lies between the start and an opener:
    /// a new attempt then follows the start's closure instead of starting
    /// at the openers, so their effects are recorded.
    pub(crate) start_closure: bool,
    /// Whether the program separates calls with `next`: an attempt's
    /// completions are then held until it ends, and one that a later
    /// completion of the same attempt reads further at no more cost is
    /// dropped (the attempt reads every call it can).
    pub(crate) grouped: bool,
}

impl Program {
    /// Whether the program's attempts open at a `commit` rather than at
    /// their opener literal.
    #[inline]
    pub(crate) const fn deferred(&self) -> bool {
        !self.committed.is_empty()
    }

    /// Whether a configuration at `pc` belongs to a committed attempt.
    #[inline]
    pub(crate) fn commits(&self, pc: u32) -> bool {
        self.committed.get(pc as usize).copied().unwrap_or(false)
    }

    #[inline]
    pub(crate) const fn starts_with(&self, byte: u8) -> bool {
        self.first[(byte >> 6) as usize] & (1 << (byte & 63)) != 0
    }
}

/// Repair tables of one literal: its declared closer variants and its
/// confusable spellings, each with a trie over them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiteralTables {
    pub(crate) variants: Spellings,
    pub(crate) confusables: Spellings,
    /// In a `json-envelope` grammar, the JSON-syntax spellings of the
    /// literal, one table per repair ([`JsonSpelling`]); empty otherwise.
    pub(crate) json: [Spellings; 3],
}

/// The JSON-syntax spellings a `json-envelope` grammar reads for a literal,
/// each recorded under the `json.syntax.*` rule the JSON codec records for
/// the same slip inside arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JsonSpelling {
    /// Single quotes for double quotes, or a raw control character beside
    /// a lone quote (`json.syntax.string`).
    String = 0,
    /// A member key without its quotes (`json.syntax.bare-key`).
    BareKey = 1,
    /// The literal's `:` separators left out (`json.syntax.missing-colon`).
    MissingColon = 2,
}

impl JsonSpelling {
    pub(crate) const ALL: [Self; 3] = [Self::String, Self::BareKey, Self::MissingColon];

    pub(crate) const fn operator(self) -> super::edit::EditOperator {
        match self {
            Self::String => super::edit::EditOperator::JsonString,
            Self::BareKey => super::edit::EditOperator::JsonBareKey,
            Self::MissingColon => super::edit::EditOperator::JsonMissingColon,
        }
    }
}

/// A list of spellings and the trie recognizing them (labelled by index).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Spellings {
    pub(crate) list: Vec<Vec<u8>>,
    pub(crate) trie: Trie,
}

impl Spellings {
    fn build(list: Vec<Vec<u8>>, governor: &mut Governor) -> Result<Self, GrammarError> {
        let labelled: Vec<(Vec<u8>, u32)> = list
            .iter()
            .enumerate()
            .map(|(index, bytes)| (bytes.clone(), index_u32(index)))
            .collect();
        let trie = Trie::build(&labelled, governor)?;
        Ok(Self { list, trie })
    }

    fn bytes(&self) -> usize {
        self.list.iter().map(Vec::len).sum()
    }
}

struct Builder<'g> {
    governor: &'g mut Governor,
    literals: Vec<Literal>,
    closers: Vec<CloserClass>,
    classes: Vec<ClassTerminal>,
    delimiter_sets: Vec<Vec<Vec<u8>>>,
    names: Vec<(String, Name, usize)>,
    production_offsets: Vec<usize>,
}

#[derive(Clone, Copy)]
enum Name {
    Production(u32),
    Closer(u32),
}

fn charge(governor: &mut Governor, work: u64, scratch: u64) -> Result<(), GrammarError> {
    governor.charge(Resource::Work, work.saturating_mul(K_COMPILE))?;
    governor.charge(Resource::ScratchBytes, scratch)?;
    Ok(())
}

impl<'g> Builder<'g> {
    /// An empty builder charging `governor`.
    const fn new(governor: &'g mut Governor) -> Self {
        Self {
            governor,
            literals: Vec::new(),
            closers: Vec::new(),
            classes: Vec::new(),
            delimiter_sets: Vec::new(),
            names: Vec::new(),
            production_offsets: Vec::new(),
        }
    }
}

impl Builder<'_> {
    fn intern_literal(
        &mut self,
        bytes: &[u8],
        closer: Option<CloserId>,
    ) -> Result<u32, GrammarError> {
        if let Some(index) = self
            .literals
            .iter()
            .position(|literal| literal.bytes == bytes && literal.closer == closer)
        {
            return Ok(index_u32(index));
        }
        charge(self.governor, 1, size(bytes.len()).saturating_add(48))?;
        self.literals.push(Literal {
            bytes: bytes.to_vec(),
            closer,
        });
        Ok(index_u32(self.literals.len() - 1))
    }

    fn intern_delimiters(&mut self, literals: &[Vec<u8>]) -> Result<u32, GrammarError> {
        if let Some(index) = self.delimiter_sets.iter().position(|set| set == literals) {
            return Ok(index_u32(index));
        }
        let bytes: usize = literals.iter().map(Vec::len).sum();
        charge(
            self.governor,
            size(literals.len()),
            size(bytes).saturating_add(64),
        )?;
        self.delimiter_sets.push(literals.to_vec());
        Ok(index_u32(self.delimiter_sets.len() - 1))
    }

    fn intern_class(&mut self, source: &str, at: usize) -> Result<u32, GrammarError> {
        if let Some(index) = self.classes.iter().position(|class| class.source == source) {
            return Ok(index_u32(index));
        }
        let anchored = format!("^(?:{source})$");
        let pattern =
            Pattern::compile_governed(&anchored, self.governor).map_err(|error| match error {
                PatternCompileError::Syntax(error) => GrammarError::InvalidClass {
                    offset: at,
                    message: error.to_string(),
                },
                PatternCompileError::Resource(reason) => GrammarError::Resource(reason),
            })?;
        let Some(automaton) = pattern.into_automaton() else {
            return Err(GrammarError::InvalidClass {
                offset: at,
                message: "backreferences and lookaround are not finite-state".into(),
            });
        };
        let mut dfa = automaton.dfa(self.governor)?;
        let start = dfa.start();
        if dfa.accepting(start, self.governor)? {
            return Err(GrammarError::InvalidClass {
                offset: at,
                message: "a class must not match the empty string; use `?` or `*`".into(),
            });
        }
        drop(dfa);
        self.classes.push(ClassTerminal {
            source: source.to_owned(),
            automaton,
        });
        Ok(index_u32(self.classes.len() - 1))
    }

    fn lookup(&self, name: &str) -> Option<Name> {
        self.names
            .iter()
            .find(|(known, _, _)| known == name)
            .map(|(_, target, _)| *target)
    }

    fn resolve(&mut self, expr: &PExpr) -> Result<Expr, GrammarError> {
        charge(self.governor, 1, 64)?;
        Ok(match expr {
            PExpr::Literal { bytes, .. } => {
                Expr::Literal(LiteralId(self.intern_literal(bytes, None)?))
            }
            PExpr::Class { source, at } => Expr::Class(ClassId(self.intern_class(source, *at)?)),
            PExpr::Until { literals, .. } => {
                Expr::Until(DelimiterSetId(self.intern_delimiters(literals)?))
            }
            PExpr::Slot {
                codec,
                codec_at,
                literals,
                ..
            } => {
                let codec =
                    CodecKind::from_name(codec).ok_or_else(|| GrammarError::UnknownCodec {
                        offset: *codec_at,
                        name: codec.clone(),
                    })?;
                Expr::Slot {
                    codec,
                    delimiters: DelimiterSetId(self.intern_delimiters(literals)?),
                }
            }
            PExpr::Name { name, at } => match self.lookup(name) {
                Some(Name::Production(index)) => Expr::Reference(ProductionId(index)),
                Some(Name::Closer(index)) => Expr::Closer(CloserId(index)),
                None => {
                    return Err(GrammarError::UnknownName {
                        offset: *at,
                        name: name.clone(),
                    });
                }
            },
            PExpr::Capture { kind, at, inner } => {
                let kind =
                    CaptureKind::from_name(kind).ok_or_else(|| GrammarError::UnknownCapture {
                        offset: *at,
                        name: kind.clone(),
                    })?;
                if matches!(kind, CaptureKind::Arguments | CaptureKind::ParamValue)
                    && !matches!(**inner, PExpr::Slot { .. })
                {
                    return Err(GrammarError::CaptureTarget {
                        offset: *at,
                        capture: kind.name(),
                    });
                }
                Expr::Capture {
                    kind,
                    inner: Box::new(self.resolve(inner)?),
                }
            }
            PExpr::Sequence(items, _) => Expr::Sequence(
                items
                    .iter()
                    .map(|item| self.resolve(item))
                    .collect::<Result<_, _>>()?,
            ),
            PExpr::Choice(items, _) => Expr::Choice(
                items
                    .iter()
                    .map(|item| self.resolve(item))
                    .collect::<Result<_, _>>()?,
            ),
            PExpr::Commit => Expr::Commit,
            PExpr::Next { .. } => Expr::Next,
            PExpr::Insert { bytes } => Expr::Insert(LiteralId(self.intern_literal(bytes, None)?)),
            PExpr::Repeat { inner, op, .. } => {
                let (min, max) = match op {
                    b'*' => (0, None),
                    b'+' => (1, None),
                    _ => (0, Some(1)),
                };
                Expr::Repeat {
                    inner: Box::new(self.resolve(inner)?),
                    min,
                    max,
                }
            }
        })
    }
}

/// Check that parameter captures appear only as adjacent
/// `param-name`/`param-value` pairs among one sequence's items.
fn check_pairs(expr: &PExpr, in_sequence: bool) -> Result<(), GrammarError> {
    match expr {
        PExpr::Capture { kind, at, inner } => {
            let parameter = matches!(kind.as_str(), "param-name" | "param-value");
            if parameter && !in_sequence {
                return Err(GrammarError::UnpairedParameter { offset: *at });
            }
            check_pairs(inner, false)
        }
        PExpr::Sequence(items, _) => {
            let mut expect_value: Option<usize> = None;
            for item in items {
                if let PExpr::Capture { kind, at, .. } = item {
                    match (kind.as_str(), expect_value) {
                        ("param-name", None) => expect_value = Some(*at),
                        ("param-value", Some(_)) => expect_value = None,
                        ("param-name" | "param-value", _) => {
                            return Err(GrammarError::UnpairedParameter { offset: *at });
                        }
                        _ => {}
                    }
                }
                check_pairs(item, true)?;
            }
            expect_value.map_or(Ok(()), |offset| {
                Err(GrammarError::UnpairedParameter { offset })
            })
        }
        PExpr::Choice(items, _) => items.iter().try_for_each(|item| check_pairs(item, false)),
        PExpr::Repeat { inner, .. } => check_pairs(inner, false),
        PExpr::Literal { .. }
        | PExpr::Class { .. }
        | PExpr::Until { .. }
        | PExpr::Slot { .. }
        | PExpr::Name { .. }
        | PExpr::Commit
        | PExpr::Next { .. }
        | PExpr::Insert { .. } => Ok(()),
    }
}

fn references(expr: &Expr, into: &mut Vec<u32>) {
    match expr {
        Expr::Reference(id) => into.push(id.0),
        Expr::Capture { inner, .. } | Expr::Repeat { inner, .. } => references(inner, into),
        Expr::Sequence(items) | Expr::Choice(items) => {
            for item in items {
                references(item, into);
            }
        }
        Expr::Literal(_)
        | Expr::Closer(_)
        | Expr::Class(_)
        | Expr::Until(_)
        | Expr::Slot { .. }
        | Expr::Commit
        | Expr::Next
        | Expr::Insert(_) => {}
    }
}

/// Whether `expr` can reach a `next`, following references. Productions
/// are acyclic, so the walk terminates; each visit charges one `Work` unit.
fn holds_next(
    productions: &[Production],
    expr: &Expr,
    governor: &mut Governor,
) -> Result<bool, GrammarError> {
    charge(governor, 1, 0)?;
    Ok(match expr {
        Expr::Next => true,
        Expr::Capture { inner, .. } | Expr::Repeat { inner, .. } => {
            holds_next(productions, inner, governor)?
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            let mut found = false;
            for item in items {
                if holds_next(productions, item, governor)? {
                    found = true;
                    break;
                }
            }
            found
        }
        Expr::Reference(id) => holds_next(productions, &productions[id.index()].expr, governor)?,
        Expr::Literal(_)
        | Expr::Closer(_)
        | Expr::Class(_)
        | Expr::Until(_)
        | Expr::Slot { .. }
        | Expr::Commit
        | Expr::Insert(_) => false,
    })
}

/// Refuse a `next` that cannot separate calls: one inside a capture, or
/// reachable from the `result` or `declaration` production (each prints and
/// reads exactly one item).
fn check_next(
    productions: &[Production],
    offsets: &[usize],
    single: [ProductionId; 2],
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    fn captures_next(
        productions: &[Production],
        expr: &Expr,
        governor: &mut Governor,
    ) -> Result<bool, GrammarError> {
        Ok(match expr {
            Expr::Capture { inner, .. } => holds_next(productions, inner, governor)?,
            Expr::Repeat { inner, .. } => captures_next(productions, inner, governor)?,
            Expr::Sequence(items) | Expr::Choice(items) => {
                let mut found = false;
                for item in items {
                    if captures_next(productions, item, governor)? {
                        found = true;
                        break;
                    }
                }
                found
            }
            _ => false,
        })
    }
    for (index, production) in productions.iter().enumerate() {
        charge(governor, 1, 0)?;
        let single = single.iter().any(|id| id.index() == index);
        if (single && holds_next(productions, &production.expr, governor)?)
            || captures_next(productions, &production.expr, governor)?
        {
            return Err(GrammarError::MisplacedNext {
                offset: offsets[index],
                name: production.name.clone(),
            });
        }
    }
    Ok(())
}

/// Whether a call of `root` can capture `kind`: some path through the
/// production, following references, holds a `kind:` capture. Linear in the
/// resolved syntax tree (each production is walked once), which compiling
/// has already charged node by node; productions are acyclic.
fn captures(productions: &[Production], root: ProductionId, kind: CaptureKind) -> bool {
    fn walk(
        productions: &[Production],
        expr: &Expr,
        kind: CaptureKind,
        memo: &mut [Option<bool>],
    ) -> bool {
        match expr {
            Expr::Capture { kind: found, inner } => {
                *found == kind || walk(productions, inner, kind, memo)
            }
            Expr::Repeat { inner, .. } => walk(productions, inner, kind, memo),
            Expr::Sequence(items) | Expr::Choice(items) => {
                items.iter().any(|item| walk(productions, item, kind, memo))
            }
            Expr::Reference(id) => {
                if let Some(known) = memo[id.index()] {
                    return known;
                }
                let found = walk(productions, &productions[id.index()].expr, kind, memo);
                memo[id.index()] = Some(found);
                found
            }
            Expr::Literal(_)
            | Expr::Closer(_)
            | Expr::Class(_)
            | Expr::Until(_)
            | Expr::Slot { .. }
            | Expr::Commit
            | Expr::Next
            | Expr::Insert(_) => false,
        }
    }
    let mut memo = vec![None; productions.len()];
    walk(
        productions,
        &productions[root.index()].expr,
        kind,
        &mut memo,
    )
}

/// Refuse a production reachable from itself.
fn check_acyclic(
    productions: &[Production],
    offsets: &[usize],
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    // 0 = unvisited, 1 = on the current path, 2 = finished.
    let mut color = vec![0_u8; productions.len()];
    for root in 0..productions.len() {
        if color[root] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, Vec<u32>, usize)> = Vec::new();
        let mut edges = Vec::new();
        references(&productions[root].expr, &mut edges);
        color[root] = 1;
        stack.push((root, edges, 0));
        while let Some((node, edges, next)) = stack.last_mut() {
            charge(governor, 1, 0)?;
            if let Some(&target) = edges.get(*next) {
                *next += 1;
                let target = target as usize;
                match color[target] {
                    1 => {
                        return Err(GrammarError::Recursive {
                            offset: offsets[target],
                            name: productions[target].name.clone(),
                        });
                    }
                    0 => {
                        color[target] = 1;
                        let mut child = Vec::new();
                        references(&productions[target].expr, &mut child);
                        stack.push((target, child, 0));
                    }
                    _ => {}
                }
            } else {
                color[*node] = 2;
                stack.pop();
            }
        }
    }
    Ok(())
}

struct Emitter<'a, 'g> {
    productions: &'a [Production],
    closer_literals: &'a [u32],
    insts: Vec<Inst>,
    governor: &'g mut Governor,
}

impl Emitter<'_, '_> {
    fn push(&mut self, inst: Inst) -> Result<u32, GrammarError> {
        charge(self.governor, 1, size(<Inst as ScratchCharge>::CHARGE))?;
        self.insts.push(inst);
        Ok(index_u32(self.insts.len() - 1))
    }

    /// Compile `expr` so that it continues at `next`; returns its entry.
    fn emit(&mut self, expr: &Expr, next: u32) -> Result<u32, GrammarError> {
        Ok(match expr {
            Expr::Literal(id) => self.push(Inst::Lit { lit: id.0, next })?,
            Expr::Closer(id) => self.push(Inst::Lit {
                lit: self.closer_literals[id.index()],
                next,
            })?,
            Expr::Class(id) => self.push(Inst::Class { class: id.0, next })?,
            Expr::Until(id) => self.push(Inst::Until { set: id.0, next })?,
            Expr::Slot { codec, delimiters } => self.push(Inst::Slot {
                codec: *codec,
                set: delimiters.0,
                next,
            })?,
            Expr::Capture { kind, inner } => {
                let close = self.push(Inst::Close { kind: *kind, next })?;
                let body = self.emit(inner, close)?;
                self.push(Inst::Open {
                    kind: *kind,
                    next: body,
                })?
            }
            Expr::Sequence(items) => {
                let mut entry = next;
                for item in items.iter().rev() {
                    entry = self.emit(item, entry)?;
                }
                entry
            }
            Expr::Choice(items) => {
                let mut entries = Vec::with_capacity(items.len());
                for item in items {
                    entries.push(self.emit(item, next)?);
                }
                let mut entry = entries.pop().expect("a choice has alternatives");
                while let Some(other) = entries.pop() {
                    entry = self.push(Inst::Split(other, entry))?;
                }
                entry
            }
            Expr::Repeat { inner, min, max } => match (min, max) {
                (0, Some(1)) => {
                    let body = self.emit(inner, next)?;
                    self.push(Inst::Split(body, next))?
                }
                (min, None) => {
                    let split = self.push(Inst::Split(next, next))?;
                    let body = self.emit(inner, split)?;
                    self.insts[split as usize] = Inst::Split(body, next);
                    if *min == 0 { split } else { body }
                }
                _ => unreachable!("the DSL only builds `*`, `+` and `?`"),
            },
            Expr::Reference(id) => {
                let expr = &self.productions[id.index()].expr;
                self.emit(expr, next)?
            }
            Expr::Commit => self.push(Inst::Commit { next })?,
            Expr::Next => self.push(Inst::Next { next })?,
            Expr::Insert(id) => self.push(Inst::Insert { lit: id.0, next })?,
        })
    }
}

/// Every pc reachable from `start` without consuming input.
fn epsilon_closure(insts: &[Inst], start: u32) -> Vec<u32> {
    let mut seen = vec![false; insts.len()];
    let mut stack = vec![start];
    let mut reached = Vec::new();
    while let Some(pc) = stack.pop() {
        if core::mem::replace(&mut seen[pc as usize], true) {
            continue;
        }
        match insts[pc as usize] {
            Inst::Split(first, second) => {
                stack.push(second);
                stack.push(first);
            }
            Inst::Open { next, .. }
            | Inst::Close { next, .. }
            | Inst::Commit { next }
            | Inst::Next { next }
            | Inst::Insert { next, .. } => {
                stack.push(next);
            }
            _ => reached.push(pc),
        }
    }
    reached.sort_unstable();
    reached
}

/// The locations only a committed attempt reaches: those not reachable
/// from `start` without passing a `commit` instruction. Empty when the
/// program has no `commit`. Charges one `Work` unit per instruction.
fn committed(
    insts: &[Inst],
    start: u32,
    governor: &mut Governor,
) -> Result<Vec<bool>, GrammarError> {
    if !insts.iter().any(|inst| matches!(inst, Inst::Commit { .. })) {
        return Ok(Vec::new());
    }
    charge(governor, size(insts.len()), size(insts.len()))?;
    let mut reached = vec![false; insts.len()];
    let mut stack = vec![start];
    while let Some(pc) = stack.pop() {
        if core::mem::replace(&mut reached[pc as usize], true) {
            continue;
        }
        match insts[pc as usize] {
            Inst::Lit { next, .. }
            | Inst::Class { next, .. }
            | Inst::Until { next, .. }
            | Inst::Slot { next, .. }
            | Inst::Open { next, .. }
            | Inst::Close { next, .. }
            | Inst::Next { next }
            | Inst::Insert { next, .. } => stack.push(next),
            Inst::Split(first, second) => stack.extend([first, second]),
            Inst::Commit { .. } | Inst::Accept => {}
        }
    }
    Ok(reached.into_iter().map(|reached| !reached).collect())
}

/// Every confusable spelling of `bytes`: for each non-empty subset of the
/// confusable pairs occurring in it, replace every occurrence of those pairs'
/// canonical text by its variant.
fn confusable_spellings(
    bytes: &[u8],
    confusables: &[(Vec<u8>, Vec<u8>)],
    governor: &mut Governor,
) -> Result<Vec<Vec<u8>>, GrammarError> {
    let present: Vec<usize> = (0..confusables.len())
        .filter(|&index| {
            let canonical = &confusables[index].0;
            bytes
                .windows(canonical.len())
                .any(|window| window == canonical.as_slice())
        })
        .collect();
    let mut spellings = Vec::new();
    if present.is_empty() {
        return Ok(spellings);
    }
    let subsets = 1_u64
        .checked_shl(u32::try_from(present.len()).unwrap_or(u32::MAX))
        .ok_or(GrammarError::Resource(StopReason::BudgetExceeded(
            Resource::Work,
        )))?;
    for mask in 1..subsets {
        charge(
            governor,
            size(bytes.len()),
            size(bytes.len()).saturating_mul(2).saturating_add(24),
        )?;
        let mut spelling = Vec::with_capacity(bytes.len());
        let mut index = 0;
        'scan: while index < bytes.len() {
            for (bit, &pair) in present.iter().enumerate() {
                let (canonical, variant) = &confusables[pair];
                if mask & (1 << bit) != 0 && bytes[index..].starts_with(canonical) {
                    spelling.extend_from_slice(variant);
                    index += canonical.len();
                    continue 'scan;
                }
            }
            spelling.push(bytes[index]);
            index += 1;
        }
        if spelling != bytes && !spellings.contains(&spelling) {
            spellings.push(spelling);
        }
    }
    Ok(spellings)
}

/// The JSON-syntax spellings of `bytes` (see [`JsonSpelling`]): every `"`
/// as `'` (and, for a lone `"`, a raw control character before or after
/// it); every `"KEY"` that is a member key (followed by `:` in the literal,
/// or, when `key_at_end`, ending it) without its quotes; and every `:`
/// outside quotes left out.
fn json_spellings(bytes: &[u8], key_at_end: bool) -> [Vec<Vec<u8>>; 3] {
    let mut string = Vec::new();
    if bytes.contains(&b'"') {
        string.push(
            bytes
                .iter()
                .map(|&b| if b == b'"' { b'\'' } else { b })
                .collect(),
        );
    }
    // Quoted words and colons outside quotes.
    let mut bare = Vec::with_capacity(bytes.len());
    let mut keys = 0;
    let mut colonless = Vec::with_capacity(bytes.len());
    let mut colons = 0;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'"' {
            let close = bytes[index + 1..].iter().position(|&b| b == b'"');
            if let Some(length) = close {
                let word = &bytes[index + 1..index + 1 + length];
                let after = index + 2 + length;
                let colon_follows = bytes[after..]
                    .iter()
                    .find(|b| **b != b' ')
                    .is_some_and(|b| *b == b':');
                let is_key = !word.is_empty()
                    && word.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
                    && (colon_follows || (after == bytes.len() && key_at_end));
                if is_key {
                    bare.extend_from_slice(word);
                    keys += 1;
                } else {
                    bare.extend_from_slice(&bytes[index..after]);
                }
                colonless.extend_from_slice(&bytes[index..after]);
                index = after;
                continue;
            }
        }
        if byte == b':' && bytes.len() > 1 {
            colons += 1;
        } else {
            colonless.push(byte);
        }
        bare.push(byte);
        index += 1;
    }
    let bare = if keys > 0 { vec![bare] } else { Vec::new() };
    let colonless = if colons > 0 && !colonless.is_empty() {
        vec![colonless]
    } else {
        Vec::new()
    };
    [string, bare, colonless]
}

fn literal_tables(
    literal: &Literal,
    closers: &[CloserClass],
    confusables: &[(Vec<u8>, Vec<u8>)],
    json: Option<bool>,
    governor: &mut Governor,
) -> Result<LiteralTables, GrammarError> {
    let mut variants: Vec<Vec<u8>> = Vec::new();
    if let Some(closer) = literal.closer {
        for variant in closers[closer.index()].spellings.iter().skip(1) {
            if *variant != literal.bytes && !variants.contains(variant) {
                variants.push(variant.clone());
            }
        }
    }
    let spellings: Vec<Vec<u8>> = confusable_spellings(&literal.bytes, confusables, governor)?
        .into_iter()
        .filter(|spelling| !variants.contains(spelling))
        .collect();
    let mut json_tables = [Vec::new(), Vec::new(), Vec::new()];
    if let Some(key_at_end) = json {
        let mut seen: Vec<Vec<u8>> = variants.iter().chain(&spellings).cloned().collect();
        seen.push(literal.bytes.clone());
        for (table, generated) in json_tables
            .iter_mut()
            .zip(json_spellings(&literal.bytes, key_at_end))
        {
            for spelling in generated {
                if !seen.contains(&spelling) {
                    seen.push(spelling.clone());
                    table.push(spelling);
                }
            }
        }
    }
    let [string, bare, colon] = json_tables;
    Ok(LiteralTables {
        variants: Spellings::build(variants, governor)?,
        confusables: Spellings::build(spellings, governor)?,
        json: [
            Spellings::build(string, governor)?,
            Spellings::build(bare, governor)?,
            Spellings::build(colon, governor)?,
        ],
    })
}

/// Each literal's key flag, and per program the key uses `(pc, copy)` to
/// point at a literal's key copy (see `split_keys`).
type KeySplit = (Vec<bool>, Renames);

/// Per program, the `Lit` instructions (`pc`) to point at another literal.
type Renames = Vec<Vec<(u32, u32)>>;

/// The literal tables of a `json-envelope` grammar, JSON spellings
/// included, over `literals` with their key copies (see `split_keys`), and
/// the key uses to point at the copies.
fn json_tables(
    programs: &[Program],
    literals: &mut Vec<Literal>,
    closers: &[CloserClass],
    confusables: &[(Vec<u8>, Vec<u8>)],
    governor: &mut Governor,
) -> Result<(Vec<LiteralTables>, Renames), GrammarError> {
    let (keys, renames) = split_keys(programs, literals, governor)?;
    charge(governor, size(literals.len()), 0)?;
    let tables = literals
        .iter()
        .zip(&keys)
        .map(|(literal, &key)| literal_tables(literal, closers, confusables, Some(key), governor))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((tables, renames))
}

/// Each `Lit` use of each program, `(pc, literal, key)`: whether the use
/// is followed by a literal beginning with `:` (optional class runs, such
/// as whitespace, between), as a member key is.
fn key_uses(programs: &[Program], literals: &[Literal]) -> Vec<Vec<(u32, u32, bool)>> {
    programs
        .iter()
        .map(|program| {
            let mut uses = Vec::new();
            for (pc, inst) in program.insts.iter().enumerate() {
                let Inst::Lit { lit, next } = *inst else {
                    continue;
                };
                let mut colon = true;
                let mut any = false;
                let mut seen = vec![false; program.insts.len()];
                let mut stack = vec![next];
                while let Some(pc) = stack.pop() {
                    if core::mem::replace(&mut seen[pc as usize], true) {
                        continue;
                    }
                    match program.insts[pc as usize] {
                        Inst::Split(first, second) => stack.extend([first, second]),
                        Inst::Open { next, .. }
                        | Inst::Close { next, .. }
                        | Inst::Commit { next }
                        | Inst::Next { next }
                        | Inst::Insert { next, .. }
                        | Inst::Class { next, .. } => stack.push(next),
                        Inst::Lit { lit, .. } => {
                            any = true;
                            colon &= literals[lit as usize].bytes.first() == Some(&b':');
                        }
                        Inst::Until { .. } | Inst::Slot { .. } | Inst::Accept => colon = false,
                    }
                }
                uses.push((index_u32(pc), lit, any && colon));
            }
            uses
        })
        .collect()
}

/// The member-key literals of a `json-envelope` grammar: a literal every
/// use of which is a key is one, and a literal used both as a key and
/// otherwise (`"function"`, the key of `OpenAI`'s function object and the
/// value of its `type`) gets a copy, appended to `literals`, for its key
/// uses. Returns each literal's key flag and, per program, the key uses
/// (`pc`, copy) to point at the copies.
fn split_keys(
    programs: &[Program],
    literals: &mut Vec<Literal>,
    governor: &mut Governor,
) -> Result<KeySplit, GrammarError> {
    let uses = key_uses(programs, literals);
    let mut counts = vec![(0_u32, 0_u32); literals.len()];
    for &(_, lit, key) in uses.iter().flatten() {
        let count = &mut counts[lit as usize];
        count.0 += 1;
        count.1 += u32::from(key);
    }
    let mut keys: Vec<bool> = counts
        .iter()
        .map(|&(all, keyed)| all > 0 && all == keyed)
        .collect();
    let mut copies: Vec<Option<u32>> = vec![None; literals.len()];
    let mut per_program = Vec::with_capacity(uses.len());
    for program in &uses {
        let mut moved = Vec::new();
        for &(pc, lit, key) in program {
            let (all, keyed) = counts[lit as usize];
            if !key || keyed == all {
                continue;
            }
            let copy = if let Some(copy) = copies[lit as usize] {
                copy
            } else {
                charge(governor, 1, size(literals[lit as usize].bytes.len()))?;
                literals.push(literals[lit as usize].clone());
                keys.push(true);
                let copy = index_u32(literals.len() - 1);
                copies[lit as usize] = Some(copy);
                copy
            };
            moved.push((pc, copy));
        }
        per_program.push(moved);
    }
    Ok((keys, per_program))
}

fn compile_program(
    productions: &[Production],
    closer_literals: &[u32],
    root: ProductionId,
    offset: usize,
    tables: &Tables<'_>,
    renames: &[(u32, u32)],
    governor: &mut Governor,
) -> Result<Program, GrammarError> {
    let mut emitter = Emitter {
        productions,
        closer_literals,
        insts: Vec::new(),
        governor,
    };
    let accept = emitter.push(Inst::Accept)?;
    let start = emitter.emit(&productions[root.index()].expr, accept)?;
    let mut insts = emitter.insts;
    // Emission does not depend on the tables, so a use found at `pc` by an
    // earlier compile of the same productions is at `pc` again.
    for &(pc, copy) in renames {
        if let Inst::Lit { lit, .. } = &mut insts[pc as usize] {
            *lit = copy;
        }
    }
    let invalid = || GrammarError::InvalidOpener {
        offset,
        production: productions[root.index()].name.clone(),
    };
    let openers = epsilon_closure(&insts, start);
    let mut first = [0_u64; 4];
    for &pc in &openers {
        let Inst::Lit { lit, .. } = insts[pc as usize] else {
            return Err(invalid());
        };
        let mut mark = |byte: u8| first[usize::from(byte >> 6)] |= 1 << (byte & 63);
        mark(tables.literals[lit as usize].bytes[0]);
        let repairs = &tables.repairs[lit as usize];
        for byte in repairs
            .variants
            .trie
            .first_bytes()
            .chain(repairs.confusables.trie.first_bytes())
            .chain(repairs.json.iter().flat_map(|json| json.trie.first_bytes()))
        {
            mark(byte);
        }
    }
    let mut arguments = false;
    let mut parameters = false;
    let mut bound = 0_u64;
    for inst in &insts {
        bound = bound.saturating_add(match inst {
            Inst::Lit { lit, .. } => size(tables.literals[*lit as usize].bytes.len())
                .saturating_add(size(tables.repairs[*lit as usize].trie_nodes()))
                .saturating_add(size(tables.stray_nodes)),
            Inst::Class { .. } | Inst::Until { .. } | Inst::Slot { .. } => 1,
            Inst::Open { kind, .. } => {
                arguments |= *kind == CaptureKind::Arguments;
                parameters |= *kind == CaptureKind::ParamName;
                0
            }
            Inst::Split(..)
            | Inst::Close { .. }
            | Inst::Commit { .. }
            | Inst::Next { .. }
            | Inst::Insert { .. }
            | Inst::Accept => 0,
        });
    }
    if arguments && parameters {
        return Err(GrammarError::ConflictingArguments { offset });
    }
    let committed = committed(&insts, start, governor)?;
    let grouped = insts.iter().any(|inst| matches!(inst, Inst::Next { .. }));
    let start_closure = {
        let mut seen = vec![false; insts.len()];
        let mut stack = vec![start];
        let mut effects = false;
        while let Some(pc) = stack.pop() {
            if core::mem::replace(&mut seen[pc as usize], true) {
                continue;
            }
            match insts[pc as usize] {
                Inst::Split(first, second) => stack.extend([first, second]),
                Inst::Open { .. }
                | Inst::Close { .. }
                | Inst::Commit { .. }
                | Inst::Next { .. }
                | Inst::Insert { .. } => effects = true,
                _ => {}
            }
        }
        effects
    };
    Ok(Program {
        insts,
        start,
        openers,
        first,
        bound,
        committed,
        start_closure,
        grouped,
    })
}

impl LiteralTables {
    fn trie_nodes(&self) -> usize {
        self.variants.bytes()
            + self.confusables.bytes()
            + self.json.iter().map(Spellings::bytes).sum::<usize>()
    }
}

struct Tables<'a> {
    literals: &'a [Literal],
    repairs: &'a [LiteralTables],
    stray_nodes: usize,
}

/// What the `call` and `result` programs are compiled from, over literal
/// tables that a `json-envelope` grammar rebuilds once.
struct Programs<'p> {
    productions: &'p [Production],
    closer_ids: &'p [u32],
    offsets: &'p [usize],
    stray_nodes: usize,
    roots: [ProductionId; 2],
}

impl Programs<'_> {
    fn compile(
        &self,
        literals: &[Literal],
        repairs: &[LiteralTables],
        renames: &[Vec<(u32, u32)>],
        governor: &mut Governor,
    ) -> Result<[Program; 2], GrammarError> {
        let tables = Tables {
            literals,
            repairs,
            stray_nodes: self.stray_nodes,
        };
        let mut program = |index: usize| {
            let root = self.roots[index];
            compile_program(
                self.productions,
                self.closer_ids,
                root,
                self.offsets[root.index()],
                &tables,
                renames.get(index).map_or(&[][..], Vec::as_slice),
                governor,
            )
        };
        Ok([program(0)?, program(1)?])
    }
}

/// The non-production statements of a document, and its production bodies
/// in order (registered in the builder's namespace).
struct Declared<'d> {
    reasoning: Vec<(Vec<u8>, Vec<u8>)>,
    /// Per `reasoning` pair, whether it is declared `explicit`.
    explicit_reasoning: Vec<bool>,
    confusables: Vec<(Vec<u8>, Vec<u8>)>,
    bodies: Vec<(String, &'d PExpr)>,
    layouts: Vec<(&'d str, &'d [u8], usize)>,
    optional_id: bool,
    declaration_content: bool,
    json_envelope: bool,
}

fn declare<'d>(
    builder: &mut Builder<'_>,
    statements: &'d [Statement],
) -> Result<Declared<'d>, GrammarError> {
    let mut declared = Declared {
        reasoning: Vec::new(),
        explicit_reasoning: Vec::new(),
        confusables: Vec::new(),
        bodies: Vec::new(),
        layouts: Vec::new(),
        optional_id: false,
        declaration_content: false,
        json_envelope: false,
    };
    let duplicate = |offset: usize, name: &str| GrammarError::DuplicateName {
        offset,
        name: name.to_owned(),
    };
    for statement in statements {
        match statement {
            Statement::Reasoning {
                open,
                close,
                explicit,
                at,
            } => {
                let pair = (open.clone(), close.clone());
                if declared.reasoning.contains(&pair) {
                    return Err(duplicate(*at, "reasoning"));
                }
                declared.reasoning.push(pair);
                declared.explicit_reasoning.push(*explicit);
            }
            Statement::OptionalId { at } => {
                if declared.optional_id {
                    return Err(duplicate(*at, "optional-id"));
                }
                declared.optional_id = true;
            }
            Statement::DeclarationContent { at } => {
                if declared.declaration_content {
                    return Err(duplicate(*at, "declaration-content"));
                }
                declared.declaration_content = true;
            }
            Statement::JsonEnvelope { at } => {
                if declared.json_envelope {
                    return Err(duplicate(*at, "json-envelope"));
                }
                declared.json_envelope = true;
            }
            Statement::Layout { name, text, at } => {
                declared.layouts.push((name.as_str(), text.as_slice(), *at));
            }
            Statement::Confusable {
                canonical, variant, ..
            } => declared
                .confusables
                .push((canonical.clone(), variant.clone())),
            Statement::Closer {
                name,
                spellings,
                at,
            } => {
                if builder.lookup(name).is_some() {
                    return Err(duplicate(*at, name));
                }
                let index = index_u32(builder.closers.len());
                builder.names.push((name.clone(), Name::Closer(index), *at));
                builder.closers.push(CloserClass {
                    name: name.clone(),
                    spellings: spellings.clone(),
                });
            }
            Statement::Production { name, expr, at } => {
                if builder.lookup(name).is_some() {
                    return Err(duplicate(*at, name));
                }
                check_pairs(expr, false)?;
                let index = index_u32(declared.bodies.len());
                builder
                    .names
                    .push((name.clone(), Name::Production(index), *at));
                builder.production_offsets.push(*at);
                declared.bodies.push((name.clone(), expr));
            }
        }
    }
    Ok(declared)
}

/// Intern each closer class's canonical spelling as a closer-marked literal.
fn closer_literals(
    literals: &mut Vec<Literal>,
    closers: &[CloserClass],
    governor: &mut Governor,
) -> Result<Vec<u32>, GrammarError> {
    let mut ids = Vec::with_capacity(closers.len());
    for (index, closer) in closers.iter().enumerate() {
        let id = Some(CloserId(index_u32(index)));
        let canonical = &closer.spellings[0];
        let known = literals
            .iter()
            .position(|literal| literal.bytes == *canonical && literal.closer == id);
        if let Some(known) = known {
            ids.push(index_u32(known));
        } else {
            charge(governor, 1, size(canonical.len()).saturating_add(48))?;
            literals.push(Literal {
                bytes: canonical.clone(),
                closer: id,
            });
            ids.push(index_u32(literals.len() - 1));
        }
    }
    Ok(ids)
}

/// Every closer spelling of every class: what a stray-closer deletion
/// recognizes.
fn stray_spellings(
    closers: &[CloserClass],
    governor: &mut Governor,
) -> Result<Spellings, GrammarError> {
    let mut list: Vec<Vec<u8>> = Vec::new();
    for spelling in closers.iter().flat_map(|closer| &closer.spellings) {
        if !list.contains(spelling) {
            list.push(spelling.clone());
        }
    }
    Spellings::build(list, governor)
}

/// Each delimiter set's matcher, over its literals and their confusable
/// spellings.
fn delimiter_sets(
    sets: Vec<Vec<Vec<u8>>>,
    confusables: &[(Vec<u8>, Vec<u8>)],
    governor: &mut Governor,
) -> Result<Vec<DelimiterSet>, GrammarError> {
    let mut delimiters = Vec::with_capacity(sets.len());
    for declared in sets {
        let mut expanded = declared.clone();
        for literal in &declared {
            for spelling in confusable_spellings(literal, confusables, governor)? {
                if !expanded.contains(&spelling) {
                    expanded.push(spelling);
                }
            }
        }
        let matcher = Matcher::build(&expanded, governor)?;
        delimiters.push(DelimiterSet { declared, matcher });
    }
    Ok(delimiters)
}

/// The longest literal spelling, delimiter, or reasoning marker.
fn longest_literal(
    literals: &[Literal],
    repairs: &[LiteralTables],
    stray: &Spellings,
    delimiters: &[DelimiterSet],
    reasoning: &[(Vec<u8>, Vec<u8>)],
) -> usize {
    literals
        .iter()
        .map(|literal| &literal.bytes)
        .chain(repairs.iter().flat_map(|tables| {
            tables
                .variants
                .list
                .iter()
                .chain(&tables.confusables.list)
                .chain(tables.json.iter().flat_map(|json| &json.list))
        }))
        .chain(&stray.list)
        .map(Vec::len)
        .chain(delimiters.iter().map(|set| set.matcher.longest()))
        .chain(
            reasoning
                .iter()
                .flat_map(|(open, close)| [open.len(), close.len()]),
        )
        .max()
        .unwrap_or(0)
}

/// Resolve, check and compile a parsed document.
pub(crate) fn compile(
    document: Document,
    digest: Digest,
    governor: &mut Governor,
) -> Result<CompiledGrammar, GrammarError> {
    let mut builder = Builder::new(governor);
    let Declared {
        reasoning,
        explicit_reasoning,
        confusables,
        bodies,
        layouts: declared_layouts,
        optional_id,
        declaration_content,
        json_envelope,
    } = declare(&mut builder, &document.statements)?;
    let mut productions = Vec::with_capacity(bodies.len());
    for (name, expr) in bodies {
        let expr = builder.resolve(expr)?;
        productions.push(Production { name, expr });
    }
    let layouts = layouts(&mut builder, &productions, &declared_layouts)?;
    let required = |name: &'static str| {
        productions
            .iter()
            .position(|production| production.name == name)
            .map(|index| ProductionId(index_u32(index)))
            .ok_or(GrammarError::MissingProduction { name })
    };
    let call = required("call")?;
    let result = required("result")?;
    let declaration = required("declaration")?;
    let Builder {
        governor,
        mut literals,
        closers,
        classes,
        delimiter_sets: sets,
        production_offsets,
        ..
    } = builder;
    check_acyclic(&productions, &production_offsets, governor)?;
    check_next(
        &productions,
        &production_offsets,
        [result, declaration],
        governor,
    )?;
    let closer_ids = closer_literals(&mut literals, &closers, governor)?;
    let mut literal_tables = literals
        .iter()
        .map(|literal| self::literal_tables(literal, &closers, &confusables, None, governor))
        .collect::<Result<Vec<_>, _>>()?;
    let stray = stray_spellings(&closers, governor)?;
    let delimiters = delimiter_sets(sets, &confusables, governor)?;
    let source = Programs {
        productions: &productions,
        closer_ids: &closer_ids,
        offsets: &production_offsets,
        stray_nodes: stray.bytes(),
        roots: [call, result],
    };
    let compile_programs =
        |literals: &[Literal],
         tables: &[LiteralTables],
         renames: &[Vec<(u32, u32)>],
         governor: &mut Governor| { source.compile(literals, tables, renames, governor) };
    let mut programs = compile_programs(&literals, &literal_tables, &[], governor)?;
    if json_envelope {
        // The JSON spellings need to know which literals are member keys,
        // which the compiled programs tell; the programs are then compiled
        // again over the extended tables, key uses of a literal also used
        // otherwise pointing at its key copy.
        let renames;
        (literal_tables, renames) =
            json_tables(&programs, &mut literals, &closers, &confusables, governor)?;
        programs = compile_programs(&literals, &literal_tables, &renames, governor)?;
    }
    let longest = longest_literal(&literals, &literal_tables, &stray, &delimiters, &reasoning);
    let call_ends = call_ends(&programs[0], &literals, &literal_tables, governor)?;
    // An `optional-id` grammar reads a call id where a model adds one, but
    // its markup does not provide for one.
    let call_ids = !optional_id && captures(&productions, call, CaptureKind::Id);
    Ok(CompiledGrammar {
        id: document.id,
        digest,
        literals,
        literal_tables,
        closers,
        confusables,
        classes,
        delimiters,
        productions,
        call,
        call_ids,
        declaration_content,
        json_envelope,
        result,
        declaration,
        reasoning,
        explicit_reasoning,
        call_ends,
        programs,
        stray: stray.trie,
        longest,
        layouts,
    })
}

/// Per opener alternative of the `call` production, the spellings of its
/// opener and of the literals that can end it (a literal from which the
/// production completes without further input: the wrapper or section
/// closer, the final brace of a bare object), each with its declared
/// closer variants and confusable spellings. A rejected attempt's markup
/// runs to its alternative's end literal (see
/// `ToolSet::recover`).
fn call_ends(
    program: &Program,
    literals: &[Literal],
    tables: &[LiteralTables],
    governor: &mut Governor,
) -> Result<Vec<CallEnd>, GrammarError> {
    // One walk of the call program per opener alternative, and one
    // backward walk for completion.
    charge(
        governor,
        size(program.openers.len())
            .saturating_add(1)
            .saturating_mul(size(program.insts.len())),
        size(program.insts.len()).saturating_mul(16),
    )?;
    let spellings = |lit: u32| {
        let tables = &tables[lit as usize];
        core::iter::once(literals[lit as usize].bytes.clone())
            .chain(tables.variants.list.iter().cloned())
            .chain(tables.confusables.list.iter().cloned())
            .collect::<Vec<_>>()
    };
    let next_of = |inst: &Inst| -> Vec<u32> {
        match *inst {
            Inst::Lit { next, .. }
            | Inst::Class { next, .. }
            | Inst::Until { next, .. }
            | Inst::Slot { next, .. }
            | Inst::Open { next, .. }
            | Inst::Close { next, .. }
            | Inst::Commit { next }
            | Inst::Next { next }
            | Inst::Insert { next, .. } => vec![next],
            Inst::Split(first, second) => vec![first, second],
            Inst::Accept => Vec::new(),
        }
    };
    // Whether the production completes from each instruction without
    // input: one backward walk from `Accept` over the epsilon instructions.
    let count = program.insts.len();
    let mut before: Vec<Vec<u32>> = vec![Vec::new(); count];
    let mut stack = Vec::new();
    for (pc, inst) in program.insts.iter().enumerate() {
        let pc = index_u32(pc);
        match *inst {
            Inst::Accept => stack.push(pc),
            Inst::Split(first, second) => {
                before[first as usize].push(pc);
                before[second as usize].push(pc);
            }
            Inst::Open { next, .. }
            | Inst::Close { next, .. }
            | Inst::Commit { next }
            | Inst::Next { next }
            | Inst::Insert { next, .. } => before[next as usize].push(pc),
            _ => {}
        }
    }
    let mut completing = vec![false; count];
    while let Some(pc) = stack.pop() {
        if !core::mem::replace(&mut completing[pc as usize], true) {
            stack.extend(before[pc as usize].iter().copied());
        }
    }
    let completes = |pc: u32| completing[pc as usize];
    Ok(program
        .openers
        .iter()
        .filter_map(|&opener| {
            let Inst::Lit { lit, .. } = program.insts[opener as usize] else {
                return None;
            };
            let mut seen = vec![false; program.insts.len()];
            let mut stack = vec![opener];
            let mut ends: Vec<Vec<u8>> = Vec::new();
            let mut chained = false;
            while let Some(pc) = stack.pop() {
                if core::mem::replace(&mut seen[pc as usize], true) {
                    if pc == opener {
                        chained = true;
                    }
                    continue;
                }
                let inst = &program.insts[pc as usize];
                if let Inst::Lit { lit: again, .. } = *inst
                    && pc != opener
                    && again == lit
                {
                    chained = true;
                }
                if let Inst::Lit { lit, next } = *inst
                    && pc != opener
                    && completes(next)
                {
                    for spelling in spellings(lit) {
                        if !ends.contains(&spelling) {
                            ends.push(spelling);
                        }
                    }
                }
                stack.extend(next_of(inst));
            }
            Some(CallEnd {
                openers: spellings(lit),
                closers: ends,
                chained,
            })
        })
        .collect())
}

/// Each production's printing layout: the text a `layout` statement gives
/// it, checked to be a member of the class the production names.
fn layouts(
    builder: &mut Builder<'_>,
    productions: &[Production],
    declared: &[(&str, &[u8], usize)],
) -> Result<Vec<Option<Vec<u8>>>, GrammarError> {
    let mut layouts: Vec<Option<Vec<u8>>> = vec![None; productions.len()];
    for &(name, text, at) in declared {
        let invalid = |message: &str| GrammarError::InvalidLayout {
            offset: at,
            name: name.to_owned(),
            message: message.to_owned(),
        };
        let index = match builder.lookup(name) {
            Some(Name::Production(index)) => index as usize,
            Some(Name::Closer(_)) => return Err(invalid("a closer has canonical spellings")),
            None => {
                return Err(GrammarError::UnknownName {
                    offset: at,
                    name: name.to_owned(),
                });
            }
        };
        if layouts[index].is_some() {
            return Err(GrammarError::DuplicateName {
                offset: at,
                name: format!("layout {name}"),
            });
        }
        let mut expr = &productions[index].expr;
        while let Expr::Reference(id) = expr {
            expr = &productions[id.index()].expr;
        }
        let Expr::Class(class) = expr else {
            return Err(invalid("only a class production has a layout"));
        };
        charge(
            builder.governor,
            size(text.len()).saturating_add(1),
            size(text.len()),
        )?;
        if !builder.classes[class.index()].accepts(text, builder.governor)? {
            return Err(invalid("the layout text is not a member of the class"));
        }
        layouts[index] = Some(text.to_vec());
    }
    Ok(layouts)
}
