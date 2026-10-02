// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Printing: a dialect grammar's productions interpreted forward.
//!
//! Printing is the inverse of recognition (D2): the printer walks the same
//! compiled [`Expr`] productions the recognizer runs, so whatever it prints
//! is, by construction, a string of the production. One walker serves every
//! printing client; they differ only in how each choice is made and where
//! captured values come from:
//!
//! - [`witness::generate`](super::witness::generate) makes seeded choices
//!   and generates shape-conforming values;
//! - the canonical printer behind
//!   [`tool_call::emit`](crate::tool_call::emit) (calls, tool results,
//!   declarations) makes the canonical choices below and prints given
//!   values.
//!
//! # The walk
//!
//! - A literal prints its bytes and a closer class its canonical (first)
//!   spelling. A literal inside a `name:` or `id:` capture must equal the
//!   printed name or id.
//! - A captured class prints the captured value, which the class must accept
//!   as a whole ([`PrintError::Unrepresentable`] otherwise); an uncaptured
//!   class prints a shortest member [`Pattern::generate`] finds.
//! - A slot prints the value's text in the slot codec's encoding, and an
//!   `until` body its verbatim text; neither may contain one of the
//!   delimiters that would end it early ([`PrintError::Unrepresentable`]).
//! - A reference prints the referenced production.
//!
//! # Canonical choices
//!
//! The canonical printer has a set of *available* captures: the values it
//! was given (a tool result without an id has no `id:` value). An
//! expression is *printable* when it can be printed without a capture that
//! is not available (an alternation needs one printable alternative, a
//! repetition with a zero minimum is always printable). Then:
//!
//! - an alternation prints its first printable alternative among those
//!   holding the most kinds of available captures, never a repaired form
//!   (one holding `insert(...)`) when another alternative is printable;
//! - a repetition holding `param-name:` prints once per argument member; one
//!   holding a `dropped:` capture prints its minimum (dropped text is never
//!   part of canonical markup); one holding an available capture, and
//!   printable, prints at least once; an optional occurrence of a production
//!   with a `layout` (see the [DSL](super)) prints once, as its layout text;
//!   any other prints its minimum, so other optional whitespace is omitted;
//! - an uncaptured production with a `layout` prints its layout text;
//! - a `content:` capture of a class prints the content as a JSON string
//!   literal ([`json::quote`]); of an `until` body, verbatim;
//! - JSON text in a slot whose delimiters (or, for `xml-params`, parameter
//!   markup) it would contain has every `<` written `\u003c`, which every
//!   JSON reader decodes to the same value.
//!
//! # Resources
//!
//! Charges [`K_PRINT`] `Work` per expression visited and per byte printed,
//! the printed bytes as `ScratchBytes`, and every class pattern compilation,
//! match and generation under the same governor.
//!
//! ```
//! use purrassist_core::grammar::print::PrintError;
//! use purrassist_core::{StopReason, contract::Resource};
//! let error = PrintError::from(StopReason::BudgetExceeded(Resource::Work));
//! assert_eq!(error, PrintError::Resource(StopReason::BudgetExceeded(Resource::Work)));
//! assert!(PrintError::Unrepresentable { text: "a</c>".into() }.to_string().contains("a</c>"));
//! ```
use super::codec::CodecKind;
use super::cost::K_PRINT;
use super::{CaptureKind, ClassId, CompiledGrammar, DelimiterSetId, Expr};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, StopReason};
use crate::json::{self, Member, Value};
use crate::pattern::{GenerateOptions, GenerateOutcome, Pattern, PatternCompileError};
use core::fmt;

/// Why a production could not be printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrintError {
    /// The production has a form the printer cannot fill: a `name:`, `id:`
    /// or `param-name:` capture that is not a class or a literal, a capture
    /// with no value to print, a slot outside an `arguments:` or
    /// `param-value:` capture, a codec that cannot carry the value, or too
    /// few parameters for a repetition's minimum.
    Unsupported {
        /// What could not be filled.
        reason: String,
    },
    /// A value is not accepted by the class capturing it, or printed text
    /// contains a delimiter ending its slot or body.
    Unrepresentable {
        /// The text that cannot be printed in place.
        text: String,
    },
    /// A class has no member within the generation bounds.
    NoCandidate {
        /// The class pattern source.
        class: String,
    },
    /// The governor's budget was exhausted, or the caller cancelled.
    Resource(StopReason),
}

impl fmt::Display for PrintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(formatter, "unsupported production: {reason}"),
            Self::Unrepresentable { text } => {
                write!(formatter, "`{text}` cannot be printed in place")
            }
            Self::NoCandidate { class } => write!(formatter, "class /{class}/ has no candidate"),
            Self::Resource(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for PrintError {}

impl From<StopReason> for PrintError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

pub(crate) fn unsupported(reason: impl Into<String>) -> PrintError {
    PrintError::Unsupported {
        reason: reason.into(),
    }
}

/// A set of capture kinds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Kinds(u16);

impl Kinds {
    const fn bit(kind: CaptureKind) -> u16 {
        1 << (kind as u16)
    }

    /// This set with `kind` added.
    pub(crate) const fn with(self, kind: CaptureKind) -> Self {
        Self(self.0 | Self::bit(kind))
    }

    pub(crate) const fn contains(self, kind: CaptureKind) -> bool {
        self.0 & Self::bit(kind) != 0
    }

    /// How many kinds of this set `other` also holds.
    pub(crate) const fn shared(self, other: Self) -> u32 {
        (self.0 & other.0).count_ones()
    }
}

/// Which captures an expression holds.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag answers an independent question the printer asks of an expression"
)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Holds {
    /// A `name:` capture.
    pub(crate) name: bool,
    /// Call captures (every kind but content, reasoning and dropped).
    pub(crate) count: usize,
    /// A `param-name:` capture.
    pub(crate) parameters: bool,
    /// A `dropped:` capture.
    pub(crate) dropped: bool,
    /// A `next` (the expression prints further calls).
    pub(crate) next: bool,
    /// Every capture kind held.
    pub(crate) kinds: Kinds,
}

/// How many times a repetition prints its body.
pub(crate) enum Repetition {
    /// This many times.
    Times(u32),
    /// Once per parameter occurrence, in order (see [`Fill::occurrence`]).
    Parameters(usize),
}

/// Whether `count` repetitions fit `min..=max`.
pub(crate) fn fits(count: usize, min: u32, max: Option<u32>) -> bool {
    size(count) >= u64::from(min) && max.is_none_or(|max| size(count) <= u64::from(max))
}

/// The choices and values of one printing client.
pub(crate) trait Fill {
    /// The alternative of a choice to print.
    fn choose(&mut self, printer: &Printer<'_>, alternatives: &[Expr]) -> usize;
    /// How often to print a repetition of `inner`.
    fn repetition(
        &mut self,
        printer: &Printer<'_>,
        inner: &Expr,
        min: u32,
        max: Option<u32>,
    ) -> Result<Repetition, PrintError>;
    /// Enter parameter occurrence `occurrence` (`None` once the repetition
    /// ends).
    fn occurrence(&mut self, occurrence: Option<usize>);
    /// Check a literal printed inside a capture.
    fn literal(&mut self, capture: CaptureKind, bytes: &[u8]) -> Result<(), PrintError>;
    /// The text of a class terminal.
    fn class(
        &mut self,
        printer: &mut Printer<'_>,
        id: ClassId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<String, PrintError>;
    /// The body text of a slot.
    fn slot(
        &mut self,
        printer: &Printer<'_>,
        codec: CodecKind,
        delimiters: DelimiterSetId,
        capture: Option<CaptureKind>,
    ) -> Result<String, PrintError>;
    /// The body text of an `until`.
    fn until(&mut self, capture: Option<CaptureKind>) -> Result<String, PrintError>;
    /// Whether uncaptured productions with a `layout` print as their
    /// layout text.
    fn layouts(&self) -> bool {
        false
    }
    /// A `next`: the printing moves on to the next call of the group.
    fn next_call(&mut self, governor: &mut Governor) -> Result<(), PrintError>;
}

/// The production walker: the printed bytes and the class patterns compiled
/// so far.
pub(crate) struct Printer<'g> {
    pub(crate) grammar: &'g CompiledGrammar,
    patterns: Vec<Option<Pattern>>,
    pub(crate) out: Vec<u8>,
}

impl<'g> Printer<'g> {
    pub(crate) fn new(grammar: &'g CompiledGrammar) -> Self {
        Self {
            grammar,
            patterns: vec![None; grammar.classes.len()],
            out: Vec::new(),
        }
    }

    /// Append `bytes`, charging their work and scratch.
    pub(crate) fn emit(&mut self, bytes: &[u8], governor: &mut Governor) -> Result<(), StopReason> {
        governor.charge(Resource::Work, size(bytes.len()).saturating_mul(K_PRINT))?;
        governor.charge(Resource::ScratchBytes, size(bytes.len()))?;
        self.out.extend_from_slice(bytes);
        Ok(())
    }

    /// Print `expr` (inside `capture`, when one encloses it).
    pub(crate) fn walk<F: Fill>(
        &mut self,
        fill: &mut F,
        expr: &Expr,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<(), PrintError> {
        governor.charge(Resource::Work, K_PRINT)?;
        let grammar = self.grammar;
        match expr {
            Expr::Literal(id) => {
                let bytes = grammar.literal(*id);
                if let Some(kind) = capture {
                    fill.literal(kind, bytes)?;
                }
                self.emit(bytes, governor)?;
            }
            Expr::Closer(id) => self.emit(&grammar.closer_spellings(*id)[0], governor)?,
            Expr::Class(id) => {
                let text = fill.class(self, *id, capture, governor)?;
                if !self.accepts(*id, text.as_bytes(), governor)? {
                    return Err(PrintError::Unrepresentable { text });
                }
                self.emit(text.as_bytes(), governor)?;
            }
            Expr::Until(delimiters) => {
                let text = fill.until(capture)?;
                self.bounded(&text, *delimiters, governor)?;
            }
            Expr::Slot { codec, delimiters } => {
                let text = fill.slot(self, *codec, *delimiters, capture)?;
                governor.charge(Resource::Work, size(text.len()).saturating_mul(K_PRINT))?;
                self.bounded(&text, *delimiters, governor)?;
            }
            Expr::Capture { kind, inner } => self.walk(fill, inner, Some(*kind), governor)?,
            Expr::Sequence(items) => {
                for item in items {
                    self.walk(fill, item, capture, governor)?;
                }
            }
            Expr::Choice(items) => {
                let pick = fill.choose(self, items);
                self.walk(fill, &items[pick], capture, governor)?;
            }
            Expr::Repeat { inner, min, max } => match fill.repetition(self, inner, *min, *max)? {
                Repetition::Times(count) => {
                    for _ in 0..count {
                        self.walk(fill, inner, capture, governor)?;
                    }
                }
                Repetition::Parameters(count) => {
                    for occurrence in 0..count {
                        fill.occurrence(Some(occurrence));
                        self.walk(fill, inner, capture, governor)?;
                    }
                    fill.occurrence(None);
                }
            },
            Expr::Reference(id) => match grammar.layout(*id) {
                Some(text) if capture.is_none() && fill.layouts() => self.emit(text, governor)?,
                _ => self.walk(fill, grammar.production_expr(*id), capture, governor)?,
            },
            // Zero-width markers print nothing; an omitted token stays
            // omitted.
            Expr::Commit | Expr::Insert(_) => {}
            Expr::Next => fill.next_call(governor)?,
        }
        Ok(())
    }

    /// Emit a body that must not contain one of `delimiters`.
    fn bounded(
        &mut self,
        text: &str,
        delimiters: DelimiterSetId,
        governor: &mut Governor,
    ) -> Result<(), PrintError> {
        if text.is_empty() {
            return Ok(());
        }
        if self.cut(delimiters, text.as_bytes()) {
            return Err(PrintError::Unrepresentable { text: text.into() });
        }
        self.emit(text.as_bytes(), governor)?;
        Ok(())
    }

    /// Whether one of the delimiters of set `id` occurs in `bytes`.
    pub(crate) fn cut(&self, id: DelimiterSetId, bytes: &[u8]) -> bool {
        contains_any(bytes, self.grammar.delimiters(id))
    }

    /// Which captures `expr` holds.
    pub(crate) fn holds(&self, expr: &Expr) -> Holds {
        let mut holds = Holds::default();
        self.collect_holds(expr, &mut holds);
        holds
    }

    fn collect_holds(&self, expr: &Expr, holds: &mut Holds) {
        match expr {
            Expr::Capture { kind, inner } => {
                match kind {
                    CaptureKind::Name => holds.name = true,
                    CaptureKind::ParamName => holds.parameters = true,
                    CaptureKind::Dropped => holds.dropped = true,
                    _ => {}
                }
                if !matches!(
                    kind,
                    CaptureKind::Content | CaptureKind::Reasoning | CaptureKind::Dropped
                ) {
                    holds.count += 1;
                }
                holds.kinds = holds.kinds.with(*kind);
                self.collect_holds(inner, holds);
            }
            Expr::Repeat { inner, .. } => self.collect_holds(inner, holds),
            Expr::Sequence(items) | Expr::Choice(items) => {
                for item in items {
                    self.collect_holds(item, holds);
                }
            }
            Expr::Reference(id) => self.collect_holds(self.grammar.production_expr(*id), holds),
            Expr::Next => holds.next = true,
            _ => {}
        }
    }

    /// Whether every printing of `expr` holds an `insert(...)`: a repaired
    /// form, which an alternation never chooses when another of its
    /// alternatives is printable.
    pub(crate) fn repaired(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Insert(_) => true,
            Expr::Capture { inner, .. } => self.repaired(inner),
            Expr::Sequence(items) => items.iter().any(|item| self.repaired(item)),
            Expr::Choice(items) => items.iter().all(|item| self.repaired(item)),
            Expr::Repeat { inner, min, .. } => *min > 0 && self.repaired(inner),
            Expr::Reference(id) => self.repaired(self.grammar.production_expr(*id)),
            _ => false,
        }
    }

    /// Whether `expr` can be printed with only the `available` captures.
    pub(crate) fn printable(&self, expr: &Expr, available: Kinds) -> bool {
        match expr {
            Expr::Capture { kind, inner } => {
                available.contains(*kind) && self.printable(inner, available)
            }
            Expr::Sequence(items) => items.iter().all(|item| self.printable(item, available)),
            Expr::Choice(items) => items.iter().any(|item| self.printable(item, available)),
            Expr::Repeat { inner, min, .. } => *min == 0 || self.printable(inner, available),
            Expr::Reference(id) => self.printable(self.grammar.production_expr(*id), available),
            _ => true,
        }
    }

    /// The codec and delimiters of the first `arguments:` or `param-value:`
    /// slot in `expr`.
    pub(crate) fn arguments_slot(&self, expr: &Expr) -> Option<(CodecKind, DelimiterSetId)> {
        match expr {
            Expr::Capture {
                kind: CaptureKind::Arguments | CaptureKind::ParamValue,
                inner,
            } => match **inner {
                Expr::Slot { codec, delimiters } => Some((codec, delimiters)),
                _ => self.arguments_slot(inner),
            },
            Expr::Capture { inner, .. } | Expr::Repeat { inner, .. } => self.arguments_slot(inner),
            Expr::Sequence(items) | Expr::Choice(items) => {
                items.iter().find_map(|item| self.arguments_slot(item))
            }
            Expr::Reference(id) => self.arguments_slot(self.grammar.production_expr(*id)),
            _ => None,
        }
    }

    /// Whether class `id` matches all of `text`.
    pub(crate) fn accepts(
        &self,
        id: ClassId,
        text: &[u8],
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        self.grammar.classes[id.index()].accepts(text, governor)
    }

    /// A member of class `id` of at least `min_len` characters (or the
    /// shortest member, when there is none that long).
    pub(crate) fn generate_class(
        &mut self,
        id: ClassId,
        min_len: usize,
        governor: &mut Governor,
    ) -> Result<String, PrintError> {
        let source = self.grammar.class_source(id);
        if self.patterns[id.index()].is_none() {
            let anchored = format!("^(?:{source})$");
            let pattern =
                Pattern::compile_governed(&anchored, governor).map_err(|error| match error {
                    PatternCompileError::Resource(reason) => PrintError::Resource(reason),
                    PatternCompileError::Syntax(error) => {
                        unsupported(format!("class /{source}/: {error}"))
                    }
                })?;
            self.patterns[id.index()] = Some(pattern);
        }
        let pattern = self.patterns[id.index()].as_ref().expect("compiled above");
        for min_len in [min_len, 0] {
            let outcome = pattern.generate(
                governor,
                GenerateOptions {
                    min_len,
                    max_len: Some(min_len + 16),
                    avoid: &[],
                },
            )?;
            if let GenerateOutcome::Witness(text) = outcome
                && !text.is_empty()
            {
                return Ok(text);
            }
        }
        Err(PrintError::NoCandidate {
            class: source.to_owned(),
        })
    }
}

/// Whether one of `needles` occurs in `bytes`.
pub(crate) fn contains_any<N: AsRef<[u8]>>(bytes: &[u8], needles: &[N]) -> bool {
    needles.iter().any(|needle| {
        let needle = needle.as_ref();
        !needle.is_empty() && bytes.windows(needle.len()).any(|window| window == needle)
    })
}

/// Compact JSON text that contains none of `risky`: `text` itself, or, when
/// it contains one, `text` with every `<` written `\u003c` (a `<` occurs in
/// compact JSON only inside a string, where the escape decodes to it).
pub(crate) fn guard_json<N: AsRef<[u8]>>(text: String, risky: &[N]) -> String {
    if contains_any(text.as_bytes(), risky) {
        text.replace('<', "\\u003c")
    } else {
        text
    }
}

/// The values a canonical printing fills in.
#[derive(Clone, Copy, Default)]
pub(crate) struct Values<'v> {
    /// The `name:` value.
    pub(crate) name: Option<&'v str>,
    /// The `id:` value.
    pub(crate) id: Option<&'v str>,
    /// The `content:` value.
    pub(crate) content: Option<&'v str>,
    /// The text of an `arguments:` slot.
    pub(crate) arguments: Option<&'v str>,
    /// The members `param-name:`/`param-value:` pairs print, in order.
    pub(crate) parameters: Option<&'v [Member]>,
}

/// The canonical printing of [`Values`] (see the module documentation).
pub(crate) struct Canonical<'v> {
    values: Values<'v>,
    /// The values of the group's further calls, printed after each `next`.
    rest: Vec<Values<'v>>,
    available: Kinds,
    occurrence: Option<usize>,
    printed: Kinds,
}

impl<'v> Canonical<'v> {
    pub(crate) fn new(values: Values<'v>) -> Self {
        Self::group(values, Vec::new())
    }

    /// The canonical printing of a group of calls in one production
    /// instance: `first`, then each of `rest` after a `next`.
    pub(crate) fn group(first: Values<'v>, mut rest: Vec<Values<'v>>) -> Self {
        rest.reverse();
        Self {
            available: Self::kinds(&first),
            values: first,
            rest,
            occurrence: None,
            printed: Kinds::default(),
        }
    }

    fn kinds(values: &Values<'_>) -> Kinds {
        let mut available = Kinds::default();
        for (present, kind) in [
            (values.name.is_some(), CaptureKind::Name),
            (values.id.is_some(), CaptureKind::Id),
            (values.content.is_some(), CaptureKind::Content),
            (values.arguments.is_some(), CaptureKind::Arguments),
            (values.parameters.is_some(), CaptureKind::ParamName),
            (values.parameters.is_some(), CaptureKind::ParamValue),
        ] {
            if present {
                available = available.with(kind);
            }
        }
        available
    }

    /// Whether every call of the group was printed.
    pub(crate) const fn finished(&self) -> bool {
        self.rest.is_empty()
    }

    /// Whether a value of `kind` was printed.
    pub(crate) const fn printed(&self, kind: CaptureKind) -> bool {
        self.printed.contains(kind)
    }

    /// Whether `expr` can be printed with these values.
    pub(crate) fn printable(&self, printer: &Printer<'_>, expr: &Expr) -> bool {
        printer.printable(expr, self.available)
    }

    fn missing(kind: CaptureKind) -> PrintError {
        unsupported(format!("no value for a `{}:` capture", kind.name()))
    }

    fn member(&mut self) -> Result<&'v Member, PrintError> {
        self.printed = self
            .printed
            .with(CaptureKind::ParamName)
            .with(CaptureKind::ParamValue);
        let parameters = self
            .values
            .parameters
            .ok_or_else(|| Self::missing(CaptureKind::ParamName))?;
        self.occurrence
            .and_then(|occurrence| parameters.get(occurrence))
            .ok_or_else(|| unsupported("a parameter capture outside a repetition"))
    }

    fn value(&mut self, kind: CaptureKind) -> Result<&'v str, PrintError> {
        self.printed = self.printed.with(kind);
        match kind {
            CaptureKind::Name => self.values.name,
            CaptureKind::Id => self.values.id,
            CaptureKind::Content => self.values.content,
            _ => None,
        }
        .ok_or_else(|| Self::missing(kind))
    }
}

impl Fill for Canonical<'_> {
    fn choose(&mut self, printer: &Printer<'_>, alternatives: &[Expr]) -> usize {
        let mut best: Option<(u32, usize)> = None;
        let repaired = |alternative: &Expr| printer.repaired(alternative);
        let clean = alternatives.iter().any(|alternative| {
            !repaired(alternative) && printer.printable(alternative, self.available)
        });
        for (index, alternative) in alternatives.iter().enumerate() {
            if !printer.printable(alternative, self.available) || (clean && repaired(alternative)) {
                continue;
            }
            let score = printer.holds(alternative).kinds.shared(self.available);
            if best.is_none_or(|(top, _)| score > top) {
                best = Some((score, index));
            }
        }
        best.map_or(0, |(_, index)| index)
    }

    fn repetition(
        &mut self,
        printer: &Printer<'_>,
        inner: &Expr,
        min: u32,
        max: Option<u32>,
    ) -> Result<Repetition, PrintError> {
        let holds = printer.holds(inner);
        if holds.next {
            // Once per further call of the group.
            let count = self.rest.len();
            if !fits(count, min, max) {
                return Err(unsupported(format!(
                    "{count} further calls do not fit a repetition of {min}..{max:?}"
                )));
            }
            return Ok(Repetition::Times(u32::try_from(count).unwrap_or(u32::MAX)));
        }
        if holds.parameters {
            let count = self.values.parameters.map_or(0, <[Member]>::len);
            if !fits(count, min, max) {
                return Err(unsupported(format!(
                    "{count} parameters do not fit a repetition of {min}..{max:?}"
                )));
            }
            return Ok(Repetition::Parameters(count));
        }
        let once = min.max(1);
        if let Expr::Reference(id) = inner
            && printer.grammar.layout(*id).is_some()
            && fits(once as usize, min, max)
        {
            // Optional layout whitespace prints as its layout text.
            return Ok(Repetition::Times(once));
        }
        let wanted = !holds.dropped
            && holds.kinds.shared(self.available) > 0
            && printer.printable(inner, self.available)
            && fits(once as usize, min, max);
        Ok(Repetition::Times(if wanted { once } else { min }))
    }

    fn occurrence(&mut self, occurrence: Option<usize>) {
        self.occurrence = occurrence;
    }

    fn literal(&mut self, capture: CaptureKind, bytes: &[u8]) -> Result<(), PrintError> {
        if !matches!(capture, CaptureKind::Name | CaptureKind::Id) {
            return Ok(());
        }
        let value = self.value(capture)?;
        if value.as_bytes() == bytes {
            Ok(())
        } else {
            Err(PrintError::Unrepresentable {
                text: value.to_owned(),
            })
        }
    }

    fn class(
        &mut self,
        printer: &mut Printer<'_>,
        id: ClassId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<String, PrintError> {
        match capture {
            None => printer.generate_class(id, 0, governor),
            Some(CaptureKind::ParamName) => Ok(self.member()?.key.clone()),
            Some(CaptureKind::Content) => Ok(json::quote(self.value(CaptureKind::Content)?)),
            Some(kind @ (CaptureKind::Name | CaptureKind::Id)) => Ok(self.value(kind)?.to_owned()),
            Some(kind) => Err(Self::missing(kind)),
        }
    }

    fn slot(
        &mut self,
        printer: &Printer<'_>,
        codec: CodecKind,
        delimiters: DelimiterSetId,
        capture: Option<CaptureKind>,
    ) -> Result<String, PrintError> {
        match (capture, codec) {
            (Some(CaptureKind::Arguments), _) => {
                self.printed = self.printed.with(CaptureKind::Arguments);
                self.values
                    .arguments
                    .map(str::to_owned)
                    .ok_or_else(|| Self::missing(CaptureKind::Arguments))
            }
            (Some(CaptureKind::ParamValue), CodecKind::Json) => Ok(guard_json(
                self.member()?.value.to_json(),
                printer.grammar.delimiters(delimiters),
            )),
            (Some(CaptureKind::ParamValue), CodecKind::Raw) => {
                let value = &self.member()?.value;
                match &value.value {
                    Value::String(text) => Ok(text.clone()),
                    _ => Err(PrintError::Unrepresentable {
                        text: value.to_json(),
                    }),
                }
            }
            _ => Err(unsupported(format!(
                "a {} slot under {:?} cannot carry a value",
                codec.name(),
                capture.map(CaptureKind::name)
            ))),
        }
    }

    fn layouts(&self) -> bool {
        true
    }

    fn next_call(&mut self, _: &mut Governor) -> Result<(), PrintError> {
        let values = self
            .rest
            .pop()
            .ok_or_else(|| unsupported("a `next` with no further call to print"))?;
        self.available = Self::kinds(&values);
        self.values = values;
        self.occurrence = None;
        Ok(())
    }

    fn until(&mut self, capture: Option<CaptureKind>) -> Result<String, PrintError> {
        capture.map_or_else(
            || Ok(String::new()),
            |kind| self.value(kind).map(str::to_owned),
        )
    }
}
