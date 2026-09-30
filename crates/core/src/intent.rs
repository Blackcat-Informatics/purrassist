// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Caller-owned semantic rules: explicit concepts, modifiers, and auditable resolution.
use crate::contract::saturating_size as size;
use crate::{
    AssistResult, Budget, Effort, Governor, Outcome, Receipt, StopReason,
    contract::{Evidence, Resource},
};
use std::collections::{BTreeMap, BTreeSet};
mod selection;
mod wire;
use selection::Selection;

/// A candidate's domain meaning, supplied explicitly by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Concept {
    /// Stable identity returned without normalization.
    pub id: String,
    /// Display text for the selected concept.
    pub label: String,
    /// Canonical attribute/value assertions about this concept.
    pub attributes: BTreeMap<String, String>,
}
/// A lexical phrase and its declared effect on the desired meaning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// Versioned, caller-owned rule identity.
    pub id: String,
    /// Phrase interpreted with Unicode normalization and word tokenization.
    pub phrase: String,
    /// Assignments; later phrases overwrite earlier values for the same attribute.
    pub assignments: BTreeMap<String, String>,
}
/// Explicit domain vocabulary; no model, network, or undeclared world knowledge.
#[derive(Clone, Debug, Default)]
pub struct Domain {
    /// Versioned identity recorded in every resolution receipt.
    pub id: String,
    /// Phrase rules. Longer phrases take precedence over overlapping shorter ones.
    pub rules: Vec<Rule>,
    /// Words explicitly permitted without semantic effects, normalized at evaluation.
    pub ignored_words: BTreeSet<String>,
}
/// A selected concept together with the attributes inferred from the request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interpretation {
    /// Caller-owned concept, unchanged.
    pub concept: Concept,
    /// Constraints derived by the explicit rules.
    pub desired: BTreeMap<String, String>,
}
#[derive(Clone)]
struct State {
    desired: BTreeMap<String, String>,
    evidence: Vec<Evidence>,
}

impl Domain {
    /// Resolve a request against caller-supplied concepts.
    ///
    /// Unknown words fail explicitly, preventing silent loss of modifiers such as
    /// negations or quantities. Equal-length conflicting phrase rules branch;
    /// multiple matching concepts remain ambiguous. Later modifiers replace earlier
    /// assignments, and receipts identify both rules. Scores are not probabilities.
    /// Ambiguous outputs have ordered alternative ledgers; incompatible decisions
    /// are never combined in the top-level ledger. Identical outputs reached by
    /// multiple derivations retain the first supporting derivation in rule order.
    #[must_use]
    pub fn resolve(
        &self,
        query: &str,
        concepts: &[Concept],
        budget: Budget,
    ) -> AssistResult<Interpretation> {
        self.resolve_governed(query, concepts, &mut Governor::new(budget))
    }
    /// Resolve with a governor shared with caller-side domain preparation.
    ///
    /// Exact query bytes are retained after admission. Invalid vocabulary and
    /// exhausted budgets produce `Unresolved` with an explicit receipt reason.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, intent::Domain};
    /// let mut governor = Governor::new(Budget::default());
    /// let result = Domain::default().resolve_governed("unknown", &[], &mut governor);
    /// assert_eq!(result.outcome, Outcome::Unresolved);
    /// assert_eq!(result.receipt.sources[0].text, "unknown");
    /// ```
    #[must_use]
    pub fn resolve_governed(
        &self,
        query: &str,
        concepts: &[Concept],
        governor: &mut Governor,
    ) -> AssistResult<Interpretation> {
        let mut receipt = Receipt::new(
            query.as_bytes(),
            "intent.resolve.v1",
            Effort::Standard,
            *governor.limits(),
        );
        let result = receipt
            .retain_source(query, governor)
            .and_then(|()| self.resolve_with(query, concepts, governor));
        let outcome = match result {
            Ok(answers) => match selection::finish(answers, &mut receipt, governor) {
                Ok(outcome) => outcome,
                Err(reason) => {
                    receipt.stopped = reason;
                    Outcome::Unresolved
                }
            },
            Err(reason) => {
                receipt.stopped = reason;
                Outcome::Unresolved
            }
        };
        receipt.usage = governor.usage();
        AssistResult { outcome, receipt }
    }
    fn resolve_with(
        &self,
        query: &str,
        concepts: &[Concept],
        governor: &mut Governor,
    ) -> Result<Vec<Selection>, StopReason> {
        governor.charge(Resource::InputBytes, size(query.len()))?;
        governor.charge(
            Resource::ScratchBytes,
            size(query.len()).saturating_mul(128),
        )?;
        governor.charge(Resource::Work, size(query.len()))?;
        let query = crate::text::comparison_key(query);
        let tokens = crate::text::lexical_tokens(&query);
        let mut identities = BTreeSet::new();
        let mut phrases = Vec::new();
        for rule in &self.rules {
            governor.charge(Resource::Work, size(rule.phrase.len()))?;
            governor.charge(
                Resource::ScratchBytes,
                size(rule.phrase.len()).saturating_mul(128),
            )?;
            if !identities.insert(&rule.id) {
                return Err(StopReason::InvalidInput(
                    "duplicate semantic rule identity".into(),
                ));
            }
            let phrase = crate::text::comparison_key(&rule.phrase);
            let words: Vec<String> = crate::text::lexical_tokens(&phrase);
            if words.is_empty() {
                return Err(StopReason::InvalidInput(
                    "semantic rule phrase has no words".into(),
                ));
            }
            phrases.push(words);
        }
        let ignored: BTreeSet<_> = self
            .ignored_words
            .iter()
            .map(|word| crate::text::comparison_key(word))
            .collect();
        let mut states = vec![State {
            desired: BTreeMap::new(),
            evidence: Vec::new(),
        }];
        let mut position = 0;
        while position < tokens.len() {
            let mut matches = Vec::new();
            let mut longest = 0;
            for (rule, phrase) in self.rules.iter().zip(&phrases) {
                governor.charge(Resource::Work, size(phrase.len()))?;
                if tokens
                    .get(position..position.saturating_add(phrase.len()))
                    .is_some_and(|window| {
                        window.iter().zip(phrase).all(|(left, right)| left == right)
                    })
                {
                    if phrase.len() > longest {
                        matches.clear();
                        longest = phrase.len();
                    }
                    if phrase.len() == longest {
                        matches.push(rule);
                    }
                }
            }
            if matches.is_empty() {
                if !ignored.contains(&tokens[position]) {
                    return Err(StopReason::InvalidInput(format!(
                        "unrecognized semantic token {:?}; define a rule or explicitly ignore it",
                        tokens[position]
                    )));
                }
                position += 1;
                continue;
            }
            states = self.apply_rules(&states, &matches, governor)?;
            position += longest;
        }
        selection::select(concepts, states, governor)
    }
    fn apply_rules(
        &self,
        states: &[State],
        rules: &[&Rule],
        governor: &mut Governor,
    ) -> Result<Vec<State>, StopReason> {
        let mut next = Vec::new();
        for state in states {
            for rule in rules {
                governor.charge(Resource::Candidates, 1)?;
                selection::admit_branch(state, rule, &self.id, governor)?;
                let mut state = state.clone();
                for (attribute, value) in &rule.assignments {
                    governor.charge(Resource::EvidenceEntries, 1)?;
                    let previous = state.desired.insert(attribute.clone(), value.clone());
                    state.evidence.push(Evidence {
                        rule: format!("{}::{}", self.id, rule.id),
                        destination: format!(
                            "/desired/{}",
                            crate::json::pointer_segment(attribute)
                        ),
                        span: None,
                        explanation: format!(
                            "phrase {:?} assigns {value:?}; prior assignment {previous:?}",
                            rule.phrase
                        ),
                    });
                }
                next.push(state);
            }
        }
        Ok(next)
    }
}

/// Caller-supplied vector similarity. No embedding model is loaded by the core.
///
/// # Errors
/// Rejects dimension mismatch, empty/zero vectors, non-finite values, and exhausted work.
pub fn cosine_similarity(
    left: &[f64],
    right: &[f64],
    governor: &mut Governor,
) -> Result<f64, StopReason> {
    if left.is_empty() || left.len() != right.len() {
        return Err(StopReason::InvalidInput(
            "embedding dimensions must be equal and nonzero".into(),
        ));
    }
    governor.charge(Resource::Work, size(left.len()).saturating_mul(3))?;
    if left.iter().chain(right).any(|value| !value.is_finite()) {
        return Err(StopReason::InvalidInput(
            "embedding contains non-finite values".into(),
        ));
    }
    let a = left
        .iter()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    let b = right
        .iter()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    if a == 0.0 || b == 0.0 {
        return Err(StopReason::InvalidInput(
            "zero embedding has no direction".into(),
        ));
    }
    let (mut dot, mut norm_a, mut norm_b) = (0.0, 0.0, 0.0);
    for (x, y) in left.iter().zip(right) {
        let x = x / a;
        let y = y / b;
        dot = x.mul_add(y, dot);
        norm_a = x.mul_add(x, norm_a);
        norm_b = y.mul_add(y, norm_b);
    }
    Ok((dot / (norm_a.sqrt() * norm_b.sqrt())).clamp(-1.0, 1.0))
}
