// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Assertion-backed, single-process workloads for external allocation profiling.
use purrassist_core::{
    Budget, Governor, Outcome, json,
    markdown::{Dialect, Document, ListItemEdit},
    matching::{self, Candidate, MatchRequest},
    schema::{Schema, recovery::RecoveryRequest},
    text,
};
use std::hint::black_box;

const CASES: &[&str] = &[
    "empty",
    "schema_compile",
    "json_parse",
    "json_composed",
    "json_ambiguous",
    "markdown_render",
    "markdown_list",
    "unicode",
    "matching",
];
const SCHEMA: &str = r#"{"type":"object","required":["temperature"],"properties":{"temperature":{"type":"number"}},"additionalProperties":false}"#;
fn main() {
    let mut args = std::env::args().skip(1);
    let selected = args.next().unwrap_or_else(|| "--bench".into());
    let rss = match args.next().as_deref() {
        None => false,
        Some("--rss") => true,
        _ => panic!("unexpected resource benchmark argument"),
    };
    assert!(args.next().is_none(), "extra resource benchmark argument");
    if selected == "--list" {
        for case in CASES {
            println!("{case}");
        }
    } else if selected == "--bench" {
        for case in CASES {
            run(case);
        }
    } else {
        run(&selected);
    }
    if rss {
        report_rss();
    }
}
fn report_rss() {
    // Native-only probe: profiling runs omit --rss, keeping its allocations out
    // of Memcheck/Massif totals. VmHWM belongs to this post-exec address space.
    let status = std::fs::read_to_string("/proc/self/status").expect("Linux procfs RSS");
    let row = status
        .lines()
        .find(|line| line.starts_with("VmHWM:"))
        .expect("VmHWM");
    let fields: Vec<_> = row.split_whitespace().collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[2], "kB");
    let bytes = fields[1]
        .parse::<u64>()
        .expect("RSS integer")
        .checked_mul(1024)
        .expect("RSS bytes");
    println!("rss_bytes {bytes}");
}
fn run(case: &str) {
    match case {
        "empty" => {}
        "schema_compile" => {
            black_box(Schema::compile(SCHEMA).expect("schema"));
        }
        "json_parse" => {
            let source = format!(
                "[{}]",
                vec![r#"{"name":"café","value":123.4500,"enabled":true}"#; 256].join(",")
            );
            let parsed = json::parse(black_box(&source)).expect("JSON");
            assert_eq!(parsed.to_json(), source);
            black_box(parsed);
        }
        "json_composed" => {
            let schema = Schema::compile(SCHEMA).expect("schema");
            let result = schema.recover(
                "```json\n[[{temperatur: '42',}]]\n```",
                RecoveryRequest::default(),
            );
            let Outcome::Resolved(value) = &result.outcome else {
                panic!("composed recovery");
            };
            assert_eq!(value.to_json(), r#"{"temperature":42}"#);
            black_box(result);
        }
        "json_ambiguous" => {
            let schema = Schema::compile(SCHEMA).expect("schema");
            let result = schema.recover(
                r#"{"temperature":1,"temperature":2}"#,
                RecoveryRequest::default(),
            );
            let Outcome::Ambiguous(values) = &result.outcome else {
                panic!("ambiguity");
            };
            assert_eq!(values.len(), 2);
            assert!(
                values
                    .iter()
                    .any(|value| value.to_json() == r#"{"temperature":1}"#)
            );
            assert!(
                values
                    .iter()
                    .any(|value| value.to_json() == r#"{"temperature":2}"#)
            );
            black_box(result);
        }
        "markdown_render" => {
            let source =
                "# Tea\n\n> - [x] **Cold** café\n\n| Drink |\n| --- |\n| tea |\n\n".repeat(32);
            let mut g = Governor::new(Budget::default());
            let doc = Document::parse(&source, &mut g).expect("Markdown");
            let html = doc.render_html(Dialect::Gfm, &mut g).expect("HTML");
            assert_eq!(html.matches("<h1>Tea</h1>").count(), 32);
            assert_eq!(html.matches("<table>").count(), 32);
            black_box((doc, html));
        }
        "markdown_list" => list(),
        "unicode" => {
            let source = "Straße CAFÉ Ｔｅａ e\u{301} ".repeat(256);
            let result = text::comparison_key(black_box(&source));
            assert_eq!(result, "strasse café tea é ".repeat(256));
            black_box(result);
        }
        "matching" => {
            let candidates: Vec<_> = (0..64)
                .map(|index| Candidate::new(index.to_string(), format!("document_{index}.json")))
                .collect();
            let result = matching::rank("document_42.json", &candidates, &MatchRequest::default());
            let Outcome::Resolved(values) = &result.outcome else {
                panic!("matching");
            };
            assert_eq!(values[0].candidate.id, "42");
            black_box(result);
        }
        _ => panic!("unknown resource case: {case}"),
    }
    println!("ok {case}");
}
fn list() {
    let source = "> - hot\n".repeat(64);
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(&source, &mut g).expect("list");
    let plan = doc
        .edit_list_item(
            &[0, 32],
            &ListItemEdit::After {
                marker: "-".into(),
                body: "[x] iced café".into(),
            },
            &mut g,
        )
        .expect("list edit");
    let applied = plan.apply_governed(&source, &mut g);
    let Outcome::Resolved(value) = &applied.outcome else {
        panic!("apply");
    };
    assert!(value.text.contains("> - [x] iced café\n"));
    let undone = value.inverse.apply_governed(&value.text, &mut g);
    let Outcome::Resolved(undo) = &undone.outcome else {
        panic!("undo");
    };
    assert_eq!(undo.text, source);
    black_box((doc, plan, applied, undone));
}
