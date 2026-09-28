// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Official link/image sections using explicit paragraph-level definition collection.
use purrassist_core::{
    Budget, Governor,
    json::{self, Value},
    markdown::{Dialect, InlineFragment, References},
};

#[test]
fn official_link_and_image_sections() {
    let mut failures = Vec::new();
    let mut count = 0;
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
            let section = case.get("section").unwrap().as_str().unwrap();
            let number: usize = case.get("example").unwrap().to_json().parse().unwrap();
            if section == "Link reference definitions" {
                let normalized = number + if dialect == Dialect::Gfm { 31 } else { 0 };
                // These five cases need code blocks, headings, or blockquotes.
                if [211, 212, 214, 215, 218].contains(&normalized) {
                    continue;
                }
            } else if !matches!(section, "Links" | "Images") {
                continue;
            }
            let source = case.get("markdown").unwrap().as_str().unwrap();
            let expected = case.get("html").unwrap().as_str().unwrap();
            let blocks: Vec<_> = source.split("\n\n").collect();
            let mut governor = Governor::new(Budget::default());
            let references = References::parse_blocks(&blocks, &mut governor).unwrap();
            let mut actual = String::new();
            for (block, consumed) in blocks.iter().zip(references.consumed()) {
                if *consumed == block.len() {
                    continue;
                }
                let fragment = InlineFragment::parse_with_references(
                    &block[*consumed..],
                    dialect,
                    &references,
                    &mut governor,
                )
                .unwrap();
                actual.push_str("<p>");
                actual.push_str(&fragment.render_html(&mut governor).unwrap());
                actual.push_str("</p>\n");
            }
            if actual != expected {
                failures.push(format!(
                    "{dialect:?} {}: {source:?}; expected {expected:?}, got {actual:?}",
                    case.get("example").unwrap().to_json()
                ));
            }
            count += 1;
        }
    }
    assert_eq!(count, 266);
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn references_retain_duplicate_sources_and_exact_link_origins() {
    use purrassist_core::{digest::Digest, markdown::InlineKind};
    let first = "[Straße]: /first 'café'\r\n[STRASSE]: /second\r\ntail";
    let mut governor = Governor::new(Budget::default());
    let refs = References::parse_blocks(&[first, "[other]: /third\n"], &mut governor).unwrap();
    assert_eq!(
        refs.sources(),
        &[first.to_owned(), "[other]: /third\n".to_owned()]
    );
    assert_eq!(refs.definitions().len(), 3);
    assert_eq!(&first[refs.consumed()[0]..], "tail");
    let definition = &refs.definitions()[0];
    assert_eq!(definition.origin.source, Digest::of(first.as_bytes()));
    assert_eq!(
        &first[definition.origin.span.clone()],
        "[Straße]: /first 'café'\r\n"
    );
    let source = "[STRASSE][]";
    let fragment =
        InlineFragment::parse_with_references(source, Dialect::CommonMark, &refs, &mut governor)
            .unwrap();
    assert_eq!(fragment.tokens()[0].span, 0..source.len());
    let InlineKind::Link {
        reference,
        children,
        ..
    } = &fragment.tokens()[0].kind
    else {
        panic!("reference link")
    };
    assert_eq!(reference.as_ref(), Some(&definition.origin));
    assert_eq!(&source[children[0].span.clone()], "STRASSE");
    assert_eq!(
        fragment.render_html(&mut governor).unwrap(),
        "<a href=\"/first\" title=\"café\">STRASSE</a>"
    );
}

#[test]
fn labels_are_bounded_and_do_not_decode_escapes_or_entities() {
    let mut governor = Governor::new(Budget::default());
    let long = "界".repeat(999);
    let definition = format!("[{long}]: /yes\n");
    let too_long = format!("[{long}界]: /no\n");
    let refs = References::parse_blocks(
        &[
            &definition,
            &too_long,
            "[&amp;]: /entity\n[a\\!]: /escape\n[a\u{a0}b]: /space\n",
        ],
        &mut governor,
    )
    .unwrap();
    assert_eq!(refs.consumed()[1], 0);
    for (source, expected) in [
        (format!("[{long}]"), format!("<a href=\"/yes\">{long}</a>")),
        (
            "[&amp;] [&]".into(),
            "<a href=\"/entity\">&amp;</a> [&amp;]".into(),
        ),
        (
            "[a\\!] [a!]".into(),
            "<a href=\"/escape\">a!</a> [a!]".into(),
        ),
        ("[A \t B]".into(), "<a href=\"/space\">A \t B</a>".into()),
    ] {
        let fragment = InlineFragment::parse_with_references(
            &source,
            Dialect::CommonMark,
            &refs,
            &mut governor,
        )
        .unwrap();
        assert_eq!(fragment.render_html(&mut governor).unwrap(), expected);
    }
}

#[test]
fn collection_exhaustion_and_cancel_are_not_partial_success() {
    use purrassist_core::StopReason;
    let budget = Budget {
        work: 0,
        ..Budget::default()
    };
    let mut governor = Governor::new(budget);
    assert!(matches!(
        References::parse_blocks(&["[a]: /a\n"], &mut governor),
        Err(StopReason::BudgetExceeded(
            purrassist_core::contract::Resource::Work
        ))
    ));
    let mut governor = Governor::new(Budget::default());
    governor.cancel();
    assert!(matches!(
        References::parse_blocks(&["[a]: /a\n"], &mut governor),
        Err(StopReason::Cancelled)
    ));
}
