<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Qualification record

This is an implementation checkpoint, not release approval. The complete accepted
scope in `IMPLEMENTATION.md` remains required. No registry publication is claimed.

## Executable checks

| Check | Command | Coverage |
| --- | --- | --- |
| Local gate | `make check` | Formatting, provenance, Clippy, optimized tests, Rustdoc |
| Schema | `cargo test -p purrassist-core --test schema_conformance` | 1,301 main Draft 2020-12 tests; 121 selected optional tests |
| Unicode | `cargo test -p purrassist-core --test unicode_conformance` | 20,034 Unicode 17 normalization cases |
| Portability | `make wasm-test` | Executed native/WASI recovery and all five grammar transcripts; browser-WASM core build |
| Independent syntax | `make syntax-oracles` | 15 fixed positive/negative probes against Python, rustc, Node, and TypeScript 5.9.3 |
| Baseline | `make benchmark` | Six deterministic recovery cases, 200 samples each, quality assertions and latency |
| Resources | `make resources` | Nine assertion-backed processes, Memcheck allocations, Massif heap observations, native RSS, and native executable/build measurements; Linux prerequisites in `RESOURCES.md` |
| Coverage-guided JSON | `make fuzz-json` | Source-region feedback, retained mutation corpus, round-trip/schema/receipt properties, explicit failures and replay; prerequisites and limits in `FUZZING.md` |
| MSRV | `rustup run 1.98.0 cargo test --workspace --locked` | Workspace tests on Rust 1.98 |
| Packages | `make package` | Cargo package construction and verification in dependency order |

Selected optional schema suites cover large numbers, overflow, ECMA regex cases,
non-BMP characters, references into unknown keywords, anchors, dynamic references,
identifiers, missing dialect declarations, and unknown keywords. They do not imply
format-assertion support or unrestricted ECMAScript regex support.

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
  Full HTML-output comparison against the pinned official corpora now passes;
  this does not finish structural editing or resource/performance qualification.
- Extend composed recovery/witness generation and regex coverage; audit recursive
  schemas and resource-sensitive compilation. Qualify unsupported vocabulary and
  format behavior without silently weakening assertions.
- Complete allocation/work accounting throughout candidate generation, receipts,
  schema compilation, and helper preparation. Schema compilation now shares a
  governor across root/external/dialect parsing, indexing, pattern preparation,
  bundle hashing, and subsequent recovery. It exposes typed resource errors and
  avoids subtree cloning for reference-only targets. These accounting additions
  do not establish measured allocator bounds or complete the allocation audit. Bound external parser allocations;
  progress cancellation alone is not a hard memory limit.
- Extend alternative-specific ledgers beyond JSON recovery and semantic resolution,
  and finish uniform
  Rust/CLI/MCP access to remaining document operations. Domain-rule
  intent resolution and path resolution now use the shared tool dispatcher,
  including epistemic status and one governor across domain/index admission
  and resolution. Markdown inspection, context extraction, section-body plans,
  nested task-checkbox plans, and buffer/conditional-file plan application also
  share the tool dispatcher.
- Expand independent oracle and adversarial corpora, coverage-guided fuzzing beyond
  the bounded JSON campaign,
  memory/allocation measurements, build/artifact metrics, and representative
  incorrect-correction evaluation beyond the small baseline.
- Qualify Windows filesystem durability and crash boundaries. Unix tests and a
  configured Windows engine job do not establish Windows host support.
- Execute hosted CI, audit final package contents and notices, configure trusted
  publishing, verify downloadable packages and installed behavior, and only then
  make a release. No remote publication settings have been changed.
