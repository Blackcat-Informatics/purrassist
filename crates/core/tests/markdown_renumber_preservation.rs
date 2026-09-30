// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Renumbering cannot reinterpret ordered-item bodies when marker widths change.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    markdown::{Dialect, Document},
};

#[test]
fn width_growth_refuses_indented_code_reinterpretation_in_nested_containers() {
    for (old, new) in [(1_u64, 10), (9, 10), (99, 100)] {
        for newline in ["\n", "\r\n"] {
            for quote in ["", "> "] {
                let indent = " ".repeat(old.to_string().len() + 6);
                let source = format!(
                    "{quote}{old}. tea{newline}{quote}{newline}{quote}{indent}indented code{newline}{quote}{}. keep{newline}",
                    old + 1
                );
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert!(
                    document
                        .render_html(Dialect::Gfm, &mut governor)
                        .unwrap()
                        .contains("<pre><code>indented code"),
                    "{source:?}"
                );
                let path: &[usize] = if quote.is_empty() { &[0] } else { &[0, 0] };
                assert!(
                    matches!(
                        document.renumber_list(path, new, &mut governor),
                        Err(StopReason::InvalidInput(_))
                    ),
                    "{source:?}"
                );
                assert_eq!(document.source(), source);
            }
        }
    }
}

#[test]
fn width_growth_refuses_tab_indented_code_reinterpretation() {
    for newline in ["\n", "\r\n"] {
        let source = format!("1. tea{newline}{newline}\t   indented code{newline}2. keep{newline}");
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(&source, &mut governor).unwrap();
        assert!(
            document
                .render_html(Dialect::Gfm, &mut governor)
                .unwrap()
                .contains("<pre><code>indented code")
        );
        assert!(matches!(
            document.renumber_list(&[0], 10, &mut governor),
            Err(StopReason::InvalidInput(_))
        ));
    }
}

#[test]
fn safe_width_growth_retains_nested_body_and_round_trips() {
    let source = "9. tea\n\n   ```text\n   code\n   ```\n10. keep\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    let plan = document.renumber_list(&[0], 8, &mut governor).unwrap();
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply")
    };
    assert_eq!(
        applied.text,
        "8. tea\n\n   ```text\n   code\n   ```\n9. keep\n"
    );
    let updated = Document::parse(&applied.text, &mut governor).unwrap();
    assert!(
        updated
            .render_html(Dialect::Gfm, &mut governor)
            .unwrap()
            .contains("<pre><code class=\"language-text\">code")
    );
    let inverse = plan.invert(source, &mut governor).unwrap();
    let Outcome::Resolved(restored) = inverse.apply(&applied.text, Budget::default()).outcome
    else {
        panic!("inverse")
    };
    assert_eq!(restored.text, source);
}
