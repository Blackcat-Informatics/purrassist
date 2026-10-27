<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Accepted implementation contract

The existing `Grammar::compile_governed` path now admits complete private DSL
syntax with governed scans/decoding/copies, fallible owned tokens and lists,
and exact syntax diagnostics/offsets. Unary syntax children use one-element
scratch vectors. Caller depth and explicit native parser-recursion and AST
ownership/drop ceilings of 128 include postfix chains. Syntax owners remain
release-owned through lowering failures; the grammar identity is retained only
after remaining fallible construction and final cancellation checks. The actual
`canonical_print` consumer exercises this compiler path.

First-party declaration, name resolution and retained expression ownership now
share that governor. Declared literals, closers, delimiters, marker pairs, names,
layouts and expression lists/children use fallible release-owned storage.
Public `ExprChild`/`ExprItems` provide read-only borrowed views; unused deep
`Clone` implementations were removed, while dialects continue sharing grammars
through `Arc`. The existing cycle check runs before layout traversal and caches
expanded production heights, including finished DAG tails. Caller depth and an
aggregate native ceiling of 128 refuse excessive reference expansion; cyclic
layouts return a typed recursive-production error.

Derived closer, confusable, JSON and stray spellings, sparse repair tries and
outer repair/delimiter tables now retain fallible scratch owners as well.
Comparisons, copies, deduplication, trie searches/insertion shifts and JSON key
classification admit work before processing. Trie construction borrows labelled
spellings; its flat storage preserves literals longer than 128 bytes. Temporary
JSON flags/renames and delimiter expansions release on every later refusal.

Recognizer Program instructions and all retained control tables now use fallible
scratch owners through the same ordinary/JSON-envelope compiler. Choice buffers,
closures, reverse edges, traversal stacks and insertion alternatives admit scans,
comparisons, sorting and growth before processing. Recursive emission preserves
caller/native depth admission; checked PCs and fixed-width bound sums refuse
resource overflow. Completed Program owners release on later compiler refusal
or when the compiled grammar drops.

Dense matcher construction/storage and derived call metadata retain governed
owners. Call alternatives, literal spellings and codec markup use fallible
byte lists; closer IDs, capture memo and traversal temporaries release after use.
Capture/next queries and longest-spelling scans admit each visited entry.
Borrowed `CallEnd` getters, `call_literals` and `CodecContext` expose the same
read-only owners. Recovery's existing runtime byte copies remain explicit.
Dialect-set metadata preparation also uses retained fallible owners and
admitted scans. Engine/recovery runtime allocation governance retains its separate contract.

Native pattern parsing now separately admits decoded characters, capture
names/indices/alternative paths, pending numeric references and lexical text
through fallible temporary scratch owners. Source/name/path scans, lookahead,
numeric conversion, escape/property-name scans and exact syntax diagnostics
admit work before processing. Capture-name validation borrows immutable Unicode
identifier ranges; missing generated fixed properties fail explicitly. Temporary
metadata releases on every exit; returned diagnostic strings retain admission.
The old uncharged scalar-count pass is removed. Native AST, Unicode and
classification construction now use actual admitted owners, as described below.
Derivative artifacts retain release-owned buffers and actual consumers borrow
prepared patterns.

Full cold backend governance remains #36
acceptance, alongside witness, GBNF, CompactSignature, projection, routing,
ledgers and graph integration.

Status: merged to `main` (PR #2, `699d9ff`); tool-call recovery (#3, #8) and
`purrassist-derive` (#15) merged since; published as `rust-v0.1.0`. Executable
qualification and its limits are recorded in `QUALIFICATION.md`; publication
followed the procedure in `RELEASE.md`.

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

Wire declaration output for `OpenAi`, `Mcp` and `Anthropic` now uses the shared
governed JSON writer through `CompiledTool::render_declaration_governed`.
Same-form bytes and canonical cross-form/schema bytes are preserved, with
fallible output storage and atomic typed refusals. The `wire-declarations`
example exercises the actual MCP adapter and all three targets. Source/tool
preparation remains separate; complete tool-graph family projection and routing
remain under #36.

`grammar::print::canonical_governed` prints call, result and declaration roots
over a caller-prepared `CompiledGrammar` with borrowed values and grouped calls.
Existing canonical emitters use the same walker after their request preparation.
Input admission, recursive scans, retained-automaton generation, temporary and
output storage, diagnostics and cancellation share the supplied governor. The
`canonical_print` consumer exercises complete output and typed refusals. Cold
grammar compilation, higher-level emitter preparation and full seeded/witness
value generation remain separate; #36 retains those integration requirements.

`FramedPrompt::assemble_bytes` now admits complete explicit prompt framing and
ordered caller-prepared declarations under one governor and an independent exact
byte ceiling. Success and typed refusals retain the original source/part
identities. Checked sizing, fallible output storage and final cancellation keep
publication atomic. The `framed_prompt_bytes` consumer prepares two real wire
declarations separately, assembles their tools-array bytes and validates the JSON.
This byte-only operation does not establish token admission, model acceptance,
family placement/selection, graph loading, routing or complete projection ledgers;
those requirements remain under #36 and #47.

The internal tool-graph crate also admits complete callable FnO bindings from
caller-prepared upstream catalogs. Its governed checker preserves original
records and declaration order; the `admit_callable` example exercises the actual
upstream reader and public API. This does not implement governed RDF/FnO source
admission, SHACL/G7 integration, graph loading or manifest conversion (#33).

Governed JSON emission shares the existing serializer's structural/escaping
authority through `Node::to_json_governed` and `json::quote_governed`. The
`emit_context_json` example exercises context-shaped output with explicit work,
depth and fallible storage admission. Full tool-graph manifest
validation and host integration remain outstanding (#47, #33).

The tool-graph `ContextManagement` codec now validates and emits the complete
MCP context fragment under an explicit enclosing v2 format. It uses the existing
mask/output-limit engines and shared JSON writer, with governed numeric retention,
exact integer reading and formatting. Its `roundtrip_context` example exercises
canonical bidirectional conversion. Source DOM construction is caller-owned; this
does not establish governed raw JSON parsing, full manifests, RDF loading or
host result release. Those integration requirements remain under #33 and #47.

The internal tool-graph crate also provides governed AppArmor glob syntax and
safe policy words through `AppArmorGlob`. Its flat arena retains exact source,
byte/range semantics, brace links and caller-declared variable identities without
expanding alternatives. Literal host binding refuses unsafe syntax explicitly.
The `apparmor_path_words` example exercises standalone rendered path rules;
permission analysis, full policy synthesis and confinement remain planned.

`GlobLanguage` now lowers these admitted semantic models and shared literal
bindings into the core's `ByteLanguage`, a direct postfix/full-byte driver over
the existing derivative arena/session. It proves exact language inclusion or
returns a shortest counterexample as bytes, including invalid UTF-8. Exhaustion
remains an explicit error. The `glob_containment` example exercises actual
lowering and inclusion. Native file-rule component restrictions, lexical brace
and escape context, expanded literal variables and raw pattern slash normalization
are preserved; pathname bytes are never normalized. Unescaped class wildcards and
escaped class comma are refused because native conversion changes their semantics.
Complete permission/owner/deny analysis, hat admission,
policy synthesis and host enforcement remain outstanding (#34/#35).

The standalone `InternetRule` path validates caller-prepared Internet declarations
and emits atomic governed fragments with explicit fine-grained ABI requirements.
CIDRs remain symbolic until checked endpoint/product ceilings admit complete
numeric enumeration. Hostnames, family/access/port mismatches and excessive
expansion are typed refusals. The `internet_rules` consumer emits an IPv4 CIDR
and IPv6 fixture with the required ABI; offline parser/DFA qualification is no
host-enforcement evidence. Admitted-graph synthesis, original v3/v4 targets,
Unix-rule synthesis, complete permission analysis and host confinement remain
required under #34/#35.

The standalone `UnixRule` path now emits governed Unix socket fragments from
caller-prepared declarations. Abstract, anonymous and unconstrained scopes remain
explicit; peer-access restrictions and exact safe bindings are validated. It
shares the existing AppArmor policy-word authority and common native socket
access implementation, with Internet API/error/governance behavior preserved.
The `unix_rules` consumer assembles an offline fixture with the returned ABI
requirement. Fine-grained `network/af_unix` is required; no coarse network
fallback, file-language matcher or host authority is implied. Complete admitted-
graph synthesis, original v3/v4 integration, static permission/owner/deny
analysis, other IPC families and host confinement remain under #34/#35.

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
- `purrassist-toolgraph`: internal, unpublished pure vocabulary/loss-profile constructors
  and a complete caller-owned MCP context fragment codec, including field-mask
  parsing and exact output byte-limit enforcement, plus governed AppArmor glob
  words. Graph loading, full manifests,
  production schema bridge, authorization and execution remain planned. The direct
  `plain_tool_schemas` consumer adopts upstream plain FnO schema compilation and
  validates exact worked schemas with the core engine (#70). G7's plain-number support
  remains open under #43; graph loading and original-SHACL integration remain
  #33. Exact upstream Git dependencies
  keep it outside the publishable facade until registry packaging is qualified.

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
   evidence is recorded in `QUALIFICATION.md`; 0.1.0 is published, with the
   downloaded packages and installed CLI/MCP verified (see `RELEASE.md`).

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

Standalone `FileRule` renders the documented native permission/execute subset
over caller-prepared `AppArmorGlob` values and exact literal bindings. It
preserves audit/deny/owner conditions and original source identities, records
denied `ix` becoming native `x`, and refuses ambiguous targets or unsupported
unconfined requests. Its `file_rules` consumer exercises actual binding and
shared governed formatting. The selected `3.0` file-mediation requirement is
an offline target contract, without a running-kernel or loaded-policy claim.
Full permission/hat analysis, authenticated waivers, graph admission, policy
synthesis and host confinement remain under #34 and #35.

Standalone `SignalRule` and `PtraceRule` now render explicit closed access/signal
sets and mandatory native peer labels through the existing governed policy-word
writer. Signal membership retains its original field text alongside canonical
ordering and RT spelling. Shared label output applies no filename normalization
or Unix-address interpretation. The `ipc_rules` consumer exercises both APIs and
literal bindings. ABI `3.0` signal/ptrace metadata is an offline target contract;
it establishes no peer permission, authenticated admission or loaded policy.
Full #34/#35 graph, conflict, synthesis, trust and confinement acceptance remains
open; import handling remains a separate original requirement.

Standalone `CapabilityRule` and `MountRule` now render governed named capability
and unscoped mount-denial fragments. The explicit ABI `3.0` vocabulary admits
forty names, refuses all fourteen normative dangerous grants, and refuses
`checkpoint_restore` because native compatibility lowering would broaden it to
`sys_admin`. The complete governed dangerous-capability baseline grants nothing.
Mount requests admit only explicit denials for the four graph operations. The
`privilege_rules` consumer uses these actual public operations. Full graph-wide
baseline composition, static admission, v3/v4 synthesis, trust and host
confinement remain #34/#35; import handling remains required.

Standalone `ResourceLimits` now emits the five mandatory positive finite profile
limits through the existing governed formatter, preserving original numeric
values and source identity in complete artifacts and typed refusals. Selected
AppArmor 4.1.7 ABI `3.0` representations use exact decimal bytes or exact `K`
quotients, bounded counts and explicit whole CPU seconds. Unsupported values,
including native sentinel/saturation/wrap cases, are refused before output.
The `resource_limits` example is the actual five-field consumer. Offline rlimit
metadata and native serialization checks do not establish actual host support,
loaded policy or enforcement. Full admitted-graph synthesis, static permission/
hat analysis, trust and original v3/v4 integration remain under #34/#35.


## Release-owned derivative artifacts and borrowed consumers

Native Unicode and direct-byte compilation retain term/index scratch owners,
class `Buffer` owners and cut storage through the compiled artifact lifetime.
The old permanent-retention and class-table transfer paths are removed. Native
translation uses the existing metered arena for lookup/shift work and admits
traversal, interval comparisons/copies, canonical unions, boundary lookups, cut
sorting/deduplication and final cancellation before publication. Translation
root depth is one; each AST child adds one. Caller depth and a native recursive
translation ceiling of 128 both apply and refuse with sticky typed exhaustion.
Term IDs and finite repetition counts use checked encoding; an unrepresentable
finite `max + 1` returns sticky work exhaustion without changing its language.
Parser construction now admits actual AST/Unicode owners and accumulates classification, as described below.

`Pattern` owns its actual AST/interval scratch storage. Dropping syntax,
including `into_automaton`, releases those owners; a detached automaton keeps
its original derivative owners until drop. Runtime sessions retain their
existing admitted mutable copies and contracts.

Infallible `Clone` is removed from patterns/automata, schemas, compiled tools,
tool sets and their pattern-bearing shape owners. Schema child preparation
snapshots only vocabularies, document, route, pointer and base, without copying
unused children/pattern tables. Generation and checking borrow existing patterns;
`GenerateOptions::avoid` takes borrowed pattern references through admitted
reference storage. Host `Pending` borrows the caller's schema through completion.
Actual callers, examples and tests retain their schemas and exclusions in scope;
request/provenance and language semantics are preserved.

These construction phases do not establish runtime allocation governance. Lazy built-ins retain their separate governor. Full #36
witness, GBNF, signature, projection, routing, ledger and graph integration remain
required.


## Native pattern AST and Unicode construction

Native pattern construction retains actual AST lists, one-element unary
children, resolved backreference indices and canonical interval sets in fallible
scratch owners. Unicode aliases/table scans, copies, concrete interval heapsort,
coalescing, complement and scalar clipping admit work and storage before
processing. Generated Unicode tables and the separate IDNA runtime lookup are
unchanged. Unsupported properties remain distinct from resource refusal.

Temporary parser results accumulate height and backtracking classification;
each parent is admitted before construction. Leaves have height one, parents add
one to their tallest child, and caller depth plus a native AST/drop ceiling of
128 apply. Source-parser nesting is independently capped at 128, including
collapsed noncapturing groups. Flat widths remain shallow. The AST estimate,
lease, recursive classification pass and infallible AST sharing are removed.
Actual AST/range owners release on refusal, pattern drop and detachment; the
detached automaton retains its own derivative buffers. Exact syntax diagnostics
and governed metadata keep their existing contracts.

Runtime/backtracking allocation governance, lazy built-ins under their separate
governor and full #36 witness, GBNF, signature, projection, routing, ledger and
graph integration remain separate requirements.
