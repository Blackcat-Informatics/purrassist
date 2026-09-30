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
malformed-token replacements JSON syntax repair re-encodes per repair
(`json/repair.rs`), the lease/release sites in `matching.rs` and
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
- `pattern.rs`'s bounded backtracking search (`repeat`) clones its capture
  state on every iteration of a repetition, in addition to the clone
  `evaluate`'s own per-call lease already covers; `repeat` now leases a bound
  covering its whole stack/results growth, released when it returns.
- `json/repair.rs`'s syntax recovery re-encodes every repaired bare/malformed
  token's replacement through `Node::to_json`, one allocation per repaired
  token, against a flat, too-small per-input-byte precharge; the precharge
  is now sized against a per-token cost, not a per-byte guess.

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
| `json_recover_syntax` | Unquoted/single-quoted object, 2 extra keys | 32 extra keys | 2,048 |
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

These `K` values, the workloads at both scales, and the inequality above are
asserted by `allocation_oracle.rs` itself; a regression that undercounts real
allocation by more than its row's `K`, at either scale, fails that test. They
are not a certification that any *other* input to these operations stays
within the same bound (an input shaped very differently from the oracle's
generators, or scaled far beyond 16x, could still expose a gap this two-point
check does not sample), and they do not replace `make resources`' broader,
allocator-agnostic measurements above.

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
inherited. No endpoint is embedded in this repository or the report.

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

[Linux x86_64 resource baseline](qualification/resources-linux-x86_64.json) was measured from commit `856d2ecdbc5835e561d70c7bb85ceb717c29f568`. The report records the dirty working tree and a stable engine source digest checked before and after the campaign. It records the isolated runtime and executable identities. Ordinary build durations describe their observed Cargo cache states; the separate `cold_build` records compilation with empty target and intermediate directories and disabled compiler wrappers.

| Workload | Allocation calls | Total allocated bytes | Observed requested-heap peak | Native reported RSS |
| --- | ---: | ---: | ---: | ---: |
| empty | 13 | 3,817 | 1,736 | 4,689,920 |
| schema_compile | 106 | 9,168 | 4,538 | 5,353,472 |
| json_parse | 1,574 | 236,097 | 173,150 | 5,349,376 |
| json_composed | 2,541 | 192,835 | 62,611 | 5,636,096 |
| json_ambiguous | 389 | 33,349 | 15,689 | 5,496,832 |
| markdown_render | 4,096 | 777,195 | 225,787 | 5,677,056 |
| markdown_list | 2,811 | 242,688 | 152,036 | 5,705,728 |
| unicode | 23 | 125,931 | 68,955 | 5,152,768 |
| matching | 7,222 | 313,232 | 23,944 | 5,222,400 |

Values include the full benchmark process as described above. They are not per-function allocation counts, portable guarantees, or a production sizing rule.

The release CLI in this snapshot is 9,762,176 bytes. The instrumentable benchmark (12,969,312 bytes) includes retained debug information and is not a release CLI size comparison.

### Artifact sizes

| Artifact | Bytes |
| --- | ---: |
| Native release CLI (`purrassist-cli`) | 9,762,176 |
| `purrassist-core` `wasm32-wasip1` (`portable-core`) | 1,998,896 |
| `purrassist-syntax` `wasm32-wasip1` (`portable-syntax`) | 5,223,153 |
| `purrassist-host` `wasm32-wasip1` (`portable-host`) | 1,467,114 |
| `wasm-harness` `wasm32-unknown-unknown` | 2,120,547 |

### Cold build

The recorded release CLI cold build took 30.140 seconds and produced 35 compiler artifacts, with 0 reused artifacts. Both compiler wrapper layers were disabled; the target and intermediate directories were new and empty. Registry sources and OS page caches remained available. The report also records the resulting executable's size and digest.

### Latency workload samples (selected)

Full detail is in the committed report. Each row measures 200 samples in its own process; allocation and heap observations cover one operation after sampling.

| Core workload | Median ns | p95 ns | Allocation calls | Peak live bytes | Peak RSS bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| schema_compile | 6,040 | 6,700 | 93 | 3,928 | 5,722,112 |
| schema_recover | 123,436 | 127,856 | 2,528 | 62,005 | 6,266,880 |
| matching_resolve | 852,574 | 892,522 | 3,684 | 19,838 | 5,632,000 |
| edits_fuzzy | 610,301 | 640,940 | 12,980 | 12,676 | 5,738,496 |
| markdown_render | 30,769 | 35,449 | 450 | 24,840 | 5,906,432 |

| Syntax workload | Median ns | p95 ns | C allocation calls | Rust allocation calls | C peak bytes | Rust peak growth bytes | Peak RSS bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| python | 354,359 | 381,328 | 573 | 59 | 56,720 | 4,760 | 3,387,392 |
| rust | 295,601 | 314,220 | 480 | 42 | 48,896 | 3,048 | 3,862,528 |
| javascript | 326,069 | 339,570 | 476 | 25 | 48,360 | 2,264 | 3,485,696 |
| typescript | 354,669 | 390,578 | 652 | 42 | 64,040 | 3,184 | 4,177,920 |
| tsx | 374,948 | 403,847 | 545 | 25 | 53,032 | 2,288 | 4,329,472 |

C and Rust peak values are measured separately and must not be added as a simultaneous peak.

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
