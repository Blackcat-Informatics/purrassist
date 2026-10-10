# R14 root cause (G22): Mistral v3 call arrays

Base: head bdb102a. Release builds. Input: `[TOOL_CALLS] [` + k copies of
`{"name": "write", "arguments": {"content": "a"}, "id": "a1B2c3D4e"}` joined by `, ` + `]`,
`ToolRecoveryRequest::sized(Effort::Standard, ..)`.

## Phase 1: confirmation (throwaway counter in the `earlier` loop of `hold_failures`, removed)

| k | bytes | work/B | entries examined by `hold_failures` | wall |
| ---: | ---: | ---: | ---: | ---: |
| 256 | 17,677 | 73.6 | 4,572,928 | 0.108 s |
| 512 | 35,341 | 73.6 | 18,189,824 | 0.358 s |
| 1024 | 70,669 | 73.5 | 72,555,520 | 1.26 s |
| 2048 | 141,325 | 73.5 | 289,814,528 | 4.46 s |
| 4096 | 282,637 | 73.5 | 1,158,443,008 | 24.0 s |

Charged work is linear; examined entries grow x4 per doubling, wall follows.
Cause as in R13-rootcause: `Engine::hold_failures` took no `Governor`, kept
one literal failure per array element (the JSON-closer-met-comma `trailing`
rule holds it while any configuration of its attempt lives), and re-ran the
`sibling` predicate (with a `live.iter().any`) over every held failure at
every byte.

## Phase 2: fix design (crates/core/src/grammar/engine.rs)

`Table::failures` is now `HeldFailures`, grouped by attempt (`BTreeMap`),
each group an insertion-ordered `BTreeMap<seq, Failure>` plus a
`BTreeSet<(entry, seq)>` of the non-`trailing` failures. The old predicate is
equivalent to: static part (respellable and `Sub::Lit`, fixed at insertion;
`trailing` is also fixed because the byte at `failure.at` stays in the window
while its attempt lives, `base <= attempt start <= failure.at`) and, per step,
`floor(attempt)` = earliest `entry` among the attempt's non-speculative live
configurations: a failure survives iff a floor exists and (`trailing` or
`floor <= failure.entry`).

- Each step builds the sorted floors (from `live`, which this step advanced).
- If the floors equal those of the last examination (`settled`), no held
  failure can have become droppable, and only the fresh failures are checked
  and inserted: O(fresh).
- Otherwise `drop_below` visits each group once (charged one `Work` unit per
  group), drops the whole group if its attempt has no floor, else pops the
  keyed set while `entry < floor` (one `Work` unit per failure dropped).
- Scratch is charged per held failure (`HELD_FAILURE_CHARGE` 224) through the
  table's `ScratchMeter`, released on drop/removal; `abandon` and `finish`
  use `remove_attempt`/`clear`. Resource exhaustion propagates via `?`.
- `settle`'s stuck-record branch reads the group in place, charging `Work`
  per held failure it copies.
- Not charged separately (justified in a comment): the passes over `live` and
  `failed`, whose configurations were charged `K_THREAD` when stepped. The
  first version charged them and added 6 to 14 work/byte to every dialect;
  that charge is redundant, so it was removed.
- R13-reviewer uncharged vectors: `Engine::release`'s `order`, `tables`,
  `keep` (lease of 40 B per held completion, `RELEASE_VECTOR_BYTES`) and
  `close`'s `pending` list (96 B per unbuilt `next` node,
  `CLOSE_PENDING_BYTES`, charged on push, released on pop); hold_failures'
  `floors`, `fresh`, `emptied` vectors are leased too.

## Differential (temporary, removed)

The old `hold_failures` was kept beside the new one (shadow `Vec<Failure>`
per table, same `abandon`/`finish` treatment); after every call the per-attempt
failure sequences were asserted equal. Run on: lib, tool_call_goldens/rules/
recover/roundtrip/cost (all dense shapes)/dialects/negative, grammar_codecs/
engine, tool_adapters, tool_definitions, recovery_goldens, resource_accounting;
Mistral v3 arrays k = 256..4096 (~1.7M comparisons at k=4096); six malformed
array variants at k = 50 and 300 (unclosed element, trailing comma, missing
brace, odd separators, unclosed array; ~450K comparisons); and 4,000 mutated
golden inputs through `fuzz-tool-call` (no differential failure; 46 runs hit
the known #6 rejected-span oracle panic at fuzz_tool_call.rs:1084, unrelated).
No difference anywhere. The same run also confirmed `to_node()` identical (see
Digest).

## Before / after (release)

| k (bytes) | work/B before -> after | examined before -> after | wall before -> after |
| ---: | ---: | ---: | ---: |
| 256 (17.7 KB) | 73.6 -> 79.7 | 4.57M -> 0 held scans | 0.108 s -> 0.07 s |
| 1024 (70.7 KB) | 73.5 -> 79.6 | 72.6M -> 0 | 1.26 s -> 0.25 s |
| 2048 (141 KB) | 73.5 -> 79.6 | 290M -> 0 | 4.46 s -> 0.56 s |
| 4096 (283 KB) | 73.5 -> 79.6 | 1,158M -> 0 | 24.0 s -> 1.35 s (0.75 s on a quiet machine) |
| 15,195 (1 MiB) | not finished in 300 s | - | 5.9 s, Completed under plain `sized`, 75,972 evidence entries |

Peak scratch/B 183 -> 186; evidence 0.072/B. Wall time per byte still creeps
up (3.5 us/B at 70 KB, 5.6 us/B at 1 MiB); the perf profile at 16K calls is
flat (no frame above 3.5%), so no further quadratic term, consistent with
cache and `BTreeMap` log factors. (Symbols were stripped in the profile build;
a flat address histogram was the evidence.)

Other shapes (dense, 1 MiB work/B before (published) -> after): Hermes 76.8 ->
82.4, Llama 3 69.1 -> 76.2, Kimi-K2 138.3 -> 142.3, Harmony 115.4 -> 118.9,
bare JSON 95.1 -> 101.4, Anthropic 51.6 -> 56.5, Mistral v3 calls 66.9 ->
71.9; String-argument rows move by under 0.01% (hermes 17,834,183 -> 17,834,596
at 1 MiB). The increase is the honest cost of the now-charged group visits and
drops (the `settled` skip keeps it to floor changes).

## Margins (constants unchanged by the user's decision)

Work: maximum 142.3 (Kimi-K2) against `RECOVER_WORK_PER_BYTE` 176 (24% margin,
was 27%; 4 KiB Kimi 143.3). The new shape: 79.6. Scratch: maximum 301.6
(DeepSeek-V3) against 368 (22%, unchanged; 4 KiB 324.5); the new shape 186.0.
Evidence: 0.072/B against ceil(n/7) = 0.143/B. No shape exceeds a constant.

## Phase 3: tests and docs

- `tool_call_cost::mistral_v3_call_arrays_cost_is_linear`: k = 1024, 2048, 4096,
  8192, 16384; all `Resolved`, `Completed`, work/scratch-charged/peak within
  1.05x of the byte ratio both ways, work <= RECOVER_WORK_PER_BYTE*n, peak <=
  RECOVER_SCRATCH_PER_BYTE*n. Because the loops are now charged, the old
  behaviour (examining all held failures per byte) would show as superlinear
  work.
- `grammar::engine::tests::a_step_over_held_failures_costs_what_it_changes`
  (lib): the structural counter. A step over 100 and over 20,000 held failures
  charges the same (0 with unchanged floors); a floor rise charges 1 + the 2,500
  failures it drops; order preserved; no floor drops the group; scratch balanced.
- `mistral-v3-array` added to the `DENSE` shapes (new `Dense::ArrayOfCalls`):
  runs at 64 KiB in `tool_call_cost::dense_markup_fits_published_budget` and at
  1 MiB in `tool_call_scale::one_mib_dense_markup_fits_published_budget`
  (`make corpus`): 15,195 calls, Completed under plain `sized`.
- `tool_call_scale` (corpus lane) release: 4 tests, 61-68 s (the whole lane,
  `make corpus`, 5 min 8 s including the other suites; v3 array 1 MiB ~6 s).
- RESOURCES.md: removed the Mistral v3 "Not covered" item; added the array row
  to the dense table, a paragraph describing the fix and numbers; refreshed
  every dense-table work/scratch value, the 52-139 range (now 53-142), the
  4 KiB figures and the 27% margin (now 24%); the "(3)" document item was
  taken to be the R13 addendum (restarted DeepSeek chains superlinear, left
  for the user's decision) and is now a "Not covered" bullet with the measured
  numbers (see Deviations).

## Digest rule

The four `PINNED_*_DIGEST` constants hash `result.to_node()` including
`receipt.usage`. Demonstration: `wire_digest` was temporarily made to dump
`to_node().to_json()`; the same test (`deterministic_pinned_digest`: three
dialect permutations, the `Prefix` policy, two ambiguous forms = 7 results) was
run on a `git archive` copy of bdb102a and on the working tree. With the
`"usage":{...}` object removed, all 7 dumps are byte-identical (including the
`budget` object). Only usage differs: work 20,357 -> 20,977 and peak scratch
72,130 -> 80,450 (total scratch charged 757,489 -> 886,593) for the pinned
turn; the ambiguous forms change only `total_scratch_charged`. Re-pinned:
`PINNED_DIGEST` 219f94f7..., `PINNED_PREFIX_DIGEST` 0c3a7133...,
`PINNED_AMBIGUOUS_DIGEST` f02fa417..., `PINNED_PREFIX_AMBIGUOUS_DIGEST`
7e153524.... No golden or other test needed re-blessing.

## Checks

- `cargo test -q -p purrassist-core --test tool_call_cost --test tool_call_goldens --test tool_call_rules --test tool_call_recover --test tool_call_roundtrip --test grammar_codecs --test grammar_engine --test tool_call_dialects --test tool_call_negative --test tool_adapters --test tool_definitions --test recovery_goldens --test resource_accounting`: all pass.
- `--lib` (120), `--doc` (377): pass. `cargo test --workspace --no-fail-fast`: no failure.
- `cargo test --release --test tool_call_scale`: 4 passed (61 s).
- `make corpus`: exit 0 (5 min 8 s).
- `make hygiene`: exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --all --check`: clean.
- No `make check`/`make fuzz`. No `*.profraw` found. Probes, shadow code, the digest-dump hack and the temporary test file were removed.

## Deviations

- Interpretation of "(3)": taken as the R13 addendum on restarted DeepSeek
  chains (documented, not fixed). If it meant something else, say so.
- A held failure's scratch is now charged through the table's meter at 224 B
  per failure instead of the `ScratchVec` capacity charge (160 B x capacity);
  peak scratch moved by under 1%.
- Work per byte rose 2-14 units on other dialects (margin to 176 drops to 24%);
  the constants were not touched.
