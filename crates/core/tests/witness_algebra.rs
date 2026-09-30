// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Task 8 witness-generation algebra: coverage over the official Draft
//! 2020-12 suite, and closed-form unsatisfiability proofs.
use purrassist_core::json::{Node, Value, parse};
use purrassist_core::schema::{Schema, algebra::WitnessOutcome};
use purrassist_core::{Effort, Governor};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn files(directory: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(directory).expect("fixture directory") {
        let path = entry.expect("fixture entry").path();
        if path.is_dir() {
            paths.extend(files(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}
fn load_resources(root: &Path) -> BTreeMap<String, String> {
    let mut resources = BTreeMap::new();
    for path in files(&root.join("remotes")) {
        let relative = path
            .strip_prefix(root.join("remotes"))
            .expect("relative path")
            .to_string_lossy()
            .replace('\\', "/");
        resources.insert(
            format!("http://localhost:1234/{relative}"),
            std::fs::read_to_string(path).expect("resource"),
        );
    }
    resources
}

/// Every official suite schema group with at least one valid test instance
/// must yield a witness, validated internally before ever being returned
/// and re-verified here for the test's own confidence — never
/// [`WitnessOutcome::Unsatisfiable`] (a false claim about a schema known to
/// admit at least one instance) and never [`WitnessOutcome::Exhausted`]
/// either: that outcome stays legitimate in general (a schema genuinely
/// outside this algebra's scope), but no schema this census can compile and
/// that the suite itself marks satisfiable may hit it.
#[test]
fn official_suite_with_a_valid_instance_never_reports_unsatisfiable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/json-schema");
    let resources = load_resources(&root);
    let mut paths = files(&root.join("tests/draft2020-12"));
    // The suite's own remotes are exercised through `$ref`, not as their own
    // top-level census entries.
    paths.retain(|path| !path.starts_with(root.join("remotes")));
    assert!(!paths.is_empty(), "official suite must be vendored");

    let mut groups_with_valid_instance = 0usize;
    let mut witnessed = 0usize;
    let mut exhausted = 0usize;
    let mut falsely_unsat: Vec<String> = Vec::new();
    for path in &paths {
        let suite = parse(&std::fs::read_to_string(path).expect("suite file")).expect("suite json");
        let Value::Array(groups) = suite.value else {
            panic!("suite array")
        };
        for group in groups {
            let Value::Array(tests) = &group.get("tests").expect("tests").value else {
                panic!("tests array")
            };
            let has_valid_instance = tests
                .iter()
                .any(|test| test.get("valid").expect("valid").value == Value::Bool(true));
            if !has_valid_instance {
                continue;
            }
            let source = group.get("schema").expect("schema").to_json();
            let Ok(schema) = Schema::with_resources(&source, &resources) else {
                // A schema this suite marks valid-somewhere but that fails
                // to compile under our supported vocabulary set is out of
                // this census's scope (compile-time rejection, not a
                // witness-generation question).
                continue;
            };
            groups_with_valid_instance += 1;
            let mut governor = Governor::new(Effort::Maximum.budget());
            let outcome = schema
                .generate_witness_governed(&mut governor)
                .unwrap_or(WitnessOutcome::Exhausted);
            match outcome {
                WitnessOutcome::Witness(node) => {
                    assert!(
                        schema.validate(&node).expect("governed validate").valid,
                        "{}: witness {} does not validate",
                        path.display(),
                        node.to_json()
                    );
                    witnessed += 1;
                }
                WitnessOutcome::Exhausted => {
                    exhausted += 1;
                    eprintln!(
                        "  exhausted: {} / {}",
                        path.file_name().expect("name").to_string_lossy(),
                        group
                            .get("description")
                            .and_then(Node::as_str)
                            .unwrap_or("")
                    );
                }
                WitnessOutcome::Unsatisfiable(refutation) => falsely_unsat.push(format!(
                    "{} / {}: falsely reported unsatisfiable ({})",
                    path.file_name().expect("name").to_string_lossy(),
                    group
                        .get("description")
                        .and_then(Node::as_str)
                        .unwrap_or(""),
                    refutation.reason
                )),
            }
        }
    }
    eprintln!(
        "witness_algebra: {groups_with_valid_instance} schema groups with a valid instance; {witnessed} witnessed, {exhausted} exhausted, {} falsely unsatisfiable",
        falsely_unsat.len()
    );
    for line in &falsely_unsat {
        eprintln!("{line}");
    }
    assert!(
        falsely_unsat.is_empty(),
        "{} schema group(s) known to be satisfiable were reported unsatisfiable",
        falsely_unsat.len()
    );
    assert!(
        groups_with_valid_instance >= 200,
        "census sanity: expected at least 200 schema groups with a valid instance, saw {groups_with_valid_instance}"
    );
    // Every official-suite schema group with a valid instance must yield a
    // witness that validates — zero exhaustions, not merely a high witness
    // rate (see this function's own doc comment above).
    assert!(
        exhausted == 0 && witnessed == groups_with_valid_instance,
        "{witnessed}/{groups_with_valid_instance} witnessed, {exhausted} exhausted: every schema group with a valid instance must yield a witness (see the `exhausted:` lines above)"
    );
}

fn unsatisfiable(source: &str) -> String {
    let schema = Schema::compile(source).expect("schema compiles");
    match schema.generate_witness() {
        WitnessOutcome::Unsatisfiable(refutation) => refutation.reason,
        other => panic!("expected Unsatisfiable for {source}, got {other:?}"),
    }
}

#[test]
fn boolean_false_schema_is_proven_unsatisfiable() {
    unsatisfiable("false");
}

#[test]
fn allof_of_incompatible_types_is_proven_unsatisfiable() {
    unsatisfiable(r#"{"allOf":[{"type":"string"},{"type":"number"}]}"#);
}

#[test]
fn crossed_numeric_bounds_are_proven_unsatisfiable() {
    unsatisfiable(r#"{"type":"integer","minimum":2,"maximum":1}"#);
}

#[test]
fn pattern_and_its_own_negation_is_proven_unsatisfiable() {
    unsatisfiable(r#"{"type":"string","pattern":"^a$","not":{"pattern":"^a$"}}"#);
}

#[test]
fn const_outside_enum_is_proven_unsatisfiable() {
    unsatisfiable(r#"{"const":3,"enum":[1,2]}"#);
}

#[test]
fn required_property_with_an_unsatisfiable_schema_is_proven_unsatisfiable() {
    unsatisfiable(r#"{"type":"object","required":["x"],"properties":{"x":false}}"#);
}

/// A satisfiable schema (`type` intersected across `allOf` down to exactly
/// `integer`, with compatible bounds) must never be misreported
/// unsatisfiable just because it shares surface syntax with the negative
/// cases above.
#[test]
fn compatible_allof_types_still_yield_a_witness() {
    let schema =
        Schema::compile(r#"{"allOf":[{"type":"integer"},{"minimum":10}],"not":{"const":12}}"#)
            .unwrap();
    let WitnessOutcome::Witness(node) = schema.generate_witness() else {
        panic!("expected a witness")
    };
    assert!(schema.validate(&node).unwrap().valid);
}
