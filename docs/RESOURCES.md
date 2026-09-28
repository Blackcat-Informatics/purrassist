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
