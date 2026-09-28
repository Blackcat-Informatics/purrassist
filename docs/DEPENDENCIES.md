<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Dependency ownership

The computational core has no third-party Cargo dependencies. JSON parsing,
exact decimals, schema evaluation, regex matching, normalization algorithms,
edit distances, phonetics, recovery search, and receipts are first-party Rust.

Standards data remains third-party material. Unicode tables and conformance
fixtures retain the Unicode-3.0 license. Official JSON Schema meta-schemas use
the upstream BSD license; the full upstream license is retained beside them.
The JSON Schema Test Suite retains its MIT license. The WHATWG named-character
table is incorporated as BSD-3-Clause source data; its original JSON fixture is
CC-BY-4.0. The full upstream notice is packaged beside the generated table.
An offline generator checks pinned source hashes and generates only semicolon-
terminated names; the gate checks the generated digest. No HTML parser library
is linked. Source URLs, versions, and
SHA-256 digests are recorded in the manifests listed in
`vendor/dependencies.json`. These assets are not relicensed as first-party code.

The accepted grammar-parser exception is Tree-sitter and its Python, Rust,
JavaScript, and TypeScript grammar family. The pinned integration currently resolves 10 external runtime packages and 27
packages in the full Cargo graph. Tree-sitter also brings regex/query support,
a small language ABI crate, and a streaming iterator; its C build brings `cc`,
platform compiler discovery, and a JSON-driven generation toolchain. Exact
versions, licenses, and upstream repositories are recorded in the inventory.
No provider SDK, HTTP client, TLS stack, async runtime, or model runtime is linked.
WASI syntax builds additionally require a WASI C sysroot; the pure core does not.
Its build-only archive is pinned by SHA-256 in `vendor/wasi.json`.

Independent syntax qualification uses the installed Python, Rust, and Node
compilers, plus a TypeScript 5.9.3 compiler archive pinned by SHA-512. This adds
no Cargo dependency and runs no package-installation scripts. It is used only as
an independent TS/TSX/JSX test oracle; its package has no runtime dependencies.
Tool and CI action identities/notices are recorded in `vendor/tooling.json`;
the GitHub checkout action is pinned to a full commit with credentials disabled.

`make hygiene` checks the resolved Cargo graph against the checked-in inventory
and verifies retained license notices, Unicode artifact/fixture hashes, and each
crate's first-party license copies. It refuses any dependency in the core. No optional feature may conceal a core
dependency. Development tools are separate from linked runtime dependencies:
Rust/Cargo/Clippy/rustfmt, Python 3 for repository verification, and Node.js for
executed WASI comparisons. None is invoked by the pure core.

Official CommonMark 0.31.2 and GFM 0.29.0.gfm.13 examples are qualification-only
fixtures under CC-BY-SA-4.0, separately attributed to John MacFarlane and the GFM
specification contributors. The manifest retains upstream and normalized digests,
source URLs, and extraction details; accompanying upstream license notices remain
beside the examples. These test strings are not linked into the runtime engine.

Allocation qualification uses the installed Valgrind 3.25.1 executable as an
external process; no allocator wrapper, client request, or profiler library is
linked into a crate. The Linux runner can use an isolated stock glibc loader and
libc when a host loader uses instructions unsupported by the profiler. That
opt-in runtime is pinned by archive digest in `vendor/profiling-runtime.json`,
extracted with system `bsdtar`, and kept only in the ignored cache. Tool and
runtime license notices are registered in `vendor/tooling.json`. This adds no
Cargo dependency or crate runtime dependency. Profiling requires appropriate
loader debug symbols; installed debuginfod configuration is inherited, without
embedding any endpoint in repository configuration or reports.
