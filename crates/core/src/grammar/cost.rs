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
//! per-candidate `Work`, `Candidates` and `ScratchBytes`), and the number of
//! explored configurations is capped by the caller's [`Effort`](crate::Effort)
//! budget. Exhaustion is returned as an explicit
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

/// `Work` per input byte for the lexical layer: the reasoning-marker
/// automaton, the opener first-byte tables, and the capture window.
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

/// Cost of decoding stringified arguments (`tool_call.arguments.stringified`):
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
