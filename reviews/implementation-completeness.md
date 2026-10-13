# Implementation completeness review: issue #26 (plan revision 3)

Branch `paudley/26-lint-the-whole-workspace-with-the`, head `2212844` on base `8f1f42d`, draft PR #28.
Read-only review. The only file written is this one.

## Checks run by this review

| Command | Result |
|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | rc 0. "Release gate identity verified", "8 stable occurrences match rust-version 1.98, 12 nightly occurrences match nightly-2026-09-14" |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py` | rc 0, 54 tests OK |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py` | rc 0 |
| `make hygiene` | rc 0 (52, 54, 252, 81 and 14 tests OK) |
| Scratch-copy mutation probes on `validate_release_gate` (repo untouched) | Swapped install order: caught ("must install the same toolchains"). Renamed `package` job: caught. Duplicate `release-gate` target: caught. |
| `gh run view` 38376755652 / 38377011106, job logs 115226959552 / 115227814445 | See R1, R3 and R4 below |

Not run, as instructed: `make check` and `make release-gate`. The local 1.98.0 `make release-gate` pass (652 s) and the nightly
`make check` pass are recorded in `validation.md` only.

## Contract rows

| Id | Status | Evidence |
|---|---|---|
| R1 | DELIVERED | Run 38376755652: `event` is `pull_request`, `headSha` is `22128449080b…` (the PR #28 head), all 8 jobs succeeded. The `release-gate` job 115226959552 log shows `RUSTUP_TOOLCHAIN: 1.98.0`, `rustc 1.98.0 (88d9e12ae 2026-08-18)` installed, then `make check` → `cargo clippy --workspace --all-targets --locked -- -D warnings` (09:33:06, Finished) → `make corpus` → `test-fuzz-feedback.py` → `test-verify-published.py` → `make package` → "Verified 6 release archives". Local 1.98.0 `make release-gate` exit 0 is recorded (validation.md:6). |
| R2 | DELIVERED | `.github/workflows/verify.yml:35-49`: `msrv` is now `release-gate`, with `RUSTUP_TOOLCHAIN: 1.98.0`, running `make release-gate`. `git grep msrv` finds only a negative fixture (`scripts/test-check-release.py:315`) and generic MSRV prose. Guarded by `scripts/check-release.py` (`verify.yml must define a release-gate job`, `…must run make release-gate as its only make command`) with fixtures `test_a_missing_verify_release_gate_job_fails` and `test_a_verify_job_without_the_gate_fails`. |
| R3 | DELIVERED | Run 38377011106 (`pull_request`, head `f06cde1`, whose parent is `2212844`): only `release-gate` (115227814445) failed. The two nightly `native` jobs and the five other jobs succeeded. The log shows `cargo clippy --workspace --all-targets --locked -- -D warnings` at line 363, `error: strict comparison of f32 or f64` at `crates/core/src/tool_call/from_node.rs:336:9`, `-D clippy::float-cmp implied by -D warnings`, the help URL under `rust-1.98.0`, then `make[1]: *** [Makefile:23: lint] Error 101` and `make: *** [Makefile:56: release-gate] Error 2`. |
| R4 | DELIVERED (recording sub-item minor gap) | The hosted demonstration is the same as R3. Probe PR #29 is CLOSED with `mergedAt` null, is a draft, has the titled name and no closing references. `validation.md:19-34` has the run URL and a verbatim log excerpt. The issue #26 comment issuecomment-6143289107 has the run id, the error and location, and `-D clippy::float-cmp`, but not the run URL the row asks for, and the excerpt is paraphrased rather than verbatim. Advisory: put both run URLs into the Task 3 handoff comment. |
| P1 | DELIVERED | `scripts/check-release.py` `validate_release_gate` covers the exact four named steps plus one unnamed checkout first; `make` exactly once, as `make release-gate`, in `Verify the release gate`; the verify job running only `make release-gate`; env equality; one stable install with `clippy,rustfmt` and one nightly with `llvm-tools` per job; install equality; the Makefile `release-gate` target with no prerequisites and the exact five recipe lines in order; and the exact `--locked` lint recipe. `workflow_steps` is imported from `check-docs.py` through importlib, not duplicated. It is called from `main()` (`check-release.py:507`) and so runs in `make hygiene` and `make package`. Fixtures exist in `scripts/test-check-release.py` (`ReleaseGateIdentity`). Minor: two error messages have no fixture of their own, `the release-gate job must install the same toolchains as release qualification` and `release-qualification.yml must define a package job`. This review's scratch probes show both fire. Both rules overlap with fixtured rules (P2's pin drift and the component fixtures), so this is advisory. |
| P2 | DELIVERED | `toolchain_pins` / `validate_toolchain_pins` read only the name (ENV_PIN, INSTALL_PIN) across all three workflows and classify a name with a `nightly` prefix as nightly. They check stable identity, stable major.minor against `rust-version` (`Cargo.toml:10`), and nightly against `rust-toolchain.toml` `channel`. Fixtures: install line bumped alone, env bumped alone, publish.yml-only bump, rust-version mismatch, nightly drift in one verify job, channel change, plus publish.yml's component-less install still matching. |
| P3 | DELIVERED | `docs/QUALIFICATION.md:250` adds the `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` command. `:257-259` names the `release-gate` job and `make release-gate` on Rust 1.98.0 as "also the MSRV 1.98 build-and-test evidence". "stable MSRV workspace tests" is gone from the tree. `docs/RELEASE.md:23-31` (check-release paragraph) and `:138-148` (step 4 and the PR-job paragraph) are updated. The `check-docs.py` `release-qualification-steps` rule passes on `Verify the release gate`. |

## Tasks

| Task | Status | Evidence |
|---|---|---|
| T1 files | DELIVERED | Makefile (`.PHONY`, `lint --locked`, `release-gate` at Makefile:51-61), release-qualification.yml:42-43, verify.yml:35-49 (`timeout-minutes: 90`, `RUSTFLAGS` dropped, both installs), check-release.py, test-check-release.py, test-check-docs.py:1124, both docs. |
| T1 focused checks | DELIVERED | This review re-ran the check-release, test and hygiene checks. The 1.98.0 `release-gate` and nightly `make check` are recorded in validation.md:6-7. |
| T1 then-steps | DELIVERED | `tasks/T1-review.md` PASS. Commit `2212844` was pushed. Draft PR #28 has head `2212844`. The hosted R1 pass is recorded. The issue was updated (issuecomment-6143289107). |
| T2 steps 1-8 | DELIVERED | Probe `f06cde1` changes one assertion, +2/-2, per T2-review. Local 1.98.0 Clippy exit 101 is recorded. Draft PR #29 has no closing keyword and is closed unmerged. The branch is deleted (T2-review). The record is in validation.md and on the issue. `tasks/T2-review.md` PASS. |
| T3 | PENDING (expected) | This review is step 1. PR #28 is still a draft, and steps 2-6 remain. The probe rerun is not needed at `2212844`, but a docs-only follow-up commit (see B1) changes neither `verify.yml` nor `Makefile`. It needs a fresh green hosted `release-gate` on the new head, not a probe rerun. |

## Dispositions

All revision-2 and later dispositions are reflected in the delivered code and docs:

- **Delivered as planned:**
  - ledger ids;
  - exact step pinning;
  - one-`make` rule;
  - five-line recipe, including `package`;
  - lint-recipe assertion;
  - `RUSTFLAGS` dropped, with env equality;
  - pin extractor that ignores components;
  - major.minor comparison;
  - reused `workflow_steps`;
  - `test-check-docs.py` assertion;
  - MSRV-evidence sentence;
  - runtime-true probe;
  - draft PR with recorded SHA.
- **Declined items left alone:**
  - required check, recorded as an owner option in the PR body;
  - permanent failing fixture;
  - `windows-engines` Clippy kept.
- **Scope:** the PR body discloses the scope broadening (the whole release gate on PRs, the plan's TRANSFORMATION) for owner objection.
- **T1 review nit:** the 136-column line is resolved. No added docs line exceeds 81 columns.

## Public claims, stale wording, scope, attribution

- **B1 (blocking, stale wording turned into a false claim).** `docs/QUALIFICATION.md:234-235`: "`make fuzz-hosted`, which the CI `fuzz` job and the release gate run, completes both campaigns instead". This branch makes "the release gate" a defined artifact, `make release-gate`, described 20 lines later in the same file. That target does not run `make fuzz-hosted`: its recipe is pinned by `check-release.py` `GATE_RECIPE` to check, corpus, `test-fuzz-feedback.py`, `test-verify-published.py` and package. The hosted log of job 115226959552 shows no `fuzz-hosted`, and `make fuzz-hosted` runs only in `verify.yml:111` (`fuzz` job). The sentence predates the branch (added in `7e30c67`), where "release gate" most plausibly meant RELEASE.md's manual pre-tag step (`docs/RELEASE.md:274-275`). Now it reads as a capability claim about the new gate with negative executable evidence. Fix: name what actually runs it, for example "which the CI `fuzz` job runs and the pre-tag release checklist (RELEASE.md) requires locally". Docs-only change, so no probe rerun is needed.
- **RELEASE.md:145-147** ("a lint or test that fails only on the release toolchain fails before the tag, not at it") has executable evidence for lint (R3). Tests on 1.98.0 run inside `make check` in the same job. It does not claim merge blocking, which is consistent with the declined required check. Advisory: one clause noting the job is not yet a required check would make the residual visible to readers.
- **No other stale wording:**
  - no `msrv` job;
  - no `Verify workspace` outside a negative fixture;
  - old step names gone from docs;
  - `FUZZING.md:1073` ("release qualification runs it too", meaning the feedback test) is still true;
  - the release-qualification.yml header comment is still accurate.
- **Scope beyond plan:** none. The extra rules (unnamed-checkout-first, install equality, duplicate-target detection) are mechanical consequences of P1's stated assertions. No new dependencies (importlib is stdlib) and no new files, so no SPDX or registration changes are needed.
- **Attribution:** none.
  - The commit `2212844` author and committer are the user.
  - A grep of the message for co-authored, generated, claude, anthropic, codex, copilot and agent finds nothing.
  - The same grep over the diff finds nothing.
  - PR #28 body, issue comments and probe commit (per T2-review) are clean.

## Required actions

1. B1: correct `docs/QUALIFICATION.md:234-235` so it no longer says the release gate runs `make fuzz-hosted`. Commit normally and push. Then confirm the hosted `release-gate` job is green on the new head (Task 3 step 3). No probe rerun is needed, because `verify.yml` and `Makefile` are unchanged.

Advisory (non-blocking):
- Put both run URLs (38376755652 and 38377011106) and a verbatim excerpt into the Task 3 handoff comment on #26, which completes R4's on-issue record.
- Add fixtures for the install-equality and missing-`package`-job messages in `scripts/test-check-release.py`.

VERDICT: BLOCKED

## Recheck at `fdd10ff` (supersedes the verdict above)

Commit `fdd10ff` (parent `2212844`) changes only `docs/QUALIFICATION.md`, `docs/RELEASE.md` and `scripts/test-check-release.py`.
`git diff --name-only 2212844 fdd10ff -- .github Makefile` is empty, so the gate files are unchanged and the R3/R4 probe still stands
without a rerun.

- **B1: RESOLVED.** `docs/QUALIFICATION.md:234-236` now says `make fuzz-hosted` is run by the CI `fuzz` job and locally by the
  pre-tag checklist in RELEASE.md, and "is not part of `make release-gate`". Both halves hold: `verify.yml:111` runs it, and
  `docs/RELEASE.md:274-275` says "Before tagging, run `make fuzz-hosted` locally". `git grep "release gate run" docs` finds nothing.
- **Advisory, R4 issue record:** still open. It belongs in the Task 3 handoff comment and is not a completeness row.
- **Advisory, P1 fixtures: ADOPTED.** `test_a_different_stable_install_in_the_pr_job_fails` asserts "the release-gate job must
  install the same toolchains as release", and `test_a_missing_release_package_job_fails` asserts
  "release-qualification.yml must define a `package` job". `test-check-release.py`: 56 tests OK.
- **Advisory, RELEASE.md required-check note: ADOPTED, and the claim is checked.** `docs/RELEASE.md:147-149` says the job is not
  yet a required status check on `main`. `gh api repos/.../rules/branches/main` returns `deletion, non_fast_forward,
  required_linear_history, required_signatures, pull_request`, with no `required_status_checks` rule. Classic branch protection
  returns 404 ("Branch not protected").
- **Checks re-run on `fdd10ff`:**
  - `check-release.py` (encoding flags): rc 0;
  - `test-check-release.py`: 56 OK;
  - `check-docs.py`: rc 0;
  - `make hygiene`: rc 0 (52, 56, 252, 81 and 14 OK).
- **Attribution:** the `fdd10ff` author and committer are the user. The message and the diff contain no attribution terms.
- **Hosted, Task 3 step 3:** Verify run 38382758016 (`pull_request`, head `fdd10ff`, which is PR #28's current head) is
  **in progress**. R1 is evidenced by run 38376755652 on `2212844`, and nothing that run executes as a gate has changed since.
  Before PR #28 is marked ready, Task 3 must confirm that 38382758016's `release-gate` job concludes `success`. This review does
  not claim that result.

Every contract row R1-R4 and P1-P3 is DELIVERED, Tasks 1 and 2 are complete, and no required omission remains.

VERDICT: PASS
