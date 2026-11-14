// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Anthropic Messages streaming event logs reassembled into tool calls: the
//! same calls as the batch reading of the equivalent complete message, the
//! same result for every chunking, and explicit faults for logs that are
//! malformed, truncated or out of order.
//!
//! The logs here are written from the event shapes of the Anthropic Messages
//! streaming reference and Python SDK types (`raw_content_block_start_event`,
//! `input_json_delta`); they are not captured from a provider.
use purrassist_core::tool_call::{
    DialectSet, EventFaultClass, EventFaultKind, EventLogRecovery, GenerationStop, Param,
    StreamEvent, ToolDefinition, ToolRecoveryRequest, ToolSet,
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

/// One server-sent event: its name line and its data line.
fn sse(name: &str, data: &str) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

fn start(index: u32, block: &str) -> String {
    sse(
        "content_block_start",
        &format!(r#"{{"type":"content_block_start","index":{index},"content_block":{block}}}"#),
    )
}

fn delta(index: u32, body: &str) -> String {
    sse(
        "content_block_delta",
        &format!(r#"{{"type":"content_block_delta","index":{index},"delta":{body}}}"#),
    )
}

fn json_delta(index: u32, fragment: &str) -> String {
    let escaped = fragment.replace('\\', "\\\\").replace('"', "\\\"");
    delta(
        index,
        &format!(r#"{{"type":"input_json_delta","partial_json":"{escaped}"}}"#),
    )
}

fn stop(index: u32) -> String {
    sse(
        "content_block_stop",
        &format!(r#"{{"type":"content_block_stop","index":{index}}}"#),
    )
}

fn head() -> String {
    sse(
        "message_start",
        r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[]}}"#,
    )
}

fn tail(reason: &str) -> String {
    sse(
        "message_delta",
        &format!(r#"{{"type":"message_delta","delta":{{"stop_reason":"{reason}"}}}}"#),
    ) + &sse("message_stop", r#"{"type":"message_stop"}"#)
}

const TOOL_USE: &str = r#"{"type":"tool_use","id":"toolu_01","name":"read_file","input":{}}"#;

/// The tool block's argument fragments, split inside a string and a number.
const FRAGMENTS: [&str; 4] = ["", r#"{"path": "sr"#, r#"c/main.rs", "li"#, r#"mit": 20}"#];

fn tool_block(index: u32) -> String {
    let mut text = start(index, TOOL_USE);
    for fragment in FRAGMENTS {
        text += &json_delta(index, fragment);
    }
    text + &stop(index)
}

/// Thinking, text, then one tool call: every block kind the reader assembles.
fn golden_log() -> String {
    head()
        + &start(0, r#"{"type":"thinking","thinking":"","signature":""}"#)
        + &delta(
            0,
            r#"{"type":"thinking_delta","thinking":"Need the file."}"#,
        )
        + &delta(0, r#"{"type":"signature_delta","signature":"EqQB"}"#)
        + &stop(0)
        + &start(1, r#"{"type":"text","text":""}"#)
        + &delta(1, r#"{"type":"text_delta","text":"Reading \"main\"."}"#)
        + &stop(1)
        + &tool_block(2)
        + &sse("ping", r#"{"type":"ping"}"#)
        + &tail("tool_use")
}

/// The complete message the golden log describes.
const GOLDEN_MESSAGE: &str = concat!(
    r#"{"role":"assistant","content":["#,
    r#"{"type":"thinking","thinking":"Need the file.","signature":"EqQB"},"#,
    r#"{"type":"text","text":"Reading \"main\"."},"#,
    r#"{"type":"tool_use","id":"toolu_01","name":"read_file","input":{"path": "src/main.rs", "limit": 20}}"#,
    "]}"
);

fn recover(log: &str) -> EventLogRecovery {
    tools().recover_anthropic_events(
        log.as_bytes(),
        ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    )
}

fn kinds(result: &EventLogRecovery) -> Vec<EventFaultKind> {
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
    assert_eq!(call.source_id.as_deref(), Some("toolu_01"));
    assert_eq!(
        call.arguments.to_json(),
        r#"{"path":"src/main.rs","limit":20}"#
    );
    assert_eq!(result.block_of(call), Some(2));
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
fn observed(result: &EventLogRecovery, events: &[StreamEvent]) -> String {
    let receipt = &result.recovery.receipt;
    format!(
        "{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}\n{:#?}",
        result.recovery.outcome,
        result.faults,
        result.blocks,
        receipt.evidence,
        receipt.losses,
        receipt.inferences,
        events
    )
}

fn chunked(log: &[u8], cuts: &[usize]) -> String {
    let tools = tools();
    let mut stream =
        tools.stream_anthropic_events(ToolRecoveryRequest::default(), DialectSet::built_in());
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
/// the log is split, including between the CR and LF of a line end and inside
/// a faulted event.
#[test]
fn every_split_of_an_event_log_gives_the_same_result() {
    let clean = golden_log();
    let crlf = clean.replace('\n', "\r\n");
    let faulted = head()
        + &start(0, TOOL_USE)
        + &json_delta(0, r#"{"path": "a""#)
        + &stop(0)
        + &json_delta(0, "}");
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

/// A fault is reported with its class, its block and its position, a valid
/// neighbour has none, and no call of a faulted block is ever offered as usable
/// (the call is still read from the message the valid events built).
fn negative(valid: &str, faulted: &str, kind: EventFaultKind, class: EventFaultClass) {
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

/// An event whose data is not JSON is a malformed fault; the tool block
/// beside it, whose events are valid, is read as usual.
#[test]
fn malformed_event_is_an_explicit_fault() {
    let valid = head() + &tool_block(0) + &tail("tool_use");
    let faulted = head()
        + &tool_block(0)
        + &sse(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"#,
        )
        + &tail("tool_use");
    negative(
        &valid,
        &faulted,
        EventFaultKind::NotJson,
        EventFaultClass::Malformed,
    );
    // The fault belongs to the whole log: the intact block is not offered.
    let bad = recover(&faulted);
    assert_eq!(bad.faults[0].block, None);
}

/// A log that stops before its blocks and message end is truncated: the cut
/// block is closed by the recovery's own repairs, reported as open, and never
/// usable, unlike the same bytes followed by their stop events.
#[test]
fn truncated_log_is_an_explicit_fault() {
    let valid = head() + &tool_block(0) + &tail("tool_use");
    let cut =
        head() + &start(0, TOOL_USE) + &json_delta(0, FRAGMENTS[1]) + &json_delta(0, FRAGMENTS[2]);
    negative(
        &valid,
        &cut,
        EventFaultKind::OpenBlock,
        EventFaultClass::Truncated,
    );
    let bad = recover(&cut);
    assert!(kinds(&bad).contains(&EventFaultKind::MissingMessageStop));

    // An event cut before its blank line is discarded and reported.
    let partial = valid.trim_end_matches('\n').to_owned();
    let bad = recover(&partial);
    assert!(kinds(&bad).contains(&EventFaultKind::PartialEvent));
    assert_eq!(bad.usable().len(), 0);
}

/// A fragment that arrives after its block's stop is not applied, even though
/// its bytes would complete the arguments; the anomaly stays reported and the
/// call is not usable, unlike the same fragment before the stop.
#[test]
fn out_of_order_delta_after_stop_is_an_explicit_fault() {
    let open = head() + &start(0, TOOL_USE) + &json_delta(0, r#"{"path": "a""#);
    let valid = open.clone() + &json_delta(0, "}") + &stop(0) + &tail("tool_use");
    let faulted = open + &stop(0) + &json_delta(0, "}") + &tail("tool_use");
    negative(
        &valid,
        &faulted,
        EventFaultKind::DeltaAfterStop,
        EventFaultClass::OutOfOrder,
    );
    let bad = recover(&faulted);
    let fault = bad.faults[0];
    assert_eq!((fault.block, fault.event), (Some(0), 4));
}

/// A log that never states why the turn stopped (a lost `message_delta`, or
/// a reason this reader does not know) cannot be told from a `max_tokens` cut,
/// so its calls are not usable; the same bytes with a stated reason are.
#[test]
fn missing_or_unrecognized_stop_reason_is_an_explicit_fault() {
    let valid = head() + &tool_block(0) + &tail("tool_use");
    let lost = head() + &tool_block(0) + &sse("message_stop", r#"{"type":"message_stop"}"#);
    negative(
        &valid,
        &lost,
        EventFaultKind::MissingMessageDelta,
        EventFaultClass::Truncated,
    );
    let unknown = head() + &tool_block(0) + &tail("brand_new_reason");
    negative(
        &valid,
        &unknown,
        EventFaultKind::UnrecognizedStopReason,
        EventFaultClass::Malformed,
    );
}

/// A provider `error` event makes the whole log unusable.
#[test]
fn provider_error_event_is_an_explicit_fault() {
    let valid = head() + &tool_block(0) + &tail("tool_use");
    let faulted = head()
        + &tool_block(0)
        + &sse(
            "error",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        );
    negative(
        &valid,
        &faulted,
        EventFaultKind::ProviderError,
        EventFaultClass::Provider,
    );
}

/// A fault in one block withholds only that block's call: the clean call of
/// the same log stays usable, strictly and when guesses are accepted.
#[test]
fn faulted_block_withholds_only_its_own_call() {
    let log = head()
        + &tool_block(0)
        + &start(1, TOOL_USE)
        + &json_delta(1, r#"{"path": "b""#)
        + &stop(1)
        + &json_delta(1, "}")
        + &tail("tool_use");
    let result = recover(&log);
    let Outcome::Resolved(calls) = &result.recovery.outcome else {
        panic!("{:#?}", result.recovery.outcome);
    };
    assert_eq!(calls.calls.len(), 2);
    assert_eq!(kinds(&result), [EventFaultKind::DeltaAfterStop]);
    for usable in [result.usable(), result.usable_accepting_guesses()] {
        assert_eq!(usable.len(), 1);
        assert_eq!(result.block_of(usable[0]), Some(0));
        assert_eq!(
            usable[0].arguments.to_json(),
            r#"{"path":"src/main.rs","limit":20}"#
        );
    }
}

/// Governor refusal and cancellation stop interpretation explicitly: the
/// receipt says why, nothing is usable, and no log is retained.
#[test]
fn refusal_and_cancellation_are_explicit_outcomes() {
    let log = golden_log();
    let tools = tools();

    let request = ToolRecoveryRequest {
        budget: Budget {
            work: 50,
            ..Budget::default()
        },
        ..ToolRecoveryRequest::default()
    };
    let refused = tools.recover_anthropic_events(log.as_bytes(), request, DialectSet::built_in());
    assert!(matches!(
        refused.recovery.receipt.stopped,
        StopReason::BudgetExceeded(_)
    ));

    let mut stream =
        tools.stream_anthropic_events(ToolRecoveryRequest::default(), DialectSet::built_in());
    let mut events = Vec::new();
    let (first, rest) = log.as_bytes().split_at(log.len() / 2);
    stream.feed(first, &mut events);
    stream.cancel();
    stream.feed(rest, &mut events);
    let cancelled = stream.finish(&mut events);
    assert_eq!(cancelled.recovery.receipt.stopped, StopReason::Cancelled);

    for result in [refused, cancelled] {
        assert_eq!(result.recovery.outcome, Outcome::Unresolved);
        assert_eq!(result.usable().len(), 0);
        assert_eq!(result.log.len(), 0);
    }
}
