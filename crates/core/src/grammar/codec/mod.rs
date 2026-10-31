// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Argument codecs: the sub-machines that read a codec slot's body.
//!
//! A slot `slot(CODEC, "delimiter", ...)` hands every body byte to its
//! codec, in order, once the engine knows the byte cannot begin one of the
//! slot's delimiters (the engine holds back at most the longest delimiter's
//! length). The codec either consumes the byte, reports that its value
//! ended earlier ([`CodecStep::Complete`], for self-delimiting codecs), or
//! reports a [`CodecFault`]. When the body ends, at a delimiter, at the end
//! of input, or where a self-delimiting value completed, [`Codec::finish`]
//! turns it into a [`CodecValue`]: a source-spanned [`ArgTree`] plus the
//! [`MarkupEdit`]s the codec recorded while reading it. The engine appends
//! those edits to the call's edits (they become evidence like any markup
//! edit) and adds their costs to the call's cost.
//!
//! Codecs form a closed set ([`CodecKind`]) dispatched by `match` over the
//! engine's per-slot codec state: there is no dynamic dispatch per byte.
//! Adding a codec means adding a [`CodecKind`] variant, its [`Codec`]
//! implementation, and its arm in each state dispatch; the engine itself is
//! unchanged.
//!
//! | Codec | Slot name | Value |
//! | --- | --- | --- |
//! | [`RawCodec`] | `raw` | the verbatim body as one string leaf |
//! | [`JsonCodec`] | `json` | one JSON value, tolerant of syntax errors; a string holding JSON is decoded as stringified arguments |
//! | [`KvPairsCodec`] | `kv-pairs` | alternating key/value elements, typed by the declared tool, with verbatim string content |
//! | [`PythonicCodec`] | `pythonic` | keyword arguments with native Python literals, self-delimited before the call closer |
//! | [`XmlParamsCodec`] | `xml-params` | `<parameter=NAME>value</parameter>` sequences, typed by the call's declared tool |
//!
//! # Context
//!
//! A codec may depend on the call's tool: [`Codec::finish`] receives a
//! [`CodecContext`] holding the [`ArgumentShape`] of the declared tool the
//! call names, when the recognizer was given a [`ToolSet`](crate::tool_call::ToolSet)
//! ([`Recognizer::with_tools`](crate::tool_call::Recognizer::with_tools))
//! and the grammar captured the call's `name:` before the slot. Lookup is
//! exact ([`ToolSet::lookup_exact`](crate::tool_call::ToolSet::lookup_exact):
//! canonical names and aliases). With no shape, every codec still decodes,
//! without type information; such a call is later resolved against its tool
//! (or found to name none).
//!
//! # Malformed UTF-8
//!
//! Every built-in codec reads bytes that are not well-formed UTF-8 as
//! U+FFFD, one per maximal malformed subpart (the substitution the Unicode
//! Standard recommends), and records each run as a
//! [`EditOperator::InvalidUtf8Value`] edit (`tool_call.value.invalid-utf8`)
//! whose span and original bytes are exact: never a silent replacement. It
//! is a data loss, so the call is kept with its decodable content and is
//! at most a best guess, never run on a `Mutating` tool. Offsets after the
//! replacement are unchanged. A final character cut off by a length stop
//! is dropped instead (the value is truncated, not malformed).
//!
//! # Determinism
//!
//! Codec state advances one byte at a time and every charge depends only on
//! the bytes, so values, edits and governor usage are identical however the
//! recognizer's input was split.
//!
//! ```
//! use purrassist_core::grammar::codec::{Codec, CodecContext, CodecEnd, CodecStep, RawCodec};
//! use purrassist_core::{Budget, Governor};
//! let mut governor = Governor::new(Budget::default());
//! let body = "héllo".as_bytes();
//! let mut codec = RawCodec::begin(10, &mut governor)?;
//! for (index, &byte) in body.iter().enumerate() {
//!     assert_eq!(codec.feed(byte, 10 + index, &mut governor)?, CodecStep::Consumed);
//! }
//! let context = CodecContext::default();
//! let value = codec.finish(body, 10..16, CodecEnd::Delimited, context, &mut governor)?.unwrap();
//! assert_eq!(value.tree.root.as_str(), Some("héllo"));
//! assert_eq!(value.tree.leaves[0].raw, 10..16);
//! assert!(value.edits.is_empty());
//! # Ok::<(), purrassist_core::StopReason>(())
//! ```
mod json;
mod kv;
mod python;
mod python_names;
mod raw;
mod xml;

pub use json::JsonCodec;
pub use kv::KvPairsCodec;
pub use python::PythonicCodec;
pub use raw::RawCodec;
pub use xml::XmlParamsCodec;

/// The `xml-params` codec's canonical parameter closer.
pub(crate) const XML_CLOSER: &[u8] = b"</parameter>";
/// The variant closer spelling read as [`XML_CLOSER`].
pub(crate) const XML_CLOSER_VARIANT: &[u8] = b"</parameters>";

use super::edit::{EditOperator, MarkupEdit};
use crate::contract::{Governor, Resource, ScratchLease, ScratchVec, StopReason, saturating_size};
use crate::tool_call::{ArgTree, ArgumentShape};
use core::ops::Range;

/// Canonical literal emission behind the codec module's crate-facing boundary.
pub(crate) fn print_pythonic(
    arguments: &crate::json::Node,
    governor: &mut Governor,
) -> Result<String, crate::grammar::print::PrintError> {
    python::print(arguments, governor)
}

/// The closed set of argument codecs a slot can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CodecKind {
    /// Verbatim capture: the body is one UTF-8 string.
    Raw,
    /// One tolerant JSON value (see [`JsonCodec`]).
    Json,
    /// One tolerant JSON value that must be an arguments value: an object,
    /// or a string that is empty or holds stringified arguments (its text
    /// begins with `{` after spaces). A value beginning any other way
    /// faults at that byte, so a grammar without other evidence that its
    /// text is a call does not read `{"name": ..., "arguments": 3}` or
    /// `{"name": ..., "arguments": "prose"}` as one.
    JsonArgs,
    /// XML parameter elements (see [`XmlParamsCodec`]).
    XmlParams,
    /// GLM key/value elements, typed by the declared tool (see [`KvPairsCodec`]).
    KvPairs,
    /// Native Python keyword arguments and literal values.
    Pythonic,
}

impl CodecKind {
    /// Every codec, in declaration order.
    pub const ALL: [Self; 6] = [
        Self::Raw,
        Self::Json,
        Self::JsonArgs,
        Self::XmlParams,
        Self::KvPairs,
        Self::Pythonic,
    ];

    /// The slot name this codec is registered under.
    ///
    /// ```
    /// use purrassist_core::grammar::codec::CodecKind;
    /// assert_eq!(CodecKind::Raw.name(), "raw");
    /// assert_eq!(CodecKind::XmlParams.name(), "xml-params");
    /// ```
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Json => "json",
            Self::JsonArgs => "json-args",
            Self::XmlParams => "xml-params",
            Self::KvPairs => "kv-pairs",
            Self::Pythonic => "pythonic",
        }
    }

    /// The codec registered under `name`.
    ///
    /// ```
    /// use purrassist_core::grammar::codec::CodecKind;
    /// assert_eq!(CodecKind::from_name("raw"), Some(CodecKind::Raw));
    /// assert_eq!(CodecKind::from_name("json"), Some(CodecKind::Json));
    /// assert_eq!(CodecKind::from_name("yaml"), None);
    /// ```
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "raw" => Some(Self::Raw),
            "json" => Some(Self::Json),
            "json-args" => Some(Self::JsonArgs),
            "xml-params" => Some(Self::XmlParams),
            "kv-pairs" => Some(Self::KvPairs),
            "pythonic" => Some(Self::Pythonic),
            _ => None,
        }
    }

    /// Whether [`Codec::finish`] reads its [`CodecContext`], so the engine
    /// must look up the call's tool before finishing the slot.
    pub(crate) const fn uses_context(self) -> bool {
        !matches!(self, Self::Raw)
    }

    pub(crate) fn begin(
        self,
        at: usize,
        governor: &mut Governor,
    ) -> Result<CodecState, StopReason> {
        Ok(match self {
            Self::Raw => CodecState::Raw(RawCodec::begin(at, governor)?),
            Self::Json => CodecState::Json(Box::new(JsonCodec::begin(at, governor)?)),
            Self::JsonArgs => CodecState::Json(Box::new(JsonCodec::begin_arguments(at, governor)?)),
            Self::XmlParams => CodecState::XmlParams(XmlParamsCodec::begin(at, governor)?),
            Self::KvPairs => CodecState::KvPairs(KvPairsCodec::begin(at, governor)?),
            Self::Pythonic => CodecState::Pythonic(PythonicCodec::begin(at, governor)?),
        })
    }
}

/// What a codec did with one body byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecStep {
    /// The byte belongs to the value.
    Consumed,
    /// The value ended at absolute offset `end`, at or before the offered
    /// byte; the bytes from `end` on are not part of the body.
    Complete {
        /// Absolute offset just past the value.
        end: usize,
    },
    /// The body cannot be decoded.
    Fault(CodecFault),
}

/// Why a codec cannot decode its body.
///
/// A fault rejects the call attempt; argument bytes that merely fail to decode are recorded as unresolved
/// values of an otherwise recognized call instead (see
/// [`ArgTree::unresolved`](crate::tool_call::ArgTree::unresolved)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CodecFault {
    /// Pythonic calls require keyword arguments; a positional argument was found.
    PythonicPositionalRejected {
        /// Original source offset of the positional argument.
        at: usize,
    },
    /// The body is not well-formed UTF-8; `at` is the absolute offset of
    /// the first byte of the malformed sequence. No built-in codec
    /// produces it: they read malformed bytes as U+FFFD and record
    /// `tool_call.value.invalid-utf8`, a data loss that keeps the call.
    InvalidUtf8 {
        /// Absolute offset of the malformed sequence.
        at: usize,
    },
    /// The body's structure is malformed (the `xml-params` codec): text
    /// outside any parameter, text after a whole-JSON body, or a typed value
    /// left incomplete where a declared parameter's opener ended it.
    Malformed {
        /// Absolute offset of the offending byte.
        at: usize,
    },
}

/// How a slot body ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecEnd {
    /// At one of the slot's delimiters, or where a self-delimiting value
    /// completed.
    Delimited,
    /// At the end of input, under a stop that completes the output.
    EndOfInput,
    /// At the end of input, under a length stop: the body was cut off.
    Truncated,
}

/// What a codec may know about the call whose body it decodes.
///
/// Markup borrows read-only owners prepared by [`Grammar::compile_governed`](crate::grammar::Grammar::compile_governed).
/// Preparation can return a typed resource error; borrowing the resulting
/// context does not allocate or readmit the bytes.
///
/// ```
/// use purrassist_core::grammar::{Grammar, codec::CodecContext};
/// use purrassist_core::{Budget, Governor, tool_call::{Param, ToolDefinition, ToolSet}};
/// let tools = ToolSet::compile([ToolDefinition::new("read").param("path", Param::string())], Budget::default()).unwrap();
/// let source = r#"grammar "context"; call = "<c>" "</c>"; result = "<r>"; declaration = "<d>";"#;
/// let grammar = Grammar::compile_governed(source, &mut Governor::new(Budget::default())).unwrap();
/// let alternative = &grammar.call_ends()[0];
/// let context = CodecContext {
///     shape: tools.argument_shape("read"),
///     openers: alternative.openers(),
///     closers: alternative.call_closers(),
/// };
/// assert_eq!(context.openers[0].as_slice(), b"<c>");
/// assert_eq!(context.shape.unwrap().tool, "read");
/// assert!(CodecContext::default().shape.is_none());
/// assert!(CodecContext::default().openers.is_empty());
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct CodecContext<'t> {
    /// The argument shape of the declared tool the call names, when known.
    pub shape: Option<&'t ArgumentShape>,
    /// Every spelling of a call opener of the dialect the body belongs to
    /// (Qwen3-Coder XML: `<tool_call>`, `<function=`): a value holding call
    /// markup holds whole calls, whose own markup is not the value's (see
    /// `XmlParamsCodec`). Empty when unknown.
    pub openers: &'t [ScratchVec<u8>],
    /// Every spelling of a literal ending a call of that dialect
    /// (`</function>`, `</tool_call>` and their variants), as the value may
    /// hold them: closer text a value holds is weighed against the markup
    /// it would otherwise be (see `XmlParamsCodec`). Empty when unknown.
    pub closers: &'t [ScratchVec<u8>],
}

/// A decoded slot body: the value and the edits recorded while reading it,
/// in source order, and the body's independently ambiguous readings.
///
/// `alternative` is set only by the `xml-params` codec, for a body holding
/// a value with text after an unpaired closer (see [`XmlParamsCodec`]): the
/// reading that holds the text, and the alternative that ends the value at
/// that closer and drops the text. The engine records both readings of the
/// body side by side (their edits cost the same), so the call is
/// `Ambiguous`. Independent ambiguous values produce a chain containing every
/// combination; the chain is consumed and dropped without recursive traversal.
///
/// ```
/// use purrassist_core::grammar::codec::{Codec, CodecContext, CodecEnd, XmlParamsCodec};
/// use purrassist_core::{Budget, Governor};
/// let mut governor = Governor::new(Budget::default());
/// let body = b"<parameter=a>\nrm -rf build</parameter> Then I set b.\n</parameter>";
/// let value = XmlParamsCodec::begin(0, &mut governor)?
///     .finish(body, 0..body.len(), CodecEnd::Delimited, CodecContext::default(), &mut governor)?
///     .unwrap();
/// assert_eq!(value.tree.root.to_json(), r#"{"a":"rm -rf build</parameter> Then I set b."}"#);
/// let other = value.alternative.as_deref().unwrap();
/// assert_eq!(other.tree.root.to_json(), r#"{"a":"rm -rf build"}"#);
/// assert!(other.alternative.is_none());
/// let cost = |edits: &[purrassist_core::grammar::edit::MarkupEdit]| -> u64 {
///     edits.iter().map(|edit| edit.rule().cost).sum()
/// };
/// assert_eq!(cost(&value.edits), cost(&other.edits));
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
pub struct CodecValue {
    /// The value.
    pub tree: ArgTree,
    /// Every edit the codec recorded, in source order.
    pub edits: Vec<MarkupEdit>,
    /// The next independently selected reading, if one remains. Every reading
    /// retains its own edits; the engine keeps all equally least-cost readings.
    pub alternative: Option<CodecAlternative>,
}

/// One owned link to an alternative codec reading. Dereferencing borrows the
/// reading; `link.into_value()` transfers it and its remaining alternatives.
/// Its destructor releases arbitrarily long chains iteratively.
pub struct CodecAlternative {
    value: Vec<CodecValue>,
    lease: Option<ScratchLease>,
}

impl CodecAlternative {
    pub(crate) fn new(value: CodecValue, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(Resource::Work, 1)?;
        let lease = governor.lease_scratch(saturating_size(core::mem::size_of::<CodecValue>()))?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(1)
            .map_err(|error| crate::contract::reserve_failure(&error))?;
        if values.capacity() != 1 {
            return Err(governor.stop_resource(Resource::ScratchBytes));
        }
        values.push(value);
        Ok(Self {
            value: values,
            lease: Some(lease),
        })
    }

    // Ordinary public Clone has no governor; production branching uses new().
    fn cloned(value: CodecValue) -> Self {
        Self {
            value: vec![value],
            lease: None,
        }
    }

    /// Consume this link and return its original reading without cloning.
    /// For example, `value.alternative.map(CodecAlternative::into_value)` moves
    /// the next reading into ordinary owned form, keeping its edits and suffix.
    ///
    /// # Panics
    /// Panics if the private single-reading invariant is broken. Construction
    /// and public access preserve that invariant.
    #[must_use]
    pub fn into_value(mut self) -> CodecValue {
        self.value
            .pop()
            .expect("a live alternative owns its reading")
    }
}

impl core::ops::Deref for CodecAlternative {
    type Target = CodecValue;
    fn deref(&self) -> &Self::Target {
        self.value
            .first()
            .expect("a live alternative owns its reading")
    }
}
impl core::ops::DerefMut for CodecAlternative {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
            .first_mut()
            .expect("a live alternative owns its reading")
    }
}
impl Drop for CodecAlternative {
    fn drop(&mut self) {
        while let Some(mut value) = self.value.pop() {
            let buffer = core::mem::take(&mut self.value);
            let lease = self.lease.take();
            if let Some(mut next) = value.alternative.take() {
                self.value = core::mem::take(&mut next.value);
                self.lease = next.lease.take();
            }
            drop(value);
            drop(buffer);
            drop(lease);
        }
    }
}
impl Clone for CodecAlternative {
    fn clone(&self) -> Self {
        Self::cloned((**self).clone())
    }
}
impl core::fmt::Debug for CodecAlternative {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&**self, formatter)
    }
}
impl PartialEq for CodecAlternative {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl Eq for CodecAlternative {}

impl Clone for CodecValue {
    fn clone(&self) -> Self {
        let mut result = Self {
            tree: self.tree.clone(),
            edits: self.edits.clone(),
            alternative: None,
        };
        let mut tail = &mut result.alternative;
        let mut source = self.alternative.as_deref();
        while let Some(value) = source {
            *tail = Some(CodecAlternative::cloned(Self {
                tree: value.tree.clone(),
                edits: value.edits.clone(),
                alternative: None,
            }));
            tail = &mut tail.as_mut().expect("the reading was appended").alternative;
            source = value.alternative.as_deref();
        }
        result
    }
}
impl core::fmt::Debug for CodecValue {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut readings = formatter.debug_list();
        let mut next = Some(self);
        while let Some(value) = next {
            readings.entry(&(&value.tree, &value.edits));
            next = value.alternative.as_deref();
        }
        readings.finish()
    }
}
impl PartialEq for CodecValue {
    fn eq(&self, other: &Self) -> bool {
        let (mut left, mut right) = (Some(self), Some(other));
        loop {
            match (left, right) {
                (Some(a), Some(b)) if a.tree == b.tree && a.edits == b.edits => {
                    left = a.alternative.as_deref();
                    right = b.alternative.as_deref();
                }
                (None, None) => return true,
                _ => return false,
            }
        }
    }
}
impl Eq for CodecValue {}

impl CodecValue {
    /// Move one reading out while detaching its remaining alternatives.
    pub(crate) fn into_parts(self) -> (ArgTree, Vec<MarkupEdit>, Option<CodecAlternative>) {
        (self.tree, self.edits, self.alternative)
    }
}

/// One argument codec: a push-based sub-machine over a slot body.
pub trait Codec: Sized {
    /// The kind this implementation provides.
    const KIND: CodecKind;

    /// A codec for a body starting at absolute offset `at`.
    ///
    /// # Errors
    /// Returns the governor's exhaustion or cancellation reason.
    fn begin(at: usize, governor: &mut Governor) -> Result<Self, StopReason>;

    /// Offer the next body byte, at absolute offset `at`.
    ///
    /// The default consumes every byte: a codec that reads its whole body
    /// in [`Self::finish`] (`raw`, `xml-params`) keeps it.
    ///
    /// # Errors
    /// Returns the governor's exhaustion or cancellation reason.
    fn feed(
        &mut self,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<CodecStep, StopReason> {
        let _ = (byte, at, governor);
        Ok(CodecStep::Consumed)
    }

    /// Build the value of the whole `body`, whose absolute range is `span`,
    /// charging exactly the value's [`ArgTree::estimated_bytes`] and each
    /// returned edit's retained size of `ScratchBytes`.
    ///
    /// # Errors
    /// The outer `Err` is the governor's exhaustion or cancellation reason;
    /// `Ok(Err(fault))` reports a body the codec cannot decode.
    fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason>;
}

/// Incremental UTF-8 validation, one comparison per byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Utf8Check {
    remaining: u8,
    low: u8,
    high: u8,
    pending: u8,
    sequence: usize,
    fault: Option<usize>,
}

impl Utf8Check {
    pub(crate) const fn new() -> Self {
        Self {
            remaining: 0,
            low: 0x80,
            high: 0xBF,
            pending: 0,
            sequence: 0,
            fault: None,
        }
    }

    /// Check the byte at `at`; the offset of the malformed sequence once one
    /// is found (sticky).
    pub(crate) const fn step(&mut self, byte: u8, at: usize) -> Option<usize> {
        if self.fault.is_some() {
            return self.fault;
        }
        if self.remaining == 0 {
            let (remaining, low, high) = match byte {
                0x00..=0x7F => return None,
                0xC2..=0xDF => (1, 0x80, 0xBF),
                0xE0 => (2, 0xA0, 0xBF),
                0xED => (2, 0x80, 0x9F),
                0xE1..=0xEF => (2, 0x80, 0xBF),
                0xF0 => (3, 0x90, 0xBF),
                0xF1..=0xF3 => (3, 0x80, 0xBF),
                0xF4 => (3, 0x80, 0x8F),
                _ => {
                    self.fault = Some(at);
                    return self.fault;
                }
            };
            self.remaining = remaining;
            self.low = low;
            self.high = high;
            self.pending = 1;
            self.sequence = at;
            return None;
        }
        if byte < self.low || byte > self.high {
            self.fault = Some(self.sequence);
            return self.fault;
        }
        self.remaining -= 1;
        self.pending += 1;
        self.low = 0x80;
        self.high = 0xBF;
        if self.remaining == 0 {
            self.pending = 0;
        }
        None
    }

    /// Whether the last checked byte completed a character (or was ASCII).
    pub(crate) const fn at_boundary(&self) -> bool {
        self.pending == 0 && self.fault.is_none()
    }

    /// The offset of the malformed sequence, given how the input ended: a
    /// cut-off final character is malformed unless a length stop cut it.
    pub(crate) const fn malformed(&self, end: CodecEnd) -> Option<usize> {
        if self.fault.is_some() {
            return self.fault;
        }
        if self.pending == 0 || matches!(end, CodecEnd::Truncated) {
            None
        } else {
            Some(self.sequence)
        }
    }
}

/// A charged edit of absolute `span` within a body starting at `base`.
fn edit(
    operator: EditOperator,
    span: Range<usize>,
    body: &[u8],
    base: usize,
    replacement: Vec<u8>,
    governor: &mut Governor,
) -> Result<MarkupEdit, StopReason> {
    let edit = MarkupEdit {
        operator,
        original: body[span.start - base..span.end - base].to_vec(),
        span,
        replacement,
    };
    governor.charge(Resource::ScratchBytes, edit.estimated_bytes())?;
    Ok(edit)
}

/// `bytes`, which start at absolute offset `base`, decoded as UTF-8 with
/// each maximal malformed subpart read as U+FFFD (the substitution of
/// maximal subparts the Unicode Standard recommends, as
/// `String::from_utf8_lossy` does), and one
/// [`EditOperator::InvalidUtf8Value`] edit per run of malformed bytes: its
/// span and original bytes are exact, its replacement the U+FFFDs read.
/// Callers charge the bytes as they read them; well-formed bytes cost
/// nothing more, and a malformed body charges one more `Work` unit per
/// byte (the substitution pass) and the edits' retained size.
pub(crate) fn lossy(
    bytes: &[u8],
    base: usize,
    governor: &mut Governor,
) -> Result<(String, Vec<MarkupEdit>), StopReason> {
    if let Ok(text) = core::str::from_utf8(bytes) {
        return Ok((text.to_owned(), Vec::new()));
    }
    governor.charge(Resource::Work, saturating_size(bytes.len()))?;
    let mut text = String::with_capacity(bytes.len());
    let mut edits: Vec<MarkupEdit> = Vec::new();
    let mut at = base;
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        at += chunk.valid().len();
        let invalid = chunk.invalid();
        if invalid.is_empty() {
            continue;
        }
        text.push(char::REPLACEMENT_CHARACTER);
        let end = at + invalid.len();
        match edits.last_mut() {
            // Adjacent malformed subparts are one run.
            Some(last) if chunk.valid().is_empty() && last.span.end == at => {
                last.span.end = end;
                last.original.extend_from_slice(invalid);
                last.replacement.extend_from_slice("\u{FFFD}".as_bytes());
            }
            _ => edits.push(MarkupEdit {
                operator: EditOperator::InvalidUtf8Value,
                span: at..end,
                original: invalid.to_vec(),
                replacement: "\u{FFFD}".as_bytes().to_vec(),
            }),
        }
        at = end;
    }
    for edit in &edits {
        governor.charge(Resource::ScratchBytes, edit.estimated_bytes())?;
    }
    Ok((text, edits))
}

/// The length of `body` without a final multi-byte character a length
/// stop cut off (its leading bytes are not malformed, only incomplete).
pub(crate) fn uncut(body: &[u8]) -> usize {
    let from = body.len().saturating_sub(3);
    for start in (from..body.len()).rev() {
        match body[start] {
            0xC0..=0xFF => {
                return match core::str::from_utf8(&body[start..]) {
                    Err(error) if error.error_len().is_none() => start,
                    _ => body.len(),
                };
            }
            0x80..=0xBF => {}
            _ => break,
        }
    }
    body.len()
}

/// Charge a finished tree's [`ArgTree::estimated_bytes`] and return it as
/// a codec value.
fn value(
    tree: ArgTree,
    edits: Vec<MarkupEdit>,
    governor: &mut Governor,
) -> Result<Result<CodecValue, CodecFault>, StopReason> {
    governor.charge(Resource::ScratchBytes, tree.estimated_bytes())?;
    Ok(Ok(CodecValue {
        tree,
        edits,
        alternative: None,
    }))
}

/// The live state of one codec slot: one variant per [`CodecKind`].
#[derive(Debug)]
pub(crate) enum CodecState {
    /// A [`RawCodec`].
    Raw(RawCodec),
    /// A [`JsonCodec`], boxed: its parser state is large.
    Json(Box<JsonCodec>),
    /// An [`XmlParamsCodec`].
    XmlParams(XmlParamsCodec),
    /// A [`KvPairsCodec`].
    KvPairs(KvPairsCodec),
    /// A [`PythonicCodec`].
    Pythonic(PythonicCodec),
}

impl CodecState {
    pub(crate) const fn kind(&self) -> CodecKind {
        match self {
            Self::Raw(_) => CodecKind::Raw,
            Self::Json(codec) if codec.arguments_only() => CodecKind::JsonArgs,
            Self::Json(_) => CodecKind::Json,
            Self::XmlParams(_) => CodecKind::XmlParams,
            Self::KvPairs(_) => CodecKind::KvPairs,
            Self::Pythonic(_) => CodecKind::Pythonic,
        }
    }

    /// Scratch bytes held out of line, beyond the engine's per-slot charge.
    pub(crate) const fn boxed_bytes(&self) -> u64 {
        match self {
            Self::Json(_) => super::cost::JSON_CODEC_STATE,
            Self::Raw(_) | Self::XmlParams(_) | Self::KvPairs(_) | Self::Pythonic(_) => 0,
        }
    }

    /// Whether a self-delimiting value is inside one of its strings (see
    /// [`JsonCodec::in_string`]); `false` for the other codecs.
    pub(crate) const fn in_string(&self) -> bool {
        match self {
            Self::Json(codec) => codec.in_string(),
            Self::Raw(_) | Self::XmlParams(_) | Self::KvPairs(_) | Self::Pythonic(_) => false,
        }
    }

    /// Where a self-delimiting value already complete ends: the end of
    /// input then ends it with no recovery edit. `None` for a value still
    /// open, and for the codecs that are not self-delimiting.
    pub(crate) const fn complete_end(&self) -> Option<usize> {
        match self {
            Self::Json(codec) => codec.complete_end(),
            Self::Raw(_) | Self::XmlParams(_) | Self::KvPairs(_) => None,
            Self::Pythonic(codec) => codec.complete_end(),
        }
    }

    pub(crate) fn feed(
        &mut self,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<CodecStep, StopReason> {
        match self {
            Self::Raw(codec) => codec.feed(byte, at, governor),
            Self::Json(codec) => codec.feed(byte, at, governor),
            Self::XmlParams(codec) => codec.feed(byte, at, governor),
            Self::KvPairs(codec) => codec.feed(byte, at, governor),
            Self::Pythonic(codec) => codec.feed(byte, at, governor),
        }
    }

    pub(crate) fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        match self {
            Self::Raw(codec) => codec.finish(body, span, end, context, governor),
            Self::Json(codec) => codec.finish(body, span, end, context, governor),
            Self::XmlParams(codec) => codec.finish(body, span, end, context, governor),
            Self::KvPairs(codec) => codec.finish(body, span, end, context, governor),
            Self::Pythonic(codec) => codec.finish(body, span, end, context, governor),
        }
    }
}

const _: () = assert!(core::mem::size_of::<JsonCodec>() as u64 <= super::cost::JSON_CODEC_STATE);
