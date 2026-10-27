// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Prepared-grammar printing: scalar bounds, exact refusals and transient ownership.

use purrassist_core::grammar::witness::{self, WitnessOptions};
use purrassist_core::grammar::{
    CompiledGrammar, Grammar,
    print::{PrintError, Root, Values, canonical_governed},
};
use purrassist_core::tool_call::{ToolDefinition, ToolSet};
use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use std::fmt::Write as _;

fn governor() -> Governor {
    Governor::new(Budget::default())
}
fn grammar(result: &str) -> CompiledGrammar {
    Grammar::compile_governed(
        &format!(
            r#"grammar "printing-test"; call = "<c>"; result = {result}; declaration = "<d>";"#
        ),
        &mut governor(),
    )
    .unwrap()
}

#[test]
fn retained_generator_keeps_unicode_scalar_bounds_and_requested_minimum_fallback() {
    for (class, expected) in [(r"/(?:Ω|😀){2}/", "ΩΩ"), (r"/a{16}/", "aaaaaaaaaaaaaaaa")] {
        let grammar = grammar(&format!(r#""<r>" {class} "</r>""#));
        let mut g = governor();
        let printed =
            canonical_governed(&grammar, Root::Result, Values::default(), &[], &mut g).unwrap();
        assert_eq!(printed.bytes, format!("<r>{expected}</r>").as_bytes());
    }
    let too_long = grammar(r#""<r>" /a{17}/ "</r>""#);
    let mut g = governor();
    let error =
        canonical_governed(&too_long, Root::Result, Values::default(), &[], &mut g).unwrap_err();
    let PrintError::NoCandidate { class } = error else {
        panic!("expected bounded class refusal")
    };
    assert_eq!(class, "a{17}");

    let grammar = Grammar::compile_governed(
        r#"
grammar "unicode-id";
word = /[a-z]+/;
glyph = /(?:Ω|😀){2}/;
call = "<c>" id:glyph ":" name:word " " arguments:slot(json, "</c>") "</c>";
result = "<r>";
declaration = "<d>";
"#,
        &mut governor(),
    )
    .unwrap();
    let tools = ToolSet::compile([ToolDefinition::new("read")], Budget::default()).unwrap();
    let shape = tools.argument_shape("read").unwrap();
    let printed = witness::generate(
        &grammar,
        &[shape],
        WitnessOptions::default(),
        &mut governor(),
    )
    .unwrap();
    // Seeded ids first request at least four scalars; this two-scalar class
    // therefore exercises the existing zero-minimum fallback, not a byte bound.
    assert_eq!(printed.text, "<c>ΩΩ:read {}</c>");
    assert_eq!(printed.calls[0].id.as_deref(), Some("ΩΩ"));
}

#[test]
fn canonical_boundaries_release_intermediates_and_preserve_sticky_typed_causes() {
    let grammar =
        grammar(r#"("<r>" content:until("</r>") "</r>" | "<q>" content:until("</q>") "</q>")"#);
    let content = "a".repeat(80);
    let values = Values {
        content: Some(&content),
        ..Values::default()
    };
    let mut measured = governor();
    let expected = canonical_governed(&grammar, Root::Result, values, &[], &mut measured).unwrap();
    assert_eq!(expected.bytes, format!("<r>{content}</r>").as_bytes());
    let usage = measured.usage();
    let exact = Budget {
        work: usage.work,
        scratch_bytes: usage.peak_scratch_bytes,
        input_bytes: u64::try_from(content.len()).unwrap(),
        ..Budget::default()
    };
    assert_eq!(
        canonical_governed(
            &grammar,
            Root::Result,
            values,
            &[],
            &mut Governor::new(exact)
        )
        .unwrap()
        .bytes,
        expected.bytes
    );
    for (budget, resource) in [
        (
            Budget {
                work: exact.work - 1,
                ..exact
            },
            Resource::Work,
        ),
        (
            Budget {
                scratch_bytes: exact.scratch_bytes - 1,
                ..exact
            },
            Resource::ScratchBytes,
        ),
        (
            Budget {
                input_bytes: exact.input_bytes - 1,
                ..exact
            },
            Resource::InputBytes,
        ),
        (Budget { depth: 0, ..exact }, Resource::Depth),
    ] {
        let mut g = Governor::new(budget);
        assert_eq!(
            canonical_governed(&grammar, Root::Result, values, &[], &mut g).unwrap_err(),
            PrintError::Resource(StopReason::BudgetExceeded(resource))
        );
        assert_eq!(g.check(), Err(StopReason::BudgetExceeded(resource)));
        assert_eq!(g.scratch_meter().live(), 0);
    }
    let token = CancellationToken::new();
    token.cancel();
    let mut g = Governor::with_cancellation(exact, token);
    assert_eq!(
        canonical_governed(&grammar, Root::Result, values, &[], &mut g).unwrap_err(),
        PrintError::Resource(StopReason::Cancelled)
    );
    assert_eq!(g.usage().work, 0);
    check_temporary_then_refusal();
    check_native_ceiling();
}

fn check_temporary_then_refusal() {
    let grammar = grammar(r#""<r>" content:until("</r>") "</r>""#);
    let content = "a".repeat(256);
    let values = Values {
        content: Some(&content),
        ..Values::default()
    };
    let mut g = Governor::new(Budget {
        scratch_bytes: 300,
        ..Budget::default()
    });
    assert_eq!(
        canonical_governed(&grammar, Root::Result, values, &[], &mut g).unwrap_err(),
        PrintError::Resource(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
    assert!(
        g.usage().peak_scratch_bytes >= 256,
        "the intermediate was admitted before output growth failed"
    );
    assert_eq!(g.scratch_meter().live(), 0);
    let bad = Values {
        content: Some("a</r>"),
        ..Values::default()
    };
    let mut g = governor();
    let PrintError::Unrepresentable { text } =
        canonical_governed(&grammar, Root::Result, bad, &[], &mut g).unwrap_err()
    else {
        panic!("expected original delimiter refusal")
    };
    assert_eq!(text, "a</r>");
}

fn check_native_ceiling() {
    // Caller-large depth cannot bypass the operation's native traversal bound.
    let mut source = String::from(
        "grammar \"deep-print\"; call = \"<c>\"; declaration = \"<d>\"; result = \"prefix\" n0;",
    );
    for index in 0..130 {
        write!(source, "n{index} = n{};", index + 1).unwrap();
    }
    source.push_str("n130 = \"tail\";");
    let grammar = Grammar::compile_governed(
        &source,
        &mut Governor::new(Budget {
            depth: 4096,
            ..Budget::default()
        }),
    )
    .unwrap();
    let mut g = Governor::new(Budget {
        depth: 4096,
        ..Budget::default()
    });
    assert_eq!(
        canonical_governed(&grammar, Root::Result, Values::default(), &[], &mut g).unwrap_err(),
        PrintError::Resource(StopReason::BudgetExceeded(Resource::Depth))
    );
    assert_eq!(g.check(), Err(StopReason::BudgetExceeded(Resource::Depth)));
    assert_eq!(g.scratch_meter().live(), 0);
}
