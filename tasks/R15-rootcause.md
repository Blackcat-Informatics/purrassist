# R15 root cause (G25): residual superlinear time for many repeated items

Base head 917553e, release builds, 32-core machine under load average 14-16
(wall times are noisy, +-30%; operation counts are exact). Input: `plan_outfits`
(array of objects `wearer: string required`, `garments: array<string>`), k copies of
`<parameter=outfits><parameter=wearer>Alice</parameter><parameter=garments>["hat"]</parameter></parameter>\n`
inside one `<tool_call><function=plan_outfits>`, `ToolRecoveryRequest::sized(Effort::Standard, ..)`
(budget never exhausted, so no lifting was needed).

## Verdict: FIXED (not a cache effect). Two quadratic, uncharged steps.

## Phase 1 evidence

Before (base), all dialects / qwen3-coder-xml.v1 only:

| k | bytes | work (all) | wall (all) | ns/work (all) | work (qwen) | wall (qwen) | ns/work (qwen) |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8,000 | 848,060 | 46.7M | 1.8 s | 39 | 13.8M | 0.77 s | 56 |
| 16,000 | 1,696,060 | 93.5M | 4.0 s | 43 | 27.6M | 2.2 s | 81 |
| 32,000 | 3,392,060 | 187.0M | 7.0-9.1 s | 37-49 | 55.3M | 5.4 s | 97 |
| 64,000 | 6,784,060 | 374.1M | 18.2-25.5 s | 49-68 | 110.6M | 15.4 s | 139 |

Charged work and total scratch are exactly linear (about 55 work/byte all dialects, 16 for qwen alone);
time per charged unit drifts up 2.5x for qwen alone. So the growth lives in the qwen path
(resolution/provenance), not in the other dialects' grammars (those add the linear 39 work/byte and
dilute the drift, which is why the all-dialect table looked "only" about 2.2x per doubling).

Profile (`perf record -g`, line-tables build, qwen only), self time share, 16k -> 64k:
`recognizer::Builder::call_derivations` 10.2% -> 21.3%, `recover::push_ledger` 9.5% -> 20.9%
(inlined `Derivation::push`), libc `memmove` (unsymbolised 0x1b7d..) 6% -> 15%. Every other function's
share fell or stayed flat (`Governor::charge` 9.1 -> 5.8, `Engine::settle` 5.1 -> 2.2, `Step::step` 4.7 -> 2.2).

Operation counts (temporary atomics, removed), qwen only:

| k | `Derivation::push` calls | members compared by `order.contains` | `ScratchMap::insert` calls (Forest index) | entries shifted by those inserts |
| ---: | ---: | ---: | ---: | ---: |
| 2,000 | 8,003 | 16.0M | 18,128 | 7.9M |
| 4,000 | 16,003 | 64.0M | 34,128 | 25.6M |
| 8,000 | 32,003 | 256M | 66,128 | 79.7M |
| 16,000 | 64,003 | 1,024M | 130,128 | 510M |
| 32,000 | 128,003 | 4,096M | 258,128 | 1,779M |

Calls are linear; comparisons x4 per doubling (exactly quadratic); shifts x3-6 per doubling.

Causes:
1. `Derivation::push` (provenance.rs) deduplicated with `Vec::contains`, O(members) per push; one derivation of a
   call holds all of its items' evidence (about 4 nodes per item), so building it is quadratic.
2. `Forest::intern` stored its dedup index in a `ScratchMap`, a sorted `Vec<(String, NodeId)>`
   (documented `O(n)` insert); keys are `Debug` strings of nodes with spans, arriving in no order, so each new
   node shifts on average half the index. Neither step charged work for its scan or shift.

## Phase 2 fix (identical readings, identical charges)

- `crates/core/src/provenance.rs`: `Derivation` keeps a `BTreeSet<NodeId>` beside `order`; `push` and
  `contains` are O(log n); first-seen order and equality semantics unchanged.
- `crates/core/src/contract.rs`: new `pub(crate) ScratchOrdMap<K, V>` (BTreeMap storage) that charges exactly the
  capacity steps a `ScratchMap<K, V>` would (same `grown_capacity`, `(K,V)` at `ScratchCharge::CHARGE`, charged
  before the entry is stored, released on drop), so `work`, `peak_scratch_bytes` and `total_scratch_charged` stay
  byte-identical (verified below). `Forest.index` uses it. The public `ScratchMap` is unchanged (no API change).
  Uncharged heap beyond the charge: BTree node overhead, O(1) per interned node, bounded by the charged
  `EvidenceEntries` and by the node's own 6x-size intern lease.
- Tests added: `contract::tests::ordered_index_charges_like_scratch_map_and_inserts_in_log_steps` (identical
  total/live charge vs `ScratchMap`, released on drop, compare count within 4n(2 log n + 8) via a counting `Ord`),
  `provenance::tests::derivation_membership_is_a_set_lookup`, and `tool_call_cost.rs`
  `repeated_nested_items_with_array_children_cost_is_linear` (k = 500..4000, work and total scratch within 1.05x of the
  byte ratio, k items recovered, wall recorded; about 1.2 s at k = 4000 in debug).

## After (same machine and load)

| k | work (all) | total scratch (all) | wall (all) | ns/work (all) | wall (qwen) | ns/work (qwen) |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8,000 | 46,747,699 | 2,916,114,099 | 1.36 s | 29 | 0.68 s | 49 |
| 16,000 | 93,503,700 | 5,832,640,293 | 3.0 s | 33 | 1.45 s | 52 |
| 32,000 | 187,023,700 | 11,665,364,645 | 7.8 s | 42 | 2.20 s | 40 |
| 64,000 | 374,063,700 | 23,330,813,349 | 11.2-15.5 s | 30-41 | 4.66 s | 42 |

Work and scratch columns equal the base values exactly (e.g. 46,747,699 / 2,916,114,099 at 8k; qwen 850,927,546 and
6,810,782,604 scratch at 8k and 64k). Qwen-only time at 64k fell from 15.4 s to 4.7 s and now doubles per doubling;
ns/work is flat within load noise. Post-fix profile at 16k vs 64k (all dialects) is stable: `Governor::charge`
15.3 -> 14.5%, `Step::step` 6.3 -> 7.3%, `Engine::settle` 5.9 -> 6.6%; `call_derivations`, `push_ledger` and
`memmove` are out of the top 14.

## Test results

- `cargo test -q -p purrassist-core --test tool_call_cost --test tool_call_goldens --test tool_call_recover --test tool_call_rules`: 19 + 117 + 56 + 44 pass; `--lib`: 124 pass (2 new).
- All goldens and PINNED_*_DIGEST unchanged (no re-bless; charges byte-identical, so the Digest rule needs no re-pin).
- `make corpus` exit 0, `make hygiene` exit 0, `cargo clippy -p purrassist-core --all-targets -- -D warnings` clean, `cargo fmt --check` clean.
- Full `cargo test -p purrassist-core`: all pass except `receipt_wire::golden_tool_call_invalid_utf8_receipt_matches_the_committed_fixture`,
  which fails identically at base 917553e (got `work 8026, peak_scratch 151345, total_scratch_charged 523479`
  vs fixture `7769, 159393, 453471`; got is the same with and without this change). Pre-existing stale fixture
  `crates/core/tests/data/receipts/v3-tool-call-invalid-utf8.json` (charging from R13/R14 not re-recorded); unrelated to R15 but
  it should be refreshed under the Digest rule by whoever owns it.
- Probe example and Cargo.toml entry removed; no perf.data or profraw files left.
