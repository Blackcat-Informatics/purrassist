// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Task 7 requirement 5: explicit unsupported-`$vocabulary` behavior.
use purrassist_core::{
    Budget, Governor, StopReason,
    contract::Resource,
    json,
    schema::{CompilationError, Schema, recovery::RecoveryRequest},
};
use std::collections::BTreeMap;

const REQUIRED_UNKNOWN_DIALECT: &str = "urn:test:required-unknown-vocab";
const OPTIONAL_UNKNOWN_DIALECT: &str = "urn:test:optional-unknown-vocab";
const UNKNOWN_URI: &str = "https://example.invalid/vocab/x";

fn resources() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            REQUIRED_UNKNOWN_DIALECT.into(),
            format!(
                r#"{{"$vocabulary":{{
                    "https://json-schema.org/draft/2020-12/vocab/core": true,
                    "https://json-schema.org/draft/2020-12/vocab/validation": true,
                    "{UNKNOWN_URI}": true
                }}}}"#
            ),
        ),
        (
            OPTIONAL_UNKNOWN_DIALECT.into(),
            format!(
                r#"{{"$vocabulary":{{
                    "https://json-schema.org/draft/2020-12/vocab/core": true,
                    "https://json-schema.org/draft/2020-12/vocab/validation": true,
                    "{UNKNOWN_URI}": false
                }}}}"#
            ),
        ),
    ])
}

/// Acceptance: a meta-schema declaring `{"https://example.invalid/vocab/x":
/// true}` makes `Schema::compile` return that error.
#[test]
fn required_unknown_vocabulary_fails_to_compile() {
    let source = format!(r#"{{"$schema":"{REQUIRED_UNKNOWN_DIALECT}","type":"object"}}"#);
    let error =
        Schema::with_resources(&source, &resources()).expect_err("must reject the unknown URI");
    assert!(
        error.message.contains("unsupported required vocabulary"),
        "unexpected message: {error}"
    );
    assert!(error.message.contains(UNKNOWN_URI));
    let mut measured = Governor::new(Budget::default());
    assert_eq!(
        Schema::with_resources_governed(&source, &resources(), &mut measured).unwrap_err(),
        CompilationError::InvalidSchema(error)
    );
    let mut limited = Governor::new(Budget {
        work: measured.usage().work - 1,
        ..Budget::default()
    });
    assert_eq!(
        Schema::with_resources_governed(&source, &resources(), &mut limited).unwrap_err(),
        CompilationError::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
}

/// Acceptance: the same URI with `false` compiles, recovery of an invalid
/// instance still reports the core-vocabulary violation, and the receipt
/// contains the `unsupported-vocabulary` evidence entry for that URI
/// listing the schema keywords it left unevaluated.
#[test]
fn optional_unknown_vocabulary_compiles_and_still_validates_known_keywords() {
    let source = format!(
        r#"{{"$schema":"{OPTIONAL_UNKNOWN_DIALECT}","type":"object","required":["a"],"customKeyword":"ignored-by-us"}}"#
    );
    let schema = Schema::with_resources(&source, &resources())
        .expect("an optional unknown vocabulary must not block compilation");

    assert_eq!(
        schema.unsupported_vocabularies(),
        vec![purrassist_core::schema::UnsupportedVocabulary {
            uri: UNKNOWN_URI.into(),
            keywords: vec!["customKeyword".into()],
        }]
    );

    // The core `required` keyword (a supported vocabulary) is still fully
    // enforced: an instance missing `a` is genuinely invalid.
    let report = schema.validate(&json::parse("{}").unwrap()).unwrap();
    assert!(!report.valid);
    assert!(
        report
            .violations
            .iter()
            .any(|violation| violation.keyword == "required")
    );

    let result = schema.recover("{}", RecoveryRequest::default());
    assert!(
        result.receipt.evidence.iter().any(|evidence| evidence.rule
            == "schema.vocabulary.unsupported-v1"
            && evidence.destination == UNKNOWN_URI
            && evidence.explanation.contains("customKeyword")),
        "receipt evidence: {:?}",
        result.receipt.evidence
    );
}

/// Every other (supported) vocabulary keeps validating normally alongside
/// an unsupported optional one.
#[test]
fn other_vocabularies_remain_fully_validated() {
    let source =
        format!(r#"{{"$schema":"{OPTIONAL_UNKNOWN_DIALECT}","type":"string","minLength":3}}"#);
    let schema = Schema::with_resources(&source, &resources()).unwrap();
    assert!(
        !schema
            .validate(&json::parse(r#""ab""#).unwrap())
            .unwrap()
            .valid
    );
    assert!(
        schema
            .validate(&json::parse(r#""abc""#).unwrap())
            .unwrap()
            .valid
    );
}
