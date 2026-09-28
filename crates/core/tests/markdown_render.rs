// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Full-document rendering qualification against both official corpora.
use purrassist_core::{
    Budget, Governor,
    json::{self, Value},
    markdown::{Dialect, Document, Extension, Extensions},
};

#[test]
fn official_documents() {
    let mut failures = Vec::new();
    let mut passed = 0;
    let Value::Array(profiles) = json::parse(include_str!("data/markdown/gfm-profiles.json"))
        .unwrap()
        .value
    else {
        panic!("profiles")
    };
    for (dialect, corpus) in [
        (
            Dialect::CommonMark,
            include_str!("data/markdown/commonmark-0.31.2.json"),
        ),
        (
            Dialect::Gfm,
            include_str!("data/markdown/gfm-0.29.0.gfm.13.json"),
        ),
    ] {
        let Value::Array(cases) = json::parse(corpus).unwrap().value else {
            panic!("array")
        };
        for case in cases {
            let source = case.get("markdown").unwrap().as_str().unwrap();
            let expected = case.get("html").unwrap().as_str().unwrap();
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(source, &mut governor).unwrap();
            let number: usize = case.get("example").unwrap().to_json().parse().unwrap();
            let mut extensions = Extensions::default();
            if dialect == Dialect::Gfm {
                match profiles[number - 1].as_str().unwrap() {
                    "autolink" => extensions = extensions.with(Extension::Autolinks),
                    "strikethrough" => extensions = extensions.with(Extension::Strikethrough),
                    "table" => extensions = extensions.with(Extension::Tables),
                    "tagfilter" => extensions = extensions.with(Extension::TagFilter),
                    "disabled" => extensions = extensions.with(Extension::TaskLists),
                    "" => {}
                    unknown => panic!("unknown profile: {unknown}"),
                }
            }
            let actual = document
                .render_with_extensions(dialect, extensions, &mut governor)
                .unwrap();
            if actual == expected {
                passed += 1;
            } else {
                failures.push(format!(
                    "{dialect:?} {} {}: {source:?}; expected {expected:?}, got {actual:?}",
                    case.get("example").unwrap().to_json(),
                    case.get("section").unwrap().as_str().unwrap()
                ));
            }
        }
    }
    assert_eq!(passed + failures.len(), 1324);
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn tab_projection_retains_original_bytes_and_inline_tabs() {
    let source = "> \t# a\tb\r\n>\t\tcode\r\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    assert_eq!(document.source(), source);
    assert_eq!(document.blocks()[0].span, 0..source.len());
    assert_eq!(
        document
            .render_html(Dialect::CommonMark, &mut governor)
            .unwrap(),
        "<blockquote>\n<h1>a\tb</h1>\n<pre><code>  code\n</code></pre>\n</blockquote>\n"
    );
    for child in &document.blocks()[0].children {
        assert_eq!(child.lines.len(), child.line_padding.len());
        for range in &child.lines {
            assert!(source.get(range.clone()).is_some());
        }
    }
}

#[test]
fn definitions_inside_containers_resolve_but_code_definitions_do_not() {
    let source = "[a] [b] [c]\n\n> [a]: /quote\n\n- [b]: /list\n\n```\n[c]: /code\n```\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    assert_eq!(
        document
            .render_html(Dialect::CommonMark, &mut governor)
            .unwrap(),
        "<p><a href=\"/quote\">a</a> <a href=\"/list\">b</a> [c]</p>\n<blockquote>\n</blockquote>\n<ul>\n<li></li>\n</ul>\n<pre><code>[c]: /code\n</code></pre>\n"
    );
}

#[test]
fn default_gfm_combines_extensions_and_protects_code_and_existing_links() {
    let source = "- [x] ~~go~~ www.example.org\n\n[www.example.org](/chosen) `www.example.org ~~code~~ <script>`\n\n<script>blocked</script>\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    assert_eq!(
        document.render_html(Dialect::Gfm, &mut governor).unwrap(),
        "<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> <del>go</del> <a href=\"http://www.example.org\">www.example.org</a></li>\n</ul>\n<p><a href=\"/chosen\">www.example.org</a> <code>www.example.org ~~code~~ &lt;script&gt;</code></p>\n&lt;script>blocked&lt;/script>\n"
    );
}

#[test]
fn extension_nodes_keep_source_ranges_and_budget_failures() {
    use purrassist_core::{
        StopReason,
        contract::Resource,
        markdown::{InlineFragment, InlineKind, References},
    };
    let source = "café ~~*tea*~~ (www.example.org/a)) a_b@c.de";
    let mut governor = Governor::new(Budget::default());
    let fragment = InlineFragment::parse_with_extensions(
        source,
        Dialect::Gfm,
        &References::default(),
        Extensions::gfm(),
        &mut governor,
    )
    .unwrap();
    let mut cursor = 0;
    for token in fragment.tokens() {
        assert_eq!(token.span.start, cursor);
        cursor = token.span.end;
        match &token.kind {
            InlineKind::Autolink { label, .. } => assert_eq!(&source[token.span.clone()], label),
            InlineKind::Strikethrough { children } => {
                assert_eq!(&source[token.span.clone()], "~~*tea*~~");
                assert_eq!(&source[children[0].span.clone()], "*tea*");
            }
            _ => {}
        }
    }
    assert_eq!(cursor, source.len());
    let document = Document::parse(source, &mut governor).unwrap();
    let mut tiny = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert_eq!(
        document.render_html(Dialect::Gfm, &mut tiny),
        Err(StopReason::BudgetExceeded(Resource::Work))
    );
    let mut cancelled = Governor::new(Budget::default());
    cancelled.cancel();
    assert_eq!(
        document.render_html(Dialect::Gfm, &mut cancelled),
        Err(StopReason::Cancelled)
    );
}

#[test]
fn inline_tag_filter_is_explicit_and_does_not_change_code() {
    use purrassist_core::markdown::{InlineFragment, References};
    let mut g = Governor::new(Budget::default());
    let source = "<SCRIPT>x</SCRIPT> <scripture> `<script>`";
    let fragment = InlineFragment::parse_with_extensions(
        source,
        Dialect::Gfm,
        &References::default(),
        Extensions::default().with(Extension::TagFilter),
        &mut g,
    )
    .unwrap();
    assert_eq!(fragment.source(), source);
    assert_eq!(
        fragment.render_html(&mut g).unwrap(),
        "&lt;SCRIPT>x&lt;/SCRIPT> <scripture> <code>&lt;script&gt;</code>"
    );
}
