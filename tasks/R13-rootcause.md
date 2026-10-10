# R13 root cause (G21) and R14 characterization (G22)

Method: throwaway counters in `engine.rs` and a throwaway test (both removed)
fed `[TOOL_CALLS]write[CALL_ID]a1B2c3D4e[ARGS]{"content": "a"}\n` x k to
`ToolSet::recover` with every budget lifted (true cost), HEAD 8fb63a4.
Wall-time hot spots from `perf record` on a line-table build.

## R13: Mistral v11 calls chained by whitespace

### Measurements at HEAD (counters)

| k calls | segment() runs | parts read (= reading = fields = ArgTree clones) | work/B | peak scratch/B | release wall |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 64 | 127 | 2,143 | 76 | 418 | 13 ms |
| 128 | 255 | 8,383 | 89 | 614 | 31 ms |
| 256 | 511 | 33,151 | 116 | 1,016 | 88 ms |
| 512 | 1,023 | 131,839 | 169 | 1,817 | 278 ms |
| 1024 | 2,047 | 525,823 | 275 | 3,420 | 968 ms |

Segment runs are 2k-1 (the chain attempt reports once per call, plus one
single-call attempt per later `[TOOL_CALLS]` opener); parts read are k(k+1)/2
+ O(k): quadratic.

### Cause (exact functions)

1. `Engine::report` (engine.rs) is called at every completion of the
   attempt. A `next`-chained production may end after any call, so the
   attempt completes after call 1, after call 2, ... Each report calls
   `Step::segment(thread.hist)`, which calls `histories` (walks the whole
   history, one `K_CAPTURE` charge per node), then `part_readings`, which
   calls `reading` -> `trace` -> `fields` -> `value` (clones the codec
   `ArgTree`) for EVERY call part 0..i of the history again. Call i's
   history is call i-1's history plus one `Next` node and one call, but
   nothing was reused: report i re-reads i parts.
2. `report` pushes the full `Parts` (i cloned calls) into
   `Table::held`; `Engine::release` keeps them until the attempt ends, so
   retained scratch is also quadratic (and `Event.charged` of dominated
   completions was never released). `release` compared every pair
   (`size(m)^2` Work) and `Held::prefix` re-summed costs per pair.
3. Two uncharged quadratic per-byte scans surfaced after (1)-(2) were gone
   (profile: `Recognizer::settle` 68%): `Recognizer::settle` ran
   `pending.iter().any(..)` over every queued report per byte, and
   `Engine::held_floor` iterated every held completion per byte.
   `Engine::release` also scanned and partitioned the whole held list per
   ending attempt.

### Fix (implemented)

- `Step::segment_chained`: a completion whose history reaches its nearest
  `Next` node along paths that all reach the same cut is segmented as
  (memoized chain of the calls closed before that `Next`) + its last call.
  Each `Next` node closes its call once (`Step::close`, memo in
  `Arena::memo`, parts in `Arena::chains`, shared by all later
  completions). Skipped parts, a leading skipped part's lead edits and
  `lead_split` cost moves, undeclared-name weights, packed alternatives
  inside one call (competing readings, Candidates charged as before) are
  kept; histories whose paths reach different cuts, or differ in
  skipped/unreadable calls, fall back to the unchanged `segment`.
- `Held` is lazy: a chain completion keeps its last part and a chain
  pointer; `Engine::release` evaluates dominance with O(1) totals, builds a
  prefix-cost table only for a dominator it actually consults, tries the
  latest completion first, charges the pairs actually compared, and
  materializes (`Arena::parts`, charged) only the survivors.
- `HeldSet` (by attempt, cached floor) replaces the held vector;
  `Recognizer::pending` is a `BTreeMap` keyed (offset, phase, arrival) so
  `settle` pops what is due instead of scanning.
- Verified identical to the old path by a temporary differential check
  (asserting the chained and original segmentations, costs, prefix tables
  and rejections equal on every report) across all goldens, grammar engine,
  dialect, round-trip and recover suites, plus chains with a garbled first
  call, a garbled middle call, duplicate keys, truncation and prose lead.

### After

| k | work/B | peak scratch/B | release wall |
| ---: | ---: | ---: | ---: |
| 256 | 64 | 218 | 27 ms |
| 1024 | 64 | 218 | 108 ms |
| 2048 | 64 | 219 | 228 ms |
| 4096 | 64 | 219 | 505 ms |

1 MiB chained (18,078 calls): 64.1 work, 220.1 peak scratch, 0.086 evidence
entries per byte, `Completed` under plain `sized`.

## R14: Mistral v3 `[TOOL_CALLS]` array (characterization only, not fixed)

Measured (release, lifted budgets): work 73/B and peak 183/B constant, wall
44 ms (k=128), 124 ms (256), 340 ms (512), 1.19 s (1024), 4.2 s (2048):
quadratic wall, linear charge. `perf` puts 89% in
`Engine::hold_failures` (engine.rs). Counter: bytes examined by its
`earlier` loop, summed over all calls: 4.6M (k=256), 18.3M (512), 72.8M
(1024), 290M (2048): x4 per doubling.

Cause: `hold_failures` runs on every byte step that has failures. It keeps
the literal failures a still-live sibling configuration could repair; in a
JSON-envelope array each element's closer/`,` meets a failure that the
`trailing` rule holds "while any configuration of its attempt lives", so
the held list grows by one per element and is re-filtered (each entry
scanning `live` with `.any`) at every byte. The function takes no
`Governor`, so none of this is charged: the uncharged quadratic step.
Fix direction (R14): bound or index the held failures by attempt/entry so
a byte examines only the failures that can change, and charge the scan.

## Addendum: restarted DeepSeek chains (not fixed; pre-existing)

The R13 review measured DeepSeek inputs with many sections restarted after a
truncated call: work per byte 489 at 64 sections, 1,666 at 256 sections,
superlinear and identical on HEAD 8fb63a4 (not an R13 regression). These
inputs reach the whole-segmentation fallback (a packed history whose paths
reach different call boundaries), once per section; the growth comes from
the restart handling, not from chained segmentation. Left for the user's
decision. `tool_call_cost::whole_segmentation_fallback_is_correct_and_charged`
pins the correctness and the per-byte bound of the single-section case.

## Addendum: measurements refreshed (dense, 1 MiB, after R13)

DeepSeek-V3.1 peak scratch 86.7 -> 98.8 per byte, DeepSeek-V3 291.8 ->
301.6 (4 KiB: 318 -> 328); all other dense rows re-measured unchanged.
Margin of `RECOVER_SCRATCH_PER_BYTE` 368 over the maximum (301.6) is 22%.
