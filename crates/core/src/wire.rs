// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Versioned JSON representations shared by command-line and protocol surfaces.
use crate::contract::saturating_size as size;
use crate::contract::{
    AlternativeReceipt, AssistResult, Budget, Evidence, Inference, InferenceKind, Loss, Outcome,
    Receipt, SourceDocument, StopReason, Validation,
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
/// Serialize one evidence entry using the receipt's public wire representation.
///
/// A present span retains its source digest and half-open byte offsets; an absent
/// span becomes JSON `null`. Offsets exceeding `u64` saturate to `u64::MAX`, as
/// in receipt serialization. This function returns no validation errors: it
/// encodes the supplied evidence without checking the referenced source or
/// changing its rule, explanation, destination, or span.
///
/// ```
/// use purrassist_core::{contract::Evidence, wire};
/// let entry = Evidence {
///     rule: "caller.literal.v1".into(),
///     explanation: "The caller supplied the value verbatim.".into(),
///     destination: "/answer".into(),
///     span: None,
/// };
/// let encoded = wire::evidence(&entry);
/// assert_eq!(encoded.get("rule").and_then(|node| node.as_str()), Some("caller.literal.v1"));
/// assert_eq!(encoded.get("span").unwrap().to_json(), "null");
/// ```
#[must_use]
pub fn evidence(entry: &Evidence) -> Node {
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
pub(crate) fn losses(entries: &[Loss]) -> Node {
    array(entries.iter().map(|entry| {
        object([
            ("evidence", evidence(&entry.evidence)),
            ("original", string(&entry.original)),
        ])
    }))
}
pub(crate) fn inferences(entries: &[Inference]) -> Node {
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
/// Kinds/rules of the provenance nodes distinguishing two ranked-adjacent
/// alternatives (see [`ambiguity`]), over the already-published per-alternative
/// ledgers: since [`crate::provenance::Forest::publish`] promotes exactly the
/// nodes reachable from *every* derivation to the receipt's top-level ledgers,
/// the symmetric difference of two alternatives' own (non-promoted) ledgers is
/// exactly [`crate::provenance::Forest::distinguishing`] restricted to the
/// non-validation nodes; validation membership is compared by index directly.
fn distinguishing(
    a: &AlternativeReceipt,
    b: &AlternativeReceipt,
    validations: &[Validation],
) -> Node {
    let mut nodes: Vec<(&'static str, String)> = Vec::new();
    for (mine, other) in [(&a.evidence, &b.evidence), (&b.evidence, &a.evidence)] {
        for entry in mine {
            if !other.contains(entry) {
                nodes.push(("evidence", entry.rule.clone()));
            }
        }
    }
    for (mine, other) in [(&a.losses, &b.losses), (&b.losses, &a.losses)] {
        for entry in mine {
            if !other.contains(entry) {
                nodes.push(("loss", entry.evidence.rule.clone()));
            }
        }
    }
    for (mine, other) in [
        (&a.inferences, &b.inferences),
        (&b.inferences, &a.inferences),
    ] {
        for entry in mine {
            if !other.contains(entry) {
                nodes.push(("inference", entry.evidence.rule.clone()));
            }
        }
    }
    for (mine, other) in [
        (&a.validations, &b.validations),
        (&b.validations, &a.validations),
    ] {
        for &index in mine {
            if !other.contains(&index) {
                let rule = validations
                    .get(index)
                    .map_or_else(|| format!("validation:{index}"), |v| v.validator.clone());
                nodes.push(("validation", rule));
            }
        }
    }
    array(
        nodes
            .into_iter()
            .map(|(kind, rule)| object([("kind", string(kind)), ("rule", string(rule))])),
    )
}
/// The wire `ambiguity` annotation for a receipt with two or more published
/// alternatives: `derivations` is the competing-answer count, `valid` counts
/// alternatives whose [`AlternativeReceipt::valid`] annotation held, and
/// `distinguishing` gives, for each ranked-adjacent pair (`alternatives` is
/// already in ascending [`crate::provenance::Forest::rank`] order; see
/// [`crate::provenance::Published::order`]), the evidence/loss/inference/
/// validation nodes reachable from exactly one of the pair — "why is this
/// ambiguous", following [`crate::provenance::Forest::distinguishing`].
/// `null` when fewer than two alternatives were published.
fn ambiguity(alternatives: &[AlternativeReceipt], validations: &[Validation]) -> Node {
    if alternatives.len() < 2 {
        return Node::new(Value::Null);
    }
    let valid = alternatives.iter().filter(|entry| entry.valid).count();
    object([
        ("derivations", integer(size(alternatives.len()))),
        ("valid", integer(size(valid))),
        (
            "distinguishing",
            array(
                alternatives
                    .windows(2)
                    .map(|pair| distinguishing(&pair[0], &pair[1], validations)),
            ),
        ),
    ])
}
/// Serialize one retained source as receipt version 3 `sources[]` entry.
///
/// The object carries `digest` and exactly one of `text` (the retained bytes
/// are valid UTF-8, encoded as a JSON string) or `bytes_hex` (they are not,
/// encoded as lowercase hexadecimal, two digits per byte). Nothing is
/// replaced, dropped, or normalized; this encoding cannot fail.
///
/// ```
/// use purrassist_core::{contract::SourceDocument, digest::Digest, wire};
/// let text = wire::source_document(&SourceDocument { digest: Digest::of(b"ok"), bytes: b"ok".to_vec() });
/// assert_eq!(text.get("text").and_then(|node| node.as_str()), Some("ok"));
/// assert!(text.get("bytes_hex").is_none());
/// let binary = wire::source_document(&SourceDocument { digest: Digest::of(&[0xff, 0x0a]), bytes: vec![0xff, 0x0a] });
/// assert_eq!(binary.get("bytes_hex").and_then(|node| node.as_str()), Some("ff0a"));
/// assert!(binary.get("text").is_none());
/// ```
#[must_use]
pub fn source_document(source: &SourceDocument) -> Node {
    object([
        ("digest", string(source.digest.to_string())),
        exact_bytes(&source.bytes),
    ])
}

/// The byte-exact member encoding `bytes`: `("text", string)` when they are
/// valid UTF-8, otherwise `("bytes_hex", lowercase hexadecimal)`.
pub(crate) fn exact_bytes(bytes: &[u8]) -> (&'static str, Node) {
    if let Ok(text) = core::str::from_utf8(bytes) {
        return ("text", string(text));
    }
    let mut hex = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        hex.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    ("bytes_hex", string(hex))
}

/// A half-open byte range as `{"start", "end"}`, saturating at `u64::MAX`.
pub(crate) fn byte_range(range: &core::ops::Range<usize>) -> Node {
    object([
        (
            "start",
            integer(u64::try_from(range.start).unwrap_or(u64::MAX)),
        ),
        ("end", integer(u64::try_from(range.end).unwrap_or(u64::MAX))),
    ])
}

/// One validation result in the receipt's wire representation.
pub(crate) fn validation(entry: &Validation) -> Node {
    object([
        ("validator", string(&entry.validator)),
        ("constraint", string(&entry.constraint)),
        ("passed", Node::new(Value::Bool(entry.passed))),
        ("diagnostics", array(entry.diagnostics.iter().map(string))),
    ])
}
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
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
        // `self.alternatives` is already ordered ascending by
        // `crate::provenance::Forest::rank`: every publishing call site
        // permutes its own `Outcome::Ambiguous` candidate vector by the same
        // `crate::provenance::Published::order` `Forest::publish` used to
        // build this Vec (see that type's documentation), so this wire array
        // and `Outcome::Ambiguous`'s own wire array (see
        // `AssistResult::to_node` below) already agree — nothing is
        // reordered here.
        object([
            ("version", integer(u64::from(self.version))),
            ("source", string(self.source.to_string())),
            ("sources", array(self.sources.iter().map(source_document))),
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
            (
                "ambiguity",
                ambiguity(&self.alternatives, &self.validations),
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
                array(self.validations.iter().map(validation)),
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
/// A value with one documented JSON wire representation, used to encode an
/// [`AssistResult`]'s answers.
///
/// Implementations must be deterministic: the bytes of `to_node().to_json()`
/// are the identity hashed into `AlternativeReceipt::output` wherever an
/// ambiguous operation publishes the value as an alternative. Implemented by
/// [`Node`] (its own representation) and [`crate::matching::RankedMatch`].
///
/// ```
/// use purrassist_core::{json::Node, wire::{self, WireValue}};
/// let node: Node = wire::string("tea");
/// assert_eq!(WireValue::to_node(&node).to_json(), "\"tea\"");
/// ```
///
/// Encoding is infallible; a value that cannot be represented must not
/// implement this trait.
pub trait WireValue {
    /// Encode this value in its documented wire representation.
    fn to_node(&self) -> Node;
}
impl WireValue for Node {
    /// A JSON node is its own wire representation, cloned unchanged.
    fn to_node(&self) -> Node {
        self.clone()
    }
}
impl<T: WireValue> AssistResult<T> {
    /// Encode the answer's status and receipt without hiding guesses or alternatives.
    ///
    /// The object carries `status` (`resolved`, `best_guess`, `ambiguous`, or
    /// `unresolved`), `value` (the single answer, or `null`), `alternatives`
    /// (every ambiguous answer in receipt order, otherwise empty), and
    /// `receipt`. Encoding is infallible.
    ///
    /// ```
    /// use purrassist_core::{AssistResult, Budget, Effort, Outcome, Receipt, wire};
    /// let result = AssistResult {
    ///     outcome: Outcome::BestGuess(wire::integer(7)),
    ///     receipt: Receipt::new(b"7", "example.v1", Effort::Standard, Budget::default()),
    /// };
    /// let node = result.to_node();
    /// assert_eq!(node.get("status").and_then(|n| n.as_str()), Some("best_guess"));
    /// assert_eq!(node.get("value").unwrap().to_json(), "7");
    /// ```
    #[must_use]
    pub fn to_node(&self) -> Node {
        let (status, value, alternatives) = match &self.outcome {
            Outcome::Resolved(value) => ("resolved", value.to_node(), array([])),
            Outcome::BestGuess(value) => ("best_guess", value.to_node(), array([])),
            Outcome::Ambiguous(values) => {
                // `values` is already in the same order as
                // `self.receipt.alternatives` (see that field's
                // documentation): the operation that built this
                // `Outcome::Ambiguous` permuted its own candidate vector by
                // `crate::provenance::Published::order` when it published
                // more than one alternative, exactly as `Receipt::alternatives`
                // itself was ordered. Nothing is reordered again here.
                (
                    "ambiguous",
                    Node::new(Value::Null),
                    array(values.iter().map(WireValue::to_node)),
                )
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
