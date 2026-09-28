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
