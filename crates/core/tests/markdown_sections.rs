// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Section edits preserve outside structure and reject boundary capture.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason, contract::Resource, markdown::Document,
};

#[test]
fn replacement_preserves_surrounding_containers_and_exact_undo() {
    for newline in ["\n", "\r\n", "\r"] {
        for heading in ["# Tea", "Tea\n==="] {
            let source = format!("Preamble\n\n{heading}\nOld\n## Child\nRemove\n# Keep\n> - ```rust\n>   code\n>   ```\n").replace('\n', newline);
            let body = "Cold café.\n\n## New child\n- [x] ready\n".replace('\n', newline);
            let mut g = Governor::new(Budget::default());
            let doc = Document::parse(&source, &mut g).unwrap();
            let section = &doc.sections()[0];
            let plan = doc
                .replace_section_governed(section.heading, &body, &mut g)
                .unwrap();
            let Outcome::Resolved(applied) = plan.apply(&source, Budget::default()).outcome else {
                panic!("apply");
            };
            let mut expected = source.clone();
            expected.replace_range(section.body.clone(), &body);
            assert_eq!(applied.text, expected);
            let Outcome::Resolved(undo) = applied
                .inverse
                .apply(&applied.text, Budget::default())
                .outcome
            else {
                panic!("undo");
            };
            assert_eq!(undo.text, source);
            assert!(matches!(
                plan.apply("stale", Budget::default()).outcome,
                Outcome::Unresolved
            ));
        }
    }
}

#[test]
fn capture_peer_headings_and_missing_separators_are_rejected() {
    let doc = Document::parse(
        "# Tea\nOld\n# Keep\nKeep\n",
        &mut Governor::new(Budget::default()),
    )
    .unwrap();
    for body in [
        "```\ncapture\n",
        "<!--\ncapture\n",
        "# Peer\n",
        "plain without newline",
        "Title\n===\n",
    ] {
        assert!(
            matches!(
                doc.replace_section(0, body),
                Err(StopReason::InvalidInput(_))
            ),
            "{body:?}"
        );
    }
    let eof = Document::parse("# Tea", &mut Governor::new(Budget::default())).unwrap();
    assert!(matches!(
        eof.replace_section(0, "body"),
        Err(StopReason::InvalidInput(_))
    ));
    assert!(matches!(
        doc.replace_section(1, ""),
        Err(StopReason::InvalidInput(_))
    ));
}

#[test]
fn empty_noop_and_final_section_have_explicit_behavior() {
    let source = "# Tea\nOld\n# Keep\nTail";
    let doc = Document::parse(source, &mut Governor::new(Budget::default())).unwrap();
    assert_eq!(doc.replace_section(0, "Old\n").unwrap().edits, []);
    let plan = doc.replace_section(0, "").unwrap();
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply");
    };
    assert_eq!(applied.text, "# Tea\n# Keep\nTail");
    let last = doc.sections()[1].heading;
    assert_eq!(
        doc.replace_section(last, "New EOF").unwrap().edits[0].replacement,
        "New EOF"
    );
    // An unclosed fence at EOF remains inside the chosen section.
    assert!(doc.replace_section(last, "```\nbody").is_ok());
}

#[test]
fn exhausted_governor_cannot_return_a_plan_even_for_noop() {
    let doc = Document::parse("# Tea\nOld\n", &mut Governor::new(Budget::default())).unwrap();
    let mut g = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert_eq!(
        doc.replace_section_governed(0, "Old\n", &mut g)
            .unwrap_err(),
        StopReason::BudgetExceeded(Resource::Work)
    );
}

#[test]
fn nested_sections_follow_container_paths_and_preserve_outside_bytes() {
    for newline in ["\n", "\r\n", "\r"] {
        for (source, path, prefix) in [
            (
                "> # Tea\n> Old\n> # Keep\n> Tail\nOutside\n",
                vec![0, 0],
                "> ",
            ),
            (
                "- # Tea\n  Old\n  # Keep\n  Tail\n- Outside\n",
                vec![0, 0],
                "  ",
            ),
            (
                "> 12. # Tea\n>     Old\n>     # Keep\n>     Tail\n> 13. Outside\n",
                vec![0, 0, 0],
                ">     ",
            ),
            (
                "- > # Tea\n  > Old\n  > # Keep\n  > Tail\n- Outside\n",
                vec![0, 0, 0],
                "  > ",
            ),
            (
                "> - > Tea\n>   > ===\n>   > Old\n>   > # Keep\n>   > Tail\n",
                vec![0, 0, 0, 0],
                ">   > ",
            ),
            (
                "-\t# Tea\n\tOld\n\t# Keep\n\tTail\n- Outside\n",
                vec![0, 0],
                "    ",
            ),
        ] {
            let source = source.replace('\n', newline);
            let body = format!("Cold café.{newline}{newline}## Child{newline}Ready{newline}");
            let mut g = Governor::new(Budget::default());
            let doc = Document::parse(&source, &mut g).unwrap();
            let sites = doc.section_sites(&mut g).unwrap();
            let site = sites.iter().find(|site| site.path == path).unwrap();
            assert_eq!(site.source, doc.digest());
            let plan = doc
                .replace_section_at(&path, &body, &mut g)
                .unwrap_or_else(|err| panic!("{source:?} {err:?}"));
            assert_eq!(plan.edits[0].range, site.body);
            let replacement = format!(
                "{prefix}Cold café.{newline}{prefix}{newline}{prefix}## Child{newline}{prefix}Ready{newline}"
            );
            assert_eq!(plan.edits[0].replacement, replacement);
            let Outcome::Resolved(applied) = plan.apply(&source, Budget::default()).outcome else {
                panic!("apply");
            };
            let mut expected = source.clone();
            expected.replace_range(site.body.clone(), &replacement);
            assert_eq!(applied.text, expected);
            let Outcome::Resolved(undo) = applied
                .inverse
                .apply(&applied.text, Budget::default())
                .outcome
            else {
                panic!("undo");
            };
            assert_eq!(undo.text, source);
        }
    }
}

#[test]
fn nested_noops_and_invalid_bodies_keep_original_prefix_spelling() {
    let source = "># Tea\n>Old\n># Keep\n>Tail\n";
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(source, &mut g).unwrap();
    assert_eq!(
        doc.replace_section_at(&[0, 0], "Old\n", &mut g)
            .unwrap()
            .edits,
        []
    );
    for body in ["# Peer\n", "```\n", "<!--\n", "no ending"] {
        assert!(
            matches!(
                doc.replace_section_at(&[0, 0], body, &mut g),
                Err(StopReason::InvalidInput(_))
            ),
            "{body:?}"
        );
    }
    assert!(matches!(
        doc.replace_section_at(&[], "", &mut g),
        Err(StopReason::InvalidInput(_))
    ));
    assert!(matches!(
        doc.replace_section_at(&[0, 1], "", &mut g),
        Err(StopReason::InvalidInput(_))
    ));
    let mut exhausted = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert_eq!(
        doc.replace_section_at(&[0, 0], "Old\n", &mut exhausted)
            .unwrap_err(),
        StopReason::BudgetExceeded(Resource::Work)
    );
}

#[test]
fn nested_final_sections_empty_bodies_and_container_escape_checks() {
    for (source, path, expected) in [
        ("> # Tea\n> Old\n", vec![0, 0], "> # Tea\n"),
        ("- # Tea\n  Old\n- Keep\n", vec![0, 0], "- # Tea\n- Keep\n"),
        (
            "> - # Tea\n>   Old\n> - Keep\n",
            vec![0, 0, 0],
            "> - # Tea\n> - Keep\n",
        ),
    ] {
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(source, &mut g).unwrap();
        let plan = doc.replace_section_at(&path, "", &mut g).unwrap();
        let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
            panic!("apply");
        };
        assert_eq!(applied.text, expected);
    }
    for (source, path, prefix) in [
        ("> # Tea\n", vec![0, 0], "> "),
        ("- # Tea\n", vec![0, 0], "  "),
    ] {
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(source, &mut g).unwrap();
        let plan = doc.replace_section_at(&path, "```\ncode", &mut g).unwrap();
        assert_eq!(
            plan.edits[0].replacement,
            format!("{prefix}```\n{prefix}code")
        );
    }
    // Layout recovery: a trailing blank body line is re-indented to the
    // container's own level (it is ejected there regardless, by this
    // parser's blank-line-before-dedent rule) instead of being rejected.
    let mut g = Governor::new(Budget::default());
    let source = "- # Tea\n  Old\n- Keep\n";
    let doc = Document::parse(source, &mut g).unwrap();
    let plan = doc.replace_section_at(&[0, 0], "Cold\n\n", &mut g).unwrap();
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply");
    };
    assert_eq!(applied.text, "- # Tea\n  Cold\n\n- Keep\n");
    let Outcome::Resolved(undo) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo");
    };
    assert_eq!(undo.text, source);

    // Layout recovery: a leading tab is re-indented to the number of spaces
    // it would occupy starting at column zero -- matching how the body
    // parses standalone -- instead of being rejected as column-ambiguous.
    let quote_source = "> # Tea\n> Old\n";
    let quote = Document::parse(quote_source, &mut g).unwrap();
    let plan = quote
        .replace_section_at(&[0, 0], "\tcode\n", &mut g)
        .unwrap();
    let Outcome::Resolved(applied) = plan.apply(quote_source, Budget::default()).outcome else {
        panic!("apply");
    };
    assert_eq!(applied.text, "> # Tea\n>     code\n");
    let Outcome::Resolved(undo) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("undo");
    };
    assert_eq!(undo.text, quote_source);
}
