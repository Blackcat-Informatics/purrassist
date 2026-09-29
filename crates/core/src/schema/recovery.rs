// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Composed, best-first recovery with whole-schema validation and explicit guesses.
mod proposals;
use super::Schema;
use crate::contract::{
    AssistResult, Budget, Effort, Evidence, Governor, Inference, InferenceKind, Loss, Outcome,
    Receipt, Resource, SourceSpan, StopReason, Validation,
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
    governor: &'a mut Governor,
    receipt: Receipt,
    queue: BTreeMap<(u64, u64), Candidate>,
    seen: BTreeMap<String, u64>,
    serial: u64,
    /// Task 6 wiring: deduplicated by destination (one distinct compiled
    /// pattern), across every candidate this search actually validated.
    pattern_evidence: BTreeMap<String, Evidence>,
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
        self.recover_governed(input, request, &mut Governor::new(budget))
    }

    /// Recover using the same governor as schema compilation or caller preparation.
    ///
    /// The governor's limits are authoritative; `request.budget` is ignored here.
    /// Effort still selects allowed recovery rules. Cancellation and exhaustion
    /// return `Unresolved` with cumulative usage and an explicit termination reason.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, schema::{Schema, recovery::RecoveryRequest}};
    /// let mut governor = Governor::new(Budget::default());
    /// let schema = Schema::compile_governed(r#"{"type":"integer"}"#, &mut governor)?;
    /// let result = schema.recover_governed("[[42]]", RecoveryRequest::default(), &mut governor);
    /// assert!(matches!(result.outcome, Outcome::Resolved(_)));
    /// assert_eq!(result.receipt.usage, governor.usage());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn recover_governed(
        &self,
        input: &str,
        request: RecoveryRequest,
        governor: &mut Governor,
    ) -> AssistResult<Node> {
        let budget = *governor.limits();
        let mut search = Search {
            schema: self,
            request,
            governor,
            receipt: Receipt::new(input.as_bytes(), "json.recover.v1", request.effort, budget),
            queue: BTreeMap::new(),
            seen: BTreeMap::new(),
            serial: 0,
            pattern_evidence: BTreeMap::new(),
        };
        let outcome = match search
            .unsupported_vocabulary_evidence()
            .and_then(|()| search.run(input))
        {
            Ok(outcome) => outcome,
            Err(reason) => {
                search.receipt.stopped = reason;
                Outcome::Unresolved
            }
        };
        // Task 6 wiring: every distinct pattern actually validated during
        // this search, regardless of which candidate (if any) was accepted.
        search
            .receipt
            .evidence
            .extend(search.pattern_evidence.into_values());
        search.receipt.usage = search.governor.usage();
        AssistResult {
            outcome,
            receipt: search.receipt,
        }
    }
}
impl Search<'_> {
    /// Requirement 5: every recovery receipt carries an
    /// `unsupported-vocabulary` evidence entry for each optional (`false`)
    /// `$vocabulary` URI this schema's dialects did not recognize, naming
    /// the declaring resource's own keywords left unevaluated.
    fn unsupported_vocabulary_evidence(&mut self) -> Result<(), StopReason> {
        for unsupported in self.schema.unsupported_vocabularies() {
            self.governor.charge(Resource::EvidenceEntries, 1)?;
            let explanation = format!(
                "vocabulary {} is not supported; keywords left unevaluated: {}",
                unsupported.uri,
                unsupported.keywords.join(", ")
            );
            self.governor.charge(
                Resource::ScratchBytes,
                size(unsupported.uri.len())
                    .saturating_add(size(explanation.len()))
                    .saturating_add(64),
            )?;
            self.receipt.evidence.push(Evidence {
                rule: "schema.vocabulary.unsupported-v1".into(),
                explanation,
                span: None,
                destination: unsupported.uri,
            });
        }
        Ok(())
    }
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
        self.receipt.retain_source(input, self.governor)?;
        self.governor
            .charge(Resource::InputBytes, size(input.len()))?;
        match crate::json::parse_with(input, self.governor) {
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
            let report = self.schema.validate_with(&candidate.node, self.governor)?;
            self.governor.charge(Resource::EvidenceEntries, 1)?;
            // Every violation's diagnostic line is permanently retained in the
            // receipt (never released), so it is charged directly, before the
            // `format!` calls that build it, like `retain_source`.
            let diagnostic_bytes = report.violations.iter().fold(0_u64, |sum, violation| {
                sum.saturating_add(size(violation.instance_path.len()))
                    .saturating_add(size(violation.message.len()))
                    .saturating_add(size(violation.schema_path.len()))
                    .saturating_add(8)
            });
            self.governor
                .charge(Resource::ScratchBytes, diagnostic_bytes)?;
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
            // Requirement 1 / Task 6 wiring: format annotation evidence and
            // pattern-engine evidence are search-time signals, not tied to
            // any one alternative, so they are retained unconditionally
            // (after the index captured above) rather than routed through
            // the derivation forest's per-alternative ledgers.
            self.governor.charge(
                Resource::EvidenceEntries,
                size(report.format_evidence.len()),
            )?;
            let format_bytes = report.format_evidence.iter().fold(0_u64, |sum, entry| {
                sum.saturating_add(size(entry.validator.len()))
                    .saturating_add(size(entry.constraint.len()))
                    .saturating_add(
                        entry
                            .diagnostics
                            .iter()
                            .fold(0, |inner, line| inner.saturating_add(size(line.len()))),
                    )
                    .saturating_add(32)
            });
            self.governor.charge(Resource::ScratchBytes, format_bytes)?;
            self.receipt
                .validations
                .extend(report.format_evidence.iter().cloned());
            for evidence in &report.pattern_evidence {
                if !self.pattern_evidence.contains_key(&evidence.destination) {
                    self.governor.charge(Resource::EvidenceEntries, 1)?;
                    self.governor.charge(
                        Resource::ScratchBytes,
                        size(evidence.rule.len())
                            .saturating_add(size(evidence.explanation.len()))
                            .saturating_add(size(evidence.destination.len()))
                            .saturating_add(32),
                    )?;
                    self.pattern_evidence
                        .insert(evidence.destination.clone(), evidence.clone());
                }
            }
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
                    &mut Vec::new(),
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
        let mut nodes = Vec::with_capacity(accepted.len());
        if ambiguous {
            // Route through the derivation forest: evidence, losses, and
            // inferences reachable from *every* accepted candidate (for
            // example, an admission-level correction applied identically
            // regardless of which later proposal was chosen) are promoted
            // once to the shared, top-level ledgers instead of being
            // duplicated into every alternative. Nothing is admitted to
            // `self.receipt` until every candidate is interned and the whole
            // forest is published, so exhaustion here never leaves a partial
            // alternative list.
            let mut forest = crate::provenance::Forest::new(self.governor);
            let mut derivations = Vec::with_capacity(accepted.len());
            for (candidate, validation) in accepted {
                let output = candidate.node.to_json();
                self.governor.charge(Resource::Work, size(output.len()))?;
                let mut derivation = crate::provenance::Derivation::new(
                    crate::digest::Digest::of(output.as_bytes()),
                    candidate.cost,
                );
                for evidence in candidate.evidence {
                    derivation.push(forest.evidence(self.governor, evidence)?);
                }
                for loss in candidate.losses {
                    derivation.push(forest.loss(self.governor, loss)?);
                }
                for inference in candidate.inferences {
                    derivation.push(forest.inference(self.governor, inference)?);
                }
                derivation.push(forest.validation(self.governor, validation, true)?);
                derivations.push(derivation);
                nodes.push(candidate.node);
            }
            let published = forest.publish(&derivations, self.governor)?;
            self.receipt.evidence = published.evidence;
            self.receipt.losses = published.losses;
            self.receipt.inferences = published.inferences;
            self.receipt.alternatives = published.alternatives;
        } else {
            let (candidate, _validation) = accepted
                .into_iter()
                .next()
                .expect("exactly one accepted candidate");
            self.receipt.evidence.extend(candidate.evidence);
            self.receipt.losses.extend(candidate.losses);
            self.receipt.inferences.extend(candidate.inferences);
            nodes.push(candidate.node);
        }
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
                match crate::json::recover_syntax(&input[start..], self.governor) {
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
                    // Charge the retained loss copy (and its evidence clone)
                    // before allocating either.
                    self.governor.charge(
                        Resource::ScratchBytes,
                        size(range.len()).saturating_add(128),
                    )?;
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
                    self.governor.charge(
                        Resource::ScratchBytes,
                        size(range.len()).saturating_add(128),
                    )?;
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
        // `parent.clone()` duplicates the whole candidate (node tree and
        // every ledger so far); `original`/`replacement` are each serialized
        // at least once below. Charge a bound on all of it before any of
        // those allocations happen, and release exactly that bound once this
        // change either lands in the queue or is discarded as a duplicate.
        let clone_bytes = candidate_weight(parent)
            .saturating_add(original.estimated_bytes())
            .saturating_add(replacement.estimated_bytes().saturating_mul(3))
            .saturating_add(512);
        let _lease = self.governor.lease_scratch(clone_bytes)?;
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
/// A conservative byte-size estimate for a whole candidate, used to charge
/// scratch before `.clone()`-ing it while exploring a proposal.
fn candidate_weight(candidate: &Candidate) -> u64 {
    candidate.node.estimated_bytes().saturating_add(
        size(candidate.evidence.len())
            .saturating_add(size(candidate.losses.len()))
            .saturating_add(size(candidate.inferences.len()))
            .saturating_mul(256),
    )
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
