// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Deterministic native/WASI acceptance transcript using the actual core engines.
//!
//! The transcript body lives in `portable_transcript.rs`, included below by
//! `#[path]` so `crates/wasm-harness` (wasm32-unknown-unknown) can produce the
//! identical JSON without duplicating it or adding public API to
//! `purrassist-core`.
use purrassist_core::digest::Digest;

#[path = "portable_transcript.rs"]
mod portable_transcript;

fn main() {
    let output = portable_transcript::transcript_json();
    println!("{}", Digest::of(output.as_bytes()));
    println!("{output}");
}
