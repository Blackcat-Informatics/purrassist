<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Security

PurRAssist 0.1.0 is merged to `main` and not yet tagged `rust-v0.1.0`. Only
`0.0.0` name reservations exist on crates.io, so there is no supported published
release yet.

Report vulnerabilities privately to <security@blackcatinformatics.ca>, with
`SECURITY` and `PurRAssist` in the subject. Include affected crates, revisions,
reproduction inputs, and observed impact. Do not include confidential input in a
public issue. Maintainers will coordinate validation, remediation, and disclosure.

Relevant surfaces include untrusted parsers, regex/schema evaluation, recovery
budgets, misleading correction receipts, file transactions, MCP framing, and
dependency provenance. A validated guess remains a guess. File tools coordinate
cooperating writers; their portable path checks are not a hostile-process sandbox.

The workspace denies `unsafe_code` by default. Its two production exceptions
are `crates/syntax/src/alloc.rs` and `crates/host/src/files/windows.rs`;
the rest of those library crates retains the denial. Four qualification-only
exceptions are confined to `crates/wasm-harness/src/lib.rs`,
`crates/core/tests/allocation_oracle.rs`, `crates/core/benches/latency.rs`, and
`crates/syntax/benches/latency.rs`. Their source and justification are registered
in `docs/DEPENDENCIES.md`. `crates/host/src/files/windows.rs` calls `kernel32`
through `windows-sys` (`SetFileInformationByHandle`, `MoveFileExW`,
`FlushFileBuffers`) to implement Windows filesystem durability — these are
raw FFI calls with no safe std equivalent, each with a `SAFETY` comment.

`crates/syntax/src/alloc.rs` implements
`malloc`/`calloc`/`realloc`/`free` wrappers over `std::alloc::System` so
tree-sitter's own C allocations can be charged to the caller's governor
instead of remaining external to it. `tree_sitter::set_allocator` replaces
tree-sitter's C allocator for the whole process, not just this crate: an
embedder linking a *different* crate that also calls `set_allocator` (or
`ffi::ts_set_allocator`) in the same process is incompatible, since
tree-sitter's own safety contract on that call requires no live tree-sitter
object exist when either installation happens, and whichever call happens
last wins. Every call site inside tree-sitter's own C sources dereferences
the allocator's return value without a null check, matching the default
allocator's own "abort on failure" contract; this module's wrappers preserve
that (`std::process::abort` on genuine `std::alloc::System` exhaustion), so
budget enforcement never depends on returning null — it happens exclusively
through the governed progress-callback checks documented in
`docs/RESOURCES.md`. Per-parse accounting is attributed through a strong
reference stored in each allocation's own header, not a thread-local
counter, so a block freed on a different thread than the one that allocated
it (tree-sitter's `Tree` is `Send`) still decrements the correct account.

Reporter credit is offered using the reporter's chosen name or handle; requests
for anonymity are respected. This reporting contact follows the Blackcat
Informatics repository family policy.
