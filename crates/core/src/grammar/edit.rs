// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Weighted edit operators on grammar terminals and the edits they record.
//!
//! Every repair the grammar engine can make is one [`EditOperator`] with a
//! stable rule id, a cost (see [`super::cost`]), a guess flag, whether it
//! admits a single or multiple readings, what grounds it, and the kind of
//! loss it produces. [`OPERATORS`] lists them; the tool-call rule registry
//! imports this table rather than restating it.
//!
//! | Operator | Rule id | Cost | Guess | Readings | Grounding | Loss | Repair |
//! | --- | --- | --- | --- | --- | --- | --- | --- |
//! | [`InsertCloser`](EditOperator::InsertCloser) | `tool_call.markup.missing-closer` | 2 | no | single | syntax | none | yes |
//! | [`DeleteStrayCloser`](EditOperator::DeleteStrayCloser) | `tool_call.markup.stray-closer` | 3 | no | single | syntax | markup | yes |
//! | [`CloserVariant`](EditOperator::CloserVariant) | `tool_call.markup.closer-variant` | 1 | no | single | syntax | markup | yes |
//! | [`TerminalConfusable`](EditOperator::TerminalConfusable) | `tool_call.markup.terminal-confusable` | 1 | no | single | syntax | markup | yes |
//! | [`ConsumedCloser`](EditOperator::ConsumedCloser) | `tool_call.stop.consumed-closer` | 1 | no | single | syntax | none | yes |
//! | [`ReasoningUnclosed`](EditOperator::ReasoningUnclosed) | `tool_call.reasoning.unclosed` | 4 | yes | multiple | syntax | none | no |
//! | [`ReasoningEmbeddedCall`](EditOperator::ReasoningEmbeddedCall) | `tool_call.reasoning.embedded-call` | 0 | yes | multiple | syntax | none | no |
//! | [`ReasoningUnterminated`](EditOperator::ReasoningUnterminated) | `tool_call.reasoning.unterminated` | 0 | no | single | syntax | data | no |
//! | [`MissingParameterClose`](EditOperator::MissingParameterClose) | `tool_call.markup.missing-parameter-close` | 2 | no | single | definition | none | yes |
//! | [`FramingNewline`](EditOperator::FramingNewline) | `tool_call.xml.framing-newline` | 0 | no | single | syntax | markup | no |
//! | [`StringifiedArguments`](EditOperator::StringifiedArguments) | `tool_call.arguments.stringified` | 0 | no | single | syntax | none | no |
//! | [`JsonComment`](EditOperator::JsonComment) | `json.syntax.comment` | 1 | no | single | syntax | markup | no |
//! | [`JsonMissingValue`](EditOperator::JsonMissingValue) | `json.syntax.missing-value` | 4 | yes | multiple | syntax | none | no |
//! | [`JsonLiteralCase`](EditOperator::JsonLiteralCase) | `json.syntax.literal-case` | 1 | no | single | syntax | markup | no |
//! | [`JsonBareString`](EditOperator::JsonBareString) | `json.syntax.bare-string` | 4 | yes | multiple | syntax | none | no |
//! | [`JsonBareKey`](EditOperator::JsonBareKey) | `json.syntax.bare-key` | 1 | no | single | syntax | none | no |
//! | [`JsonString`](EditOperator::JsonString) | `json.syntax.string` | 1 | no | single | syntax | markup | no |
//! | [`JsonUnterminatedString`](EditOperator::JsonUnterminatedString) | `json.syntax.string` | 4 | yes | multiple | syntax | none | no |
//! | [`JsonLiteralEscape`](EditOperator::JsonLiteralEscape) | `json.syntax.literal-escape` | 4 | yes | multiple | syntax | none | no |
//! | [`JsonMissingColon`](EditOperator::JsonMissingColon) | `json.syntax.missing-colon` | 1 | no | single | syntax | none | no |
//! | [`JsonMissingComma`](EditOperator::JsonMissingComma) | `json.syntax.missing-comma` | 1 | no | single | syntax | none | no |
//! | [`JsonTrailingComma`](EditOperator::JsonTrailingComma) | `json.syntax.trailing-comma` | 1 | no | single | syntax | markup | no |
//! | [`JsonCloseContainer`](EditOperator::JsonCloseContainer) | `json.syntax.close-container` | 1 | no | single | syntax | none | no |
//!
//! The first eight operators are the engine's markup repairs. The rest are
//! recorded by argument codecs ([`super::codec`]): the XML-parameter rules,
//! the stringified-arguments decoding, and one operator per tolerant JSON
//! recovery rule of [`crate::json::incremental`] (same rule ids; the
//! unterminated-string case of `json.syntax.string` is a guess and so its own
//! operator). *Repair* marks the operators that change call markup and so
//! make a call [`Closure::Repaired`](crate::tool_call::Closure::Repaired);
//! the others record how argument bytes were read.
//!
//! ```
//! use purrassist_core::grammar::edit::{EditOperator, LossKind, OPERATORS};
//! let stray = EditOperator::DeleteStrayCloser.rule();
//! assert_eq!(stray.id, "tool_call.markup.stray-closer");
//! assert_eq!(stray.loss, Some(LossKind::Markup));
//! assert!(!stray.guess && stray.repair);
//! assert_eq!(OPERATORS.len(), 23);
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
    /// Insert a missing closer terminal.
    InsertCloser,
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
    /// Consume one framing newline on one side of an XML parameter value.
    FramingNewline,
    /// Decode arguments that arrived as a JSON string holding JSON.
    StringifiedArguments,
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

/// A markup repair of the engine.
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

/// Every grammar edit operator, in [`EditOperator`] order.
pub static OPERATORS: &[OperatorRule] = &[
    repair(
        EditOperator::InsertCloser,
        "tool_call.markup.missing-closer",
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
    note(
        EditOperator::FramingNewline,
        "tool_call.xml.framing-newline",
        cost::FRAMING_NEWLINE,
        false,
        Some(LossKind::Markup),
    ),
    note(
        EditOperator::StringifiedArguments,
        "tool_call.arguments.stringified",
        cost::STRINGIFIED_ARGUMENTS,
        false,
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
        "json.syntax.string",
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
];

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
    /// unterminated string from a requoted one. `None` for any other id.
    ///
    /// ```
    /// use purrassist_core::grammar::edit::EditOperator;
    /// assert_eq!(EditOperator::json("json.syntax.string", true), Some(EditOperator::JsonUnterminatedString));
    /// assert_eq!(EditOperator::json("json.syntax.string", false), Some(EditOperator::JsonString));
    /// assert_eq!(EditOperator::json("tool_call.markup.missing-closer", false), None);
    /// ```
    #[must_use]
    pub fn json(rule: &str, guessed: bool) -> Option<Self> {
        OPERATORS[Self::JsonComment as usize..]
            .iter()
            .find(|entry| {
                entry.id == rule && (entry.guess == guessed || rule != "json.syntax.string")
            })
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
///     operator: EditOperator::InsertCloser,
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
            assert_eq!(guessed.rule().id, rule);
        }
    }
}
