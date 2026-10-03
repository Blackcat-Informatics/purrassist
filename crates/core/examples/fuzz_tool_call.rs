// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-isolated tool-call recovery probe for the source-region fuzz
//! runner (`scripts/fuzz.py --target tool-call`).
//!
//! Bounded stdin is raw bytes, never rejected for invalid UTF-8 (recovery
//! accepts any bytes and records invalid runs as losses), so the probe's
//! completion line is always `ok`. Byte 0 is a control byte and byte 1 a
//! split byte; the rest is one model turn, recovered against a fixed
//! first-party tool set (read-only and mutating tools with string, integer,
//! number, boolean, enumeration, array, and nested object parameters) over
//! every built-in dialect.
//!
//! Control byte: bits 0-1 the repair policy (per tool, all `Structural`, all
//! `Tolerant`, per tool), bits 2-3 the generation stop (`Stop`, `Length`,
//! `ToolCalls`, `Unknown`), bits 4-5 the reasoning policy (separate, starting
//! open, as content, separate), bits 6-7 the tool choice (`Auto`, `None`,
//! `Required`, forced `read_file`). Split byte: bits 0-1 the effort
//! (`Exact`, `Quick`, `Standard`, `Deep`), bit 2 the ambiguity policy
//! (`Report`, `Reject`), bits 3-7 the split seed.
//!
//! Checks (a panic is a fuzz failure):
//! - recovery never panics, names the output's digest, and stays within its
//!   budget; a value outcome completed, `Unresolved` did not; an `Ambiguous`
//!   outcome's readings match the receipt's alternatives, and every
//!   ranked-adjacent pair of them has distinguishing evidence; a `Resolved`
//!   turn records no guess;
//! - (A13) a `Resolved` call carries no guess and no data loss, so no guessed
//!   call on a mutating tool is `Resolved`; `usable()` holds only `Resolved`
//!   calls that name a tool; `usable_accepting_guesses()` never holds a guess
//!   on a tool that is not read-only; a call embedded in reasoning is never
//!   `Resolved` and never usable;
//! - (F5, F7) the verdict, finish, emission and `finish_reason` agree:
//!   `emitted_calls()` is `usable_accepting_guesses()` when the tool-choice
//!   check passed and empty otherwise; the verdict is `Run` exactly when it
//!   is not empty, and the CLI exit code is 0 exactly for `Run` and
//!   `NoCalls`; the `OpenAI` array holds exactly the emitted calls; a finish
//!   with calls implies `Run`, a `Run` turn finishing otherwise is
//!   `Unresolved` beside a rejected, unresolved or ambiguous attempt, and a
//!   failed tool choice finishes `Unresolved` with `NothingUsable`;
//!   `finish_reason` is `"length"` exactly when the stop is a length stop
//!   (whatever the finish) and otherwise `"tool_calls"` exactly when the
//!   `OpenAI` array holds a call;
//!   the wire `verdict` and usable-call counts match;
//! - (F4) a rejected attempt runs past where its grammar failed (the
//!   furthest the recognizer read an attempt of its dialect at its opener)
//!   only to a literal that ends a call of its dialect, to complete a
//!   literal begun before it, or (T3) to the bracket that balances a JSON
//!   value it left open, so prose after a broken opener stays content;
//! - (S3/F6) a rejected attempt keeps its own closing markup: when its
//!   raw text does not end with a literal that ends a call of its dialect
//!   (any alternative's, a chained call's own; not a single byte), the
//!   content after it up to the next segment holds none;
//! - (F7) a rejected span never ends inside a literal it began;
//! - (F5) every embedded call is offered once, in span order;
//! - (T2) no call, embedded call or rejected attempt holds bytes of a
//!   reasoning range's opener or closer marker in the same reading;
//! - (F8) a malformed attempt's `at` lies inside its span or at its end, or
//!   past it only inside a literal the failing reading began and matched
//!   in part (at most the longest literal spelling past the end);
//! - every call, rejected-attempt, content, and reasoning range lies within
//!   the output, never splits a well-formed UTF-8 character, and slices back
//!   to the retained source (a rejected attempt's raw bytes are exactly its
//!   range); every call's and rejected attempt's candidate dialects are
//!   sorted, unique, and contain its dialect, except that unparsed markup
//!   (`Rejection::UnsupportedDialect`) has no candidate, names its family's
//!   built-in dialect if any, and lies inside one content range; content,
//!   reasoning, and call ranges never overlap; an embedded
//!   call lies inside a reasoning range;
//! - every `Resolved` call printed in its own dialect recovers to the same
//!   tool with `Node::equivalent` arguments, `Resolved`; printing that
//!   recovery again gives the same bytes when the printed calls were the
//!   turn's first calls (ids number calls by index), and is otherwise a
//!   fixed point from its first recovery on; a `Resolved` call is skipped
//!   only as unrepresentable, or as `ToolChoiceFailed` when the turn failed
//!   its tool-choice check (and a call is skipped as `ToolChoiceFailed`
//!   only then, and only when it would otherwise run);
//! - pushing the turn to a [`Recognizer`] in one chunk, split in two at the
//!   seeded position, or in seeded fixed-size chunks gives an equal
//!   recognized turn (or the same error) and equal governor usage;
//! - (S1) a reasoning mode never makes call markup vanish: every call the
//!   request recovers with reasoning as content overlaps a call, an
//!   embedded call or a rejected attempt under the requested mode, unless
//!   its bytes hold a marker of a non-`explicit` pair or an `explicit`
//!   opener precedes its end;
//! - under a budget too small for the output, recovery is `Unresolved` with
//!   `StopReason::BudgetExceeded`, empty ledgers, and no alternatives.
use core::ops::Range;
use purrassist_core::contract::Usage;
use purrassist_core::digest::Digest;
use purrassist_core::tool_call::{
    AmbiguityPolicy, CallOutcome, DialectId, DialectSet, Effects, Finish, GenerationStop, IdPolicy,
    ObjectSpec, Param, Piece, PolicySelection, ReasoningPolicy, RecognizedTurn, Recognizer,
    Rejection, RepairPolicy, Segment, SkipReason, Skipped, ToolCalls, ToolChoice, ToolDefinition,
    ToolRecoveryRequest, ToolSet, Verdict, emit,
};
use purrassist_core::wire::WireValue;
use purrassist_core::{AssistResult, Budget, Effort, Governor, Outcome, StopReason, json};
use std::io::{self, Read};

/// The fixed tool set: the conformance fixtures' tools (`write_file`
/// mutating, with every parameter kind), a read-only tool with no required
/// parameter (the issue's motivating stray-closer case), and a mutating
/// tool with an alias and a repeatable parameter.
fn tools() -> ToolSet {
    let owner = ObjectSpec::new()
        .param("name", Param::string())
        .param("uid", Param::integer());
    let options = ObjectSpec::new()
        .param("backup", Param::boolean())
        .param("retries", Param::integer())
        .param("owner", Param::object(owner));
    ToolSet::compile(
        [
            ToolDefinition::new("read_file")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required().alias("file"))
                .param("limit", Param::integer()),
            ToolDefinition::new("list_dir")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required()),
            ToolDefinition::new("current_time")
                .effects(Effects::ReadOnly)
                .param("zone", Param::string()),
            ToolDefinition::new("get_weather")
                .effects(Effects::ReadOnly)
                .param("city", Param::string().required())
                .param("unit", Param::enumeration(&["celsius", "fahrenheit"])),
            ToolDefinition::new("write_file")
                .effects(Effects::Mutating)
                .param("path", Param::string().required())
                .param("content", Param::string().required())
                .param("mode", Param::enumeration(&["create", "overwrite"]))
                .param("ratio", Param::number())
                .param("count", Param::integer())
                .param("tags", Param::array(Param::string()))
                .param("options", Param::object(options)),
            ToolDefinition::new("delete_path")
                .alias("rm")
                .effects(Effects::Mutating)
                .param("path", Param::string().required().repeatable()),
        ],
        Budget::default(),
    )
    .expect("the fixed tool set compiles")
}

/// The request knobs the two header bytes select.
fn request(control: u8, split: u8, output: &[u8]) -> ToolRecoveryRequest {
    let effort =
        [Effort::Exact, Effort::Quick, Effort::Standard, Effort::Deep][usize::from(split & 3)];
    let policy = [
        PolicySelection::PerTool,
        PolicySelection::All(RepairPolicy::Structural),
        PolicySelection::All(RepairPolicy::Tolerant),
        PolicySelection::PerTool,
    ][usize::from(control & 3)];
    let stop = [
        GenerationStop::Stop,
        GenerationStop::Length,
        GenerationStop::ToolCalls,
        GenerationStop::Unknown,
    ][usize::from((control >> 2) & 3)]
    .clone();
    let reasoning = [
        ReasoningPolicy::Separate { starts_open: false },
        ReasoningPolicy::Separate { starts_open: true },
        ReasoningPolicy::AsContent,
        ReasoningPolicy::Separate { starts_open: false },
    ][usize::from((control >> 4) & 3)];
    let choice = match control >> 6 {
        0 => ToolChoice::Auto,
        1 => ToolChoice::None,
        2 => ToolChoice::Required,
        _ => ToolChoice::Forced("read_file".into()),
    };
    let ambiguity = if split & 4 == 0 {
        AmbiguityPolicy::Report
    } else {
        AmbiguityPolicy::Reject
    };
    ToolRecoveryRequest::sized(effort, output)
        .policy(policy)
        .stop(stop)
        .reasoning(reasoning)
        .tool_choice(choice)
        .ambiguity(ambiguity)
}

/// Whether `position` falls strictly inside a well-formed UTF-8 character.
fn splits_character(bytes: &[u8], position: usize) -> bool {
    if position == 0 || position >= bytes.len() || bytes[position] & 0xC0 != 0x80 {
        return false;
    }
    for back in 1..=3 {
        let Some(lead) = position.checked_sub(back) else {
            return false;
        };
        let width = match bytes[lead] {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            0x80..=0xBF => continue,
            _ => return false,
        };
        return lead + width > position
            && bytes
                .get(lead..lead + width)
                .is_some_and(|text| std::str::from_utf8(text).is_ok());
    }
    false
}

fn check_range(output: &[u8], range: &Range<usize>, what: &str) {
    assert!(
        range.start <= range.end && range.end <= output.len(),
        "{what} range {range:?} exceeds the {}-byte output",
        output.len()
    );
    for position in [range.start, range.end] {
        assert!(
            !splits_character(output, position),
            "{what} range {range:?} splits a UTF-8 character at {position}"
        );
    }
}

fn check_disjoint(mut ranges: Vec<(Range<usize>, &str)>) {
    ranges.sort_by_key(|(range, _)| (range.start, range.end));
    for pair in ranges.windows(2) {
        let ((first, first_kind), (second, second_kind)) = (&pair[0], &pair[1]);
        assert!(
            first.end <= second.start,
            "{first_kind} range {first:?} overlaps {second_kind} range {second:?}"
        );
    }
}

/// Every rejected attempt keeps its exact bytes; an attempt a parser made
/// names its dialect among its sorted candidates, and unparsed markup has no
/// candidate and lies inside one content range.
fn check_rejected(output: &[u8], calls: &ToolCalls) {
    for rejected in &calls.rejected {
        check_range(output, &rejected.span, "rejected");
        assert_eq!(
            rejected.raw.as_slice(),
            &output[rejected.span.clone()],
            "a rejected attempt's raw bytes are not its range"
        );
        if let Some(text) = rejected.text() {
            assert_eq!(text.as_bytes(), rejected.raw.as_slice());
        }
        if let Rejection::UnsupportedDialect { family } = rejected.reason {
            // Unparsed markup: no allowed dialect attempted it, and its
            // bytes stay content.
            assert!(rejected.candidate_dialects.is_empty(), "{rejected:?}");
            assert_eq!(rejected.dialect, family.dialect(), "{rejected:?}");
            assert!(
                calls
                    .content
                    .iter()
                    .any(|range| range.start <= rejected.span.start
                        && rejected.span.end <= range.end),
                "unparsed markup outside content: {rejected:?}"
            );
        } else {
            let dialect = rejected.dialect.as_ref().expect("a parser's attempt");
            assert!(
                rejected.candidate_dialects.contains(dialect),
                "a rejected attempt's candidates omit its dialect: {rejected:?}"
            );
        }
        assert!(
            rejected
                .candidate_dialects
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "candidate dialects are not sorted and unique: {rejected:?}"
        );
    }
}

/// Every literal that ends a call of `dialect`, in any alternative (a
/// section's or wrapper's closer, a chained call's own `<|tool_call_end|>`,
/// another alternative's `</function>`), longer than one byte: a single
/// byte (a JSON bracket, a separator) is also prose.
fn dialect_closers(dialect: &DialectId) -> Vec<Vec<u8>> {
    let mut closers: Vec<Vec<u8>> = dialect
        .grammar()
        .call_ends()
        .iter()
        .flat_map(|end| end.call_closers.iter())
        .filter(|closer| closer.len() > 1)
        .cloned()
        .collect();
    closers.sort();
    closers.dedup();
    closers
}

/// S3/F6: a rejected attempt keeps its own closing markup. For an attempt
/// a parser made, when its raw text does not end with a literal that ends
/// a call of its dialect ([`dialect_closers`]), the content after it, up to
/// the next call, reasoning block or other attempt, holds none: that
/// closer belongs to the attempt.
fn check_rejected_markup(output: &[u8], calls: &ToolCalls) {
    for rejected in &calls.rejected {
        if matches!(rejected.reason, Rejection::UnsupportedDialect { .. }) {
            continue;
        }
        let Some(dialect) = &rejected.dialect else {
            continue;
        };
        let closers = dialect_closers(dialect);
        if closers.iter().any(|closer| rejected.raw.ends_with(closer)) {
            continue;
        }
        let end = rejected.span.end;
        let next = calls
            .calls
            .iter()
            .map(|call| call.span.start)
            .chain(calls.reasoning.iter().map(|range| range.start))
            .chain(calls.rejected.iter().map(|other| other.span.start))
            .filter(|start| *start >= end && *start > rejected.span.start)
            .min()
            .unwrap_or(output.len());
        for range in &calls.content {
            let start = range.start.max(end);
            let stop = range.end.min(next);
            if start >= stop {
                continue;
            }
            let text = &output[start..stop];
            for closer in &closers {
                assert!(
                    !text
                        .windows(closer.len())
                        .any(|window| window == closer.as_slice()),
                    "content {start}..{stop} after a rejected attempt holds its closer {:?}",
                    String::from_utf8_lossy(closer)
                );
            }
        }
    }
}

/// F7: a rejected span never ends inside a literal it began: the content
/// right after it never starts with the rest of a literal spelling of its
/// dialect that begins inside the span.
fn check_rejected_literals(output: &[u8], calls: &ToolCalls) {
    for rejected in &calls.rejected {
        if matches!(rejected.reason, Rejection::UnsupportedDialect { .. }) {
            continue;
        }
        let Some(dialect) = &rejected.dialect else {
            continue;
        };
        let end = rejected.span.end;
        if !calls.content.iter().any(|range| range.start == end) {
            continue;
        }
        for spelling in dialect.grammar().call_literals() {
            for inside in 1..spelling.len() {
                let Some(start) = end.checked_sub(inside) else {
                    break;
                };
                if start < rejected.span.start {
                    break;
                }
                assert!(
                    !output[start..].starts_with(spelling),
                    "rejected attempt {:?} ends inside the literal {:?}",
                    rejected.span,
                    String::from_utf8_lossy(spelling)
                );
            }
        }
    }
}

/// F8: a malformed attempt's `at` (the first byte no reading could
/// consume) lies inside its span or at its end, or past its end only
/// inside a literal the failing reading began at or before that end and
/// matched only in part (the span leaves it out: it may be prose or begin
/// the next opener). Such a reading consumed less than the literal (one
/// stray byte deleted at most), so `at` is at most the longest literal
/// spelling past the span's end.
fn check_rejected_at(calls: &ToolCalls) {
    for rejected in &calls.rejected {
        let (Rejection::Malformed { at }, Some(dialect)) = (rejected.reason, &rejected.dialect)
        else {
            continue;
        };
        let end = rejected.span.end;
        let longest = dialect
            .grammar()
            .call_literals()
            .first()
            .map_or(0, Vec::len);
        assert!(
            at >= rejected.span.start && at <= end + longest,
            "rejected attempt {:?} fails at {at}, beyond a literal past its end",
            rejected.span
        );
    }
}

/// Every structural and A13 invariant of one recovered value.
fn check_calls(output: &[u8], calls: &ToolCalls) {
    for call in calls.calls.iter().chain(&calls.embedded) {
        check_range(output, &call.span, "call");
        assert!(
            call.candidate_dialects.contains(&call.dialect),
            "a call's candidates omit its dialect: {call:?}"
        );
        assert!(
            call.candidate_dialects
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "candidate dialects are not sorted and unique: {call:?}"
        );
        if call.outcome == CallOutcome::Resolved {
            assert!(
                !call.repairs.guessed(),
                "a Resolved call records a guess: {call:?}"
            );
            assert!(
                !call.repairs.data_loss(),
                "a Resolved call records a data loss: {call:?}"
            );
        }
        if call.effects == Some(Effects::Mutating) && call.repairs.guessed() {
            assert_ne!(call.outcome, CallOutcome::Resolved, "A13: {call:?}");
        }
    }
    for call in calls.usable() {
        assert_eq!(call.outcome, CallOutcome::Resolved, "usable(): {call:?}");
        assert!(call.tool.is_some(), "usable() names no tool: {call:?}");
    }
    for call in calls.usable_accepting_guesses() {
        assert!(
            call.outcome == CallOutcome::Resolved
                || (call.outcome == CallOutcome::BestGuess
                    && call.effects == Some(Effects::ReadOnly)),
            "usable_accepting_guesses() holds {call:?}"
        );
    }
    check_verdict(calls);
    check_rejected(output, calls);
    // F5: every embedded call is offered once, in span order.
    assert!(
        calls.embedded.windows(2).all(|pair| {
            (pair[0].span.start, pair[0].span.end) < (pair[1].span.start, pair[1].span.end)
        }),
        "embedded calls repeat a span or are out of order: {:?}",
        calls
            .embedded
            .iter()
            .map(|call| call.span.clone())
            .collect::<Vec<_>>()
    );
    check_rejected_markup(output, calls);
    check_rejected_literals(output, calls);
    check_rejected_at(calls);
    for range in &calls.content {
        check_range(output, range, "content");
    }
    for range in &calls.reasoning {
        check_range(output, range, "reasoning");
    }
    for embedded in &calls.embedded {
        assert_ne!(embedded.outcome, CallOutcome::Resolved, "{embedded:?}");
        assert!(
            calls.reasoning.iter().any(|range| {
                range.start <= embedded.span.start && embedded.span.end <= range.end
            }),
            "embedded call {:?} lies outside every reasoning range {:?}",
            embedded.span,
            calls.reasoning
        );
    }
    let ranges = calls
        .content
        .iter()
        .map(|range| (range.clone(), "content"))
        .chain(
            calls
                .reasoning
                .iter()
                .map(|range| (range.clone(), "reasoning")),
        )
        .chain(calls.calls.iter().map(|call| (call.span.clone(), "call")))
        .filter(|(range, _)| !range.is_empty())
        .collect();
    check_disjoint(ranges);
}

/// F5/F7: the verdict, the finish, the emission and `finish_reason` of one
/// recovered value never contradict each other, and the wire value states
/// the same verdict and counts.
fn check_verdict(calls: &ToolCalls) {
    let emitted = calls.emitted_calls();
    let usable = calls.usable_accepting_guesses();
    if calls.tool_choice.passed {
        assert_eq!(emitted, usable, "emitted_calls() is not the usable calls");
    } else {
        assert!(emitted.is_empty(), "a failed tool choice emits calls");
    }
    let verdict = calls.verdict();
    assert_eq!(verdict == Verdict::Run, !emitted.is_empty(), "{verdict:?}");
    let code = verdict.exit_code();
    assert_eq!(
        code == 0,
        matches!(verdict, Verdict::Run | Verdict::NoCalls),
        "{verdict:?}"
    );
    let with_calls = matches!(calls.finish, Finish::Calls | Finish::TruncatedWithCalls);
    let truncated = matches!(calls.finish, Finish::Truncated | Finish::TruncatedWithCalls);
    if with_calls {
        assert_eq!(verdict, Verdict::Run, "finish {:?}", calls.finish);
    } else if verdict == Verdict::Run {
        let wrong = !calls.rejected.is_empty()
            || calls.calls.iter().any(|call| {
                matches!(
                    call.outcome,
                    CallOutcome::Unresolved { .. } | CallOutcome::Ambiguous
                )
            });
        assert!(
            calls.finish == Finish::Unresolved && wrong,
            "a Run turn finishes {:?} with nothing wrong",
            calls.finish
        );
    }
    if !calls.tool_choice.passed {
        assert_eq!(calls.finish, Finish::Unresolved);
        assert_eq!(verdict, Verdict::NothingUsable);
    }
    let array = emit::openai_tool_calls(
        calls,
        &IdPolicy::Prefix {
            prefix: "c".into(),
            start: 0,
        },
    )
    .expect("OpenAI emission");
    assert_eq!(array.calls.len(), emitted.len(), "the OpenAI array");
    // `"length"` exactly for a length stop (a truncated finish is one),
    // otherwise `"tool_calls"` exactly when the array holds a call.
    if truncated {
        assert!(
            calls.length_stop,
            "a truncated finish without a length stop"
        );
    }
    let reason = calls.openai_finish_reason();
    assert_eq!(
        reason == "length",
        calls.length_stop,
        "finish_reason {reason}"
    );
    assert_eq!(
        reason == "tool_calls",
        !calls.length_stop && !array.calls.is_empty(),
        "finish_reason {reason}"
    );
    let wire = calls.to_node();
    assert_eq!(
        wire.get("verdict").and_then(json::Node::as_str),
        Some(verdict.code())
    );
    assert_eq!(
        wire.get("usable_calls").map(json::Node::to_json),
        Some(calls.usable().len().to_string())
    );
    assert_eq!(
        wire.get("usable_calls_accepting_guesses")
            .map(json::Node::to_json),
        Some(usable.len().to_string())
    );
    assert_eq!(
        wire.get("openai_finish_reason")
            .and_then(json::Node::as_str),
        Some(reason)
    );
}

fn single(outcome: Outcome<ToolCalls>, context: &str) -> ToolCalls {
    match outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
        other => panic!("{context}: canonical markup has no single reading: {other:?}"),
    }
}

/// Print every `Resolved` call in its own dialect and recover it again.
fn check_reemission(tools: &ToolSet, calls: &ToolCalls) {
    let mut dialects: Vec<DialectId> = calls
        .calls
        .iter()
        .filter(|call| call.outcome == CallOutcome::Resolved)
        .map(|call| call.dialect.clone())
        .collect();
    dialects.sort();
    dialects.dedup();
    for dialect in dialects {
        let ids = IdPolicy::default();
        let printed = emit::emit(tools, calls, &dialect, &ids).expect("emission");
        for skipped in &printed.skipped {
            if let Skipped::Call { index, reason } = skipped {
                let original = calls
                    .calls
                    .iter()
                    .find(|call| call.index == *index)
                    .expect("a skipped call is a call of the turn");
                if matches!(reason, SkipReason::ToolChoiceFailed) {
                    assert!(
                        !calls.tool_choice.passed && original.is_usable_accepting_guesses(),
                        "skipped as ToolChoiceFailed: {original:?}"
                    );
                } else if original.outcome == CallOutcome::Resolved {
                    assert!(
                        matches!(reason, SkipReason::Unrepresentable { .. }),
                        "a Resolved call was skipped: {reason:?}"
                    );
                }
            }
        }
        let set = DialectSet::new([dialect.clone()], &mut Governor::new(Budget::default()))
            .expect("a one-dialect set");
        let request = ToolRecoveryRequest::sized_for(Effort::Standard, &printed.output, set);
        let context = format!(
            "{} re-emission {:?}",
            dialect.id(),
            String::from_utf8_lossy(&printed.output)
        );
        let again = single(tools.recover(&printed.output, &request).outcome, &context);
        assert!(again.rejected.is_empty(), "{context}: {:?}", again.rejected);
        assert_eq!(again.calls.len(), printed.calls.len(), "{context}");
        for (index, recovered) in printed.calls.iter().zip(&again.calls) {
            let original = calls
                .calls
                .iter()
                .find(|call| call.index == *index)
                .expect("a printed call is a call of the turn");
            if original.outcome != CallOutcome::Resolved {
                continue;
            }
            assert_eq!(recovered.tool, original.tool, "{context}");
            assert_eq!(recovered.outcome, CallOutcome::Resolved, "{context}");
            assert!(
                recovered.arguments.equivalent(&original.arguments),
                "{context}: {} != {}",
                recovered.arguments.to_json(),
                original.arguments.to_json()
            );
        }
        // Ids number calls by their index in the turn: the printed text is
        // reproduced exactly when the printed calls were the turn's first
        // calls, and is a fixed point from its first recovery on.
        let reprinted = emit::emit(tools, &again, &dialect, &ids).expect("emission again");
        if printed.calls.iter().copied().eq(0..printed.calls.len()) {
            assert_eq!(reprinted.output, printed.output, "{context}: unstable");
        }
        let settled = single(tools.recover(&reprinted.output, &request).outcome, &context);
        let fixed = emit::emit(tools, &settled, &dialect, &ids).expect("emission settled");
        assert_eq!(fixed.output, reprinted.output, "{context}: no fixed point");
    }
}

fn check_result(
    tools: &ToolSet,
    output: &[u8],
    request: &ToolRecoveryRequest,
    result: &AssistResult<ToolCalls>,
) {
    let receipt = &result.receipt;
    assert_eq!(receipt.source, Digest::of(output));
    assert!(receipt.usage.work <= request.budget.work);
    assert!(receipt.usage.candidates <= request.budget.candidates);
    assert!(receipt.usage.peak_scratch_bytes <= request.budget.scratch_bytes);
    assert!(receipt.usage.evidence_entries <= request.budget.evidence_entries);
    let wire = result.to_node().to_json();
    assert!(json::parse(&wire).is_ok(), "the wire form is JSON");
    let values = match &result.outcome {
        Outcome::Resolved(calls) => {
            assert!(
                !receipt.contains_guesses(),
                "a Resolved turn records a guess"
            );
            std::slice::from_ref(calls)
        }
        Outcome::BestGuess(calls) => std::slice::from_ref(calls),
        Outcome::Ambiguous(values) => {
            assert!(values.len() > 1);
            assert_eq!(values.len(), receipt.alternatives.len());
            let ambiguity = result.to_node();
            let pairs = ambiguity
                .pointer("/receipt/ambiguity/distinguishing")
                .expect("an ambiguity annotation");
            let json::Value::Array(pairs) = &pairs.value else {
                panic!("distinguishing is an array")
            };
            for pair in pairs {
                assert!(
                    !matches!(&pair.value, json::Value::Array(nodes) if nodes.is_empty()),
                    "an ambiguity without distinguishing evidence"
                );
            }
            values.as_slice()
        }
        Outcome::Unresolved => {
            assert_ne!(receipt.stopped, StopReason::Completed);
            &[]
        }
    };
    if !values.is_empty() {
        assert_eq!(receipt.stopped, StopReason::Completed);
    }
    // The CLI exit code is the selected reading's verdict, 3 for an
    // ambiguous operation and 4 for an unresolved one.
    let code = purrassist_core::tool_call::exit_code(&result.outcome);
    match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => {
            assert_eq!(code, calls.verdict().exit_code());
        }
        Outcome::Ambiguous(_) => assert_eq!(code, 3),
        Outcome::Unresolved => assert_eq!(code, 4),
    }
    let read = if values.is_empty() {
        Vec::new()
    } else {
        read_spans(tools, output, request)
    };
    for calls in values {
        check_rejected_extension(output, calls, &read);
        assert_eq!(
            calls.length_stop,
            request.stop.is_truncation(),
            "length_stop is the request's stop"
        );
        check_calls(output, calls);
        check_reemission(tools, calls);
    }
}

/// Every rejected attempt the recognizer read, in any reading of the
/// turn: its start, dialect, and where its grammar failed.
fn read_spans(
    tools: &ToolSet,
    output: &[u8],
    request: &ToolRecoveryRequest,
) -> Vec<(Range<usize>, DialectId)> {
    let Ok(turn) = recognize(tools, request, &[output]).0 else {
        return Vec::new();
    };
    let mut spans = Vec::new();
    for piece in turn.pieces() {
        let segments: Vec<&Segment> = match piece {
            Piece::Segment(segment) => vec![segment],
            Piece::Choice(choice) => choice.readings.iter().flatten().collect(),
        };
        for segment in segments {
            if let Segment::Rejected { span, dialect, .. } = segment {
                spans.push((span.clone(), dialect.clone()));
            }
        }
    }
    spans
}

/// F4: recovery never moves bytes that are not shown to be call markup
/// from content into a rejected attempt. A parser's rejected attempt may
/// run past where its grammar failed (the furthest the recognizer read an
/// attempt of its dialect at its opener) only to a literal that ends a
/// call of its dialect (its raw text then ends with one), or to complete a
/// literal spelling that began before that point. Otherwise the bytes
/// after the failure point are not the attempt's and stay content.
fn check_rejected_extension(output: &[u8], calls: &ToolCalls, read: &[(Range<usize>, DialectId)]) {
    for rejected in &calls.rejected {
        if matches!(rejected.reason, Rejection::UnsupportedDialect { .. }) {
            continue;
        }
        let Some(dialect) = &rejected.dialect else {
            continue;
        };
        let Some(failed) = read
            .iter()
            .filter(|(span, read)| span.start == rejected.span.start && read == dialect)
            .map(|(span, _)| span.end)
            .max()
        else {
            continue;
        };
        let end = rejected.span.end;
        if end <= failed {
            continue;
        }
        let grammar = dialect.grammar();
        let closed = grammar
            .call_ends()
            .iter()
            .flat_map(|ends| &ends.call_closers)
            .any(|closer| rejected.raw.ends_with(closer));
        let completed = grammar.call_literals().iter().any(|spelling| {
            end >= spelling.len()
                && end - spelling.len() < failed
                && end - spelling.len() >= rejected.span.start
                && output[end - spelling.len()..end] == spelling[..]
        });
        // T3: or to the bracket that balances a JSON value the attempt left
        // open: its bytes open more brackets than they close (outside JSON
        // strings) at every point from where it failed until its last byte,
        // which balances them.
        let balanced = {
            let (mut depth, mut quoted, mut escaped, mut open) = (0_usize, false, false, true);
            for (offset, &byte) in output[rejected.span.start..end].iter().enumerate() {
                let at = rejected.span.start + offset;
                if quoted {
                    match byte {
                        _ if escaped => escaped = false,
                        b'\\' => escaped = true,
                        b'"' => quoted = false,
                        _ => {}
                    }
                } else {
                    match byte {
                        b'"' => quoted = true,
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                }
                if at >= failed && at + 1 < end && depth == 0 {
                    open = false;
                }
            }
            open && depth == 0 && matches!(output[end - 1], b'}' | b']')
        };
        assert!(
            closed || completed || balanced,
            "rejected attempt {:?} runs past where it failed ({failed}) without its closer: {:?}",
            rejected.span,
            String::from_utf8_lossy(&output[failed..end])
        );
    }
}

/// Recognize `output` pushed in `chunks`.
fn recognize(
    tools: &ToolSet,
    request: &ToolRecoveryRequest,
    chunks: &[&[u8]],
) -> (Result<RecognizedTurn, StopReason>, Usage) {
    let mut governor = Governor::new(request.budget);
    let result = Recognizer::with_tools(&request.dialects, tools, request.reasoning, &mut governor)
        .and_then(|mut recognizer| {
            for chunk in chunks {
                recognizer.push(chunk, &mut governor)?;
            }
            recognizer.finish(request.stop.clone(), &mut governor)
        });
    (result, governor.usage())
}

fn check_chunking(tools: &ToolSet, output: &[u8], request: &ToolRecoveryRequest, seed: u8) {
    let whole = recognize(tools, request, &[output]);
    let position = usize::from(seed) * (output.len() + 1) / 32;
    let position = position.min(output.len());
    let halves = recognize(tools, request, &[&output[..position], &output[position..]]);
    let pieces: Vec<&[u8]> = output.chunks(usize::from(seed % 13) + 1).collect();
    let chunked = recognize(tools, request, &pieces);
    for (name, other) in [("split", &halves), ("chunked", &chunked)] {
        assert_eq!(other.0, whole.0, "{name} push changed the recognized turn");
        assert_eq!(other.1, whole.1, "{name} push changed the governor usage");
    }
}

fn check_exhaustion(tools: &ToolSet, output: &[u8], request: &ToolRecoveryRequest) {
    let tiny = Budget {
        work: u64::try_from(output.len() / 2).unwrap_or(u64::MAX),
        ..Effort::Quick.budget()
    };
    let request = request.clone().budget(tiny);
    let result = tools.recover(output, &request);
    assert_eq!(result.receipt.source, Digest::of(output));
    assert!(result.receipt.usage.work <= tiny.work);
    if output.len() >= 2 {
        assert_eq!(
            result.outcome,
            Outcome::Unresolved,
            "a starved recovery returned a value"
        );
        assert!(
            matches!(result.receipt.stopped, StopReason::BudgetExceeded(_)),
            "a starved recovery stopped with {:?}",
            result.receipt.stopped
        );
        let receipt = &result.receipt;
        let ledgers = (
            receipt.evidence.len(),
            receipt.losses.len(),
            receipt.inferences.len(),
            receipt.alternatives.len(),
        );
        assert_eq!(
            ledgers,
            (0, 0, 0, 0),
            "a starved recovery kept ledger entries"
        );
    } else {
        check_result(tools, output, &request, &result);
    }
}

/// S1: a reasoning mode never makes call markup vanish. Every call the
/// same request recovers with reasoning markers as plain content
/// ([`ReasoningPolicy::AsContent`]) overlaps a call, an embedded call or a
/// rejected attempt of the recovery under the requested mode, unless its
/// bytes hold a marker of a pair a prompt template can open (a marker
/// inside a call is a marker), or an `explicit` pair's opener comes before
/// its end (that block may end inside the call at its JSON-like closer).
fn check_reasoning_keeps_calls(
    tools: &ToolSet,
    output: &[u8],
    request: &ToolRecoveryRequest,
    result: &AssistResult<ToolCalls>,
) {
    if request.reasoning == ReasoningPolicy::AsContent {
        return;
    }
    let (Outcome::Resolved(separate) | Outcome::BestGuess(separate)) = &result.outcome else {
        return;
    };
    let plain = tools.recover(
        output,
        &request.clone().reasoning(ReasoningPolicy::AsContent),
    );
    let (Outcome::Resolved(plain) | Outcome::BestGuess(plain)) = &plain.outcome else {
        return;
    };
    // Markers that may legitimately change how a call's bytes read: any
    // marker of a pair a prompt template can open, inside the call; and an
    // `explicit` pair's opener anywhere before the call ends (such a block
    // may end inside the call at its JSON-like closer).
    let mut free: Vec<&[u8]> = Vec::new();
    let mut explicit_openers: Vec<&[u8]> = Vec::new();
    for dialect in request.dialects.dialects() {
        let grammar = dialect.grammar();
        for ((open, close), explicit) in grammar
            .reasoning_markers()
            .iter()
            .zip(grammar.reasoning_explicit())
        {
            if *explicit {
                explicit_openers.push(open);
            } else {
                free.extend([open.as_slice(), close.as_slice()]);
            }
        }
    }
    let contains =
        |bytes: &[u8], marker: &[u8]| bytes.windows(marker.len()).any(|window| window == marker);
    let holds_marker = |range: &Range<usize>| {
        free.iter()
            .any(|marker| contains(&output[range.clone()], marker))
            || explicit_openers
                .iter()
                .any(|marker| contains(&output[..range.end], marker))
    };
    let overlaps = |a: &Range<usize>, b: &Range<usize>| a.start < b.end && b.start < a.end;
    for call in &plain.calls {
        if call.span.is_empty() || holds_marker(&call.span) {
            continue;
        }
        let kept = separate
            .calls
            .iter()
            .chain(&separate.embedded)
            .map(|other| &other.span)
            .chain(separate.rejected.iter().map(|rejected| &rejected.span))
            .any(|span| overlaps(span, &call.span));
        assert!(
            kept,
            "{:?} swallowed the call at {:?}: reasoning {:?}, content {:?}",
            request.reasoning, call.span, separate.reasoning, separate.content
        );
    }
}

/// T2: no byte belongs to both a reasoning marker and a call in one
/// reading. A reasoning range's markers are its opener (the range begins
/// with an opener spelling of an allowed dialect) and its closer (it ends
/// with a closer spelling); no call, embedded call or rejected attempt of
/// the reading overlaps them.
fn check_markers_not_calls(
    output: &[u8],
    request: &ToolRecoveryRequest,
    result: &AssistResult<ToolCalls>,
) {
    let readings: Vec<&ToolCalls> = match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => vec![calls],
        Outcome::Ambiguous(values) => values.iter().collect(),
        Outcome::Unresolved => Vec::new(),
    };
    let mut pairs: Vec<(&[u8], &[u8])> = Vec::new();
    for dialect in request.dialects.dialects() {
        for (open, close) in dialect.grammar().reasoning_markers() {
            pairs.push((open, close));
        }
    }
    let overlaps = |a: &Range<usize>, b: &Range<usize>| a.start < b.end && b.start < a.end;
    for calls in readings {
        for range in &calls.reasoning {
            let text = &output[range.clone()];
            let mut markers: Vec<Range<usize>> = Vec::new();
            if let Some(open) = pairs
                .iter()
                .map(|(open, _)| *open)
                .filter(|open| !open.is_empty() && text.starts_with(open))
                .map(<[u8]>::len)
                .max()
            {
                markers.push(range.start..range.start + open);
            }
            if let Some(close) = pairs
                .iter()
                .map(|(_, close)| *close)
                .filter(|close| !close.is_empty() && text.ends_with(close))
                .map(<[u8]>::len)
                .max()
            {
                markers.push(range.end - close..range.end);
            }
            let spans = calls
                .calls
                .iter()
                .chain(&calls.embedded)
                .map(|call| &call.span)
                .chain(calls.rejected.iter().map(|rejected| &rejected.span));
            for span in spans {
                for marker in &markers {
                    assert!(
                        !overlaps(span, marker),
                        "the call or attempt at {span:?} holds bytes of the reasoning marker at {marker:?}"
                    );
                }
            }
        }
    }
}

fn main() {
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "probe input exceeds 4096 bytes");
    let control = bytes.first().copied().unwrap_or(0);
    let split = bytes.get(1).copied().unwrap_or(0);
    let output = bytes.get(2..).unwrap_or_default();
    let tools = tools();
    let request = request(control, split, output);
    let result = tools.recover(output, &request);
    check_result(&tools, output, &request, &result);
    check_reasoning_keeps_calls(&tools, output, &request, &result);
    check_markers_not_calls(output, &request, &result);
    check_chunking(&tools, output, &request, split >> 3);
    check_exhaustion(&tools, output, &request);
    println!("ok");
}
