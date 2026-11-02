<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Resource measurements

## Private grammar DSL syntax admission

`Grammar::compile_governed` uses one private lexer/parser with the current
governor. Original input is admitted first. A checked 64 work units per source
byte reserve covers trivia/identifier/literal/class scans, UTF-8 and escape
decoding, copies and fixed keyword comparisons. Node processing and list-height
scans charge separately. Diagnostic formatting pre-admits bounded computation
and uses the shared governed sink for actual bytes and fallible storage.

Tokens move their owned payloads into fallible `ScratchString`/`ScratchVec`
syntax storage. The positioned AST has no private `Box` allocation: a checked
one-element scratch vector owns each unary child. Cached heights admit each
parent before construction. Parser recursion and ownership/drop height each
have native ceilings of 128 as well as caller depth, including repeated postfix
operators. Refused inputs never construct a deeper tree. Partial syntax and
temporary diagnostics release exact owned reservations; escaping syntax errors
retain their admitted messages and exact offsets.

Syntax owners remain live throughout lowering and drop on its failures. Only
the grammar ID is published after remaining fallible construction and a final
cancellation check. The actual compiler consumer and existing syntax/grammar
cases exercise this path. Later backend table
construction has its own admitted phases below; native pattern construction
also admits actual AST/Unicode owners. Runtime and graph integration remain
separate contracts.
Parent #36 retains those requirements and the remaining generation/integration
work.

## Native pattern parser metadata admission

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

`make resources` builds an assertion-backed resource benchmark and the release
CLI, then profiles nine fixed workloads and measures core and syntax latency,
allocation, RSS and an uncached release build. It requires Linux,
Python 3, Cargo, rustup and the Valgrind version registered in
`vendor/tooling.json`. The benchmark counting allocators are confined to bench
binaries. Syntax's production allocator accounting enforces its resource
governor; the core library retains its unsafe-code prohibition and zero runtime
dependencies.

Each workload runs separately under the native runtime, Memcheck, and Massif.
The report records:

- Allocation/free call counts and total allocated bytes from Memcheck.
- Maximum observed requested heap bytes and heap plus estimated allocator
  overhead from Massif, with zero configured peak inaccuracy and retained raw
  snapshots. Heap measurements exclude stacks and mappings.
- Native process peak RSS from its own Linux `/proc/self/status` `VmHWM`,
  converted from KiB to bytes. This avoids including the Python launcher's
  pre-exec memory. RSS includes mappings, runtime overhead, and the small procfs
  read used to obtain it; it is not live heap. The probe runs only in the native
  measurement, so it does not inflate profiler allocation counts.
- Native process elapsed time. Startup, construction, assertions,
  output, and teardown are included; this is not per-operation latency.
- Executable SHA-256 identities, compiler/runtime identities,
  source commit/dirty state/tree digest, build elapsed time, and Cargo's fresh
  versus rebuilt artifact counts.

`source.engine_source_tree_sha256` binds the measured engines and compiled-in
JSON data, benchmarks, examples, manifests, lockfile, toolchain and vendor
configuration, the Makefile and resource runner/checker/test scripts before
any build starts. The runner compares this digest again
after measurement and refuses to write reports if the inputs differ.
Documentation and generated reports are excluded from this digest, so writing
evidence does not invalidate its identity. `nonignored_file_tree_sha256` is an
end-of-run snapshot before writing the final report, rather than a digest of the
subsequent tree containing that report itself.

The empty workload records process overhead. It is not subtracted from peaks:
peaks from separate processes need not coincide. These fixed cases do not prove
worst-case allocator bounds, broad quality, or representative production costs.
Governor reservations and profiler observations measure different things.

The benchmark retains line debug information and symbols for actionable profiler
reports. The CLI uses the ordinary release profile. Builds inherit configured
Cargo settings and environment, and do not clean caches. Build elapsed times are
observations of those invocations, not clean-build speed claims. Compare results
only with matching compiler flags, toolchains, runtime, and workload conditions.

Outputs default to `.cache/resource-report.json`; a unique ignored directory
retains raw build, Memcheck, and Massif logs. `--output PATH` changes the report
location. Workload assertion failures, profiler failures, memory-error reports,
missing summaries, and malformed snapshots fail the run. Reports are written only
after every workload passes. `scripts/test-resource-report.py` checks the parsers
and fail-closed behavior and runs before profiling through the Make target.

## Grammar declarations and retained expressions

`Grammar::compile_governed` also governs first-party declaration and namespace
storage, duplicate/name comparisons, capture/codec/reference binding, parameter
pair checks and source interning. Source identity hashing admits its byte scan
before computation. Original declared byte buffers, class source,
marker pairs, production names, layouts and expression storage remain owned by
`ScratchString`/`ScratchVec` through all later failures and their actual compiled
artifact lifetime. Temporary namespace, offset and graph-admission storage drops
after use. Escaping diagnostics retain only their completed payloads.

`ExprChild` enforces one child without an infallible `Box`; `ExprItems` owns a
read-only list. Deref provides borrowed expression/slice views and equality
compares contents, independent of budget identity. Unused deep `Clone` derives
on expressions, productions and compiled grammars were removed; dialect `Arc`
sharing remains. Actual readers borrow source bytes and existing recovery-owned
copies stay explicit.

The existing cycle admission runs before reference-following layouts. Its
postorder caches expanded production heights, so already-finished shared DAG
tails count toward both caller depth and the aggregate native ceiling of 128.
A cyclic layout is a typed recursive-production refusal, rather than an
unbounded traversal. Structural scans, byte comparisons, copies, diagnostics and
fallible storage admit work before processing; resource causes remain sticky.

Local class source/anchoring/diagnostics use governed buffers around the existing
native pattern authority.

Derived closer/confusable/JSON/stray spellings and their sparse repair tries now
use retained `ScratchVec` owners, including the outer repair/delimiter tables.
Spelling comparisons, copying, deduplication, trie binary searches and sorted
edge insertion shifts admit work before processing. Trie construction borrows
labelled bytes instead of making a labelled deep copy. Its iterative flat
storage admits one caller depth frame; byte-prefix length has no recursive
128-byte ceiling. Temporary JSON key-use flags/counts/copies/renames and expanded
delimiter spellings remain governed until use completes and release on refusal.

Recognizer Program construction now retains fallible owners for instructions,
entry openers, committed flags, grouped-call opener/closer locations and nested
insertion alternatives. Emission choices, closures, reverse edges, reachability
flags and traversal stacks remain release-owned through construction and later
compiler failures. Work admission precedes scans, initialization, comparison,
sorting, renames and growth. Recursive emission preserves caller depth and the
existing native aggregate ceiling; iterative queue length adds no recursion.
Program counters and fixed-width repair-byte/configuration-bound sums are checked;
refusal keeps the original sticky resource cause. Ordinary and JSON-envelope
recompilation share the same path and instruction identities. Shared backward
reachability also admits its actual call-metadata callers.

Dense matcher construction/storage and derived call metadata retain governed
owners. Call alternatives, literal spellings and codec markup use fallible
byte lists; closer IDs, capture memo and traversal temporaries release after use.
Capture/next queries and longest-spelling scans admit each visited entry.
Borrowed `CallEnd` getters, `call_literals` and `CodecContext` expose the same
read-only owners. Recovery's existing runtime byte copies remain explicit.
Dialect-set metadata preparation also uses retained fallible owners and
admitted scans. Engine/recovery runtime allocation governance retains its separate contract.
Delimiter expansion feeds the same governed dense matcher. Complete cold
compiler governance and the remaining #36 capabilities are still required.


## Derived call metadata admission

`CallEnd` opener/closer/call-closer lists, `call_literals` and `call_markup`
retain fallible nested scratch owners until the compiled grammar drops.
Per-opener traversal flags/stacks and temporary literal/closer IDs release after
use. Scans, byte copies, comparisons, deduplication, in-place sorting and growth
admit work/storage before processing. The fixed two-slot successor iterator
serves both compiler and engine callers without allocating a successor list.
Capture memo initialization, capture/next traversal and longest-spelling scans
admit work through the same compiler, including empty spelling groups. Recursive
queries preserve caller/native depth limits and sticky resource causes.

Read-only `CallEnd` getters and shared `ScratchVec<u8>` slices replace deep
metadata cloning. The engine, emitter and XML codec borrow the retained markup;
recovery's existing `Markup` and closer collections keep explicit ordinary byte
copies. Those runtime allocations remain outside this compiler admission phase.
Canonical-first openers, alternative/closer encounter order, longest-first literal
ordering and lexical ties, markup deduplication and chained-call detection retain
their original semantics. JSON repair spellings are not added to call metadata.
Refusals drop unpublished partial tables and completed metadata from earlier
steps; a successful borrow does not allocate or readmit the prepared bytes.

## Schema bundle identity construction

`Schema::compile_governed` and `with_resources_governed` stream their complete
bundle identity through the existing `DigestBuilder`. The identity protocol is
unchanged: versioned dialect prefix, exact root digest, then fixed-width digests
of each supplied URI and document in sorted URI order. Unused resources remain
part of the identity; caller insertion order does not affect it.

Every hash update admits its byte length and one control work unit before
processing each chunk of at most 4,096 bytes. Finalization admits 128 work units
for at most two SHA-256 padding blocks. Native exhaustion/cancellation remains
`CompilationError::Resource`; a final governor check precedes publication.
Original unused-document `InputBytes` admission remains before parsing. Root
hashing occurs once, after the root's parsing/index walk; its private intermediate
identity cannot escape before linkage and final complete bundle hashing succeed.

The identity path allocates no framing buffer and adds no scratch storage. Its
obsolete nominal buffer charge and upfront hash-work estimates are removed;
actual hashing work is admitted where it occurs. Other compiler allocations,
tolerant source parsing and witness candidate/evaluation/publication storage
remain separate governance work. This does not claim complete schema compilation or
tool-graph loading governance. The real plain FnO consumer exercises this core
compiler path without replacing the upstream schema compiler.

## Witness preparation and branch ownership

The existing canonicalization, requirement merge and generation driver borrow
immutable schema values, exact numbers and names. Synthesized canonical children,
facets, requirement lists and snapshots retain fallible scratch owners. Cycle
keys retain owned instance paths; active keys and dynamic scopes unwind on every
result, including copy or scope-admission refusal. Preparation owners release on
witness success, contradiction, exhausted search and resource refusal.

Each keyword/member scan, dependency-key construction, compiled child/pattern
lookup, exact numeric comparison, snapshot copy and buffer relocation admits
work before processing. Ordered lists preserve branch and dependency order.
The shared array layout and dependency view propagate native refusal to actual
witness, evaluator and recovery callers. Numeric comparison delegates to the
existing arbitrary-decimal implementation without narrowing operands.

Canonical construction and copying check caller `Depth` and a native ceiling
of 128. The driver also admits live recursive continuations: sibling scoped
branches can accumulate native frames independently of canonical tree height.
Exceeding either limit returns `BudgetExceeded(Depth)`; it is not a refutation.
Linear facets advance iteratively. Finite Work and ScratchBytes refusal points
change because actual preparation operations and owners are now admitted.

Witness candidates retain owned JSON trees through selection and final schema
revalidation. Literal copies preserve number spelling, member order and source
spans. Numeric/string buffers and array/object composition release on rejection
or refusal; public witness publication retains completed output. Both pattern
engines transfer owned strings to schema generation. Required-property recovery
transfers the completed witness into its existing retained-candidate contract
without cloning. General assertion/format computation, later recovery retention
and host serialization remain separate. Public output retains its admission,
so live scratch need not return to zero after publication. This does not complete
witness computation, cold-backend or graph-loading governance.

## Evaluator control-state ownership

The existing evaluator retains one fallible active-key stack, preserving cycle
identity as compiled schema location plus exact instance path. Each visited
entry admits its ID comparison and, when needed, path comparison; copying and
stack relocation are admitted before processing. A non-progressing cycle keeps
its InvalidInput diagnostic text, now built through governed output formatting.
Integer tests and numeric/count comparisons also use admitted operations. Count
text is allocated only for present bounds, through a temporary owner; diagnostic
messages and serialized expected types retain temporary owners until collection.
Active keys and dynamic scopes unwind unconditionally after recursive evaluation
or scope-entry refusal. Caller `Depth` bounds live evaluator recursion, with
the root at zero. Reference and applicator frames count too: a recursive
object/array schema can require several frames per data level. The ordinary
caller preset remains 128. Raising it requires enough native stack for recursive
evaluation; core does not provision stack or impose a separate fixed ceiling.
Private depth counters use a wider integer than the public limit so the first
frame beyond even the maximum caller limit refuses without arithmetic overflow.

Registered host output validation has a separate automatic Work policy: the
standard base plus 256 units per returned node. A fixed cursor stack counts
structure under the standard Work and Depth limits, without serialization or
heap allocation, and carries consumed sizing work into validation. Other
limits and explicit output governors remain unchanged. Sizing and validation
refusals preserve the complete returned node and completed-effect context. This
is a finite default allowance, not a bound for arbitrary schema complexity.

Transient combinator keys, escaped member paths, property-name paths and string
instances, array indices, pattern-property keys and unevaluated paths use actual
fallible owners. Pointer spelling comes from the shared pointer builder; decimal
formatting and compiled-index searches use their existing admitted authorities.
Route-dispatch metadata scans admit work too. The shared array-layout view now
resolves child IDs for both evaluator and recovery, borrowing fixed rest keywords
and owning only synthesized positional keys. No alternate evaluator or pointer
encoder is introduced for this control path.

The old nominal dependentSchemas key lease is removed: dependency preparation
already owns its actual escaped keys. Work and scratch counters therefore change,
including finite-budget refusal points. Remaining assertion computation,
candidate construction and host serialization retain separate contracts. Tests
of control-owner release distinguish retained outputs; no total-zero claim is
made for calls that publish diagnostics or reports.

## Evaluator report and recovery observation ownership

Violation strings, format diagnostics, annotation collections and exercised
pattern records retain fallible owners through branch merging and rejection.
Internal witness and recovery checks release discarded records. Public validation
prepares all output collections and checks the governor before retaining their
storage. Branch and annotation scans, sorting, lookup and moves admit Work.

Recovery converts whole-instance diagnostics while the report remains temporary,
then commits that validation and moves format annotations without cloning their
payloads. Validation indices retain their established order. Individually owned
pattern observations deduplicate by destination across all attempted candidates,
including rejected candidates. Each observation reserves its final plain evidence
slot before commit. Stopped finalization moves committed records into those slots
without allocation or further fallible admission.

Search publication is staged until its evidence can join the observations. Unique
results append transformation evidence after the initial observation prefix;
ambiguous results replace that prefix as before. Patterns remain the sorted suffix.
Late refusal preserves committed observations and the native stop cause. Existing
provisional Search/Forest retained charges are unchanged. General scalar/format
computation, recovery replacement construction and host serialization remain
outside this boundary.

## Final refutation publication

Internal witness contradictions carry static reason descriptors. Discarded
branches and required-property recovery do not materialize public refutation
strings or evidence. The existing internal witness entry still revalidates every
candidate with the real evaluator before recovery can consume it.

Only `Schema::generate_witness_governed` constructs the final public `Refutation`.
It admits one `EvidenceEntries` unit, string-copy work and fallible storage for
the reason, rule, explanation and evidence vector. String reservations remain
leased through vector construction and the final governor check. No reservation
is retained until every fallible operation has completed; late Work or scratch
refusal releases all temporary publication owners and returns the exact native
cause. Complete public output retains its storage admission, including after the
ordinary strings and vector are dropped by the caller.

Reason text, rule identity, explanation, empty destination and absent span are
unchanged. Witness candidate ownership is described above. Recovery replacement
storage and host receipt/wire serialization remain separate contracts. This boundary does not complete witness or tool-graph governance.

## Type-enforced scratch accounting

`crates/core/src/contract.rs` charges scratch through a shared, atomic
`ScratchMeter` rather than a plain counter on `Governor`. `ScratchVec<T>`,
`ScratchString`, and `ScratchMap<K, V>` charge the meter (via
`try_reserve_exact`) before growing their backing storage, and release
exactly what they charged when dropped; `into_vec`/`into_string`/`into_pairs`
skip the release, permanently retaining the charge for content the caller now
owns outside scratch tracking. `ScratchLease` does the same for one opaque
byte reservation, and `ScratchLease::retain` keeps it permanently instead of
releasing it. Exhaustion from any of these is sticky on the governor that
issued them, even though they charge the meter directly rather than going
through `Governor::charge`: `Governor::check` also consults the meter's own
sticky-exhausted flag. `Governor` is `Clone`, but a clone gets its own
independent scratch-meter snapshot; only a lease or collection issued *from
one governor instance* shares that governor's live meter, so it keeps
charging the governor's budget after the call that created it returns.

Schema reference resolution uses release-owned URI, decoded fragment/token and
pointer-route buffers. Already compiled anchors and resource roots return location
IDs without copying their route, pointer or base. Reference-only linkage borrows the
resolved URI through document loading and target lookup, then holds any needed
pointer/base owners through compilation. URI and pointer scans, UTF-8 validation,
member searches, dynamic overrides and diagnostic construction share the caller's
governor. Validation and both witness signs use the same path as recovery.

Compiled schema indexes use a private AVL tree whose nodes are stored in a
fallible scratch arena. Stable numeric IDs survive arena growth and rotations.
Each actual search step admits its query comparison; insertion admits the bounded
ancestor/rotation work and any arena relocation before reserving and linking.
Balancing then cannot allocate or refuse. Ordered metadata traversal admits each
bounded tree step without a separate iterator allocation. Tree construction remains
amortized logarithmic per insertion; it does not shift a sorted array on every insert.
Caller-supplied resource maps retain the conservative standard-library lookup bound.

Location routes, pointers, bases, dynamic anchors, child/pattern names, inherited
dialects and optional-vocabulary metadata retain their actual owners with the schema.
Temporary snapshots and child spellings release after construction. Dropping the
schema or refusing after index preparation releases all these owners, including
prepared patterns. Strict JSON parsing uses fallible scratch strings and vectors
for decoded strings, exact number spellings, array slots and object members.
Growth admits relocation before allocation. Completed buffers transfer their
actual reservations into one immutable document owner without releasing or
charging again. Root and referenced documents retain these reservations until
schema drop; temporary meta-schemas and partial failures release their buffers.
Public strict parsing uses the same parser and explicitly retains reservations
when publishing an ordinary Node. Syntax failures carry allocation-free descriptors
until the final JSON or schema diagnostic is published. Manually constructed,
cloned or mutated Nodes and tolerant parsing remain outside this ownership path.
Keyword and vocabulary validation admit actual lookup, duplicate index, numeric
scan and diagnostic work. Cold compilation no longer charges a spelling-based
permanent storage estimate. Successful schema drop and resource refusal release
the covered owners completely. Pattern syntax diagnostics stay scratch-owned
while schema compilation adds its context, so refusal during outer formatting
releases the intermediate message as well. Only final published JSON, pattern
or schema diagnostic buffers retain their documented output admission. Reference decoding reserves known buffer bounds,
and string append/formatting charges existing-byte relocation when capacity grows.
The public owned unsupported-vocabulary output remains outside governed construction;
recovery borrows and traverses the retained metadata under its caller's governor.

Recovery's per-candidate cloning (`schema/recovery.rs`, `schema/recovery/
proposals.rs`), the evaluator's retained violation strings
(`schema/evaluate.rs`), regex compilation (`pattern::Pattern::compile_governed`,
which `Pattern::compile` now delegates to with a default governor), repeated
`Pattern::is_match` searches (including the backtracking search's own
per-iteration state clones in `repeat`), the lexical-Jaccard token sets
`matching.rs` computes for both strings at `Standard` effort and above, the
output JSON syntax recovery builds (`json/incremental.rs`: strings, arrays,
members and edits charged by logical length as they grow, with no up-front
per-input-byte allowance), the lease/release sites in `matching.rs` and
`edits/fuzzy.rs`, Markdown's rendered-HTML output buffer
(`markdown/render.rs`, via `ScratchString::reserve`/`into_string`), and helper
request preparation (`crates/host/src/helpers.rs::Pending::prepare`, which now
takes the caller's own governor instead of building an unshared one
internally) charge scratch before allocating rather than after, or not at
all. Permanently published output (a receipt's retained source, a validation's diagnostic strings, an `EditPlan`'s
replacement text) is still charged with a direct, never-released
`Governor::charge` (or a `ScratchString`/`ScratchLease` whose charge is
explicitly retained past the call, e.g. `ScratchString::into_string`),
matching `Receipt::retain_source`: the new collection types release when the
memory they charged for is genuinely transient scratch this crate's own call
stack controls, not memory a caller may go on holding indefinitely.

Schema recovery runs on the generic best-first engine in `search.rs`. Each
queued candidate charges `Work` for its deduplication key, one `Candidates`
unit, and `key_len * 32 + 256` scratch bytes. (The grammar engine's repair of
a stuck tool-call dialect attempt uses `Search::uncounted`, whose queue
charges no `Candidates`: there, only readings that survive to compete for
selection are counted; see `purrassist_core::grammar::cost`.) Popping the candidate releases
the queue-entry share; the retained deduplication key's share (`key_len + 64`,
once per distinct key) and the reused, `ScratchVec`-charged key buffer are
released when the search is dropped, as are entries still queued when the
first accepted cost is exhausted. A queued entry made stale by a cheaper
duplicate is skipped when popped, without validation or expansion. The search
order is the total key `(cost, tie, serial)`, where the state-supplied tie key
makes equal-cost order independent of proposal emission order; schema recovery
uses the unit tie key, so its order remains `(cost, serial)`.
`Search::seed_with` admits a seed by its key and precomputed cost before its
state and ledger are built: a duplicate at no lower cost charges only its
key's `Work`, and its ledger (evidence, loss copies, guesses) is never built
or charged.

## Incremental JSON recovery and linear extraction

`json::incremental::IncrementalRecovery` is the only tolerant JSON parser;
`json::recover_syntax` is its single-chunk wrapper. It is push-based, keeps
open containers on an explicit stack, and suspends anywhere: inside strings,
escapes, `\u` surrogate pairs, bare tokens, comments, the two-character
`//`, `/*` and `*/` delimiters, and multi-byte UTF-8 sequences. Lookahead
characters are held unconsumed until the next character decides them, and
end-of-input repairs happen only in `finish`. Each consumed byte charges one
`Work` unit and each value one more; output storage is charged by logical
length as it grows. Every charge is therefore a function of the byte
sequence alone. `crates/core/tests/json_incremental.rs` checks that every
single split point, and 64 seeded multi-split chunkings per input under two
seeds, reproduce the batch node, spans, consumed length, edits and complete
governor `Usage` over all recovery-golden inputs and fixtures, the JSON
Schema suite's instance data (inputs up to 2,048 bytes) and suspension-heavy
malformed inputs. The same file measures `Work` at 64 KiB, 256 KiB and 1 MiB
for one large string value, wide and deep nesting, and repaired relaxed
syntax; every work ratio is within 0.01% of its byte ratio (the test allows
5%). For example, a 1,048,572-byte document holding one string costs
1,048,574 Work.

When strict parsing fails, schema recovery offers the value at offset 0 and
every bracketed value as a candidate, each priced with the `json.extract`
loss `5 + 10 × brackets` for each non-blank discarded side. Candidate
discovery is linear. `scan::Tracker` (a string- and escape-aware, push-based
tracker of maximal `{…}`/`[…]` regions) finds the next opener outside earlier
parses, one tracked recovery parse reads each region, and every container
nested in it becomes a candidate as a subtree, with the consumed end and
edits its standalone recovery would have, without a re-parse. An opener a
region parse consumed as text (inside a string, comment or bare token) gets
one parse of its own unless an earlier such parse covered it; an opener
nested in text that such a parse itself consumed as text is not offered
again. Earlier recovery attempted a full parse from every bracket, which was
quadratic in the number of brackets and charged permanent scratch per
attempt.

`schema::recovery::EXTRACT_WORK_PER_BYTE` (48) bounds discovery `Work` at
`48 × n` for an `n`-byte input whose candidates are not nested in one another
(its Rustdoc derives at most `39n`), and at `48 × (n + s)` in general, where
`s` sums the source spans of nested candidates and is at most `n` times the
nesting depth. The extraction test recovers, within that bound and without
exhausting any budget:

| Input | Bytes | Whole-recovery Work | Budget |
| --- | ---: | ---: | --- |
| Prose with 100,000 `{` (empty placeholders) | 200,047 | 800,141 | `Effort::Standard` |
| A JSON string holding 100,000 `{` | 100,050 | 500,150 | `Effort::Standard` |
| A 1 MiB tool-call argument in prose | 1,048,592 | 5,204,027 | Deep, scratch raised to 1 GiB |

The 1 MiB case raises only the scratch limit: its recovered tree, candidate
copy and search key storage exceed the presets' 64 MiB. Recovery goldens and
a differential comparison against the previous implementation (goldens with
and without guesses, the JSON Schema suite's instances, and curated prose
with brackets) produce identical outcomes and ledgers; only usage changes.

Derivative pattern matching (`Pattern::is_match` for
`pattern.engine.derivative-v1`) steps the resumable byte-level automaton
`pattern::DerivativeDfa` over the text's UTF-8 bytes. It charges one `Work`
unit per byte plus the lazily built transition and nullability caches'
`Work` and `ScratchBytes`, and no longer buffers the text as scalars;
backtracking matching still admits its scalar buffer. A partially decoded
character is an explicit `DfaState` value, so charges depend only on the
byte sequence, not on how a caller splits it.

## Measured-allocation oracle

`crates/core/tests/allocation_oracle.rs` installs a test-only counting
`#[global_allocator]` (that file only; no dependencies; `#![allow(unsafe_code)]`
scoped to the file) and asserts, for the representative governed operations
listed below, **at
two deterministically generated input scales — a base size and a size at
least 16x larger** — that real allocator bytes never exceed

```
measured_bytes_allocated_during_op ≤ governor.usage().total_scratch_charged + input_len + K
```

with the *same* `K` at both scales. `total_scratch_charged`
(`Usage::total_scratch_charged`, backed by `ScratchMeter::total_charged`) is
the monotone sum of every scratch byte a governor has ever charged, never
reduced by a release — unlike `peak_scratch_bytes`, a high-water mark of only
*simultaneously* live scratch, which stays low for an operation that charges
and releases scratch many times in a loop (the fuzzy edit-distance region
scan, regex backtracking, per-candidate ranking) even though the allocator
performs one real call per iteration and those bytes sum over the whole
operation. Holding `K` constant across a 16x input-size change increases
sensitivity to allocations that grow with the workload. A test fails when
the measured excess exceeds that same `K` at either tested scale. Smaller
uncharged allocations can remain within the slack or be absorbed by other
conservative charges; two tested scales cannot establish a universal bound.
This oracle caught and
led to fixing three such regressions during development, each confirmed fixed
by measuring the per-unit gap across four to five scales, not just the two
the committed test asserts:

- `matching.rs`'s lexical-Jaccard token sets (`lexical_tokens`, called twice
  per candidate at `Standard` effort and above) were tokenized and
  deduplicated without any charge; the reservations in `signals` and
  `protected_class_violations` now include that cost.
- The bounded backtracking search clones capture state while evaluating
  repetitions and alternatives. Its capture vectors, exploration stack and
  results now each own their admitted storage through shared scratch
  containers; their leases remain live until the actual allocations drop.
- The former JSON syntax repair re-encoded every repaired bare/malformed
  token's replacement through `Node::to_json`, one allocation per repaired
  token, against a flat, too-small per-input-byte precharge. Its successor,
  `json/incremental.rs`, charges each replacement (at its worst-case quoted
  length) and every output container and string as it grows, so no
  per-input-byte precharge remains. The `json_recover_syntax` workload runs a
  whole recovery, so its `K` now covers the same fixed receipt, search and
  validation-log overhead as `recover`, which the old precharge used to
  absorb.

The `tool_call_recover` row, added with the tool-call cost bound, found
tool-call recovery allocating more per call than it charged (about 0.7 KiB
per call on its workload, and up to 22 KiB per call for a Hermes call with
16 arguments). Resolution copied every argument value into each partial
reading of a call and again for every later parameter (quadratic in the
parameter count), serialized each reading's arguments into an uncharged
buffer to digest it, and kept its readings, drafts, slots and alternatives
in uncharged containers; the recognizer copied its attempt and failure
lists each step, and merged configurations with a stable sort whose merge
buffer was uncharged. Resolution now moves values instead of copying them,
streams the digests, and charges its structures at portable sizes asserted
against `size_of`; the recognizer leases its per-step copies and merges in
place. The recovered calls, outcomes and receipts are unchanged apart from
the scratch usage they report.

Schema validation of each call's arguments (`Schema::validate_with`, which
resolution runs on the recovery's governor) also allocated about 1 to
1.5 KiB per call that it did not charge: the evaluator kept its recursion
set, member names, evaluated properties and items, and exercised patterns in
`BTreeSet`s whose nodes (several hundred bytes each on a 64-bit target)
were never charged, cloned base URIs into its scope stack, held applicator branches in
growing vectors, and formatted child keys and instance paths uncharged.
Measured on a generated two-string-parameter tool schema, validating
`{"path":"a.txt","content":"hello"}` allocated 1,131 bytes (peak live 1,006)
against 170 charged (peak 112), so a scratch limit between them validated on
uncharged memory. The `tool_call_recover` row's slack hid the gap: other
charges of the same recovery exceed its allocation by far more. The
evaluator implementation measured below used `ScratchVec`/`ScratchMap` at
portable element charges, charged held strings, leased child keys, instance
paths, reference resolution and held branch evaluations, and charged violation
messages by allocated capacity. At that measurement the same input allocated
374 bytes (peak live 318) against 515 charged (peak 412). These figures are
historical; the current control-state owner boundary is described above and
requires its own execution evidence.
`allocation_oracle::oracle_tool_call_validation_charges_its_working_set` is
a zero-slack control over generated and declared (`$ref`, `anyOf`) tool
schemas with valid and invalid arguments: every allocation is admitted by a
charge first, the allocator's peak and total never exceed the charged peak
and total, a scratch limit one byte below the measured allocator peak
returns `BudgetExceeded(ScratchBytes)`, and the charged peak suffices:

| Arguments | Allocator peak | Allocator total | Charged peak | Charged total |
| --- | ---: | ---: | ---: | ---: |
| `{"path":"a.txt","content":"hello"}` | 318 | 374 | 412 | 515 |
| `{"path":"a","content":"b","mode":"fast","count":3}` | 389 | 526 | 449 | 716 |
| `{"path":"a.txt"}` (missing `content`) | 750 | 777 | 862 | 935 |
| Declared schema, valid `$ref`, `anyOf` and array arguments | 1,453 | 2,799 | 1,661 | 4,097 |
| Declared schema, three violations | 2,684 | 4,700 | 3,084 | 5,675 |

`K` is fixed test slack for observed residual allocation at the two tested
scales, including per-call overhead (`Parser`/`Renderer` construction, digest
computation) and retained-output structures without individual charged
containers (schema index/children maps and small result vectors). Its fixed
value does not prove that every covered residual is scale-invariant. The
oracle rejects a proportional-allocation regression when its measured excess
crosses the inequality at a tested scale.

The oracle measures the current thread's process-wide allocator traffic; since
`cargo test` runs this file's tests as separate threads that can be reused
across test functions, each test acquires a file-wide `Mutex` for its *entire
body*, not just the measured call, so no other test's ordinary, unmeasured
setup work can add to the same global counter mid-measurement. Each governor
is constructed fresh inside the measured closure (never shared across the
warm-up and measured calls, and never across the two scales), so
`total_scratch_charged` is exactly one call's charge, not a running total; and
each measured operation runs once, unmeasured, before the timed call, to
absorb one-time per-thread costs (lazy statics, `std::fmt` machinery) that are
not the operation's own.

| Operation | Base workload | 16x-larger workload | K (bytes) |
| --- | --- | --- | --- |
| `schema_compile` | Object schema, 2 extra properties, nested `pattern` | 32 extra properties | 4,096 |
| `schema_validate` | Object instance, 2 properties | 32 properties | 2,048 |
| `recover` | One malformed key, 2 valid padding properties | 32 padding properties | 8,192 |
| `recover_composed_ambiguous` | Duplicate-key array, 2 elements | 32 elements | 4,096 |
| `json_parse` | Object, 2 extra key/value pairs | 32 pairs | 1,024 |
| `json_recover_syntax` | Unquoted/single-quoted object, 2 extra keys | 32 extra keys | 8,192 |
| `pattern_compile` | `^[A-Za-z]{2}$`-shaped pattern | 32 repeated atoms | 2,048 |
| `pattern_match` | `identifier_` doubled against `^[A-Za-z_]+$` | repeated 32x | 8,192 |
| `markdown_render` | Heading, 2-item list, 2-row table, 2 references | 32 of each | 4,096 |
| `markdown_plan_section` | 2 sections, replace section 0 | 32 sections | 4,096 |
| `markdown_plan_list` | 2-item list, insert after item 0 | 32 items | 4,096 |
| `markdown_plan_table_row` | 2-row GFM table, insert row 0 | 32 rows | 4,096 |
| `markdown_plan_table_cell` | 2-row GFM table, replace one cell | 32 rows | 4,096 |
| `markdown_plan_reference` | 2 reference definitions, edit occurrence 0 | 32 definitions | 4,096 |
| `matching_rank` | 2 non-matching candidates plus an exact one | 32 candidates | 8,192 |
| `matching_resolve` | 2 non-matching candidates plus an exact one | 32 candidates | 8,192 |
| `paths` | 2-path index, resolve path 0 | 32 paths | 4,096 |
| `edits_exact` | Padded source, locate an exact substring | 32x padding | 2,048 |
| `edits_fuzzy` | Padded source, locate within one edit | 32x padding | 8,192 |
| `edits_unified` | One hunk, 2 padding lines before it | 32 padding lines | 2,048 |
| `embeddings` | 2 non-matching candidates plus an exact one | 32 candidates | 2,048 |
| `intent` | 2 unrelated rules plus a matching one | 32 rules | 2,048 |
| `tool_call_recover` | Reasoning and prose, then 2 Qwen3-Coder XML calls alternating with 2 Hermes calls | 32 of each | 4,096 |

These `K` values, the workloads at both scales, and the inequality above are
asserted by `allocation_oracle.rs` itself; a regression that undercounts real
allocation by more than its row's `K`, at either scale, fails that test. They
are not a certification that any *other* input to these operations stays
within the same bound (an input shaped very differently from the oracle's
generators, or scaled far beyond 16x, could still expose a gap this two-point
check does not sample), and they do not replace `make resources`' broader,
allocator-agnostic measurements above.

Pattern generation has six additional controls with no `input_len` allowance
and no `K`. They compile patterns and construct governors before the measurement
window, then compare both cumulative allocator requests and peak live requested
bytes during generation against the corresponding governor charges. Derivative
`^a+$` witnesses are measured at lengths 64, 1,024, 8,192 and 32,768;
backreference witnesses are measured at lengths 64, 1,024 and 4,096. Generation
uses admitted state/frontier/cycle storage and a flat parent-index prefix arena,
with iterative reconstruction rather than cloning each growing prefix. The
backtracking engine owns its captures, states, and traversal frames individually.
Canonical alternation merges and derivative nullable/step traversal also use
admitted iterative continuations with per-term Work and cancellation checks.
Compilation propagates traversal stops rather than replacing failed work with a
dead language. Direct controls exercise 32,768 flat literals at translation depth two and
runtime depth zero, the previously uncharged nullable traversal, and overlapping root/residual
alternation chains with compilation and generation Work boundaries.

Compilation has two additional zero-slack allocator controls for wide alternations
with 8, 64 and 128 branches. Compile terms, their sorted lookup index, classes,
and predicate cuts own admitted capacities; actual AST/interval buffers also
release on failure or when syntax drops. Controls cover the formerly undercharged
128-branch arena under a 140,000-byte budget, exact scratch and Work boundaries,
and cancellation at initial, intermediate and final allocation checkpoints.
The complete allocator oracle contains 32 controls.

The controls also require exact charged-peak budgets to succeed and one byte
less to return typed scratch exhaustion, low Work to return typed exhaustion,
and cancellation before generation, during traversal, and before output
transfer to release all temporary charges. A retained witness keeps its charge
under the existing output contract. The original request for an 8,192-byte
witness under 4,096 scratch bytes now exhausts explicitly. These finite workloads
establish the measured logical allocation contract; they do not measure allocator
metadata, resident memory, or every possible pattern/input combination.

## Tree-sitter allocation bound

`crates/syntax/src/alloc.rs` installs itself as tree-sitter's C allocator
(`tree_sitter::set_allocator`, called exactly once via `std::sync::Once`,
before this crate ever constructs a `Parser`/`Language`) so tree-sitter's own
allocations are no longer external to the governor. Every `malloc`/`calloc`/
`realloc` tree-sitter's C library performs is attributed, via a header stored
immediately before the returned pointer, to the specific parse ("account")
active on the allocating thread; freeing that block — even from a different
thread, since `tree_sitter::Tree` is `Send` — decrements the same account
directly from the header, never a thread-local counter. The progress callback
`inspect`/`parse_governed` install on every parse (and, when this crate uses a
`QueryCursor`, the equivalent `QueryCursorOptions::progress_callback`) reads
that account's live-byte delta since its last check, charges it to
`Resource::ScratchBytes`, and breaks the parse with `Cancelled` or
`BudgetExceeded(ScratchBytes)` on failure; a break never returns a partial
tree, and every byte charged during a failed attempt is released before the
call returns. A successfully retained tree's bytes are charged once more and
tied to a `ScratchLease` released when the tree (`ParsedTree`, or `inspect`'s
own local tree) is dropped.

Because the real allocator can only be checked *between* progress-callback
invocations, real usage can momentarily exceed the last charge by however much
tree-sitter allocates in one callback interval — the *overshoot*. This is not
a null-return failure mode (tree-sitter aborts if `malloc` ever returns null;
`crates/syntax/src/alloc.rs`'s module docs cover why budget enforcement never
relies on that path) but a bound on how far real memory can run ahead of the
governor's own ledger before the next check catches up and either charges the
difference or stops the parse. `crates/syntax/tests/allocation.rs` measures
the largest such interval observed per grammar, over a small oracle corpus
covering each pinned grammar (mirroring `scripts/check-syntax-oracles.py`'s
probes, extended with one ~1 MiB generated input per grammar so the bound
reflects sustained parsing, not just a single token):

| Grammar | Measured maximum overshoot | Documented bound |
| --- | ---: | ---: |
| Python | 22,152 bytes | 40,960 bytes |
| Rust | 67,256 bytes | 102,400 bytes |
| JavaScript (incl. JSX) | 21,016 bytes | 40,960 bytes |
| TypeScript | 17,992 bytes | 40,960 bytes |
| TSX | 20,944 bytes | 40,960 bytes |

A caller sizing a `scratch_bytes` budget around this crate's parsing should
add the appropriate row's documented bound as headroom above the size it
actually intends to admit, on top of the input/output bytes themselves. These
bounds describe the oracle corpus above, not every possible input; a
pathologically different input shape could still exceed them, in which case
`crates/syntax/tests/allocation.rs` (which asserts exactly these numbers) is
the place to re-measure and update this table.

## Loader compatibility

A loader compiled with instructions unsupported by Valgrind can fail before the
workload starts. An explicit compatibility runtime is available on x86_64 Linux:

```sh
make resources RESOURCE_ARGS=--portable-runtime
```

This downloads the stock glibc archive pinned in `vendor/profiling-runtime.json`,
verifies its SHA-256, and extracts only its loader and libc using `bsdtar` into the
ignored cache. It changes no system installation. Both native and profiled runs
then use that loader/libc; other libraries still come from the host. The report
records the runtime archive and extracted-file hashes. These measurements qualify
that runtime, not the host's alternate allocator implementation. Appropriate
loader debug symbols must be available; installed debuginfod configuration is
inherited for ordinary system-runtime profiling. Portable profiling instead
downloads the exact loader's debug file from the public Arch source registered
in `vendor/profiling-runtime.json`, verifies its SHA-256, and keeps it beside
the loader under its GNU debuglink name. The report includes this file's digest.
This mode requires no ambient debuginfod configuration or system debug package.

Every measurement first runs the real empty workload natively and under both
Memcheck and Massif, with the same strict flags and output/log validation as the
campaign. `python3 scripts/measure-resources.py --portable-runtime --check-tools`
runs only that prerequisite check and writes no measurement report. Verify runs
it before its independent qualification campaigns; its toolchain installs both
WASM targets and its hosted runner provisions `libarchive-tools` for `bsdtar`.

Profiler stdout, stderr and raw tool logs are retained in the announced ignored
resource-log directory. Any nonzero profiler exit, including startup exit 1 or
Memcheck's memory-error exit 97, fails with all three diagnostics visible.
Successful process exit also requires exact workload output and valid tool logs.

The original hosted failure did not expose the raw startup diagnostic. A
controlled reproduction with the pinned profiler and portable loader, empty
debug cache and no `DEBUGINFOD_URLS`, exited 1 on a missing mandatory loader
redirection. Supplying the verified matching debug file made the same strict
run pass with zero Memcheck errors. This establishes the debug prerequisite;
the original hidden failure is not mislabeled as a workload memory error.

The measurement definitions follow the upstream
[Massif manual](https://valgrind.org/docs/manual/ms-manual.html) and
[Memcheck manual](https://valgrind.org/docs/manual/mc-manual.html). Massif's peak
recording depends on deallocations; observed peaks are not a universal memory
bound. External profiling tool/runtime provenance is in `docs/DEPENDENCIES.md`
and `vendor/tooling.json`.

## Latency and cold-build measurements

`make resources` also builds and runs `crates/core/benches/latency.rs` and
`crates/syntax/benches/latency.rs` (each `harness = false`, matching the
existing `resources`/`recovery` benches), and adds the following top-level report
fields:

- `latency_workloads`/`syntax_latency_workloads`: each named workload's
  median/p95 latency in nanoseconds over 200 samples (one unmeasured warm-up
  call first), plus real allocation call count and real peak live-heap
  bytes from that bench binary's own counting instrument -- a
  `#[global_allocator]` scoped to `crates/core/benches/latency.rs` (distinct
  from `crates/core/tests/allocation_oracle.rs`'s, and measuring peak live
  bytes rather than total bytes granted) for the core workloads, and this
  crate's own `alloc::live_bytes`/`peak_bytes` tree-sitter accounting (see
  above) for the per-grammar syntax workloads. Core workloads cover Markdown
  rendering and structural operations (section insert/remove/
  move/rename, list edit/renumber, table row/column move, reference add/
  relabel/remove/unlink), exact/fuzzy/unified edits, matching rank/resolve,
  paths, pattern compile/match (both the derivative and backtracking
  engines), and schema compile/validate/recover.
- `recovery_benchmark`: `crates/core/benches/recovery.rs`'s six deterministic
  quality/latency cases, captured verbatim and also written to
  [`qualification/recovery-bench-linux-x86_64.json`](qualification/recovery-bench-linux-x86_64.json)
  as its own committed artifact.
- `cold_build`: elapsed nanoseconds for a release CLI build using the real
  toolchain Cargo resolved through `rustup which cargo`. Both the target
  directory and Cargo's separate intermediate build directory are newly
  created and empty, under `/tmp` outside the checkout. `RUSTC_WRAPPER`,
  `RUSTC_WORKSPACE_WRAPPER`, and the
  corresponding Cargo environment/config values are cleared. Every Cargo
  compiler-artifact event must report `fresh: false`; the report records
  those counts and the resulting CLI's SHA-256. Registry
  downloads and OS page caches remain available. This measures an uncached
  compilation and link, excluding dependency download time and page-cache
  flushing.

Each latency workload runs in its own process. Its `process_peak_rss_bytes`
is Linux `VmHWM` after the warm-up and 200 samples, including process startup,
fixtures, the sample vector, assertions and the small procfs read. Core
`allocation_calls` includes successful allocate, zeroed-allocate and resize
calls; `peak_live_bytes` is the high-water increase above allocations already
live when the one measured operation starts. Neither includes profiler or
procfs-reading allocations. Syntax records ordinary Rust allocator calls and
peak growth separately as `rust_allocation_calls` and `rust_peak_live_bytes`.
Its `tree_allocation_calls`, `retained_bytes`, and `peak_bytes` cover the
parse's actual tree-sitter C allocations (payload bytes exclude its accounting
headers), which use `System` directly. These independent peaks are not added
or described as a simultaneous combined peak. Parse traversal's Rust
allocations are included in the Rust instrument. Latency and allocation
observations cover fixture construction, correctness checks (including JSON
serialization of the composed-recovery expected value), and dropping outputs.
These are measurements of the complete assertion-backed workload.

On a host whose default compiler flags target an instruction set Valgrind
cannot decode (for example, a wrapper that appends `-C target-cpu=native`),
building the profiled `resources` bench with an explicit, portable
`target-cpu` (this project builds it with `x86-64-v2`) opts that one build
out, matching the escape hatch such a wrapper documents for itself: an
explicit `target-cpu` anywhere already in the resolved `rustflags` wins over
any flag the wrapper would otherwise add.

## Recorded baseline

[Linux x86_64 resource baseline](qualification/resources-linux-x86_64.json) was measured from commit `300f7cffe188ffbd7b569e6b76476cc27f6ded4a` with a dirty working tree. The engine source digest `659c7a62fa86d072676a2730af8978c7dd4bce0e59d87ecf724858334d13a211` was checked before and after the campaign. The run used the isolated runtime with verified matching loader debug symbols and an empty `DEBUGINFOD_URLS`; it records all three runtime file hashes and executable identities. Raw logs are retained locally in `.cache/resource-logs-awk6qz7k`. Ordinary build durations describe their observed Cargo cache states; the separate `cold_build` records compilation with empty target and intermediate directories and disabled compiler wrappers.

| Workload | Allocation calls | Total allocated bytes | Observed requested-heap peak | Native reported RSS |
| --- | ---: | ---: | ---: | ---: |
| empty | 13 | 3,817 | 1,736 | 4,689,920 |
| schema_compile | 106 | 9,168 | 4,538 | 5,353,472 |
| json_parse | 1,574 | 236,097 | 173,150 | 5,337,088 |
| json_composed | 2,541 | 192,835 | 62,611 | 5,636,096 |
| json_ambiguous | 389 | 33,349 | 15,689 | 5,455,872 |
| markdown_render | 4,096 | 777,195 | 225,787 | 5,644,288 |
| markdown_list | 2,811 | 242,688 | 152,036 | 5,652,480 |
| unicode | 23 | 125,931 | 68,955 | 5,103,616 |
| matching | 7,222 | 313,232 | 23,944 | 5,234,688 |

Values include the full benchmark process as described above. They are not per-function allocation counts, portable guarantees, or a production sizing rule.

### Cold build

The recorded release CLI cold build took 27.364 seconds and produced 35 compiler artifacts, with 0 reused artifacts. Both compiler wrapper layers were disabled; the target and intermediate directories were new and empty. Registry sources and OS page caches remained available. The resulting executable has SHA-256 `24ea7dd813a878666c1d304740ec49a43a5ef0c045b071d135055b8d6f3aa053`.

### Latency workload samples (selected)

Full detail is in the committed report. Each row measures 200 samples in its own process; allocation and heap observations cover one operation after sampling.

| Core workload | Median ns | p95 ns | Allocation calls | Peak live bytes | Peak RSS bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| schema_compile | 3,200 | 3,450 | 93 | 3,928 | 5,754,880 |
| schema_recover | 68,988 | 74,337 | 2,528 | 62,005 | 6,197,248 |
| matching_resolve | 430,966 | 448,736 | 3,684 | 19,838 | 5,722,112 |
| edits_fuzzy | 297,341 | 308,080 | 12,980 | 12,676 | 5,664,768 |
| markdown_render | 16,700 | 18,799 | 450 | 24,840 | 5,947,392 |

| Syntax workload | Median ns | p95 ns | C allocation calls | Rust allocation calls | C peak bytes | Rust peak growth bytes | Peak RSS bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| python | 177,255 | 184,354 | 573 | 59 | 56,720 | 4,760 | 3,457,024 |
| rust | 152,015 | 160,875 | 480 | 42 | 48,896 | 3,048 | 3,936,256 |
| javascript | 162,175 | 171,545 | 476 | 25 | 48,360 | 2,264 | 3,436,544 |
| typescript | 202,783 | 212,293 | 652 | 42 | 64,040 | 3,184 | 4,263,936 |
| tsx | 185,994 | 196,384 | 545 | 25 | 53,032 | 2,288 | 4,378,624 |

C and Rust peak values are measured separately and must not be added as a simultaneous peak.

## Tool-call recovery cost bound

`ToolSet::recover` charges every step of `tool_call.recover.v1` to one
governor, with the named constants of `purrassist_core::grammar::cost`.
Recovering `n` output bytes charges exactly

```text
recover     = W_intake + recognition + resolution
W_intake    = 2·n (source insertion, encoding scan) + G_source(n) + r + G_ranges(r)
recognition = K_LEX·n + K_THREAD·(configuration steps) + K_EVENT·(segment events)
            + K_CAPTURE·(captured bytes and history nodes materialized)
            + K_ERR·(error configurations explored)
            + K_CODEC·(codec scan bytes, name lookups and tree nodes)
            + (JSON recovery units) + (class-automaton units) + (search key bytes)
resolution  = one unit per member, option, alternative and validation step,
              plus the recovered value, ids, checks and ledger entries
            + (unparsed-markup scan: one unit per content byte read)
```

Here `r` is the number of maximal invalid UTF-8 runs. `G_source` and
`G_ranges` sum the occupied lengths charged before each capacity growth of
the source and invalid-range vectors. The shared doubling growth policy gives
`G_source(n) ≤ 2·n`, `G_ranges(r) ≤ 2·r` and `r ≤ n`, so intake work is at most
`7·n`. Empty vectors contribute zero relocation work. These terms describe
admitted work. A governor or capture refusal stops admission at its original
cause; a semantic request error stops interpretation while admitted source
capture can continue.

Revision facts prepare one borrowed leaf interval index per raw reading with
operator payloads. Original leaf positions sort by source interval; balanced
subtree maximum ends prune leaves that cannot enclose an edit. Every enclosing
leaf must agree on its parameter pointer. Different overlapping pointers remain
unknown, including adjacent leaves sharing an empty insertion's endpoint. The
index copies neither argument trees nor pointer text. Construction, sorting,
traversal and pointer comparisons admit Work; its fallible scratch storage
releases after that reading's facts are prepared, including on refusal. The
existing comparison of matching edit facts across alternatives stays unchanged.
Disjoint leaf ranges avoid a full leaf scan for every edit; this does not claim
a measured budget pass or a linear bound for arbitrary overlapping intervals.

Pythonic's finite-state framing and canonical decimal literal path scan
linearly. Converting a binary, octal or hexadecimal integer with `n` digits
to exact decimal uses O(n²) digit work, admitted before each multiplication
pass. This typed conversion is additional codec work, outside the grammar's
finite-state scan bound below. A finite caller budget can stop it explicitly;
there is no universal linear completion claim for nondecimal Python integers.

The unparsed-markup scan (`tool_call::detect_unparsed`) reads each reading's
content once and, at each anchor, a bounded window (256 bytes, 64 for
Harmony's message tokens); after a hit it resumes at the hit's end.
`tool_call_cost::unparsed_markup_scan_is_linear` measures it on 64 KiB and
1 MiB of prose (2.9 units per byte) and of inputs dense with each family's
anchor (at most 29.5 units per byte, for `to=functions.` on every line),
linear within 1.05x and under 32 units per byte. A turn that is a single
call has no content to scan.

Tool-call recovery now uses one incremental intake path. It admits the source
document's metadata before validating the request and constructing the
recognizer. Each input byte then admits the cumulative input length, source
storage and copying, UTF-8 classification, and recognition, in that order.
Source and invalid-range vectors admit insertion and relocation work before
growth; their capacity schedule depends on the byte sequence, never the number
or size of caller chunks. The pending UTF-8 scalar holds at most three bytes
between byte steps. Invalid runs retain their original ranges and receive the
existing loss records once the complete source digest is available.

This intentionally replaces the earlier ordering that retained the whole
source and scanned its encoding before recognition began. Usage counters and
the first refusal may therefore differ from that ordering. A semantic invalid
request stops interpretation but keeps capturing the original source under the
same healthy governor. If source admission then fails, that explicit refusal
takes precedence over the semantic error and no retained prefix is published.
After a governor or capture refusal, remaining supplied bytes contribute only
to the source digest: no further source copying, admission or interpretation
occurs. Failed source allocation is never retried. A receipt retains the
complete admitted source or no source; a fully captured source remains
available when a later stage refuses. The metadata vector is reserved before
intake, so publishing a complete source after refusal moves existing storage
without allocating.

Batch recovery runs this path now. Its recognizer also maintains a conservative
native continuation frontier after each byte. The frontier intersects the
content state, every active attempt's turn interpretations, open reasoning and
promoted bases, and pending and engine ownership barriers. Turn histories cache
their unambiguous structural prefix with one skip link per node; prefix
intersection visits those links rather than enumerating or replaying histories.
The portable history-node reservation is 48 bytes. Traversals admit work before
each visited step, and scanning live owners admits work for their records.
Pending reports keep their dispatch order separately from a counted source
index. Paged source-offset counts and hierarchical occupancy bitsets retain the
exact minimum, including a later-ending report whose source begins earlier.
Each occupancy level marks nonempty words of the level below it; all levels
exist from the first origin through one fixed root word. Updates and minimum
descent visit at most eleven levels on 64-bit targets, or six on 32-bit targets,
without assuming source origins arrive or leave in order. Every prepared update
admits 104 Work on either target for bounded planning, validation and writes,
including the root read and page bookkeeping. Growth separately admits newly
initialized directory entries, count slots and bitmap words, and live slots
potentially relocated by a capacity increase. Directory and bitmap capacities
are reserved before their lengths grow. Each touched page reserves exactly 64
native-width counts, while an empty inner owner represents an unallocated page.
A prepared update owns a newly initialized page until queue admission succeeds;
refusal drops that page without installing it. Live counts, occupancy and the
cached minimum change only after the complete update and queue admission
succeed. Removal is planned before a report leaves the dispatch queue, with no
fallible charge between that removal and applying the source update. Invalid
source offsets, count overflow
and inconsistent occupancy paths fail explicitly.

The index grows only to the largest queued origin, which must not exceed the
fed byte count, including an empty completion's EOF offset. Dividing the source
coordinate into a page and slot avoids an overflowing `source + 1` at the native
offset limit. Scratch vectors own and charge their reserved capacity until the
recognizer drops; the old 256-byte reservation per distinct origin is removed.
The directory stores 40-byte inner owners with geometric growth, while page
capacity stays at 64 counts. Together with the occupancy buffers this reserves
less than 9.504 bytes per indexed source position plus rounding and fixed level
overhead. A late origin after a large source gap needs the directory and its own
page, without allocating count pages for the gap. Hermes JSON strings followed
by `}` can queue such a late origin: their closing quote and brace also match an
Anthropic reasoning closer, which is queued before ownership interpretation.
These source-derived storage and update bounds do not establish whether a
workload fits its Work or scratch budget. Frontier updates read the cached exact
minimum without scanning the pending
queue.

A successful native projection retains an exact memo of its engine and
pending-source floors, final ownership barrier and effective cutoff. Every byte
still scans the engine owners with their original admissions, then admits one
work unit for a fixed-key lookup. With both floors unchanged and no processed
turn report, the memo is reused only when the advancing lexical cutoff capped
by its saved barrier equals the prior effective cutoff. A stationary candidate
can therefore avoid repeated interpretation scans and ancestry intersections;
ordinary content still advances when its cutoff changes. Skipped traversals do
not run or charge their former work. Any processed report invalidates the memo
before changing turn histories, interpretations, reasoning or late embedded
segments. A missed refresh clears the old memo before fallible projection and
records a replacement only after success. No owner minimum or semantic choice
is approximated, and feed boundaries do not participate in this key.

The shared intake can notify a private callback when that frontier changes.
It caps the reported range before incomplete UTF-8 and charges `K_EVENT` work
per notification. Every reported end also checks its admitted source boundary:
at most three backward byte probes and one scalar validation of at most four
bytes move a cutoff inside valid UTF-8 back to the scalar's start. This admits
12 work units per normalization. Separate fixed prefix/content caches admit
one work unit per lookup and retain only successful results keyed by the
UTF-8-capped cutoff integer. An unchanged cutoff skips normalization and its
charge; matching content and prefix ends still share one result. A capped
cutoff cannot later fall inside a newly completed valid scalar because the
encoding cursor withholds every viable incomplete scalar at its start.
Malformed subparts cannot become valid through later appends. Invalid byte
sequences retain their exact boundaries. Batch uses the
same producer with a discard sink. Feed count and chunk boundaries do not
decide notification boundaries. These notifications
carry native ownership metadata, not resolved calls or permission to execute.
The same intake also constructs owned `ContentDelta` events for advances
of the initial native content run, before any call, attempt or reasoning block.
Each payload copies only the newly exposed source range into a temporary
`ScratchVec`, admitting its capacity and `K_EVENT` plus one work unit per copied
byte before notification. Every event additionally admits 512 bytes for its
record and per-event collector insertion, plus four work units for amortized
collector insertion and relocation. The record reservation covers four native
slots, including a fresh collector receiving one event. A prepared event owns
that temporary reservation and every payload reservation through sink handoff.
Vectors, strings, typed parameter trees and copied outcome text transfer their
reservations into this owner without permanent retention. The producer releases
its reservations after the sink returns, including a refused collector insertion.
Batch discard, external sinks and convenience collectors run the same admission
and release schedule. Performed work and cumulative scratch charges remain;
discarded event payloads no longer become permanent live scratch debt.

Sink and collector storage belongs to the caller from inception, independently
of the producer's temporary handoff ownership. Caller retention, collector
capacity and feed boundaries do not change recovery admissions or releases.
The convenience collectors reserve fallibly during handoff; allocation failure
stops interpretation with an explicit refusal. Empty feeds allocate
no events, and input chunks do not set event boundaries. A closed
native content node contributes its full settled range, including when the
lexical boundary lands exactly on its end.

Live content after a structural segment is still withheld: final rejection extension
and stray-closer handling can remove such bytes from native content. Unsupported
markup reports over the initial content run preserve its content bytes. This
producer never replays source. At EOF, one completed whole-message reading
flushes its remaining content and reasoning ranges from the existing result.
Both batch discard and streaming sinks run that same final producer.

A native event cursor follows the structurally shared history exactly once.
Its direct fixed-key lookup admits one work unit. A successful traversal is
cached by settled native history identity and normalized end; an unchanged key
skips repeated ancestry traversal and its charges. A changed end reconsiders a
step held beyond the earlier cutoff. Stored native ancestry is immutable;
late reasoning embedding changes neither the text ranges nor the call readings
this event cursor inspects. Progress and content callbacks still precede this
traversal, and feed boundaries do not affect either cache.
The next-node lookup uses existing skip ancestry and admits every traversal;
it does not rebuild or resolve prefix turns. Stored reasoning segments publish
their exact marker-free text. Calls publish `CallStarted` only when every native
reading is immune to examples-as-content and declaration-as-content suppression.
A potentially suppressible call blocks subsequent live call indexing. Canonical
tool-name or explicit-alias agreement across every reading publishes `CallName`;
its lookup admission uses the actual canonical-name and alias index cardinality.
Fuzzy identities wait for final resolution. A unique shared arguments-capture range
publishes raw `ArgumentDelta`, including for mutating tools: raw bytes make no
claim about settled parameter values or permission to execute.

Before resolution consumes the finished native turn, the cursor records
governed argument-capture offsets and authentic grammar-edit payloads. A charged sort and binary lookup allow EOF
events to use captures whose native alternatives agree, without replaying the
input or decoding arguments again. The completed unique reading fills missing
call starts, canonical names and raw arguments. EOF names may follow a guessed
final identity; the final outcome and receipt retain that uncertainty.

The same EOF producer now emits `CallComplete` with the exact final per-call
outcome, preserving `BestGuess`, `Ambiguous` and every unresolved reason.
Blocker text is copied fallibly under a transient storage lease and published
only after the event record is admitted. For resolved or best-guess calls,
`ParameterComplete` follows `CallComplete`, including for mutating tools.
Values use `OwnedDocument::copy_from`; their real reservations stay with the
prepared event through handoff, preserving lexical numbers, typed values and
spans. Parameters are emitted in canonical name order. Ambiguous and unresolved calls, and
duplicate member names, do not publish a supposedly settled parameter value.

`CallRejected` carries each final rejected attempt's complete span and precise
reason. Its `index` is `None` when no `CallStarted` was emitted for that attempt;
rejected-list ordinals are never substituted for accepted-call indices. A
future identity-change event may use `Some` only for that already started
identity, followed by a new `CallStarted` for its replacement.

Native `Revision` events match retained operator facts against selected ledger
rule ids and exact source spans. Their original and replacement bytes come
directly from `MarkupEdit`, and their guess flag comes from that operator's
registry entry. Multiple native candidates must agree on both payloads. A
parameter JSON Pointer is carried only when the original argument-leaf owners
agree; markup or an uncertain owner uses `None`. Neither explanations nor
serialized diagnostic text are parsed to reconstruct edits. Revisions created
by semantic search still need structured transformation records at their
production sites; this native route does not fabricate their missing payloads.

Revision rule lookup uses a static ordinal index sorted at compile time from
the operator registry. Equal identifiers keep their original registry order,
so the first matching operator remains the same. An explicit lower-bound loop
halves its exclusive search interval on every comparison. Each string comparison
admits the queried identifier's byte length plus one Work before it runs,
including the final equality check when an indexed entry exists. A lookup uses
at most the registry count's bit width plus one comparisons: at most seven
for the current 47 operators. This index adds no heap storage and preserves
evidence and event order; these source bounds do not establish where the
restarted DeepSeek workload exhausts or whether it fits its Work budget.

Packed ambiguity and grouped attempts can hold this conservative frontier even
when later recovery would fold their meanings together. Proving the longest
semantic stable prefix, ambiguous EOF event intersection, exact held/candidate
byte accounting, semantic-search revision events,
decoding queries, governed cloning and CLI/MCP progress remain
unfinished under #16.

`ToolSet::stream` and `stream_governed` now own the request and original governor
while borrowing the tools and static built-in dialect set. Explicit dialect
constructors borrow a prepared set instead, without a self-reference or clone.
`ToolCallStream::feed` extends a caller's event sink, and `feed_collect` returns
newly published events. `cancel` marks the original governor; `finish(stop)`
checks refusal before completing and resolving the live recognizer.
`finish_with_events` and `finish_collect` preserve EOF events; plain `finish`
delegates to the same producer with a discard sink. Allocation refusal during
EOF collection is explicit in the returned result. This facade implements the
source facts described above, not the complete streaming event contract.

Each dialect `d` of the explicit prepared allow-list keeps at most `B_d`
configurations live per input byte, where `B_d` is
`CompiledGrammar::configuration_bound()` (one per program location plus one
per byte position of each literal spelling: canonical, closer variant,
confusable and, in a `json-envelope` grammar, the single-quoted, bare-key and
colonless JSON spellings of its envelope literals); `K_d = K_THREAD·B_d` is its
per-byte term. The published bound is

```text
work ≤ W_intake + (K_LEX + Σ_d K_d)·n + K_ERR·E + W_aux,   K_d = K_THREAD·B_d
```

Here `W_aux` includes the separately admitted class-automaton construction,
segment events, captured history, codec and JSON-recovery work, search keys,
and resolution listed in the exact decomposition above. The configuration
term bounds finite-state framing, not arbitrary codec/schema computation or
materialization of competing readings. `E` is the number of error configurations the repair search explores,
capped by the effort's `Work` budget (explicit `BudgetExceeded` beyond it).
Current intake counters have not been measured; historical throughput rows
do not qualify the new byte-fold costs. The configuration term is an upper bound per byte: on the fast path only a
handful of the `B_d` configurations are live at any byte, and the remaining
linear terms (segment events, captured bytes, codec scans, JSON recovery
units, resolution) are each a small constant per byte: recorded baseline recoveries
measured 10 to 26 units per byte in total (the throughput table below), far
inside the slack of any built-in `K_d`.
`crates/core/tests/tool_call_scale.rs` (`make corpus`) checks the bound on
64 KiB and 1 MiB Qwen3-Coder XML, Hermes, Llama 3 JSON, Mistral v3, Mistral v11,
DeepSeek-V3.1, DeepSeek-V3, Kimi-K2, Anthropic, Harmony and bare JSON
string arguments
(`E = 0`: no repair is recorded) and that the 64 KiB to 1 MiB work ratio is
within 1.05x of the byte ratio; `crates/core/tests/tool_call_cost.rs` (`make
check`) runs the same checks from 64 KiB to 256 KiB, and checks that pushing the same output in chunks of 1, 16 and 4096 bytes
charges identical usage, and that adding a dialect to the allow-list adds
at most its `K_d` per byte (measured, adding the dialects one at a time in
the order Qwen3-Coder XML, Hermes, `OpenAI`, Llama 3 JSON, Mistral v3,
Mistral v11, DeepSeek-V3.1, DeepSeek-V3, Kimi-K2, Anthropic, Harmony, bare
JSON: each dialect adds 5 to 8 units per byte on an argument in its own
format, the configurations of the call it now reads, and 0 on the others',
except that adding Hermes adds 6 on a Qwen3-Coder XML argument (after
Hermes rejects the shared `<tool_call>`, the bare `<function=>` element is a
second Qwen3-Coder XML reading kept alive until the wrapped call ends),
adding bare JSON adds 7 on a Hermes or Mistral v3 argument (whose call
object it also reads until the wrapper's call covers it), and adding Mistral
v3 or DeepSeek-V3.1 adds 1 on a Mistral v11 or Kimi-K2 argument and
DeepSeek-V3.1 5 on a DeepSeek-V3 argument, which share tokens). These are checks of the published inequality on those
inputs, not a proof for every input; the effort budgets bound everything
else.

`resources_doc_matches_cost_constants` parses this section and requires
every value below to equal the code's.

| Constant | Value | Meaning |
| --- | ---: | --- |
| `K_LEX` | 1 | `Work` per input byte for the lexical layer |
| `K_THREAD` | 1 | `Work` per configuration step |
| `K_EVENT` | 1 | `Work` per segment event |
| `K_CAPTURE` | 1 | `Work` per captured byte or history node materialized |
| `K_ERR` | 8 | `Work` per error configuration the repair search explores |
| `K_COMPILE` | 1 | `Work` per grammar source byte and compiled table entry |
| `K_PRINT` | 1 | `Work` per expression visited and byte printed by the printer |
| `K_WITNESS` | 1 | `Work` per value a witness generates |
| `K_CODEC` | 1 | `Work` per byte, lookup or node an argument codec visits |
| `JSON_CODEC_STATE` | 1024 | `ScratchBytes` for one `json` codec slot's state |
| `CLOSER_VARIANT` | 1 | Repair cost: a declared closer variant |
| `TERMINAL_CONFUSABLE` | 1 | Repair cost: a confusable spelling of a literal |
| `CONSUMED_CLOSER` | 1 | Repair cost: a closer the stop sequence consumed |
| `INSERT_CLOSER` | 2 | Repair cost: a missing closer |
| `GROUP_SPLIT` | 2 | Repair cost: an attempt reading several calls ended before the call it could not read |
| `CLOSER_MISSPELLED` | 2 | Repair cost: a closer token one edit away from exactly one closer |
| `TOKEN_MISSPELLED` | 2 | Repair cost: a markup token other than a closer one edit away from exactly one literal of the grammar |
| `CLOSER_IN_VALUE` | 0 | Reading a closer's exact spelling inside a verbatim value as text (no repair) |
| `TEXT_AFTER_CLOSER` | 0 | Each of the two readings of a verbatim value holding text after an unpaired closer (holding the text, or ending the value at the closer and dropping it): equal, so the call is ambiguous |
| `DELETE_STRAY_CLOSER` | 3 | Repair cost: a stray closer |
| `REASONING_UNCLOSED` | 4 | Repair cost: a reasoning block closed at a call opener |
| `REJECT` | 16 | Cost of rejecting an unrepairable call attempt |
| `MISSING_PARAMETER_CLOSE` | 2 | Repair cost: a value ended at a declared parameter's opener |
| `FRAMING_NEWLINE` | 0 | Reading one XML framing newline |
| `STRINGIFIED_ARGUMENTS` | 0 | Decoding stringified arguments |
| `JSON_REPAIR` | 1 | One non-guessed JSON recovery edit in a value |
| `JSON_GUESS` | 4 | One guessed JSON recovery edit in a value |
| `STRAY_CLOSER_GAP` | 32 | Longest whitespace run before an absorbed stray closer |
| `QUEUED_REPORT` | 96 | `ScratchBytes` per queued recognizer report |
| `EXTRACT_PROSE` | 2 | Repair cost: prose dropped inside a call |
| `EXTRACT_FENCE` | 1 | Repair cost: a fence marker dropped inside a call |
| `UNDECLARED_NAME` | 4 | Selection weight of a call whose name denotes no declared tool |
| `RECOVER_WORK_PER_BYTE` | 224 | `Work` per output byte `ToolRecoveryRequest::sized` adds |
| `RECOVER_SCRATCH_PER_BYTE` | 368 | `ScratchBytes` per output byte `ToolRecoveryRequest::sized` adds |
| `RECOVER_EVIDENCE_BYTES_PER_ENTRY` | 7 | `ToolRecoveryRequest::sized` adds `⌈n / 7⌉` `EvidenceEntries` |
| `RECOVER_OPENER_CANDIDATE_DIVISOR` | 8 | `ToolRecoveryRequest::sized` adds `⌈preset.candidates / 8⌉` `Candidates` per call opener |

| Built-in dialect | `B_d` | `K_d` per byte |
| --- | ---: | ---: |
| `anthropic-tool-use.v1` | 3391 | 3391 |
| `bare-json.v1` | 378 | 378 |
| `deepseek-v3.1.v1` | 2945 | 2945 |
| `deepseek-v3.v1` | 4793 | 4793 |
| `gemma3-tool-code.v1` | 97 | 97 |
| `glm-4.5.v1` | 107 | 107 |
| `harmony.v1` | 3890 | 3890 |
| `hermes.v1` | 1137 | 1137 |
| `kimi-k2.v1` | 2436 | 2436 |
| `llama3-json.v1` | 986 | 986 |
| `mistral-v11.v1` | 136 | 136 |
| `mistral-v3.v1` | 2228 | 2228 |
| `openai-tool-calls.v1` | 6665 | 6665 |
| `pythonic.v1` | 129 | 129 |
| `qwen3-coder-xml.v1` | 971 | 971 |

The default request allows all fifteen built-in dialects. Their combined bound is
`(2 + 1 + 30289)·n + 8·E = 30292·n + 8·E`.

The `kv-pairs` codec scans argument fields forward with a bounded tag
comparison at each byte. Typed JSON boundaries use the native JSON cursor,
then decode the bounded value once. Exact closers take precedence over fuzzy
substrings in verbatim text. The shared linear, sized-budget and dense-markup
checks include GLM calls.

Gemma uses the same finite-state Pythonic framer and native literal conversion.
Its long-argument, sized-budget and dense-call runtime checks pass. The
configuration bound is included above, and the dense scale measurement appears
in the table below.

### Budgets for large outputs and busy turns

Fixed effort presets are sized for ordinary turns, so a large output or a
turn with many calls needs a budget that grows with it.
`ToolRecoveryRequest::sized(effort, output)` (built-in dialects) and
`ToolRecoveryRequest::sized_for(effort, output, dialects)` derive one in two
phases. First a linear, allocation-free pre-scan counts the output's call
openers `m` (`DialectSet::call_openers`: each byte offset where a dialect's
`call` production can open, counted once per such dialect, so the
`<tool_call>` that Qwen3-Coder XML, Hermes and GLM share counts three times, and
Qwen3-Coder XML counts every `<function=` too, since a bare function element
is a call; Llama 3 JSON, Anthropic `tool_use` and bare JSON, whose attempts
open at `{` and commit only on call-shaped evidence, each count every `{`). Then
`ToolRecoveryRequest::sized_budget(effort, n, m)` sizes the budget from the
preset, the output length `n`, and the constants above:

```text
work             = preset.work          + RECOVER_WORK_PER_BYTE    · n   (224 per byte)
scratch_bytes    = preset.scratch_bytes + RECOVER_SCRATCH_PER_BYTE · n   (368 per byte)
input_bytes      = max(preset.input_bytes, n)
candidates       = preset.candidates
                 + ⌈preset.candidates / RECOVER_OPENER_CANDIDATE_DIVISOR⌉ · m
evidence_entries = preset.evidence_entries
                 + ⌈n / RECOVER_EVIDENCE_BYTES_PER_ENTRY⌉
```

The per-byte constants and each preset's fixed allowances form a measured
sizing recipe (see "Dense markup" below). The supplied shapes through
1 MiB recover under plain
`sized(effort, output)`. The budget stays linear in `n`; the published
worst-case bound `work ≤ (2 + K_LEX + Σ_d K_d)·n + K_ERR·E` above is the
analytical bound, and `RECOVER_WORK_PER_BYTE` is the measured sizing, not a
replacement for it.

The candidate term is the effort's multiplier per opener: 1 at `Exact`, 2 at
`Quick`, 16 at `Standard`, 128 at `Deep`, and 1,024 at `Maximum`. Competing
readings are charged per call attempt (a second dialect's attempt at a
shared opener, a call's second reading, each further segmentation of a
contested span), so a fixed preset that suits an ordinary turn exhausts on
a busy one; the opener term grows with the calls and keeps every limit
finite and linear in `n` (`m ≤ n`). Recovery folds the segmentations of a
contested span that recover the same calls before it enumerates
whole-message readings, so such spans add readings instead of multiplying
them. The derivation is saturating and deterministic; every other limit is
the preset's, so repair search stays bounded by the effort, and readings
that genuinely multiply (a call with many repeated parameters) still exhaust
as `BudgetExceeded(Candidates)`.

`tool_call_cost::busy_turn_resolves_every_call_under_sized` recovers 64
Qwen3-Coder XML calls, each needing the
`tool_call.markup.missing-parameter-close` repair and each followed by a
stray `<tool_call>` that both dialects contest (`m = 320`: two openers at
each `<tool_call>` and one at each `<function=`): the unmodified
`Effort::Standard` preset (128 candidates) exhausts explicitly, and
`sized(Effort::Standard, output)` (128 + 16 · 320 = 5,248 candidates)
recovers every call `Resolved`, charging 129 candidates.
`tool_call_cost::per_call_competition_scales_with_calls` does the same for
64 read-only calls that each have a near-miss name and a repeated parameter
(192 candidates), and
`tool_call_cost::crafted_reading_explosion_exhausts_explicitly` requires a
call with 16 doubled parameters (2^16 readings) to exhaust explicitly at
every effort: `Candidates`, or at `Maximum`, whose larger per-opener
candidate share lets the readings outgrow it first, `ScratchBytes`. The host and CLI tests
`tool_call_tools::busy_turn_resolves_every_call_by_default` and
`tool_calls_cli::busy_turn_resolves_every_call_by_default` recover the same
64-call turn through the MCP tool and the CLI with no effort option.

`tool_call_scale::one_mib_argument_fits_published_budget` recovers a 1 MiB
string argument in each built-in dialect's shape and checks that the call is
`Resolved` under `sized(effort, output)` for every effort from `Quick` to
`Maximum` (and, for Qwen3-Coder XML, Hermes, Llama 3 JSON, Mistral v3,
Mistral v11, DeepSeek-V3.1, DeepSeek-V3, Kimi-K2, Anthropic `tool_use`,
Harmony and bare JSON, under the unmodified `Effort::Maximum` preset), with
identical charges in every case, peak scratch at most 16 bytes per input
byte for those eleven, and
at most `RECOVER_SCRATCH_PER_BYTE` for `OpenAI` stringified arguments, whose
escaped string is also decoded, recorded as the decoding's replacement and
parsed again. The charges include the call's schema validation (see
"Measured-allocation oracle"):

| Input | Bytes | Work | Work per byte | Peak scratch | Scratch per byte |
| --- | ---: | ---: | ---: | ---: | ---: |
| Qwen3-Coder XML `<parameter>` string | 1,048,663 | 17,831,002 | 17.00 | 11,644,176 | 11.10 |
| Hermes JSON string argument | 1,048,648 | 17,833,269 | 17.01 | 11,649,509 | 11.11 |
| Llama 3 JSON string argument | 1,048,624 | 12,588,743 | 12.01 | 8,397,746 | 8.01 |
| Mistral v3 JSON string argument | 1,048,657 | 17,833,607 | 17.01 | 11,647,317 | 11.11 |
| Mistral v11 JSON string argument | 1,048,632 | 10,490,105 | 10.00 | 8,399,489 | 8.01 |
| DeepSeek-V3.1 JSON string argument | 1,048,720 | 10,493,971 | 10.01 | 8,397,978 | 8.01 |
| DeepSeek-V3 fenced JSON string argument | 1,048,741 | 14,692,492 | 14.01 | 8,399,375 | 8.01 |
| Kimi-K2 JSON string argument | 1,048,726 | 10,508,318 | 10.02 | 8,398,198 | 8.01 |
| Anthropic `tool_use` JSON string argument | 1,048,649 | 12,588,649 | 12.00 | 8,398,045 | 8.01 |
| Harmony JSON string argument | 1,048,686 | 12,599,510 | 12.01 | 8,397,816 | 8.01 |
| Bare JSON string argument | 1,048,623 | 12,588,732 | 12.01 | 8,397,745 | 8.01 |
| `OpenAI` stringified string argument | 1,048,679 | 27,273,065 | 26.01 | 47,200,607 | 45.01 |

`OpenAI` stringified arguments are decoded and parsed again, and an element
that starts with `"id"` becomes a call attempt only at its `"type":
"function"` or `"function"` member (`commit`, so an Anthropic message's
leading `"id"` stays content); a program with commit points checks each live
configuration for them, about 2 work per byte. The element's function object
`{"name": ..., "arguments": "..."}` is also a bare JSON call, so the
`bare-json.v1` dialect reads and decodes the same stringified arguments
until the element's call covers it (9 of the 26 work per byte); in the same
way it reads the call object inside a Hermes `<tool_call>` or a Mistral v3
array (7 of their 17). The recognizer releases the scratch of a reading it
drops unread (an attempt that began inside another attempt's committed
span) as soon as it drops it, and the engine's history arena is released
when the recognizer finishes, so such a second reading adds little to the
peak.
An attempt that opens inside another attempt's committed span, where no
reading of the turn can take it (a Harmony message's second opener at its
`<|channel|>`, a DeepSeek-V3.1 attempt inside a DeepSeek-V3 section), is
abandoned as soon as the turn level sees it open, so it is not read to the
end. A wrapped Qwen3-Coder XML call is read twice: under the default
allow-list Hermes also attempts the `<tool_call>` and rejects it at the
`<function=` body, and in that segmentation the bare function element is a
call attempt of its own, kept until the wrapped call ends (6 of its 17 work
per byte). All use the
default request of that capture (the twelve built-in dialects then available); charging schema
validation's working set left these measurements unchanged, because
validating one string argument peaks below the recognizer's buffers. The
constants leave headroom over these fast-path measurements; inputs that need repair
search spend from the preset part of the budget, and exhaustion is reported
as `BudgetExceeded` naming the resource.

Repeated parameters are covered by the same linear bound. A declared array
parameter (or a `$ref` or `anyOf` scope) written as k repeated
`<parameter=...>` occurrences costs `work` and `total_scratch_charged` linear
in the output: resolution appends each item to the partial array in place
when the item has one reading, and copies (and charges the copy of) the
array only for an item with several readings, which genuinely multiplies the
readings (`Candidates` bounds those). Before this, each item copied and
charged the growing partial array, so k items charged O(k²) scratch
(measured on the previous head: 13.99 GB at k = 8,000 nested items, against
1.54 GB now) and took quadratic wall time (9.0 s against 1.8 s).
`tool_call_cost::repeated_parameters_cost_is_linear` recovers 1,000 to 8,000
occurrences as nested items, as one-item JSON arrays and as string items,
and requires both counters to track the byte ratio within 1.05x. At k =
8,000 the nested form charges 29.2 million work (59 per byte), 1.54 GB of
total scratch and peaks at 130 MB (262 per byte), the JSON form 52 work and
104 peak scratch per byte, and the string form 61 and 244. The test recovers
every k under plain `sized(Effort::Standard, output)`, with no raised limit,
and asserts `RECOVER_WORK_PER_BYTE` and `RECOVER_SCRATCH_PER_BYTE` as
ceilings.

### Dense markup

Output that is nothing but one small unit repeated (many tiny calls, many
repeated parameters) costs more per byte than a long string argument: 53 to
142 work and 80 to 302 peak scratch per byte, against 10 to 26 and 8 to 45.
Before the constants were raised (32 work and 48 scratch per byte), plain
`sized(Effort::Standard, output)` exhausted explicitly on such output:
`BudgetExceeded(Work)` for every shape at 1 MiB but DeepSeek-V3, which ran
out of `ScratchBytes` first. With work and scratch raised alone, the shapes
that record more than 65,536 evidence entries per MiB (0.0625 per byte:
Qwen3-Coder XML nested items, string items and calls, `OpenAI`, Mistral v11
and Anthropic) would still have run out of the preset's flat
`EvidenceEntries`.
`tool_call_scale::
one_mib_dense_markup_fits_published_budget` (`make corpus`) now recovers each of these shapes at 1 MiB under plain
`sized(Effort::Standard, output)` with the stop `Completed` and every unit
recovered; `tool_call_cost::dense_markup_fits_published_budget` (`make
check`) runs the same shapes at 64 KiB. The Gemma row uses the current
fifteen-dialect request. The Pythonic row uses the fourteen-dialect request. The GLM row uses the thirteen-dialect request captured before Pythonic;
the other recorded rows use the twelve-dialect request before GLM.

| Dense shape (1 MiB) | Units | Work per byte | Peak scratch per byte | Evidence per byte |
| --- | ---: | ---: | ---: | ---: |
| Gemma calls | 29,127 | 48.5 | 226.5 | 0.028 |
| Pythonic calls | 49,932 | 69.2 | 380.2 | 0.048 |
| GLM 4.5 calls | 13,107 | 106.0 | 88.8 | 0.012 |
| Qwen3-Coder XML repeated nested items | 16,911 | 59.4 | 268.3 | 0.065 |
| Qwen3-Coder XML repeated JSON-array items | 30,838 | 52.9 | 103.6 | 0.000 |
| Qwen3-Coder XML repeated string items | 34,950 | 62.1 | 250.5 | 0.067 |
| Qwen3-Coder XML calls | 11,781 | 72.4 | 281.6 | 0.101 |
| Hermes calls | 14,169 | 82.4 | 114.0 | 0.014 |
| `OpenAI` stringified calls | 10,485 | 72.0 | 183.0 | 0.070 |
| Mistral v3 calls (one section each) | 12,633 | 71.9 | 160.6 | 0.060 |
| Mistral v3 calls (one `[TOOL_CALLS] [..]` array) | 15,195 | 79.6 | 186.0 | 0.072 |
| Mistral v11 calls (`</s>` after each) | 16,912 | 58.6 | 203.7 | 0.081 |
| Mistral v11 calls (chained, a newline after each) | 18,078 | 67.8 | 220.8 | 0.086 |
| Harmony calls | 9,446 | 118.9 | 80.1 | 0.009 |
| DeepSeek-V3.1 calls | 11,522 | 73.0 | 98.7 | 0.011 |
| DeepSeek-V3 calls | 9,361 | 82.3 | 301.6 | 0.045 |
| Kimi-K2 calls | 10,809 | 142.3 | 131.4 | 0.052 |
| Llama 3 JSON calls | 20,971 | 76.2 | 150.1 | 0.020 |
| Anthropic `tool_use` calls | 13,981 | 56.5 | 170.0 | 0.067 |
| Bare JSON calls with empty arguments | 34,952 | 101.4 | 165.6 | 0.033 |
| Qwen3-Coder XML 1 MiB string (above) | 1 | 17.0 | 11.1 | 0.000 |

These are measured ratios, not per-input upper bounds. Fixed storage and
capacity growth can change them at other input lengths. The supplied
64 KiB and 1 MiB shapes complete under the sized budgets; larger or
schema-heavy inputs can still exhaust explicitly. For the dense Pythonic
case, scratch is closer to its budget than Work or evidence storage.

The fourteen-dialect default uses `RECOVER_WORK_PER_BYTE` = 224. In the
current implementation, adding Pythonic to the prepared default set changes
the existing 300-parameter nullable-integer Qwen schema-typing case from
3,039,693 to 3,162,209 Work for 10,032 bytes. The Standard preset plus
224 Work per byte admits 3,247,168 Work, preserving that case's complete
result with 84,959 units remaining. This measured recipe neither bounds
arbitrary schema complexity nor claims a marginal bound of 16 Work per
input byte. Explicit caller budgets and effort presets retain their values.

`RECOVER_SCRATCH_PER_BYTE` remains 368. The fixed 64 MiB allowance covers
the measured Pythonic peak of 380.2 scratch bytes per input byte through
the supplied 1 MiB case; this does not qualify larger Pythonic inputs.
`RECOVER_EVIDENCE_BYTES_PER_ENTRY` remains 7. A 1 MiB output carries a
budget of about 236 million Work, 432 MiB of scratch and 215,000 evidence
entries at `Effort::Standard`. The larger Work allowance also permits more
computation before pathological input exhausts.

Restarted DeepSeek-V3.1 sections (issue #10) now normalize bracket depth
for multi-byte resynchronization targets. These markers match at any depth;
quote, escape, colon and partial-marker state remain unchanged. The existing
configuration merge can therefore combine equivalent scans rather than keeping
one depth-distinguished scan per restarted section. Each attempt retains only
its latest ignored opener because the current opener position increases
monotonically; sibling targets still share that record.

The exact issue reproduction is included in the existing dense-markup cases at
64 KiB and 1 MiB. `restarted_deepseek_sections_cost_is_linear` checks Work,
total scratch and peak scratch against input growth from 16 through 1,024
sections, with the existing 1.05 tolerance and standard sized budgets. Growth
is measured above the first sample to account for fixed setup and the closing
tail. These
current-source execution results remain pending automatic CI; no new measured
counters or timing result is claimed here.

Historical work-heavy shapes under the twelve-dialect sized recipe include
the following explicit exhaustions. These measurements do not assert their
current counters or termination under the fourteen-dialect recipe:

- Text with no call in it that opens many attempts: a megabyte of `{` charges
  895 work per byte (every `{` may open a Llama 3 JSON, Anthropic `tool_use`
  or bare JSON attempt, and each stays live), and a megabyte of `{"a":`
  195. Both are linear in `n`, and both end `BudgetExceeded(Work)`.
- A mutated Qwen3-Coder XML turn calling a tool whose array items are
  free-form objects (issue #11): the repair search spends about 240 to 280 ns
  per charged work unit, against 40 to 80 ns for ordinary recovery. Charged
  work therefore under-counts its real work several times over. Under plain
  `sized` the 192-byte reproduction ends `BudgetExceeded(Work)` in 0.3 s; at
  `Effort::Maximum` it completes in 1.65 s (6.9 million work). The fix is
  tracked in issue #11.

Consecutive Mistral v11 calls with nothing but whitespace between them (or
nothing at all) are one `next`-chained attempt. Each call extends the
segmentation of the calls before it, reading only itself, so work, scratch
and wall time are linear: the "Mistral v11 calls (chained)" row above, and
`tool_call_cost::chained_mistral_v11_calls_cost_is_linear` (256 to 4,096
calls, three separators). Before this, the segmentation was re-run at each
call, and work and peak scratch per byte grew with the number of calls (141
and 2,009 at 409 calls, 1,139 and 27,354 at 6,553), so `sized` exhausted.

The chained segmentation covers every history whose alternative readings
lie within one call. A history whose equal-cost readings reach different
call boundaries, or differ in whether a call was skipped, is segmented
whole, as before, at the completion that reaches it; the work is charged
(per history node, per call read) like any segmentation. No Mistral v11
chain tried reached this path. DeepSeek-V3.1 sections restarted after a
truncated call cover it in
`tool_call_cost::whole_segmentation_fallback_is_correct_and_charged`.
Saved computation traces for issue #10 attributed its quadratic growth to
resynchronization scans and retained-attempt visits, rather than this fallback.

A Mistral v3 `[TOOL_CALLS] [..]` array of thousands of calls holds one
literal failure per element (each element's closing `}` or `]` meets the
`,` that follows it, and a sibling reading takes the comma), kept as a
repair seed while any configuration of the array's attempt lives. They are
indexed by the position they began at, grouped by attempt, so a byte
examines only the failures that its step makes droppable (none while the
attempt's earliest live position is unchanged) and every examination is
charged as `Work`. Before, each byte re-examined every held failure without
charging it: the work charged stayed linear (73.5 per byte) while the wall
time was quadratic (release build: 0.108 s at 256 calls, 1.26 s at 1,024,
4.46 s at 2,048, and no finish at 1 MiB within 300 s). Now the work is 79.6 per
byte from 256 calls to 1 MiB and 1 MiB (15,195 calls) recovers in about 6 s
(release build): the "Mistral v3 calls (one `[TOOL_CALLS] [..]` array)" row
above, and `tool_call_cost::mistral_v3_call_arrays_cost_is_linear` (1,024 to
16,384 calls: work, scratch ever charged and peak scratch within 1.05x of the
byte ratio), with a unit test of the held failures' per-step work in
`grammar::engine`.

## Tool-call recovery throughput (illustrative)

`crates/core/benches/tool_calls.rs` (`cargo bench -p purrassist-core --bench
tool_calls`; `-- --list` lists its workloads, `-- NAME` runs one) recovers a
write call with a 1 KiB, 64 KiB or 1 MiB string argument as Qwen3-Coder XML,
Hermes, `OpenAI` stringified arguments, Llama 3 JSON, Mistral v3, Mistral
v11, DeepSeek-V3.1, DeepSeek-V3, Kimi-K2, Anthropic `tool_use`, Harmony,
bare JSON, and a Qwen3-Coder XML call followed by a Hermes call under a Qwen3-Coder + Hermes allow-list, each under
`ToolRecoveryRequest::sized_for(Effort::Standard, output, dialects)`, and feeds a 64 KiB
Qwen3-Coder XML output to a `Recognizer` in chunks of 1, 16 and 4096 bytes,
asserting identical charged usage for every chunking. Work and peak scratch
per byte are functions of the input alone; the wall-clock columns are
**illustrative only**: one run on one machine, not a qualification
threshold. This bench is deliberately not one of the pinned
`LATENCY_CASES`/`RECOVERY_CASES`/`PROFILING_CASES` sets of
`scripts/check-resource-limits.py`, so no committed report or limit depends
on its timings.

Recorded on Linux 7.2.7-1-cachyos x86_64 (AMD RYZEN AI MAX+ 395 w/ Radeon
8060S, 32 hardware threads), `rustc 1.100.0-nightly (4b6d04e70 2026-09-13)`,
commit `dd95ec8` with this change applied:

| Workload | Bytes | Samples | Median ms | MiB/s | Work/byte | Peak scratch/byte |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `qwen_xml_1kib` | 1111 | 200 | 0.537 | 1.97 | 21.98 | 108.94 |
| `qwen_xml_64kib` | 65623 | 30 | 25.895 | 2.42 | 18.83 | 12.66 |
| `qwen_xml_1mib` | 1048663 | 7 | 405.965 | 2.46 | 18.78 | 11.10 |
| `hermes_1kib` | 1096 | 200 | 0.800 | 1.31 | 24.35 | 125.31 |
| `hermes_64kib` | 65608 | 30 | 41.839 | 1.50 | 18.87 | 12.74 |
| `hermes_1mib` | 1048648 | 7 | 670.350 | 1.49 | 18.78 | 11.11 |
| `openai_stringified_1kib` | 1127 | 200 | 0.933 | 1.15 | 34.17 | 126.54 |
| `openai_stringified_64kib` | 65639 | 30 | 49.424 | 1.27 | 27.89 | 45.15 |
| `openai_stringified_1mib` | 1048679 | 7 | 782.243 | 1.28 | 27.78 | 45.01 |
| `llama3_json_1kib` | 1072 | 200 | 0.508 | 2.01 | 18.58 | 118.48 |
| `llama3_json_64kib` | 65584 | 30 | 26.325 | 2.38 | 13.86 | 8.57 |
| `llama3_json_1mib` | 1048624 | 7 | 421.964 | 2.37 | 13.78 | 8.01 |
| `mistral_v3_1kib` | 1105 | 200 | 0.805 | 1.31 | 24.46 | 122.59 |
| `mistral_v3_64kib` | 65617 | 30 | 42.170 | 1.48 | 18.87 | 12.71 |
| `mistral_v3_1mib` | 1048657 | 7 | 674.341 | 1.48 | 18.78 | 11.11 |
| `mistral_v11_1kib` | 1080 | 200 | 0.472 | 2.18 | 15.17 | 119.08 |
| `mistral_v11_64kib` | 65592 | 30 | 24.684 | 2.53 | 11.83 | 8.61 |
| `mistral_v11_1mib` | 1048632 | 7 | 394.859 | 2.53 | 11.78 | 8.01 |
| `deepseek_v31_1kib` | 1168 | 200 | 0.603 | 1.85 | 17.34 | 113.30 |
| `deepseek_v31_64kib` | 65680 | 30 | 25.166 | 2.49 | 11.88 | 8.66 |
| `deepseek_v31_1mib` | 1048720 | 7 | 389.356 | 2.57 | 11.78 | 8.01 |
| `deepseek_v3_1kib` | 1189 | 200 | 0.911 | 1.24 | 24.02 | 139.46 |
| `deepseek_v3_64kib` | 65701 | 30 | 40.588 | 1.54 | 15.93 | 9.16 |
| `deepseek_v3_1mib` | 1048741 | 7 | 643.825 | 1.55 | 15.79 | 8.01 |
| `kimi_k2_1kib` | 1174 | 200 | 0.731 | 1.53 | 29.47 | 134.66 |
| `kimi_k2_64kib` | 65686 | 30 | 24.533 | 2.55 | 12.09 | 9.03 |
| `kimi_k2_1mib` | 1048726 | 7 | 382.674 | 2.61 | 11.80 | 8.01 |
| `anthropic_tool_use_1kib` | 1097 | 200 | 0.493 | 2.12 | 18.07 | 110.87 |
| `anthropic_tool_use_64kib` | 65609 | 30 | 25.379 | 2.47 | 13.85 | 8.49 |
| `anthropic_tool_use_1mib` | 1048649 | 7 | 431.944 | 2.32 | 13.78 | 8.01 |
| `harmony_1kib` | 1134 | 200 | 0.656 | 1.65 | 27.06 | 134.43 |
| `harmony_64kib` | 65646 | 30 | 25.628 | 2.44 | 14.01 | 8.96 |
| `harmony_1mib` | 1048686 | 7 | 399.391 | 2.50 | 13.79 | 8.01 |
| `bare_json_1kib` | 1071 | 200 | 0.514 | 1.99 | 18.59 | 117.99 |
| `bare_json_64kib` | 65583 | 30 | 27.293 | 2.29 | 13.86 | 8.57 |
| `bare_json_1mib` | 1048623 | 7 | 431.933 | 2.32 | 13.78 | 8.01 |
| `mixed_qwen_hermes_1kib` | 1184 | 200 | 0.396 | 2.85 | 15.34 | 36.97 |
| `mixed_qwen_hermes_64kib` | 65696 | 30 | 18.475 | 3.39 | 13.53 | 9.02 |
| `mixed_qwen_hermes_1mib` | 1048736 | 7 | 293.477 | 3.41 | 13.50 | 8.53 |
| `push_chunk_1` | 65623 | 30 | 15.026 | 4.16 | 8.01 | 5.21 |
| `push_chunk_16` | 65623 | 30 | 14.716 | 4.25 | 8.01 | 5.21 |
| `push_chunk_4096` | 65623 | 30 | 14.713 | 4.25 | 8.01 | 5.21 |

## Resource limits

`scripts/check-resource-limits.py <report.json>` checks a resource report
(freshly generated, or the committed baseline above) against the table
below, extracted from the first ```` ```json ```` fence following this
heading. Every limit is set with headroom over an actually measured value
recorded above (never below one); a limit failing here means measured
resource use grew past a deliberately generous ceiling, not that it
regressed by any amount. `make hygiene` uses `--schema-only` against the *committed*
`qualification/resources-linux-x86_64.json` (complete inventories, valid types,
digests, uncached-build evidence and correctness observations); the CI `qualification` job runs `make resources` fresh and
checks that output instead.

Rationale per category:

- `cold_build_elapsed_ns`: 180 seconds. This limits the uncached release
  CLI compilation and link measured with empty build directories; 180s is a
  generous ceiling for an actually cold compile of this workspace's six
  publishable crates and their dependencies on ordinary CI hardware.
- `latency_workloads`/`syntax_latency_workloads`: core median/p95 ceilings
  are 10/15 milliseconds; syntax median/p95 ceilings are 3/4 milliseconds.
  These provide headroom over the recorded workloads while detecting large
  slowdowns. Tail latency includes scheduling interference on a loaded
  machine; these ceilings are qualification thresholds, not a guarantee
  that every host will pass them.
- `workloads` (the existing Memcheck/Massif/RSS fields): roughly 8-10x the
  largest measured workload in the recorded baseline table above, wide
  enough to tolerate the allocator/libc/loader differences the "Loader
  compatibility" section above describes across hosts.

```json
{
  "cold_build_elapsed_ns": 180000000000,
  "latency_workloads": {
    "median_ns": 10000000,
    "p95_ns": 15000000,
    "allocation_calls": 150000,
    "peak_live_bytes": 1000000,
    "process_peak_rss_bytes": 50000000
  },
  "syntax_latency_workloads": {
    "median_ns": 3000000,
    "p95_ns": 4000000,
    "retained_bytes": 1000000,
    "peak_bytes": 1000000,
    "max_overshoot_bytes": 150000,
    "tree_allocation_calls": 150000,
    "rust_allocation_calls": 150000,
    "rust_peak_live_bytes": 1000000,
    "process_peak_rss_bytes": 50000000
  },
  "workloads": {
    "process_peak_rss_bytes": 50000000,
    "total_allocated_bytes": 8000000,
    "observed_peak_heap_plus_estimated_overhead_bytes": 2000000
  }
}
```


## Stage 2 repair measurements

The normal project `make resources RESOURCE_ARGS=--portable-runtime` target
completed after the Stage 2 repairs. The
[Stage 2 resource report](qualification/resources-stage2-linux-x86_64.json)
and [recovery benchmark](qualification/recovery-bench-stage2-linux-x86_64.json)
retain the observed source, tool, runtime and executable identities. Earlier
baselines remain byte-identical. No caller environment overrides or alternate
Cargo launchers were used; profiling and cold-build isolation follow the existing
measurement framework.

The report records commit `66b3756776a072af1889596ae98a5faa5a50cea6` with a dirty documentation
tree and guarded engine SHA-256
`132196375955100e6f289a314d3f51cb1f41bf9e8d2d530fb5f1926f95d96a83`. Its nine profiles,
25 core latency cases, five syntax cases and six recovery cases passed the
documented limits. Strict native, Memcheck and Massif
startup/output checks completed, including every workload's correctness checks.

The framework's cold CLI build took 21.671 seconds, built
35 artifacts and reused 0. Its executable has SHA-256 `f6621c50e03cc55e0cbf24fe715052eb4b9ebdfc19cfd24a0a230ffbd9be3190`. This is an
observation of the recorded machine and build conditions, not a speedup claim.

## Governed JSON emission

`json::Node::to_json_governed` and `json::quote_governed` share the existing
serializer's structural cursor and escaping rules. They charge each cursor
transition, string scan and emitted byte before work; value depth is checked
before descent. Root depth is zero and object keys add no value level. A
fallible `ScratchVec` stores only the active ancestor chain, and `ScratchString`
admits actual output capacity growth before allocation. Returned string capacity
remains charged; failure drops temporary storage and exposes no partial success.
The supplied DOM or input string is caller-owned, outside emission's governance.

Existing convenience and append callers use the same cursor/escaping semantics
with recursive scheduling and no new heap cursor collection. This preserves
their bytes and append behavior; it does not certify their historical
`estimated_bytes` pre-admission assumptions or silently turn them into governed
operations. No heap-footprint estimate is an output-capacity proof for the new
APIs, especially when JSON escaping expands control characters.

## Context fragment and numeric primitives

`ContextManagement::decode_mcp` takes an immutable caller-owned JSON DOM. Its
encoder constructs a governed DOM and delegates JSON output to the existing
shared writer. Both require the explicit enclosing v2 format and check logical
value depth: root zero, field values one, tag items two. Closed-field searches,
comparisons, masks, quantities, strings and retained collection growth are
admitted before work/storage. No full-manifest or raw-parser governance claim
follows from this fragment codec.

`Number::integer_magnitude` and its governed variant share one allocation-free
reader. It preserves exact integer/fractional classification, negative zero and
arbitrary exponent semantics. Exponent cancellation is bounded by the actual mantissa's
length before an exponent is classified as beyond the representable range; no
exponent-sized storage is created. The governed variant admits every bounded
scan before executing it. Numeric lexical retention shares validation with
`FromStr` and uses fallible scratch storage. The actual JSON parser's numeric
copy uses the same retained-source helper without re-admitting its source input.
Other parser paths remain outside this delivery's governance claims.
JSON recovery receipts include the admitted numeric validation/copy work and
retained numeric capacity in their existing usage counters.

`Number::is_multiple_of` uses the same admitted decimal normalization and signed
arithmetic as numeric comparison. Coefficients, exponent differences, factor
quotients and remainders retain fallible temporary owners. Decimal character
appends admit relocation before buffer growth. Success and refusal release all
arithmetic storage; only a completed zero-divisor diagnostic retains its output
reservation after the final governor check. Witness numeric type filtering and
bound checks reuse the governed integer reader and comparator. Huge exponents
remain exact without exponent-sized allocation; resource refusal remains distinct
from a false assertion or an unsatisfiability result. General format computation
and recovery candidate/search ownership remain separate contracts.

`format_output_governed` admits emitted-byte work and actual output capacity,
preserving the first writer failure even if a formatter ignores it. It does not
govern arbitrary formatter computation. The numeric constructor pre-admits the
standard unsigned formatter's bounded digit work; `FieldMask::render_governed`
admits its member traversal and uses its original display implementation. Tags
are copied fallibly without normalization, deduplication or reordering. Returned
owned buffers retain their core scratch charges; counters can conservatively
retain admitted storage after owned intermediate values are dropped.

## Governed AppArmor glob words

`AppArmorGlob::parse` admits input, caller declaration names, work and logical
group/class depth before use. `ScratchString` retains original input and declared
names; `ScratchVec` stores a flat semantic arena and an iterative brace stack.
Tokens charge a portable 48 bytes, declaration spans and brace frames 16 bytes,
with compile-time layout assertions. Literal runs are coalesced; alternatives
and variables are never expanded. Storage grows linearly with source and
declarations, including deeply nested inputs, and destruction never recurses.
Declaration uniqueness and binding-name lookup use explicitly charged linear
comparisons; declaration admission can be quadratic in declaration count.

Policy-word rendering and literal binding admit actual output capacity before
allocation and work before copies, validation or comparisons. Returned strings
and model storage remain charged; failures expose no partial successful output.
Host canonicalization and variable trust are caller-owned. No filesystem,
environment, clock or kernel access occurs in these library operations.

The byte grammar and contextual escape refusals are documented on `AppArmorGlob`.
Policy quoting preserves wildcards rather than literalizing them. Literal host
paths containing glob/variable metacharacters, quotes, backslashes or controls
are refused, never interpolated as new syntax. This parser/printer does not
establish matching, containment, admission or kernel enforcement.

## Exact byte-language containment

`pattern::ByteLanguage` directly compiles postfix instructions through the
existing derivative term/class arena and actual-capacity retained storage.
It does not invoke the Unicode regex parser or its syntax AST.
Leaf expression depth is zero; star and positive n-ary concatenation/alternation
add one plus their deepest child. Postfix stack width and the internal binary
term representation do not define logical expression depth.

Byte compilation and sessions opt into admitted fixed-size map comparisons,
potential shifted entries and class lookups through a temporary arena adapter.
No flag widens retained Unicode/session layouts or changes their established
work schedule. Flat full-byte matching never decodes UTF-8, rejoins a search root
or makes earlier acceptance sticky. Original Unicode behavior remains unchanged.

Inclusion explores the product of the two existing derivative sessions over
joint byte-class cut points. Visited pairs, session caches and predecessor nodes
use fallible admitted storage. One representative per interval suffices because
all predicates in both programs are constant throughout that interval. The
canonical dead left term is pruned because none of its continuations can witness
noninclusion. Only an exhausted reachable frontier proves containment. A witness
is reconstructed fallibly as bytes in shortest-length, ascending-byte order;
depth bounds each discovered prefix length. Work, storage, depth and cancellation
refusal return errors, never containment or an unsupported impossibility claim.

`GlobLanguage` lowers the admitted `AppArmorGlob` tokens directly and shares
literal-binding preparation with its policy-word printer. No brace products,
rendered regex text or reparsed policy words are introduced. Its filename
alphabet excludes NUL; explicit byte classes may contain slash. Source models
are caller-owned. Permission sets, owner/deny semantics and complete hat
admission are not established by this standalone language proof.

## Wire declaration output

`CompiledTool::render_declaration_governed` writes the existing `OpenAi`, `Mcp`
and `Anthropic` forms through the shared JSON output sink. The sink uses one
`ScratchString`: it admits each emitted segment's byte length as work before
copying it, and admits actual capacity growth before allocation. Quoted name and
description values additionally admit their input byte length before the shared
escaping scan. Original same-form declaration bytes and cross-form embedded
schema text are copied verbatim without parsing or normalization. A final check
precedes complete-output publication; failures preserve the original cause and
publish no partial string.

Caller-owned source/tool preparation is separate. No intermediate quoted or
concatenated strings are created by this wire-output path. Prompt-dialect
printing has the separate prepared-operation contract below; cold built-in
grammar preparation remains caller-owned.

## Prepared canonical grammar printing

`grammar::print::canonical_governed` accepts a compiled grammar, one root and
borrowed `Values`/group bindings. It admits each original text and recursive
parameter value under this governor before walking. `InputBytes` checks each
individual input size. Expression and metadata visits charge `K_PRINT` work and
check depth; all traversals share a fixed aggregate ceiling of 128 in addition
to the caller's depth budget. Crossing either limit returns the original sticky
`BudgetExceeded(Depth)` cause.

Borrowed choice selections and group storage, JSON serialization/quoting,
delimiter guards, ordinal formatting, composite ids, diagnostics, generated
class members and final output use admitted fallible storage. Transient owners
release their exact reservations on failure or after copying; only published
output or an escaping diagnostic retains its reservation. Cancellation and
resource failures publish no partial output. Internal emitter probes retain
scratch ownership until final publication.

Uncaptured classes use the retained automaton through the existing derivative
BFS. Length bounds count Unicode scalars: attempts use the requested minimum,
then zero, each with a maximum sixteen scalars larger. Empty candidates remain
rejected; bounded failure remains `PrintError::NoCandidate`.

Cold grammar/parser/automaton preparation, existing higher-level emitter request
preparation and seeded/witness value generation remain outside this operation.
The shared seeded walker preserves its existing random consumption order; this
does not establish complete governed witness generation or close #36.

## Standalone file fragments

`FilePermissions`, `FileExecution` and `FileTarget` borrow their source or store
bounded typed values. Parsing admits the field input and an eight-times-byte
plus eight work bound covering one scan and the closed spelling comparisons.
Overflow is an explicit work refusal. Permission output performs exactly six
bounded tests, pre-admitted separately from emitted-byte work.

`FileRule::render` admits its source identity and rule checks, and checks each
borrowed path/target input against this governor even if preparation used a
different governor. Input admission checks the maximum individual input size,
not a cumulative byte sum. It then reuses the existing complete binding validator before inspecting the parsed first unit.
Only an unescaped literal slash or an absolute first-variable binding admits
the path. That prefix proves every branch absolute without building a language
or scanning a rendered word. The existing `bind_literal_paths` operation then
validates and renders the exact word. This deliberately reuses the public path,
including its retained intermediate-string charge. No second escaping,
substitution or matching implementation is introduced.

The shared `format_output_governed` sink admits final output copies and actual
capacity growth. A final check precedes artifact publication; semantic and
resource refusals expose no partial fragment. Source globs/bindings and targets
remain caller-owned; parsing or constructing those inputs is a separate
admission. The artifact preserves their identities and the denied-execution
canonicalization without asserting waiver, graph or host authority.

## Standalone signal and ptrace fragments

IPC set parsing admits each complete original field and a checked work bound
before splitting or comparing names. The bound covers the actual closed table
plus ASCII splitting/trimming and bounded RT digit scans. Each set fits a fixed
`u128`; no list buffer, ambient signal-number query or expansion is used.
Leading zeros are removed from a borrowed slice before a maximum two-digit
standard integer parse. Signals outside the declared 0–32 RT range are refused.

Rendering rechecks the current governor's individual input ceiling for borrowed
peer and original signal-set text, even when preparation used another governor.
Canonical set traversal has a fixed, pre-admitted table cost. The existing
policy `emit` function admits each byte write and fallible `ScratchString`
growth; peer bindings and quoted output use `append_bound_policy_word` directly.
There is no intermediate word copy or second escaping/matching implementation.
The private partial buffer is discarded on refusal and a final cancellation
check precedes complete artifact publication. Source/field/peer identities and
original binding offsets or governor causes remain available on typed errors.
Original source preparation and actual host mediation are separate contracts.

## Standalone exact profile resource limits

`ResourceLimits::render` admits the original source identity against the current
input ceiling, then reserves 512 work units before bounded numeric computation:
five validations, two possible exact byte divisions and five u64 decimal
displays of at most twenty digits each. Fixed-size caller numbers require no
ambient queries, string parsing or dynamic intermediate storage. All five
values are validated before any output allocation. Byte values above the bare
decimal range require exact divisibility by 1024; the quotient is emitted with
`K`. Counts and whole CPU seconds have explicit selected-parser ceilings.

The existing `format_output_governed` admits each actual byte write and fallible
scratch growth. A final cancellation check precedes atomic publication of all
five declarations. Exact input/work/scratch ceilings pass; one-below ceilings
and pre-cancellation return their original causes with all input numbers and
source identity retained. Private partial storage is dropped on refusal.
No native parser runs in production. Parser acceptance can mask saturation or
overflow, so qualification checks serialized values rather than exit status
alone. These are selected representation limits; host enforcement and graph/
policy admission remain separate requirements.

## Exact framed prompt bytes

`FramedPrompt::assemble_bytes` accepts explicit prefix, separator and suffix
text, ordered borrowed `PromptBytePart` occurrences, an opaque source identity,
a semantic byte ceiling and one core governor. It emits the prefix, every
original declaration separated exactly once, then the suffix. An empty list
still emits both framing strings. It preserves order, duplicate identities,
whitespace, Unicode and escaping; it performs no normalization or omission.

Each original framing, identity and declaration string admits its individual
`InputBytes` length and constant sizing work before processing. Declarations'
byte lengths, every separator occurrence and framing totals use checked integer
arithmetic. Overflow is a typed sizing refusal. The independent ceiling compares
the exact full requirement before output allocation; zero admits only a completely
empty representation. A ceiling error retains both required and allowed bytes
without exhausting an otherwise usable governor.

Output-copy work is admitted before allocation/copying. One `ScratchString`
reserves the complete output fallibly, and per-part work/checks remain under the
same governor. Resource refusals release unpublished output and preserve the
native cause. A final cancellation check precedes complete String publication;
no partial prompt or successful measurement escapes on failure. Both outcomes
retain the original borrowed source, named parts and framing.

The byte-only operation retains that contract. `assemble_accounted` adds a
separate token ceiling through the same assembler. The exact mode calls a
caller-supplied deterministic `TokenCounter` once over the complete assembled
bytes, after byte-ceiling admission. Original tokenizer and configuration labels
are captured before the call; empty identities fail without normalization.
Callbacks must be pure, use the supplied governor for work/storage and count the
entire named configuration, including implicit tokens. Applicability and callback
correctness remain caller obligations, not library qualification.

The conservative mode records a `ByteLevelTokenContract`: the caller asserts at
most one token per input byte, no uncounted implicit special tokens, and no
representation-expanding preprocessing, with original encoding identity and
admission basis. It bounds tokens by exact complete UTF-8 bytes, including all
framing. It never labels that bound an exact count. Missing accounting and
unavailable counters refuse explicitly; neither mode silently selects the other.

Accounted output stays in its `ScratchString` through counting, token-ceiling
admission and the final governor check. A callback cannot conceal a stopped
governor by returning success. Failures release unpublished storage and retain
the original inputs, original accounting labels once captured, and any completed
measurement. Successful `AccountedPrompt` exposes immutable bytes alongside the
separate exact-byte and token quantities. Neither quantity relaxes the other's
ceiling. The wire-declaration consumer uses an explicit scalar encoding to show
an exact count below its byte count and a conservative refusal at the same token
ceiling; this example does not qualify a model tokenizer.

Caller preparation, actual model-family placement/selection, qualified tokenizer
evidence, graph projection, routing and complete lane ledgers remain under #36
and #47. Tool-result `OutputByteLimit` remains its separate contract.

## Governed dense delimiter and marker matching

The existing dense byte `Matcher` owns transitions, depths and matches in
fallible scratch vectors. Construction borrows admitted literals, reuses the
fallible sparse trie as its sole goto/prefix authority, checks literal/state
IDs and row sizes, and admits zero initialization, parent/child scans, binary
searches, fallback processing and queue growth before work. Temporary trie,
failure links and breadth-first queue release on every exit. A final check
precedes the complete matcher; its final buffers release with their grammar or
prepared dialect set. Byte-prefix length is iterative and has no 128-byte
recursion ceiling. Duplicate labels, earliest-ending matches and longest-match
priority keep their original meaning.

`DialectSet` and `Matcher` no longer deep-clone. The cached built-in set returns
a static borrow; custom sets remain owned by the caller. `ToolRecoveryRequest`
contains owned cloneable policy/budget settings, while `ToolSet::recover` and
`recover_governed` take the prepared set explicitly. Sizing can borrow the same
set through `sized_for`. The host retains custom preparation through recovery
under its original shared governor; CLI/MCP selectors and outcome/provenance
contracts are unchanged. Derived call metadata and compiler queries also retain
governed owners and admitted scans. Dialect-set metadata also uses governed
retained owners. Engine/recovery runtime allocation governance remains outside these completed compiler phases under parent #36.


## Prepared dialect metadata admission

`DialectSet::new` admits collection, canonical `(id, digest)` comparisons,
duplicate diagnostics, marker-byte copies, lexical ordering/deduplication and
explicit-marker scans before processing. Dialects, marker pairs and flags retain
fallible scratch owners through errors and set lifetime; temporary storage
releases on refusal. A returned duplicate diagnostic retains its admitted
String charge. Shared pairs are explicit
only when every declaration is explicit. Fallible borrowed byte-view handles feed
the existing matcher in flattened opener/closer order without cloning marker
buffers. Temporary handles end before the completed set moves its owners.

Lookbehind arithmetic and the aggregate configuration bound are checked before
publication, followed by cancellation admission. `reasoning_markers` borrows the
same read-only byte owners. Existing recognizer runtime copies remain explicit.
Caller-prepared custom grammar/Arc construction and the input iterator belong to
the caller; lazy built-in grammar/cache preparation keeps its own default
governor. Engine/recovery runtime governance and full
#36 witness, GBNF, signature, projection, routing, ledger and graph integration
remain separate required capabilities. No set-owned disallowed-signature table
exists: detection filters static family rules at runtime through its allow-list.


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

The complete ABI/profile/hat document and source spans publish together after
final cancellation admission. New indices, text and spans use fallible scratch
owners. Existing String-returning renderers retain their documented conservative
reservations; this compiler does not claim prompt release of their detached
intermediate charges. Original input and exact native rule/resource errors
remain available on refusal. ABI 3.0 refuses fine-grained Internet rules instead
of dropping scope. Aggregated ABI/mediation metadata describes an offline target,
not running-kernel, loaded-policy or dispatch evidence.

The same compiler also emits mandatory abstraction imports from ordered
`PolicyImport` occurrences and opaque `PolicyDependency` files. Paths must be
normalized relative `abstractions/...` files; traversal, variables, wildcards,
include delimiters and control bytes are refused. Every direct import must bind
to one declared file. Duplicate dependency paths, including unused transitive
records and identical contents, are a typed conflict preserving both records.
Repeated import occurrences with distinct source identities remain in caller
order, with exact profile/hat spans and manifest indices.

Dependency bytes remain separate external files. Governed SHA-256 computation
records their exact identities; the library never reads the filesystem, parses
imported permissions, or concatenates those bytes into a profile body. All
original declared records remain available, including opaque transitive files.
Their presence does not prove that the caller supplied a complete import closure.
`local_mediation()` describes local typed rules only. `complete_mediation()`
returns a typed refusal pointing to the first import span whenever imports are
opaque. No caller boolean can turn that source artifact into complete feature
requirements or dispatch authority.

The actual `policy_document` consumer retains its full/baseline modes with
unchanged no-import bytes. `--bound-imports NEW_INCLUDE_ROOT` compiles through the
same production path and materializes first-party synthetic dependency files in
a fresh caller-selected directory. Offline qualification uses an explicit isolated
include root, matching ABI/features and fatal warnings; missing required direct
or transitive files fail rather than becoming optional includes. This establishes
no compatibility claim for installed standard abstractions.

Full RDF/SRL/static admission, permission/owner/deny proofs, graph import closure
and imported-permission trust, nested graph profile synthesis, transition-target
existence, graph CLI, loaded mediation and host confinement remain
#33/#34/#35/#39 requirements.

## Semantic JSON comparison

`Node::equivalent_governed` accepts an explicit caller governor. Its stack holds
borrowed node pairs and their actual value depths, with root depth zero. Every
stack growth is admitted before allocation and releases on success, inequality
or refusal. No input DOM is copied. Distinct object keys can be reordered;
duplicate values compare in their original occurrence order within each key.
Key and string scans are charged before byte comparison.

Exact decimal comparison shares ordinary normalization and signed decimal
arithmetic. Mantissa and exponent buffers retain their scratch owners until the
comparison releases them. Exponent storage scales with the spelling's digit
count, rather than the exponent's mathematical magnitude. Negative zero, large
exponents and cancellation between mantissa zeros and exponent digits preserve
mathematical equality. Both semantic outcomes pass a final cancellation check.

Schema evaluation/recovery/algebra and emitter roundtrip consumers propagate
native comparison refusal through their real governor. Neither enum/exclusion
search nor contradiction detection converts refusal into a semantic answer.
Ordinary public comparison remains an explicitly ordinary operation. This does
not extend scratch ownership to caller DOM construction, full witness algebra,
XML decoding or higher-level emitter preparation.


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

## Prepared output-schema admission

New output source retention uses `ScratchString` owners through successful tool
construction, later refusal and drop. Field/fragment/type scans and new copies
admit work before processing; exact schema compilation shares the construction
governor through `Schema::compile_governed`. Contextual output diagnostics use
fallible governed formatting and tool-name storage. Published owned diagnostics
retain their admitted charge under the existing error contract.

The optional caller-owned builder string adds 24 portable bytes to the
`ToolDefinition` and `Pending` record charges, now 184 and 200 respectively.
Existing `CompiledTool` and declaration record bounds cover their new inline
output contract and remain checked against actual layouts. Output validation
borrows the compiler's existing DOM rather than allocating a second copy.

No default governor, result schema or wire extension is inferred. Unsupported
declaration targets fail before output publication. This extends the existing
schema compiler's native admission contract; broader legacy catalog/schema
allocator ownership remains a separate requirement.
