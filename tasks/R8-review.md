# R8 review (calls_key distinguishes reasoning block splits)

1. Test: PASS. `calls_key_distinguishes_reasoning_block_splits` (recover.rs ~5712-5750) builds
   readings via `Recovery::recognized` on `<think>a</think><think>b</think>`, asserts merged
   reasoning `[0..32]` and 2 blocks. Control: two fresh readings share a key. Only
   `reasoning_blocks` is mutated (one 0..32/7..24 block; calls count and merged reasoning asserted
   equal); keys differ. Text-only change (`text = 22..24`) also differs.
2. Mutation: reproduced. With calls_key hashing `value.reasoning` ranges instead of blocks,
   `cargo test -q -p purrassist-core --lib calls_key` fails at recover.rs:5735 `left != right`
   ("a different block split over the same merged reasoning must be a different reading"; digests
   equal). Restored; `git diff` byte-identical to the pre-mutation diff (verified with diff).
   (My first restore attempt via a/b-prefix patch failed harmlessly; reapplied with -p0 and verified.)
3. Doc claim: accurate. Combined `marker_matcher` is built once per DialectSet from the union of
   all dialects' markers (dialect.rs ~468-496) and the recognizer uses that single matcher
   (recognizer.rs ~908), so block boundaries do not depend on which dialect reads a call.
   `equivalent_options` compares `Reasoning` segments with `a != b` (recover.rs 3254-3258).
   The "single cheapest block path" and the ~600,000-output exhaustive search are not
   re-verifiable from code here; the doc frames them as "known", which is appropriately hedged.
4. Stage 1 key change (keying on block span+text, commit 248d07a): correct. Spans/text come from
   the shared matcher, so identical calls with different dialect marker lengths cannot arise
   within one set; readings of one output that agree on calls and segmentation agree on blocks, so
   no legitimate merge is blocked. Keying on blocks is strictly finer than merged ranges and the
   only way to lose a merge would be a genuine observable block difference.
Checks: `cargo test -q -p purrassist-core` all pass (1219 tests across targets), clippy
`--all-targets -D warnings` clean.

VERDICT: PASS
