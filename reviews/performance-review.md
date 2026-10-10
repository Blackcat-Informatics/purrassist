# Performance review: PR #9 (943fa04..HEAD)

## Scope
Diff only: `ArgTree::adopt` / `shift_item` (ir.rs), resolve.rs array collect
renumbering, xml.rs `nesting()` / item frames / `pop()`, recover.rs
`reasoning_blocks` and `calls_key`, and the strict forced-choice scans.

## Method
- Read the diff and the published bound (docs/RESOURCES.md, checked by
  tests/tool_call_cost.rs and tool_call_scale.rs, 1 MiB `argument_work_bound`).
  That bound covers one big string argument. No existing test covers many
  repeated parameters.
- Temporary integration test (removed afterwards) calling `ToolSet::recover`
  in release mode with k = 1k..16k repeated `<parameter=xs>` occurrences.
  Budgets were raised to near-unlimited so runs complete. Receipt usage
  reported. The same inputs ran on a detached worktree of 943fa04 under /tmp
  (removed) as a baseline.
- Isolated microbench of `ArgTree::from_parameters` (public, calls `adopt`)
  with k one-item array values under the same name.

## Measurements (release, Standard effort)
Whole `recover`, ms / charged work (Resource::Work) / total_scratch_charged:

| case | k | bytes | ms | work | scratch_total |
|---|---|---|---|---|---|
| nested items (xs: array of objects, 2 members) | 1000 | 87k | 293 | 5.27M | 0.55G |
| | 2000 | 176k | 867 | 10.6M | 1.66G |
| | 4000 | 354k | 2778 | 21.2M | 5.55G |
| | 8000 | 710k | 8348 | 42.4M | 20.0G |
| | 16000 | 1.43M | 48766 | 85.0M | 75.5G |
| 1-item JSON array `["..."]` | 1000 | 43k | 250 | 3.04M | 0.37G |
| | 4000 | 175k | 2452 | 12.2M | 3.83G |
| | 16000 | 709k | 37421 | 49.0M | 52.8G |
| string array, repeated `ys` | 1000 | 33k | 137 | 2.19M | 0.23G |
| | 4000 | 135k | 1125 | 8.8M | 2.30G |
| | 16000 | 549k | 12476 | 35.4M | 31.2G |

Baseline 943fa04, same inputs (nested items are unsupported there and yield
no call): json1 k=1000/2000/4000/8000 = 477/909/2959/9307 ms,
scratch_total 0.37G/1.14G/3.83G/13.9G; str-items 107/270/816/2692 ms,
scratch_total 0.23G/0.69G/2.30G/8.26G. These match the PR within noise.

Charged `work` is linear in bytes in every case (about 60 per byte). Wall time
and `total_scratch_charged` are quadratic in k, identically before and after
the PR. Under the sized (published) budget the run already stops at k around
1000 with `BudgetExceeded(ScratchBytes)`.

Isolated `ArgTree::from_parameters`, k one-item array values, same name:

| k | 1000 | 2000 | 4000 | 8000 | 16000 | 32000 |
|---|---|---|---|---|---|---|
| time | 1.7 ms | 6.5 ms | 25.6 ms | 144 ms | 501 ms | 1943 ms |

That is about 4x per doubling, so quadratic. On the base commit this
function is linear.

## Findings

### REQUIRED 1: `ArgTree::adopt` is O(members) per call, so O(k^2), and is not charged
ir.rs:325-336. For each array-valued adoption it walks every member of the
object (`members.iter().filter(key == name)`) to sum the earlier items. That
is O(m) per call with m the object's total member count, so O(k^2) for k
repeats of one name. It is also O(m) when the names differ, because the
filter still visits every member. The step takes no `Governor`, and the
reader's charges (xml.rs) are per byte or per event, so the scan is
uncharged. This is new in the PR. The published bound is linear in n, and the
scan is the one super-linear, uncharged step the PR adds.

Impact is bounded in practice. It is about 1.3% of recover wall time at
k=16000, where the pre-existing quadratic dominates. Published sized budgets
stop the run near k=1000, where adopt costs under 2 ms. It becomes reachable
with a caller-owned `Governor` that has large limits, for example
`recover_governed`, or with a future linear fix of the pre-existing cost. At
k=32000 (about 1 MiB of `<parameter=xs>["a"]</parameter>`) adopt alone is
1.9 s, uncharged.

Fix: make it O(1) per adopt. Keep a running item count per name in
`ArgTree`, for example a `Vec<(String, usize)>` or `HashMap<String, usize>`
of "items held under this key", or only the count for the most recent
duplicate key. Update it when pushing the member (`items.len()` for an
array, 1 otherwise). `earlier` then becomes a lookup. If the tree stays
serializable or compared, mark the field `#[doc(hidden)]` or skip it in
equality. Alternatively keep the scan but charge `Resource::Work` for the
members visited, which makes it governed but still quadratic. The counter is
preferred. `shift_item` itself (ir.rs:638) rewrites only the incoming value's
own leaves, once per adopt, so it is linear in the pointers it touches. It
allocates one String per leaf, which is fine.

### ADVISORY 2: resolve.rs collect renumbering is linear
resolve.rs:2310. `before = items.len()` is O(1), and the format of the
pointer is per item. No issue, apart from the pointer allocation per item
being uncharged, as it was before.

### ADVISORY 3: pre-existing quadratic wall time and scratch for repeated openers (not in the diff)
Both the baseline and the PR show wall time and `total_scratch_charged` that
grow about 4x per doubling of repeated parameters, with `work` linear.
`total_scratch_charged` is 20-75 GB for 0.7-1.4 MB. This looks like a
per-opener cost that is O(n) (peak scratch is released between openers, so
only the cumulative counter reveals it). It is outside this PR's diff, but it
means the published linear bound is silent on repeated parameters. Suggest
adding a repeated-parameter case (k = 1k..4k, assert work ratio and
`total_scratch_charged` ratio near the byte ratio) to `tool_call_cost.rs`.
That case would also catch the adopt regression.

### ADVISORY 4: xml.rs item frames
xml.rs:1656 `nesting()` is an O(1) shape match. The `Frame.item` flag is
O(1). `pop` (xml.rs:851) charges the `ArrayItem` edit through `self.record`.
`wrap_item` re-roots the item's own leaves once (O(leaves in item)). With
arrays of arrays of objects, each enclosing level's `adopt` prefix rewrite
touches the leaves again, which is O(depth x leaves). That pattern already
existed for nested objects and is bounded by the depth budget. No per-item
work depends on prior items, except adopt (finding 1).

### ADVISORY 5: reasoning blocks and calls_key
recover.rs:3782 pushes one 32-byte `ReasoningBlock` per block, O(1) per block.
Blocks are bounded by the marker count the recognizer already charged. The
vector is not scratch-leased. `calls_key` (recover.rs:4942) serializes the
blocks into the JSON it hashes and charges `Work` and a scratch lease on
`json.len()`, so the new block hashing is charged in proportion to its size.
The cost is linear, and the unmerged blocks add at most about 40 bytes of
JSON per block.

### ADVISORY 6: strict forced choice scans
recover.rs:4362 `strict`. The content scan stops at the first non-whitespace
byte in each range, so it is at most n bytes in total. It is uncharged but
linear and cheap (one pass, with no allocation beyond the diagnostic). The
`exactly_one` loop formats one diagnostic per extra call, and each includes
the constraint string, so it is O(calls x (label + constraint)). Calls are
bounded by openers. The reasoning loop emits one diagnostic per merged range.
Optionally charge `Work` by the bytes scanned (`size(range.len())`) and cap
the diagnostics, for example report the first and then a count, to bound
output when an adversary supplies many calls under `exactly_one`.

VERDICT: BLOCKED
