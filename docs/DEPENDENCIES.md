<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Dependency ownership

The computational core has no third-party Cargo dependencies. JSON parsing,
exact decimals, schema evaluation, regex matching, normalization algorithms,
edit distances, phonetics, recovery search, and receipts are first-party Rust.

The unpublished `purrassist-toolgraph` foundation uses `purrdf-core` for native
IRI validation and the shared loss registry, and `purrdf-shapes` for its shared
namespace table, FnO reader and public plain argument-schema compiler. Both use
exact Git revision `742bbbfbe2240e1fc6f178badd049420753ef358` of the public
PurRDF repository. This retains the plain profile from upstream issue #788 / PR
#806 and adopts numeric projection from upstream issue #885 / merged PR #888.
Default and all-feature metadata at adoption contain the same 99 package
identities as the previous pin: all 19 PurRDF Git packages change source
revision, with no registry package/version or core dependency drift. The
registered name/version baseline therefore stays unchanged. License notices
remain byte-identical; their existing `purrdf-2ed9845` storage paths are retained,
while each package's source registration identifies the adopted revision.
The numeric compiler shares upstream's exact XSD arithmetic and records the
original SHACL representability check for unrestricted IEEE domains; it adds no
dependency to the core.
The graph crate's resolved all-target normal/build closure contains 19 PurRDF
packages, 53 registry packages and the first-party core. This revision update
adds no package identity to the workspace inventory. Every resolved
package and its shortest dependency path is registered in
`vendor/dependencies.json` and `scripts/data/dependency-baseline.json`, with
byte-preserved license notices under `vendor/licenses/`.

The three PurRDF first-party grants are retained alongside Unicode-3.0 notices
for embedded Unicode data. The lexical crate additionally retains the original
libyaml/unsafe-libyaml MIT and WHATWG BSD notices for its incorporated emitter
rules and character-reference data. `r-efi`'s `AUTHORS` file contains its original
MIT permission and copyright notice and is retained verbatim. These inherited
notices are not replaced by the local first-party licenses.

The graph crate stays outside the publishable release set until upstream's
registry source contains the required APIs and packaging is qualified. The
six existing release packages do not yet link it. The loader delivery retains
facade/host integration and registry-resolvable packaging acceptance; no registry
release or publication is claimed by Git-pinned workspace compilation.

`purrassist-derive`, the procedural-macro crate behind
`#[derive(ToolDefinition)]` and `#[derive(ToolEnum)]`, also has no third-party
dependencies, and no `syn`, `quote` or `proc-macro2`: it uses only the
compiler's built-in `proc_macro` crate, with a first-party token parser and
code writer. Its only Cargo dependency is a dev-dependency on
`purrassist-core`, used by its tests and examples; `scripts/check-repository.py`
requires its normal and build closure to be empty and its dev closure to reach
only `purrassist-core`, and scans `crates/derive/src` for ambient access.
`purrassist-core` never depends on `purrassist-derive`.

Pythonic literal decoding uses Unicode 17.0.0 character names and aliases.
`scripts/generate-python-names.py` derives the static lookup from the pinned
UnicodeData and NameAliases sources in `vendor/unicode/python-names.json`;
the existing Unicode-3.0 notice is retained. This includes algorithmic Hangul
and ideograph names and adds no third-party runtime code.

Standards data remains third-party material. Unicode tables and conformance
fixtures retain the Unicode-3.0 license. UTS #39 confusables data (Unicode
17.0.0, published alongside the UCD release starting with that version) is
generated into a first-party skeleton lookup table by
`scripts/generate-unicode.py`, with its source URL and SHA-256 pinned in
`vendor/unicode/manifest.json` alongside the other Unicode Character Database
inputs; no confusables detection library is linked. Official JSON Schema meta-schemas
(Draft 2020-12 and Draft-07, the latter from the json-schema-spec `draft-07`
branch, byte-identical to the published `http://json-schema.org/draft-07/schema`)
use the upstream BSD license; the full upstream license is retained beside them.
The JSON Schema Test Suite retains its MIT license. The WHATWG named-character
table is incorporated as BSD-3-Clause source data; its original JSON fixture is
CC-BY-4.0. The full upstream notice is packaged beside the generated table.
An offline generator checks pinned source hashes and generates only semicolon-
terminated names; the gate checks the generated digest. No HTML parser library
is linked. Source URLs, versions, and
SHA-256 digests are recorded in the manifests listed in
`vendor/dependencies.json`. These assets are not relicensed as first-party code.

The same generator (`scripts/generate-unicode.py`) also produces the pattern
engine's Unicode property tables: `Script`, `Script_Extensions` (from
`Scripts.txt`, `ScriptExtensions.txt`, and the `sc` value aliases in
`PropertyValueAliases.txt`), and every ECMA-262 binary Unicode property (from
`PropList.txt`, `DerivedCoreProperties.txt`, `emoji/emoji-data.txt`,
`DerivedNormalizationProps.txt`, and the property aliases in
`PropertyAliases.txt`; `ASCII`, `Any`, `Assigned`, and `Bidi_Mirrored` are
computed rather than sourced from one file). It also produces the IDNA/
UTS-46 mapping table (`idna/IdnaMappingTable.txt`) the hostname/
idn-hostname format validators consume. All are Unicode-3.0, pinned by URL
and SHA-256 in `vendor/unicode/manifest.json` alongside the other Unicode
Character Database inputs; the generated Rust tables carry the same license
header. The pattern engine's `test262` RegExp `property-escapes/generated`
conformance fixtures (`crates/core/tests/data/test262/`) are vendored
verbatim at a pinned commit (`834912b0a6b1d973244a5e8a1baad25d6b9ef238`,
"Add tests for Unicode 17 property escapes") under their original
BSD-3-Clause license (Ecma International); a first-party Rust parser in
`crates/core/tests/pattern.rs` reads their `matchSymbols`/`nonMatchSymbols`
literals directly (no JavaScript engine is linked or invoked). None of this
data is relicensed as first-party code.

The accepted grammar-parser exception is Tree-sitter and its Python, Rust,
JavaScript, and TypeScript grammar family. The shipping grammar integration resolves 12 external runtime packages.
Repository tooling additionally includes the unpublished helper census; the exact
full Cargo graph is checked against `vendor/dependencies.json`. Tree-sitter also brings regex/query support,
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
`rust-lang/crates-io-auth-action` (MIT OR Apache-2.0), pinned by full commit SHA
`c6f97d42243bad5fab37ca0427f495c86d5b1a18` (tag `v1.0.5`), exchanges the
`crates-io` environment's OIDC trusted-publishing token for a short-lived
registry token inside `.github/workflows/publish.yml` only; it is never
invoked from `verify.yml` or `release-qualification.yml`, and it never sees a
long-lived credential.

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
loader debug symbols. System-runtime profiling inherits installed debuginfod
configuration. The isolated runtime explicitly caches the corresponding public
Arch loader debug file, pinned by build-id and SHA-256 in
`vendor/profiling-runtime.json` and registered under `profiling-glibc` in the
tooling inventory. Its license is the existing glibc license set. Its verified
file identity is recorded with the runtime; no ambient service is required.
Verify provisions the existing external `bsdtar` extractor through Ubuntu's
`libarchive-tools` package (libarchive 3.7.2 on Ubuntu 24.04). Its distribution
revision is logged by CI, and the actual extractor version is recorded per run.
This distro dependency is registered separately from the local bsdtar 3.8.9
entry. The complete upstream per-file copyright and license terms from Ubuntu
source revision `3.7.2-2ubuntu0.8` are preserved with their source archive digest;
apt selects the supported distro revision rather than a pinned executable.

`crates/syntax/src/alloc.rs` is a registered `unsafe_code` exception to the
workspace-wide `unsafe_code = "deny"` lint (`#![allow(unsafe_code)]` scoped to
that one file; the workspace deny stays in force everywhere else, including
the rest of `purrassist-syntax`). The rationale: `tree_sitter::set_allocator`
is a safe function's *only* way to route tree-sitter's own C allocations
through a governed accounting layer instead of leaving them external to the
core's resource governor (R3); its four `unsafe extern "C" fn` pointers and
the raw per-block header they share cannot be expressed without `unsafe`. The
module implements `malloc`/`calloc`/`realloc`/`free` wrappers over
`std::alloc::System`, installs itself exactly once via `std::sync::Once`
before this crate constructs any `Parser`/`Language`, and documents (in its
own module docs, and in `SECURITY.md`) the process-global effect on any other
tree-sitter allocator installation in the same process, the "abort rather
than return null" contract tree-sitter's C call sites require, and why
per-parse accounting is attributed through each allocation's own header
rather than a thread-local counter (frees can happen on any thread; see
`docs/RESOURCES.md`'s "Tree-sitter allocation bound" section). No allocator
wrapper, client request, or profiler library is linked for this; it is
first-party code exercising a public, documented tree-sitter API.

`purrassist-host` has one Windows-only Cargo dependency, declared under
`[target.'cfg(windows)'.dependencies]` with `default-features = false` and
pinned to an exact version: `windows-sys = "=0.61.2"`, features
`Win32_Foundation`, `Win32_Storage_FileSystem`, and
`Win32_System_WindowsProgramming` (the last only for the
`FILE_RENAME_FLAG_POSIX_SEMANTICS`/`FILE_RENAME_FLAG_REPLACE_IF_EXISTS`
constants). It brings one
transitive dependency, `windows-link@0.2.1` (an extern-fn linking shim; no
executable logic). Both are MIT OR Apache-2.0, from
`https://github.com/microsoft/windows-rs`, with license notices vendored
under `vendor/licenses/windows-sys-0.61.2/` and
`vendor/licenses/windows-link-0.2.1/`. `cargo metadata` resolves target-gated
dependencies on every host platform regardless of which platform invokes it
(Cargo does not filter `packages` by the running OS unless
`--filter-platform` is passed, and `scripts/check-repository.py` never passes
it), so `windows-sys`/`windows-link` appear in the dependency inventory and
are checked on Linux and macOS exactly like every other resolved package,
even though the code that uses them only compiles under `cfg(windows)`.
`windows-sys` provides raw `kernel32` FFI signatures and constants only
(`SetFileInformationByHandle`, `MoveFileExW`, `FlushFileBuffers`, and their
associated flag/error constants); it contains no logic of its own. It backs
R6 Windows filesystem durability in `crates/host/src/files/windows.rs`:
POSIX-semantics rename via `FileRenameInfoEx` on a write-through handle,
falling back to `MoveFileExW` where the filesystem rejects POSIX semantics,
and `FlushFileBuffers` for directory metadata flush. No provider SDK, HTTP
client, TLS stack, async runtime, or model runtime is linked by this
dependency; it is the narrowest possible surface for the `Win32` calls the
durability contract requires, since `std::fs` exposes no write-through or
POSIX-rename-semantics primitive.

`crates/host/src/files/windows.rs` is the workspace's second `unsafe_code`
exception (`#![allow(unsafe_code)]` scoped to that one file; the workspace
deny stays in force everywhere else, including the rest of
`purrassist-host`; the first exception is `crates/syntax/src/alloc.rs`,
documented above). The rationale: calling `kernel32` functions through
`windows-sys` — `SetFileInformationByHandle`, `MoveFileExW`,
`FlushFileBuffers`, `GetLastError` — is inherently `unsafe` (they are raw
`extern "C"` FFI calls whose safety the compiler cannot check), and building
the variable-length `FILE_RENAME_INFO` buffer for the rename call requires
raw-pointer writes into a manually allocated, correctly aligned buffer (a
`Vec<u8>` is not guaranteed aligned for a struct containing a `HANDLE`
field). Every `unsafe` block carries a `SAFETY` comment. No allocator
wrapper, client request, or profiler library is linked for this; it is
first-party code calling public, documented `Win32` APIs.

Coverage-guided and differential qualification (`scripts/fuzz.py --target
<json|markdown|schema|pattern|unified|fuzzy|matching|patch>`, see
`docs/FUZZING.md`) uses the `llvm-tools` rustup component's `llvm-cov` and
`llvm-profdata` (LLVM 23.1.1, resolved via `rustc --print sysroot` so they
are always built from the exact LLVM revision the active `rustc` itself
uses) with the Rust compiler's matching coverage runtime. Only the
qualification example for the target under test is instrumented. LLVM's
Apache-2.0 WITH LLVM-exception notice and exact tool version are recorded in
`vendor/tooling.json`. The first-party mutation runner uses Python's
standard library; it adds no Cargo dependency, vendored fuzzing engine, or
instrumentation to production crates. The `pattern` target's differential
probe invokes the already-pinned Node runtime (`new RegExp(p,
'u').test(s)`) as an independent oracle, restricted to the ECMA-262 subset
both engines implement. The `markdown` target's differential probe builds
`cmark-gfm` `0.29.0.gfm.13` (BSD-2-Clause AND MIT; matching the GFM example
corpus already vendored above) from a digest-verified source tarball into
`.cache/tools/`, registered in `vendor/tooling.json` with its license notice
under `vendor/licenses/`; it is never linked into any crate and is used only
as an independent HTML-rendering oracle, out of process.

The same verified archive also produces a separate executable with one
registered cache correction in `src/inlines.c`: preserve the furthest known
backtick closer instead of overwriting it with an earlier closer. The exact
unique patch preimage and `backtick_cache_patch_sha256` in
`vendor/tooling.json` are checked before building. The original executable
remains unchanged. Reports identify the original archive, correction,
corrected source file, and both executable digests, and retain actual
original/corrected disagreements. GFM §6.3 and executed regression controls
justify the correction; see `docs/FUZZING.md`. Its source retains the upstream
licenses above. Neither executable is linked into or shipped with a crate.


An additional isolated oracle executable combines that backtick correction
with distinct close-only and dual-purpose emphasis opener floors. The exact
unique preimages and `emphasis_floors_patch_sha256` registration are checked
before building. Raw `*` or `_` selects it before probe execution. GFM's
emphasis algorithm and actual upstream cmark 0.31.1 outputs justify the
change; each campaign requires 11 authoritative controls and unchanged
output on all 672 official examples. Raw, backtick-only, and combined answers,
patch/source/archive/executable identities, and unchanged specification
mismatches remain explicit. Upstream licenses continue to apply, and no
corrected source or executable is included in a production crate.


Repository scripting requires Python 3.12 or newer (including the standard
library archive extraction filter); cmark-gfm qualification requires CMake
3.16 or newer and the native C build toolchain. Python and CMake notices are
registered in `vendor/tooling.json`. The pinned Valgrind source and Linux Node
binary are installed by `scripts/install-qualification-tool.py` into an ignored
cache in CI, with archive hashes checked before extraction. Tool versions are
recorded in each report. These build and verification processes add no core
runtime dependencies.

The first-party correction evaluation corpora are reproducibly generated from
curated templates and the pinned confusables fixture. Their generator, case
counts, and output hashes are registered in the evaluation manifest and checked
by repository hygiene. `vendor/dependencies.json` also records the exact set and
hashes of notices that belong in each crate archive; package auditing compares
both checkout and archive against that reviewed registry.

The core and syntax latency benchmark binaries use scoped first-party counting
allocators under `#![allow(unsafe_code)]`, confined to their bench files. They
measure requested allocation rather than add a production allocator or dependency.
The existing allocation-oracle test has the same isolated exception. Their unsafe
blocks document the standard allocator delegation and counter invariants.

## Network-free, dependency-free core

`purrassist-core` links into environments that forbid network crates, ambient
I/O, and nondeterminism. `scripts/check-repository.py` (run by `make hygiene`)
enforces this with checks whose logic is unit-tested against synthetic metadata
and sources by `scripts/test-repository-audit.py`:

- **Empty core closure.** From `cargo metadata --locked --all-features`, the
  resolved closure of `purrassist-core` over normal, build, and dev edges for all
  targets must be empty. A failure names each package and its path from the core.
- **Network denylist.** Independently, the normal and build closure of every
  publishable workspace crate must not contain networking, TLS, HTTP, or async
  runtime packages (`hyper*`, `reqwest`, `h2`, `h3*`, `http`, `http-body*`,
  `tokio*`, `mio`, `async-std`, `async-io`, `smol`, `socket2`, `rustls*`,
  `native-tls`, `openssl*`, `ureq`, `curl*`, `isahc`, `surf`, `quinn*`, `tonic`,
  `axum`, `actix*`, `warp`). A failure names the crate, the matched package, and
  the dependency path.
- **Ambient-access scan.** Code tokens (comments and doc comments are skipped,
  string contents are not) in `crates/core/src/**/*.rs`, including generated data,
  must not use `std::net`, `TcpStream`, `TcpListener`, `UdpSocket`,
  `ToSocketAddrs`, `std::process`, `std::fs`, `std::env`, `SystemTime`, `Instant`,
  `RandomState`, `HashMap`, `HashSet`, `DefaultHasher`, `std::thread`, `stdin(`,
  `stdout(`, or `stderr(`; grouped imports such as `use std::{fs}` are also caught.
  There is no `#[cfg(test)]` exemption. Failures report `file:line`.
- **Inventory baseline.** Every third-party package must be registered in
  `vendor/dependencies.json` and must appear in the pinned
  `scripts/data/dependency-baseline.json` list of `name@version` entries. Any new
  package fails the audit. `purrassist-core` must also declare no
  `[dev-dependencies]` or `[build-dependencies]`, including target-specific ones.

No dependency may be added without explicit maintainer approval. Once approved,
register it in `vendor/dependencies.json` and update the baseline in the same
change.

## Structural helper census

`purrassist-helper-census` is unpublished repository tooling ported from
PurRDF's helper-centralization branch. It parses Rust with `syn`, traverses
module/item bodies, and groups normalized implementations and forwarding shims.
It uses the already registered `proc-macro2`, `syn` and `regex` versions, with
span-location, full syntax/visitor and regex performance features. Their resolved
transitive dependencies, including any feature-enabled additions, are covered by
the same exact inventory and retained license notices. First-party SHA-256 and
JSON serialization come from `purrassist-core`; fingerprints consequently differ
from the source tool's BLAKE3 ids while normalized equality stays the same.

No third-party dependency is added to the core, and no census code or dependency
is bundled into a published crate. Upstream commit and original file digests are
recorded in `crates/helper-census/UPSTREAM.json`; the source license headers and
three first-party license copies are retained. Its documented repository commands
distinguish an inspection report from strict helper-ledger enforcement.

The independently corrected qualification-only `cmark-gfm` executables also
retain bare-destination parenthesis balance in `manual_scan_link_url_2`.
`scripts/fuzz.py` requires an exact unique full-function preimage and the patch
digest registered in `vendor/tooling.json`; reports retain original/previous/
corrected source and executable identities, full output disagreements and
executed compatibility controls. Existing upstream source licenses/notices
apply. This reference tooling is never linked or packaged into production crates.

The final independent reference also resets each retried extended email's domain
period counter at the unique `found_at:` scanner label in
`extensions/autolink.c`. The registered patch and both corrected source-file
hashes are required. Raw at-signs or entity syntax select that oracle before
execution; original/earlier/final binaries and full answers remain distinct.
Actual controls and the full official corpus execute for each campaign. This
supersedes the earlier narrow email HTML projection for new reports, preserving
all historical observations unchanged and retaining the same upstream licenses.

The unpublished `wasm-harness` owns the shared core acceptance transcript.
Its native/WASI `portable-core` example and browser export use that Cargo
library directly. The harness depends on core; core has no dependency on the
harness. Adding its `rlib` output beside the browser `cdylib` adds no third-party
or published-package dependency. Artifact reports retain the tested engine's
logical `purrassist-core` label while recording the measured module identity.

The repository social-preview image `docs/social-preview.png` is rendered by hand
from `docs/social-preview.svg`; `docs/assets.json` registers both digests, the
exact render command, the `rsvg-convert` (librsvg, `LGPL-2.1-or-later`) version
and the Noto Sans (`OFL-1.1`) font version, upstream project and font-file
digests. Both tools are registered in `vendor/tooling.json` with vendored
license notices. They are justified only as manual asset-rendering tools: the
PNG is a committed binary, so neither tool is invoked at build or runtime,
by `make check`, or by any workflow, and neither is linked or packaged into any
crate. Rendering is byte-reproducible only under the recorded versions, so a
re-render must reproduce the registered PNG digest. The PNG's license is
recorded by the REUSE-style sidecar `docs/social-preview.png.license`.

The MCP Registry publisher `mcp-publisher` 1.8.1 (`Apache-2.0 AND MIT`) is
downloaded by `.github/workflows/mcp-registry.yml`, pinned by version and the
release archive's sha256, and is registered in `vendor/tooling.json` with its
vendored license notice. It is justified only for submitting `server.json` to
the MCP Registry from that manually triggered workflow, which authenticates
with the workflow's GitHub OIDC token. It is never invoked at build or runtime
or by `make check`, and it is never linked or packaged into any crate.
