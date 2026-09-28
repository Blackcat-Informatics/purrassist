// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Identical native/WASI grammar acceptance and syntax repair transcript.
use purrassist_core::{
    Budget, Governor, Outcome,
    digest::Digest,
    edits::{EditPlan, TextEdit},
};
use purrassist_syntax::{Language, apply_checked, inspect};

fn main() {
    for (language, source) in [
        (Language::Python, "def answer():\n    return 42\n"),
        (Language::Rust, "fn answer() -> u32 { 42 }"),
        (Language::JavaScript, "function answer() { return <div/>; }"),
        (
            Language::TypeScript,
            "function answer(): number { return 42; }",
        ),
        (Language::Tsx, "function answer() { return <div/>; }"),
    ] {
        let report = inspect(source, language, &mut Governor::new(Budget::default()))
            .expect("grammar inspection");
        assert!(report.valid);
        println!("{report:?}");
    }
    let source = "fn answer() -> u32 { 42 }";
    let plan = EditPlan {
        source: Digest::of(source.as_bytes()),
        edits: vec![TextEdit {
            range: 0..source.len(),
            replacement: "fn answer() -> u32 { let x = 42 x }".into(),
        }],
    };
    let result = apply_checked(source, &plan, Language::Rust, Budget::default());
    let Outcome::Resolved(applied) = result.outcome else {
        panic!("{result:?}")
    };
    println!("{}", applied.text);
    let Outcome::Resolved(undone) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo")
    };
    assert_eq!(undone.text, source);
}
