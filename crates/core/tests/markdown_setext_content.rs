// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Setext concatenation preserves inline whitespace and original edit spans.
use purrassist_core::{
    Budget, Governor, Outcome,
    markdown::{BlockKind, Dialect, Document},
};

#[test]
fn setext_interior_whitespace_is_inline_source() {
    for (input, title, body) in [
        ("a  \nb\n---\n", "a  \nb", "a<br />\nb"),
        ("a     \nb\n---\n", "a     \nb", "a<br />\nb"),
        ("a\\\nb\n===\n", "a\\\nb", "a<br />\nb"),
        (" a\t\n   b  \t\n---\n", "a\t\nb", "a\nb"),
        (" `a  \n  b`\n---\n", "`a  \nb`", "<code>a   b</code>"),
        ("\u{b}a\nb\u{c}\n---\n", "\u{b}a\nb\u{c}", "\u{b}a\nb\u{c}"),
        (
            "| a | b |\n| -     \n| -     | -- |\n|\n-\n",
            "| a | b |\n| -     \n| -     | -- |\n|",
            "| a | b |\n| -<br />\n| -     | -- |\n|",
        ),
    ] {
        for newline in ["\n", "\r\n", "\r"] {
            let source = input.replace('\n', newline);
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            assert_eq!(document.source(), source);
            assert_eq!(document.blocks().len(), 1);
            assert_eq!(document.blocks()[0].span, 0..source.len());
            let BlockKind::Heading {
                level,
                title: actual,
            } = &document.blocks()[0].kind
            else {
                panic!("expected a heading: {source:?}")
            };
            assert_eq!(actual, title, "{source:?}");
            for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                assert_eq!(
                    document.render_html(dialect, &mut governor).unwrap(),
                    format!("<h{level}>{body}</h{level}>\n"),
                    "{dialect:?}: {source:?}"
                );
            }
            let renamed = document
                .rename_heading(0, "Renamed", &mut governor)
                .unwrap();
            let Outcome::Resolved(applied) = renamed.apply(&source, Budget::default()).outcome
            else {
                panic!("rename did not resolve")
            };
            let Outcome::Resolved(undo) = applied
                .inverse
                .apply(&applied.text, Budget::default())
                .outcome
            else {
                panic!("inverse did not resolve")
            };
            assert_eq!(undo.text, source);
            let underline = source
                .trim_end_matches(['\r', '\n'])
                .rsplit(['\r', '\n'])
                .next()
                .unwrap();
            let leading: String = source
                .chars()
                .take_while(|character| matches!(character, ' ' | '\t'))
                .collect();
            assert_eq!(
                applied.text,
                format!("{leading}Renamed{newline}{underline}{newline}")
            );
        }
    }
}

#[test]
fn partially_consumed_lazy_tabs_keep_their_virtual_padding() {
    for (suffix, code) in [(" ", "a    b"), ("  ", "a     b")] {
        for (underline, level) in [("---", 2), ("===", 1)] {
            for newline in ["\n", "\r\n", "\r"] {
                let source =
                    format!("> > `a{newline}>\t{suffix}b`{newline}> > {underline}{newline}");
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                let heading = document.block_at(&[0, 0, 0]).unwrap();
                assert_eq!(heading.line_padding[1], 2);
                let content = source.find('b').unwrap();
                assert_eq!(heading.lines[1].start, content - suffix.len());
                assert_eq!(document.source(), source);
                for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                    assert_eq!(
                        document.render_html(dialect, &mut governor).unwrap(),
                        format!(
                            "<blockquote>\n<blockquote>\n<h{level}><code>{code}</code></h{level}>\n</blockquote>\n</blockquote>\n"
                        )
                    );
                }
            }
        }
    }
}

#[test]
fn marked_and_lazy_continuation_indentation_have_distinct_semantics() {
    for (input, title, body) in [
        (
            "> `a\n    b`\n> ---\n",
            "`a\n    b`",
            "<code>a     b</code>",
        ),
        ("> `a\n>     b`\n> ---\n", "`a\nb`", "<code>a b</code>"),
        ("> a  \n> b\n> ---\n", "a  \nb", "a<br />\nb"),
        (
            "> a<span title=\"x\n    y\">b\n> ---\n",
            "a<span title=\"x\n    y\">b",
            "a<span title=\"x\n    y\">b",
        ),
    ] {
        for newline in ["\n", "\r\n", "\r"] {
            let source = input.replace('\n', newline);
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let heading = document.block_at(&[0, 0]).unwrap();
            assert_eq!(
                heading.kind,
                BlockKind::Heading {
                    level: 2,
                    title: title.into()
                }
            );
            assert_eq!(document.source(), source);
            for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                assert_eq!(
                    document.render_html(dialect, &mut governor).unwrap(),
                    format!("<blockquote>\n<h2>{body}</h2>\n</blockquote>\n"),
                    "{source:?}"
                );
            }
        }
    }
}
