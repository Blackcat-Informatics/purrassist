// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Unpublished native core acceptance transcript.
mod transcript;

pub use transcript::transcript_json;

/// Build the exact digest-and-JSON lines for the native acceptance transcript.
///
/// ```
/// let text = acceptance_harness::transcript_text();
/// let (digest, json) = text.trim_end_matches('\n').split_once('\n').unwrap();
/// assert_eq!(digest, purrassist_core::digest::Digest::of(json.as_bytes()).to_string());
/// ```
///
/// # Panics
/// Panics if transcript construction violates a qualification invariant.
#[must_use]
pub fn transcript_text() -> String {
    let transcript = transcript_json();
    let digest = purrassist_core::digest::Digest::of(transcript.as_bytes());
    format!("{digest}\n{transcript}\n")
}
