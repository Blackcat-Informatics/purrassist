<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Qualification record

This record describes the implemented 0.2.0 capability and its executable
qualification. Registry name records at 0.0.0 are distinct from a published
0.2.0 release. Status: merged to `main` (PR #2, `699d9ff`); tool-call recovery
(#3, #8) and `purrassist-derive` (#15) merged since; not yet tagged
`rust-v0.2.0`. Publication and downloaded-package acceptance follow the
procedure in [RELEASE.md](RELEASE.md). Open limitations are listed under
[Known limitations](#known-limitations).

## Capability and evidence

| Contract | Executable evidence | Scope |
| --- | --- | --- |
| Pure, portable first-party core | `make hygiene`, `make check` | Zero runtime dependencies; registered data/tooling/source notices; executed transcripts |
| JSON/schema validation | `schema_conformance`, `draft07`, `formats`, `vocabularies`, `schema_evidence` | 1,301 main Draft 2020-12 cases, 121 selected optional cases, all 878 vendored format/assertion cases; all 929 required Draft-07 cases and 905 of 911 Draft-07 optional cases (6 listed exclusions: annotation-only content, Draft 2019-09 cross-draft reference); nested observations and instance-scoped annotations |
| Pattern engine | `pattern`, `pattern_corpus`, `pattern_generation`, `witness_pattern_generation` | 441 `test262` Unicode-property fixtures; derivative/backtracking agreement; assertion-sensitive witnesses and exclusions; 3,024 exhaustive finite generation cases |
| Witness generation | `witness_algebra`, `witness_applicability`, `witness_formats`, `recursive_schemas` | 430 official-suite groups with valid instances witnessed; type-applicable bounds, occurrence-scoped contains counts, dialect-specific formats; explicit refutations and exhaustion; recursive/dynamic references |
| Composed recovery | `recovery_goldens`, `corruption_oracle`, `recovery` | 66 curated goldens; deterministic known-origin corruptions; schema revalidation and guess labels |
| Unicode | `unicode_conformance`, `matching_quality` | 20,034 normalization fixtures; refinement lattice and protected token contrasts |
| Markdown parsing/rendering | `markdown_corpus`, `markdown_render` | All 652 CommonMark and 672 GFM examples; source partitions and focused regressions |
| Structural document edits | `markdown_structure`, `markdown_relabel_preservation`, `markdown_renumber_preservation` and section/list/table/reference tests | Section insert/remove/move/rename; list move/renumber with body preservation; table edits/moves/padding; reference add/relabel/remove/unlink with display/alt preservation |
| Patch algebra and preservation | `patch_laws`, `preservation_properties`, `fuzzy_patch` | Invert/apply, composition, commute/rebase, independent splice oracle, unchanged bytes and loss spans |
| Alternative-specific ledgers | `provenance`, `edit_ambiguity`, `recovery_validation_binding`, `receipt_wire`, syntax tests | Digest-bound candidates and whole-schema validations, shared-node deduplication, ranked ambiguity, atomic ledger exhaustion; receipt wire v3 |
| Resource boundaries | `resource_accounting`, `schema_budgets`, `allocation_oracle`, syntax `allocation`, host `helpers` | Charge-before-allocation, cancellation, helper admission, measured allocator comparisons including zero-slack pattern compilation and generation, external parser accounts |
| Filesystem transaction boundaries | host `files`, `crash_recovery`, `journal_limits`, `orphan_preservation`, `edit_tools` | Eight crash checkpoints; bounded escaped journals, retry/recovery, temporary ownership checks, stale plans, locks, conditional undo and explicit batch outcomes |
| Shared Rust/CLI/MCP surface | host tests and CLI `protocol` | Single registry; every tool has CLI/MCP equivalence fixtures and schema-checked output |
| Tool-call grammars and dialects | `grammar_engine`, `grammar_codecs`, `tool_call_dialects`, `tool_call_negative` | DSL compilation and errors; linear fast path; split-invariant push recognition; `commit` points (no rejection before call-shaped evidence); `next` (several calls in one attempt); `insert` (an omitted token read as inserted); `optional-id`, `declaration-content` and `json-envelope` statements (the JSON envelope syntax slips on all six JSON-envelope dialects); factorized ambiguity; `raw`/`json`/`json-args`/`xml-params`/`kv-pairs`/`pythonic` codecs; canonical anchor fixtures for `qwen3-coder-xml.v1`, `hermes.v1`, `openai-tool-calls.v1`, `llama3-json.v1`, `mistral-v3.v1`, `mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`, `kimi-k2.v1`, `anthropic-tool-use.v1`, `harmony.v1`, `gemma3-tool-code.v1`, `glm-4.5.v1`, `bare-json.v1`, `pythonic.v1`, `deepseek-v3-stripped.v1`, `deepseek-v3.1-stripped.v1`, `harmony-stripped.v1`, `hermes-stripped.v1`, `kimi-k2-stripped.v1`, `llama3-json-stripped.v1`, `mistral-v11-stripped.v1`, and `mistral-v3-stripped.v1`, all twenty-three in one mixed turn; deliberate rejections; unparsed call markup reported |
| Tool-call resolution and recovery | `tool_definitions`, `tool_adapters`, `tool_call_rules`, `tool_call_goldens`, `tool_call_recover`, `tool_call_roundtrip` | Typed definitions; byte-exact `OpenAI`/MCP/Anthropic render-back including Draft-07 MCP schemas; registry admission, costs and loss kinds; `RULES.md`/`GRAMMAR.md` checked against the code; one captured golden per rule; mutating-guess guards; `Exact` effort; stop-reason semantics; pinned order-independent result digest; print/recognize round trips for every dialect pair; `OpenAI` `tool_calls` shape |
| Tool-call cost and quality | `tool_call_cost`, `tool_call_scale`, `correction_evaluation` | Published linear work bound and constants; 1 MiB arguments and 64-call busy turns under `ToolRecoveryRequest::sized`; 18,218 model turns with zero incorrect automatic answers and 100% abstention recall |
| Tool-call surfaces | host `tool_call_tools`, CLI `tool_calls_cli` and `protocol` | `recover_tool_calls` MCP tool and `recover-tool-calls` command equal direct core recovery; raw-byte input with invalid UTF-8 recorded; exit codes |
| Independent syntax | `make syntax-oracles` | 15 positive/negative Python, Rust, JavaScript/JSX, TypeScript/TSX compiler probes |
| Correction quality | `correction_evaluation`, `matching_oracle` | Four provenance-bound corpora, risk/coverage sweeps, exact distance and path oracles |
| Coverage-guided/differential probes | `make fuzz` (see [Known limitations](#known-limitations)) | Eleven targets (including `provider-log`), retained source-region novelty, exact failure replay, V8/cmark-gfm comparisons. The default `make fuzz` stops on no known signature, but the historical reports do not establish a passing default run for `tool-call-ref` |
| Typed tool definitions (`purrassist-derive`) | `crates/derive/tests/derive.rs`, `crates/derive/tests/crate_override.rs`, the `compile_fail` doctests in `crates/derive/src/lib.rs` (each rejected construct with a passing twin) and the derive README doctest, `crates/derive/src/{model,write,parse}.rs` unit tests (including the default-number agreement test against core's JSON parser), `crates/purrassist/tests/derive_facade.rs`, `crates/host/tests/derive_facade.rs`, core `tool_call::derive_support` unit tests and doctests, and `scripts/check-repository.py`'s README derive-example drift check | Derived definitions equal hand-built ones; recovery through `hermes.v1` and `qwen3-coder-xml.v1` converts to the struct with exact `FromNodeError` pointers; single-pass member classification (duplicate vs. missing vs. unknown); defaults, naming, `extra`, hygiene against shadowed names, recursion and every rejected input; nested undeclared members dropped with a recorded loss unless the nested struct declares `unknown`; `f32` and `Box<T>` fields; the `#[tool(crate = ..)]` override; the `__DERIVE_ABI_<major>_<minor>` marker; facade derives work with no direct core dependency; the crate has no dependencies (`make hygiene` audit) |
| Tool-call fuzzing | `make fuzz` (`TARGET=tool-call` alone; see [Known limitations](#known-limitations)) | Raw-byte model turns over every built-in dialect with fuzzed policy, stop, reasoning, tool choice, effort, ambiguity and chunking. The probe checks for panics, budget overruns, mutating-guess guards and span invariants, but the default `make fuzz` stops on no known signature, and neither tool-call target has a recorded 10,000-mutation baseline; see [FUZZING.md](FUZZING.md) |
| Structural helper detector | `make helper-test`, `make helper-census`, `make helper-check` | syn-based normalized body/shim/constant/table census, strict ledger validity and exact group enforcement; repository tooling only |
| Local release artifacts | `make package` | Six Cargo-verified archives in dependency order; source identity and registered notice audit |

These integration-test names are in `crates/core/tests` unless qualified with
another crate. `make check` runs the formatting, hygiene, Clippy, helper
detector self-tests, strict helper-ledger, workspace test, and Rustdoc gate
with warnings as errors. `make test` runs workspace unit tests, integration
tests, and doctests. The test profile uses light optimization and keeps debug
assertions and overflow checks. The corpus suites (`correction_evaluation`,
`corruption_oracle`, `markdown_corpus`, `matching_oracle`, `pattern_corpus`,
`schema_conformance`, `tool_call_scale`, `unicode_conformance`) are
`test = false` targets. `make conformance` runs those suites through
`make corpus` and runs the documentation checker and fuzz runner self-tests.
Run it locally as needed. The CI `corpus` job and release qualification both
run it. The coverage-guided and differential fuzzing campaign
and its corpus-feedback self-test run under `make fuzz` locally and
`make fuzz-hosted` in the CI `fuzz` job. The table's corpus counts, conformance totals and 1 MiB measurements
come from those lanes. Focused test results establish their listed
contracts; they do not replace these gates.

## Recovery and document contracts

Recovery retains exact input, including duplicate occurrences and numeric
spelling, and validates every answer against the complete schema. Invented and
inferred values are labeled guesses. Required unknown vocabularies fail schema
compilation; optional unknown vocabularies identify their URI and unevaluated
keywords in evidence. Formats are evaluated as annotations and asserted when
the dialect declares format-assertion. Unknown asserted formats fail compilation.
Nested format observations and pattern-engine identities remain visible even
when evaluated alternatives fail. Their evaluated-property/item annotations
remain scoped to the instance that owns them. Competing recovery alternatives
reference the passing whole-schema validation separately from format observations.

Witness generation reports `witness`, `unsatisfiable`, or `exhausted`. Only a
supported contradiction proof can establish unsatisfiability. Exhaustion,
cancellation, or an unsuccessful search cannot establish impossibility. Pattern
matching uses derivatives for regular patterns and bounded backtracking for
backreferences/lookaround; the chosen engine is evidence. Finite conformance
corpora establish tested behavior, rather than unrestricted proof of correctness.
Crossed bounds exclude only the types to which their keywords apply. Each
`contains` occurrence owns its counts; counts without `contains` have no effect.
Annotation-only formats do not constrain positive or negated witnesses.
Pattern generation retains start and word-boundary context; end-dependent
exclusions reject complete candidates without prematurely rejecting extensions.
Canonical alternation merges and nullable/derivative traversals use admitted
iterative storage, Work checks and cancellation polling. Flat derivative chains
do not require proportional native recursion; long-flat-pattern controls and
explicit compilation/matching/generation budget failures exercise this boundary.

Every document edit is a digest-bound patch. Applying its inverse restores the
original bytes. Disjoint intervening changes can be rebased with a transformation
receipt; noncommuting changes are refused. Lossless reference removal inlines
links, while unlinking records the dropped link semantics as a loss. Container
layout normalization, heading releveling, and materializing padded table cells
are reported transformations. Planning, application, and commit receipts remain
separate so selecting a plan preserves its earlier guess/ambiguity context.
Reference relabeling preserves visible link text and image alt text, including
shortcut and collapsed references, and rejects changes to rendering in either
dialect. List renumbering rejects marker-width changes that alter an item's body
structure or logical content.

The operation registry exposes structural editing, validation, witnesses,
helper request preparation/completion, file recovery/batch commits, and existing
buffer/file operations through the same Rust, CLI, and MCP handlers. Helper
adapters use caller-supplied responses and endpoints; the qualification
transcripts perform no model-backed execution.

## Tool-call recovery contracts

Tool-call recovery retains the exact output bytes, including bytes that are
not UTF-8, and itemizes every repair, loss and inference on the call that
records it. A guess or a data loss makes a call at most a best guess; a guess
on a mutating tool is never resolved, never usable and never emitted; a
required argument is never invented and a value is never coerced. A length
stop is truncation, and every other stop treats an open call as malformed.
Emission prints only the calls `usable_accepting_guesses` selects and lists
every other call and attempt as skipped. The integrator guarantees and the
test proving each are mapped in
[tool-calls/INTEGRATION.md](tool-calls/INTEGRATION.md); the 18,218-case
evaluation is a deterministic generated corpus, so its zero-incorrect result
establishes behavior on those cases rather than a production error rate.
Twenty-three dialects ship (`qwen3-coder-xml.v1`, `hermes.v1`,
`openai-tool-calls.v1`, `llama3-json.v1`, `mistral-v3.v1`,
`mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`, `kimi-k2.v1`,
`anthropic-tool-use.v1`, `harmony.v1`, `gemma3-tool-code.v1`, `glm-4.5.v1`, `bare-json.v1`, `pythonic.v1`, `deepseek-v3-stripped.v1`, `deepseek-v3.1-stripped.v1`, `harmony-stripped.v1`, `hermes-stripped.v1`, `kimi-k2-stripped.v1`, `llama3-json-stripped.v1`, `mistral-v11-stripped.v1`, and `mistral-v3-stripped.v1`). Streaming recognition (#16) is implemented.
The Rust APIs, CLI and MCP recover Anthropic Messages and OpenAI Chat
Completions event logs. The native acceptance harness replays recorded responses
and truncated prefixes, and the `provider-log` fuzz target compares
provider evidence and recovery across chunkings. The recording sources
and licenses are pinned in the [fixture provenance](../crates/acceptance-harness/src/provider_logs/README.md).
Current qualification of the added
recipes is pending automatic CI; the recorded evaluation numbers above
refer to the prior source.

## Tool-graph projection determinism

The native worked-example cases for MCP Extended and GBNF compare artifact
bytes and complete ordered loss ledgers across independent loads of the
same Turtle. MCP Extended and CompactSignature also have byte goldens.
A native case swaps the two complete tool declarations in the authored
Turtle, including their anonymous FnO lists, and compares CompactSignature
and GBNF bytes and ordered loss ledgers. MCP Extended parity after Turtle
reordering and host receipt parity remain open under
[tool-graph conformance](https://github.com/Blackcat-Informatics/purrassist/issues/39).

## Tool-graph grammar qualification

The GBNF lane emits both grammars from the worked Turtle graph. The Linux x86-64
qualification at `02ac7860` used the upstream parser and recognizer in
`test-gbnf-validator`, built from llama.cpp `b12543` and linked with the
matching CPU release. The validator ran with no model.
[Worked Turtle](tool-graph/TOOLS.md) · [Validator source](https://github.com/ggml-org/llama.cpp/blob/41d8297fe4c26b9ff304805178199dc14aa9ab52/tests/test-gbnf-validator.cpp)

The reference validator accepted both valid argument objects and refused
a non-string edit value, a customer ID outside its declared pattern, and
a grammar with an unresolved rule. The unmodified query grammar refused
`limit: 101`. Removing its upper bound admitted that value while leaving
zero refused and 100 accepted. The repository regression checks that the
original input schema refuses 101 and the lane records no input loss for
that bound.

## Tool-result byte-limit qualification

The full MCP Extended manifest test checks the v2 byte contract against the
worked RDF graph. It exercises the v1 bundle, legacy token limit, missing and
unknown enforcement modes, duplicate and unknown fields, a zero byte limit,
and a positive limit that disagrees with the source graph.

The shipped `project_result` tool exercises the same UTF-8 artifact on both
sides of its byte boundary through CLI `call` and MCP `tools/call`. Counting one
token per Unicode scalar gives eleven tokens for the twelve-byte JSON Lines
artifact. Both transports report exact-byte enforcement, the configured limit
and the twelve measured bytes. They release the artifact at twelve and
withhold it at eleven.

Both transports also check a valid result whose array uniqueness validation
exhausts the exact effort preset's work budget. The typed refusal and receipt
retain the work exhaustion and the original JSON stays available. The
standard effort preset releases the same result as a complete JSON Lines
artifact at its exact byte limit.

The graph-bound result path remains part of
[#37](https://github.com/Blackcat-Informatics/purrassist/issues/37).

## Resource and platform qualification

The core scratch governor uses shared meters, leases, and budgeted containers.
Its measured-allocation oracle compares representative operations at multiple
input scales with explicit per-operation slack. Pattern compilation and generation
also compare live and cumulative allocator requests against charges with no input
allowance or slack. Controls cover wide compile arenas, derivative and backreference
workloads, exact budget boundaries, exhaustion, and cancellation. Returned patterns
and witnesses retain their charges; temporary storage releases its charges. These
are evidence for those
workloads, rather than a whole-program allocator bound. External Tree-sitter
allocations have per-owner live/peak accounts, survive cross-thread tree drops,
and are reconciled at progress callbacks. The grammar-specific overshoot limits
and their measurement scope are in [RESOURCES.md](RESOURCES.md).

Windows replacement uses write-through handles, POSIX rename semantics when
accepted by the filesystem, an explicit write-through fallback, bounded sharing
violation retries, and evidence for directory-flush access denial. Crash tests
abort a subprocess at each journal/replacement checkpoint and verify exact
version recovery, retry, and undo. A cross-compile alone does not prove durability. The `windows-engines` lane of
[Verify run 36702475819](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36702475819)
ran the engine, host and CLI tests (including the host file and crash-recovery
tests) on Windows at `dc23bc81bebacef95f2b84d29dfd20031fdcd44c`, before PR #2
merged; that run does not cover later changes to `main`.
File transactions admit JSON escaping when bounding journal metadata and derive
parsing budgets from the admitted serialized size. Resource errors remain typed
errors in the Rust store; CLI/MCP handlers retain their explanations as tool
errors. Recovery deletes a temporary file only after validating its journal,
recorded target-parent path, and bounded replacement-byte prefix. Unknown or
unvalidated artifacts remain in place with an observable explanation.

The native acceptance harness exercises provider-log recovery, shared-token
cancellation, and tool-call recovery with a fuzzy tool name and a stray closer.
Run `cargo run --locked -p acceptance-harness --example portable-core` to print
the core transcript. `make check` executes its Rustdoc and native tests.

## Quality, fuzzing, and performance limits

[MATCHING-QUALITY.md](MATCHING-QUALITY.md) records corpus counts, conservative
abstention, zero observed incorrect automatic answers, risk/coverage sweeps,
and the assumptions needed for its binomial intervals. Deterministic, related
cases are not independent production samples; the interval cannot be interpreted
as a production error guarantee. The independent distance oracle exhausts all
14,641 ordered pairs of three-symbol strings up to length four.

[FUZZING.md](FUZZING.md) describes the eleven mutation targets, qualification
counts, retained coverage regions, independent-oracle scope, compilation
rejections/timeouts, and replay. Reports live in `docs/qualification/` and retain
the source/tool identities and deterministic seed. A bounded campaign establishes
that its executions passed, rather than absence of defects or a whole-crate
coverage percentage. Production fixes found by these campaigns have focused
regressions as well as the independent comparison.

[RESOURCES.md](RESOURCES.md) distinguishes accounting from measured allocations,
requested heap from RSS, whole-process time from per-operation latency, and
cache-disabled cold build cost from warm compilation. Use `make resources`
to collect diagnostic measurements on a comparable host. Hygiene validates
the committed report schema. Results are machine-specific observations.

## Helper census contract

The unpublished helper census parses repository Rust and groups normalized
function bodies, forwarding shims, integer constants and hex tables. These are
structural candidates for review; equality under normalization does not prove
semantic equivalence. `make helper-census` writes the actual inspection report.
`make helper-check` additionally rejects unsanctioned groups and forbidden
implementations against the exact helper ledger. Existing findings remain
visible without blanket sanctions.

`make check` requires detector positive/negative self-tests and strict ledger
checking (`make helper-check`): resolved symbols, documentation anchors, exact group membership, and
absence of cross-package source includes. The consolidation removed all six
body groups and six forwarding groups from the initial inspection. Only two
exact documented groups remain: distinct integer constructor input domains and
conversions into three distinct public error types. Shared resource conversion,
error construction, evidence serialization, Markdown source/publication logic,
percent decoding, word classification, and scratch storage now have one
implementation each. The portable core fixture belongs to the unpublished
harness Cargo library. See [HELPER-CENSUS.md](HELPER-CENSUS.md) and its actual
consolidation report for the source-bound census and review decisions.

## Known limitations

No limitation remains open at 0.2.0. The recorded list is empty and
matches its single source, [known-limitations.json](known-limitations.json);
`scripts/check-docs.py` compares the two.

The default `make fuzz` stops on no known signature. The recorded
`tool-call-ref` campaign of the earlier implementation exceeded the default
10 s `--timeout` at execution 682; [FUZZING.md](FUZZING.md) records that slow
input and the limits of the historical campaigns. The two tool-call targets
have no recorded 10,000-mutation baseline. `make fuzz-hosted`, which the CI
`fuzz` job and the pre-tag checklist in [RELEASE.md](RELEASE.md) run
(separately from `make release-gate`), uses the same default probe timeout for
every target. Every probe panic fails both fuzz commands.

## Qualification commands

```sh
make check
make conformance
make syntax-oracles
make fuzz-hosted
make package
RUSTUP_TOOLCHAIN=1.98.0 make release-gate
```

The last command reproduces release qualification locally: it runs the same
`make release-gate` on the release toolchain that `release-qualification.yml`
and the `Verify` workflow's `release-gate` job run.

The `Verify` workflow additionally runs native Linux/macOS gates, the
`release-gate` job (`make release-gate` on the release toolchain, Rust 1.98.0,
which is also the MSRV 1.98 build-and-test evidence), Windows engines/files/CLI,
and Linux qualification. Exact commit and run
evidence is recorded in issue #1 and the PR; a green run applies to its commit
only. The trusted-publishing workflow is separate from qualification and uses
the registered publisher identity.

## Release acceptance

The established issue #1 sequence reserves publication for the exact `main`
commit to be tagged. It requires the full gate and hosted CI on that commit, verified
trusted-publisher settings, explicit release-tag authorization, dependency-order
publication, all six downloadable archives, installed CLI/MCP transcripts from
crates.io, and the GitHub release. The verifier's offline local-registry tests
prove its detection path; successful local packages and installs cannot establish
that the real registry downloads work. Its default MCP transcript reads a seeded
file, applies a journaled replacement, conditionally undoes it, and requires
byte-exact restoration. Pipe writes and response reads have bounded timeouts,
with child cleanup tested for missing responses, partial output, and stderr
flooding. Until the real acceptance steps execute, 0.2.0 is not
claimed as published.


## Historical: Stage 2 record bound to `66b3756`

This section records the local integration at `66b3756`, which predates tool-call
recovery and `purrassist-derive`; it is not evidence for current `main`.
The recovery measurements report records no source commit, so it is bound to
`66b3756` by this record only; the other two reports record that commit. The
repaired engines passed the normal `make check`, `make syntax-oracles`,
`make wasm-test` and all-target `make fuzz` commands. Fresh source-bound
[smoke reports](qualification/fuzz-stage2-linux-x86_64.json),
[resource measurements](qualification/resources-stage2-linux-x86_64.json) and
[recovery measurements](qualification/recovery-bench-stage2-linux-x86_64.json)
record their actual observed identities; existing phase reports are unchanged.
The resource measurements passed the unchanged published limits. These commands
used the normal project framework without caller environment overrides or
alternate Cargo launchers. This local record is separate from registry publication
and downloaded-package acceptance in the approved release sequence.
