// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! `OpenAI` chat-completions streaming logs reassembled into tool calls: the
//! same calls as the batch reading of the equivalent complete message, the
//! same result for every chunking, and explicit faults for logs that are
//! malformed, truncated or out of order.
//!
//! The logs here are written from the chunk shapes of the `OpenAI` chat
//! completions streaming reference (`chat.completion.chunk`); they are not
//! captured from a provider.
use purrassist_core::tool_call::{
    ChatFaultClass, ChatFaultKind, ChatLogRecovery, DialectSet, GenerationStop, Param, StreamEvent,
    ToolDefinition, ToolRecoveryRequest, ToolSet,
};
use purrassist_core::{Budget, Outcome, StopReason};

fn tools() -> ToolSet {
    ToolSet::compile(
        [ToolDefinition::new("read_file")
            .param("path", Param::string().required())
            .param("limit", Param::integer())],
        Budget::default(),
    )
    .expect("tools compile")
}

/// One server-sent event: a single data line and its blank line.
fn sse(data: &str) -> String {
    format!("data: {data}\n\n")
}

fn chunk(delta: &str, finish: &str) -> String {
    sse(&format!(
        r#"{{"id":"chatcmpl-1","object":"chat.completion.chunk","choices":[{{"index":0,"delta":{delta},"finish_reason":{finish}}}]}}"#
    ))
}

fn text(content: &str) -> String {
    chunk(&format!(r#"{{"content":"{content}"}}"#), "null")
}

fn call(index: u32, head: &str, function: &str) -> String {
    chunk(
        &format!(r#"{{"tool_calls":[{{"index":{index}{head},"function":{{{function}}}}}]}}"#),
        "null",
    )
}

fn args(index: u32, fragment: &str) -> String {
    let escaped = fragment.replace('\\', "\\\\").replace('"', "\\\"");
    call(index, "", &format!(r#""arguments":"{escaped}""#))
}

const HEAD: &str = r#","id":"call_1","type":"function""#;

/// The argument fragments, split inside a string and a number.
const FRAGMENTS: [&str; 3] = [r#"{"path": "sr"#, r#"c/main.rs", "li"#, r#"mit": 20}"#];

/// A call opened with a name split in two, then its arguments.
fn tool_call(index: u32) -> String {
    let mut log = call(index, HEAD, r#""name":"read_","arguments":"""#);
    log += &call(index, "", r#""name":"file""#);
    for fragment in FRAGMENTS {
        log += &args(index, fragment);
    }
    log
}

fn end(reason: &str) -> String {
    chunk("{}", &format!("\"{reason}\"")) + &sse("[DONE]")
}

/// Text, then one tool call, a usage chunk and a keep-alive comment: every
/// shape of chunk the reader assembles or passes over.
fn golden_log() -> String {
    chunk(r#"{"role":"assistant","content":""}"#, "null")
        + &text(r#"Reading \"main\"."#)
        + &tool_call(0)
        + ": keep-alive\n\n"
        + &end("tool_calls").replace(
            "data: [DONE]",
            r#"data: {"choices":[],"usage":{"total_tokens":9}}

data: [DONE]"#,
        )
}

/// The complete message the golden log describes.
const GOLDEN_MESSAGE: &str = concat!(
    r#"{"role":"assistant","content":"Reading \"main\".","#,
    r#""tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_file","#,
    r#""arguments":"{\"path\": \"src/main.rs\", \"limit\": 20}"}}]}"#
);

fn recover(log: &str) -> ChatLogRecovery {
    tools().recover_openai_chat(
        log.as_bytes(),
        ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    )
}

fn kinds(result: &ChatLogRecovery) -> Vec<ChatFaultKind> {
    result.faults.iter().map(|fault| fault.kind).collect()
}

/// A well-formed log is read as exactly the call the batch reading of its
/// equivalent message gives: same outcome, repairs, losses and receipt
/// evidence, and the receipt keeps the reassembled message while the result
/// keeps the log.
#[test]
fn golden_log_matches_batch_reading_of_the_equivalent_message() {
    let log = golden_log();
    let result = recover(&log);
    assert!(result.is_clean(), "{:#?}", result.faults);
    assert_eq!(result.log, log.as_bytes());
    let Outcome::Resolved(calls) = &result.recovery.outcome else {
        panic!("{:#?}", result.recovery.outcome);
    };
    assert_eq!(calls.calls.len(), 1);
    let call = &calls.calls[0];
    assert_eq!(call.tool.as_deref(), Some("read_file"));
    assert_eq!(call.source_id.as_deref(), Some("call_1"));
    assert_eq!(
        call.arguments.to_json(),
        r#"{"path":"src/main.rs","limit":20}"#
    );
    assert_eq!(result.index_of(call), Some(0));
    assert_eq!(result.usable().len(), 1);
    assert_eq!(
        result.recovery.receipt.sources[0].bytes.as_slice(),
        GOLDEN_MESSAGE.as_bytes()
    );

    let request = ToolRecoveryRequest::default().stop(GenerationStop::ToolCalls);
    let batch = tools().recover(GOLDEN_MESSAGE.as_bytes(), &request, DialectSet::built_in());
    assert_eq!(result.recovery.outcome, batch.outcome);
    let (left, right) = (&result.recovery.receipt, &batch.receipt);
    assert_eq!(left.source, right.source);
    assert_eq!(left.evidence, right.evidence);
    assert_eq!(left.losses, right.losses);
    assert_eq!(left.inferences, right.inferences);
    assert_eq!(left.validations, right.validations);
}

/// Everything observable except resource usage, which varies with feeding.
fn observed(result: &ChatLogRecovery, events: &[StreamEvent]) -> String {
    let receipt = &result.recovery.receipt;
    format!(
        "{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}",
        result.recovery.outcome,
        result.faults,
        result.calls,
        receipt.evidence,
        receipt.losses,
        receipt.inferences,
        events
    )
}

fn chunked(log: &[u8], cuts: &[usize]) -> String {
    let tools = tools();
    let mut stream =
        tools.stream_openai_chat(ToolRecoveryRequest::default(), DialectSet::built_in());
    let mut events = Vec::new();
    let mut from = 0;
    for &cut in cuts.iter().chain(&[log.len()]) {
        stream.feed(&log[from..cut], &mut events);
        from = cut;
    }
    let result = stream.finish(&mut events);
    assert_eq!(result.log, log);
    observed(&result, &events)
}

/// The result, the faults and the recognizer's events do not depend on where
/// the log is split, including between the CR and LF of a line end, inside a
/// name or arguments fragment, and inside a faulted chunk.
#[test]
fn every_split_of_a_chat_log_gives_the_same_result() {
    let clean = golden_log();
    let crlf = clean.replace('\n', "\r\n");
    let faulted = tool_call(0) + &args(1, r#"{"path": "b"}"#) + &args(0, "}");
    for log in [clean, crlf, faulted] {
        let bytes = log.as_bytes();
        let whole = chunked(bytes, &[]);
        for cut in 0..=bytes.len() {
            assert_eq!(chunked(bytes, &[cut]), whole, "split at {cut}");
        }
        let singles: Vec<usize> = (1..bytes.len()).collect();
        assert_eq!(chunked(bytes, &singles), whole, "one byte at a time");
    }
}

/// A fault is reported with its class, its call and its position, a valid
/// neighbour has none, and no call it belongs to is ever offered as usable
/// (the call is still read from the message the valid chunks built).
fn negative(valid: &str, faulted: &str, kind: ChatFaultKind, class: ChatFaultClass) {
    let good = recover(valid);
    assert!(good.is_clean(), "{:#?}", good.faults);
    assert_eq!(good.usable().len(), 1, "{:#?}", good.recovery.outcome);

    let bad = recover(faulted);
    assert!(kinds(&bad).contains(&kind), "{:#?}", bad.faults);
    assert_eq!(kind.class(), class);
    assert!(!bad.is_clean());
    assert_eq!(bad.usable().len(), 0);
    assert_eq!(bad.usable_accepting_guesses().len(), 0);
    assert_eq!(bad.log, faulted.as_bytes());
}

/// A chunk whose data is not JSON is a malformed fault of the whole log; the
/// call beside it, whose chunks are valid, is not offered.
#[test]
fn malformed_chunk_is_an_explicit_fault() {
    let valid = tool_call(0) + &end("tool_calls");
    let faulted = tool_call(0) + &sse(r#"{"choices":[{"index":0,"#) + &end("tool_calls");
    negative(
        &valid,
        &faulted,
        ChatFaultKind::NotJson,
        ChatFaultClass::Malformed,
    );
    assert_eq!(recover(&faulted).faults[0].call, None);
}

/// A log that stops before its finish reason and `[DONE]` is truncated and
/// never usable, unlike the same bytes followed by them; each missing marker
/// is reported on its own, and a chunk cut before its blank line is reported.
#[test]
fn truncated_log_is_an_explicit_fault() {
    let valid = tool_call(0) + &end("tool_calls");
    let cut = tool_call(0);
    negative(
        &valid,
        &cut,
        ChatFaultKind::MissingFinishReason,
        ChatFaultClass::Truncated,
    );
    assert_eq!(
        kinds(&recover(&cut)),
        [
            ChatFaultKind::MissingDone,
            ChatFaultKind::MissingFinishReason
        ]
    );
    let no_done = tool_call(0) + &chunk("{}", r#""tool_calls""#);
    assert_eq!(kinds(&recover(&no_done)), [ChatFaultKind::MissingDone]);
    let no_finish = tool_call(0) + &sse("[DONE]");
    assert_eq!(
        kinds(&recover(&no_finish)),
        [ChatFaultKind::MissingFinishReason]
    );
    let partial = valid.trim_end_matches('\n').to_owned();
    let bad = recover(&partial);
    assert!(kinds(&bad).contains(&ChatFaultKind::PartialEvent));
    assert_eq!(bad.usable().len(), 0);
}

/// A fragment for a call after a later call has started is not applied, even
/// though its bytes would complete the arguments; the anomaly stays reported
/// against that call, unlike the same fragment before the later call starts.
#[test]
fn out_of_order_fragment_for_a_closed_call_is_an_explicit_fault() {
    let open = call(
        0,
        HEAD,
        r#""name":"read_file","arguments":"{\"path\": \"a\"""#,
    );
    let next = call(
        1,
        r#","id":"call_2","type":"function""#,
        r#""name":"read_file","arguments":"{\"path\": \"b\"}""#,
    );
    let valid = open.clone() + &args(0, "}") + &end("tool_calls");
    let faulted = open + &next + &args(0, "}") + &end("tool_calls");
    let bad = recover(&faulted);
    assert_eq!(
        (bad.faults[0].kind, bad.faults[0].call, bad.faults[0].event),
        (ChatFaultKind::DeltaForClosedCall, Some(0), 2)
    );
    assert_eq!(
        ChatFaultKind::DeltaForClosedCall.class(),
        ChatFaultClass::OutOfOrder
    );
    let good = recover(&valid);
    assert!(good.is_clean(), "{:#?}", good.faults);
    assert_eq!(good.usable().len(), 1);
}

/// A call is faulted on its own: the clean call beside a faulted one stays
/// usable, while the faulted call is not offered even though the message
/// reads it as a complete call.
#[test]
fn clean_call_stays_usable_beside_a_faulted_one() {
    let faulted = call(
        0,
        HEAD,
        r#""name":"read_file","arguments":"{\"path\": \"a\"}""#,
    ) + &call(
        1,
        r#","id":"call_2","type":"function""#,
        r#""name":"read_file","arguments":"{\"path\": \"b\"}""#,
    ) + &args(0, " ")
        + &end("tool_calls");
    let result = recover(&faulted);
    assert_eq!(kinds(&result), [ChatFaultKind::DeltaForClosedCall]);
    let Outcome::Resolved(calls) = &result.recovery.outcome else {
        panic!("{:#?}", result.recovery.outcome);
    };
    assert_eq!(calls.calls.len(), 2);
    let usable = result.usable();
    assert_eq!(usable.len(), 1);
    assert_eq!(usable[0].source_id.as_deref(), Some("call_2"));
    assert_eq!(result.index_of(usable[0]), Some(1));
}

/// A governor refusal is an explicit `Unresolved` result that names the
/// exhausted resource in the receipt and retains no log; it is never a
/// clean empty turn.
#[test]
fn governor_refusal_is_an_explicit_unresolved_outcome_with_no_log() {
    let request = ToolRecoveryRequest::default().budget(Budget {
        work: 64,
        ..Budget::default()
    });
    let log = golden_log();
    let result = tools().recover_openai_chat(log.as_bytes(), request, DialectSet::built_in());
    assert!(matches!(result.recovery.outcome, Outcome::Unresolved));
    assert!(matches!(
        result.recovery.receipt.stopped,
        StopReason::BudgetExceeded(_)
    ));
    assert_eq!(result.log.len(), 0);
    assert_eq!(result.usable().len(), 0);
}
