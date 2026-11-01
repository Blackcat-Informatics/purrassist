<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Accepted implementation contract

Internal grammar repair proposals retain only their saturated search cost.
Actual edit provenance stays in the parser history and is still published by
the existing resolver. A cost-only proposal refuses a parent that carries
provenance rather than discarding it; ordinary schema-search proposals retain
their complete ledgers. Successors within one replay share a lazily computed
identity digest; the cache stays local to one parent, operator and offset.
Existing work admission and wire counters are preserved. Repair eligibility
borrows the pending suffix instead of copying it. Current performance and differential
execution evidence for issue #11 remains pending automatic CI.

Compiled schema indexes now retain fallible owners for location/document arenas,
routes, resource and anchor lookup, dialect inheritance, optional-vocabulary
metadata, child tables and pattern tables. A private AVL arena preserves stable
IDs and logarithmic tree operations; growth, searches, comparisons and balancing
are admitted before mutation. Construction snapshots, escaped pointers and names
release on every exit. Validation, witness generation and recovery borrow the same
prepared metadata; dynamic scopes retain location IDs. Successful schema drop and
late compilation refusal release the index and prepared pattern owners. Strict
JSON documents now own their actual string, number and container reservations.
One existing parser transfers fallible buffers into an immutable document owner;
root, external, built-in and temporary meta-schema documents share this path.
Schema drop and parsing/compilation refusal release those reservations. Public
strict JSON parsing explicitly retains them when publishing an ordinary Node.
Syntax descriptors remain allocation-free until the final diagnostic boundary.
Cold schema compilation now retains only actual governed owners. The old
spelling-based compiler storage allowance is removed. Successful schema drop
and resource refusal release those owners completely; only final published
diagnostics retain their documented output admission. Nested pattern diagnostics
stay scratch-owned until the public pattern or schema error boundary.

Schema reference preparation and runtime resolution now share one governed URI,
fragment, pointer and dynamic-scope path. URI scanning and dot removal, decoded
text, pointer routes, comparisons and diagnostics admit work and fallible storage
through the caller's governor. Compiled anchors borrow location IDs; reference-only
targets retain temporary owners through linkage, and algebra dynamic edges retain
originating location IDs rather than duplicate reference strings. Validation,
positive and negated witness generation, and recovery preserve native resource
refusals. Compiler keyword lookups, duplicate sets, numeric constraints,
vocabulary loading and diagnostic construction now admit their work explicitly.
Temporary validation sets release their fallible owners; numeric sign and
integrality checks preserve arbitrary exponents without constructing arithmetic
intermediates. Strict schema parsing also admits scanning, decoding, copies and
buffer relocation; this does not complete #33 or #36.

Exact numeric divisibility now shares the admitted decimal normalization and
signed arithmetic authority used by numeric comparison. Normalized coefficients,
exponent differences, factor-removal quotients and remainders retain fallible
temporary owners; arithmetic character appends admit relocation before growth.
Success and refusal release all arithmetic reservations. Zero divisors preserve
their diagnostic text and retain only the completed diagnostic after its final
governor check. Arbitrarily large exponents remain exact without exponent-sized
integer allocation. Witness numeric type filtering and bound checks use the same
governed integer classification and comparison as validation, preserving seed
order and native refusal causes. Recovery candidate ownership remains a separate
contract.

Hostname and email format evaluation now admits Unicode mapping, normalization,
Punycode, contextual and Bidi checks through the caller governor. Actual temporary
string/vector owners replace per-label storage estimates and release on pass,
fail or refusal. Normalization shares one ordinary/admitted algorithm: an explicit
decomposition stack checks caller Depth, and stable counting preserves equal
combining-class order while skipping already ordered runs. Punycode owns encoding
and decoding buffers, admitting scans, growth and insertion movement. Ordinary
public normalization preserves its output; this does not govern unrelated text
matching or case-folding operations. Email admits delimiter and local-part scans
separately and reuses the governed IP validators. IPv6 group parsing is borrowed;
both IP validators admit documented scan bounds after their finite length guards
and check cancellation before returning. Their standalone usage counters change
as well. Pinned Unicode data, format language and annotation/assertion semantics
remain unchanged.

Remaining schema assertion scans now admit keyword and compiled-pattern lookups,
repeated schema routes, Unicode scalar counts and witness member-name searches.
URI/IRI, date/time/duration and UUID validation use documented bounds over their
existing borrowed parsers; pointer and template validation retain forward cursors.
Regex format validation drops owned syntax diagnostics without publishing them.
Every format outcome checks cancellation, including unknown annotation formats.
Exact results, witness seed order and dialect-driven assertion behavior remain
unchanged; usage reflects the additional admitted computation. General
recovery/search ownership and graph loading remain separate.
These changes do not complete the full contracts of parent issues #33 and #36.

Witness canonicalization, requirement merging and the existing branch driver
share the caller's governor. Immutable schema operands and pattern names remain
borrowed; synthesized canonical lists, requirement snapshots, child obligations,
cycle keys and paths use fallible owners that release on every exit. Keyword and
dependency scans, exact numeric comparisons, copies, lookup and relocation admit
work before processing. The evaluator and recovery use the same governed array
layout and dependency view. Canonical construction/copying and live branch
continuations enforce caller depth and a native ceiling of 128; refusal remains
a resource outcome, never an unsatisfiability proof.

Witness candidate trees now reuse the existing owned JSON document representation.
Literal copying preserves number spelling, member order and node/member source
spans. String and numeric seeds, integer-step temporaries, arrays, objects and
duplicate-item copies hold actual fallible buffer reservations through branch
selection and final schema revalidation. Literal copying checks caller Depth and
a native ceiling of 128, with the copied root at depth zero. Rejected candidates
and refused construction release their owners; public witness publication retains
only completed output. Derivative and backtracking pattern generation share an
internal owned result, while ordinary pattern generation publishes its output
through one adapter. Required-property recovery transfers a completed witness
owner directly into the candidate tree without early publication.

Schema recovery now retains owned JSON trees from strict parsing or extracted
seed copying through replacement construction, queue retention, rejected/stale
candidate discard and acceptance. Every production proposal uses the same owned
representation, including decoded strings, shape changes, renaming, removal,
relocation, invented required values and date/time repair text. Route replacement
copies only retained ancestors and siblings; removed subtrees are never cloned
or charged to the replacement owner. Copying preserves lexical numbers, member
order and source spans, while string decoding clears its inapplicable spans.
Attachment checks caller Depth and the native ceiling of 128 against the combined
tree depth. Only completed recovery output transfers its tree reservation into
ordinary public nodes, after staged publication and its final governor check.

Shared Search publication moves accepted states into the provenance forest's
complete permutation using an admitted inverse-index buffer. It checks the
permutation and cancellation before changing receipt fields; states need not be
cloneable. Original input, equally ranked answers and Guess/BestGuess semantics
remain unchanged. Ledger Change construction/cloning, queue and deduplication
index estimates, Search/Forest receipt publication ownership, recovery traversal
and name-cost scans, extraction storage and host serialization remain separate.
This does not complete recovery ownership or parent issues #33 and #36.

Evaluator recursion and transient path/key state now use fallible owners on the
same caller governor. One active stack retains schema-location/instance-path
pairs; admitted comparisons preserve cycle identity without duplicate path
copies. Shared pointer/formatting builders and compiled child lookups cover
combinators, object members, property names, arrays and unevaluated paths.
Array layout resolves child IDs through one governed path shared with recovery.
Active and dynamic-scope entries unwind on every result, including scope-entry
refusal. Integer tests and numeric/count comparisons use admitted operations. Temporary
violation messages and serialized type diagnostics hold fallible owners until
collection succeeds. Caller Depth and a distinct native ceiling of 256 bound evaluator recursion;
cycle diagnostics retain their existing InvalidInput text through governed
publication. Obsolete nominal dependency-key leases are removed now that the
shared dependency view owns its actual temporary keys.

Evaluator reports and annotations now retain fallible string/vector owners
through construction, merge and discarded branches. Internal witness and
recovery checks release their reports; public validation retains ordinary
output only after fallible preparation and a final governor check. Annotation
sorting, lookup, copies and moves admit Work before processing. Recovery commits
whole-instance diagnostics and moved format annotations with stable validation
indices, deduplicates individually owned patterns by destination, and reserves
final observation slots before commit. Refusal preserves committed observations;
finalization after exhaustion or cancellation requires no allocation. Search
results retain the existing evidence order and ambiguous-prefix replacement.
Provisional Search/Forest receipt publication retains its existing charges;
its ownership and host serialization remain separate contracts.

Final refutations are published only by the public governed witness boundary.
The existing driver and required-property recovery carry allocation-free static
reason descriptors. The publisher admits one evidence entry, copying work and
fallible reason/evidence buffers, retaining their storage only after every
fallible operation and the final governor check succeed. Late refusal releases
all temporary publication owners and preserves the native resource cause.
Public types, reason text and evidence fields remain unchanged; successful
ordinary output retains its admission even after the caller drops it. Recovery
replacements and host serialization remain separate contracts.

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

Schema bundle identity construction now streams through the existing SHA-256
builder without a framing allocation. Root, URI and resource hashes retain the
same dialect-prefixed identity bytes, including unused supplied resources in
sorted URI order. Hashing admits byte/control work at bounded checkpoints and
finalization work before processing; complete schema publication checks the
shared governor once more. The actual plain FnO consumer uses this compiler.
Tolerant parsing and the remaining witness construction, evaluation and
publication paths retain separate unfinished governance contracts under #33 and #36.

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

## Schema recovery complexity

Issue #128 guarantees polynomial recovery for certified independent mandatory
array wraps in recursive tree schemas. It does not require polynomial recovery
for arbitrary coupled JSON Schema constraints.

The guarantee applies to syntactically valid JSON, with guesses disabled, when
the repair chart proves a unique minimum-cost repaired value using mandatory
array wraps.
Every compiled schema location is either a static `$ref` without sibling
assertions or an explicitly typed `object`, `array` or `string`. Objects may use
`properties` and `required`; arrays may use homogeneous `items`. The additional
allowed keywords are `$schema`, `$id`, `$defs` and `definitions`. Recursive
references are supported. Required repairs must be independent: competing
unwraps/coercions, duplicate-key ambiguity, key renaming, field relocation and
decoded-string interactions prevent certification.

The chart shares states by schema location and original input occurrence,
solves minimum costs before selecting edits, and replays certified wraps through
the normal validator and receipt publisher. State and edge counts, relaxation
rounds and replay work are polynomial in the input and compiled schema sizes.
This covers both the reported deeply nested leaf repair and arbitrarily many
independent mandatory wraps; it is not a fixed nesting-depth shortcut. Caller
resource limits still apply and exhaustion remains explicit.

Outside that family, recovery retains exact governed search without the
polynomial guarantee. Qwen and Hermes identify tool-call grammars, not argument
schema complexity classes; neither name alone establishes this guarantee.
Accepted answers, rankings, ambiguity/guess semantics and transformation
provenance are preserved. Exploration diagnostics, usage counters and
finite-budget exhaustion points can change, as recorded in the changelog.

Executable coverage is in `schema::recovery::factoring_tests`: comparison with
joint search, duplicate-key ambiguity, depths of 12/24/48, independent repair
counts of 12/24/48, and explicit Work refusal retaining the original input.

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
  grammars in a first-party DSL; fifteen ship (`qwen3-coder-xml.v1`,
  `hermes.v1`, `openai-tool-calls.v1`, `llama3-json.v1`, `mistral-v3.v1`,
  `mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`, `kimi-k2.v1`,
  `anthropic-tool-use.v1`, `harmony.v1`, `gemma3-tool-code.v1`, `glm-4.5.v1`, `bare-json.v1`, `pythonic.v1`) and callers compile
  their own. A
  best-first search repairs call markup with named, costed rules; each call
  is resolved against its tool with a per-call outcome, itemized repairs,
  markup/data losses, ranked alternatives, and caller-chosen ids. Effects
  select conservative policies: a guess on a mutating tool is never resolved,
  usable, or emitted. The model's stop reason is an input, a length stop is always
  `finish_reason` `"length"`, and recovered calls print in any dialect or as
  the `OpenAI` `tool_calls` array. Recovery is deterministic, governed by a
  published grammar scan bound (nondecimal Python integers additionally need
  governed quadratic conversion), and exposed through Rust, CLI
  (`recover-tool-calls`), and MCP (`recover_tool_calls`). Typed tool
  definitions can be derived from Rust structs and enums (`purrassist-derive`).
  Still tracked: the `*-stripped` variants (#18),
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
The companion `assemble_accounted` operation now uses the same assembler for
independent byte/token admission. It records either an exact deterministic caller
counter with tokenizer/configuration identity, or a conservative byte-derived
bound under explicit caller encoding assumptions. Complete immutable bytes retain
all framing; unavailable accounting and native or semantic refusals preserve the
original input and any completed measurement, releasing unpublished storage.
The scalar-encoding example demonstrates these separate quantities without a
model-tokenizer qualification claim. Model acceptance, family placement/selection,
graph loading, routing and complete projection ledgers remain under #36 and #47.

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

The MCP envelope reader now checks each execution-profile fragment through
`codec::mcp::ExecutionProfile`. Its borrowed view retains original JSON and
locations, checks both declared modes and all required fields, and reads exact
positive `u64` resource quantities. Wire quantities are independent of the native
renderer limits. Encoding uses the existing governed JSON writer with fixed key
order and preserved scalar spellings; refusals expose the offending occurrence
and enclosing tool index. The `execution_profile_codec` cases cover round trips,
located invalid neighbors and output admission. Full RDF agreement, graph loading
and manifest integration remain under #33; host release remains under #37.

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
  validates exact worked schemas with the core engine (#70). G7's numeric
  prerequisite (#43) adopts exact decimals, finite IEEE carriers, registered
  representability losses and source-bearing typed refusals; graph loading and original-SHACL integration remain
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
incorrect correction, latency, allocation, memory and build cost.

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


## Validated single-result projection — implemented

`ContextManagement::project_result` accepts a caller-prepared output `Schema`,
an immutable original JSON `Node` and its governor. It validates the complete
original before masking, applies the existing four-form `FieldMask`, serializes
the complete projected stream through core's shared JSON cursor/escaping writer,
and calls the canonical `OutputByteLimit` once before publishing any artifact.

The explicit representation is compact JSON Lines: every emitted value has one
trailing LF, while zero values produce zero bytes. There is no outer array.
D6 fixes the four mask forms but did not fix a stream delimiter; this callable
path fixes LF framing explicitly. Quotes, escapes, UTF-8 and every LF count.
A path or field object emits one value. Missing/null named lookups propagate
null; other non-object traversal fails. Field objects retain mask key order.
`.[]` emits root-array members in order. `.name[]?` emits a selected array's
members, or zero values for a missing/null/non-array selection. Selected duplicate
names fail as ambiguous rather than choosing a first/last value or emitting null.
Whole selected values preserve nested duplicate occurrences and numeric spelling.

Success and refusal retain the original node, original mask, full validation
report when available, last attempted stage and explicit transformations for
null insertion, optional omission and completed mask application. These are
transformations rather than inferred source facts. Source DOM/schema/context
preparation is caller-owned. Native resource/cancellation refusal is distinct
from the exact-byte semantic ceiling. Oversize output is refused intact; no
partial or truncated stream is returned. Token evidence cannot override it.
New projection/trace/output buffers use admitted scratch owners; a refused
unpublished serialized stream releases its storage.

Core's narrow `JsonField`/`JsonLine` borrowed views and
`json::lines_to_json_scratch` reuse the same cursor and escaping authority as
ordinary node emission. They copy no input DOM and preserve existing node
serialization bytes and resource scheduling. The `project_result` example reads
a bounded stdin envelope containing `format`, `context`, `outputSchema` and
`result`, delegates version/legacy/mode validation to the existing context codec,
and writes only complete checked bytes to stdout. Refusals go to stderr. This
input envelope is not an admitted graph manifest.

History pruning, full RDF/SHACL/SRL/GTS bundle admission/roundtrip, graph-backed
registry/dispatch, authorization/confinement, graph/proof-bound receipts and all
remaining #33/#36/#37/#47/#39 acceptance remain required. This operation provides
no graph authority and adds no host/facade dependency on the unpublished crate.


## Caller-prepared AppArmor policy documents

`PolicyDocument::prepare` accepts an explicit `PolicyInput`: offline ABI 3.0 or
4.0-ip, a literal absolute HOME and caller-owned standalone profiles or parents
with flat hats. Bodies contain the existing typed file, Internet, Unix, signal,
ptrace, capability, mount and resource-limit declarations. Existing per-rule
literal bindings and Internet enumeration ceilings remain authoritative inputs.
Preparation owns only the fixed baseline globs/access sets and borrows the exact
original input. It grants no graph, session or execution authority.

`compile` validates names with `FileTarget`, rejects duplicate root names, local
hat names and caller rule identities within a body, and sorts those scopes by
original byte identities. Identical rules with different identities survive.
Every execution body receives the SSH/environment-file, dangerous-capability,
broad ptrace and four mount-operation denials through the existing rule
renderers. HOME uses existing literal escaping and unsafe-path refusals; root
and trailing slash inputs add no accidental separator. Compiler additions have explicit
baseline origins rather than fabricated caller provenance.

Bodies also accept ordered mandatory `PolicyImport` occurrences bound to opaque
`PolicyDependency` records on the input. Imports follow fixed baselines and precede
caller rules; repeated paths with distinct occurrence identities remain in order.
Paths must be normalized relative `abstractions/...` file names. Absolute paths,
traversal, empty components, variables, wildcards, delimiters, controls and
unsupported syntax fail explicitly. Direct imports require a declared binding;
duplicate dependency paths fail even for unreferenced or byte-identical records.
Errors retain the complete input and conflicting original records.

The output retains every declared dependency, including transitive records, in
input order with SHA-256 identities computed from its exact supplied bytes.
Import source spans identify the original occurrence and bound manifest index.
The compiler performs no filesystem lookup, dependency-content interpolation,
imported-permission analysis or transitive-closure proof. `local_mediation`
reports classes from local rules only. `complete_mediation` returns a typed
refusal identifying the first import span whenever opaque imports are emitted.
Neither accessor grants execution authority or attests running-kernel support.

The complete ABI/profile/hat document, dependency manifest and source spans
publish together after final cancellation admission. New scans, comparisons,
hashing, indices, text, manifests and spans use the actual governor and fallible
scratch owners for retained storage. Existing String-returning renderers retain
their documented conservative reservations; this compiler does not claim prompt
release of their detached intermediate charges. Original input and exact native
rule/resource errors remain available on refusal. ABI 3.0 refuses fine-grained
Internet rules instead of dropping scope. The selected ABI describes an offline target, not
running-kernel, loaded-policy or dispatch evidence.

The actual `policy_document` example prints a complete compiled document. Its
`--bound-imports` mode materializes first-party synthetic dependency files under
a fresh caller-selected include root. Native resolution and required transitive
files remain the external parser's responsibility. Full RDF/SRL/static admission,
permission/owner/deny proofs, graph import closure and trust, nested graph profile
synthesis, transition-target existence, graph CLI, loaded mediation and host
confinement remain #33/#34/#35/#39 requirements.

## Governed semantic JSON equivalence

`Node::equivalent_governed` compares caller-prepared immutable JSON values under
one supplied governor. It preserves mathematical decimal equality, independent
object key-group order, ordered duplicate occurrences within each key, and array
order. Source spans do not affect the result. Its iterative traversal admits
actual value depth with the root at zero. Key/string comparisons, mantissa
normalization and arbitrary-length signed decimal exponent arithmetic admit
work before processing. Temporary traversal and decimal buffers use fallible,
release-owned storage; resource refusal and cancellation never become inequality.
Ordinary equality and governed equality share the same structural and numeric
comparison authority, without constructing a default governor.

Schema const/enum/uniqueness evaluation, recovery no-op detection, witness
exclusion/merge checks and emitter XML/splice roundtrip checks use this operation
with their actual caller governor. Merge and conditional preparation propagate
native refusals without manufacturing a contradiction or an unsatisfiability
claim. Source-spelling syntax fallback remains distinct from native refusal.

This completes comparison governance. Original DOM construction, general schema
algebra storage, XML decoding and full emitter preparation retain their separate
ownership requirements. It supplies no graph admission, execution authority,
complete witness, GBNF, signature, family-projection or routing implementation;
remaining parent requirements stay open.


## Validated result projection through shipped surfaces

The pure `output` module now owns the existing field-mask parser, context codec,
exact output-byte checker and validated result pipeline. Toolgraph reexports the
same types. The published facade exposes `purrassist::output`, with no dependency
on the unpublished graph crate. Core keeps its zero-dependency and ambient-access
contracts; the six-crate release set is unchanged.

The read-only `project_result` registry operation accepts exact `result_json`,
`output_schema`, `format`, the complete `context`, and optional `effort`. The
existing CLI `call` and MCP `tools/call` routes use that handler. Accepted
processing decodes context, emits and compiles the schema under the governor,
strictly parses the original JSON, then validates the entire original before
applying its mask. It performs no recovery or stringified-JSON unwrapping.

Replies retain exact original JSON, requested context JSON and every requested
mask occurrence, the actual preparation/pipeline stage, available validation and
transformations, a typed failure or complete artifact, and a receipt. Unavailable
validation is null. Native resource stops remain native receipt terminations.
Generic invocation validation, cancellation before handler entry and malformed
protocol messages keep the existing transport policy.

Only successful replies contain `application/x-ndjson` artifact text: one LF per
compact JSON value, no outer array, and zero bytes for zero selected values.
The exact byte measurement applies to decoded UTF-8 artifact text. JSON string
escaping, receipt and control fields, the CLI envelope LF and MCP wrappers are
outside that measurement and retain the existing host materialization policy.
A valid processing-refusal envelope uses the ordinary CLI `call` success exit;
its `status`, typed `error` and receipt describe the unresolved operation.

This is caller-requested result processing. It grants no graph authority, tool
execution, authorization, confinement or proof-bound release. Loading, intent and
history metadata is decoded without implementing those policies. Full
#33/#34/#35/#36/#37/#39/#47 acceptance remains required.

## Exact prepared output contracts

Prepared tools now retain optional exact output-schema source and an independent
core validator. `ToolDefinition::output_schema` accepts caller-provided object
or boolean schema text, admitted during compilation. Explicit `true` and `false`
remain present contracts; absence remains `None`. Output schemas do not change
argument projection, defaults or input validation, and no output DOM is cloned.

MCP `outputSchema` requires an object with one root `type` equal to `object`.
Nested boolean schemas remain valid. Missing, duplicated or incompatible root
fields, malformed schemas, unsupported dialects and unresolved references fail
explicitly. Compilation and all new retention/scanning/diagnostic work use the
actual caller governor; native refusal never becomes an absent contract.

`CompiledTool::output_schema` borrows the validator and `output_schema_text`
borrows its exact source. `ToolDeclaration` exposes exact output text and its
original input range. The output fragment includes adjacent JSON whitespace
between the colon, value and next member/outer delimiter. This preserves explicit
builder whitespace and schema identity through MCP rendering and reimport.
Same-form MCP declarations remain byte-exact. MCP export carries eligible output
contracts verbatim; other targets, and non-object builder outputs exported to
MCP, return typed unsupported errors before emitting declaration bytes.

The facade `output_contract` example validates a caller-prepared result. The
existing host registry's declarations supply admitted output validators for
actual tool results. Admission alone does not enforce runtime output release,
authorize execution or provide graph/proof binding. CompactSignature, TypeScript,
full graph and remaining parent requirements remain separate.

## Exact prepared trusted-policy contracts

`PreparedPolicyContract::prepare` admits complete caller-owned SRL document
records with exact location, optional external base, explicit issuer, original
UTF-8 text and ordered direct references. Unique identities, root resolution,
all references and complete reachability are required. Every document receives
an exact-text SHA-256 and its own rooted ordered graph SHA-256. Versioned,
length-framed first-use and back-reference transcripts preserve repeated edges,
sharing and cycles without recursive hashes. Document-table order is immaterial;
location, base presence and bytes, issuer, text and ordered closure are bound.
The exact transcript is documented in `policy_trust.rs` and independently
cross-checked for a synthetic cyclic fixture.

`TrustedPolicyRegistry::prepare` retains original host grant records and a
canonical complete registry SHA-256. Independently supplied 32-byte approvals
bind document SHA-256, rooted contract SHA-256, one tool, opaque host vocabulary
identity, issuer and session-fact-schema SHA-256. Provenance labels also affect
the registry identity. Duplicate authority tuples are refused even when labels
differ. Registry order changes neither identity nor which original grants match.
Issuer names identify audited sources; strings alone do not authenticate them.

`bind` requires an exact grant for every reachable document under one selected
scope. It publishes an immutable matched handle only after final cancellation
admission, retaining original table bytes and bases, matched grants and registry
identity. Scans, comparisons, hashing, graph traversal, retained manifests and
match indices share the supplied governor and fallible release-owned storage.
Typed refusals retain original request and registry context, usage and exact
native resource causes. No partial handle accompanies a refusal.

The actual `trusted_policy` consumer loads independent synthetic host approvals,
uses matched originals and recorded bases with native `srl::parse().imports()`,
checks declared ordered edges and calls native `srl::check` at `Syntax` with an
explicit complete source table. It exercises no-import, shared/repeated and
cyclic tables plus missing, duplicate and unused native import refusals. Native
imports use their locations as external bases; the consumer explicitly refuses
recorded-base mismatches rather than changing admitted meaning. Native parsing
and checking retain their own allocation contract, outside this API's governor.

This completes prepared-contract matching only. It authenticates no host
configuration, verifies no native closure or protected-head property in the
library, evaluates no rules and grants no dispatch authority. Native governed
preparation/evaluation, graph admission, real dispatch and result release remain
required under #33/#34/#35/#36/#37/#39/#47.


## Runtime registered host output enforcement

The actual shared host registry dispatch now compiles both selected exact
schemas and validates arguments before invoking the handler once. It uses the
existing native governed schema engine directly, including array-root output
contracts. The whole returned node is validated before successful Rust, CLI or
MCP release; a declared unconstrained field is not strengthened implicitly.

Cancellable admission has its own default budget. Once a handler returns,
output validation uses a separate governor without a cancellation token,
preserving completed transaction outcomes despite late signals. A bounded,
allocation-free cursor traversal sizes its default Work allowance to the
standard base plus 256 units per returned node; all other limits stay unchanged.
The traversal admits cursor steps and depth under the original default limits,
and carries its consumed Work into validation. Sizing refusal retains the
original output and actual stopped governor. This structural allowance is a
finite scheduling policy for registered schemas, not a universal completion
guarantee; explicitly supplied governors keep their exact limits. Existing
handler payloads and receipts are unchanged and do not claim registry costs.

A post-handler schema violation or native validation stop returns the existing
String error transport containing a structured JSON diagnostic. It retains the
complete original returned node without duplicate-member collapse, selected tool
and compiled schema identity, native reason or full report, and separate admission
and output-validation receipts with their own source, budget, usage and termination.
The diagnostic states that the handler returned and effects may already have
occurred. It retains transaction/undo identities and performs no retry or rollback.
CLI releases no successful stdout result on refusal; MCP returns an error result
without successful structured content. Diagnostic materialization follows the
existing host policy, outside the bounded schema phases.

Three combined production-entry regression cases cover pre-effect admission,
complete result enforcement and native stops, and actual journaled mutation with
late cancellation and successful or refused output release. Existing complete
registry fixtures exercise real CLI/MCP object/array results and parity.

This completes registered host schema enforcement only. Graph-backed registry
metadata, authorization, confinement, admitted-graph recovery, ledger-driven
checks, manifests, bound receipts and proof-bound projection/release remain
required under #37 and their original tracking contracts. The existing
`project_result` operation still validates its supplied original before its own
field mask; outer registry validation checks that operation's reply envelope.


## Independent XML closer-text readings

The XML parameter reader retains both equal-cost guesses when text after an
unpaired closer reaches a declared parameter opener or the body end. The source
closer anchors the boundary; a value with no such closer keeps the existing
missing-close and non-length end-of-output rules. Paired markup and stray-only
closers retain their existing single reading, and a next-call opener is never
held after an unpaired closer. The outer grammar retains separate call attempts
and its existing next-opener completion rule.

Ambiguous values use one independently indexed choice plan at a time through
the existing reader. This returns every combination without eager Cartesian
materialization or an infallible copy of opaque reader state. The original
per-body work is repeated once per syntactic output; every pass and retained
reading uses the original governor. Exhaustion returns its original resource outcome
without publishing a truncated set of readings. Every completed combination
carries its own source spans, edits and guess flags through the existing
least-cost history packing. Alternative construction is fallible, and chain
consumption, comparison, cloning, formatting and destruction are iterative.

`CodecValue::alternative` stores `CodecAlternative`; dereference it to inspect
the reading, or consume it with `into_value()` to move its fields and remaining
alternatives without cloning. The link retains its original allocation lease
until its buffer is destroyed.
