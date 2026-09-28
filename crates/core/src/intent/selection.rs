// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Candidate-specific semantic evidence and atomic ledger publication.
use super::{Concept, Interpretation, Rule, State, size};
use crate::{
    Governor, Outcome, Receipt, StopReason,
    contract::{AlternativeReceipt, Evidence, Inference, InferenceKind, Resource, Validation},
    digest::Digest,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Selection {
    value: Interpretation,
    evidence: Vec<Evidence>,
}
fn map_size(map: &BTreeMap<String, String>) -> u64 {
    map.iter().fold(0u64, |size_so_far, (key, value)| {
        size_so_far
            .saturating_add(size(key.len()))
            .saturating_add(size(value.len()))
            .saturating_add(256)
    })
}
fn evidence_size(evidence: &[Evidence]) -> u64 {
    evidence.iter().fold(0u64, |total, entry| {
        total
            .saturating_add(size(entry.rule.len()))
            .saturating_add(size(entry.destination.len()))
            .saturating_add(size(entry.explanation.len()))
            .saturating_add(256)
    })
}
pub(super) fn admit_branch(
    state: &State,
    rule: &Rule,
    domain: &str,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    governor.charge(
        Resource::Work,
        size(state.evidence.len())
            .saturating_add(size(state.desired.len()))
            .saturating_add(size(rule.assignments.len())),
    )?;
    let strings = map_size(&state.desired)
        .saturating_add(map_size(&rule.assignments))
        .saturating_add(evidence_size(&state.evidence));
    let labels = size(domain.len())
        .saturating_add(size(rule.id.len()))
        .saturating_add(size(rule.phrase.len()));
    let reserve = strings
        .saturating_add(labels.saturating_mul(size(rule.assignments.len())))
        .saturating_mul(8)
        .saturating_add(512);
    governor.charge(Resource::Work, reserve)?;
    governor.charge(Resource::ScratchBytes, reserve)?;
    governor.charge(Resource::EvidenceEntries, size(state.evidence.len()))
}
pub(super) fn select(
    concepts: &[Concept],
    states: Vec<State>,
    governor: &mut Governor,
) -> Result<Vec<Selection>, StopReason> {
    let mut identities = BTreeSet::new();
    for concept in concepts {
        governor.charge(Resource::Work, size(concept.id.len()).saturating_add(1))?;
        governor.charge(Resource::ScratchBytes, 64)?;
        if !identities.insert(&concept.id) {
            return Err(StopReason::InvalidInput(
                "duplicate concept identity".into(),
            ));
        }
    }
    let mut answers: Vec<Selection> = Vec::new();
    for state in states {
        if state.desired.is_empty() {
            continue;
        }
        let desired_size = map_size(&state.desired);
        for concept in concepts {
            governor.charge(
                Resource::Work,
                desired_size.saturating_mul(size(concept.attributes.len()).saturating_add(1)),
            )?;
            if !state
                .desired
                .iter()
                .all(|(key, value)| concept.attributes.get(key) == Some(value))
            {
                continue;
            }
            governor.charge(
                Resource::Work,
                desired_size
                    .saturating_add(size(concept.id.len()))
                    .saturating_mul(size(answers.len())),
            )?;
            if answers.iter().any(|answer| {
                answer.value.concept.id == concept.id && answer.value.desired == state.desired
            }) {
                continue;
            }
            governor.charge(
                Resource::Work,
                size(state.evidence.len()).saturating_add(size(concept.attributes.len())),
            )?;
            let storage = desired_size
                .saturating_add(map_size(&concept.attributes))
                .saturating_add(size(concept.id.len()))
                .saturating_add(size(concept.label.len()))
                .saturating_add(evidence_size(&state.evidence));
            governor.charge(
                Resource::ScratchBytes,
                storage.saturating_mul(8).saturating_add(512),
            )?;
            governor.charge(Resource::EvidenceEntries, size(state.evidence.len()))?;
            answers.push(Selection {
                value: Interpretation {
                    concept: concept.clone(),
                    desired: state.desired.clone(),
                },
                evidence: state.evidence.clone(),
            });
        }
    }
    // Stable sort preserves rule-order provenance when concept identities tie.
    governor.charge(
        Resource::Work,
        size(answers.len()).saturating_mul(size(answers.len())),
    )?;
    answers.sort_by(|a, b| a.value.concept.id.cmp(&b.value.concept.id));
    Ok(answers)
}
pub(super) fn finish(
    answers: Vec<Selection>,
    receipt: &mut Receipt,
    governor: &mut Governor,
) -> Result<Outcome<Interpretation>, StopReason> {
    if answers.is_empty() {
        receipt.stopped = StopReason::NoCandidate;
        return Ok(Outcome::Unresolved);
    }
    let ambiguous = answers.len() > 1;
    let mut values = Vec::new();
    let mut ledgers = Vec::new();
    let mut validations = Vec::new();
    for answer in answers {
        governor.charge(
            Resource::EvidenceEntries,
            2u64.saturating_add(size(answer.value.desired.len())),
        )?;
        governor.charge(
            Resource::Work,
            size(answer.evidence.len())
                .saturating_mul(size(answer.value.desired.len()))
                .saturating_add(1),
        )?;
        let encoding = map_size(&answer.value.concept.attributes)
            .saturating_add(map_size(&answer.value.desired))
            .saturating_add(size(answer.value.concept.id.len()))
            .saturating_add(size(answer.value.concept.label.len()));
        governor.charge(Resource::Work, encoding.saturating_mul(8))?;
        governor.charge(
            Resource::ScratchBytes,
            encoding
                .saturating_add(evidence_size(&answer.evidence))
                .saturating_mul(16)
                .saturating_add(1024),
        )?;
        let output = Digest::of(answer.value.to_node().to_json().as_bytes());
        let inferences = derived(&answer);
        let validation = validations.len();
        validations.push(Validation {
            validator: "purrassist.intent.attributes.v1".into(),
            constraint: format!(
                "concept {:?} satisfies every derived attribute; output {output}",
                answer.value.concept.id
            ),
            passed: true,
            diagnostics: Vec::new(),
        });
        ledgers.push(AlternativeReceipt {
            output,
            cost: 0,
            evidence: answer.evidence,
            losses: Vec::new(),
            inferences,
            validations: vec![validation],
        });
        values.push(answer.value);
    }
    // Publish only after all candidates and their independent ledgers fit.
    receipt.validations = validations;
    if ambiguous {
        receipt.alternatives = ledgers;
        Ok(Outcome::Ambiguous(values))
    } else {
        let ledger = ledgers.pop().expect("one accepted interpretation");
        receipt.evidence = ledger.evidence;
        receipt.inferences = ledger.inferences;
        Ok(Outcome::Resolved(
            values.pop().expect("one accepted interpretation"),
        ))
    }
}
fn derived(answer: &Selection) -> Vec<Inference> {
    answer
        .value
        .desired
        .iter()
        .map(|(key, value)| {
            let destination = format!("/desired/{}", crate::json::pointer_segment(key));
            let evidence = answer
                .evidence
                .iter()
                .rev()
                .find(|entry| entry.destination == destination)
                .expect("every desired attribute came from a recorded rule")
                .clone();
            Inference {
                evidence,
                kind: InferenceKind::Derived,
                value: crate::wire::string(value).to_json(),
            }
        })
        .collect()
}
