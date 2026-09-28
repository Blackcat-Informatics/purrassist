<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Resource measurements

`make resources` builds an assertion-backed resource benchmark and the release
CLI, then measures nine fixed workloads. It requires Linux, Python 3, Cargo,
and the Valgrind version registered in `vendor/tooling.json`. No profiler or
allocator wrapper is linked into the crates, and the core retains its unsafe-code
prohibition and zero runtime dependencies.

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
scoped to the file) and asserts, for every governed public operation, **at
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
operation. Because both sides of the inequality now scale with input size,
holding `K` constant across a 16x input-size change is a real falsifiability
property, not a numerically convenient one: an uncharged allocation that
grows with input size, at any multiplier, is caught at the larger scale even
if a generously chosen `K` hid it at the smaller one. This oracle caught and
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

`K` still covers a smaller, now genuinely scale-invariant residual: fixed,
per-call overhead (`Parser`/`Renderer` construction, digest computation) that
does not grow with input size, plus any remaining retained-output structure
this task did not individually convert to a charged type (index/children maps
built during schema compilation, small result vectors). A regression that
reintroduces an uncounted allocation *proportional to input size* — however
small at the base scale — fails at the 16x scale even with a generous `K`,
because `K` cannot absorb a term that itself grows with input.

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

## Recorded baseline

[Linux x86_64 resource baseline](qualification/resources-linux-x86_64.json) was
measured from clean commit `c2f5d18f218033bae408ba28a543f0926cdfa7cb`. It records the
isolated runtime and executable identities. Build artifacts were already cached;
its build durations are not cold-build measurements.

| Workload | Allocation calls | Total allocated bytes | Observed requested-heap peak | Native reported RSS |
| --- | ---: | ---: | ---: | ---: |
| empty | 13 | 3,766 | 1,736 | 3,293,184 |
| schema_compile | 105 | 9,061 | 4,482 | 3,670,016 |
| json_parse | 1,573 | 235,990 | 173,150 | 3,825,664 |
| json_composed | 844 | 90,691 | 40,156 | 3,883,008 |
| json_ambiguous | 309 | 23,628 | 12,132 | 3,784,704 |
| markdown_render | 3,785 | 665,496 | 114,883 | 3,977,216 |
| markdown_list | 1,903 | 217,925 | 151,980 | 3,993,600 |
| unicode | 23 | 125,880 | 68,955 | 3,657,728 |
| matching | 5,464 | 524,546 | 101,905 | 3,862,528 |

Values include the full benchmark process as described above. They are not
per-function allocation counts, portable guarantees, or a production sizing rule.
The release CLI executable in this snapshot is 7,316,832 bytes; the instrumentable
benchmark executable includes retained debug information and is not a release
CLI size comparison.
