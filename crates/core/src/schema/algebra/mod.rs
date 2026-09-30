// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Witness generation for Draft 2020-12 JSON Schema.
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
//!   automaton (Task 6's `Pattern::generate`) exhausting every reachable
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

use super::{Schema, SchemaError};
use crate::contract::{Evidence, Governor, StopReason};
use crate::json::Node;

/// Preserve a compilation diagnostic when witness search rejects malformed input.
fn invalid(error: &SchemaError) -> StopReason {
    crate::contract::invalid_input(error.to_string())
}

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

/// Generate a witness for `schema`'s compiled location `id` (an internal
/// index, not part of the public API — external callers use
/// [`Schema::generate_witness`], always rooted; `crate::schema::recovery`
/// uses this directly for sub-schema locations, e.g. one `required`
/// property's own schema).
pub(super) fn generate(
    schema: &Schema,
    id: usize,
    governor: &mut Governor,
) -> Result<WitnessOutcome, StopReason> {
    Ok(match generate::run(schema, id, governor)? {
        generate::Attempt::Unsatisfiable(refutation) => WitnessOutcome::Unsatisfiable(refutation),
        generate::Attempt::Exhausted => WitnessOutcome::Exhausted,
        generate::Attempt::Witness(node) => {
            let checked = super::evaluate::run(schema, id, &node, governor)?;
            if checked.violations.is_empty() {
                WitnessOutcome::Witness(node)
            } else {
                WitnessOutcome::Exhausted
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
    /// [`WitnessOutcome`].
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
        generate(self, self.root, governor)
    }
}
