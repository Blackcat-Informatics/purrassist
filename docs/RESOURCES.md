<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Resource measurements

`make resources` builds an assertion-backed resource benchmark and the release
CLI, then profiles nine fixed workloads and measures core and syntax latency,
allocation, RSS, artifact sizes and an uncached release build. It requires Linux,
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
- Executable byte sizes and SHA-256 identities, compiler/runtime identities,
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

Recovery's per-candidate cloning (`schema/recovery.rs`, `schema/recovery/
proposals.rs`), the evaluator's violation/active-path strings
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
all. Permanently retained output (a compiled schema or pattern, a receipt's
retained source, a validation's diagnostic strings, an `EditPlan`'s
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
led to fixing three such regressions during this task, each confirmed fixed
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
dead language. Direct controls exercise 32,768 flat literals at logical depth
zero, the previously uncharged nullable traversal, and overlapping root/residual
alternation chains with compilation and generation Work boundaries.

Compilation has two additional zero-slack allocator controls for wide alternations
with 8, 64 and 128 branches. Compile terms, their sorted lookup index, classes,
and predicate cuts own admitted capacities; the syntax estimate is leased and
released on a failed compilation. Controls cover the formerly undercharged
128-branch arena under a 140,000-byte budget, exact scratch and Work boundaries,
and cancellation at initial, intermediate and final allocation checkpoints.
The complete allocator oracle contains 31 controls.

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

## Latency, artifact size, and cold-build measurements

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
- `artifact_sizes`: byte size and SHA-256 identity of the native release
  CLI, the `wasm32-wasip1` `portable-core`/`portable-syntax`/`portable-host`
  example modules (the same fixed-transcript examples
  `scripts/check-wasm.py` executes; the syntax/host builds use the same
  pinned WASI C sysroot that script's `wasi_environment()` sets up, via a
  small duplicated copy in `measure-resources.py` -- `check-wasm.py` runs its
  own full build/execute/compare check as top-level code the instant it is
  imported, so importing it here would trigger that check as a side effect
  instead of reusing just the helper), and the `wasm32-unknown-unknown`
  `wasm-harness` module.
- `cold_build`: elapsed nanoseconds for a release CLI build using the real
  toolchain Cargo resolved through `rustup which cargo`. Both the target
  directory and Cargo's separate intermediate build directory are newly
  created and empty, under `/tmp` outside the checkout. `RUSTC_WRAPPER`,
  `RUSTC_WORKSPACE_WRAPPER`, and the
  corresponding Cargo environment/config values are cleared. Every Cargo
  compiler-artifact event must report `fresh: false`; the report records
  those counts and the resulting CLI's byte size and SHA-256. Registry
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

The release CLI in this snapshot is 9,757,120 bytes. The instrumentable benchmark (12,967,216 bytes) includes retained debug information and is not a release CLI size comparison.

### Artifact sizes

| Artifact | Bytes |
| --- | ---: |
| Native release CLI (`purrassist-cli`) | 9,757,120 |
| `purrassist-core` `wasm32-wasip1` (`portable-core`) | 1,998,277 |
| `purrassist-syntax` `wasm32-wasip1` (`portable-syntax`) | 5,223,153 |
| `purrassist-host` `wasm32-wasip1` (`portable-host`) | 1,467,114 |
| `wasm-harness` `wasm32-unknown-unknown` | 1,937,506 |

### Cold build

The recorded release CLI cold build took 27.364 seconds and produced 35 compiler artifacts, with 0 reused artifacts. Both compiler wrapper layers were disabled; the target and intermediate directories were new and empty. Registry sources and OS page caches remained available. The resulting executable is 9,668,520 bytes with SHA-256 `24ea7dd813a878666c1d304740ec49a43a5ef0c045b071d135055b8d6f3aa053`.

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
recover     = 2·n (retaining the output, the encoding scan) + recognition + resolution
recognition = K_LEX·n + K_THREAD·(configuration steps) + K_EVENT·(segment events)
            + K_CAPTURE·(captured bytes and history nodes materialized)
            + K_ERR·(error configurations explored)
            + K_CODEC·(codec scan bytes, name lookups and tree nodes)
            + (JSON recovery units) + (class-automaton units) + (search key bytes)
resolution  = one unit per member, option, alternative and validation step,
              plus the recovered value, ids, checks and ledger entries
```

Each dialect `d` of the request's allow-list keeps at most `B_d`
configurations live per input byte, where `B_d` is
`CompiledGrammar::configuration_bound()` (one per program location plus one
per byte position of each literal spelling); `K_d = K_THREAD·B_d` is its
per-byte term. The published bound is

```text
work ≤ (2 + K_LEX + Σ_d K_d)·n + K_ERR·E,   K_d = K_THREAD·B_d
```

where `E` is the number of error configurations the repair search explores,
capped by the effort's `Work` budget (explicit `BudgetExceeded` beyond it).
The configuration term is an upper bound per byte: on the fast path only a
handful of the `B_d` configurations are live at any byte, and the remaining
linear terms (segment events, captured bytes, codec scans, JSON recovery
units, resolution) are each a small constant per byte: whole recoveries
measure 10 to 17 units per byte in total (the throughput table below), far
inside the slack of any built-in `K_d`.
`crates/core/tests/tool_call_cost.rs` checks the bound on 64 KiB and 1 MiB
Qwen3-Coder XML and Hermes string arguments (`E = 0`: no repair is
recorded), that the 64 KiB to 1 MiB work ratio is within 1.05x of the byte
ratio, that pushing the same output in chunks of 1, 16 and 4096 bytes
charges identical usage, and that adding a dialect to the allow-list adds
at most its `K_d` per byte (measured: adding Hermes to a Qwen3-Coder XML
allow-list adds 0 units per byte on a Qwen3-Coder XML argument and 7 on a
Hermes argument, the configurations of the call it now reads; adding
`OpenAI` adds 0). These are checks of the published inequality on those
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
| `RECOVER_WORK_PER_BYTE` | 32 | `Work` per output byte `ToolRecoveryRequest::sized` adds |
| `RECOVER_SCRATCH_PER_BYTE` | 48 | `ScratchBytes` per output byte `ToolRecoveryRequest::sized` adds |

| Built-in dialect | `B_d` | `K_d` per byte |
| --- | ---: | ---: |
| `hermes.v1` | 635 | 635 |
| `openai-tool-calls.v1` | 2151 | 2151 |
| `qwen3-coder-xml.v1` | 442 | 442 |

The default request allows all three, so its bound is
`(2 + 1 + 3228)·n + 8·E = 3231·n + 8·E`.

### Budgets for large outputs

Fixed effort presets are sized for ordinary turns, so a large output needs a
budget that grows with it. `ToolRecoveryRequest::sized(effort, n)` derives
one from the preset and the two per-byte constants above:

```text
work          = preset.work          + RECOVER_WORK_PER_BYTE    · n   (32 per byte)
scratch_bytes = preset.scratch_bytes + RECOVER_SCRATCH_PER_BYTE · n   (48 per byte)
input_bytes   = max(preset.input_bytes, n)
```

The derivation is saturating and deterministic; every other limit is the
preset's, so repair search stays bounded by the effort.
`tool_call_cost::one_mib_argument_fits_published_budget` recovers a 1 MiB
string argument in each built-in dialect's shape and checks that the call is
`Resolved` under `sized(effort, n)` for every effort from `Quick` to
`Maximum` (and, for Qwen3-Coder XML and Hermes, under the unmodified
`Effort::Maximum` preset), with identical charges in every case, peak
scratch at most 16 bytes per input byte for Qwen3-Coder XML and Hermes, and
at most `RECOVER_SCRATCH_PER_BYTE` for `OpenAI` stringified arguments, whose
escaped string is also decoded, recorded as the decoding's replacement and
parsed again:

| Input | Bytes | Work | Work per byte | Peak scratch | Scratch per byte |
| --- | ---: | ---: | ---: | ---: | ---: |
| Qwen3-Coder XML `<parameter>` string | 1,048,663 | 11,536,663 | 11.00 | 8,415,425 | 8.02 |
| Hermes JSON string argument | 1,048,648 | 10,488,490 | 10.00 | 9,447,556 | 9.00 |
| `OpenAI` stringified string argument | 1,048,679 | 15,732,143 | 15.00 | 47,201,161 | 45.01 |

All use the default request (all three built-in dialects). The constants
leave headroom over these fast-path measurements; inputs that need repair
search spend from the preset part of the budget, and exhaustion is reported
as `BudgetExceeded` naming the resource.

## Tool-call recovery throughput (illustrative)

`crates/core/benches/tool_calls.rs` (`cargo bench -p purrassist-core --bench
tool_calls`; `-- --list` lists its workloads, `-- NAME` runs one) recovers a
write call with a 1 KiB, 64 KiB or 1 MiB string argument as Qwen3-Coder XML,
Hermes, `OpenAI` stringified arguments, and a Qwen3-Coder XML call followed
by a Hermes call under a Qwen3-Coder + Hermes allow-list, each under
`ToolRecoveryRequest::sized(Effort::Standard, n)`, and feeds a 64 KiB
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
commit `2b5c98b` with this change applied:

| Workload | Bytes | Samples | Median ms | MiB/s | Work/byte | Peak scratch/byte |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `qwen_xml_1kib` | 1111 | 200 | 0.290 | 3.64 | 12.23 | 31.34 |
| `qwen_xml_64kib` | 65623 | 30 | 14.759 | 4.24 | 11.02 | 8.39 |
| `qwen_xml_1mib` | 1048663 | 7 | 229.810 | 4.35 | 11.00 | 8.02 |
| `hermes_1kib` | 1096 | 200 | 0.309 | 3.38 | 11.83 | 42.94 |
| `hermes_64kib` | 65608 | 30 | 15.729 | 3.97 | 10.03 | 9.14 |
| `hermes_1mib` | 1048648 | 7 | 250.520 | 3.99 | 10.00 | 9.00 |
| `openai_stringified_1kib` | 1127 | 200 | 0.357 | 3.00 | 16.73 | 54.37 |
| `openai_stringified_64kib` | 65639 | 30 | 18.380 | 3.40 | 15.02 | 45.16 |
| `openai_stringified_1mib` | 1048679 | 7 | 295.754 | 3.38 | 15.00 | 45.01 |
| `mixed_qwen_hermes_1kib` | 1184 | 200 | 0.298 | 3.79 | 12.24 | 36.92 |
| `mixed_qwen_hermes_64kib` | 65696 | 30 | 14.257 | 4.39 | 10.53 | 9.01 |
| `mixed_qwen_hermes_1mib` | 1048736 | 7 | 218.880 | 4.56 | 10.50 | 8.53 |
| `push_chunk_1` | 65623 | 30 | 12.385 | 5.05 | 8.00 | 5.17 |
| `push_chunk_16` | 65623 | 30 | 12.133 | 5.15 | 8.00 | 5.17 |
| `push_chunk_4096` | 65623 | 30 | 12.198 | 5.13 | 8.00 | 5.17 |

## Resource limits

`scripts/check-resource-limits.py <report.json>` checks a resource report
(freshly generated, or the committed baseline above) against the table
below, extracted from the first ```` ```json ```` fence following this
heading. Every limit is set with headroom over an actually measured value
recorded above (never below one); a limit failing here means measured
resource use grew past a deliberately generous ceiling, not that it
regressed by any amount. `make hygiene` uses `--schema-only` against the *committed*
`qualification/resources-linux-x86_64.json` (complete inventories, valid types,
digests, uncached-build evidence and correctness observations); Task 17's CI `qualification` job runs `make resources` fresh and
checks that output instead.

Rationale per category:

- `artifact_sizes`: roughly 2x the measured byte size in this document's
  "Artifact sizes" table, generous enough to absorb ordinary dependency and
  toolchain growth between releases without masking an accidental doubling.
- `cold_build_elapsed_ns`: 180 seconds. This limits the uncached release
  CLI compilation and link measured with empty build directories; 180s is a
  generous ceiling for an actually cold compile of this workspace's five
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
  "artifact_sizes": {
    "native_cli_bytes": 20000000,
    "purrassist_core_wasm32_wasip1_bytes": 4000000,
    "purrassist_syntax_wasm32_wasip1_bytes": 10000000,
    "purrassist_host_wasm32_wasip1_bytes": 3000000,
    "wasm_harness_wasm32_unknown_unknown_bytes": 4000000
  },
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
25 core latency cases, five syntax cases, six recovery cases and five artifact
sizes passed the unchanged documented limits. Strict native, Memcheck and Massif
startup/output checks completed, including every workload's correctness checks.

The framework's cold CLI build took 21.671 seconds, built
35 artifacts and reused 0. Its executable is 9,694,344 bytes
with SHA-256 `f6621c50e03cc55e0cbf24fe715052eb4b9ebdfc19cfd24a0a230ffbd9be3190`. This is an
observation of the recorded machine and build conditions, not a speedup claim.
