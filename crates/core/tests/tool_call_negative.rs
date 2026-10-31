// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The negative corpus (A11): model output that must never become a usable
//! tool call. Every case is Qwen3-Coder XML under a plain stop, recovered
//! twice: against read-only tools (so `Tolerant`, where guesses are
//! admitted) and against mutating tools (so `Structural`).
//!
//! The malformed cases (an unterminated opener, an empty name,
//! a nested `<function=`, trailing text inside the wrapper,
//! positional text) are rejected attempts whose reason is kept and whose
//! raw span is the whole call markup, closers included, so no content range
//! holds call markup; a hallucinated tool is a per-call `Unresolved` call. Either way the
//! turn finishes `Unresolved` and neither `usable()` nor
//! `usable_accepting_guesses()` holds a call. Only a call written inside
//! fenced code or quoted prose may instead be a guess-flagged reading
//! (`tool_call.context.fenced-example`): `BestGuess` at most under
//! `Tolerant`, `Unresolved` under `Structural`, and never in `usable()`.
//!
//! Text after an unpaired parameter closer retains both equally ranked guesses
//! through the next parameter or body end (issue #7), and neither is usable.
//!
//! JSON content without call-shaped evidence is plain content under the
//! default allow-list (every built-in dialect): no Llama 3 JSON attempt
//! commits, so there is no call and no rejected attempt.
use core::ops::Range;
use purrassist_core::tool_call::{
    CallOutcome, DialectId, DialectSet, Effects, Finish, GenerationStop, Param, Rejection,
    RepairPolicy, ToolCalls, ToolDefinition, ToolRecoveryRequest, ToolSet, UnresolvedReason,
};
use purrassist_core::{Budget, Governor, Outcome};

/// A call to `f` with one parameter `a` = `x`, in canonical layout.
const CALL: &str =
    "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\n</tool_call>";

const PROFILES: [Effects; 2] = [Effects::ReadOnly, Effects::Mutating];

fn tools(effects: Effects) -> ToolSet {
    ToolSet::compile(
        [
            ToolDefinition::new("f")
                .effects(effects)
                .param("a", Param::string())
                .param("b", Param::string()),
            ToolDefinition::new("g")
                .effects(effects)
                .param("a", Param::string()),
        ],
        Budget::default(),
    )
    .expect("tools compile")
}

/// Recover `input` against the profile's tools; a negative case never
/// makes the operation itself unresolved, so there is one reading.
fn recover(input: &str, effects: Effects) -> ToolCalls {
    let dialects = DialectSet::new(
        [DialectId::Qwen3CoderXml],
        &mut Governor::new(Budget::default()),
    )
    .expect("dialect set");
    let result =
        tools(effects).recover(input.as_bytes(), &ToolRecoveryRequest::default(), &dialects);
    match result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
        other => panic!("{input:?}: expected one reading, got {other:?}"),
    }
}

fn rejected(calls: &ToolCalls) -> Vec<(Range<usize>, Rejection)> {
    calls
        .rejected
        .iter()
        .map(|rejected| (rejected.span.clone(), rejected.reason))
        .collect()
}

fn rules(calls: &ToolCalls) -> Vec<Vec<&str>> {
    calls
        .calls
        .iter()
        .map(|call| {
            call.repairs
                .evidence
                .iter()
                .map(|evidence| evidence.rule.as_str())
                .filter(|rule| *rule != "tool_call.xml.framing-newline")
                .collect()
        })
        .collect()
}

/// What every negative case guarantees, whatever its reading: nothing is
/// usable, and every rejected attempt keeps its exact bytes and reason.
fn assert_never_usable(input: &str, calls: &ToolCalls) {
    assert!(calls.usable().is_empty(), "{input:?}: {calls:#?}");
    for rejected in &calls.rejected {
        assert_eq!(
            rejected.raw.as_slice(),
            &input.as_bytes()[rejected.span.clone()],
            "{input:?}"
        );
    }
}

/// No content range holds call markup: a rejected attempt keeps its whole
/// markup, closers included, as its raw text.
fn assert_no_markup_in_content(input: &str, calls: &ToolCalls) {
    for range in &calls.content {
        let text = &input[range.clone()];
        for token in [
            "<tool_call>",
            "</tool_call>",
            "<function=",
            "</function>",
            "<parameter=",
            "</parameter>",
            "</parameters>",
        ] {
            assert!(
                !text.contains(token),
                "{input:?}: content {text:?} holds {token}"
            );
        }
    }
}

/// A malformed case: the attempt is rejected with `expected` (span and
/// reason), no call is recognized, the turn is `Unresolved`, and nothing
/// is usable even accepting guesses, under both profiles.
fn assert_rejected(input: &str, expected: &[(Range<usize>, Rejection)]) {
    for effects in PROFILES {
        let calls = recover(input, effects);
        assert_never_usable(input, &calls);
        assert_no_markup_in_content(input, &calls);
        assert_eq!(rejected(&calls), expected, "{effects:?} {input:?}");
        assert_eq!(calls.calls.len(), 0, "{effects:?} {input:?}");
        assert_eq!(calls.finish, Finish::Unresolved, "{effects:?} {input:?}");
        assert_eq!(calls.usable_accepting_guesses().len(), 0);
    }
}

#[test]
fn unterminated_opener() {
    let input = "<tool_call><function=f";
    assert_rejected(input, &[(0..22, Rejection::Unterminated)]);
}

#[test]
fn empty_function_name() {
    let input = "<tool_call><function=>x</function></tool_call>";
    assert_rejected(input, &[(0..46, Rejection::Malformed { at: 21 })]);
}

/// Issue #7 retains both readings while preserving the negative corpus's
/// requirement that no interpretation is usable, even accepting guesses.
fn assert_ambiguous_tail(input: &str, held: &str) {
    for effects in PROFILES {
        let calls = recover(input, effects);
        assert_never_usable(input, &calls);
        assert_no_markup_in_content(input, &calls);
        assert!(calls.rejected.is_empty(), "{effects:?} {input:?}");
        assert_eq!(calls.calls.len(), 1, "{effects:?} {input:?}");
        let call = &calls.calls[0];
        assert_eq!(
            call.outcome,
            CallOutcome::Ambiguous,
            "{effects:?} {call:#?}"
        );
        let mut values: Vec<_> = core::iter::once(&call.arguments)
            .chain(call.alternatives.iter().map(|reading| &reading.arguments))
            .map(|arguments| {
                arguments
                    .get("a")
                    .and_then(purrassist_core::json::Node::as_str)
                    .expect("parameter a")
            })
            .collect();
        values.sort_unstable();
        let mut expected = ["x", held];
        expected.sort_unstable();
        assert_eq!(values, expected, "{effects:?} {input:?}");
        assert_eq!(calls.finish, Finish::Unresolved, "{effects:?} {input:?}");
        assert!(calls.usable_accepting_guesses().is_empty());
    }
}

#[test]
fn text_between_parameters() {
    let input = "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\nsome text\n<parameter=b>\ny\n</parameter>\n</function>\n</tool_call>";
    assert_ambiguous_tail(input, "x\n</parameter>\nsome text");
}

#[test]
fn nested_function_inside_function() {
    let input = "<tool_call>\n<function=f>\n<function=g>\n<parameter=a>\nx\n</parameter>\n</function>\n</function>\n</tool_call>";
    assert_rejected(input, &[(0..103, Rejection::Malformed { at: 25 })]);
}

#[test]
fn trailing_text_inside_wrapper() {
    let input = "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\nthat is all\n</tool_call>";
    assert_rejected(input, &[(0..90, Rejection::Malformed { at: 66 })]);
}

/// Repeated closers after an ambiguous body tail remain markup; only the
/// prose after them stays content.
#[test]
fn stray_closers_after_ambiguous_body_tail() {
    let input = "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\nthat is all</function>\n</tool_call>\n</tool_call> tail";
    let tail = input.len() - " tail".len();
    assert_ambiguous_tail(input, "x\n</parameter>\nthat is all");
    for effects in PROFILES {
        let calls = recover(input, effects);
        assert_never_usable(input, &calls);
        assert_no_markup_in_content(input, &calls);
        assert!(calls.rejected.is_empty(), "{effects:?}");
        assert_eq!(calls.content, vec![tail..input.len()], "{effects:?}");
    }
}

#[test]
fn positional_text_in_function_body() {
    let input = "<tool_call>\n<function=f>\nhello\n</function>\n</tool_call>";
    assert_rejected(input, &[(0..55, Rejection::Malformed { at: 25 })]);
}

#[test]
fn hallucinated_tool() {
    let input = "<tool_call>\n<function=send_email>\n<parameter=a>\nx\n</parameter>\n</function>\n</tool_call>";
    for effects in PROFILES {
        let calls = recover(input, effects);
        assert_never_usable(input, &calls);
        assert_eq!(rejected(&calls), [], "{effects:?}");
        let outcomes: Vec<&CallOutcome> = calls.calls.iter().map(|call| &call.outcome).collect();
        assert_eq!(
            outcomes,
            [&CallOutcome::Unresolved {
                reason: UnresolvedReason::UnknownTool {
                    name: "send_email".into()
                }
            }],
            "{effects:?}"
        );
        assert_eq!(calls.calls[0].tool, None);
        assert_eq!(calls.calls[0].span, 0..input.len());
        assert_eq!(calls.finish, Finish::Unresolved, "{effects:?}");
        assert_eq!(calls.usable_accepting_guesses().len(), 0);
    }
}

/// A call inside fenced code or quoted prose: a guess-flagged reading of
/// `f {"a": "x"}` (`tool_call.context.fenced-example`), `BestGuess` on the
/// read-only tool and refused under `Structural` on the mutating one;
/// never in `usable()`.
fn assert_example(input: &str) {
    for effects in PROFILES {
        let calls = recover(input, effects);
        assert_never_usable(input, &calls);
        assert_eq!(rejected(&calls), [], "{effects:?} {input:?}");
        assert_eq!(
            rules(&calls),
            [["tool_call.context.fenced-example"]],
            "{effects:?} {input:?}"
        );
        let call = &calls.calls[0];
        assert_eq!(call.arguments.to_json(), r#"{"a":"x"}"#);
        let expected = match effects {
            Effects::ReadOnly => CallOutcome::BestGuess,
            Effects::Mutating => CallOutcome::Unresolved {
                reason: UnresolvedReason::RuleNotAdmitted {
                    rule: "tool_call.context.fenced-example".into(),
                    policy: RepairPolicy::Structural,
                },
            },
        };
        assert_eq!(call.outcome, expected, "{effects:?} {input:?}");
        // A read-only guess is selectable only when the caller accepts
        // guesses; a mutating one never is.
        assert_eq!(
            calls.usable_accepting_guesses().len(),
            usize::from(effects == Effects::ReadOnly),
            "{effects:?} {input:?}"
        );
    }
}

#[test]
fn fenced_example_in_prose() {
    assert_example(&format!(
        "For example:\n```\n{CALL}\n```\nThat is the format."
    ));
}

#[test]
fn quoted_inline_code_in_prose() {
    assert_example(
        "Use `<tool_call><function=f><parameter=a>x</parameter></function></tool_call>` to call it.",
    );
}

#[test]
fn quoted_block_quote_line() {
    assert_example("> <tool_call><function=f><parameter=a>x</parameter></function></tool_call>\n");
}

#[test]
fn quoted_block_quote_lines_are_malformed() {
    // A call quoted line by line carries the quote markers inside its
    // markup: it is not a call at all. The `<tool_call>` attempt fails at
    // the quote marker, and so does the bare `<function=f>` element after
    // it: two rejected attempts, never silent content. The inner element's
    // markup runs to its `</function>`; the wrapper's runs over it to the
    // quoted `</tool_call>`, so its raw text holds the whole call and no
    // closer is left as content.
    let input = "> <tool_call>\n> <function=f>\n> <parameter=a>\n> x\n> </parameter>\n> </function>\n> </tool_call>\n";
    assert_rejected(
        input,
        &[
            (2..92, Rejection::Malformed { at: 14 }),
            (16..77, Rejection::Malformed { at: 29 }),
        ],
    );
}

/// JSON objects in prose, schemas, code, and objects with a `"name"` or a
/// `"parameters"` member but not both in call order stay content under the
/// default allow-list, whatever the stop.
#[test]
fn json_content_without_call_evidence_is_content() {
    for input in [
        r#"Saved {"name": "Ada", "age": 36}."#,
        r#"{"name": "f"}"#,
        r#"{"name": "f", "description": "no parameters member"}"#,
        r#"{"parameters": {"a": "x"}}"#,
        r#"{"parameters": {"a": "x"}, "title": "f"}"#,
        r#"[{"name": "f", "value": 1}, {"name": "g", "value": 2}]"#,
        r#"{"type": "object", "properties": {"name": {"type": "string"}}}"#,
        "fn main() { let name = \"f\"; }",
        r#"{"user": {"name": "f", "roles": ["a"]}, "parameters": 3}"#,
        "{\n  \"name\": \"f\",\n  \"tags\": []\n}",
        r#"{"name": "f", "#,
    ] {
        for effects in PROFILES {
            for stop in [GenerationStop::Stop, GenerationStop::Length] {
                let result = tools(effects).recover(
                    input.as_bytes(),
                    &ToolRecoveryRequest::default().stop(stop),
                    purrassist_core::tool_call::DialectSet::built_in(),
                );
                let Outcome::Resolved(calls) = result.outcome else {
                    panic!("{input:?}: {:?}", result.outcome)
                };
                assert!(calls.calls.is_empty(), "{input:?}: {:?}", calls.calls);
                assert!(calls.rejected.is_empty(), "{input:?}: {:?}", calls.rejected);
                assert_eq!(calls.content, vec![0..input.len()], "{input:?}");
            }
        }
    }
}

/// The negative corpus as issue #3 writes it, without the `<tool_call>`
/// wrapper (whose special tokens detokenizers commonly strip): a bare
/// `<function=` element is a call attempt, so each malformed one is a
/// rejected attempt, never silent content.
#[test]
fn unwrapped_negative_corpus_is_rejected() {
    assert_ambiguous_tail(
        "<function=f>\n<parameter=a>\nx\n</parameter>\nsome text\n<parameter=b>\ny\n</parameter>\n</function>",
        "x\n</parameter>\nsome text",
    );
    for (input, expected) in [
        ("<function=f", vec![(0..11, Rejection::Unterminated)]),
        (
            "<function=>x</function>",
            vec![(0..23, Rejection::Malformed { at: 10 })],
        ),
        (
            "<function=f>\n<function=g>\n<parameter=a>\nx\n</parameter>\n</function>\n</function>",
            vec![(0..78, Rejection::Malformed { at: 13 })],
        ),
        (
            "<function=f>\nhello\n</function>",
            vec![(0..30, Rejection::Malformed { at: 13 })],
        ),
    ] {
        assert_rejected(input, &expected);
    }
}
