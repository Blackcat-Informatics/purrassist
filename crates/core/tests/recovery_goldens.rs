// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Task 8 curated composed-recovery goldens: each fixture under
//! `tests/data/recovery-goldens/` pairs an `input` and `schema` with a
//! curated `description`; the expected outcome kind, output(s), and ledger
//! rule set are captured by an actual governed run rather than
//! hand-transcribed, to avoid silent drift from the engine's real behavior.
//!
//! `PURRASSIST_BLESS=1` re-captures every fixture's expected fields from a
//! fresh run (panicking if `CI` is set, since blessing must never run
//! unattended); without it, this test asserts every fixture's captured
//! fields still match.
use purrassist_core::json::{Member, Node, Value, parse};
use purrassist_core::schema::{Schema, recovery::RecoveryRequest};
use purrassist_core::{AssistResult, Outcome};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/recovery-goldens")
}
/// The same vendored official-suite `remotes/` resources
/// `schema_conformance.rs` loads, so a golden's `$schema` may reference the
/// format-assertion dialect without duplicating its meta-schema text here.
fn dialect_resources() -> BTreeMap<String, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/json-schema/remotes");
    let mut resources = BTreeMap::new();
    let mut stack = vec![root.clone()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("remotes directory") {
            let path = entry.expect("remotes entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                let relative = path
                    .strip_prefix(&root)
                    .expect("relative path")
                    .to_string_lossy()
                    .replace('\\', "/");
                resources.insert(
                    format!("http://localhost:1234/{relative}"),
                    std::fs::read_to_string(&path).expect("resource"),
                );
            }
        }
    }
    resources
}
fn golden_files() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(goldens_dir())
        .expect("goldens directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
}
fn member(key: &str, value: Node) -> Member {
    Member {
        key: key.into(),
        key_span: None,
        value,
    }
}
const fn outcome_kind(outcome: &Outcome<Node>) -> &'static str {
    match outcome {
        Outcome::Resolved(_) => "resolved",
        Outcome::BestGuess(_) => "best_guess",
        Outcome::Ambiguous(_) => "ambiguous",
        Outcome::Unresolved => "unresolved",
    }
}
/// Every rule name recorded anywhere in the receipt: top-level and (for an
/// ambiguous outcome) every alternative's own ledger.
fn ledger_rules(result: &AssistResult<Node>) -> Vec<String> {
    let mut rules: Vec<String> = result
        .receipt
        .evidence
        .iter()
        .map(|entry| entry.rule.clone())
        .collect();
    for alternative in &result.receipt.alternatives {
        rules.extend(alternative.evidence.iter().map(|entry| entry.rule.clone()));
    }
    rules.sort();
    rules.dedup();
    rules
}
fn strings(values: &[String]) -> Node {
    Node::new(Value::Array(
        values
            .iter()
            .map(|value| Node::new(Value::String(value.clone())))
            .collect(),
    ))
}
fn captured_fields(result: &AssistResult<Node>) -> Vec<Member> {
    let mut fields = vec![member(
        "outcome",
        Node::new(Value::String(outcome_kind(&result.outcome).into())),
    )];
    match &result.outcome {
        Outcome::Resolved(node) | Outcome::BestGuess(node) => {
            fields.push(member("output", node.clone()));
        }
        Outcome::Ambiguous(nodes) => {
            fields.push(member("outputs", Node::new(Value::Array(nodes.clone()))));
        }
        Outcome::Unresolved => {}
    }
    fields.push(member("ledger_rules", strings(&ledger_rules(result))));
    fields
}
/// Replace every captured field (`outcome`/`output`/`outputs`/
/// `ledger_rules`) in `golden`, preserving `description`/`schema`/`input`
/// and their original member order.
fn reblessed(golden: &Node, result: &AssistResult<Node>) -> Node {
    let Value::Object(members) = &golden.value else {
        panic!("golden must be a JSON object")
    };
    let mut kept: Vec<Member> = members
        .iter()
        .filter(|member| matches!(member.key.as_str(), "description" | "schema" | "input"))
        .cloned()
        .collect();
    kept.extend(captured_fields(result));
    Node::new(Value::Object(kept))
}
fn pretty(node: &Node) -> String {
    let mut output = String::new();
    write_pretty(node, 0, &mut output);
    output.push('\n');
    output
}
fn write_pretty(node: &Node, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    let inner_pad = "  ".repeat(indent + 1);
    match &node.value {
        Value::Object(members) if members.is_empty() => out.push_str("{}"),
        Value::Object(members) => {
            out.push_str("{\n");
            for (index, member) in members.iter().enumerate() {
                out.push_str(&inner_pad);
                out.push_str(&Node::new(Value::String(member.key.clone())).to_json());
                out.push_str(": ");
                write_pretty(&member.value, indent + 1, out);
                if index + 1 < members.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push('}');
        }
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                out.push_str(&inner_pad);
                write_pretty(item, indent + 1, out);
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push(']');
        }
        _ => out.push_str(&node.to_json()),
    }
}
fn compare_nodes(
    path: &Path,
    field: &str,
    expected: &Node,
    actual: &Node,
    failures: &mut Vec<String>,
) {
    if !expected.equivalent(actual) {
        failures.push(format!(
            "{}: {field} mismatch\n  expected: {}\n  actual:   {}",
            path.display(),
            expected.to_json(),
            actual.to_json()
        ));
    }
}
#[test]
fn recovery_goldens_match_curated_expectations() {
    let bless = std::env::var("PURRASSIST_BLESS").as_deref() == Ok("1");
    assert!(
        !(bless && std::env::var("CI").is_ok()),
        "PURRASSIST_BLESS=1 must never run with CI set"
    );
    let files = golden_files();
    assert!(
        files.len() >= 60,
        "Task 8 requires >= 60 curated golden cases; found {}",
        files.len()
    );
    let resources = dialect_resources();
    let mut failures = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let golden = parse(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let schema_source = golden
            .get("schema")
            .unwrap_or_else(|| panic!("{}: missing 'schema'", path.display()))
            .to_json();
        let input = golden
            .get("input")
            .and_then(Node::as_str)
            .unwrap_or_else(|| panic!("{}: missing string 'input'", path.display()));
        let schema = Schema::with_resources(&schema_source, &resources)
            .unwrap_or_else(|error| panic!("{}: schema fails to compile: {error}", path.display()));
        let result = schema.recover(input, RecoveryRequest::default());
        if bless {
            std::fs::write(path, pretty(&reblessed(&golden, &result)))
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            continue;
        }
        let Some(expected_kind) = golden.get("outcome").and_then(Node::as_str) else {
            failures.push(format!(
                "{}: missing blessed 'outcome'; run with PURRASSIST_BLESS=1",
                path.display()
            ));
            continue;
        };
        let actual_kind = outcome_kind(&result.outcome);
        if expected_kind != actual_kind {
            failures.push(format!(
                "{}: outcome mismatch: expected {expected_kind}, got {actual_kind}",
                path.display()
            ));
            continue;
        }
        match &result.outcome {
            Outcome::Resolved(node) | Outcome::BestGuess(node) => {
                let expected = golden.get("output").unwrap_or_else(|| {
                    panic!("{}: blessed outcome lacks 'output'", path.display())
                });
                compare_nodes(path, "output", expected, node, &mut failures);
            }
            Outcome::Ambiguous(nodes) => {
                let expected = golden.get("outputs").unwrap_or_else(|| {
                    panic!("{}: blessed outcome lacks 'outputs'", path.display())
                });
                compare_nodes(
                    path,
                    "outputs",
                    expected,
                    &Node::new(Value::Array(nodes.clone())),
                    &mut failures,
                );
            }
            Outcome::Unresolved => {}
        }
        let expected_rules = golden
            .get("ledger_rules")
            .and_then(|node| {
                if let Value::Array(values) = &node.value {
                    Some(
                        values
                            .iter()
                            .filter_map(Node::as_str)
                            .map(str::to_string)
                            .collect::<Vec<_>>(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let actual_rules = ledger_rules(&result);
        if expected_rules != actual_rules {
            failures.push(format!(
                "{}: ledger_rules mismatch\n  expected: {expected_rules:?}\n  actual:   {actual_rules:?}",
                path.display()
            ));
        }
    }
    assert!(
        !bless,
        "{} golden(s) (re)blessed under tests/data/recovery-goldens/; re-run without PURRASSIST_BLESS to verify",
        files.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
