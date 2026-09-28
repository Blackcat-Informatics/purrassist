// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-bound text edits with overlap checks, conditional inverse plans, and receipts.
use crate::{
    AssistResult, Budget, Effort, Governor, Outcome, Receipt, StopReason,
    contract::{Evidence, Loss, Resource, SourceSpan, Validation},
    digest::Digest,
};
use std::ops::Range;
mod fuzzy;
pub use fuzzy::locate_fuzzy;
mod unified;
pub use unified::plan_unified;

/// Replace exactly one half-open UTF-8 byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    /// Byte range in the original source; insertion uses an empty range.
    pub range: Range<usize>,
    /// Text inserted verbatim.
    pub replacement: String,
}
/// Immutable proposal bound to the exact bytes from which its ranges were derived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditPlan {
    /// Expected original source digest.
    pub source: Digest,
    /// Independent edits, interpreted against the same original source.
    pub edits: Vec<TextEdit>,
}
/// Applied output and a conditional inverse bound to that exact output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedEdit {
    /// Complete updated source.
    pub text: String,
    /// Restores the exact original only while the updated source remains unchanged.
    pub inverse: EditPlan,
}
impl EditPlan {
    /// Apply atomically in memory. No output is returned on stale, overlapping,
    /// out-of-bounds, non-UTF-8-boundary, or exhausted edits.
    #[must_use]
    pub fn apply(&self, source: &str, budget: Budget) -> AssistResult<AppliedEdit> {
        self.apply_governed(source, &mut Governor::new(budget))
    }
    /// Apply using a governor shared with syntax validation or a larger operation.
    #[must_use]
    pub fn apply_governed(
        &self,
        source: &str,
        governor: &mut Governor,
    ) -> AssistResult<AppliedEdit> {
        let budget = *governor.limits();
        let mut receipt = Receipt::new(source.as_bytes(), "text.apply.v1", Effort::Exact, budget);
        let outcome = match self.apply_with(source, governor, &mut receipt) {
            Ok(value) => Outcome::Resolved(value),
            Err(reason) => {
                receipt.stopped = reason;
                Outcome::Unresolved
            }
        };
        receipt.usage = governor.usage();
        AssistResult { outcome, receipt }
    }
    fn apply_with(
        &self,
        source: &str,
        governor: &mut Governor,
        receipt: &mut Receipt,
    ) -> Result<AppliedEdit, StopReason> {
        receipt.retain_source(source, governor)?;
        governor.charge(Resource::Work, size(source.len()))?;
        if self.source != receipt.source {
            return Err(StopReason::InvalidInput(
                "stale edit plan: source digest changed".into(),
            ));
        }
        let mut edits: Vec<_> = self.edits.iter().collect();
        governor.charge(
            Resource::Work,
            size(edits.len()).saturating_mul(size(edits.len().max(1).ilog2() as usize + 1)),
        )?;
        edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
        let mut length = source.len();
        let mut previous: Option<&TextEdit> = None;
        for edit in &edits {
            if edit.range.start > edit.range.end
                || !source.is_char_boundary(edit.range.start)
                || !source.is_char_boundary(edit.range.end)
            {
                return Err(StopReason::InvalidInput(
                    "edit range is outside source or splits a UTF-8 scalar".into(),
                ));
            }
            if previous.is_some_and(|prior| {
                let overlaps = prior.range.end > edit.range.start;
                let same_start = prior.range.start == edit.range.start;
                overlaps || same_start
            }) {
                return Err(StopReason::InvalidInput(
                    "overlapping edits or ambiguous insertion order".into(),
                ));
            }
            length = length
                .checked_sub(edit.range.len())
                .and_then(|length| length.checked_add(edit.replacement.len()))
                .ok_or(StopReason::BudgetExceeded(Resource::ScratchBytes))?;
            previous = Some(edit);
        }
        governor.charge(
            Resource::ScratchBytes,
            size(length)
                .saturating_add(size(source.len()))
                .saturating_add(size(edits.len()).saturating_mul(256)),
        )?;
        governor.charge(Resource::Work, size(length))?;
        let mut output = String::with_capacity(length);
        let mut inverse: Vec<TextEdit> = Vec::new();
        let mut cursor = 0;
        for edit in edits {
            governor.charge(Resource::EvidenceEntries, 1)?;
            output.push_str(&source[cursor..edit.range.start]);
            let start = output.len();
            output.push_str(&edit.replacement);
            if let Some(previous) = inverse
                .last_mut()
                .filter(|previous| previous.range.end == start)
            {
                previous.range.end = output.len();
                previous.replacement.push_str(&source[edit.range.clone()]);
            } else {
                inverse.push(TextEdit {
                    range: start..output.len(),
                    replacement: source[edit.range.clone()].into(),
                });
            }
            let evidence = Evidence {
                rule: "text.replace-range.v1".into(),
                explanation: format!(
                    "replace {} bytes with {} bytes",
                    edit.range.len(),
                    edit.replacement.len()
                ),
                span: Some(SourceSpan {
                    source: self.source,
                    bytes: edit.range.clone(),
                }),
                destination: format!("bytes:{start}..{}", output.len()),
            };
            receipt.evidence.push(evidence.clone());
            receipt.losses.push(Loss {
                evidence,
                original: source[edit.range.clone()].into(),
            });
            cursor = edit.range.end;
        }
        output.push_str(&source[cursor..]);
        receipt.validations.push(Validation {
            validator: "text.range-integrity.v1".into(),
            constraint: self.source.to_string(),
            passed: true,
            diagnostics: Vec::new(),
        });
        Ok(AppliedEdit {
            inverse: Self {
                source: Digest::of(output.as_bytes()),
                edits: inverse,
            },
            text: output,
        })
    }
}

/// Locate exact replacement regions without choosing arbitrarily among duplicates.
/// Empty needles are rejected; explicit byte-range edits support insertion.
#[must_use]
pub fn locate(
    source: &str,
    needle: &str,
    replacement: &str,
    budget: Budget,
) -> AssistResult<EditPlan> {
    let mut receipt = Receipt::new(source.as_bytes(), "text.locate.v1", Effort::Exact, budget);
    let mut governor = Governor::new(budget);
    let result = (|| {
        receipt.retain_source(source, &mut governor)?;
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
            if source[position..].starts_with(needle) {
                governor.charge(Resource::Candidates, 1)?;
                governor.charge(
                    Resource::ScratchBytes,
                    size(replacement.len()).saturating_add(128),
                )?;
                plans.push(EditPlan {
                    source: receipt.source,
                    edits: vec![TextEdit {
                        range: position..position + needle.len(),
                        replacement: replacement.into(),
                    }],
                });
            }
        }
        Ok(plans)
    })();
    let outcome = match result {
        Ok(plans) => match plans.as_slice() {
            [] => {
                receipt.stopped = StopReason::NoCandidate;
                Outcome::Unresolved
            }
            [plan] => Outcome::Resolved(plan.clone()),
            _ => Outcome::Ambiguous(plans),
        },
        Err(reason) => {
            receipt.stopped = reason;
            Outcome::Unresolved
        }
    };
    receipt.usage = governor.usage();
    AssistResult { outcome, receipt }
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
