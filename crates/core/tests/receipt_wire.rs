// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Receipt wire version 2: `total_scratch_charged` is present, a golden
//! fixture pins the exact wire shape and a real ambiguous run's exact
//! bytes, and every receipt shape this crate produces (resolved, best
//! guess, ambiguous, and unresolved) validates against
//! `docs/schema/receipt.schema.json`.
use purrassist_core::{
    Budget, Governor, Outcome, RECEIPT_VERSION,
    edits::locate_governed,
    matching::{self, Candidate, MatchRequest, SelectionPolicy},
    schema::{Schema, recovery::RecoveryRequest},
};
use std::path::Path;

fn receipt_schema() -> Schema {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/schema/receipt.schema.json");
    let source = std::fs::read_to_string(path).expect("receipt schema fixture");
    Schema::compile(&source).expect("receipt schema compiles")
}

fn assert_valid(schema: &Schema, node: &purrassist_core::json::Node) {
    let report = schema.validate(node).unwrap();
    assert!(report.valid, "{:?}", report.violations);
}

#[test]
fn receipt_version_is_two() {
    assert_eq!(RECEIPT_VERSION, 2);
}

#[test]
fn golden_ambiguous_json_recovery_receipt_matches_the_committed_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/receipts/v2-json-recover-ambiguous-duplicate-keys.json");
    let fixture_text = std::fs::read_to_string(&path).expect("golden receipt fixture");
    let fixture = purrassist_core::json::parse(&fixture_text).expect("fixture is valid JSON");

    let schema = Schema::compile(
        r#"{"type":"object","properties":{"x":{"type":"integer"}},"required":["x"]}"#,
    )
    .unwrap();
    let result = schema.recover(r#"{"x":1,"x":2}"#, RecoveryRequest::default());
    assert!(matches!(result.outcome, Outcome::Ambiguous(_)));
    let node = result.receipt.to_node();

    assert!(
        node.equivalent(&fixture),
        "receipt wire shape drifted from the committed golden fixture at {}:\n\
         got:      {}\n\
         expected: {}",
        path.display(),
        node.to_json(),
        fixture.to_json()
    );
    assert_valid(&receipt_schema(), &node);
}

#[test]
fn ambiguous_receipts_carry_a_ranked_ambiguity_annotation() {
    // A tie between two candidates: the wire receipt must carry a top-level
    // `ambiguity` object naming the derivation count, how many of them
    // validated, and one distinguishing-node list per adjacent ranked pair.
    let ambiguous = matching::resolve(
        "reader",
        &[Candidate::new("a", "reader"), Candidate::new("b", "reader")],
        &MatchRequest::default(),
        SelectionPolicy {
            minimum_score: 10_000,
            minimum_margin: 0,
        },
    );
    assert!(matches!(ambiguous.outcome, Outcome::Ambiguous(_)));
    let node = ambiguous.receipt.to_node();
    assert_eq!(
        node.pointer("/ambiguity/derivations").unwrap().to_json(),
        "2"
    );
    assert_eq!(node.pointer("/ambiguity/valid").unwrap().to_json(), "2");
    let distinguishing = node.pointer("/ambiguity/distinguishing").unwrap();
    let purrassist_core::json::Value::Array(pairs) = &distinguishing.value else {
        panic!("distinguishing is an array")
    };
    assert_eq!(
        pairs.len(),
        1,
        "two alternatives have exactly one adjacent ranked pair"
    );
    // A single-answer receipt never carries an ambiguity annotation.
    let resolved = locate_governed(
        "hello",
        "hello",
        "world",
        &mut Governor::new(Budget::default()),
    );
    assert_eq!(
        resolved
            .receipt
            .to_node()
            .pointer("/ambiguity")
            .unwrap()
            .to_json(),
        "null"
    );
    assert_valid(&receipt_schema(), &ambiguous.receipt.to_node());
}

#[test]
fn every_outcome_shape_validates_against_the_receipt_schema() {
    let schema = receipt_schema();

    // Resolved: a byte-exact match.
    let resolved = locate_governed(
        "hello",
        "hello",
        "world",
        &mut Governor::new(Budget::default()),
    );
    assert!(matches!(resolved.outcome, Outcome::Resolved(_)));
    assert_valid(&schema, &resolved.receipt.to_node());

    // BestGuess: a unique fuzzy match.
    let guess = matching::resolve(
        "raeder",
        &[Candidate::new("reader", "reader")],
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    assert!(matches!(guess.outcome, Outcome::BestGuess(_)));
    assert_valid(&schema, &guess.receipt.to_node());

    // Ambiguous: a tie.
    let ambiguous = matching::resolve(
        "reader",
        &[Candidate::new("a", "reader"), Candidate::new("b", "reader")],
        &MatchRequest::default(),
        SelectionPolicy {
            minimum_score: 10_000,
            minimum_margin: 0,
        },
    );
    assert!(matches!(ambiguous.outcome, Outcome::Ambiguous(_)));
    assert_valid(&schema, &ambiguous.receipt.to_node());

    // Unresolved: no acceptable candidate.
    let unresolved = matching::resolve(
        "unrelated",
        &[Candidate::new("tea", "tea")],
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    assert_eq!(unresolved.outcome, Outcome::Unresolved);
    assert_valid(&schema, &unresolved.receipt.to_node());
}
