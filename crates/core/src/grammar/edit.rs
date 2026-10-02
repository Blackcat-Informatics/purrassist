// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Weighted edit operators on grammar terminals and the edits they record.
//!
//! Every repair the grammar engine can make is one [`EditOperator`] with a
//! stable rule id, a cost (see [`super::cost`]), a guess flag, whether it
//! admits a single or multiple readings, what grounds it, and the kind of
//! loss it produces. [`OPERATORS`] lists them; the tool-call rule registry
//! ([`crate::tool_call::RULES`]) imports this table rather than restating it,
//! and every rule id appears in exactly one row.
//!
//! | Operator | Rule id | Cost | Guess | Readings | Grounding | Loss | Repair |
//! | --- | --- | --- | --- | --- | --- | --- | --- |
//! | [`MissingFunctionClose`](EditOperator::MissingFunctionClose) | `tool_call.markup.missing-function-close` | 2 | no | single | syntax | none | yes |
//! | [`MissingWrapperClose`](EditOperator::MissingWrapperClose) | `tool_call.markup.missing-wrapper-close` | 2 | no | single | syntax | none | yes |
//! | [`MissingClosersAtEnd`](EditOperator::MissingClosersAtEnd) | `tool_call.markup.missing-closers-at-end` | 2 | no | single | syntax | none | yes |
//! | [`DeleteStrayCloser`](EditOperator::DeleteStrayCloser) | `tool_call.markup.stray-closer` | 3 | no | single | syntax | markup | yes |
//! | [`CloserVariant`](EditOperator::CloserVariant) | `tool_call.markup.closer-variant` | 1 | no | single | syntax | markup | yes |
//! | [`TerminalConfusable`](EditOperator::TerminalConfusable) | `tool_call.markup.terminal-confusable` | 1 | no | single | syntax | markup | yes |
//! | [`ConsumedCloser`](EditOperator::ConsumedCloser) | `tool_call.stop.consumed-closer` | 1 | no | single | syntax | none | yes |
//! | [`ReasoningUnclosed`](EditOperator::ReasoningUnclosed) | `tool_call.reasoning.unclosed` | 4 | yes | multiple | syntax | none | no |
//! | [`ReasoningEmbeddedCall`](EditOperator::ReasoningEmbeddedCall) | `tool_call.reasoning.embedded-call` | 0 | yes | multiple | syntax | none | no |
//! | [`ReasoningUnterminated`](EditOperator::ReasoningUnterminated) | `tool_call.reasoning.unterminated` | 0 | no | single | syntax | data | no |
//! | [`MissingParameterClose`](EditOperator::MissingParameterClose) | `tool_call.markup.missing-parameter-close` | 2 | no | single | definition | none | yes |
//! | [`ParameterClosedByElement`](EditOperator::ParameterClosedByElement) | `tool_call.markup.parameter-closed-by-element` | 2 | no | single | syntax | none | yes |
//! | [`UndeclaredParameterBoundary`](EditOperator::UndeclaredParameterBoundary) | `tool_call.markup.undeclared-parameter-boundary` | 2 | yes | multiple | syntax | none | yes |
//! | [`ClosersOnlyEmpty`](EditOperator::ClosersOnlyEmpty) | `tool_call.markup.closers-only-empty` | 3 | no | single | definition | markup | yes |
//! | [`NestedParameter`](EditOperator::NestedParameter) | `tool_call.markup.nested-parameter` | 0 | no | single | definition | none | no |
//! | [`JsonBody`](EditOperator::JsonBody) | `tool_call.markup.json-body` | 0 | no | single | syntax | none | no |
//! | [`ArrayItem`](EditOperator::ArrayItem) | `tool_call.xml.array-item` | 0 | no | single | definition | none | no |
//! | [`TrimNonverbatim`](EditOperator::TrimNonverbatim) | `tool_call.value.trim-nonverbatim` | 0 | no | single | definition | none | no |
//! | [`FramingNewline`](EditOperator::FramingNewline) | `tool_call.xml.framing-newline` | 0 | no | single | syntax | markup | no |
//! | [`StringifiedArguments`](EditOperator::StringifiedArguments) | `tool_call.arguments.decode-string` | 0 | no | single | syntax | none | no |
//! | [`ExtractProse`](EditOperator::ExtractProse) | `tool_call.extract.prose` | 2 | no | single | syntax | data | yes |
//! | [`ExtractFence`](EditOperator::ExtractFence) | `tool_call.extract.fence` | 1 | no | single | syntax | markup | yes |
//! | [`ContextFencedExample`](EditOperator::ContextFencedExample) | `tool_call.context.fenced-example` | 0 | yes | multiple | syntax | none | no |
//! | [`JsonComment`](EditOperator::JsonComment) | `json.syntax.comment` | 1 | no | single | syntax | markup | no |
//! | [`JsonMissingValue`](EditOperator::JsonMissingValue) | `json.syntax.missing-value` | 4 | yes | multiple | syntax | none | no |
//! | [`JsonLiteralCase`](EditOperator::JsonLiteralCase) | `json.syntax.literal-case` | 1 | no | single | syntax | markup | no |
//! | [`JsonBareString`](EditOperator::JsonBareString) | `json.syntax.bare-string` | 4 | yes | multiple | syntax | none | no |
//! | [`JsonBareKey`](EditOperator::JsonBareKey) | `json.syntax.bare-key` | 1 | no | single | syntax | none | no |
//! | [`JsonString`](EditOperator::JsonString) | `json.syntax.string` | 1 | no | single | syntax | markup | no |
//! | [`JsonUnterminatedString`](EditOperator::JsonUnterminatedString) | `json.syntax.unterminated-string` | 4 | yes | multiple | syntax | none | no |
//! | [`JsonLiteralEscape`](EditOperator::JsonLiteralEscape) | `json.syntax.literal-escape` | 4 | yes | multiple | syntax | none | no |
//! | [`JsonMissingColon`](EditOperator::JsonMissingColon) | `json.syntax.missing-colon` | 1 | no | single | syntax | none | no |
//! | [`JsonMissingComma`](EditOperator::JsonMissingComma) | `json.syntax.missing-comma` | 1 | no | single | syntax | none | no |
//! | [`JsonTrailingComma`](EditOperator::JsonTrailingComma) | `json.syntax.trailing-comma` | 1 | no | single | syntax | markup | no |
//! | [`JsonCloseContainer`](EditOperator::JsonCloseContainer) | `json.syntax.close-container` | 1 | no | single | syntax | none | no |
//! | [`MissingWrapperCloseBeforeText`](EditOperator::MissingWrapperCloseBeforeText) | `tool_call.markup.missing-wrapper-close.before-text` | 2 | yes | multiple | syntax | none | yes |
//! | [`MissingToken`](EditOperator::MissingToken) | `tool_call.markup.missing-token` | 2 | no | single | syntax | none | yes |
//!
//! The first seven operators are the engine's markup repairs: a missing
//! closer inserted mid-message is a *function* close when more of the
//! production follows it and a *wrapper* close when it ends the production
//! (both symmetric with a missing `</parameter>`). A wrapper close is the
//! only reading when what follows the call (after whitespace and absorbed
//! stray closers) is another call's opener or the end of output; when text
//! follows and no closer of the grammar comes before the next opener or the
//! end of output, the text may be content after the call or text inside an
//! unterminated wrapper, so the insertion is the guess
//! `tool_call.markup.missing-wrapper-close.before-text` (the last row: it is
//! appended so the other operators keep their positions); when a closer does
//! come, the text is inside the wrapper and the attempt is rejected.
//! `tool_call.markup.missing-token` (appended after it) records a token a
//! grammar names with `insert("...")` as read where the model omitted it. Closers inserted at the
//! end of output under a non-length stop are
//! `tool_call.markup.missing-closers-at-end`, usable only when the call is
//! otherwise complete (D10, enforced by tool-call resolution). The reasoning
//! operators are recorded by the turn layer, the `extract` operators for a
//! call's `dropped:` captures (a fence marker is markup; any other dropped
//! text inside a call is data), and `tool_call.context.fenced-example` for a
//! call recognized inside fenced code, an inline code span or a block quote.
//! The rest are recorded by argument codecs ([`super::codec`]): the
//! `xml-params` rules, the stringified-arguments decoding, and one operator
//! per tolerant JSON recovery rule of [`crate::json::incremental`] (same rule
//! ids, except that an *unterminated* string, which the JSON engine records
//! as a guessed `json.syntax.string`, is its own rule
//! `json.syntax.unterminated-string`: it has its own cost and guess flag, and
//! one id names one rule). *Repair* marks the operators that change call
//! markup and so make a call
//! [`Closure::Repaired`](crate::tool_call::Closure::Repaired); the others
//! record how bytes were read.
//!
//! ```
//! use purrassist_core::grammar::edit::{EditOperator, LossKind, OPERATORS};
//! let stray = EditOperator::DeleteStrayCloser.rule();
//! assert_eq!(stray.id, "tool_call.markup.stray-closer");
//! assert_eq!(stray.loss, Some(LossKind::Markup));
//! assert!(!stray.guess && stray.repair);
//! assert_eq!(OPERATORS.len(), 37);
//! let framing = EditOperator::FramingNewline.rule();
//! assert_eq!((framing.loss, framing.repair), (Some(LossKind::Markup), false));
//! ```
use super::cost;
use core::ops::Range;

/// Whether a rule admits exactly one reading of its input or several.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Readings {
    /// The repair is the only reading of the markup it changes.
    Single,
    /// Other readings of the same markup remain possible.
    Multiple,
}

/// What a rule's decision rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Grounding {
    /// A tool definition (parameter names, types, requiredness).
    Definition,
    /// The dialect grammar alone.
    Syntax,
}

/// The kind of information a loss discards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LossKind {
    /// Syntax bytes that carry no argument data (closers, framing).
    Markup,
    /// Bytes or structure that may carry data.
    Data,
}

/// One weighted edit operator of the grammar engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EditOperator {
    /// Insert a missing closer mid-message that more of the production
    /// follows (a function or parameter element's closer).
    MissingFunctionClose,
    /// Insert a missing closer mid-message that ends the production (the
    /// call's outermost wrapper closer) where, after whitespace and absorbed
    /// stray closers, the opener of another call of the same production or
    /// the end of output follows: the only reading. Before other text it is
    /// [`Self::MissingWrapperCloseBeforeText`] instead.
    MissingWrapperClose,
    /// Insert a missing closer at the end of output under a non-length stop.
    MissingClosersAtEnd,
    /// Delete a closer spelling the grammar does not expect here.
    DeleteStrayCloser,
    /// Accept a declared variant spelling of the expected closer.
    CloserVariant,
    /// Accept a confusable spelling of an expected literal.
    TerminalConfusable,
    /// Reinsert the expected closer the generation stop sequence consumed.
    ConsumedCloser,
    /// Close an unclosed reasoning block at the first call opener.
    ReasoningUnclosed,
    /// Offer a call found inside a closed reasoning block as an alternative.
    ReasoningEmbeddedCall,
    /// Keep an unterminated reasoning block as reasoning to end of output.
    ReasoningUnterminated,
    /// End an open XML parameter value at the opener of a parameter the
    /// call's tool declares (the value's `</parameter>` is missing).
    MissingParameterClose,
    /// End an open XML parameter value (or nested parameter object) where
    /// its enclosing element closes (the `</parameter>` is missing).
    ParameterClosedByElement,
    /// End an open XML parameter value at a `<parameter=` opener of a call
    /// that names no declared tool: the opener may also be text of the
    /// value, so the boundary is a guess.
    UndeclaredParameterBoundary,
    /// Read a function body made only of closers and whitespace as no
    /// arguments (`{}`); usable only when the tool requires no parameter.
    ClosersOnlyEmpty,
    /// Read nested `<parameter=` elements as an object parameter's members.
    NestedParameter,
    /// Read a whole JSON object placed directly in a function body as the
    /// arguments.
    JsonBody,
    /// Read an array parameter's XML value that is not a JSON array as one
    /// item of the array.
    ArrayItem,
    /// Trim whitespace around a value of a non-verbatim declared type
    /// (integer, number, boolean, enumeration) beyond the framing newlines.
    TrimNonverbatim,
    /// Consume one framing newline on one side of an XML parameter value.
    FramingNewline,
    /// Decode arguments that arrived as a JSON string holding JSON.
    StringifiedArguments,
    /// Drop prose inside a call's markup (a `dropped:` capture).
    ExtractProse,
    /// Drop a code-fence marker inside a call's markup (a `dropped:`
    /// capture that is only a fence marker).
    ExtractFence,
    /// A call recognized inside fenced code, an inline code span or a block
    /// quote: it may be an example rather than a call.
    ContextFencedExample,
    /// JSON recovery: drop a comment.
    JsonComment,
    /// JSON recovery: insert `null` where a value is missing.
    JsonMissingValue,
    /// JSON recovery: respell a literal in canonical case.
    JsonLiteralCase,
    /// JSON recovery: quote a bare token as a string.
    JsonBareString,
    /// JSON recovery: quote a bare object key.
    JsonBareKey,
    /// JSON recovery: requote a non-canonically quoted string.
    JsonString,
    /// JSON recovery: close an unterminated string.
    JsonUnterminatedString,
    /// JSON recovery: keep an invalid escape as literal text.
    JsonLiteralEscape,
    /// JSON recovery: insert a missing `:`.
    JsonMissingColon,
    /// JSON recovery: insert a missing `,`.
    JsonMissingComma,
    /// JSON recovery: drop a trailing comma.
    JsonTrailingComma,
    /// JSON recovery: close a container left open at the end of its value.
    JsonCloseContainer,
    /// Insert the call's missing wrapper closer mid-message before text that
    /// no closer of the grammar follows up to the next opener of the same
    /// production or the end of output. The text may be content after the
    /// call or text inside a wrapper that never closes, so this is a guess
    /// admitting another reading. (Had a closer followed, the text would be
    /// inside the wrapper and the attempt is rejected instead.)
    MissingWrapperCloseBeforeText,
    /// Read a token the grammar's `insert("...")` names as omitted by the
    /// model and inserted here (the brackets of a Mistral v3 call list
    /// written without them).
    MissingToken,
}

/// Static metadata of one [`EditOperator`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperatorRule {
    /// The operator.
    pub operator: EditOperator,
    /// Stable rule identifier recorded as evidence.
    pub id: &'static str,
    /// Search cost added per application.
    pub cost: u64,
    /// Whether the edit invents content (recorded as a guess inference).
    pub guess: bool,
    /// Whether other readings of the same markup remain possible.
    pub readings: Readings,
    /// What the decision rests on.
    pub grounding: Grounding,
    /// The kind of loss recorded, if the edit discards original bytes.
    pub loss: Option<LossKind>,
    /// Whether the edit repairs call markup (and so makes a call
    /// [`Closure::Repaired`](crate::tool_call::Closure::Repaired)) rather than
    /// recording how argument bytes were read.
    pub repair: bool,
}

/// An operator rule from its table columns (`repair` is set by the caller).
const fn rule(
    operator: EditOperator,
    id: &'static str,
    cost: u64,
    guess: bool,
    grounding: Grounding,
    loss: Option<LossKind>,
) -> OperatorRule {
    OperatorRule {
        operator,
        id,
        cost,
        guess,
        readings: if guess {
            Readings::Multiple
        } else {
            Readings::Single
        },
        grounding,
        loss,
        repair: false,
    }
}

/// A syntax-grounded operator that records a reading rather than repairing
/// markup.
const fn note(
    operator: EditOperator,
    id: &'static str,
    cost: u64,
    guess: bool,
    loss: Option<LossKind>,
) -> OperatorRule {
    rule(operator, id, cost, guess, Grounding::Syntax, loss)
}

/// A definition-grounded operator that records a reading rather than
/// repairing markup.
const fn reading(operator: EditOperator, id: &'static str) -> OperatorRule {
    rule(operator, id, 0, false, Grounding::Definition, None)
}

/// A markup repair.
const fn repair(
    operator: EditOperator,
    id: &'static str,
    cost: u64,
    grounding: Grounding,
    loss: Option<LossKind>,
) -> OperatorRule {
    OperatorRule {
        repair: true,
        ..rule(operator, id, cost, false, grounding, loss)
    }
}

/// A guessed markup repair.
const fn guessed_repair(operator: EditOperator, id: &'static str, cost: u64) -> OperatorRule {
    OperatorRule {
        repair: true,
        ..rule(operator, id, cost, true, Grounding::Syntax, None)
    }
}

/// The number of [`EditOperator`]s.
pub const OPERATOR_COUNT: usize = 37;

/// Every grammar edit operator, in [`EditOperator`] order, as a constant
/// table (the tool-call rule registry is built from it at compile time).
pub(crate) const OPERATOR_TABLE: [OperatorRule; OPERATOR_COUNT] = [
    repair(
        EditOperator::MissingFunctionClose,
        "tool_call.markup.missing-function-close",
        cost::INSERT_CLOSER,
        Grounding::Syntax,
        None,
    ),
    repair(
        EditOperator::MissingWrapperClose,
        "tool_call.markup.missing-wrapper-close",
        cost::INSERT_CLOSER,
        Grounding::Syntax,
        None,
    ),
    repair(
        EditOperator::MissingClosersAtEnd,
        "tool_call.markup.missing-closers-at-end",
        cost::INSERT_CLOSER,
        Grounding::Syntax,
        None,
    ),
    repair(
        EditOperator::DeleteStrayCloser,
        "tool_call.markup.stray-closer",
        cost::DELETE_STRAY_CLOSER,
        Grounding::Syntax,
        Some(LossKind::Markup),
    ),
    repair(
        EditOperator::CloserVariant,
        "tool_call.markup.closer-variant",
        cost::CLOSER_VARIANT,
        Grounding::Syntax,
        Some(LossKind::Markup),
    ),
    repair(
        EditOperator::TerminalConfusable,
        "tool_call.markup.terminal-confusable",
        cost::TERMINAL_CONFUSABLE,
        Grounding::Syntax,
        Some(LossKind::Markup),
    ),
    repair(
        EditOperator::ConsumedCloser,
        "tool_call.stop.consumed-closer",
        cost::CONSUMED_CLOSER,
        Grounding::Syntax,
        None,
    ),
    note(
        EditOperator::ReasoningUnclosed,
        "tool_call.reasoning.unclosed",
        cost::REASONING_UNCLOSED,
        true,
        None,
    ),
    note(
        EditOperator::ReasoningEmbeddedCall,
        "tool_call.reasoning.embedded-call",
        0,
        true,
        None,
    ),
    note(
        EditOperator::ReasoningUnterminated,
        "tool_call.reasoning.unterminated",
        0,
        false,
        Some(LossKind::Data),
    ),
    repair(
        EditOperator::MissingParameterClose,
        "tool_call.markup.missing-parameter-close",
        cost::MISSING_PARAMETER_CLOSE,
        Grounding::Definition,
        None,
    ),
    repair(
        EditOperator::ParameterClosedByElement,
        "tool_call.markup.parameter-closed-by-element",
        cost::INSERT_CLOSER,
        Grounding::Syntax,
        None,
    ),
    guessed_repair(
        EditOperator::UndeclaredParameterBoundary,
        "tool_call.markup.undeclared-parameter-boundary",
        cost::INSERT_CLOSER,
    ),
    repair(
        EditOperator::ClosersOnlyEmpty,
        "tool_call.markup.closers-only-empty",
        cost::DELETE_STRAY_CLOSER,
        Grounding::Definition,
        Some(LossKind::Markup),
    ),
    reading(
        EditOperator::NestedParameter,
        "tool_call.markup.nested-parameter",
    ),
    note(
        EditOperator::JsonBody,
        "tool_call.markup.json-body",
        0,
        false,
        None,
    ),
    reading(EditOperator::ArrayItem, "tool_call.xml.array-item"),
    reading(
        EditOperator::TrimNonverbatim,
        "tool_call.value.trim-nonverbatim",
    ),
    note(
        EditOperator::FramingNewline,
        "tool_call.xml.framing-newline",
        cost::FRAMING_NEWLINE,
        false,
        Some(LossKind::Markup),
    ),
    note(
        EditOperator::StringifiedArguments,
        "tool_call.arguments.decode-string",
        cost::STRINGIFIED_ARGUMENTS,
        false,
        None,
    ),
    repair(
        EditOperator::ExtractProse,
        "tool_call.extract.prose",
        cost::EXTRACT_PROSE,
        Grounding::Syntax,
        Some(LossKind::Data),
    ),
    repair(
        EditOperator::ExtractFence,
        "tool_call.extract.fence",
        cost::EXTRACT_FENCE,
        Grounding::Syntax,
        Some(LossKind::Markup),
    ),
    note(
        EditOperator::ContextFencedExample,
        "tool_call.context.fenced-example",
        0,
        true,
        None,
    ),
    note(
        EditOperator::JsonComment,
        "json.syntax.comment",
        cost::JSON_REPAIR,
        false,
        Some(LossKind::Markup),
    ),
    note(
        EditOperator::JsonMissingValue,
        "json.syntax.missing-value",
        cost::JSON_GUESS,
        true,
        None,
    ),
    note(
        EditOperator::JsonLiteralCase,
        "json.syntax.literal-case",
        cost::JSON_REPAIR,
        false,
        Some(LossKind::Markup),
    ),
    note(
        EditOperator::JsonBareString,
        "json.syntax.bare-string",
        cost::JSON_GUESS,
        true,
        None,
    ),
    note(
        EditOperator::JsonBareKey,
        "json.syntax.bare-key",
        cost::JSON_REPAIR,
        false,
        None,
    ),
    note(
        EditOperator::JsonString,
        "json.syntax.string",
        cost::JSON_REPAIR,
        false,
        Some(LossKind::Markup),
    ),
    note(
        EditOperator::JsonUnterminatedString,
        "json.syntax.unterminated-string",
        cost::JSON_GUESS,
        true,
        None,
    ),
    note(
        EditOperator::JsonLiteralEscape,
        "json.syntax.literal-escape",
        cost::JSON_GUESS,
        true,
        None,
    ),
    note(
        EditOperator::JsonMissingColon,
        "json.syntax.missing-colon",
        cost::JSON_REPAIR,
        false,
        None,
    ),
    note(
        EditOperator::JsonMissingComma,
        "json.syntax.missing-comma",
        cost::JSON_REPAIR,
        false,
        None,
    ),
    note(
        EditOperator::JsonTrailingComma,
        "json.syntax.trailing-comma",
        cost::JSON_REPAIR,
        false,
        Some(LossKind::Markup),
    ),
    note(
        EditOperator::JsonCloseContainer,
        "json.syntax.close-container",
        cost::JSON_REPAIR,
        false,
        None,
    ),
    guessed_repair(
        EditOperator::MissingWrapperCloseBeforeText,
        "tool_call.markup.missing-wrapper-close.before-text",
        cost::INSERT_CLOSER,
    ),
    repair(
        EditOperator::MissingToken,
        "tool_call.markup.missing-token",
        cost::INSERT_CLOSER,
        Grounding::Syntax,
        None,
    ),
];

/// Every grammar edit operator, in [`EditOperator`] order.
pub static OPERATORS: &[OperatorRule] = &OPERATOR_TABLE;

impl EditOperator {
    /// This operator's static metadata.
    ///
    /// ```
    /// use purrassist_core::grammar::edit::EditOperator;
    /// assert_eq!(EditOperator::CloserVariant.rule().id, "tool_call.markup.closer-variant");
    /// ```
    #[must_use]
    pub fn rule(self) -> &'static OperatorRule {
        &OPERATORS[self as usize]
    }

    /// The operator recording a tolerant JSON recovery edit with rule id
    /// `rule` (see [`crate::json::incremental`]); `guessed` separates an
    /// unterminated string (the guessed `json.syntax.string`, its own rule
    /// [`EditOperator::JsonUnterminatedString`]) from a requoted one. `None`
    /// for any other id.
    ///
    /// ```
    /// use purrassist_core::grammar::edit::EditOperator;
    /// assert_eq!(EditOperator::json("json.syntax.string", true), Some(EditOperator::JsonUnterminatedString));
    /// assert_eq!(EditOperator::json("json.syntax.string", false), Some(EditOperator::JsonString));
    /// assert_eq!(EditOperator::json("tool_call.markup.stray-closer", false), None);
    /// ```
    #[must_use]
    pub fn json(rule: &str, guessed: bool) -> Option<Self> {
        if rule == "json.syntax.string" && guessed {
            return Some(Self::JsonUnterminatedString);
        }
        OPERATORS[Self::JsonComment as usize..=Self::JsonCloseContainer as usize]
            .iter()
            .find(|entry| entry.id == rule && entry.operator != Self::JsonUnterminatedString)
            .map(|entry| entry.operator)
    }
}

/// One edit the engine applied to recognize a segment.
///
/// `span` is the original byte range the edit affects (empty for an
/// insertion), `original` its exact bytes, and `replacement` the canonical
/// bytes the grammar expected there.
///
/// ```
/// use purrassist_core::grammar::edit::{EditOperator, MarkupEdit};
/// let edit = MarkupEdit {
///     operator: EditOperator::MissingFunctionClose,
///     span: 7..7,
///     original: Vec::new(),
///     replacement: b"</arg>".to_vec(),
/// };
/// assert_eq!(edit.rule().cost, 2);
/// assert!(edit.span.is_empty());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkupEdit {
    /// The operator applied.
    pub operator: EditOperator,
    /// Original byte range affected.
    pub span: Range<usize>,
    /// The original bytes in `span`.
    pub original: Vec<u8>,
    /// The canonical bytes the grammar expected.
    pub replacement: Vec<u8>,
}

impl MarkupEdit {
    /// The static metadata of this edit's operator.
    ///
    /// See [`MarkupEdit`] for an example.
    #[must_use]
    pub fn rule(&self) -> &'static OperatorRule {
        self.operator.rule()
    }

    /// The portable scratch charge of retaining this edit: its byte
    /// payloads plus a fixed record overhead.
    pub(crate) fn estimated_bytes(&self) -> u64 {
        crate::contract::saturating_size(self.original.len() + self.replacement.len())
            .saturating_add(96)
    }
}

#[cfg(test)]
mod tests {
    use super::{EditOperator, OPERATORS};

    #[test]
    fn table_is_indexed_by_operator() {
        for (index, rule) in OPERATORS.iter().enumerate() {
            assert_eq!(rule.operator as usize, index);
            assert_eq!(rule.operator.rule(), rule);
        }
        assert_eq!(
            EditOperator::ReasoningUnterminated.rule().id,
            "tool_call.reasoning.unterminated"
        );
    }

    #[test]
    fn every_json_recovery_rule_has_an_operator() {
        let mut ids: Vec<&str> = OPERATORS.iter().map(|rule| rule.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), OPERATORS.len(), "one id per rule");
        for rule in [
            "json.syntax.comment",
            "json.syntax.missing-value",
            "json.syntax.literal-case",
            "json.syntax.bare-string",
            "json.syntax.bare-key",
            "json.syntax.string",
            "json.syntax.literal-escape",
            "json.syntax.missing-colon",
            "json.syntax.missing-comma",
            "json.syntax.trailing-comma",
            "json.syntax.close-container",
        ] {
            let operator = EditOperator::json(rule, false).expect("mapped");
            assert_eq!(operator.rule().id, rule);
            assert!(!operator.rule().repair);
            let guessed = EditOperator::json(rule, true).expect("mapped");
            if rule == "json.syntax.string" {
                assert_eq!(guessed.rule().id, "json.syntax.unterminated-string");
                assert!(guessed.rule().guess);
            } else {
                assert_eq!(guessed.rule().id, rule);
            }
        }
    }
}
