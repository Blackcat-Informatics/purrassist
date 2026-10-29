// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Witness generation for Draft 2020-12 and Draft-07 JSON Schema.
//!
//! Draft-07 needs no separate algebra: tuple-form `items` + `additionalItems`
//! canonicalize to the same facets as `prefixItems` + `items`, and
//! `dependencies` to the `dependentRequired`/`dependentSchemas` facets by
//! entry form; a Draft-07 `$ref` hides its siblings from every stage, exactly
//! as the evaluator ignores them.
//!
//! Implements the Attouche, Baazizi, Colazzo, Ghelli, Sartiani pipeline
//! ("Witness Generation for JSON Schema", VLDB 2022): canonicalize
//! (`canon::build`) → `not` elimination (`canon::build_negated`) →
//! `and`-merge (`merge::Requirements::apply`) → preparation → generation
//! (`generate::Gen`), recursing over the reference graph including
//! `$dynamicRef`'s dynamic scope stack.
//!
//! Every outcome is one of exactly three shapes, matching the accepted
//! contract:
//! - [`WitnessOutcome::Witness`]: re-validated with the real evaluator
//!   (`super::evaluate::run`) before this module ever returns it, so an
//!   unsound simplification anywhere upstream can only ever cost a
//!   downgrade to [`WitnessOutcome::Exhausted`], never an invalid witness.
//! - [`WitnessOutcome::Unsatisfiable`]: a closed-form refutation — an
//!   empty merged `TypeSet`, a crossed numeric/length/count bound,
//!   an incompatible literal, or a pattern proven empty by the derivative
//!   automaton (`Pattern::generate` of the derivative pattern engine) exhausting every reachable
//!   state — never a claim inferred from search exhaustion.
//! - [`WitnessOutcome::Exhausted`]: this stage's bounded, deliberately
//!   incomplete search (a fixed-size numeric/string candidate scan, or a
//!   structural keyword this module leaves to the `OpaqueNot` canon's
//!   generate-and-check fallback) found neither of the above.
//!
//! `StopReason` is reserved for governor hard-stops (`BudgetExceeded`,
//! `Cancelled`, a malformed `$ref`): unsatisfiability is deliberately never
//! expressed as a `StopReason`, preserving that type's existing contract
//! that exhaustion never asserts unsatisfiability (see `crate::contract`).
mod canon;
mod generate;
mod merge;
mod storage;
#[cfg(test)]
mod tests;

use super::Schema;
use crate::contract::{Evidence, Governor, Resource, ScratchVec, StopReason};
use crate::json::{Node, OwnedDocument};

/// Closed-form refutation evidence for [`WitnessOutcome::Unsatisfiable`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refutation {
    /// Human-readable reason the merged constraints admit no instance.
    pub reason: String,
    /// The same reason, as one audit evidence entry.
    pub evidence: Vec<Evidence>,
}

/// The three possible shapes [`Schema::generate_witness`](crate::schema::Schema::generate_witness) returns; see the module
/// documentation for exactly what each one guarantees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WitnessOutcome {
    /// A value that validates against the requested schema location.
    Witness(Node),
    /// No instance can ever satisfy the requested schema location.
    Unsatisfiable(Refutation),
    /// Neither of the above was established within this stage's bounds.
    Exhausted,
}

/// Internal results carry no public refutation storage. Recovery consumes
/// these directly; only the public witness boundary materializes evidence.
pub(super) enum Attempt {
    Witness(OwnedDocument),
    Unsatisfiable(&'static str),
    Exhausted,
}

impl Refutation {
    /// Keep every reservation releasable until all copying and admission finish.
    fn publish(reason: &'static str, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(Resource::EvidenceEntries, 1)?;
        let reason = super::reference::copy(reason, governor)?;
        let rule = super::reference::copy("schema.algebra.unsatisfiable-v1", governor)?;
        let explanation = super::reference::copy(&reason, governor)?;
        let (reason, reason_lease) = reason.into_owned_parts();
        let (rule, rule_lease) = rule.into_owned_parts();
        let (explanation, explanation_lease) = explanation.into_owned_parts();
        let mut evidence = ScratchVec::new(governor);
        storage::push(
            &mut evidence,
            Evidence {
                rule,
                explanation,
                span: None,
                destination: String::new(),
            },
            governor,
        )?;
        governor.check()?;
        reason_lease.retain();
        rule_lease.retain();
        explanation_lease.retain();
        Ok(Self {
            reason,
            evidence: evidence.into_vec(),
        })
    }
}

/// Generate a witness for `schema`'s compiled location `id` (an internal
/// index, not part of the public API — external callers use
/// [`Schema::generate_witness`], always rooted; `crate::schema::recovery`
/// uses this directly for sub-schema locations, e.g. one `required`
/// property's own schema). This preserves witness revalidation without
/// materializing public evidence for refutations that recovery discards.
pub(super) fn generate(
    schema: &Schema,
    id: usize,
    governor: &mut Governor,
) -> Result<Attempt, StopReason> {
    Ok(match generate::run(schema, id, governor)? {
        Attempt::Unsatisfiable(refutation) => Attempt::Unsatisfiable(refutation),
        Attempt::Exhausted => Attempt::Exhausted,
        Attempt::Witness(node) => {
            let checked = super::evaluate::run(schema, id, node.root(), governor)?;
            if checked.violations.is_empty() {
                Attempt::Witness(node)
            } else {
                Attempt::Exhausted
            }
        }
    })
}

impl Schema {
    /// Generate a witness instance for this complete schema, using
    /// ordinary resource bounds.
    ///
    /// ```
    /// use purrassist_core::schema::{Schema, algebra::WitnessOutcome};
    /// let schema = Schema::compile(r#"{"type":"integer","minimum":5}"#).unwrap();
    /// let WitnessOutcome::Witness(node) = schema.generate_witness() else { panic!("expected a witness") };
    /// assert!(schema.validate(&node).unwrap().valid);
    /// ```
    #[must_use]
    pub fn generate_witness(&self) -> WitnessOutcome {
        self.generate_witness_governed(&mut Governor::new(crate::contract::Budget::default()))
            .unwrap_or(WitnessOutcome::Exhausted)
    }

    /// Generate a witness instance for this complete schema under a
    /// caller-owned governor, distinguishing resource exhaustion from every
    /// [`WitnessOutcome`]. Candidate trees retain actual buffer owners through
    /// selection and final revalidation. Literal copies preserve number spelling
    /// and source spans, enforcing caller Depth and a native ceiling of 128.
    /// Final refutations admit one evidence entry, copying work and owned output
    /// buffers. Only completed public output retains its storage admission;
    /// discarded internal candidates and refutations publish nothing. Dropping
    /// ordinary public output does not release that retained admission. General
    /// assertion/format computation retains its separate resource contract.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, schema::{Schema, algebra::WitnessOutcome}};
    /// let mut governor = Governor::new(Budget::default());
    /// let schema = Schema::compile_governed("false", &mut governor).unwrap();
    /// assert!(matches!(
    ///     schema.generate_witness_governed(&mut governor).unwrap(),
    ///     WitnessOutcome::Unsatisfiable(_)
    /// ));
    /// ```
    ///
    /// # Errors
    /// Returns a governor hard-stop (`BudgetExceeded`/`Cancelled`), never an
    /// unsatisfiability claim: see the module documentation.
    pub fn generate_witness_governed(
        &self,
        governor: &mut Governor,
    ) -> Result<WitnessOutcome, StopReason> {
        let attempt = generate(self, self.root, governor)?;
        governor.check()?;
        Ok(match attempt {
            Attempt::Witness(node) => WitnessOutcome::Witness(node.into_node()),
            Attempt::Unsatisfiable(reason) => {
                WitnessOutcome::Unsatisfiable(Refutation::publish(reason, governor)?)
            }
            Attempt::Exhausted => WitnessOutcome::Exhausted,
        })
    }
}
