// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! GLM key/value tags, decoded at their original source coordinates.
use super::json::{Decoded, JsonCodec, tree as json_tree};
use super::{
    Codec, CodecContext, CodecEnd, CodecFault, CodecKind, CodecStep, CodecValue, edit, lossy,
    uncut, value,
};
use crate::contract::{Governor, Resource, StopReason, saturating_size as size};
use crate::grammar::edit::{EditOperator, MarkupEdit};
use crate::json::{Node, Value};
use crate::tool_call::{ArgLeaf, ArgOrigin, ArgTree, ArgumentFault, Spanned, ValueShape};
use core::ops::Range;

const TAGS: [&[u8]; 4] = [b"<arg_key>", b"</arg_key>", b"<arg_value>", b"</arg_value>"];

/// GLM's alternating `<arg_key>` and `<arg_value>` elements.
///
/// Keys and declared strings retain their exact text, including newlines.
/// Other declared values use the native JSON codec; unknown or unmodeled
/// values retain text for the resolver's schema typing. Duplicate keys remain
/// separate occurrences for the resolver's collection/ambiguity policy.
///
/// ```
/// use purrassist_core::grammar::codec::{Codec, CodecContext, CodecEnd, KvPairsCodec};
/// use purrassist_core::{Budget, Governor};
/// let body = b"<arg_key>message</arg_key><arg_value>\nhello\n</arg_value>";
/// let mut governor = Governor::new(Budget::default());
/// let result = KvPairsCodec::begin(0, &mut governor)?
///     .finish(body, 0..body.len(), CodecEnd::Delimited, CodecContext::default(), &mut governor)?.unwrap();
/// assert_eq!(result.tree.root.get("message").unwrap().as_str(), Some("\nhello\n"));
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KvPairsCodec {
    start: usize,
}

impl Codec for KvPairsCodec {
    const KIND: CodecKind = CodecKind::KvPairs;
    fn begin(at: usize, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.check()?;
        Ok(Self { start: at })
    }
    fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        debug_assert_eq!(self.start, span.start);
        Reader {
            body,
            base: span.start,
            end,
            edits: Vec::new(),
            governor,
        }
        .run(span, context)
    }
}

#[derive(Clone, Copy)]
struct Tag {
    start: usize,
    end: usize,
    kind: usize,
    repaired: bool,
}
struct Reader<'a, 'g> {
    body: &'a [u8],
    base: usize,
    end: CodecEnd,
    edits: Vec<MarkupEdit>,
    governor: &'g mut Governor,
}
impl Reader<'_, '_> {
    fn run(
        mut self,
        span: Range<usize>,
        context: CodecContext<'_>,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        let mut tree = ArgTree::value(Node {
            value: Value::Object(Vec::new()),
            span: Some(span),
        });
        let mut cursor = self.space(0)?;
        while cursor < self.body.len() {
            let Some(open) = self.tag(cursor)? else {
                return Ok(Err(self.fault(cursor)));
            };
            if open.kind != 0 {
                return Ok(Err(self.fault(cursor)));
            }
            self.respell(open)?;
            let Some(key_end) = self.next_boundary(open.end, 1, 2)? else {
                return Ok(Err(self.fault(open.end)));
            };
            if !matches!(key_end.kind, 1 | 2) || key_end.start == open.end {
                return Ok(Err(self.fault(key_end.start)));
            }
            let raw_key = self.absolute(open.end..key_end.start);
            let (key, invalid) = lossy(
                &self.body[open.end..key_end.start],
                raw_key.start,
                self.governor,
            )?;
            self.edits.extend(invalid);
            let value_open = if key_end.kind == 1 {
                self.respell(key_end)?;
                let start = self.space(key_end.end)?;
                let Some(tag) = self.tag(start)? else {
                    return Ok(Err(self.fault(start)));
                };
                if tag.kind != 2 {
                    return Ok(Err(self.fault(start)));
                }
                tag
            } else {
                self.record(
                    EditOperator::MissingParameterClose,
                    key_end.start..key_end.start,
                    TAGS[1],
                )?;
                key_end
            };
            self.respell(value_open)?;
            let canonical = context
                .shape
                .and_then(|shape| shape.arguments.resolve_name(&key))
                .unwrap_or(&key);
            let declared = context
                .shape
                .and_then(|shape| shape.arguments.parameter(canonical))
                .map(|parameter| &parameter.value);
            let typed = !matches!(
                declared,
                None | Some(
                    ValueShape::String(_) | ValueShape::Enumeration(_) | ValueShape::Unmodeled
                )
            );
            let boundary = if typed {
                self.json_boundary(value_open.end)?
            } else {
                self.next_boundary(value_open.end, 3, 0)?
            };
            if boundary.is_some_and(|tag| !matches!(tag.kind, 0 | 3)) {
                return Ok(Err(self.fault(boundary.unwrap().start)));
            }
            let stop = boundary.map_or(self.body.len(), |tag| tag.start);
            let closed = boundary.is_some_and(|tag| tag.kind == 3);
            let decoded = self.decode(declared, value_open.end..stop, closed)?;
            let decoded = match decoded {
                Ok(tree) => tree,
                Err(fault) => return Ok(Err(fault)),
            };
            tree.adopt_keyed(
                Spanned {
                    value: key.clone(),
                    span: raw_key,
                },
                canonical,
                decoded,
                false,
            );
            if let Some(tag) = boundary.filter(|tag| tag.kind == 3) {
                self.respell(tag)?;
                cursor = self.space(tag.end)?;
            } else {
                self.missing_close(stop, boundary, declared)?;
                cursor = stop;
            }
        }
        tree.tool = context.shape.map(|shape| shape.tool.clone());
        self.governor.check()?;
        value(tree, self.edits, self.governor)
    }
    fn missing_close(
        &mut self,
        stop: usize,
        boundary: Option<Tag>,
        declared: Option<&ValueShape>,
    ) -> Result<(), StopReason> {
        let operator = if self.end == CodecEnd::Truncated && boundary.is_none() {
            EditOperator::MissingClosersAtEnd
        } else if boundary.is_none() && self.end == CodecEnd::Delimited {
            EditOperator::ParameterClosedByElement
        } else if matches!(
            declared,
            Some(
                ValueShape::Integer
                    | ValueShape::Number
                    | ValueShape::Boolean
                    | ValueShape::Object(_)
                    | ValueShape::Array(_)
            )
        ) {
            EditOperator::MissingParameterClose
        } else {
            EditOperator::UndeclaredParameterBoundary
        };
        if self.end != CodecEnd::Truncated || boundary.is_some() {
            self.record(operator, stop..stop, TAGS[3])?;
        }
        Ok(())
    }
    fn decode(
        &mut self,
        declared: Option<&ValueShape>,
        range: Range<usize>,
        closed: bool,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        let cut = !closed && range.end == self.body.len() && self.end == CodecEnd::Truncated;
        let text = matches!(
            declared,
            None | Some(ValueShape::String(_) | ValueShape::Enumeration(_) | ValueShape::Unmodeled)
        );
        if text {
            if !closed && range.end == self.body.len() && self.end == CodecEnd::EndOfInput {
                return Ok(Err(self.fault(range.end)));
            }
            let end = if cut {
                range.start + uncut(&self.body[range.clone()])
            } else {
                range.end
            };
            let raw = self.absolute(range.start..end);
            let (text, invalid) = lossy(&self.body[range.start..end], raw.start, self.governor)?;
            self.edits.extend(invalid);
            let mut tree = ArgTree::value(Node {
                value: Value::String(text),
                span: Some(raw.clone()),
            });
            tree.leaves.push(ArgLeaf {
                pointer: String::new(),
                origin: ArgOrigin::XmlText,
                raw,
            });
            return Ok(Ok(tree));
        }
        let raw = self.absolute(range.clone());
        let (decoded, trailing, invalid) = JsonCodec::scan(
            &self.body[range],
            raw.start,
            if cut {
                CodecEnd::Truncated
            } else {
                CodecEnd::Delimited
            },
            self.governor,
        )?;
        self.edits.extend(invalid);
        match (decoded, trailing) {
            (
                Decoded::Value {
                    node,
                    edits,
                    incomplete,
                },
                None,
            ) if closed || cut || !incomplete => {
                let (tree, edits) = json_tree(
                    node,
                    edits,
                    ArgOrigin::Json,
                    self.body,
                    self.base,
                    self.governor,
                )?;
                self.edits.extend(edits);
                Ok(Ok(tree))
            }
            _ if !closed && !cut => Ok(Err(self.fault(raw.end - self.base))),
            (Decoded::Fault(fault), _) => Ok(Ok(ArgTree::unresolved(fault, raw))),
            (_, trailing) => Ok(Ok(ArgTree::unresolved(
                ArgumentFault::InvalidJson {
                    at: trailing.unwrap_or(raw.end),
                },
                raw,
            ))),
        }
    }
    fn space(&mut self, mut at: usize) -> Result<usize, StopReason> {
        while self.body.get(at).is_some_and(u8::is_ascii_whitespace) {
            self.governor.charge(Resource::Work, 1)?;
            at += 1;
        }
        Ok(at)
    }
    /// Exact closers outrank fuzzy closer text. A next exact opener bounds
    /// the search, so selecting an earlier fuzzy closer never rescans a suffix
    /// beyond the next field. Fuzzy openers are repaired only at field starts.
    fn next_boundary(
        &mut self,
        mut at: usize,
        closer: usize,
        opener: usize,
    ) -> Result<Option<Tag>, StopReason> {
        let mut fuzzy = None;
        while at < self.body.len() {
            self.governor.charge(Resource::Work, 1)?;
            if let Some(tag) = self.tag(at)? {
                if tag.kind == closer {
                    if !tag.repaired {
                        return Ok(Some(tag));
                    }
                    if fuzzy.is_none() {
                        fuzzy = Some(tag);
                    }
                } else if tag.kind == opener && !tag.repaired {
                    return Ok(fuzzy.or(Some(tag)));
                }
            }
            at += 1;
        }
        Ok(fuzzy)
    }
    /// Use the native JSON cursor to keep tag-looking bytes inside strings.
    /// Decoding the bounded value below uses the same parser and absolute spans.
    fn json_boundary(&mut self, from: usize) -> Result<Option<Tag>, StopReason> {
        let mut json = JsonCodec::begin(self.base + from, self.governor)?;
        for at in from..self.body.len() {
            if !json.in_string()
                && let Some(tag) = self.tag(at)?
            {
                return Ok(Some(tag));
            }
            if let CodecStep::Complete { end } =
                json.feed(self.body[at], self.base + at, self.governor)?
            {
                let next = self.space(end - self.base)?;
                return self.tag(next);
            }
        }
        Ok(None)
    }
    fn tag(&mut self, at: usize) -> Result<Option<Tag>, StopReason> {
        if self.body.get(at) != Some(&b'<') {
            return Ok(None);
        }
        for (kind, token) in TAGS.iter().enumerate() {
            self.governor.charge(Resource::Work, size(token.len()))?;
            if self.body[at..].starts_with(token) {
                return Ok(Some(Tag {
                    start: at,
                    end: at + token.len(),
                    kind,
                    repaired: false,
                }));
            }
        }
        // A misspelling preserves tag shape and has at most one extra byte.
        // The fixed window prevents repeated '<' from causing quadratic scans.
        let tail = &self.body[at..self.body.len().min(at.saturating_add(13))];
        self.governor.charge(Resource::Work, size(tail.len()))?;
        let Some(width) = tail
            .iter()
            .position(|&byte| byte == b'>')
            .map(|last| last + 1)
        else {
            return Ok(None);
        };
        self.governor
            .charge(Resource::Work, size(width).saturating_mul(16))?;
        let classes = TAGS.map(|token| [token]);
        Ok(crate::grammar::misspelling::misspelled_class(
            &tail[..width],
            classes.iter().map(<[&[u8]; 1]>::as_slice),
        )
        .map(|kind| Tag {
            start: at,
            end: at + width,
            kind,
            repaired: true,
        }))
    }
    fn respell(&mut self, tag: Tag) -> Result<(), StopReason> {
        if tag.repaired {
            self.record(
                if tag.kind.is_multiple_of(2) {
                    EditOperator::TokenMisspelled
                } else {
                    EditOperator::CloserMisspelled
                },
                tag.start..tag.end,
                TAGS[tag.kind],
            )?;
        }
        Ok(())
    }
    fn record(
        &mut self,
        operator: EditOperator,
        range: Range<usize>,
        replacement: &[u8],
    ) -> Result<(), StopReason> {
        self.edits.push(edit(
            operator,
            self.absolute(range),
            self.body,
            self.base,
            replacement.to_vec(),
            self.governor,
        )?);
        Ok(())
    }
    const fn absolute(&self, range: Range<usize>) -> Range<usize> {
        self.base + range.start..self.base + range.end
    }
    const fn fault(&self, at: usize) -> CodecFault {
        CodecFault::Malformed { at: self.base + at }
    }
}
