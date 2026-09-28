// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Independent upstream entity values, grammar edges, and official inline examples.
use purrassist_core::{
    Budget, Governor,
    json::{self, Value},
    markdown::{InlineFragment, InlineKind},
};

fn rendered(source: &str) -> String {
    let mut governor = Governor::new(Budget::default());
    let fragment = InlineFragment::parse(source, &mut governor).unwrap();
    assert_eq!(fragment.source(), source);
    fragment.render_html(&mut governor).unwrap()
}

#[test]
fn every_upstream_named_reference_decodes_without_reinterpreting_its_value() {
    let Value::Object(entries) = json::parse(include_str!("data/entities/entities.json"))
        .unwrap()
        .value
    else {
        panic!("entity object")
    };
    let mut count = 0;
    for entry in entries {
        if !entry.key.ends_with(';') {
            continue;
        }
        let expected = entry.value.get("characters").unwrap().as_str().unwrap();
        let mut governor = Governor::new(Budget::default());
        let fragment = InlineFragment::parse(&entry.key, &mut governor).unwrap();
        assert_eq!(fragment.tokens().len(), 1, "{}", entry.key);
        assert_eq!(fragment.tokens()[0].span, 0..entry.key.len());
        assert_eq!(
            fragment.tokens()[0].kind,
            InlineKind::Text(expected.into()),
            "{}",
            entry.key
        );
        count += 1;
    }
    assert_eq!(count, 2125);
}

#[test]
fn numeric_limits_and_invalid_scalars_are_explicit() {
    for (source, expected) in [
        ("&#0; &#xD800; &#x110000; &#9999999;", "� � � �"),
        ("&#x10ffff;", "\u{10ffff}"),
        ("&#0000065; &#x000041;", "A A"),
        ("&#00000065; &#x0000041;", "&amp;#00000065; &amp;#x0000041;"),
        (
            "&#x; &#; &#-1; &#65 &#Xg;",
            "&amp;#x; &amp;#; &amp;#-1; &amp;#65 &amp;#Xg;",
        ),
        ("&#13; &#128;", "\r \u{80}"),
        (
            "&amp;copy; \\&copy; &copy",
            "&amp;copy; &amp;copy; &amp;copy",
        ),
        ("&#42;x&#42; &ast;x&ast;", "*x* *x*"),
        (
            "[x](a&amp;b 'c&amp;d')",
            "<a href=\"a&amp;b\" title=\"c&amp;d\">x</a>",
        ),
        ("[x](a\\&amp;b)", "<a href=\"a&amp;amp;b\">x</a>"),
        ("[x](url '&#13;')", "<a href=\"url\" title=\"\r\">x</a>"),
        (
            "<https://x.test/?a&amp;b>",
            "<a href=\"https://x.test/?a&amp;amp;b\">https://x.test/?a&amp;amp;b</a>",
        ),
        ("<i title='&copy;'>", "<i title='&copy;'>"),
        ("`&copy;`", "<code>&amp;copy;</code>"),
    ] {
        assert_eq!(rendered(source), expected, "{source:?}");
    }
}

#[test]
fn official_entity_examples_with_inline_only_structure() {
    let mut checked = 0;
    for (offset, corpus) in [
        (0, include_str!("data/markdown/commonmark-0.31.2.json")),
        (296, include_str!("data/markdown/gfm-0.29.0.gfm.13.json")),
    ] {
        let Value::Array(cases) = json::parse(corpus).unwrap().value else {
            panic!("array")
        };
        for case in cases {
            if case.get("section").unwrap().as_str().unwrap()
                != "Entity and numeric character references"
            {
                continue;
            }
            let number: usize = case.get("example").unwrap().to_json().parse().unwrap();
            // Document-level HTML blocks, definitions, fences, indented code,
            // and lists are qualified by the future complete block renderer.
            if [31, 33, 34, 36, 38].contains(&(number - offset)) {
                continue;
            }
            let source = case.get("markdown").unwrap().as_str().unwrap();
            let expected = case.get("html").unwrap().as_str().unwrap();
            assert_eq!(
                format!("<p>{}</p>\n", rendered(source)),
                expected,
                "{number}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 24);
}
