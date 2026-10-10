# R10 review (G18 repeated-parameter quadratic cost)

## Scope
Uncommitted diff vs 3111582: resolve.rs, tests/tool_call_cost.rs, docs/RESOURCES.md, docs/tool-calls/INTEGRATION.md.

## Evidence
1. Root cause confirmed. HEAD `collect` and `typed()` Array branch called `combined_bytes(base, alt)` -> `base.value.estimated_bytes()` (full walk) and `base.clone()` per item per base: O(items so far) walk, copy and charge per item. `work` stays linear (1 per combination). Matches R10-rootcause.md.
2. Fix equivalence. `extend_readings` iterates bases in order and alternatives in order (reading-major, same as HEAD); same Work-then-Scratch charge order; `cost`, `blocker`, `ledger` combination unchanged; empty alternatives list yields empty `next` as at HEAD. Multi-reading: every alternative except the last still goes through `combined_bytes` (full copy charge) and `clone()`; only the last alternative takes the base by move and is charged `appended_bytes` (item weight + 2 slots for Vec growth + ledger entries), because no copy of the base is made. `Sized.bytes` tracks `Node::estimated_bytes` exactly (array = CHARGE + len*CHARGE + sum of items; each push adds slot + weight), so charges for the copy path equal HEAD's. Nothing held is uncharged: the pushed item clone is charged by weight; the moved base was already charged when created. Every `?` still propagates StopReason; no swallowing. The `unreachable!` arm is truly unreachable (None only after the last alternative). Only behavioural difference: usage numbers (total_scratch_charged, peak) are lower for single-reading arrays, so exhaustion points move later; intended.
3. Oracle: `cargo test -p purrassist-core --test tool_call_goldens --test tool_call_rules --test tool_call_recover` all pass (115/56/43), no golden or digest edits in diff (pinned digests unchanged; their inputs have no repeated array so usage is unchanged).
4. Cost test passes (13 tests, 6.8 s total; repeated test a few seconds). Measured (this run): nested k=4000 761 ms, k=8000 1.69 s; JSON 8000 848 ms; string 8000 966 ms; k=1000 ~110 ms. 4k->8k ratio ~2.2x (linear; was 9 s at k=8000 before). Work and total_scratch_charged ratios hold within 1.05x of byte ratio (asserted both directions). Per-byte ceilings asserted.
5. clippy `-p purrassist-core --all-targets -D warnings`: clean.

## Findings
REQUIRED
- R1 Doc/test-comment numeric claim is wrong. RESOURCES.md, the test doc comment and (implicitly) INTEGRATION.md say "from about 5,000 nested items `sized(Effort::Standard, output)` alone exhausts". Computed: Standard preset work 1,000,000 + 32/byte; nested item costs 3,655 work per item of 62 bytes (59/byte), so sized work = 1M + 32n falls below work at n ~ 37 KB, i.e. about 600 to 1,000 nested items (k=1000: needs 3.66M vs sized 2.99M). JSON (52/byte) crosses near 1,500 items, string (61/byte) near 1,100. The ~5,000 figure is only the scratch crossover (peak 262/byte vs 64 MiB + 48n). Fix wording: "from about 1,000 items on Work (about 5,000 on ScratchBytes)". Also INTEGRATION.md "very large such outputs" understates; state ~1,000.
ADVISORY
- A1 "14 GB at k = 8,000" in RESOURCES.md is extrapolated from k=2000 (collect 0.78 GB x16 plus linear rest ~ 13 to 14 GB); not measured. Say "about" / "extrapolated", or note it.
- A2 Plan asked for a k=16k wall smoke (recorded, not asserted) and "k=8k completes under sized(Effort::Standard)"; the second is impossible (below), the first is absent. Wall time is now printed per k, which suffices as a record.
- A3 Elapsed is printed only; consider noting results in the task log.

## Facts for the user decision on per-byte constants (not a recommendation to change)
- Published RECOVER_WORK_PER_BYTE=32 and RECOVER_SCRATCH_PER_BYTE=48 are documented (cost.rs) as headroom over the FAST PATH: 10 to 17 work/byte (26 for OpenAI stringified), 8 to 12 scratch/byte (45 OpenAI). Dense markup of many tiny parameters is outside that measured population: 52 to 61 work/byte, peak scratch 104 (JSON) to 262 (nested) per byte. Cause per root-cause analysis: grammar engine across 12 dialects (release_deferred 39%, advance 16%), per byte and structure-dependent; not repeated-parameter specific and not changed by this fix (work charges are identical to HEAD; HEAD peak scratch was higher).
- So this is pre-existing, not introduced by R10: HEAD's work at these inputs is the same, and HEAD also exceeded sized on scratch (quadratic). The existing 1 MiB test fits because it uses ordinary single-string arguments (fast path); other tests in this run show 3 to 27 work/byte for ordinary 262 KB bodies.
- Deviation judgement: raising the test's budget to 80 work and 320 scratch per byte, with measured values asserted as ceilings, is honest and the doc text discloses it (once R1 is corrected). The failure mode with plain `sized` is an explicit `BudgetExceeded`, linear, never silent. The plan's acceptance line "k=8k completes under sized(Effort::Standard)" cannot be met without changing the published constants (needs about 60+ work and 262 scratch per byte, i.e. roughly 2x and 5.5x the published values) or reducing engine per-byte cost; that is the user's call. Raising constants widens every sized budget (and the published worst-case bound, docs/tool-calls/README.md line ~380, tests at tool_call_cost.rs:478 and the 1 MiB support test) for all callers.

VERDICT: BLOCKED (R1 doc number is factually wrong; code, tests, semantics are sound. Fix the wording and this passes.)

## Recheck
- Numbers verified from my own measurements: nested 3,655 work/item (62 B) vs sized 1M+32n gives 3.66M vs 2.99M at k=1000 (true crossover about 600, so "exhausts at k=1000" is correct, merely not the first point). JSON 1,778 work/item (34 B): crossover k about 1,450 (doc: ~1,500, ok). String 1,845 work/item (30 B): crossover about 1,130 (doc: ~1,100, ok). Scratch: nested peak 65.0 MB at 4k vs sized 79.0 MB (ok); at 8k peak 130 MB vs sized 90.9 MB = 86.7 MiB (docs say 88 MiB; ADVISORY nit, 86.7 would be exact).
- "13.99 GB / 9.0 s on previous head" is now labelled as measured on the previous head; I did not re-measure HEAD (9.0 s matches the root-cause note; 13.99 GB consistent with extrapolation).
- `cargo test -p purrassist-core --test tool_call_cost`: 13 passed, 6.2 s.
- Original REQUIRED R1 is resolved.

VERDICT: PASS
