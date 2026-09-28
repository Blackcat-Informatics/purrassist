// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-preserving list item insertion, replacement, removal, and undo.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    markdown::{Document, ListItemEdit},
};
fn edit(source: &str, path: &[usize], operation: &ListItemEdit, expected: &str) {
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(source, &mut g).unwrap();
    let plan = doc
        .edit_list_item(path, operation, &mut g)
        .unwrap_or_else(|e| panic!("{source:?} {operation:?}: {e:?}"));
    let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else {
        panic!("apply");
    };
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
#[test]
fn insertion_preserves_markers_outer_containers_and_line_endings() {
    for nl in ["\n", "\r\n", "\r"] {
        for (source, path, before, after) in [
            (
                "- hot\n- keep\n",
                vec![0],
                "+ iced\n- hot\n- keep\n",
                "- hot\n+ iced\n- keep\n",
            ),
            (
                "  - hot\n  - keep\n",
                vec![0],
                "  + iced\n  - hot\n  - keep\n",
                "  - hot\n  + iced\n  - keep\n",
            ),
            (
                "> - hot\n> - keep\n",
                vec![0, 0],
                "> + iced\n> - hot\n> - keep\n",
                "> - hot\n> + iced\n> - keep\n",
            ),
            (
                "- - hot\n  - keep\n",
                vec![0, 0],
                "- + iced\n  - hot\n  - keep\n",
                "- - hot\n  + iced\n  - keep\n",
            ),
            (
                "> 7. hot\n> 9. keep\n",
                vec![0, 0],
                "> + iced\n> 7. hot\n> 9. keep\n",
                "> 7. hot\n> + iced\n> 9. keep\n",
            ),
        ] {
            edit(
                &source.replace('\n', nl),
                &path,
                &ListItemEdit::Before {
                    marker: "+".into(),
                    body: "iced".into(),
                },
                &before.replace('\n', nl),
            );
            edit(
                &source.replace('\n', nl),
                &path,
                &ListItemEdit::After {
                    marker: "+".into(),
                    body: "iced".into(),
                },
                &after.replace('\n', nl),
            );
        }
    }
}
#[test]
fn removal_retains_enclosing_markers_and_other_items() {
    for (source, path, expected) in [
        ("- hot\n- keep\n", vec![0], "- keep\n"),
        ("- keep\n- hot\n", vec![1], "- keep\n"),
        ("> - hot\n> - keep\n", vec![0, 0], "> - keep\n"),
        ("- - hot\n  - keep\n", vec![0, 0], "- - keep\n"),
        ("- - keep\n  - hot\n", vec![0, 1], "- - keep\n"),
        ("> - hot\n", vec![0, 0], "> \n"),
        ("- - hot\n", vec![0, 0], "- \n"),
        ("- hot", vec![0], ""),
    ] {
        edit(source, &path, &ListItemEdit::Remove, expected);
    }
}
#[test]
fn replacement_and_eof_insertions_preserve_requested_body_and_existing_numbers() {
    edit(
        "> 007) hot\n> 009) keep\n",
        &[0, 0],
        &ListItemEdit::Replace {
            body: "[x] café\n\n## Child\nReady".into(),
        },
        "> 007) [x] café\n>      \n>      ## Child\n>      Ready\n> 009) keep\n",
    );
    for (source, path, expected) in [
        ("- hot", vec![0], "- hot\n8. iced"),
        ("> - hot", vec![0, 0], "> - hot\n> 8. iced"),
    ] {
        edit(
            source,
            &path,
            &ListItemEdit::After {
                marker: "8.".into(),
                body: "iced".into(),
            },
            expected,
        );
    }
    edit(
        "- hot\n",
        &[0],
        &ListItemEdit::Replace {
            body: String::new(),
        },
        "- \n",
    );
}
#[test]
fn invalid_markers_structure_capture_and_exhaustion_are_explicit() {
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse("- hot\n- keep\n", &mut g).unwrap();
    for marker in ["", "--", "1234567890.", "1x", "1.\n-", "💥."] {
        assert!(matches!(
            doc.edit_list_item(
                &[0],
                &ListItemEdit::Before {
                    marker: marker.into(),
                    body: "cold".into()
                },
                &mut g
            ),
            Err(StopReason::InvalidInput(_))
        ));
    }
    for body in ["hot\n\n", "\n\n"] {
        assert!(matches!(
            doc.edit_list_item(&[0], &ListItemEdit::Replace { body: body.into() }, &mut g),
            Err(StopReason::InvalidInput(_))
        ));
    }
    // A sibling marker closes the item even when its internal fence is unclosed.
    edit(
        "- hot\n- keep\n",
        &[0],
        &ListItemEdit::Replace {
            body: "```\ncode".into(),
        },
        "- ```\n  code\n- keep\n",
    );
    let mut exhausted = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert_eq!(
        doc.edit_list_item(&[0], &ListItemEdit::Remove, &mut exhausted)
            .unwrap_err(),
        StopReason::BudgetExceeded(Resource::Work)
    );
}
