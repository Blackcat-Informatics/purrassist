// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Reference field spans, duplicate preservation, semantic encoding, and exact undo.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    edits::EditPlan,
    markdown::{Dialect, Document, References},
};

fn apply(plan: &EditPlan, source: &str) -> String {
    let Outcome::Resolved(value) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply")
    };
    let Outcome::Resolved(undone) = value.inverse.apply(&value.text, Budget::default()).outcome
    else {
        panic!("undo")
    };
    assert_eq!(undone.text, source);
    assert_eq!(
        plan.apply(&format!("{source}changed"), Budget::default())
            .outcome,
        Outcome::Unresolved
    );
    value.text
}

#[test]
fn multiline_titles_remove_raw_indentation_and_retain_encoded_whitespace_and_spans() {
    for newline in ["\n", "\r\n", "\r"] {
        for indent in ["", " ", "    ", "\t", "\t  "] {
            let source = format!(
                "[a]: /url \" first{newline}{indent}&#32;&#9;text{newline}{indent}b  \"{newline}{newline}[a]{newline}"
            );
            let mut governor = Governor::new(Budget::default());
            let refs = References::parse_blocks(&[&source], &mut governor).unwrap();
            let definition = &refs.definitions()[0];
            assert_eq!(definition.title.as_deref(), Some(" first\n \ttext\nb  "));
            assert_eq!(
                &source[definition.title_span.clone().unwrap()],
                format!("\" first{newline}{indent}&#32;&#9;text{newline}{indent}b  \"")
            );
            assert_eq!(refs.sources(), std::slice::from_ref(&source));
            let document = Document::parse(&source, &mut governor).unwrap();
            let sites = document.reference_sites(&mut governor).unwrap();
            assert_eq!(sites[0].title, definition.title);
            assert_eq!(
                &source[sites[0].title_span.clone().unwrap()],
                &source[definition.title_span.clone().unwrap()]
            );
            assert_eq!(
                document
                    .replace_reference(
                        &sites[0].path,
                        0,
                        "/url",
                        definition.title.as_deref(),
                        &mut governor
                    )
                    .unwrap()
                    .edits,
                []
            );
            for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                assert_eq!(
                    document.render_html(dialect, &mut governor).unwrap(),
                    "<p><a href=\"/url\" title=\" first\n \ttext\nb  \">a</a></p>\n"
                );
            }
            let changed = document
                .replace_reference(&sites[0].path, 0, "/cold", Some("new title"), &mut governor)
                .unwrap();
            assert!(apply(&changed, &source).contains("/cold"));
        }
    }
}

#[test]
fn lazy_reference_titles_keep_indentation_and_replacements_preserve_reference_semantics() {
    for newline in ["\n", "\r\n", "\r"] {
        for (middle, closing, expected_title) in [
            ("    next", "end", "first\n    next\nend"),
            (">     next", "> end", "first\nnext\nend"),
            ("> &#10;    next", "> end", "first\n\n    next\nend"),
        ] {
            let source = format!(
                "> [a]: /url \"first{newline}{middle}{newline}{closing}\"{newline}{newline}[a]{newline}"
            );
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let sites = document.reference_sites(&mut governor).unwrap();
            assert_eq!(sites[0].title.as_deref(), Some(expected_title));
            for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                assert_eq!(
                    document.render_html(dialect, &mut governor).unwrap(),
                    format!(
                        "<blockquote>\n</blockquote>\n<p><a href=\"/url\" title=\"{expected_title}\">a</a></p>\n"
                    )
                );
            }
            assert_eq!(
                document
                    .replace_reference(
                        &sites[0].path,
                        0,
                        "/url",
                        Some(expected_title),
                        &mut governor
                    )
                    .unwrap()
                    .edits,
                []
            );
            let plan = document
                .remove_reference(&sites[0].path, 0, &mut governor)
                .unwrap();
            let changed = apply(&plan, &source);
            assert!(changed.starts_with("> "));
            let updated = Document::parse(&changed, &mut governor).unwrap();
            assert_eq!(
                updated
                    .render_html(Dialect::CommonMark, &mut governor)
                    .unwrap(),
                format!(
                    "<blockquote>\n</blockquote>\n<p><a href=\"/url\" title=\"{expected_title}\">a</a></p>\n"
                )
            );
        }
    }
}

#[test]
fn partially_consumed_tabs_are_explicit_reference_continuation_indentation() {
    for newline in ["\n", "\r\n", "\r"] {
        for (opening, middle, closing, path) in [
            ("> - ", ">\t    ", ">   ", vec![0, 0, 0]),
            ("- - ", "\t  ", "\t  ", vec![0, 0, 0]),
        ] {
            let source = format!(
                "{opening}[a]: /url \"first{newline}{middle}&#32;next{newline}{closing}end\"{newline}{newline}[a]{newline}"
            );
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let sites = document.reference_sites(&mut governor).unwrap();
            assert_eq!(sites.len(), 1);
            let site = &sites[0];
            assert_eq!(site.path, path);
            assert_eq!(site.title.as_deref(), Some("first\n next\nend"));
            assert_eq!(&source[site.destination_span.clone()], "/url");
            assert_eq!(
                &source[site.title_span.clone().unwrap()],
                format!("\"first{newline}{middle}&#32;next{newline}{closing}end\"")
            );
            assert_eq!(
                &source[site.span.clone()],
                format!(
                    "[a]: /url \"first{newline}{middle}&#32;next{newline}{closing}end\"{newline}"
                )
            );
            assert_eq!(
                document
                    .replace_reference(&site.path, 0, "/url", site.title.as_deref(), &mut governor)
                    .unwrap()
                    .edits,
                []
            );
            let before = document
                .render_html(Dialect::CommonMark, &mut governor)
                .unwrap();
            assert!(before.contains("title=\"first\n next\nend\""));
            let plan = document
                .remove_reference(&site.path, 0, &mut governor)
                .unwrap();
            let changed = apply(&plan, &source);
            let updated = Document::parse(&changed, &mut governor).unwrap();
            for dialect in [Dialect::CommonMark, Dialect::Gfm] {
                assert_eq!(updated.render_html(dialect, &mut governor).unwrap(), before);
            }
        }
    }
}

#[test]
fn token_spans_include_delimiters_but_not_surrounding_whitespace() {
    let source = "  [A]: </café>  'cold tea'  \r\n[A]: /shadow\r\n";
    let refs = References::parse_blocks(&[source], &mut Governor::new(Budget::default())).unwrap();
    assert_eq!(
        &source[refs.definitions()[0].destination_span.clone()],
        "</café>"
    );
    assert_eq!(
        &source[refs.definitions()[0].title_span.clone().unwrap()],
        "'cold tea'"
    );
    assert_eq!(
        &source[refs.definitions()[1].destination_span.clone()],
        "/shadow"
    );
    assert_eq!(refs.definitions()[1].title_span, None);
}

#[test]
fn nested_multiline_fields_and_labels_keep_exact_surrounding_source() {
    for source in [
        "> - [Tea]:\r\n>     /hot\r\n>     \"warm\r\n>     drink\"\r\n>   [tea]: /shadow\r\n",
        "> [Long\r\n> Label]: /hot\r\n> [other]: /shadow\r\n",
        ">\t[tea]:\t</hot>\t'title'\r\n",
        "[tea]: /hot \"title\"  \n[tea]: /shadow\n\n[TEA]\n",
    ] {
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(source, &mut g).unwrap();
        let before = doc.reference_sites(&mut g).unwrap();
        assert_ne!(before, []);
        let site = &before[0];
        assert!(source[site.destination_span.clone()].contains("/hot"));
        let plan = doc
            .replace_reference(
                &site.path,
                site.occurrence,
                "/iced",
                Some("Cold tea"),
                &mut g,
            )
            .unwrap();
        let result = apply(&plan, source);
        let edit = &plan.edits[0];
        assert!(result.starts_with(&source[..edit.range.start]));
        assert!(result.ends_with(&source[edit.range.end..]));
        let updated = Document::parse(&result, &mut g).unwrap();
        let after = updated.reference_sites(&mut g).unwrap();
        assert_eq!(after.len(), before.len());
        assert_eq!(after[0].label, before[0].label);
        assert_eq!(after[0].destination, "/iced");
        assert_eq!(after[0].title.as_deref(), Some("Cold tea"));
        for (old, new) in before.iter().skip(1).zip(after.iter().skip(1)) {
            assert_eq!(old.label, new.label);
            assert_eq!(old.destination, new.destination);
            assert_eq!(old.title, new.title);
        }
        assert_eq!(
            updated
                .replace_reference(
                    &after[0].path,
                    after[0].occurrence,
                    "/iced",
                    Some("Cold tea"),
                    &mut g
                )
                .unwrap()
                .edits,
            []
        );
    }
}

#[test]
fn semantic_fields_round_trip_entities_punctuation_newlines_and_empty_values() {
    for (destination, title) in [
        (
            "/iced?x=1&y=<2>\\",
            Some("Cold \"tea\" & <ice>\nnext\r\n\tline"),
        ),
        ("/line\nbreak\ttab", Some("literal &quot; and café")),
        ("", Some("")),
        ("/new", None),
    ] {
        let mut g = Governor::new(Budget::default());
        let source = "[tea]: /hot 'old'\n[tea]: /shadow\n\n[TEA]\n";
        let doc = Document::parse(source, &mut g).unwrap();
        let output = apply(
            &doc.replace_reference(&[0], 0, destination, title, &mut g)
                .unwrap(),
            source,
        );
        let updated = Document::parse(&output, &mut g).unwrap();
        let sites = updated.reference_sites(&mut g).unwrap();
        assert_eq!(sites[0].destination, destination);
        assert_eq!(sites[0].title.as_deref(), title);
        assert_eq!(sites[1].destination, "/shadow");
        assert!(
            !updated
                .render_html(Dialect::CommonMark, &mut g)
                .unwrap()
                .contains("href=\"/shadow\"")
        );
    }
}

#[test]
fn duplicates_are_selectable_and_invalid_inputs_or_exhaustion_return_no_plan() {
    let mut g = Governor::new(Budget::default());
    let source = "[tea]: /hot\n[TEA]: /shadow\n\n[tea]\n\n```\n[hidden]: /code\n```\n";
    let doc = Document::parse(source, &mut g).unwrap();
    assert_eq!(doc.reference_sites(&mut g).unwrap().len(), 2);
    let output = apply(
        &doc.replace_reference(&[0], 1, "/other", None, &mut g)
            .unwrap(),
        source,
    );
    let updated = Document::parse(&output, &mut g).unwrap();
    assert!(
        updated
            .render_html(Dialect::Gfm, &mut g)
            .unwrap()
            .contains("href=\"/hot\"")
    );
    for (path, occurrence, destination, title) in [
        (vec![], 0, "/new", None),
        (vec![99], 0, "/new", None),
        (vec![0], 99, "/new", None),
        (vec![0], 0, "\0", None),
        (vec![0], 0, "/new", Some("\0")),
    ] {
        assert!(matches!(
            doc.replace_reference(&path, occurrence, destination, title, &mut g),
            Err(StopReason::InvalidInput(_))
        ));
    }
    let mut exhausted = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert_eq!(
        doc.replace_reference(&[0], 0, "/new", None, &mut exhausted),
        Err(StopReason::BudgetExceeded(Resource::Work))
    );
    let mut cancelled = Governor::new(Budget::default());
    cancelled.cancel();
    assert!(doc.reference_sites(&mut cancelled).is_err());
}
