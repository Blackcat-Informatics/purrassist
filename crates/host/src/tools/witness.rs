// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `generate_json_witness`: governed, reachable access to
//! [`purrassist_core::schema::Schema::generate_witness_governed`] — before
//! this tool, `Schema::generate_witness`/`generate_witness_governed` had
//! only `crate::schema::recovery` as a production consumer (the
//! witness-generation surface). Compilation (including any caller-supplied
//! `resources`) and generation share exactly one governor call; a governor
//! hard stop (`BudgetExceeded`/`Cancelled`) is reported as `"exhausted"`
//! (never `"unsatisfiable"` — see [`purrassist::schema::algebra`]'s module
//! documentation), distinguished from a deliberately incomplete search only
//! by the receipt's own `termination`.
use purrassist::{
    CancellationToken, Receipt,
    contract::Evidence,
    json::{Node, Value},
    schema::{CompilationError, Schema, algebra::WitnessOutcome},
    wire::{array, evidence, object, string},
};
use std::collections::BTreeMap;

pub(super) fn call(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let effort = super::effort(args)?;
    let schema_text = args.get("schema").ok_or("missing schema")?.to_json();
    let resources = resources(args)?;
    let mut governor = super::governor(effort.budget(), cancellation);
    let mut receipt = Receipt::new(
        schema_text.as_bytes(),
        "schema.witness.v1",
        effort,
        effort.budget(),
    );
    let schema = match Schema::with_resources_governed(&schema_text, &resources, &mut governor) {
        Ok(schema) => schema,
        // A malformed caller schema is a bad argument, exactly as
        // `decode_json` treats it: reported as a tool-call error rather than
        // manufacturing a witness outcome for it.
        Err(CompilationError::InvalidSchema(error)) => return Err(error.to_string()),
        // Resource exhaustion or cancellation *during compilation* is a
        // genuine governed outcome: no witness search ever started, so it is
        // reported as `"exhausted"` with the receipt's `termination` naming
        // the actual reason, never as `"unsatisfiable"`.
        Err(CompilationError::Resource(reason)) => {
            receipt.stopped = reason;
            receipt.usage = governor.usage();
            return Ok(finish("exhausted", Node::new(Value::Null), &[], &receipt));
        }
    };
    let (outcome, witness, refutation) = match schema.generate_witness_governed(&mut governor) {
        Ok(WitnessOutcome::Witness(node)) => ("witness", node, Vec::new()),
        Ok(WitnessOutcome::Unsatisfiable(refutation)) => {
            receipt.evidence.extend(refutation.evidence.clone());
            ("unsatisfiable", Node::new(Value::Null), refutation.evidence)
        }
        Ok(WitnessOutcome::Exhausted) => ("exhausted", Node::new(Value::Null), Vec::new()),
        Err(reason) => {
            receipt.stopped = reason;
            ("exhausted", Node::new(Value::Null), Vec::new())
        }
    };
    receipt.usage = governor.usage();
    Ok(finish(outcome, witness, &refutation, &receipt))
}

/// Decode the optional `resources` argument into the URI-to-schema-source map
/// `Schema::with_resources_governed` expects — the same per-entry shape
/// (`object`/`boolean`) the registry already accepts for the top-level
/// `schema` argument, so a caller reuses one mental model for both.
fn resources(args: &Node) -> Result<BTreeMap<String, String>, String> {
    let mut resources = BTreeMap::new();
    let Some(node) = args.get("resources") else {
        return Ok(resources);
    };
    let Value::Object(members) = &node.value else {
        return Err("resources must be an object".into());
    };
    for member in members {
        resources.insert(member.key.clone(), member.value.to_json());
    }
    Ok(resources)
}

fn finish(outcome: &str, witness: Node, refutation: &[Evidence], receipt: &Receipt) -> Node {
    object([
        ("outcome", string(outcome)),
        ("witness", witness),
        ("refutation", array(refutation.iter().map(evidence))),
        ("receipt", receipt.to_node()),
    ])
}
