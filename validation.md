# Validation — issue #8

## Baseline (base 943fa0413f68888437bb2088f15122c63a3ec0b8, before any change)

- `python3 scripts/fuzz.py --target tool-call` (bounded default, 1,000 mutations): **FAILS on base**
  after ~500 executions. Panic at `crates/core/examples/fuzz_tool_call.rs:495`:
  `rejected attempt 218..322 fails at 159, beyond a literal past its end`. This is the
  rejected-span oracle tracked by issue #6 and is not caused by this work. Later fuzz runs are
  compared against this signature; any new invariant failure is this branch's responsibility.

## Focused checks per task

(filled per task)

### Task 1 (nested-parameter array items)
- `cargo test -p purrassist-core --test tool_call_goldens --test tool_call_rules --test correction_evaluation --test tool_call_roundtrip --test tool_call_recover`: pass (93/41/10/15/41 after the orchestrator added the $ref golden).
- `make corpus`: pass (implementer run); corpus 18166 cases, 0 incorrect, abstention recall 100%.
- `cargo clippy -p purrassist-core --all-targets -- -D warnings`: clean; `cargo fmt --check`: clean.
- Re-bless changed no existing golden; 9 new goldens.

### Task 2 (strict forced tool choice)
- `cargo test -p purrassist-core --test tool_call_goldens --test tool_call_recover --test tool_call_rules`: pass (100/43/41).
- `cargo test -p purrassist-core --doc tool_call`: pass (204).
- `cargo test -p purrassist-host --test tool_call_tools`: pass (21); `-p purrassist-cli --test tool_calls_cli`: pass (24).
- `python3 scripts/test-verify-published.py`: pass (fixture recaptured; diff adds only the `tool_choice_strict` input member and a description clause).
- `python3 scripts/test-fuzz.py`: pass; the 5 new strict seeds run clean through `fuzz-tool-call`.
- `make hygiene`: pass; `cargo clippy --workspace --all-targets -D warnings`: clean; `cargo fmt --check`: clean.
- Re-bless changed no existing golden; 6 new goldens.

### Task 3 (marker-free reasoning blocks)
- `cargo test -p purrassist-core --test tool_call_recover --test tool_call_goldens --test tool_call_rules --test tool_call_roundtrip`: pass (50/100/41/15); `--doc tool_call`: pass (206).
- `cargo test -p purrassist-host --test tool_call_tools`: pass (22); `-p purrassist-cli --test tool_calls_cli`: pass (24).
- `cargo test -p wasm-harness` and `cargo run -p wasm-harness --example portable-core`: pass with no re-pin (implementer run). `scripts/check-wasm.py` (native/WASI transcript comparison) was NOT run in this task.
- `python3 scripts/test-verify-published.py`: pass (recaptured; adds only `reasoning_blocks`). `python3 scripts/test-fuzz.py`: pass.
- `make hygiene`: exit 0 (orchestrator). `make corpus`: exit 0 after the `calls_key` change (orchestrator). clippy `-D warnings` and fmt: clean.
- Golden re-bless: 481 files changed. 479 only add `reasoning_text`. 2 Ambiguous goldens reorder `readings`/`ledger_rules`, because tie order is the wire-JSON digest, which now includes `reasoning_blocks`. The reorder was verified independently by the reviewer.
- Four pinned digests were re-pinned; the reviewer verified that restoring the old pins passes once `reasoning_blocks` is removed.

### Bounded tool-call fuzz on the branch (Tasks 1-3)
- `python3 scripts/fuzz.py --target tool-call` FAILS after ~1001 executions, at `fuzz_tool_call.rs:1056`: "rejected attempt 0..165 runs past where it failed (26) without its closer".
- **Attribution:** the retained input (`.cache/fuzz-tool-call-rjgczyq9/current.bin`) replayed through the probe built from base 943fa04 panics with the identical message, at base line 988. This is the pre-existing rejected-span oracle failure tracked by #6, not a regression from this branch. The new `reasoning_blocks` and strict-choice invariants did not fire.

## Full gate (Task 4)
- `make check` on 248d07a: **exit 0** (fmt, hygiene, helper checks, lint, test, doc).
- `make wasm-test` (`scripts/check-wasm.py`, native/WASM transcript comparison): **NOT RUN**. The script stopped before any comparison with "Node identity mismatch: expected v26.10.0, got v26.11.1"; the local Node differs from the pinned version. This is an environment limitation, not a branch failure. Native evidence: `cargo test -p wasm-harness` and the `portable-core` example pass with the existing pins. The WASM comparison remains unverified for this branch.
- `make fuzz` (tool-call target): fails on the pre-existing #6 oracle; attributed to base by exact replay (see above).

# Stage 2

## Review path
**Full path.** The PR changes shared contracts (the `tool_call.recover.v1` wire value, the MCP input/output schemas, and the public `ToolChoice`/`ToolCalls`/`ReasoningSegment` types) and the codec that parses untrusted model output.

## Evidence captured (raw/)
- PR #9: head 248d07a; base main = 943fa04, with no base drift. There are 0 reviews, 0 inline comments and 0 review threads. The only comments are the Stage 1 plan and handoff.
- Hosted CI: all 8 Verify jobs did not start; `main`'s last Verify run shows the same. Validation is local.
- Issue analysis: the Stage 1 `issue-analysis.md` is reused. The issue has no new requirements, and its 5 comments are this workflow's own plan and task posts.

## Stage 2 remediation

### R1 / G1: Node repin (c1fa81d)
- `make wasm-test`: **exit 0**. Native, WASI (core/syntax/host) and browser-WASM (core) transcripts all match, under local Node v26.11.1. The WASI legs fetched the project's pinned, digest-verified sysroot into the worktree's gitignored `.cache/wasi`; no system tooling was installed.
- `python3 scripts/check-repository.py`: pass. `python3 scripts/test-fuzz.py`: pass (66).
- Review `tasks/G-R1-review.md`: PASS. The binary digest matches the official SHASUMS256.txt (not GPG-verified); the LICENSE is byte-identical upstream.

### R2 / G2 (CRITICAL), G16, A6-doc: scope graph
- Built CLI `recover-tool-calls`, re-executed by the reviewer:
  - p9r → nested `{"children":[{"name":"a","children":[{"name":"b"}]}]}`, Resolved, exit 0, both with default effort and with `--effort exact`.
  - p10_ref_plan → integer sizes, Resolved.
  - rm/keep → `path` stays in its item, Resolved.
  - opaque → `best_guess`, `nothing_usable`, exit 4.
- `cargo test -p purrassist-core --test tool_call_goldens --test tool_call_rules --test tool_adapters --test tool_call_recover --test correction_evaluation --test grammar_codecs --test tool_call_cost`: pass (109/42/15/51/10/15/12). `--doc`: pass (375). `make corpus`: pass. clippy and fmt: clean. `check-repository.py`: pass.
- Corpus: 18,176 cases, **0 incorrect**, abstention recall 100%.
- A4: the base corpus run on head code reproduces the base tally (11,261/0/5,177). The implementer compared per-case dumps against a base worktree and found them identical; the reviewer checked the tally arithmetic.
- Adversarial schema probes (cyclic `$defs`, a 3000-long ref chain, a 2000×50 anyOf, 5000-deep) give explicit `BudgetExceeded(Work|Depth)` in under 0.1 s, with no panic.
- Pinned digests unchanged. Goldens: 1 removed (the wrong `…_are_siblings`), 9 added; no existing golden changed.
- Review `tasks/R2-review.md`: PASS (and recheck PASS).

### R3 / G14 (HIGH), G11: reasoning text never reverses
- CLI `recover-tool-calls --dialect anthropic-tool-use.v1` on `{"type": "redacted_thinking", "data": "}x` gives exit 0, `best_guess`, block span 0..41 and text 39..41. HEAD printed the reversed `text 39..38`.
- Mutation check: the new golden test, the all-goldens slicing test and the capture test all fail on HEAD; the HEAD fuzz probe panics on the new seed.
- 27 CLI probes across dialects give output identical to HEAD, so no legitimate call is suppressed.
- `tool_call_goldens` (112), `tool_call_recover` (51), `tool_call_rules` (42), `--doc` (376), `test-fuzz.py` (66) and clippy pass. Only the new golden was added.
- Review `tasks/R3-review.md`: BLOCKED on a stale description, fixed by the orchestrator; recheck PASS.

### R4 / G3 (HIGH), G15, G17, A3: strict choice
- Built CLI, p7 with hermes.v1 and all strict flags: exit 4, `passed: false`, `nothing_usable`, usable 0/0, `emitted []` (it was exit 0 and `run` on the previous head).
- A single valid strict call still passes (exit 0, 1 emitted). Non-strict `Forced` on p7 is identical to the previous head.
- MCP stdio: p7, the Anthropic `disable_parallel` form and the G15 combination behave as specified.
- Core, host (25) and CLI (26) suites pass, as do `--doc` (377), `test-fuzz.py` (68), `test-verify-published.py` (10), `make hygiene` and clippy.
- Mutation checks: removing the rejected-attempt counting, the zero-usable guard, the G15 error or the fail-closed merge each fails its named test.
- Goldens: 5 `forced_*` files changed only `usable_calls` 1→0; 2 added. Pinned digests unchanged.
- Published replay recaptured: description strings only.
- Fuzz seeds 3/4 re-encoded to `0xc6` (a meaning correction), and seeds 5-7 added; all strict seeds print `ok`.
- Review `tasks/R4-review.md`: PASS.

### R5 / G5 (HIGH), A5, G6: linear item numbering, canonical keys, evidence by span
- Built CLI p6 with `--policy tolerant`: `decode-typed` spans are `/xs/0` 43..45 and `/xs/1` 74..78. The first item's span was previously reported for both.
- `adopt` is O(1) per adoption: a unit-test counter touches at most one member per adopt at k=32768. Mutation checks (implementer): breaking canonical keying or span lookup fails the alias, codec and golden tests.
- `tool_call_cost::repeated_parameters_cost_is_linear`: the work ratio equals the byte ratio (7.99×). Scratch is still 36× for an 8× byte increase; R10 targets this.
- `make corpus`: exit 0 (orchestrator). Focused suites, `--lib ir`, `--doc` (377), clippy and fmt pass. Only the new golden was added; pinned digests unchanged.
- Review `tasks/R5-review.md`: PASS.

### R10 / G18 (user scope): repeated parameters are linear
- Root cause (`tasks/R10-rootcause.md`): `Resolver::collect` and the `typed()` array branch re-measured and cloned the whole partial array for every item, making charged scratch and wall time O(k²). Found by backtrace-recorded scratch charges.
- Fix `extend_readings`: readings, order and decisions are identical. Goldens pass with no re-bless; pinned digests unchanged.
- Measured with nested items, before → after: k=8000 scratch 13.99 GB → 1.54 GB, wall time 9.0 s → 1.8 s. k=16k/k=4k is 4.65× for 4× the bytes.
- `tool_call_cost::repeated_parameters_cost_is_linear` asserts work and scratch within 1.05× of the byte ratio for nested, JSON and string items, and runs in about 7 s.
- The default `sized` budget still exhausts on Work at about 1,000 nested items. This is pre-existing; R12 raises the published constants (user decision).
- `make corpus` exit 0 (implementer). Review `tasks/R10-review.md`: BLOCKED on doc numbers, which were fixed; recheck PASS.

### HashMap fix (342f2d4)
- R5 had introduced `HashMap` (a seeded hasher, i.e. ambient randomness) into `purrassist-core`. The commit hook does not run `make hygiene`, so this went uncaught until R12's run of `check-repository.py`. It was replaced with `BTreeMap`; `check-repository.py` passes, as do the `ir` lib tests, goldens, rules and clippy.
- **Process change:** every later fix runs `make hygiene` before commit.

### R12 / G20 (user decision): published per-byte budget raised
- Measured all 12 dialects at 1 MiB (`tasks/R12-measurements.md`): maximum work 138.3 per byte (kimi-k2), peak scratch 293 per byte (deepseek-v3), evidence 0.101 entries per byte (qwen calls).
- Constants: work 32 → 176, scratch 48 → 368, and new evidence ⌈n/7⌉. Margins: 25-27% on work and scratch, 41% on evidence.
- `dense_markup_fits_published_budget` (15 shapes at 64 KiB) and `tool_call_scale::one_mib_dense_markup_fits_published_budget` (1 MiB, corpus lane). The reviewer showed both fail with the old constants.
- Goldens and pinned digests unchanged. The published transcript changes only its budget numbers.
- Cost, goldens and recover tests, `test-verify-published`, `make hygiene` (exit 0) and clippy pass. Review `tasks/R12-review.md`: PASS.

### R13 / G21 (user decision): Mistral v11 chained calls are linear
- Root cause (`tasks/R13-rootcause.md`): `Engine::report` re-segmented the whole chain at every completion (`fields()` cloned 525,823 times at k=1024), plus two uncharged per-byte scans (`Recognizer::settle` and `Engine::held_floor`).
- Fix: memoized chained segmentation, dominance-aware materialization, `HeldSet`, and a `BTreeMap` for pending.
- Measured at k=1024, per byte: work 275 → 64, peak scratch 3,420 → 218; wall time 968 → 108 ms. 1 MiB (18k calls) fits plain `sized`.
- Identity: about 14,000 differential runs of HEAD against the tree (493 goldens, 150 adversarial chains, 6,000 fuzzed inputs) were byte-identical apart from usage. No golden changed and the pinned digests are unchanged.
- The fallback whole segmentation fires only on DeepSeek-V3.1 sections restarted after truncation. It is charged and has a new test.
- DeepSeek dense rows were re-measured; the scratch margin is now 22% (stated).
- Cost (16), goldens, roundtrip and `tool_call_scale` pass; `make corpus` exit 0 (implementer); `make hygiene` exit 0 (orchestrator); clippy and fmt clean.
- Review `tasks/R13-review.md`: PASS, with recheck PASS.
- **New pre-existing finding:** restarted DeepSeek chains are superlinear, identically on HEAD (489 work/byte at 64 sections, 1,666 at 256). Held for a user decision.

### R14 / G22 (user decision): Mistral v3 call arrays are linear and fully charged
- Root cause (`tasks/R14-rootcause.md`): `Engine::hold_failures` took no governor and re-filtered every held literal failure on every byte. Entries examined: 290M at k=2048 and 1,158M at 4096. Charged work stayed linear (73.5 per byte) while wall time went from 0.108 s to 24 s at k=256 and 4096.
- Fix: held failures are indexed by attempt and begin position, and only fresh or droppable ones are examined. Every visit is charged, and so are the release-time vectors. `HELD_FAILURE_CHARGE` = `Failure::CHARGE` + 128 = 288, which covers the measured 274.
- Measured: 79.6 work per byte from 256 calls to 1 MiB; wall time at k=4096 dropped from 24 s to about 1 s; 1 MiB (15,195 calls) runs in about 6 s, where before it did not finish in 300 s.
- Identity: 2,715 differential cases (2,445 golden-derived and 270 array shapes) are identical apart from usage. Goldens unchanged.
- Digest rule: all 7 pinned results were dumped at charge 224 and at 288 and are byte-identical apart from `usage`. `PINNED_DIGEST` and `PINNED_PREFIX_DIGEST` were re-pinned (twice in R14: once for the charged loops, once for the 288 charge, both usage-only).
- `engine::tests::a_step_over_held_failures_costs_what_it_changes` detects the old re-filtering. `mistral_v3_call_arrays_cost_is_linear` does not, because HEAD's charged work was already linear.
- Margins: Kimi-K2 142.3 work per byte (24% under 176); DeepSeek-V3 303 scratch per byte (22% under 368).
- Cost, goldens, recover and roundtrip tests pass; `make corpus` exit 0 (implementer); `make hygiene` exit 0 (orchestrator); clippy and fmt clean.
- Review `tasks/R14-review.md`: PASS, with recheck PASS.

### G24: restarted DeepSeek-V3.1 sections (user decision)
- Filed as **issue #10** with a reproduction. Reproduced by the orchestrator on the built CLI: work per byte 238, 460, 911 and 1,813 at 8, 16, 32 and 64 sections; wall time 0.3, 3.8 and 42 s at 16, 32 and 64. `main` 943fa04 gives the same numbers.
- Accepted for #8 as a documented "not covered" shape (RESOURCES.md cites #10).

### R6 / G7 and R5-advisory 2: reasoning markers inside JSON strings of repeated or alias parameters stay value text
- Built CLI: p12 and p12j give exit 0, with Bob's `"garments":["a <think> scarf"]` and `reasoning: []`. HEAD gave exit 4 and `{}`.
- `marker_cut` uses a `DataIndex` built once per reading: sorted string-leaf spans with prefix-max, and each marker is one `partition_point`. It is charged, and its scratch is a RAII lease.
- Differential against HEAD over 576 + 174 probes: the only differences are the intended repeated-parameter fixes.
- Wall time with markers equals the marker-free baseline (single array of 128k items: 17.1 s, against 39.1 s at HEAD).
- Tests: 2 goldens, an alias-repetition test, lookup-step unit tests and `reasoning_marker_in_every_repeated_item_is_linear`. Fail-without-fix confirmed. `make corpus` exit 0; `make hygiene` exit 0.
- Review `tasks/R6-review.md`: BLOCKED on an uncharged quadratic, fixed; recheck PASS.

### Receipt fixture refresh (7136da7): process gap found
- `receipt_wire::golden_tool_call_invalid_utf8_receipt_matches_the_committed_fixture` failed on the pushed head 917553e. The charging changes in this series moved its usage numbers, and none of the per-fix focused test lists included `receipt_wire`. R15's full-crate run found it.
- Refresh under the Digest rule: the current receipt equals the fixture field for field once `usage` is set aside. Only work (7769 → 8026), peak scratch and total scratch were updated. `receipt_wire` passes (7).
- **Process change:** every later fix runs the whole `cargo test -p purrassist-core`, not a selected list. The final `make check` remains the full gate.

### R15 / G25: residual superlinear wall time was real (fixed)
- Root cause (`tasks/R15-rootcause.md`): `Derivation::push` deduplicated with `Vec::contains` (16M → 4.1G comparisons as k went 2k → 32k), and `Forest::intern`'s sorted-Vec index shifted O(n) per insert (1.78G entries at 32k). Both were uncharged.
- Fix: a `BTreeSet` beside `order`, and a `BTreeMap`-backed `ScratchOrdMap` with identical charges. Charged usage and outcomes are identical to HEAD for k up to 64k (reviewer).
- Time per charged unit, qwen-only: 50/54/84/136 ns before and 50/47/36/37 ns after, at k = 8k/16k/32k/64k.
- Revert-detecting tests: `derivation_push_compares_ids_in_log_steps` and `forest_index_insert_shifts_no_entries` fail on HEAD's code (16,000,000 > 240,000; 3,999,421 shifted). The reviewer confirmed this.
- Full `cargo test -p purrassist-core` has 0 failures. `make corpus` exit 0 (implementer), `make hygiene` exit 0, check-repository, clippy and fmt pass.
- Review `tasks/R15-review.md`: BLOCKED for lack of a revert-detecting test, fixed; recheck PASS.

### Published-transcript refresh (c6ce650): same process gap
- `test-verify-published.py` failed at c2f1916: the replayed `recover_tool_calls` response's usage had moved with the charging changes. It was not in the R13-R15 checks.
- Recaptured from a clean worktree at c2f1916. The diff is identical apart from `usage`, including the embedded JSON text, and `test-verify-published.py` passes there.
- **Process change:** every later fix also runs `test-verify-published.py`.

### R11 / G19 (user decision): root `$ref` declarations project their parameters
- Projection covers a root `$ref` (Draft-07 `definitions` or 2020-12 `$defs`, followed transitively and capped at 64 hops with a cycle check), a root `allOf` of one `$ref`, and a root `anyOf`/`oneOf` of objects (union of names). Draft-07 ignores the siblings of `$ref`; 2020-12 applies them. Validation still uses the exact schema.
- Render-back is byte-exact in all three wire forms for every new declaration. Prompt forms print the exact schema, so they are unchanged.
- Built CLI with a FastMCP Draft-07 root-`$ref` declaration: qwen and hermes calls both Resolved with exact typed arguments, exit 0.
- Ten new MCP goldens and eight adapter tests. `assert_draft07_root_rules` is updated, with old-versus-new recorded in `tasks/R11-notes.md`.
- Found and fixed during review:
  - uncharged O(n²) in R11's own projection code (per-name branch scans and `relative()`);
  - a pre-existing O(n²) charged scan in the receipt validation log (`Log::add`), which wide unions exposed. It is now indexed by (span, constraint). A call over a 500-branch union drops from 537,298 to 34,795 work.
- `root_union_projection_cost_is_linear` and `root_union_call_recovery_cost_is_linear`.
- Differential over 491 goldens, HEAD against tree: identical apart from usage.
- The default-budget ceiling for one-property root unions moves from about 670 to about 610 branches, a 10% constant overhead; exhaustion stays explicit.
- Full core, host and CLI tests pass, as do `make corpus` (implementer), `make hygiene` (orchestrator), `test-verify-published`, clippy and fmt.
- Review `tasks/R11-review.md`: PASS, with recheck and recheck 2 PASS.

### R7 / G4 (HIGH), G9: fuzz reaches the new paths
- New `tool-call-ref` target (`fuzz-tool-call --tools ref`). Its tool set holds `$ref`/`anyOf`/inline array-of-object items, colliding names, a recursive tree, free-form items and a root `$ref`. It has 101 seeds. The `tool-call` target and its 408 seeds are unchanged.
- `--continue-on-known` matches only the two #6 panic signatures, anchored to the panic line, and retains each input; `make fuzz` never uses it. The G9 trailer test is in place.
- Bounded campaigns with `--continue-on-known`:
  - `tool-call`: 1408 executions, 4 #6 inputs, 0 branch-only.
  - `tool-call-ref`: 1101 executions, 2 #6 inputs, 0 branch-only.
  - Attribution: all 6 #6 inputs reproduce on a base 943fa04 probe, and both retained corpora replay `ok` on base.
- Coverage (llvm-cov over the rebuilt stream): `tool-call-ref` mutated executions reach `pop`'s item branch (214), `nesting`'s item arms (216 / 79 / 136 / 52) and the scope-graph runtime (121-136). `tool-call` reaches none of them.
- Slow input at `tool-call-ref` execution 682 (pre-existing, also on `main`): issue #11, accepted by user decision. `make fuzz` at the default timeout fails there.
- `test-fuzz.py` (78), `make hygiene`, `test-verify-published`, full core tests and clippy pass. Review `tasks/R7-review.md`: PASS.

### R8 / G8: reading identity distinguishes reasoning block splits
- `calls_key_distinguishes_reasoning_block_splits` uses real readings from `Recovery::recognized`. It shows equal keys for a control, and different keys for a different block split or a text-only change.
- Mutation: hashing the merged `value.reasoning` fails the test at recover.rs:5735, with equal digests. The implementer and the reviewer each demonstrated this and restored the code.
- No recognizer input produces such a pair today: one shared marker matcher, and `equivalent_options` compares whole segments. A search over about 598k outputs found none. The doc comment marks the key as defensive.
- Full core tests (1219), `test-verify-published`, `make hygiene` and clippy pass. Review `tasks/R8-review.md`: PASS.

### R9 / G10, G13: compatibility statement and published replay
- `docs/tool-calls/INTEGRATION.md` `## Compatibility` covers wire and MCP, CLI, behaviour and budgets, plus a `### Rust API` subsection (12 breaking items and 6 additions). The full list is in `tasks/R9-compat.md`; the reviewer independently derived it from `git diff 943fa04` and found no omissions.
- Published replay: two new `recover_tool_calls` requests (ids 20 and 21) with `<think>`, a hermes call and `tool_choice_strict`. Id 20 passes (`forced[exactly-one,no-content]`, non-empty `reasoning_blocks`, `run`). Id 21 fails on reasoning (`nothing_usable`, usable 0). The diff adds exactly 2 lines per file.
- `test-verify-published` (10), `make hygiene`, `check-repository`, full core, host and CLI tests, clippy and fmt pass.
- Review `tasks/R9-review.md`: BLOCKED on missing Rust API breaks, fixed; recheck PASS.

## Settle phase on the final head 42899ab
- First `make check` (on 2484700): FAILED at `helper-check`. R2's `ScopeNode::member` duplicated the binary-search body of `ToolSet::tool`. Fixed by the shared `tool_call::by_name` helper (42899ab, behaviour unchanged).
- **`make check` on 42899ab: exit 0.**
- **`make wasm-test` on 42899ab: exit 0.** Native, WASI (core/syntax/host) and browser-WASM transcripts all match.
- **Bounded fuzz on 42899ab, with `--continue-on-known`:**
  - `tool-call`: passed, 1,408 executions, 0 crashes, 4 known #6 inputs.
  - `tool-call-ref` (`--timeout 120`, #11): passed, 1,101 executions, 0 crashes, 2 known #6 inputs.
  - Attribution: all 6 known inputs replayed on a base 943fa04 release probe exit 101 with the identical panic and spans, so they are #6's. Branch-only failures: 0.
  - Not run: `make fuzz` in its gate form (no `--continue-on-known`, default 10 s timeout). It would stop on #6 for `tool-call` and on #11 for `tool-call-ref`; both are known and tracked.
- Evidence inventory: all gates OK; 53 stage files, no symlinks; credential scan clean (the "token" hits are identifiers). The attribution guarantee rests on the repository's commit hooks, which ran on every commit.
- Hosted CI: not run.

## Stage 2 exit (completion gate)
- **Exit completion audit: `completion-audit-exit.md`, VERDICT: PASS** (fresh opus/xhigh auditor, HEAD 42899ab).
  - Every in-scope issue #8 criterion and every Stage 2 gap input was re-executed on the shipped CLI and the stdio MCP.
  - The first audit's F1, F2 and F3 are closed with evidence.
  - Already-satisfied behaviours are identical to base 943fa04.
  - No failure shapes are open.
  - The auditor reused this file's `make check` and `make wasm-test` results for the same head, and replayed all 727 retained fuzz inputs on the head.
- What Stage 3 can reuse: everything above is on 42899ab. Any new commit invalidates `make check`, `make wasm-test` and the fuzz results.
- Still not run or blocked:
  - hosted CI;
  - `make fuzz` in its gate form (stops on #6 and #11, both pre-existing and tracked).
- Conditional specialists: security and performance were used. Not used: quality-engineer (the gap analyzer plus goldens and corpus covered edge cases) and system-architect (no new subsystem beyond the scope graph, which the plan review and the R2 review assessed).
- PR body updated to the current state (Rust API list, scope decisions, residual risks).

# Stage 3

## Integration assessment
- PR base `main` = origin/main = 943fa04 = merge base: no base drift.
- `git merge-tree --write-tree --no-messages 943fa04 42899ab`: exit 0, tree e77f41d. That is identical to HEAD's tree (`42899ab^{tree}` = e77f41d).
- The candidate is therefore byte-identical to the tested head tree, and the base side contributes no changes, so no interaction with base changes is possible. A semantic assessment of an empty base delta has nothing to compare, so none was dispatched. All evidence bound to 42899ab applies to the integration result. No synchronization was needed.
- Reused evidence (same tree): `make check` exit 0; `make wasm-test` exit 0; correction corpus 0 incorrect; bounded fuzz on both targets with #6 attribution; the Stage 2 exit completion audit.

## Hosted CI
- All 8 Verify jobs show `fail`; they did not run. There is no branch protection and no rulesets, so no required checks are configured.
- **User decision: merge on local evidence.**

## Completion audit reuse
- The Stage 2 exit audit (`completion-audit-exit.md`, PASS) assessed HEAD 42899ab. HEAD is unchanged since, the integration tree equals the head tree, and there is no new feedback (0 reviews, 0 inline comments, 0 threads). The only changes since are stage-evidence wording and the PR body text, neither of which affects code or criteria.
- The audit therefore still applies to the actual candidate. No fresh auditor is required.
