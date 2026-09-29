// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-bound text edits with overlap checks, conditional inverse plans, and receipts.
//!
//! `EditPlan` and `TextEdit` are aliases for [`crate::patch::Patch`] and
//! [`crate::patch::TextEdit`]: the crate's single edit representation, whose
//! `apply`/`invert`/`compose`/`commute`/`rebase` algebra lives in
//! [`crate::patch`]. This module keeps the exact/fuzzy/unified planning
//! operations that produce one.
use crate::{
    AssistResult, Budget, Effort, Governor, Outcome, Receipt, StopReason,
    contract::{Evidence, Resource, SourceSpan},
    digest::Digest,
};
mod fuzzy;
pub use fuzzy::{locate_fuzzy, locate_fuzzy_governed};
mod unified;
pub use unified::{plan_unified, plan_unified_governed};

pub use crate::patch::{AppliedEdit, Patch, TextEdit};
/// The crate's single edit representation (see [`crate::patch::Patch`]);
/// every exact/fuzzy/unified/Markdown planning operation in this crate
/// produces one.
pub type EditPlan = Patch;

/// Locate exact replacement regions without choosing arbitrarily among duplicates.
/// Empty needles are rejected; explicit byte-range edits support insertion.
#[must_use]
pub fn locate(
    source: &str,
    needle: &str,
    replacement: &str,
    budget: Budget,
) -> AssistResult<EditPlan> {
    locate_governed(source, needle, replacement, &mut Governor::new(budget))
}

/// Run the same planning operation with cumulative caller-owned limits and cancellation.
///
/// Returns `Unresolved` on cancellation or exhaustion; the receipt records the
/// governor's effective limits and cumulative usage. Completed plans remain
/// proposals and perform no filesystem effects.
///
/// ```
/// use purrassist_core::{Budget, Governor, Outcome, StopReason, edits};
/// let mut governor = Governor::new(Budget::default());
/// governor.cancel();
/// let result = edits::locate_governed("tea", "tea", "coffee", &mut governor);
/// assert!(matches!(result.outcome, Outcome::Unresolved));
/// assert_eq!(result.receipt.stopped, StopReason::Cancelled);
/// ```
#[must_use]
pub fn locate_governed(
    source: &str,
    needle: &str,
    replacement: &str,
    governor: &mut Governor,
) -> AssistResult<EditPlan> {
    let budget = *governor.limits();
    let mut receipt = Receipt::new(source.as_bytes(), "text.locate.v1", Effort::Exact, budget);
    let result = (|| {
        receipt.retain_source(source, governor)?;
        governor.charge(Resource::InputBytes, size(source.len()))?;
        governor.charge(
            Resource::Work,
            size(source.len()).saturating_mul(size(needle.len().max(1))),
        )?;
        if needle.is_empty() {
            return Err(StopReason::InvalidInput(
                "replacement needle must not be empty".into(),
            ));
        }
        let mut plans = Vec::new();
        // Include overlapping occurrences; match_indices alone omits them.
        for (position, _) in source.char_indices() {
            governor.check()?;
            if source[position..].starts_with(needle) {
                governor.charge(Resource::Candidates, 1)?;
                governor.charge(
                    Resource::ScratchBytes,
                    size(replacement.len()).saturating_add(128),
                )?;
                let range = position..position + needle.len();
                governor.charge(Resource::EvidenceEntries, 1)?;
                governor.charge(
                    Resource::ScratchBytes,
                    size(needle.len()).saturating_mul(4).saturating_add(256),
                )?;
                let evidence = Evidence {
                    rule: "text.locate.v1".into(),
                    explanation: format!("exact occurrence of {needle:?}"),
                    span: Some(SourceSpan {
                        source: receipt.source,
                        bytes: range.clone(),
                    }),
                    destination: format!("bytes:{}..{}", range.start, range.end),
                };
                plans.push((
                    EditPlan {
                        source: receipt.source,
                        edits: vec![TextEdit {
                            range,
                            replacement: replacement.into(),
                        }],
                    },
                    evidence,
                ));
            }
        }
        Ok(plans)
    })();
    let outcome = match result {
        Ok(mut plans) => match plans.len() {
            0 => {
                receipt.stopped = StopReason::NoCandidate;
                Outcome::Unresolved
            }
            1 => {
                let (plan, evidence) = plans.remove(0);
                receipt.evidence.push(evidence);
                Outcome::Resolved(plan)
            }
            _ => match populate_locate_alternatives(plans, governor, &mut receipt) {
                Ok(plans) => Outcome::Ambiguous(plans),
                Err(reason) => {
                    receipt.stopped = reason;
                    Outcome::Unresolved
                }
            },
        },
        Err(reason) => {
            receipt.stopped = reason;
            Outcome::Unresolved
        }
    };
    receipt.usage = governor.usage();
    AssistResult { outcome, receipt }
}
/// Build one derivation per candidate occurrence and publish shared vs.
/// per-occurrence evidence through the derivation forest.
///
/// # Errors
/// Returns `BudgetExceeded` before any ledger is committed to `receipt`.
fn populate_locate_alternatives(
    plans: Vec<(EditPlan, Evidence)>,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<Vec<EditPlan>, StopReason> {
    let mut forest = crate::provenance::Forest::new(governor);
    let mut derivations = Vec::with_capacity(plans.len());
    let mut outputs = Vec::with_capacity(plans.len());
    for (plan, evidence) in plans {
        let output = Digest::of(plan.to_node().to_json().as_bytes());
        let mut derivation = crate::provenance::Derivation::new(output, 0);
        derivation.push(forest.evidence(governor, evidence)?);
        derivations.push(derivation);
        outputs.push(plan);
    }
    let published = forest.publish(&derivations, governor)?;
    receipt.evidence = published.evidence;
    receipt.alternatives = published.alternatives;
    Ok(outputs)
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
