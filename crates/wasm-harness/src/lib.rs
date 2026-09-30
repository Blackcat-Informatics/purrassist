// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
#![allow(unsafe_code)]
//! Unpublished `wasm32-unknown-unknown` execution harness for the portable
//! acceptance transcript (`crates-io` never sees this crate: `publish =
//! false`; it exists only so `scripts/check-wasm.py` can execute the same
//! transcript `purrassist-core`'s `portable-core` example produces, on a
//! target that has no WASI host to run a plain example binary against).
//!
//! # Scope of the `unsafe_code = "deny"` exception
//! The workspace denies `unsafe_code` by default; this crate is a
//! registered exception (`#![allow(unsafe_code)]` above), scoped to this one
//! crate. [`portable_main`] must be callable from JavaScript by a stable,
//! unmangled export name; the only standard way to guarantee that without a
//! code-generation tool (`wasm-bindgen` and similar are exactly the kind of
//! third-party surface this harness avoids pulling in) is the `#[no_mangle]`
//! attribute, which the 2024 edition requires spelling `#[unsafe(no_mangle)]`
//! and which `rustc` classifies under the same `unsafe_code` lint as an
//! `unsafe` block. No unsafe block appears anywhere in this crate: every
//! operation in [`portable_main`] (slice-to-pointer, pointer-to-integer,
//! leaking an owned allocation) is a safe Rust operation on its own; only the
//! attribute syntax itself is what the lint is reacting to. See
//! `crates/host/src/files/windows.rs` for the same kind of registered,
//! file-scoped exception used elsewhere in this workspace.
//!
//! # Design
//! The transcript body itself is not duplicated here. `crates/core/examples/
//! portable_transcript.rs` (included below via `#[path]`, the same mechanism
//! `crates/core/examples/portable.rs` uses) is the single source of the JSON
//! transcript; both callers depend on `purrassist_core` and therefore see the
//! identical `purrassist_core::` imports inside it. Sharing the file this way
//! adds no public API to `purrassist-core` itself: the shared function stays
//! `pub(crate)` to whichever crate happens to include it.
#[path = "../../core/examples/portable_transcript.rs"]
mod portable_transcript;

/// Build the portable acceptance transcript and hand its UTF-8 bytes to the
/// caller as a packed `(pointer, length)` pair, without any WASI import.
///
/// `scripts/run-wasm.mjs` instantiates the compiled module with Node's plain
/// `WebAssembly` API (no `wasi_snapshot_preview1` imports are required or
/// provided) and calls this export directly.
///
/// # Return value
/// The high 32 bits of the returned `u64` are the byte offset into the
/// module's exported linear memory (`memory`) where the transcript's UTF-8
/// bytes begin; the low 32 bits are the byte length. `wasm32-unknown-unknown`
/// pointers are always 32 bits, so this packing is lossless and needs no
/// multi-value return support. The two lines written are exactly
/// `{sha256-digest-of-transcript}\n{transcript-json}\n`, matching the two
/// `println!` calls in `crates/core/examples/portable.rs`'s `main`, so
/// `scripts/check-wasm.py` can compare this function's decoded output
/// byte-for-byte against that example's captured native stdout.
///
/// # Errors
/// There is no error return: transcript construction only fails by panicking
/// on an internal invariant violation (as `portable.rs` does natively), which
/// traps the wasm instance instead of returning — `scripts/run-wasm.mjs`
/// surfaces that trap as a thrown JavaScript exception.
///
/// # Memory
/// The returned bytes are leaked (never freed): this harness runs the
/// transcript exactly once per module instantiation and the instance is then
/// discarded, so reclaiming the allocation would add complexity with no
/// observable benefit.
#[unsafe(no_mangle)]
pub extern "C" fn portable_main() -> u64 {
    let transcript = portable_transcript::transcript_json();
    let digest = purrassist_core::digest::Digest::of(transcript.as_bytes());
    let text = format!("{digest}\n{transcript}\n");
    let bytes: &'static [u8] = Box::leak(text.into_bytes().into_boxed_slice());
    let ptr = bytes.as_ptr() as u64;
    let len = bytes.len() as u64;
    (ptr << 32) | len
}
