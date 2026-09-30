// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Caller-owned embedding index; no model, transport, or ambient state.
use crate::contract::invalid_input as invalid;
use crate::contract::saturating_size as size;
use crate::{Governor, Outcome, StopReason, contract::Resource};
use std::collections::BTreeSet;

/// Named vector supplied by the caller in one declared embedding space.
#[derive(Clone, Debug)]
pub struct Embedding {
    /// Unique, nonempty candidate identity.
    pub id: String,
    /// Finite, nonzero vector coordinates.
    pub vector: Vec<f64>,
}
/// A candidate's cosine score; this is not a probability or semantic proof.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddingMatch {
    /// Original candidate identity.
    pub id: String,
    /// Cosine similarity in the inclusive range -1 through 1.
    pub score: f64,
}
/// Explicit thresholds for numerical candidate selection.
#[derive(Clone, Copy, Debug)]
pub struct EmbeddingPolicy {
    /// Minimum cosine score in [-1, 1].
    pub minimum_score: f64,
    /// Candidates within this margin of the best score remain ambiguous, including ties.
    pub minimum_margin: f64,
}
impl Default for EmbeddingPolicy {
    fn default() -> Self {
        Self {
            minimum_score: 0.75,
            minimum_margin: 0.05,
        }
    }
}
/// Immutable, normalized vectors for repeated model-free matching.
///
/// The caller is responsible for declaring a consistent embedding space.
/// This low-level engine returns numerical selections; shared host tools add receipts.
#[derive(Clone, Debug)]
pub struct EmbeddingIndex {
    space: String,
    dimensions: usize,
    entries: Vec<Embedding>,
}
impl EmbeddingIndex {
    /// Validate identities/dimensions and normalize each supplied vector once.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, embeddings::{Embedding, EmbeddingIndex, EmbeddingPolicy}, Outcome};
    /// let mut g = Governor::new(Budget::default());
    /// let index = EmbeddingIndex::new("caller-space-v1", &[Embedding { id: "tea".into(), vector: vec![1.0, 0.0] }], &mut g)?;
    /// assert!(matches!(index.select(&[1.0, 0.0], EmbeddingPolicy::default(), &mut g)?, Outcome::BestGuess(_)));
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects empty spaces/identities, duplicate identities, mismatched dimensions,
    /// empty/zero/nonfinite vectors, and resource exhaustion or cancellation.
    pub fn new(
        space: &str,
        candidates: &[Embedding],
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        governor.charge(Resource::InputBytes, size(space.len()))?;
        governor.charge(Resource::Work, size(space.len()).saturating_add(1))?;
        governor.charge(Resource::ScratchBytes, size(space.len()))?;
        if space.is_empty() {
            return Err(invalid("embedding space must be named"));
        }
        let mut result = Self {
            space: space.into(),
            dimensions: candidates.first().map_or(0, |entry| entry.vector.len()),
            entries: Vec::new(),
        };
        let mut seen = BTreeSet::new();
        for entry in candidates {
            governor.charge(
                Resource::InputBytes,
                size(entry.id.len()).saturating_add(size(entry.vector.len()).saturating_mul(8)),
            )?;
            governor.charge(
                Resource::Work,
                size(entry.id.len())
                    .saturating_mul(size(candidates.len()).max(1).ilog2().into())
                    .saturating_add(1),
            )?;
            governor.charge(
                Resource::ScratchBytes,
                size(entry.id.len()).saturating_mul(2).saturating_add(128),
            )?;
            if entry.id.is_empty() || !seen.insert(entry.id.as_str()) {
                return Err(invalid("embedding identities must be nonempty and unique"));
            }
            if entry.vector.len() != result.dimensions {
                return Err(invalid("embedding dimensions differ"));
            }
            result.entries.push(Embedding {
                id: entry.id.clone(),
                vector: unit(&entry.vector, governor)?,
            });
        }
        Ok(result)
    }
    /// Caller-declared vector space identity.
    #[must_use]
    pub fn space(&self) -> &str {
        &self.space
    }
    /// Score every admitted candidate and select under explicit thresholds.
    ///
    /// A unique numerical winner is always a best guess about semantic intent.
    /// Equal/near scores remain ambiguous in original input order. Exhaustion
    /// never returns a partially searched winner. No qualifying candidates yields unresolved.
    ///
    /// # Errors
    /// Rejects invalid thresholds or query vectors and reports resource exhaustion.
    pub fn select(
        &self,
        query: &[f64],
        policy: EmbeddingPolicy,
        governor: &mut Governor,
    ) -> Result<Outcome<EmbeddingMatch>, StopReason> {
        governor.check()?;
        if !policy.minimum_score.is_finite()
            || !(-1.0..=1.0).contains(&policy.minimum_score)
            || !policy.minimum_margin.is_finite()
            || !(0.0..=2.0).contains(&policy.minimum_margin)
        {
            return Err(invalid("invalid embedding selection thresholds"));
        }
        let query = unit(query, governor)?;
        if !self.entries.is_empty() && query.len() != self.dimensions {
            return Err(invalid("query dimensions differ from index"));
        }
        governor.charge(Resource::Candidates, size(self.entries.len()))?;
        governor.charge(
            Resource::Work,
            size(self.entries.len()).saturating_mul(size(query.len()).saturating_add(2)),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.entries.len()).saturating_mul(64),
        )?;
        let mut scored = Vec::new();
        let mut best = -1.0_f64;
        for entry in &self.entries {
            let score = query
                .iter()
                .zip(&entry.vector)
                .fold(0.0_f64, |sum, (a, b)| a.mul_add(*b, sum))
                .clamp(-1.0, 1.0);
            if score >= policy.minimum_score {
                best = best.max(score);
                scored.push((entry, score));
            }
        }
        let mut matches = Vec::new();
        for (entry, score) in scored {
            if best - score <= policy.minimum_margin {
                governor.charge(
                    Resource::ScratchBytes,
                    size(entry.id.len()).saturating_add(64),
                )?;
                matches.push(EmbeddingMatch {
                    id: entry.id.clone(),
                    score,
                });
            }
        }
        Ok(if matches.len() > 1 {
            Outcome::Ambiguous(matches)
        } else {
            matches
                .pop()
                .map_or(Outcome::Unresolved, Outcome::BestGuess)
        })
    }
}
fn unit(values: &[f64], governor: &mut Governor) -> Result<Vec<f64>, StopReason> {
    governor.charge(Resource::InputBytes, size(values.len()).saturating_mul(8))?;
    governor.charge(
        Resource::Work,
        size(values.len()).saturating_mul(4).saturating_add(1),
    )?;
    governor.charge(Resource::ScratchBytes, size(values.len()).saturating_mul(8))?;
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return Err(invalid("embedding must be nonempty and finite"));
    }
    let scale = values
        .iter()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    if scale == 0.0 {
        return Err(invalid("zero embedding has no direction"));
    }
    let mut result: Vec<_> = values.iter().map(|value| value / scale).collect();
    let norm = result
        .iter()
        .fold(0.0_f64, |sum, value| value.mul_add(*value, sum))
        .sqrt();
    for value in &mut result {
        *value /= norm;
    }
    Ok(result)
}
