// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Per-alternative ledgers for the three edit-planning ambiguity sites
//! (`edits::locate_governed`, `edits::locate_fuzzy_governed`,
//! `edits::plan_unified_governed`): every ambiguous edit-planning operation
//! must publish one digest-bound `AlternativeReceipt` per candidate plan,
//! with shared evidence promoted to the top-level ledgers, exhaustion
//! mid-build never publishing a partial list, and every produced receipt
//! validating against `docs/schema/receipt.schema.json`.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    digest::Digest,
    edits::{locate_fuzzy_governed, locate_governed, plan_unified_governed},
    schema::Schema,
};
use std::path::Path;

fn receipt_schema() -> Schema {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/schema/receipt.schema.json");
    let source = std::fs::read_to_string(path).expect("receipt schema fixture");
    Schema::compile(&source).expect("receipt schema compiles")
}

#[test]
fn locate_ambiguous_matches_publish_one_alternative_each() {
    let schema = receipt_schema();
    let result = locate_governed("aaa", "aa", "x", &mut Governor::new(Budget::default()));
    let Outcome::Ambiguous(plans) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(plans.len(), 2);
    assert_eq!(result.receipt.alternatives.len(), plans.len());
    for (plan, ledger) in plans.iter().zip(&result.receipt.alternatives) {
        assert_eq!(
            ledger.output,
            Digest::of(plan.to_node().to_json().as_bytes())
        );
        assert_eq!(ledger.evidence.len(), 1);
    }
    // Every occurrence's evidence carries its own byte span, so no two
    // overlapping matches share evidence content: the intersection is empty
    // and the top-level ledger stays empty.
    assert_eq!(result.receipt.evidence, Vec::new());
    let report = schema.validate(&result.receipt.to_node()).unwrap();
    assert!(report.valid, "{:?}", report.violations);
}

#[test]
fn locate_exhaustion_mid_build_publishes_no_alternatives() {
    let baseline = locate_governed("aaa", "aa", "x", &mut Governor::new(Budget::default()));
    assert!(matches!(baseline.outcome, Outcome::Ambiguous(_)));
    let limit = baseline.receipt.usage.evidence_entries - 1;
    let mut governor = Governor::new(Budget {
        evidence_entries: limit,
        ..Budget::default()
    });
    let starved = locate_governed("aaa", "aa", "x", &mut governor);
    assert_eq!(starved.outcome, Outcome::Unresolved);
    assert_eq!(
        starved.receipt.stopped,
        StopReason::BudgetExceeded(Resource::EvidenceEntries)
    );
    assert_eq!(starved.receipt.alternatives, Vec::new());
    assert_eq!(starved.receipt.evidence, Vec::new());
}

#[test]
fn fuzzy_ties_publish_one_alternative_each() {
    let schema = receipt_schema();
    let result = locate_fuzzy_governed(
        "cat bat",
        "hat",
        "dog",
        1,
        &mut Governor::new(Budget::default()),
    );
    let Outcome::Ambiguous(plans) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(plans.len(), 4);
    assert_eq!(result.receipt.alternatives.len(), plans.len());
    for (plan, ledger) in plans.iter().zip(&result.receipt.alternatives) {
        assert_eq!(
            ledger.output,
            Digest::of(plan.to_node().to_json().as_bytes())
        );
    }
    assert_eq!(result.receipt.evidence, Vec::new());
    assert_eq!(result.receipt.inferences, Vec::new());
    let report = schema.validate(&result.receipt.to_node()).unwrap();
    assert!(report.valid, "{:?}", report.violations);
}

#[test]
fn fuzzy_exhaustion_mid_build_publishes_no_alternatives() {
    let baseline = locate_fuzzy_governed(
        "cat bat",
        "hat",
        "dog",
        1,
        &mut Governor::new(Budget::default()),
    );
    assert!(matches!(baseline.outcome, Outcome::Ambiguous(_)));
    let limit = baseline.receipt.usage.evidence_entries - 1;
    let mut governor = Governor::new(Budget {
        evidence_entries: limit,
        ..Budget::default()
    });
    let starved = locate_fuzzy_governed("cat bat", "hat", "dog", 1, &mut governor);
    assert_eq!(starved.outcome, Outcome::Unresolved);
    assert_eq!(
        starved.receipt.stopped,
        StopReason::BudgetExceeded(Resource::EvidenceEntries)
    );
    assert_eq!(starved.receipt.alternatives, Vec::new());
}

#[test]
fn unified_context_ambiguity_publishes_one_alternative_each() {
    let schema = receipt_schema();
    let patch = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+A\n";
    let result = plan_unified_governed("a\na\n", patch, &mut Governor::new(Budget::default()));
    let Outcome::Ambiguous(plans) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(plans.len(), 2);
    assert_eq!(result.receipt.alternatives.len(), plans.len());
    for (plan, ledger) in plans.iter().zip(&result.receipt.alternatives) {
        assert_eq!(
            ledger.output,
            Digest::of(plan.to_node().to_json().as_bytes())
        );
        assert_eq!(ledger.evidence.len(), 1);
    }
    // Each combination's context evidence is bound to a different matched
    // line's byte range, so nothing is shared and the top-level ledger
    // stays empty.
    assert_eq!(result.receipt.evidence, Vec::new());
    let report = schema.validate(&result.receipt.to_node()).unwrap();
    assert!(report.valid, "{:?}", report.violations);
}

#[test]
fn unified_exhaustion_mid_build_publishes_no_alternatives() {
    let patch = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+A\n";
    let baseline = plan_unified_governed("a\na\n", patch, &mut Governor::new(Budget::default()));
    assert!(matches!(baseline.outcome, Outcome::Ambiguous(_)));
    let limit = baseline.receipt.usage.evidence_entries - 1;
    let mut governor = Governor::new(Budget {
        evidence_entries: limit,
        ..Budget::default()
    });
    let starved = plan_unified_governed("a\na\n", patch, &mut governor);
    assert_eq!(starved.outcome, Outcome::Unresolved);
    assert_eq!(
        starved.receipt.stopped,
        StopReason::BudgetExceeded(Resource::EvidenceEntries)
    );
    assert_eq!(starved.receipt.alternatives, Vec::new());
}
