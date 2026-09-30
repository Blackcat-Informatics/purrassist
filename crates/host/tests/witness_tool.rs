// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `generate_json_witness`'s three outcomes reached through the host tool
//! surface (Task 10's witness-generation surface): a produced witness
//! actually validates against the requested schema through the sibling
//! `validate_json` tool, a proven-unsatisfiable outcome carries non-empty
//! refutation evidence, and a governor hard stop reached while generating
//! (not compiling) is reported as `"exhausted"`/`"budget_exceeded"`, never
//! `"unsatisfiable"`.
use purrassist::{
    json::{self, Value},
    wire::object,
};
use purrassist_host::tools::Tools;
use std::sync::atomic::{AtomicUsize, Ordering};

fn fresh_tools(tag: &str) -> Tools {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "purrassist-witness-tool-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    Tools::new(&root).unwrap()
}

#[test]
fn witness_outcome_validates_against_the_schema_via_validate_json() {
    let tools = fresh_tools("witness");
    let schema = json::parse(
        r#"{"type":"object","required":["a"],"properties":{"a":{"type":"integer","minimum":3}}}"#,
    )
    .unwrap();
    let result = tools
        .call(
            "generate_json_witness",
            &object([("schema", schema.clone())]),
        )
        .unwrap();
    assert_eq!(
        result.pointer("/outcome").and_then(json::Node::as_str),
        Some("witness")
    );
    let witness = result.get("witness").expect("witness field").clone();
    let validated = tools
        .call(
            "validate_json",
            &object([("schema", schema), ("instance", witness)]),
        )
        .unwrap();
    assert_eq!(validated.get("valid").unwrap().to_json(), "true");
    assert!(
        result.pointer("/receipt/usage").is_some(),
        "receipt must carry governor usage"
    );
}

#[test]
fn unsatisfiable_outcome_carries_nonempty_refutation_evidence() {
    let tools = fresh_tools("unsatisfiable");
    let args =
        json::parse(r#"{"schema":{"allOf":[{"type":"string"},{"type":"number"}]}}"#).unwrap();
    let result = tools.call("generate_json_witness", &args).unwrap();
    assert_eq!(
        result.pointer("/outcome").and_then(json::Node::as_str),
        Some("unsatisfiable")
    );
    assert_eq!(result.pointer("/witness").unwrap().to_json(), "null");
    let Value::Array(refutation) = &result.get("refutation").unwrap().value else {
        panic!("refutation must be an array")
    };
    assert!(
        !refutation.is_empty(),
        "an unsatisfiable outcome must carry refutation evidence"
    );
    assert_eq!(
        result
            .pointer("/receipt/termination/kind")
            .and_then(json::Node::as_str),
        Some("completed"),
        "proven unsatisfiability is a completed search, never a governor hard stop"
    );
}

#[test]
fn budget_exhaustion_while_generating_is_exhausted_and_budget_exceeded_not_unsatisfiable() {
    // A long, strictly progressing (non-cyclic) `$ref` chain, entirely flat in
    // `resources` (no deep literal JSON nesting, so argument validation itself
    // never approaches the depth budget): resolving it recurses the witness
    // algebra past the governor's structural recursion-depth limit, a
    // genuine `BudgetExceeded(Depth)` reached only while generating, after
    // compilation of every individual (shallow) resource already succeeded.
    const CHAIN: u32 = 140;
    let tools = fresh_tools("exhausted");
    let mut resources = Vec::with_capacity(CHAIN as usize + 1);
    for index in 0..CHAIN {
        resources.push((
            format!("urn:chain:{index}"),
            json::parse(&format!(r#"{{"$ref":"urn:chain:{}"}}"#, index + 1)).unwrap(),
        ));
    }
    resources.push((
        format!("urn:chain:{CHAIN}"),
        json::parse(r#"{"type":"integer"}"#).unwrap(),
    ));
    let args = object([
        ("schema", json::parse(r#"{"$ref":"urn:chain:0"}"#).unwrap()),
        ("resources", object(resources)),
    ]);
    let result = tools.call("generate_json_witness", &args).unwrap();
    assert_eq!(
        result.pointer("/outcome").and_then(json::Node::as_str),
        Some("exhausted"),
        "budget exhaustion must never be reported as a witness or as unsatisfiable"
    );
    assert_eq!(result.pointer("/refutation").unwrap().to_json(), "[]");
    assert_eq!(
        result
            .pointer("/receipt/termination/kind")
            .and_then(json::Node::as_str),
        Some("budget_exceeded")
    );
    assert_eq!(
        result
            .pointer("/receipt/termination/detail")
            .and_then(json::Node::as_str),
        Some("budget exceeded: Depth")
    );
}
