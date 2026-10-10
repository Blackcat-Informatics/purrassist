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

/// Which codec produced an argument leaf, and so how its
/// [`ArgLeaf::raw`] bytes relate to its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArgOrigin {
    /// The verbatim `raw` codec: the raw bytes are the string value.
    Raw,
    /// A JSON value read by the `json` codec (directly, or as a typed
    /// `xml-params` value): the raw bytes are the value's JSON spelling.
    Json,
    /// A verbatim XML parameter string read by the `xml-params` codec: the
    /// raw bytes are the string value (framing newlines excluded).
    XmlText,
    /// A JSON value decoded from inside a stringified-arguments JSON string:
    /// the raw bytes are the escaped source text that encodes the value (or
    /// the whole enclosing string when the escape decoding could not be
    /// mapped exactly).
    StringifiedJson,
}

/// Why an argument value could not be decoded. The value is absent from
/// [`ArgTree::root`]; its raw bytes are kept in [`UnresolvedArgument::raw`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArgumentFault {
    /// `InvalidUtf8InArgument`: the value's bytes are not well-formed UTF-8;
    /// `at` is the absolute offset of the first malformed sequence. The
    /// built-in codecs no longer produce it (they read malformed bytes as
    /// U+FFFD and record `tool_call.value.invalid-utf8`, a data loss); it
    /// remains for custom codecs and stable wire values.
    InvalidUtf8InArgument {
        /// Absolute offset of the malformed sequence.
        at: usize,
    },
    /// JSON that tolerant syntax recovery cannot repair (a mismatched
    /// closer, a member without a key, ...), or text after the JSON value;
    /// `at` is the absolute offset where decoding stopped.
    InvalidJson {
        /// Absolute offset where decoding stopped.
        at: usize,
    },
    /// `stringified_arguments_invalid_json`: the arguments were a JSON string
    /// whose content is not one JSON object (unrecoverable syntax, text after
    /// the value, or a non-object value).
    StringifiedArgumentsInvalidJson,
}

impl core::fmt::Display for ArgumentFault {
    /// A human-readable statement of the fault, with its absolute offset.
    ///
    /// ```
    /// use purrassist_core::tool_call::ArgumentFault;
    /// assert_eq!(
    ///     ArgumentFault::InvalidJson { at: 7 }.to_string(),
    ///     "JSON that cannot be recovered at byte 7"
    /// );
    /// ```
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidUtf8InArgument { at } => {
                write!(formatter, "bytes at {at} are not well-formed UTF-8")
            }
            Self::InvalidJson { at } => {
                write!(formatter, "JSON that cannot be recovered at byte {at}")
            }
            Self::StringifiedArgumentsInvalidJson => formatter
                .write_str("the stringified arguments do not hold one recoverable JSON object"),
        }
    }
}

/// One argument value that could not be decoded.
///
/// ```
/// use purrassist_core::tool_call::{ArgumentFault, UnresolvedArgument};
/// let unresolved = UnresolvedArgument {
///     pointer: "/path".into(),
///     fault: ArgumentFault::InvalidUtf8InArgument { at: 12 },
///     raw: 10..14,
/// };
/// assert!(unresolved.raw.contains(&12));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedArgument {
    /// JSON Pointer the value would have had; empty when the whole
    /// arguments value is unresolved.
    pub pointer: String,
    /// Why it could not be decoded.
    pub fault: ArgumentFault,
    /// The value's verbatim source range.
    pub raw: Range<usize>,
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
/// origin and verbatim source range, every value that could not be decoded,
/// and how the arguments were encoded.
///
/// A value listed in [`Self::unresolved`] is absent from [`Self::root`];
/// when the whole arguments value is unresolved (an entry with the empty
/// pointer), `root` is an empty-object stand-in spanning the raw bytes.
/// A call with any unresolved argument cannot be executed as recognized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgTree {
    /// The value tree; every node read from input carries its span.
    pub root: Node,
    /// Every leaf read from input, in document order.
    pub leaves: Vec<ArgLeaf>,
    /// Every value that could not be decoded, in document order.
    pub unresolved: Vec<UnresolvedArgument>,
    /// `arguments_were_stringified`: the arguments arrived as a JSON string
    /// whose content was decoded as JSON (`"arguments": "{\"a\":1}"`). Such
    /// arguments must be validated against the tool's schema before use.
    pub arguments_were_stringified: bool,
    /// The canonical name of the declared tool whose
    /// [`ArgumentShape`](super::ArgumentShape) specialized decoding (see
    /// [`ToolSet::argument_shape`](super::ToolSet::argument_shape)), or
    /// `None` when no declared tool was known to the codec.
    pub tool: Option<String>,
    /// Array members whose repeated occurrences the codec merged into one
    /// array, where the typed shape does not declare the member (a `$ref`
    /// or `anyOf` scope), in document order. Resolution records each as
    /// `tool_call.parameter.collect`, as it does for a declared array
    /// parameter's occurrences.
    pub collected: Vec<CollectedMember>,
}

/// A repeated array member the codec merged into one array (see
/// [`ArgTree::collected`]).
///
/// ```
/// use purrassist_core::tool_call::CollectedMember;
/// let merged = CollectedMember { pointer: "/children".into(), key_span: 3..11, occurrences: 2 };
/// assert_eq!(merged.occurrences, 2);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectedMember {
    /// The merged member's pointer in the arguments.
    pub pointer: String,
    /// The byte range of the first occurrence's name.
    pub key_span: Range<usize>,
    /// How many occurrences were merged (at least two).
    pub occurrences: usize,
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
        Self::value(Node {
            value: Value::Object(Vec::new()),
            span: None,
        })
    }

    /// A tree holding `root` with no leaves, no unresolved values, and no
    /// tool.
    ///
    /// ```
    /// use purrassist_core::json::{Node, Value};
    /// use purrassist_core::tool_call::ArgTree;
    /// let tree = ArgTree::value(Node { value: Value::Null, span: Some(3..7) });
    /// assert_eq!(tree.root.to_json(), "null");
    /// assert!(tree.leaves.is_empty() && tree.unresolved.is_empty() && tree.tool.is_none());
    /// ```
    #[must_use]
    pub const fn value(root: Node) -> Self {
        Self {
            root,
            leaves: Vec::new(),
            unresolved: Vec::new(),
            arguments_were_stringified: false,
            tool: None,
            collected: Vec::new(),
        }
    }

    /// An argument value that could not be decoded: an empty-object
    /// stand-in spanning `raw`, with one unresolved entry at the empty
    /// pointer.
    ///
    /// ```
    /// use purrassist_core::tool_call::{ArgTree, ArgumentFault};
    /// let tree = ArgTree::unresolved(ArgumentFault::InvalidJson { at: 4 }, 2..9);
    /// assert!(!tree.is_resolved());
    /// assert_eq!(tree.unresolved[0].pointer, "");
    /// assert_eq!(tree.root.span, Some(2..9));
    /// ```
    #[must_use]
    pub fn unresolved(fault: ArgumentFault, raw: Range<usize>) -> Self {
        let mut tree = Self::value(Node {
            value: Value::Object(Vec::new()),
            span: Some(raw.clone()),
        });
        tree.unresolved.push(UnresolvedArgument {
            pointer: String::new(),
            fault,
            raw,
        });
        tree
    }

    /// Whether every argument value was decoded.
    ///
    /// See [`ArgTree::unresolved`] for an example.
    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        self.unresolved.is_empty()
    }

    /// Whether the whole value is unresolved (`root` is the empty stand-in).
    pub(crate) fn is_stand_in(&self) -> bool {
        self.unresolved
            .iter()
            .any(|unresolved| unresolved.pointer.is_empty())
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
        let pointers = self
            .leaves
            .iter()
            .map(|leaf| leaf.pointer.len())
            .chain(self.unresolved.iter().map(|value| value.pointer.len()));
        pointers.fold(
            self.root
                .estimated_bytes()
                .saturating_add(saturating_size(self.tool.as_ref().map_or(0, String::len))),
            |total, pointer| {
                total
                    .saturating_add(LEAF_CHARGE)
                    .saturating_add(saturating_size(pointer))
            },
        )
    }

    /// An object built from `(name, value)` parameter pairs, in order;
    /// every leaf pointer is re-rooted under its parameter name.
    ///
    /// Unresolved values are re-rooted the same way; a parameter whose whole
    /// value is unresolved contributes no member. The result is stringified
    /// when any parameter was.
    ///
    /// ```
    /// use purrassist_core::json::{Node, Value};
    /// use purrassist_core::tool_call::{ArgLeaf, ArgOrigin, ArgTree, ArgumentFault, Spanned};
    /// let mut value = ArgTree::value(Node { value: Value::String("1".into()), span: Some(8..9) });
    /// value.leaves.push(ArgLeaf { pointer: String::new(), origin: ArgOrigin::Raw, raw: 8..9 });
    /// let broken = ArgTree::unresolved(ArgumentFault::InvalidUtf8InArgument { at: 20 }, 20..21);
    /// let tree = ArgTree::from_parameters(vec![
    ///     (Spanned { value: "a/b".into(), span: 5..8 }, value),
    ///     (Spanned { value: "c".into(), span: 17..18 }, broken),
    /// ]);
    /// assert_eq!(tree.root.to_json(), r#"{"a/b":"1"}"#);
    /// assert_eq!(tree.leaves[0].pointer, "/a~1b");
    /// assert_eq!(tree.unresolved[0].pointer, "/c");
    /// ```
    #[must_use]
    pub fn from_parameters(parameters: Vec<(Spanned<String>, Self)>) -> Self {
        let mut tree = Self::empty_object();
        for (name, value) in parameters {
            tree.adopt(name, value);
        }
        tree
    }

    /// Append `value` as member `name` of this object tree, re-rooting its
    /// leaves and unresolved values under the member's pointer.
    ///
    /// A repeated name keeps every occurrence as a duplicate member, and
    /// resolution collects the occurrences of an array parameter into one
    /// array, in order. An array occurrence is therefore re-rooted past the
    /// items the earlier occurrences hold (`/outfits/1/wearer` for the second
    /// occurrence's item, not `/outfits/0/wearer` again), so every leaf and
    /// unresolved value names the item it was read from.
    pub(crate) fn adopt(&mut self, name: Spanned<String>, value: Self) {
        self.adopt_collecting(name, value, false);
    }

    /// [`Self::adopt`], and with `collect` an array `value` is appended to
    /// the array an earlier occurrence of `name` already holds instead of
    /// becoming a duplicate member: the occurrences of an array member of a
    /// scope the typed model does not declare (a `$ref` or `anyOf` item)
    /// are collected here, as resolution collects those of a declared array
    /// parameter.
    pub(crate) fn adopt_collecting(
        &mut self,
        name: Spanned<String>,
        mut value: Self,
        collect: bool,
    ) {
        let prefix = format!("/{}", pointer_segment(&name.value));
        let whole = value.is_stand_in();
        if matches!(value.root.value, Value::Array(_)) && !whole {
            let earlier = match &self.root.value {
                Value::Object(members) => members
                    .iter()
                    .filter(|member| member.key == name.value)
                    .map(|member| match &member.value.value {
                        Value::Array(items) => items.len(),
                        _ => 1,
                    })
                    .sum(),
                _ => 0,
            };
            if earlier > 0 {
                for leaf in &mut value.leaves {
                    leaf.pointer = shift_item(&leaf.pointer, earlier);
                }
                for unresolved in &mut value.unresolved {
                    unresolved.pointer = shift_item(&unresolved.pointer, earlier);
                }
                for merged in &mut value.collected {
                    merged.pointer = shift_item(&merged.pointer, earlier);
                }
            }
        }
        self.collected
            .extend(value.collected.into_iter().map(|merged| CollectedMember {
                pointer: format!("{prefix}{}", merged.pointer),
                ..merged
            }));
        self.leaves
            .extend(value.leaves.into_iter().map(|leaf| ArgLeaf {
                pointer: format!("{prefix}{}", leaf.pointer),
                ..leaf
            }));
        self.unresolved.extend(
            value
                .unresolved
                .into_iter()
                .map(|unresolved| UnresolvedArgument {
                    pointer: format!("{prefix}{}", unresolved.pointer),
                    ..unresolved
                }),
        );
        self.arguments_were_stringified |= value.arguments_were_stringified;
        if whole {
            return;
        }
        if let Value::Object(members) = &mut self.root.value {
            if collect
                && let Value::Array(items) = &mut value.root.value
                && let Some(earlier) = members.iter_mut().find(|member| {
                    member.key == name.value && matches!(member.value.value, Value::Array(_))
                })
                && let Value::Array(into) = &mut earlier.value.value
            {
                into.append(items);
                let first = earlier
                    .key_span
                    .clone()
                    .unwrap_or_else(|| name.span.clone());
                match self
                    .collected
                    .iter_mut()
                    .find(|merged| merged.pointer == prefix)
                {
                    Some(merged) => merged.occurrences += 1,
                    None => self.collected.push(CollectedMember {
                        pointer: prefix,
                        key_span: first,
                        occurrences: 2,
                    }),
                }
                earlier.value.span = match (earlier.value.span.take(), value.root.span.take()) {
                    (Some(first), Some(last)) => {
                        Some(first.start.min(last.start)..first.end.max(last.end))
                    }
                    (span, other) => span.or(other),
                };
                return;
            }
            members.push(Member {
                key: name.value,
                key_span: Some(name.span),
                value: value.root,
            });
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
    /// Whether the call's first source byte is `{`: no opener token (a
    /// tag, a special token, a wrapper) comes before its JSON object, so
    /// the object's own members are the only evidence that it is a call.
    /// A `declaration-content` grammar weighs such a call against the
    /// declared tool when its arguments have a tool declaration's shape
    /// ([`CompiledGrammar::declaration_content`](crate::grammar::CompiledGrammar::declaration_content)).
    pub bare: bool,
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
        /// Absolute offset of the byte that could not be consumed, or of
        /// the first byte of the well-formed character it belongs to. It
        /// lies inside the rejected span, at its end, or past it only
        /// inside a literal the failing reading began at or before that end
        /// and matched only in part, which the span leaves out (see
        /// `RejectedCall::span`).
        at: usize,
    },
    /// The attempt failed at a byte beginning a sequence that is not
    /// well-formed UTF-8 where its grammar admits none (a tool name: every
    /// built-in name grammar is an ASCII identifier), or a custom codec
    /// faulted on one. Bytes inside an argument value, an id or a parameter
    /// name are read as U+FFFD instead (`tool_call.value.invalid-utf8`).
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
    /// Call markup of a family no allowed dialect reads: a family not
    /// built in yet, or a built-in dialect the request's allow-list leaves
    /// out ([`detect_unparsed`](super::detect_unparsed)). No parser read
    /// it; its bytes also stay content.
    UnsupportedDialect {
        /// The detected family.
        family: super::MarkupFamily,
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
///
/// `text` is `span` without its markers, so slicing the output by it never
/// panics for a segment the recognizer produced:
///
/// ```
/// use purrassist_core::tool_call::{ReasoningClosure, ReasoningSegment};
/// let output = b"<think>abc</think>Done.";
/// let segment = ReasoningSegment {
///     span: 0..18,
///     text: 7..10,
///     closure: ReasoningClosure::Closed,
///     embedded: Vec::new(),
///     edits: Vec::new(),
/// };
/// assert_eq!(&output[segment.text.clone()], b"abc");
/// assert!(segment.span.start <= segment.text.start && segment.text.end <= segment.span.end);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReasoningSegment {
    /// The source range, markers included.
    pub span: Range<usize>,
    /// The block's text: `span` without its markers. It begins where the
    /// opening marker ends (at `0` for a block the turn starts in, which has
    /// no opener) and ends where the closing marker begins; a block closed at
    /// a call opener ends at the opener, and an unterminated one at the end of
    /// output. The bytes are verbatim source offsets, never unescaped
    /// (Anthropic thinking text stays JSON-escaped). For an Anthropic
    /// `redacted_thinking` block the text is the opaque encrypted `data`
    /// payload, not reasoning prose. Invariant: `text.start <= text.end` and
    /// `text` lies inside `span`; a closing marker that begins before the
    /// text does not close the block.
    pub text: Range<usize>,
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
        /// The attempt's source range, from its opener to where its grammar
        /// stopped reading: the end of the last token it read whole (a
        /// literal matched only in part is left out). Recovery extends it
        /// over the attempt's own markup (`RejectedCall::span`).
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

/// `pointer`, whose first segment is an array index, with `by` added to the
/// index; a pointer without an index segment is unchanged.
fn shift_item(pointer: &str, by: usize) -> String {
    let Some(rest) = pointer.strip_prefix('/') else {
        return pointer.to_owned();
    };
    let (index, tail) = rest
        .split_once('/')
        .map_or((rest, ""), |(index, tail)| (index, tail));
    let Ok(index) = index.parse::<usize>() else {
        return pointer.to_owned();
    };
    if tail.is_empty() {
        format!("/{}", index.saturating_add(by))
    } else {
        format!("/{}/{tail}", index.saturating_add(by))
    }
}
