# R14 review (G22, Mistral v3 call arrays) -- independent

Scope: uncommitted tree vs HEAD bdb102a: grammar/engine.rs (HeldFailures, hold_failures, settle, abandon, close, release), tool_call_cost.rs and support, tool_call_recover.rs re-pins, docs/RESOURCES.md (incl. DeepSeek "not covered" bullet vs issue #10).

## Evidence

1. Identity. Release CLIs built from HEAD and from the tree, default dialects, JSON compared with `usage` stripped (budget kept), plus exit code.
   - 2,445 cases over 489 goldens with `tools_openai` (original + 3 random byte mutations: delete/truncate/replace; plus `--effort maximum`): 0 differences.
   - 270 cases of Mistral v3 arrays, k in {1,2,3,5,17,100,256,1024,2048} x 15 shapes (ok, missing commas all/some, nested brackets in string args, array-valued args, truncated last element, truncated mid-name, prose after, prose before, trailing comma, missing brace mid-array, double comma, two arrays, k separate sections, array + Qwen call): 0 differences, no timeouts. Outputs are real (best_guess with calls), not trivial errors.
2. Index/floor logic (read + reasoned). Old predicate: survive iff some non-speculative live thread of the same attempt has `entry <= failure.entry` (or any such thread when `trailing`). New: floor = min entry per attempt (sort + dedup keeps min); survive iff floor exists and (trailing or floor <= entry). Dropped set is monotone, so examining only on floor change is exact: `settled` is rewritten on every examination and fresh failures are admitted against current floors, so held failures always satisfy the predicate under `settled`. Attempt with no floor: group dropped whole; fresh failure of such attempt not held. `trailing` stability holds because `Engine::collect` sets `base` to the minimum attempt start, so `failure.at >= base` while the attempt lives. Order: groups keep insertion sequence, `of()` yields oldest first = old per-attempt order of the flat vec (drop keeps relative order). The early return (no groups, no fresh) leaves `settled` stale but only causes an extra examination later, never a missed one.
3. Charging. Release, k = 1024..16384 (70 KB..1.13 MB): work/B 79.6 constant, scratch ever charged/B 5502 constant, wall 0.32/0.62/1.27/3.26/7.04 s = 55-78 ns per charged work unit (stable to 1.4x; the drift at 8k+ is cache growth, not a superlinear trend). 1 MiB about 6.5 s. HEAD was 24 s at k=4096 vs 1.3 s now. Live/failed passes uncharged: justified, `live`/`failed` hold only configurations stepped this step, each charged K_THREAD; the floors/fresh vectors are leased. Held-failure examination (per group on floor change, per dropped failure, per held failure copied by a stuck record, per failure removed by abandon) is charged.
4. Digest rule. Patched `wire_digest` (scratch worktrees, removed) to dump `result.to_node().to_json()` for all 7 pinned inputs (3 order permutations, prefix, 2 ambiguous, prefix-ambiguous): HEAD vs tree identical after removing `usage` (all 7 True), raw differ (usage only); tree passes with the new constants.
5. Tests.
   - `mistral_v3_call_arrays_cost_is_linear` PASSES ON HEAD (ran it there: 266 s, work 73.5/B linear), because HEAD charged linear work. It guards charged-structure regressions (and 1 MiB dense completion would time out on HEAD), but does not itself detect the old uncharged quadratic. Honest in its doc comment (says the unit test counts examination).
   - Unit test mutation checks in a scratch tree: forcing `examine = true` fails (`[1,1]` vs 0); removing the per-failure drop charge fails (1 vs 2501). A re-filter that scans uncharged would pass, which is inherent to any charge-based check.
   - Dense test table row and 1-MiB-class `mistral-v3-array` shape are present; dense output reproduces the RESOURCES table (Kimi 142.3, v3 array 79.6/184.9 at 64 KiB, DeepSeek-V3 303.0).
6. Docs. Table, 53-142 range, 24% margin ((176-142.3)/142.3), 4 KiB figures consistent with measurement. DeepSeek bullet matches issue #10: 238/460/911/1813 at 8/16/32/64; wall 0.3/3.8/42 s at 16/32/64; plain `sized` fails from k>=16; pre-existing on main 943fa04.
7. Gates: `cargo test -p purrassist-core --test tool_call_cost --test tool_call_goldens --test tool_call_recover --test tool_call_roundtrip` (17/115/56/15 pass); `--lib engine` pass; `make hygiene` OK; `cargo clippy -p purrassist-core --all-targets -- -D warnings` clean.

## Findings

REQUIRED: none.

ADVISORY
- A1. `HELD_FAILURE_CHARGE` = 224 under-charges real heap. Measured with a counting allocator (BTreeMap<u64,[u8;120]> + BTreeSet<(usize,u64)>, ascending insert): about 274 B per keyed failure, 240 B per trailing one (Failure is 120 B). Scratch metering is ~18% low for this structure (constant factor, not growth). Raise to 288 or derive from size_of with a node-fill allowance.
- A2. New cost test passes on HEAD (see 5); say so in the commit message so reviewers do not read it as the regression detector. The unit test is the detector.
- A3. RESOURCES.md wall numbers for HEAD (0.12 / 1.19 / 4.2 s at 256 / 1,024 / 2,048) differ slightly from R14-rootcause.md (0.108 / 1.26 / 4.46 s); harmless, but pick one source.
- A4. DeepSeek bullet: "the numbers are the same on the release before it" should read "on main 943fa04" per issue #10; and the wall times were taken in the dev profile (opt-level 3), worth stating.
- A5. The `tests::sizes`-style assertion is absent: `HELD_FAILURE_CHARGE` has no compile-time tie to `size_of::<Failure>()` (unlike CLOSE_PENDING_BYTES), so growth of Failure would silently under-charge.

Scratch worktrees /tmp/r14head and /tmp/r14tree removed and pruned.

VERDICT: PASS

## Recheck

- A1/A5 closed: `HELD_FAILURE_CHARGE` = `Failure::CHARGE` (160) + `HELD_FAILURE_INDEX_BYTES` (128) = 288, with a const assert `size_of::<Failure>() <= Failure::CHARGE`. 288 >= measured 274 (keyed) / 240 (trailing); no undercharge, and it tracks Failure growth. Charge and release use the same constant.
- Margins: dense run at 64 KiB unchanged: Mistral v3 array peak scratch 184.9/B (work 79.6), DeepSeek-V3 303.0/B, Kimi 142.3 work/B; all well inside 368 / 176. Peak is not driven by the held failures.
- Identity re-run on the rebuilt tree vs HEAD: 270 array cases and 2,445 golden cases, 0 differences (usage stripped).
- Digest: coordinator's dumps at 224 vs 288 are stated byte-identical minus usage; the HEAD-vs-tree equality I showed earlier is unaffected by a usage-only change. Consistent with the Digest rule.
- A3/A4 closed: RESOURCES.md Mistral v3 wall times now 0.108 / 1.26 / 4.46 s; DeepSeek bullet cites main 943fa04 and the dev profile.
- Gates: tool_call_cost 17, goldens 115, recover 56, roundtrip 15 pass; clippy -D warnings clean.

VERDICT: PASS
