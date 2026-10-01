// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Composed, best-first recovery with whole-schema validation and explicit guesses.
mod proposals;
use super::Schema;
use crate::contract::saturating_size as size;
use crate::contract::{
    AssistResult, Budget, Effort, Evidence, Governor, Outcome, Receipt, Resource, ScratchVec,
    SourceSpan, StopReason,
};
use crate::digest::Digest;
use crate::json::{Node, ParseFailure, Value};
use crate::search::{Acceptance, Change, Ledger, Proposals, Search, SearchState};
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
/// Search-wide recovery state shared by every [`Repair`] candidate.
struct Recovery<'a> {
    schema: &'a Schema,
    request: RecoveryRequest,
    receipt: Receipt,
    /// Task 6 wiring: deduplicated by destination (one distinct compiled
    /// pattern), across every candidate this search actually validated.
    pattern_evidence: BTreeMap<String, Evidence>,
}
/// One candidate JSON document; its deduplication key and output identity
/// are its compact JSON bytes, and equal-cost candidates keep enqueue order.
#[derive(Clone)]
struct Repair(Node);
/// One expansion of a rejected [`Repair`]: the schema walk in `proposals`
/// proposes local changes to `parent` through [`Walk::change`].
struct Walk<'w, 'p> {
    schema: &'w Schema,
    request: RecoveryRequest,
    source: Digest,
    governor: &'w mut Governor,
    proposals: &'w mut Proposals<'p, Repair>,
    parent: &'w Node,
}
impl SearchState for Repair {
    type Tie = ();
    type Context<'c> = Recovery<'c>;
    fn tie(&self) {}
    fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason> {
        let json = self.0.to_json();
        key.reserve(json.len())?;
        key.extend(json.bytes())
    }
    fn accept(
        &self,
        recovery: &mut Recovery<'_>,
        governor: &mut Governor,
    ) -> Result<Acceptance, StopReason> {
        recovery.validate(&self.0, governor)
    }
    fn expand(
        &self,
        recovery: &mut Recovery<'_>,
        proposals: &mut Proposals<'_, Self>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if recovery.request.effort == Effort::Exact {
            return Ok(());
        }
        let mut walk = Walk {
            schema: recovery.schema,
            request: recovery.request,
            source: recovery.receipt.source,
            governor,
            proposals,
            parent: &self.0,
        };
        walk.propose(
            &self.0,
            recovery.schema.root,
            (&[], ""),
            0,
            &mut BTreeSet::new(),
            &mut Vec::new(),
        )
    }
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
        let mut recovery = Recovery {
            schema: self,
            request,
            receipt: Receipt::new(input.as_bytes(), "json.recover.v1", request.effort, budget),
            pattern_evidence: BTreeMap::new(),
        };
        let mut search = Search::new(governor);
        let outcome = match recovery
            .unsupported_vocabulary_evidence(governor)
            .and_then(|()| recovery.run(&mut search, input, governor))
        {
            Ok(outcome) => outcome,
            Err(reason) => {
                recovery.receipt.stopped = reason;
                Outcome::Unresolved
            }
        };
        drop(search);
        // Task 6 wiring: every distinct pattern actually validated during
        // this search, regardless of which candidate (if any) was accepted.
        recovery
            .receipt
            .evidence
            .extend(recovery.pattern_evidence.into_values());
        recovery.receipt.usage = governor.usage();
        AssistResult {
            outcome,
            receipt: recovery.receipt,
        }
    }
}
impl Recovery<'_> {
    /// Requirement 5: every recovery receipt carries an
    /// `unsupported-vocabulary` evidence entry for each optional (`false`)
    /// `$vocabulary` URI this schema's dialects did not recognize, naming
    /// the declaring resource's own keywords left unevaluated.
    fn unsupported_vocabulary_evidence(
        &mut self,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        for unsupported in self.schema.unsupported_vocabularies() {
            governor.charge(Resource::EvidenceEntries, 1)?;
            let explanation = format!(
                "vocabulary {} is not supported; keywords left unevaluated: {}",
                unsupported.uri,
                unsupported.keywords.join(", ")
            );
            governor.charge(
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
    fn run(
        &mut self,
        search: &mut Search<Repair>,
        input: &str,
        governor: &mut Governor,
    ) -> Result<Outcome<Node>, StopReason> {
        self.receipt.retain_source(input, governor)?;
        governor.charge(Resource::InputBytes, size(input.len()))?;
        match crate::json::parse_with(input, governor) {
            Ok(node) => search.seed(governor, Repair(node), Ledger::default())?,
            Err(ParseFailure::Resource(reason)) => return Err(reason),
            Err(ParseFailure::Syntax(error)) if self.request.effort == Effort::Exact => {
                return Err(StopReason::InvalidInput(error.to_string()));
            }
            Err(ParseFailure::Syntax(_)) => self.syntax(search, input, governor)?,
        }
        let accepted = search.run(self, governor)?;
        let outcome = search.finish(accepted, &mut self.receipt, governor)?;
        Ok(outcome.map(|repair| repair.0))
    }
    /// Validate one candidate against the whole schema, retaining its
    /// validation, format, and pattern evidence in the receipt.
    fn validate(&mut self, node: &Node, governor: &mut Governor) -> Result<Acceptance, StopReason> {
        let report = self.schema.validate_with(node, governor)?;
        governor.charge(Resource::EvidenceEntries, 1)?;
        // Every violation's diagnostic line is permanently retained in the
        // receipt (never released), so it is charged directly, before the
        // `format!` calls that build it, like `retain_source`.
        let diagnostic_bytes = report.violations.iter().fold(0_u64, |sum, violation| {
            sum.saturating_add(size(violation.instance_path.len()))
                .saturating_add(size(violation.message.len()))
                .saturating_add(size(violation.schema_path.len()))
                .saturating_add(8)
        });
        governor.charge(Resource::ScratchBytes, diagnostic_bytes)?;
        let validation = self.receipt.validations.len();
        self.receipt.validations.push(report.to_validation());
        // Requirement 1 / Task 6 wiring: format annotation evidence and
        // pattern-engine evidence are search-time signals, not tied to
        // any one alternative, so they are retained unconditionally
        // (after the index captured above) rather than routed through
        // the derivation forest's per-alternative ledgers.
        governor.charge(
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
        governor.charge(Resource::ScratchBytes, format_bytes)?;
        self.receipt
            .validations
            .extend(report.format_evidence.iter().cloned());
        for evidence in &report.pattern_evidence {
            if !self.pattern_evidence.contains_key(&evidence.destination) {
                governor.charge(Resource::EvidenceEntries, 1)?;
                governor.charge(
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
        Ok(if report.valid {
            Acceptance::Accepted { validation }
        } else {
            Acceptance::Rejected
        })
    }
    fn syntax(
        &self,
        search: &mut Search<Repair>,
        input: &str,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut starts = vec![0];
        starts.extend(
            input
                .char_indices()
                .filter(|(offset, c)| *offset > 0 && matches!(c, '[' | '{'))
                .map(|(offset, _)| offset),
        );
        for start in starts {
            governor.charge(Resource::Work, 1)?;
            let (mut node, consumed, edits) =
                match crate::json::recover_syntax(&input[start..], governor) {
                    Ok(result) => result,
                    Err(StopReason::InvalidInput(_)) => continue,
                    Err(reason) => return Err(reason),
                };
            if !self.request.allow_guesses && edits.iter().any(|edit| edit.guessed) {
                continue;
            }
            shift_spans(&mut node, start);
            let mut ledger = Ledger::default();
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
                if !range.is_empty() {
                    // Charge the retained loss copy (and its evidence clone)
                    // before allocating either.
                    governor.charge(
                        Resource::ScratchBytes,
                        size(range.len()).saturating_add(128),
                    )?;
                }
                ledger.record(Change {
                    evidence,
                    cost: if edit.guessed { 100 } else { 1 },
                    original: (!range.is_empty()).then(|| input[range].into()),
                    guess: edit.guessed.then_some(edit.replacement),
                });
            }
            for range in [0..start, start + consumed..input.len()] {
                if !input[range.clone()].trim().is_empty() {
                    governor.charge(Resource::EvidenceEntries, 1)?;
                    let evidence = Evidence {
                        rule: "json.extract".into(),
                        explanation: "select a JSON region from surrounding content".into(),
                        span: Some(SourceSpan {
                            source: self.receipt.source,
                            bytes: range.clone(),
                        }),
                        destination: String::new(),
                    };
                    governor.charge(
                        Resource::ScratchBytes,
                        size(range.len()).saturating_add(128),
                    )?;
                    let brackets = input[range.clone()]
                        .chars()
                        .filter(|c| matches!(c, '[' | ']' | '{' | '}'))
                        .count();
                    ledger.record(Change {
                        evidence,
                        cost: 5 + 10 * size(brackets),
                        original: Some(input[range].into()),
                        guess: None,
                    });
                }
            }
            search.seed(governor, Repair(node), ledger)?;
        }
        Ok(())
    }
}
impl Walk<'_, '_> {
    /// Propose replacing the node at `route` (reported at `path`) in the
    /// parent candidate with `edit`'s `(replacement, rule, cost, guessed)`.
    fn change(
        &mut self,
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
        let parent = self.parent;
        let original = at(parent, route);
        if original.equivalent(&replacement) {
            return Ok(());
        }
        // Cloning the parent duplicates the whole candidate (node tree and
        // every ledger so far); `original`/`replacement` are each serialized
        // at least once below. Charge a bound on all of it before any of
        // those allocations happen, and release exactly that bound once this
        // change either lands in the queue or is discarded as a duplicate.
        let clone_bytes = parent
            .estimated_bytes()
            .saturating_add(self.proposals.parent().estimated_bytes())
            .saturating_add(original.estimated_bytes())
            .saturating_add(replacement.estimated_bytes().saturating_mul(3))
            .saturating_add(512);
        let _lease = self.governor.lease_scratch(clone_bytes)?;
        let evidence = Evidence {
            rule: rule.into(),
            explanation: format!("replace with {}", replacement.to_json()),
            destination: path.into(),
            span: original.span.clone().map(|bytes| SourceSpan {
                source: self.source,
                bytes,
            }),
        };
        let change = Change {
            evidence,
            cost,
            original: Some(original.to_json()),
            guess: guessed.then(|| replacement.to_json()),
        };
        let mut node = parent.clone();
        *at_mut(&mut node, route) = replacement;
        self.proposals.commit(self.governor, Repair(node), change)
    }
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
