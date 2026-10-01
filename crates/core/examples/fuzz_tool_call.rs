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
//!   outcome's readings match the receipt's alternatives; a `Resolved` turn
//!   records no guess;
//! - (A13) a `Resolved` call carries no guess and no data loss, so no guessed
//!   call on a mutating tool is `Resolved`; `usable()` holds only `Resolved`
//!   calls; `usable_accepting_guesses()` never holds a guess on a tool that
//!   is not read-only; a call embedded in reasoning is never `Resolved` and
//!   never usable;
//! - every call, rejected-attempt, content, and reasoning range lies within
//!   the output, never splits a well-formed UTF-8 character, and slices back
//!   to the retained source (a rejected attempt's raw bytes are exactly its
//!   range); content, reasoning, and call ranges never overlap; an embedded
//!   call lies inside a reasoning range;
//! - every `Resolved` call printed in its own dialect recovers to the same
//!   tool with `Node::equivalent` arguments, `Resolved`; printing that
//!   recovery again gives the same bytes when the printed calls were the
//!   turn's first calls (ids number calls by index), and is otherwise a
//!   fixed point from its first recovery on; a `Resolved` call is skipped
//!   only as unrepresentable;
//! - pushing the turn to a [`Recognizer`] in one chunk, split in two at the
//!   seeded position, or in seeded fixed-size chunks gives an equal
//!   recognized turn (or the same error) and equal governor usage;
//! - under a budget too small for the output, recovery is `Unresolved` with
//!   `StopReason::BudgetExceeded`, empty ledgers, and no alternatives.
use core::ops::Range;
use purrassist_core::contract::Usage;
use purrassist_core::digest::Digest;
use purrassist_core::tool_call::{
    AmbiguityPolicy, CallOutcome, DialectId, DialectSet, Effects, GenerationStop, IdPolicy,
    ObjectSpec, Param, PolicySelection, ReasoningPolicy, RecognizedTurn, Recognizer, RepairPolicy,
    SkipReason, Skipped, ToolCalls, ToolChoice, ToolDefinition, ToolRecoveryRequest, ToolSet, emit,
};
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
fn request(control: u8, split: u8, len: usize) -> ToolRecoveryRequest {
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
    ToolRecoveryRequest::sized(effort, len)
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

/// Every structural and A13 invariant of one recovered value.
fn check_calls(output: &[u8], calls: &ToolCalls) {
    for call in calls.calls.iter().chain(&calls.embedded) {
        check_range(output, &call.span, "call");
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
    }
    for call in calls.usable_accepting_guesses() {
        assert!(
            call.outcome == CallOutcome::Resolved
                || (call.outcome == CallOutcome::BestGuess
                    && call.effects == Some(Effects::ReadOnly)),
            "usable_accepting_guesses() holds {call:?}"
        );
    }
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
    }
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
                if original.outcome == CallOutcome::Resolved {
                    assert!(
                        matches!(reason, SkipReason::Unrepresentable { .. }),
                        "a Resolved call was skipped: {reason:?}"
                    );
                }
            }
        }
        let set = DialectSet::new([dialect.clone()], &mut Governor::new(Budget::default()))
            .expect("a one-dialect set");
        let request =
            ToolRecoveryRequest::sized(Effort::Standard, printed.output.len()).dialects(set);
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
    for calls in values {
        check_calls(output, calls);
        check_reemission(tools, calls);
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

fn main() {
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "probe input exceeds 4096 bytes");
    let control = bytes.first().copied().unwrap_or(0);
    let split = bytes.get(1).copied().unwrap_or(0);
    let output = bytes.get(2..).unwrap_or_default();
    let tools = tools();
    let request = request(control, split, output.len());
    let result = tools.recover(output, &request);
    check_result(&tools, output, &request, &result);
    check_chunking(&tools, output, &request, split >> 3);
    check_exhaustion(&tools, output, &request);
    println!("ok");
}
