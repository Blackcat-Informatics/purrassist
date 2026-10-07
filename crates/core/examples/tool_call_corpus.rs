// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Writes the tool-call mutation corpus,
//! `crates/core/tests/data/evaluation/tool-calls.jsonl`, and registers it in
//! that directory's `manifest.json` (its digest, case and distinct-request
//! counts, and the digests of this generator's sources) under
//! `generators`, leaving every other manifest entry untouched.
//!
//! ```sh
//! cargo run --locked -p purrassist-core --example tool-call-corpus
//! ```
//!
//! The generator is deterministic (see `tests/tool_call_corpus/mod.rs`):
//! `correction_evaluation.rs` regenerates the corpus in memory and requires
//! it to equal the committed file byte for byte.
#[path = "../tests/tool_call_corpus/mod.rs"]
mod tool_call_corpus;

use purrassist_core::digest::Digest;
use purrassist_core::json::{self, Member, Node, Value};
use std::path::Path;

/// The manifest's name for the corpus.
const OUTPUT: &str = "tool-calls.jsonl";

/// The generator's sources, relative to the repository root.
const SOURCES: [&str; 2] = [
    "crates/core/examples/tool_call_corpus.rs",
    "crates/core/tests/tool_call_corpus/mod.rs",
];

fn sha256(bytes: &[u8]) -> String {
    Digest::of(bytes)
        .to_string()
        .trim_start_matches("sha256:")
        .to_owned()
}

fn string(text: &str) -> Node {
    Node::new(Value::String(text.to_owned()))
}

fn count(value: usize) -> Node {
    Node::new(Value::Number(json::Number::from_count(value)))
}

fn object(members: Vec<(String, Node)>) -> Node {
    Node::new(Value::Object(
        members
            .into_iter()
            .map(|(key, value)| Member {
                key,
                key_span: None,
                value,
            })
            .collect(),
    ))
}

/// JSON text indented by two spaces per level, as Python's
/// `json.dumps(value, indent=2)` prints it (for ASCII content), so either
/// generator can rewrite the shared manifest without reformatting it.
fn pretty(node: &Node, depth: usize, out: &mut String) {
    let indent = |out: &mut String, depth: usize| out.push_str(&"  ".repeat(depth));
    match &node.value {
        Value::Object(members) if !members.is_empty() => {
            out.push_str("{\n");
            for (index, member) in members.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                indent(out, depth + 1);
                out.push_str(&json::quote(&member.key));
                out.push_str(": ");
                pretty(&member.value, depth + 1, out);
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
        Value::Array(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                indent(out, depth + 1);
                pretty(item, depth + 1, out);
            }
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        _ => out.push_str(&node.to_json()),
    }
}

fn main() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = package
        .parent()
        .and_then(Path::parent)
        .expect("the package lives in crates/core");
    let cases = tool_call_corpus::generate();
    let text = tool_call_corpus::render(&cases);
    assert!(text.is_ascii(), "the corpus is ASCII JSON Lines");
    let corpus = package.join(tool_call_corpus::RELATIVE_PATH);
    std::fs::write(&corpus, &text)
        .unwrap_or_else(|error| panic!("writing {}: {error}", corpus.display()));

    let sources = SOURCES
        .iter()
        .map(|source| {
            let bytes = std::fs::read(root.join(source))
                .unwrap_or_else(|error| panic!("reading {source}: {error}"));
            ((*source).to_owned(), string(&sha256(&bytes)))
        })
        .collect();
    let distinct = tool_call_corpus::distinct_requests(&cases);
    let entry = object(vec![
        (
            "command".to_owned(),
            string("cargo run --locked -p purrassist-core --example tool-call-corpus"),
        ),
        ("sources".to_owned(), object(sources)),
        ("cases".to_owned(), count(cases.len())),
        ("distinct_requests".to_owned(), count(distinct)),
        ("sha256".to_owned(), string(&sha256(text.as_bytes()))),
    ]);

    let manifest_path = corpus.with_file_name("manifest.json");
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", manifest_path.display()));
    let mut manifest = json::parse(&manifest_text).expect("the manifest is JSON");
    let Value::Object(members) = &mut manifest.value else {
        panic!("the manifest is an object")
    };
    let position = members.iter().position(|member| member.key == "generators");
    let generators = if let Some(index) = position {
        &mut members[index].value
    } else {
        members.push(Member {
            key: "generators".to_owned(),
            key_span: None,
            value: object(Vec::new()),
        });
        &mut members.last_mut().expect("just pushed").value
    };
    let Value::Object(outputs) = &mut generators.value else {
        panic!("`generators` is an object")
    };
    match outputs.iter_mut().find(|member| member.key == OUTPUT) {
        Some(member) => member.value = entry,
        None => outputs.push(Member {
            key: OUTPUT.to_owned(),
            key_span: None,
            value: entry,
        }),
    }
    let mut rendered = String::new();
    pretty(&manifest, 0, &mut rendered);
    rendered.push('\n');
    std::fs::write(&manifest_path, rendered)
        .unwrap_or_else(|error| panic!("writing {}: {error}", manifest_path.display()));
    println!(
        "{OUTPUT}: {} cases, {distinct} distinct requests",
        cases.len()
    );
}
