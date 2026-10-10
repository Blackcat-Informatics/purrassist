# R15 review (G25 residual superlinear wall time)

Scope: uncommitted diff vs HEAD 7136da7 (contract.rs ScratchOrdMap, provenance.rs Derivation.seen + Forest.index,
tool_call_cost.rs new test). Reviewed against remediation-plan R15 and R15-rootcause.md.

## Evidence
1. Root cause plausible from code: `Derivation::push` was `Vec::contains` (O(members)); one derivation holds ~4 nodes per
   item, so quadratic; `ScratchMap::insert` is binary_search + `Vec::insert` (O(n) memmove) over `Debug`-string keys that
   arrive unordered. Both uncharged. Recorded counts (x4 per doubling compares, ~x3-6 shifts) match that.
2. Identity: `order` push/first-seen order unchanged; `seen` is a pure function of `order`, so derived PartialEq/Clone
   are equivalent; no serialization of Derivation. `NodeId` is Ord. Forest ids come from `nodes` order, index only maps
   key to id, so ids/order unchanged. `ScratchOrdMap::insert` charges only when key is new and len == capacity, with
   `grown_capacity(len, capacity, 1)`, delta = (target-cap)*max(CHARGE,1) of `(K,V)`, charge before store, one lease per
   step released on drop. This matches `ScratchMap::insert` -> `ScratchVec::reserve` (len==capacity check; try_reserve_exact
   makes real capacity == target). Unit test compares total and live charge against ScratchMap for n=1k/4k/16k and zero
   live after drop.
3. Charges unchanged, measured (release, HEAD in a detached worktree vs HEAD+patch): full Usage (work, peak scratch, total
   scratch, evidence entries) and an FNV hash of Outcome+alternatives are identical for k=1,7,300,8000,16000,32000 (all
   dialects) and qwen-only k=8000..64000 (e.g. work 46,747,699; total scratch 2,916,114,099 at 8k). `receipt_wire` passes at
   HEAD and with the change (7 passed both). PINNED_*_DIGEST tests (tool_call_recover.rs) pass.
4. Memory: BTreeSet<NodeId> (and BTreeMap nodes) are not charged. Parity with the existing uncharged `order: Vec<NodeId>`
   and key Strings, which the 512+6x intern reservation covers; BTree overhead is O(1) per interned node. Acceptable,
   but see advisory A1.
5. Wall time (qwen only, release, ns per charged work, HEAD vs patch): 8k 50/50, 16k 54/47, 32k 84/36, 64k 136/37.
   Patched is flat; HEAD drifts 2.7x. All-dialect patched: 27, 29, 39(noisy) at 8k/16k/32k, HEAD 42, 45, 54.
6. Full `cargo test -q -p purrassist-core`: 0 failures. `python3 scripts/check-repository.py` OK (no HashMap etc.).
   `cargo clippy -p purrassist-core --all-targets -- -D warnings` clean.

## Findings
REQUIRED
- R1. No test fails on the old behaviour; the plan demands a deterministic operation-count test.
  * `ordered_index_charges_like_scratch_map_and_inserts_in_log_steps` counts `Ord::cmp` calls. The old sorted-Vec
    `ScratchMap` also does `binary_search` (O(log n) compares) and its cost is the uncounted `Vec::insert` memmove, so the
    assertion (<= 4n(2log n+8)) passes for the old structure too. It proves charge parity (good) but not the speedup.
  * `derivation_membership_is_a_set_lookup` uses 2000 nodes; the old `Vec::contains` passes trivially (functional only).
  * `repeated_nested_items_with_array_children_cost_is_linear` asserts charged ratios, which were already linear at HEAD
    (the gap was that charge hid the time). It would pass at HEAD.
  Fix: make the quadratic step observable deterministically, e.g. a counting element type for Derivation membership (a
  `NodeId`-like Ord counter is not possible since NodeId is fixed, so instead test the index with a key whose `Ord` and
  `Clone`/move are counted, or assert via a generic structure whose scan count is exposed), or a cfg(test) counter
  of members compared/entries shifted that the old code would exceed; at minimum a bounded-time test is not deterministic,
  so prefer an op counter. Verify the new test fails against HEAD's implementation.
ADVISORY
- A1. `Derivation.clone()` now clones a BTreeSet as well (uncharged heap, ~size of order); `clone_reservation` does not
  count it. Same order of magnitude as the existing uncharged `order` Vec clone; note in docs or fold into reservation.
- A2. Derivation derived `PartialEq` compares `seen` redundantly with `order` (cost O(n log n)-ish extra); could compare
  `order` only via a manual impl. Harmless.
- A3. At 32k all-dialect ns/work moved 29 -> 39 in one run; the qwen-only table is flat (36-37), so this is load noise.

VERDICT: BLOCKED

## Recheck

- Revert demonstrations (provenance.rs backed up, reverted, run, restored; diff stat unchanged afterwards):
  `Derivation::push` back to `!self.order.contains(&node)` fails `derivation_push_compares_ids_in_log_steps`
  (16000000 id comparisons > 240000). `Forest.index` back to `ScratchMap` fails `forest_index_insert_shifts_no_entries`
  (3999421 entries shifted for 4000 inserts). R1 is resolved: both are deterministic counts.
- cfg(test) safety: the `NodeId` counting `PartialEq/Eq/PartialOrd/Ord` impls and the `SHIFTED` counter in
  `ScratchMap::insert` exist only under `cfg(test)`; non-test builds still use the derived traits. The test impls compare
  `self.0` exactly as the derive does, `partial_cmp` delegates to `cmp`, so ordering and eq are consistent. No `Hash` is
  derived or used on `NodeId`. The manual `Derivation` `PartialEq` (output, cost, order) is equivalent to the old derive
  because `seen` is a function of `order`. The `SHIFTED` update is a test-only side effect of the existing branch.
- Full `cargo test -q -p purrassist-core`: 0 failures (lib 126 passed). `check-repository.py` OK. Clippy -D warnings clean.
- A1 (uncharged `seen` clone) is documented and matches the uncharged `order` clone: accepted.

VERDICT: PASS
