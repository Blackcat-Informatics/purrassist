// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Task 7: explicit `format` behavior independent of the official suite
//! (`schema_conformance.rs` already runs the vendored `optional/format/`
//! corpus under assertion). Compile-time unknown-format-under-assertion
//! failure, always-on annotation evidence without assertion, per-format
//! smoke coverage, and `date`/`date-time`/`time` recovery repairs.
use purrassist_core::{
    Outcome,
    json::{self, Node},
    schema::{Schema, recovery::RecoveryRequest},
};
use std::collections::BTreeMap;

const ASSERTION_DIALECT: &str = "urn:test:format-assertion";

fn assertion_resources() -> BTreeMap<String, String> {
    BTreeMap::from([(
        ASSERTION_DIALECT.into(),
        r#"{"$vocabulary":{
            "https://json-schema.org/draft/2020-12/vocab/core": true,
            "https://json-schema.org/draft/2020-12/vocab/applicator": true,
            "https://json-schema.org/draft/2020-12/vocab/unevaluated": true,
            "https://json-schema.org/draft/2020-12/vocab/validation": true,
            "https://json-schema.org/draft/2020-12/vocab/meta-data": true,
            "https://json-schema.org/draft/2020-12/vocab/format-assertion": true
        }}"#
        .into(),
    )])
}

fn under_assertion(format: &str) -> String {
    format!(r#"{{"$schema":"{ASSERTION_DIALECT}","format":"{format}"}}"#)
}

/// Requirement 3: an unrecognized format under an active format-assertion
/// vocabulary is a compile-time error naming the format.
#[test]
fn unknown_format_under_assertion_fails_to_compile() {
    let error = Schema::with_resources(
        &under_assertion("not-a-real-format"),
        &assertion_resources(),
    )
    .expect_err("unknown format under assertion must not compile");
    assert!(
        error
            .message
            .contains("unknown format under format-assertion"),
        "unexpected message: {error}"
    );
    assert!(error.message.contains("not-a-real-format"));
}

/// Requirement 1: format is always evaluated as annotation evidence, even
/// without format-assertion, and a mismatched value still validates.
#[test]
fn annotation_evidence_is_present_without_assertion() {
    let schema = Schema::compile(r#"{"type":"string","format":"ipv4"}"#).unwrap();
    let report = schema
        .validate(&json::parse(r#""not-an-ipv4""#).unwrap())
        .unwrap();
    assert!(
        report.valid,
        "no assertion: an invalid format must not fail"
    );
    let entry = report
        .format_evidence
        .iter()
        .find(|entry| entry.validator == "schema.format.ipv4-v1")
        .expect("format annotation evidence recorded");
    assert!(!entry.passed);
    assert_eq!(entry.constraint, "");
}

/// Requirement 1 (converse): a conforming value's annotation evidence
/// reports `passed`.
#[test]
fn annotation_evidence_reports_pass_for_conforming_values() {
    let schema = Schema::compile(r#"{"type":"string","format":"ipv4"}"#).unwrap();
    let report = schema
        .validate(&json::parse(r#""127.0.0.1""#).unwrap())
        .unwrap();
    assert!(report.valid);
    let entry = report
        .format_evidence
        .iter()
        .find(|entry| entry.validator == "schema.format.ipv4-v1")
        .expect("format annotation evidence recorded");
    assert!(entry.passed);
}

/// Requirement 3 (annotation half): without format-assertion active, an
/// unknown format compiles fine and is reported as "not evaluated", never
/// an assertion failure (the assertion-active half — a compile-time error —
/// is `unknown_format_under_assertion_fails_to_compile` above).
#[test]
fn unknown_format_is_annotation_only_without_assertion() {
    let schema = Schema::compile(r#"{"type":"string","format":"totally-unknown"}"#).unwrap();
    let report = schema
        .validate(&json::parse(r#""anything""#).unwrap())
        .unwrap();
    assert!(report.valid);
    assert!(
        report
            .format_evidence
            .iter()
            .any(|entry| entry.validator == "schema.format.unknown-v1")
    );
}

fn assert_format(name: &str, valid_value: &str, invalid_value: &str) {
    let schema = Schema::with_resources(&under_assertion(name), &assertion_resources())
        .unwrap_or_else(|error| panic!("{name} must compile under format-assertion: {error}"));
    let valid = schema.validate(&json::parse(valid_value).unwrap()).unwrap();
    assert!(
        valid.valid,
        "{name}: {valid_value} should satisfy the format"
    );
    let invalid = schema
        .validate(&json::parse(invalid_value).unwrap())
        .unwrap();
    assert!(
        !invalid.valid,
        "{name}: {invalid_value} should violate the format"
    );
    assert!(
        invalid
            .violations
            .iter()
            .any(|violation| violation.keyword == "format")
    );
}

#[test]
fn every_format_asserts_at_least_one_pass_and_one_fail() {
    assert_format(
        "date-time",
        r#""2020-01-01T00:00:00Z""#,
        r#""not-a-date-time""#,
    );
    assert_format("date", r#""2020-01-01""#, r#""2020-13-01""#);
    assert_format("time", r#""08:30:06Z""#, r#""25:00:00Z""#);
    assert_format("duration", r#""P1D""#, r#""1D""#);
    assert_format("email", r#""joe.bloggs@example.com""#, r#""not an email""#);
    assert_format("idn-email", r#""δοκιμή@example.com""#, r#""@""#);
    assert_format("hostname", r#""example.com""#, r#""-bad""#);
    assert_format("idn-hostname", r#""example.com""#, r#""-bad""#);
    assert_format("ipv4", r#""127.0.0.1""#, r#""999.0.0.1""#);
    assert_format("ipv6", r#""::1""#, r#""not-ipv6""#);
    assert_format("uri", r#""http://example.com""#, r#""not a uri""#);
    assert_format("uri-reference", r#""/abc""#, r#""not a uri\n""#);
    assert_format("iri", r#""http://例え.com""#, r#""not an iri""#);
    assert_format("iri-reference", r#""/例え""#, r#""\\bad""#);
    assert_format(
        "uuid",
        r#""2eb8aa08-aa98-11ea-b4aa-73b441d16380""#,
        r#""not-a-uuid""#,
    );
    assert_format("uri-template", r#""{+var}""#, r#""{}""#);
    assert_format("json-pointer", r#""/foo/bar""#, r#""foo/bar""#);
    assert_format("relative-json-pointer", r#""1/foo""#, r#""/foo""#);
    assert_format("regex", r#""[a-z]+""#, r#""[a-z""#);
}

/// Requirement 4: `date`/`date-time`/`time` violations under an active
/// format-assertion vocabulary are repaired as labeled `Guess` inferences.
fn repaired_value(source: &str, format: &str) -> Node {
    let schema = Schema::with_resources(&under_assertion(format), &assertion_resources()).unwrap();
    let result = schema.recover(&format!("{source:?}"), RecoveryRequest::default());
    match result.outcome {
        Outcome::BestGuess(value) | Outcome::Resolved(value) => value,
        other => panic!(
            "expected a repaired value, got {other:?}; receipt={:?}",
            result.receipt
        ),
    }
}
#[test]
fn date_repair_zero_pads_a_loose_month_and_day() {
    let repaired = repaired_value("2026-9-28", "date");
    assert_eq!(repaired.as_str(), Some("2026-09-28"));
}
#[test]
fn date_time_repair_normalizes_a_space_separator() {
    let repaired = repaired_value("2026-09-28 10:00:00Z", "date-time");
    assert_eq!(repaired.as_str(), Some("2026-09-28T10:00:00Z"));
}
#[test]
fn time_repair_zero_pads_loose_components() {
    let repaired = repaired_value("8:3:6Z", "time");
    assert_eq!(repaired.as_str(), Some("08:03:06Z"));
}
#[test]
fn repairs_are_labeled_guess_inferences() {
    let schema = Schema::with_resources(&under_assertion("date"), &assertion_resources()).unwrap();
    let result = schema.recover("\"2026-9-28\"", RecoveryRequest::default());
    assert!(matches!(result.outcome, Outcome::BestGuess(_)));
    assert!(result.receipt.contains_guesses());
    assert!(
        result
            .receipt
            .inferences
            .iter()
            .any(|inference| inference.evidence.rule == "json.schema.format-repair")
    );
}

/// Requirement 4: a non-repairable format violation (e.g. `email`) is never
/// invented; recovery yields an explicit non-success, not a fabricated fix.
#[test]
fn non_repairable_format_violations_are_never_invented() {
    let schema = Schema::with_resources(&under_assertion("email"), &assertion_resources()).unwrap();
    let result = schema.recover("\"not-an-email\"", RecoveryRequest::default());
    assert!(matches!(result.outcome, Outcome::Unresolved));
}
