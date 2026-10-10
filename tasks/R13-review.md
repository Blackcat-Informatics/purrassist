# R13 review (G21: Mistral v11 whitespace-chained calls)

Scope: uncommitted diff vs HEAD 8fb63a4 (engine.rs, recognizer.rs, tool_call_cost.rs,
tool_call_cost_support/mod.rs, RESOURCES.md, INTEGRATION.md). Read-only review; scratch under /tmp, removed.

## Evidence

1. Identity. Built HEAD and the working tree (release CLI). `recover-tool-calls` JSON with
   `receipt.usage` removed, plus exit code, compared on:
   - all 493 goldens' inputs, with their own tools and with a generic tools file, default dialects,
     effort standard and maximum, and `--dialect mistral-v11.v1` / `mistral-v3.v1 + hermes.v1`;
   - 150 hand-built adversarial inputs (v11 chains k=1..8 with newline/space/none/mixed/CRLF
     separators, truncated, garbled, unreadable middle call, duplicate keys, repeated
     `[TOOL_CALLS]`, missing/extra closers, CALL_ID, v3 arrays, hermes, cross-dialect, bare);
   - 1,500 + 1,500 + 3,000 mutated/random inputs (v11 mixes, golden-input splices, v11 token soup, k up to 12).
   About 14,000 runs: 0 differences (byte-identical, same exit codes).
   Code reading: memo keys are append-only history node ids (unique per arena, cleared in
   `Arena::reset`, which only runs when no attempt is live, so no held `Chained` outlives it); a closed
   interior part depends only on the history prefix and the node's own position/cost, not on later bytes,
   so a later byte cannot change it. Release dominance logic matches the old predicate (self skipped,
   same branches, same `calls`/`total` semantics); `HeldSet::floor` is recomputed on `take`, so
   out-of-order release is correct; `pending` key (at, phase, arrival) equals the old
   (at, phase, sequence-in-queue-order) sort.
2. Charging. v11 newline chain, `--effort maximum`, release build: work/byte 74.2 and peak scratch/byte
   338 constant from 128 to 32,768 calls (HEAD: 98 -> 168 work/B and 1,034 -> 3,119 sc/B by 512 calls,
   ScratchBytes exhaustion at 1,024). Wall time linear (0.54 s at 4,096 calls, 7.5 s at 32,768
   calls; time/charged ratio stable). Same for space/none separators and 17 other v11 shapes (unreadable
   middle or every 8th call, garbled, missing closers, fuzzy names, CALL_ID, duplicate keys, name-only):
   all linear, none exhausts at 2,048 calls where HEAD exhausted at 512. The lower charged work vs HEAD on
   the chain is the removed re-segmentation and the removed count^2 release charge; new loops charge
   (`tail`, `parts`, `table`, release pairs, sort, per-part scratch).
3. Fallback. Instrumented a copy of the working tree to print each fallback. Across all ~14,000
   inputs and 40 scalable shapes, the fallback was reached by 7 inputs, all DeepSeek (restarted
   `<tool_calls_begin>` after a truncated call), and 0 Mistral v11 chains (3,000 v11 token-soup
   fuzz inputs, k up to 12, plus all shapes above). In the DeepSeek cases it fires once per section (one
   completion), count independent of k, so no growth. I could not construct a quadratic fallback.
4. Tests. `chained_mistral_v11_calls_cost_is_linear` would fail on HEAD (HEAD exhausts ScratchBytes
   at 1,024 calls and the ratios exceed 1.05 by 512); it checks work, total scratch and peak, three
   separators, 256..4,096. DENSE gets a 16th entry (mistral-v11-chained-calls); tool_call_scale
   release run 55 s total (4 tests), acceptable. `cargo test -p purrassist-core --test tool_call_cost`
   15 passed (5 s); goldens + roundtrip pass, no golden file changed; `make hygiene` OK;
   `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Findings

REQUIRED: none.

ADVISORY:
- A1. The fallback is untested: no test constructs a `Closed::Whole`/`tail == None` history on a
  chained attempt and asserts identity or the documented charged quadratic. Today only an
  indirect golden (deepseek31_rejection_balances_only_read_brackets) reaches it. Consider one
  test that pins the output for that shape.
- A2. RESOURCES.md says the fallback segments "at each of its completions (cost quadratic in
  the chain, charged)" and sits under the Mistral v11 paragraph. By code reading that holds (a `Whole`
  state is memoized and inherited by later nodes), but I could not reach it with any Mistral v11 chain
  and measured it only on DeepSeek, where it is a single completion. The text is accurate as a
  worst-case bound but implies a v11 exposure I could not demonstrate. Suggest saying "not reached by any
  v11 input we found; reached by restarted DeepSeek sections".
- A3. Non-target dialects pay a little scratch: dense DeepSeek V3.1 peak scratch 88.3 -> 100.3 per
  1000 bytes (+14%), V3 293 -> 303 (+3%); work unchanged (+0.01%). The published dense table rows for
  these (86.7, 291.8) were not refreshed (already slightly stale at HEAD). Within the `sized` budget.
- A4. New O(count) vectors at release (`order`, `tables`, `keep`, about 40 bytes per held completion)
  and the `pending` list in `close` are uncharged scratch; small beside the 338 B/byte charged.
- A5. The same RESOURCES.md paragraph still lists the v3 quadratic wall time (uncharged) as open;
  that is R14, outside this task. DeepSeek restart-repeat chains remain superlinear (work/B 489 -> 1,666
  at 64 -> 256 sections) identically on HEAD; not an R13 regression.

Decision check: user's "fix (1)" is met for every shape tried; common shapes do not go quadratic.

VERDICT: PASS

## Recheck

- Doc text matches my instrumentation: the fallback is described as equal-cost readings reaching
  different cuts or differing skips, charged, not reached by any v11 chain tried, reached only by
  DeepSeek-V3.1 sections restarted after truncation, once per section. A2 resolved. A1 resolved by
  `whole_segmentation_fallback_is_correct_and_charged` (present in the diff, passes).
- Spot-check of the re-measured rows: the dense test at 64 KiB gives V3.1 100.3 and V3 303.2 peak
  scratch per 1000 bytes; the table rows are measured at about 11.5 KB (98.8, 301.6), the same
  roughly +1.5 offset the old rows had against the 64 KiB figures (86.7 vs 88.3). Consistent. The
  margin against 368 is 22% (21% at 64 KiB), as stated. A3 resolved.
- `cargo test -q -p purrassist-core --test tool_call_cost`: 16 passed.
- Remaining advisories A4 and A5 unchanged and acceptable.

VERDICT: PASS
