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
//!   class prints a shortest member of its retained automaton.
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
//! the printed bytes and temporary storage as `ScratchBytes`, and retained
//! class matching and generation under the same governor. Original borrowed
//! values are admitted before traversal. Recursive work checks caller depth and
//! a fixed aggregate ceiling of 128; either limit is a sticky typed refusal.
//! Only complete output or an escaping diagnostic retains storage. Grammar
//! compilation and higher-level emitter/witness preparation are caller-owned.
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
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchString, ScratchVec, StopReason,
    format_scratch_governed,
};
use crate::json::{self, Member, Value};
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

impl std::error::Error for PrintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resource(reason) => Some(reason),
            _ => None,
        }
    }
}

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

/// A diagnostic keeps its exact reservation until its actual public boundary.
#[derive(Debug)]
pub(crate) enum ScratchPrintError {
    Unsupported { reason: ScratchString },
    Unrepresentable { text: ScratchString },
    NoCandidate { class: ScratchString },
    Resource(StopReason),
    Prepared(PrintError),
}
impl From<StopReason> for ScratchPrintError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}
impl From<PrintError> for ScratchPrintError {
    fn from(error: PrintError) -> Self {
        match error {
            PrintError::Resource(reason) => Self::Resource(reason),
            error => Self::Prepared(error),
        }
    }
}
impl From<ScratchPrintError> for PrintError {
    fn from(error: ScratchPrintError) -> Self {
        match error {
            ScratchPrintError::Unsupported { reason } => Self::Unsupported {
                reason: reason.into_string(),
            },
            ScratchPrintError::Unrepresentable { text } => Self::Unrepresentable {
                text: text.into_string(),
            },
            ScratchPrintError::NoCandidate { class } => Self::NoCandidate {
                class: class.into_string(),
            },
            ScratchPrintError::Resource(reason) => Self::Resource(reason),
            ScratchPrintError::Prepared(error) => error,
        }
    }
}
impl fmt::Display for ScratchPrintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { reason } => {
                write!(formatter, "unsupported production: {}", reason.as_str())
            }
            Self::Unrepresentable { text } => {
                write!(formatter, "`{}` cannot be printed in place", text.as_str())
            }
            Self::NoCandidate { class } => {
                write!(formatter, "class /{}/ has no candidate", class.as_str())
            }
            Self::Resource(reason) => write!(formatter, "{reason}"),
            Self::Prepared(error) => write!(formatter, "{error}"),
        }
    }
}
impl ScratchPrintError {
    pub(crate) fn into_detail(self, governor: &mut Governor) -> Result<ScratchString, StopReason> {
        governor.charge(Resource::Work, 128)?;
        format_scratch_governed(format_args!("{self}"), governor)
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

/// One of a prepared grammar's three canonical production roots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    /// The call envelope, optionally including further calls through `next`.
    Call,
    /// A tool result.
    Result,
    /// One tool declaration.
    Declaration,
}

/// Borrowed values supplied to the existing canonical production interpreter.
///
/// Arguments are already prepared codec text. Parameter members preserve order,
/// duplicates and exact numeric spelling; their DOM preparation is caller-owned.
#[derive(Clone, Copy, Default, Debug)]
pub struct Values<'v> {
    /// The `name:` value.
    pub name: Option<&'v str>,
    /// The `id:` value.
    pub id: Option<&'v str>,
    /// The `content:` value.
    pub content: Option<&'v str>,
    /// Prepared text for an `arguments:` slot, including its chosen encoding.
    pub arguments: Option<&'v str>,
    /// Ordered occurrences for `param-name:`/`param-value:` captures.
    pub parameters: Option<&'v [Member]>,
}
impl ScratchCharge for Values<'_> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 80);
        80
    };
}
impl ScratchCharge for &Expr {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 8);
        8
    };
}

/// Complete canonical bytes and the capture kinds actually printed.
#[derive(Debug)]
pub struct Printed {
    /// Complete production bytes. No partial artifact is returned on failure.
    pub bytes: Vec<u8>,
    printed: Kinds,
}
impl Printed {
    /// Whether the canonical walk emitted a value of this capture kind.
    ///
    /// Callers retain the supplied values and can report unsupported omissions
    /// using this metadata, as the existing result/declaration adapters do.
    #[must_use]
    pub const fn printed(&self, kind: CaptureKind) -> bool {
        self.printed.contains(kind)
    }
}

/// Print a canonical production over an already prepared grammar and values.
///
/// `rest` supplies later calls in order to a `next` production. The existing
/// choice/layout/repetition/codec rules remain authoritative. Every borrowed
/// value, structural scan, temporary/output allocation and final cancellation
/// check belongs to this governor. Grammar compilation, DOM construction and
/// higher-level emitter preparation are separate caller-owned operations.
/// Full seeded witness value generation is not covered by this canonical API.
/// Traversal has a fixed 128-level native stack ceiling in addition to caller
/// depth limits; helper scans inherit the active walk depth and cannot reset it.
///
/// # Errors
/// Preserves semantic [`PrintError`] payloads and exact resource causes. A group
/// whose further values cannot all be printed is unsupported. Resource failures
/// and cancellation never publish partially printed bytes or metadata.
///
/// ```
/// use purrassist_core::{Budget, Governor, grammar::{Grammar, print::{canonical_governed, Root, Values, PrintError}}};
/// let source = r#"grammar "demo"; call = "<c>"; result = "<r>" content:until("</r>") "</r>"; declaration = "<d>";"#;
/// // Preparation is deliberately separate from the printing governor.
/// let grammar = Grammar::compile_governed(source, &mut Governor::new(Budget::default())).unwrap();
/// let values = Values { content: Some("ok"), ..Values::default() };
/// let output = canonical_governed(&grammar, Root::Result, values, &[], &mut Governor::new(Budget::default())).unwrap();
/// assert_eq!(output.bytes, b"<r>ok</r>");
/// let bad = Values { content: Some("a</r>"), ..values };
/// assert!(matches!(canonical_governed(&grammar, Root::Result, bad, &[], &mut Governor::new(Budget::default())), Err(PrintError::Unrepresentable { .. })));
/// ```
pub fn canonical_governed<'v>(
    grammar: &CompiledGrammar,
    root: Root,
    first: Values<'v>,
    rest: &'v [Values<'v>],
    governor: &mut Governor,
) -> Result<Printed, PrintError> {
    let printed = canonical_scratch(grammar, root, first, rest, governor)?;
    Ok(Printed {
        bytes: printed.bytes.into_vec(),
        printed: printed.printed,
    })
}

/// Internal consumers retain transient ownership until actual publication.
pub(crate) struct ScratchPrinted {
    pub(crate) bytes: ScratchVec<u8>,
    printed: Kinds,
}
impl ScratchPrinted {
    pub(crate) const fn printed(&self, kind: CaptureKind) -> bool {
        self.printed.contains(kind)
    }
    pub(crate) fn check_utf8(&self, governor: &mut Governor) -> Result<(), ScratchPrintError> {
        governor.charge(Resource::Work, size(self.bytes.len()))?;
        if core::str::from_utf8(self.bytes.as_slice()).is_err() {
            return Err(ScratchPrintError::Unrepresentable {
                text: lossy_text(self.bytes.as_slice(), governor)?,
            });
        }
        governor.check()?;
        Ok(())
    }
    pub(crate) fn into_string(self, governor: &mut Governor) -> Result<String, PrintError> {
        self.check_utf8(governor)?;
        Ok(String::from_utf8(self.bytes.into_vec()).expect("validated canonical UTF-8 bytes"))
    }
}

pub(crate) fn canonical_scratch<'v>(
    grammar: &CompiledGrammar,
    root: Root,
    first: Values<'v>,
    rest: &'v [Values<'v>],
    governor: &mut Governor,
) -> Result<ScratchPrinted, ScratchPrintError> {
    governor.check()?;
    admit_values(first, governor)?;
    governor.charge(Resource::Work, size(rest.len()))?;
    for &values in rest {
        admit_values(values, governor)?;
    }
    let mut printer = Printer::new(grammar, governor);
    let expression = match root {
        Root::Call => grammar.call(),
        Root::Result => grammar.result(),
        Root::Declaration => grammar.declaration(),
    };
    let mut fill = Canonical::group(first, rest);
    printer.walk(&mut fill, expression, None, governor)?;
    if !fill.finished() {
        return Err(unsupported_governed(
            format_args!("the production prints fewer calls than the group holds"),
            governor,
        ));
    }
    governor.check()?;
    Ok(ScratchPrinted {
        bytes: printer.out,
        printed: fill.printed,
    })
}

fn admit_text(text: &str, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(Resource::InputBytes, size(text.len()))?;
    governor.charge(Resource::Work, size(text.len()).saturating_add(1))
}
fn visit(depth: u64, governor: &mut Governor) -> Result<(), StopReason> {
    if depth > 128 {
        return Err(governor.stop_resource(Resource::Depth));
    }
    governor.charge(Resource::Depth, depth)?;
    governor.charge(Resource::Work, K_PRINT)
}
fn admit_node(
    node: &crate::json::Node,
    depth: u64,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    visit(depth, governor)?;
    match &node.value {
        Value::String(text) => admit_text(text, governor)?,
        Value::Number(number) => admit_text(number.as_str(), governor)?,
        Value::Array(items) => {
            for item in items {
                admit_node(item, depth.saturating_add(1), governor)?;
            }
        }
        Value::Object(members) => {
            for member in members {
                admit_text(&member.key, governor)?;
                admit_node(&member.value, depth.saturating_add(1), governor)?;
            }
        }
        Value::Null | Value::Bool(_) => {}
    }
    Ok(())
}
fn admit_values(values: Values<'_>, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(Resource::Work, 6)?;
    for text in [values.name, values.id, values.content, values.arguments]
        .into_iter()
        .flatten()
    {
        admit_text(text, governor)?;
    }
    if let Some(members) = values.parameters {
        for member in members {
            admit_text(&member.key, governor)?;
            admit_node(&member.value, 0, governor)?;
        }
    }
    Ok(())
}

fn unsupported_governed(
    arguments: fmt::Arguments<'_>,
    governor: &mut Governor,
) -> ScratchPrintError {
    // Internal diagnostics format only bounded numeric/enumeration values and
    // borrowed text. The sink admits all bytes; no arbitrary callback is public.
    if let Err(reason) = governor.charge(Resource::Work, 128) {
        return ScratchPrintError::Resource(reason);
    }
    match format_scratch_governed(arguments, governor) {
        Ok(reason) => ScratchPrintError::Unsupported { reason },
        Err(reason) => ScratchPrintError::Resource(reason),
    }
}
fn lossy_text(bytes: &[u8], governor: &mut Governor) -> Result<ScratchString, ScratchPrintError> {
    // Cover decoder lookahead, prefix revalidation, copies, three-byte
    // replacements and loop control before any decoding or allocation.
    let work = size(bytes.len())
        .checked_mul(12)
        .and_then(|work| work.checked_add(1))
        .ok_or_else(|| governor.stop_resource(Resource::Work))?;
    governor.charge(Resource::Work, work)?;
    let mut output = ScratchString::new(governor);
    let mut remaining = bytes;
    while !remaining.is_empty() {
        governor.check()?;
        match core::str::from_utf8(remaining) {
            Ok(text) => {
                output.push_str(text)?;
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                // The standard decoder establishes this prefix's UTF-8 validity.
                let prefix =
                    core::str::from_utf8(&remaining[..valid]).expect("validated UTF-8 prefix");
                output.push_str(prefix)?;
                output.push('\u{fffd}')?;
                match error.error_len() {
                    Some(length) => remaining = &remaining[valid + length..],
                    None => break,
                }
            }
        }
    }
    Ok(output)
}

/// Whether one of `needles` occurs in `bytes` (legacy emitter preparation).
pub(crate) fn contains_any<N: AsRef<[u8]>>(bytes: &[u8], needles: &[N]) -> bool {
    needles.iter().any(|needle| {
        let needle = needle.as_ref();
        !needle.is_empty() && bytes.windows(needle.len()).any(|window| window == needle)
    })
}
/// Existing higher-level preparation, outside canonical printing governance.
pub(crate) fn guard_json<N: AsRef<[u8]>>(text: String, risky: &[N]) -> String {
    if contains_any(text.as_bytes(), risky) {
        text.replace('<', "\\u003c")
    } else {
        text
    }
}
fn contains_any_governed<N: AsRef<[u8]>>(
    bytes: &[u8],
    needles: &[N],
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    for needle in needles {
        governor.charge(Resource::Work, 1)?;
        let needle = needle.as_ref();
        if needle.is_empty() {
            continue;
        }
        for window in bytes.windows(needle.len()) {
            governor.charge(Resource::Work, size(needle.len()).saturating_add(1))?;
            if window == needle {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn guard_json_governed<N: AsRef<[u8]>>(
    text: ScratchString,
    risky: &[N],
    governor: &mut Governor,
) -> Result<ScratchString, ScratchPrintError> {
    if !contains_any_governed(text.as_str().as_bytes(), risky, governor)? {
        return Ok(text);
    }
    // Admit the split scan, at most six output bytes per input byte and
    // loop control before the iterator scans for its first separator.
    let work = size(text.as_str().len())
        .checked_mul(8)
        .and_then(|work| work.checked_add(1))
        .ok_or_else(|| governor.stop_resource(Resource::Work))?;
    governor.charge(Resource::Work, work)?;
    let mut output = ScratchString::new(governor);
    for (index, part) in text.as_str().split('<').enumerate() {
        governor.check()?;
        if index > 0 {
            output.push_str("\\u003c")?;
        }
        output.push_str(part)?;
    }
    Ok(output)
}

/// The choices and values of one printing client. Seeded generation shares the
/// walk, while its shape/value preparation remains a separate contract.
pub(crate) trait Fill {
    fn choose(
        &mut self,
        printer: &Printer<'_>,
        alternatives: &[&Expr],
        governor: &mut Governor,
    ) -> Result<usize, ScratchPrintError>;
    fn repetition(
        &mut self,
        printer: &Printer<'_>,
        inner: &Expr,
        min: u32,
        max: Option<u32>,
        governor: &mut Governor,
    ) -> Result<Repetition, ScratchPrintError>;
    fn occurrence(&mut self, occurrence: Option<usize>);
    fn literal(
        &mut self,
        capture: CaptureKind,
        bytes: &[u8],
        governor: &mut Governor,
    ) -> Result<(), ScratchPrintError>;
    fn class(
        &mut self,
        printer: &mut Printer<'_>,
        id: ClassId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<ScratchString, ScratchPrintError>;
    fn slot(
        &mut self,
        printer: &Printer<'_>,
        codec: CodecKind,
        delimiters: DelimiterSetId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<ScratchString, ScratchPrintError>;
    fn until(
        &mut self,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<ScratchString, ScratchPrintError>;
    fn layouts(&self) -> bool {
        false
    }
    fn next_call(&mut self, governor: &mut Governor) -> Result<(), ScratchPrintError>;
    fn ordinal(&mut self, governor: &mut Governor) -> Result<ScratchString, ScratchPrintError>;
    fn composite_id(
        &mut self,
        text: &str,
        governor: &mut Governor,
    ) -> Result<(), ScratchPrintError>;
    fn name(&self) -> Option<&str> {
        None
    }
}

/// The existing single production walker, with admitted output storage.
pub(crate) struct Printer<'g> {
    pub(crate) grammar: &'g CompiledGrammar,
    pub(crate) out: ScratchVec<u8>,
    composite: bool,
    active_depth: u64,
}
impl<'g> Printer<'g> {
    pub(crate) fn new(grammar: &'g CompiledGrammar, governor: &Governor) -> Self {
        Self {
            grammar,
            out: ScratchVec::new(governor),
            composite: false,
            active_depth: 0,
        }
    }
    pub(crate) fn emit(&mut self, bytes: &[u8], governor: &mut Governor) -> Result<(), StopReason> {
        governor.charge(Resource::Work, size(bytes.len()).saturating_mul(K_PRINT))?;
        self.out.extend(bytes.iter().copied())
    }
    pub(crate) fn walk<F: Fill>(
        &mut self,
        fill: &mut F,
        expr: &Expr,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<(), ScratchPrintError> {
        self.walk_at(fill, expr, capture, 0, governor)
    }
    fn walk_at<F: Fill>(
        &mut self,
        fill: &mut F,
        expr: &Expr,
        capture: Option<CaptureKind>,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<(), ScratchPrintError> {
        visit(depth, governor)?;
        let outer = core::mem::replace(&mut self.active_depth, depth);
        let result = self.walk_body(fill, expr, capture, depth, governor);
        self.active_depth = outer;
        result
    }

    fn walk_body<F: Fill>(
        &mut self,
        fill: &mut F,
        expr: &Expr,
        capture: Option<CaptureKind>,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<(), ScratchPrintError> {
        let grammar = self.grammar;
        let child = depth.saturating_add(1);
        match expr {
            Expr::Literal(id) => {
                let bytes = grammar.literal(*id);
                governor.charge(Resource::InputBytes, size(bytes.len()))?;
                if let Some(kind) = capture {
                    fill.literal(kind, bytes, governor)?;
                }
                self.emit(bytes, governor)?;
            }
            Expr::Closer(id) => {
                let bytes = &grammar.closer_spellings(*id)[0];
                governor.charge(Resource::InputBytes, size(bytes.len()))?;
                self.emit(bytes, governor)?;
            }
            Expr::Class(id) => self.print_class(fill, *id, capture, governor)?,
            Expr::Until(delimiters) => {
                let text = fill.until(capture, governor)?;
                self.bounded(text.as_str(), *delimiters, governor)?;
            }
            Expr::Slot { codec, delimiters } => {
                let text = fill.slot(self, *codec, *delimiters, capture, governor)?;
                self.bounded(text.as_str(), *delimiters, governor)?;
            }
            Expr::Capture {
                kind: CaptureKind::Id,
                inner,
            } if self.holds(inner, governor)?.name => {
                let start = self.out.len();
                let outer = core::mem::replace(&mut self.composite, true);
                let printed = self.walk_at(fill, inner, None, child, governor);
                self.composite = outer;
                printed?;
                let text = lossy_text(&self.out.as_slice()[start..], governor)?;
                fill.composite_id(text.as_str(), governor)?;
            }
            Expr::Capture { kind, inner } => {
                self.walk_at(fill, inner, Some(*kind), child, governor)?;
            }
            Expr::Sequence(items) => {
                for item in items {
                    self.walk_at(fill, item, capture, child, governor)?;
                }
            }
            Expr::Choice(items) => {
                let mut fitting = ScratchVec::new(governor);
                for item in items {
                    if self.name_fits(item, fill.name(), child, governor)? {
                        fitting.push(item)?;
                    }
                }
                if fitting.is_empty() || fitting.len() == items.len() {
                    fitting.clear();
                    fitting.extend(items.iter())?;
                }
                let pick = fill.choose(self, fitting.as_slice(), governor)?;
                self.walk_at(fill, fitting.as_slice()[pick], capture, child, governor)?;
            }
            Expr::Repeat { inner, min, max } => {
                match fill.repetition(self, inner, *min, *max, governor)? {
                    Repetition::Times(count) => {
                        for _ in 0..count {
                            self.walk_at(fill, inner, capture, child, governor)?;
                        }
                    }
                    Repetition::Parameters(count) => {
                        for occurrence in 0..count {
                            fill.occurrence(Some(occurrence));
                            self.walk_at(fill, inner, capture, child, governor)?;
                        }
                        fill.occurrence(None);
                    }
                }
            }
            Expr::Reference(id) => match grammar.layout(*id) {
                Some(text) if capture.is_none() && fill.layouts() => {
                    governor.charge(Resource::InputBytes, size(text.len()))?;
                    self.emit(text, governor)?;
                }
                _ => self.walk_at(fill, grammar.production_expr(*id), capture, child, governor)?,
            },
            Expr::Commit | Expr::Insert(_) => {}
            Expr::Next => fill.next_call(governor)?,
        }
        Ok(())
    }
    fn print_class<F: Fill>(
        &mut self,
        fill: &mut F,
        id: ClassId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<(), ScratchPrintError> {
        let mut ordinal = None;
        if capture.is_none() && self.composite {
            let text = fill.ordinal(governor)?;
            if self.accepts(id, text.as_str().as_bytes(), governor)? {
                ordinal = Some(text);
            }
        }
        let text = match ordinal {
            Some(text) => text,
            None => fill.class(self, id, capture, governor)?,
        };
        if !self.accepts(id, text.as_str().as_bytes(), governor)? {
            return Err(ScratchPrintError::Unrepresentable { text });
        }
        self.emit(text.as_str().as_bytes(), governor)?;
        Ok(())
    }
    fn bounded(
        &mut self,
        text: &str,
        delimiters: DelimiterSetId,
        governor: &mut Governor,
    ) -> Result<(), ScratchPrintError> {
        if text.is_empty() {
            return Ok(());
        }
        if self.cut(delimiters, text.as_bytes(), governor)? {
            return Err(ScratchPrintError::Unrepresentable {
                text: ScratchString::copy_from(text, governor)?,
            });
        }
        self.emit(text.as_bytes(), governor)?;
        Ok(())
    }
    pub(crate) fn cut(
        &self,
        id: DelimiterSetId,
        bytes: &[u8],
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        contains_any_governed(bytes, self.grammar.delimiters(id), governor)
    }
    pub(crate) fn holds(&self, expr: &Expr, governor: &mut Governor) -> Result<Holds, StopReason> {
        self.holds_at(expr, self.active_depth.saturating_add(1), governor)
    }
    fn holds_at(
        &self,
        expr: &Expr,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<Holds, StopReason> {
        let mut holds = Holds::default();
        self.collect_holds(expr, &mut holds, depth, governor)?;
        Ok(holds)
    }
    fn collect_holds(
        &self,
        expr: &Expr,
        holds: &mut Holds,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        visit(depth, governor)?;
        let child = depth.saturating_add(1);
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
                    holds.count = holds.count.saturating_add(1);
                }
                holds.kinds = holds.kinds.with(*kind);
                self.collect_holds(inner, holds, child, governor)?;
            }
            Expr::Repeat { inner, .. } => self.collect_holds(inner, holds, child, governor)?,
            Expr::Sequence(items) | Expr::Choice(items) => {
                for item in items {
                    self.collect_holds(item, holds, child, governor)?;
                }
            }
            Expr::Reference(id) => {
                self.collect_holds(self.grammar.production_expr(*id), holds, child, governor)?;
            }
            Expr::Next => holds.next = true,
            _ => {}
        }
        Ok(())
    }
    pub(crate) fn repaired(
        &self,
        expr: &Expr,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        self.repaired_at(expr, self.active_depth.saturating_add(1), governor)
    }
    fn repaired_at(
        &self,
        expr: &Expr,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        visit(depth, governor)?;
        let child = depth.saturating_add(1);
        Ok(match expr {
            Expr::Insert(_)
            | Expr::Capture {
                kind: CaptureKind::Dropped,
                ..
            } => true,
            Expr::Capture {
                kind: CaptureKind::Name,
                inner,
            } if !self.terminal(inner, child, governor)? => true,
            Expr::Capture { inner, .. } => self.repaired_at(inner, child, governor)?,
            Expr::Sequence(items) => {
                let mut found = false;
                for item in items {
                    if self.repaired_at(item, child, governor)? {
                        found = true;
                        break;
                    }
                }
                found
            }
            Expr::Choice(items) => {
                let mut all = true;
                for item in items {
                    if !self.repaired_at(item, child, governor)? {
                        all = false;
                        break;
                    }
                }
                all
            }
            Expr::Repeat { inner, min, .. } => {
                *min > 0 && self.repaired_at(inner, child, governor)?
            }
            Expr::Reference(id) => {
                self.repaired_at(self.grammar.production_expr(*id), child, governor)?
            }
            _ => false,
        })
    }
    fn name_fits(
        &self,
        expr: &Expr,
        name: Option<&str>,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        visit(depth, governor)?;
        let Some(name) = name else {
            return Ok(true);
        };
        let child = depth.saturating_add(1);
        match expr {
            Expr::Capture {
                kind: CaptureKind::Name,
                inner,
            } => {
                let mut inner = &**inner;
                let mut depth = child;
                while let Expr::Reference(id) = inner {
                    visit(depth, governor)?;
                    depth = depth.saturating_add(1);
                    inner = self.grammar.production_expr(*id);
                }
                match inner {
                    Expr::Class(id) => self.accepts(*id, name.as_bytes(), governor),
                    _ => Ok(true),
                }
            }
            Expr::Reference(id) => self.name_fits(
                self.grammar.production_expr(*id),
                Some(name),
                child,
                governor,
            ),
            Expr::Sequence(items) => {
                for item in items {
                    if self.holds_at(item, child, governor)?.name {
                        return self.name_fits(item, Some(name), child, governor);
                    }
                }
                Ok(true)
            }
            _ => Ok(true),
        }
    }
    pub(crate) fn bare_slot(
        &self,
        expr: &Expr,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        self.bare_slot_at(expr, self.active_depth.saturating_add(1), governor)
    }
    fn bare_slot_at(
        &self,
        expr: &Expr,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        visit(depth, governor)?;
        let child = depth.saturating_add(1);
        Ok(match expr {
            Expr::Slot { .. } => true,
            Expr::Sequence(items) | Expr::Choice(items) => {
                let mut found = false;
                for item in items {
                    if self.bare_slot_at(item, child, governor)? {
                        found = true;
                        break;
                    }
                }
                found
            }
            Expr::Repeat { inner, .. } => self.bare_slot_at(inner, child, governor)?,
            Expr::Reference(id) => {
                self.bare_slot_at(self.grammar.production_expr(*id), child, governor)?
            }
            _ => false,
        })
    }
    fn terminal(
        &self,
        expr: &Expr,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        visit(depth, governor)?;
        match expr {
            Expr::Literal(_) | Expr::Class(_) => Ok(true),
            Expr::Reference(id) => self.terminal(
                self.grammar.production_expr(*id),
                depth.saturating_add(1),
                governor,
            ),
            _ => Ok(false),
        }
    }
    pub(crate) fn printable(
        &self,
        expr: &Expr,
        available: Kinds,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        self.printable_at(
            expr,
            available,
            self.active_depth.saturating_add(1),
            governor,
        )
    }
    fn printable_at(
        &self,
        expr: &Expr,
        available: Kinds,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        visit(depth, governor)?;
        let child = depth.saturating_add(1);
        Ok(match expr {
            Expr::Capture {
                kind: CaptureKind::Id,
                inner,
            } if self.holds_at(inner, child, governor)?.name => {
                self.printable_at(inner, available, child, governor)?
            }
            Expr::Capture { kind, inner } => {
                available.contains(*kind) && self.printable_at(inner, available, child, governor)?
            }
            Expr::Sequence(items) => {
                let mut all = true;
                for item in items {
                    if !self.printable_at(item, available, child, governor)? {
                        all = false;
                        break;
                    }
                }
                all
            }
            Expr::Choice(items) => {
                let mut found = false;
                for item in items {
                    if self.printable_at(item, available, child, governor)? {
                        found = true;
                        break;
                    }
                }
                found
            }
            Expr::Repeat { inner, min, .. } => {
                *min == 0 || self.printable_at(inner, available, child, governor)?
            }
            Expr::Reference(id) => self.printable_at(
                self.grammar.production_expr(*id),
                available,
                child,
                governor,
            )?,
            _ => true,
        })
    }
    pub(crate) fn arguments_slot(
        &self,
        expr: &Expr,
        governor: &mut Governor,
    ) -> Result<Option<(CodecKind, DelimiterSetId)>, StopReason> {
        self.arguments_slot_at(expr, self.active_depth.saturating_add(1), governor)
    }
    fn arguments_slot_at(
        &self,
        expr: &Expr,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<Option<(CodecKind, DelimiterSetId)>, StopReason> {
        visit(depth, governor)?;
        let child = depth.saturating_add(1);
        match expr {
            Expr::Capture {
                kind: CaptureKind::Arguments | CaptureKind::ParamValue,
                inner,
            } => match **inner {
                Expr::Slot { codec, delimiters } => Ok(Some((codec, delimiters))),
                _ => self.arguments_slot_at(inner, child, governor),
            },
            Expr::Capture { inner, .. } | Expr::Repeat { inner, .. } => {
                self.arguments_slot_at(inner, child, governor)
            }
            Expr::Sequence(items) | Expr::Choice(items) => {
                for item in items {
                    if let Some(slot) = self.arguments_slot_at(item, child, governor)? {
                        return Ok(Some(slot));
                    }
                }
                Ok(None)
            }
            Expr::Reference(id) => {
                self.arguments_slot_at(self.grammar.production_expr(*id), child, governor)
            }
            _ => Ok(None),
        }
    }
    pub(crate) fn accepts(
        &self,
        id: ClassId,
        text: &[u8],
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        self.grammar.classes[id.index()].accepts(text, governor)
    }
    pub(crate) fn generate_class(
        &self,
        id: ClassId,
        min_len: usize,
        governor: &mut Governor,
    ) -> Result<ScratchString, ScratchPrintError> {
        let class = &self.grammar.classes[id.index()];
        for min_len in [min_len, 0] {
            let max = min_len.checked_add(16).ok_or(ScratchPrintError::Resource(
                StopReason::BudgetExceeded(Resource::Work),
            ))?;
            if let Some(text) = class.automaton.generate_member(min_len, max, governor)?
                && !text.is_empty()
            {
                return Ok(text);
            }
        }
        admit_text(&class.source, governor)?;
        Err(ScratchPrintError::NoCandidate {
            class: ScratchString::copy_from(&class.source, governor)?,
        })
    }
}

/// Canonical values and existing choice state; further bindings stay borrowed.
pub(crate) struct Canonical<'v> {
    values: Values<'v>,
    rest: &'v [Values<'v>],
    available: Kinds,
    occurrence: Option<usize>,
    printed: Kinds,
    position: usize,
}
impl<'v> Canonical<'v> {
    pub(crate) fn new(values: Values<'v>) -> Self {
        Self::group(values, &[])
    }
    pub(crate) fn group(first: Values<'v>, rest: &'v [Values<'v>]) -> Self {
        Self {
            available: Self::kinds(&first),
            values: first,
            rest,
            occurrence: None,
            printed: Kinds::default(),
            position: 0,
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
    pub(crate) const fn finished(&self) -> bool {
        self.rest.is_empty()
    }
    pub(crate) fn printable(
        &self,
        printer: &Printer<'_>,
        expr: &Expr,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        printer.printable(expr, self.available, governor)
    }
    fn missing(kind: CaptureKind, governor: &mut Governor) -> ScratchPrintError {
        unsupported_governed(
            format_args!("no value for a `{}:` capture", kind.name()),
            governor,
        )
    }
    fn member(&mut self, governor: &mut Governor) -> Result<&'v Member, ScratchPrintError> {
        self.printed = self
            .printed
            .with(CaptureKind::ParamName)
            .with(CaptureKind::ParamValue);
        let parameters = self
            .values
            .parameters
            .ok_or_else(|| Self::missing(CaptureKind::ParamName, governor))?;
        self.occurrence
            .and_then(|index| parameters.get(index))
            .ok_or_else(|| {
                unsupported_governed(
                    format_args!("a parameter capture outside a repetition"),
                    governor,
                )
            })
    }
    fn value(
        &mut self,
        kind: CaptureKind,
        governor: &mut Governor,
    ) -> Result<&'v str, ScratchPrintError> {
        self.printed = self.printed.with(kind);
        match kind {
            CaptureKind::Name => self.values.name,
            CaptureKind::Id => self.values.id,
            CaptureKind::Content => self.values.content,
            _ => None,
        }
        .ok_or_else(|| Self::missing(kind, governor))
    }
}
impl Fill for Canonical<'_> {
    fn name(&self) -> Option<&str> {
        self.values.name
    }
    fn choose(
        &mut self,
        printer: &Printer<'_>,
        alternatives: &[&Expr],
        governor: &mut Governor,
    ) -> Result<usize, ScratchPrintError> {
        let mut best: Option<(u32, usize)> = None;
        let mut clean = false;
        for alternative in alternatives {
            if !printer.repaired(alternative, governor)?
                && printer.printable(alternative, self.available, governor)?
            {
                clean = true;
                break;
            }
        }
        for (index, alternative) in alternatives.iter().enumerate() {
            if !printer.printable(alternative, self.available, governor)?
                || (clean && printer.repaired(alternative, governor)?)
            {
                continue;
            }
            let score = printer
                .holds(alternative, governor)?
                .kinds
                .shared(self.available);
            if best.is_none_or(|(top, _)| score > top) {
                best = Some((score, index));
            }
        }
        Ok(best.map_or(0, |(_, index)| index))
    }
    fn repetition(
        &mut self,
        printer: &Printer<'_>,
        inner: &Expr,
        min: u32,
        max: Option<u32>,
        governor: &mut Governor,
    ) -> Result<Repetition, ScratchPrintError> {
        let holds = printer.holds(inner, governor)?;
        if holds.next {
            let count = self.rest.len();
            if !fits(count, min, max) {
                return Err(unsupported_governed(
                    format_args!("{count} further calls do not fit a repetition of {min}..{max:?}"),
                    governor,
                ));
            }
            return Ok(Repetition::Times(u32::try_from(count).unwrap_or(u32::MAX)));
        }
        if holds.parameters {
            let count = self.values.parameters.map_or(0, <[Member]>::len);
            if !fits(count, min, max) {
                return Err(unsupported_governed(
                    format_args!("{count} parameters do not fit a repetition of {min}..{max:?}"),
                    governor,
                ));
            }
            return Ok(Repetition::Parameters(count));
        }
        let once = min.max(1);
        if let Expr::Reference(id) = inner
            && printer.grammar.layout(*id).is_some()
            && fits(once as usize, min, max)
        {
            return Ok(Repetition::Times(once));
        }
        let wanted = !holds.dropped
            && holds.kinds.shared(self.available) > 0
            && printer.printable(inner, self.available, governor)?
            && fits(once as usize, min, max);
        Ok(Repetition::Times(if wanted { once } else { min }))
    }
    fn occurrence(&mut self, occurrence: Option<usize>) {
        self.occurrence = occurrence;
    }
    fn literal(
        &mut self,
        capture: CaptureKind,
        bytes: &[u8],
        governor: &mut Governor,
    ) -> Result<(), ScratchPrintError> {
        if !matches!(capture, CaptureKind::Name | CaptureKind::Id) {
            return Ok(());
        }
        let value = self.value(capture, governor)?;
        governor.charge(
            Resource::Work,
            size(value.len()).saturating_add(size(bytes.len())),
        )?;
        if value.as_bytes() == bytes {
            Ok(())
        } else {
            Err(ScratchPrintError::Unrepresentable {
                text: ScratchString::copy_from(value, governor)?,
            })
        }
    }
    fn class(
        &mut self,
        printer: &mut Printer<'_>,
        id: ClassId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<ScratchString, ScratchPrintError> {
        match capture {
            None => printer.generate_class(id, 0, governor),
            Some(CaptureKind::ParamName) => {
                ScratchString::copy_from(&self.member(governor)?.key, governor)
                    .map_err(ScratchPrintError::from)
            }
            Some(CaptureKind::Content) => Ok(json::quote_scratch(
                self.value(CaptureKind::Content, governor)?,
                governor,
            )?),
            Some(kind @ (CaptureKind::Name | CaptureKind::Id)) => {
                ScratchString::copy_from(self.value(kind, governor)?, governor)
                    .map_err(ScratchPrintError::from)
            }
            Some(kind) => Err(Self::missing(kind, governor)),
        }
    }
    fn slot(
        &mut self,
        printer: &Printer<'_>,
        codec: CodecKind,
        delimiters: DelimiterSetId,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<ScratchString, ScratchPrintError> {
        match (capture, codec) {
            (Some(CaptureKind::Arguments), _) => {
                self.printed = self.printed.with(CaptureKind::Arguments);
                let value = self
                    .values
                    .arguments
                    .ok_or_else(|| Self::missing(CaptureKind::Arguments, governor))?;
                ScratchString::copy_from(value, governor).map_err(ScratchPrintError::from)
            }
            (Some(CaptureKind::ParamValue), CodecKind::Json | CodecKind::JsonArgs) => {
                let text = self.member(governor)?.value.to_json_scratch(governor)?;
                guard_json_governed(text, printer.grammar.delimiters(delimiters), governor)
            }
            (Some(CaptureKind::ParamValue), CodecKind::Raw) => {
                let value = &self.member(governor)?.value;
                match &value.value {
                    Value::String(text) => {
                        ScratchString::copy_from(text, governor).map_err(ScratchPrintError::from)
                    }
                    _ => Err(ScratchPrintError::Unrepresentable {
                        text: value.to_json_scratch(governor)?,
                    }),
                }
            }
            _ => Err(unsupported_governed(
                format_args!(
                    "a {} slot under {:?} cannot carry a value",
                    codec.name(),
                    capture.map(CaptureKind::name)
                ),
                governor,
            )),
        }
    }
    fn layouts(&self) -> bool {
        true
    }
    fn next_call(&mut self, governor: &mut Governor) -> Result<(), ScratchPrintError> {
        governor.charge(Resource::Work, 6)?;
        let Some((values, rest)) = self.rest.split_first() else {
            return Err(unsupported_governed(
                format_args!("a `next` with no further call to print"),
                governor,
            ));
        };
        self.available = Self::kinds(values);
        self.values = *values;
        self.rest = rest;
        self.occurrence = None;
        self.position = self
            .position
            .checked_add(1)
            .ok_or(ScratchPrintError::Resource(StopReason::BudgetExceeded(
                Resource::Work,
            )))?;
        Ok(())
    }
    fn ordinal(&mut self, governor: &mut Governor) -> Result<ScratchString, ScratchPrintError> {
        governor.charge(Resource::Work, u64::from(usize::BITS))?;
        Ok(format_scratch_governed(
            format_args!("{}", self.position),
            governor,
        )?)
    }
    fn composite_id(&mut self, _: &str, governor: &mut Governor) -> Result<(), ScratchPrintError> {
        governor.charge(Resource::Work, 1)?;
        self.printed = self.printed.with(CaptureKind::Id);
        Ok(())
    }
    fn until(
        &mut self,
        capture: Option<CaptureKind>,
        governor: &mut Governor,
    ) -> Result<ScratchString, ScratchPrintError> {
        match capture {
            None => Ok(ScratchString::new(governor)),
            Some(kind) => ScratchString::copy_from(self.value(kind, governor)?, governor)
                .map_err(ScratchPrintError::from),
        }
    }
}
