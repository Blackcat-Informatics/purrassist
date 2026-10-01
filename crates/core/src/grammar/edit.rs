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
//! | Operator | Rule id | Cost | Guess | Readings | Grounding | Loss |
//! | --- | --- | --- | --- | --- | --- | --- |
//! | [`InsertCloser`](EditOperator::InsertCloser) | `tool_call.markup.missing-closer` | 2 | no | single | syntax | none |
//! | [`DeleteStrayCloser`](EditOperator::DeleteStrayCloser) | `tool_call.markup.stray-closer` | 3 | no | single | syntax | markup |
//! | [`CloserVariant`](EditOperator::CloserVariant) | `tool_call.markup.closer-variant` | 1 | no | single | syntax | markup |
//! | [`TerminalConfusable`](EditOperator::TerminalConfusable) | `tool_call.markup.terminal-confusable` | 1 | no | single | syntax | markup |
//! | [`ConsumedCloser`](EditOperator::ConsumedCloser) | `tool_call.stop.consumed-closer` | 1 | no | single | syntax | none |
//! | [`ReasoningUnclosed`](EditOperator::ReasoningUnclosed) | `tool_call.reasoning.unclosed` | 4 | yes | multiple | syntax | none |
//! | [`ReasoningEmbeddedCall`](EditOperator::ReasoningEmbeddedCall) | `tool_call.reasoning.embedded-call` | 0 | yes | multiple | syntax | none |
//! | [`ReasoningUnterminated`](EditOperator::ReasoningUnterminated) | `tool_call.reasoning.unterminated` | 0 | no | single | syntax | data |
//!
//! ```
//! use purrassist_core::grammar::edit::{EditOperator, LossKind, OPERATORS};
//! let stray = EditOperator::DeleteStrayCloser.rule();
//! assert_eq!(stray.id, "tool_call.markup.stray-closer");
//! assert_eq!(stray.loss, Some(LossKind::Markup));
//! assert!(!stray.guess);
//! assert_eq!(OPERATORS.len(), 8);
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
}

/// Every grammar edit operator, in [`EditOperator`] order.
pub static OPERATORS: &[OperatorRule] = &[
    OperatorRule {
        operator: EditOperator::InsertCloser,
        id: "tool_call.markup.missing-closer",
        cost: cost::INSERT_CLOSER,
        guess: false,
        readings: Readings::Single,
        grounding: Grounding::Syntax,
        loss: None,
    },
    OperatorRule {
        operator: EditOperator::DeleteStrayCloser,
        id: "tool_call.markup.stray-closer",
        cost: cost::DELETE_STRAY_CLOSER,
        guess: false,
        readings: Readings::Single,
        grounding: Grounding::Syntax,
        loss: Some(LossKind::Markup),
    },
    OperatorRule {
        operator: EditOperator::CloserVariant,
        id: "tool_call.markup.closer-variant",
        cost: cost::CLOSER_VARIANT,
        guess: false,
        readings: Readings::Single,
        grounding: Grounding::Syntax,
        loss: Some(LossKind::Markup),
    },
    OperatorRule {
        operator: EditOperator::TerminalConfusable,
        id: "tool_call.markup.terminal-confusable",
        cost: cost::TERMINAL_CONFUSABLE,
        guess: false,
        readings: Readings::Single,
        grounding: Grounding::Syntax,
        loss: Some(LossKind::Markup),
    },
    OperatorRule {
        operator: EditOperator::ConsumedCloser,
        id: "tool_call.stop.consumed-closer",
        cost: cost::CONSUMED_CLOSER,
        guess: false,
        readings: Readings::Single,
        grounding: Grounding::Syntax,
        loss: None,
    },
    OperatorRule {
        operator: EditOperator::ReasoningUnclosed,
        id: "tool_call.reasoning.unclosed",
        cost: cost::REASONING_UNCLOSED,
        guess: true,
        readings: Readings::Multiple,
        grounding: Grounding::Syntax,
        loss: None,
    },
    OperatorRule {
        operator: EditOperator::ReasoningEmbeddedCall,
        id: "tool_call.reasoning.embedded-call",
        cost: 0,
        guess: true,
        readings: Readings::Multiple,
        grounding: Grounding::Syntax,
        loss: None,
    },
    OperatorRule {
        operator: EditOperator::ReasoningUnterminated,
        id: "tool_call.reasoning.unterminated",
        cost: 0,
        guess: false,
        readings: Readings::Single,
        grounding: Grounding::Syntax,
        loss: Some(LossKind::Data),
    },
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
}
