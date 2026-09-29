// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Composed recovery, ambiguity, inference, and resource regression cases.
use purrassist_core::{
    Budget, Effort, Outcome, StopReason, json,
    schema::{Schema, recovery::RecoveryRequest},
};

fn value(outcome: Outcome<json::Node>) -> json::Node {
    match outcome {
        Outcome::Resolved(value) | Outcome::BestGuess(value) => value,
        other => panic!("unexpected {other:?}"),
    }
}
#[test]
fn composed_nested_syntax_key_and_type_repairs() {
    let schema = Schema::compile(r#"{"type":"object","required":["temperature","drink"],"properties":{"temperature":{"type":"number"},"drink":{"enum":["tea","coffee"]}},"additionalProperties":false}"#).unwrap();
    let result = schema.recover(
        "```json\n[[{temperatur: '42', drink: 'Tea',}]]\n```",
        RecoveryRequest::default(),
    );
    let node = value(result.outcome);
    assert!(node.equivalent(&json::parse(r#"{"temperature":42,"drink":"tea"}"#).unwrap()));
    assert!(schema.validate(&node).unwrap().valid);
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .any(|e| e.rule == "json.key.match")
    );
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .any(|e| e.rule == "json.shape.unwrap-array")
    );
    assert!(
        result
            .receipt
            .losses
            .iter()
            .any(|loss| loss.original.contains("```"))
    );
    assert!(!result.receipt.contains_guesses());
    assert_eq!(
        result.receipt.sources[0].text,
        "```json\n[[{temperatur: '42', drink: 'Tea',}]]\n```"
    );
    assert_eq!(result.receipt.sources[0].digest, result.receipt.source);
}
#[test]
fn input_retention_obeys_admission_without_copying_rejected_input() {
    let schema = Schema::compile("true").unwrap();
    for budget in [
        Budget {
            input_bytes: 1,
            ..Budget::default()
        },
        Budget {
            scratch_bytes: 1,
            ..Budget::default()
        },
    ] {
        let result = schema.recover(
            "123",
            RecoveryRequest {
                budget: Some(budget),
                ..RecoveryRequest::default()
            },
        );
        assert_eq!(result.outcome, Outcome::Unresolved);
        assert_eq!(result.receipt.sources, Vec::new());
        assert!(matches!(
            result.receipt.stopped,
            StopReason::BudgetExceeded(_)
        ));
    }
}
#[test]
fn unrepresentable_invention_size_is_exhaustion_not_no_candidate() {
    let schema = Schema::compile(r#"{"type":"object","required":["x"],"properties":{"x":{"type":"string","minLength":1e100}}}"#).unwrap();
    let result = schema.recover("{}", RecoveryRequest::default());
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert!(matches!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(_)
    ));
}
#[test]
fn duplicate_values_remain_ambiguous() {
    let schema = Schema::compile(
        r#"{"type":"object","properties":{"x":{"type":"integer"}},"required":["x"]}"#,
    )
    .unwrap();
    let result = schema.recover(r#"{"x":1,"x":2}"#, RecoveryRequest::default());
    let Outcome::Ambiguous(choices) = &result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(choices.len(), 2);
    assert_eq!(result.receipt.alternatives.len(), choices.len());
    assert_eq!(result.receipt.evidence, Vec::new());
    assert_eq!(result.receipt.losses, Vec::new());
    for (choice, ledger) in choices.iter().zip(&result.receipt.alternatives) {
        assert_eq!(
            ledger.output,
            purrassist_core::digest::Digest::of(choice.to_json().as_bytes())
        );
        assert_eq!(ledger.cost, 10);
        assert_eq!(ledger.evidence.len(), 1);
        assert_eq!(
            ledger.evidence[0].explanation,
            format!("replace with {}", choice.to_json())
        );
        assert_eq!(ledger.losses[0].original, r#"{"x":1,"x":2}"#);
        assert_eq!(ledger.inferences, Vec::new());
        assert_eq!(ledger.validations.len(), 1);
        assert!(result.receipt.validations[ledger.validations[0]].passed);
    }
    let encoded = result.to_node();
    let json::Value::Array(ledgers) = &encoded
        .get("receipt")
        .unwrap()
        .get("alternatives")
        .unwrap()
        .value
    else {
        panic!("missing alternative ledgers");
    };
    assert_eq!(ledgers.len(), choices.len());
    for (ledger, choice) in ledgers.iter().zip(choices) {
        assert_eq!(
            ledger.get("output").unwrap().as_str().unwrap(),
            purrassist_core::digest::Digest::of(choice.to_json().as_bytes()).to_string()
        );
    }
    assert!(
        choices
            .iter()
            .all(|node| schema.validate(node).unwrap().valid)
    );
}
#[test]
fn ambiguous_guesses_keep_their_own_inferences() {
    let schema =
        Schema::compile(r#"{"type":"object","required":["x"],"properties":{"x":{"enum":[1,2]}}}"#)
            .unwrap();
    let result = schema.recover("{}", RecoveryRequest::default());
    let Outcome::Ambiguous(choices) = &result.outcome else {
        panic!("{result:?}")
    };
    assert!(result.receipt.contains_guesses());
    assert_eq!(result.receipt.inferences, Vec::new());
    for (choice, ledger) in choices.iter().zip(&result.receipt.alternatives) {
        assert_eq!(ledger.inferences.len(), 1);
        assert_eq!(ledger.inferences[0].value, choice.to_json());
    }
    for budget in [
        Budget {
            work: result.receipt.usage.work - 1,
            ..Budget::default()
        },
        Budget {
            evidence_entries: result.receipt.usage.evidence_entries - 1,
            ..Budget::default()
        },
    ] {
        let exhausted = schema.recover(
            "{}",
            RecoveryRequest {
                budget: Some(budget),
                ..RecoveryRequest::default()
            },
        );
        assert_eq!(exhausted.outcome, Outcome::Unresolved);
        assert!(matches!(
            exhausted.receipt.stopped,
            StopReason::BudgetExceeded(_)
        ));
        assert_eq!(exhausted.receipt.alternatives, Vec::new());
    }
}
#[test]
fn required_invention_is_a_guess_and_can_be_disabled() {
    let schema = Schema::compile(
        r#"{"type":"object","required":["answer"],"properties":{"answer":{"const":42}}}"#,
    )
    .unwrap();
    let result = schema.recover("{}", RecoveryRequest::default());
    assert!(matches!(result.outcome, Outcome::BestGuess(_)));
    assert!(result.receipt.contains_guesses());
    let request = RecoveryRequest {
        allow_guesses: false,
        ..RecoveryRequest::default()
    };
    assert_eq!(schema.recover("{}", request).outcome, Outcome::Unresolved);
}
#[test]
fn complete_schema_and_budget_are_never_weakened() {
    let schema =
        Schema::compile(r#"{"allOf":[{"type":"integer"},{"minimum":10}],"not":{"const":12}}"#)
            .unwrap();
    assert_eq!(
        schema.recover("\"12\"", RecoveryRequest::default()).outcome,
        Outcome::Unresolved
    );
    let request = RecoveryRequest {
        budget: Some(Budget {
            work: 0,
            ..Budget::default()
        }),
        ..RecoveryRequest::default()
    };
    let result = schema.recover("10", request);
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert!(matches!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(_)
    ));
    let request = RecoveryRequest {
        effort: Effort::Exact,
        ..RecoveryRequest::default()
    };
    assert_eq!(
        schema.recover("\"10\"", request).outcome,
        Outcome::Unresolved
    );
}
#[test]
fn truncated_containers_and_nested_stringification() {
    let schema = Schema::compile(r#"{"type":"object","required":["x"],"properties":{"x":{"type":"array","items":{"type":"integer"}}}}"#).unwrap();
    let result = schema.recover("{'x': [1, 2", RecoveryRequest::default());
    assert_eq!(value(result.outcome).to_json(), r#"{"x":[1,2]}"#);
    let schema = Schema::compile(r#"{"type":"integer"}"#).unwrap();
    assert_eq!(
        value(
            schema
                .recover(r#""\"42\"""#, RecoveryRequest::default())
                .outcome
        )
        .to_json(),
        "42"
    );
}
/// Task 6 wiring: recovery receipts name which engine evaluated each
/// distinct `pattern` this search actually validated.
#[test]
fn recovery_receipts_name_the_pattern_engine_used() {
    let derivative = Schema::compile(r#"{"type":"string","pattern":"^[a-z]+$"}"#).unwrap();
    let result = derivative.recover(r#""abc""#, RecoveryRequest::default());
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "pattern.engine.derivative-v1"),
        "receipt evidence: {:?}",
        result.receipt.evidence
    );
    let backtracking = Schema::compile(r#"{"type":"string","pattern":"^(a)\\1$"}"#).unwrap();
    let result = backtracking.recover(r#""aa""#, RecoveryRequest::default());
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "pattern.engine.backtrack-v1"),
        "receipt evidence: {:?}",
        result.receipt.evidence
    );
}
