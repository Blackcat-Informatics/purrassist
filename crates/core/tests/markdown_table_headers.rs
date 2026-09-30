// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Table-header promotion retains the admission of an open paragraph.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    markdown::{Block, BlockKind, Dialect, Document},
};
// Independent pinned cmark-GFM controls; table recognition changes paragraph
// content, while standalone indented code retains its block precedence.
const PROMOTED: &[(&str, &str, &str)] = &[
    (
        "a\n    b\n-|\n",
        "<p>a\nb\n-|</p>\n",
        "<p>a</p>\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n",
    ),
    (
        "a\n\tb\n-|\n",
        "<p>a\nb\n-|</p>\n",
        "<p>a</p>\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n",
    ),
    (
        "> a\n    b\n> -|\n]\n",
        "<blockquote>\n<p>a\nb\n-|\n]</p>\n</blockquote>\n",
        "<blockquote>\n<p>a</p>\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n</blockquote>\n<p>]</p>\n",
    ),
    (
        "> a\n>     b\n> -|\n]\n",
        "<blockquote>\n<p>a\nb\n-|\n]</p>\n</blockquote>\n",
        "<blockquote>\n<p>a</p>\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n</blockquote>\n<p>]</p>\n",
    ),
    (
        "> a\nb\n> -|\n]\n",
        "<blockquote>\n<p>a\nb\n-|\n]</p>\n</blockquote>\n",
        "<blockquote>\n<p>a</p>\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n</blockquote>\n<p>]</p>\n",
    ),
    (
        "> > a\n>\t b\n> > -|\n]\n",
        "<blockquote>\n<blockquote>\n<p>a\nb\n-|\n]</p>\n</blockquote>\n</blockquote>\n",
        "<blockquote>\n<blockquote>\n<p>a</p>\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n</blockquote>\n</blockquote>\n<p>]</p>\n",
    ),
    (
        "- a\n      b\n  -|\n]\n",
        "<ul>\n<li>a\nb\n-|\n]</li>\n</ul>\n",
        "<ul>\n<li>a\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n</li>\n</ul>\n<p>]</p>\n",
    ),
    (
        "> - a\n      b\n>   -|\n]\n",
        "<blockquote>\n<ul>\n<li>a\nb\n-|\n]</li>\n</ul>\n</blockquote>\n",
        "<blockquote>\n<ul>\n<li>a\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n</li>\n</ul>\n</blockquote>\n<p>]</p>\n",
    ),
    (
        "- a\n\t\tb\n  -|\n]\n",
        "<ul>\n<li>a\nb\n-|\n]</li>\n</ul>\n",
        "<ul>\n<li>a\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n</li>\n</ul>\n<p>]</p>\n",
    ),
    (
        "> a\n    b | c\n> | - | - |\n]\n",
        "<blockquote>\n<p>a\nb | c\n| - | - |\n]</p>\n</blockquote>\n",
        "<blockquote>\n<p>a</p>\n<table>\n<thead>\n<tr>\n<th>b</th>\n<th>c</th>\n</tr>\n</thead>\n</table>\n</blockquote>\n<p>]</p>\n",
    ),
    (
        "[> quo-td\notd!\n\t>-\n>-td!\n\t>-\n>-|\n]\n",
        "<p>[&gt; quo-td\notd!\n&gt;-</p>\n<blockquote>\n<p>-td!\n&gt;-\n-|\n]</p>\n</blockquote>\n",
        "<p>[&gt; quo-td\notd!\n&gt;-</p>\n<blockquote>\n<p>-td!</p>\n<table>\n<thead>\n<tr>\n<th>&gt;-</th>\n</tr>\n</thead>\n</table>\n</blockquote>\n<p>]</p>\n",
    ),
];
const BOUNDARIES: &[(&str, &str, &str)] = &[
    (
        "    b\n-|\n",
        "<pre><code>b\n</code></pre>\n<p>-|</p>\n",
        "<pre><code>b\n</code></pre>\n<p>-|</p>\n",
    ),
    (
        ">     b\n> -|\n]\n",
        "<blockquote>\n<pre><code>b\n</code></pre>\n<p>-|\n]</p>\n</blockquote>\n",
        "<blockquote>\n<pre><code>b\n</code></pre>\n<p>-|\n]</p>\n</blockquote>\n",
    ),
    (
        "a\n\n    b\n-|\n",
        "<p>a</p>\n<pre><code>b\n</code></pre>\n<p>-|</p>\n",
        "<p>a</p>\n<pre><code>b\n</code></pre>\n<p>-|</p>\n",
    ),
    (
        "> a\n    b\n-|\n]\n",
        "<blockquote>\n<p>a\nb\n-|\n]</p>\n</blockquote>\n",
        "<blockquote>\n<p>a\nb\n-|\n]</p>\n</blockquote>\n",
    ),
    (
        "a\n    b | c\n-|\n",
        "<p>a\nb | c\n-|</p>\n",
        "<p>a\nb | c\n-|</p>\n",
    ),
    (
        "> a\n    b\n> -|\n    c\n",
        "<blockquote>\n<p>a\nb\n-|\nc</p>\n</blockquote>\n",
        "<blockquote>\n<p>a</p>\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n</blockquote>\n<pre><code>c\n</code></pre>\n",
    ),
    (
        "> a\n    b\n> -|\n>     c\n",
        "<blockquote>\n<p>a\nb\n-|\nc</p>\n</blockquote>\n",
        "<blockquote>\n<p>a</p>\n<table>\n<thead>\n<tr>\n<th>b</th>\n</tr>\n</thead>\n</table>\n<pre><code>c\n</code></pre>\n</blockquote>\n",
    ),
];

#[test]
fn indented_headers_in_open_paragraphs_follow_container_context() {
    check_renderings(PROMOTED);
}
#[test]
fn standalone_code_lazy_delimiters_and_table_body_boundaries_stay_distinct() {
    check_renderings(BOUNDARIES);
}
fn check_renderings(cases: &[(&str, &str, &str)]) {
    for &(input, common, gfm) in cases {
        for newline in ["\n", "\r\n", "\r"] {
            let source = input.replace('\n', newline);
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            assert_eq!(document.source(), source);
            for (dialect, expected) in [(Dialect::CommonMark, common), (Dialect::Gfm, gfm)] {
                assert_eq!(
                    document.render_html(dialect, &mut governor).unwrap(),
                    expected,
                    "{dialect:?}: {source:?}"
                );
            }
        }
    }
}
fn table_path(blocks: &[Block]) -> Option<Vec<usize>> {
    for (index, block) in blocks.iter().enumerate() {
        if matches!(block.kind, BlockKind::Table { .. }) {
            return Some(vec![index]);
        }
        if let Some(mut path) = table_path(&block.children) {
            path.insert(0, index);
            return Some(path);
        }
    }
    None
}
#[test]
fn promoted_table_cell_edits_retain_indent_source_spans_and_exact_undo() {
    for &(input, _, _) in PROMOTED {
        for newline in ["\n", "\r\n", "\r"] {
            let source = input.replace('\n', newline);
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let path = table_path(document.blocks()).unwrap();
            let table = document.block_at(&path).unwrap();
            let BlockKind::Table { header, .. } = &table.kind else {
                panic!("table path")
            };
            assert_eq!(table.lines.len(), 2);
            let plan = document
                .replace_table_cell(&path, 0, 0, "iced", &mut governor)
                .unwrap();
            assert_eq!(plan.edits.len(), 1);
            let range = plan.edits[0].range.clone();
            assert_eq!(&source[range.clone()], header[0]);
            assert!(table.lines[0].start <= range.start && range.end <= table.lines[0].end);
            let expected = format!("{}iced{}", &source[..range.start], &source[range.end..]);
            let Outcome::Resolved(applied) = plan.apply(&source, Budget::default()).outcome else {
                panic!("cell edit must resolve")
            };
            assert_eq!(applied.text, expected);
            let Outcome::Resolved(undone) = applied
                .inverse
                .apply(&applied.text, Budget::default())
                .outcome
            else {
                panic!("inverse must resolve")
            };
            assert_eq!(undone.text, source);
            assert_eq!(
                plan.apply(&format!("{source}changed"), Budget::default())
                    .outcome,
                Outcome::Unresolved
            );
            assert_eq!(
                document
                    .replace_table_cell(
                        &path,
                        0,
                        0,
                        "iced",
                        &mut Governor::new(Budget {
                            work: 0,
                            ..Budget::default()
                        })
                    )
                    .unwrap_err(),
                StopReason::BudgetExceeded(Resource::Work)
            );
        }
    }
}
#[test]
fn contextual_table_admission_exhaustion_is_explicit() {
    let source = "> a\n    b\n> -|\n]\n";
    for (budget, resource) in [
        (
            Budget {
                work: 0,
                ..Budget::default()
            },
            Resource::Work,
        ),
        (
            Budget {
                scratch_bytes: 0,
                ..Budget::default()
            },
            Resource::ScratchBytes,
        ),
        (
            Budget {
                depth: 0,
                ..Budget::default()
            },
            Resource::Depth,
        ),
    ] {
        assert_eq!(
            Document::parse(source, &mut Governor::new(budget)).unwrap_err(),
            StopReason::BudgetExceeded(resource)
        );
    }
}
