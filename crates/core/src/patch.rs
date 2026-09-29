// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The crate's single edit representation.
//!
//! An ordered set of non-overlapping byte-range replacements bound to an
//! exact source digest, with a governed algebra of `apply`, `invert`,
//! `compose`, `commute`, `rebase`, and `diff`.
//!
//! Every other edit-planning surface (`crate::edits::{locate, locate_fuzzy,
//! plan_unified}`, every `crate::markdown` structural operation, and the host
//! `apply_edit`/`apply_file` journal) produces or consumes a [`Patch`]; none
//! of them build or check byte ranges independently. `crate::edits::EditPlan`
//! and `crate::edits::TextEdit` are aliases for [`Patch`] and [`TextEdit`]
//! (defined here), not a second implementation.
//!
//! [`Patch::compose`], [`Patch::commute`], and [`Patch::rebase`] are all
//! defined in terms of the *exact source text*, not purely as range algebra:
//! every real call site (a host tool reconciling a stale plan against a
//! freshly read file, a property test generating text alongside patches)
//! already has the text available, and grounding every derived patch's
//! `source` digest against real applied bytes is simpler and strictly safer
//! than reconstructing it from range arithmetic alone.
use crate::contract::{Evidence, Loss, Resource, SourceSpan, Validation};
use crate::digest::Digest;
use crate::{AssistResult, Budget, Effort, Governor, Outcome, Receipt, StopReason};
use std::ops::Range;

/// Replace exactly one half-open UTF-8 byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    /// Byte range in the original source; insertion uses an empty range.
    pub range: Range<usize>,
    /// Text inserted verbatim.
    pub replacement: String,
}

/// An immutable, source-bound set of ordered, non-overlapping byte-range
/// replacements: the crate's single edit representation.
///
/// A `Patch` names the exact bytes it was derived from (`source`) and never
/// claims to apply to anything else; [`Patch::apply`] refuses a changed
/// source outright rather than guess at realignment. Every planning surface
/// in this crate (exact/fuzzy/unified text edits, every Markdown structural
/// operation) produces a `Patch`; `crate::edits::EditPlan` is an alias for
/// this type, not a parallel representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
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
    pub inverse: Patch,
}

/// A governed [`Patch::rebase`]'s successful outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rebased {
    /// The rebased patch, bound to the intervening change's output digest.
    pub patch: Patch,
    /// Evidence that this patch was shifted to account for a commuting
    /// intervening change, rather than applied unchanged.
    pub transformation: Evidence,
}

impl Patch {
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

    /// Apply without building a full receipt, for internal use by [`Patch::invert`],
    /// [`Patch::compose`], [`Patch::commute`], and [`Patch::rebase`].
    fn apply_raw(&self, source: &str, governor: &mut Governor) -> Result<AppliedEdit, StopReason> {
        let budget = *governor.limits();
        let mut receipt = Receipt::new(source.as_bytes(), "patch.apply.v1", Effort::Exact, budget);
        self.apply_with(source, governor, &mut receipt)
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

    /// Compute the exact inverse patch bound to the applied output's digest.
    ///
    /// Equivalent to discarding [`AppliedEdit::text`] from
    /// [`Patch::apply_governed`] and keeping only its `inverse`, provided
    /// directly for callers (including [`Patch::compose`], [`Patch::commute`],
    /// and [`Patch::rebase`]) that need only the inverse. `invert` composed
    /// with `apply` is the identity: applying `self` to `source`, then
    /// applying the result to that output, restores `source` exactly.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, digest::Digest, patch::{Patch, TextEdit}};
    /// let mut g = Governor::new(Budget::default());
    /// let patch = Patch {
    ///     source: Digest::of(b"tea"),
    ///     edits: vec![TextEdit { range: 0..1, replacement: "T".into() }],
    /// };
    /// let inverse = patch.invert("tea", &mut g).unwrap();
    /// let Outcome::Resolved(applied) = patch.apply("tea", Budget::default()).outcome else { panic!() };
    /// let Outcome::Resolved(restored) = inverse.apply(&applied.text, Budget::default()).outcome else { panic!() };
    /// assert_eq!(restored.text, "tea");
    /// ```
    ///
    /// # Errors
    /// Propagates every refusal [`Patch::apply_governed`] can return: a stale
    /// source digest, an out-of-bounds or non-UTF-8-boundary range, overlapping
    /// edits, or resource exhaustion/cancellation.
    pub fn invert(&self, source: &str, governor: &mut Governor) -> Result<Self, StopReason> {
        self.apply_raw(source, governor)
            .map(|applied| applied.inverse)
    }

    /// Sequentially compose `self` (applied to `base`) with `other` (applied
    /// to `self`'s output) into one patch mapping `base` directly to `other`'s
    /// final output.
    ///
    /// The composed patch is a single covering replacement bound to `base`'s
    /// digest (see [`Patch::diff`]); it does not preserve each input patch's
    /// individual edit granularity, only the combined byte-exact result.
    /// `compose` is associative: composing `(p ∘ q) ∘ r` and `p ∘ (q ∘ r)`
    /// against the same base produces patches whose application yields
    /// identical text, since both are grounded in the same sequential
    /// application of `p`, then `q`, then `r`.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, digest::Digest, patch::{Patch, TextEdit}};
    /// let mut g = Governor::new(Budget::default());
    /// let p = Patch { source: Digest::of(b"tea"), edits: vec![TextEdit { range: 0..1, replacement: "T".into() }] };
    /// let Outcome::Resolved(mid) = p.apply("tea", Budget::default()).outcome else { panic!() };
    /// let q = Patch { source: Digest::of(mid.text.as_bytes()), edits: vec![TextEdit { range: 3..3, replacement: "!".into() }] };
    /// let composed = p.compose(&q, "tea", &mut g).unwrap();
    /// let Outcome::Resolved(applied) = composed.apply("tea", Budget::default()).outcome else { panic!() };
    /// assert_eq!(applied.text, "Tea!");
    /// ```
    ///
    /// # Errors
    /// Rejects a `self` not defined against `base`, an `other` not defined
    /// against `self`'s output on `base`, and every refusal `apply` can
    /// return for either patch. Resource exhaustion and cancellation propagate.
    pub fn compose(
        &self,
        other: &Self,
        base: &str,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        governor.charge(
            Resource::Work,
            size(self.edits.len()).saturating_add(size(other.edits.len())),
        )?;
        if self.source != Digest::of(base.as_bytes()) {
            return Err(invalid(
                "compose: patch is not defined against the supplied base",
            ));
        }
        let mid = self.apply_raw(base, governor)?;
        if other.source != mid.inverse.source {
            return Err(invalid(
                "compose: other patch is not defined against this patch's output",
            ));
        }
        let end = other.apply_raw(&mid.text, governor)?;
        Self::diff(base, &end.text, governor)
    }

    /// Determine whether `self` and `other`, both defined against the same
    /// `base`, commute: whether their edited ranges are disjoint, so applying
    /// either one first and the other rebased onto it reaches the same final
    /// text regardless of order.
    ///
    /// Returns `Ok(None)` when the two patches' ranges overlap (they do not
    /// commute); [`Patch::rebase`] refuses in exactly that case. On success,
    /// returns `Some((self_onto_other, other_onto_self))`: `self_onto_other`
    /// is valid immediately after `other`'s output, and `other_onto_self` is
    /// valid immediately after `self`'s output. Both reach the identical
    /// combined text: applying `other` then `self_onto_other` equals applying
    /// `self` then `other_onto_self`.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, digest::Digest, patch::{Patch, TextEdit}};
    /// let mut g = Governor::new(Budget::default());
    /// let base = "hot tea";
    /// let p = Patch { source: Digest::of(base.as_bytes()), edits: vec![TextEdit { range: 0..3, replacement: "T".into() }] };
    /// let q = Patch { source: Digest::of(base.as_bytes()), edits: vec![TextEdit { range: 4..7, replacement: "Tea".into() }] };
    /// let (p2, q2) = p.commute(&q, base, &mut g).unwrap().expect("disjoint ranges commute");
    /// let Outcome::Resolved(via_q) = q.apply(base, Budget::default()).outcome else { panic!() };
    /// let Outcome::Resolved(first) = p2.apply(&via_q.text, Budget::default()).outcome else { panic!() };
    /// let Outcome::Resolved(via_p) = p.apply(base, Budget::default()).outcome else { panic!() };
    /// let Outcome::Resolved(second) = q2.apply(&via_p.text, Budget::default()).outcome else { panic!() };
    /// assert_eq!(first.text, second.text);
    /// ```
    ///
    /// # Errors
    /// Rejects patches not defined against `base`, or either patch's own
    /// internally overlapping edits. Resource exhaustion and cancellation propagate.
    pub fn commute(
        &self,
        other: &Self,
        base: &str,
        governor: &mut Governor,
    ) -> Result<Option<(Self, Self)>, StopReason> {
        governor.charge(
            Resource::Work,
            size(self.edits.len())
                .saturating_add(size(other.edits.len()))
                .saturating_mul(4),
        )?;
        let digest = Digest::of(base.as_bytes());
        if self.source != digest || other.source != digest {
            return Err(invalid(
                "commute: patches are not defined against the same source",
            ));
        }
        let Some(merged) = merge_disjoint(&self.edits, &other.edits) else {
            return Ok(None);
        };
        let combined = Self {
            source: self.source,
            edits: merged,
        };
        let combined_applied = combined.apply_raw(base, governor)?;
        let self_applied = self.apply_raw(base, governor)?;
        let other_applied = other.apply_raw(base, governor)?;
        let self_onto_other = Self::diff(&other_applied.text, &combined_applied.text, governor)?;
        let other_onto_self = Self::diff(&self_applied.text, &combined_applied.text, governor)?;
        Ok(Some((self_onto_other, other_onto_self)))
    }

    /// Governed rebase: reconcile a plan (`self`), made against `base`, with
    /// an intervening change (`onto`) also made against `base`, so the plan
    /// applies correctly to `onto`'s output instead.
    ///
    /// Succeeds exactly when `self` and `onto` commute (their edited ranges
    /// are disjoint; see [`Patch::commute`]), recording a `transformation`
    /// describing the rebase. A plan whose edits overlap the intervening
    /// change cannot be unambiguously reinterpreted and is refused as stale.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, digest::Digest, patch::{Patch, TextEdit}};
    /// let mut g = Governor::new(Budget::default());
    /// let base = "hot tea";
    /// let stale_plan = Patch { source: Digest::of(base.as_bytes()), edits: vec![TextEdit { range: 0..3, replacement: "T".into() }] };
    /// let intervening = Patch { source: Digest::of(base.as_bytes()), edits: vec![TextEdit { range: 4..7, replacement: "Tea".into() }] };
    /// let rebased = stale_plan.rebase(&intervening, base, &mut g).unwrap();
    /// assert_eq!(rebased.transformation.rule, "patch.rebase.v1");
    /// ```
    ///
    /// # Errors
    /// Returns an explicit stale refusal (`InvalidInput`) when `self` and
    /// `onto` do not commute, plus every error [`Patch::commute`] can return.
    pub fn rebase(
        &self,
        onto: &Self,
        base: &str,
        governor: &mut Governor,
    ) -> Result<Rebased, StopReason> {
        match self.commute(onto, base, governor)? {
            Some((rebased, _)) => {
                let transformation = Evidence {
                    rule: "patch.rebase.v1".into(),
                    explanation: format!(
                        "rebased {} edit(s) onto a commuting intervening change of {} edit(s)",
                        self.edits.len(),
                        onto.edits.len(),
                    ),
                    span: None,
                    destination: rebased.source.to_string(),
                };
                Ok(Rebased {
                    patch: rebased,
                    transformation,
                })
            }
            None => Err(invalid(
                "stale plan: intervening change overlaps this plan's edited ranges",
            )),
        }
    }

    /// Synthesize a patch from `prior` directly to `current`, as a single
    /// covering replacement bounded by the longest common UTF-8-aligned
    /// prefix and suffix.
    ///
    /// This is not a minimal multi-hunk diff: it never reports more than one
    /// edit. It is exact (applying the result to `prior` reproduces `current`
    /// byte-for-byte) and is the mechanism [`Patch::compose`] and
    /// [`Patch::commute`] use to ground a derived patch's source digest in
    /// real applied bytes, and that host tools use to turn a prior/current
    /// file pair into an intervening [`Patch`] for [`Patch::rebase`].
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, patch::Patch};
    /// let mut g = Governor::new(Budget::default());
    /// let patch = Patch::diff("hot tea", "hot coffee", &mut g).unwrap();
    /// let Outcome::Resolved(applied) = patch.apply("hot tea", Budget::default()).outcome else { panic!() };
    /// assert_eq!(applied.text, "hot coffee");
    /// ```
    ///
    /// # Errors
    /// Returns resource exhaustion or cancellation before any comparison.
    pub fn diff(prior: &str, current: &str, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(
            Resource::Work,
            size(prior.len()).saturating_add(size(current.len())),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(current.len()).saturating_add(64),
        )?;
        let source = Digest::of(prior.as_bytes());
        if prior == current {
            return Ok(Self {
                source,
                edits: Vec::new(),
            });
        }
        let mut prefix = prior
            .bytes()
            .zip(current.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while prefix > 0 && !prior.is_char_boundary(prefix) {
            prefix -= 1;
        }
        while prefix > 0 && !current.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let prior_rest = &prior[prefix..];
        let current_rest = &current[prefix..];
        let mut suffix = prior_rest
            .bytes()
            .rev()
            .zip(current_rest.bytes().rev())
            .take_while(|(a, b)| a == b)
            .count();
        suffix = suffix.min(prior_rest.len()).min(current_rest.len());
        while suffix > 0 && !prior_rest.is_char_boundary(prior_rest.len() - suffix) {
            suffix -= 1;
        }
        while suffix > 0 && !current_rest.is_char_boundary(current_rest.len() - suffix) {
            suffix -= 1;
        }
        let range = prefix..prior.len() - suffix;
        let replacement = current[prefix..current.len() - suffix].to_string();
        Ok(Self {
            source,
            edits: vec![TextEdit { range, replacement }],
        })
    }
}

/// Merge two individually well-formed (internally sorted, non-overlapping,
/// non-touching) edit lists into one combined sorted list, or `None` if any
/// edit in either list overlaps or shares a start point with an edit in the
/// other (or, defensively, within either list itself).
fn merge_disjoint(a: &[TextEdit], b: &[TextEdit]) -> Option<Vec<TextEdit>> {
    let mut a: Vec<TextEdit> = a.to_vec();
    let mut b: Vec<TextEdit> = b.to_vec();
    a.sort_by_key(|edit| (edit.range.start, edit.range.end));
    b.sort_by_key(|edit| (edit.range.start, edit.range.end));
    let mut merged = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].range.start.cmp(&b[j].range.start) {
            std::cmp::Ordering::Less => {
                merged.push(a[i].clone());
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                merged.push(b[j].clone());
                j += 1;
            }
            std::cmp::Ordering::Equal => return None,
        }
    }
    merged.extend(a[i..].iter().cloned());
    merged.extend(b[j..].iter().cloned());
    if merged
        .windows(2)
        .all(|pair| pair[0].range.end <= pair[1].range.start)
    {
        Some(merged)
    } else {
        None
    }
}

fn invalid(message: &str) -> StopReason {
    StopReason::InvalidInput(message.into())
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{Digest, Patch, TextEdit};
    use crate::{Budget, Governor, Outcome};

    #[test]
    fn diff_is_exact_for_prefix_suffix_and_disjoint_middle_changes() {
        let mut g = Governor::new(Budget::default());
        for (prior, current) in [
            ("", ""),
            ("same", "same"),
            ("hot tea", "hot coffee"),
            ("", "grew"),
            ("shrank", ""),
            ("café\u{301}", "café"),
        ] {
            let patch = Patch::diff(prior, current, &mut g).unwrap();
            assert_eq!(patch.source, Digest::of(prior.as_bytes()));
            let Outcome::Resolved(applied) = patch.apply(prior, Budget::default()).outcome else {
                panic!("diff patch must apply to its own prior text")
            };
            assert_eq!(applied.text, current);
        }
    }

    #[test]
    fn commute_refuses_overlapping_same_source_patches() {
        let mut g = Governor::new(Budget::default());
        let base = "hot tea";
        let p = Patch {
            source: Digest::of(base.as_bytes()),
            edits: vec![TextEdit {
                range: 0..3,
                replacement: "cold".into(),
            }],
        };
        let q = Patch {
            source: Digest::of(base.as_bytes()),
            edits: vec![TextEdit {
                range: 2..5,
                replacement: "t ".into(),
            }],
        };
        assert_eq!(p.commute(&q, base, &mut g), Ok(None));
        assert!(p.rebase(&q, base, &mut g).is_err());
    }

    #[test]
    fn invert_then_apply_restores_the_exact_original() {
        let mut g = Governor::new(Budget::default());
        let source = "hot tea\r\nkeep\r\n";
        let patch = Patch {
            source: Digest::of(source.as_bytes()),
            edits: vec![TextEdit {
                range: 4..7,
                replacement: "coffee".into(),
            }],
        };
        let Outcome::Resolved(applied) = patch.apply(source, Budget::default()).outcome else {
            panic!("apply")
        };
        let inverse = patch.invert(source, &mut g).unwrap();
        assert_eq!(inverse, applied.inverse);
        let Outcome::Resolved(restored) = inverse.apply(&applied.text, Budget::default()).outcome
        else {
            panic!("invert-apply")
        };
        assert_eq!(restored.text, source);
    }
}
