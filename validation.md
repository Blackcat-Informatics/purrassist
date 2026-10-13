# Validation index — issue #26

## Task 1 (`2212844`)

Local:
- The 1.98.0 `make release-gate` exits 0 (652 s; six archives verified). Run through `rust-validate` with rustup proxies on PATH; see the T1 report.
- Plain `make check` (nightly) exits 0 (348 s). `make hygiene` exits 0.
- `test-check-release.py`: 54 tests OK, with failing fixtures for each P1/P2 rule. `test-check-docs.py`: 252 OK.
- Independent review (tasks/T1-review.md): PASS, with six mutation probes, each caught.

Hosted (R1):
- Verify run 38376755652 on draft PR #28 head `22128449080bb2193d31037cd6657640101e9b91`: all eight jobs passed.
- `release-gate` job 115226959552 (2026-10-13T09:30:11Z to 09:40:45Z) ran with `RUSTUP_TOOLCHAIN: 1.98.0`. Its log shows, in order: `make check` → `cargo clippy --workspace --all-targets --locked -- -D warnings` → `make corpus` → `python3 scripts/test-fuzz-feedback.py` → `python3 scripts/test-verify-published.py` → `make package`. Conclusion: success.

## Task 2: hosted acceptance probe (R3, R4)

- Probe branch `probe/26-float-cmp`, commit `f06cde1` (on top of `2212844`), draft PR #29. It restores the pre-#24 exact `assert_eq!(above_midpoint.parse::<f64>().unwrap() as f32, 1.0, …)` in `crates/core/src/tool_call/from_node.rs`.
- Local: 1.98.0 Clippy exits 101 (`strict comparison of f32 or f64` at from_node.rs:336:9); nightly Clippy exits 0; the test passes.
- Hosted Verify run 38377011106: the `release-gate` job (115227814445) **failed** with

  ```
  error: strict comparison of `f32` or `f64`
     --> crates/core/src/tool_call/from_node.rs:336:9
      = note: `-D clippy::float-cmp` implied by `-D warnings`
  error: could not compile `purrassist-core` (lib test) due to 1 previous error
  make: *** [Makefile:56: release-gate] Error 2
  ```

  All seven other jobs passed (native ubuntu/macos on nightly, windows-engines, wasm, corpus, fuzz, qualification). The release-toolchain gate alone caught the stable-only lint.
- The probe PR was closed unmerged, and only its branch and worktree were removed.

Run URLs:
- R1: https://github.com/Blackcat-Informatics/purrassist/actions/runs/38376755652
- R3/R4: https://github.com/Blackcat-Informatics/purrassist/actions/runs/38377011106

PR #28 head is still `2212844`, the probe's parent; `verify.yml` and `Makefile` are unchanged since, so no probe rerun is needed.

## Task 3: final head `fdd10ff`

- Docs fix (QUALIFICATION.md: `make fuzz-hosted` is not part of `make release-gate`), RELEASE.md required-check note, and two more fixtures. 56 tests OK; check-docs OK; make hygiene exit 0.
- Hosted Verify run 38382758016 (https://github.com/Blackcat-Informatics/purrassist/actions/runs/38382758016) on `fdd10ff`: all eight jobs passed, including `release-gate`.
- No probe rerun was needed: `verify.yml` and `Makefile` are unchanged since `2212844`.
- Completeness review (reviews/implementation-completeness.md): PASS.

---

# Stage 2 (PR #28)

## Review path: full

Reason: the change restructures `release-qualification.yml`, the workflow `publish.yml` gates on. That is a shared release contract, so the lightweight path does not qualify even though the diff is small.

## Inputs

- `issue-analysis.md`: 24 R-rows, 0 quote misses, 3/3 comments covered.
- `historical-fit.md`: fits precedent and doctrine. The old `msrv` job name has no consumers.
- PR feedback (raw/pr-28*.json): 2 owner comments; 0 reviews, inline comments or threads.
- Hosted CI on head `fdd10ff`: all eight jobs passed.

## Conditional specialists

- quality-engineer: not dispatched. The gap analyzer covers checker edge cases, and every new rule has a failing fixture.
- security-engineer: not dispatched. The change adds no untrusted input, secrets, network or `id-token` permission. Workflow permissions are unchanged (`contents: read`), and `publish.yml` is untouched.
- performance-engineer: not dispatched. No runtime code changed; the CI minute cost is assessed in historical-fit.md.
- system-architect: not dispatched. No modules changed in the shipped crates.

## Stage 2 G3

Corpus suites are linted by the second `make lint` line. Checks: `check-release.py` passes; `test-check-release.py` runs 96 tests, all OK, including 12 new corpus-lint fixtures; `make hygiene` and nightly `make lint` exit 0.

### `rust-validate 1.98.0 make lint` on the clean tree: exit 0

```
cargo clippy --workspace --all-targets --locked -- -D warnings
    Finished `dev` profile [optimized + debuginfo] target(s) in 0.23s
cargo clippy -p purrassist-core --locked --test correction_evaluation --test corruption_oracle --test markdown_corpus --test matching_oracle --test pattern_corpus --test schema_conformance --test tool_call_scale --test unicode_conformance -- -D warnings
    Checking purrassist-core v0.1.0 (.../crates/core)
    Finished `dev` profile [optimized + debuginfo] target(s) in 0.83s
```

### Temporary `assert_eq!("0.5".parse::<f64>().unwrap(), 0.5);` in crates/core/tests/correction_evaluation.rs: exit 2, from the SECOND line

The first (workspace) line passed; the second reported:

```
cargo clippy -p purrassist-core --locked --test correction_evaluation ... -- -D warnings
error: strict comparison of `f32` or `f64`
   --> crates/core/tests/correction_evaluation.rs:673:5
673 |     assert_eq!("0.5".parse::<f64>().unwrap(), 0.5);
    = note: `-D clippy::float-cmp` implied by `-D warnings`
error: could not compile `purrassist-core` (test "correction_evaluation") due to 1 previous error
make: *** [Makefile:24: lint] Error 101
```

The edit was reverted; `git diff --stat crates/` is empty.

## Stage 2 final evidence (head `fd898ed`)

Stage 2 fix commits: `5e5159e` (G1+G2), `0c4c25e` (G3), `fd898ed` (G5+G6+G8). Each passed independent review (tasks/G1-review.md, G3-review.md, G5-review.md).

Local, single full run:
- Nightly `make check`: exit 0, 178 s, 110 test groups. Before figure: 348 s in Task 1. The difference reflects machine load; the corpus lint line adds about 0.2 s.
- Rust 1.98.0 `make release-gate` (rust-validate, rustup proxies on PATH): exit 0, 403 s, "Verified 6 release archives" for `fd898ed`.
- `test-check-release.py`: 110 tests OK. `test-check-docs.py`: 260 OK. `make hygiene` passes.

Hosted:
- Final head, Verify run 38430626503 (https://github.com/Blackcat-Informatics/purrassist/actions/runs/38430626503): all eight jobs passed, including `release-gate`.
- Probe rerun (G7, required after `verify.yml`/`Makefile` changed): draft PR #30, `probe/26-float-cmp-2`, commit `81aa453` on `fd898ed`, restoring the pre-#24 exact assertion. Verify run 38430682936 (https://github.com/Blackcat-Informatics/purrassist/actions/runs/38430682936): **only `release-gate` failed**, with `error: strict comparison of f32 or f64 --> crates/core/src/tool_call/from_node.rs:336:9`, `-D clippy::float-cmp`, `make: *** [Makefile:59: release-gate] Error 2`. All seven other jobs passed. PR #30 was closed unmerged; its branch and worktree were removed.

## Stage 2 exit (final head `8c7ab3b`)

Later commits, after the exit audit's advisories:
- `a3bbed0`: the Makefile may not set, export or unexport `RUSTUP_TOOLCHAIN` or declare `.IGNORE`.
- `8c7ab3b`: only the existing `MAKEFLAGS += --no-print-directory` line is allowed. Docs and docstrings match.
- Both touch only `scripts/check-release.py`, `scripts/test-check-release.py` and `docs/RELEASE.md`. The workflows, Makefile and crates are byte-identical to `fd898ed`, so the hosted gate and probe evidence from `fd898ed` (runs 38430626503 and 38430682936) still applies.
- `test-check-release.py`: 115 tests OK. check-docs and make hygiene pass.

Hosted, final head: Verify run 38438995951 (https://github.com/Blackcat-Informatics/purrassist/actions/runs/38438995951) on `8c7ab3b`, all eight jobs passed. The superseded run 38438344049 on `a3bbed0` was also all green.

Exit completion audit: completion-audit-exit.md, VERDICT PASS for `8c7ab3b`. Its conditions are now satisfied: the hosted run above is green, and the issue summary naming PR #30 and run 38430682936 is posted with this exit.
- Covered: issue R1–R4, demonstrated on hosted runs and probes, plus remediation H1, M1–M3, A1 and L1–L6, each probed adversarially on scratch copies.
- Recorded limit, not a finding: deliberately evasive Makefile spellings such as `MAKE := make -i` or `export GNUMAKEFLAGS := -i` can still defeat a text-only check. All plausible direct spellings are rejected.
- L7: the module loader is left duplicated, with the reason (circular import) recorded in remediation-plan.md.

## Stage 3 integration assessment

- PR base: `origin/main` at `8f1f42d`, fetched at Stage 3 start. Merge base of the head (`8c7ab3b`) and the base is `8f1f42d`, so the base has no commits the branch lacks.
- `git merge-tree --write-tree --no-messages origin/main 8c7ab3b`: exit 0, tree `980f4bc5`, identical to `8c7ab3b^{tree}`. The squash candidate is byte-identical to the tested head.
- Semantic assessment: the base-side change set is empty, so there are no shared consumers or build configuration changes to reconcile. No separate semantic agent was dispatched, because there is no second change set to assess.
- Evidence reuse:
  - Hosted Verify run 38438995951 (all eight jobs passed) and the script tests (115 and 260) ran on `8c7ab3b` and cover the candidate directly.
  - The local single full run (Rust 1.98.0 `make release-gate`, nightly `make check`) and probe PR #30 (run 38430682936) ran on `fd898ed`. Its workflows, Makefile and crates are byte-identical to `8c7ab3b`.
  - Probe PR #29 (run 38377011106) ran on the earlier Task 1 gate and is historical.
  - No check was invalidated; nothing was rerun.
- Branch protection: `main` has no classic protection (API 404). Its repository ruleset (squash-only, signed commits, linear history, no deletion or force push) has no required status checks, so no check is formally required. All eight PR checks pass (pr-checks.txt, `state: pass`).
- Completion audit reused: completion-audit-exit.md (PASS, `8c7ab3b`). It still applies because the head, base and candidate tree are unchanged since that audit and no new feedback arrived.

- Merge-plan review (reviews/merge-plan-review.md) was BLOCKED on B1 (requirement labels in the squash notes) and B2 (evidence coverage overclaimed). Both are corrected in the notes and above, and the ruleset advisory is applied.
