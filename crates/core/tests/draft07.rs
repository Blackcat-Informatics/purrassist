// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Draft-07 dialect selection, validator identity, witness generation, and
//! schema-guided recovery.
use purrassist_core::{
    Outcome,
    json::parse,
    schema::{Dialect, Schema, algebra::WitnessOutcome, recovery::RecoveryRequest},
};

const DRAFT_07: &str = "http://json-schema.org/draft-07/schema#";

fn draft07(body: &str) -> Schema {
    Schema::compile(&format!(r#"{{"$schema":"{DRAFT_07}",{body}}}"#)).unwrap()
}

fn valid(schema: &Schema, instance: &str) -> bool {
    schema.validate(&parse(instance).unwrap()).unwrap().valid
}

#[test]
fn both_uri_spellings_select_draft_07() {
    for uri in [DRAFT_07, "http://json-schema.org/draft-07/schema"] {
        let schema = Schema::compile(&format!(r#"{{"$schema":"{uri}"}}"#)).unwrap();
        assert_eq!(schema.dialect(), Dialect::Draft07, "{uri}");
    }
    assert_eq!(
        Schema::compile(r#"{"$schema":"https://json-schema.org/draft/2020-12/schema"}"#)
            .unwrap()
            .dialect(),
        Dialect::Draft202012
    );
    // Unimplemented dialects remain explicit compilation errors.
    let error =
        Schema::compile(r#"{"$schema":"http://json-schema.org/draft-04/schema#"}"#).unwrap_err();
    assert!(error.message.contains("unresolved dialect"), "{error}");
}

#[test]
fn draft_07_keywords_have_draft_07_meaning() {
    let tuple =
        draft07(r#""items":[{"type":"string"},{"type":"integer"}],"additionalItems":false"#);
    assert!(valid(&tuple, r#"["a",1]"#));
    assert!(!valid(&tuple, r#"["a",1,2]"#));
    assert!(!valid(&tuple, r#"[1,"a"]"#));
    let dependencies = draft07(r#""dependencies":{"a":["b"],"c":{"required":["d"]}}"#);
    assert!(valid(&dependencies, r#"{"a":1,"b":2}"#));
    assert!(!valid(&dependencies, r#"{"a":1}"#));
    assert!(!valid(&dependencies, r#"{"c":1}"#));
    // `$ref` overrides its siblings, including `$id`.
    let reference = draft07(
        r##""definitions":{"s":{"type":"string"}},"properties":{"x":{"$ref":"#/definitions/s","maxLength":1}}"##,
    );
    assert!(valid(&reference, r#"{"x":"long"}"#));
    // Plain-name `$id` fragments are anchors.
    let anchor = draft07(
        r##""definitions":{"n":{"$id":"#num","type":"number"}},"allOf":[{"$ref":"#num"}]"##,
    );
    assert!(valid(&anchor, "1"));
    assert!(!valid(&anchor, r#""x""#));
    // 2020-12-only keywords are unknown, hence ignored, under Draft-07.
    let foreign = draft07(
        r#""prefixItems":[{"type":"string"}],"unevaluatedProperties":false,"dependentRequired":{"a":["b"]},"minContains":2,"contains":{"type":"integer"}"#,
    );
    assert!(valid(&foreign, "[1]"));
    assert!(valid(&foreign, r#"{"a":1,"z":2}"#));
    // `format` is an annotation: recorded, never asserted.
    let format = draft07(r#""format":"email""#);
    let report = format.validate(&parse(r#""nope""#).unwrap()).unwrap();
    assert!(report.valid);
    assert!(!report.format_evidence[0].passed);
}

#[test]
fn validator_identity_and_digest_follow_the_dialect() {
    let schema = draft07(r#""type":"integer""#);
    let validation = schema
        .validate(&parse("1").unwrap())
        .unwrap()
        .to_validation();
    assert_eq!(validation.validator, "purrassist.schema.draft-07.v1");
    let other = Schema::compile(
        r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"integer"}"#,
    )
    .unwrap();
    assert_eq!(
        other
            .validate(&parse("1").unwrap())
            .unwrap()
            .to_validation()
            .validator,
        "purrassist.schema.draft2020-12.v1"
    );
    assert_ne!(schema.digest(), other.digest());
}

#[test]
fn draft_07_metaschema_is_built_in() {
    let meta = Schema::compile(&format!(r#"{{"$ref":"{DRAFT_07}"}}"#)).unwrap();
    assert!(valid(
        &meta,
        r#"{"items":[{"type":"string"}],"dependencies":{"a":["b"]}}"#
    ));
    assert!(!valid(&meta, r#"{"type":1}"#));
    assert!(!valid(&meta, r#"{"dependencies":{"a":[1]}}"#));
}

#[test]
fn witnesses_follow_draft_07_semantics() {
    let schema = draft07(
        r#""type":"array","items":[{"type":"string"},{"const":7}],"additionalItems":false,"minItems":2"#,
    );
    let WitnessOutcome::Witness(node) = schema.generate_witness() else {
        panic!("expected a witness")
    };
    assert!(schema.validate(&node).unwrap().valid);
    // Tuple `items` + `additionalItems` is the same algebra as 2020-12
    // `prefixItems` + `items`: identical outcomes, including refutations.
    for (draft07_body, draft2020) in [
        (
            r#""items":[{"type":"string"}],"additionalItems":false,"minItems":2"#,
            r#"{"prefixItems":[{"type":"string"}],"items":false,"minItems":2}"#,
        ),
        (
            r#""type":"array","items":[false],"minItems":1"#,
            r#"{"type":"array","prefixItems":[false],"minItems":1}"#,
        ),
        (
            r#""items":[{"type":"string"}],"additionalItems":{"type":"integer"},"minItems":3"#,
            r#"{"prefixItems":[{"type":"string"}],"items":{"type":"integer"},"minItems":3}"#,
        ),
        (
            r#""type":"object","required":["a"],"dependencies":{"a":["b"]}"#,
            r#"{"type":"object","required":["a"],"dependentRequired":{"a":["b"]}}"#,
        ),
        (
            r#""type":"object","required":["a"],"dependencies":{"a":{"required":["c"]}}"#,
            r#"{"type":"object","required":["a"],"dependentSchemas":{"a":{"required":["c"]}}}"#,
        ),
        (
            r#""type":"object","required":["a"],"dependencies":{"a":false}"#,
            r#"{"type":"object","required":["a"],"dependentSchemas":{"a":false}}"#,
        ),
    ] {
        let legacy = draft07(draft07_body).generate_witness();
        let current = Schema::compile(draft2020).unwrap().generate_witness();
        assert_eq!(
            core::mem::discriminant(&legacy),
            core::mem::discriminant(&current),
            "{draft07_body}: {legacy:?} vs {current:?}"
        );
        assert_eq!(legacy, current, "{draft07_body}");
    }
    // `$ref` siblings are ignored, so this is satisfiable under Draft-07.
    let overridden = draft07(
        r##""definitions":{"s":{"type":"string"}},"$ref":"#/definitions/s","type":"integer""##,
    );
    let WitnessOutcome::Witness(node) = overridden.generate_witness() else {
        panic!("expected a witness")
    };
    assert!(overridden.validate(&node).unwrap().valid);
}

#[test]
fn recovery_follows_draft_07_applicators_and_names_its_validator() {
    let schema = draft07(
        r#""type":"array","items":[{"type":"string"},{"type":"object","properties":{"mode":{"enum":["fast","slow"]}}}],"additionalItems":false"#,
    );
    let result = schema.recover(r#"["a",{"mode":"fsat"}]"#, RecoveryRequest::default());
    let (Outcome::Resolved(node) | Outcome::BestGuess(node)) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(node.to_json(), r#"["a",{"mode":"fast"}]"#);
    assert!(
        result
            .receipt
            .validations
            .iter()
            .any(|entry| entry.validator == "purrassist.schema.draft-07.v1" && entry.passed)
    );
}
