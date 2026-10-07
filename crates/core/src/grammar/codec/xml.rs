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
/// # Closer text in a value
///
/// A string value (declared as a string, unmodeled, or not declared; an
/// array item read as one) may hold the exact text of a closer:
/// `<parameter>v</parameter>`, or a doubled `</parameter>`. Every closer
/// before the next declared parameter's opener (or the end of the body) is
/// a place the value may end, and the readings are weighed by the rule
/// costs of the repairs each needs: its own end's (a variant or misspelled
/// closer), and what it leaves before the next parameter, which must be
/// whitespace and closers (closing open nested objects, then stray) or
/// begin a parameter. The least costly reading wins, the earliest among
/// equals, so a value whose first closer needs nothing more ends there. A
/// value ending at a later closer holds the earlier ones as text, recorded
/// as `tool_call.value.closer-in-value` (no guess: every other reading
/// needs a repair, or leaves text between parameters that no repair reads),
/// spanning from the first closer it holds to where it ends. A declared
/// parameter's opener always ends the region the value may run to, and so
/// does a misspelled one after a canonical closer (between parameters it is
/// a parameter boundary); text after a closer is read into the value only up
/// to a later closer, never up to an opener or the end of the body. A string
/// whose declared pattern rejects the value holding closer text ends at its
/// first closer. Typed values (parsed as JSON) and enumerations end at their
/// first closer.
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
/// ends the slot body with its delimiters. A body may also read on past a
/// closer delimiter (`</function>`, `</tool_call>`, their variants) beside
/// the reading that ends there, so a value may hold the call's closers as
/// text too: the engine weighs those readings by the same costs
/// ([`EditOperator::CloserInValue`]).
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

/// A place a value may end (see `Reader::terminator`): an offset into the
/// body and what ends the value there; for an opener, the open scopes that
/// declare its parameter, whether it is misspelled, and whether it is
/// `text` of the value (an undeclared parameter's: no end of the value,
/// though after a closer it begins a parameter).
struct Candidate {
    at: usize,
    terminator: Terminator,
    owners: Vec<usize>,
    respelled: bool,
    text: bool,
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
        let first = self.skip_space(from)?;
        let verbatim = verbatim(declared, self.body[first..].starts_with(b"["));
        let (mut stop, mut terminator, mut inner) = self.terminator(from, verbatim)?;
        if inner.is_some() && !self.admits(declared, from, stop, terminator)? {
            // The declared pattern rejects the value holding closer text:
            // that is no reading of the parameter, so it ends at its first
            // closer.
            (stop, terminator, inner) = self.terminator(from, false)?;
        }
        let tree = match self.value(declared, from, stop, terminator)? {
            Ok(tree) => tree,
            Err(fault) => return Ok(Err(fault)),
        };
        if let Some(inner) = inner {
            self.closer_text(inner..stop)?;
        }
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

    /// Where the value starting at `from` ends, and what ends it, and for a
    /// verbatim value the first exact closer spelling (`</parameter>`,
    /// `</parameters>`) it holds as text, if any.
    ///
    /// A typed value (parsed as JSON) ends at the first canonical
    /// `</parameter>` unless a parameter opener (canonical, of a declared
    /// parameter) comes before it; a repaired spelling (the `</parameters>`
    /// variant, a misspelled closer, a misspelled opener) ends it only where
    /// no canonical closer comes before that opener or the end of the body.
    ///
    /// A verbatim value ([`verbatim`]) may hold closer text: every closer
    /// before the next declared parameter's opener (or the end of the body)
    /// is a place it may end, and so are that opener (or end) and every
    /// misspelled opener before it. Each reading is weighed by the repairs
    /// it needs, with the rule costs of the edits it would record: its own
    /// end's (a variant or misspelled closer, a missing closer), and those
    /// of the bytes it leaves before the next parameter, which must be only
    /// whitespace and closers (closing open nested objects, then stray). A
    /// reading leaving other text there is malformed. The least costly
    /// reading wins, the earliest among equals (so a value whose first
    /// closer needs nothing more ends there, as before); when every reading
    /// is malformed, the value ends as a typed one would.
    fn terminator(
        &mut self,
        from: usize,
        verbatim: bool,
    ) -> Result<(usize, Terminator, Option<usize>), StopReason> {
        let mut at = from;
        let mut ends: Vec<Candidate> = Vec::new();
        // Whether a canonical closer came first (the value may have ended).
        let mut between = false;
        let last = loop {
            let Some(offset) = self.body[at..].iter().position(|&byte| byte == b'<') else {
                at = self.body.len();
                break Candidate {
                    at,
                    terminator: Terminator::End,
                    owners: Vec::new(),
                    respelled: false,
                    text: false,
                };
            };
            at += offset;
            if let Some((width, spelling)) = self.closer(at) {
                if !verbatim && spelling == Spelling::Canonical {
                    self.charge(at - from)?;
                    return Ok((at, Terminator::Closer(width, spelling), None));
                }
                between |= spelling == Spelling::Canonical;
                ends.push(Candidate {
                    at,
                    terminator: Terminator::Closer(width, spelling),
                    owners: Vec::new(),
                    respelled: false,
                    text: false,
                });
            } else if let Some(opener) = self.opener(at)? {
                let text = !self.boundary(&opener.name.value)?;
                let candidate = Candidate {
                    at,
                    terminator: Terminator::Boundary,
                    owners: self.owners(&opener.name.value)?,
                    respelled: opener.respelled.is_some(),
                    text,
                };
                // A declared parameter's opener ends the region: written
                // exactly, or (between parameters, after a canonical closer)
                // misspelled. Only before any closer may a misspelled opener
                // be text of the value.
                if !candidate.text && (!candidate.respelled || (verbatim && between)) {
                    break candidate;
                }
                // An opener of an undeclared parameter is text of the value
                // (no end of it), but after a closer it begins a parameter.
                if verbatim || !candidate.text {
                    ends.push(candidate);
                }
            }
            at += 1;
        };
        self.charge(last.at - from)?;
        if verbatim && let Some(index) = self.cheapest(&ends, &last)? {
            let chosen = ends.get(index).unwrap_or(&last);
            let inner = ends[..index]
                .iter()
                .find(|candidate| {
                    matches!(
                        candidate.terminator,
                        Terminator::Closer(_, Spelling::Canonical | Spelling::Variant)
                    )
                })
                .map(|candidate| candidate.at);
            return Ok((chosen.at, chosen.terminator, inner));
        }
        // A typed value, or a verbatim one every reading of which is
        // malformed: the first canonical closer, else the first repaired
        // spelling.
        let chosen = ends
            .iter()
            .find(|candidate| {
                candidate.terminator == Terminator::Closer(CLOSER.len(), Spelling::Canonical)
            })
            .or_else(|| ends.iter().find(|candidate| !candidate.text))
            .unwrap_or(&last);
        Ok((chosen.at, chosen.terminator, None))
    }

    /// Whether the string value `from..stop`, ended by `terminator`, is one
    /// the declared pattern admits (a parameter declared as a string with a
    /// pattern; any other value is admitted). Its framing newlines are not
    /// part of it. Charges the pattern's matching.
    fn admits(
        &mut self,
        declared: Option<&ValueShape>,
        from: usize,
        stop: usize,
        terminator: Terminator,
    ) -> Result<bool, StopReason> {
        let Some(ValueShape::String(Some(constraint))) = declared else {
            return Ok(true);
        };
        let (mut start, mut end) = (from, stop);
        if start < end && self.body[start] == b'\n' {
            start += 1;
        }
        if !self.cut(terminator) && start < end && self.body[end - 1] == b'\n' {
            end -= 1;
        }
        self.charge(end - start)?;
        let text = String::from_utf8_lossy(&self.body[start..end]);
        constraint.pattern.is_match(&text, self.governor)
    }

    /// Record the closer text a value holds (`tool_call.value.closer-in-value`),
    /// from its first exact closer to where the value ends (`range`): a
    /// reading ending the value at that closer was weighed. In source order
    /// among the value's own edits (its framing newlines), the last recorded.
    fn closer_text(&mut self, range: Range<usize>) -> Result<(), StopReason> {
        let span = self.absolute(range);
        let edit = edit(
            EditOperator::CloserInValue,
            span,
            self.body,
            self.base,
            Vec::new(),
            self.governor,
        )?;
        let at = self.edits.len()
            - self
                .edits
                .iter()
                .rev()
                .take_while(|other| other.span.start > edit.span.start)
                .count();
        self.edits.insert(at, edit);
        Ok(())
    }

    /// The scope indices (into the open frames) that declare `name`.
    fn owners(&mut self, name: &str) -> Result<Vec<usize>, StopReason> {
        self.charge(self.frames.len())?;
        Ok(self
            .frames
            .iter()
            .enumerate()
            .filter(|(_, frame)| declares(frame.scope, name))
            .map(|(index, _)| index)
            .collect())
    }

    /// The least costly reading of a verbatim value among the places it may
    /// end (`ends`, in order, then `last`; see [`Self::terminator`]): its
    /// index (`ends.len()` for `last`), the earliest among equals, or
    /// `None` when every reading is malformed. One pass from the back.
    fn cheapest(
        &mut self,
        ends: &[Candidate],
        last: &Candidate,
    ) -> Result<Option<usize>, StopReason> {
        let nested = self.frames.len() - 1;
        self.charge(ends.len().saturating_mul(nested.max(1)))?;
        let cost = |operator: EditOperator| operator.rule().cost;
        let at_end = self.closed_at_end().map_or(0, cost);
        // Ending a value at an opener rather than at a closer.
        let missing = if self.frames[0].scope.is_some() {
            cost(EditOperator::MissingParameterClose)
        } else {
            cost(EditOperator::UndeclaredParameterBoundary)
        };
        // What the opener or end `stop` costs once the value has ended and
        // `open` nested objects are still open: the objects it closes, and
        // its own respelling.
        let settle = |stop: &Candidate, open: usize| -> u64 {
            if stop.terminator == Terminator::End {
                return size(open).saturating_mul(at_end);
            }
            let owner = stop
                .owners
                .iter()
                .copied()
                .filter(|owner| *owner <= open)
                .max()
                .unwrap_or(open);
            let respelled = if stop.respelled {
                cost(EditOperator::TokenMisspelled)
            } else {
                0
            };
            size(open - owner)
                .saturating_mul(missing)
                .saturating_add(respelled)
        };
        let spelling = |terminator: Terminator| match terminator {
            Terminator::Closer(_, Spelling::Variant) => cost(EditOperator::CloserVariant),
            Terminator::Closer(_, Spelling::Misspelled) => cost(EditOperator::CloserMisspelled),
            _ => 0,
        };
        let mut best: Option<(u64, usize)> = None;
        let mut consider = |reading: Option<u64>, index: usize| {
            if let Some(reading) = reading
                && best.is_none_or(|(least, _)| reading <= least)
            {
                best = Some((reading, index));
            }
        };
        // A value ends at an opener, or at the end of the body, only where
        // no canonical closer comes before it: text after a closer is never
        // read into the value up to an opener (text between parameters is
        // malformed), only up to a later closer.
        let first = ends.iter().position(|candidate| {
            candidate.terminator == Terminator::Closer(CLOSER.len(), Spelling::Canonical)
        });
        let canonical = |index: usize| first.is_some_and(|first| first < index);
        // The value runs to `last`: a string still open at the end of
        // output is malformed (the output may have ended inside it).
        let whole = match last.terminator {
            _ if canonical(ends.len()) => None,
            Terminator::End if self.end == CodecEnd::EndOfInput => None,
            Terminator::End => Some(size(nested + 1).saturating_mul(at_end)),
            _ => Some(missing.saturating_add(settle(last, nested))),
        };
        consider(whole, ends.len());
        // The spelling costs of the closers between the current place and
        // the next opener or end (`stop`), latest first, while only
        // whitespace separates them; `None` once text does.
        let mut tail: Option<Vec<u64>> = Some(Vec::new());
        let mut stop = last;
        for (index, candidate) in ends.iter().enumerate().rev() {
            let next = ends.get(index + 1).map_or(last.at, |next| next.at);
            if let Terminator::Closer(width, _) = candidate.terminator {
                if !self.body[candidate.at + width..next]
                    .iter()
                    .all(u8::is_ascii_whitespace)
                {
                    tail = None;
                }
                // Ending here, the closers after it close open nested
                // objects (paying their respelling), then are stray.
                let reading = tail.as_ref().map(|tail| {
                    let pops = tail.len().min(nested);
                    let popped: u64 = tail.iter().rev().take(pops).sum();
                    let strays = size(tail.len() - pops)
                        .saturating_mul(cost(EditOperator::DeleteStrayCloser));
                    settle(stop, nested - pops)
                        .saturating_add(popped)
                        .saturating_add(strays)
                        .saturating_add(spelling(candidate.terminator))
                });
                consider(reading, index);
                if let Some(tail) = tail.as_mut() {
                    tail.push(spelling(candidate.terminator));
                }
            } else {
                // A misspelled opener of a declared parameter: the value may
                // end there; and the closers before any opener lead up to it.
                if !candidate.text && !canonical(index) {
                    consider(
                        Some(missing.saturating_add(settle(candidate, nested))),
                        index,
                    );
                }
                tail = Some(Vec::new());
                stop = candidate;
            }
        }
        Ok(best.map(|(_, index)| index))
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

/// Whether a value of a parameter declared as `declared` (`None` when the
/// parameter or the tool is not declared) is verbatim text, which may hold
/// closer text (see `Reader::terminator`): a string, an unmodeled or
/// undeclared parameter, or an array item read as one of those (a value
/// that is not a JSON array, `bracket` telling whether it begins with
/// `[`). Typed values are parsed as JSON, and an enumeration's members
/// (trimmed, a non-verbatim type) never hold closer text.
fn verbatim(declared: Option<&ValueShape>, bracket: bool) -> bool {
    match declared {
        None | Some(ValueShape::String(_) | ValueShape::Unmodeled) => true,
        Some(ValueShape::Array(item)) => !bracket && verbatim(Some(item), false),
        Some(_) => false,
    }
}

/// Whether `scope` declares `name` (exactly, or as an alias).
fn declares(scope: Option<&ObjectShape>, name: &str) -> bool {
    scope.is_some_and(|scope| scope.resolve_name(name).is_some())
}
