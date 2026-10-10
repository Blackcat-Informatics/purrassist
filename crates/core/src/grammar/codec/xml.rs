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
    ArgLeaf, ArgOrigin, ArgTree, ArgumentFault, CollectedMember, Nesting, ObjectShape, ScopeGraph,
    ScopeNode, Spanned, UnknownParameters, UnresolvedArgument, ValueShape,
};
use core::ops::Range;

/// The parameter opener prefix; the name runs to the next `>`.
const OPENER: &[u8] = b"<parameter=";
use super::{XML_CLOSER as CLOSER, XML_CLOSER_VARIANT as CLOSER_VARIANT};

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
///   resolution types the same way. The same holds for an unmodeled item of
///   an array parameter (see below).
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
/// - An array parameter declared with object items (or unmodeled items,
///   `$ref`, `anyOf`, ...) whose item begins with a `<parameter=` opener
///   holds one nested object item: its elements are the item's members
///   (recorded as `tool_call.markup.nested-parameter`, typed by the item
///   object's declared shape, or untyped for an unmodeled item and typed later
///   by the tool's exact schema), and the item is recorded as
///   `tool_call.xml.array-item` where it closes. Each repetition of the
///   parameter is one more item, collected in source order with items written
///   as inline JSON. An array of strings (or any verbatim or scalar item) never
///   nests: tags in its value stay text. Nesting needs the tool known when the
///   body is read: with no declared tool there is no scope, so no value nests.
///   An item's members are declared only in the item's scope, so a member
///   whose name is the array's own name is the item's member (a child), never
///   the next item. That holds for an unmodeled item too (a `$ref` or an
///   `anyOf`, recursive or not): the tool's exact schema declares its member
///   names, and the call's scope graph ([`ScopeGraph`], compiled once per
///   tool from the exact schema, one node per schema location, a recursive
///   `$ref` a back-edge) supplies them, so such an item nests like its inline
///   twin. Under a union scope (`anyOf`/`oneOf`/`allOf`) a member nests when
///   its body begins with `<parameter=` and any object branch declares it an
///   object or an array of objects. Typing stays with the exact schema,
///   except that a member whose every non-null branch is an array reads text
///   that is not a JSON array as one item, and its occurrences collect into
///   one array. A scope that names no member (a free-form object, a `$ref` to
///   a non-object) has unknown membership (an opaque scope): an opener an
///   enclosing scope declares closes it as a guess
///   (`tool_call.markup.unmodeled-scope-boundary`), never as the
///   definition-grounded `tool_call.markup.missing-parameter-close`.
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
///   inside a value), and an already-bound `NAME` simply repeats; the nested
///   objects such an opener closes are closed the same way, except an
///   object whose scope names no member, closed as the guess
///   `tool_call.markup.unmodeled-scope-boundary`;
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
/// to a later closer, never up to an opener or the end of the body. Typed
/// values (parsed as JSON) and enumerations end at their first closer.
///
/// Where a closer sits on its line (after a newline, mid-line, indented)
/// and whether the value is framed by newlines never decide how it is read
/// (the maintainer: "Newlines have zero meaning here"). What follows the
/// closers a value would hold decides:
///
/// - a closer that closes an element the value opened (`<parameter>` ...
///   `</parameter>`, `<tool_calls>` ... `</tool_calls>`, by tag name) is
///   paired markup, text of the value;
/// - unpaired closers followed only by closers and whitespace are stray
///   markup, never held (`a.py</parameter></parameter>` reads as
///   `a.py\n</parameter>\n</parameter>`);
/// - a call opener after an unpaired closer is the next call: no reading
///   holds it (two calls in a row are never one call holding the other);
/// - text after an unpaired closer, before the closer the value would end
///   at, gives the body two readings that cost the same: the value holding
///   the text (recorded as `tool_call.value.text-after-closer`) and, as the
///   value's [`CodecValue::alternative`], the value ending at that closer
///   with the bytes up to the holding reading's end dropped as text between
///   parameters (`tool_call.markup.text-between-parameters`, a data loss).
///   Both are guesses, so the call is `Ambiguous`, never usable on a
///   mutating tool. When the declared pattern of a string rejects either
///   value, neither is a reading: the value ends at its first closer (and
///   the text after it makes the call malformed).
///
/// ```
/// use purrassist_core::grammar::codec::{Codec, CodecContext, CodecEnd, XmlParamsCodec};
/// use purrassist_core::{Budget, Governor};
/// let read = |body: &[u8]| {
///     let mut governor = Governor::new(Budget::default());
///     XmlParamsCodec::begin(0, &mut governor)
///         .unwrap()
///         .finish(body, 0..body.len(), CodecEnd::Delimited, CodecContext::default(), &mut governor)
///         .unwrap()
///         .unwrap()
/// };
/// // The same text after a closer, mid-line or on a line of its own.
/// for body in [
///     &b"<parameter=a>\nx</parameter> more\n</parameter>"[..],
///     &b"<parameter=a>\nx\n</parameter>\nmore\n</parameter>"[..],
/// ] {
///     let value = read(body);
///     assert!(value.tree.root.to_json().contains("more"));
///     let other = value.alternative.expect("the other reading");
///     assert_eq!(other.tree.root.to_json(), r#"{"a":"x"}"#);
/// }
/// // Stray closers, and paired markup, in any layout: one reading.
/// let stray = read(b"<parameter=a>\na.py</parameter></parameter>");
/// assert_eq!(stray.tree.root.to_json(), r#"{"a":"a.py"}"#);
/// assert!(stray.alternative.is_none());
/// let paired = read(b"<parameter=a>x <parameter>v</parameter> y</parameter>");
/// assert_eq!(paired.tree.root.to_json(), r#"{"a":"x <parameter>v</parameter> y"}"#);
/// assert!(paired.alternative.is_none());
/// ```
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
/// let context = CodecContext { shape: tools.argument_shape("edit"), ..CodecContext::default() };
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
        let base = span.start;
        let (result, forked) = Reader::new(body, base, end, context, Mode::Hold, governor)
            .run(span.clone(), context)?;
        let Ok(mut value) = result else {
            return Ok(result);
        };
        if forked {
            // The other reading: every value that held text after an
            // unpaired closer ends at that closer instead.
            let (other, _) = Reader::new(body, base, end, context, Mode::EndFirst, governor)
                .run(span, context)?;
            value.alternative = other.ok().map(Box::new);
        }
        Ok(Ok(value))
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

/// Where a value ends (see `Reader::terminator`): the offset into the
/// body, what ends it there, the first exact closer spelling it holds as
/// text (if any), and, when it holds text after an unpaired closer, that
/// closer's offset (the value has a second reading ending there).
struct Ending {
    stop: usize,
    terminator: Terminator,
    inner: Option<usize>,
    fork: Option<usize>,
}

impl Ending {
    /// A value ending at `stop`, holding no closer.
    const fn at(stop: usize, terminator: Terminator) -> Self {
        Self {
            stop,
            terminator,
            inner: None,
            fork: None,
        }
    }
}

/// One object scope being read: the top-level arguments or a nested object
/// parameter.
struct Frame<'t> {
    scope: Scope<'t>,
    tree: ArgTree,
    /// The parameter holding this object; `None` at the top level.
    name: Option<Spanned<String>>,
    /// Whether this object is one item of the array parameter `name`
    /// (repeated nested elements of an array of objects): it is adopted as a
    /// one-element array, recorded as `tool_call.xml.array-item`.
    item: bool,
    /// Whether the parameter `name` is an array member of a graph scope
    /// ([`Scope::Schema`]), whose repeated occurrences are collected into
    /// one array when adopted.
    collect: bool,
}

struct Reader<'b, 't, 'g> {
    body: &'b [u8],
    base: usize,
    end: CodecEnd,
    /// Whether the call's tool is known (a declared tool's shape is given).
    known: bool,
    /// The scope graph of the call's tool (see [`Scope::Schema`]).
    graph: Option<&'t ScopeGraph>,
    frames: Vec<Frame<'t>>,
    edits: Vec<MarkupEdit>,
    /// The dialect's call openers and closers (see `CodecContext`).
    openers: &'t [Vec<u8>],
    closers: &'t [Vec<u8>],
    /// Which reading of a value holding text after an unpaired closer this
    /// pass reads (see [`Mode`]).
    mode: Mode,
    /// Whether some value of this pass held text after an unpaired closer
    /// (the body has a second reading).
    forked: bool,
    governor: &'g mut Governor,
}

/// Which of the two readings of a value holding text after an unpaired
/// closer a pass reads (see `Reader::terminator`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Hold the text (`tool_call.value.text-after-closer`).
    Hold,
    /// End the value at the closer and drop the text
    /// (`tool_call.markup.text-between-parameters`).
    EndFirst,
}

/// Whether a verbatim value may end at a place, holding what comes before
/// it (see `Held::admits`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hold {
    /// It holds no unpaired closer, or holds one followed by nothing but
    /// value text it reads cleanly.
    Clean,
    /// It holds text after an unpaired closer: one of two readings.
    Forked,
    /// It holds unpaired closers followed only by closers and whitespace
    /// (stray markup), or a whole call after one (the next call): no
    /// reading.
    Never,
}

/// What a verbatim value read on past its first unpaired closer would
/// hold (see `Reader::terminator`): the first closer it holds that closes
/// no markup opened in the value and lies outside any call it holds whole,
/// and what comes after it. Where the closer sits on its line (after a
/// newline, mid-line, indented) carries no meaning. Offsets are into the
/// body.
struct Held {
    /// The first unpaired closer (a parameter or a call closer) it holds.
    first: Option<usize>,
    /// The first text (not whitespace, not an unpaired closer) after `first`.
    text: Option<usize>,
    /// The first call it holds whole after `first`.
    call: Option<usize>,
    /// For each closer tag name (see `Reader::tag`), how many elements of
    /// that name the value opened and has not closed: a closer of the same
    /// name closes one (it is paired markup, text of the value).
    open: Vec<usize>,
}

impl Held {
    /// An unpaired closer at `at` outside any call the value holds.
    fn closer(&mut self, at: usize) {
        self.first.get_or_insert(at);
    }

    /// The bytes `range`, outside any call the value holds and not an
    /// unpaired closer: text, unless only whitespace.
    fn text(&mut self, body: &[u8], range: Range<usize>) {
        let start = range.start;
        let Some(offset) = body[range]
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
        else {
            return;
        };
        self.mark(start + offset);
    }

    /// Text at `at`.
    fn mark(&mut self, at: usize) {
        if self.first.is_some() {
            self.text.get_or_insert(at);
        }
    }

    /// The opener of a call the value holds whole, at `at`.
    fn call(&mut self, at: usize) {
        if self.first.is_some() {
            self.call.get_or_insert(at);
        }
    }

    /// Whether the value may end at `stop`, holding what comes before it.
    /// Closers followed by nothing but closers and whitespace are stray
    /// markup (issue #3 §3: "repeated or stray closers are all one
    /// class"); a call after an unpaired closer is the next call (two calls
    /// in a row are never one call holding the other); text after an
    /// unpaired closer is either text of the value or text between
    /// parameters, and nothing in the layout tells which ("Newlines have
    /// zero meaning here"), so that reading is one of two.
    fn admits(&self, stop: usize) -> Hold {
        if self.first.is_none_or(|first| first >= stop) {
            return Hold::Clean;
        }
        if self.call.is_some_and(|call| call < stop) {
            return Hold::Never;
        }
        if self.text.is_some_and(|text| text < stop) {
            Hold::Forked
        } else {
            Hold::Never
        }
    }
}

impl<'b, 't, 'g> Reader<'b, 't, 'g> {
    /// A pass over `body` (starting at absolute offset `base`) reading
    /// `mode`'s reading of a value holding text after an unpaired closer.
    fn new(
        body: &'b [u8],
        base: usize,
        end: CodecEnd,
        context: CodecContext<'t>,
        mode: Mode,
        governor: &'g mut Governor,
    ) -> Self {
        Self {
            body,
            base,
            end,
            known: context.shape.is_some(),
            graph: context.shape.map(|shape| &shape.scopes),
            frames: Vec::new(),
            edits: Vec::new(),
            openers: context.openers,
            closers: context.closers,
            mode,
            forked: false,
            governor,
        }
    }
}

impl<'t> Reader<'_, 't, '_> {
    /// Read the body; the value, and whether a value held text after an
    /// unpaired closer (the body has another reading).
    fn run(
        mut self,
        span: Range<usize>,
        context: CodecContext<'t>,
    ) -> Result<(Result<CodecValue, CodecFault>, bool), StopReason> {
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
            Err(fault) => return Ok((Err(fault), self.forked)),
        };
        tree.tool = tool;
        Ok((value(tree, self.edits, self.governor)?, self.forked))
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

    /// Read the call markup at `at` inside a call the value holds (`nested`
    /// deep): its parameters and closers are its own, text of the value.
    /// Where the scan goes on.
    fn nested_markup(&mut self, at: usize, nested: &mut usize) -> Result<usize, StopReason> {
        Ok(if let Some(width) = self.markup(self.closers, at)? {
            *nested -= 1;
            at + width
        } else if let Some(width) = self.markup(self.openers, at)? {
            *nested += 1;
            at + width
        } else {
            at + 1
        })
    }

    /// Read a call's markup at `at` outside any call the value holds: a
    /// call closer the value holds (read on past it), or the opener of a
    /// call it holds whole. Where the scan goes on, or `None` for no call
    /// markup there.
    fn call_markup(
        &mut self,
        at: usize,
        nested: &mut usize,
        held: &mut Held,
    ) -> Result<Option<usize>, StopReason> {
        if let Some(width) = self.markup(self.closers, at)? {
            if self.paired(at, held)? {
                held.mark(at);
            } else {
                held.closer(at);
            }
            return Ok(Some(at + width));
        }
        if let Some(width) = self.markup(self.openers, at)? {
            held.call(at);
            *nested += 1;
            return Ok(Some(at + width));
        }
        Ok(None)
    }

    /// The closer tag names (`parameter`, `parameters`, and the call
    /// closers' names: `function`, `tool_call`, ...): each spelling of the
    /// form `</NAME>`, in a fixed order.
    fn tags(&self) -> impl Iterator<Item = &[u8]> {
        [CLOSER, CLOSER_VARIANT]
            .into_iter()
            .chain(self.closers.iter().map(Vec::as_slice))
            .filter_map(|spelling| spelling.strip_prefix(b"</")?.strip_suffix(b">"))
    }

    /// The index (into [`Self::tags`]) of the closer tag name an element
    /// opened at `at` carries (`<NAME>`, `<NAME ...>`, `<NAME=...>`), if
    /// any. Charges one `Work` unit per name compared.
    fn tag(&mut self, at: usize) -> Result<Option<usize>, StopReason> {
        let names = 2 + self.closers.len();
        self.charge(names)?;
        let rest = &self.body[at + 1..];
        Ok(self.tags().position(|name| {
            rest.starts_with(name)
                && matches!(
                    rest.get(name.len()),
                    Some(b'>' | b'=' | b' ' | b'\t' | b'\r' | b'\n')
                )
        }))
    }

    /// Record an element opened at `at` in the value, if it carries a
    /// closer tag name.
    fn open_tag(&mut self, at: usize, held: &mut Held) -> Result<(), StopReason> {
        if let Some(index) = self.tag(at)? {
            if held.open.len() <= index {
                held.open.resize(index + 1, 0);
            }
            held.open[index] += 1;
        }
        Ok(())
    }

    /// Whether the closer at `at` closes an element the value opened (it
    /// is paired markup, `<parameter>v</parameter>`, text of the value);
    /// if so, that element is closed.
    fn paired(&mut self, at: usize, held: &mut Held) -> Result<bool, StopReason> {
        let names = 2 + self.closers.len();
        self.charge(names)?;
        let rest = &self.body[at..];
        let Some(index) = self.tags().position(|name| {
            rest.get(2..2 + name.len()) == Some(name) && rest.get(2 + name.len()) == Some(&b'>')
        }) else {
            return Ok(false);
        };
        Ok(match held.open.get_mut(index) {
            Some(open) if *open > 0 => {
                *open -= 1;
                true
            }
            _ => false,
        })
    }

    /// The width of the longest of `spellings` at `at`, charging one `Work`
    /// unit per spelling compared.
    fn markup(&mut self, spellings: &[Vec<u8>], at: usize) -> Result<Option<usize>, StopReason> {
        self.charge(spellings.len())?;
        let rest = &self.body[at..];
        Ok(spellings
            .iter()
            .filter(|spelling| rest.starts_with(spelling))
            .map(Vec::len)
            .max())
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
        if !self.known {
            return Ok(true);
        }
        self.charge(self.frames.len())?;
        Ok(self.frames.iter().any(|frame| frame.scope.declares(name)))
    }

    /// The innermost open scope declaring `name`, or the innermost scope.
    fn owner(&mut self, name: &str) -> Result<usize, StopReason> {
        self.charge(self.frames.len())?;
        Ok(self
            .frames
            .iter()
            .rposition(|frame| frame.scope.declares(name))
            .unwrap_or(self.frames.len() - 1))
    }

    /// Close the innermost nested object at `at`; an item of an array
    /// parameter is adopted as a one-element array (`tool_call.xml.array-item`).
    fn pop(&mut self, at: usize) -> Result<(), StopReason> {
        let frame = self.frames.pop().expect("a nested scope is open");
        let mut tree = frame.tree;
        let end = self.base + at;
        tree.root.span = tree.root.span.map(|span| span.start..end);
        let name = frame.name.expect("nested scopes are named");
        if frame.item {
            let from = tree
                .root
                .span
                .as_ref()
                .map_or(at, |span| span.start - self.base);
            self.record(EditOperator::ArrayItem, from..at, b"")?;
            tree = wrap_item(tree);
        }
        let parent = self
            .frames
            .last_mut()
            .expect("the top-level scope stays open");
        let canonical = parent.scope.canonical(&name.value).to_owned();
        parent
            .tree
            .adopt_keyed(name, &canonical, tree, frame.collect);
        Ok(())
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
            scope: context
                .shape
                .map_or(Scope::Opaque, |shape| Scope::Declared(&shape.arguments)),
            tree: ArgTree::value(Node {
                value: Value::Object(Vec::new()),
                span: Some(span),
            }),
            name: None,
            item: false,
            collect: false,
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
                self.pop(at)?;
            } else {
                self.record(EditOperator::DeleteStrayCloser, at..at + width, b"")?;
            }
            at += width;
        }
        while self.frames.len() > 1 {
            if let Some(operator) = self.closed_at_end() {
                self.record(operator, at..at, CLOSER)?;
            }
            self.pop(at)?;
        }
        Ok(Ok(self.frames.pop().expect("the top-level scope").tree))
    }

    /// The scope the nested parameter elements of a member open, and
    /// whether the object is one item of an array parameter, for a member
    /// declared as `declared` (`None` for an untyped member of a graph
    /// scope) whose scope-graph entry is `member`; `None` when the member
    /// does not nest.
    ///
    /// An object parameter nests as a typed object. An unmodeled parameter
    /// (or an array of unmodeled items) nests as an untyped object whose
    /// names are its node of the scope graph, or opaque with none. A member
    /// of a graph scope nests when the schema location declares it an
    /// object or an array of objects in any branch.
    fn nesting(
        &self,
        declared: Option<&'t ValueShape>,
        member: Nesting,
    ) -> Option<(Scope<'t>, bool)> {
        let node = |index: usize| {
            self.graph
                .and_then(|graph| graph.node(index))
                .map_or(Scope::Opaque, Scope::of_node)
        };
        let unmodeled = |item: bool| {
            let scope = match member {
                Nesting::Object(index) | Nesting::ArrayOfObject(index) => node(index),
                Nesting::None => Scope::Opaque,
            };
            Some((scope, item))
        };
        match declared {
            Some(ValueShape::Object(inner)) => Some((Scope::of_object(inner), false)),
            Some(ValueShape::Unmodeled) => unmodeled(false),
            Some(ValueShape::Array(item)) => match &**item {
                ValueShape::Object(inner) => Some((Scope::of_object(inner), true)),
                ValueShape::Unmodeled => unmodeled(true),
                _ => None,
            },
            Some(_) => None,
            None => match member {
                Nesting::Object(index) => Some((node(index), false)),
                Nesting::ArrayOfObject(index) => Some((node(index), true)),
                Nesting::None => None,
            },
        }
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
            // A scope whose schema names no member has unknown membership:
            // the opener may be its own member, so closing it is a guess.
            let operator = if matches!(self.frames[self.frames.len() - 1].scope, Scope::Opaque) {
                EditOperator::UnmodeledScopeBoundary
            } else {
                EditOperator::MissingParameterClose
            };
            self.record(operator, opener.start..opener.start, CLOSER)?;
            self.pop(opener.start)?;
        }
        let scope = self.frames.last().expect("a scope is open").scope;
        let member = scope.member(&opener.name.value);
        let (declared, collect) = (member.declared, member.collect);
        let from = opener.end;
        // An object parameter's nested elements are its members, typed by
        // its declared shape; an unmodeled parameter's (`$ref`, `anyOf`,
        // ...) are members of an untyped object whose member names come from
        // the tool's scope graph, typed later by the tool's exact schema
        // (`tool_call.value.schema-typed`).
        // The same holds for an array of objects (or of unmodeled items): an
        // item whose body begins with an opener is one nested object, and
        // each repetition of the parameter is one more item.
        let nested = self.nesting(declared, member.nesting);
        if let Some((inner, item)) = nested {
            let next = self.skip_space(from)?;
            if self.body[next..].starts_with(OPENER) {
                // The frame being pushed is one more level of depth, the
                // arguments object being the first.
                self.governor
                    .charge(Resource::Depth, size(self.frames.len() + 1))?;
                self.record(EditOperator::NestedParameter, opener.start..from, b"")?;
                self.frames.push(Frame {
                    scope: inner,
                    tree: ArgTree::value(Node {
                        value: Value::Object(Vec::new()),
                        span: Some(self.absolute(from..from)),
                    }),
                    name: Some(opener.name),
                    item,
                    collect,
                });
                return Ok(Ok(from));
            }
        }
        let first = self.skip_space(from)?;
        let verbatim = verbatim(declared, self.body[first..].starts_with(b"["));
        let (ending, dropped) = self.reading(declared, from, verbatim)?;
        // A member of a graph scope whose schema is an array holds a value
        // that is not a JSON array as one item (the item type stays with the
        // exact schema).
        let Ending {
            stop,
            terminator,
            inner,
            fork,
        } = ending;
        let end = dropped.unwrap_or(stop);
        let tree = if collect && !self.body[first..end].starts_with(b"[") {
            self.array_item(from, end, terminator)?
        } else {
            self.value(declared, from, end, terminator)?
        };
        let tree = match tree {
            Ok(tree) => tree,
            Err(fault) => return Ok(Err(fault)),
        };
        if let Some(inner) = inner {
            self.closer_text(EditOperator::CloserInValue, inner..dropped.unwrap_or(stop))?;
        }
        if let Some(first) = dropped {
            self.record(EditOperator::TextBetweenParameters, first..stop, b"")?;
        } else if let Some(first) = fork {
            self.closer_text(EditOperator::TextAfterCloser, first..stop)?;
        }
        let parent = self.frames.last_mut().expect("a scope is open");
        let canonical = parent.scope.canonical(&opener.name.value).to_owned();
        parent
            .tree
            .adopt_keyed(opener.name, &canonical, tree, collect);
        Ok(Ok(match terminator {
            Terminator::Closer(width, spelling) => {
                self.respelled(stop, width, spelling)?;
                stop + width
            }
            Terminator::Boundary => {
                // Grounded in the definition when the tool is known; with no
                // known tool every opener is a boundary, a guess (the opener
                // may be text of the value).
                let operator = if self.known {
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

    /// Where the value of a parameter declared as `declared` starting at
    /// `from` ends in this pass (see [`Self::terminator`]), and, when this
    /// pass ends a value holding text after an unpaired closer at that
    /// closer, the closer's offset (the bytes from it to the end are
    /// dropped). A value holding such text has two readings that tie: this
    /// pass holds the text ([`Mode::Hold`]) or ends the value at that closer
    /// and drops the text up to the closer that ends the holding reading
    /// ([`Mode::EndFirst`]). When the declared pattern rejects the value
    /// holding closer text, or the value ending at that closer, that is no
    /// reading of the parameter: it ends at its first closer (and text left
    /// after it makes the call malformed), so neither reading is ever
    /// picked alone.
    fn reading(
        &mut self,
        declared: Option<&ValueShape>,
        from: usize,
        verbatim: bool,
    ) -> Result<(Ending, Option<usize>), StopReason> {
        let mut ending = self.terminator(from, verbatim)?;
        if ending.inner.is_none() && ending.fork.is_none() {
            return Ok((ending, None));
        }
        let rejected = !self.admits(declared, from, ending.stop, ending.terminator)?
            || match ending.fork {
                Some(first) => !self.admits(declared, from, first, ending.terminator)?,
                None => false,
            };
        if rejected {
            return Ok((self.terminator(from, false)?, None));
        }
        let Some(first) = ending.fork else {
            return Ok((ending, None));
        };
        if self.mode == Mode::Hold {
            self.forked = true;
            return Ok((ending, None));
        }
        // Ending at that closer, the value holds only the paired closers
        // before it.
        ending.inner = ending.inner.filter(|&inner| inner < first);
        ending.fork = None;
        Ok((ending, Some(first)))
    }

    /// The text `from..stop` as one item of an array member of a graph
    /// scope, as a verbatim string re-rooted under `/0`
    /// (`tool_call.xml.array-item`).
    fn array_item(
        &mut self,
        from: usize,
        stop: usize,
        terminator: Terminator,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        let item = match self.string(from, stop, terminator, false)? {
            Ok(item) => item,
            Err(fault) => return Ok(Err(fault)),
        };
        if item.is_stand_in() {
            return Ok(Ok(item));
        }
        self.record(EditOperator::ArrayItem, from..stop, b"")?;
        Ok(Ok(wrap_item(item)))
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
    fn terminator(&mut self, from: usize, verbatim: bool) -> Result<Ending, StopReason> {
        let mut at = from;
        let mut ends: Vec<Candidate> = Vec::new();
        // Whether a canonical closer came first (the value may have ended).
        let mut between = false;
        // What a reading holding closer text would hold, and the depth of
        // the calls the value holds whole (their markup is theirs).
        let mut held = Held {
            first: None,
            text: None,
            call: None,
            open: Vec::new(),
        };
        let mut nested = 0_usize;
        let mut gap = from;
        let last = loop {
            let Some(offset) = self.body[at..].iter().position(|&byte| byte == b'<') else {
                at = self.body.len();
                if nested == 0 {
                    held.text(self.body, gap..at);
                }
                break Candidate {
                    at,
                    terminator: Terminator::End,
                    owners: Vec::new(),
                    respelled: false,
                    text: false,
                };
            };
            at += offset;
            if verbatim && nested == 0 {
                held.text(self.body, gap..at);
            }
            gap = at;
            if verbatim && nested > 0 {
                at = self.nested_markup(at, &mut nested)?;
                gap = at;
                continue;
            }
            if let Some((width, spelling)) = self.closer(at) {
                if !verbatim && spelling == Spelling::Canonical {
                    self.charge(at - from)?;
                    return Ok(Ending::at(at, Terminator::Closer(width, spelling)));
                }
                between |= spelling == Spelling::Canonical;
                // A misspelled closer held by the value is text of it (no
                // repair reads it as one: T5/F9), and so is one closing an
                // element the value opened; other exact spellings are markup.
                if spelling == Spelling::Misspelled || (verbatim && self.paired(at, &mut held)?) {
                    held.mark(at);
                } else {
                    held.closer(at);
                }
                ends.push(Candidate {
                    at,
                    terminator: Terminator::Closer(width, spelling),
                    owners: Vec::new(),
                    respelled: false,
                    text: false,
                });
                gap = at + width;
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
                if verbatim && candidate.text {
                    self.open_tag(at, &mut held)?;
                }
                if verbatim || !candidate.text {
                    ends.push(candidate);
                }
            } else if verbatim && let Some(to) = self.call_markup(at, &mut nested, &mut held)? {
                at = to;
                gap = at;
                continue;
            } else if verbatim {
                self.open_tag(at, &mut held)?;
            }
            at += 1;
        };
        self.charge(last.at - from)?;
        self.select(&ends, &last, &held, verbatim)
    }

    /// The place a value ends among the places it may (`ends`, then
    /// `last`; see [`Self::terminator`]), given what a reading holding
    /// closer text would hold (`held`).
    fn select(
        &mut self,
        ends: &[Candidate],
        last: &Candidate,
        held: &Held,
        verbatim: bool,
    ) -> Result<Ending, StopReason> {
        if verbatim && let Some(index) = self.cheapest(ends, last, held)? {
            let chosen = ends.get(index).unwrap_or(last);
            let inner = ends[..index]
                .iter()
                .find(|candidate| {
                    matches!(
                        candidate.terminator,
                        Terminator::Closer(_, Spelling::Canonical | Spelling::Variant)
                    )
                })
                .map(|candidate| candidate.at);
            let fork = (held.admits(chosen.at) == Hold::Forked)
                .then_some(held.first)
                .flatten();
            return Ok(Ending {
                stop: chosen.at,
                terminator: chosen.terminator,
                inner,
                fork,
            });
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
            .unwrap_or(last);
        Ok(Ending::at(chosen.at, chosen.terminator))
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
    fn closer_text(
        &mut self,
        operator: EditOperator,
        range: Range<usize>,
    ) -> Result<(), StopReason> {
        let span = self.absolute(range);
        let edit = edit(
            operator,
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
            .filter(|(_, frame)| frame.scope.declares(name))
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
        held: &Held,
    ) -> Result<Option<usize>, StopReason> {
        let nested = self.frames.len() - 1;
        self.charge(ends.len().saturating_mul(nested.max(1)))?;
        let cost = |operator: EditOperator| operator.rule().cost;
        let at_end = self.closed_at_end().map_or(0, cost);
        // Ending a value at an opener rather than at a closer.
        let missing = if self.known {
            cost(EditOperator::MissingParameterClose)
        } else {
            cost(EditOperator::UndeclaredParameterBoundary)
        };
        // What closing each open frame costs at an opener: a scope whose
        // schema names no member is closed as a guess
        // (`tool_call.markup.unmodeled-scope-boundary`), any other at the
        // definition's boundary.
        let unit: Vec<u64> = self
            .frames
            .iter()
            .map(|frame| match frame.scope {
                Scope::Opaque => cost(EditOperator::UnmodeledScopeBoundary),
                _ => missing,
            })
            .collect();
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
            unit[owner + 1..=open]
                .iter()
                .fold(respelled, |total, unit| total.saturating_add(*unit))
        };
        let spelling = |terminator: Terminator| match terminator {
            Terminator::Closer(_, Spelling::Variant) => cost(EditOperator::CloserVariant),
            Terminator::Closer(_, Spelling::Misspelled) => cost(EditOperator::CloserMisspelled),
            _ => 0,
        };
        let mut best: Option<(u64, usize)> = None;
        let mut consider = |reading: Option<u64>, index: usize| {
            let stop = ends.get(index).map_or(last.at, |candidate| candidate.at);
            if let Some(reading) = reading
                && held.admits(stop) != Hold::Never
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
    tree.collected = item
        .collected
        .into_iter()
        .map(|merged| CollectedMember {
            pointer: format!("/0{}", merged.pointer),
            ..merged
        })
        .collect();
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

/// The member names an open object frame accepts.
#[derive(Clone, Copy)]
enum Scope<'t> {
    /// The declared parameters of a typed object (a tool's arguments, or an
    /// object parameter, or an object item).
    Declared(&'t ObjectShape),
    /// The names a schema location declares where the typed shape stops
    /// (`$ref`, `anyOf`, ...): a node of the tool's scope graph.
    Schema(&'t ScopeNode),
    /// A scope whose membership is unknown: it declares no member and may
    /// hold any (a free-form object, an unmodeled value naming none, or no
    /// tool at all). It declares no name, so an opener an enclosing scope
    /// declares ends it, as a guess.
    Opaque,
}

impl<'t> Scope<'t> {
    /// The scope of a nested object: a free-form object (no declared
    /// parameter, unknown members passed through) has unknown membership.
    fn of_object(object: &'t ObjectShape) -> Self {
        if object.parameters.is_empty()
            && object.unknown_parameters == UnknownParameters::PassThrough
        {
            Self::Opaque
        } else {
            Self::Declared(object)
        }
    }

    /// The scope of a graph node.
    const fn of_node(node: &'t ScopeNode) -> Self {
        if node.is_opaque() {
            Self::Opaque
        } else {
            Self::Schema(node)
        }
    }

    /// Whether the scope declares `name` (exactly, or as an alias).
    fn declares(self, name: &str) -> bool {
        match self {
            Self::Declared(object) => object.resolve_name(name).is_some(),
            Self::Schema(node) => node.declares(name),
            Self::Opaque => false,
        }
    }

    /// The canonical name of `name` in the scope: the declared parameter an
    /// alias denotes, else `name` itself. Occurrences of one canonical name
    /// share one item sequence.
    fn canonical<'a>(self, name: &'a str) -> &'a str
    where
        't: 'a,
    {
        match self {
            Self::Declared(object) => object.resolve_name(name).unwrap_or(name),
            Self::Schema(_) | Self::Opaque => name,
        }
    }

    /// The member `name` of the scope (see [`Member`]).
    fn member(self, name: &str) -> Member<'t> {
        match self {
            Self::Declared(object) => {
                let parameter = object
                    .resolve_name(name)
                    .and_then(|canonical| object.parameter(canonical));
                Member {
                    declared: parameter.map(|parameter| &parameter.value),
                    nesting: parameter.map_or(Nesting::None, |parameter| parameter.nesting),
                    collect: false,
                }
            }
            Self::Schema(node) => {
                let member = node.member(name);
                Member {
                    declared: None,
                    nesting: member.map_or(Nesting::None, |member| member.nesting),
                    collect: member.is_some_and(|member| member.array),
                }
            }
            Self::Opaque => Member {
                declared: None,
                nesting: Nesting::None,
                collect: false,
            },
        }
    }
}

/// What a scope says about one member it holds.
struct Member<'t> {
    /// The member's declared shape (a typed scope's parameter); `None` for
    /// an undeclared member and for any member of a graph scope, whose
    /// typing stays with the exact schema.
    declared: Option<&'t ValueShape>,
    /// How the member's nested elements are scoped when `declared` does not
    /// say (an unmodeled parameter, or a member of a graph scope).
    nesting: Nesting,
    /// Whether the member is an array member of a graph scope: a value that
    /// is not a JSON array is one item, and occurrences are collected.
    collect: bool,
}
