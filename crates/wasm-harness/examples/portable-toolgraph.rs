// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Native and WASI caller of the single-source tool-graph load transcript.
fn main() {
    print!("{}", wasm_harness::toolgraph_transcript_text());
}
