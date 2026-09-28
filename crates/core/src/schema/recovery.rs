// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Composed, best-first recovery with whole-schema validation and explicit guesses.
mod proposals;
use super::Schema;
use crate::contract::{
    AlternativeReceipt, AssistResult, Budget, Effort, Evidence, Governor, Inference, InferenceKind,
    Loss, Outcome, Receipt, Resource, SourceSpan, StopReason, Validation,
};
use crate::json::{Node, ParseFailure, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Recovery policy; effort changes search bounds, never the schema's constraints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryRequest {
    /// Search preset. Exact performs no transformations.
    pub effort: Effort,
    /// Explicit resource override; otherwise use the effort preset.
    pub budget: Option<Budget>,
    /// Permit invented values, always returned as `BestGuess` or ambiguity.
    pub allow_guesses: bool,
}
impl Default for RecoveryRequest {
    fn default() -> Self {
        Self {
            effort: Effort::Standard,
            budget: None,
            allow_guesses: true,
        }
    }
}
#[derive(Clone)]
struct Candidate {
    node: Node,
    cost: u64,
    evidence: Vec<Evidence>,
    losses: Vec<Loss>,
    inferences: Vec<Inference>,
}
struct Search<'a> {
    schema: &'a Schema,
    request: RecoveryRequest,
    governor: Governor,
    receipt: Receipt,
    queue: BTreeMap<(u64, u64), Candidate>,
    seen: BTreeMap<String, u64>,
    serial: u64,
}
impl Schema {
    /// Recover a schema-valid interpretation of imperfect UTF-8 JSON input.
    ///
    /// Every accepted answer is validated against this complete schema. Equal-cost
    /// valid interpretations remain ambiguous. Resource exhaustion returns
    /// `Unresolved` with an explicit termination reason, even if an unranked valid
    /// candidate has been encountered. Invented values are never `Resolved`.
    /// Ambiguous answers have separate `receipt.alternatives` ledgers in output
    /// order, with output digests and indices into the receipt's validation log.
    ///
    /// ```
    /// use purrassist_core::{Outcome, schema::{Schema, recovery::RecoveryRequest}};
    /// let schema = Schema::compile(r#"{"type":"object"}"#).unwrap();
    /// let result = schema.recover(r#"{"x":1,"x":2}"#, RecoveryRequest::default());
    /// let Outcome::Ambiguous(choices) = result.outcome else { panic!("expected alternatives") };
    /// assert_eq!(choices.len(), result.receipt.alternatives.len());
    /// for ledger in &result.receipt.alternatives {
    ///     assert!(result.receipt.validations[ledger.validations[0]].passed);
    /// }
    /// ```
    #[must_use]
    pub fn recover(&self, input: &str, request: RecoveryRequest) -> AssistResult<Node> {
        let budget = request.budget.unwrap_or_else(|| request.effort.budget());
        let mut search = Search {
            schema: self,
            request,
            governor: Governor::new(budget),
            receipt: Receipt::new(input.as_bytes(), "json.recover.v1", request.effort, budget),
            queue: BTreeMap::new(),
            seen: BTreeMap::new(),
            serial: 0,
        };
        let outcome = match search.run(input) {
            Ok(outcome) => outcome,
            Err(reason) => {
                search.receipt.stopped = reason;
                Outcome::Unresolved
            }
        };
        search.receipt.usage = search.governor.usage();
        AssistResult {
            outcome,
            receipt: search.receipt,
        }
    }
}
impl Search<'_> {
    fn evidence(&self, node: &Node, path: &str, rule: &str, explanation: String) -> Evidence {
        Evidence {
            rule: rule.into(),
            explanation,
            destination: path.into(),
            span: node.span.clone().map(|bytes| SourceSpan {
                source: self.receipt.source,
                bytes,
            }),
        }
    }
    fn enqueue(&mut self, candidate: Candidate) -> Result<(), StopReason> {
        let key = candidate.node.to_json();
        self.governor.charge(Resource::Work, size(key.len()))?;
        if self
            .seen
            .get(&key)
            .is_some_and(|cost| *cost <= candidate.cost)
        {
            return Ok(());
        }
        self.governor.charge(Resource::Candidates, 1)?;
        self.governor.charge(
            Resource::ScratchBytes,
            size(key.len()).saturating_mul(32).saturating_add(256),
        )?;
        self.seen.insert(key, candidate.cost);
        self.queue.insert((candidate.cost, self.serial), candidate);
        self.serial += 1;
        Ok(())
    }
    fn run(&mut self, input: &str) -> Result<Outcome<Node>, StopReason> {
        self.receipt.retain_source(input, &mut self.governor)?;
        self.governor
            .charge(Resource::InputBytes, size(input.len()))?;
        match crate::json::parse_with(input, &mut self.governor) {
            Ok(node) => self.enqueue(Candidate {
                node,
                cost: 0,
                evidence: Vec::new(),
                losses: Vec::new(),
                inferences: Vec::new(),
            })?,
            Err(ParseFailure::Resource(reason)) => return Err(reason),
            Err(ParseFailure::Syntax(error)) if self.request.effort == Effort::Exact => {
                return Err(StopReason::InvalidInput(error.to_string()));
            }
            Err(ParseFailure::Syntax(_)) => self.syntax(input)?,
        }
        let mut accepted = Vec::new();
        let mut winning_cost = None;
        while let Some(((cost, _), candidate)) = self.queue.pop_first() {
            if winning_cost.is_some_and(|winner| cost > winner) {
                break;
            }
            let report = self
                .schema
                .validate_with(&candidate.node, &mut self.governor)?;
            self.governor.charge(Resource::EvidenceEntries, 1)?;
            self.receipt.validations.push(Validation {
                validator: "purrassist.schema.draft2020-12.v1".into(),
                constraint: self.schema.digest.to_string(),
                passed: report.valid,
                diagnostics: report
                    .violations
                    .iter()
                    .map(|violation| {
                        format!(
                            "{}: {} ({})",
                            violation.instance_path, violation.message, violation.schema_path
                        )
                    })
                    .collect(),
            });
            if report.valid {
                winning_cost = Some(cost);
                accepted.push((candidate, self.receipt.validations.len() - 1));
            } else if self.request.effort != Effort::Exact && winning_cost.is_none() {
                let mut visited = BTreeSet::new();
                self.propose(
                    &candidate,
                    &candidate.node,
                    self.schema.root,
                    (&[], ""),
                    0,
                    &mut visited,
                )?;
            }
        }
        self.finish(accepted)
    }
    fn finish(&mut self, accepted: Vec<(Candidate, usize)>) -> Result<Outcome<Node>, StopReason> {
        if accepted.is_empty() {
            self.receipt.stopped = StopReason::NoCandidate;
            return Ok(Outcome::Unresolved);
        }
        let guesses = accepted
            .iter()
            .any(|(candidate, _)| !candidate.inferences.is_empty());
        let ambiguous = accepted.len() > 1;
        // Admit the new records before publishing any of them. Existing ledger
        // entries are moved, preserving their original resource charges.
        if ambiguous {
            self.governor
                .charge(Resource::EvidenceEntries, size(accepted.len()))?;
            self.governor.charge(
                Resource::ScratchBytes,
                size(accepted.len()).saturating_mul(256),
            )?;
        }
        let mut nodes = Vec::new();
        let mut alternatives = Vec::new();
        for (candidate, validation) in accepted {
            if ambiguous {
                let output = candidate.node.to_json();
                self.governor.charge(Resource::Work, size(output.len()))?;
                alternatives.push(AlternativeReceipt {
                    output: crate::digest::Digest::of(output.as_bytes()),
                    cost: candidate.cost,
                    evidence: candidate.evidence,
                    losses: candidate.losses,
                    inferences: candidate.inferences,
                    validations: vec![validation],
                });
            } else {
                self.receipt.evidence.extend(candidate.evidence);
                self.receipt.losses.extend(candidate.losses);
                self.receipt.inferences.extend(candidate.inferences);
            }
            nodes.push(candidate.node);
        }
        self.receipt.alternatives = alternatives;
        Ok(if nodes.len() > 1 {
            Outcome::Ambiguous(nodes)
        } else if guesses {
            Outcome::BestGuess(nodes.remove(0))
        } else {
            Outcome::Resolved(nodes.remove(0))
        })
    }
    fn syntax(&mut self, input: &str) -> Result<(), StopReason> {
        let mut starts = vec![0];
        starts.extend(
            input
                .char_indices()
                .filter(|(offset, c)| *offset > 0 && matches!(c, '[' | '{'))
                .map(|(offset, _)| offset),
        );
        for start in starts {
            self.governor.charge(Resource::Work, 1)?;
            let (mut node, consumed, edits) =
                match crate::json::recover_syntax(&input[start..], &mut self.governor) {
                    Ok(result) => result,
                    Err(StopReason::InvalidInput(_)) => continue,
                    Err(reason) => return Err(reason),
                };
            if !self.request.allow_guesses && edits.iter().any(|edit| edit.guessed) {
                continue;
            }
            shift_spans(&mut node, start);
            let mut candidate = Candidate {
                node,
                cost: 0,
                evidence: Vec::new(),
                losses: Vec::new(),
                inferences: Vec::new(),
            };
            for edit in edits {
                let range = edit.span.start + start..edit.span.end + start;
                let evidence = Evidence {
                    rule: edit.rule.into(),
                    explanation: format!("replace source range with {}", edit.replacement),
                    span: Some(SourceSpan {
                        source: self.receipt.source,
                        bytes: range.clone(),
                    }),
                    destination: String::new(),
                };
                candidate.cost += if edit.guessed { 100 } else { 1 };
                if edit.guessed {
                    candidate.inferences.push(Inference {
                        evidence: evidence.clone(),
                        kind: InferenceKind::Guess,
                        value: edit.replacement,
                    });
                }
                if !range.is_empty() {
                    candidate.losses.push(Loss {
                        evidence: evidence.clone(),
                        original: input[range].into(),
                    });
                }
                candidate.evidence.push(evidence);
            }
            for range in [0..start, start + consumed..input.len()] {
                if !input[range.clone()].trim().is_empty() {
                    self.governor.charge(Resource::EvidenceEntries, 1)?;
                    let evidence = Evidence {
                        rule: "json.extract".into(),
                        explanation: "select a JSON region from surrounding content".into(),
                        span: Some(SourceSpan {
                            source: self.receipt.source,
                            bytes: range.clone(),
                        }),
                        destination: String::new(),
                    };
                    candidate.losses.push(Loss {
                        evidence: evidence.clone(),
                        original: input[range.clone()].into(),
                    });
                    candidate.evidence.push(evidence);
                    candidate.cost += 5 + 10
                        * size(
                            input[range]
                                .chars()
                                .filter(|c| matches!(c, '[' | ']' | '{' | '}'))
                                .count(),
                        );
                }
            }
            self.enqueue(candidate)?;
        }
        Ok(())
    }
    fn change(
        &mut self,
        parent: &Candidate,
        route: &[usize],
        path: &str,
        edit: (Node, &str, u64, bool),
    ) -> Result<(), StopReason> {
        let (replacement, rule, cost, guessed) = edit;
        if guessed && !self.request.allow_guesses {
            return Ok(());
        }
        self.governor.charge(Resource::Work, 1)?;
        self.governor.charge(Resource::EvidenceEntries, 1)?;
        let original = at(&parent.node, route);
        if original.equivalent(&replacement) {
            return Ok(());
        }
        let evidence = self.evidence(
            original,
            path,
            rule,
            format!("replace with {}", replacement.to_json()),
        );
        let mut candidate = parent.clone();
        candidate.cost = candidate.cost.saturating_add(cost);
        candidate.evidence.push(evidence.clone());
        candidate.losses.push(Loss {
            evidence: evidence.clone(),
            original: original.to_json(),
        });
        if guessed {
            candidate.inferences.push(Inference {
                evidence,
                kind: InferenceKind::Guess,
                value: replacement.to_json(),
            });
        }
        *at_mut(&mut candidate.node, route) = replacement;
        self.enqueue(candidate)
    }
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
fn at<'a>(mut node: &'a Node, route: &[usize]) -> &'a Node {
    for &index in route {
        node = match &node.value {
            Value::Array(items) => &items[index],
            Value::Object(members) => &members[index].value,
            _ => unreachable!("generated route"),
        };
    }
    node
}
fn at_mut<'a>(mut node: &'a mut Node, route: &[usize]) -> &'a mut Node {
    for &index in route {
        node = match &mut node.value {
            Value::Array(items) => &mut items[index],
            Value::Object(members) => &mut members[index].value,
            _ => unreachable!("generated route"),
        };
    }
    node
}
fn shift_spans(node: &mut Node, offset: usize) {
    if let Some(span) = &mut node.span {
        span.start += offset;
        span.end += offset;
    }
    match &mut node.value {
        Value::Array(items) => {
            for item in items {
                shift_spans(item, offset);
            }
        }
        Value::Object(members) => {
            for member in members {
                if let Some(span) = &mut member.key_span {
                    span.start += offset;
                    span.end += offset;
                }
                shift_spans(&mut member.value, offset);
            }
        }
        _ => {}
    }
}
