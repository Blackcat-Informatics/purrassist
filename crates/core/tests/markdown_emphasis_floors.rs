// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Delimiter search bounds retain the closer's flanking category.
use purrassist_core::{
    Budget, Governor, StopReason,
    contract::Resource,
    markdown::{Dialect, Inline, InlineFragment, InlineKind},
};

#[test]
fn dual_purpose_failure_does_not_hide_a_later_close_only_match() {
    // CommonMark's processing algorithm indexes opener bounds by marker,
    // length modulo three, and whether the closer can also open. These
    // expected answers were independently checked with upstream cmark 0.31.1.
    let cases = [
        ("*a**b** c**", "<em>a<strong>b</strong> c</em>*"),
        ("*)a**b** c**", "<em>)a<strong>b</strong> c</em>*"),
        ("*a**b** c*", "<em>a<strong>b</strong> c</em>"),
        ("_a_b_ c__", "<em>a_b</em> c__"),
        ("a***b***c", "a<em><strong>b</strong></em>c"),
        ("***a***", "<em><strong>a</strong></em>"),
        ("**a*b*c**", "<strong>a<em>b</em>c</strong>"),
        (
            "[*a**b** c**](u)",
            "<a href=\"u\"><em>a<strong>b</strong> c</em>*</a>",
        ),
        (
            "*a**b** c** and *d**e** f**",
            "<em>a<strong>b</strong> c</em>* and <em>d<strong>e</strong> f</em>*",
        ),
        (
            "Hot *)and**nd** _ict *)and**nd** _icd** _iced_.",
            "Hot <em>)and<strong>nd</strong> _ict <em>)and<strong>nd</strong> _icd</em></em> <em>iced</em>.",
        ),
    ];
    for dialect in [Dialect::CommonMark, Dialect::Gfm] {
        for (source, expected) in cases {
            let mut governor = Governor::new(Budget::default());
            let fragment = InlineFragment::parse_with_dialect(source, dialect, &mut governor)
                .expect("admitted delimiter input");
            assert_eq!(fragment.source(), source);
            assert_eq!(fragment.render_html(&mut governor).unwrap(), expected);
            assert_eq!(partition(fragment.tokens(), source, 0), source.len());
        }
    }
}

fn partition(tokens: &[Inline], source: &str, mut cursor: usize) -> usize {
    for token in tokens {
        assert_eq!(token.span.start, cursor);
        assert!(source.get(token.span.clone()).is_some());
        if let InlineKind::Emphasis { strong, children } = &token.kind {
            let width = if *strong { 2 } else { 1 };
            assert_eq!(
                partition(children, source, token.span.start + width),
                token.span.end - width
            );
        }
        cursor = token.span.end;
    }
    cursor
}

#[test]
fn delimiter_category_search_exhaustion_is_an_explicit_outcome() {
    let source = "*a**b** c** and *d**e** f**";
    for resource in [Resource::Work, Resource::ScratchBytes, Resource::Depth] {
        let mut budget = Budget::default();
        match resource {
            Resource::Work => budget.work = 0,
            Resource::ScratchBytes => budget.scratch_bytes = 0,
            Resource::Depth => budget.depth = 0,
            _ => unreachable!("test resource inventory"),
        }
        assert_eq!(
            InlineFragment::parse(source, &mut Governor::new(budget)).unwrap_err(),
            StopReason::BudgetExceeded(resource)
        );
    }
}
