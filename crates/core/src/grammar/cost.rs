// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Named work and edit-operator costs of the grammar engine.
//!
//! # Published work bound
//!
//! Recognizing `n` input bytes over a [`DialectSet`](crate::tool_call::DialectSet)
//! charges exactly
//!
//! ```text
//! work = K_LEX·n + K_THREAD·(configuration steps) + K_EVENT·(segment events)
//!      + K_CAPTURE·(captured bytes materialized) + K_ERR·(error configurations explored)
//!      + K_CODEC·(codec scan bytes and lookups) + (JSON recovery units)
//!      + (class-automaton units) + (search key bytes)
//! ```
//!
//! and every term is linear in `n` on the fast path:
//!
//! - each dialect production keeps at most one configuration per program
//!   location (plus one per literal byte position), so configuration steps
//!   per byte are bounded by [`CompiledGrammar::configuration_bound`]
//!   (`K_d = K_THREAD · configuration_bound(d)`), and a delimiter replay
//!   revisits at most [`DialectSet::lookbehind`] bytes;
//! - a class terminal additionally charges its derivative automaton one unit
//!   per byte plus one-time transition-cache admissions;
//! - segment events and captured bytes are bounded by the input length;
//! - an argument codec scans its body a constant number of times (JSON
//!   recovery charges one unit per byte and per value), so codec work is
//!   linear in the slot body.
//!
//! The error term applies only at failure configurations: each configuration
//! popped by the repair search charges [`K_ERR`] (and the search's own
//! per-candidate `Work` and `ScratchBytes`), and the number of explored
//! configurations is capped by the caller's [`Effort`](crate::Effort)
//! budget.
//!
//! `Candidates` counts only readings that genuinely compete for selection:
//! every distinct reading of a completed segment beyond its first, and every
//! distinct segmentation of a contested span (charged by the recognizer).
//! A dialect attempt that dies on its own, and repairs the error search
//! explores and discards, charge `Work` and `ScratchBytes` but no
//! `Candidates`, so allowing more dialects never spends candidates on
//! attempts that cannot compete. Competing readings grow with the calls of
//! a turn, so [`RECOVER_OPENER_CANDIDATE_DIVISOR`] sizes the candidate
//! budget per call opener. Exhaustion is returned as an explicit
//! [`StopReason::BudgetExceeded`](crate::StopReason::BudgetExceeded).
//!
//! Every charge is a function of the byte sequence alone: pushing the same
//! bytes in any split yields identical usage.
//!
//! [`CompiledGrammar::configuration_bound`]: super::CompiledGrammar::configuration_bound
//! [`DialectSet::lookbehind`]: crate::tool_call::DialectSet::lookbehind
//!
//! ```
//! use purrassist_core::grammar::cost;
//! // Repairing markup is always cheaper than rejecting the attempt.
//! assert!(cost::INSERT_CLOSER + cost::DELETE_STRAY_CLOSER < cost::REJECT);
//! // Deleting bytes costs more than inserting the expected closer.
//! assert!(cost::INSERT_CLOSER < cost::DELETE_STRAY_CLOSER);
//! // Reading well-formed argument markup costs nothing; guessing costs more
//! // than correcting syntax.
//! assert_eq!(cost::FRAMING_NEWLINE, 0);
//! assert!(cost::JSON_REPAIR < cost::JSON_GUESS);
//! ```

/// `Work` per input byte for the lexical layer.
///
/// That is: the reasoning-marker automaton, the opener first-byte tables,
/// the capture window, and the Markdown context scan (code fences, inline
/// code, block quotes) that flags calls written as examples.
pub const K_LEX: u64 = 1;

/// `Work` per configuration step: one live configuration consuming one byte
/// (including a byte replayed after a delimiter or a repair).
pub const K_THREAD: u64 = 1;

/// `Work` per segment event (an opener, a completion, an attempt ending, or
/// a reasoning marker) processed by the turn-level segmentation.
pub const K_EVENT: u64 = 1;

/// `Work` per captured byte copied into a recognized value or per history
/// node visited while materializing a reading.
pub const K_CAPTURE: u64 = 1;

/// `Work` per error configuration explored by the repair search.
pub const K_ERR: u64 = 8;

/// `Work` per grammar source byte during compilation, and per compiled
/// table entry (instruction, trie node, automaton state).
pub const K_COMPILE: u64 = 1;

/// `Work` per expression visited and per byte printed by the production
/// printer ([`print`](super::print)), which drives
/// [`witness::generate`](super::witness::generate) and every
/// [`tool_call::emit`](crate::tool_call::emit) printing.
pub const K_PRINT: u64 = 1;
/// `Work` per value [`witness::generate`](super::witness::generate)
/// generates (its printing is charged at [`K_PRINT`]).
pub const K_WITNESS: u64 = 1;
/// `Work` per byte, lookup or node an argument codec visits.
///
/// That is: per slot-body byte an argument codec scans when it finishes a
/// body (the `xml-params` structural pass, the stringified-arguments escape
/// decoding), per declared-name lookup, per tree node, and per history node
/// visited to find the call's name for a codec. Bytes a codec hands to the JSON
/// recovery parser are charged by that parser (see
/// [`crate::json::incremental`]).
pub const K_CODEC: u64 = 1;

/// `ScratchBytes` charged for the boxed state of one `json` codec slot, on
/// top of the engine's per-slot charge (the parser's own growth is charged
/// by the parser).
pub const JSON_CODEC_STATE: u64 = 1024;

/// Cost of substituting a declared closer variant (`</calls>` for `</call>`).
pub const CLOSER_VARIANT: u64 = 1;

/// Cost of accepting a confusable spelling of a literal (ASCII `|` for `｜`).
pub const TERMINAL_CONFUSABLE: u64 = 1;

/// Cost of reinserting an expected closer that equals the generation stop
/// sequence.
pub const CONSUMED_CLOSER: u64 = 1;

/// Cost of inserting a missing closer.
pub const INSERT_CLOSER: u64 = 2;

/// Cost of deleting a stray closer (it discards input bytes).
pub const DELETE_STRAY_CLOSER: u64 = 3;

/// Cost of closing an unclosed reasoning block at the first call opener.
pub const REASONING_UNCLOSED: u64 = 4;

/// Cost of rejecting a call attempt whose markup cannot be repaired.
pub const REJECT: u64 = 16;

/// Cost of ending an open XML parameter value at the opener of a parameter
/// the call's tool declares (`tool_call.markup.missing-parameter-close`):
/// the same as inserting any missing closer.
pub const MISSING_PARAMETER_CLOSE: u64 = INSERT_CLOSER;

/// Cost of consuming one XML framing newline (`tool_call.xml.framing-newline`):
/// zero, since it is the dialect's reading of well-formed markup, not a
/// repair.
pub const FRAMING_NEWLINE: u64 = 0;

/// Cost of decoding stringified arguments (`tool_call.arguments.decode-string`):
/// zero, since the encoding is lossless; such arguments are flagged for
/// mandatory validation instead.
pub const STRINGIFIED_ARGUMENTS: u64 = 0;

/// Cost of one non-guessed tolerant JSON recovery edit inside an argument
/// value (a comment, a requoted string or key, a respelled literal, a
/// missing `:` or `,`, a trailing comma, a closed container).
pub const JSON_REPAIR: u64 = 1;

/// Cost of one guessed tolerant JSON recovery edit inside an argument value.
///
/// That is a missing value, a bare token read as a string, a literal escape,
/// or an unterminated string: it invents content, so it costs more than any
/// purely syntactic repair.
pub const JSON_GUESS: u64 = 4;

/// The longest whitespace run between a call and a stray closer it absorbs.
///
/// That is a run of ASCII whitespace (space, tab, newline, carriage return)
/// between a completed call and a stray closer it absorbs, or between two
/// absorbed closers (`tool_call.markup.stray-closer`).
/// A longer run, and whitespace after the last absorbed closer, stays
/// content. The bound keeps the hold of a completed call finite: its end is
/// decided within this many bytes plus the longest literal, and a dialect
/// set whose grammars declare closers adds it to its lookbehind, so the
/// decision never depends on how the output is chunked.
pub const STRAY_CLOSER_GAP: usize = 32;

/// Scratch bytes charged for each report the recognizer's turn layer queues.
///
/// A report (an engine event or a reasoning marker) is charged when queued
/// and released when processed: the portable 64-bit size of one queued
/// report, asserted never to undercount it on the compiling target.
///
/// Reports normally wait at most [`DialectSet::lookbehind`] bytes; behind a
/// call whose inserted wrapper closer is still being decided
/// (`tool_call.markup.missing-wrapper-close.before-text`) they wait until
/// the decision, so a long wait is bounded by the scratch budget and ends in
/// `BudgetExceeded(ScratchBytes)`, never in uncharged growth. Reports are
/// queued and processed per byte, so the charge is the same for every way
/// the output is chunked.
///
/// [`DialectSet::lookbehind`]: crate::tool_call::DialectSet::lookbehind
pub const QUEUED_REPORT: u64 = 96;

/// Cost of dropping prose inside a call's markup (`tool_call.extract.prose`):
/// the dropped text may carry data, so it costs as much as inserting a
/// missing closer (and the call records a data loss).
pub const EXTRACT_PROSE: u64 = 2;

/// Cost of dropping a code-fence marker inside a call's markup
/// (`tool_call.extract.fence`): a markup-only deletion, like reading a
/// closer variant.
pub const EXTRACT_FENCE: u64 = 1;

/// `Work` per output byte that [`ToolRecoveryRequest::sized`] adds to an
/// effort preset's budget for tool-call recovery.
///
/// Recovering `n` output bytes charges, per byte: one unit to retain the
/// bytes in the receipt, one for the encoding scan, [`K_LEX`] plus the
/// configuration steps of every dialect that is live at that byte, captured
/// bytes ([`K_CAPTURE`]), the argument codec's scan ([`K_CODEC`]) and the
/// JSON recovery parser's units, and the per-byte resolution and
/// digest work. On the fast path (string arguments with no repair) that is
/// 10 to 11 units per byte for Qwen3-Coder XML and Hermes arguments and 15
/// for `OpenAI` stringified arguments, over the built-in dialect set (see
/// `docs/RESOURCES.md`); this constant leaves headroom above them. Repair
/// search beyond that stays bounded by the preset itself.
///
/// [`ToolRecoveryRequest::sized`]: crate::tool_call::ToolRecoveryRequest::sized
pub const RECOVER_WORK_PER_BYTE: u64 = 32;

/// `ScratchBytes` per output byte that [`ToolRecoveryRequest::sized`] adds
/// to an effort preset's budget for tool-call recovery.
///
/// It covers the retained source, the recognizer's capture and codec
/// buffers, the argument value tree, and the recovered call's copy of it:
/// 8 to 9 bytes per byte on the fast path for Qwen3-Coder XML and Hermes
/// arguments, and 45 for `OpenAI` stringified arguments, whose escaped
/// string is also decoded, recorded as the decoding's replacement, and
/// parsed again (see `docs/RESOURCES.md`).
///
/// [`ToolRecoveryRequest::sized`]: crate::tool_call::ToolRecoveryRequest::sized
pub const RECOVER_SCRATCH_PER_BYTE: u64 = 48;

/// The share of an effort preset's `Candidates` that
/// [`ToolRecoveryRequest::sized`] adds per call opener.
///
/// Each opener of the output (counted once per dialect that can open a call
/// there, see [`DialectSet::call_openers`]) adds `⌈preset.candidates /
/// RECOVER_OPENER_CANDIDATE_DIVISOR⌉` candidates.
///
/// Competing readings are charged per call attempt: a second dialect's
/// attempt at a shared opener, a call's second reading (a repeated scalar,
/// a near-miss name), each further segmentation of a contested span. They
/// grow with the number of calls, so a fixed preset that suits an ordinary
/// turn exhausts on a busy one. One eighth of the preset per opener is 16
/// candidates per opener at `Effort::Standard` and 2 at `Effort::Quick`,
/// several times what a call with a repair and a repeated or near-miss
/// name competes for, while keeping the budget linear in the openers (and
/// so in the output length): a single call whose readings multiply still
/// exhausts explicitly.
///
/// [`ToolRecoveryRequest::sized`]: crate::tool_call::ToolRecoveryRequest::sized
/// [`DialectSet::call_openers`]: crate::tool_call::DialectSet::call_openers
pub const RECOVER_OPENER_CANDIDATE_DIVISOR: u64 = 8;
