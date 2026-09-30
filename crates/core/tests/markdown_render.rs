// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Full-document rendering qualification against both official corpora.
use purrassist_core::{
    Budget, Governor,
    json::{self, Value},
    markdown::{Dialect, Document, Extension, Extensions},
};

#[test]
fn lazy_setext_candidates_keep_the_whole_container_paragraph_open() {
    use purrassist_core::{StopReason, contract::Resource, markdown::BlockKind};

    // CommonMark setext examples 92–94 forbid a lazy underline. Its text and
    // every later continuation stay in one paragraph, even if a later line
    // restores the container marker. A marked underline still creates a heading.
    for newline in ["\n", "\r\n", "\r"] {
        for underline in ["--", "=", "==", " --\t"] {
            for (prefix, continuation, open, close, path) in [
                (
                    ">",
                    "",
                    "<blockquote>\n<p>",
                    "</p>\n</blockquote>\n",
                    vec![0, 0],
                ),
                (
                    "> ",
                    "> ",
                    "<blockquote>\n<p>",
                    "</p>\n</blockquote>\n",
                    vec![0, 0],
                ),
                ("- ", "", "<ul>\n<li>", "</li>\n</ul>\n", vec![0, 0]),
                (
                    ">> ",
                    "",
                    "<blockquote>\n<blockquote>\n<p>",
                    "</p>\n</blockquote>\n</blockquote>\n",
                    vec![0, 0, 0],
                ),
                (
                    "> - ",
                    "> ",
                    "<blockquote>\n<ul>\n<li>",
                    "</li>\n</ul>\n</blockquote>\n",
                    vec![0, 0, 0],
                ),
            ] {
                let source = format!("{prefix}: {newline}{underline}{newline}{continuation}|\"");
                let expected = format!("{open}:\n{}\n|&quot;{close}", underline.trim());
                for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                    let mut governor = Governor::new(Budget::default());
                    let document = Document::parse(&source, &mut governor).unwrap();
                    assert_eq!(document.source(), source);
                    assert_eq!(document.blocks()[0].span, 0..source.len());
                    let paragraph = document.block_at(&path).unwrap();
                    assert!(matches!(paragraph.kind, BlockKind::Paragraph));
                    assert_eq!(paragraph.lines.len(), 3);
                    assert_eq!(paragraph.span.end, source.len());
                    assert_eq!(
                        document.render_html(dialect, &mut governor).unwrap(),
                        expected,
                        "{dialect:?}: {source:?}"
                    );
                    assert_eq!(
                        Document::parse(
                            &source,
                            &mut Governor::new(Budget {
                                work: 0,
                                ..Budget::default()
                            })
                        )
                        .unwrap_err(),
                        StopReason::BudgetExceeded(Resource::Work)
                    );
                    assert_eq!(
                        Document::parse(
                            &source,
                            &mut Governor::new(Budget {
                                scratch_bytes: 0,
                                ..Budget::default()
                            })
                        )
                        .unwrap_err(),
                        StopReason::BudgetExceeded(Resource::ScratchBytes)
                    );
                }
            }
        }
    }
}

#[test]
fn lazy_setext_respects_ancestor_prefixes_and_real_block_boundaries() {
    for newline in ["\n", "\r\n", "\r"] {
        for (input, expected) in [
            (
                "> - :\n  ==\nend\n",
                "<blockquote>\n<ul>\n<li>:\n==\nend</li>\n</ul>\n</blockquote>\n",
            ),
            (
                "> - :\n  --\n  ==\nend\n",
                "<blockquote>\n<ul>\n<li>:\n--\n==\nend</li>\n</ul>\n</blockquote>\n",
            ),
            (
                "> :\nbefore\n==\nend\n",
                "<blockquote>\n<p>:\nbefore\n==\nend</p>\n</blockquote>\n",
            ),
            (
                "> :\n> --\nend\n",
                "<blockquote>\n<h2>:</h2>\n</blockquote>\n<p>end</p>\n",
            ),
            (
                "> :\n--\n> ==\nend\n",
                "<blockquote>\n<h1>:\n--</h1>\n</blockquote>\n<p>end</p>\n",
            ),
            (
                "> :\n---\nend\n",
                "<blockquote>\n<p>:</p>\n</blockquote>\n<hr />\n<p>end</p>\n",
            ),
            (
                "> :\n\n--\nend\n",
                "<blockquote>\n<p>:</p>\n</blockquote>\n<p>--\nend</p>\n",
            ),
        ] {
            let source = input.replace('\n', newline);
            for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert_eq!(
                    document.render_html(dialect, &mut governor).unwrap(),
                    expected,
                    "{dialect:?}: {source:?}"
                );
            }
        }
    }
}

#[test]
fn unmatched_backtick_run_does_not_hide_later_same_width_code_spans() {
    // GFM §6.3 requires each code span to end at the next equal-width run.
    // An unmatched earlier run is literal and cannot consume later spans.
    for newline in ["\n", "\r\n", "\r"] {
        for unmatched in ["`", "```", "````"] {
            for width in [2, 5] {
                let ticks = "`".repeat(width);
                let source = format!(
                    "[{unmatched}rust{newline}{ticks}one{ticks} c {ticks}two{ticks}{newline}"
                );
                let expected =
                    format!("<p>[{unmatched}rust\n<code>one</code> c <code>two</code></p>\n");
                for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                    let mut governor = Governor::new(Budget::default());
                    let document = Document::parse(&source, &mut governor).unwrap();
                    assert_eq!(document.source(), source);
                    assert_eq!(
                        document.render_html(dialect, &mut governor).unwrap(),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn multiline_setext_precedes_single_column_table_interruption() {
    for newline in ["\n", "\r\n"] {
        for first in ["before", "| a"] {
            for second in ["||", "|a|", "a | b"] {
                for (underline, level) in [("-", 2), ("---", 2), ("=", 1), ("===", 1)] {
                    for (initial, continuation, before, after) in [
                        ("", "", "", ""),
                        ("> ", "> ", "<blockquote>\n", "</blockquote>\n"),
                        ("- ", "  ", "<ul>\n<li>\n", "</li>\n</ul>\n"),
                    ] {
                        let source = format!(
                            "{initial}{first}{newline}{continuation}{second}{newline}{continuation}{underline}{newline}"
                        );
                        let expected =
                            format!("{before}<h{level}>{first}\n{second}</h{level}>\n{after}");
                        for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                            let mut governor = Governor::new(Budget::default());
                            let document = Document::parse(&source, &mut governor).unwrap();
                            assert_eq!(document.source(), source);
                            assert_eq!(
                                document.render_html(dialect, &mut governor).unwrap(),
                                expected,
                                "{dialect:?}: {source:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn autolink_domain_admission_precedes_url_suffix_and_delimiter_trimming() {
    for newline in ["\n", "\r\n"] {
        for (text, html, plain) in [
            (
                "www.[org",
                "<a href=\"http://www.%5Borg\">www.[org</a>",
                "www.[org",
            ),
            (
                "www.]org",
                "<a href=\"http://www.%5Dorg\">www.]org</a>",
                "www.]org",
            ),
            (
                "www.{org",
                "<a href=\"http://www.%7Borg\">www.{org</a>",
                "www.{org",
            ),
            (
                "www./org",
                "<a href=\"http://www./org\">www./org</a>",
                "www./org",
            ),
            (
                "www.?org",
                "<a href=\"http://www.?org\">www.?org</a>",
                "www.?org",
            ),
            ("www.", "www.", "www."),
            ("www._org", "www._org", "www._org"),
            ("www.a_", "<a href=\"http://www.a\">www.a</a>_", "www.a_"),
            (
                "www._a.org. next",
                "<a href=\"http://www._a.org\">www._a.org</a>. next",
                "www._a.org. next",
            ),
            ("www.a._org. next", "www.a._org. next", "www.a._org. next"),
            ("www.\\_org", "www._org", "www._org"),
            ("http://[org", "http://[org", "http://[org"),
            (
                "http://a_",
                "<a href=\"http://a\">http://a</a>_",
                "http://a_",
            ),
        ] {
            let source = format!("{text}{newline}");
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            assert_eq!(document.source(), source);
            assert_eq!(
                document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                format!("<p>{html}</p>\n"),
                "{source:?}"
            );
            assert_eq!(
                document
                    .render_html(Dialect::CommonMark, &mut governor)
                    .unwrap(),
                format!("<p>{plain}</p>\n"),
                "{source:?}"
            );
        }
    }
}

#[test]
fn task_markers_require_separating_whitespace_before_empty_or_nonempty_content() {
    use purrassist_core::markdown::BlockKind;
    for ending in ["", "\n", "\r\n", "\r"] {
        for marker in ["[ ]", "[x]", "[X]"] {
            for separator in ["", " ", "\t", "\u{b}", "\u{c}"] {
                let source = format!("- {marker}{separator}{ending}");
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert_eq!(document.source(), source);
                let BlockKind::ListItem { checked, .. } = document.blocks()[0].kind else {
                    panic!()
                };
                let expected_state = (!separator.is_empty()).then_some(marker != "[ ]");
                assert_eq!(checked, expected_state, "{source:?}");
                let expected = expected_state.map_or_else(|| format!("<ul>\n<li>{marker}</li>\n</ul>\n"), |checked| {
                    let state = if checked { " checked=\"\"" } else { "" };
                    let retained = if separator == "\u{b}" || separator == "\u{c}" {
                        separator
                    } else {
                        ""
                    };
                    format!(
                        "<ul>\n<li><input{state} disabled=\"\" type=\"checkbox\"> {retained}</li>\n</ul>\n"
                    )
                });
                assert_eq!(
                    document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                    expected,
                    "{source:?}"
                );
                assert_eq!(
                    document
                        .render_html(Dialect::CommonMark, &mut governor)
                        .unwrap(),
                    format!(
                        "<ul>\n<li>{marker}{}</li>\n</ul>\n",
                        if separator == "\u{b}" || separator == "\u{c}" {
                            separator
                        } else {
                            ""
                        }
                    ),
                    "{source:?}"
                );
                if expected_state.is_none() {
                    assert!(document.set_task(0, true).is_err());
                }
            }
        }
    }
    for (source, expected) in [
        (
            "- [ ] odo\n- [x]\n",
            "<ul>\n<li><input disabled=\"\" type=\"checkbox\"> odo</li>\n<li>[x]</li>\n</ul>\n",
        ),
        (
            "- [x]\n  continuation\n",
            "<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> continuation</li>\n</ul>\n",
        ),
        (
            "- [x] \n  continuation\n",
            "<ul>\n<li><input checked=\"\" disabled=\"\" type=\"checkbox\"> continuation</li>\n</ul>\n",
        ),
    ] {
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(source, &mut governor).unwrap();
        assert_eq!(
            document.render_html(Dialect::Gfm, &mut governor).unwrap(),
            expected
        );
    }
}

#[test]
fn single_column_tables_allow_pipe_free_headers_and_preserve_setext_precedence() {
    use purrassist_core::markdown::BlockKind;
    for newline in ["\n", "\r\n"] {
        for (delimiter, alignment) in [
            ("-|", ""),
            ("|-", ""),
            ("|:-:|", " align=\"center\""),
            (":-:", " align=\"center\""),
            ("-:", " align=\"right\""),
        ] {
            for before in [false, true] {
                for container in ["", "> ", "- "] {
                    let continuation = if container == "- " { "  " } else { container };
                    let source = if before {
                        format!(
                            "{container}before{newline}{continuation}head{newline}{continuation}{delimiter}{newline}"
                        )
                    } else {
                        format!("{container}head{newline}{continuation}{delimiter}{newline}")
                    };
                    let mut governor = Governor::new(Budget::default());
                    let document = Document::parse(&source, &mut governor).unwrap();
                    assert_eq!(document.source(), source);
                    let blocks = if container.is_empty() {
                        document.blocks()
                    } else {
                        &document.blocks()[0].children
                    };
                    assert!(
                        matches!(&blocks[usize::from(before)].kind, BlockKind::Table { header, rows, .. } if header == &["head"] && rows.is_empty())
                    );
                    let table = format!(
                        "<table>\n<thead>\n<tr>\n<th{alignment}>head</th>\n</tr>\n</thead>\n</table>\n"
                    );
                    let expected = if container == "- " {
                        format!(
                            "<ul>\n<li>{}{table}</li>\n</ul>\n",
                            if before { "before\n" } else { "\n" }
                        )
                    } else {
                        let body =
                            format!("{}{table}", if before { "<p>before</p>\n" } else { "" });
                        if container.is_empty() {
                            body
                        } else {
                            format!("<blockquote>\n{body}</blockquote>\n")
                        }
                    };
                    assert_eq!(
                        document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                        expected,
                        "{source:?}"
                    );
                    // Tables disabled: the same indexed source renders as an ordinary paragraph.
                    let plain = if before {
                        format!("before\nhead\n{delimiter}")
                    } else {
                        format!("head\n{delimiter}")
                    };
                    let expected = if container == "- " {
                        format!("<ul>\n<li>{plain}</li>\n</ul>\n")
                    } else if container.is_empty() {
                        format!("<p>{plain}</p>\n")
                    } else {
                        format!("<blockquote>\n<p>{plain}</p>\n</blockquote>\n")
                    };
                    assert_eq!(
                        document
                            .render_html(Dialect::CommonMark, &mut governor)
                            .unwrap(),
                        expected,
                        "{source:?}"
                    );
                }
            }
        }
        for underline in ["-", "---", "=", "==="] {
            let source = format!("before{newline}head{newline}{underline}{newline}");
            let level = if underline.starts_with('=') { 1 } else { 2 };
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                assert_eq!(
                    document.render_html(dialect, &mut governor).unwrap(),
                    format!("<h{level}>before\nhead</h{level}>\n")
                );
            }
        }
    }
}

#[test]
fn fuzz_regressions_preserve_autolink_boundaries_and_quote_containers() {
    for (source, expected) in [
        (
            "[\r\n| a | b |\n| - | - |\n| 1 | 2 |\n]",
            "<p>[</p>\n<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>1</td>\n<td>2</td>\n</tr>\n<tr>\n<td>]</td>\n<td></td>\n</tr>\n</tbody>\n</table>\n",
        ),
        (
            "[tea]: /tea \"\"\n\n1.[tea]",
            "<p>1.<a href=\"/tea\">tea</a></p>\n",
        ),
        (
            "> a <x\n a=v> b\n",
            "<blockquote>\n<p>a <x\n a=v> b</p>\n</blockquote>\n",
        ),
        ("- a <x\n a=v> b\n", "<ul>\n<li>a <x\n a=v> b</li>\n</ul>\n"),
        ("a <!--x---> b\n", "<p>a <!--x---> b</p>\n"),
        ("<!e>    \ne>\n]_", "<p>&lt;!e&gt;<br />\ne&gt;\n]_</p>\n"),
        ("a <!E>", "<p>a &lt;!E&gt;</p>\n"),
        ("a <!E >", "<p>a <!E ></p>\n"),
        ("a <!EAa >", "<p>a &lt;!EAa &gt;</p>\n"),
        (
            "~~strike~~ www.exa~~strike~~ www.example.org user@example.c:om\n",
            "<p><del>strike</del> <a href=\"http://www.exa~~strike\">www.exa~~strike</a>~~ <a href=\"http://www.example.org\">www.example.org</a> <a href=\"mailto:user@example.c\">user@example.c</a>:om</p>\n",
        ),
        (
            "[# Te`[# Te`\n Te`\n Te`\n]",
            "<p>[# Te<code>[# Te</code>\nTe<code> Te</code>\n]</p>\n",
        ),
        (
            "~~strike~~ www.[org user@exa\r\nmple.com\n",
            "<p><del>strike</del> <a href=\"http://www.%5Borg\">www.[org</a> user@exa\nmple.com</p>\n",
        ),
        (
            "~~strike~~ www.|mple.com\n",
            "<p><del>strike</del> <a href=\"http://www.%7Cmple.com\">www.|mple.com</a></p>\n",
        ),
        (
            "| a | b |\n| - | - |\n| 1 | 2 |\n |\n",
            "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>1</td>\n<td>2</td>\n</tr>\n</tbody>\n</table>\n<p>|</p>\n",
        ),
        (
            "[a [link](/url) www.example.org]",
            "<p>[a <a href=\"/url\">link</a> www.example.org]</p>\n",
        ),
        (
            "mailto:user@example.com",
            "<p><a href=\"mailto:user@example.com\">mailto:user@example.com</a></p>\n",
        ),
        (
            "xmpp:user@example.com/path",
            "<p><a href=\"xmpp:user@example.com/path\">xmpp:user@example.com/path</a></p>\n",
        ),
        ("user@exa..mple.com", "<p>user@exa..mple.com</p>\n"),
        (
            "user@example.comuser@example.com",
            "<p>user@<a href=\"mailto:example.comuser@example.com\">example.comuser@example.com</a></p>\n",
        ),
        ("[www.example.org]", "<p>[www.example.org]</p>\n"),
        (
            "[~~strike~~ www.example.org user@example.com\n]",
            "<p>[<del>strike</del> www.example.org <a href=\"mailto:user@example.com\">user@example.com</a>\n]</p>\n",
        ),
        (
            "> text\n- \n",
            "<blockquote>\n<p>text</p>\n</blockquote>\n<ul>\n<li></li>\n</ul>\n",
        ),
        (
            "> text\n2. outside\n",
            "<blockquote>\n<p>text</p>\n</blockquote>\n<ol start=\"2\">\n<li>outside</li>\n</ol>\n",
        ),
    ] {
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(source, &mut governor).unwrap();
        assert_eq!(document.source(), source);
        assert_eq!(
            document.render_html(Dialect::Gfm, &mut governor).unwrap(),
            expected,
            "{source:?}"
        );
    }
}

/// A GFM table interrupts a paragraph; without Tables the same source is one
/// paragraph. Both projections retain the original document and table address.
#[test]
fn generated_tables_interrupt_paragraphs_only_when_rendering_the_extension() {
    for newline in ["\n", "\r\n"] {
        for header in ["| a | b |", "a | b", "  | a | b |"] {
            let source =
                format!("before{newline}{header}{newline}| - | - |{newline}| 1 | 2 |{newline}");
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            assert_eq!(document.source(), source);
            assert!(matches!(
                document.blocks()[1].kind,
                purrassist_core::markdown::BlockKind::Table { .. }
            ));
            let expected = "<p>before</p>\n<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>1</td>\n<td>2</td>\n</tr>\n</tbody>\n</table>\n";
            assert_eq!(
                document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                expected,
                "{source:?}"
            );
            let commonmark = format!(
                "<p>before\n{}\n| - | - |\n| 1 | 2 |</p>\n",
                header.trim_start()
            );
            assert_eq!(
                document
                    .render_html(Dialect::CommonMark, &mut governor)
                    .unwrap(),
                commonmark,
                "{source:?}"
            );
            assert_eq!(
                document
                    .render_with_extensions(Dialect::Gfm, Extensions::default(), &mut governor)
                    .unwrap(),
                commonmark,
                "{source:?}"
            );
        }
    }
}

#[test]
fn html_block_ends_a_table_but_does_not_interrupt_a_commonmark_paragraph() {
    let source = "before\na|b\n|-|-|\n<x>\n# after\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    assert_eq!(document.source(), source);
    let table = "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n</table>\n";
    assert_eq!(
        document.render_html(Dialect::Gfm, &mut governor).unwrap(),
        format!("<p>before</p>\n{table}<x>\n# after\n")
    );
    let commonmark = "<p>before\na|b\n|-|-|\n<x></p>\n<h1>after</h1>\n";
    assert_eq!(
        document
            .render_html(Dialect::CommonMark, &mut governor)
            .unwrap(),
        commonmark
    );
    assert_eq!(
        document
            .render_with_extensions(Dialect::Gfm, Extensions::default(), &mut governor)
            .unwrap(),
        commonmark
    );
}

/// Tight-list grammar uses actual source blank lines. The pinned reference
/// extension incorrectly marks a header-only table as blank after its delimiter.
#[test]
fn header_only_tables_preserve_tight_lists_without_source_blank_lines() {
    for row in ["", "  | 1 | 2 |\n"] {
        let source = format!("- before\n  a|b\n  |-|-|\n{row}  |\n");
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(&source, &mut governor).unwrap();
        assert_eq!(document.source(), source);
        let body = if row.is_empty() {
            ""
        } else {
            "<tbody>\n<tr>\n<td>1</td>\n<td>2</td>\n</tr>\n</tbody>\n"
        };
        let expected = format!(
            "<ul>\n<li>before\n<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n{body}</table>\n|</li>\n</ul>\n"
        );
        assert_eq!(
            document.render_html(Dialect::Gfm, &mut governor).unwrap(),
            expected,
            "{source:?}"
        );
    }
}

/// Empty titles retain their parsed source but omit the HTML title attribute,
/// matching the reference renderer for both links and images.
#[test]
fn generated_link_titles_preserve_source_and_render_nonempty_values() {
    let mut cases = 0;
    for (open, close) in [('"', '"'), ('\'', '\''), ('(', ')')] {
        for title in ["", "x", " ", "&amp;"] {
            for image in [false, true] {
                for reference in [false, true] {
                    let prefix = if image { "!" } else { "" };
                    let title_source = format!("{open}{title}{close}");
                    let source = if reference {
                        format!("[tea]: /tea {title_source}\n\n{prefix}[tea]\n")
                    } else {
                        format!("{prefix}[tea](/tea {title_source})\n")
                    };
                    let mut governor = Governor::new(Budget::default());
                    let document = Document::parse(&source, &mut governor).unwrap();
                    assert_eq!(document.source(), source);
                    let attribute = if title.is_empty() {
                        String::new()
                    } else {
                        format!(" title=\"{title}\"")
                    };
                    let expected = if image {
                        format!("<p><img src=\"/tea\" alt=\"tea\"{attribute} /></p>\n")
                    } else {
                        format!("<p><a href=\"/tea\"{attribute}>tea</a></p>\n")
                    };
                    assert_eq!(
                        document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                        expected,
                        "{source:?}"
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 48);
}

/// `CommonMark` 0.31.2 §6.6 permits hyphens before a comment's closing marker.
/// Generate literal inline comment forms independently of cmark's old scanner.
#[test]
fn generated_html_comments_preserve_specified_literal_content() {
    let mut cases = 0;
    for body in ["", "x", "-", "--", "a--b", "\n", "\0"] {
        for close in ["-->", "--->", "---->"] {
            let comment = format!("<!--{body}{close}");
            for (prefix, before, after) in [
                ("", "<p>", "</p>\n"),
                ("> ", "<blockquote>\n<p>", "</p>\n</blockquote>\n"),
                ("- ", "<ul>\n<li>", "</li>\n</ul>\n"),
            ] {
                let source = format!("{prefix}a {comment} b\n");
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert_eq!(document.source(), source);
                let expected = format!("{before}a {} b{after}", comment.replace('\0', "\u{fffd}"));
                assert_eq!(
                    document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                    expected,
                    "{source:?}"
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 63);
}

/// Generate the GFM §5.2 list-composition and §5.3 task-marker rules, including
/// same-line container openings the cmark-gfm tasklist extension misses.
#[test]
fn generated_tasklist_rules_preserve_nested_containers_and_source() {
    let mut cases = 0;
    for depth in 1..=8 {
        for quote in [false, true] {
            for marker in [" ", "x", "X"] {
                for separator in [" ", "\t"] {
                    for ending in ["\n", "\r\n"] {
                        let prefix = if quote {
                            "> ".repeat(depth) + "- "
                        } else {
                            "- ".repeat(depth)
                        };
                        let source = format!("{prefix}[{marker}]{separator}done{ending}");
                        let checked = if marker == " " { "" } else { " checked=\"\"" };
                        let task = format!(
                            "<ul>\n<li><input{checked} disabled=\"\" type=\"checkbox\"> done</li>\n</ul>\n"
                        );
                        let expected = if quote {
                            "<blockquote>\n".repeat(depth)
                                + &task
                                + &"</blockquote>\n".repeat(depth)
                        } else {
                            "<ul>\n<li>\n".repeat(depth - 1)
                                + &task
                                + &"</li>\n</ul>\n".repeat(depth - 1)
                        };
                        let mut governor = Governor::new(Budget::default());
                        let document = Document::parse(&source, &mut governor).unwrap();
                        assert_eq!(document.source(), source);
                        assert_eq!(
                            document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                            expected,
                            "{source:?}"
                        );
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 192);
}

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
fn reference_prefix_removal_retains_open_paragraph_continuations() {
    // CommonMark's block phase removes reference prefixes when an open
    // paragraph closes. Removing a definition must not make its indented
    // continuation into a fresh code block (section 4.4), or let type-7 HTML
    // and noninterrupting ordered markers interrupt that paragraph.
    let cases = [
        ("[te.a]: \tTe-a\"\n    ]\n", "<p>]</p>\n"),
        ("[a]: /url\n\t[a]\n", "<p><a href=\"/url\">a</a></p>\n"),
        ("[a]: /url\n    # text\n", "<p># text</p>\n"),
        ("[a]: /url\n    x\n---\n", "<h2>x</h2>\n"),
        (
            "[a]: /url\n<span>text</span>\n",
            "<p><span>text</span></p>\n",
        ),
        ("[a]: /url\n2. text\nnext\n", "<p>2. text\nnext</p>\n"),
        ("[a]: /url\n---\nnext\n", "<p>---\nnext</p>\n"),
        ("[a]: /url\n===\nnext\n", "<p>===\nnext</p>\n"),
        ("[a]: /url\n***\n", "<hr />\n"),
        ("[a]: /url\n# text\n", "<h1>text</h1>\n"),
        (
            "[a]: /url\n\n    code\n",
            "<pre><code>code\n</code></pre>\n",
        ),
        (
            "[a]: /url\n[b]: /b\n    [a] [b]\n",
            "<p><a href=\"/url\">a</a> <a href=\"/b\">b</a></p>\n",
        ),
        (
            "> [a]: /url\n>     [a]\n",
            "<blockquote>\n<p><a href=\"/url\">a</a></p>\n</blockquote>\n",
        ),
        (
            "- [a]: /url\n      [a]\n",
            "<ul>\n<li><a href=\"/url\">a</a></li>\n</ul>\n",
        ),
    ];
    for (input, expected) in cases {
        for newline in ["\n", "\r\n", "\r"] {
            let source = input.replace('\n', newline);
            for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert_eq!(document.source(), source);
                assert_eq!(
                    document.render_html(dialect, &mut governor).unwrap(),
                    expected,
                    "{dialect:?}: {source:?}"
                );
            }
        }
    }
}

#[test]
fn reference_definitions_cannot_absorb_interrupting_blocks() {
    let cases = [
        (
            "[teatea]:\n>\n",
            "<p>[teatea]:</p>\n<blockquote>\n</blockquote>\n",
        ),
        (
            "[a]:\n> /url\n",
            "<p>[a]:</p>\n<blockquote>\n<p>/url</p>\n</blockquote>\n",
        ),
        (
            "[a]:\n- /url\n",
            "<p>[a]:</p>\n<ul>\n<li>/url</li>\n</ul>\n",
        ),
        ("[a]:\n1. url\n", "<p>[a]:</p>\n<ol>\n<li>url</li>\n</ol>\n"),
        ("[a]:\n2. url\n", "<p>[a]:\n2. url</p>\n"),
        ("[a]:\n# h\n", "<p>[a]:</p>\n<h1>h</h1>\n"),
        ("[a]:\n---\n", "<h2>[a]:</h2>\n"),
        ("[a]:\n***\n", "<p>[a]:</p>\n<hr />\n"),
        ("[a]:\n```\n", "<p>[a]:</p>\n<pre><code></code></pre>\n"),
        ("[a]:\n<div>\n", "<p>[a]:</p>\n<div>\n"),
        ("[a]:\n#foo\n\n[a]\n", "<p><a href=\"#foo\">a</a></p>\n"),
        ("[a]:\n    /url\n\n[a]\n", "<p><a href=\"/url\">a</a></p>\n"),
        ("[a]:\n<span>\n\n[a]\n", "<p><a href=\"span\">a</a></p>\n"),
        (
            "[a]: /url\n\"a\n> text\nb\"\n\n[a]\n",
            "<p>&quot;a</p>\n<blockquote>\n<p>text\nb&quot;</p>\n</blockquote>\n<p><a href=\"/url\">a</a></p>\n",
        ),
        (
            "[a]: /url\n\"a\n    text\nb\"\n\n[a]\n",
            "<p><a href=\"/url\" title=\"a\ntext\nb\">a</a></p>\n",
        ),
    ];
    for (input, expected) in cases {
        for newline in ["\n", "\r\n", "\r"] {
            let source = input.replace('\n', newline);
            for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert_eq!(document.source(), source);
                assert_eq!(
                    document.render_html(dialect, &mut governor).unwrap(),
                    expected,
                    "{dialect:?}: {source:?}"
                );
            }
        }
    }
    let source = "[a]: /url\n| - |\n";
    let mut governor = Governor::new(Budget::default());
    let document = Document::parse(source, &mut governor).unwrap();
    assert_eq!(
        document.render_html(Dialect::Gfm, &mut governor).unwrap(),
        "<table>\n<thead>\n<tr>\n<th>[a]: /url</th>\n</tr>\n</thead>\n</table>\n"
    );
    assert_eq!(
        document
            .render_html(Dialect::CommonMark, &mut governor)
            .unwrap(),
        "<p>| - |</p>\n"
    );
    for newline in ["\n", "\r\n", "\r"] {
        let source = "[x]:\n-|\n\n[x]\n".replace('\n', newline);
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(&source, &mut governor).unwrap();
        assert_eq!(
            document
                .render_html(Dialect::CommonMark, &mut governor)
                .unwrap(),
            "<p><a href=\"-%7C\">x</a></p>\n"
        );
        assert_eq!(
            document.render_html(Dialect::Gfm, &mut governor).unwrap(),
            "<table>\n<thead>\n<tr>\n<th>[x]:</th>\n</tr>\n</thead>\n</table>\n<p>[x]</p>\n"
        );
    }
}

#[test]
fn lazy_container_lines_cannot_open_or_extend_tables() {
    // CommonMark laziness admits paragraph continuation only. A GFM table
    // therefore needs marked/indented delimiter and body lines at every
    // enclosing container level. Ordinary paragraphs still allow them.
    let heading = "| a | b |";
    let delimiter = "| - | - |";
    let row = "| 1 | 2 |";
    let table_head = "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n";
    let table_body = "<tbody>\n<tr>\n<td>1</td>\n<td>2</td>\n</tr>\n</tbody>\n";
    for container in ["quote", "list", "nested"] {
        let (first, marked, unmarked) = match container {
            "quote" => ("> ", "> ", ""),
            "list" => ("- ", "  ", ""),
            _ => ("> - ", ">   ", "> "),
        };
        for marked_delimiter in [false, true] {
            for marked_body in [false, true] {
                let input = format!(
                    "{first}{heading}\n{}{delimiter}\n{}{row}\n",
                    if marked_delimiter { marked } else { unmarked },
                    if marked_body { marked } else { unmarked }
                );
                for newline in ["\n", "\r\n", "\r"] {
                    let source = input.replace('\n', newline);
                    for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                        let table = dialect == Dialect::Gfm && marked_delimiter;
                        let inner = if table {
                            format!(
                                "{table_head}{}</table>\n",
                                if marked_body { table_body } else { "" }
                            )
                        } else {
                            format!("{heading}\n{delimiter}\n{row}")
                        };
                        let list = if table {
                            format!("<ul>\n<li>\n{inner}</li>\n</ul>\n")
                        } else {
                            format!("<ul>\n<li>{inner}</li>\n</ul>\n")
                        };
                        let outside = if table && !marked_body {
                            format!("<p>{row}</p>\n")
                        } else {
                            String::new()
                        };
                        let expected = match container {
                            "quote" => format!(
                                "<blockquote>\n{}</blockquote>\n{outside}",
                                if table {
                                    inner
                                } else {
                                    format!("<p>{inner}</p>\n")
                                }
                            ),
                            "list" => format!("{list}{outside}"),
                            _ => format!("<blockquote>\n{list}{outside}</blockquote>\n"),
                        };
                        let mut governor = Governor::new(Budget::default());
                        let document = Document::parse(&source, &mut governor).unwrap();
                        assert_eq!(document.source(), source);
                        assert_eq!(
                            document.render_html(dialect, &mut governor).unwrap(),
                            expected,
                            "{dialect:?}: {source:?}"
                        );
                    }
                }
            }
        }
    }
    // Text admitted as a lazy paragraph line can become a header when a
    // subsequent delimiter carries the quote marker. A lazy delimiter itself
    // is not a failed explicit table attempt that forbids later tables.
    for source in [
        "> lead\n| a | b |\n> | - | - |\n> | 1 | 2 |\n",
        "> | a | b |\n| - | - |\n> | a | b |\n> | - | - |\n",
    ] {
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(source, &mut governor).unwrap();
        let expected = if source.starts_with("> lead") {
            format!("<blockquote>\n<p>lead</p>\n{table_head}{table_body}</table>\n</blockquote>\n")
        } else {
            format!(
                "<blockquote>\n<p>{heading}\n{delimiter}</p>\n{table_head}</table>\n</blockquote>\n"
            )
        };
        assert_eq!(
            document.render_html(Dialect::Gfm, &mut governor).unwrap(),
            expected
        );
    }
}

#[test]
fn partially_consumed_tabs_preserve_code_and_task_paragraph_semantics() {
    use purrassist_core::Outcome;
    for newline in ["\n", "\r\n", "\r"] {
        for (input, before, after, task_path) in [
            (
                "> - [ ] `first\n>\t    next\n>   end`\n",
                "<blockquote>\n<ul>\n<li>[ ] <code>first next end</code></li>\n</ul>\n</blockquote>\n",
                "<blockquote>\n<ul>\n<li><input disabled=\"\" type=\"checkbox\"> <code>first next end</code></li>\n</ul>\n</blockquote>\n",
                [0, 0],
            ),
            (
                "- - [ ] `first\n\t  next\n\t  end`\n",
                "<ul>\n<li>\n<ul>\n<li>[ ] <code>first next end</code></li>\n</ul>\n</li>\n</ul>\n",
                "<ul>\n<li>\n<ul>\n<li><input disabled=\"\" type=\"checkbox\"> <code>first next end</code></li>\n</ul>\n</li>\n</ul>\n",
                [0, 0],
            ),
        ] {
            let source = input.replace('\n', newline);
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            assert_eq!(document.source(), source);
            assert_eq!(
                document
                    .render_html(Dialect::CommonMark, &mut governor)
                    .unwrap(),
                before
            );
            assert_eq!(
                document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                after
            );
            let plan = document
                .set_task_at(&task_path, true, &mut governor)
                .unwrap();
            let Outcome::Resolved(applied) = plan.apply(&source, Budget::default()).outcome else {
                panic!("task apply")
            };
            assert_eq!(applied.text, source.replacen("[ ]", "[x]", 1));
            let Outcome::Resolved(undone) = applied
                .inverse
                .apply(&applied.text, Budget::default())
                .outcome
            else {
                panic!("task undo")
            };
            assert_eq!(undone.text, source);
            let updated = Document::parse(&applied.text, &mut governor).unwrap();
            assert_eq!(
                updated.render_html(Dialect::Gfm, &mut governor).unwrap(),
                after.replace("<input disabled", "<input checked=\"\" disabled")
            );
        }
        // A genuinely lazy line has the same original offset AND padding as
        // its parent and still retains meaningful indentation inside code.
        let source = "> `first\n    next\n> end`\n".replace('\n', newline);
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(&source, &mut governor).unwrap();
        for dialect in [Dialect::CommonMark, Dialect::Gfm] {
            assert_eq!(
                document.render_html(dialect, &mut governor).unwrap(),
                "<blockquote>\n<p><code>first     next end</code></p>\n</blockquote>\n"
            );
        }
    }
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

#[test]
fn table_admission_preserves_failed_delimiters_and_list_boundaries() {
    for (text, expected) in [
        (
            "before\n|\nhead\n|-|\n",
            "<p>before\n|</p>\n<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n",
        ),
        (
            "before\n|||\nhead\n|-|\n",
            "<p>before\n|||</p>\n<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n",
        ),
        ("head\n    |-|\n", "<p>head\n|-|</p>\n"),
        (
            "before\n    |-|\nhead\n|-|\n",
            "<p>before\n|-|</p>\n<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n",
        ),
        (
            "a|b\n    |-|\nhead\n|-|\n",
            "<p>a|b\n|-|</p>\n<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n",
        ),
        (
            "before\n    :-:\nhead\n|-|\n",
            "<p>before\n:-:</p>\n<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n",
        ),
        (
            "[| a | b |\n| -\n | - |\n| 1 | 2 |\n |\n]",
            "<p>[| a | b |\n| -\n| - |\n| 1 | 2 |\n|\n]</p>\n",
        ),
        (
            "[| a | b |\n|| - | - |\n|\n| - | - |\n- | - |\n| 1 | 2 |\n]",
            "<p>[| a | b |\n|| - | - |\n|\n| - | - |</p>\n<ul>\n<li>| - |\n| 1 | 2 |\n]</li>\n</ul>\n",
        ),
        (
            "| a | b |\n| - |\n - |\n|\n| - | - |\n| 1 | #",
            "<p>| a | b |\n| - |</p>\n<ul>\n<li>|\n|\n| - | - |\n| 1 | #</li>\n</ul>\n",
        ),
        (
            "a | b\n- | - |\n",
            "<p>a | b</p>\n<ul>\n<li>| - |</li>\n</ul>\n",
        ),
        (
            "before\n| a | b |\n- | - |\n",
            "<p>before\n| a | b |</p>\n<ul>\n<li>| - |</li>\n</ul>\n",
        ),
        (
            "a | b\n - | - |\n",
            "<p>a | b</p>\n<ul>\n<li>| - |</li>\n</ul>\n",
        ),
        (
            "a | b\n|-|-|\n",
            "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n</table>\n",
        ),
        (
            "before\n|a|b|\n||-|-|\n| a | b |\n|-|-|\n",
            "<p>before\n|a|b|\n||-|-|</p>\n<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n</table>\n",
        ),
        ("a|b\n|-|\nhead\n|-|\n", "<p>a|b\n|-|\nhead\n|-|</p>\n"),
        (
            "a|b\n|-|\n\nhead\n|-|\n",
            "<p>a|b\n|-|</p>\n<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n",
        ),
    ] {
        for newline in ["\n", "\r\n"] {
            let source = text.replace('\n', newline);
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            assert_eq!(document.source(), source);
            assert_eq!(
                document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                expected,
                "{source:?}"
            );
        }
    }
}

#[test]
fn generated_hardbreaks_ignore_physical_indent_but_preserve_semantic_whitespace() {
    for newline in ["\n", "\r\n", "\r"] {
        for spelling in ["\\", "  "] {
            for (prefix, continuation, before, after) in [
                ("", " ", "<p>", "</p>\n"),
                ("- ", " ", "<ul>\n<li>", "</li>\n</ul>\n"),
                (
                    "> - ",
                    ">  ",
                    "<blockquote>\n<ul>\n<li>",
                    "</li>\n</ul>\n</blockquote>\n",
                ),
                (
                    "- - ",
                    "   ",
                    "<ul>\n<li>\n<ul>\n<li>",
                    "</li>\n</ul>\n</li>\n</ul>\n",
                ),
            ] {
                let source = format!("{prefix}a{spelling}{newline}{continuation}b{newline}");
                let expected = format!("{before}a<br />\nb{after}");
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert_eq!(document.source(), source);
                for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                    assert_eq!(
                        document.render_html(dialect, &mut governor).unwrap(),
                        expected,
                        "{dialect:?}: {source:?}"
                    );
                }
            }
        }
    }
    for (source, expected) in [
        ("a\\\n &#32;b\n", "<p>a<br>\n b</p>\n"),
        ("- a\\\n &#32;b\n", "<ul>\n<li>a<br>\n b</li>\n</ul>\n"),
        ("- a\\\n &#9;b\n", "<ul>\n<li>a<br>\n\tb</li>\n</ul>\n"),
        (
            "- `a\\\n b`\n",
            "<ul>\n<li><code>a\\  b</code></li>\n</ul>\n",
        ),
        ("a<br>\n b\n", "<p>a<br>\nb</p>\n"),
    ] {
        let mut governor = Governor::new(Budget::default());
        let document = Document::parse(source, &mut governor).unwrap();
        for dialect in [Dialect::CommonMark, Dialect::Gfm] {
            let expected = if source.contains('<') {
                expected.into()
            } else {
                expected.replace("<br>\n", "<br />\n")
            };
            assert_eq!(
                document.render_html(dialect, &mut governor).unwrap(),
                expected,
                "{dialect:?}: {source:?}"
            );
        }
    }
}

#[test]
fn table_body_ends_before_every_list_item_and_indented_code() {
    for (text, expected) in [
        (
            "[[[| >- | - |\n|> 2 |\n| - \n - \n |\n]]]",
            "<p>[[[| &gt;- | - |</p>\n<table>\n<thead>\n<tr>\n<th>&gt; 2</th>\n</tr>\n</thead>\n</table>\n<ul>\n<li></li>\n</ul>\n<p>|\n]]]</p>\n",
        ),
        (
            "head\n|-|\n- \n",
            "<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n<ul>\n<li></li>\n</ul>\n",
        ),
        (
            "head\n|-|\n2. tea\n",
            "<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n<ol start=\"2\">\n<li>tea</li>\n</ol>\n",
        ),
        (
            "head\n|-|\n1.\n",
            "<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n<ol>\n<li></li>\n</ol>\n",
        ),
        (
            "head\n|-|\n+\n",
            "<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n<ul>\n<li></li>\n</ul>\n",
        ),
        (
            "head\n|-|\n*\n",
            "<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n<ul>\n<li></li>\n</ul>\n",
        ),
        (
            "head\n|-|\n    code\n",
            "<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n<pre><code>code\n</code></pre>\n",
        ),
        (
            "head\n|-|\n    | row |\n",
            "<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n<pre><code>| row |\n</code></pre>\n",
        ),
        (
            "head\n|-|\n  2. tea\n",
            "<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n</table>\n<ol start=\"2\">\n<li>tea</li>\n</ol>\n",
        ),
        (
            "head\n|-|\nplain body\n",
            "<table>\n<thead>\n<tr>\n<th>head</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td>plain body</td>\n</tr>\n</tbody>\n</table>\n",
        ),
    ] {
        for newline in ["\n", "\r\n"] {
            let source = text.replace('\n', newline);
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            assert_eq!(document.source(), source);
            assert_eq!(
                document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                expected,
                "{source:?}"
            );
        }
    }
}

#[test]
fn email_retries_respect_previously_examined_domain_boundaries() {
    for (text, expected) in [
        (
            "[~~strike~~ www.example.orgple.org user@example.@example.com\n]",
            "<p>[<del>strike</del> www.example.orgple.org user@example<a href=\"mailto:.@example.com\">.@example.com</a>\n]</p>\n",
        ),
        (
            "a@b@c.com",
            "<p>a@<a href=\"mailto:b@c.com\">b@c.com</a></p>\n",
        ),
        (
            "a@b.@c.com",
            "<p>a@b<a href=\"mailto:.@c.com\">.@c.com</a></p>\n",
        ),
        (
            "a@b.c@d.com",
            "<p>a@<a href=\"mailto:b.c@d.com\">b.c@d.com</a></p>\n",
        ),
        (
            "a@b..c@d.com",
            "<p>a@b<a href=\"mailto:..c@d.com\">..c@d.com</a></p>\n",
        ),
        (
            "a@b.c!d@e.com",
            "<p><a href=\"mailto:a@b.c\">a@b.c</a>!<a href=\"mailto:d@e.com\">d@e.com</a></p>\n",
        ),
        (
            "a@b.@c.@d.com",
            "<p>a@b.@c<a href=\"mailto:.@d.com\">.@d.com</a></p>\n",
        ),
        (
            "a@b..c..@d.com",
            "<p>a@b<a href=\"mailto:..c..@d.com\">..c..@d.com</a></p>\n",
        ),
        (
            "mailto:a@b.@c.com",
            "<p>mailto:a@b<a href=\"mailto:.@c.com\">.@c.com</a></p>\n",
        ),
        (
            "xmpp:a@b.@c.com",
            "<p>xmpp:a@b<a href=\"mailto:.@c.com\">.@c.com</a></p>\n",
        ),
        (
            "a@b.@c.com tail",
            "<p>a@b<a href=\"mailto:.@c.com\">.@c.com</a> tail</p>\n",
        ),
        (
            "a@b.@c.com+d@e.org",
            "<p>a@b<a href=\"mailto:.@c.com\">.@c.com</a><a href=\"mailto:+d@e.org\">+d@e.org</a></p>\n",
        ),
        (
            "word .@c.com",
            "<p>word <a href=\"mailto:.@c.com\">.@c.com</a></p>\n",
        ),
        (
            "a@b_@c.com",
            "<p>a@<a href=\"mailto:b_@c.com\">b_@c.com</a></p>\n",
        ),
        (
            "a@b-@c.com",
            "<p>a@<a href=\"mailto:b-@c.com\">b-@c.com</a></p>\n",
        ),
        (
            "a@b.@c123.com",
            "<p>a@b<a href=\"mailto:.@c123.com\">.@c123.com</a></p>\n",
        ),
    ] {
        for newline in ["\n", "\r\n"] {
            let source = text.replace('\n', newline);
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            assert_eq!(document.source(), source);
            assert_eq!(
                document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                expected,
                "{source:?}"
            );
        }
    }
}

#[test]
fn task_checked_state_comes_only_from_first_marker_and_keeps_literal_content() {
    for newline in ["\n", "\r\n"] {
        for separator in [" ", "\t", "\u{b}", "\u{c}"] {
            for later in ["[x]", "[X]", "- [ ( [x][", "~- [x] done"] {
                let source = format!("- [ ]{separator}{later}{newline}");
                let retained = if matches!(separator, "\u{b}" | "\u{c}") {
                    separator
                } else {
                    ""
                };
                let expected = format!(
                    "<ul>\n<li><input disabled=\"\" type=\"checkbox\"> {retained}{later}</li>\n</ul>\n"
                );
                let mut governor = Governor::new(Budget::default());
                let document = Document::parse(&source, &mut governor).unwrap();
                assert_eq!(document.source(), source);
                assert_eq!(
                    document.render_html(Dialect::Gfm, &mut governor).unwrap(),
                    expected,
                    "{source:?}"
                );
            }
        }
    }
}
