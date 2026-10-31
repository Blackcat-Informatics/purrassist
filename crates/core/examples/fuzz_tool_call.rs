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
//! `--tools ref` (the `tool-call-ref` target) selects a second fixed tool
//! set compiled with `ToolSet::from_openai`: arrays of `$ref` items, `anyOf`
//! items, member names that collide with an outer parameter, a recursive
//! `$ref` tree, free-form object items, inline object items, and a Draft-07
//! root `$ref` tool. The default (`--tools fixed`) is unchanged: new tools
//! live only in the `ref` set so no earlier seed changes meaning. Every
//! other byte of the protocol and every check below is shared.
//! `--snapshot` prints the complete recovery wire result instead of running
//! the invariant checks and printing `ok`. Cross-revision CI uses this mode
//! with the identical driver, tools and request decoding on both revisions.
//! `--snapshot-restarted` instead accepts a raw restarted-section turn,
//! using its exact two-tool schema, DeepSeek-V3.1, and a fixed finite budget
//! shared by both revisions. Default fuzz input decoding is unchanged.
//!
//! Control byte: bits 0-1 the repair policy (per tool, all `Structural`, all
//! `Tolerant`, per tool), bits 2-3 the generation stop (`Stop`, `Length`,
//! `ToolCalls`, `Unknown`), bits 4-5 the reasoning policy (separate, starting
//! open, as content, separate), bits 6-7 the tool choice (`Auto`, `None`,
//! `Required`, forced `read_file`). Split byte: bits 0-1 the effort
//! (`Exact`, `Quick`, `Standard`, `Deep`), bit 2 the ambiguity policy
//! (`Report`, `Reject`), bits 3-7 the split seed.
//!
//! Strict forced choice: the control bits are full, so the options of
//! [`StrictForced`] come from a trailer, a separate later seed byte. Under
//! the forced tool choice, a last byte of `0xF8..=0xFF` (never valid UTF-8,
//! and the last byte of no earlier seed) is consumed as the trailer instead
//! of being part of the turn: bit 0 `exactly_one`, bit 1 no content, bit 2
//! no reasoning (`0xF8` is the all-lenient degenerate form). Every other
//! input keeps its earlier meaning: the turn is `bytes[2..]` and the choice
//! is the plain `Forced`. A strict choice with no reasoning (bit 2) under
//! the reasoning policy "as content" (bits 4-5 `10`) is a request error
//! (`invalid_input`): the probe asserts that error, with no value, and prints
//! `ok`.
//!
//! Checks (a panic is a fuzz failure):
//! - recovery never panics, names the output's digest, and stays within its
//!   budget; a value outcome completed, `Unresolved` did not; an `Ambiguous`
//!   outcome's readings match the receipt's alternatives, and every
//!   ranked-adjacent pair of them has distinguishing evidence; a `Resolved`
//!   turn records no guess;
//! - (A13) a `Resolved` call carries no guess and no data loss, so no guessed
//!   call on a mutating tool is `Resolved`; `usable()` holds only `Resolved`
//!   calls that name a tool, and none when the tool-choice check failed;
//!   `usable_accepting_guesses()` never holds a guess on a tool that is not
//!   read-only, and is empty too when the check failed (the wire
//!   `usable_calls` counts are then 0, as the emission is); a call embedded
//!   in reasoning is never `Resolved` and never usable;
//! - (F5, F7) the verdict, finish, emission and `finish_reason` agree:
//!   `emitted_calls()` is `usable_accepting_guesses()` (empty when the
//!   tool-choice check failed); the verdict is `Run` exactly when it
//!   is not empty, and the CLI exit code is 0 exactly for `Run` and
//!   `NoCalls`; the `OpenAI` array holds exactly the emitted calls; a finish
//!   with calls implies `Run`, a `Run` turn finishing otherwise is
//!   `Unresolved` beside a rejected, unresolved or ambiguous attempt, and a
//!   failed tool choice finishes `Unresolved` with `NothingUsable`;
//!   `finish_reason` is `"length"` exactly when the stop is a length stop
//!   (whatever the finish) and otherwise `"tool_calls"` exactly when the
//!   `OpenAI` array holds a call;
//!   the wire `verdict` and usable-call counts match;
//! - a strict forced choice that passed implies, by the options drawn:
//!   exactly one call when `exactly_one`; no content byte other than ASCII
//!   whitespace when content is not allowed; no reasoning range when
//!   reasoning is not allowed; no rejected attempt when `exactly_one` or
//!   content is not allowed; and conversely a second call, or a rejected
//!   attempt, under `exactly_one` fails the check;
//! - (F4) a rejected attempt runs past where its grammar failed (the
//!   furthest the recognizer read an attempt of its dialect at its opener)
//!   and (T5) no further than the failure its reason reports, only to a
//!   literal that ends a call of its dialect (T5: a single-byte one only
//!   closing a bracket that opened before the failure, T6: in the markup
//!   of the calls and parts of its split attempt right before it too,
//!   brackets paired by kind, a call or attempt it covers never prose),
//!   (T6) to an opening reasoning marker that cut a literal of its dialect
//!   it had begun, over the maximal ill-formed UTF-8 subpart an
//!   `InvalidUtf8` attempt failed on, (T6) under a length stop to the end
//!   of output when no closer of its dialect follows the failure (an open
//!   call is truncation), to the next call
//!   of its attempt (T6: read or rejected; an attempt of its dialect
//!   begins there), to complete a
//!   literal begun before it, (T3) to the bracket that balances a JSON
//!   value it left open with no prose between (T6: outside the strings
//!   its bytes open from its start, brackets paired by kind), or (T4) to such a closing literal written with one
//!   slip (keeping its delimiters), so prose after a broken opener stays
//!   content;
//! - (F16) no call body is content: the bytes after a rejected attempt
//!   hold no call member (`"name"`, then `:`) before any prose (a token
//!   fragment such as `t_begin|>` is no prose), and an attempt may run over
//!   the call body it began (a call member past the failure, no prose).
//!   Initial markup retains strings opened before failure; a body after
//!   that markup ends reads fresh from its debris and from its member;
//! - (S3/F6) a rejected attempt keeps its own closing markup: when its
//!   raw text does not end with a literal that ends a call of its dialect
//!   (any alternative's, a chained call's own, or one with one slip; not
//!   a single byte), the
//!   content after it up to the next segment holds none;
//! - (F7) a rejected span never ends inside a literal it began;
//! - (F5) every embedded call is offered once, in span order;
//! - (T2) no call, embedded call or rejected attempt holds bytes of a
//!   reasoning range's opener or closer marker in the same reading;
//! - the reasoning blocks (`reasoning_blocks`, unmerged) are ordered and
//!   disjoint, each text lies inside its block, each block inside one
//!   `reasoning` range, the blocks cover exactly the `reasoning` ranges, and
//!   the bytes between a block and its text are a marker pair's opener (none
//!   for the block a `starts_open` turn begins in) and its closer (none when
//!   the block was closed at a call opener or is unterminated);
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
    Rejection, RepairPolicy, Segment, SkipReason, Skipped, StrictForced, ToolCalls, ToolChoice,
    ToolDefinition, ToolRecoveryRequest, ToolSet, Verdict, emit,
};
use purrassist_core::wire::WireValue;
use purrassist_core::{AssistResult, Budget, Effort, Governor, Outcome, StopReason, json};
use std::io::{self, Read};

/// The fixed tool set: the conformance fixtures' tools (`write_file`
/// mutating, with every parameter kind), a read-only tool with no required
/// parameter (the issue's motivating stray-closer case), and a mutating
/// tool with an alias and a repeatable parameter.
fn fixed_tools() -> ToolSet {
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

/// The `--tools ref` set: a second fixed tool set compiled with
/// [`ToolSet::from_openai`] so schema keywords the builder cannot express
/// (`$ref`, `anyOf`, `additionalProperties`, `$defs` roots) reach the
/// nested-parameter code. Every tool is mutating (the adapter's default).
/// `read_file` is kept so the forced tool choice of the header names a tool.
///
/// - `read_file`: a string `path` and an integer `limit`;
/// - `batch_refs`: an outer `name` beside an array of `$ref` items whose
///   `name` collides with it, and a plain array of strings;
/// - `any_items`: an array whose items are `anyOf` an object or a string;
/// - `tree_walk`: a recursive `$ref` tree (`children` items refer to the node);
/// - `free_items`: a `path` and an array of free-form object items
///   (`additionalProperties` true);
/// - `list_records`: an array of inline object items (one member an array
///   of strings), the plain `Param::array(Param::object(..))` shape;
/// - `root_ref`: a root `$ref` into `definitions` under Draft-07, with an
///   array of object items inside the target.
const REF_TOOLS: &str = r##"[
 {"type":"function","function":{"name":"read_file","parameters":{"type":"object",
  "properties":{"path":{"type":"string"},"limit":{"type":"integer"}},"required":["path"]}}},
 {"type":"function","function":{"name":"batch_refs","parameters":{"type":"object",
  "properties":{"name":{"type":"string"},
   "items":{"type":"array","items":{"$ref":"#/$defs/rec"}},
   "tags":{"type":"array","items":{"type":"string"}}},
  "$defs":{"rec":{"type":"object","properties":{"name":{"type":"string"},"count":{"type":"integer"},
   "tags":{"type":"array","items":{"type":"string"}}}}}}}},
 {"type":"function","function":{"name":"any_items","parameters":{"type":"object",
  "properties":{"path":{"type":"string"},
   "choices":{"type":"array","items":{"anyOf":[
    {"type":"object","properties":{"kind":{"type":"string"},"n":{"type":"integer"}}},
    {"type":"string"}]}}}}}},
 {"type":"function","function":{"name":"tree_walk","parameters":{"type":"object",
  "properties":{"root":{"$ref":"#/$defs/node"}},
  "$defs":{"node":{"type":"object","properties":{"label":{"type":"string"},
   "children":{"type":"array","items":{"$ref":"#/$defs/node"}}}}}}}},
 {"type":"function","function":{"name":"free_items","parameters":{"type":"object",
  "properties":{"path":{"type":"string"},
   "keep":{"type":"array","items":{"type":"object","additionalProperties":true}}}}}},
 {"type":"function","function":{"name":"list_records","parameters":{"type":"object",
  "properties":{"owner":{"type":"string"},
   "records":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},
    "uid":{"type":"integer"},"tags":{"type":"array","items":{"type":"string"}}}}}}}}},
 {"type":"function","function":{"name":"root_ref","parameters":{
  "$schema":"http://json-schema.org/draft-07/schema#","$ref":"#/definitions/args",
  "definitions":{"args":{"type":"object","properties":{"path":{"type":"string"},
   "entries":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},
    "size":{"type":"integer"}}}}},"required":["path"]}}}}}
]"##;

fn ref_tools() -> ToolSet {
    ToolSet::from_openai(REF_TOOLS, Budget::default()).expect("the ref tool set compiles")
}

/// The selected tool set and whether to print a cross-revision snapshot.
/// Existing fuzz invocations retain their invariant checks and `ok` output.
/// Unknown options are a usage error, never a silent fallback.
fn tools() -> (ToolSet, ProbeMode) {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] | ["--tools", "fixed"] => (fixed_tools(), ProbeMode::Checks),
        ["--tools", "ref"] => (ref_tools(), ProbeMode::Checks),
        ["--snapshot"] | ["--snapshot", "--tools", "fixed"] => (fixed_tools(), ProbeMode::Snapshot),
        ["--snapshot", "--tools", "ref"] => (ref_tools(), ProbeMode::Snapshot),
        ["--snapshot-restarted"] => (restarted_tools(), ProbeMode::Restarted),
        _ => panic!(
            "usage: fuzz-tool-call [--snapshot] [--tools fixed|ref] or --snapshot-restarted; got {arguments:?}"
        ),
    }
}

enum ProbeMode {
    Checks,
    Snapshot,
    Restarted,
}

/// The same tool definitions as `tool_call_cost_support::restarted_deepseek_tools`.
fn restarted_tools() -> ToolSet {
    ToolSet::compile(
        [
            ToolDefinition::new("delete_path")
                .effects(Effects::Mutating)
                .param("path", Param::string().required()),
            ToolDefinition::new("read_file")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required()),
        ],
        Budget::default(),
    )
    .expect("restart tools compile")
}

/// The request knobs the two header bytes select.
fn request(control: u8, split: u8, strict: Option<u8>, output: &[u8]) -> ToolRecoveryRequest {
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
        _ => strict.map_or_else(
            || ToolChoice::Forced("read_file".into()),
            |bits| {
                ToolChoice::ForcedStrict(StrictForced {
                    name: "read_file".into(),
                    exactly_one: bits & 1 != 0,
                    allow_content: bits & 2 == 0,
                    allow_reasoning: bits & 4 == 0,
                })
            },
        ),
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
/// names its dialect among its sorted candidates. Unparsed shared shapes retain
/// every family candidate and remain inside one content range.
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
            if family.dialect().is_some() {
                assert!(rejected.candidate_dialects.is_empty(), "{rejected:?}");
            } else {
                assert_eq!(
                    rejected.candidate_dialects,
                    family.dialects(),
                    "{rejected:?}"
                );
            }
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
        .flat_map(|end| end.call_closers().iter())
        .filter(|closer| closer.len() > 1)
        .map(|bytes| bytes.as_slice().to_vec())
        .collect();
    closers.sort();
    closers.dedup();
    closers
}

/// F16: whether the bytes of a rejected attempt from `start` (failed at
/// `failed`) to `end` hold, past the failure, a call's member (`"name"`,
/// then `:`) and no prose: the body of the call object the attempt began
/// (its opening brace lost to a cut special token, or left open), which is
/// the attempt's markup.
///
/// Prose is judged exactly as `check_no_call_body` and the library judge it
/// (INTEGRATION.md, stage-3 pass 5, F16: "a call body after the attempt's
/// markup ... is covered the same way when only markup debris comes between
/// (... never prose, read with strings opening and closing where the markup
/// ended), and a member's spelling begins a body wherever it is written"):
/// the bytes from the failure to the first member, and the body from that
/// member on, are each read fresh. Reading them with the quotes counted from
/// the attempt's start instead reads the same bytes differently after a
/// broken string (`"id": "m4N8: {"path": "notes.txt"` makes `notes.txt` a
/// word), so this clause and F16 could admit no span at all; the fresh
/// reading is the one the F16 text states.
fn call_body(output: &[u8], start: usize, failed: usize, end: usize) -> bool {
    let after = &output[failed.clamp(start, end)..end];
    (0..after.len())
        .find(|&at| member_at(&after[at..]))
        .is_some_and(|member| !prose(&after[..member], 0, &[]) && !prose(&after[member..], 0, &[]))
}

/// F16's initial markup walk retains a string that began before failure.
/// Unlike a body after the markup ended, this walk does not restart its
/// quotes at the failure. A later member must begin outside that original
/// string, with no prose before it or in the body read fresh from its key.
fn initial_call_body(output: &[u8], start: usize, failed: usize, end: usize) -> bool {
    let bytes = &output[start..end];
    let failed = failed.clamp(start, end) - start;
    let (mut quoted, mut escaped, mut inside_at_failure) = (false, false, false);
    for (at, &byte) in bytes.iter().enumerate() {
        if at == failed {
            // When the failure is outside a string, call_body already
            // supplies the fresh reading; this exception adds nothing.
            inside_at_failure = quoted;
            if !inside_at_failure {
                return false;
            }
        }
        if at >= failed && inside_at_failure && !quoted && member_at(&bytes[at..]) {
            return !prose(&bytes[..at], failed, &[]) && !prose(&bytes[at..], 0, &[]);
        }
        if quoted {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => quoted = false,
                _ => {}
            }
        } else if byte == b'"' {
            quoted = true;
        }
    }
    false
}

/// Whether `bytes` begin with a call's member: `"name"`, then `:`.
fn member_at(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\"name\"")
        && bytes[6..]
            .iter()
            .find(|byte| !byte.is_ascii_whitespace())
            .is_some_and(|byte| *byte == b':')
}

/// F16: a call body is never content. The bytes after a rejected attempt,
/// up to the next call, reasoning block or other attempt, hold no call
/// member (`"name"`, then `:`) before any prose: the body of a call object
/// the attempt began (its opening brace lost to a cut special token, as in
/// `<|python_tag|>t_begin|>"name": ...}`) is the attempt's markup, which its
/// rejection covers (issue #3: "Never accept trailing free text as an
/// argument" has its mirror in no call markup left as assistant text).
fn check_no_call_body(output: &[u8], calls: &ToolCalls) {
    for rejected in &calls.rejected {
        if matches!(rejected.reason, Rejection::UnsupportedDialect { .. }) {
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
        let after = &output[end..next];
        let member = (0..after.len()).find(|&at| member_at(&after[at..]));
        if let Some(at) = member {
            assert!(
                prose(&after[..at], 0, &[]),
                "a call body after rejected attempt {:?} is content: {:?}",
                rejected.span,
                String::from_utf8_lossy(after)
            );
        }
    }
}

/// S3/F6: a rejected attempt keeps its own closing markup. For an attempt
/// a parser made, when its raw text does not end with a literal that ends
/// a call of its dialect ([`dialect_closers`], or one written with one
/// slip, which ends it as the closer does), the content after it, up to
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
        // (T4) A closer written with one slip ends the attempt as the
        // closer does (see `ends_with_slip`).
        if closers
            .iter()
            .any(|closer| rejected.raw.ends_with(closer) || ends_with_slip(&rejected.raw, closer))
        {
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
            .map_or(0, purrassist_core::ScratchVec::len);
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
    check_no_call_body(output, calls);
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
    assert_eq!(emitted, usable, "emitted_calls() is not the usable calls");
    if !calls.tool_choice.passed {
        // A3: nothing of a failed turn is usable, as nothing is emitted.
        assert!(emitted.is_empty(), "a failed tool choice emits calls");
        assert!(
            calls.usable().is_empty() && usable.is_empty(),
            "a failed tool choice leaves usable calls"
        );
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
        let request = ToolRecoveryRequest::sized_for(Effort::Standard, &printed.output, &set);
        let context = format!(
            "{} re-emission {:?}",
            dialect.id(),
            String::from_utf8_lossy(&printed.output)
        );
        let again = single(
            tools.recover(&printed.output, &request, &set).outcome,
            &context,
        );
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
        let settled = single(
            tools.recover(&reprinted.output, &request, &set).outcome,
            &context,
        );
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
        check_strict(output, request, calls);
    }
}

/// The options of a strict forced choice, as drawn: a passed check implies
/// exactly one call (when `exactly_one`), no content byte other than ASCII
/// whitespace (when content is not allowed) and no reasoning range (when
/// reasoning is not allowed); a second call under `exactly_one` fails it.
fn check_strict(output: &[u8], request: &ToolRecoveryRequest, calls: &ToolCalls) {
    let ToolChoice::ForcedStrict(strict) = &request.tool_choice else {
        return;
    };
    if strict.exactly_one && calls.calls.len() > 1 {
        assert!(
            !calls.tool_choice.passed,
            "{} calls pass exactly-one",
            calls.calls.len()
        );
    }
    if strict.exactly_one && !calls.rejected.is_empty() {
        assert!(
            !calls.tool_choice.passed,
            "{} rejected attempts pass exactly-one",
            calls.rejected.len()
        );
    }
    if !calls.tool_choice.passed {
        return;
    }
    if strict.exactly_one || !strict.allow_content {
        assert!(
            calls.rejected.is_empty(),
            "a passed exactly-one or no-content check holds rejected attempts {:?}",
            calls
                .rejected
                .iter()
                .map(|r| r.span.clone())
                .collect::<Vec<_>>()
        );
    }
    if strict.exactly_one {
        assert_eq!(calls.calls.len(), 1, "a passed exactly-one check");
    }
    if !strict.allow_content {
        for range in &calls.content {
            assert!(
                output[range.clone()].iter().all(u8::is_ascii_whitespace),
                "a passed no-content check holds content {range:?}"
            );
        }
    }
    if !strict.allow_reasoning {
        assert!(
            calls.reasoning.is_empty(),
            "a passed no-reasoning check holds reasoning"
        );
    }
}

/// Every rejected attempt the recognizer read, in any reading of the
/// turn: its start, dialect, and where its grammar failed.
fn read_spans(
    tools: &ToolSet,
    output: &[u8],
    request: &ToolRecoveryRequest,
) -> Vec<(Range<usize>, DialectId)> {
    let Ok(turn) = recognize(tools, request, DialectSet::built_in(), &[output]).0 else {
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
/// attempt of its dialect at its opener, and no further than the failure
/// its reason reports) only to a literal that ends a
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
        let Some(read_to) = read
            .iter()
            .filter(|(span, read)| span.start == rejected.span.start && read == dialect)
            .map(|(span, _)| span.end)
            .max()
        else {
            continue;
        };
        // T5: the grammar read no further than where the attempt failed,
        // whatever span the engine reported (an attempt that read on past a
        // skipped call reports the whole stretch it scanned).
        let failed = match rejected.reason {
            Rejection::Malformed { at }
            | Rejection::InvalidUtf8 { at }
            | Rejection::Overlapped { at } => read_to.min(at.max(rejected.span.start)),
            _ => read_to,
        };
        let end = rejected.span.end;
        if end <= failed {
            continue;
        }
        let grammar = dialect.grammar();
        // T6: the markup a part of a split attempt continues (the calls and
        // rejected parts of its dialect right before it, back to back) is
        // read from where that run begins: a bracket it opened (a Mistral
        // list's `[`) is structure the attempt framed.
        let chain = chain_start(calls, dialect, rejected.span.start);
        let nested = nested_segments(calls, failed, end, chain);
        // A single-byte closer (a JSON bracket or separator, which prose
        // writes too) counts only as structure the attempt framed itself:
        // its markup ends by closing every bracket it opened (see
        // `structural_end`).
        let closed = grammar
            .call_ends()
            .iter()
            .flat_map(purrassist_core::grammar::CallEnd::call_closers)
            .any(|closer| {
                let trimmed = rejected.raw.trim_ascii_end();
                if closer.len() > 1 {
                    trimmed.ends_with(closer)
                } else {
                    structural_end(&output[chain..end], failed - chain, &nested)
                }
            });
        let completed = grammar.call_literals().iter().any(|spelling| {
            end >= spelling.len()
                && end - spelling.len() < failed
                && end - spelling.len() >= rejected.span.start
                && output[end - spelling.len()..end] == spelling[..]
        });
        // T3: or to the bracket that balances a JSON value the attempt left
        // open: its bytes open more brackets than they close (outside JSON
        // strings) at every point from where it failed until its last byte,
        // which balances them, with no prose between (prose is no JSON).
        let balanced = balanced_end(output, rejected.span.start, failed, end);
        // T4: or to a closer of its dialect written with one slip (one
        // byte inserted, deleted or substituted, or two adjacent bytes
        // swapped, keeping the closer's delimiters), which ends a rejected
        // attempt's markup as it ends a call's.
        let misspelled = grammar
            .call_ends()
            .iter()
            .flat_map(purrassist_core::grammar::CallEnd::call_closers)
            .filter(|closer| closer.len() > 1)
            .any(|closer| ends_with_slip(&rejected.raw, closer));
        // T4/T5: or to the next call of its attempt (a skipped call left
        // without its closer runs to where the next call's opener begins),
        // read or rejected: an attempt begins only at an opener of its
        // dialect, so either shows that opener begins exactly there.
        let next_call = calls
            .calls
            .iter()
            .any(|call| call.span.start == end && &call.dialect == dialect)
            || calls
                .rejected
                .iter()
                .any(|other| other.span.start == end && other.dialect.as_ref() == Some(dialect));
        // T6: or to an opening reasoning marker that cut a literal of its
        // dialect the attempt had begun (see `cut_by_marker`).
        let cut = cut_by_marker(
            output,
            calls,
            dialect.grammar().call_literals(),
            rejected.span.start,
            failed,
            end,
        );
        // T6: or over the invalid UTF-8 it failed on: the maximal ill-formed
        // subpart at its reported failure (`InvalidUtf8`).
        let invalid = matches!(rejected.reason, Rejection::InvalidUtf8 { at } if at == failed)
            && invalid_subpart(&output[failed..]).is_some_and(|length| failed + length == end);
        // T6: or, under a length stop, to the end of output when the
        // attempt was still open at the cut (no closer of its dialect after
        // where it failed): the stop reads an open call as truncation, so
        // the bytes up to the cut are the attempt's.
        let truncated = calls.length_stop
            && end == output.len()
            && !dialect_closers(dialect).iter().any(|closer| {
                output[failed..end]
                    .windows(closer.len())
                    .any(|window| window == &closer[..])
            });
        // F16: or over the call body it began (a call's member past the
        // failure, no prose): its markup, however it ends.
        let body = call_body(output, rejected.span.start, failed, end)
            || initial_call_body(output, rejected.span.start, failed, end);
        assert!(
            closed
                || body
                || completed
                || balanced
                || misspelled
                || next_call
                || cut
                || invalid
                || truncated,
            "rejected attempt {:?} runs past where it failed ({failed}) without its closer: {:?}",
            rejected.span,
            String::from_utf8_lossy(&output[failed..end])
        );
    }
}

/// The length of the maximal ill-formed UTF-8 subpart `bytes` begin with
/// (the rest of `bytes` when they end inside a truncated sequence), or
/// `None` when they begin with valid UTF-8.
fn invalid_subpart(bytes: &[u8]) -> Option<usize> {
    let error = core::str::from_utf8(bytes).err()?;
    (error.valid_up_to() == 0).then(|| error.error_len().unwrap_or(bytes.len()))
}

/// Where the run of `dialect`'s calls and rejected parts that ends at
/// `start`, back to back, begins (`start` when none ends there).
fn chain_start(calls: &ToolCalls, dialect: &DialectId, start: usize) -> usize {
    let mut chain = start;
    while let Some(earlier) = calls
        .calls
        .iter()
        .filter(|call| &call.dialect == dialect)
        .map(|call| &call.span)
        .chain(
            calls
                .rejected
                .iter()
                .filter(|other| other.dialect.as_ref() == Some(dialect))
                .map(|other| &other.span),
        )
        .find(|span| span.end == chain && span.start < chain)
        .map(|span| span.start)
    {
        chain = earlier;
    }
    chain
}

/// T3: whether the bytes of a rejected attempt from `start` (failed at
/// `failed`) to `end` open more brackets than they close (outside JSON
/// strings, paired by kind: a `}` closes the innermost open `{` and any
/// bracket opened inside it, never a `[` alone) at every point from the
/// failure until the last byte, which balances them, with no prose
/// between.
fn balanced_end(output: &[u8], start: usize, failed: usize, end: usize) -> bool {
    // The open brackets by kind, innermost last: a closer closes the
    // innermost open bracket of its kind (and any opened inside it); with
    // none of its kind open it closes nothing.
    let mut open: Vec<u8> = Vec::new();
    let (mut quoted, mut escaped, mut unbalanced) = (false, false, true);
    for (offset, &byte) in output[start..end].iter().enumerate() {
        let at = start + offset;
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
                b'{' | b'[' => open.push(byte),
                b'}' | b']' => {
                    let kind = if byte == b'}' { b'{' } else { b'[' };
                    if let Some(index) = open.iter().rposition(|&opener| opener == kind) {
                        open.truncate(index);
                    }
                }
                _ => {}
            }
        }
        if at >= failed && at + 1 < end && open.is_empty() {
            unbalanced = false;
        }
    }
    unbalanced
        && open.is_empty()
        && matches!(output[end - 1], b'}' | b']')
        && !prose(&output[start..end], failed.clamp(start, end) - start, &[])
}

/// Whether a rejected attempt from `start` that failed at `failed` and ends
/// at `end` ran into an opening reasoning marker that cut a literal of its
/// dialect (`literals`) it had begun at or before where it failed: a
/// reasoning block begins at `end`, and the bytes from the literal's start
/// to `end` are a proper prefix of it (`</functio` before `<think>`). A
/// rejected attempt that ran over an opening marker ends at it.
fn cut_by_marker(
    output: &[u8],
    calls: &ToolCalls,
    literals: &[purrassist_core::ScratchVec<u8>],
    start: usize,
    failed: usize,
    end: usize,
) -> bool {
    calls.reasoning.iter().any(|block| block.start == end)
        && literals.iter().any(|spelling| {
            (1..spelling.len()).any(|length| {
                end >= length
                    && end - length >= start
                    && end - length <= failed
                    && spelling.starts_with(&output[end - length..end])
            })
        })
}

/// The calls and attempts (any dialect) a rejected attempt's markup covers
/// between where its grammar failed and `end`, relative to `chain`:
/// segments the markup passes over, never read as prose (see
/// `structural_end`).
fn nested_segments(
    calls: &ToolCalls,
    failed: usize,
    end: usize,
    chain: usize,
) -> Vec<Range<usize>> {
    calls
        .calls
        .iter()
        .map(|call| &call.span)
        .chain(calls.rejected.iter().map(|other| &other.span))
        .filter(|span| span.start >= failed && span.end <= end && span.start < span.end)
        .map(|span| span.start - chain..span.end - chain)
        .collect()
}

/// Whether `bytes` hold prose outside JSON strings: a bare word (beginning
/// at a letter or `_` that no letter, digit or `_` precedes; not `true`,
/// `false` or `null`) followed by neither JSON punctuation
/// (`,` `:` `}` `]` `"`, after spaces) nor the end, i.e. by another word or
/// sentence punctuation, at or after `from`: strings open and close from
/// the start of `bytes` (where the attempt's scan starts), so a failure
/// inside a JSON string reads the rest of the string as text, never as
/// words. `nested` (offsets into `bytes`, in any order) are
/// calls and attempts a rejected attempt's markup covers (T6): markup, not
/// prose, so a word right before one is followed by no prose, and no word
/// inside one is read (strings still open and close across it).
fn prose(bytes: &[u8], from: usize, nested: &[Range<usize>]) -> bool {
    let (mut quoted, mut escaped, mut at) = (false, false, 0);
    while at < bytes.len() {
        let byte = bytes[at];
        let inside = at < from || nested.iter().any(|span| span.contains(&at));
        if quoted {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => quoted = false,
                _ => {}
            }
            at += 1;
            continue;
        }
        if byte == b'"' {
            quoted = true;
        } else if (byte.is_ascii_alphabetic() || byte == b'_')
            && !inside
            && (at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_'))
        {
            // A word ends where a nested segment begins.
            let limit = nested
                .iter()
                .map(|span| span.start)
                .filter(|start| *start > at)
                .min()
                .unwrap_or(bytes.len());
            let word = bytes[at..limit]
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
                .count();
            let after = bytes[at + word..limit]
                .iter()
                .find(|byte| !matches!(byte, b' ' | b'\t'));
            // The tail of a special token (`t_begin|>`) is markup.
            let rest = &bytes[at + word..limit];
            let fragment = rest.starts_with(b"|") || rest.starts_with("｜".as_bytes());
            if !matches!(&bytes[at..at + word], b"true" | b"false" | b"null")
                && !fragment
                && !matches!(after, None | Some(b',' | b':' | b'}' | b']' | b'"'))
            {
                return true;
            }
            at += word;
            continue;
        }
        at += 1;
    }
    false
}

/// Whether `raw` ends with JSON structure its grammar read (outside
/// strings): after trailing whitespace, a separator `,` and closing
/// brackets that close what `raw` did not open (an enclosing list), its
/// last byte is a `}` or `]` closing a bracket of its kind (brackets pair
/// by kind) that opened before `failed`
/// (an offset into `raw`, where the grammar stopped reading), or after it
/// with no prose from the failure to the close: a bracket prose opened
/// after the failure (`oops. Sorry ... {a, b}`) frames nothing the
/// attempt read. `nested` (offsets into `raw`) are the calls and attempts
/// the markup covers, never prose (see `prose`).
fn structural_end(raw: &[u8], failed: usize, nested: &[Range<usize>]) -> bool {
    let (mut quoted, mut escaped) = (false, false);
    let mut open: Vec<(usize, u8)> = Vec::new();
    // For each byte, the offset of the bracket it closes (outside strings):
    // the nearest open bracket of its kind (closing any opened inside it),
    // or none `raw` opened (`Some(None)`) when no bracket of its kind is
    // open, so a stray `}` never stands for the `]` of a list.
    let mut closes: Vec<Option<Option<usize>>> = Vec::with_capacity(raw.len());
    for (offset, &byte) in raw.iter().enumerate() {
        if quoted {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => quoted = false,
                _ => {}
            }
            closes.push(None);
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' | b'[' => open.push((offset, byte)),
            b'}' | b']' => {
                let kind = if byte == b'}' { b'{' } else { b'[' };
                let pair = open
                    .iter()
                    .rposition(|&(_, opener)| opener == kind)
                    .map(|index| {
                        let opened = open[index].0;
                        open.truncate(index);
                        opened
                    });
                closes.push(Some(pair));
                continue;
            }
            _ => {}
        }
        closes.push(None);
    }
    let mut at = raw.len();
    while at > 0 {
        let byte = raw[at - 1];
        let closes_outer = closes[at - 1] == Some(None);
        if byte.is_ascii_whitespace() || byte == b',' || closes_outer {
            at -= 1;
        } else {
            break;
        }
    }
    // The bracket opened before the failure, or after it with no prose
    // from the failure to its close (a JSON value after stray bytes).
    let inside: Vec<Range<usize>> = nested
        .iter()
        .filter(|span| span.end <= at)
        .cloned()
        .collect();
    at > 0
        && matches!(closes[at - 1], Some(Some(opened))
            if opened < failed || !prose(&raw[..at], failed.min(at), &inside))
}

/// Whether `raw` ends with `closer` written with one slip (see `slipped`).
fn ends_with_slip(raw: &[u8], closer: &[u8]) -> bool {
    (closer.len().saturating_sub(1)..=closer.len() + 1)
        .any(|length| length <= raw.len() && slipped(&raw[raw.len() - length..], closer))
}

/// Whether `token` is `closer` written with one slip: not `closer` itself,
/// the same leading and trailing runs of non-alphanumeric bytes, and inner
/// bytes one insertion, deletion, substitution or adjacent swap apart.
fn slipped(token: &[u8], closer: &[u8]) -> bool {
    let lead = closer
        .iter()
        .take_while(|byte| !byte.is_ascii_alphanumeric())
        .count();
    let trail = closer[lead..]
        .iter()
        .rev()
        .take_while(|byte| !byte.is_ascii_alphanumeric())
        .count();
    if token == closer
        || token.len() < lead + trail
        || token[..lead] != closer[..lead]
        || !token.ends_with(&closer[closer.len() - trail..])
    {
        return false;
    }
    let (a, b) = (
        &token[lead..token.len() - trail],
        &closer[lead..closer.len() - trail],
    );
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[prefix..], &b[prefix..]);
    match (a.len(), b.len()) {
        (x, y) if x == y => {
            !a.is_empty()
                && (a[1..] == b[1..]
                    || (a.len() >= 2 && a[0] == b[1] && a[1] == b[0] && a[2..] == b[2..]))
        }
        (x, y) if x + 1 == y => a == &b[1..],
        (x, y) if y + 1 == x => &a[1..] == b,
        _ => false,
    }
}

/// Recognize `output` pushed in `chunks`.
fn recognize(
    tools: &ToolSet,
    request: &ToolRecoveryRequest,
    dialects: &DialectSet,
    chunks: &[&[u8]],
) -> (Result<RecognizedTurn, StopReason>, Usage) {
    let mut governor = Governor::new(request.budget);
    let result = Recognizer::with_tools(dialects, tools, request.reasoning, &mut governor)
        .and_then(|mut recognizer| {
            for chunk in chunks {
                recognizer.push(chunk, &mut governor)?;
            }
            recognizer.finish(request.stop.clone(), &mut governor)
        });
    (result, governor.usage())
}

fn check_chunking(tools: &ToolSet, output: &[u8], request: &ToolRecoveryRequest, seed: u8) {
    let whole = recognize(tools, request, DialectSet::built_in(), &[output]);
    let position = usize::from(seed) * (output.len() + 1) / 32;
    let position = position.min(output.len());
    let halves = recognize(
        tools,
        request,
        DialectSet::built_in(),
        &[&output[..position], &output[position..]],
    );
    let pieces: Vec<&[u8]> = output.chunks(usize::from(seed % 13) + 1).collect();
    let chunked = recognize(tools, request, DialectSet::built_in(), &pieces);
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
    let result = tools.recover(
        output,
        &request,
        purrassist_core::tool_call::DialectSet::built_in(),
    );
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
    dialects: &DialectSet,
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
        dialects,
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
    for dialect in dialects.dialects() {
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
fn check_markers_not_calls(output: &[u8], dialects: &DialectSet, result: &AssistResult<ToolCalls>) {
    let readings: Vec<&ToolCalls> = match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => vec![calls],
        Outcome::Ambiguous(values) => values.iter().collect(),
        Outcome::Unresolved => Vec::new(),
    };
    let mut pairs: Vec<(&[u8], &[u8])> = Vec::new();
    for dialect in dialects.dialects() {
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

/// The marker-free reasoning text of every reading: the blocks are ordered
/// and disjoint, each `text` lies inside its `span`, each `span` inside one
/// `reasoning` range, and the spans cover exactly the bytes the `reasoning`
/// ranges cover. The bytes between a block's `span` and its `text` are
/// markers: the head is an opener spelling of an allowed dialect (empty only
/// for the block a `starts_open` turn begins in, at offset 0), and the tail
/// is that pair's closer or empty (a block closed at a call opener, or
/// unterminated). A `text` may itself start or end with a marker spelling (a
/// second `<think>` inside a block is text); only the stripped edges are
/// markers.
fn check_reasoning_blocks(
    output: &[u8],
    request: &ToolRecoveryRequest,
    dialects: &DialectSet,
    result: &AssistResult<ToolCalls>,
) {
    let readings: Vec<&ToolCalls> = match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => vec![calls],
        Outcome::Ambiguous(values) => values.iter().collect(),
        Outcome::Unresolved => Vec::new(),
    };
    let mut pairs: Vec<(&[u8], &[u8])> = Vec::new();
    for dialect in dialects.dialects() {
        for (open, close) in dialect.grammar().reasoning_markers() {
            pairs.push((open, close));
        }
    }
    let starts_open = request.reasoning == ReasoningPolicy::Separate { starts_open: true };
    for calls in readings {
        let blocks = &calls.reasoning_blocks;
        if request.reasoning == ReasoningPolicy::AsContent {
            assert!(
                blocks.is_empty() && calls.reasoning.is_empty(),
                "reasoning as content records blocks: {blocks:?}"
            );
        }
        let mut covered = vec![false; output.len()];
        for range in &calls.reasoning {
            covered[range.clone()].fill(true);
        }
        let mut blocked = vec![false; output.len()];
        for block in blocks {
            check_range(output, &block.span, "reasoning block span");
            check_range(output, &block.text, "reasoning block text");
            assert!(
                block.text.start <= block.text.end,
                "reasoning text {:?} is reversed",
                block.text
            );
            assert!(
                block.span.start <= block.text.start && block.text.end <= block.span.end,
                "reasoning text {:?} lies outside its block {:?}",
                block.text,
                block.span
            );
            assert!(
                calls
                    .reasoning
                    .iter()
                    .any(|range| range.start <= block.span.start && block.span.end <= range.end),
                "reasoning block {:?} lies in no reasoning range {:?}",
                block.span,
                calls.reasoning
            );
            blocked[block.span.clone()].fill(true);
            let head = &output[block.span.start..block.text.start];
            let tail = &output[block.text.end..block.span.end];
            if head.is_empty() {
                assert!(
                    starts_open && block.span.start == 0,
                    "reasoning block {:?} has no opener outside a starts_open turn",
                    block.span
                );
            }
            assert!(
                head.is_empty()
                    && (tail.is_empty() || pairs.iter().any(|(_, close)| *close == tail))
                    || pairs.iter().any(|(open, close)| {
                        *open == head && (tail.is_empty() || *close == tail)
                    }),
                "reasoning block {:?} (text {:?}) is not delimited by a marker pair: head {head:?}, tail {tail:?}",
                block.span,
                block.text
            );
        }
        assert_eq!(
            covered, blocked,
            "the reasoning blocks {blocks:?} do not cover exactly the reasoning ranges {:?}",
            calls.reasoning
        );
        assert!(
            blocks
                .windows(2)
                .all(|pair| pair[0].span.end <= pair[1].span.start),
            "reasoning blocks are out of order or overlap: {blocks:?}"
        );
    }
}

fn main() {
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "probe input exceeds 4096 bytes");
    let (tools, mode) = tools();
    if matches!(mode, ProbeMode::Restarted) {
        let request = ToolRecoveryRequest::new(Effort::Standard).budget(Budget {
            work: 100_000_000,
            candidates: 1_000_000,
            input_bytes: 4096,
            scratch_bytes: 256 * 1024 * 1024,
            depth: 128,
            evidence_entries: 1_000_000,
            helper_calls: 0,
        });
        let dialects = DialectSet::new(
            [DialectId::DeepSeekV31],
            &mut Governor::new(Budget::default()),
        )
        .expect("DeepSeek dialect");
        let result = tools.recover(&bytes, &request, &dialects);
        println!("{}", result.to_node().to_json());
        return;
    }
    let control = bytes.first().copied().unwrap_or(0);
    let split = bytes.get(1).copied().unwrap_or(0);
    let strict = bytes
        .last()
        .copied()
        .filter(|last| bytes.len() > 2 && control >> 6 == 3 && *last >= 0xF8)
        .map(|last| last & 7);
    let end = bytes.len() - usize::from(strict.is_some());
    let output = bytes.get(2..end).unwrap_or_default();
    let request = request(control, split, strict, output);
    let result = tools.recover(
        output,
        &request,
        purrassist_core::tool_call::DialectSet::built_in(),
    );
    if matches!(mode, ProbeMode::Snapshot) {
        println!("{}", result.to_node().to_json());
        return;
    }
    if let ToolChoice::ForcedStrict(StrictForced {
        allow_reasoning: false,
        ..
    }) = &request.tool_choice
        && request.reasoning == ReasoningPolicy::AsContent
    {
        // G15: forbidding reasoning while reasoning markers are content is a
        // contradictory request, an error rather than a vacuous pass.
        assert_eq!(result.outcome, Outcome::Unresolved, "{result:?}");
        assert!(
            matches!(result.receipt.stopped, StopReason::InvalidInput(_)),
            "{:?}",
            result.receipt.stopped
        );
        assert_eq!(result.receipt.source, Digest::of(output));
        println!("ok");
        return;
    }
    check_result(&tools, output, &request, &result);
    check_reasoning_keeps_calls(&tools, output, &request, DialectSet::built_in(), &result);
    check_markers_not_calls(output, DialectSet::built_in(), &result);
    check_reasoning_blocks(output, &request, DialectSet::built_in(), &result);
    check_chunking(&tools, output, &request, split >> 3);
    check_exhaustion(&tools, output, &request);
    println!("ok");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Initial markup retains the malformed id's string until its real
    /// closing quote; later call members justify the rejected body, not prose.
    #[test]
    fn initial_body_oracle_preserves_pre_failure_strings_without_absorbing_prose() {
        let seed = b"[\xe6#[TOOL_CALLS][\t{\"id\":\"call_90d71.50ated \\n  block\\n\"}\t\t,\"name\"\t:\"list_t\"},\t\t\"name\"\t:\t\t\"get_weathe<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c>\x96ed \xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c>\x96ed \\n  block\\n\"}\t\t,\"name\"\t:\"list_di";
        let output = &seed[2..];
        let tools = fixed_tools();
        let request = request(seed[0], seed[1], None, output);
        let result = tools.recover(output, &request, DialectSet::built_in());
        let (Outcome::Resolved(calls) | Outcome::BestGuess(calls)) = result.outcome else {
            panic!("the malformed input retains one rejected reading");
        };
        let read = read_spans(&tools, output, &request);
        assert!(read.contains(&(1..39, DialectId::MistralV3)));
        let rejected = calls
            .rejected
            .iter()
            .find(|part| part.span.start == 1)
            .expect("the original Mistral attempt is rejected");
        assert_eq!(rejected.span, 1..179);
        assert_eq!(rejected.reason, Rejection::Malformed { at: 39 });
        assert!(!call_body(output, 1, 39, 179));
        assert!(initial_call_body(output, 1, 39, 179));
        check_rejected_extension(output, &calls, &read);

        for invalid in [
            br#"[TOOL_CALLS][{"id":"bad value"} Independent prose. "name":"f"}]"#.as_slice(),
            br#"[TOOL_CALLS][{"id":"bad value", "name":"f"}] Independent prose."#,
            br#"[TOOL_CALLS][{"id":"bad "name": "f"}]"#,
            br#"[TOOL_CALLS][{"id":"bad value"}]"#,
        ] {
            let failed = invalid
                .windows(3)
                .position(|word| word == b"bad")
                .expect("each boundary neighbor fails within its id")
                + 3;
            assert!(
                !initial_call_body(invalid, 0, failed, invalid.len()),
                "the initial-body witness must refuse {invalid:?}"
            );
        }
    }

    /// F4 must reject an overlong span even when F16 admits the real body
    /// after a broken string. This injects the span-extension bug into the
    /// actual recovery result rather than weakening the oracle's exception.
    #[test]
    fn fresh_body_oracle_rejects_span_extended_over_prose() {
        let seed = b"[\x05\x91Let me check both.\x0a[TOOL_CALLS][{\"name\": \"get_weather\", \"arguments\": {\"city\": \"Oslo\"}, \"id\": \"Wq7kP2xZr\"}, {\"id\": \"m4N8: {\"path\": \"notes.txt\", \"limit\": 10}, \"name\": \"read_file\"}]]";
        let mut output = seed[2..].to_vec();
        output.extend_from_slice(b" This is independent prose.");
        let tools = fixed_tools();
        let request = request(seed[0], seed[1], None, &output);
        let result = tools.recover(
            &output,
            &request,
            purrassist_core::tool_call::DialectSet::built_in(),
        );
        let (Outcome::Resolved(mut calls) | Outcome::BestGuess(mut calls)) = result.outcome else {
            panic!("the regression has one recoverable reading");
        };
        let rejected = calls
            .rejected
            .iter()
            .find(|part| part.span.start == 108)
            .expect("the malformed second call is rejected");
        assert_eq!(rejected.span, 108..178);
        assert!(call_body(&output, 108, 121, 178));
        let read = [(
            rejected.span.clone(),
            rejected.dialect.clone().expect("parsed dialect"),
        )];
        assert!(
            std::panic::catch_unwind(|| check_rejected_extension(&output, &calls, &read)).is_ok(),
            "the actual span must pass before injecting the overrun"
        );
        let rejected = calls
            .rejected
            .iter_mut()
            .find(|part| part.span.start == 108)
            .expect("the same rejected part");
        rejected.span.end = output.len();
        rejected.raw = output[rejected.span.clone()].to_vec();
        let panic = std::panic::catch_unwind(|| check_rejected_extension(&output, &calls, &read))
            .expect_err("the injected overrun must fail");
        let message = panic
            .downcast_ref::<String>()
            .expect("oracle panic message");
        assert!(message.contains("runs past where it failed"));
    }
}
