// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Pythonic calls preserve native literal semantics and reject executable text.
use purrassist_core::tool_call::{
    CallOutcome, DialectId, DialectSet, GenerationStop, ReasoningPolicy, Recognizer, RejectedCall,
    ToolCall, ToolCalls, ToolRecoveryRequest, ToolSet,
};
use purrassist_core::{Budget, Governor, Outcome};

fn tools() -> ToolSet {
    ToolSet::from_openai(r#"[{"type":"function","function":{"name":"f","parameters":{"type":"object","properties":{"text":{"type":"string"},"data":{},"number":{"type":"number"}},"additionalProperties":false}}}]"#, Budget::default()).expect("tools")
}
fn recover(input: &str) -> ToolCalls {
    let set = DialectSet::new([DialectId::Pythonic], &mut Governor::new(Budget::default()))
        .expect("dialect");
    match tools()
        .recover(input.as_bytes(), &ToolRecoveryRequest::default(), &set)
        .outcome
    {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
        other => panic!("{input:?}: {other:?}"),
    }
}

#[test]
fn native_quotes_escapes_names_and_raw_strings() {
    for (literal, expected) in [
        (r"'single\'quote'", "single'quote"),
        (r#""double\"quote""#, "double\"quote"),
        ("'''first\nsecond'''", "first\nsecond"),
        ("\"\"\"first\nsecond\"\"\"", "first\nsecond"),
        (r"r'\n\x41'", "\\n\\x41"),
        (r"u'\a\v\x41\101\u03B1\U0001F408'", "\u{7}\u{b}AAα🐈"),
        ("'first\\\nsecond'", "firstsecond"),
        (r"'\q'", "\\q"),
        (r"'\α\β'", "\\α\\β"),
        (
            r"'\N{LATIN SMALL LETTER A}\N{BOM}\N{HANGUL SYLLABLE GA}\N{CJK UNIFIED IDEOGRAPH-4E00}'",
            "a\u{feff}가一",
        ),
    ] {
        let input = format!("[f(text={literal})]");
        let calls = recover(&input);
        assert_eq!(calls.calls.len(), 1, "{literal}");
        let call = &calls.calls[0];
        assert_eq!(call.outcome, CallOutcome::Resolved, "{literal}");
        let value = call.arguments.get("text").expect("text");
        assert_eq!(value.as_str(), Some(expected), "{literal}");
        assert_eq!(&input[value.span.clone().expect("source span")], literal);
    }
}

#[test]
fn nested_literals_shield_call_delimiters_and_keep_types() {
    let calls = recover(
        r"[f(data={'text': ')], f(number=99)', 'items': [1, 2.50, True, False, None]}, number=.5)]",
    );
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    let expected = purrassist_core::json::parse(
        r#"{"data":{"text":")], f(number=99)","items":[1,2.50,true,false,null]},"number":0.5}"#,
    )
    .expect("expected");
    assert!(calls.calls[0].arguments.equivalent(&expected));
}

#[test]
fn python_numeric_spellings_are_exact_values() {
    for (literal, expected) in [
        ("0xFF", "255"),
        ("-0b101", "-5"),
        ("0o17", "15"),
        ("1_000", "1000"),
        ("1.", "1.0"),
        ("001.5", "1.5"),
        ("+2e3", "2e3"),
    ] {
        let calls = recover(&format!("[f(number={literal})]"));
        assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
        assert_eq!(
            calls.calls[0].arguments.get("number").unwrap().to_json(),
            expected
        );
    }
}

#[test]
fn positions_and_nonliteral_expressions_never_run() {
    use purrassist_core::tool_call::Rejection;
    let calls = recover("[f(42)]");
    assert_eq!(
        calls.usable_accepting_guesses(),
        <[&ToolCall; 0]>::default()
    );
    assert!(
        calls
            .rejected
            .iter()
            .any(|call| matches!(call.reason, Rejection::PythonicPositionalRejected { .. })),
        "{:?}",
        calls.rejected
    );
    for input in [
        "[f(number=1+2)]",
        "[f(data=other())]",
        "[f(data=[x for x in y])]",
        "[f(data={1: 'not a JSON key'})]",
        "[f(**mapping)]",
        "[f(class=1)]",
        "[f(text=f'{value}')]",
        "[f(text='\\N{KEYCAP DIGIT ONE}')]",
        "[f(text='\\uD800')]",
        "[f(text='unterminated)]",
        "[f(number=1e)]",
        "[f(number=.)]",
        "[f(number=-.)]",
        "[f(number=.e1)]",
    ] {
        assert_eq!(
            recover(input).usable_accepting_guesses(),
            <[&ToolCall; 0]>::default(),
            "{input}"
        );
    }
}

#[test]
fn only_admitted_contexts_produce_calls_and_fences_do_not_leak() {
    for input in [
        "[f(number=1)]",
        "Intro\n  [f(number=1)]",
        "Intro <|python_start|>[f(number=1)]",
        "<|python_tag|>[f(number=1)]",
    ] {
        let calls = recover(input);
        assert_eq!(calls.calls.len(), 1, "{input}");
        assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    }
    for input in [
        "Example: [f(number=1)]",
        "[1, 2, 3]",
        "```python\n[f(number=1)]\n```",
        "`example: <|python_start|>[f(number=1)]`",
        "> <|python_start|>[f(number=1)]",
        "```python\n[f(42)]\n```",
        "`unclosed prose <|python_start|>[f(number=1)]`",
    ] {
        let calls = recover(input);
        assert_eq!(calls.calls, <[ToolCall; 0]>::default(), "{input}");
        assert_eq!(calls.rejected, <[RejectedCall; 0]>::default(), "{input}");
        assert_eq!(
            calls.usable_accepting_guesses(),
            <[&ToolCall; 0]>::default(),
            "{input}"
        );
    }
    let input = "<think>\n```python\n[f(42)]\n[f(number=1)]\n```\n</think>";
    let mut governor = Governor::new(Budget::default());
    let set = DialectSet::built_in();
    let mut recognizer =
        Recognizer::new(set, ReasoningPolicy::default(), &mut governor).expect("recognizer");
    recognizer
        .push(input.as_bytes(), &mut governor)
        .expect("push");
    let turn = recognizer
        .finish(GenerationStop::Stop, &mut governor)
        .expect("turn");
    assert_eq!(
        turn.rejected_attempts(),
        <[(usize, DialectId); 0]>::default()
    );
    let recovered = tools().recover(input.as_bytes(), &ToolRecoveryRequest::default(), set);
    let Outcome::Resolved(calls) = recovered.outcome else {
        panic!("{recovered:?}")
    };
    assert_eq!(calls.calls, <[ToolCall; 0]>::default());
    assert_eq!(calls.rejected, <[RejectedCall; 0]>::default());
    let calls = recover("```python\n[f(42)]\n```\n[f(number=2)]");
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(
        calls.calls[0].arguments.get("number").unwrap().to_json(),
        "2"
    );
}
