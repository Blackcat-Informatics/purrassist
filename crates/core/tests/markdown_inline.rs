// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Whole selected specification sections for the first inline parser layer.
use purrassist_core::{
    Budget, Governor, StopReason,
    contract::Resource,
    json::{self, Value},
    markdown::{Dialect, InlineFragment, InlineKind},
};

#[test]
fn official_emphasis_code_autolink_break_and_raw_html_sections() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for (current, corpus) in [
        (true, include_str!("data/markdown/commonmark-0.31.2.json")),
        (false, include_str!("data/markdown/gfm-0.29.0.gfm.13.json")),
    ] {
        let Value::Array(cases) = json::parse(corpus).unwrap().value else {
            panic!("corpus")
        };
        for case in cases {
            let section = case.get("section").unwrap().as_str().unwrap();
            if !(matches!(
                section,
                "Code spans" | "Autolinks" | "Soft line breaks" | "Emphasis and strong emphasis"
            ) || section == "Raw HTML")
            {
                continue;
            }
            let source = case.get("markdown").unwrap().as_str().unwrap();
            let expected = case.get("html").unwrap().as_str().unwrap();
            let mut actual = String::new();
            // These sections contain paragraph-only examples; a blank line
            // separates the two paragraphs in CommonMark's short-comment case.
            for paragraph in source.split("\n\n") {
                let mut governor = Governor::new(Budget::default());
                let fragment = InlineFragment::parse_with_dialect(
                    paragraph,
                    if current {
                        Dialect::CommonMark
                    } else {
                        Dialect::Gfm
                    },
                    &mut governor,
                )
                .unwrap();
                assert_partition(&fragment);
                actual.push_str("<p>");
                actual.push_str(&fragment.render_html(&mut governor).unwrap());
                actual.push_str("</p>\n");
            }
            if actual != expected {
                failures.push(format!("current={current} {} {section}: {source:?}; expected {expected:?}, got {actual:?}", case.get("example").unwrap().to_json()));
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 389);
    assert_eq!(failures, Vec::<String>::new());
}
fn assert_partition(fragment: &InlineFragment) {
    assert_eq!(
        check_tokens(fragment.tokens(), fragment.source(), 0),
        fragment.source().len()
    );
}
fn check_tokens(
    tokens: &[purrassist_core::markdown::Inline],
    source: &str,
    mut position: usize,
) -> usize {
    for token in tokens {
        assert_eq!(token.span.start, position);
        assert!(source.get(token.span.clone()).is_some());
        match &token.kind {
            InlineKind::Emphasis { strong, children } => {
                let width = if *strong { 2 } else { 1 };
                assert_eq!(
                    check_tokens(children, source, token.span.start + width),
                    token.span.end - width
                );
            }
            InlineKind::Link { children, .. } | InlineKind::Image { children, .. } => {
                let width = if matches!(token.kind, InlineKind::Image { .. }) {
                    2
                } else {
                    1
                };
                let end = check_tokens(children, source, token.span.start + width);
                assert!(source[end..].starts_with("]("));
            }
            _ => {}
        }
        position = token.span.end;
    }
    position
}

#[test]
fn crlf_escapes_code_and_inline_html_preserve_exact_spans() {
    let source = "café \\* ` x\r\ny `  \r\n  <a title='x>y'>☕</a>";
    let mut governor = Governor::new(Budget::default());
    let fragment = InlineFragment::parse(source, &mut governor).unwrap();
    assert_eq!(fragment.source(), source);
    assert_partition(&fragment);
    assert!(fragment.tokens().iter().any(
        |token| matches!(&token.kind, InlineKind::Code(text) if text == "x y")
            && &source[token.span.clone()] == "` x\r\ny `"
    ));
    assert_eq!(
        fragment.render_html(&mut governor).unwrap(),
        "café * <code>x y</code><br />\n<a title='x>y'>☕</a>"
    );
    for (source, expected) in [
        ("foo\\\r\n", "foo\\"),
        ("foo  \rbar", "foo<br />\nbar"),
        ("`  `", "<code>  </code>"),
        ("`\u{a0}`", "<code>\u{a0}</code>"),
        ("\0", "\u{fffd}"),
        (
            "<https://x/\0>",
            "<a href=\"https://x/%EF%BF%BD\">https://x/\u{fffd}</a>",
        ),
        ("<a\n\nb>", "&lt;a\n\nb&gt;"),
        (
            "<https://x/☕>",
            "<a href=\"https://x/%E2%98%95\">https://x/☕</a>",
        ),
    ] {
        let fragment =
            InlineFragment::parse(source, &mut Governor::new(Budget::default())).unwrap();
        assert_partition(&fragment);
        assert_eq!(
            fragment
                .render_html(&mut Governor::new(Budget::default()))
                .unwrap(),
            expected
        );
    }
}
#[test]
fn lookahead_and_rendering_obey_sticky_resource_failures() {
    let mut governor = Governor::new(Budget {
        work: 5000,
        ..Budget::default()
    });
    let error = InlineFragment::parse(&"<".repeat(200), &mut governor).unwrap_err();
    assert_eq!(error, StopReason::BudgetExceeded(Resource::Work));
    assert_eq!(governor.check(), Err(error));
    let fragment = InlineFragment::parse("`hello`", &mut Governor::new(Budget::default())).unwrap();
    let mut cancelled = Governor::new(Budget::default());
    cancelled.cancel();
    assert_eq!(
        fragment.render_html(&mut cancelled),
        Err(StopReason::Cancelled)
    );
    let mut tiny = Governor::new(Budget {
        scratch_bytes: 1,
        ..Budget::default()
    });
    assert_eq!(
        fragment.render_html(&mut tiny),
        Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
}

#[test]
fn all_official_sources_keep_a_contiguous_inline_partition() {
    let mut checked = 0;
    for corpus in [
        include_str!("data/markdown/commonmark-0.31.2.json"),
        include_str!("data/markdown/gfm-0.29.0.gfm.13.json"),
    ] {
        let Value::Array(cases) = json::parse(corpus).unwrap().value else {
            panic!("corpus")
        };
        for case in cases {
            let source = case.get("markdown").unwrap().as_str().unwrap();
            let fragment = InlineFragment::parse(
                source,
                &mut Governor::new(purrassist_core::Effort::Maximum.budget()),
            )
            .unwrap();
            assert_eq!(fragment.source(), source);
            assert_partition(&fragment);
            checked += 1;
        }
    }
    assert_eq!(checked, 1324);
}

#[test]
fn inline_links_images_and_precedence_keep_nested_source_ranges() {
    for (source, expected) in [
        (
            "[a *b*](foo(and\\(more\\)) 'title')",
            "<a href=\"foo(and(more))\" title=\"title\">a <em>b</em></a>",
        ),
        (
            "[a [b](inner)](outer)",
            "[a <a href=\"inner\">b</a>](outer)",
        ),
        (
            "![a *b* [c](u)](image.png \"t\")",
            "<img src=\"image.png\" alt=\"a b c\" title=\"t\" />",
        ),
        ("[x](<a b>)", "<a href=\"a%20b\">x</a>"),
        ("[x](foo\\(bar\\))", "<a href=\"foo(bar)\">x</a>"),
        ("[x](foo bar)", "[x](foo bar)"),
        (
            "[<https://x>](outer)",
            "[<a href=\"https://x\">https://x</a>](outer)",
        ),
        ("[\\*literal\\*](u)", "<a href=\"u\">*literal*</a>"),
        ("[x]( \"title\")", "<a href=\"\" title=\"title\">x</a>"),
    ] {
        let mut governor = Governor::new(Budget::default());
        let fragment = InlineFragment::parse(source, &mut governor).unwrap();
        assert_partition(&fragment);
        assert_eq!(fragment.render_html(&mut governor).unwrap(), expected);
    }
}
#[test]
fn nested_emphasis_can_be_selected_edited_and_undone_exactly() {
    use purrassist_core::{
        Outcome,
        edits::{EditPlan, TextEdit},
    };
    let source = "café **cold *tea***\r\n";
    let fragment = InlineFragment::parse(source, &mut Governor::new(Budget::default())).unwrap();
    let InlineKind::Emphasis { children, .. } = &fragment.tokens()[1].kind else {
        panic!("outer emphasis")
    };
    let selection = children
        .iter()
        .find(|token| matches!(token.kind, InlineKind::Emphasis { strong: false, .. }))
        .unwrap();
    assert_eq!(&source[selection.span.clone()], "*tea*");
    let plan = EditPlan {
        source: fragment.digest(),
        edits: vec![TextEdit {
            range: selection.span.clone(),
            replacement: "`tea`".into(),
        }],
    };
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply")
    };
    assert_eq!(applied.text, "café **cold `tea`**\r\n");
    let Outcome::Resolved(undone) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo")
    };
    assert_eq!(undone.text, source);
}
#[test]
fn nested_inline_depth_limits_apply_during_parse_and_render() {
    let source = format!("{}x{}", "*".repeat(300), "*".repeat(300));
    let error = InlineFragment::parse(&source, &mut Governor::new(Budget::default())).unwrap_err();
    assert_eq!(error, StopReason::BudgetExceeded(Resource::Depth));
    let fragment =
        InlineFragment::parse("***tea***", &mut Governor::new(Budget::default())).unwrap();
    let mut governor = Governor::new(Budget {
        depth: 1,
        ..Budget::default()
    });
    assert_eq!(
        fragment.render_html(&mut governor),
        Err(StopReason::BudgetExceeded(Resource::Depth))
    );
}
