<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Coverage-guided JSON qualification

Run `make fuzz-json` with Rust 1.98.0 and installed `llvm-cov` and
`llvm-profdata` 22.1.8. The runner checks both tool versions against the compiler's
LLVM version and `vendor/tooling.json`. It uses Python's standard library and a
first-party Rust example; there are no additional Cargo dependencies. Only the
qualification executable receives `-C instrument-coverage`. It is built at
optimization level 1 with debug information, debug assertions, and overflow
checks. This campaign does not qualify the exact release binary.

The default campaign executes 14 initial inputs, then 1,000 mutations. Each input
is limited to 4,096 bytes and runs in a separate process. The fixed LCG seed,
mutation algorithm, and retained corpus determine subsequent inputs. Insert,
delete, replace, duplicate, and wrap mutations use JSON punctuation, common
fields, numbers, escapes, and Unicode tokens. Every initial input is retained;
subsequent inputs are retained only when they execute a previously unseen code
region in `crates/core/src`. Harness and standard-library regions, uncovered
regions, execution-count changes, and expansion/gap regions do not count as
novelty. Generic instantiations share source-coordinate identities. This is
source-region feedback, not edge coverage or sanitizer instrumentation.

The probe accepts UTF-8 JSON input. Invalid UTF-8 is explicitly rejected before
entering the core; the report counts those executions separately. For admitted
inputs it checks:

- Strict parse/serialize/parse value equivalence and stable serialization.
- Recovery against four fixed schemas covering objects, arrays, enums, unique
  items, numbers, booleans, required fields, and a disjoint union.
- Complete schema validation and serialization round trips for every returned
  answer, including each ambiguous alternative.
- Source digest, guess classification, ambiguity ledger count, completion status,
  and reported work/candidate/scratch consumption within the supplied limits.

Recovery uses 20,000 work units, 32 candidates, and 1,000,000 reserved scratch
bytes per schema. Other dimensions use standard defaults. Logical budget checks
are not hard allocator limits. Schema validation is a consistency oracle from
the same engine, not an independent implementation. The default ten-second
timeout applies separately to each probe and LLVM invocation; a timeout fails
the campaign. Tool errors, malformed coverage, unexpected output, and assertion
failures also fail rather than contributing successful executions.

Artifacts are retained under the unique `.cache/fuzz-json-*` directory printed
at startup. `report.json` contains source/build/tool identities, campaign
parameters, execution counts, retained input digests, parent corpus indexes,
and new-region counts. `regions.json` contains the cumulative source-region
set. `corpus/*.bin` stores the seeds and discoveries. `current.bin` is written
before launching each probe and remains the failing input on an error;
`failure.txt` and `probe.stderr` provide diagnostics. Successful per-input
profiles are replaced on the next execution, after deleting the prior profiles;
they are never merged across inputs for novelty decisions.

```sh
make fuzz-json FUZZ_ARGS='--iterations 10000 --seed 6491701121104122699'
make fuzz-json FUZZ_ARGS='--replay .cache/fuzz-json-EXAMPLE/current.bin'
```

Replay rebuilds and tests the saved bytes against the current source. Preserve
the report and source revision to reproduce the original build. Exact discovery
counts are tied to compiler, flags, and source mapping; they are not comparable
coverage percentages across toolchain versions. The report records a digest of
inherited Rust flags rather than exposing machine-specific flag values.

The lightweight runner tests run in `make check`; the instrumented campaign is a
separate qualification command. Bounded campaigns cannot establish absence of
bugs, complete recovery quality, or whole-crate coverage. Schema mutation,
Markdown/editing/matching targets, additional budget/effort policies, larger
inputs, independent oracles, and longer campaigns remain qualification work.

The instrumentation and profile format follow the
[Rust compiler coverage documentation](https://doc.rust-lang.org/rustc/instrument-coverage.html)
and [LLVM coverage tool](https://llvm.org/docs/CommandGuide/llvm-cov.html).
