<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Structural helper census

The unpublished `purrassist-helper-census` workspace tool finds repeated Rust
implementations and reports the source locations that need review. Structural
similarity is a candidate for consolidation, not a proof that two functions have
identical contracts. Existing findings remain visible: `helpers-ledger.toml`
contains no sanctioned variants or distinct-group exceptions.

## Commands and enforcement

```sh
make helper-test
make helper-census
make helper-check
cargo test --locked -p purrassist-helper-census
cargo clippy --locked -p purrassist-helper-census --all-targets -- -D warnings
cargo run --quiet --locked -p purrassist-helper-census -- --index
cargo run --quiet --locked -p purrassist-helper-census -- --dump-ledger
```

`helper-test` runs the Rust seeded detector cases, the Python positive/negative
validator fixtures, and independent ledger validation. `helper-census` writes a
real workspace inspection to `.cache/helper-census/clone-baseline.json`. This
report does not create an acceptance baseline or suppress any finding.
`helper-check` enforces the complete ledger and rejects unreviewed groups.
Ledger validation reports current body/shim groups and cross-package includes
without accepting or suppressing them. The existing include at
`crates/wasm-harness/src/lib.rs:34` compiles
`crates/core/examples/portable_transcript.rs` into the unpublished WASM
qualification harness. It deliberately shares the same transcript fixture for
native/WASM equivalence; it remains visible and strict `helper-check` rejects
it. Porting the detector does not impose a new global include ban in `make check`.

The Rust `--check` directly enforces body/shim groups and forbidden matches;
the Python gate additionally checks files, vectors, bench targets, documented
anchors, stale declarations and cross-package `#[path]` attributes.

`--census JOB` prints every match for a declared job. `--count JOB` prints
`copies=<n>` after home-package and sanctioned-variant classification. Unknown
jobs and unknown options fail. Every mode requires exactly one operation.
`--root DIR` chooses an explicit source root; `--ledger FILE` chooses the ledger.
The default root is derived from this crate's build-time manifest directory.

## Detection and traversal

The tool parses Rust with `syn` and uses source spans from `proc-macro2`. It walks
libraries, binaries and build scripts of packages under `crates/` and
`bindings/`, following inline and external modules, `#[path]`, imports,
re-exports, impl methods and trait bodies. Cargo's read-only `read-manifest`
command validates the full disk manifest grammar and discovers actual target
roots, including commented/single-quoted paths and automatic binaries. A root
Cargo manifest is independently validated with `cargo metadata --no-deps
--locked`. The in-memory fixture walker uses a strict first-party TOML subset.

Shipping-unit groups exclude `#[test]`, cfg predicates requiring `test`, and
package `tests/`, `benches/`, `examples/` and `generated/` directories. Other
conditional configurations are examined syntactically, without selecting only
the current host's cfg. Macro invocations are parsed tokens; the tool does not
expand macros or prove program equivalence. AST architecture rules examine all
first-party Rust files, including tests/examples, except where their documented
rule limits explicitly select shipping code. Cross-package path checks likewise
include all first-party Rust files and handle multiline attributes, raw/escaped
strings and nested comments without treating strings/comments as code.

The structural body form preserves keywords, delimiters, punctuation, referenced
item names and call targets. Local bindings, parameters, fields and lifetimes
are renamed by first appearance, and literals are replaced by their kinds.
Bodies of at least 30 normalized tokens are grouped (clone types 1/2). The shim
form preserves fixed arguments and referenced names, renaming only parameters;
forwarders need at least six tokens. Targets are compared as written, so
`Self::new(value)` in two different types can form a review group.

Integer literals are compared by numeric value across radix, separators and
suffixes. Cross-package constants greater than `0xffff` are reported separately;
matching constants alone do not establish shared functionality. Recognized
lowercase/uppercase hex-digit tables are indexed. Generic opt-in ledger rules
cover default std hashers, hex formatting/decoding, whitespace classification,
JSON-pointer escaping, home/vocabulary literals, framing, slice integer loads,
alignment masks, hand-written ceiling division and XOR first-mismatch loops.
No opt-in architecture policy is imposed without a declared ledger job.

Fingerprints are `<kind>:<16 lowercase hex digits>`, the first eight bytes of
PurRAssist core SHA-256 over the normalized form. They differ from the source
tool's BLAKE3 ids; upstream ids must not be pasted into this ledger. Reports use
core JSON serialization with sorted object keys and compact formatting.
Filesystem/read/entry errors, invalid UTF-8 names or source, malformed manifests,
missing declared roots and Rust parse failures fail the CLI; they cannot produce
a plausibly clean partial census. Absent optional `bindings/` and automatically
discovered binary directories are legitimate. Symlink entries are explicitly
rejected rather than silently skipped or recursively followed.

## Ledger schema

The ledger accepts basic/literal strings, booleans, decimal integers, arrays,
and table/array-of-table headers. Unsupported TOML forms and unknown keys fail
with diagnostics. The Python validator compares the Rust reader's complete
result with standard-library `tomllib` on every validation run.

Each `[[job]]` requires `id`, `summary`, `home`, `entry_points`, `spec`, `vectors`,
`bench`, `sites`, `replaces_external`, `enforced` and `[job.forbidden]`.
`home` and every entry point resolve through the symbol index. `vectors` lists
existing repository-relative files; `bench` lists real `package:bench-target`
identities. Empty arrays explicitly mean none. `sites` and `replaces_external`
must be empty here: PurRDF's SIMD measurement and banned-dependency registries
are not PurRAssist registries, so nonempty claims fail.

`[job.forbidden]` requires `constants`, `fingerprints` and `names`. Constants
are Rust integer literal spellings; fingerprints are current `body:`, `shim:`,
`table:` or implemented `rule:` ids; names are validated regex patterns.
An enforced job rejects forbidden matches outside its home package. An
unenforced job with no remaining copies is stale and fails validation.

Optional `[[job.variant]]` rows require `symbol`, `file`, `detector`, `criterion`,
`anchor` and `reason`. Detectors are `isomorphic` or `forbidden`; the symbol must
resolve in its declared file, the anchor must resolve to a documented item,
and the variant must still match its declared detector. Missing vectors,
unknown benches, stale fingerprints and variants fail.

`[[distinct]]` requires `members`, `kind` (`body` or `shim`), `criterion`, `anchor`
and `reason`. Its member multiset must equal exactly one current group; adding
another member makes that group unreviewed again. Missing members, nonexistent
groups and undocumented/unresolved anchors fail. Criteria are `a` (specified
conformance difference), `b` (documented tangible benefit), `c` (measured
significant speedup that cannot be centralized), or `layering` (callers cannot
reach the shared home). A review must substantiate the reason; the tool checks
the declaration and source evidence, not whether a prose argument is persuasive.

## Value of the first destination inspection

The initial destination inspection found **1,429 units, six body groups, eight
shim groups, one hex table and two shared-constant groups**. A full current
report is regenerated by `make helper-census`; source edits can change counts.
The completed port inspection reports **1,438 units** with the same six body
groups, eight shim groups, one hex table and two constant groups; nine additional
units are detector implementation helpers. The full report and exact check
results are archived in
[the local qualification report](qualification/helper-census-linux-x86_64.json).
Its source hashes are explicitly observations after the run, not a reconstructed
atomic build/start identity. The archived checks record 38 unit tests, 18 Rust
CLI fixture cases and 86 Python fixture cases, all passing; strict checking
returns failure for 14 unreviewed groups and the existing transcript include.
Useful consolidation candidates include:

| Candidate | Locations | Why it merits review |
| --- | --- | --- |
| Evidence JSON serialization | `core/src/wire.rs::evidence`, `host/src/tools/witness.rs::evidence` | Same receipt fields and serialization body in two layers. |
| Percent-escape recognition | `core/src/schema/format/template.rs::pct_encoded`, `core/src/schema/format/uri.rs::pct_encoded` | Same bounds/hex-pair validation. |
| Line-ending recognition | `core/src/markdown/lists.rs::ending`, `core/src/markdown/table_rows.rs::ending` | Same LF/CRLF/CR utility. |
| Logical line reconstruction | `core/src/markdown/render.rs::text`, `core/src/markdown/sections/nested.rs::logical_text` | Same source ranges and virtual-tab padding reconstruction; preserve byte/indent contracts. |
| Saturating size conversion | 23 `size` helpers across core/host | Same `usize` to `u64` conversion and overflow handling. |

All paths in this table are under `crates/`. Other groups require semantic
classification: reference/table `record` functions have different surrounding
validation and literal policies; `ScratchVec::new` and `ScratchMap::new` build
different collections; error `From` implementations construct different error
types; `Number::from_unsigned` and `from_count` expose distinct typed inputs.
Constants such as `0x10000` occur in unrelated Unicode, resource and storage
contracts. None of these have been automatically merged or sanctioned.

## Provenance and dependency boundary

The immutable copied source is PurRDF commit
`0387e83e01842edb4bb5f4141ed788a0b71638dd`, on its helper-centralization branch.
The branch advanced from the initially requested
`e58250b3e65ae8bfeb85147ccc07975d6aba183b` before copying. Every original file's
SHA-256, including the generic Python validator, is recorded and verified
against the immutable actual copy commit in
`crates/helper-census/UPSTREAM.json`. Original SPDX attribution and three
license texts are retained.

All generic body/shim/constant/table detection, source traversal, ledger modes,
strict validation and positive/negative self-tests are retained. PurRDF-specific
IRI-singleton enforcement, delegated raw/shared hash-domain policies, SIMD-site
registry and banned-dependency imports are excluded because their architecture
and registries do not exist here. Their delegated detector ids are rejected,
not silently accepted. Generic home/vocabulary literal algorithms and their
adversarial fixtures remain available without importing PurRDF job declarations.

The tool reuses pinned `syn` 3.0.6, `proc-macro2` 1.0.107 and `regex` 1.13.1.
Regex performance features add registered `aho-corasick` 1.1.5. Every tool-only
and transitive dependency and its notices are in `vendor/dependencies.json`.
`purrassist-core` supplies SHA-256 and JSON and retains zero third-party runtime
dependencies. This unpublished crate and its dependencies are not packaged in
published runtime crates.
