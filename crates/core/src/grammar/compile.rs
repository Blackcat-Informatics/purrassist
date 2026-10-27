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
    DelimiterSet, DelimiterSetId, Expr, ExprChild, ExprItems, GrammarError, Literal, LiteralId,
    Production, ProductionId, index_u32,
};
use crate::contract::saturating_size as size;
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchString, ScratchVec, format_scratch_governed,
};
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
    /// For a grouped program, the literal instructions after which one call
    /// ends (the production completes or the next call begins, without
    /// input): where an attempt that could not read a call resynchronizes
    /// (see the engine's resynchronization); empty otherwise.
    pub(crate) call_closers: Vec<u32>,
    /// For a grouped program, the literal instructions a call after a
    /// `next` begins with (reached from the `next` without input): where an
    /// attempt resynchronizes at the next call when the call it could not
    /// read has no closer; empty otherwise.
    pub(crate) call_openers: Vec<u32>,
    /// Per `insert` instruction's literal, the literals the program reads
    /// at the same input position instead (the alternatives of the
    /// insertion: the token written, an optional segment's literal before
    /// it); the engine never reads a token as omitted where one of them is
    /// written.
    pub(crate) insert_alternatives: Vec<(u32, Vec<u32>)>,
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
#[derive(Debug, PartialEq, Eq)]
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
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Spellings {
    pub(crate) list: ByteList,
    pub(crate) trie: Trie,
}

impl Spellings {
    fn build(list: ByteList, governor: &mut Governor) -> Result<Self, GrammarError> {
        symbol_index(list.len(), governor)?;
        let trie = Trie::build(
            list.iter()
                .enumerate()
                .map(|(index, bytes)| (bytes.as_slice(), index_u32(index))),
            governor,
        )?;
        governor.check()?;
        Ok(Self { list, trie })
    }

    fn bytes(&self) -> usize {
        self.list.iter().map(Bytes::len).sum()
    }
}
impl ScratchCharge for LiteralTables {
    const CHARGE: usize = 400;
}
impl ScratchCharge for DelimiterSet {
    // Includes the existing dense matcher's handle, whose buffers remain separate.
    const CHARGE: usize = 120;
}
const _: () = {
    assert!(core::mem::size_of::<LiteralTables>() <= LiteralTables::CHARGE);
    assert!(core::mem::size_of::<DelimiterSet>() <= DelimiterSet::CHARGE);
};

type Bytes = ScratchVec<u8>;
type ByteList = ScratchVec<Bytes>;
type BytePairs = ScratchVec<(Bytes, Bytes)>;

struct Symbol {
    name: ScratchString,
    target: Name,
}
impl ScratchCharge for Symbol {
    const CHARGE: usize = 48;
}
struct Body<'a> {
    name: ScratchString,
    expr: &'a PExpr,
}
impl ScratchCharge for Body<'_> {
    const CHARGE: usize = 48;
}
struct Layout<'a> {
    name: &'a str,
    text: &'a [u8],
    at: usize,
}
impl ScratchCharge for Layout<'_> {
    const CHARGE: usize = 40;
}
impl ScratchCharge for Option<Bytes> {
    const CHARGE: usize = 48;
}
impl ScratchCharge for Literal {
    const CHARGE: usize = 48;
}
impl ScratchCharge for CloserClass {
    const CHARGE: usize = 80;
}
impl ScratchCharge for ClassTerminal {
    // Source owner (40), two compile-arena vectors (48), root/padding (8), two vectors (48).
    const CHARGE: usize = 144;
}
impl ScratchCharge for Production {
    const CHARGE: usize = 104;
}
const _: () = {
    assert!(core::mem::size_of::<Symbol>() <= Symbol::CHARGE);
    assert!(core::mem::size_of::<Literal>() <= Literal::CHARGE);
    assert!(core::mem::size_of::<CloserClass>() <= CloserClass::CHARGE);
    assert!(core::mem::size_of::<ClassTerminal>() <= ClassTerminal::CHARGE);
    assert!(core::mem::size_of::<Production>() <= Production::CHARGE);
};
struct Builder<'g> {
    governor: &'g mut Governor,
    literals: ScratchVec<Literal>,
    closers: ScratchVec<CloserClass>,
    classes: ScratchVec<ClassTerminal>,
    delimiter_sets: ScratchVec<ByteList>,
    names: ScratchVec<Symbol>,
    production_offsets: ScratchVec<usize>,
}
#[derive(Clone, Copy)]
enum Name {
    Production(u32),
    Closer(u32),
}

// Later backend tables retain their existing coarser accounting contract.
fn charge(governor: &mut Governor, work: u64, scratch: u64) -> Result<(), GrammarError> {
    governor.charge(Resource::Work, work.saturating_mul(K_COMPILE))?;
    governor.charge(Resource::ScratchBytes, scratch)?;
    Ok(())
}
fn visit(depth: u64, governor: &mut Governor) -> Result<(), GrammarError> {
    if depth > 128 {
        return Err(governor.stop_resource(Resource::Depth).into());
    }
    governor.charge(Resource::Depth, depth)?;
    governor.charge(Resource::Work, K_COMPILE)?;
    Ok(())
}
fn symbol_index(index: usize, governor: &mut Governor) -> Result<u32, GrammarError> {
    u32::try_from(index).map_err(|_| governor.stop_resource(Resource::ScratchBytes).into())
}
fn work_for(
    length: usize,
    factor: u64,
    extra: u64,
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    let work = u64::try_from(length)
        .ok()
        .and_then(|n| n.checked_mul(factor))
        .and_then(|n| n.checked_add(extra))
        .ok_or_else(|| governor.stop_resource(Resource::Work))?;
    governor.charge(Resource::Work, work)?;
    Ok(())
}
fn owned_bytes(bytes: &[u8], g: &mut Governor) -> Result<Bytes, GrammarError> {
    g.charge(Resource::InputBytes, size(bytes.len()))?;
    work_for(bytes.len(), 1, 1, g)?;
    let mut output = ScratchVec::new(g);
    output.extend(bytes.iter().copied())?;
    Ok(output)
}
fn owned_text(text: &str, g: &mut Governor) -> Result<ScratchString, GrammarError> {
    g.charge(Resource::InputBytes, size(text.len()))?;
    work_for(text.len(), 1, 1, g)?;
    let mut output = ScratchString::new(g);
    output.push_str(text)?;
    Ok(output)
}
fn equal(left: &[u8], right: &[u8], g: &mut Governor) -> Result<bool, GrammarError> {
    work_for(left.len().min(right.len()), 1, 1, g)?;
    Ok(left == right)
}
fn diagnostic(text: &str, g: &mut Governor) -> Result<String, GrammarError> {
    let owned = owned_text(text, g)?;
    g.check()?;
    Ok(owned.into_string())
}
impl<'g> Builder<'g> {
    fn new(governor: &'g mut Governor) -> Self {
        Self {
            literals: ScratchVec::new(governor),
            closers: ScratchVec::new(governor),
            classes: ScratchVec::new(governor),
            delimiter_sets: ScratchVec::new(governor),
            names: ScratchVec::new(governor),
            production_offsets: ScratchVec::new(governor),
            governor,
        }
    }
}
fn intern_literal(
    literals: &mut ScratchVec<Literal>,
    bytes: &[u8],
    closer: Option<CloserId>,
    governor: &mut Governor,
) -> Result<u32, GrammarError> {
    for (index, literal) in literals.iter().enumerate() {
        if equal(&literal.bytes, bytes, governor)? && literal.closer == closer {
            return symbol_index(index, governor);
        }
    }
    let index = symbol_index(literals.len(), governor)?;
    let bytes = owned_bytes(bytes, governor)?;
    literals.push(Literal { bytes, closer })?;
    Ok(index)
}

impl Builder<'_> {
    fn intern_delimiters(&mut self, literals: &[Bytes]) -> Result<u32, GrammarError> {
        for (index, set) in self.delimiter_sets.iter().enumerate() {
            self.governor.charge(Resource::Work, 1)?;
            if set.len() != literals.len() {
                continue;
            }
            let mut same = true;
            for (left, right) in set.iter().zip(literals) {
                if !equal(left, right, self.governor)? {
                    same = false;
                    break;
                }
            }
            if same {
                return symbol_index(index, self.governor);
            }
        }
        let index = symbol_index(self.delimiter_sets.len(), self.governor)?;
        let mut set = ScratchVec::new(self.governor);
        for literal in literals {
            set.push(owned_bytes(literal, self.governor)?)?;
        }
        self.delimiter_sets.push(set)?;
        Ok(index)
    }
    fn intern_class(&mut self, source: &str, at: usize) -> Result<u32, GrammarError> {
        for (index, class) in self.classes.iter().enumerate() {
            if equal(class.source.as_bytes(), source.as_bytes(), self.governor)? {
                return symbol_index(index, self.governor);
            }
        }
        let index = symbol_index(self.classes.len(), self.governor)?;
        let source_copy = owned_text(source, self.governor)?;
        work_for(source.len(), 1, 128, self.governor)?;
        let anchored = format_scratch_governed(format_args!("^(?:{source})$"), self.governor)?;
        // Native pattern parsing/automaton preparation remains a separate contract.
        let pattern = match Pattern::compile_governed(anchored.as_str(), self.governor) {
            Ok(pattern) => pattern,
            Err(PatternCompileError::Resource(reason)) => return Err(reason.into()),
            Err(PatternCompileError::Syntax(error)) => {
                work_for(source.len(), 64, 128, self.governor)?;
                let message =
                    format_scratch_governed(format_args!("{error}"), self.governor)?.into_string();
                return Err(GrammarError::InvalidClass {
                    offset: at,
                    message,
                });
            }
        };
        let Some(automaton) = pattern.into_automaton() else {
            return Err(GrammarError::InvalidClass {
                offset: at,
                message: diagnostic(
                    "backreferences and lookaround are not finite-state",
                    self.governor,
                )?,
            });
        };
        let mut dfa = automaton.dfa(self.governor)?;
        let start = dfa.start();
        if dfa.accepting(start, self.governor)? {
            return Err(GrammarError::InvalidClass {
                offset: at,
                message: diagnostic(
                    "a class must not match the empty string; use `?` or `*`",
                    self.governor,
                )?,
            });
        }
        drop(dfa);
        self.classes.push(ClassTerminal {
            source: source_copy,
            automaton,
        })?;
        Ok(index)
    }
    fn lookup(&mut self, name: &str) -> Result<Option<Name>, GrammarError> {
        for symbol in &self.names {
            if equal(symbol.name.as_bytes(), name.as_bytes(), self.governor)? {
                return Ok(Some(symbol.target));
            }
        }
        Ok(None)
    }
    fn resolve(&mut self, expr: &PExpr) -> Result<Expr, GrammarError> {
        self.resolve_at(expr, 0)
    }
    fn resolve_child(&mut self, expr: &PExpr, depth: u64) -> Result<ExprChild, GrammarError> {
        let value = self.resolve_at(expr, depth)?;
        let mut values = ScratchVec::new(self.governor);
        values.push(value)?;
        Ok(ExprChild(values))
    }
    fn resolve_at(&mut self, expr: &PExpr, depth: u64) -> Result<Expr, GrammarError> {
        visit(depth, self.governor)?;
        let child = depth + 1;
        Ok(match expr {
            PExpr::Literal { bytes, .. } => Expr::Literal(LiteralId(intern_literal(
                &mut self.literals,
                bytes,
                None,
                self.governor,
            )?)),
            PExpr::Insert { bytes } => Expr::Insert(LiteralId(intern_literal(
                &mut self.literals,
                bytes,
                None,
                self.governor,
            )?)),
            PExpr::Class { source, at } => {
                Expr::Class(ClassId(self.intern_class(source.as_str(), *at)?))
            }
            PExpr::Until { literals, .. } => {
                Expr::Until(DelimiterSetId(self.intern_delimiters(literals)?))
            }
            PExpr::Slot {
                codec,
                codec_at,
                literals,
                ..
            } => {
                work_for(codec.len(), 4, 4, self.governor)?;
                let Some(codec_kind) = CodecKind::from_name(codec.as_str()) else {
                    return Err(GrammarError::UnknownCodec {
                        offset: *codec_at,
                        name: diagnostic(codec.as_str(), self.governor)?,
                    });
                };
                Expr::Slot {
                    codec: codec_kind,
                    delimiters: DelimiterSetId(self.intern_delimiters(literals)?),
                }
            }
            PExpr::Name { name, at } => match self.lookup(name.as_str())? {
                Some(Name::Production(index)) => Expr::Reference(ProductionId(index)),
                Some(Name::Closer(index)) => Expr::Closer(CloserId(index)),
                None => {
                    return Err(GrammarError::UnknownName {
                        offset: *at,
                        name: diagnostic(name.as_str(), self.governor)?,
                    });
                }
            },
            PExpr::Capture { kind, at, inner } => {
                work_for(kind.len(), 8, 8, self.governor)?;
                let Some(kind) = CaptureKind::from_name(kind.as_str()) else {
                    return Err(GrammarError::UnknownCapture {
                        offset: *at,
                        name: diagnostic(kind.as_str(), self.governor)?,
                    });
                };
                if matches!(kind, CaptureKind::Arguments | CaptureKind::ParamValue)
                    && !matches!(inner.get(), PExpr::Slot { .. })
                {
                    return Err(GrammarError::CaptureTarget {
                        offset: *at,
                        capture: kind.name(),
                    });
                }
                Expr::Capture {
                    kind,
                    inner: self.resolve_child(inner.get(), child)?,
                }
            }
            PExpr::Sequence(items, _, _) | PExpr::Choice(items, _, _) => {
                let mut values = ScratchVec::new(self.governor);
                for item in items {
                    values.push(self.resolve_at(item, child)?)?;
                }
                if matches!(expr, PExpr::Sequence(..)) {
                    Expr::Sequence(ExprItems(values))
                } else {
                    Expr::Choice(ExprItems(values))
                }
            }
            PExpr::Commit => Expr::Commit,
            PExpr::Next { .. } => Expr::Next,
            PExpr::Repeat { inner, op, .. } => {
                let (min, max) = match op {
                    b'*' => (0, None),
                    b'+' => (1, None),
                    _ => (0, Some(1)),
                };
                Expr::Repeat {
                    inner: self.resolve_child(inner.get(), child)?,
                    min,
                    max,
                }
            }
        })
    }
}

/// Check that parameter captures appear only as adjacent
/// `param-name`/`param-value` pairs among one sequence's items.
fn check_pairs(
    expr: &PExpr,
    in_sequence: bool,
    depth: u64,
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    visit(depth, governor)?;
    let child = depth + 1;
    match expr {
        PExpr::Capture { kind, at, inner } => {
            work_for(kind.len(), 2, 2, governor)?;
            let parameter = matches!(kind.as_str(), "param-name" | "param-value");
            if parameter && !in_sequence {
                return Err(GrammarError::UnpairedParameter { offset: *at });
            }
            check_pairs(inner.get(), false, child, governor)
        }
        PExpr::Sequence(items, _, _) => {
            let mut expect_value: Option<usize> = None;
            for item in items.as_slice() {
                if let PExpr::Capture { kind, at, .. } = item {
                    work_for(kind.len(), 2, 2, governor)?;
                    match (kind.as_str(), expect_value) {
                        ("param-name", None) => expect_value = Some(*at),
                        ("param-value", Some(_)) => expect_value = None,
                        ("param-name" | "param-value", _) => {
                            return Err(GrammarError::UnpairedParameter { offset: *at });
                        }
                        _ => {}
                    }
                }
                check_pairs(item, true, child, governor)?;
            }
            expect_value.map_or(Ok(()), |offset| {
                Err(GrammarError::UnpairedParameter { offset })
            })
        }
        PExpr::Choice(items, _, _) => items
            .as_slice()
            .iter()
            .try_for_each(|item| check_pairs(item, false, child, governor)),
        PExpr::Repeat { inner, .. } => check_pairs(inner.get(), false, child, governor),
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

fn references(
    expr: &Expr,
    into: &mut ScratchVec<u32>,
    depth: u64,
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    visit(depth, governor)?;
    match expr {
        Expr::Reference(id) => into.push(id.0)?,
        Expr::Capture { inner, .. } | Expr::Repeat { inner, .. } => {
            references(inner, into, depth + 1, governor)?;
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            for item in items {
                references(item, into, depth + 1, governor)?;
            }
        }
        _ => {}
    }
    Ok(())
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
                name: diagnostic(production.name.as_str(), governor)?,
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
struct ReferenceFrame {
    node: usize,
    edges: ScratchVec<u32>,
    next: usize,
}
impl ScratchCharge for ReferenceFrame {
    const CHARGE: usize = 56;
}
const _: () = assert!(core::mem::size_of::<ReferenceFrame>() <= ReferenceFrame::CHARGE);

// Compute expanded height in the same postorder admission: all referenced
// targets are finished, including DAG tails visited from an earlier root.
fn expanded_height(
    expr: &Expr,
    heights: &[u64],
    depth: u64,
    governor: &mut Governor,
) -> Result<u64, GrammarError> {
    visit(depth, governor)?;
    let below = match expr {
        Expr::Reference(id) => heights[id.index()],
        Expr::Capture { inner, .. } | Expr::Repeat { inner, .. } => {
            expanded_height(inner, heights, depth + 1, governor)?
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            let mut tallest = 0;
            for item in items {
                tallest = tallest.max(expanded_height(item, heights, depth + 1, governor)?);
            }
            tallest
        }
        _ => 0,
    };
    let height = below
        .checked_add(1)
        .ok_or_else(|| governor.stop_resource(Resource::Depth))?;
    visit(height, governor)?;
    Ok(height)
}
fn check_acyclic(
    productions: &[Production],
    offsets: &[usize],
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    let mut color = ScratchVec::new(governor);
    let mut heights = ScratchVec::new(governor);
    governor.charge(Resource::Work, size(productions.len()).saturating_mul(2))?;
    color.extend(core::iter::repeat_n(0_u8, productions.len()))?;
    heights.extend(core::iter::repeat_n(0_u64, productions.len()))?;
    for root in 0..productions.len() {
        visit(0, governor)?;
        if color[root] != 0 {
            continue;
        }
        let mut stack = ScratchVec::new(governor);
        let mut edges = ScratchVec::new(governor);
        references(&productions[root].expr, &mut edges, 0, governor)?;
        color.as_mut_slice()[root] = 1;
        stack.push(ReferenceFrame {
            node: root,
            edges,
            next: 0,
        })?;
        while !stack.is_empty() {
            let path_depth = size(stack.len());
            visit(path_depth, governor)?;
            let frame = stack
                .as_mut_slice()
                .last_mut()
                .expect("nonempty reference path");
            if let Some(&target) = frame.edges.get(frame.next) {
                frame.next += 1;
                let target = target as usize;
                match color[target] {
                    1 => {
                        return Err(GrammarError::Recursive {
                            offset: offsets[target],
                            name: diagnostic(productions[target].name.as_str(), governor)?,
                        });
                    }
                    0 => {
                        visit(path_depth + 1, governor)?;
                        color.as_mut_slice()[target] = 1;
                        let mut edges = ScratchVec::new(governor);
                        references(&productions[target].expr, &mut edges, 0, governor)?;
                        stack.push(ReferenceFrame {
                            node: target,
                            edges,
                            next: 0,
                        })?;
                    }
                    _ => {}
                }
            } else {
                let node = frame.node;
                let height = expanded_height(&productions[node].expr, &heights, 0, governor)?;
                heights.as_mut_slice()[node] = height;
                color.as_mut_slice()[node] = 2;
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
fn contains_bytes(list: &[Bytes], bytes: &[u8], g: &mut Governor) -> Result<bool, GrammarError> {
    for known in list {
        if equal(known, bytes, g)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn append_bytes(output: &mut Bytes, bytes: &[u8], g: &mut Governor) -> Result<(), GrammarError> {
    work_for(bytes.len(), 1, 1, g)?;
    output.extend(bytes.iter().copied())?;
    Ok(())
}
fn confusable_spellings(
    bytes: &[u8],
    confusables: &[(Bytes, Bytes)],
    governor: &mut Governor,
) -> Result<ByteList, GrammarError> {
    let mut present = ScratchVec::new(governor);
    for (index, (canonical, _)) in confusables.iter().enumerate() {
        work_for(bytes.len(), size(canonical.len()).max(1), 1, governor)?;
        if bytes
            .windows(canonical.len())
            .any(|window| window == canonical.as_slice())
        {
            present.push(index)?;
        }
    }
    let mut spellings = ScratchVec::new(governor);
    if present.is_empty() {
        return Ok(spellings);
    }
    let subsets = u32::try_from(present.len())
        .ok()
        .and_then(|n| 1_u64.checked_shl(n))
        .ok_or_else(|| governor.stop_resource(Resource::Work))?;
    for mask in 1..subsets {
        governor.charge(Resource::Work, 1)?;
        let mut spelling = ScratchVec::new(governor);
        let mut index = 0;
        'scan: while index < bytes.len() {
            governor.charge(Resource::Work, 1)?;
            for (bit, &pair) in present.iter().enumerate() {
                governor.charge(Resource::Work, 1)?;
                let (canonical, variant) = &confusables[pair];
                if mask & (1 << bit) != 0 {
                    work_for(canonical.len(), 1, 1, governor)?;
                    if bytes[index..].starts_with(canonical) {
                        append_bytes(&mut spelling, variant, governor)?;
                        index += canonical.len();
                        continue 'scan;
                    }
                }
            }
            spelling.push(bytes[index])?;
            index += 1;
        }
        if !equal(&spelling, bytes, governor)? && !contains_bytes(&spellings, &spelling, governor)?
        {
            spellings.push(spelling)?;
        }
    }
    governor.check()?;
    Ok(spellings)
}

/// Existing JSON-syntax spellings, retaining every temporary until publication.
fn json_spellings(
    bytes: &[u8],
    key_at_end: bool,
    governor: &mut Governor,
) -> Result<[ByteList; 3], GrammarError> {
    // All scans below advance through disjoint words/whitespace runs. This
    // bound includes quote detection, word tests and every copied byte.
    work_for(bytes.len(), 8, 1, governor)?;
    let mut string = ScratchVec::new(governor);
    if bytes.contains(&b'"') {
        let mut spelling = ScratchVec::new(governor);
        spelling.extend(bytes.iter().map(|&b| if b == b'"' { b'\'' } else { b }))?;
        string.push(spelling)?;
    }
    let mut bare = ScratchVec::new(governor);
    let mut keys = false;
    let mut colonless = ScratchVec::new(governor);
    let mut colons = false;
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
                    bare.extend(word.iter().copied())?;
                    keys = true;
                } else {
                    bare.extend(bytes[index..after].iter().copied())?;
                }
                colonless.extend(bytes[index..after].iter().copied())?;
                index = after;
                continue;
            }
        }
        if byte == b':' && bytes.len() > 1 {
            colons = true;
        } else {
            colonless.push(byte)?;
        }
        bare.push(byte)?;
        index += 1;
    }
    let mut bare_list = ScratchVec::new(governor);
    if keys {
        bare_list.push(bare)?;
    }
    let mut colon_list = ScratchVec::new(governor);
    if colons && !colonless.is_empty() {
        colon_list.push(colonless)?;
    }
    governor.check()?;
    Ok([string, bare_list, colon_list])
}

fn literal_tables(
    literal: &Literal,
    closers: &[CloserClass],
    confusables: &[(Bytes, Bytes)],
    json: Option<bool>,
    governor: &mut Governor,
) -> Result<LiteralTables, GrammarError> {
    let mut variants = ScratchVec::new(governor);
    if let Some(closer) = literal.closer {
        for variant in closers[closer.index()].spellings.iter().skip(1) {
            if !equal(variant, &literal.bytes, governor)?
                && !contains_bytes(&variants, variant, governor)?
            {
                variants.push(owned_bytes(variant, governor)?)?;
            }
        }
    }
    let mut generated = confusable_spellings(&literal.bytes, confusables, governor)?;
    work_for(generated.len(), 1, 1, governor)?;
    generated.as_mut_slice().reverse();
    let mut spellings = ScratchVec::new(governor);
    while let Some(spelling) = generated.pop() {
        if !contains_bytes(&variants, &spelling, governor)? {
            spellings.push(spelling)?;
        }
    }
    let mut json_tables: [ByteList; 3] = core::array::from_fn(|_| ScratchVec::new(governor));
    if let Some(key_at_end) = json {
        for (index, mut generated) in json_spellings(&literal.bytes, key_at_end, governor)?
            .into_iter()
            .enumerate()
        {
            work_for(generated.len(), 1, 1, governor)?;
            generated.as_mut_slice().reverse();
            while let Some(spelling) = generated.pop() {
                let mut seen = equal(&literal.bytes, &spelling, governor)?
                    || contains_bytes(&variants, &spelling, governor)?
                    || contains_bytes(&spellings, &spelling, governor)?;
                for table in &json_tables {
                    seen |= contains_bytes(table, &spelling, governor)?;
                }
                if !seen {
                    json_tables[index].push(spelling)?;
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

/// Prepare the same repair-table owners for ordinary and JSON-envelope grammars.
fn literal_repairs(
    literals: &[Literal],
    closers: &[CloserClass],
    confusables: &[(Bytes, Bytes)],
    json_keys: Option<&[bool]>,
    governor: &mut Governor,
) -> Result<ScratchVec<LiteralTables>, GrammarError> {
    let mut tables = ScratchVec::new(governor);
    for (index, literal) in literals.iter().enumerate() {
        governor.charge(Resource::Work, 1)?;
        let json = json_keys.map(|keys| keys[index]);
        tables.push(literal_tables(
            literal,
            closers,
            confusables,
            json,
            governor,
        )?)?;
    }
    governor.check()?;
    Ok(tables)
}

/// Each literal's key flag, and per program the key uses `(pc, copy)` to
/// point at a literal's key copy (see `split_keys`).
type KeySplit = (ScratchVec<bool>, Renames);

/// Per program, the `Lit` instructions (`pc`) to point at another literal.
type Renames = ScratchVec<ScratchVec<(u32, u32)>>;

/// The literal tables of a `json-envelope` grammar, JSON spellings
/// included, over `literals` with their key copies (see `split_keys`), and
/// the key uses to point at the copies.
fn json_tables(
    programs: &[Program],
    literals: &mut ScratchVec<Literal>,
    closers: &[CloserClass],
    confusables: &[(Bytes, Bytes)],
    governor: &mut Governor,
) -> Result<(ScratchVec<LiteralTables>, Renames), GrammarError> {
    let (keys, renames) = split_keys(programs, literals, governor)?;
    let tables = literal_repairs(literals, closers, confusables, Some(&keys), governor)?;
    governor.check()?;
    Ok((tables, renames))
}

/// Each `Lit` use of each program, `(pc, literal, key)`: whether the use
/// is followed by a literal beginning with `:` (optional class runs, such
/// as whitespace, between), as a member key is.
struct KeyUse {
    pc: u32,
    lit: u32,
    key: bool,
}
impl ScratchCharge for KeyUse {
    const CHARGE: usize = 12;
}
struct KeyCopy(Option<u32>);
impl ScratchCharge for KeyCopy {
    const CHARGE: usize = 8;
}
const _: () = {
    assert!(core::mem::size_of::<KeyUse>() <= KeyUse::CHARGE);
    assert!(core::mem::size_of::<KeyCopy>() <= KeyCopy::CHARGE);
};
fn key_uses(
    programs: &[Program],
    literals: &[Literal],
    governor: &mut Governor,
) -> Result<ScratchVec<ScratchVec<KeyUse>>, GrammarError> {
    let mut per_program = ScratchVec::new(governor);
    for program in programs {
        let mut uses = ScratchVec::new(governor);
        for (pc, inst) in program.insts.iter().enumerate() {
            governor.charge(Resource::Work, 1)?;
            let Inst::Lit { lit, next } = *inst else {
                continue;
            };
            let mut colon = true;
            let mut any = false;
            work_for(program.insts.len(), 1, 1, governor)?;
            let mut seen = ScratchVec::new(governor);
            seen.extend(core::iter::repeat_n(false, program.insts.len()))?;
            let mut stack = ScratchVec::new(governor);
            stack.push(next)?;
            while let Some(pc) = stack.pop() {
                governor.charge(Resource::Work, 1)?;
                if core::mem::replace(&mut seen.as_mut_slice()[pc as usize], true) {
                    continue;
                }
                match program.insts[pc as usize] {
                    Inst::Split(first, second) => stack.extend([first, second])?,
                    Inst::Open { next, .. }
                    | Inst::Close { next, .. }
                    | Inst::Commit { next }
                    | Inst::Next { next }
                    | Inst::Insert { next, .. }
                    | Inst::Class { next, .. } => stack.push(next)?,
                    Inst::Lit { lit, .. } => {
                        any = true;
                        colon &= literals[lit as usize].bytes.first() == Some(&b':');
                    }
                    Inst::Until { .. } | Inst::Slot { .. } | Inst::Accept => colon = false,
                }
            }
            uses.push(KeyUse {
                pc: symbol_index(pc, governor)?,
                lit,
                key: any && colon,
            })?;
        }
        per_program.push(uses)?;
    }
    Ok(per_program)
}

/// The member-key literals of a `json-envelope` grammar: a literal every
/// use of which is a key is one, and a literal used both as a key and
/// otherwise (`"function"`, the key of `OpenAI`'s function object and the
/// value of its `type`) gets a copy, appended to `literals`, for its key
/// uses. Returns each literal's key flag and, per program, the key uses
/// (`pc`, copy) to point at the copies.
fn split_keys(
    programs: &[Program],
    literals: &mut ScratchVec<Literal>,
    governor: &mut Governor,
) -> Result<KeySplit, GrammarError> {
    let uses = key_uses(programs, literals, governor)?;
    work_for(literals.len(), 3, 1, governor)?;
    let mut counts = ScratchVec::new(governor);
    counts.extend(core::iter::repeat_n((0_u32, 0_u32), literals.len()))?;
    for program in &uses {
        for key_use in program {
            governor.charge(Resource::Work, 1)?;
            let count = &mut counts.as_mut_slice()[key_use.lit as usize];
            count.0 = count
                .0
                .checked_add(1)
                .ok_or_else(|| governor.stop_resource(Resource::Work))?;
            count.1 = count
                .1
                .checked_add(u32::from(key_use.key))
                .ok_or_else(|| governor.stop_resource(Resource::Work))?;
        }
    }
    let mut keys = ScratchVec::new(governor);
    keys.extend(counts.iter().map(|&(all, keyed)| all > 0 && all == keyed))?;
    let mut copies = ScratchVec::new(governor);
    copies.extend(core::iter::repeat_with(|| KeyCopy(None)).take(literals.len()))?;
    let mut per_program = ScratchVec::new(governor);
    for program in &uses {
        let mut moved = ScratchVec::new(governor);
        for &KeyUse { pc, lit, key } in program {
            governor.charge(Resource::Work, 1)?;
            let (all, keyed) = counts[lit as usize];
            if !key || keyed == all {
                continue;
            }
            let copy = if let Some(copy) = copies[lit as usize].0 {
                copy
            } else {
                // Key uses deliberately need a distinct derived table identity.
                let copy_bytes = owned_bytes(&literals[lit as usize].bytes, governor)?;
                let copy_closer = literals[lit as usize].closer;
                let copy = symbol_index(literals.len(), governor)?;
                literals.push(Literal {
                    bytes: copy_bytes,
                    closer: copy_closer,
                })?;
                keys.push(true)?;
                copies.as_mut_slice()[lit as usize].0 = Some(copy);
                copy
            };
            moved.push((pc, copy))?;
        }
        per_program.push(moved)?;
    }
    governor.check()?;
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
    let openers = epsilon_closure(&insts, start);
    let mut first = [0_u64; 4];
    for &pc in &openers {
        let Inst::Lit { lit, .. } = insts[pc as usize] else {
            return Err(GrammarError::InvalidOpener {
                offset,
                production: diagnostic(productions[root.index()].name.as_str(), governor)?,
            });
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
    let call_closers = call_closer_lits(&insts, grouped, governor)?;
    let call_openers = call_opener_lits(&insts, governor)?;
    let insert_alternatives = insert_alternatives(&insts, governor)?;
    Ok(Program {
        insts,
        start,
        openers,
        first,
        bound,
        committed,
        start_closure,
        grouped,
        call_closers,
        call_openers,
        insert_alternatives,
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
        renames: &[ScratchVec<(u32, u32)>],
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
                renames.get(index).map_or(&[][..], ScratchVec::as_slice),
                governor,
            )
        };
        Ok([program(0)?, program(1)?])
    }
}

/// The non-production statements of a document, and its production bodies
/// in order (registered in the builder's namespace).
struct Declared<'d> {
    reasoning: BytePairs,
    explicit_reasoning: ScratchVec<bool>,
    confusables: BytePairs,
    bodies: ScratchVec<Body<'d>>,
    layouts: ScratchVec<Layout<'d>>,
    optional_id: bool,
    declaration_content: bool,
    json_envelope: bool,
}
impl Declared<'_> {
    fn new(governor: &Governor) -> Self {
        Self {
            reasoning: ScratchVec::new(governor),
            explicit_reasoning: ScratchVec::new(governor),
            confusables: ScratchVec::new(governor),
            bodies: ScratchVec::new(governor),
            layouts: ScratchVec::new(governor),
            optional_id: false,
            declaration_content: false,
            json_envelope: false,
        }
    }
}
fn duplicate(offset: usize, name: &str, g: &mut Governor) -> Result<GrammarError, GrammarError> {
    Ok(GrammarError::DuplicateName {
        offset,
        name: diagnostic(name, g)?,
    })
}
fn declare<'d>(
    builder: &mut Builder<'_>,
    statements: &'d [Statement],
) -> Result<Declared<'d>, GrammarError> {
    let mut declared = Declared::new(builder.governor);
    for statement in statements {
        visit(0, builder.governor)?;
        match statement {
            Statement::Reasoning {
                open,
                close,
                explicit,
                at,
            } => {
                for (left, right) in &declared.reasoning {
                    if equal(left, open, builder.governor)?
                        && equal(right, close, builder.governor)?
                    {
                        return Err(duplicate(*at, "reasoning", builder.governor)?);
                    }
                }
                declared.reasoning.push((
                    owned_bytes(open, builder.governor)?,
                    owned_bytes(close, builder.governor)?,
                ))?;
                declared.explicit_reasoning.push(*explicit)?;
            }
            Statement::OptionalId { at } => {
                if declared.optional_id {
                    return Err(duplicate(*at, "optional-id", builder.governor)?);
                }
                declared.optional_id = true;
            }
            Statement::DeclarationContent { at } => {
                if declared.declaration_content {
                    return Err(duplicate(*at, "declaration-content", builder.governor)?);
                }
                declared.declaration_content = true;
            }
            Statement::JsonEnvelope { at } => {
                if declared.json_envelope {
                    return Err(duplicate(*at, "json-envelope", builder.governor)?);
                }
                declared.json_envelope = true;
            }
            Statement::Layout { name, text, at } => declared.layouts.push(Layout {
                name: name.as_str(),
                text,
                at: *at,
            })?,
            Statement::Confusable {
                canonical, variant, ..
            } => {
                declared.confusables.push((
                    owned_bytes(canonical, builder.governor)?,
                    owned_bytes(variant, builder.governor)?,
                ))?;
            }
            Statement::Closer {
                name,
                spellings,
                at,
            } => {
                if builder.lookup(name.as_str())?.is_some() {
                    return Err(duplicate(*at, name.as_str(), builder.governor)?);
                }
                let index = symbol_index(builder.closers.len(), builder.governor)?;
                builder.names.push(Symbol {
                    name: owned_text(name.as_str(), builder.governor)?,
                    target: Name::Closer(index),
                })?;
                let mut copies = ScratchVec::new(builder.governor);
                for spelling in spellings {
                    copies.push(owned_bytes(spelling, builder.governor)?)?;
                }
                builder.closers.push(CloserClass {
                    name: owned_text(name.as_str(), builder.governor)?,
                    spellings: copies,
                })?;
            }
            Statement::Production { name, expr, at } => {
                if builder.lookup(name.as_str())?.is_some() {
                    return Err(duplicate(*at, name.as_str(), builder.governor)?);
                }
                check_pairs(expr, false, 0, builder.governor)?;
                let index = symbol_index(declared.bodies.len(), builder.governor)?;
                builder.names.push(Symbol {
                    name: owned_text(name.as_str(), builder.governor)?,
                    target: Name::Production(index),
                })?;
                builder.production_offsets.push(*at)?;
                declared.bodies.push(Body {
                    name: owned_text(name.as_str(), builder.governor)?,
                    expr,
                })?;
            }
        }
    }
    Ok(declared)
}

/// Intern each closer class's canonical spelling as a closer-marked literal.
fn closer_literals(
    literals: &mut ScratchVec<Literal>,
    closers: &[CloserClass],
    governor: &mut Governor,
) -> Result<Vec<u32>, GrammarError> {
    let mut ids = Vec::with_capacity(closers.len());
    for (index, closer) in closers.iter().enumerate() {
        let id = Some(CloserId(symbol_index(index, governor)?));
        ids.push(intern_literal(
            literals,
            &closer.spellings[0],
            id,
            governor,
        )?);
    }
    Ok(ids)
}

/// Every closer spelling of every class: what a stray-closer deletion
/// recognizes.
fn stray_spellings(
    closers: &[CloserClass],
    productions: &[Production],
    governor: &mut Governor,
) -> Result<Spellings, GrammarError> {
    // XML parameter-body closers are also stray call closers.
    let mut xml = false;
    for production in productions {
        if xml_params(&production.expr, 0, governor)? {
            xml = true;
            break;
        }
    }
    let codec: &[&[u8]] = if xml {
        &[super::codec::XML_CLOSER, super::codec::XML_CLOSER_VARIANT]
    } else {
        &[]
    };
    let mut list = ScratchVec::new(governor);
    for spelling in closers
        .iter()
        .flat_map(|closer| closer.spellings.iter().map(Bytes::as_slice))
        .chain(codec.iter().copied())
    {
        if !contains_bytes(&list, spelling, governor)? {
            list.push(owned_bytes(spelling, governor)?)?;
        }
    }
    Spellings::build(list, governor)
}

/// Every spelling of a call opener and of a literal ending a call, any
/// alternative's (see `CodecContext::openers`), charged.
fn call_markup(
    call_ends: &[CallEnd],
    governor: &mut Governor,
) -> Result<[Vec<Vec<u8>>; 2], GrammarError> {
    let spelled: usize = call_ends
        .iter()
        .flat_map(|end| end.openers.iter().chain(&end.call_closers))
        .map(|spelling| spelling.len() + 24)
        .sum();
    charge(governor, size(spelled), size(spelled))?;
    let mut markup: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
    for end in call_ends {
        markup[0].extend(end.openers.iter().cloned());
        markup[1].extend(end.call_closers.iter().cloned());
    }
    for spellings in &mut markup {
        spellings.sort();
        spellings.dedup();
    }
    Ok(markup)
}

/// Whether `expr` holds an `xml-params` slot.
fn xml_params(expr: &Expr, depth: u64, governor: &mut Governor) -> Result<bool, GrammarError> {
    visit(depth, governor)?;
    Ok(match expr {
        Expr::Slot { codec, .. } => *codec == CodecKind::XmlParams,
        Expr::Capture { inner, .. } | Expr::Repeat { inner, .. } => {
            xml_params(inner, depth + 1, governor)?
        }
        Expr::Sequence(items) | Expr::Choice(items) => {
            let mut found = false;
            for item in items {
                if xml_params(item, depth + 1, governor)? {
                    found = true;
                    break;
                }
            }
            found
        }
        _ => false,
    })
}

/// Declared delimiters and governed expansion, around the separately prepared
/// dense matcher. Dense matcher buffers retain their existing accounting.
fn delimiter_sets(
    mut sets: ScratchVec<ByteList>,
    confusables: &[(Bytes, Bytes)],
    governor: &mut Governor,
) -> Result<ScratchVec<DelimiterSet>, GrammarError> {
    let mut delimiters = ScratchVec::new(governor);
    work_for(sets.len(), 1, 1, governor)?;
    sets.as_mut_slice().reverse();
    while let Some(declared) = sets.pop() {
        let mut expanded = ScratchVec::new(governor);
        for bytes in &declared {
            expanded.push(owned_bytes(bytes, governor)?)?;
        }
        for literal in &declared {
            let mut generated = confusable_spellings(literal, confusables, governor)?;
            work_for(generated.len(), 1, 1, governor)?;
            generated.as_mut_slice().reverse();
            while let Some(spelling) = generated.pop() {
                if !contains_bytes(&expanded, &spelling, governor)? {
                    expanded.push(spelling)?;
                }
            }
        }
        let matcher = Matcher::build(expanded.as_slice(), governor)?;
        delimiters.push(DelimiterSet { declared, matcher })?;
    }
    governor.check()?;
    Ok(delimiters)
}

/// The longest literal spelling, delimiter, or reasoning marker.
fn longest_literal(
    literals: &[Literal],
    repairs: &[LiteralTables],
    stray: &Spellings,
    delimiters: &[DelimiterSet],
    reasoning: &[(Bytes, Bytes)],
) -> usize {
    literals
        .iter()
        .map(|literal| literal.bytes.len())
        .chain(
            repairs
                .iter()
                .flat_map(|tables| {
                    tables
                        .variants
                        .list
                        .iter()
                        .chain(&tables.confusables.list)
                        .chain(tables.json.iter().flat_map(|json| &json.list))
                })
                .map(Bytes::len),
        )
        .chain(stray.list.iter().map(Bytes::len))
        .chain(delimiters.iter().map(|set| set.matcher.longest()))
        .chain(
            reasoning
                .iter()
                .flat_map(|(open, close)| [open.len(), close.len()]),
        )
        .max()
        .unwrap_or(0)
}

fn resolve_productions(
    builder: &mut Builder<'_>,
    bodies: ScratchVec<Body<'_>>,
) -> Result<ScratchVec<Production>, GrammarError> {
    let mut productions = ScratchVec::new(builder.governor);
    let mut bodies = bodies;
    builder
        .governor
        .charge(Resource::Work, size(bodies.len()))?;
    bodies.as_mut_slice().reverse();
    while let Some(Body { name, expr }) = bodies.pop() {
        let expr = builder.resolve(expr)?;
        productions.push(Production { name, expr })?;
    }
    Ok(productions)
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
    } = declare(&mut builder, document.statements.as_slice())?;
    let productions = resolve_productions(&mut builder, bodies)?;
    check_acyclic(&productions, &builder.production_offsets, builder.governor)?;
    let layouts = layouts(&mut builder, &productions, &declared_layouts)?;
    let mut required = |name: &'static str| -> Result<ProductionId, GrammarError> {
        for (index, production) in productions.iter().enumerate() {
            if equal(
                production.name.as_bytes(),
                name.as_bytes(),
                builder.governor,
            )? {
                return Ok(ProductionId(symbol_index(index, builder.governor)?));
            }
        }
        Err(GrammarError::MissingProduction { name })
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
    check_next(
        &productions,
        &production_offsets,
        [result, declaration],
        governor,
    )?;
    let closer_ids = closer_literals(&mut literals, &closers, governor)?;
    let mut literal_tables = literal_repairs(&literals, &closers, &confusables, None, governor)?;
    let stray = stray_spellings(&closers, &productions, governor)?;
    let delimiters = delimiter_sets(sets, &confusables, governor)?;
    let source = Programs {
        productions: &productions,
        closer_ids: &closer_ids,
        offsets: &production_offsets,
        stray_nodes: stray.bytes(),
        roots: [call, result],
    };
    let mut programs = source.compile(&literals, &literal_tables, &[], governor)?;
    if json_envelope {
        // The JSON spellings need to know which literals are member keys,
        // which the compiled programs tell; the programs are then compiled
        // again over the extended tables, key uses of a literal also used
        // otherwise pointing at its key copy.
        let renames;
        (literal_tables, renames) =
            json_tables(&programs, &mut literals, &closers, &confusables, governor)?;
        programs = source.compile(&literals, &literal_tables, &renames, governor)?;
    }
    let longest = longest_literal(&literals, &literal_tables, &stray, &delimiters, &reasoning);
    let call_ends = call_ends(&programs[0], &literals, &literal_tables, governor)?;
    let call_literals = call_literals(&programs[0], &literals, &literal_tables, governor)?;
    // An optional-id grammar reads model-added ids absent from its markup.
    let call_ids = !optional_id && captures(&productions, call, CaptureKind::Id);
    let call_markup = call_markup(&call_ends, governor)?;
    governor.check()?;
    Ok(CompiledGrammar {
        id: document.id.into_string(),
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
        call_markup,
        call_ends,
        call_literals,
        programs,
        stray: stray.trie,
        longest,
        layouts,
    })
}

/// The literal instructions a call after a `next` of `insts` begins with:
/// those reached from each `next` over zero-width instructions (not an
/// insertion).
fn call_opener_lits(insts: &[Inst], governor: &mut Governor) -> Result<Vec<u32>, GrammarError> {
    let mut openers: Vec<u32> = Vec::new();
    for inst in insts {
        let Inst::Next { next } = *inst else {
            continue;
        };
        charge(governor, size(insts.len()), 0)?;
        let mut seen = vec![false; insts.len()];
        let mut stack = vec![next];
        while let Some(pc) = stack.pop() {
            if core::mem::replace(&mut seen[pc as usize], true) {
                continue;
            }
            match insts[pc as usize] {
                Inst::Lit { .. } => {
                    if !openers.contains(&pc) {
                        openers.push(pc);
                    }
                }
                Inst::Split(first, second) => stack.extend([first, second]),
                Inst::Open { next, .. } | Inst::Close { next, .. } | Inst::Commit { next } => {
                    stack.push(next);
                }
                _ => {}
            }
        }
    }
    Ok(openers)
}

/// For a grouped program, the literal instructions after which one call
/// ends without input (the production completes, or a `next` begins the
/// next call); empty for a program without `next`.
fn call_closer_lits(
    insts: &[Inst],
    grouped: bool,
    governor: &mut Governor,
) -> Result<Vec<u32>, GrammarError> {
    if !grouped {
        return Ok(Vec::new());
    }
    charge(governor, size(insts.len()), 0)?;
    let ending = reach_back(
        insts,
        |inst| matches!(inst, Inst::Accept | Inst::Next { .. }),
        false,
    );
    Ok(insts
        .iter()
        .enumerate()
        .filter_map(|(pc, inst)| match *inst {
            Inst::Lit { next, .. } if ending[next as usize] => Some(index_u32(pc)),
            _ => None,
        })
        .collect())
}

/// Per literal an `insert` instruction of `insts` inserts, the literals read
/// at the same input position as the insertion: a backward walk from each
/// such instruction over the zero-width instructions (not `next`, not
/// another insertion) and a forward walk over them from every instruction
/// reached, collecting the literals met.
fn insert_alternatives(
    insts: &[Inst],
    governor: &mut Governor,
) -> Result<Vec<(u32, Vec<u32>)>, GrammarError> {
    let count = insts.len();
    let zero_width = |inst: &Inst| match *inst {
        Inst::Split(first, second) => vec![first, second],
        Inst::Open { next, .. } | Inst::Close { next, .. } | Inst::Commit { next } => vec![next],
        _ => Vec::new(),
    };
    let mut before: Vec<Vec<u32>> = vec![Vec::new(); count];
    for (pc, inst) in insts.iter().enumerate() {
        for next in zero_width(inst) {
            before[next as usize].push(index_u32(pc));
        }
    }
    let mut found: Vec<(u32, Vec<u32>)> = Vec::new();
    for (pc, inst) in insts.iter().enumerate() {
        let Inst::Insert { lit, .. } = *inst else {
            continue;
        };
        charge(governor, size(count), 0)?;
        let mut back = vec![false; count];
        let mut stack = vec![index_u32(pc)];
        while let Some(at) = stack.pop() {
            if !core::mem::replace(&mut back[at as usize], true) {
                stack.extend(before[at as usize].iter().copied());
            }
        }
        let mut seen = vec![false; count];
        let mut stack: Vec<u32> = (0..count)
            .filter(|&at| back[at] && at != pc)
            .map(index_u32)
            .collect();
        let mut lits = vec![lit];
        while let Some(at) = stack.pop() {
            if core::mem::replace(&mut seen[at as usize], true) {
                continue;
            }
            match insts[at as usize] {
                Inst::Lit { lit, .. } => {
                    if !lits.contains(&lit) {
                        lits.push(lit);
                    }
                }
                ref inst => stack.extend(zero_width(inst)),
            }
        }
        match found.iter_mut().find(|(known, _)| *known == lit) {
            Some((_, known)) => {
                for lit in lits {
                    if !known.contains(&lit) {
                        known.push(lit);
                    }
                }
            }
            None => found.push((lit, lits)),
        }
    }
    Ok(found)
}

/// The instructions `inst` continues at.
pub(crate) fn successors(inst: &Inst) -> Vec<u32> {
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
}

/// Per instruction of `program`, whether an instruction `seed` holds for
/// is reachable from it without input: one backward walk over the epsilon
/// instructions (`next` among them only when `through_next`).
fn reach_back(insts: &[Inst], seed: impl Fn(&Inst) -> bool, through_next: bool) -> Vec<bool> {
    let count = insts.len();
    let mut before: Vec<Vec<u32>> = vec![Vec::new(); count];
    let mut stack = Vec::new();
    for (pc, inst) in insts.iter().enumerate() {
        let pc = index_u32(pc);
        if seed(inst) {
            stack.push(pc);
        }
        match *inst {
            Inst::Split(first, second) => {
                before[first as usize].push(pc);
                before[second as usize].push(pc);
            }
            Inst::Next { next } if through_next => before[next as usize].push(pc),
            Inst::Open { next, .. }
            | Inst::Close { next, .. }
            | Inst::Commit { next }
            | Inst::Insert { next, .. } => before[next as usize].push(pc),
            _ => {}
        }
    }
    let mut reached = vec![false; count];
    while let Some(pc) = stack.pop() {
        if !core::mem::replace(&mut reached[pc as usize], true) {
            stack.extend(before[pc as usize].iter().copied());
        }
    }
    reached
}

/// Every spelling (canonical, declared variant, confusable) of every
/// literal the `call` program matches, longest first, without repeats.
fn call_literals(
    program: &Program,
    literals: &[Literal],
    tables: &[LiteralTables],
    governor: &mut Governor,
) -> Result<Vec<Vec<u8>>, GrammarError> {
    charge(governor, size(program.insts.len()), 0)?;
    let mut lits: Vec<u32> = program
        .insts
        .iter()
        .filter_map(|inst| match *inst {
            Inst::Lit { lit, .. } => Some(lit),
            _ => None,
        })
        .collect();
    lits.sort_unstable();
    lits.dedup();
    let mut spellings: Vec<Vec<u8>> = Vec::new();
    for lit in lits {
        let tables = &tables[lit as usize];
        for spelling in core::iter::once(literals[lit as usize].bytes.as_slice())
            .chain(tables.variants.list.iter().map(Bytes::as_slice))
            .chain(tables.confusables.list.iter().map(Bytes::as_slice))
        {
            charge(governor, size(spellings.len()), size(spelling.len()))?;
            if !spellings.iter().any(|known| known.as_slice() == spelling) {
                spellings.push(spelling.to_vec());
            }
        }
    }
    spellings.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    Ok(spellings)
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
    // One walk of the call program per opener alternative, and two
    // backward walks (production and call completion).
    charge(
        governor,
        size(program.openers.len())
            .saturating_add(2)
            .saturating_mul(size(program.insts.len())),
        size(program.insts.len()).saturating_mul(16),
    )?;
    let spellings = |lit: u32| {
        let tables = &tables[lit as usize];
        core::iter::once(literals[lit as usize].bytes.as_slice().to_vec())
            .chain(
                tables
                    .variants
                    .list
                    .iter()
                    .map(|bytes| bytes.as_slice().to_vec()),
            )
            .chain(
                tables
                    .confusables
                    .list
                    .iter()
                    .map(|bytes| bytes.as_slice().to_vec()),
            )
            .collect::<Vec<_>>()
    };
    // Whether the production completes from each instruction without
    // input, and whether one call ends there (the production completes or
    // the next call of the attempt begins, a `next`).
    let completing = reach_back(&program.insts, |inst| matches!(inst, Inst::Accept), true);
    let ending = reach_back(
        &program.insts,
        |inst| matches!(inst, Inst::Accept | Inst::Next { .. }),
        false,
    );
    let completes = |pc: u32| completing[pc as usize];
    let ends_call = |pc: u32| ending[pc as usize];
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
            let mut call_closers: Vec<Vec<u8>> = Vec::new();
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
                {
                    if completes(next) {
                        for spelling in spellings(lit) {
                            if !ends.contains(&spelling) {
                                ends.push(spelling);
                            }
                        }
                    }
                    if ends_call(next) {
                        for spelling in spellings(lit) {
                            if !call_closers.contains(&spelling) {
                                call_closers.push(spelling);
                            }
                        }
                    }
                }
                stack.extend(successors(inst));
            }
            Some(CallEnd {
                openers: spellings(lit),
                closers: ends,
                call_closers,
                chained,
            })
        })
        .collect())
}

/// Each production's printing layout: the text a `layout` statement gives
/// it, checked to be a member of the class the production names.
fn invalid_layout(
    at: usize,
    name: &str,
    message: &str,
    governor: &mut Governor,
) -> Result<GrammarError, GrammarError> {
    let name = owned_text(name, governor)?;
    let message = owned_text(message, governor)?;
    governor.check()?;
    Ok(GrammarError::InvalidLayout {
        offset: at,
        name: name.into_string(),
        message: message.into_string(),
    })
}

fn layouts(
    builder: &mut Builder<'_>,
    productions: &[Production],
    declared: &[Layout<'_>],
) -> Result<ScratchVec<Option<Bytes>>, GrammarError> {
    let mut layouts = ScratchVec::new(builder.governor);
    builder
        .governor
        .charge(Resource::Work, size(productions.len()))?;
    layouts.extend(core::iter::repeat_with(|| None).take(productions.len()))?;
    for &Layout { name, text, at } in declared {
        visit(0, builder.governor)?;
        let index = match builder.lookup(name)? {
            Some(Name::Production(index)) => index as usize,
            Some(Name::Closer(_)) => {
                return Err(invalid_layout(
                    at,
                    name,
                    "a closer has canonical spellings",
                    builder.governor,
                )?);
            }
            None => {
                return Err(GrammarError::UnknownName {
                    offset: at,
                    name: diagnostic(name, builder.governor)?,
                });
            }
        };
        if layouts[index].is_some() {
            work_for(name.len(), 1, 128, builder.governor)?;
            return Err(GrammarError::DuplicateName {
                offset: at,
                name: format_scratch_governed(format_args!("layout {name}"), builder.governor)?
                    .into_string(),
            });
        }
        let mut expr = &productions[index].expr;
        let mut depth = 0;
        while let Expr::Reference(id) = expr {
            visit(depth, builder.governor)?;
            depth += 1;
            expr = &productions[id.index()].expr;
        }
        let Expr::Class(class) = expr else {
            return Err(invalid_layout(
                at,
                name,
                "only a class production has a layout",
                builder.governor,
            )?);
        };
        // Native automaton matching is still the existing class authority.
        if !builder.classes[class.index()].accepts(text, builder.governor)? {
            return Err(invalid_layout(
                at,
                name,
                "the layout text is not a member of the class",
                builder.governor,
            )?);
        }
        layouts.as_mut_slice()[index] = Some(owned_bytes(text, builder.governor)?);
    }
    Ok(layouts)
}

#[cfg(test)]
mod admission_tests {
    use super::{LiteralTables, literal_tables, owned_bytes};
    use crate::contract::{Budget, CancellationToken, Governor, Resource, ScratchVec, StopReason};
    use crate::grammar::{GrammarError, Literal};

    fn prepared(g: &mut Governor) -> Result<LiteralTables, GrammarError> {
        let literal = Literal {
            bytes: owned_bytes(b"\"key\":X", g)?,
            closer: None,
        };
        let mut pairs = ScratchVec::new(g);
        pairs.push((owned_bytes(b"X", g)?, owned_bytes(b"expanded", g)?))?;
        let tables = literal_tables(&literal, &[], &pairs, Some(true), g)?;
        // A later refusal must drop fully prepared tables as well as temporaries.
        g.charge(Resource::Work, 1)?;
        Ok(tables)
    }

    #[test]
    fn derived_spelling_refusals_release_partial_and_completed_json_repair_tables() {
        let mut measured = Governor::new(Budget::default());
        let tables = prepared(&mut measured).unwrap();
        let usage = measured.usage();
        assert!(!tables.confusables.list.is_empty());
        drop(tables);
        assert_eq!(measured.scratch_meter().live(), 0);
        for (budget, resource) in [
            (
                Budget {
                    work: usage.work - 1,
                    ..Budget::default()
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: usage.peak_scratch_bytes - 1,
                    ..Budget::default()
                },
                Resource::ScratchBytes,
            ),
        ] {
            let mut g = Governor::new(budget);
            let reason = StopReason::BudgetExceeded(resource);
            assert_eq!(
                prepared(&mut g).unwrap_err(),
                GrammarError::Resource(reason.clone())
            );
            assert_eq!(g.check(), Err(reason));
            assert_eq!(g.scratch_meter().live(), 0);
        }
        let token = CancellationToken::new();
        token.cancel();
        let mut g = Governor::with_cancellation(Budget::default(), token);
        assert_eq!(
            prepared(&mut g).unwrap_err(),
            GrammarError::Resource(StopReason::Cancelled)
        );
        assert_eq!(g.scratch_meter().live(), 0);
    }
}
