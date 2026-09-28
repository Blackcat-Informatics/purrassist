// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Grammar acceptance, source selection, introduced-error repair, and conditional undo.
use purrassist_core::{
    Budget, Governor, Outcome,
    digest::Digest,
    edits::{EditPlan, TextEdit},
};
use purrassist_syntax::{Language, apply_checked, inspect};

#[test]
fn supported_grammars_accept_valid_source_and_retain_declarations() {
    for (language, source, name) in [
        (Language::Python, "def answer():\n    return 42\n", "answer"),
        (Language::Rust, "fn answer() -> u32 { 42 }", "answer"),
        (
            Language::JavaScript,
            "function answer() { return <div/>; }",
            "answer",
        ),
        (
            Language::TypeScript,
            "function answer(): number { return 42; }",
            "answer",
        ),
        (
            Language::Tsx,
            "function answer() { return <div/>; }",
            "answer",
        ),
    ] {
        let report = inspect(source, language, &mut Governor::new(Budget::default())).unwrap();
        assert!(report.valid, "{language:?}: {report:?}");
        assert!(
            report
                .symbols
                .iter()
                .any(|symbol| symbol.name == name && source.is_char_boundary(symbol.span.start))
        );
    }
}
#[test]
fn parser_indicated_repair_is_validated_and_undo_restores_original() {
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
        panic!("{result:?}");
    };
    assert!(
        inspect(
            &applied.text,
            Language::Rust,
            &mut Governor::new(Budget::default())
        )
        .unwrap()
        .valid
    );
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .any(|entry| entry.rule == "syntax.missing-punctuation.v1")
    );
    let Outcome::Resolved(undone) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo failed");
    };
    assert_eq!(undone.text, source);
}
#[test]
fn malformed_unrepairable_edit_is_not_applied() {
    let source = "def answer():\n    return 42\n";
    let plan = EditPlan {
        source: Digest::of(source.as_bytes()),
        edits: vec![TextEdit {
            range: 0..source.len(),
            replacement: "def ???".into(),
        }],
    };
    assert_eq!(
        apply_checked(source, &plan, Language::Python, Budget::default()).outcome,
        Outcome::Unresolved
    );
}
#[test]
fn mismatched_jsx_tag_yields_two_ambiguous_rename_repairs() {
    // A mismatched closing tag name admits two independently valid,
    // competing repairs: rename the closing tag to match the opening one,
    // or the opening tag to match the closing one. Neither is more "right"
    // than the other from source text alone, so `apply_checked` reports
    // both as an `Ambiguous` outcome with one alternative per repair,
    // reaching `populate_repair_alternatives` through the crate's public
    // API rather than hand-built test data (see the `checked` unit test's
    // doc comment for why two *independent* missing-punctuation defects
    // cannot reach this same path).
    for language in [Language::JavaScript, Language::Tsx] {
        let source = "const x = <div></div>;\n";
        let broken = "const x = <div></span>;\n";
        let plan = EditPlan {
            source: Digest::of(source.as_bytes()),
            edits: vec![TextEdit {
                range: 0..source.len(),
                replacement: broken.into(),
            }],
        };
        let result = apply_checked(source, &plan, language, Budget::default());
        let Outcome::Ambiguous(answers) = result.outcome else {
            panic!("{language:?}: expected Ambiguous, got {:?}", result.outcome);
        };
        assert_eq!(answers.len(), 2, "{language:?}: {answers:?}");
        let texts: Vec<&str> = answers.iter().map(|answer| answer.text.as_str()).collect();
        assert!(texts.contains(&"const x = <div></div>;\n"), "{texts:?}");
        assert!(texts.contains(&"const x = <span></span>;\n"), "{texts:?}");
        assert_eq!(result.receipt.alternatives.len(), 2);
        for (answer, ledger) in answers.iter().zip(&result.receipt.alternatives) {
            assert_eq!(
                ledger.output,
                Digest::of(answer.to_node().to_json().as_bytes())
            );
            // Undoing either alternative restores the original broken text
            // it was derived from applying the original plan to, not the
            // pre-plan `source` — matching `parser_indicated_repair_is_validated_and_undo_restores_original`.
            let Outcome::Resolved(undone) = answer
                .inverse
                .apply(&answer.text, Budget::default())
                .outcome
            else {
                panic!("undo failed for {:?}", answer.text);
            };
            assert_eq!(undone.text, source);
        }
        assert!(
            result
                .receipt
                .evidence
                .iter()
                .chain(result.receipt.alternatives.iter().flat_map(|a| &a.evidence))
                .any(|entry| entry.rule == "syntax.jsx-tag-rename.v1"),
            "{:?}",
            result.receipt
        );
    }
}
#[test]
fn jsx_tag_names_match_including_member_names_and_fragments() {
    for language in [Language::JavaScript, Language::Tsx] {
        for (source, valid) in [
            ("const x = <div></span>;", false),
            ("const x = <Foo.Bar></Foo.Baz>;", false),
            ("const x = <div><span/></div>;", true),
            ("const x = <><Foo.Bar></Foo.Bar></>;", true),
        ] {
            let report = inspect(source, language, &mut Governor::new(Budget::default())).unwrap();
            assert_eq!(report.valid, valid, "{source}: {report:?}");
            if !valid {
                assert!(
                    report
                        .issues
                        .iter()
                        .any(|issue| issue.kind == "jsx.mismatched-tags.v1")
                );
            }
        }
    }
}
