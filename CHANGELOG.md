<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Changelog

All notable changes to PurRAssist are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). All six release
crates share one version.

## [Unreleased]

### Added

- Qwen3-Coder XML recovery reads a function body element named after a
  declared parameter, written without its `<parameter=NAME>` wrapper
  (`<function=set_note><note>v</note></function>`), as that parameter. Such
  a body used to be a `malformed` rejected attempt. The new rule
  `tool_call.markup.bare-parameter-element` records the repair on each tag.
  It is a single-reading markup repair: `Structural` admits it, and
  `Effort::Exact` refuses it. A body that could be read another way is
  still rejected: an undeclared name, a name bound twice, text outside the
  elements, or markup inside the value.

### Changed

- Witness preparation and branch state now borrow immutable schema operands and
  retain fallible owners for synthesized lists, snapshots and cycle paths.
  Scans, copies, lookup and exact numeric comparisons share the caller governor;
  recursive preparation and continuations enforce caller/native depth limits.
  Resource refusal remains distinct from unsatisfiability. Shared dependency
  views also govern evaluator and recovery callers. Candidate construction,
  evaluator storage and final publication remain separate contracts.

- Strict JSON parsing now owns decoded strings, exact number spellings and
  container buffers until document publication or release. Schema compilation
  retains root and referenced documents and releases temporary meta-schemas and
  partial failures. Resource usage now reflects buffer growth and relocation;
  parsed values, duplicate ordering, spans and numeric spellings are preserved.

- Breaking: `grammar::edit::EditOperator` gains the variant
  `BareParameterElement`, so exhaustive matches no longer compile.
  `grammar::edit::OPERATOR_COUNT` and `OPERATORS.len()` go from 46 to 47, and
  `tool_call::rules::RULE_COUNT` grows by one.

### Fixed

- Schema compilation retains fallible, release-owned indexes and location
  metadata. Balanced indexes admit actual lookup and insertion paths; initial
  ordered routes and child tables are assembled directly. Temporary buffers
  release after construction, and completed index storage releases with its
  schema. Parsed JSON storage remains a separate resource contract.

- Compiler linkage, validation, witness generation and recovery share governed
  URI and reference resolution, including pointer decoding and dynamic scopes.
  Native resource refusals remain distinct from malformed references. Work and
  scratch counters, and therefore finite-budget refusal points, change to
  reflect the admitted operations and retained owners. Compiler keyword and
  vocabulary checks now admit lookups, temporary duplicate sets, exact numeric
  classification and diagnostics instead of a blanket Work allowance. Buffer
  growth admits relocation, and decoding reserves known bounds before writing.

- Remove artifact-size measurement, reporting and qualification limits. Resource
  qualification retains runtime, memory, correctness and build-time checks.

- Recursive schema recovery factors certified independent mandatory array wraps
  through shared schema/occurrence states, avoiding combinations of unrelated
  unwraps. Minimum costs are solved before choosing edits; the existing search
  still validates and publishes the result and retains the same accepted wrap
  evidence, losses, ordering and source spans. Work and candidate accounting now
  include chart preparation, including unsuccessful certification, and omit
  discarded combinations. Rejected validation diagnostics and finite-budget
  exhaustion points change with that exploration. The certificate currently
  covers static references and typed
  object/array/string products with guesses disabled. Coupled constraints,
  ambiguous optimal transformations, decoded strings and unproved name or
  relocation interactions retain the exact joint search; no universal polynomial
  complexity claim is made for that fallback.

- Refresh CLI and MCP receipt usage fixtures after governed schema bundle
  identity hashing. The captured responses change only numeric usage counters;
  requests, answers, schema identities and transformation evidence are unchanged.

- Rejected tool-call spans retain only demonstrated markup before a reasoning
  marker. Resynchronized groups report each rejected part's own failure.
  The fresh call-body oracle has regression seeds and an injected-overrun
  check. Hosted fuzzing stops on every probe panic.

### Known limitations

These remaining limitations were recorded on 2026-10-26. The 0.1.0 release
accepted them without blocking publication. The list
matches its single source, [known-limitations.json](docs/known-limitations.json);
`scripts/check-docs.py` compares the two.

- [#7](https://github.com/Blackcat-Informatics/purrassist/issues/7) Audit the unreviewed #3 stage-3 commits and close the F14/F19 rule's known limits. Some commits of the #3 tool-call work were merged without an audit, and the known limits of the F14/F19 rule remain open.
- [#10](https://github.com/Blackcat-Informatics/purrassist/issues/10) Tool-call recovery: restarted DeepSeek-V3.1 sections cost quadratic work and super-quadratic time. A turn with many restarted DeepSeek-V3.1 sections costs quadratic work and super-quadratic time.
- [#11](https://github.com/Blackcat-Informatics/purrassist/issues/11) Tool-call recovery: repair search under-charges work on mutated free-form-item turns (seconds for 192 bytes). One 192-byte mutated turn takes about 5 s in a release build, so the default `make fuzz` exceeds its timeout on `tool-call-ref`; the CLI and MCP sized budget ends it explicitly in 0.3 s.
- [#12](https://github.com/Blackcat-Informatics/purrassist/issues/12) Tool-call recovery: add the pythonic.v1 dialect and pythonic codec. `pythonic.v1` is not built in, so its call markup is not recovered as a tool call.
- [#13](https://github.com/Blackcat-Informatics/purrassist/issues/13) Tool-call recovery: add the glm-4.5.v1 dialect and kv-pairs codec. `glm-4.5.v1` is not built in, so its call markup is not recovered as a tool call.
- [#14](https://github.com/Blackcat-Informatics/purrassist/issues/14) Tool-call recovery: add the gemma3-tool-code.v1 dialect. `gemma3-tool-code.v1` is not built in, so its call markup is not recovered as a tool call.
- [#16](https://github.com/Blackcat-Informatics/purrassist/issues/16) Tool-call recovery: streaming recognizer (ToolCallStream, stable-prefix events, CLI --stream, MCP progress). There is no `ToolCallStream`, no stable-prefix events, no CLI `--stream` option and no MCP progress reporting for recovery.
- [#17](https://github.com/Blackcat-Informatics/purrassist/issues/17) Tool-call recovery: reassemble provider streaming events (Anthropic SSE tool_use, OpenAI tool_calls deltas). Anthropic SSE `tool_use` and OpenAI `tool_calls` delta streams are not reassembled into a turn.
- [#18](https://github.com/Blackcat-Informatics/purrassist/issues/18) Tool-call recovery: special-token-stripped dialect variants (*-stripped.v1). Dialect variants for output whose special tokens were stripped (`*-stripped.v1`) are not built in.

## [0.1.0] - 2026-10-13

The first release: six crates, one recovery engine, and three ways to use it.
The [release procedure](docs/RELEASE.md) records the publication gates.

### Highlights

- **Rust crate**: `purrassist` is the Rust interface. It re-exports the pure
  engines, the syntax integration and the derives, so
  `Schema::compile(..).recover(..)` returns an outcome (resolved, best guess,
  ambiguous or unresolved) with a receipt of what changed. Typed tool
  definitions derive from Rust structs and enums with
  `#[derive(ToolDefinition)]` and `#[derive(ToolEnum)]`. See the
  [crate README](crates/purrassist/README.md).
- **MCP server**: `purrassist mcp --root DIR` serves every tool over
  newline-delimited JSON-RPC on stdio. It handles cancellation while tools run
  and admits at most four active tool calls. See the
  [tool reference](docs/TOOLS.md).
- **CLI**: the `purrassist` executable offers `decode`, `validate`, `match`,
  `recover-tool-calls`, `call` and `mcp`. `purrassist call TOOL --root DIR`
  invokes the same operation as MCP, and a test checks that the CLI and MCP
  results are identical for every registered tool. See the
  [README](README.md#one-engine-three-interfaces).

### Crates

Release order:

| Crate | Role |
|---|---|
| `purrassist-core` | The pure recovery, matching, schema, Markdown and tool-call engines, with zero third-party runtime dependencies. |
| `purrassist-derive` | Zero-dependency `#[derive(ToolDefinition)]` and `#[derive(ToolEnum)]`. |
| `purrassist-syntax` | Isolated Tree-sitter grammar integration for source inspection and validated repairs. |
| `purrassist` | The Rust facade that re-exports the engines and the derives. |
| `purrassist-host` | Root-scoped files, journaled replacement and the shared tool registry behind the CLI and the MCP server. |
| `purrassist-cli` | The `purrassist` executable: the CLI and the stdio MCP server. |

### Capability areas

- **JSON recovery**: nested unwrapping, syntax repair, approximate keys and
  enums, stringified values and whole-schema validation against Draft 2020-12
  (the default) or Draft-07, with invented values produced by a first-party
  witness-generation algebra. See the
  [implementation contract](docs/IMPLEMENTATION.md).
- **Matching**: Unicode, lexical, Levenshtein, Damerau-Levenshtein, English
  Soundex, alias and contextual path matching, explicit semantic domain rules,
  and caller-supplied embedding similarity. No model is required, loaded or
  contacted by default.
- **Evidence receipts**: every recovery, matching, Markdown and edit-plan
  result carries its sources, spans, transforms, alternatives, losses and
  inferences. The receipt wire format is described by
  [receipt.schema.json](docs/schema/receipt.schema.json).
- **Markdown and edits**: source-preserving CommonMark/GFM parsing and
  rendering, structural section, list, table and reference operations, bounded
  fuzzy replacement, relocatable unified diffs and exact-byte undo. See
  [plan application](docs/EDIT-APPLICATION.md).
- **Syntax inspection**: Python, Rust, JavaScript/JSX and TypeScript/TSX
  grammar inspection and validated punctuation repairs. Grammar acceptance does
  not prove program correctness.
- **Files**: root-scoped, journaled single-file replacement and conditional
  undo for cooperating writers, with explicit multi-file outcomes. It is not a
  sandbox against hostile processes.
- **Tool-call recovery**: recovery of the tool calls of one model turn from raw
  output, across twelve built-in dialects, with itemized repairs and
  conservative guards for mutating tools. See
  [tool-call recovery](docs/tool-calls/README.md).
- **Helpers and limits**: resumable helper request/response adapters
  ([helpers](docs/HELPERS.md)), caller-owned cancellation
  ([cancellation](docs/CANCELLATION.md)) and finite, explicit resource budgets
  ([resources](docs/RESOURCES.md)). Exhaustion is reported, never a success.

Executable evidence and its limits are in the
[qualification record](docs/QUALIFICATION.md).

### Compatibility

- Rust edition 2024, minimum supported Rust version (MSRV) 1.98.
- `purrassist-derive` and `purrassist-core` must share a `major.minor` version;
  their patch versions may differ. Generated code references
  `purrassist_core::tool_call::__DERIVE_ABI_<major>_<minor>`, so a patch release
  must not change what generated code needs from core: such a change requires a
  minor version bump, which renames the marker. See the
  [release procedure](docs/RELEASE.md).
- JSON Schema Draft 2019-09, draft-06 and earlier dialects are rejected at
  compilation.

### Known limitations

These issues were open on 2026-10-12 and were accepted for the 0.1.0
release published on 2026-10-13. This historical list records that release;
the preceding section tracks the current source.

- [#6](https://github.com/Blackcat-Informatics/purrassist/issues/6) Tool-call fuzz: finish the rejected-span oracle failures left at the #3 merge. A plain tool-call fuzz campaign stops at the first rejected-span oracle panic, so the default `make fuzz` fails there.
- [#7](https://github.com/Blackcat-Informatics/purrassist/issues/7) Audit the unreviewed #3 stage-3 commits and close the F14/F19 rule's known limits. Some commits of the #3 tool-call work were merged without an audit, and the known limits of the F14/F19 rule remain open.
- [#10](https://github.com/Blackcat-Informatics/purrassist/issues/10) Tool-call recovery: restarted DeepSeek-V3.1 sections cost quadratic work and super-quadratic time. A turn with many restarted DeepSeek-V3.1 sections costs quadratic work and super-quadratic time.
- [#11](https://github.com/Blackcat-Informatics/purrassist/issues/11) Tool-call recovery: repair search under-charges work on mutated free-form-item turns (seconds for 192 bytes). One 192-byte mutated turn takes about 5 s in a release build, so the default `make fuzz` exceeds its timeout on `tool-call-ref`; the CLI and MCP sized budget ends it explicitly in 0.3 s.
- [#12](https://github.com/Blackcat-Informatics/purrassist/issues/12) Tool-call recovery: add the pythonic.v1 dialect and pythonic codec. `pythonic.v1` is not built in, so its call markup is not recovered as a tool call.
- [#13](https://github.com/Blackcat-Informatics/purrassist/issues/13) Tool-call recovery: add the glm-4.5.v1 dialect and kv-pairs codec. `glm-4.5.v1` is not built in, so its call markup is not recovered as a tool call.
- [#14](https://github.com/Blackcat-Informatics/purrassist/issues/14) Tool-call recovery: add the gemma3-tool-code.v1 dialect. `gemma3-tool-code.v1` is not built in, so its call markup is not recovered as a tool call.
- [#16](https://github.com/Blackcat-Informatics/purrassist/issues/16) Tool-call recovery: streaming recognizer (ToolCallStream, stable-prefix events, CLI --stream, MCP progress). There is no `ToolCallStream`, no stable-prefix events, no CLI `--stream` option and no MCP progress reporting for recovery.
- [#17](https://github.com/Blackcat-Informatics/purrassist/issues/17) Tool-call recovery: reassemble provider streaming events (Anthropic SSE tool_use, OpenAI tool_calls deltas). Anthropic SSE `tool_use` and OpenAI `tool_calls` delta streams are not reassembled into a turn.
- [#18](https://github.com/Blackcat-Informatics/purrassist/issues/18) Tool-call recovery: special-token-stripped dialect variants (*-stripped.v1). Dialect variants for output whose special tokens were stripped (`*-stripped.v1`) are not built in.
