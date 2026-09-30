// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Witness format assertions follow each location's dialect; annotations remain evidence.
use purrassist_core::{
    Effort, Governor, Outcome,
    json::{Node, parse},
    schema::{DIALECT, Schema, algebra::WitnessOutcome, recovery::RecoveryRequest},
};
use std::collections::BTreeMap;

const ASSERTED: &str = "urn:test:witness-format-asserted";
const OPTIONAL_ASSERTED: &str = "urn:test:witness-format-optional-asserted";

fn resources() -> BTreeMap<String, String> {
    [(ASSERTED, true), (OPTIONAL_ASSERTED, false)]
        .into_iter()
        .map(|(uri, required)| {
            (
                uri.to_string(),
                format!(
                    r#"{{"$vocabulary":{{
                        "https://json-schema.org/draft/2020-12/vocab/core":true,
                        "https://json-schema.org/draft/2020-12/vocab/validation":true,
                        "https://json-schema.org/draft/2020-12/vocab/applicator":true,
                        "https://json-schema.org/draft/2020-12/vocab/format-assertion":{required}
                    }}}}"#
                ),
            )
        })
        .collect()
}

fn witnessed(schema: &Schema, known_instance: &str) -> Node {
    let known = parse(known_instance).unwrap();
    assert!(schema.validate(&known).unwrap().valid);
    let mut governor = Governor::new(Effort::Maximum.budget());
    let outcome = schema.generate_witness_governed(&mut governor).unwrap();
    let WitnessOutcome::Witness(node) = outcome else {
        panic!("known valid instance {known_instance} produced {outcome:?}");
    };
    assert!(schema.validate(&node).unwrap().valid);
    node
}

fn refuted(schema: &Schema, invalid_instance: &str) {
    assert!(
        !schema
            .validate(&parse(invalid_instance).unwrap())
            .unwrap()
            .valid
    );
    let mut governor = Governor::new(Effort::Maximum.budget());
    let outcome = schema.generate_witness_governed(&mut governor).unwrap();
    assert!(
        matches!(outcome, WitnessOutcome::Unsatisfiable(_)),
        "{outcome:?}"
    );
}

#[test]
fn annotation_formats_do_not_refute_const_or_enum_candidates() {
    for source in [
        r#"{"const":"not an email","format":"email"}"#,
        r#"{"enum":["not an email"],"format":"email"}"#,
        r#"{"allOf":[{"const":"not an email"},{"format":"email"}]}"#,
    ] {
        let schema = Schema::compile(source).unwrap();
        let node = witnessed(&schema, r#""not an email""#);
        assert_eq!(node.as_str(), Some("not an email"));
        let report = schema.validate(&node).unwrap();
        assert!(
            report
                .format_evidence
                .iter()
                .any(|entry| { entry.validator == "schema.format.email-v1" && !entry.passed })
        );
    }
}

#[test]
fn annotation_formats_do_not_restrict_pattern_or_conditional_witnesses() {
    for source in [
        r#"{"type":"string","pattern":"^bad$","format":"email"}"#,
        r#"{"if":{"format":"email"},"then":{"const":"bad"},"else":false}"#,
        r#"{"type":"string","maxLength":0,"format":"email"}"#,
    ] {
        let sample = if source.contains("maxLength") {
            r#""""#
        } else {
            r#""bad""#
        };
        let schema = Schema::compile(source).unwrap();
        let node = witnessed(&schema, sample);
        assert!(
            schema
                .validate(&node)
                .unwrap()
                .format_evidence
                .iter()
                .any(|entry| { entry.validator == "schema.format.email-v1" && !entry.passed })
        );
    }
}

#[test]
fn negated_annotation_only_formats_are_not_format_complements() {
    for source in [
        r#"{"not":{"format":"email"}}"#,
        r#"{"const":"bad","not":{"type":"string","format":"email"}}"#,
        r#"{"not":{"format":"unknown-annotation"}}"#,
    ] {
        refuted(&Schema::compile(source).unwrap(), r#""bad""#);
    }
    let schema = Schema::compile(r#"{"not":{"type":"string","format":"email"}}"#).unwrap();
    witnessed(&schema, "null");
}

#[test]
fn supported_format_assertion_declarations_enforce_literals_and_negation() {
    for dialect in [ASSERTED, OPTIONAL_ASSERTED] {
        let schema = Schema::with_resources(
            &format!(r#"{{"$schema":"{dialect}","const":"bad","format":"email"}}"#),
            &resources(),
        )
        .unwrap();
        refuted(&schema, r#""bad""#);
        let schema = Schema::with_resources(
            &format!(r#"{{"$schema":"{dialect}","const":"bad","not":{{"format":"email"}}}}"#),
            &resources(),
        )
        .unwrap();
        witnessed(&schema, r#""bad""#);
        let schema = Schema::with_resources(
            &format!(
                r#"{{"$schema":"{dialect}","const":"a@example.com","not":{{"format":"email"}}}}"#
            ),
            &resources(),
        )
        .unwrap();
        refuted(&schema, r#""a@example.com""#);
    }
}

#[test]
fn nested_locations_inherit_assertion_and_can_override_it() {
    let inherited = Schema::with_resources(
        &format!(r#"{{"$schema":"{ASSERTED}","type":"object","required":["contact"],"properties":{{"contact":{{"const":"bad","format":"email"}}}}}}"#),
        &resources(),
    ).unwrap();
    refuted(&inherited, r#"{"contact":"bad"}"#);
    let overridden = Schema::with_resources(
        &format!(r#"{{"$schema":"{ASSERTED}","type":"object","required":["contact"],"properties":{{"contact":{{"$id":"urn:test:witness-annotated-child","$schema":"{DIALECT}","const":"bad","format":"email"}}}}}}"#),
        &resources(),
    ).unwrap();
    let node = witnessed(&overridden, r#"{"contact":"bad"}"#);
    let report = overridden.validate(&node).unwrap();
    assert!(
        report
            .format_evidence
            .iter()
            .any(|entry| { entry.constraint == "/contact" && !entry.passed })
    );
    let child_assertion = Schema::with_resources(
        &format!(r#"{{"type":"object","required":["contact"],"properties":{{"contact":{{"$id":"urn:test:witness-asserted-child","$schema":"{ASSERTED}","const":"bad","format":"email"}}}}}}"#),
        &resources(),
    ).unwrap();
    refuted(&child_assertion, r#"{"contact":"bad"}"#);
}

#[test]
fn referenced_resources_use_their_own_dialects() {
    let mut supplied = resources();
    supplied.insert(
        "urn:test:witness-annotated-resource".into(),
        format!(r#"{{"$schema":"{DIALECT}","const":"bad","format":"email"}}"#),
    );
    supplied.insert(
        "urn:test:witness-asserted-resource".into(),
        format!(r#"{{"$schema":"{ASSERTED}","const":"bad","format":"email"}}"#),
    );
    let annotation = Schema::with_resources(
        &format!(r#"{{"$schema":"{ASSERTED}","$ref":"urn:test:witness-annotated-resource"}}"#),
        &supplied,
    )
    .unwrap();
    witnessed(&annotation, r#""bad""#);
    let assertion = Schema::with_resources(
        r#"{"$ref":"urn:test:witness-asserted-resource"}"#,
        &supplied,
    )
    .unwrap();
    refuted(&assertion, r#""bad""#);
}

#[test]
fn recovering_an_annotated_invention_preserves_the_failed_observation() {
    let schema = Schema::compile(
        r#"{"type":"object","required":["contact"],"properties":{"contact":{"const":"bad","format":"email"}}}"#,
    ).unwrap();
    let result = schema.recover("{}", RecoveryRequest::default());
    let Outcome::BestGuess(node) = result.outcome else {
        panic!("expected a labeled invention: {:?}", result.outcome);
    };
    assert!(node.equivalent(&parse(r#"{"contact":"bad"}"#).unwrap()));
    assert!(result.receipt.validations.iter().any(|entry| {
        entry.validator == "schema.format.email-v1"
            && entry.constraint == "/contact"
            && !entry.passed
    }));
}

#[test]
fn unknown_annotation_formats_remain_observations() {
    let schema = Schema::compile(r#"{"const":"bad","format":"unknown-annotation"}"#).unwrap();
    let node = witnessed(&schema, r#""bad""#);
    let report = schema.validate(&node).unwrap();
    assert!(report.format_evidence.iter().any(|entry| {
        entry.validator == "schema.format.unknown-v1"
            && entry
                .diagnostics
                .iter()
                .any(|line| line.contains("unknown-annotation"))
    }));
}
