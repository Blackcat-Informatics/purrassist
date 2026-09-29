<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Qualification record

This is an implementation checkpoint, not release approval. The complete accepted
scope in `IMPLEMENTATION.md` remains required. No registry publication is claimed.

## Executable checks

| Check | Command | Coverage |
| --- | --- | --- |
| Local gate | `make check` | Formatting, provenance, Clippy, optimized tests, Rustdoc |
| Schema | `cargo test -p purrassist-core --test schema_conformance --test formats --test vocabularies` | 1,301 main Draft 2020-12 tests; 121 selected optional tests; 878 vendored `optional/format/*` + `format-assertion.json` cases run under a dialect declaring format-assertion (every 2020-12 format, including RFC 5892/5893 IDNA2008 contextual/Bidi rules); explicit unknown-format-under-assertion and unsupported-vocabulary behavior |
| Pattern engine | `cargo test -p purrassist-core --test pattern` | 441-fixture test262 property-escapes census (Unicode 17); pathological `(a+)+b` linear-time check; 10,000-pair derivative/backtracking differential; `generate` witness/emptiness properties |
| Witness algebra | `cargo test -p purrassist-core --test witness_algebra --test recursive_schemas --test recovery_goldens --test corruption_oracle` | 430/430 official-suite schema groups witnessed (0 exhaustions, 0 false unsatisfiability claims); 4 known-unsatisfiable schemas proven; 7 recursive-schema audit cases; 66 curated composed-recovery goldens; corruption oracle over 446 witnessed schemas × 6 deterministic corruptions (791 informative trials, 0 wrong answers) |
| Unicode | `cargo test -p purrassist-core --test unicode_conformance` | 20,034 Unicode 17 normalization cases |
| Portability | `make wasm-test` | Executed native/WASI recovery, all five grammar, and fixed-provider-transcript host transcripts (`purrassist-core`, `purrassist-syntax`, `purrassist-host` each on wasm32-wasip1); `purrassist-core`'s transcript additionally executed on wasm32-unknown-unknown (no WASI, via the unpublished `wasm-harness` crate and Node's plain `WebAssembly` API) and compared byte-for-byte against its native output |
| Independent syntax | `make syntax-oracles` | 15 fixed positive/negative probes against Python, rustc, Node, and TypeScript 5.9.3 |
| Baseline | `make benchmark` | Six deterministic recovery cases, 200 samples each, quality assertions and latency |
| Resources | `make resources` | Nine assertion-backed processes, Memcheck allocations, Massif heap observations, native RSS, and native executable/build measurements; Linux prerequisites in `RESOURCES.md` |
| Coverage-guided JSON | `make fuzz-json` | Source-region feedback, retained mutation corpus, round-trip/schema/receipt properties, explicit failures and replay; prerequisites and limits in `FUZZING.md` |
| MSRV | `rustup run 1.98.0 cargo test --workspace --locked` | Workspace tests on Rust 1.98 |
| Packages | `make package` | Cargo package construction and verification in dependency order |

Selected optional schema suites cover large numbers, overflow, ECMA regex cases,
non-BMP characters, references into unknown keywords, anchors, dynamic references,
identifiers, missing dialect declarations, and unknown keywords.

Every Draft 2020-12 format (`date-time`, `date`, `time`, `duration`, `email`,
`idn-email`, `hostname`, `idn-hostname`, `ipv4`, `ipv6`, `uri`, `uri-reference`,
`iri`, `iri-reference`, `uuid`, `uri-template`, `json-pointer`,
`relative-json-pointer`, `regex`) is a first-party validator, always evaluated as
annotation evidence; assertion is driven solely by the dialect's `$vocabulary`
declaring the format-assertion vocabulary, never a caller switch. `idn-hostname`
implements IDNA2008 (RFC 5890-5893): UTS-46 nontransitional mapping, Punycode
A-label decode/encode with the RFC 5891 section 5.4 round-trip check, the RFC 5892
Appendix A CONTEXTJ/CONTEXTO rules, and the RFC 5893 Bidi rule, using generated
Bidi_Class/Joining_Type tables alongside the existing Script/UTS-46 tables. An
unrecognized format under an active format-assertion vocabulary is a compile-time
error; recovery repairs `date`/`date-time`/`time` violations as labeled `Guess`
inferences and leaves every other failing format an explicit non-invention. An
unsupported optional (`$vocabulary` ... `false`) vocabulary compiles and every
recovery receipt (and `Schema::unsupported_vocabularies`) names the URI and the
keywords left unevaluated; a required unknown vocabulary fails to compile. This
does not imply unrestricted ECMAScript regex support beyond Task 6's engine.

Regression tests cover composed healing, duplicate ambiguity, retained sources,
inferences, exhaustion, Unicode/path matching, fuzzy replacement minima, unified
diff relocation, CRLF preservation, no-final-newline markers, exact undo, stale
writes, interrupted journal/temp writes, independent batch outcomes, provider
transcripts, and the actual CLI/MCP subprocess. Mutation probes exercise 2,000
reproducible malformed JSON inputs; those unit probes have no coverage feedback.
The separate `make fuzz-json` campaign uses LLVM source-region coverage to retain
new inputs and select subsequent mutation parents. It checks four fixed schemas
with bounded recovery and retains failures for replay. This does not qualify
arbitrary schemas, every effort policy, or the document/matching engines.

A clean-source campaign at `ea50c8965a005744ca680c69d89a61e93bea324f`
passed 1,014 executions (14 seeds and 1,000 mutations), with 39 invalid-UTF-8
inputs explicitly rejected at the probe boundary. The seeds covered 4,051 core
code regions; mutation feedback added 248 regions and retained 49 discoveries,
for 4,299 regions and 63 corpus entries. No probe assertion or timeout failed.
The tool/build/source identities, deterministic seed, parent corpus indexes,
and discovery counts are recorded in
`qualification/fuzz-json-linux-x86_64.json`. These are covered-region counts,
not a whole-crate coverage percentage. A saved discovery also passed replay;
a separate forced-timeout check produced a failed report with its input retained.

Ambiguous JSON recovery results carry separate ordered alternative ledgers with
output digests, search costs, transformations, losses, inferences, and indices into
the actual validation log. Regressions check candidate association, wire encoding,
guess reporting, and exhaustion before a complete ledger can be returned.

Local package verification has built all five packages. A local release CLI was
installed into a disposable checkout cache and exercised on the published example
inputs, including exact-source retention. This does not verify downloadable registry
packages. Hosted CI passed for checkpoint `922b36fa966cb00f0d8fdaafcf66a5944632a1dd`:
[run 36422654690](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36422654690)
completed the Linux/macOS local gates, MSRV workspace tests, Windows core/syntax
tests, and native/WASI comparisons. This is evidence for that checkpoint only;
subsequent changes need their own qualification.

At `4103c50a041cc81e8a01af47af97f55819018815`, `make package` constructed
and Cargo-verified all five archives. The package auditor found the expected
first-party licenses and upstream notices, zero core dependencies, and source
files identical to the checkout. A fresh local release-profile install of
`purrassist-cli` passed the example JSON decode, source-bound `apply_edit`, and
stdio MCP initialize/decode smoke. Hosted [CI run 36432747721](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36432747721)
passed for that same commit. These are checkout artifacts, not downloaded
registry packages. The release-qualification workflow is package-only pending
the publication and registry gates described in `RELEASE.md`.
The hosted [release-qualification run 36433883963](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36433883963)
also passed on `de8db9d7b0d7a15259bb783529372f29cfdc9f78`, including the
stable full gate and Cargo verification of all five archives.
The subsequent [Verify run 36437227125](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36437227125)
and [release-qualification run 36437268473](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36437268473)
passed on `a68beebbc2bdfde6ae38ed51a269e81b64a4c8f5`. The latter used a
commit-scoped Cargo home to avoid resolving an older cached same-version crate.

The initial six-case baseline observed approximately 2 microseconds median for
exact integer recovery and 42 microseconds for composed syntax/key/wrapper/type
repair. These are single-machine observations, not portable performance guarantees.
Reported scratch usage is governor accounting, not measured allocator consumption.
Run the benchmark on the target platform; do not generalize its zero incorrect
corrections to arbitrary AI output.

## Markdown qualification in progress

The pinned corpora contain 652 CommonMark 0.31.2 examples and 672 GFM examples.
`cargo test -p purrassist-core --test markdown_corpus` exercises complete-source
and nested-range preservation for every example, first-block classification for
228 leaf-block examples, and quote hierarchy for 50 blockquote examples. These
checks are narrower than full rendering conformance; passing them does not prove
inline semantics, list tightness, tab-column handling, or complete GFM behavior.
The container/context/task checkpoint passed the full `make check` gate on Rust
1.98.0, including optimized workspace tests and Rustdoc. Hosted CI subsequently
passed for this checkpoint, including nightly analysis, as recorded above.

The shared Markdown adapters passed the full local gate on Rust 1.98.0.
Tests exercise CRLF/Unicode context, nested block encoding, section-body and
checkbox plans, no-op edits, exact undo, stale-plan rejection, invalid selections,
resource exhaustion, and identical CLI/MCP context results. These transport and
editing checks do not extend the parser conformance claim.

Governed schema compilation passed the full local gate on Rust 1.98.0, including
both official schema suites and regressions distinguishing invalid schemas from
work/storage/input/depth exhaustion and cancellation. Tests cover cumulative
external-resource/dialect preparation, unused-resource hashing admission, shared
compilation/recovery limits, and tool-level exhaustion reporting.

Semantic alternatives now carry independent, output-bound rule and inference
ledgers with validation references. The full Rust 1.98.0 local gate passed,
including contradictory rules, rejected and duplicate derivations, final derived
attributes, host wire/digest association, and atomic failure on ledger exhaustion.
Semantic alternative costs are zero because the surviving interpretations are
not ranked against each other.

The inline layer is checked against 389 complete examples: emphasis and strong
emphasis, code spans, autolinks, soft breaks, and raw HTML from both pinned corpora.
It follows the [CommonMark inline rules](https://spec.commonmark.org/0.31.2/#inlines).
Tests compare exact HTML, including link/emphasis precedence, Unicode delimiter
flanking, code whitespace, URI escaping, and malformed tags. Explicit dialect
selection preserves the pinned GFM corpus's suppression of directly nested strong
tags while CommonMark retains them. Inline links/images have additional focused
coverage for nested labels, escaped destinations, titles, and link precedence.

All 1,324 examples also pass recursive inline byte-span partition checks.
Additional regressions check CRLF/Unicode source spans, selected nested edits and
exact undo, backslash hard breaks, NUL replacement in autolinks, cancellation,
storage/depth exhaustion, and bounded lookahead. This increment passed the full
local Rust 1.98.0 gate, including optimized workspace tests and Rustdoc.
The shared-rendering checkpoint passed hosted CI, including nightly analysis, as
recorded above. Those focused inline tests are supplemented by the full-document
comparison described below.

Named/numeric entity decoding passes all 2,125 semicolon-terminated WHATWG names,
24 official inline-only entity examples, and numeric digit-limit/scalar tests.
The tests use the retained upstream JSON independently of the generated lookup.
Additional cases cover escaped ampersands, single-pass decoding, syntax
non-reinterpretation, titles/destinations, and preservation in code/HTML/autolinks.
Document-level entity cases are now covered by the full-document comparison. Data, generated output, and upstream notices are pinned
and checked; generation uses no network. The full local Rust 1.98.0 gate passed
for this increment, including optimized workspace tests and Rustdoc.

Reference-aware parsing passes all 221 link/image examples from both corpora and
45 paragraph-level definition examples, comparing exact HTML. The harness supplies
paragraph blocks explicitly; the remaining definition cases require code blocks,
headings, or blockquotes and do not yet qualify full-document behavior. Regressions
cover duplicate-first precedence, Unicode case folding and whitespace, 999-character
labels, escape/entity distinction, exact source retention and winning-definition
provenance, cancellation, and work exhaustion. Definitions retain every occurrence,
including shadowed duplicates, and links/images retain the selected source digest
and definition range. The full local Rust 1.98.0 gate passed, including optimized
workspace tests and Rustdoc. Container-aware collection is now exercised below.

Document rendering collects references across containers and retains original-source
line maps, including partial-tab padding. All 652 CommonMark and 672 pinned GFM
examples pass exact full-document HTML comparison. The GFM harness uses the
upstream per-example extension annotations retained in a hash-pinned sidecar;
expected HTML was not changed. Both upstream-disabled task-list examples are
still executed. Normal GFM rendering enables all five extensions together.
Regressions cover combined extensions, protected code and existing links, source
ranges, inline tag filtering, definitions in quotes/lists versus code, Unicode/CRLF,
partial tabs, cancellation, and exhaustion. Passing these finite corpora is not
proof for arbitrary input; broader generated/adversarial testing remains required.
The full local Rust 1.98.0 gate passed for this document-rendering increment,
including optimized workspace tests, Rustdoc examples, and documentation builds.

The shared `render_markdown` operation exposes the same renderer through Rust,
CLI, and MCP. Its result includes renderer/specification identities, a canonical
effective extension profile, HTML and output digest, plus original-source identity.
The receipt retains exact input and records the rendering decision. Tests cover
default/overridden profiles, Unicode/CRLF retention, invalid/duplicate extensions,
exhaustion without partial output, source line maps, and CLI/MCP wire equivalence.
The full local Rust 1.98.0 gate passed for the shared operation; the documented
rendering example also executed successfully with the verification-profile CLI.

Nested task editing uses source-bound block paths emitted by inspection, supports
quotes and nested lists, and changes only the original checkbox-state byte.
Regression coverage includes uppercase checked markers, Unicode/CRLF, sibling and
container-prefix preservation, exact inverse replay, stale-source rejection,
no-op states, invalid selectors, cancellation, and shared tool path selection.
The full local Rust 1.98.0 gate passed, including CLI/MCP equivalence for nested
paths, optimized workspace tests, Rustdoc examples, and documentation builds.

Caller-owned embedding matching now has a reusable normalized core index and the
shared `match_embeddings` tool. Tests cover extreme scales, ties and margin
selection, weak matches, duplicate identities, invalid dimensions/coordinates,
zero vectors, candidate exhaustion, cancellation, retained decimal inputs, and
independent alternative ledgers. Numerical winners remain semantic guesses.
No embedding model or transport is used. The tool explicitly converts decimal
coordinates to binary64 and rejects overflow/nonzero underflow; cosine scores
are not probabilities. Measured performance and broader vector/target coverage
remain to be qualified.
The full local Rust 1.98.0 `make check` gate passed, including the embedding
regressions and CLI/MCP wire equivalence. The documented embedding example was
executed through the optimized CLI and returned `tea` as a best guess.

GFM table cell planning now selects nested tables through block paths and edits
explicit header/body cells while preserving their source margins, delimiters,
container prefixes, and line endings. It reparses proposals and compares the
complete selected table against the expected single-cell change. Focused tests
cover CRLF, Unicode, escaped pipes, empty cells, header edits, nested quotes/lists,
exact undo, stale sources, invalid selection, cancellation, and work exhaustion.
Direct cell edits still reject implicit padded cells and cells beyond header width.
The full local Rust 1.98.0 `make check` gate passed, including the official
Markdown corpora and CLI/MCP equivalence for this operation. The optimized CLI
executed `examples/markdown-table-cell.json` and returned the expected single-cell
proposal.

Table body-row insertion/removal now uses nested paths and explicit body indices.
Insertion requires the header width, inherits container prefixes and line endings,
and preserves EOF's final-newline convention. Proposals are reparsed and compared
with the expected table. Both cell and row tools emit structural validation
receipts. Focused tests cover nested lists/quotes, LF/CRLF/CR, unterminated EOF,
empty tables/cells, escaped pipes, header preservation, exact undo/stale rejection,
invalid row/cell selection, cancellation, and storage exhaustion.
The final row source passed the complete local Rust 1.98.0 `make check` gate and
workspace/all-target Clippy on nightly-2026-09-14. This includes the new row tests,
CLI/MCP equivalence, official Markdown corpora, and Rustdoc. The rebuilt CLI
executed `examples/markdown-table-row.json` and returned the expected inserted row
and structural-validation receipt.

Nested table-column plans now insert/remove columns and change alignment across
the shared Rust/CLI/MCP surface. Insert/remove preserves existing cell spelling,
materializes implicit padding, retains ignored extra cells beyond the new header
width, and adds outer pipes when needed. Sparse-table padding growth is budgeted
before allocation. Tests cover insertion boundaries, nested containers, CRLF,
escaped pipes, Unicode, ragged rows, exact undo, stale-source rejection, invalid
action fields, final-column removal rejection, no-op alignment, and exhaustion.
Alignment-only edits do not reserve rectangular padding growth. Column operations
and their complete edit/undo receipts also matched in the executed native/WASI
transcript; nightly Clippy and the browser-WASM core build passed.
The final column source passed the complete local Rust 1.98.0 `make check` gate,
including the sparse-alignment budget regression, CLI/MCP equivalence, official
Markdown corpora, and Rustdoc. The optimized CLI executed
`examples/markdown-table-column.json` and returned the expected three source-bound
edits and structural-validation receipt.

The executed native/WASI core transcript now includes embedding selection at
extreme finite scales, exact binary64 score bits, ordered ties, weak matches,
zero-vector rejection, and candidate exhaustion. It also includes nested Markdown
cell edits, body-row and column insertion/removal, alignment changes, task edits,
GFM rendering, LF/CRLF/CR,
Unicode/escaped pipes, unterminated EOF, exact inverse replay, stale-source
rejection, and complete edit/undo/failure receipts. The transcripts matched
byte-for-byte on native Linux and WASI. The syntax transcript also matched, and
the browser-WASM core build passed. These are executed fixtures, not a proof for
all floating-point inputs or browser execution. The expanded example passed the
full Rust 1.98.0 local gate and nightly-2026-09-14 all-target Clippy.

Reference occurrence inspection now exposes original document field spans and
nested selectors, retaining duplicate labels. Destination/title plans use decoded
values, preserve label spelling and surrounding bytes, encode syntax safely, and
reparse every definition to check unselected values. Tests cover nested quote/list
prefixes, tabs, Unicode, multiline labels/titles, CRLF, entities, empty destinations
and titles, title removal, shadowed duplicates, no-op edits, exact undo, stale
sources, NUL rejection, cancellation, and work exhaustion. Definition metadata is
reserved separately from text. The expanded native/WASI reference transcript,
complete edit/undo receipts, browser-WASM build, and nightly Clippy passed.
The final reference source passed the full local Rust 1.98.0 `make check` gate,
including official Markdown corpora, reference regressions, CLI/MCP equivalence,
and Rustdoc. The optimized CLI executed `examples/markdown-reference.json` and
returned the expected field replacement and structural-validation receipt.

Reference and inline-link parsing now charge scanned labels, whitespace,
destinations, and titles, plus decoding/indexing and token grouping, instead of
repeatedly reserving unrelated remaining input. Blank-line detection scans without
allocating normalized copies and preserves LF, CRLF, and CR behavior. Regression
tests cover 256/512/1,024 definitions and repeated links under standard effort,
with less than 2.5x work growth when input doubles. At 1,024 unique definitions,
parsing consumes 228,414 work units; document parsing plus occurrence inspection
consumes 657,534, within the standard 1,000,000-unit limit. These are logical
budget measurements, not elapsed-time or allocator bounds. Long malformed
destinations, titles, and whitespace exhaust work inside their scans with typed
errors. The full Rust 1.98.0 `make check` gate and nightly Clippy passed.
Native/WASI transcripts, including the repeated-definition budget record, match;
the browser-WASM core build also passed.

Section-body replacement now shares a caller governor through proposal parsing
and rejects changed section boundaries or outside block structure. The standard
Rust convenience method uses the same checked implementation. CLI/MCP plans
record `markdown.section-structure.v1` validation. Regression cases cover ATX
and setext headings, changed lower-ranked subsections, exact undo/stale checks,
CR/LF/CRLF, empty/no-op/final sections, missing separators, peer-heading injection,
and unclosed fences/HTML that capture the following section. These root-section regressions establish the original checked API. The full Rust 1.98.0 `make check` gate, nightly Clippy,
expanded native/WASI edit/undo transcripts, and browser-WASM build passed.

Nested section inspection and replacement now use digest-bound paths into quote
and list containers. Replacement bodies omit outer prefixes; inserted lines
receive canonical quote markers and list-continuation spaces. Reparsing checks
body structure and leaf text, the selected heading, section boundary, ancestors,
and outside siblings. Tests cover mixed nesting, ordered markers, tabbed container
prefixes, setext headings, CR/LF/CRLF, no-op spelling preservation, empty/final
sections, exact undo, rejected container escape, and tool selector validation.
Bodies with trailing blanks that escape list items or column-sensitive tabs that
change body structure are explicitly rejected; broader layout recovery remains
open. Further structural layout recovery remains release work.
The full Rust 1.98.0 local gate, nightly Clippy, native/WASI nested edit and undo
transcripts, and browser-WASM build passed. The optimized CLI executed
`examples/markdown-section.json` with the expected prefixed replacement and
structural-validation receipt.

List-item edits now support before/after insertion with explicit markers, body
replacement retaining marker spelling, and removal at root or nested paths.
Reparsing checks the isolated intended item, outside siblings, and ancestor
identity. Existing items retain their numbering; generated lines use canonical
spacing and container prefixes. Tests cover CR/LF/CRLF, indented lists, mixed
quote/list nesting, ordered-marker spelling, task bodies, nested headings, EOF
insertion, only-item removal retaining a container, exact undo/stale rejection,
invalid markers, extra sibling blocks, exhaustion, and shared tool schemas.
The full Rust 1.98.0 gate, nightly Clippy, native/WASI list edit/undo transcript,
and browser-WASM build passed. The optimized CLI executed
`examples/markdown-list-item.json` and returned the expected nested task insertion
and `markdown.list-item-structure.v1` validation.

Resource qualification now has a dependency-free Rust workload executable and
an external Linux measurement runner. All nine cases passed native assertions,
Memcheck, and Massif using the pinned isolated glibc runtime. The runner checks
profiler summaries and fails on errors; native RSS comes from the workload’s own
post-exec VmHWM, avoiding inherited Python-launcher memory. Profiling exposed an
optimized bounds comparison against the unused payload of an empty Unicode
composition starter `Option`; explicitly initializing the separately guarded
index removed the diagnostic. All 20,034 Unicode normalization cases, the full
Rust 1.98.0 gate, nightly Clippy, native/WASI comparisons, and browser-WASM build
passed. The six-case latency baseline still passed 200/200 assertions per case.
Measurement definitions, runtime identity, cache caveats, and limitations are in
`RESOURCES.md`, with a saved clean-commit baseline under
`docs/qualification/resources-linux-x86_64.json`; these observations do not complete the allocation audit or prove
hard memory limits.

## Remaining release work

Source-bound plan application is available through `apply_edit` and the `plan`
form of `apply_file`, closing the buffer-reconstruction gap between planning and
application tools. Tests cover nested Markdown plan/inverse round trips, CRLF and
UTF-8 preservation, code validation, stale/overlapping/out-of-range plans,
explicit exhaustion, competing input rejection, conditional commit, and journal
undo. Planning and application receipts remain separate; selecting a concrete
plan does not erase a previous guess or ambiguity. See `EDIT-APPLICATION.md`.

Caller-owned cancellation reaches the governor, governed matching/edit/syntax
entry points, `Tools::call_with_cancellation`, and active stdio MCP calls. The
source, transaction, admission, and shutdown boundaries are documented in
`CANCELLATION.md`. Deterministic controller/worker races, real CLI output failure,
and five live cancellation/ping/EOF sessions passed. This establishes the tested
protocol behavior, not a maximum response latency or global allocator bound.

- Extend Markdown section, list, table, and reference structural editing and broaden
  adversarial/preservation qualification.
  Full HTML-output comparison against the pinned official corpora now passes.
  `crates/core/src/patch.rs` adds `Patch` (an ordered, non-overlapping,
  source-digest-bound set of byte-range replacements) as the crate's single
  edit representation, with a governed `apply`/`invert`/`compose`/`commute`/
  `rebase`/`diff` algebra; `edits::EditPlan`/`TextEdit` are aliases for it,
  not a parallel implementation. `Patch::rebase` reconciles a stale plan
  against an intervening change when their edited ranges commute (disjoint),
  recording a `patch.rebase.v1` transformation, and refuses as stale
  otherwise; `apply_edit`/`apply_file` expose this as an explicit
  `rebase_from` input (the prior text the plan was made against), diffing
  prior-to-current into the intervening patch via `Patch::diff`.
  `crates/core/tests/patch_laws.rs` checks invert-apply identity, compose
  associativity, and commute consistency (`p;q' ≡ q;p'`) over a deterministic
  in-test LCG generating well over 1,000 patches, plus rebase success and
  non-commuting refusal; `crates/host/tests/edit_tools.rs` covers
  `apply_edit`/`apply_file` rebasing a commuting stale plan and refusing a
  non-commuting one. New governed, digest-bound, reparse-checked, inverse-
  providing Markdown operations: `insert_section`, `remove_section`,
  `move_section` (with optional re-leveling of the moved section's own top
  heading, reported as a transformation), `rename_heading`;
  `ListItemEdit::Move` and `renumber_list`; `TableRowEdit::Move`,
  `TableColumnEdit::Move`, and direct cell edits on a body row's implicit
  padded cells, which now materialize the missing cells instead of refusing;
  `add_reference`, `relabel_reference` (rewrites every shortcut/collapsed/
  full-reference use resolving to the renamed definition, wherever
  CommonMark/GFM itself resolves one — paragraph/heading text and GFM table
  cells, at any container nesting depth — while leaving a code span,
  autolink, raw HTML, or code block spelling the label untouched, since
  those are literal there), `remove_reference`
  (inlines the destination/title into every use, lossless), and
  `unlink_reference` (removes the definition and turns every use into plain
  text, each rewritten byte range recorded as an ordinary `Loss`). Layout
  recovery: a body whose trailing line is blank, or whose leading indentation
  uses a tab, previously refused; both are now recognized and reindented (the
  trailing blank line to the container's own level, matching this parser's
  existing blank-line-before-dedent ejection rule; the tab to the number of
  spaces it would occupy from column zero, matching how the body parses
  standalone) instead of refused. `crates/core/tests/markdown_structure.rs`
  checks every new operation for success, byte-identical inverse round-trip,
  CRLF preservation, stale-digest refusal, and exhaustion.
  The CLI/MCP operation registry exposes every one of these new operations
  through `plan_markdown_structure` (Task 10; `crates/host/src/tools/registry.rs`,
  `crates/host/src/tools/markdown/structure.rs`), with a CLI≡MCP equivalence
  fixture in `crates/cli/tests/protocol.rs`. Adversarial/preservation
  qualification (Task 11) remains open; this bullet is otherwise complete.
- Operation registry and complete Rust/CLI/MCP surface (Task 10):
  `crates/host/src/tools/registry.rs` is the single declarative registry
  (name, description, input/output JSON Schema, write annotation, handler)
  backing `Tools::definitions()`, the CLI `call`/`mcp` commands, and MCP
  `tools/list`/`tools/call`; there is no second, hand-maintained dispatch
  path. New tools: `plan_markdown_structure` (above); `recover_files`/
  `commit_files` over `Store::recover`/`Store::commit_batch`; `validate_json`
  (schema + instance → `valid`/`violations`/always-evaluated
  `format_evidence`/`pattern_evidence`/`unsupported_vocabularies`/receipt),
  which the CLI `purrassist validate` command now emits verbatim instead of a
  hand-built object; `prepare_helper`/`complete_helper` giving
  `purrassist_host::helpers::Pending` a production tool surface (see
  `docs/HELPERS.md`) with a tamper-evident, versioned `pending` envelope.
  `effort` is now accepted by `plan_replacement`, `plan_diff`, and
  `inspect_code`. `inspect_code` is implemented via
  `purrassist_syntax::parse_governed`/`inspect_with_allocation`, reporting
  each issue's JSX/TSX rename repair options and the parse's
  `AllocationReport`; `crates/host/tests/inspect_tool.rs` drives it and
  `apply_edit` through the tool boundary to measured `ScratchBytes` budget
  exhaustion, cancellation, and a two-alternative JSX rename ambiguity.
  Forest annotations reach the wire: `crates/core/src/provenance.rs`'s
  `Forest::rank`/`multiplicity`/`valid` gain production callers inside
  `Forest::publish` (an `AlternativeReceipt` now carries its own `rank` and
  `valid`), and `crates/core/src/wire.rs` orders every receipt's
  `alternatives` array by that rank and adds a top-level `ambiguity` object
  (`derivations`, `valid`, and, per `Forest::distinguishing`, a
  `distinguishing` node list for each ranked-adjacent pair) whenever two or
  more alternatives publish, still under receipt wire version 2 (unreleased);
  `docs/schema/receipt.schema.json` and the version-2 golden fixture are
  updated to match, and `crates/core/tests/receipt_wire.rs` asserts the
  annotation directly. `crates/cli/tests/protocol.rs` iterates every registry
  entry with a required fixture per tool (a missing fixture fails the test),
  asserts MCP `tools/list` names equal the registry, validates every tool's
  output against its own registered output schema, and includes fixtures for
  the `ambiguity` object (a `resolve_string` tie), `validate_json`, and both
  helper tools.
- Regex coverage is complete: `crates/core/src/pattern/` implements the full
  ECMA-262 `u`-mode grammar (named groups with the ECMA duplicate-name
  mutual-exclusive-alternative early-error rule, `\k<name>` resolved at
  match time to whichever same-named group actually participated — at most
  one can, since duplicates are mutually exclusive — matching empty when
  none did, `\u{...}`, surrogate-pair escape combination, `\p{...}`/`\P{...}`
  for `General_Category`, `Script`, `Script_Extensions`, and every
  ECMA-262 binary Unicode property; lone
  braces, a class range ending in a class escape, and an identity escape of
  anything but a syntax character or `/` are `SyntaxError`s) over two engines
  selected per pattern and recorded as evidence
  (`pattern.engine.derivative-v1`/`pattern.engine.backtrack-v1`): Antimirov/
  Brzozowski derivatives over Unicode interval-set predicates with minterm
  partitioning and a governed, lazily-built state cache (linear-time; used
  for every pattern without a backreference or lookaround) for
  `Pattern::is_match` and `Pattern::generate`, falling back to the
  Work-charged-per-step backtracking evaluator only for backreferences and
  lookaround (neither is a regular language). `cargo test -p
  purrassist-core --test pattern` runs the complete vendored test262
  `built-ins/RegExp/property-escapes/generated` set (commit
  `834912b0a6b1d973244a5e8a1baad25d6b9ef238`, Unicode-17-aligned; 441
  fixtures, BSD-3-Clause, `crates/core/tests/data/test262/`) with a
  bidirectional census (every supported property has a fixture; every
  fixture names a supported property), asserts `(a+)+b` on 10,000 `a`s
  completes under the Standard effort budget and returns `false`
  (catastrophic backtracking is a backtracking-engine failure mode this
  primary engine does not have), asserts the derivative and backtracking
  engines agree on 10,000 deterministic generated (pattern, input) pairs
  without backreferences/lookarounds, and asserts `Pattern::generate`'s
  witnesses always match and never match an `avoid` pattern, proving
  emptiness (rather than assuming it) for both an intersection with a
  pattern's own complement and `[^\s\S]`. The single Unicode 17 generator
  (`scripts/generate-unicode.py`) now also produces the `Script`,
  `Script_Extensions`, and ECMA-262 binary-property tables plus the IDNA/
  UTS-46 mapping table for the format work below, all pinned by URL and
  SHA-256 in `vendor/unicode/manifest.json`.
- `format` is a first-party validator in `crates/core/src/schema/format/` for
  every Draft 2020-12 format (`date-time`, `date`, `time`, `duration`, `email`,
  `idn-email`, `hostname`, `idn-hostname`, `ipv4`, `ipv6`, `uri`,
  `uri-reference`, `iri`, `iri-reference`, `uuid`, `uri-template`,
  `json-pointer`, `relative-json-pointer`, `regex`, the last via Task 6's
  parser), always evaluated and reported as `schema.format.<name>-v1`
  annotation evidence (`ValidationReport::format_evidence`) regardless of
  assertion; assertion (a `Violation`) applies solely when the dialect's
  `$vocabulary` declares the format-assertion vocabulary URI (either boolean
  value counts), never a caller switch. `idn-hostname` implements IDNA2008
  (RFC 5890-5893) over the existing UTS-46 mapping table plus a new
  first-party Punycode codec (`crates/core/src/text/punycode.rs`, RFC 3492,
  including the RFC 5891 section 5.4 A-label round-trip check): nontransitional
  mapping and NFC normalization, the RFC 5892 Appendix A CONTEXTJ/CONTEXTO
  rules, the RFC 5892 section 2.6 derived-property exceptions, and the RFC 5893
  Bidi rule, using two new generated tables (`Bidi_Class`, `Joining_Type` from
  `DerivedBidiClass.txt`/`DerivedJoiningType.txt`, extending the single Task 6
  generator) alongside the existing `Script`/UTS-46 tables. An unrecognized
  format under an active format-assertion vocabulary is a compile-time
  `SchemaError`; recovery repairs `date`/`date-time`/`time` violations as
  labeled `Guess` inferences (`json.schema.format-repair`) and never invents a
  value for any other failing format. An unsupported optional
  (`$vocabulary` ... `false`) vocabulary compiles; `Schema::unsupported_vocabularies`
  and every recovery receipt (`schema.vocabulary.unsupported-v1` evidence) name
  the URI and the declaring resource's own keywords left unevaluated, while
  every other vocabulary keeps validating fully. A required unknown vocabulary
  still fails to compile. `cargo test -p purrassist-core --test
  schema_conformance --test formats --test vocabularies` runs the complete
  vendored `optional/format/` corpus (enumerated from its manifest; 21 files,
  874 cases) plus `optional/format-assertion.json` (4 cases) under a dialect
  declaring format-assertion — 878 cases, zero skipped, on top of the existing
  1,301/121-case censuses — and exercises the compile-time/annotation/repair/
  vocabulary behaviors above directly.
- `crates/core/src/schema/algebra/` implements the Attouche, Baazizi, Colazzo,
  Ghelli, Sartiani witness-generation pipeline (VLDB 2022) as five traceable
  stages: canonicalize (`canon::build`) and `not`-elimination (`canon::build_negated`,
  De Morgan duality down to a closed-form dual for every keyword that has one —
  `type`, `const`/`enum`, numeric bounds, string length, `pattern`, `format`,
  array length, `uniqueItems` — falling back to `Canon::OpaqueNot`, "some
  candidate that fails this exact sub-schema", checked with the real evaluator
  rather than decided symbolically, for the structural keywords that do not);
  `and`-merge and preparation (`merge::Requirements`, detecting an empty merged
  `TypeSet`, a crossed numeric/length/count bound, or an incompatible literal
  before generation ever runs); and generation (`generate::Gen`, type-directed
  construction using Task 6's `Pattern::generate` for `pattern`/`patternProperties`
  and Task 7's format validators for format witnesses, recursing over `$ref`/
  `$dynamicRef` under the same non-progressing-cycle guard and dynamic-scope
  stack as the evaluator). Every returned witness is re-validated with the real
  evaluator before this module returns it, so an unsound simplification anywhere
  upstream can only ever cost a downgrade to exhaustion, never an invalid
  witness. The outcome is exactly one of three shapes
  (`schema::algebra::WitnessOutcome`, `Schema::generate_witness`/
  `_governed`): `Witness`; `Unsatisfiable` (a closed-form refutation, never
  inferred from search exhaustion); or `Exhausted` (this stage's bounded,
  deliberately incomplete search found neither). Unsatisfiability is
  deliberately never expressed as a `StopReason`: that type's own contract is
  that exhaustion never asserts unsatisfiability, and `WitnessOutcome` is a new,
  separate type rather than a new `StopReason` variant, so nothing changes in
  `crate::wire`'s existing `StopReason` encoding. `crates/core/src/schema/recovery/proposals.rs`
  now invents every guessed value through this algebra (`WitnessOutcome::Witness`
  only; `Unsatisfiable`/`Exhausted` propose nothing) instead of the old
  `witnesses`/`typed_witnesses` heuristic, which is fully removed — one path,
  not two. Proposal traversal now also follows `patternProperties`,
  `additionalProperties`/`unevaluatedProperties` schemas, `dependentSchemas`
  (at the object's own route, since it constrains the whole object), `contains`,
  `unevaluatedItems`, `if`/`then`/`else` (recursing into whichever branch
  currently applies), and dynamic `$dynamicRef` (resolved against a live
  `scope` stack threaded through `propose`, the same dynamic-anchor-override
  rule as the evaluator and the algebra, not a purely static resolution as
  before). `not` and a failing `propertyNames` key have no local repair — a
  `not` violation is left an explicit no-candidate for that rule rather than
  fabricating an unrelated whole-candidate replacement, and a `propertyNames`
  violation is repaired narrowly (renaming just that key to a witness of the
  `propertyNames` schema's own `pattern`) rather than inventing a new object
  from scratch; `key_repair_proposals`'s rename candidates now also include
  `required` names with no `properties` entry, so a typo'd required-only key
  is still recognized. `crates/core/tests/recursive_schemas.rs` (7 cases) checks
  self-recursion, mutual recursion, the `$dynamicRef`/`$dynamicAnchor`
  extensible-schema pattern resolving against the live scope, a non-progressing
  cycle (`{"$ref":"#"}`, an explicit `InvalidInput` from the evaluator and
  `Exhausted` from the algebra, never a hang or a false pass), compile-time
  `Depth` exhaustion on deeply nested (not merely wide) schemas, `Depth`
  exhaustion on a 300-hop `$ref` chain during evaluation, and that deep-but-finite
  nesting is not mistaken for a cycle. `crates/core/tests/witness_algebra.rs`
  runs every schema group in the vendored official Draft 2020-12 suite with at
  least one valid test instance (430 groups) through the algebra: all 430
  witnessed and re-validated, zero exhausted, zero falsely reported
  unsatisfiable. That was a closed acceptance gap, not the starting point: 5
  groups (two `oneOf.json` cases, `dynamicRef.json`, `items.json`,
  `not.json`) initially exhausted, each defeated by a distinct construct with
  no closed-form treatment. `oneOf`'s exactly-one semantics needed
  `Canon::OneOf`/`Gen::attempt_one_of`: try branch `i`'s own canon while
  adding every *other* branch as a `Canon::OpaqueNot` exclusion, checked
  against the real evaluator, so the search can never falsely prove `oneOf`
  unsatisfiable merely because branch exclusion was inconclusive.
  `dynamicRef.json` ("after leaving a dynamic scope, it is not used by a
  `$dynamicRef`") exposed a scope-stack bug in `if`/`then`/`else`: folding
  `cond` and `then` into one flat `Canon` list under `Canon::Scoped`'s
  push/pop deferred the pop until the *entire* remaining search returned, so
  `then` was searched with `cond`'s dynamic scope still active — able to
  resolve a `$dynamicRef` against an anchor that should already have been
  left. `Gen::try_simple_merge`/`Gen::if_branch` now merge `cond`'s facets to
  completion — pushing and popping its scope within one Rust call stack,
  hence unconditionally on every return path — strictly *before* `then`/
  `else` is even built, mirroring `evaluate.rs`'s two separate, sequential
  scope entries; the previous flat-list combination remains as a sound
  fallback whenever `cond`/`then` need real backtracking search (`Or`/`Ref`/
  nested `if`) that a merge-only pass cannot resolve. `items.json` ("items
  does not look in applicators, valid case") exposed that `items` and
  `prefixItems` declared on *different* schema objects (`items` at the root,
  `prefixItems` inside an `allOf` branch) were merged as if they shared one
  prefix length, so the root's own `items` was wrongly skipped over the
  indices the `allOf` branch's `prefixItems` covered: `Facet::Items` now
  carries its own node's `prefixItems` length (0 if it declares none)
  alongside its target, so an `items` occurrence with no `prefixItems` of
  its own applies from index 0 regardless of what a sibling branch's
  `prefixItems` covers. `not.json` ("collect annotations inside a `not`,
  even if collection is disabled") exposed that `unevaluatedProperties:
  false` has no closed-form `not`-dual (falling back to `Canon::OpaqueNot`,
  whose dual is "some property is not evaluated") and the minimal,
  required-only object `build_object` builds can never witness that on its
  own; `Gen::retry_object_with_extra_member` retries once with one synthetic
  property whenever the minimal candidate fails a pending `OpaqueNot`
  obligation — bounded and sound, since the retry is re-`finalize`d, so a
  genuinely hopeless case still exhausts rather than fabricating a wrong
  witness. `false`, `allOf:[{type:string},{type:number}]`,
  `{"type":"integer","minimum":2,"maximum":1}`, and a `pattern` contradicted by
  its own `not` are each proven unsatisfiable by dedicated cases. Finding this
  gap required fixing a real pre-existing bug in Task 6's `Pattern::generate`
  (`crates/core/src/pattern/generate.rs`): its breadth-first search deduplicated
  reachable automaton states *globally* across every string length, which is
  the correct shortest-witness search when `min_len` is `0` but silently
  misreported "the language is empty" for any pattern containing a `+`/`*`/
  `{n,}` repetition once `min_len` forced the search past the point where
  states start repeating (a self-looping accepting state, once visited at an
  earlier length below `min_len`, was never revisited at a later length where
  its nullability would actually be checked). The fix dedupes only *within*
  one length step and instead recognizes a cycle in the sequence of live
  *state sets* once `min_len` is reached to keep proving genuine emptiness;
  `crates/core/tests/pattern.rs`'s existing emptiness-proof cases still pass
  unchanged. `crates/core/tests/data/recovery-goldens/*.json` (66 curated
  cases, exceeding the required 60, spanning every applicator: `type`/`const`/
  `enum`/numeric bounds/string length+`pattern`/array `minItems`+`prefixItems`+
  `contains`+`minContains`/`maxContains`/`uniqueItems`/object `minProperties`+
  `required`+`properties`+`patternProperties`+`additionalProperties`+
  `propertyNames`+`dependentRequired`+`dependentSchemas`/`allOf`/`anyOf`/
  `oneOf`/`not`/`if`-`then`-`else`/`$ref`/`$dynamicRef`/`unevaluatedProperties`+
  `unevaluatedItems`/all Task 7 formats/duplicate keys/truncated+prose-wrapped+
  stringified input) pin `description`+`schema`+`input` (hand-curated) against
  a captured `outcome`/`output`(s)/`ledger_rules` (captured from an actual
  governed run rather than hand-transcribed, via `crates/core/tests/recovery_goldens.rs`;
  `PURRASSIST_BLESS=1` re-captures them and panics if `CI` is set, so blessing
  can never run unattended). 61 goldens capture a successful repair or
  invention; 5 pin an intentional, explicit `unresolved` (an unmet
  `dependentRequired` trigger, a `uniqueItems` violation, exceeding
  `maxProperties`, a required property whose schema is `false`, and
  top-level `minProperties` padding — recovery invents a value per missing
  `required` property, never extra properties merely to satisfy a count, which
  this last golden documents as an explicit, deliberate boundary rather than a
  gap). `crates/core/tests/corruption_oracle.rs` takes an algebra witness for
  every official-suite schema group with one (446 schemas, up from 441 once
  the witness-algebra acceptance gap above was closed), applies each of
  six deterministic corruptions (truncation of a container's closing
  punctuation; prose/fence wrapping; an adjacent-character key transposition;
  full-value stringification; an information-preserving type swap; a
  duplicate key), and asserts recovery returns the exact original, an
  `Ambiguous` set containing it, or an explicit `Unresolved` — never a
  *different* answer. Of 1,134 corruption attempts, 343 turned out to already
  be a different but directly schema-valid instance on their own (many
  official-suite schema groups impose no `type` restriction at all, so
  swapping/stringifying a witness can produce an equally valid, merely
  different, instance; recovering the specific original in that case is not
  well-posed, so these are counted separately rather than either way) leaving
  791 informative trials: 773 recovered the exact original, 12 `Ambiguous`
  sets contained it, 6 were an explicit `Unresolved`, zero wrong answers. That
  oracle run surfaced one genuine, now-fixed cost-model gap:
  `json.key.resolve-duplicate` (an outright duplicate key, unambiguous JSON
  malformation with one obvious fix) was costed *above* the cheapest possible
  speculative key rename, so a short property name from an unrelated `allOf`/
  `oneOf` branch could look cheaper than simply dropping the duplicate;
  resolving a duplicate key is now costed at `2` (`crates/core/tests/recovery.rs`
  updated to match). Run all of this with `cargo test -p purrassist-core
  --test witness_algebra --test recursive_schemas --test recovery_goldens
  --test corruption_oracle --test recovery --test schema_conformance --test
  formats --test vocabularies --test pattern`.
- Complete allocation/work accounting throughout candidate generation, receipts,
  schema compilation, and helper preparation. Schema compilation now shares a
  governor across root/external/dialect parsing, indexing, pattern preparation,
  bundle hashing, and subsequent recovery. It exposes typed resource errors and
  avoids subtree cloning for reference-only targets.
  The governor's scratch counter is now a shared, atomic `ScratchMeter`
  (`crates/core/src/contract.rs`), which also tracks a monotone
  `total_scratch_charged` sum (never reduced by a release, exposed on
  `Usage`) alongside the existing high-water `peak_scratch_bytes`.
  `ScratchVec`/`ScratchString`/`ScratchMap`/`ScratchLease` charge before
  growing or reserving and release exactly what they charged on drop;
  exhaustion from any of them is sticky on the governor that issued them.
  Recovery's per-candidate cloning (`schema/recovery.rs` `change`,
  `schema/recovery/proposals.rs` witness/member-slice cloning and dedup),
  evaluator violation/active-path strings (`schema/evaluate.rs`), regex
  pattern compilation (`Pattern::compile_governed`, with the ungoverned
  `Pattern::compile` now delegating to it), repeated `Pattern::is_match`
  searches (including the bounded backtracking search's own per-iteration
  capture-state clones), the matching/edit-distance lease/release sites in
  `matching.rs` and `edits/fuzzy.rs` (now also covering the lexical-Jaccard
  token sets `matching.rs` computes per candidate, previously uncharged),
  JSON syntax repair's per-token replacement re-encoding
  (`json/repair.rs`, previously under-charged per input byte rather than per
  repaired token), Markdown's rendered-HTML output buffer
  (`markdown/render.rs`, via `ScratchString::reserve`/`into_string`), and host
  helper request preparation (`crates/host/src/helpers.rs`, which now takes
  the caller's own governor instead of building an unshared one) are charged
  before allocating rather than after or never.
  `crates/core/tests/allocation_oracle.rs` installs a test-only counting
  `#[global_allocator]` and asserts, for every governed public operation
  covering schema compile/validate/recover (including composed/ambiguous
  input), JSON parse/syntax recovery, pattern compile/match, Markdown render
  and each existing plan operation, matching rank/resolve, paths, exact/fuzzy/
  unified edits, embeddings, and intent resolution, at two deterministically
  generated input scales (a base size and a size at least 16x larger,
  generated by repeating sections/rows/candidates/schema properties/etc.),
  that real allocator bytes never exceed
  `governor.usage().total_scratch_charged + input_len + K` for a documented
  per-operation `K` that stays the *same constant* across both scales
  (`docs/RESOURCES.md`). Using the monotone `total_scratch_charged` instead of
  the high-water `peak_scratch_bytes` makes this a genuine scale-invariance
  property rather than a numerically convenient one: an operation that
  charges and releases scratch in a loop (fuzzy edit-distance probing, regex
  backtracking, ranking) still performs one real allocator call per
  iteration, and `total_scratch_charged` sums with it instead of staying flat
  the way `peak_scratch_bytes` would. Running the two-scale oracle found and
  fixed three real, previously-uncharged, input-proportional allocations —
  `matching.rs`'s per-candidate token-set construction, `pattern.rs`'s
  per-iteration backtracking-search state clones, and `json/repair.rs`'s
  per-repaired-token replacement re-encoding — each confirmed fixed by
  measuring across four to five scales, not only the two the committed test
  asserts. Markdown's rendered-HTML output buffer is now charged through
  `ScratchString` before its initial capacity is allocated, rather than
  growing an ordinary `String` behind an unrelated flat guess; the remaining
  Markdown internals already precharge their dominant allocation (the
  reparsed proposal, the retained source clone) before performing it, which
  the two-scale oracle now verifies directly for render and every existing
  plan operation instead of relying on an unverified flat guess. Tree-sitter/
  external-parser allocation and provenance-forest construction are handled
  by the syntax allocator and provenance forest tasks in this same change
  set, not by this task's governor/oracle work; progress cancellation alone
  is not a hard memory limit.
  `crates/core/tests/resource_accounting.rs` and
  `crates/host/tests/helpers.rs` additionally check that repeated pattern
  matches return live scratch to their pre-call level, that a scratch budget
  one byte below a formerly-uncharged clone yields `BudgetExceeded(ScratchBytes)`
  with no partial output, and that exhausted helper preparation yields
  `BudgetExceeded` and builds no request.
- Extend alternative-specific ledgers beyond JSON recovery, semantic resolution,
  embeddings, and path matching, and finish uniform
  Rust/CLI/MCP access to remaining document operations. Domain-rule
  intent resolution and path resolution now use the shared tool dispatcher,
  including epistemic status and one governor across domain/index admission
  and resolution. Markdown inspection, context extraction, section-body plans,
  nested task-checkbox plans, and buffer/conditional-file plan application also
  share the tool dispatcher. Ambiguous path matches now carry one output-digest-bound
  evidence and guess ledger per path; an exhausted ledger returns no partial set.
- Replace ad hoc per-site ambiguous-ledger construction with a single
  deduplicated derivation forest (`crates/core/src/provenance.rs`): evidence,
  losses, inferences, and validation references intern to a content-addressed
  node id once, charging `EvidenceEntries`/`ScratchBytes` once per distinct
  node rather than once per alternative. `Forest::publish` partitions every
  reachable node into what every derivation shares (promoted to the
  top-level, selected-result ledgers) and what one derivation alone depends
  on (that derivation's `AlternativeReceipt`); `Forest::distinguishing`
  answers "why is this ambiguous" as the symmetric difference between two
  derivations, and `Forest::rank`/`multiplicity`/`valid` are the tropical,
  counting, and Boolean semiring readings of the same forest. JSON recovery,
  semantic resolution, path matching, string selection, and the embedding
  tool now build their alternatives through the forest; conflicting semantic
  interpretations that share an identically-matched rule now show that rule
  once at the top level instead of once per alternative
  (`conflicting_rules_have_separate_output_bound_ledgers`). Four sites that
  previously published no per-alternative ledger at all, or leaked
  per-candidate evidence into the top-level ledger regardless of ambiguity,
  now populate one forest-published alternative per candidate: exact-match
  location (`edits::locate_governed`), fuzzy substring location
  (`edits::locate_fuzzy_governed`), unified-diff hunk placement
  (`edits::plan_unified_governed`), and Tree-sitter punctuation repair
  (`purrassist_syntax::apply_checked_governed`). `EditPlan`/`AppliedEdit` JSON
  encoding moved from a host-only helper into `crate::wire`
  (`EditPlan::to_node`/`AppliedEdit::to_node`), so every alternative's
  `output` digest is computed from the same encoder the host and CLI/MCP
  surfaces serialize. The receipt wire version is now 2: `usage` gained
  `total_scratch_charged`; a golden fixture
  (`crates/core/tests/data/receipts/v2-json-recover-ambiguous-duplicate-keys.json`)
  and `docs/schema/receipt.schema.json` (validated by this crate's own Draft
  2020-12 engine, not an external validator) pin the exact wire shape.
  `crates/host/src/helpers.rs`'s two hand-merged `Usage` sites now call a
  single `Usage::merge` (sums the monotone counters, takes the larger of the
  two `peak_scratch_bytes` high-water marks) instead of duplicating that
  arithmetic. Exhaustion mid-build never publishes a partial alternative
  list at any of these sites; a forest with N alternatives sharing E
  evidence nodes charges E, not N times E. A natural, tree-sitter-generated
  ambiguous *input* for the punctuation-repair site was not found: a single
  missing-token repair must reparse to a *globally* valid tree, so two
  genuinely independent missing-token defects can never both be
  independently sufficient (confirmed by direct experimentation, including
  against tree-sitter's own duplicate-position recovery for compound
  unclosed blocks, which still requires both closers); that site's
  ambiguous-publication path is instead exercised by a direct unit test with
  two realistic, independently valid repairs.
- Expand independent oracle and adversarial corpora, coverage-guided fuzzing beyond
  the bounded JSON campaign,
  memory/allocation measurements, build/artifact metrics, and representative
  incorrect-correction evaluation beyond the small baseline.
  The shared string-ranking tool accepts identified candidates and caller aliases
  while retaining legacy string arrays. It exposes effort, English phonetic
  selection, result limits, and per-signal scores. Candidate identity/alias
  admission and retained ranking storage are charged before cloning. Focused
  regressions cover alias evidence, exact-effort exclusion, malformed and duplicate
  identities, and explicit preparation exhaustion. Normalization equivalence is
  an explicit refinement lattice (identity, NFC, case-fold, width/compatibility-
  fold, diacritic-strip, UTS-39 confusable skeleton, depunctuation, phonetics),
  and decimal digit runs, negation markers, semver components, ISO dates, hex
  digests, and units are protected token classes no normalizer may collapse,
  excluding a candidate with explicit evidence unless a caller alias matches
  exactly; see `docs/MATCHING-QUALITY.md` for the lattice, the protected
  classes, and the curated 30-case evidence corpus that asserts zero incorrect
  automatic resolutions. Matching's protected-class and lattice-signal
  reservations (`matching.rs`) and the fuzzy edit-distance region scan
  (`edits/fuzzy.rs`) now use `Governor::lease_scratch`/`ScratchLease` in place
  of the earlier manual charge/`release_scratch` pairs; see the allocation
  oracle above for the measured bound this now carries. Broader allocation
  qualification beyond that fixed oracle workload remains open (Task 3 also
  fed forward into Tasks 4/5).
  The executed native/WASI string-ranking
  transcript now compares alias evidence and its receipt. Damerau matrix cells
  reserve at least eight bytes on 32-bit and 64-bit targets, keeping the
  governor decision and reported scratch usage portable for those targets;
  this remains an accounting reservation, not an allocator measurement.
  An independent breadth-first edit search checks both Levenshtein and
  unrestricted Damerau-Levenshtein on all 225 ordered pairs of binary strings
  up to three characters, with a separate Unicode transposition case. This
  finite oracle does not qualify arbitrary strings or ranking thresholds.
  String selection now applies score and margin policy across the complete
  caller candidate set, with exact precedence, weak-match rejection, guess
  status, and separate digest-bound ledgers for ambiguous alternatives. The
  `resolve_string` tool uses the same Rust operation through CLI and MCP.
  Focused tests cover exact, alias, weak, tie, invalid-policy, cancellation,
  and ledger-exhaustion behavior. Thresholds remain uncalibrated ranking
  policy; representative incorrect-correction evaluation is still required.
- Windows filesystem durability is implemented (`crates/host/src/files/windows.rs`:
  POSIX-semantics rename via `SetFileInformationByHandle`/`FileRenameInfoEx` on
  a write-through handle, `MoveFileExW` fallback, bounded sharing-violation
  retry, `FlushFileBuffers` directory durability with explicit
  access-denied evidence) and `Store::recover` now reports corrupt/torn
  journals per transaction and orphaned replacement temporaries instead of
  failing recovery outright. `crates/host/tests/crash_recovery.rs` proves
  exact-version crash/recovery/retry outcomes at eight commit checkpoints by
  re-executing its own test binary and aborting at each one; it runs on both
  Unix and Windows (no `cfg(unix)` gating beyond the pre-existing
  symlink/permission test). `cargo check -p purrassist-host -p
  purrassist-cli --target x86_64-pc-windows-gnu --all-targets` is clean
  locally. The `windows-engines` CI job now also runs `cargo test -p
  purrassist-host -p purrassist-cli --locked`. Hosted execution on a real
  Windows runner is the actual proof of this behavior and is still pending
  for this change on this branch; a green `windows-engines` run for the
  commit that lands this is the acceptance evidence, not the local
  cross-compile check alone.
- Native/WASM comparison now also covers the fixed helper transcripts and a
  browser-target execution, not only recovery and grammar acceptance.
  `purrassist-host` builds and runs unmodified for `wasm32-wasip1` (no module
  needed a `cfg`-disable: `files.rs`'s `std::fs` calls, including the
  Rust-1.89 `File::try_lock`/`unlock`, compile there as-is, and the crate's
  only conditional dependency, `windows-sys`, is `cfg(windows)`-gated and
  never resolved for a wasm target). `crates/host/examples/portable.rs`
  (`portable-host`) replays the three fixed provider transcripts (OpenAI
  Responses, Anthropic Messages, llama.cpp/chat-completions, copied verbatim
  from `crates/host/tests/helpers.rs`) through `Pending`, one governor per
  transcript, with no network, socket, or credential access; its digest,
  outputs, and full receipts execute identically natively and under WASI.
  A new unpublished, `publish = false` crate, `crates/wasm-harness`
  (`cdylib`), exports a `portable_main` entry point for
  `wasm32-unknown-unknown` that writes the exact same transcript
  `crates/core/examples/portable.rs` (`portable-core`) produces — the JSON
  body is shared via `crates/core/examples/portable_transcript.rs`, a
  `#[path]`-included module rather than a duplicate, so `purrassist-core`'s
  public API gains nothing new — into the module's own linear memory and
  returns a packed `(pointer, length)` pair. `scripts/run-wasm.mjs`
  instantiates that module with Node's plain `WebAssembly` API (no WASI
  import object) and decodes the result. `scripts/check-wasm.py` now runs
  five comparisons: native vs. `wasm32-wasip1` for `purrassist-core`,
  `purrassist-syntax`, and `purrassist-host`, plus native vs. executed
  `wasm32-unknown-unknown` for `purrassist-core`; all five match
  byte-for-byte. `scripts/check-release.py` already excluded any
  `publish = false` workspace member from the required publishable set, so
  `crates/wasm-harness` needed no change there. Node is now pinned
  (26.10.0) and its license notice registered in `vendor/tooling.json`.
- Execute hosted CI, audit final package contents and notices, configure trusted
  publishing, verify downloadable packages and installed behavior, and only then
  make a release. No remote publication settings have been changed.
