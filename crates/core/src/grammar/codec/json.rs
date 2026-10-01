// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The `json` codec: one tolerant JSON value, and stringified arguments.
use super::{
    Codec, CodecContext, CodecEnd, CodecFault, CodecKind, CodecStep, CodecValue, Utf8Check, edit,
    value,
};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, StopReason};
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
/// a `tool_call.arguments.stringified` edit is recorded. Spans inside the
/// string are mapped back through the escape decoding to the exact source
/// bytes that encode each value; when the string itself needed repair (an
/// unterminated or non-`"`-quoted string, an invalid escape kept literally),
/// exact mapping is impossible and every inner span is the enclosing
/// string's span. Content that is not one recoverable JSON object leaves the
/// arguments unresolved with
/// [`ArgumentFault::StringifiedArgumentsInvalidJson`], keeping the string's
/// span.
///
/// Bytes that are not well-formed UTF-8, and JSON the parser cannot
/// recover, leave the arguments unresolved
/// ([`ArgumentFault::InvalidUtf8InArgument`], [`ArgumentFault::InvalidJson`])
/// with the whole body as their raw span. Malformed UTF-8 does not stop the
/// parse (each malformed byte is read as `?` only to find where the value
/// ends, so the call stays recognized); after unrecoverable JSON the body
/// runs to the slot's delimiter. The codec never faults. With a [`CodecContext::shape`], the
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

impl Codec for JsonCodec {
    const KIND: CodecKind = CodecKind::Json;

    fn begin(at: usize, governor: &mut Governor) -> Result<Self, StopReason> {
        Ok(Self {
            utf8: Utf8Check::new(),
            held: [0; 4],
            held_len: 0,
            parser: IncrementalRecovery::starting_at(at, governor),
            fault: None,
            stalled: false,
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
        // The bytes handed to the parser in this step: a malformed sequence
        // is replaced, byte for byte, by `?`, so the parser can still find
        // where the value ends (offsets are unchanged); the value itself is
        // then unresolved.
        let mut chunk = [0_u8; 8];
        let mut width = 0;
        if let Some(bad) = self.utf8.step(byte, at) {
            self.fault
                .get_or_insert(ArgumentFault::InvalidUtf8InArgument { at: bad });
            width = usize::from(core::mem::take(&mut self.held_len));
            chunk[..width].fill(b'?');
            self.utf8 = Utf8Check::new();
            if self.utf8.step(byte, at).is_some() {
                self.utf8 = Utf8Check::new();
                chunk[width] = b'?';
                width += 1;
            } else {
                self.held[0] = byte;
                self.held_len = 1;
            }
        } else {
            self.held[usize::from(self.held_len)] = byte;
            self.held_len += 1;
        }
        if self.utf8.at_boundary() {
            let held = usize::from(core::mem::take(&mut self.held_len));
            chunk[width..width + held].copy_from_slice(&self.held[..held]);
            width += held;
        }
        if width == 0 {
            return Ok(CodecStep::Consumed);
        }
        match self.parser.feed(&chunk[..width], governor) {
            Ok(()) => {}
            Err(StopReason::InvalidInput(_)) => {
                self.fault
                    .get_or_insert(ArgumentFault::InvalidJson { at: at + 1 - width });
                self.stalled = true;
                return Ok(CodecStep::Consumed);
            }
            Err(reason) => return Err(reason),
        }
        Ok(if self.parser.is_done() {
            CodecStep::Complete {
                end: self.parser.position(),
            }
        } else {
            CodecStep::Consumed
        })
    }

    fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        let (mut tree, edits) = match self.decode(end, governor)? {
            Decoded::Fault(fault) => (ArgTree::unresolved(fault, span), Vec::new()),
            Decoded::Value { node, edits, .. } => {
                if matches!(node.value, Value::String(_)) {
                    stringified(node, edits, body, span.start, governor)?
                } else {
                    tree(node, edits, ArgOrigin::Json, body, span.start, governor)?
                }
            }
        };
        tree.tool = context.shape.map(|shape| shape.tool.clone());
        value(tree, edits, governor)
    }
}

impl JsonCodec {
    /// The decoded value, applying end-of-input repairs for an input that
    /// ended as `end` says.
    fn decode(self, end: CodecEnd, governor: &mut Governor) -> Result<Decoded, StopReason> {
        if let Some(fault) = self.fault {
            return Ok(Decoded::Fault(fault));
        }
        if let Some(at) = self.utf8.malformed(end) {
            return Ok(Decoded::Fault(ArgumentFault::InvalidUtf8InArgument { at }));
        }
        let before = self.parser.edit_count();
        let stop = self.parser.position();
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
            Err(StopReason::InvalidInput(_)) => {
                Ok(Decoded::Fault(ArgumentFault::InvalidJson { at: stop }))
            }
            Err(reason) => Err(reason),
        }
    }

    /// Decode `bytes`, which start at absolute offset `at`, as one JSON
    /// value; the second result is where the value completed, when bytes
    /// remain after it (its trailing whitespace and comments included).
    pub(super) fn scan(
        bytes: &[u8],
        at: usize,
        end: CodecEnd,
        governor: &mut Governor,
    ) -> Result<(Decoded, Option<usize>), StopReason> {
        let mut codec = Self::begin(at, governor)?;
        for (index, &byte) in bytes.iter().enumerate() {
            if let CodecStep::Complete { end: stop } = codec.feed(byte, at + index, governor)? {
                return Ok((codec.decode(CodecEnd::Delimited, governor)?, Some(stop)));
            }
        }
        Ok((codec.decode(end, governor)?, None))
    }
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
    let (decoded, trailing) = JsonCodec::scan(text.as_bytes(), 0, CodecEnd::Delimited, governor)?;
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
    if let Some(map) = map {
        governor.release_scratch(map.charged)?;
    }
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
struct EscapeMap {
    starts: Vec<usize>,
    ends: Vec<usize>,
    /// Absolute offset of the closing quote: where an empty range at the
    /// end of the decoded text maps.
    close: usize,
    charged: u64,
}

impl EscapeMap {
    /// The map of the string literal `source` (quotes included) starting at
    /// absolute `start`, or `None` when its strict decoding is not exactly
    /// `text` (the string needed tolerant repair).
    fn build(
        source: &[u8],
        start: usize,
        text: &str,
        governor: &mut Governor,
    ) -> Result<Option<Self>, StopReason> {
        let quoted = source.len() >= 2 && source[0] == b'"' && source[source.len() - 1] == b'"';
        if !quoted {
            return Ok(None);
        }
        let charged = size(text.len()).saturating_mul(16);
        governor.charge(Resource::ScratchBytes, charged)?;
        let mut map = Self {
            starts: Vec::with_capacity(text.len()),
            ends: Vec::with_capacity(text.len()),
            close: start + source.len() - 1,
            charged,
        };
        let content = &source[1..source.len() - 1];
        let expected = text.as_bytes();
        let mut index = 0;
        while index < content.len() {
            let Some((character, width)) = Self::character(&content[index..]) else {
                governor.release_scratch(charged)?;
                return Ok(None);
            };
            let mut encoded = [0; 4];
            let bytes = character.encode_utf8(&mut encoded).as_bytes();
            let at = map.starts.len();
            if expected.get(at..at + bytes.len()) != Some(bytes) {
                governor.release_scratch(charged)?;
                return Ok(None);
            }
            let from = start + 1 + index;
            for _ in bytes {
                map.starts.push(from);
                map.ends.push(from + width);
            }
            index += width;
        }
        if map.starts.len() != expected.len() {
            governor.release_scratch(charged)?;
            return Ok(None);
        }
        Ok(Some(map))
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
        if range.is_empty() {
            let at = self.starts.get(range.start).copied().unwrap_or(self.close);
            return at..at;
        }
        self.starts[range.start]..self.ends[range.end - 1]
    }
}

#[cfg(test)]
mod tests {
    use super::{Codec, CodecContext, CodecEnd, CodecStep, JsonCodec};
    use crate::tool_call::{ArgOrigin, ArgumentFault};
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

    #[test]
    fn unmappable_string_falls_back_to_the_enclosing_span() {
        let source = br#"'{"k":1}'"#;
        let value = decode(source);
        assert!(value.tree.arguments_were_stringified);
        assert_eq!(value.tree.leaves[0].raw, 0..source.len());
    }

    #[test]
    fn malformed_utf8_is_unresolved_not_a_fault() {
        let value = decode(b"{\"a\":\"\xFF\"}");
        assert_eq!(
            value.tree.unresolved[0].fault,
            ArgumentFault::InvalidUtf8InArgument { at: 6 }
        );
    }
}
