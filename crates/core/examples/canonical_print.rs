// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Direct canonical printing over a caller-prepared grammar, without cold-compile claims.

use purrassist_core::grammar::{
    CaptureKind, Grammar,
    print::{PrintError, Root, Values, canonical_governed},
};
use purrassist_core::{Budget, Governor, StopReason, contract::Resource};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Grammar/source/DOM preparation deliberately uses a different operation.
    let grammar = Grammar::compile_governed(
        r#"
grammar "canonical.v1";
word = /[a-z_]+/;
call = "<c>" name:word " " arguments:slot(json, "</c>") "</c>";
result = "<r>" (id:word ":")? content:until("</r>") "</r>";
declaration = "<d>" name:word " " arguments:slot(json, "</d>") "</d>";
"#,
        &mut Governor::new(Budget::default()),
    )?;
    for (root, values, expected) in [
        (
            Root::Call,
            Values {
                name: Some("read"),
                arguments: Some("{}"),
                ..Values::default()
            },
            "<c>read {}</c>",
        ),
        (
            Root::Result,
            Values {
                id: Some("call_id"),
                content: Some("ok"),
                ..Values::default()
            },
            "<r>call_id:ok</r>",
        ),
        (
            Root::Declaration,
            Values {
                name: Some("read"),
                arguments: Some(r#"{"type":"object"}"#),
                ..Values::default()
            },
            r#"<d>read {"type":"object"}</d>"#,
        ),
    ] {
        let printed = canonical_governed(
            &grammar,
            root,
            values,
            &[],
            &mut Governor::new(Budget::default()),
        )?;
        assert_eq!(printed.bytes, expected.as_bytes());
        assert!(printed.printed(if root == Root::Result {
            CaptureKind::Content
        } else {
            CaptureKind::Name
        }));
    }
    let bad = Values {
        content: Some("a</r>"),
        ..Values::default()
    };
    assert!(
        matches!(canonical_governed(&grammar, Root::Result, bad, &[], &mut Governor::new(Budget::default())), Err(PrintError::Unrepresentable { text }) if text == "a</r>")
    );
    let values = Values {
        name: Some("read"),
        arguments: Some("{}"),
        ..Values::default()
    };
    assert_eq!(
        canonical_governed(
            &grammar,
            Root::Call,
            values,
            &[],
            &mut Governor::new(Budget {
                work: 0,
                ..Budget::default()
            })
        )
        .unwrap_err(),
        PrintError::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
    Ok(())
}
