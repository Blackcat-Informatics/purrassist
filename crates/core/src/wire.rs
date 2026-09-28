// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Versioned JSON representations shared by command-line and protocol surfaces.
use crate::contract::{
    AssistResult, Budget, Evidence, Inference, InferenceKind, Loss, Outcome, Receipt, StopReason,
};
use crate::edits::{AppliedEdit, EditPlan};
use crate::json::{Member, Node, Number, Value};

/// Construct an ordered JSON object from unique field names supplied by the caller.
#[must_use]
pub fn object(fields: impl IntoIterator<Item = (impl Into<String>, Node)>) -> Node {
    Node::new(Value::Object(
        fields
            .into_iter()
            .map(|(key, value)| Member {
                key: key.into(),
                key_span: None,
                value,
            })
            .collect(),
    ))
}
/// Construct a JSON string.
#[must_use]
pub fn string(value: impl Into<String>) -> Node {
    Node::new(Value::String(value.into()))
}
/// Construct an exact unsigned JSON integer.
#[must_use]
pub fn integer(value: u64) -> Node {
    Node::new(Value::Number(Number::from_unsigned(value)))
}
/// Construct an array without altering its members.
#[must_use]
pub fn array(values: impl IntoIterator<Item = Node>) -> Node {
    Node::new(Value::Array(values.into_iter().collect()))
}
fn evidence(entry: &Evidence) -> Node {
    object([
        ("rule", string(&entry.rule)),
        ("explanation", string(&entry.explanation)),
        ("destination", string(&entry.destination)),
        (
            "span",
            entry.span.as_ref().map_or_else(
                || Node::new(Value::Null),
                |span| {
                    object([
                        ("source", string(span.source.to_string())),
                        (
                            "start",
                            integer(u64::try_from(span.bytes.start).unwrap_or(u64::MAX)),
                        ),
                        (
                            "end",
                            integer(u64::try_from(span.bytes.end).unwrap_or(u64::MAX)),
                        ),
                    ])
                },
            ),
        ),
    ])
}
fn losses(entries: &[Loss]) -> Node {
    array(entries.iter().map(|entry| {
        object([
            ("evidence", evidence(&entry.evidence)),
            ("original", string(&entry.original)),
        ])
    }))
}
fn inferences(entries: &[Inference]) -> Node {
    array(entries.iter().map(|entry| {
        object([
            ("evidence", evidence(&entry.evidence)),
            (
                "kind",
                string(match entry.kind {
                    InferenceKind::Derived => "derived",
                    InferenceKind::Guess => "guess",
                }),
            ),
            ("value", string(&entry.value)),
        ])
    }))
}
fn budget(value: Budget) -> Node {
    object([
        ("work", integer(value.work)),
        ("candidates", integer(value.candidates)),
        ("input_bytes", integer(value.input_bytes)),
        ("scratch_bytes", integer(value.scratch_bytes)),
        ("depth", integer(u64::from(value.depth))),
        ("evidence_entries", integer(value.evidence_entries)),
        ("helper_calls", integer(value.helper_calls)),
    ])
}
impl Receipt {
    /// Serialize the versioned public receipt, including failures and inferred values.
    #[must_use]
    pub fn to_node(&self) -> Node {
        object([
            ("version", integer(u64::from(self.version))),
            ("source", string(self.source.to_string())),
            (
                "sources",
                array(self.sources.iter().map(|source| {
                    object([
                        ("digest", string(source.digest.to_string())),
                        ("text", string(&source.text)),
                    ])
                })),
            ),
            (
                "alternatives",
                array(self.alternatives.iter().map(|entry| {
                    object([
                        ("output", string(entry.output.to_string())),
                        ("cost", integer(entry.cost)),
                        ("evidence", array(entry.evidence.iter().map(evidence))),
                        ("losses", losses(&entry.losses)),
                        ("inferences", inferences(&entry.inferences)),
                        (
                            "validations",
                            array(
                                entry.validations.iter().map(|index| {
                                    integer(u64::try_from(*index).unwrap_or(u64::MAX))
                                }),
                            ),
                        ),
                    ])
                })),
            ),
            ("operation", string(&self.operation)),
            (
                "effort",
                string(format!("{:?}", self.effort).to_ascii_lowercase()),
            ),
            ("budget", budget(self.budget)),
            (
                "usage",
                object([
                    ("work", integer(self.usage.work)),
                    ("candidates", integer(self.usage.candidates)),
                    ("peak_scratch_bytes", integer(self.usage.peak_scratch_bytes)),
                    (
                        "total_scratch_charged",
                        integer(self.usage.total_scratch_charged),
                    ),
                    ("evidence_entries", integer(self.usage.evidence_entries)),
                    ("helper_calls", integer(self.usage.helper_calls)),
                ]),
            ),
            ("evidence", array(self.evidence.iter().map(evidence))),
            ("losses", losses(&self.losses)),
            ("inferences", inferences(&self.inferences)),
            (
                "validations",
                array(self.validations.iter().map(|entry| {
                    object([
                        ("validator", string(&entry.validator)),
                        ("constraint", string(&entry.constraint)),
                        ("passed", Node::new(Value::Bool(entry.passed))),
                        ("diagnostics", array(entry.diagnostics.iter().map(string))),
                    ])
                })),
            ),
            (
                "termination",
                object([
                    (
                        "kind",
                        string(match self.stopped {
                            StopReason::Completed => "completed",
                            StopReason::NoCandidate => "no_candidate",
                            StopReason::BudgetExceeded(_) => "budget_exceeded",
                            StopReason::Cancelled => "cancelled",
                            StopReason::InvalidInput(_) => "invalid_input",
                        }),
                    ),
                    ("detail", string(self.stopped.to_string())),
                ]),
            ),
        ])
    }
}
impl EditPlan {
    /// Serialize this plan's exact source identity and edits. These exact
    /// JSON bytes are the identity hashed by `AlternativeReceipt::output`
    /// wherever an ambiguous edit-planning operation publishes this plan as
    /// one of its alternatives.
    ///
    /// ```
    /// use purrassist_core::{digest::Digest, edits::{EditPlan, TextEdit}};
    /// let plan = EditPlan {
    ///     source: Digest::of(b"hello"),
    ///     edits: vec![TextEdit { range: 0..1, replacement: "H".into() }],
    /// };
    /// assert_eq!(plan.to_node().pointer("/edits/0/replacement").unwrap().as_str(), Some("H"));
    /// ```
    #[must_use]
    pub fn to_node(&self) -> Node {
        object([
            ("source", string(self.source.to_string())),
            (
                "edits",
                array(self.edits.iter().map(|edit| {
                    object([
                        (
                            "start",
                            integer(u64::try_from(edit.range.start).unwrap_or(u64::MAX)),
                        ),
                        (
                            "end",
                            integer(u64::try_from(edit.range.end).unwrap_or(u64::MAX)),
                        ),
                        ("replacement", string(&edit.replacement)),
                    ])
                })),
            ),
        ])
    }
}
impl AppliedEdit {
    /// Serialize the applied text and its conditional inverse plan. These
    /// exact JSON bytes are the identity hashed by `AlternativeReceipt::output`
    /// wherever an ambiguous syntax-checked application publishes this value
    /// as one of its alternatives.
    #[must_use]
    pub fn to_node(&self) -> Node {
        object([
            ("text", string(&self.text)),
            ("inverse", self.inverse.to_node()),
        ])
    }
}
impl AssistResult<Node> {
    /// Encode the answer's status and receipt without hiding guesses or alternatives.
    #[must_use]
    pub fn to_node(&self) -> Node {
        let (status, value, alternatives) = match &self.outcome {
            Outcome::Resolved(value) => ("resolved", value.clone(), array([])),
            Outcome::BestGuess(value) => ("best_guess", value.clone(), array([])),
            Outcome::Ambiguous(values) => {
                ("ambiguous", Node::new(Value::Null), array(values.clone()))
            }
            Outcome::Unresolved => ("unresolved", Node::new(Value::Null), array([])),
        };
        object([
            ("status", string(status)),
            ("value", value),
            ("alternatives", alternatives),
            ("receipt", self.receipt.to_node()),
        ])
    }
}
