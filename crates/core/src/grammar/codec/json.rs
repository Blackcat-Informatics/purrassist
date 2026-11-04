// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The `json` codec: one tolerant JSON value, and stringified arguments.
use super::{
    Codec, CodecContext, CodecEnd, CodecFault, CodecKind, CodecStep, CodecValue, Utf8Check, edit,
    value,
};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason};
use crate::grammar::cost::K_CODEC;
use crate::grammar::edit::{EditOperator, MarkupEdit};
use crate::json::incremental::{IncrementalRecovery, decode_escape};
use crate::json::{Node, SyntaxEdit, Value, pointer_segment};
use crate::tool_call::{ArgLeaf, ArgOrigin, ArgTree, ArgumentFault};
use core::fmt::Write as _;
use core::ops::Range;

/// The `json` codec: one JSON value read by the push-based tolerant
/// recovery parser ([`crate::json::incremental`]), with absolute spans.
///
/// The codec is self-delimiting: the value ends after its trailing
/// whitespace and comments, at the first byte that cannot continue it, and
/// the grammar continues there ([`CodecStep::Complete`]); a slot delimiter
/// still ends the body first if it occurs inside the value. Every repair the
/// parser makes is recorded as a [`MarkupEdit`] with the parser's rule id
/// (see [`EditOperator::json`]); numbers keep their exact spelling, and
/// nothing is coerced. Every scalar becomes an [`ArgLeaf`] with origin
/// [`ArgOrigin::Json`] and its verbatim source range.
///
/// The codec reads a whole arguments value: when that value is a JSON
/// *string*, its content is decoded as JSON in turn (stringified arguments,
/// as in `"arguments": "{\"a\":1}"`). The decoded object replaces the
/// string, the tree is flagged
/// [`arguments_were_stringified`](ArgTree::arguments_were_stringified), and
/// a `tool_call.arguments.decode-string` edit is recorded. Spans inside the
/// string are mapped back through the escape decoding to the exact source
/// bytes that encode each value; when the string itself needed repair (an
/// unterminated or non-`"`-quoted string, an invalid escape kept literally),
/// exact mapping is impossible and every inner span is the enclosing
/// string's span. Content that is not one recoverable JSON object leaves the
/// arguments unresolved with
/// [`ArgumentFault::StringifiedArgumentsInvalidJson`], keeping the string's
/// span.
///
/// Bytes that are not well-formed UTF-8 are read as U+FFFD, one per
/// maximal malformed subpart, wherever they occur (offsets after them stay
/// exact), and each run is recorded as a `tool_call.value.invalid-utf8`
/// edit with its exact span and original bytes: a data loss, so the value
/// keeps its decodable content and the call is at most a best guess. JSON
/// the parser cannot recover leaves the arguments unresolved
/// ([`ArgumentFault::InvalidJson`]) with the whole body as their raw span;
/// the body then runs to the slot's delimiter. The codec never faults. With a [`CodecContext::shape`], the
/// tree carries the tool's canonical name ([`ArgTree::tool`]) for
/// definition-grounded resolution.
///
/// Resources: the parser charges `Work` per byte and value, `Depth` per
/// nesting level, `EvidenceEntries` per repair, and its storage as it grows
/// (retained for the operation); [`Codec::finish`] additionally charges the
/// returned tree's [`ArgTree::estimated_bytes`], each edit's retained size,
/// and [`K_CODEC`] `Work` per tree node (and per byte of a stringified
/// string).
///
/// ```
/// use purrassist_core::grammar::codec::{Codec, CodecContext, CodecEnd, CodecStep, JsonCodec};
/// use purrassist_core::{Budget, Governor};
/// let mut governor = Governor::new(Budget::default());
/// let source = br#"{"n": 1.50, "s": 'x',} </end>"#;
/// let mut codec = JsonCodec::begin(0, &mut governor)?;
/// let mut end = source.len();
/// for (at, &byte) in source.iter().enumerate() {
///     if let CodecStep::Complete { end: stop } = codec.feed(byte, at, &mut governor)? {
///         end = stop;
///         break;
///     }
/// }
/// assert_eq!(&source[end..], b"</end>");
/// let value = codec
///     .finish(&source[..end], 0..end, CodecEnd::Delimited, CodecContext::default(), &mut governor)?
///     .unwrap();
/// assert_eq!(value.tree.root.to_json(), r#"{"n":1.50,"s":"x"}"#);
/// assert_eq!(value.tree.leaves[0].raw, 6..10);
/// let rules: Vec<_> = value.edits.iter().map(|edit| edit.rule().id).collect();
/// assert_eq!(rules, ["json.syntax.string", "json.syntax.trailing-comma"]);
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
#[derive(Debug)]
pub struct JsonCodec {
    utf8: Utf8Check,
    /// The bytes of a character not yet complete, held from the parser so
    /// a cut-off character never reaches it.
    held: [u8; 4],
    held_len: u8,
    parser: IncrementalRecovery,
    /// The first reason the value cannot be decoded.
    fault: Option<ArgumentFault>,
    /// The parser stopped on unrecoverable syntax: later bytes are only
    /// consumed.
    stalled: bool,
    /// What is still checked of the value's leading bytes.
    lead: Lead,
    /// Decoded root-string bytes already checked after an escaped lead.
    decoded_prefix: usize,
    /// Admission observation only; never changes native JSON acceptance.
    native_anchor: NativeAnchor,
    /// Runs of malformed UTF-8 read as U+FFFD: each run's absolute range
    /// and how many U+FFFD (maximal malformed subparts) it was read as.
    invalid: Vec<InvalidRun>,
}

/// A run of malformed UTF-8: its absolute range and how many U+FFFD
/// (maximal malformed subparts) it was read as.
type InvalidRun = (Range<usize>, usize);

/// The leading-byte check of the `json-args` codec ([`CodecKind::JsonArgs`]):
/// the value must be an object, or a string that is empty or begins with
/// `{` (stringified arguments).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lead {
    /// The `json` codec: any value.
    Any,
    /// `json-args`, before the value's first byte.
    Value,
    /// `json-args`, after the value's opening `"`.
    Quoted,
    /// A possible escaped stringified-object lead; the native parser decodes it.
    DecodedQuoted,
    /// `json-args`, the value's leading bytes were checked.
    Checked,
}

#[derive(Clone, Copy, Debug)]
enum NativeAnchor {
    /// Only newly decoded root-string bytes need examination.
    Pending(usize),
    Object,
    Other,
}

/// A JSON value decoded from a byte range.
pub(super) enum Decoded {
    /// The recovered value and its edits; `incomplete` when recovery had to
    /// complete it at the end of its input (closing a container, inserting a
    /// missing value, closing a string or an escape).
    Value {
        node: Node,
        edits: Vec<SyntaxEdit>,
        incomplete: bool,
    },
    /// The bytes cannot be decoded.
    Fault(ArgumentFault),
}

impl JsonCodec {
    /// The `json-args` codec ([`CodecKind::JsonArgs`]): like
    /// [`Codec::begin`], but the value must be an object, or a string that
    /// is empty or whose text begins with `{` after spaces (stringified
    /// arguments); any other leading byte faults ([`CodecFault::Malformed`]).
    ///
    /// # Errors
    /// As [`Codec::begin`].
    ///
    /// ```
    /// use purrassist_core::grammar::codec::{Codec, CodecFault, CodecStep, JsonCodec};
    /// use purrassist_core::{Budget, Governor};
    /// let mut governor = Governor::new(Budget::default());
    /// let mut codec = JsonCodec::begin_arguments(0, &mut governor)?;
    /// assert_eq!(codec.feed(b' ', 0, &mut governor)?, CodecStep::Consumed);
    /// assert_eq!(codec.feed(b'3', 1, &mut governor)?, CodecStep::Fault(CodecFault::Malformed { at: 1 }));
    /// // A string must hold stringified arguments.
    /// let mut prose = JsonCodec::begin_arguments(0, &mut governor)?;
    /// assert_eq!(prose.feed(b'"', 0, &mut governor)?, CodecStep::Consumed);
    /// assert_eq!(prose.feed(b'h', 1, &mut governor)?, CodecStep::Fault(CodecFault::Malformed { at: 1 }));
    /// let mut stringified = JsonCodec::begin_arguments(0, &mut governor)?;
    /// assert_eq!(stringified.feed(b'"', 0, &mut governor)?, CodecStep::Consumed);
    /// assert_eq!(stringified.feed(b'{', 1, &mut governor)?, CodecStep::Consumed);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    pub fn begin_arguments(at: usize, governor: &mut Governor) -> Result<Self, StopReason> {
        let mut codec = <Self as Codec>::begin(at, governor)?;
        codec.lead = Lead::Value;
        Ok(codec)
    }

    /// Whether this is the `json-args` codec.
    #[must_use]
    pub const fn arguments_only(&self) -> bool {
        !matches!(self.lead, Lead::Any)
    }

    /// Whether the arguments-only scanner has retained object-shaped evidence.
    pub(crate) const fn argument_anchor(&self) -> bool {
        matches!(self.lead, Lead::Checked)
    }

    /// Observe the existing native parser, without imposing json-args policy.
    /// Decoded string prefixes are examined once, including native escapes.
    pub(crate) fn observe_native_anchor(
        &mut self,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        governor.charge(Resource::Work, 1)?;
        let NativeAnchor::Pending(mut checked) = self.native_anchor else {
            return Ok(matches!(self.native_anchor, NativeAnchor::Object));
        };
        if self.parser.root_object_open() {
            self.native_anchor = NativeAnchor::Object;
            return Ok(true);
        }
        if let Some(prefix) = self.parser.root_string_prefix() {
            let pending = prefix
                .get(checked..)
                .expect("native decoded root strings only append to their prefix");
            for character in pending.chars() {
                governor.charge(Resource::Work, size(character.len_utf8()))?;
                checked += character.len_utf8();
                if !character.is_whitespace() {
                    let object = character == '{';
                    self.native_anchor = if object {
                        NativeAnchor::Object
                    } else {
                        NativeAnchor::Other
                    };
                    return Ok(object);
                }
            }
        }
        self.native_anchor = NativeAnchor::Pending(checked);
        Ok(false)
    }
}

impl Codec for JsonCodec {
    const KIND: CodecKind = CodecKind::Json;

    fn begin(at: usize, governor: &mut Governor) -> Result<Self, StopReason> {
        Ok(Self {
            utf8: Utf8Check::new(),
            held: [0; 4],
            held_len: 0,
            parser: IncrementalRecovery::starting_at(at, governor).require_key_colons(),
            fault: None,
            stalled: false,
            lead: Lead::Any,
            decoded_prefix: 0,
            native_anchor: NativeAnchor::Pending(0),
            invalid: Vec::new(),
        })
    }

    fn feed(
        &mut self,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<CodecStep, StopReason> {
        if self.stalled {
            return Ok(CodecStep::Consumed);
        }
        match self.lead {
            Lead::Any | Lead::Checked | Lead::DecodedQuoted => {}
            // The string's first character: a stringified object (after
            // spaces), or the closing quote of an empty string.
            Lead::Quoted if byte == b' ' => {}
            Lead::Quoted if byte == b'\\' => self.lead = Lead::DecodedQuoted,
            Lead::Quoted if matches!(byte, b'{' | b'"') => self.lead = Lead::Checked,
            Lead::Value if byte.is_ascii_whitespace() => {}
            Lead::Value if byte == b'{' => self.lead = Lead::Checked,
            Lead::Value if byte == b'"' => self.lead = Lead::Quoted,
            Lead::Quoted | Lead::Value => {
                return Ok(CodecStep::Fault(CodecFault::Malformed { at }));
            }
        }
        // A malformed sequence is read as one U+FFFD (the parser keeps
        // its source width, so offsets are unchanged) and recorded; the
        // byte that revealed it is then checked afresh.
        if let Some(bad) = self.utf8.step(byte, at) {
            self.utf8 = Utf8Check::new();
            let held = usize::from(core::mem::take(&mut self.held_len));
            if held > 0
                && let Some(step) = self.replace(bad, held, governor)?
            {
                return Ok(step);
            }
            if self.utf8.step(byte, at).is_some() {
                self.utf8 = Utf8Check::new();
                return Ok(self
                    .replace(at, 1, governor)?
                    .unwrap_or(CodecStep::Consumed));
            }
        }
        self.held[usize::from(self.held_len)] = byte;
        self.held_len += 1;
        if !self.utf8.at_boundary() {
            return Ok(CodecStep::Consumed);
        }
        let held = usize::from(core::mem::take(&mut self.held_len));
        let chunk = self.held;
        let fed = self.parser.feed(&chunk[..held], governor);
        let step = self
            .settle(fed, at + 1 - held)?
            .unwrap_or(CodecStep::Consumed);
        if self.lead == Lead::DecodedQuoted
            && !self.stalled
            && let Some(fault) = self.decoded_anchor(at, governor)?
        {
            return Ok(CodecStep::Fault(fault));
        }
        Ok(step)
    }

    fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        let (decoded, invalid) = self.decode(end, governor)?;
        let (mut tree, mut edits) = match decoded {
            Decoded::Fault(fault) => (ArgTree::unresolved(fault, span.clone()), Vec::new()),
            Decoded::Value { node, edits, .. } => {
                if matches!(node.value, Value::String(_)) {
                    stringified(node, edits, body, span.start, governor)?
                } else {
                    tree(node, edits, ArgOrigin::Json, body, span.start, governor)?
                }
            }
        };
        if !invalid.is_empty() {
            edits.extend(invalid_edits(&invalid, body, span.start, governor)?);
            edits.sort_by_key(|edit| (edit.span.start, edit.span.end));
        }
        tree.tool = context.shape.map(|shape| shape.tool.clone());
        value(tree, edits, governor)
    }
}

impl JsonCodec {
    /// Use the native escape decoder once per newly decoded prefix byte.
    fn decoded_anchor(
        &mut self,
        at: usize,
        governor: &mut Governor,
    ) -> Result<Option<CodecFault>, StopReason> {
        let Some(prefix) = self.parser.root_string_prefix() else {
            return Ok(None);
        };
        let pending = prefix
            .get(self.decoded_prefix..)
            .expect("native decoded root strings only append to their prefix");
        for character in pending.chars() {
            governor.charge(
                Resource::Work,
                u64::try_from(character.len_utf8()).unwrap_or(4),
            )?;
            self.decoded_prefix += character.len_utf8();
            if character.is_whitespace() {
                continue;
            }
            if character == '{' {
                self.lead = Lead::Checked;
                return Ok(None);
            }
            return Ok(Some(CodecFault::Malformed { at }));
        }
        if self.parser.value_complete() {
            self.lead = Lead::Checked;
        }
        Ok(None)
    }

    /// Feed one U+FFFD for the `width` malformed bytes at `at` and record
    /// the run; the step to report when the value ended or stalled.
    fn replace(
        &mut self,
        at: usize,
        width: usize,
        governor: &mut Governor,
    ) -> Result<Option<CodecStep>, StopReason> {
        match self.invalid.last_mut() {
            Some((run, count)) if run.end == at => {
                run.end = at + width;
                *count += 1;
            }
            _ => {
                governor.charge(Resource::ScratchBytes, 32)?;
                self.invalid.push((at..at + width, 1));
            }
        }
        let fed = self.parser.feed_replacement(width, governor);
        self.settle(fed, at)
    }

    /// The step after feeding the parser bytes from `from`: a stall on
    /// unrecoverable syntax (later bytes are only consumed), the value's
    /// completion, or `None` to go on.
    fn settle(
        &mut self,
        fed: Result<(), StopReason>,
        from: usize,
    ) -> Result<Option<CodecStep>, StopReason> {
        match fed {
            Ok(()) => {}
            Err(StopReason::InvalidInput(_)) => {
                let at = self.parser.fault_at().unwrap_or(from);
                self.fault.get_or_insert(ArgumentFault::InvalidJson { at });
                self.stalled = true;
                return Ok(Some(CodecStep::Consumed));
            }
            Err(reason) => return Err(reason),
        }
        Ok(self.parser.is_done().then(|| CodecStep::Complete {
            end: self.parser.position(),
        }))
    }

    /// Whether the value fed so far is inside one of its strings, with no
    /// fault: a delimiter written here is text of the string.
    pub(crate) const fn in_string(&self) -> bool {
        !self.stalled && self.fault.is_none() && self.parser.in_string()
    }

    /// Where the value fed so far ends, when it is already one complete
    /// JSON value (only whitespace may follow it) that well-formed UTF-8
    /// spelled: the end of input then ends it with no recovery edit.
    pub(crate) const fn complete_end(&self) -> Option<usize> {
        if self.stalled
            || self.fault.is_some()
            || self.held_len != 0
            || !self.parser.value_complete()
        {
            None
        } else {
            Some(self.parser.position())
        }
    }

    /// The decoded value, applying end-of-input repairs for an input that
    /// ended as `end` says.
    ///
    /// Also returns the malformed UTF-8 runs read as U+FFFD (a final
    /// character cut off by a length stop is dropped, not replaced).
    fn decode(
        mut self,
        end: CodecEnd,
        governor: &mut Governor,
    ) -> Result<(Decoded, Vec<InvalidRun>), StopReason> {
        if !self.stalled
            && let Some(at) = self.utf8.malformed(end)
        {
            let held = usize::from(core::mem::take(&mut self.held_len));
            self.replace(at, held, governor)?;
        }
        let invalid = core::mem::take(&mut self.invalid);
        Ok((self.decode_value(governor)?, invalid))
    }

    fn decode_value(self, governor: &mut Governor) -> Result<Decoded, StopReason> {
        if let Some(fault) = self.fault {
            return Ok(Decoded::Fault(fault));
        }
        let before = self.parser.edit_count();
        let stop = self.parser.position();
        let fault_at = self.parser.fault_at();
        match self.parser.finish(governor) {
            Ok((node, _, edits)) => {
                let incomplete = edits[before..]
                    .iter()
                    .any(|edit| edit.rule != "json.syntax.comment");
                Ok(Decoded::Value {
                    node,
                    edits,
                    incomplete,
                })
            }
            Err(StopReason::InvalidInput(_)) => Ok(Decoded::Fault(ArgumentFault::InvalidJson {
                at: fault_at.unwrap_or(stop),
            })),
            Err(reason) => Err(reason),
        }
    }

    /// Decode `bytes`, which start at absolute offset `at`, as one JSON
    /// value; the second result is where the value completed, when bytes
    /// remain after it (its trailing whitespace and comments included), and
    /// the third the value's malformed UTF-8 runs, read as U+FFFD, as
    /// `tool_call.value.invalid-utf8` edits.
    pub(super) fn scan(
        bytes: &[u8],
        at: usize,
        end: CodecEnd,
        governor: &mut Governor,
    ) -> Result<(Decoded, Option<usize>, Vec<MarkupEdit>), StopReason> {
        let mut codec = Self::begin(at, governor)?;
        let mut stop = None;
        for (index, &byte) in bytes.iter().enumerate() {
            if let CodecStep::Complete { end: done } = codec.feed(byte, at + index, governor)? {
                stop = Some(done);
                break;
            }
        }
        let end = if stop.is_some() {
            CodecEnd::Delimited
        } else {
            end
        };
        let (decoded, invalid) = codec.decode(end, governor)?;
        let edits = invalid_edits(&invalid, bytes, at, governor)?;
        Ok((decoded, stop, edits))
    }
}

/// Malformed UTF-8 runs (absolute ranges, each with how many U+FFFD it was
/// read as) as `tool_call.value.invalid-utf8` edits over `body`, which
/// starts at `base`. A run past the body (after the value ended) is not
/// part of the value.
fn invalid_edits(
    runs: &[InvalidRun],
    body: &[u8],
    base: usize,
    governor: &mut Governor,
) -> Result<Vec<MarkupEdit>, StopReason> {
    let mut edits = Vec::with_capacity(runs.len());
    for (run, count) in runs {
        if run.end > base + body.len() {
            continue;
        }
        edits.push(edit(
            EditOperator::InvalidUtf8Value,
            run.clone(),
            body,
            base,
            "\u{FFFD}".repeat(*count).into_bytes(),
            governor,
        )?);
    }
    Ok(edits)
}

/// The argument tree of a decoded JSON value whose spans are absolute in
/// `body` (which starts at `base`), and its edits as markup edits.
pub(super) fn tree(
    node: Node,
    edits: Vec<SyntaxEdit>,
    origin: ArgOrigin,
    body: &[u8],
    base: usize,
    governor: &mut Governor,
) -> Result<(ArgTree, Vec<MarkupEdit>), StopReason> {
    let mut leaves = Vec::new();
    collect(&node, &mut String::new(), origin, &mut leaves, governor)?;
    let markup = markup(edits, body, base, governor)?;
    let mut tree = ArgTree::value(node);
    tree.leaves = leaves;
    Ok((tree, markup))
}

/// JSON recovery edits with absolute spans in `body` (which starts at
/// `base`) as markup edits.
fn markup(
    edits: Vec<SyntaxEdit>,
    body: &[u8],
    base: usize,
    governor: &mut Governor,
) -> Result<Vec<MarkupEdit>, StopReason> {
    let mut markup = Vec::with_capacity(edits.len());
    for syntax in edits {
        let operator = EditOperator::json(syntax.rule, syntax.guessed)
            .expect("every JSON recovery rule has an edit operator");
        markup.push(edit(
            operator,
            syntax.span,
            body,
            base,
            syntax.replacement.into_bytes(),
            governor,
        )?);
    }
    Ok(markup)
}

/// Every scalar of `node`, in document order, as a leaf under `pointer`.
fn collect(
    node: &Node,
    pointer: &mut String,
    origin: ArgOrigin,
    leaves: &mut Vec<ArgLeaf>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    governor.charge(Resource::Work, K_CODEC)?;
    let length = pointer.len();
    match &node.value {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                write!(pointer, "/{index}").expect("writing to a String is infallible");
                collect(item, pointer, origin, leaves, governor)?;
                pointer.truncate(length);
            }
        }
        Value::Object(members) => {
            for member in members {
                pointer.push('/');
                pointer.push_str(&pointer_segment(&member.key));
                collect(&member.value, pointer, origin, leaves, governor)?;
                pointer.truncate(length);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            leaves.push(ArgLeaf {
                pointer: pointer.clone(),
                origin,
                raw: node.span.clone().unwrap_or_default(),
            });
        }
    }
    Ok(())
}

/// Decode a string arguments value whose content is JSON.
fn stringified(
    node: Node,
    outer_edits: Vec<SyntaxEdit>,
    body: &[u8],
    base: usize,
    governor: &mut Governor,
) -> Result<(ArgTree, Vec<MarkupEdit>), StopReason> {
    let outer = node.span.clone().unwrap_or(base..base + body.len());
    let Value::String(text) = node.value else {
        unreachable!("stringified arguments are a string")
    };
    governor.charge(Resource::Work, size(text.len()).saturating_mul(K_CODEC))?;
    let mut edits = markup(outer_edits, body, base, governor)?;
    edits.push(edit(
        EditOperator::StringifiedArguments,
        outer.clone(),
        body,
        base,
        text.clone().into_bytes(),
        governor,
    )?);
    let map = EscapeMap::build(
        &body[outer.start - base..outer.end - base],
        outer.start,
        &text,
        governor,
    )?;
    // The decoded text is a `String`: it holds no malformed UTF-8.
    let (decoded, trailing, _) =
        JsonCodec::scan(text.as_bytes(), 0, CodecEnd::Delimited, governor)?;
    let mut tree = match decoded {
        Decoded::Value {
            mut node,
            edits: inner,
            ..
        } if trailing.is_none() && matches!(node.value, Value::Object(_)) => {
            let place = |range: Range<usize>| {
                map.as_ref()
                    .map_or_else(|| outer.clone(), |map| map.place(range))
            };
            relocate(&mut node, &place);
            let inner: Vec<SyntaxEdit> = inner
                .into_iter()
                .map(|syntax| SyntaxEdit {
                    span: place(syntax.span.clone()),
                    ..syntax
                })
                .collect();
            let (tree, markup) = tree(
                node,
                inner,
                ArgOrigin::StringifiedJson,
                body,
                base,
                governor,
            )?;
            edits.extend(markup);
            tree
        }
        _ => ArgTree::unresolved(ArgumentFault::StringifiedArgumentsInvalidJson, outer),
    };
    drop(map);
    tree.arguments_were_stringified = true;
    Ok((tree, edits))
}

/// Move every span of `node` (and its members' key spans) through `place`.
fn relocate(node: &mut Node, place: &impl Fn(Range<usize>) -> Range<usize>) {
    node.span = node.span.clone().map(place);
    match &mut node.value {
        Value::Array(items) => {
            for item in items {
                relocate(item, place);
            }
        }
        Value::Object(members) => {
            for member in members {
                member.key_span = member.key_span.clone().map(place);
                relocate(&mut member.value, place);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// The source range of every decoded byte of a canonical `"`-quoted JSON
/// string: the escape (or character) that produced it.
///
/// Stored as pieces rather than per byte: a run of plain ASCII characters
/// maps byte for byte, and every escape or multi-byte character maps each
/// of its decoded bytes to its whole source span. Its storage is a
/// [`ScratchVec`] charged as it grows and released when the map drops.
struct EscapeMap {
    pieces: ScratchVec<Piece>,
    /// Absolute offset of the closing quote: where an empty range at the
    /// end of the decoded text maps.
    close: usize,
}

/// One piece of an [`EscapeMap`].
#[derive(Clone, Copy, Debug)]
struct Piece {
    /// Offset of the piece's first decoded byte.
    decoded: usize,
    /// Its decoded length.
    decoded_len: usize,
    /// Absolute offset of its first source byte.
    source: usize,
    /// Its source length.
    source_len: usize,
    /// A run of plain ASCII characters, mapped byte for byte (otherwise
    /// one escape or character, every decoded byte mapped to all of it).
    run: bool,
}

impl ScratchCharge for Piece {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 40);
        40
    };
}

impl EscapeMap {
    /// The map of the string literal `source` (quotes included) starting at
    /// absolute `start`, or `None` when its strict decoding is not exactly
    /// `text` (the string needed tolerant repair).
    fn build(
        source: &[u8],
        start: usize,
        text: &str,
        governor: &Governor,
    ) -> Result<Option<Self>, StopReason> {
        let quoted = source.len() >= 2 && source[0] == b'"' && source[source.len() - 1] == b'"';
        if !quoted {
            return Ok(None);
        }
        let mut map = Self {
            pieces: ScratchVec::new(governor),
            close: start + source.len() - 1,
        };
        let content = &source[1..source.len() - 1];
        let expected = text.as_bytes();
        let mut index = 0;
        let mut decoded = 0;
        while index < content.len() {
            let Some((character, width)) = Self::character(&content[index..]) else {
                return Ok(None);
            };
            let mut encoded = [0; 4];
            let bytes = character.encode_utf8(&mut encoded).as_bytes();
            if expected.get(decoded..decoded + bytes.len()) != Some(bytes) {
                return Ok(None);
            }
            let from = start + 1 + index;
            let run = width == 1 && bytes.len() == 1;
            let extends = run
                && map
                    .pieces
                    .as_slice()
                    .last()
                    .is_some_and(|last| last.run && last.source + last.source_len == from);
            if extends {
                let last = map
                    .pieces
                    .as_mut_slice()
                    .last_mut()
                    .expect("the run being extended");
                last.decoded_len += 1;
                last.source_len += 1;
            } else {
                map.pieces.push(Piece {
                    decoded,
                    decoded_len: bytes.len(),
                    source: from,
                    source_len: width,
                    run,
                })?;
            }
            decoded += bytes.len();
            index += width;
        }
        if decoded != expected.len() {
            return Ok(None);
        }
        Ok(Some(map))
    }

    /// The source range of decoded byte `at` (`at` is within the text).
    fn byte(&self, at: usize) -> Range<usize> {
        let pieces = self.pieces.as_slice();
        let index = pieces.partition_point(|piece| piece.decoded + piece.decoded_len <= at);
        let piece = pieces[index];
        if piece.run {
            let offset = piece.source + (at - piece.decoded);
            offset..offset + 1
        } else {
            piece.source..piece.source + piece.source_len
        }
    }

    /// The first character of string content `content` as strict JSON
    /// decodes it, and the source bytes it spans.
    fn character(content: &[u8]) -> Option<(char, usize)> {
        if content[0] != b'\\' {
            let width = match content[0] {
                0x00..=0x7F => 1,
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                _ => 4,
            };
            let text = core::str::from_utf8(content.get(..width)?).ok()?;
            return text.chars().next().map(|character| (character, width));
        }
        let width = match content.get(1)? {
            b'u' => {
                let pair = content.get(6..8) == Some(b"\\u".as_slice())
                    && crate::json::incremental::hex4(content.get(2..6)?)
                        .is_some_and(|code| (0xd800..=0xdbff).contains(&code));
                if pair { 12 } else { 6 }
            }
            _ => 2,
        };
        let slice = core::str::from_utf8(content.get(..width)?).ok()?;
        decode_escape(slice).map(|character| (character, width))
    }

    /// The source range of decoded range `range`.
    fn place(&self, range: Range<usize>) -> Range<usize> {
        let decoded = self
            .pieces
            .as_slice()
            .last()
            .map_or(0, |last| last.decoded + last.decoded_len);
        if range.is_empty() {
            let at = if range.start < decoded {
                self.byte(range.start).start
            } else {
                self.close
            };
            return at..at;
        }
        self.byte(range.start).start..self.byte(range.end - 1).end
    }
}

#[cfg(test)]
mod tests {
    use super::{Codec, CodecContext, CodecEnd, CodecStep, EditOperator, JsonCodec};
    use crate::tool_call::ArgOrigin;
    use crate::{Budget, Governor};

    fn decode(source: &[u8]) -> super::CodecValue {
        let mut governor = Governor::new(Budget::default());
        let mut codec = JsonCodec::begin(0, &mut governor).unwrap();
        let mut end = source.len();
        for (at, &byte) in source.iter().enumerate() {
            if let CodecStep::Complete { end: stop } = codec.feed(byte, at, &mut governor).unwrap()
            {
                end = stop;
                break;
            }
        }
        codec
            .finish(
                &source[..end],
                0..end,
                CodecEnd::Delimited,
                CodecContext::default(),
                &mut governor,
            )
            .unwrap()
            .unwrap()
    }

    #[test]
    fn stringified_spans_map_through_escapes() {
        let source = r#""{\"k\":\"é\"}""#.as_bytes();
        let value = decode(source);
        assert!(value.tree.arguments_were_stringified);
        assert_eq!(value.tree.root.to_json(), r#"{"k":"é"}"#);
        let leaf = &value.tree.leaves[0];
        assert_eq!(leaf.origin, ArgOrigin::StringifiedJson);
        assert_eq!(&source[leaf.raw.clone()], r#"\"é\""#.as_bytes());
    }

    /// ASCII runs, escapes and multi-byte characters mixed: every leaf maps
    /// to the exact source bytes that encode it, and an empty value maps to
    /// the position of its source.
    #[test]
    fn stringified_spans_map_through_runs_and_escapes() {
        let source =
            r#""{\"name\":\"ab\\ncé\\u00e9z\",\"n\":12,\"e\":\"\",\"t\":true}""#.as_bytes();
        let value = decode(source);
        assert_eq!(
            value.tree.root.to_json(),
            r#"{"name":"ab\ncééz","n":12,"e":"","t":true}"#
        );
        let raw: Vec<&[u8]> = value
            .tree
            .leaves
            .iter()
            .map(|leaf| &source[leaf.raw.clone()])
            .collect();
        assert_eq!(
            raw,
            [
                r#"\"ab\\ncé\\u00e9z\""#.as_bytes(),
                b"12",
                br#"\"\""#,
                b"true"
            ]
        );
    }

    #[test]
    fn unmappable_string_falls_back_to_the_enclosing_span() {
        let source = br#"'{"k":1}'"#;
        let value = decode(source);
        assert!(value.tree.arguments_were_stringified);
        assert_eq!(value.tree.leaves[0].raw, 0..source.len());
    }

    #[test]
    fn malformed_utf8_is_replaced_and_recorded() {
        let value = decode(b"{\"a\":\"\xFF\xC3(\", \"b\": 1}");
        assert_eq!(
            value.tree.root.to_json(),
            "{\"a\":\"\u{FFFD}\u{FFFD}(\",\"b\":1}"
        );
        assert_eq!(value.tree.unresolved.len(), 0);
        let edits: Vec<_> = value
            .edits
            .iter()
            .map(|edit| (edit.operator, edit.span.clone(), edit.original.clone()))
            .collect();
        assert_eq!(
            edits,
            [(EditOperator::InvalidUtf8Value, 6..8, b"\xFF\xC3".to_vec())]
        );
        // Offsets after the replacement are exact.
        assert_eq!(value.tree.leaves[1].raw, 17..18);
    }
}
