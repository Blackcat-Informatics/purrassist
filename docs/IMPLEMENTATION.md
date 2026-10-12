<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Accepted implementation contract

Status: merged to `main` (PR #2, `699d9ff`); tool-call recovery (#3, #8) and
`purrassist-derive` (#15) merged since; not yet tagged `rust-v0.1.0`. Executable
qualification and its limits are recorded in `QUALIFICATION.md`; registry
publication acceptance follows the procedure in `RELEASE.md`.

PurRAssist recovers intent from imperfect AI output and requests. Every result
combines its answer with evidence, provenance, losses, and explicit inferences.
The governing requirements are in `.goals`. Maximal utility, performance, and
portability apply together. Native execution uses native capabilities while
computational engines remain wasmable.

## Agreed release scope

- First-party JSON parsing, Draft 2020-12 and Draft-07 schema validation
  (dialect selected by `$schema`; Draft 2020-12 by default; other dialects
  are rejected), and composed healing.
  Preserve duplicate occurrences and exact numeric spelling. Repair prose,
  fences, malformed syntax, truncated structures, wrappers, stringified JSON,
  approximate keys/enums, misplaced fields, and incompatible shapes/types.
- Permit schema-valid speculative values; identify each invention or inference.
  Validate the complete candidate and never silently weaken constraints.
- Exact, Unicode, case-folded, depunctuated, lexical, edit-distance, phonetic,
  alias, and semantic matching, including path context and candidate indexes.
- Model-free concepts/domain rules, caller embeddings and scorers, and helper
  adapters. Helpers propose candidates; the engine validates their proposals.
- Source-preserving CommonMark/GFM structure, selection, section editing,
  lists/tasks, tables, references, and context extraction.
- Syntax-aware Python, Rust, JavaScript/JSX, TypeScript/TSX editing. The initial
  parser exception is one isolated Tree-sitter grammar family. Edit selection,
  repair, and validation policy remain first-party Rust.
- Robust fuzzy replacement/diff application and automatic application of
  unambiguous validated edits, with receipts and conditional undo.
- Root-scoped file tools, coordinated writer locks, version checks, atomic
  single-file replacement, recovery journals, and explicit multi-file outcomes.
- Tool-call recovery (`tool_call.recover.v1`, issue #3): recover the tool
  calls of one model turn from raw output bytes against typed tool
  definitions or lossless `OpenAI`, MCP, and Anthropic declarations, whose
  exact parameters schemas are validated under Draft 2020-12 or Draft-07 (the
  dialect MCP servers commonly declare). Dialects are weighted, invertible
  grammars in a first-party DSL; twelve ship (`qwen3-coder-xml.v1`,
  `hermes.v1`, `openai-tool-calls.v1`, `llama3-json.v1`, `mistral-v3.v1`,
  `mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`, `kimi-k2.v1`,
  `anthropic-tool-use.v1`, `harmony.v1`, `bare-json.v1`) and callers compile
  their own. A
  best-first search repairs call markup with named, costed rules; each call
  is resolved against its tool with a per-call outcome, itemized repairs,
  markup/data losses, ranked alternatives, and caller-chosen ids. Effects
  select conservative policies: a guess on a mutating tool is never resolved,
  usable, or emitted. The model's stop reason is an input, a length stop is always
  `finish_reason` `"length"`, and recovered calls print in any dialect or as
  the `OpenAI` `tool_calls` array. Recovery is deterministic, governed by a
  published linear cost bound, and exposed through Rust, CLI
  (`recover-tool-calls`), and MCP (`recover_tool_calls`). Typed tool
  definitions can be derived from Rust structs and enums (`purrassist-derive`).
  Still tracked: the `pythonic.v1` (#12), `glm-4.5.v1` (#13) and
  `gemma3-tool-code.v1` (#14) dialects, the `*-stripped` variants (#18),
  streaming recognition (#16), and provider streaming events (#17).
  See `tool-calls/README.md`.
- Rust facade, CLI, and stdio MCP server using the same operations.
- Standard helper-provider adapters and caller-managed endpoints. No model is
  required. Bundled small models remain an evaluation track, not a prerequisite.

## Shared contracts

Intent + input + context -> candidates -> validation/ranking -> result + receipt.

Effort presets: Exact, Quick, Standard (default), Deep, Maximum. Explicit
budgets bound work, memory, candidates, depth, cancellation, and helper usage.
Results distinguish resolved answers, best guesses, ambiguity, and unsuccessful
recovery. Search exhaustion does not prove unsatisfiability.

Receipts identify sources, spans, transforms, alternatives, scores, validations,
rules, schemas, parsers, policies, helpers, consumed resources, and termination.
Losses and inferences are separate ledgers. Receipt serialization is versioned;
`docs/schema/receipt.schema.json` is the JSON Schema for the current version,
validated against this crate's own schema engine in
`crates/core/tests/receipt_wire.rs`. Evidence, losses, inferences, and
validation references shared by every competing alternative of an ambiguous
operation are deduplicated into a derivation forest
(`crates/core/src/provenance.rs`) and promoted once to the top-level ledgers,
rather than duplicated into each `AlternativeReceipt`.
Core operations are synchronous and resumable around supplied helper responses.

## Workspace boundaries

- `purrassist-core`: first-party pure engines and common contracts; zero external
  runtime dependencies.
- `purrassist-derive`: proc-macro crate with `#[derive(ToolDefinition)]` and
  `#[derive(ToolEnum)]`; zero dependencies (only the compiler's `proc_macro`),
  with `purrassist-core` as a dev-dependency only. Core never depends on it.
- `purrassist-syntax`: isolated syntax grammar integration.
- `purrassist`: standard public facade; re-exports the derives.
- `purrassist-host`: filesystem and helper integration behind both the CLI and
  the MCP server.
- `purrassist-cli`: executable and stdio MCP server.
- `wasm-harness`: internal, unpublished native, WASI and browser-WASM core
  acceptance transcript harness.
- `purrassist-helper-census`: internal, unpublished structural duplicate
  functionality census and helper ledger checks.

## Required delivery sequence

1. Foundation: contracts, evidence, budgets, repository/dependency gates, fixtures.
   Delivered.
2. Recovery: JSON/schema, matching, semantics, candidate search and effort.
   Delivered.
3. Documents/files: Markdown, syntax, edit planning/application, recovery, undo.
   Delivered.
4. Unified surfaces: helpers, CLI, MCP, executable end-to-end examples.
   Delivered.
5. Qualification: conformance, fuzzing, portability, performance, packaging,
   documentation, registry setup, and verified publication. Qualification
   evidence is recorded in `QUALIFICATION.md`; registry setup is partly done
   (see `RELEASE.md`); verified publication is open, as 0.1.0 is not yet tagged.

Every stage remains required. A passing subset does not qualify the release.

## Acceptance

Run official JSON Schema and CommonMark/GFM suites, independent syntax oracles,
composed-recovery goldens, Unicode/path/phonetic ambiguity tests, crash/stale/
retry/undo tests, preservation/replay/ledger properties, bounded fuzzing, and
executed native/WASM comparisons with fixed helper transcripts. Measure quality,
incorrect correction, latency, allocation, memory, build cost and artifact size.

Tool-call recovery additionally requires: canonical anchor fixtures for every
built-in dialect; one captured golden per registry rule; a negative corpus;
print/recognize round trips for every dialect pair; byte-exact declaration
render-back for all three wire forms, including Draft-07 MCP schemas; a pinned
deterministic result digest independent of tool and dialect order; the
published cost bound checked on 1 MiB arguments; a correction evaluation with
zero incorrect automatic answers and full abstention recall; the registry and
its published table in agreement (`tool-calls/RULES.md`); a coverage-guided
`tool-call` fuzz target; and the tool-call transcript in the native/WASM
comparison. `tool-calls/INTEGRATION.md` maps each integrator guarantee to its
test.

`make check` runs formatting, lint, optimized build/test, Rustdoc, and repository
hygiene with warnings as errors. Verification preserves debug/overflow checks.
Publication additionally requires conformance, packaging, installation, hosted
CI, documentation, and registry verification. Never bypass commit verification.

## Publication and identity

Use PurRAssist in prose; identifiers are lowercase `purrassist`. Preserve the
family logo's `cat-head-core` and palette, replacing its service object with a
repaired-structure emblem. First-party licensing is MIT OR Apache-2.0 OR
MulanPSL-2.0. Preserve third-party source attribution and license boundaries.

Rust edition 2024, stable MSRV 1.98, nightly development analysis, stable release
builds. Workspace version 0.1.0; release tags `rust-v*`. Publish in dependency
order with trusted publishing after registry setup. Verify actual downloadable
packages and CLI/MCP behavior before declaring completion.
