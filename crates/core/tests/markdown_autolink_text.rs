// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Extended emails are recognized in interpreted text nodes while their spans
//! retain the complete original escape/entity spelling.
use purrassist_core::{
    Budget, Governor, StopReason,
    contract::Resource,
    markdown::{Dialect, Document, Extensions, InlineFragment, InlineKind, References},
};

fn gfm(source: &str, governor: &mut Governor) -> Result<InlineFragment, StopReason> {
    InlineFragment::parse_with_extensions(
        source,
        Dialect::Gfm,
        &References::default(),
        Extensions::gfm(),
        governor,
    )
}

fn check_ranges(source: &str, tokens: &[purrassist_core::markdown::Inline]) {
    for token in tokens {
        assert!(source.get(token.span.clone()).is_some());
        if let InlineKind::Autolink { label, .. } = &token.kind {
            assert_eq!(&source[token.span.clone()], r"a\@b.com");
            assert_eq!(label, "a@b.com");
        }
        if let InlineKind::Emphasis { children, .. } = &token.kind {
            check_ranges(source, children);
        }
    }
}

#[test]
fn decoded_emails_preserve_exact_original_source_units() {
    for (spelling, label, destination) in [
        (r"a\@b.com", "a@b.com", "mailto:a@b.com"),
        (r"a@b\.com", "a@b.com", "mailto:a@b.com"),
        ("a&#64;b.com", "a@b.com", "mailto:a@b.com"),
        ("&#x61;@b.com", "a@b.com", "mailto:a@b.com"),
        ("a@b&period;com", "a@b.com", "mailto:a@b.com"),
        (
            "&fjlig;oo@example.com",
            "fjoo@example.com",
            "mailto:fjoo@example.com",
        ),
        (
            r"\_name\_@exa\.com",
            "_name_@exa.com",
            "mailto:_name_@exa.com",
        ),
        (r"mailto\:a\@b.com", "mailto:a@b.com", "mailto:a@b.com"),
        (
            r"xmpp\:a\@b.com/resource",
            "xmpp:a@b.com/resource",
            "xmpp:a@b.com/resource",
        ),
    ] {
        for prefix in ["", "pré ", "&NotEqualTilde; "] {
            let source = format!("{prefix}{spelling} après");
            let mut governor = Governor::new(Budget::default());
            let fragment = gfm(&source, &mut governor).unwrap();
            assert_eq!(fragment.source(), source);
            let links: Vec<_> = fragment
                .tokens()
                .iter()
                .filter(|token| matches!(token.kind, InlineKind::Autolink { .. }))
                .collect();
            assert_eq!(links.len(), 1, "{source:?}");
            let link = links[0];
            assert_eq!(link.span, prefix.len()..prefix.len() + spelling.len());
            assert_eq!(&source[link.span.clone()], spelling);
            assert_eq!(
                link.kind,
                InlineKind::Autolink {
                    destination: destination.into(),
                    label: label.into()
                }
            );
            let html = fragment.render_html(&mut governor).unwrap();
            assert!(
                html.contains(&format!("<a href=\"{destination}\">{label}</a>")),
                "{source:?}: {html:?}"
            );
            assert!(html.ends_with(" après"));
            let ordinary = InlineFragment::parse(&source, &mut governor).unwrap();
            assert!(
                ordinary
                    .tokens()
                    .iter()
                    .all(|token| !matches!(token.kind, InlineKind::Autolink { .. }))
            );
        }
    }
}

#[test]
fn decoded_email_retries_preserve_literal_prefix_and_suffix() {
    let source = r"user@ex@e\.com &amp; tail";
    let mut governor = Governor::new(Budget::default());
    let fragment = gfm(source, &mut governor).unwrap();
    assert_eq!(fragment.tokens()[0].kind, InlineKind::Text("user@".into()));
    assert_eq!(fragment.tokens()[0].span, 0..5);
    assert_eq!(fragment.tokens()[1].span, 5..14);
    assert_eq!(
        fragment.render_html(&mut governor).unwrap(),
        "user@<a href=\"mailto:ex@e.com\">ex@e.com</a> &amp; tail"
    );
    assert_eq!(
        fragment.tokens()[2].kind,
        InlineKind::Text(" & tail".into())
    );
    assert_eq!(fragment.tokens()[2].span, 14..source.len());
    for ending in ["\n", "\r\n", "\r"] {
        for prefix in ["", "> ", "- ", "> - "] {
            let source = format!("{prefix}user@ex@e\\.com{ending}");
            let document = Document::parse(&source, &mut governor).unwrap();
            assert_eq!(document.source(), source);
            let html = document.render_html(Dialect::Gfm, &mut governor).unwrap();
            assert!(html.contains("user@<a href=\"mailto:ex@e.com\">ex@e.com</a>"));
        }
    }
}

#[test]
fn decoded_email_mapping_respects_node_boundaries_and_single_interpretation() {
    let mut governor = Governor::new(Budget::default());
    for (source, expected) in [
        (
            r"*pré* a\@b.com &amp;copy;",
            "<em>pré</em> <a href=\"mailto:a@b.com\">a@b.com</a> &amp;copy;",
        ),
        (
            r"[a\@b.com] tail",
            "[<a href=\"mailto:a@b.com\">a@b.com</a>] tail",
        ),
        (
            r"**a\@b.com**",
            "<strong><a href=\"mailto:a@b.com\">a@b.com</a></strong>",
        ),
        (
            r"a\@b.com&#32;tail",
            "<a href=\"mailto:a@b.com\">a@b.com</a> tail",
        ),
        (
            r"lead&#10;a\@b.com",
            "lead\n<a href=\"mailto:a@b.com\">a@b.com</a>",
        ),
        (
            r"a\@b.com&#9;tail",
            "<a href=\"mailto:a@b.com\">a@b.com</a>\ttail",
        ),
        (
            r"a\@b.com &amp;#64;tail",
            "<a href=\"mailto:a@b.com\">a@b.com</a> &amp;#64;tail",
        ),
    ] {
        let fragment = gfm(source, &mut governor).unwrap();
        assert_eq!(fragment.source(), source);
        assert_eq!(
            fragment.render_html(&mut governor).unwrap(),
            expected,
            "{source:?}"
        );
        check_ranges(source, fragment.tokens());
    }
}

#[test]
fn decoded_emails_protect_code_existing_links_and_uninterpreted_escapes() {
    let mut governor = Governor::new(Budget::default());
    for source in [
        r"a\\@b.com",
        r"a\&commat;b.com",
        "`a@b.com`",
        r"[a\@b.com](/dest)",
        r"![a\@b.com](/image)",
        "<span title=\"a@b.com\">",
        "a@b&#32;.com",
    ] {
        let fragment = gfm(source, &mut governor).unwrap();
        assert!(
            fragment
                .tokens()
                .iter()
                .all(|token| !matches!(token.kind, InlineKind::Autolink { .. })),
            "{source:?}"
        );
        assert_eq!(fragment.source(), source);
    }
    for source in [r"a\@b.com", "a&#64;b.com", "&fjlig;oo@example.com"] {
        for budget in [
            Budget {
                work: 0,
                ..Budget::default()
            },
            Budget {
                scratch_bytes: 0,
                ..Budget::default()
            },
        ] {
            let mut denied = Governor::new(budget);
            let resource = if budget.work == 0 {
                Resource::Work
            } else {
                Resource::ScratchBytes
            };
            assert_eq!(
                gfm(source, &mut denied).unwrap_err(),
                StopReason::BudgetExceeded(resource)
            );
        }
        let mut cancelled = Governor::new(Budget::default());
        cancelled.cancel();
        assert_eq!(
            gfm(source, &mut cancelled).unwrap_err(),
            StopReason::Cancelled
        );
    }
}
