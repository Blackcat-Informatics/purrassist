// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Receipt wire version 3: `total_scratch_charged` is present, retained
//! sources carry exactly one of `text` (valid UTF-8) or `bytes_hex`, a
//! golden fixture pins the exact wire shape and a real ambiguous run's exact
//! bytes, and every receipt shape this crate produces (resolved, best
//! guess, ambiguous, and unresolved) validates against
//! `docs/schema/receipt.schema.json`.
use purrassist_core::{
    Budget, Effort, Governor, Outcome, RECEIPT_VERSION, Receipt,
    digest::Digest,
    edits::locate_governed,
    json::{Node, Value},
    matching::{self, Candidate, MatchRequest, SelectionPolicy},
    schema::{Schema, recovery::RecoveryRequest},
    tool_call::{AmbiguityPolicy, Effects, Param, ToolDefinition, ToolRecoveryRequest, ToolSet},
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
fn receipt_version_is_three() {
    assert_eq!(RECEIPT_VERSION, 3);
}

#[test]
fn retained_sources_encode_text_or_lowercase_hex_and_validate() {
    let schema = receipt_schema();
    let invalid: &[u8] = &[b'{', 0xff, 0xC3, b'"', 0x00, b'}'];
    let mut receipt = Receipt::new(
        invalid,
        "test.bytes.v1",
        Effort::Standard,
        Budget::default(),
    );
    let mut governor = Governor::new(Budget::default());
    receipt.retain_source_bytes(invalid, &mut governor).unwrap();
    receipt.retain_source("caf\u{e9}", &mut governor).unwrap();
    assert_eq!(receipt.sources[0].bytes, invalid);
    assert_eq!(receipt.sources[0].text(), None);
    assert_eq!(receipt.sources[0].digest, Digest::of(invalid));
    assert_eq!(receipt.sources[1].text(), Some("caf\u{e9}"));

    let node = receipt.to_node();
    let binary = node.pointer("/sources/0").unwrap();
    assert_eq!(
        binary.get("bytes_hex").and_then(Node::as_str),
        Some("7bffc322007d")
    );
    assert!(binary.get("text").is_none());
    assert_eq!(
        binary.get("digest").and_then(Node::as_str),
        Some(Digest::of(invalid).to_string().as_str())
    );
    let text = node.pointer("/sources/1").unwrap();
    assert_eq!(text.get("text").and_then(Node::as_str), Some("caf\u{e9}"));
    assert!(text.get("bytes_hex").is_none());
    assert_valid(&schema, &node);

    // Exactly one of the two encodings: neither, both, or uppercase hex is rejected.
    for (label, source) in [
        (
            "neither",
            r#"{"digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}"#,
        ),
        (
            "both",
            r#"{"digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000","text":"a","bytes_hex":"61"}"#,
        ),
        (
            "uppercase",
            r#"{"digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000","bytes_hex":"FF"}"#,
        ),
        (
            "odd",
            r#"{"digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000","bytes_hex":"fff"}"#,
        ),
    ] {
        let mut mutated = node.clone();
        let Value::Object(members) = &mut mutated.value else {
            panic!("receipt is an object")
        };
        let sources = members
            .iter_mut()
            .find(|member| member.key == "sources")
            .unwrap();
        sources.value = purrassist_core::json::parse(&format!("[{source}]")).unwrap();
        assert!(
            !schema.validate(&mutated).unwrap().valid,
            "{label} source encoding must not validate"
        );
    }
}

#[test]
fn golden_ambiguous_json_recovery_receipt_matches_the_committed_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/receipts/v3-json-recover-ambiguous-duplicate-keys.json");
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

/// A tool-call recovery over output with invalid UTF-8: the bytes are
/// retained as `bytes_hex`, each invalid run is an itemized data loss, and
/// the exact receipt is committed.
#[test]
fn golden_tool_call_invalid_utf8_receipt_matches_the_committed_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/receipts/v3-tool-call-invalid-utf8.json");
    let fixture_text = std::fs::read_to_string(&path).expect("golden receipt fixture");
    let fixture = purrassist_core::json::parse(&fixture_text).expect("fixture is valid JSON");

    let tools = ToolSet::compile(
        [ToolDefinition::new("ls").effects(Effects::ReadOnly)],
        Budget::default(),
    )
    .unwrap();
    let mut output = b"hi \xff\xfe there ".to_vec();
    output.extend_from_slice(
        b"<tool_call>\n{\"name\": \"ls\", \"arguments\": {}}\n</tool_call>\n\xc3",
    );
    let result = tools.recover(&output, &ToolRecoveryRequest::default());
    assert!(matches!(result.outcome, Outcome::BestGuess(_)));
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
    assert_eq!(
        node.pointer("/sources/0/bytes_hex").and_then(Node::as_str),
        Some(
            "686920fffe207468657265203c746f6f6c5f63616c6c3e0a7b226e616d65223a20226c73222c2022617267756d656e7473223a207b7d7d0a3c2f746f6f6c5f63616c6c3e0ac3"
        )
    );
    assert_valid(&receipt_schema(), &node);
}

/// Every tool-call recovery receipt shape validates: resolved, ambiguity
/// rejected (with alternatives), and exhausted.
#[test]
fn tool_call_recovery_receipts_validate_against_the_receipt_schema() {
    let schema = receipt_schema();
    let tools = ToolSet::compile(
        [ToolDefinition::new("read")
            .effects(Effects::ReadOnly)
            .param("path", Param::string().required())],
        Budget::default(),
    )
    .unwrap();
    let resolved = tools.recover(
        br#"<tool_call>{"name": "read", "arguments": {"path": "a"}}</tool_call>"#,
        &ToolRecoveryRequest::default(),
    );
    assert!(matches!(resolved.outcome, Outcome::Resolved(_)));
    assert_valid(&schema, &resolved.receipt.to_node());
    let rejected = tools.recover(
        br#"<tool_call>{"name": "read", "arguments": {"path": "a", "path": "b"}}</tool_call>"#,
        &ToolRecoveryRequest::default().ambiguity(AmbiguityPolicy::Reject),
    );
    assert_eq!(rejected.outcome, Outcome::Unresolved);
    assert_eq!(rejected.receipt.alternatives.len(), 2);
    assert_valid(&schema, &rejected.receipt.to_node());
    let exhausted = tools.recover(
        b"<tool_call>",
        &ToolRecoveryRequest::default().budget(Budget {
            work: 3,
            ..Budget::default()
        }),
    );
    assert_eq!(exhausted.outcome, Outcome::Unresolved);
    assert_valid(&schema, &exhausted.receipt.to_node());
}
