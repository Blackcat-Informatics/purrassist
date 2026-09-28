// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Shared assistance, resource-accounting, and evidence contracts.

use crate::digest::Digest;
use core::fmt;
use core::ops::Range;

/// Version of the machine-readable receipt contract.
pub const RECEIPT_VERSION: u32 = 1;

/// Increasing recovery effort; all presets remain finite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effort {
    /// Validate exact input, without generating repairs.
    Exact,
    /// Resolve simple local mistakes.
    Quick,
    /// Search composed repairs with ordinary resource bounds.
    #[default]
    Standard,
    /// Explore a wider set of competing interpretations.
    Deep,
    /// Use the largest standard finite search budget.
    Maximum,
}

impl Effort {
    /// Reproducible default limits; callers may provide explicit limits instead.
    #[must_use]
    pub const fn budget(self) -> Budget {
        let (work, candidates) = match self {
            Self::Exact => (100_000, 1),
            Self::Quick => (250_000, 16),
            Self::Standard => (1_000_000, 128),
            Self::Deep => (10_000_000, 1024),
            Self::Maximum => (50_000_000, 8192),
        };
        Budget {
            work,
            candidates,
            input_bytes: 16 * 1024 * 1024,
            scratch_bytes: 64 * 1024 * 1024,
            depth: 128,
            evidence_entries: 65_536,
            helper_calls: 0,
        }
    }
}

/// Explicit limits. Helper access is disabled until the caller budgets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Maximum algorithmic work units.
    pub work: u64,
    /// Maximum candidate proposals.
    pub candidates: u64,
    /// Maximum bytes admitted as one source.
    pub input_bytes: u64,
    /// Maximum simultaneously reserved scratch bytes.
    pub scratch_bytes: u64,
    /// Maximum structural nesting depth.
    pub depth: u32,
    /// Maximum evidence entries.
    pub evidence_entries: u64,
    /// Maximum externally fulfilled helper requests.
    pub helper_calls: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Effort::Standard.budget()
    }
}

/// A charged resource dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    /// Algorithmic work.
    Work,
    /// Proposed candidates.
    Candidates,
    /// Admitted source bytes.
    InputBytes,
    /// Live scratch storage.
    ScratchBytes,
    /// Structural recursion depth.
    Depth,
    /// Audit evidence entries.
    EvidenceEntries,
    /// External helper invocations.
    HelperCalls,
}

/// Why an operation stopped; exhaustion never asserts unsatisfiability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The requested operation completed.
    Completed,
    /// The finite candidate space examined produced no acceptable result.
    NoCandidate,
    /// Continuing would exceed the stated resource limit.
    BudgetExceeded(Resource),
    /// The caller cancelled the operation.
    Cancelled,
    /// Input or configuration cannot be interpreted under its contract.
    InvalidInput(String),
}

impl fmt::Display for StopReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completed => formatter.write_str("completed"),
            Self::NoCandidate => formatter.write_str("no acceptable candidate found"),
            Self::BudgetExceeded(resource) => write!(formatter, "budget exceeded: {resource:?}"),
            Self::Cancelled => formatter.write_str("cancelled"),
            Self::InvalidInput(detail) => write!(formatter, "invalid input: {detail}"),
        }
    }
}

impl std::error::Error for StopReason {}

/// Measured consumption, distinct from configured bounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Charged work units.
    pub work: u64,
    /// Candidate proposals charged.
    pub candidates: u64,
    /// High-water scratch reservation.
    pub peak_scratch_bytes: u64,
    /// Evidence entries recorded.
    pub evidence_entries: u64,
    /// Helper requests charged.
    pub helper_calls: u64,
}

/// A sticky governor: after exhaustion or cancellation, every charge fails.
#[derive(Clone, Debug)]
pub struct Governor {
    limits: Budget,
    usage: Usage,
    scratch: u64,
    stopped: Option<StopReason>,
}

impl Governor {
    /// Start an operation with explicit limits.
    #[must_use]
    pub const fn new(limits: Budget) -> Self {
        Self {
            limits,
            usage: Usage {
                work: 0,
                candidates: 0,
                peak_scratch_bytes: 0,
                evidence_entries: 0,
                helper_calls: 0,
            },
            scratch: 0,
            stopped: None,
        }
    }

    /// The effective configured limits.
    #[must_use]
    pub const fn limits(&self) -> &Budget {
        &self.limits
    }

    /// Consumption measured so far.
    #[must_use]
    pub const fn usage(&self) -> Usage {
        self.usage
    }

    /// Cancel future work without changing completed evidence.
    pub fn cancel(&mut self) {
        self.stopped = Some(StopReason::Cancelled);
    }

    /// Check whether the operation can proceed.
    ///
    /// # Errors
    /// Returns the first exhaustion reason, or cancellation.
    pub fn check(&self) -> Result<(), StopReason> {
        self.stopped.clone().map_or(Ok(()), Err)
    }

    /// Charge before performing work or allocating reserved storage.
    /// `InputBytes` and `Depth` check a single admission, rather than accumulate.
    ///
    /// # Errors
    /// Returns a sticky exhaustion or cancellation without incrementing usage.
    pub fn charge(&mut self, resource: Resource, amount: u64) -> Result<(), StopReason> {
        self.check()?;
        let (current, limit) = match resource {
            Resource::Work => (self.usage.work, self.limits.work),
            Resource::Candidates => (self.usage.candidates, self.limits.candidates),
            Resource::InputBytes => (0, self.limits.input_bytes),
            Resource::ScratchBytes => (self.scratch, self.limits.scratch_bytes),
            Resource::Depth => (0, u64::from(self.limits.depth)),
            Resource::EvidenceEntries => {
                (self.usage.evidence_entries, self.limits.evidence_entries)
            }
            Resource::HelperCalls => (self.usage.helper_calls, self.limits.helper_calls),
        };
        let Some(next) = current.checked_add(amount).filter(|next| *next <= limit) else {
            let reason = StopReason::BudgetExceeded(resource);
            self.stopped = Some(reason.clone());
            return Err(reason);
        };
        match resource {
            Resource::Work => self.usage.work = next,
            Resource::Candidates => self.usage.candidates = next,
            Resource::ScratchBytes => {
                self.scratch = next;
                self.usage.peak_scratch_bytes = self.usage.peak_scratch_bytes.max(next);
            }
            Resource::EvidenceEntries => self.usage.evidence_entries = next,
            Resource::HelperCalls => self.usage.helper_calls = next,
            Resource::Depth | Resource::InputBytes => {}
        }
        Ok(())
    }

    /// Release a previous scratch reservation.
    ///
    /// # Errors
    /// Rejects an unbalanced release, preserving the current reservation.
    pub fn release_scratch(&mut self, amount: u64) -> Result<(), StopReason> {
        let Some(next) = self.scratch.checked_sub(amount) else {
            return Err(StopReason::InvalidInput(
                "unbalanced scratch reservation".into(),
            ));
        };
        self.scratch = next;
        Ok(())
    }
}

/// Exact source identity and a half-open byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    /// Digest of the complete original source.
    pub source: Digest,
    /// Byte range in that source.
    pub bytes: Range<usize>,
}

/// One attributable correction or selection decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    /// Stable rule identifier.
    pub rule: String,
    /// Explanation grounded in the performed operation.
    pub explanation: String,
    /// Exact original location when the operation has one.
    pub span: Option<SourceSpan>,
    /// Location in the result, usually a JSON Pointer or structural selector.
    pub destination: String,
}

/// Information removed or irreversibly transformed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loss {
    /// The rule responsible for the loss.
    pub evidence: Evidence,
    /// Original information, retained verbatim or as its exact source span.
    pub original: String,
}

/// How an output value was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferenceKind {
    /// Derived from identified input, schema, or domain evidence.
    Derived,
    /// Invented to construct a candidate satisfying constraints.
    Guess,
}

/// An inferred or invented result value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inference {
    /// Supporting evidence, including the result location.
    pub evidence: Evidence,
    /// Distinguishes grounding from speculation.
    pub kind: InferenceKind,
    /// The supplied result value in its documented representation.
    pub value: String,
}

/// Evidence of an actual validation performed on a candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validation {
    /// Validator implementation and contract identity.
    pub validator: String,
    /// Constraint identity or human-readable constraint description.
    pub constraint: String,
    /// Whether the validation succeeded.
    pub passed: bool,
    /// Validation diagnostics.
    pub diagnostics: Vec<String>,
}

/// Exact source retained so transformation evidence can be replayed and inspected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDocument {
    /// Identity of these exact UTF-8 bytes.
    pub digest: Digest,
    /// Complete source before recovery, including whitespace and malformed syntax.
    pub text: String,
}

/// Evidence belonging to one competing JSON recovery interpretation.
///
/// Records follow the order of `Outcome::Ambiguous`. Their output identities hash
/// `Node::to_json()` bytes, not the original input or an equivalence-normalized JSON
/// value. Validation indices refer to the enclosing receipt's validation log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlternativeReceipt {
    /// Digest of this alternative's serialized JSON output.
    pub output: Digest,
    /// Recovery search cost; comparable within this operation only.
    pub cost: u64,
    /// Transformations that produced this alternative.
    pub evidence: Vec<Evidence>,
    /// Information discarded by this alternative alone.
    pub losses: Vec<Loss>,
    /// Derived or invented values belonging to this alternative.
    pub inferences: Vec<Inference>,
    /// Indices of validations actually performed on this output.
    pub validations: Vec<usize>,
}

/// An ordered, versioned audit receipt. Ledgers contain no hidden model reasoning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// Receipt representation version.
    pub version: u32,
    /// Original input identity.
    pub source: Digest,
    /// Retained recovery inputs, including helper proposal sources when applicable.
    /// Empty if admission failed before retention or this operation only ranks names.
    pub sources: Vec<SourceDocument>,
    /// Stable operation/profile identifier.
    pub operation: String,
    /// Requested effort preset.
    pub effort: Effort,
    /// Effective resource limits.
    pub budget: Budget,
    /// Measured consumption.
    pub usage: Usage,
    /// Recorded transformations and decisions in execution order.
    pub evidence: Vec<Evidence>,
    /// Discarded or irreversibly changed information.
    pub losses: Vec<Loss>,
    /// Derived and guessed output values.
    pub inferences: Vec<Inference>,
    /// Actual validation results.
    pub validations: Vec<Validation>,
    /// Per-alternative JSON recovery ledgers in outcome order. Empty for other
    /// operations or a single answer. Mutually exclusive transformations stay here
    /// rather than being combined into the top-level selected-result ledgers.
    pub alternatives: Vec<AlternativeReceipt>,
    /// Why processing stopped.
    pub stopped: StopReason,
}

impl Receipt {
    /// Begin a receipt tied to exact source bytes and an explicit profile.
    #[must_use]
    pub fn new(source: &[u8], operation: &str, effort: Effort, budget: Budget) -> Self {
        Self {
            version: RECEIPT_VERSION,
            source: Digest::of(source),
            sources: Vec::new(),
            operation: operation.into(),
            effort,
            budget,
            usage: Usage::default(),
            evidence: Vec::new(),
            losses: Vec::new(),
            inferences: Vec::new(),
            validations: Vec::new(),
            alternatives: Vec::new(),
            stopped: StopReason::Completed,
        }
    }

    /// Retain exact input bytes under the operation's shared resource bounds.
    ///
    /// # Errors
    /// Returns exhaustion before copying the source. Rejected oversized input is
    /// identified by the receipt digest but must remain owned by the caller.
    pub fn retain_source(
        &mut self,
        source: &str,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let length = u64::try_from(source.len()).unwrap_or(u64::MAX);
        governor.charge(Resource::InputBytes, length)?;
        governor.charge(Resource::Work, length)?;
        governor.charge(Resource::ScratchBytes, length.saturating_add(128))?;
        self.sources.push(SourceDocument {
            digest: Digest::of(source.as_bytes()),
            text: source.into(),
        });
        Ok(())
    }

    /// Whether any selected result or competing alternative was explicitly guessed.
    #[must_use]
    pub fn contains_guesses(&self) -> bool {
        self.inferences
            .iter()
            .any(|entry| entry.kind == InferenceKind::Guess)
            || self.alternatives.iter().any(|alternative| {
                alternative
                    .inferences
                    .iter()
                    .any(|entry| entry.kind == InferenceKind::Guess)
            })
    }
}

/// An answer's epistemic status, independently of schema or syntax validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    /// A preferred result supported by the recorded evidence.
    Resolved(T),
    /// A constraint-satisfying answer containing speculative information.
    BestGuess(T),
    /// Competing candidates require caller disambiguation.
    Ambiguous(Vec<T>),
    /// No acceptable answer was obtained within the operation's contract.
    Unresolved,
}

/// The answer together with its evidence; receipts also accompany failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistResult<T> {
    /// Result and its epistemic status.
    pub outcome: Outcome<T>,
    /// Source provenance, corrections, losses, inferences, and validation.
    pub receipt: Receipt,
}

#[cfg(test)]
mod tests {
    use super::{Budget, Governor, Resource, StopReason};

    #[test]
    fn exhaustion_is_sticky_and_never_overcharges() {
        let mut governor = Governor::new(Budget {
            work: 2,
            ..Budget::default()
        });
        assert_eq!(governor.charge(Resource::Work, 2), Ok(()));
        let failure = Err(StopReason::BudgetExceeded(Resource::Work));
        assert_eq!(governor.charge(Resource::Work, 1), failure);
        assert_eq!(governor.charge(Resource::Candidates, 0), failure);
        assert_eq!(governor.usage().work, 2);
    }

    #[test]
    fn scratch_tracks_live_and_high_water_usage() {
        let mut governor = Governor::new(Budget {
            scratch_bytes: 10,
            ..Budget::default()
        });
        assert_eq!(governor.charge(Resource::ScratchBytes, 8), Ok(()));
        assert_eq!(governor.release_scratch(8), Ok(()));
        assert_eq!(governor.charge(Resource::ScratchBytes, 7), Ok(()));
        assert_eq!(governor.usage().peak_scratch_bytes, 8);
        assert!(governor.release_scratch(9).is_err());
        assert!(governor.charge(Resource::ScratchBytes, 4).is_err());
    }
}
