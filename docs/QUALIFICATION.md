<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Qualification record

This record describes the implemented 0.1.0 capability and its executable
qualification. Registry name records at 0.0.0 are distinct from a published
0.1.0 release. Publication and downloaded-package acceptance follow the review
and merge sequence in [RELEASE.md](RELEASE.md).

## Capability and evidence

| Contract | Executable evidence | Scope |
| --- | --- | --- |
| Pure, portable first-party core | `make hygiene`, `make wasm-test` | Zero runtime dependencies; registered data/tooling/source notices; executed transcripts |
| JSON/schema validation | `schema_conformance`, `draft07`, `formats`, `vocabularies`, `schema_evidence` | 1,301 main Draft 2020-12 cases, 121 selected optional cases, all 878 vendored format/assertion cases; all 929 required Draft-07 cases and 905 of 911 Draft-07 optional cases (6 listed exclusions: annotation-only content, Draft 2019-09 cross-draft reference); nested observations and instance-scoped annotations |
| Pattern engine | `pattern`, `pattern_generation`, `witness_pattern_generation` | 441 test262 Unicode-property fixtures; derivative/backtracking agreement; assertion-sensitive witnesses and exclusions; 3,024 exhaustive finite generation cases |
| Witness generation | `witness_algebra`, `witness_applicability`, `witness_formats`, `recursive_schemas` | 430 official-suite groups with valid instances witnessed; type-applicable bounds, occurrence-scoped contains counts, dialect-specific formats; explicit refutations and exhaustion; recursive/dynamic references |
| Composed recovery | `recovery_goldens`, `corruption_oracle`, `recovery` | 66 curated goldens; deterministic known-origin corruptions; schema revalidation and guess labels |
| Unicode | `unicode_conformance`, `matching_quality` | 20,034 normalization fixtures; refinement lattice and protected token contrasts |
| Markdown parsing/rendering | `markdown_corpus`, `markdown_render` | All 652 CommonMark and 672 GFM examples; source partitions and focused regressions |
| Structural document edits | `markdown_structure`, `markdown_relabel_preservation`, `markdown_renumber_preservation` and section/list/table/reference tests | Section insert/remove/move/rename; list move/renumber with body preservation; table edits/moves/padding; reference add/relabel/remove/unlink with display/alt preservation |
| Patch algebra and preservation | `patch_laws`, `preservation_properties`, `fuzzy_patch` | Invert/apply, composition, commute/rebase, independent splice oracle, unchanged bytes and loss spans |
| Alternative-specific ledgers | `provenance`, `edit_ambiguity`, `recovery_validation_binding`, `receipt_wire`, syntax tests | Digest-bound candidates and whole-schema validations, shared-node deduplication, ranked ambiguity, atomic ledger exhaustion; receipt wire v2 |
| Resource boundaries | `resource_accounting`, `schema_budgets`, `allocation_oracle`, syntax `allocation`, host `helpers` | Charge-before-allocation, cancellation, helper admission, measured allocator comparisons including zero-slack pattern compilation and generation, external parser accounts |
| Filesystem transaction boundaries | host `files`, `crash_recovery`, `journal_limits`, `orphan_preservation`, `edit_tools` | Eight crash checkpoints; bounded escaped journals, retry/recovery, temporary ownership checks, stale plans, locks, conditional undo and explicit batch outcomes |
| Shared Rust/CLI/MCP surface | host tests and CLI `protocol` | Single registry; every tool has CLI/MCP equivalence fixtures and schema-checked output |
| Tool-call grammars and dialects | `grammar_engine`, `grammar_codecs`, `tool_call_dialects`, `tool_call_negative` | DSL compilation and errors; linear fast path; split-invariant push recognition; `commit` points (no rejection before call-shaped evidence); `next` (several calls in one attempt); factorized ambiguity; `raw`/`json`/`xml-params` codecs; canonical anchor fixtures for `qwen3-coder-xml.v1`, `hermes.v1`, `openai-tool-calls.v1`, `llama3-json.v1`, `mistral-v3.v1`, `mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`, `kimi-k2.v1`, `anthropic-tool-use.v1` and `harmony.v1`; deliberate rejections |
| Tool-call resolution and recovery | `tool_definitions`, `tool_adapters`, `tool_call_rules`, `tool_call_goldens`, `tool_call_recover`, `tool_call_roundtrip` | Typed definitions; byte-exact `OpenAI`/MCP/Anthropic render-back including Draft-07 MCP schemas; registry admission, costs and loss kinds; `RULES.md`/`GRAMMAR.md` checked against the code; one captured golden per rule; mutating-guess guards; `Exact` effort; stop-reason semantics; pinned order-independent result digest; print/recognize round trips for every dialect pair; `OpenAI` `tool_calls` shape |
| Tool-call cost and quality | `tool_call_cost`, `correction_evaluation` | Published linear work bound and constants; 1 MiB arguments and 64-call busy turns under `ToolRecoveryRequest::sized`; 12,396 mutated turns with zero incorrect automatic answers and 100% abstention recall |
| Tool-call surfaces | host `tool_call_tools`, CLI `tool_calls_cli` and `protocol` | `recover_tool_calls` MCP tool and `recover-tool-calls` command equal direct core recovery; raw-byte input with invalid UTF-8 recorded; exit codes |
| Independent syntax | `make syntax-oracles` | 15 positive/negative Python, Rust, JavaScript/JSX, TypeScript/TSX compiler probes |
| Correction quality | `correction_evaluation`, `matching_oracle` | Four provenance-bound corpora, risk/coverage sweeps, exact distance and path oracles |
| Coverage-guided/differential probes | `make fuzz` | Nine targets (including `tool-call`), retained source-region novelty, exact failure replay, V8/cmark-gfm comparisons |
| Tool-call fuzzing | `make fuzz` (`TARGET=tool-call` alone) | Raw-byte model turns over every built-in dialect with fuzzed policy, stop, reasoning, tool choice, effort, ambiguity and chunking; no panic, budget respected, mutating-guess guards and span invariants hold; see [FUZZING.md](FUZZING.md) |
| Resource/performance observations | `make resources`, resource-limit checker | Latency, real allocations, heap/RSS, artifact hashes/sizes, cache-disabled cold build |
| Structural helper detector | `make helper-test`, `make helper-census`, `make helper-check` | syn-based normalized body/shim/constant/table census, strict ledger validity and exact group enforcement; repository tooling only |
| Local release artifacts | `make package` | Five Cargo-verified archives in dependency order; source identity and registered notice audit |

These integration-test names are in `crates/core/tests` unless qualified with
another crate. `make check` runs the full formatting, hygiene, Clippy,
helper detector self-tests and ledger validity, verification-profile workspace
tests, and Rustdoc gate with warnings as errors.
Focused test results establish their listed contracts; they do not replace this
gate. Verification retains debug assertions and overflow checks.

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
[tool-calls/INTEGRATION.md](tool-calls/INTEGRATION.md); the 12,396-case
evaluation is a deterministic generated corpus, so its zero-incorrect result
establishes behavior on those cases rather than a production error rate.
Eleven dialects ship (`qwen3-coder-xml.v1`, `hermes.v1`,
`openai-tool-calls.v1`, `llama3-json.v1`, `mistral-v3.v1`,
`mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`, `kimi-k2.v1`,
`anthropic-tool-use.v1`, `harmony.v1`). Streaming recognition,
`purrassist-derive`, and the remaining dialects (GLM, Gemma, Pythonic, bare
JSON) are tracked in issue #4.

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
version recovery, retry, and undo. Real Windows execution is proved by the
`windows-engines` hosted lane; a cross-compile alone does not prove durability.
File transactions admit JSON escaping when bounding journal metadata and derive
parsing budgets from the admitted serialized size. Resource errors remain typed
errors in the Rust store; CLI/MCP handlers retain their explanations as tool
errors. Recovery deletes a temporary file only after validating its journal,
recorded target-parent path, and bounded replacement-byte prefix. Unknown or
unvalidated artifacts remain in place with an observable explanation.

`make wasm-test` executes and compares native/WASI core, syntax, and fixed helper
transcripts (the core transcript includes a tool-call recovery with a fuzzy
tool name and a stray closer across two dialects); it also executes the core transcript on wasm32-unknown-unknown
through Node's WebAssembly API. The unpublished harness supplies the host-free
entry point. Fixed accounting layout charges keep the core decisions and receipt
transcripts independent of native pointer width. These transcripts establish
the exercised operations rather than every conceivable platform workload.

## Quality, fuzzing, and performance limits

[MATCHING-QUALITY.md](MATCHING-QUALITY.md) records corpus counts, conservative
abstention, zero observed incorrect automatic answers, risk/coverage sweeps,
and the assumptions needed for its binomial intervals. Deterministic, related
cases are not independent production samples; the interval cannot be interpreted
as a production error guarantee. The independent distance oracle exhausts all
14,641 ordered pairs of three-symbol strings up to length four.

[FUZZING.md](FUZZING.md) describes the nine mutation targets, qualification
counts, retained coverage regions, independent-oracle scope, compilation
rejections/timeouts, and replay. Reports live in `docs/qualification/` and retain
the source/tool identities and deterministic seed. A bounded campaign establishes
that its executions passed, rather than absence of defects or a whole-crate
coverage percentage. Production fixes found by these campaigns have focused
regressions as well as the independent comparison.

[RESOURCES.md](RESOURCES.md) distinguishes accounting from measured allocations,
requested heap from RSS, whole-process time from per-operation latency, and
cache-disabled cold build cost from warm compilation. Reports contain raw
measured observations and executable limits. Hygiene validates the committed
report schema; hosted qualification regenerates measurements and applies limits
to that fresh report. Results are machine-specific observations.

## Helper census contract

The unpublished helper census parses repository Rust and groups normalized
function bodies, forwarding shims, integer constants and hex tables. These are
structural candidates for review; equality under normalization does not prove
semantic equivalence. `make helper-census` writes the actual inspection report.
`make helper-check` additionally rejects unsanctioned groups and forbidden
implementations against the exact helper ledger. Existing findings remain
visible without blanket sanctions.

`make check` requires detector positive/negative self-tests and strict ledger
checking: resolved symbols, documentation anchors, exact group membership, and
absence of cross-package source includes. The consolidation removed all six
body groups and six forwarding groups from the initial inspection. Only two
exact documented groups remain: distinct integer constructor input domains and
conversions into three distinct public error types. Shared resource conversion,
error construction, evidence serialization, Markdown source/publication logic,
percent decoding, word classification, and scratch storage now have one
implementation each. The portable core fixture belongs to the unpublished
harness Cargo library. See [HELPER-CENSUS.md](HELPER-CENSUS.md) and its actual
consolidation report for the source-bound census and review decisions.

## Qualification commands

```sh
make check
make syntax-oracles
make wasm-test
make fuzz
make resources
python3 scripts/check-resource-limits.py .cache/resource-report.json
make package
```

The `Verify` workflow additionally runs native Linux/macOS gates, stable MSRV
workspace tests, Windows engines/files/CLI, executed WASM comparisons, and Linux
qualification. Exact commit and run evidence is recorded in issue #1 and the PR;
a green run applies to its commit only. The trusted-publishing workflow is
separate from qualification and uses the registered publisher identity.

## Release acceptance after review and merge

The established issue #1 sequence reserves publication for the reviewed merge
commit. It requires the full gate and hosted CI on that commit, verified
trusted-publisher settings, explicit release-tag authorization, dependency-order
publication, all five downloadable archives, installed CLI/MCP transcripts from
crates.io, and the GitHub release. The verifier's offline local-registry tests
prove its detection path; successful local packages and installs cannot establish
that the real registry downloads work. Its default MCP transcript reads a seeded
file, applies a journaled replacement, conditionally undoes it, and requires
byte-exact restoration. Pipe writes and response reads have bounded timeouts,
with child cleanup tested for missing responses, partial output, and stderr
flooding. Until the real acceptance steps execute, 0.1.0 is not
claimed as published and issue #1 remains open.


## Stage 2 local integration record

The repaired engines passed the normal `make check`, `make syntax-oracles`,
`make wasm-test` and all-target `make fuzz` commands. Fresh source-bound
[smoke reports](qualification/fuzz-stage2-linux-x86_64.json),
[resource measurements](qualification/resources-stage2-linux-x86_64.json) and
[recovery measurements](qualification/recovery-bench-stage2-linux-x86_64.json)
record their actual observed identities; existing phase reports are unchanged.
The resource measurements passed the unchanged published limits. These commands
used the normal project framework without caller environment overrides or
alternate Cargo launchers. This local record is separate from registry publication
and downloaded-package acceptance in the approved release sequence.
