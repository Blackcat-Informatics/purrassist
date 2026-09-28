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
reproducible malformed JSON inputs; this is not coverage-guided fuzz qualification.

Ambiguous JSON recovery results carry separate ordered alternative ledgers with
output digests, search costs, transformations, losses, inferences, and indices into
the actual validation log. Regressions check candidate association, wire encoding,
guess reporting, and exhaustion before a complete ledger can be returned.

Local package verification has built all five packages. A local release CLI was
installed into a disposable checkout cache and exercised on the published example
inputs, including exact-source retention. This does not verify downloadable registry
packages. Hosted CI passed for checkpoint `4d3d204b0fd8bd3075808dc4cb3e3604cc15340d`:
[run 36399984984](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36399984984)
completed the Linux/macOS local gates, MSRV workspace tests, Windows core/syntax
tests, and native/WASI comparisons. This is evidence for that checkpoint only;
subsequent changes need their own qualification.

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
The prior inline foundation
passed hosted CI, including the corrected nightly test assertions, as recorded
above. This is not full inline or document conformance: entities, reference links,
GFM extensions, and complete document rendering remain open.

## Remaining release work

- Complete CommonMark/GFM container and inline parsing and structural editing;
  execute full HTML-output conformance against the pinned official corpora. The
  Markdown module remains an editing index; block-aligned context extraction and
  checkbox-state edits are implemented, with full-source identity and exact undo.
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
  Rust/CLI/MCP access to embedding similarity and remaining document operations. Domain-rule
  intent resolution and path resolution now use the shared tool dispatcher,
  including epistemic status and one governor across domain/index admission
  and resolution. Markdown inspection, context extraction, section-body plans,
  and root task-checkbox plans also share the tool dispatcher.
- Expand independent oracle and adversarial corpora, coverage-guided fuzzing,
  memory/allocation measurements, build/artifact metrics, and representative
  incorrect-correction evaluation beyond the small baseline.
- Qualify Windows filesystem durability and crash boundaries. Unix tests and a
  configured Windows engine job do not establish Windows host support.
- Execute hosted CI, audit final package contents and notices, configure trusted
  publishing, verify downloadable packages and installed behavior, and only then
  make a release. No remote publication settings have been changed.
