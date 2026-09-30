// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Native and WASI caller of the single-source core acceptance transcript.
fn main() {
    print!("{}", wasm_harness::transcript_text());
}
