// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The `xml-params` codec: `<parameter=NAME>value</parameter>` sequences.
use super::json::{Decoded, JsonCodec, tree as json_tree};
use super::{
    Codec, CodecContext, CodecEnd, CodecFault, CodecKind, CodecValue, edit, lossy, uncut, value,
};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, StopReason};
use crate::grammar::cost::K_CODEC;
use crate::grammar::edit::{EditOperator, MarkupEdit};
use crate::json::{Node, Value};
use crate::tool_call::{
    ArgLeaf, ArgOrigin, ArgTree, ArgumentFault, ObjectShape, Spanned, UnresolvedArgument,
    ValueShape,
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
/// - A parameter declared as a string (or with an unmodeled schema), and
///   any undeclared parameter, is a verbatim string: exactly one framing
///   newline on each side is consumed and recorded as a
///   `tool_call.xml.framing-newline` edit (a non-guess, single-reading
///   markup loss); nothing else about the text changes (`123` and `true`
///   stay strings). Its leaf has origin [`ArgOrigin::XmlText`]. Resolution
///   types an unmodeled parameter's text by the tool's exact schema
///   (`tool_call.value.schema-typed`); an unmodeled parameter whose value
///   begins with a `<parameter=` opener holds nested parameters, read as an
///   untyped object (`tool_call.markup.nested-parameter`) whose members
///   resolution types the same way.
/// - An enumeration value is read the same way, then trimmed of any
///   whitespace left around it (a non-verbatim type), recorded as
///   `tool_call.value.trim-nonverbatim`.
/// - A parameter declared as an object, number, integer or boolean is
///   parsed from its verbatim text by the `json` codec (exact number
///   spellings, tolerant repairs recorded as edits, leaves with origin
///   [`ArgOrigin::Json`]). Whitespace around an integer, number or boolean
///   beyond one framing newline on each side is recorded as
///   `tool_call.value.trim-nonverbatim` (with the value's raw span). An
///   object parameter whose value begins with a `<parameter=` opener instead
///   holds nested parameters (recorded as `tool_call.markup.nested-parameter`),
///   typed by the object's declared shape, so nested elements and inline
///   JSON produce the same tree.
/// - An array parameter whose value is a JSON array is parsed as one; any
///   other value is one item of the array, read by the item type and
///   recorded as `tool_call.xml.array-item`, so repeated elements collect
///   into one array when the call is resolved.
/// - Bytes inside a value or a parameter name that are not well-formed
///   UTF-8 are read as U+FFFD, one per maximal malformed subpart, each run
///   recorded as `tool_call.value.invalid-utf8` (a data loss keeping the
///   original bytes and offsets): the parameter keeps its decodable
///   content and the call is at most a best guess. JSON that cannot be
///   recovered, or text after the JSON value, leaves the parameter
///   unresolved with [`ArgumentFault::InvalidJson`].
/// - Repeated parameters are all recorded, in order, as duplicate members
///   (resolution decides what a repeat means for the declared type).
///
/// # Boundaries
///
/// A value ends at `</parameter>` (`</parameters>` is read as it, recorded
/// as `tool_call.markup.closer-variant`), at a `<parameter=NAME>` opener
/// where `NAME` (or an alias) is declared in the value's object scope or an
/// enclosing one, or at the end of the body:
///
/// - at a declared parameter's opener the missing closer is recorded as
///   `tool_call.markup.missing-parameter-close` (single reading, non-guess,
///   grounded in the definition: a declared parameter's opener never occurs
///   inside a value), and an already-bound `NAME` simply repeats;
/// - where the enclosing element closes (the slot's delimiter), as
///   `tool_call.markup.parameter-closed-by-element`;
/// - at the end of output under a non-length stop, as
///   `tool_call.markup.missing-closers-at-end` when the open value is a
///   complete typed value; a *string* still open at the end of output makes
///   the call malformed ([`CodecFault::Malformed`]), since the output may
///   have ended inside the text;
/// - at the end of output under a length stop, with no edit (truncation).
///
/// A typed value ended by a missing closer must be complete JSON there, or
/// the call is malformed ([`CodecFault::Malformed`]). An opener whose name
/// is not declared is not a boundary: it is text of a string value (and
/// leaves a typed value incomplete). When the call names no declared tool
/// (no tool set, an unknown or hallucinated name, or a grammar that captures
/// the name after the arguments), every `<parameter=` opener is a boundary,
/// recorded as the guess-flagged `tool_call.markup.undeclared-parameter-boundary`
/// (the opener may also be text), and every value is a string; such a call
/// is resolved, or found unresolvable (a hallucinated tool), against its
/// tool later.
///
/// A `</parameter>` outside any value closes the innermost nested object,
/// or, at the top level, is a stray closer deleted as
/// `tool_call.markup.stray-closer`. A body made only of closers and
/// whitespace is no arguments, recorded as one
/// `tool_call.markup.closers-only-empty` markup loss (usable only when the
/// tool requires no parameter). Any other text between parameters is
/// malformed.
///
/// # Whole-JSON bodies
///
/// A body whose first non-whitespace byte is `{` is one JSON object
/// (`<function=f>{"a":1}</function>`, recorded as
/// `tool_call.markup.json-body`), accepted only when the object is the
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
///         "tool_call.markup.parameter-closed-by-element",
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
    /// The width of a misspelled `<parameter=` read as it
    /// (`tool_call.markup.token-misspelled`).
    respelled: Option<usize>,
}

/// What ended a parameter value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Terminator {
    /// A closer of this width, and how it is spelled.
    Closer(usize, Spelling),
    /// The opener of a declared parameter.
    Boundary,
    /// The end of the body.
    End,
}

/// How a parameter closer is spelled.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Spelling {
    /// `</parameter>`.
    Canonical,
    /// `</parameters>` (`tool_call.markup.closer-variant`).
    Variant,
    /// One edit away from `</parameter>` (`tool_call.markup.closer-misspelled`).
    Misspelled,
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
        let result = if self.body.get(first) == Some(&b'{') {
            self.json_body(first, span)?
        } else if let Some(tree) = self.closers_only(first, span.clone())? {
            Ok(tree)
        } else {
            self.parameters(span, context)?
        };
        let mut tree = match result {
            Ok(tree) => tree,
            Err(fault) => return Ok(Err(fault)),
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

    /// Record `tool_call.value.invalid-utf8` edits, each in source order
    /// among the edits recorded so far.
    fn adopt(&mut self, invalid: Vec<MarkupEdit>) {
        for edit in invalid {
            let at = self
                .edits
                .iter()
                .position(|other| other.span.start > edit.span.start)
                .unwrap_or(self.edits.len());
            self.edits.insert(at, edit);
        }
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

    /// The closer at `at`: its width, and how it is spelled. A token within
    /// one edit of `</parameter>` that keeps its shape and is no exact
    /// spelling (`</paramete>`) is read as it, recorded as
    /// `tool_call.markup.closer-misspelled`.
    fn closer(&self, at: usize) -> Option<(usize, Spelling)> {
        let rest = &self.body[at..];
        if rest.starts_with(CLOSER_VARIANT) {
            return Some((CLOSER_VARIANT.len(), Spelling::Variant));
        }
        if rest.starts_with(CLOSER) {
            return Some((CLOSER.len(), Spelling::Canonical));
        }
        if !rest.starts_with(b"</") {
            return None;
        }
        let end = rest
            .iter()
            .take(CLOSER_VARIANT.len() + 1)
            .position(|&byte| byte == b'>')?;
        let token = &rest[..=end];
        let spellings = [CLOSER.to_vec(), CLOSER_VARIANT.to_vec()];
        (crate::grammar::misspelling::misspelled_class(token, [spellings.as_slice()]) == Some(0))
            .then_some((token.len(), Spelling::Misspelled))
    }

    /// Record the respelling of a closer read at `at..at + width`, if any.
    fn respelled(&mut self, at: usize, width: usize, spelling: Spelling) -> Result<(), StopReason> {
        match spelling {
            Spelling::Canonical => Ok(()),
            Spelling::Variant => self.record(EditOperator::CloserVariant, at..at + width, CLOSER),
            Spelling::Misspelled => {
                self.record(EditOperator::CloserMisspelled, at..at + width, CLOSER)
            }
        }
    }

    /// A body (from its first non-whitespace byte `first`) made only of
    /// parameter closers and whitespace, holding at least one closer: no
    /// arguments, with the closers recorded as one
    /// `tool_call.markup.closers-only-empty` markup loss. Whether the call
    /// may have no arguments is decided against its tool's required
    /// parameters when the call is resolved.
    fn closers_only(
        &mut self,
        first: usize,
        span: Range<usize>,
    ) -> Result<Option<ArgTree>, StopReason> {
        let (mut at, mut last) = (first, None);
        while at < self.body.len() {
            let Some((width, _)) = self.closer(at) else {
                return Ok(None);
            };
            last = Some(at + width);
            at = self.skip_space(at + width)?;
        }
        let Some(last) = last else {
            return Ok(None);
        };
        self.record(EditOperator::ClosersOnlyEmpty, first..last, b"")?;
        Ok(Some(ArgTree::value(Node {
            value: Value::Object(Vec::new()),
            span: Some(span),
        })))
    }

    /// The opener at `at`: `<parameter=` and a non-empty UTF-8 name free of
    /// whitespace and angle brackets, then `>`. A token within one edit of
    /// `<parameter=` that keeps its shape and misspells no closer
    /// (`<paramter=`) is read as it, recorded as
    /// `tool_call.markup.token-misspelled` (the opener-side mirror of a
    /// misspelled `</parameter>`).
    fn opener(&mut self, at: usize) -> Result<Option<Opener>, StopReason> {
        let rest = &self.body[at..];
        let (width, respelled) = if rest.starts_with(OPENER) {
            (OPENER.len(), None)
        } else {
            if !rest.starts_with(b"<") || rest.starts_with(b"</") {
                return Ok(None);
            }
            let Some(end) = rest
                .iter()
                .take(OPENER.len() + 1)
                .position(|&byte| byte == b'=')
            else {
                return Ok(None);
            };
            let token = &rest[..=end];
            let classes = [
                vec![OPENER.to_vec()],
                vec![CLOSER.to_vec(), CLOSER_VARIANT.to_vec()],
            ];
            if crate::grammar::misspelling::misspelled_class(
                token,
                classes.iter().map(Vec::as_slice),
            ) != Some(0)
            {
                return Ok(None);
            }
            (token.len(), Some(token.len()))
        };
        let from = at + width;
        let length = self.body[from..]
            .iter()
            .take_while(|&&byte| byte != b'>' && byte != b'<' && !byte.is_ascii_whitespace())
            .count();
        self.charge(length)?;
        let to = from + length;
        if length == 0 || self.body.get(to) != Some(&b'>') {
            return Ok(None);
        }
        // A name that is not well-formed UTF-8 is read with U+FFFD; the
        // edits are recorded when the opener is read as a parameter.
        let name = match core::str::from_utf8(&self.body[from..to]) {
            Ok(name) => name.to_owned(),
            Err(_) => String::from_utf8_lossy(&self.body[from..to]).into_owned(),
        };
        Ok(Some(Opener {
            start: at,
            name: Spanned {
                value: name,
                span: self.absolute(from..to),
            },
            end: to + 1,
            respelled,
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

    /// The operator recording a closer missing where the body ended: at the
    /// end of output, the end-of-output rule (usable only when the call is
    /// otherwise complete); where the enclosing element closed, the
    /// element-close rule; nothing for a length cut (truncation).
    const fn closed_at_end(&self) -> Option<EditOperator> {
        match self.end {
            CodecEnd::Delimited => Some(EditOperator::ParameterClosedByElement),
            CodecEnd::EndOfInput => Some(EditOperator::MissingClosersAtEnd),
            CodecEnd::Truncated => None,
        }
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
            let Some((width, spelling)) = self.closer(at) else {
                return Ok(Err(CodecFault::Malformed { at: self.base + at }));
            };
            if self.frames.len() > 1 {
                self.respelled(at, width, spelling)?;
                self.pop(at);
            } else {
                self.record(EditOperator::DeleteStrayCloser, at..at + width, b"")?;
            }
            at += width;
        }
        while self.frames.len() > 1 {
            if let Some(operator) = self.closed_at_end() {
                self.record(operator, at..at, CLOSER)?;
            }
            self.pop(at);
        }
        Ok(Ok(self.frames.pop().expect("the top-level scope").tree))
    }

    /// Read one parameter from its opener; the offset after it.
    fn parameter(&mut self, opener: Opener) -> Result<Result<usize, CodecFault>, StopReason> {
        if let Some(width) = opener.respelled {
            self.record(
                EditOperator::TokenMisspelled,
                opener.start..opener.start + width,
                OPENER,
            )?;
        }
        let name = opener.name.span.start - self.base..opener.name.span.end - self.base;
        if core::str::from_utf8(&self.body[name.clone()]).is_err() {
            let (_, invalid) = lossy(&self.body[name], opener.name.span.start, self.governor)?;
            self.adopt(invalid);
        }
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
        // An object parameter's nested elements are its members, typed by
        // its declared shape; an unmodeled parameter's (`$ref`, `anyOf`,
        // ...) are members of an untyped object, typed later by the tool's
        // exact schema (`tool_call.value.schema-typed`).
        let nested = match declared {
            Some(ValueShape::Object(inner)) => Some(Some(inner)),
            Some(ValueShape::Unmodeled) => Some(None),
            _ => None,
        };
        if let Some(inner) = nested {
            let next = self.skip_space(from)?;
            if self.body[next..].starts_with(OPENER) {
                self.governor
                    .charge(Resource::Depth, size(self.frames.len()))?;
                self.record(EditOperator::NestedParameter, opener.start..from, b"")?;
                self.frames.push(Frame {
                    scope: inner,
                    tree: ArgTree::value(Node {
                        value: Value::Object(Vec::new()),
                        span: Some(self.absolute(from..from)),
                    }),
                    name: Some(opener.name),
                });
                return Ok(Ok(from));
            }
        }
        let (stop, terminator) = self.terminator(from)?;
        let tree = match self.value(declared, from, stop, terminator)? {
            Ok(tree) => tree,
            Err(fault) => return Ok(Err(fault)),
        };
        self.frames
            .last_mut()
            .expect("a scope is open")
            .tree
            .adopt(opener.name, tree);
        Ok(Ok(match terminator {
            Terminator::Closer(width, spelling) => {
                self.respelled(stop, width, spelling)?;
                stop + width
            }
            Terminator::Boundary => {
                // Grounded in the definition when the tool is known; with no
                // known tool every opener is a boundary, a guess (the opener
                // may be text of the value).
                let operator = if self.frames[0].scope.is_some() {
                    EditOperator::MissingParameterClose
                } else {
                    EditOperator::UndeclaredParameterBoundary
                };
                self.record(operator, stop..stop, CLOSER)?;
                stop
            }
            Terminator::End => {
                if let Some(operator) = self.closed_at_end() {
                    self.record(operator, stop..stop, CLOSER)?;
                }
                stop
            }
        }))
    }

    /// The value of a parameter declared as `declared` (`None` when the
    /// parameter or the tool is not declared) in `from..stop`.
    fn value(
        &mut self,
        declared: Option<&ValueShape>,
        from: usize,
        stop: usize,
        terminator: Terminator,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        match declared {
            Some(ValueShape::Array(item)) => {
                let next = self.skip_space(from)?;
                if self.body[next..stop].starts_with(b"[") {
                    return self.typed(from, stop, terminator, false);
                }
                // A value that is not a JSON array is one item of the array.
                let item = match self.value(Some(item), from, stop, terminator)? {
                    Ok(item) => item,
                    Err(fault) => return Ok(Err(fault)),
                };
                if item.is_stand_in() {
                    return Ok(Ok(item));
                }
                self.record(EditOperator::ArrayItem, from..stop, b"")?;
                Ok(Ok(wrap_item(item)))
            }
            Some(ValueShape::Object(_)) => self.typed(from, stop, terminator, false),
            Some(ValueShape::Integer | ValueShape::Number | ValueShape::Boolean) => {
                self.typed(from, stop, terminator, true)
            }
            Some(ValueShape::Enumeration(_)) => self.string(from, stop, terminator, true),
            Some(ValueShape::String(_) | ValueShape::Unmodeled) | None => {
                self.string(from, stop, terminator, false)
            }
        }
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
            if let Some((width, spelling)) = self.closer(at) {
                break Terminator::Closer(width, spelling);
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

    /// A verbatim string value in `from..stop`; with `trim` (an enumeration
    /// value), whitespace left after the framing newlines is trimmed and
    /// recorded as `tool_call.value.trim-nonverbatim`. A string still open
    /// at the end of output (not cut by a length stop) makes the call
    /// malformed: the output may have ended inside the text.
    fn string(
        &mut self,
        from: usize,
        stop: usize,
        terminator: Terminator,
        trim: bool,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        if terminator == Terminator::End && self.end == CodecEnd::EndOfInput {
            return Ok(Err(CodecFault::Malformed {
                at: self.base + stop,
            }));
        }
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
        if trim {
            let text = &self.body[start..end];
            let inner = text.trim_ascii();
            if inner.len() < text.len() {
                let leading = text.len() - text.trim_ascii_start().len();
                let trimmed = start + leading..start + leading + inner.len();
                let replacement = inner.to_vec();
                self.record(EditOperator::TrimNonverbatim, start..end, &replacement)?;
                (start, end) = (trimmed.start, trimmed.end);
            }
        }
        self.charge(end - start)?;
        if cut {
            end = start + uncut(&self.body[start..end]);
        }
        let (text, invalid) = lossy(&self.body[start..end], self.base + start, self.governor)?;
        self.adopt(invalid);
        let raw = self.absolute(start..end);
        let mut tree = ArgTree::value(Node {
            value: Value::String(text),
            span: Some(raw.clone()),
        });
        tree.leaves.push(ArgLeaf {
            pointer: String::new(),
            origin: ArgOrigin::XmlText,
            raw,
        });
        Ok(Ok(tree))
    }

    /// A typed value in `from..stop`, parsed by the `json` codec. A value
    /// ended by a missing closer must be complete JSON. With `scalar` (an
    /// integer, number or boolean), whitespace around the value beyond one
    /// framing newline on each side is recorded as
    /// `tool_call.value.trim-nonverbatim`.
    fn typed(
        &mut self,
        from: usize,
        stop: usize,
        terminator: Terminator,
        scalar: bool,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        let cut = self.cut(terminator);
        let repaired = !cut && !matches!(terminator, Terminator::Closer(..));
        let end = if cut {
            CodecEnd::Truncated
        } else {
            CodecEnd::Delimited
        };
        let raw = self.absolute(from..stop);
        let (decoded, trailing, invalid) =
            JsonCodec::scan(&self.body[from..stop], raw.start, end, self.governor)?;
        self.adopt(invalid);
        let fault = match (decoded, trailing) {
            (
                Decoded::Value {
                    node,
                    edits,
                    incomplete,
                },
                None,
            ) if !(incomplete && repaired) => {
                if scalar {
                    self.trim(from, stop)?;
                }
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

    /// Record the trim of a scalar's raw text in `from..stop` when it holds
    /// whitespace beyond one framing newline on each side.
    fn trim(&mut self, from: usize, stop: usize) -> Result<(), StopReason> {
        let text = &self.body[from..stop];
        let inner = text.trim_ascii();
        let leading = &text[..text.len() - text.trim_ascii_start().len()];
        let trailing = &text[text.trim_ascii_end().len()..];
        let framing = |side: &[u8]| side.is_empty() || side == b"\n";
        if inner.is_empty() || (framing(leading) && framing(trailing)) {
            return Ok(());
        }
        let replacement = inner.to_vec();
        self.record(EditOperator::TrimNonverbatim, from..stop, &replacement)
    }

    /// A whole-JSON-object body starting at `first`, plus at most one stray
    /// closer.
    fn json_body(
        &mut self,
        first: usize,
        span: Range<usize>,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        let (decoded, trailing, invalid) = JsonCodec::scan(
            &self.body[first..],
            self.base + first,
            self.end,
            self.governor,
        )?;
        self.adopt(invalid);
        let value_end = trailing.map_or(self.body.len(), |stop| stop - self.base);
        self.record(EditOperator::JsonBody, first..value_end, b"")?;
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

/// An item value as a one-element array, its leaves and unresolved values
/// re-rooted under `/0`.
fn wrap_item(item: ArgTree) -> ArgTree {
    let span = item.root.span.clone();
    let mut tree = ArgTree::value(Node {
        value: Value::Array(vec![item.root]),
        span,
    });
    tree.leaves = item
        .leaves
        .into_iter()
        .map(|leaf| ArgLeaf {
            pointer: format!("/0{}", leaf.pointer),
            ..leaf
        })
        .collect();
    tree.unresolved = item
        .unresolved
        .into_iter()
        .map(|unresolved| UnresolvedArgument {
            pointer: format!("/0{}", unresolved.pointer),
            ..unresolved
        })
        .collect();
    tree.arguments_were_stringified = item.arguments_were_stringified;
    tree
}

/// Whether `scope` declares `name` (exactly, or as an alias).
fn declares(scope: Option<&ObjectShape>, name: &str) -> bool {
    scope.is_some_and(|scope| scope.resolve_name(name).is_some())
}
