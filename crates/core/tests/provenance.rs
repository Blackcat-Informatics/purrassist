// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! End-to-end derivation-forest properties, exercised through a real
//! ambiguous public operation (`matching::resolve`) rather than the
//! `provenance` module's own unit tests, plus direct forest properties that
//! are awkward to observe indirectly (multiplicity, tropical ranking,
//! Boolean validation, and `distinguishing`).
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::{Evidence, Receipt, Resource},
    digest::Digest,
    edits::{locate_fuzzy_governed, locate_governed, plan_unified_governed},
    intent::{Concept, Domain, Rule},
    json::Value,
    matching::{self, Candidate, MatchRequest, SelectionPolicy},
    paths::PathIndex,
    provenance::{Derivation, Forest},
    schema::{Schema, recovery::RecoveryRequest},
    wire::WireValue,
};
use std::{collections::BTreeMap, path::Path};

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

/// `Outcome::Ambiguous`'s own candidate vector, `receipt.alternatives`, and
/// the wire encoding of both must share exactly one order: every operation
/// that publishes more than one alternative permutes its own candidates by
/// the same `crate::provenance::Published::order` `Forest::publish` used to
/// order `alternatives` ascending by rank (cost, ties broken by output
/// digest). This never reorders only at a wire boundary (see
/// `crate::wire::Receipt::to_node` and `AssistResult::to_node`).
fn assert_alternatives_share_one_rank_order<T>(
    values: &[T],
    receipt: &Receipt,
    digest_of: impl Fn(&T) -> Digest,
) {
    assert_eq!(
        values.len(),
        receipt.alternatives.len(),
        "one alternative ledger per candidate"
    );
    for (index, (value, ledger)) in values.iter().zip(&receipt.alternatives).enumerate() {
        assert_eq!(
            digest_of(value),
            ledger.output,
            "Outcome::Ambiguous[{index}] must name the same answer as receipt.alternatives[{index}]"
        );
    }
    for pair in receipt.alternatives.windows(2) {
        assert!(
            pair[0].cost < pair[1].cost
                || (pair[0].cost == pair[1].cost && pair[0].output <= pair[1].output),
            "alternatives must be non-decreasing by rank (ascending cost, ties by output digest): {:?} then {:?}",
            (pair[0].cost, pair[0].output),
            (pair[1].cost, pair[1].output),
        );
    }
    // The same correspondence must hold in the wire encoding: this is the
    // exact invariant that broke when alternatives were reordered only at
    // `Receipt::to_node`/`AssistResult::to_node` without also reordering
    // `Outcome::Ambiguous`'s own candidate vector.
    let node = receipt.to_node();
    let Some(Value::Array(wire_alternatives)) = node.get("alternatives").map(|entry| &entry.value)
    else {
        panic!("wire alternatives must be an array")
    };
    assert_eq!(wire_alternatives.len(), values.len());
    for (index, (value, wire)) in values.iter().zip(wire_alternatives).enumerate() {
        assert_eq!(
            wire.get("output").and_then(|entry| entry.as_str()),
            Some(digest_of(value).to_string().as_str()),
            "wire alternatives[{index}] must name the same answer as Outcome::Ambiguous[{index}]"
        );
    }
}

#[test]
fn json_recovery_alternatives_share_one_order_with_ambiguous_candidates() {
    let schema = Schema::compile(
        r#"{"type":"object","properties":{"x":{"type":"integer"}},"required":["x"]}"#,
    )
    .unwrap();
    let result = schema.recover(r#"{"x":1,"x":2}"#, RecoveryRequest::default());
    let Outcome::Ambiguous(choices) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(choices.len(), 2);
    assert_alternatives_share_one_rank_order(choices, &result.receipt, |node| {
        Digest::of(node.to_json().as_bytes())
    });
}

#[test]
fn intent_alternatives_share_one_order_with_ambiguous_candidates() {
    fn attributes(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect()
    }
    let domain = Domain {
        id: "provenance.drinks.v1".into(),
        rules: vec![
            Rule {
                id: "tea".into(),
                phrase: "tea".into(),
                assignments: attributes(&[("drink", "tea")]),
            },
            Rule {
                id: "warm".into(),
                phrase: "temperature".into(),
                assignments: attributes(&[("temperature", "hot")]),
            },
            Rule {
                id: "cold".into(),
                phrase: "temperature".into(),
                assignments: attributes(&[("temperature", "cold")]),
            },
        ],
        ..Domain::default()
    };
    let concepts = vec![
        Concept {
            id: "hot-tea".into(),
            label: "cup of hot tea".into(),
            attributes: attributes(&[("drink", "tea"), ("temperature", "hot")]),
        },
        Concept {
            id: "cold-tea".into(),
            label: "cup of iced tea".into(),
            attributes: attributes(&[("drink", "tea"), ("temperature", "cold")]),
        },
    ];
    let result = domain.resolve("tea temperature", &concepts, Budget::default());
    let Outcome::Ambiguous(answers) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(answers.len(), 2);
    assert_alternatives_share_one_rank_order(answers, &result.receipt, |answer| {
        Digest::of(answer.to_node().to_json().as_bytes())
    });
}

#[test]
fn paths_alternatives_share_one_order_with_ambiguous_candidates() {
    let index = PathIndex::new(
        [
            "src/reader.rs",
            "tests/reader.rs",
            "docs/README.md",
            "other/README.md",
            "src/writer.rs",
        ]
        .into_iter()
        .map(str::to_string),
        &mut Governor::new(Budget::default()),
    )
    .unwrap();
    let result = index.resolve("README.md", "", &MatchRequest::default());
    let Outcome::Ambiguous(answers) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(answers.len(), 2);
    assert_alternatives_share_one_rank_order(answers, &result.receipt, |answer| {
        Digest::of(answer.to_node().to_json().as_bytes())
    });
}

#[test]
fn matching_alternatives_share_one_order_with_ambiguous_candidates() {
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
    let Outcome::Ambiguous(answers) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(answers.len(), 2);
    assert_alternatives_share_one_rank_order(answers, &result.receipt, |answer| {
        Digest::of(answer.to_node().to_json().as_bytes())
    });
}

#[test]
fn exact_edit_alternatives_share_one_order_with_ambiguous_candidates() {
    let result = locate_governed("aaa", "aa", "x", &mut Governor::new(Budget::default()));
    let Outcome::Ambiguous(plans) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(plans.len(), 2);
    assert_alternatives_share_one_rank_order(plans, &result.receipt, |plan| {
        Digest::of(plan.to_node().to_json().as_bytes())
    });
}

#[test]
fn fuzzy_edit_alternatives_share_one_order_with_ambiguous_candidates() {
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
    assert_alternatives_share_one_rank_order(plans, &result.receipt, |plan| {
        Digest::of(plan.to_node().to_json().as_bytes())
    });
}

#[test]
fn unified_edit_alternatives_share_one_order_with_ambiguous_candidates() {
    let patch = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+A\n";
    let result = plan_unified_governed("a\na\n", patch, &mut Governor::new(Budget::default()));
    let Outcome::Ambiguous(plans) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(plans.len(), 2);
    assert_alternatives_share_one_rank_order(plans, &result.receipt, |plan| {
        Digest::of(plan.to_node().to_json().as_bytes())
    });
}
