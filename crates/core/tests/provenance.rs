// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! End-to-end derivation-forest properties, exercised through a real
//! ambiguous public operation (`matching::resolve`) rather than the
//! `provenance` module's own unit tests, plus direct forest properties that
//! are awkward to observe indirectly (multiplicity, tropical ranking,
//! Boolean validation, and `distinguishing`).
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::{Evidence, Resource},
    digest::Digest,
    matching::{self, Candidate, MatchRequest, SelectionPolicy},
    provenance::{Derivation, Forest},
    schema::Schema,
};
use std::path::Path;

fn receipt_schema() -> Schema {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/schema/receipt.schema.json");
    let source = std::fs::read_to_string(path).expect("receipt schema fixture");
    Schema::compile(&source).expect("receipt schema compiles")
}

fn evidence(rule: &str) -> Evidence {
    Evidence {
        rule: rule.into(),
        explanation: "probe".into(),
        span: None,
        destination: "/x".into(),
    }
}

#[test]
fn ambiguous_string_selection_receipt_validates_against_the_receipt_schema() {
    let schema = receipt_schema();
    let candidates = vec![Candidate::new("a", "reader"), Candidate::new("b", "reader")];
    let result = matching::resolve(
        "reader",
        &candidates,
        &MatchRequest::default(),
        SelectionPolicy {
            minimum_score: 10_000,
            minimum_margin: 0,
        },
    );
    assert!(matches!(result.outcome, Outcome::Ambiguous(_)));
    let report = schema.validate(&result.receipt.to_node()).unwrap();
    assert!(report.valid, "{:?}", report.violations);
}

#[test]
fn forest_with_n_alternatives_sharing_e_evidence_charges_e_not_n_times_e() {
    let mut governor = Governor::new(Budget::default());
    let mut forest = Forest::new(&governor);
    let shared = evidence("shared");
    let mut derivations = Vec::new();
    for index in 0_u8..8 {
        let mut derivation = Derivation::new(Digest::of(&[index]), 0);
        derivation.push(forest.evidence(&mut governor, shared.clone()).unwrap());
        derivation.push(
            forest
                .evidence(&mut governor, evidence(&format!("distinct-{index}")))
                .unwrap(),
        );
        derivations.push(derivation);
    }
    // 1 shared node + 8 distinct nodes = 9 distinct nodes, never 8 * 2 = 16.
    assert_eq!(governor.usage().evidence_entries, 9);
    let published = forest.publish(&derivations, &mut governor).unwrap();
    assert_eq!(published.evidence, vec![shared]);
    assert_eq!(published.alternatives.len(), 8);
    for alternative in &published.alternatives {
        assert_eq!(alternative.evidence.len(), 1);
    }
}

#[test]
fn evidence_entries_exhaustion_mid_build_yields_budget_exceeded_and_no_alternatives() {
    let mut governor = Governor::new(Budget {
        evidence_entries: 1,
        ..Budget::default()
    });
    let mut forest = Forest::new(&governor);
    forest.evidence(&mut governor, evidence("first")).unwrap();
    // The budget admitted exactly one distinct node; a second, different
    // one exhausts `EvidenceEntries` before it is stored anywhere.
    let failure = forest.evidence(&mut governor, evidence("second"));
    assert_eq!(
        failure,
        Err(StopReason::BudgetExceeded(Resource::EvidenceEntries))
    );
    // The caller therefore never calls `publish`; nothing was ever assigned
    // to a receipt, matching "no alternatives published" for exhaustion.
}

#[test]
fn tropical_rank_orders_by_ascending_cost() {
    let a = Derivation::new(Digest::of(b"a"), 9);
    let b = Derivation::new(Digest::of(b"b"), 1);
    let c = Derivation::new(Digest::of(b"c"), 5);
    assert_eq!(Forest::rank(&[a, b, c]), vec![1, 2, 0]);
}

#[test]
fn counting_multiplicity_and_boolean_validation() {
    let mut governor = Governor::new(Budget::default());
    let mut forest = Forest::new(&governor);
    let shared = forest.evidence(&mut governor, evidence("shared")).unwrap();
    let passing = forest.validation(&mut governor, 0, true).unwrap();
    let failing = forest.validation(&mut governor, 1, false).unwrap();

    let mut a = Derivation::new(Digest::of(b"a"), 0);
    a.push(shared);
    a.push(passing);
    let mut b = Derivation::new(Digest::of(b"b"), 0);
    b.push(shared);
    b.push(failing);
    let mut c = Derivation::new(Digest::of(b"c"), 0);
    c.push(passing);

    assert_eq!(
        Forest::multiplicity(&[a.clone(), b.clone(), c.clone()], shared),
        2
    );
    assert_eq!(
        Forest::multiplicity(&[a.clone(), b.clone(), c.clone()], passing),
        2
    );
    assert!(forest.valid(&a));
    assert!(!forest.valid(&b));
    assert!(forest.valid(&c));
}

#[test]
fn distinguishing_reports_the_symmetric_difference() {
    let mut governor = Governor::new(Budget::default());
    let mut forest = Forest::new(&governor);
    let shared = forest.evidence(&mut governor, evidence("shared")).unwrap();
    let only_a = forest.evidence(&mut governor, evidence("only-a")).unwrap();
    let only_b = forest.evidence(&mut governor, evidence("only-b")).unwrap();
    let mut a = Derivation::new(Digest::of(b"a"), 0);
    a.push(shared);
    a.push(only_a);
    let mut b = Derivation::new(Digest::of(b"b"), 0);
    b.push(shared);
    b.push(only_b);
    let distinguishing = forest.distinguishing(&a, &b);
    assert_eq!(distinguishing.len(), 2);
    assert!(
        !distinguishing
            .iter()
            .any(|node| *node == forest.node(shared))
    );
}
