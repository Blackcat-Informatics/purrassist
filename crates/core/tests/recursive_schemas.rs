// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Task 8 recursive-schema audit: self-recursion, mutual recursion,
//! `$dynamicRef` recursion, non-progressing cycles, and compile/evaluation
//! resource exhaustion on deep structures — each an explicit outcome.
use purrassist_core::json::{Value, parse};
use purrassist_core::schema::CompilationError;
use purrassist_core::schema::{Schema, algebra::WitnessOutcome};
use purrassist_core::{Budget, Governor, StopReason};

/// A self-recursive linked-list-shaped schema terminates (the recursive
/// `next` edge is optional, so a witness can stop at the base case) and
/// still validates a genuinely nested instance.
#[test]
fn self_recursion_generates_and_validates() {
    let schema = Schema::compile(
        r#"{
            "$id": "urn:test:node",
            "type": "object",
            "properties": {
                "value": { "type": "integer" },
                "next": { "$ref": "urn:test:node" }
            },
            "required": ["value"]
        }"#,
    )
    .unwrap();
    let WitnessOutcome::Witness(node) = schema.generate_witness() else {
        panic!("expected a witness")
    };
    assert!(schema.validate(&node).unwrap().valid);
    let nested = parse(r#"{"value":1,"next":{"value":2,"next":{"value":3}}}"#).unwrap();
    assert!(schema.validate(&nested).unwrap().valid);
    let bad = parse(r#"{"value":1,"next":{"value":"not a number"}}"#).unwrap();
    assert!(!schema.validate(&bad).unwrap().valid);
}

/// Two schemas that refer to each other (never directly to themselves)
/// still terminate for both validation and witness generation.
#[test]
fn mutual_recursion_generates_and_validates() {
    let resources = std::collections::BTreeMap::from([(
        "urn:test:b".to_string(),
        r#"{
            "$id": "urn:test:b",
            "type": "object",
            "properties": {
                "label": { "type": "string" },
                "a": { "$ref": "urn:test:a" }
            },
            "required": ["label"]
        }"#
        .to_string(),
    )]);
    let schema = Schema::with_resources(
        r#"{
            "$id": "urn:test:a",
            "type": "object",
            "properties": {
                "count": { "type": "integer" },
                "b": { "$ref": "urn:test:b" }
            },
            "required": ["count"]
        }"#,
        &resources,
    )
    .unwrap();
    let WitnessOutcome::Witness(node) = schema.generate_witness() else {
        panic!("expected a witness")
    };
    assert!(schema.validate(&node).unwrap().valid);
    let nested = parse(r#"{"count":1,"b":{"label":"x","a":{"count":2}}}"#).unwrap();
    assert!(schema.validate(&nested).unwrap().valid);
}

/// The extensible-list `$dynamicRef`/`$dynamicAnchor` pattern from the
/// specification: a nested instance overriding the base `node` schema
/// resolves against the outermost matching dynamic anchor on the live
/// scope stack, not purely statically.
#[test]
fn dynamic_ref_recursion_resolves_against_the_live_scope() {
    let schema = Schema::compile(
        r##"{
            "$id": "urn:test:tree",
            "$dynamicAnchor": "node",
            "type": "object",
            "properties": {
                "data": true,
                "children": {
                    "type": "array",
                    "items": { "$dynamicRef": "#node" }
                }
            }
        }"##,
    )
    .unwrap();
    let instance = parse(r#"{"data":1,"children":[{"data":2,"children":[]},{"data":3}]}"#).unwrap();
    assert!(schema.validate(&instance).unwrap().valid);
    let bad = parse(r#"{"data":1,"children":[{"data":2,"children":"not an array"}]}"#).unwrap();
    assert!(!schema.validate(&bad).unwrap().valid);
    let WitnessOutcome::Witness(node) = schema.generate_witness() else {
        panic!("expected a witness")
    };
    assert!(schema.validate(&node).unwrap().valid);
    // Both signs of deferred dynamic edges retain the originating location.
    // The outer resource overrides the base anchor; only the root wrapper
    // requires `next`, so the positive recursive witness can terminate.
    for negated in [false, true] {
        let edge = if negated {
            r##"{"not":{"$dynamicRef":"#node"}}"##
        } else {
            r##"{"$dynamicRef":"#node"}"##
        };
        let resources = std::collections::BTreeMap::from([
            ("urn:test:base".into(), format!(r#"{{"$id":"urn:test:base","$dynamicAnchor":"node","type":"object","properties":{{"value":true,"next":{edge}}}}}"#)),
            ("urn:test:outer".into(), r#"{"$id":"urn:test:outer","$dynamicAnchor":"node","$ref":"urn:test:base","properties":{"value":{"type":"integer"}},"required":["value"]}"#.into()),
        ]);
        let schema = Schema::with_resources(
            r#"{"$ref":"urn:test:outer","required":["next"]}"#,
            &resources,
        )
        .unwrap();
        let WitnessOutcome::Witness(witness) = schema.generate_witness() else {
            panic!("dynamic witness, negated={negated}");
        };
        assert!(schema.validate(&witness).unwrap().valid);
        assert!(witness.get("next").is_some());
        if !negated {
            let input = r#"{"value":"2","next":{"value":"3"}}"#;
            assert!(!schema.validate(&parse(input).unwrap()).unwrap().valid);
            let recovered = schema.recover(
                input,
                purrassist_core::schema::recovery::RecoveryRequest::default(),
            );
            let purrassist_core::Outcome::Resolved(node) = recovered.outcome else {
                panic!("dynamic recovery: {:?}", recovered.outcome);
            };
            assert_eq!(node.to_json(), r#"{"value":2,"next":{"value":3}}"#);
        }
    }
}

/// A schema whose `$ref` targets itself at the same instance path can never
/// make progress: the evaluator's cycle guard reports this explicitly
/// (`InvalidInput`, never a hang, a panic, or a false pass/fail), and the
/// witness algebra's own guard reports the same non-progressing recursion as
/// an honest [`WitnessOutcome::Exhausted`], never a false witness or a false
/// claim of unsatisfiability.
#[test]
fn non_progressing_cycle_is_an_explicit_outcome() {
    let schema = Schema::compile(r##"{"$ref":"#"}"##).unwrap();
    let error = schema.validate(&parse("42").unwrap()).unwrap_err();
    assert_eq!(
        error,
        StopReason::InvalidInput("schema reference recurs without instance progress".into())
    );
    assert_eq!(schema.generate_witness(), WitnessOutcome::Exhausted);
}

/// A structurally deep schema (nested `allOf` wrapping, not a wide sibling
/// list) exhausts the compile-time `Depth` budget explicitly, rather than
/// stack-overflowing or silently truncating.
#[test]
fn deeply_nested_schema_exhausts_compile_time_depth_budget() {
    let mut source = r#"{"type":"integer"}"#.to_string();
    for _ in 0..500 {
        source = format!(r#"{{"allOf":[{source}]}}"#);
    }
    let mut governor = Governor::new(Budget {
        depth: 64,
        ..Budget::default()
    });
    let error = Schema::compile_governed(&source, &mut governor).unwrap_err();
    assert!(matches!(
        error,
        CompilationError::Resource(StopReason::BudgetExceeded(_))
    ));
}

/// A long `$ref` chain between distinct schema resources compiles (compiling
/// never eagerly follows `$ref`), but evaluating an instance against it
/// (which does follow the chain, one recursion level per hop) exhausts the
/// `Depth` budget explicitly.
#[test]
fn deep_reference_chain_exhausts_depth_budget_during_evaluation() {
    const HOPS: usize = 256;
    let mut resources = std::collections::BTreeMap::new();
    for hop in 0..HOPS {
        resources.insert(
            format!("urn:test:chain{hop}"),
            format!(
                r#"{{"$id":"urn:test:chain{hop}","$ref":"urn:test:chain{}"}}"#,
                hop + 1
            ),
        );
    }
    resources.insert(
        format!("urn:test:chain{HOPS}"),
        format!(r#"{{"$id":"urn:test:chain{HOPS}","type":"integer"}}"#),
    );
    let mut governor = Governor::new(Budget {
        depth: 64,
        // Lookup comparisons are admitted independently of recursive depth.
        work: 10_000_000,
        ..Budget::default()
    });
    let schema =
        Schema::with_resources_governed(r#"{"$ref":"urn:test:chain0"}"#, &resources, &mut governor)
            .unwrap();
    // The root-to-chain-0 route needs 257 levels beyond the root. Caller
    // limits below that boundary refuse, including the former native ceiling.
    for depth in [64, 256] {
        let mut governor = Governor::new(Budget {
            depth,
            work: 10_000_000,
            ..Budget::default()
        });
        let error = schema
            .validate_with(&parse("42").unwrap(), &mut governor)
            .unwrap_err();
        assert_eq!(
            error,
            StopReason::BudgetExceeded(purrassist_core::contract::Resource::Depth)
        );
    }
    for depth in [257, 512] {
        let mut governor = Governor::new(Budget {
            depth,
            work: 10_000_000,
            ..Budget::default()
        });
        assert!(
            schema
                .validate_with(&parse("42").unwrap(), &mut governor)
                .unwrap()
                .valid
        );
    }
    // Starting at chain-1 needs exactly 256 levels beyond the root.
    let mut governor = Governor::new(Budget {
        depth: 256,
        work: 10_000_000,
        ..Budget::default()
    });
    let exact =
        Schema::with_resources_governed(r#"{"$ref":"urn:test:chain1"}"#, &resources, &mut governor)
            .unwrap();
    assert!(
        exact
            .validate_with(&parse("42").unwrap(), &mut governor)
            .unwrap()
            .valid
    );
}

/// A non-recursive but still deep `allOf` chain has no cycle at all; the
/// evaluator and the witness algebra both terminate normally, distinguishing
/// "the same location recurs without progress" from "there are simply many
/// distinct nested locations".
#[test]
fn deep_but_finite_nesting_is_not_mistaken_for_a_cycle() {
    let mut source = r#"{"type":"integer","minimum":0}"#.to_string();
    for _ in 0..50 {
        source = format!(r#"{{"allOf":[{source}]}}"#);
    }
    let schema = Schema::compile(&source).unwrap();
    assert!(schema.validate(&parse("5").unwrap()).unwrap().valid);
    let WitnessOutcome::Witness(node) = schema.generate_witness() else {
        panic!("expected a witness")
    };
    let Value::Number(_) = node.value else {
        panic!("expected a number witness")
    };
}
