// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The intermediate representation of a recognized model turn.
//!
//! A [`RecognizedTurn`](super::RecognizedTurn) partitions the output bytes
//! into [`Segment`]s: content, reasoning, calls, tool results, and rejected
//! call attempts. Every call keeps its raw span, its dialect, its order, its
//! captured name and id, its arguments as a source-spanned value tree
//! ([`ArgTree`]), how it was closed ([`Closure`]), and every markup edit
//! the engine applied ([`MarkupEdit`]).
//!
//! ```
//! use purrassist_core::tool_call::{ArgTree, Closure, Spanned};
//! let empty = ArgTree::empty_object();
//! assert_eq!(empty.root.to_json(), "{}");
//! assert!(empty.leaves.is_empty());
//! let name = Spanned { value: "read_file".to_owned(), span: 6..15 };
//! assert_eq!(name.span.len(), name.value.len());
//! assert_ne!(Closure::Closed, Closure::Dangling);
//! ```
use super::DialectId;
use crate::contract::saturating_size;
use crate::grammar::CaptureKind;
use crate::grammar::edit::MarkupEdit;
use crate::json::{Member, Node, Value, pointer_segment};
use crate::provenance::Derivation;
use core::ops::Range;

/// A value with the absolute byte range it was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spanned<T> {
    /// The value.
    pub value: T,
    /// Its absolute source range.
    pub span: Range<usize>,
}

/// Which codec produced an argument leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArgOrigin {
    /// The verbatim `raw` codec.
    Raw,
}

/// Scratch bytes charged per [`ArgLeaf`] on top of its pointer text.
pub const LEAF_CHARGE: u64 = 64;

/// One leaf of an [`ArgTree`]: where it sits, which codec produced it, and
/// the exact source bytes it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgLeaf {
    /// JSON Pointer to the leaf within [`ArgTree::root`]. Duplicate member
    /// names share a pointer; the n-th leaf with a pointer is the n-th
    /// occurrence in document order.
    pub pointer: String,
    /// The codec that produced the leaf.
    pub origin: ArgOrigin,
    /// The leaf's verbatim source range.
    pub raw: Range<usize>,
}

/// A call's arguments: a source-spanned value tree plus, per leaf, its
/// origin and verbatim source range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgTree {
    /// The value tree; every node read from input carries its span.
    pub root: Node,
    /// Every leaf read from input, in document order.
    pub leaves: Vec<ArgLeaf>,
}

impl ArgTree {
    /// An empty object with no source: the arguments of a call whose
    /// markup carries no argument data.
    ///
    /// ```
    /// use purrassist_core::tool_call::ArgTree;
    /// let empty = ArgTree::empty_object();
    /// assert_eq!(empty.root.to_json(), "{}");
    /// assert!(empty.root.span.is_none() && empty.leaves.is_empty());
    /// ```
    #[must_use]
    pub const fn empty_object() -> Self {
        Self {
            root: Node {
                value: Value::Object(Vec::new()),
                span: None,
            },
            leaves: Vec::new(),
        }
    }

    /// A conservative scratch estimate of this tree: the value's
    /// [`Node::estimated_bytes`] plus [`LEAF_CHARGE`] and the pointer length
    /// per leaf. Codecs charge exactly this when they build a tree.
    ///
    /// ```
    /// use purrassist_core::tool_call::{ArgTree, LEAF_CHARGE};
    /// let empty = ArgTree::empty_object();
    /// assert_eq!(empty.estimated_bytes(), empty.root.estimated_bytes());
    /// assert!(LEAF_CHARGE > 0);
    /// ```
    #[must_use]
    pub fn estimated_bytes(&self) -> u64 {
        self.leaves
            .iter()
            .fold(self.root.estimated_bytes(), |total, leaf| {
                total
                    .saturating_add(LEAF_CHARGE)
                    .saturating_add(saturating_size(leaf.pointer.len()))
            })
    }

    /// An object built from `(name, value)` parameter pairs, in order;
    /// every leaf pointer is re-rooted under its parameter name.
    ///
    /// ```
    /// use purrassist_core::json::{Node, Value};
    /// use purrassist_core::tool_call::{ArgLeaf, ArgOrigin, ArgTree, Spanned};
    /// let value = ArgTree {
    ///     root: Node { value: Value::String("1".into()), span: Some(8..9) },
    ///     leaves: vec![ArgLeaf { pointer: String::new(), origin: ArgOrigin::Raw, raw: 8..9 }],
    /// };
    /// let tree = ArgTree::from_parameters(vec![(Spanned { value: "a/b".into(), span: 5..8 }, value)]);
    /// assert_eq!(tree.root.to_json(), r#"{"a/b":"1"}"#);
    /// assert_eq!(tree.leaves[0].pointer, "/a~1b");
    /// ```
    #[must_use]
    pub fn from_parameters(parameters: Vec<(Spanned<String>, Self)>) -> Self {
        let mut members = Vec::with_capacity(parameters.len());
        let mut leaves = Vec::new();
        for (name, value) in parameters {
            let prefix = format!("/{}", pointer_segment(&name.value));
            leaves.extend(value.leaves.into_iter().map(|leaf| ArgLeaf {
                pointer: format!("{prefix}{}", leaf.pointer),
                ..leaf
            }));
            members.push(Member {
                key: name.value,
                key_span: Some(name.span),
                value: value.root,
            });
        }
        Self {
            root: Node {
                value: Value::Object(members),
                span: None,
            },
            leaves,
        }
    }
}

/// How a recognized call's markup was closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Closure {
    /// Every terminal matched as written.
    Closed,
    /// Matched after markup edits made before the end of output, or after
    /// reinserting a closer the generation stop sequence consumed.
    Repaired,
    /// The output stopped (not by length) inside the call, and missing
    /// closers were inserted at the end of output. Whether such a call is
    /// complete enough to use is decided by the caller's completeness rule;
    /// otherwise it is malformed.
    Unterminated,
    /// The output was cut off by a length stop inside the call: truncation.
    /// The call holds what was captured before the cut.
    Dangling,
}

/// One named capture's span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureSpan {
    /// What was captured.
    pub kind: CaptureKind,
    /// The captured bytes.
    pub span: Range<usize>,
}

/// Whether a segment came from a dialect's `call` or `result` production.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProductionKind {
    /// A tool call.
    Call,
    /// A tool result.
    Result,
}

/// One reading of a recognized call (or tool result).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawCall {
    /// The call's whole source range, markup included.
    pub span: Range<usize>,
    /// The dialect that recognized it.
    pub dialect: DialectId,
    /// Whether this is a call or a tool result.
    pub kind: ProductionKind,
    /// The call's position among the calls of the turn's selected reading
    /// (see [`RecognizedTurn`](super::RecognizedTurn) for how alternatives
    /// are numbered).
    pub order: usize,
    /// The captured tool name.
    pub name: Option<Spanned<String>>,
    /// The captured call id.
    pub id: Option<Spanned<String>>,
    /// The arguments: an `arguments` slot's value, an object of parameter
    /// pairs, or an empty object when the markup carries no argument data.
    pub arguments: ArgTree,
    /// Every capture, ordered by start offset.
    pub captures: Vec<CaptureSpan>,
    /// How the markup was closed.
    pub closure: Closure,
    /// Every markup edit applied, in source order.
    pub edits: Vec<MarkupEdit>,
    /// The total edit cost of this reading.
    pub cost: u64,
}

impl RawCall {
    /// The captured text of the first capture of `kind`, when it is
    /// well-formed UTF-8.
    ///
    /// ```
    /// use purrassist_core::grammar::CaptureKind;
    /// # use purrassist_core::tool_call::{RawCall};
    /// fn content<'a>(call: &RawCall, source: &'a [u8]) -> Option<&'a str> {
    ///     call.capture_text(CaptureKind::Content, source)
    /// }
    /// ```
    #[must_use]
    pub fn capture_text<'a>(&self, kind: CaptureKind, source: &'a [u8]) -> Option<&'a str> {
        let capture = self.captures.iter().find(|capture| capture.kind == kind)?;
        core::str::from_utf8(source.get(capture.span.clone())?).ok()
    }
}

/// A recognized call or tool result: one or more equal-cost readings of
/// the same span, ranked by [`Forest::rank`](crate::provenance::Forest::rank).
/// Several readings are a local, per-call ambiguity: they never multiply
/// the turn's readings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSegment {
    /// The source range.
    pub span: Range<usize>,
    /// The readings, best first; never empty.
    pub readings: Vec<RawCall>,
    /// One derivation per reading, in the same order.
    pub derivations: Vec<Derivation>,
}

/// Why a call attempt was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rejection {
    /// No configuration could consume the byte at `at`, and no repair
    /// within the search budget applied.
    Malformed {
        /// Absolute offset of the byte that could not be consumed.
        at: usize,
    },
    /// Captured text or a slot body is not well-formed UTF-8.
    InvalidUtf8 {
        /// Absolute offset of the malformed sequence.
        at: usize,
    },
    /// The output stopped (not by length) inside the call and no
    /// end-of-output repair completes it.
    Unterminated,
    /// An earlier, still-open attempt of the same dialect absorbed this
    /// attempt's markup at `at`.
    Overlapped {
        /// Absolute offset where the attempt was absorbed.
        at: usize,
    },
}

/// How a reasoning block ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReasoningClosure {
    /// By its closing marker.
    Closed,
    /// Unclosed, and closed at the first call opener by the guess-flagged
    /// rule `tool_call.reasoning.unclosed`.
    ClosedAtOpener,
    /// Unclosed, with no call opener: reasoning to the end of output, with a
    /// loss recorded by `tool_call.reasoning.unterminated`.
    Unterminated,
}

/// A reasoning block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReasoningSegment {
    /// The source range, markers included.
    pub span: Range<usize>,
    /// How the block ended.
    pub closure: ReasoningClosure,
    /// Calls recognized inside the closed block. They remain reasoning text;
    /// each reading carries the guess-flagged edit
    /// `tool_call.reasoning.embedded-call`.
    pub embedded: Vec<CallSegment>,
    /// The reasoning edits (`unclosed` or `unterminated`) applied.
    pub edits: Vec<MarkupEdit>,
}

/// One segment of a recognized turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// Plain assistant content.
    Content(Range<usize>),
    /// A reasoning block.
    Reasoning(ReasoningSegment),
    /// A tool call.
    Call(CallSegment),
    /// A tool result.
    Result(CallSegment),
    /// A call attempt whose markup could not be recognized; the raw span is
    /// retained.
    Rejected {
        /// The attempt's source range, from its opener to where it failed.
        span: Range<usize>,
        /// The dialect whose opener started the attempt.
        dialect: DialectId,
        /// Why it was rejected.
        reason: Rejection,
    },
}

impl Segment {
    /// The segment's source range.
    ///
    /// ```
    /// use purrassist_core::tool_call::Segment;
    /// assert_eq!(Segment::Content(3..9).span(), 3..9);
    /// ```
    #[must_use]
    pub fn span(&self) -> Range<usize> {
        match self {
            Self::Content(span) | Self::Rejected { span, .. } => span.clone(),
            Self::Reasoning(reasoning) => reasoning.span.clone(),
            Self::Call(call) | Self::Result(call) => call.span.clone(),
        }
    }
}

/// A span where the turn has several competing segmentations (overlapping
/// call spans or overlapping dialects). Each reading partitions `span`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The contested source range.
    pub span: Range<usize>,
    /// The competing segmentations, best first; at least two.
    pub readings: Vec<Vec<Segment>>,
    /// One derivation per reading, in the same order.
    pub derivations: Vec<Derivation>,
}

/// One piece of a factorized turn: a settled segment, or a local choice
/// between segmentations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// A segment every reading shares.
    Segment(Segment),
    /// Competing segmentations of one span.
    Choice(Choice),
}

impl Piece {
    /// The piece's source range.
    ///
    /// ```
    /// use purrassist_core::tool_call::{Piece, Segment};
    /// assert_eq!(Piece::Segment(Segment::Content(0..4)).span(), 0..4);
    /// ```
    #[must_use]
    pub fn span(&self) -> Range<usize> {
        match self {
            Self::Segment(segment) => segment.span(),
            Self::Choice(choice) => choice.span.clone(),
        }
    }
}
