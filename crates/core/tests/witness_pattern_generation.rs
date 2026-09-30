// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! JSON Schema witness integration for assertion-sensitive pattern languages.
use purrassist_core::json::parse;
use purrassist_core::schema::{Schema, algebra::WitnessOutcome};

#[test]
fn satisfiable_assertion_and_exclusion_schemas_return_valid_witnesses() {
    for (source, known) in [
        (
            r#"{"type":"string","pattern":"^a+$","not":{"pattern":"^a$"}}"#,
            r#""aa""#,
        ),
        (
            r#"{"type":"string","pattern":"^a+$","minLength":2,"not":{"pattern":"^a$"}}"#,
            r#""aa""#,
        ),
        (
            r#"{"type":"string","pattern":"^a$","not":{"pattern":"^$"}}"#,
            r#""a""#,
        ),
        (r#"{"type":"string","pattern":"^a\\b$"}"#, r#""a""#),
        (r#"{"type":"string","pattern":"^[a-]\\b$"}"#, r#""a""#),
        (r#"{"type":"string","pattern":"\\Ba"}"#, r#""aa""#),
        (r#"{"type":"string","pattern":"a\\B"}"#, r#""aa""#),
    ] {
        let schema = Schema::compile(source).expect("schema compiles");
        assert!(
            schema
                .validate(&parse(known).expect("known instance"))
                .unwrap()
                .valid,
            "{source} admits {known}"
        );
        let outcome = schema.generate_witness();
        let WitnessOutcome::Witness(witness) = outcome else {
            panic!("satisfiable schema {source} yielded {outcome:?}")
        };
        assert!(schema.validate(&witness).unwrap().valid, "{source}");
    }
}

#[test]
fn impossible_anchored_word_assertion_retains_a_sound_refutation() {
    let schema = Schema::compile(r#"{"type":"string","pattern":"^a\\B$"}"#).unwrap();
    assert!(matches!(
        schema.generate_witness(),
        WitnessOutcome::Unsatisfiable(_)
    ));
}
