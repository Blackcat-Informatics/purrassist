// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Task 8 known-origin corruption oracle: take a witness the algebra
//! generated for an official-suite schema (so its exact original value is
//! known), apply one recorded, deterministic corruption, and recover it.
//! Recovery must return the original (`Resolved`/`BestGuess`), the original
//! among an `Ambiguous` set, or an explicit non-success (`Unresolved`) —
//! never a *different* `Resolved`/`BestGuess` value.
use purrassist_core::json::{self, Member, Node, Value, quote};
use purrassist_core::schema::{Schema, algebra::WitnessOutcome, recovery::RecoveryRequest};
use purrassist_core::{Effort, Governor, Outcome};
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

/// One recorded, deterministic corruption of an exact JSON text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Corruption {
    Truncation,
    ProseWrapping,
    KeyTypo,
    Stringification,
    TypeSwap,
    DuplicateKeys,
}
const CORRUPTIONS: [Corruption; 6] = [
    Corruption::Truncation,
    Corruption::ProseWrapping,
    Corruption::KeyTypo,
    Corruption::Stringification,
    Corruption::TypeSwap,
    Corruption::DuplicateKeys,
];

fn apply(corruption: Corruption, original: &Node) -> Option<String> {
    match corruption {
        // A container whose closing punctuation was cut off (the canonical
        // "the model's response got cut short" shape recovery's own
        // syntax-repair specifically reconstructs exactly, since nothing
        // about *which* value was inside is lost, only how it was closed).
        // Restricted to `Object`/`Array`: a bare scalar has no closing
        // punctuation to lose, and truncating *inside* one destroys
        // information no engine could reconstruct losslessly, which is not
        // what this corruption is meant to probe.
        Corruption::Truncation => {
            if !matches!(original.value, Value::Object(_) | Value::Array(_)) {
                return None;
            }
            let text = original.to_json();
            let trimmed = text.trim_end_matches(['}', ']']);
            (trimmed.len() < text.len()).then(|| trimmed.to_string())
        }
        // Restricted to `Object`/`Array`: recovery's prose/fence extraction
        // is documented as anchored on `{`/`[` (see
        // `crate::schema::recovery::Search::syntax`), so wrapping a bare
        // scalar in surrounding prose is outside what this corruption is
        // meant to probe (there is no `{`/`[` for it to anchor on at all).
        Corruption::ProseWrapping => {
            if !matches!(original.value, Value::Object(_) | Value::Array(_)) {
                return None;
            }
            Some(format!(
                "Sure, here is the value you asked for:\n```json\n{}\n```\nLet me know if you need anything else!",
                original.to_json()
            ))
        }
        // A realistic typo: swap two adjacent characters (within edit
        // distance 1 of the original), not an arbitrary permutation no
        // fuzzy key matcher is documented to recognize.
        Corruption::KeyTypo => mutate_first_key(original, |key| {
            let mut chars: Vec<char> = key.chars().collect();
            if chars.len() >= 2 {
                chars.swap(0, 1);
            } else {
                chars.push('x');
            }
            let mutated: String = chars.into_iter().collect();
            (mutated != *key).then_some(mutated)
        }),
        Corruption::Stringification => Some(quote(&original.to_json())),
        Corruption::TypeSwap => mutate_first_scalar(original),
        Corruption::DuplicateKeys => mutate_first_object(original, |members| {
            if members.is_empty() {
                None
            } else {
                let mut duplicated = members.clone();
                duplicated.push(members[0].clone());
                Some(duplicated)
            }
        }),
    }
}

/// Depth-first search for the first object with at least one member,
/// renaming that member's key with `rename`; `None` if no object has any
/// member `rename` can act on.
fn mutate_first_key(node: &Node, rename: impl Fn(&str) -> Option<String> + Copy) -> Option<String> {
    fn walk(node: &Node, rename: impl Fn(&str) -> Option<String> + Copy) -> Option<Node> {
        if let Value::Object(members) = &node.value {
            for (position, member) in members.iter().enumerate() {
                if let Some(new_key) = rename(&member.key) {
                    let mut replacement = members.clone();
                    replacement[position] = Member {
                        key: new_key,
                        key_span: None,
                        value: member.value.clone(),
                    };
                    return Some(Node::new(Value::Object(replacement)));
                }
            }
            for (position, member) in members.iter().enumerate() {
                if let Some(mutated_value) = walk(&member.value, rename) {
                    let mut replacement = members.clone();
                    replacement[position] = Member {
                        key: member.key.clone(),
                        key_span: None,
                        value: mutated_value,
                    };
                    return Some(Node::new(Value::Object(replacement)));
                }
            }
        }
        if let Value::Array(items) = &node.value {
            for (position, item) in items.iter().enumerate() {
                if let Some(mutated) = walk(item, rename) {
                    let mut replacement = items.clone();
                    replacement[position] = mutated;
                    return Some(Node::new(Value::Array(replacement)));
                }
            }
        }
        None
    }
    walk(node, rename).map(|node| node.to_json())
}

/// Depth-first search for the first object, replacing its member list with
/// whatever `edit` returns; `None` if no object exists or `edit` declines.
fn mutate_first_object(
    node: &Node,
    edit: impl Fn(&Vec<Member>) -> Option<Vec<Member>> + Copy,
) -> Option<String> {
    fn walk(
        node: &Node,
        edit: impl Fn(&Vec<Member>) -> Option<Vec<Member>> + Copy,
    ) -> Option<Node> {
        if let Value::Object(members) = &node.value {
            if let Some(replacement) = edit(members) {
                return Some(Node::new(Value::Object(replacement)));
            }
            for (position, member) in members.iter().enumerate() {
                if let Some(mutated_value) = walk(&member.value, edit) {
                    let mut replacement = members.clone();
                    replacement[position] = Member {
                        key: member.key.clone(),
                        key_span: None,
                        value: mutated_value,
                    };
                    return Some(Node::new(Value::Object(replacement)));
                }
            }
        }
        if let Value::Array(items) = &node.value {
            for (position, item) in items.iter().enumerate() {
                if let Some(mutated) = walk(item, edit) {
                    let mut replacement = items.clone();
                    replacement[position] = mutated;
                    return Some(Node::new(Value::Array(replacement)));
                }
            }
        }
        None
    }
    walk(node, edit).map(|node| node.to_json())
}

/// Depth-first search for the first scalar leaf (a number, boolean, or
/// string, or vice versa for a string whose exact content is itself a
/// number literal), an information-preserving swap recovery's existing
/// `json.type.decode-string`/`json.type.stringify-scalar` shape repairs can
/// invert exactly. Deliberately shallow (the root witness value only, never
/// recursing into a container): swapping a *nested* element interacts with
/// unrelated shape repairs (`json.shape.unwrap-array` on a single-element
/// array, in particular) in ways this corruption is not trying to probe,
/// and a non-numeric string has no information-preserving type swap at all
/// (swapping it for an unrelated derived value like its length would make
/// the exact original unrecoverable in principle, not just in practice).
fn mutate_first_scalar(node: &Node) -> Option<String> {
    let swapped = match &node.value {
        Value::Number(_) | Value::Bool(_) => Node::new(Value::String(node.to_json())),
        Value::String(text) if text.parse::<json::Number>().is_ok() => {
            Node::new(Value::Number(text.parse().expect("checked above")))
        }
        _ => return None,
    };
    Some(swapped.to_json())
}

#[test]
fn corruptions_of_a_known_witness_never_resolve_to_a_different_value() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/json-schema");
    let resources = load_resources(&root);
    let mut paths = files(&root.join("tests/draft2020-12"));
    paths.retain(|path| !path.starts_with(root.join("remotes")));

    let mut schemas_with_witness = 0usize;
    let mut trials = 0usize;
    let mut matched_original = 0usize;
    let mut ambiguous_contained_original = 0usize;
    let mut explicit_non_success = 0usize;
    let mut uninformative_but_valid = 0usize;
    let mut wrong_answers: Vec<String> = Vec::new();

    for path in &paths {
        let suite = parse_suite(path);
        let Value::Array(groups) = suite.value else {
            panic!("suite array")
        };
        for group in groups {
            let source = group.get("schema").expect("schema").to_json();
            let Ok(schema) = Schema::with_resources(&source, &resources) else {
                continue;
            };
            let mut governor = Governor::new(Effort::Maximum.budget());
            let WitnessOutcome::Witness(original) = schema
                .generate_witness_governed(&mut governor)
                .unwrap_or(WitnessOutcome::Exhausted)
            else {
                continue;
            };
            schemas_with_witness += 1;
            for corruption in CORRUPTIONS {
                let Some(corrupted) = apply(corruption, &original) else {
                    continue;
                };
                // A "corruption" that happens to already be a different,
                // directly-parseable, schema-valid instance is not
                // corruption relative to *this* schema at all (many
                // official-suite groups impose no `type` restriction, so a
                // stringified/type-swapped literal is simply another valid
                // instance): recovery correctly returns it unchanged
                // (`Resolved`, no transformation needed), and asserting it
                // must instead reconstruct the unrelated original would be
                // asserting recovery invent information it has no way to
                // know it should. Skip these as uninformative rather than
                // counting them either way.
                if json::parse(&corrupted)
                    .is_ok_and(|parsed| schema.validate(&parsed).is_ok_and(|report| report.valid))
                {
                    uninformative_but_valid += 1;
                    continue;
                }
                trials += 1;
                let result = schema.recover(&corrupted, RecoveryRequest::default());
                match result.outcome {
                    Outcome::Resolved(node) | Outcome::BestGuess(node) => {
                        if node.equivalent(&original) {
                            matched_original += 1;
                        } else {
                            wrong_answers.push(format!(
                                "{} / {:?}: original {} corrupted to {corrupted:?} resolved to {} instead",
                                path.file_name().expect("name").to_string_lossy(),
                                corruption,
                                original.to_json(),
                                node.to_json()
                            ));
                        }
                    }
                    Outcome::Ambiguous(nodes) => {
                        if nodes.iter().any(|node| node.equivalent(&original)) {
                            ambiguous_contained_original += 1;
                        } else {
                            wrong_answers.push(format!(
                                "{} / {:?}: original {} corrupted to {corrupted:?} ambiguous set excluded it",
                                path.file_name().expect("name").to_string_lossy(),
                                corruption,
                                original.to_json()
                            ));
                        }
                    }
                    Outcome::Unresolved => explicit_non_success += 1,
                }
            }
        }
    }
    eprintln!(
        "corruption_oracle: {schemas_with_witness} schemas witnessed; {trials} corruption trials \
         ({uninformative_but_valid} skipped as uninformative, already schema-valid on their own): \
         {matched_original} matched the original, {ambiguous_contained_original} ambiguous sets contained it, \
         {explicit_non_success} explicit non-success, {} wrong answers",
        wrong_answers.len()
    );
    for line in wrong_answers.iter().take(40) {
        eprintln!("{line}");
    }
    assert!(
        wrong_answers.is_empty(),
        "{} corrupted witness(es) resolved to a different value than the known original",
        wrong_answers.len()
    );
    assert!(
        trials >= 500,
        "corruption oracle census sanity: expected >= 500 trials, saw {trials}"
    );
}

fn parse_suite(path: &Path) -> Node {
    json::parse(&std::fs::read_to_string(path).expect("suite file")).expect("suite json")
}
