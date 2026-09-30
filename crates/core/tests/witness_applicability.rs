// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Type-conditional witness constraints, complements, and contains occurrences.
use purrassist_core::{
    Effort, Governor,
    json::{Node, parse},
    schema::{Schema, algebra::WitnessOutcome},
};

fn witness(source: &str, known_instance: &str) -> Node {
    let schema = Schema::compile(source).expect("valid schema");
    assert!(
        schema
            .validate(&parse(known_instance).unwrap())
            .unwrap()
            .valid,
        "the regression must establish satisfiability: {source} / {known_instance}"
    );
    let mut governor = Governor::new(Effort::Maximum.budget());
    let outcome = schema.generate_witness_governed(&mut governor).unwrap();
    let WitnessOutcome::Witness(node) = outcome else {
        panic!("satisfiable schema {source} returned {outcome:?}");
    };
    assert!(schema.validate(&node).unwrap().valid);
    node
}

fn unsatisfiable(source: &str) {
    let schema = Schema::compile(source).expect("valid schema");
    let mut governor = Governor::new(Effort::Maximum.budget());
    assert!(
        matches!(
            schema.generate_witness_governed(&mut governor).unwrap(),
            WitnessOutcome::Unsatisfiable(_)
        ),
        "expected a supported refutation: {source}"
    );
}

#[test]
fn crossed_bounds_exclude_only_the_affected_shape() {
    for (constraints, affected, samples) in [
        (
            r#""minimum":5,"maximum":1"#,
            "number",
            ["null", "false", "\"text\"", "[]", "{}"],
        ),
        (
            r#""minLength":5,"maxLength":1"#,
            "string",
            ["null", "false", "42", "[]", "{}"],
        ),
        (
            r#""minItems":5,"maxItems":1"#,
            "array",
            ["null", "false", "42", "\"text\"", "{}"],
        ),
        (
            r#""minProperties":5,"maxProperties":1"#,
            "object",
            ["null", "false", "42", "\"text\"", "[]"],
        ),
    ] {
        witness(&format!("{{{constraints}}}"), "null");
        witness(
            &format!(r#"{{"type":["{affected}","null"],{constraints}}}"#),
            "null",
        );
        for sample in samples {
            let schema = format!(r#"{{"const":{sample},{constraints}}}"#);
            assert!(witness(&schema, sample).equivalent(&parse(sample).unwrap()));
            witness(&format!(r#"{{"enum":[{sample}],{constraints}}}"#), sample);
        }
        unsatisfiable(&format!(r#"{{"type":"{affected}",{constraints}}}"#));
    }
}

#[test]
fn later_allof_and_reference_types_preserve_unaffected_literals() {
    for source in [
        r#"{"allOf":[{"minLength":5},{"maxLength":1},{"type":"null"}]}"#,
        r#"{"type":["number","null"],"enum":[3,null],"allOf":[{"minimum":5},{"maximum":1}]}"#,
        r##"{"$defs":{"bounds":{"minItems":5,"maxItems":1}},"$ref":"#/$defs/bounds","const":null}"##,
        r#"{"anyOf":[{"type":"number","minimum":5,"maximum":1},{"const":null}]}"#,
        r#"{"oneOf":[{"type":"number","minimum":5,"maximum":1},{"const":null}]}"#,
        r#"{"if":{"minLength":5,"maxLength":1},"then":false,"else":{"const":"x"},"type":"string"}"#,
    ] {
        let sample = if source.contains("\"else\"") {
            "\"x\""
        } else {
            "null"
        };
        witness(source, sample);
    }
}

#[test]
fn equal_numeric_bounds_keep_the_stricter_exclusivity_in_either_order() {
    for bound in ["exclusiveMinimum", "exclusiveMaximum"] {
        for source in [
            format!(r#"{{"type":"number","minimum":1,"maximum":1,"{bound}":1}}"#),
            format!(r#"{{"type":"number","allOf":[{{"{bound}":1}},{{"minimum":1,"maximum":1}}]}}"#),
        ] {
            unsatisfiable(&source);
            witness(
                &source.replace("\"number\"", "[\"number\",\"null\"]"),
                "null",
            );
        }
    }
}

#[test]
fn complemented_shape_keywords_require_their_own_shape() {
    for (constraint, admitted_sample, other_shape) in [
        (r#""minimum":1"#, "0", "null"),
        (r#""maximum":0"#, "1", "\"text\""),
        (r#""multipleOf":2"#, "1", "null"),
        (r#""minLength":1"#, "\"\"", "null"),
        (r#""maxLength":0"#, "\"a\"", "42"),
        (r#""minItems":1"#, "[]", "null"),
        (r#""maxItems":0"#, "[0]", "{}"),
        (r#""uniqueItems":true"#, "[0,0]", "\"text\""),
        (r#""minProperties":1"#, "{}", "null"),
        (r#""maxProperties":0"#, "{\"extra0\":0}", "[]"),
        (r#""pattern":"^a""#, "\"b\"", "null"),
    ] {
        witness(&format!(r#"{{"not":{{{constraint}}}}}"#), admitted_sample);
        unsatisfiable(&format!(
            r#"{{"const":{other_shape},"not":{{{constraint}}}}}"#
        ));
    }
}

#[test]
fn contains_bounds_apply_only_to_their_own_occurrence() {
    for source in [
        r#"{"type":"array","minContains":5,"maxContains":1}"#,
        r#"{"const":[],"allOf":[{"minContains":5},{"maxContains":1}]}"#,
        r#"{"type":"array","contains":false,"minContains":0,"maxContains":0}"#,
    ] {
        witness(source, "[]");
    }
    witness(
        r#"{"type":"array","allOf":[{"contains":{"const":1},"minContains":1,"maxContains":1},{"contains":{"const":2},"minContains":2,"maxContains":2}]}"#,
        "[1,2,2]",
    );
    witness(
        r#"{"type":"array","maxItems":2,"allOf":[{"contains":{"const":1},"minContains":1,"maxContains":1},{"contains":{"type":"integer"},"minContains":2,"maxContains":2}]}"#,
        "[1,0]",
    );
    witness(
        r#"{"type":"array","allOf":[{"contains":{"const":1}},{"minContains":9,"maxContains":0}]}"#,
        "[1]",
    );
    witness(
        r#"{"type":["array","null"],"contains":true,"minContains":2,"maxContains":1}"#,
        "null",
    );
    unsatisfiable(r#"{"type":"array","contains":true,"minContains":2,"maxContains":1}"#);
    unsatisfiable(r#"{"type":"array","contains":true,"maxContains":0}"#);
}

#[test]
fn optional_prefix_positions_are_not_existence_requirements() {
    for source in [
        r#"{"type":"array","prefixItems":[false]}"#,
        r#"{"type":"array","maxItems":0,"prefixItems":[false]}"#,
        r#"{"type":"array","allOf":[{"prefixItems":[false]},{"items":false}]}"#,
    ] {
        witness(source, "[]");
    }
    witness(
        r#"{"type":"array","minItems":1,"maxItems":1,"prefixItems":[{"const":1},false]}"#,
        "[1]",
    );
    witness(
        r#"{"type":"array","maxItems":1,"prefixItems":[{"const":1}],"contains":{"const":1}}"#,
        "[1]",
    );
    witness(
        r#"{"type":"array","minItems":1,"maxItems":1,"items":{"type":"integer"},"contains":{"const":3}}"#,
        "[3]",
    );
}
