// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Bounded approximate substring selection with explicit uncertainty.
use super::{EditPlan, TextEdit, size};
use crate::contract::{Evidence, Inference, InferenceKind, Resource, SourceSpan};
use crate::{AssistResult, Budget, Effort, Governor, Outcome, Receipt, StopReason};
use std::ops::Range;

/// Locate minimum-distance replacement regions within a caller-selected edit limit.
///
/// Byte-exact occurrences always win. Otherwise, every nonempty Unicode-scalar
/// substring within `max_edits` Levenshtein edits is considered. Equal minima
/// remain ambiguous, including overlapping regions. A unique approximate match
/// is a `BestGuess`, never a claim that the caller intended that region.
/// Empty needles and limits large enough to delete the entire needle are rejected.
/// Exhaustion returns `Unresolved`, even if an earlier match was found.
///
/// ```
/// use purrassist_core::{Budget, Outcome, edits::locate_fuzzy};
/// let result = locate_fuzzy("hello world", "wurld", "earth", 1, Budget::default());
/// let Outcome::BestGuess(plan) = result.outcome else { panic!("expected correction") };
/// let Outcome::Resolved(edit) = plan.apply("hello world", Budget::default()).outcome
///     else { panic!("expected edit") };
/// assert_eq!(edit.text, "hello earth");
/// ```
#[must_use]
pub fn locate_fuzzy(
    source: &str,
    needle: &str,
    replacement: &str,
    max_edits: usize,
    budget: Budget,
) -> AssistResult<EditPlan> {
    let mut receipt = Receipt::new(
        source.as_bytes(),
        "text.locate-fuzzy.v1",
        Effort::Standard,
        budget,
    );
    let mut governor = Governor::new(budget);
    let result = receipt
        .retain_source(source, &mut governor)
        .and_then(|()| regions(source, needle, max_edits, &mut governor))
        .and_then(|(distance, ranges)| {
            let mut plans = Vec::new();
            for range in ranges {
                governor.charge(
                    Resource::ScratchBytes,
                    size(replacement.len()).saturating_add(512),
                )?;
                governor.charge(Resource::EvidenceEntries, 1)?;
                let evidence = Evidence {
                    rule: "text.substring-levenshtein.v1".into(),
                    explanation: format!("minimum scalar distance {distance}; allowed {max_edits}"),
                    span: Some(SourceSpan {
                        source: receipt.source,
                        bytes: range.clone(),
                    }),
                    destination: format!("bytes:{}..{}", range.start, range.end),
                };
                receipt.evidence.push(evidence.clone());
                if distance > 0 {
                    receipt.inferences.push(Inference {
                        evidence,
                        kind: InferenceKind::Guess,
                        value: source[range.clone()].into(),
                    });
                }
                plans.push(EditPlan {
                    source: receipt.source,
                    edits: vec![TextEdit {
                        range,
                        replacement: replacement.into(),
                    }],
                });
            }
            Ok((distance, plans))
        });
    let outcome = match result {
        Ok((_, plans)) if plans.is_empty() => {
            receipt.stopped = StopReason::NoCandidate;
            Outcome::Unresolved
        }
        Ok((distance, mut plans)) if plans.len() == 1 => {
            let plan = plans.remove(0);
            if distance == 0 {
                Outcome::Resolved(plan)
            } else {
                Outcome::BestGuess(plan)
            }
        }
        Ok((_, plans)) => Outcome::Ambiguous(plans),
        Err(reason) => {
            receipt.stopped = reason;
            Outcome::Unresolved
        }
    };
    receipt.usage = governor.usage();
    AssistResult { outcome, receipt }
}

pub(super) fn regions(
    source: &str,
    needle: &str,
    max_edits: usize,
    governor: &mut Governor,
) -> Result<(usize, Vec<Range<usize>>), StopReason> {
    governor.charge(
        Resource::InputBytes,
        size(source.len()).saturating_add(size(needle.len())),
    )?;
    governor.charge(
        Resource::Work,
        size(source.len()).saturating_mul(size(needle.len().max(1))),
    )?;
    let length = needle.chars().count();
    if length == 0 || max_edits >= length {
        return Err(StopReason::InvalidInput(
            "fuzzy needle must be nonempty and max_edits smaller than its scalar length".into(),
        ));
    }
    let mut ranges = Vec::new();
    for (start, _) in source.char_indices() {
        if source[start..].starts_with(needle) {
            retain(&mut ranges, start..start + needle.len(), governor)?;
        }
    }
    if !ranges.is_empty() || max_edits == 0 {
        return Ok((0, ranges));
    }
    let storage = size(source.len())
        .saturating_add(size(needle.len()))
        .saturating_mul(32);
    governor.charge(Resource::ScratchBytes, storage)?;
    let mut boundaries: Vec<_> = source.char_indices().map(|(offset, _)| offset).collect();
    boundaries.push(source.len());
    let mut best = max_edits;
    for start in 0..boundaries.len() - 1 {
        let min_end = start.saturating_add(length - max_edits);
        let max_end = start
            .saturating_add(length)
            .saturating_add(max_edits)
            .min(boundaries.len() - 1);
        for end in min_end..=max_end {
            let range = boundaries[start]..boundaries[end];
            let distance = crate::matching::levenshtein(needle, &source[range.clone()], governor)?;
            if distance < best {
                ranges.clear();
                best = distance;
            }
            if distance == best {
                retain(&mut ranges, range, governor)?;
            }
        }
    }
    governor.release_scratch(storage)?;
    Ok((best, ranges))
}

fn retain(
    ranges: &mut Vec<Range<usize>>,
    range: Range<usize>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    governor.charge(Resource::Candidates, 1)?;
    governor.charge(Resource::ScratchBytes, 32)?;
    ranges.push(range);
    Ok(())
}
