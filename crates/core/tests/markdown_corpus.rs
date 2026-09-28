// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Official example source preservation and leaf-block classification.
//! This is not full HTML rendering conformance; inline/container semantics remain required.
use purrassist_core::{
    Budget, Governor,
    json::{self, Value},
    markdown::{BlockKind, Document},
};

const CORPORA: &[(&str, &str, usize)] = &[
    (
        "CommonMark 0.31.2",
        include_str!("data/markdown/commonmark-0.31.2.json"),
        652,
    ),
    (
        "GFM 0.29.0.gfm.13",
        include_str!("data/markdown/gfm-0.29.0.gfm.13.json"),
        672,
    ),
];

#[test]
fn every_official_example_retains_complete_source() {
    for &(name, corpus, count) in CORPORA {
        let Value::Array(cases) = json::parse(corpus).unwrap().value else {
            panic!("corpus must be an array")
        };
        assert_eq!(cases.len(), count);
        for case in cases {
            let source = case.get("markdown").unwrap().as_str().unwrap();
            let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
            let mut end = 0;
            for block in document.blocks() {
                assert_eq!(block.span.start, end, "{name}: {source:?}");
                assert!(source.get(block.span.clone()).is_some());
                end = block.span.end;
            }
            assert_eq!(end, source.len(), "{name}: {source:?}");
            assert_eq!(document.source(), source);
            check_children(document.blocks(), 0..source.len(), source);
        }
    }
}

#[test]
fn official_leaf_examples_start_with_the_expected_block() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for &(name, corpus, _) in CORPORA {
        let Value::Array(cases) = json::parse(corpus).unwrap().value else {
            panic!("corpus must be an array")
        };
        for case in cases {
            let section = case.get("section").unwrap().as_str().unwrap();
            if !matches!(
                section,
                "ATX headings"
                    | "Setext headings"
                    | "Thematic breaks"
                    | "Indented code blocks"
                    | "Fenced code blocks"
                    | "Paragraphs"
                    | "Blank lines"
            ) {
                continue;
            }
            let source = case.get("markdown").unwrap().as_str().unwrap();
            let expected = case.get("html").unwrap().as_str().unwrap();
            let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
            let actual = document.blocks().iter().find(|block| {
                !matches!(
                    block.kind,
                    BlockKind::Blank | BlockKind::ReferenceDefinition
                )
            });
            let kind = actual.map_or("", |block| match block.kind {
                BlockKind::Heading { level: 1, .. } => "<h1>",
                BlockKind::Heading { level: 2, .. } => "<h2>",
                BlockKind::Heading { level: 3, .. } => "<h3>",
                BlockKind::Heading { level: 4, .. } => "<h4>",
                BlockKind::Heading { level: 5, .. } => "<h5>",
                BlockKind::Heading { .. } => "<h6>",
                BlockKind::Paragraph => "<p>",
                BlockKind::IndentedCode | BlockKind::CodeFence { .. } => "<pre>",
                BlockKind::ThematicBreak => "<hr />",
                BlockKind::Quote => "<blockquote>",
                BlockKind::ListItem { ordinal: None, .. } => "<ul>",
                BlockKind::ListItem { .. } => "<ol",
                BlockKind::Html
                | BlockKind::ReferenceDefinition
                | BlockKind::Table { .. }
                | BlockKind::Blank => "unsupported",
            });
            if !expected.starts_with(kind) || (kind.is_empty() && !expected.is_empty()) {
                failures.push(format!("{name} {} ({section}): expected {expected:?}, classified {kind:?}; source={source:?}", case.get("example").unwrap().to_json()));
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 228);
    assert_eq!(failures, Vec::<String>::new());
}

// Compare quote hierarchy and immediate leaf classes against all official
// blockquote examples. Inline rendering and list-internal structure are separate.
#[test]
fn official_blockquote_hierarchy_matches_expected_html() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for &(name, corpus, _) in CORPORA {
        let Value::Array(cases) = json::parse(corpus).unwrap().value else {
            panic!("corpus must be an array")
        };
        for case in cases {
            if case.get("section").unwrap().as_str() != Some("Block quotes") {
                continue;
            }
            let source = case.get("markdown").unwrap().as_str().unwrap();
            let expected = case.get("html").unwrap().as_str().unwrap();
            let document = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
            let mut actual = String::new();
            block_signature(document.blocks(), &mut actual);
            let expected = html_signature(expected);
            if actual != expected {
                failures.push(format!(
                    "{name} {}: {source:?}; expected {expected}, actual {actual}",
                    case.get("example").unwrap().to_json()
                ));
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 50);
    assert_eq!(failures, Vec::<String>::new());
}
fn block_signature(blocks: &[purrassist_core::markdown::Block], result: &mut String) {
    for block in blocks {
        match block.kind {
            BlockKind::Quote => {
                result.push('(');
                block_signature(&block.children, result);
                result.push(')');
            }
            BlockKind::Paragraph => result.push('p'),
            BlockKind::Heading { level, .. } => result.push(char::from(b'0' + level)),
            BlockKind::IndentedCode | BlockKind::CodeFence { .. } => result.push('c'),
            BlockKind::ThematicBreak => result.push('-'),
            BlockKind::ListItem { .. } => result.push('i'),
            BlockKind::Blank | BlockKind::ReferenceDefinition => {}
            BlockKind::Html | BlockKind::Table { .. } => result.push('?'),
        }
    }
}
fn html_signature(mut html: &str) -> String {
    let mut result = String::new();
    while let Some(start) = html.find('<') {
        html = &html[start..];
        let Some(end) = html.find('>') else { break };
        let tag = &html[..=end];
        html = &html[end + 1..];
        match tag {
            "<blockquote>" => result.push('('),
            "</blockquote>" => result.push(')'),
            "<p>" => result.push('p'),
            "<pre>" => result.push('c'),
            "<hr />" => result.push('-'),
            "<li>" => {
                result.push('i');
                html = html.split_once("</li>").map_or("", |(_, rest)| rest);
            }
            "<h1>" | "<h2>" | "<h3>" | "<h4>" | "<h5>" | "<h6>" => {
                result.push(char::from(tag.as_bytes()[2]));
            }
            _ => {}
        }
    }
    result
}

fn check_children(
    blocks: &[purrassist_core::markdown::Block],
    parent: std::ops::Range<usize>,
    source: &str,
) {
    let mut end = parent.start;
    for block in blocks {
        assert!(end <= block.span.start && block.span.end <= parent.end);
        assert!(source.get(block.span.clone()).is_some());
        check_children(&block.children, block.span.clone(), source);
        end = block.span.end;
    }
}
