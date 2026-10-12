<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-syntax

Isolated Tree-sitter grammar integration for Python, Rust, JavaScript/JSX,
TypeScript, and TSX. Inspection retains source ranges; editing validates the
whole proposed buffer and can repair uniquely indicated missing punctuation.

**Status: 0.1.0.** This crate is one of the shared engines of `PurRAssist`, one
engine with three equal interfaces: the Rust crates (`purrassist`), the MCP
server and the CLI (`purrassist-cli`) all run it.

```rust
use purrassist_core::{Budget, Governor};
use purrassist_syntax::{Language, inspect};
let report = inspect("fn answer() -> u32 { 42 }", Language::Rust,
    &mut Governor::new(Budget::default())).unwrap();
assert!(report.valid);
```

Grammar acceptance does not establish type correctness or execution behavior.
Parser progress and external allocations are accounted per operation;
Tree-sitter allocation overshoot between progress callbacks has explicit,
measured limits. Native/WASI transcripts exercise all five grammars, and
independent compiler probes check accepted and rejected buffers. See the
[qualification record](https://github.com/Blackcat-Informatics/purrassist/blob/rust-v0.1.0/docs/QUALIFICATION.md)
for measurement scope and limits.

First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0. Pinned grammar dependencies
retain their own licenses and are isolated from the dependency-free core.
