// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The `xml-params` codec: `<parameter=NAME>value</parameter>` sequences.
use super::json::{Decoded, JsonCodec, tree as json_tree};
use super::{
    Codec, CodecContext, CodecEnd, CodecFault, CodecKind, CodecStep, CodecValue, edit, value,
};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, StopReason};
use crate::grammar::cost::K_CODEC;
use crate::grammar::edit::{EditOperator, MarkupEdit};
use crate::json::{Node, Value};
use crate::tool_call::{
    ArgLeaf, ArgOrigin, ArgTree, ArgumentFault, ObjectShape, Spanned, ValueShape,
};
use core::ops::Range;

/// The parameter opener prefix; the name runs to the next `>`.
const OPENER: &[u8] = b"<parameter=";
/// The canonical parameter closer.
const CLOSER: &[u8] = b"</parameter>";
/// The variant closer spelling read as [`CLOSER`].
const CLOSER_VARIANT: &[u8] = b"</parameters>";

/// The `xml-params` codec: a function body of
/// `<parameter=NAME>value</parameter>` elements (the Qwen3-Coder XML
/// shape), typed by the call's declared tool.
///
/// The structure of a body depends on the declared parameters of the
/// call's tool, so the codec reads it when the body ends
/// ([`Codec::finish`], one linear pass; [`Codec::feed`] only accepts bytes).
/// Its [`CodecContext::shape`] is the declared tool's argument shape when
/// the call's name was captured before the slot and names a declared tool.
///
/// # Values
///
/// - A parameter declared as a string (or enumeration, or with an unmodeled
///   schema), and any undeclared parameter, is a verbatim string: exactly
///   one framing newline on each side is consumed and recorded as a
///   `tool_call.xml.framing-newline` edit (a non-guess, single-reading
///   markup loss); nothing else about the text changes (`123` and `true`
///   stay strings). Its leaf has origin [`ArgOrigin::XmlText`].
/// - A parameter declared as an object, array, number, integer or boolean is
///   parsed from its verbatim text by the `json` codec (exact number
///   spellings, tolerant repairs recorded as edits, leaves with origin
///   [`ArgOrigin::Json`]). An object parameter whose value begins with a
///   `<parameter=` opener instead holds nested parameters, typed by the
///   object's declared shape, so nested elements and inline JSON produce the
///   same tree.
/// - Malformed UTF-8 inside a value leaves that parameter unresolved
///   ([`ArgumentFault::InvalidUtf8InArgument`], raw span kept); JSON that
///   cannot be recovered, or text after the JSON value, leaves it unresolved
///   with [`ArgumentFault::InvalidJson`].
/// - Repeated parameters are all recorded, in order, as duplicate members.
///
/// # Boundaries
///
/// A value ends at `</parameter>` (`</parameters>` is read as it, recorded
/// as `tool_call.markup.closer-variant`), at the end of the body (the
/// missing closer recorded as `tool_call.markup.missing-closer`), or at a
/// `<parameter=NAME>` opener where `NAME` (or an alias) is declared in the
/// value's object scope or an enclosing one: the missing closer is recorded
/// as `tool_call.markup.missing-parameter-close` (single reading, non-guess,
/// grounded in the definition), and an already-bound `NAME` simply repeats.
/// A typed value ended by a missing closer must be complete JSON there, or
/// the call is malformed ([`CodecFault::Malformed`]). An opener whose name
/// is not declared is not a boundary: it is text of a string value (and
/// leaves a typed value incomplete). When the call names no declared tool
/// (no tool set, an unknown or hallucinated name, or a grammar that captures
/// the name after the arguments), every `<parameter=` opener is a boundary
/// (recorded as the syntax-grounded `tool_call.markup.missing-closer`) and
/// every value is a string; such a call is resolved, or found unresolvable
/// (a hallucinated tool), against its tool later.
///
/// A `</parameter>` outside any value closes the innermost nested object,
/// or, at the top level, is a stray closer deleted as
/// `tool_call.markup.stray-closer`. Any other text between parameters is
/// malformed.
///
/// # Whole-JSON bodies
///
/// A body whose first non-whitespace byte is `{` is one JSON object
/// (`<function=f>{"a":1}</function>`), accepted only when the object is the
/// entire body: after it, at most one stray `</parameter>` is deleted (as
/// `tool_call.markup.stray-closer`) and only whitespace may follow; any
/// other text is malformed.
///
/// The codec's markup spellings are fixed (`<parameter=`, `</parameter>`,
/// `</parameters>`); the call's own closers belong to the grammar, which
/// ends the slot body with its delimiters, so a value cannot contain one of
/// the slot's delimiters.
///
/// ```
/// use purrassist_core::grammar::codec::{Codec, CodecContext, CodecEnd, XmlParamsCodec};
/// use purrassist_core::{Budget, Governor, tool_call::{Param, ToolDefinition, ToolSet}};
/// let tools = ToolSet::compile(
///     [ToolDefinition::new("edit").param("path", Param::string()).param("line", Param::integer())],
///     Budget::default(),
/// ).unwrap();
/// let context = CodecContext { shape: tools.argument_shape("edit") };
/// let mut governor = Governor::new(Budget::default());
/// let body = b"<parameter=path>\nsrc/a.rs\n</parameter><parameter=line>\n12<parameter=path>b";
/// let codec = XmlParamsCodec::begin(0, &mut governor)?;
/// let value = codec
///     .finish(body, 0..body.len(), CodecEnd::Delimited, context, &mut governor)?
///     .unwrap();
/// assert_eq!(value.tree.root.to_json(), r#"{"path":"src/a.rs","line":12,"path":"b"}"#);
/// assert_eq!(value.tree.tool.as_deref(), Some("edit"));
/// let rules: Vec<_> = value.edits.iter().map(|edit| edit.rule().id).collect();
/// assert_eq!(
///     rules,
///     [
///         "tool_call.xml.framing-newline",
///         "tool_call.xml.framing-newline",
///         "tool_call.markup.missing-parameter-close",
///         "tool_call.markup.missing-closer",
///     ]
/// );
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XmlParamsCodec {
    start: usize,
}

impl Codec for XmlParamsCodec {
    const KIND: CodecKind = CodecKind::XmlParams;

    fn begin(at: usize, _governor: &mut Governor) -> Result<Self, StopReason> {
        Ok(Self { start: at })
    }

    fn feed(
        &mut self,
        _byte: u8,
        _at: usize,
        _governor: &mut Governor,
    ) -> Result<CodecStep, StopReason> {
        Ok(CodecStep::Consumed)
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
        let reader = Reader {
            body,
            base: span.start,
            end,
            frames: Vec::new(),
            edits: Vec::new(),
            governor,
        };
        reader.run(span, context)
    }
}

/// A `<parameter=NAME>` opener; offsets are relative to the body.
struct Opener {
    start: usize,
    name: Spanned<String>,
    end: usize,
}

/// What ended a parameter value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Terminator {
    /// A closer of this width; `true` for the variant spelling.
    Closer(usize, bool),
    /// The opener of a declared parameter.
    Boundary,
    /// The end of the body.
    End,
}

/// One object scope being read: the top-level arguments or a nested object
/// parameter.
struct Frame<'t> {
    scope: Option<&'t ObjectShape>,
    tree: ArgTree,
    /// The parameter holding this object; `None` at the top level.
    name: Option<Spanned<String>>,
}

struct Reader<'b, 't, 'g> {
    body: &'b [u8],
    base: usize,
    end: CodecEnd,
    frames: Vec<Frame<'t>>,
    edits: Vec<MarkupEdit>,
    governor: &'g mut Governor,
}

impl<'t> Reader<'_, 't, '_> {
    fn run(
        mut self,
        span: Range<usize>,
        context: CodecContext<'t>,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        let tool = context.shape.map(|shape| shape.tool.clone());
        let first = self.skip_space(0)?;
        let mut tree = if self.body.get(first) == Some(&b'{') {
            match self.json_body(first, span)? {
                Ok(tree) => tree,
                Err(fault) => return Ok(Err(fault)),
            }
        } else {
            match self.parameters(span, context)? {
                Ok(tree) => tree,
                Err(fault) => return Ok(Err(fault)),
            }
        };
        tree.tool = tool;
        value(tree, self.edits, self.governor)
    }

    const fn absolute(&self, range: Range<usize>) -> Range<usize> {
        self.base + range.start..self.base + range.end
    }

    fn record(
        &mut self,
        operator: EditOperator,
        range: Range<usize>,
        replacement: &[u8],
    ) -> Result<(), StopReason> {
        let span = self.absolute(range);
        let edit = edit(
            operator,
            span,
            self.body,
            self.base,
            replacement.to_vec(),
            self.governor,
        )?;
        self.edits.push(edit);
        Ok(())
    }

    fn charge(&mut self, units: usize) -> Result<(), StopReason> {
        self.governor
            .charge(Resource::Work, size(units).saturating_mul(K_CODEC))
    }

    fn skip_space(&mut self, from: usize) -> Result<usize, StopReason> {
        let skipped = self.body[from..]
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        self.charge(skipped)?;
        Ok(from + skipped)
    }

    /// The closer at `at`: its width, and whether it is the variant.
    fn closer(&self, at: usize) -> Option<(usize, bool)> {
        let rest = &self.body[at..];
        if rest.starts_with(CLOSER_VARIANT) {
            Some((CLOSER_VARIANT.len(), true))
        } else if rest.starts_with(CLOSER) {
            Some((CLOSER.len(), false))
        } else {
            None
        }
    }

    /// The opener at `at`: `<parameter=` and a non-empty UTF-8 name free of
    /// whitespace and angle brackets, then `>`.
    fn opener(&mut self, at: usize) -> Result<Option<Opener>, StopReason> {
        if !self.body[at..].starts_with(OPENER) {
            return Ok(None);
        }
        let from = at + OPENER.len();
        let length = self.body[from..]
            .iter()
            .take_while(|&&byte| byte != b'>' && byte != b'<' && !byte.is_ascii_whitespace())
            .count();
        self.charge(length)?;
        let to = from + length;
        if length == 0 || self.body.get(to) != Some(&b'>') {
            return Ok(None);
        }
        let Ok(name) = core::str::from_utf8(&self.body[from..to]) else {
            return Ok(None);
        };
        Ok(Some(Opener {
            start: at,
            name: Spanned {
                value: name.to_owned(),
                span: self.absolute(from..to),
            },
            end: to + 1,
        }))
    }

    /// Whether an opener of `name` ends an open value: it names a parameter
    /// declared in some open scope, or no tool is known.
    fn boundary(&mut self, name: &str) -> Result<bool, StopReason> {
        if self.frames[0].scope.is_none() {
            return Ok(true);
        }
        self.charge(self.frames.len())?;
        Ok(self.frames.iter().any(|frame| declares(frame.scope, name)))
    }

    /// The innermost open scope declaring `name`, or the innermost scope.
    fn owner(&mut self, name: &str) -> Result<usize, StopReason> {
        self.charge(self.frames.len())?;
        Ok(self
            .frames
            .iter()
            .rposition(|frame| declares(frame.scope, name))
            .unwrap_or(self.frames.len() - 1))
    }

    /// Close the innermost nested object at `at`.
    fn pop(&mut self, at: usize) {
        let frame = self.frames.pop().expect("a nested scope is open");
        let mut tree = frame.tree;
        let end = self.base + at;
        tree.root.span = tree.root.span.map(|span| span.start..end);
        let name = frame.name.expect("nested scopes are named");
        self.frames
            .last_mut()
            .expect("the top-level scope stays open")
            .tree
            .adopt(name, tree);
    }

    /// Read the top-level parameter sequence.
    fn parameters(
        &mut self,
        span: Range<usize>,
        context: CodecContext<'t>,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        self.frames.push(Frame {
            scope: context.shape.map(|shape| &shape.arguments),
            tree: ArgTree::value(Node {
                value: Value::Object(Vec::new()),
                span: Some(span),
            }),
            name: None,
        });
        let mut at = 0;
        loop {
            at = self.skip_space(at)?;
            if at == self.body.len() {
                break;
            }
            if let Some(opener) = self.opener(at)? {
                at = match self.parameter(opener)? {
                    Ok(next) => next,
                    Err(fault) => return Ok(Err(fault)),
                };
                continue;
            }
            let Some((width, variant)) = self.closer(at) else {
                return Ok(Err(CodecFault::Malformed { at: self.base + at }));
            };
            if self.frames.len() > 1 {
                if variant {
                    self.record(EditOperator::CloserVariant, at..at + width, CLOSER)?;
                }
                self.pop(at);
            } else {
                self.record(EditOperator::DeleteStrayCloser, at..at + width, b"")?;
            }
            at += width;
        }
        while self.frames.len() > 1 {
            if self.end != CodecEnd::Truncated {
                self.record(EditOperator::InsertCloser, at..at, CLOSER)?;
            }
            self.pop(at);
        }
        Ok(Ok(self.frames.pop().expect("the top-level scope").tree))
    }

    /// Read one parameter from its opener; the offset after it.
    fn parameter(&mut self, opener: Opener) -> Result<Result<usize, CodecFault>, StopReason> {
        let owner = self.owner(&opener.name.value)?;
        while self.frames.len() - 1 > owner {
            self.record(
                EditOperator::MissingParameterClose,
                opener.start..opener.start,
                CLOSER,
            )?;
            self.pop(opener.start);
        }
        let scope = self.frames.last().expect("a scope is open").scope;
        let declared = scope.and_then(|scope| {
            let canonical = scope.resolve_name(&opener.name.value)?;
            scope.parameter(canonical).map(|parameter| &parameter.value)
        });
        let from = opener.end;
        let typed = match declared {
            Some(ValueShape::Object(inner)) => {
                let next = self.skip_space(from)?;
                if self.body[next..].starts_with(OPENER) {
                    self.governor
                        .charge(Resource::Depth, size(self.frames.len()))?;
                    self.frames.push(Frame {
                        scope: Some(inner),
                        tree: ArgTree::value(Node {
                            value: Value::Object(Vec::new()),
                            span: Some(self.absolute(from..from)),
                        }),
                        name: Some(opener.name),
                    });
                    return Ok(Ok(from));
                }
                true
            }
            Some(
                ValueShape::Array(_)
                | ValueShape::Integer
                | ValueShape::Number
                | ValueShape::Boolean,
            ) => true,
            Some(ValueShape::String(_) | ValueShape::Enumeration(_) | ValueShape::Unmodeled)
            | None => false,
        };
        let (stop, terminator) = self.terminator(from)?;
        let tree = if typed {
            match self.typed(from, stop, terminator)? {
                Ok(tree) => tree,
                Err(fault) => return Ok(Err(fault)),
            }
        } else {
            self.string(from, stop, terminator)?
        };
        self.frames
            .last_mut()
            .expect("a scope is open")
            .tree
            .adopt(opener.name, tree);
        Ok(Ok(match terminator {
            Terminator::Closer(width, variant) => {
                if variant {
                    self.record(EditOperator::CloserVariant, stop..stop + width, CLOSER)?;
                }
                stop + width
            }
            Terminator::Boundary => {
                // Grounded in the definition when the tool is known; with no
                // known tool every opener is a boundary, a syntax repair.
                let operator = if self.frames[0].scope.is_some() {
                    EditOperator::MissingParameterClose
                } else {
                    EditOperator::InsertCloser
                };
                self.record(operator, stop..stop, CLOSER)?;
                stop
            }
            Terminator::End => {
                if self.end != CodecEnd::Truncated {
                    self.record(EditOperator::InsertCloser, stop..stop, CLOSER)?;
                }
                stop
            }
        }))
    }

    /// Where the value starting at `from` ends, and what ends it.
    fn terminator(&mut self, from: usize) -> Result<(usize, Terminator), StopReason> {
        let mut at = from;
        let found = loop {
            let Some(offset) = self.body[at..].iter().position(|&byte| byte == b'<') else {
                at = self.body.len();
                break Terminator::End;
            };
            at += offset;
            if let Some((width, variant)) = self.closer(at) {
                break Terminator::Closer(width, variant);
            }
            if let Some(opener) = self.opener(at)?
                && self.boundary(&opener.name.value)?
            {
                break Terminator::Boundary;
            }
            at += 1;
        };
        self.charge(at - from)?;
        Ok((at, found))
    }

    /// Whether the value ending at a terminator was cut off by a length stop.
    fn cut(&self, terminator: Terminator) -> bool {
        terminator == Terminator::End && self.end == CodecEnd::Truncated
    }

    /// A verbatim string value in `from..stop`.
    fn string(
        &mut self,
        from: usize,
        stop: usize,
        terminator: Terminator,
    ) -> Result<ArgTree, StopReason> {
        let cut = self.cut(terminator);
        let (mut start, mut end) = (from, stop);
        if start < end && self.body[start] == b'\n' {
            self.record(EditOperator::FramingNewline, start..start + 1, b"")?;
            start += 1;
        }
        if !cut && start < end && self.body[end - 1] == b'\n' {
            self.record(EditOperator::FramingNewline, end - 1..end, b"")?;
            end -= 1;
        }
        self.charge(end - start)?;
        let text = match core::str::from_utf8(&self.body[start..end]) {
            Ok(text) => text,
            Err(error) if cut && error.error_len().is_none() => {
                end = start + error.valid_up_to();
                core::str::from_utf8(&self.body[start..end]).expect("the valid prefix")
            }
            Err(error) => {
                let at = self.base + start + error.valid_up_to();
                return Ok(ArgTree::unresolved(
                    ArgumentFault::InvalidUtf8InArgument { at },
                    self.absolute(from..stop),
                ));
            }
        };
        let raw = self.absolute(start..end);
        let mut tree = ArgTree::value(Node {
            value: Value::String(text.to_owned()),
            span: Some(raw.clone()),
        });
        tree.leaves.push(ArgLeaf {
            pointer: String::new(),
            origin: ArgOrigin::XmlText,
            raw,
        });
        Ok(tree)
    }

    /// A typed value in `from..stop`, parsed by the `json` codec. A value
    /// ended by a missing closer must be complete JSON.
    fn typed(
        &mut self,
        from: usize,
        stop: usize,
        terminator: Terminator,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        let cut = self.cut(terminator);
        let repaired = !cut && !matches!(terminator, Terminator::Closer(..));
        let end = if cut {
            CodecEnd::Truncated
        } else {
            CodecEnd::Delimited
        };
        let raw = self.absolute(from..stop);
        let (decoded, trailing) =
            JsonCodec::scan(&self.body[from..stop], raw.start, end, self.governor)?;
        let fault = match (decoded, trailing) {
            (
                Decoded::Value {
                    node,
                    edits,
                    incomplete,
                },
                None,
            ) if !(incomplete && repaired) => {
                let (tree, markup) = json_tree(
                    node,
                    edits,
                    ArgOrigin::Json,
                    self.body,
                    self.base,
                    self.governor,
                )?;
                self.edits.extend(markup);
                return Ok(Ok(tree));
            }
            (Decoded::Fault(fault @ ArgumentFault::InvalidUtf8InArgument { .. }), _) => fault,
            // A missing closer was read before this value: it must be complete.
            _ if repaired => return Ok(Err(CodecFault::Malformed { at: raw.end })),
            (Decoded::Fault(fault), _) => fault,
            (Decoded::Value { .. }, at) => ArgumentFault::InvalidJson {
                at: at.unwrap_or(raw.end),
            },
        };
        Ok(Ok(ArgTree::unresolved(fault, raw)))
    }

    /// A whole-JSON-object body starting at `first`, plus at most one stray
    /// closer.
    fn json_body(
        &mut self,
        first: usize,
        span: Range<usize>,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        let (decoded, trailing) = JsonCodec::scan(
            &self.body[first..],
            self.base + first,
            self.end,
            self.governor,
        )?;
        let tree = match decoded {
            Decoded::Fault(fault) => return Ok(Ok(ArgTree::unresolved(fault, span))),
            Decoded::Value { node, edits, .. } => {
                let (tree, markup) = json_tree(
                    node,
                    edits,
                    ArgOrigin::Json,
                    self.body,
                    self.base,
                    self.governor,
                )?;
                self.edits.extend(markup);
                tree
            }
        };
        if let Some(stop) = trailing {
            let mut at = stop - self.base;
            if let Some((width, _)) = self.closer(at) {
                self.record(EditOperator::DeleteStrayCloser, at..at + width, b"")?;
                at = self.skip_space(at + width)?;
            }
            if at < self.body.len() {
                return Ok(Err(CodecFault::Malformed { at: self.base + at }));
            }
        }
        Ok(Ok(tree))
    }
}

/// Whether `scope` declares `name` (exactly, or as an alias).
fn declares(scope: Option<&ObjectShape>, name: &str) -> bool {
    scope.is_some_and(|scope| scope.resolve_name(name).is_some())
}
