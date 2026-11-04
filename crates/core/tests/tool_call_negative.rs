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
use purrassist_core::contract::Resource;
use purrassist_core::digest::Digest;
use purrassist_core::grammar::Grammar;
use purrassist_core::tool_call::{
    CallOutcome, DialectId, DialectSet, Effects, Finish, GenerationStop, Param, ReasoningPolicy,
    Rejection, RepairPolicy, StreamEvent, ToolCalls, ToolChoice, ToolDefinition,
    ToolRecoveryRequest, ToolSet, UnresolvedReason,
};
use purrassist_core::{AssistResult, Budget, CancellationToken, Governor, Outcome, StopReason};

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
        assert_eq!(calls.usable_accepting_guesses().len(), 0);
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

/// Compare complete results and source-ordered content events at every split.
/// Competing argument readings withhold later content through EOF; a closed
/// native frame publishes its whole tail after its reports have settled.
fn stream_content(
    tools: &ToolSet,
    dialects: &DialectSet,
    input: &str,
    request: &ToolRecoveryRequest,
    tail: usize,
    live: bool,
) -> AssistResult<ToolCalls> {
    let batch = tools.recover(input.as_bytes(), request, dialects);
    for split in 0..=input.len() {
        let mut stream = tools.stream_with_dialects(request.clone(), dialects);
        let mut events = Vec::new();
        stream.feed(&input.as_bytes()[..split], &mut events);
        stream.feed(&input.as_bytes()[split..], &mut events);
        assert!(stream.held_bytes() <= stream.hold_back_bound());
        if live {
            assert_eq!(
                stream.candidate_bytes(),
                tail,
                "finite frame coverage at split {split}"
            );
        }
        let live_end = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ContentDelta { span, .. } => Some(span.end),
                _ => None,
            })
            .next_back();
        if live {
            assert_eq!(
                live_end,
                Some(input.len()),
                "closed frame withheld its settled content at split {split}"
            );
        } else {
            assert!(live_end.is_none(), "repaired frame at split {split}");
        }
        let result = stream.finish_with_events(GenerationStop::Stop, &mut events);
        assert_eq!(result, batch, "whole result differs at split {split}");
        if !live {
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, StreamEvent::CallComplete { .. })),
                "competing argument readings completed at split {split}"
            );
        }
        let mut next = tail;
        for event in events {
            if let StreamEvent::ContentDelta { span, raw } = event {
                assert_eq!(span.start, next, "content order at split {split}");
                assert!(span.start < span.end && span.end <= input.len());
                assert_eq!(raw.as_slice(), &input.as_bytes()[span.clone()]);
                next = span.end;
            }
        }
        assert_eq!(
            next,
            if live { input.len() } else { tail },
            "content coverage at split {split}"
        );
    }
    batch
}

/// Repeated closers after an ambiguous body tail remain markup; only the
/// prose after them stays batch content. Streaming withholds it after competing
/// argument readings and publishes a closed native frame's settled tail.
#[test]
fn stray_closers_after_ambiguous_body_tail() {
    let input = "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\nthat is all</function>\n</tool_call>\n</tool_call> tail";
    let tail = input.len() - " tail".len();
    assert_ambiguous_tail(input, "x\n</parameter>\nthat is all");
    for effects in PROFILES {
        let tools = tools(effects);
        let dialects = DialectSet::new(
            [DialectId::Qwen3CoderXml],
            &mut Governor::new(Budget::default()),
        )
        .expect("dialect set");
        let request = ToolRecoveryRequest::default();
        let result = stream_content(&tools, &dialects, input, &request, tail, false);
        let calls = match &result.outcome {
            Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
            other => panic!("{effects:?}: expected one reading, got {other:?}"),
        };
        assert_never_usable(input, calls);
        assert_no_markup_in_content(input, calls);
        assert!(calls.rejected.is_empty(), "{effects:?}");
        assert_eq!(calls.content, vec![tail..input.len()], "{effects:?}");
        // Qwen's verbatim body can still read past its canonical closer.
        // A self-delimiting Hermes JSON body supplies a settled native-frame
        // control, extended beyond its own compiled settlement bound.
        let settled_dialects =
            DialectSet::new([DialectId::Hermes], &mut Governor::new(Budget::default()))
                .expect("settled dialect set");
        let frame = r#"<tool_call>{"name":"f","arguments":{"a":"x"}}</tool_call>"#;
        let closed = format!(
            "{frame} tail{}",
            " ".repeat(settled_dialects.lookbehind() + 1)
        );
        stream_content(
            &tools,
            &settled_dialects,
            &closed,
            &request,
            frame.len(),
            true,
        );
    }
    let tools = tools(Effects::ReadOnly);
    lexical_content_frontier(&tools);
    uncommitted_candidate_frontier(&tools);
    stream_holdback_ownership(&tools);
}

/// Plain content depends on live opener/marker prefixes and UTF-8 carry,
/// independently of an unrelated long closer's repair settlement window.
fn lexical_content_frontier(tools: &ToolSet) {
    let closer = "Z".repeat(256);
    let grammar = format!(
        r#"grammar "lexical-frontier.v1";
        reasoning "<think>" "</think>";
        closer end = "{closer}";
        ident = /[A-Za-z_][A-Za-z0-9_]*/;
        call = "<c>" name:ident ":" arguments:slot(json, "\x00") end;
        result = "<r>" content:until("</r>") "</r>";
        declaration = "<d>";"#
    );
    let dialect = DialectId::custom(
        Grammar::compile_governed(&grammar, &mut Governor::new(Budget::default()))
            .expect("lexical grammar"),
    );
    let dialects =
        DialectSet::new([dialect], &mut Governor::new(Budget::default())).expect("dialects");
    let input = "plain <thiX <cX 😺";
    assert!(dialects.lookbehind() > input.len());
    for reasoning in [
        ReasoningPolicy::Separate { starts_open: false },
        ReasoningPolicy::AsContent,
    ] {
        let request = ToolRecoveryRequest::default().reasoning(reasoning);
        let batch = stream_content(tools, &dialects, input, &request, 0, true);
        let mut stream = tools.stream_with_dialects(request, &dialects);
        let mut events = Vec::new();
        let (mut previous, mut content) = (0, Vec::new());
        let marker_end = if reasoning == ReasoningPolicy::AsContent {
            10
        } else {
            6
        };
        for (end, published) in [
            (1, 1),
            (2, 2),
            (3, 3),
            (4, 4),
            (5, 5),
            (6, 6),
            (10, marker_end),
            (11, 11),
            (12, 12),
            (14, 12),
            (15, 15),
            (16, 16),
            (17, 16),
            (18, 16),
            (19, 16),
            (20, 20),
        ] {
            stream.feed(&input.as_bytes()[previous..end], &mut events);
            previous = end;
            for event in &events {
                let StreamEvent::ContentDelta { span, raw } = event else {
                    panic!("plain prefix produced a non-content event");
                };
                assert_eq!(span, &(content.len()..content.len() + raw.len()));
                content.extend_from_slice(raw);
            }
            events.clear();
            assert_eq!(
                content,
                input.as_bytes()[..published],
                "prefix ending at {end}"
            );
            assert_eq!(stream.candidate_bytes(), 0);
            assert_eq!(stream.held_bytes(), end - published);
            assert!(stream.held_bytes() <= stream.hold_back_bound());
        }
        assert_eq!(stream.finish(GenerationStop::Stop), batch);
    }
}

/// Completed initial delimiters own uncommitted argument bytes even when
/// their discriminating key or closing literal has not arrived.
fn uncommitted_candidate_frontier(tools: &ToolSet) {
    for dialect in [DialectId::BareJson, DialectId::Pythonic] {
        let dialects = DialectSet::new([dialect.clone()], &mut Governor::new(Budget::default()))
            .expect("candidate dialect");
        let body = "x".repeat(dialects.lookbehind() + 16);
        let input = if dialect == DialectId::BareJson {
            format!(r#"{{"arguments":{{"a":"{body}"}},"na"#)
        } else {
            format!("[f(a='{body}")
        };
        let request = ToolRecoveryRequest::default();
        let batch = tools.recover(input.as_bytes(), &request, &dialects);
        for split in 0..=input.len() {
            let mut stream = tools.stream_with_dialects(request.clone(), &dialects);
            let mut events = Vec::new();
            stream.feed(&input.as_bytes()[..split], &mut events);
            stream.feed(&input.as_bytes()[split..], &mut events);
            assert_eq!(
                stream.candidate_bytes(),
                input.len(),
                "late {dialect:?} discriminator at split {split}"
            );
            assert_eq!(stream.held_bytes(), 0);
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, StreamEvent::ContentDelta { .. }))
            );
            assert_eq!(
                stream.finish_with_events(GenerationStop::Stop, &mut events),
                batch
            );
        }
    }
}

/// D7 ownership follows actual prefixes, semantic frames and mutable stop observation.
fn ownership_dialects(name: &str, extra: &str, call: &str) -> DialectSet {
    let text = format!(r#"grammar "{name}"; {extra} {call} result = "R"; declaration = "D";"#);
    let dialect = DialectId::custom(
        Grammar::compile_governed(&text, &mut Governor::new(Budget::default()))
            .expect("ownership grammar"),
    );
    DialectSet::new([dialect], &mut Governor::new(Budget::default())).expect("ownership dialects")
}

fn stream_holdback_ownership(tools: &ToolSet) {
    stream_encoding_ownership(tools);
    stream_marker_ownership(tools);
    stream_candidate_handoffs(tools);
    stream_owner_disposal(tools);
}

fn stream_encoding_ownership(tools: &ToolSet) {
    let ascii = ownership_dialects("ownership-ascii.v1", "", r#"call = "C";"#);
    let request = ToolRecoveryRequest::default();
    let scalar = "😺".as_bytes();
    let batch = tools.recover(scalar, &request, &ascii);
    for split in 0..=scalar.len() {
        let mut stream = tools.stream_with_dialects(request.clone(), &ascii);
        assert_eq!(stream.hold_back_bound(), 3);
        let _ = stream.feed_collect(&scalar[..split]);
        assert_eq!(
            stream.held_bytes(),
            if split == scalar.len() { 0 } else { split }
        );
        assert_eq!(stream.candidate_bytes(), 0);
        // Queries and empty feeds must not change result or whole usage.
        let _ = (
            stream.held_bytes(),
            stream.candidate_bytes(),
            stream.hold_back_bound(),
        );
        let _ = stream.feed_collect(&[]);
        let _ = stream.feed_collect(&scalar[split..]);
        assert_eq!(stream.held_bytes(), 0);
        assert_eq!(stream.finish(GenerationStop::Stop), batch);
    }
    let invalid = [0xf0, 0x9f, b'x'];
    let mut stream = tools.stream_with_dialects(request.clone(), &ascii);
    let _ = stream.feed_collect(&invalid[..2]);
    assert_eq!(stream.held_bytes(), 2);
    let _ = stream.feed_collect(&invalid[2..]);
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (0, 0));
    assert_eq!(
        stream.finish(GenerationStop::Stop),
        tools.recover(&invalid, &request, &ascii)
    );
}

fn stream_marker_ownership(tools: &ToolSet) {
    let request = ToolRecoveryRequest::default();
    let scalar = "😺".as_bytes();
    let unicode = ownership_dialects(
        "ownership-unicode.v1",
        r#"reasoning "😺Z" "!";"#,
        r#"call = "C";"#,
    );
    let mut stream = tools.stream_with_dialects(request.clone(), &unicode);
    assert_eq!(stream.hold_back_bound(), 4);
    let _ = stream.feed_collect(&scalar[..3]);
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (3, 0));
    let _ = stream.feed_collect(&scalar[3..]);
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (4, 0));
    let _ = stream.feed_collect(b"?");
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (0, 0));

    let bytes = ownership_dialects(
        "ownership-byte-marker.v1",
        r#"reasoning "\x80ABCDEFZ" "!";"#,
        r#"call = "<c>";"#,
    );
    let input = "😀ABCDEF?".as_bytes();
    let batch = tools.recover(input, &request, &bytes);
    for split in 0..=input.len() {
        let mut stream = tools.stream_with_dialects(request.clone(), &bytes);
        assert_eq!(stream.hold_back_bound(), 10);
        let _ = stream.feed_collect(&input[..split]);
        assert!(stream.held_bytes() <= 10);
        if split == input.len() - 1 {
            assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (10, 0));
        }
        let _ = stream.feed_collect(&input[split..]);
        assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (0, 0));
        assert_eq!(stream.finish(GenerationStop::Stop), batch);
    }
}

fn stream_candidate_handoffs(tools: &ToolSet) {
    let request = ToolRecoveryRequest::default();
    let phases = ownership_dialects(
        "ownership-phase-loss.v1",
        "",
        r#"x = /X/; call = ("<a>" name:x | "<a>LONG") commit;"#,
    );
    let mut stream = tools.stream_with_dialects(request.clone(), &phases);
    let _ = stream.feed_collect(b"<a>");
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (0, 3));
    let _ = stream.feed_collect(b"L");
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (4, 0));
    let _ = stream.feed_collect(b"ONG");
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (0, 7));
    assert_eq!(
        stream.finish(GenerationStop::Stop),
        tools.recover(b"<a>LONG", &request, &phases)
    );

    let reasoning = ownership_dialects(
        "ownership-reasoning.v1",
        r#"reasoning "<t>" "</t>";"#,
        r#"call = "C";"#,
    );
    for starts_open in [false, true] {
        let request = request
            .clone()
            .reasoning(ReasoningPolicy::Separate { starts_open });
        let thought = format!(
            "{}{}",
            if starts_open { "" } else { "<t>" },
            "thought ".repeat(32)
        );
        let suffix = format!("</t>{}", " ".repeat(reasoning.lookbehind() + 1));
        let input = format!("{thought}{suffix}");
        let mut stream = tools.stream_with_dialects(request.clone(), &reasoning);
        let _ = stream.feed_collect(thought.as_bytes());
        assert_eq!(
            (stream.held_bytes(), stream.candidate_bytes()),
            (0, thought.len())
        );
        let events = stream.feed_collect(suffix.as_bytes());
        assert!(
            events
                .iter()
                .any(|event| matches!(event, StreamEvent::ReasoningDelta { .. }))
        );
        assert_eq!(
            (stream.held_bytes(), stream.candidate_bytes()),
            (0, suffix.len() - 4)
        );
        assert_eq!(
            stream.finish(GenerationStop::Stop),
            tools.recover(input.as_bytes(), &request, &reasoning)
        );
    }
}

fn stream_owner_disposal(tools: &ToolSet) {
    let request = ToolRecoveryRequest::default();
    let ascii = ownership_dialects("ownership-ascii.v1", "", r#"call = "C";"#);
    let scalar = "😺".as_bytes();
    let token = CancellationToken::new();
    let governor = Governor::with_cancellation(Budget::default(), token.clone());
    let mut stream = tools.stream_with_dialects_governed(request.clone(), &ascii, governor);
    let _ = stream.feed_collect(&scalar[..3]);
    token.cancel();
    assert_eq!(
        stream.held_bytes(),
        3,
        "request alone has not disposed owners"
    );
    let _ = stream.feed_collect(&[]);
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (0, 0));
    let cancelled = stream.finish(GenerationStop::Stop);
    assert_eq!(cancelled.receipt.stopped, StopReason::Cancelled);
    assert_eq!(cancelled.receipt.sources[0].bytes, scalar[..3]);

    let mut stream = tools.stream_with_dialects(
        request.clone().budget(Budget {
            input_bytes: 1,
            ..Budget::default()
        }),
        &ascii,
    );
    let _ = stream.feed_collect(&scalar[..3]);
    stream.cancel();
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (0, 0));
    let refused = stream.finish(GenerationStop::Stop);
    assert_eq!(
        refused.receipt.stopped,
        StopReason::BudgetExceeded(Resource::InputBytes)
    );
    assert_eq!(refused.receipt.source, Digest::of(&scalar[..3]));
    assert_eq!(
        refused.receipt.sources,
        [] as [purrassist_core::contract::SourceDocument; 0]
    );

    let mut stream = tools.stream_with_dialects(
        request.clone().budget(Budget {
            scratch_bytes: 0,
            ..Budget::default()
        }),
        &ascii,
    );
    assert_eq!(stream.hold_back_bound(), 3);
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (0, 0));
    let _ = stream.feed_collect(b"source");
    assert_eq!(
        stream.finish(GenerationStop::Stop).receipt.source,
        Digest::of(b"source")
    );

    let invalid_request = request.tool_choice(ToolChoice::Forced("absent".into()));
    let mut stream = tools.stream_with_dialects(invalid_request, &ascii);
    let _ = stream.feed_collect(b"source");
    assert_eq!((stream.held_bytes(), stream.candidate_bytes()), (0, 0));
    let invalid = stream.finish(GenerationStop::Stop);
    assert!(matches!(
        invalid.receipt.stopped,
        StopReason::InvalidInput(_)
    ));
    assert_eq!(invalid.receipt.sources[0].bytes, b"source");
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
