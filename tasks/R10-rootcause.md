# R10 root cause: repeated parameters (G18)

## Method
Temporary probe (removed) in `ScratchMeter::charge`: for every charge of 5,000
bytes or more, a backtrace was keyed by its first three `purrassist_core`
frames and summed. `ToolSet::recover` ran on k repeated occurrences in
qwen3-coder-xml (`tree` tool: `children` array of objects, `tags` array of
strings), budgets lifted. A second probe sampled `Work` charges
(1 in 997, weighted). `perf record` on the fixed code at k = 16,000.
Debug profile here is opt-level 3.

## Finding
One step: `Resolver::collect` (resolve.rs, the per-item loop that extends the
partial array), reached from `Resolver::parameter` <- `Resolver::object`.
For each of the k items it called `combined_bytes(base, alternative)`, which
calls `base.value.estimated_bytes()` (a walk of the whole partial array) and
charges that as `ScratchBytes`, then `base.clone()` (a copy of the whole
partial array and ledger) and pushes one item. The identical loop is in
`typed()` for `ValueShape::Array`. Each item therefore walks, copies and
charges O(items so far): O(k^2) charged scratch and wall, O(k) work.
The charge is `charge` (not a lease), so it is cumulative; peak is separate.

Evidence (charges of >= 5 KB, `collect@resolve.rs:2385`):

| shape | k | bytes | charges | bytes charged by collect | total_scratch_charged |
| --- | ---: | ---: | ---: | ---: | ---: |
| nested | 500 | 31,052 | 489 | 48,907,824 | 145,378,227 |
| nested | 1000 | 62,052 | 989 | 195,096,074 | 387,711,354 |
| nested | 2000 | 124,052 | 1989 | 779,222,574 | 1,164,206,599 |
| json | 1000 | 34,052 | 980 | 112,988,610 | 217,493,612 |
| json | 2000 | 68,052 | 1980 | 451,033,110 | 659,696,984 |
| string | 1000 | 30,052 | 980 | 112,988,610 | 211,899,156 |
| string | 2000 | 60,052 | 1980 | 451,033,110 | 648,510,120 |

`collect` charges grow 4.0x per doubling and are 50-67% of the total; every
other site (engine, codec `finish`, `Names::slots`, recognizer `Turn::store`,
`Resolver::readings`) is a single linear charge per call. `work` is linear
(2x per doubling).

Candidates in the plan that are NOT the cause (no charge above 5 KB, flat in
perf after the fix): xml.rs `reading`/`terminator`/`cheapest`/`select`/`skip`
(their `Reader::charge` work is linear; sampled work for `terminator` is 1.4%
of total), `adopt`/`record`/`Node::get`, `decoded()` leaf lookup,
`same_calls`/`calls_key`, recognizer re-reads. The closer-in-value lookahead
(F14/F19) is not involved, so the conditional stop does not apply.

Wall time on the old code: nested 378 ms (k=1000), 939, 3069, 9035 ms (k=8000).

## Not a root cause of the sized-budget shortfall (separate finding)
After the fix the cost is linear but the constant for output that is only
repeated markup is 52-61 work and 104-262 peak scratch per byte. The 8,000
nested items (496 KB) need 29.2M work vs `sized` = 1M + 32n = 16.9M, and 130 MB
peak vs 64 MiB + 48n = 88 MiB. The work is dominated by the grammar engine
(`Engine::release_deferred` 39%, `Step::advance` 16%, merges 12%), i.e. the
twelve-dialect thread cost, which is paid per byte and is higher on dense
markup. It is not repeated-parameter specific and is outside the identical-
decisions fix.
