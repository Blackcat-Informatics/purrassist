// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Gemma fences use native literals and preserve Markdown/result boundaries.
use purrassist_core::grammar::CaptureKind;
use purrassist_core::tool_call::{
    CallOutcome, DialectId, DialectSet, GenerationStop, Piece, ReasoningPolicy, Recognizer,
    RejectedCall, Segment, ToolCall, ToolCalls, ToolRecoveryRequest, ToolSet,
};
use purrassist_core::{Budget, Governor, Outcome};

fn tools() -> ToolSet {
    ToolSet::from_openai(
        r#"[{"type":"function","function":{"name":"f","parameters":{"type":"object","properties":{"text":{"type":"string"},"data":{}},"additionalProperties":false}}}]"#,
        Budget::default(),
    )
    .expect("tools")
}
fn dialects() -> DialectSet {
    DialectSet::new(
        [DialectId::Gemma3ToolCode],
        &mut Governor::new(Budget::default()),
    )
    .expect("dialect")
}
fn recover(input: &str) -> ToolCalls {
    match tools()
        .recover(
            input.as_bytes(),
            &ToolRecoveryRequest::default(),
            &dialects(),
        )
        .outcome
    {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
        other => panic!("{input:?}: {other:?}"),
    }
}

#[test]
fn quoted_fence_text_is_an_argument_and_fenced_calls_keep_order() {
    let literal = "'''first\n```\nsecond'''";
    let input = format!(
        "```tool_code\nf(text={literal}, data={{'items': [True, None, 2.50]}})\n```\n```tool_code\nf(text='next')\n```"
    );
    let calls = recover(&input);
    assert_eq!(calls.calls.len(), 2);
    for call in &calls.calls {
        assert_eq!(call.outcome, CallOutcome::Resolved);
    }
    let text = calls.calls[0].arguments.get("text").expect("text");
    assert_eq!(text.as_str(), Some("first\n```\nsecond"));
    assert_eq!(&input[text.span.clone().expect("source span")], literal);
    assert_eq!(
        calls.calls[1].arguments.get("text").unwrap().as_str(),
        Some("next")
    );

    let calls = recover("```tool_code\nf(text='before')\n```tool_code\nf(text='after')\n```");
    assert_eq!(
        calls.calls.len(),
        2,
        "missing fence preserves the next call"
    );
    assert_eq!(
        calls.calls[0].arguments.get("text").unwrap().as_str(),
        Some("before")
    );
    assert_eq!(
        calls.calls[1].arguments.get("text").unwrap().as_str(),
        Some("after")
    );
}

#[test]
fn ordinary_examples_stay_content_without_masking_the_following_call() {
    for input in [
        "```python\nf(text='example')\n```",
        "````python\n```tool_code\nf(text='example')\n```\n````",
        "````python\n```tool_code\nf(42)\n```\n````",
        "Example: ```tool_code\nf(text='example')\n```",
    ] {
        let calls = recover(input);
        assert_eq!(calls.calls, <[ToolCall; 0]>::default(), "{input}");
        assert_eq!(calls.rejected, <[RejectedCall; 0]>::default(), "{input}");
    }
    let calls =
        recover("````python\n```tool_code\nf(42)\n```\n````\n```tool_code\nf(text='actual')\n```");
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(
        calls.calls[0].arguments.get("text").unwrap().as_str(),
        Some("actual")
    );
}

#[test]
fn tool_output_is_a_result_with_verbatim_content() {
    let source = b"```tool_output\nf(text='returned ``` text')\n```";
    let mut governor = Governor::new(Budget::default());
    let set = dialects();
    let mut recognizer =
        Recognizer::new(&set, ReasoningPolicy::default(), &mut governor).expect("recognizer");
    recognizer.push(source, &mut governor).expect("push");
    let turn = recognizer
        .finish(GenerationStop::Stop, &mut governor)
        .expect("turn");
    let [Piece::Segment(Segment::Result(result))] = turn.pieces() else {
        panic!("expected one result: {:?}", turn.pieces())
    };
    assert_eq!(
        result.readings[0].capture_text(CaptureKind::Content, source),
        Some("f(text='returned ``` text')")
    );
    let calls = recover(core::str::from_utf8(source).expect("UTF-8 source"));
    assert_eq!(calls.calls, <[ToolCall; 0]>::default());
    assert_eq!(calls.content, [0..source.len()]);
}
