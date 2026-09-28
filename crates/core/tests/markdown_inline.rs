// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Whole selected specification sections for the first inline parser layer.
use purrassist_core::{
    Budget, Governor, StopReason,
    contract::Resource,
    json::{self, Value},
    markdown::{InlineFragment, InlineKind},
};

#[test]
fn official_code_autolink_softbreak_and_current_raw_html_sections() {
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
            if !(matches!(section, "Code spans" | "Autolinks" | "Soft line breaks")
                || current && section == "Raw HTML")
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
                let fragment = InlineFragment::parse(paragraph, &mut governor).unwrap();
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
    assert_eq!(checked, 106);
    assert_eq!(failures, Vec::<String>::new());
}
fn assert_partition(fragment: &InlineFragment) {
    let mut position = 0;
    for token in fragment.tokens() {
        assert_eq!(token.span.start, position);
        assert!(fragment.source().get(token.span.clone()).is_some());
        position = token.span.end;
    }
    assert_eq!(position, fragment.source().len());
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
