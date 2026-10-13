# Completion audit, Stage 2: issue #26 / PR #28

An independent auditor, not the implementer, wrote this report. Its rubric is `~/.claude/commands/includes/_completion-adversary.md`.

## Scope and authority

- **Repository:** Blackcat-Informatics/purrassist.
- **Worktree:** `.worktrees/26-lint-the-whole-workspace-with-the`.
- **Branch:** `paudley/26-lint-the-whole-workspace-with-the`.
- **Base:** `origin/main` = `8f1f42d`. The remote `main` is still `8f1f42d` (checked with `gh api .../branches/main`).
- **Head:** `fdd10ff`. PR #28 is OPEN and ready (not a draft), says `Closes #26`, and is MERGEABLE.
- **Sources read:**
  - `issue.md`: the body and all three comments.
  - `plan.md`: revision 3, the authoritative plan.
  - `issue-analysis.md`: the ledger and its five NEEDS-JUDGMENT items.
  - `validation.md`, `tasks/T1-review.md`, `tasks/T2-review.md` and `reviews/implementation-completeness.md`.
  - The full diff `origin/main..fdd10ff`: Makefile, the two workflows, `check-release.py`, `test-check-release.py`, `test-check-docs.py`, QUALIFICATION.md and RELEASE.md.
- **Context layers:** `.goals` and `AGENTS.md`. There is no `.baseline`, `CONSTITUTION.md` or ADR.
- **`.deficiencies`:** the root file has no entry for #26, and the worktree has no such file.
- **User scope decisions:** none beyond the issue. The broadened scope (the whole release gate on PRs) is the plan's own adoption. It is disclosed in the PR body and the issue for the owner to object to. I judged it as a superset of the original ask.
- **What I ran or queried:**
  - read-only `gh` queries of runs 38376755652, 38377011106 and 38382758016, their `release-gate` job logs, and the probe run's `native (ubuntu-24.04)` log;
  - the probe commit (`gh api`), PR #28/#29 state and `main`'s rules;
  - the two `check-release` scripts, `make hygiene`, and a local 1.98.0 Clippy.
- **What I did not run:**
  - a local `RUSTUP_TOOLCHAIN=1.98.0 make release-gate`. I reused the hosted run instead; see R1.
  - no PR, push or repository change. `git status` shows only `.stage/` untracked, before and after.

## Why the hosted evidence applies to head `fdd10ff`

`git diff 2212844 fdd10ff --stat` shows that only three files changed:

- `docs/QUALIFICATION.md` (+5/-4);
- `docs/RELEASE.md` (+3/-1);
- `scripts/test-check-release.py` (+9, two new fixtures).

`verify.yml`, `release-qualification.yml`, `Makefile`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` and `crates/**` are byte-identical between the probe's parent (`2212844`) and `fdd10ff`. Both hosted PR runs checked out a merge with the same base, `8f1f42d`:

- `4317140` = `2212844` + `8f1f42d`;
- `59bf4d2` = probe `f06cde1` + `8f1f42d`;
- `9944877` = `fdd10ff` + `8f1f42d`.

`make check` runs `fmt`, then `hygiene`, then `helper-test`, then `helper-check`, before `lint`.

- **On the probe:** these stages passed and the job reached `lint`. The probe log shows `cargo clippy` starting at 09:36:15.
- **On `fdd10ff`:** the same stages pass on the hosted run. Its `hygiene` includes the changed `test-check-release.py`, 56 tests OK.
- **What the new fixtures read:** only scratch copies of the workflows, Makefile and toml files, never `crates/core`.

So the probe applied to `fdd10ff` would reach the identical `lint` recipe on the identical toolchain and fail the same way. The probe rerun condition in plan Task 3 ("rerun if `verify.yml` or `Makefile` change") was not triggered.

## Criteria

| Id | Requirement (verbatim source) | Demonstration | Observation | Status |
|---|---|---|---|---|
| R1 | "Run `cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.98.0 in a pull-request job" (body L19) | Hosted run 38382758016 (`pull_request`, headSha `fdd10ffcf403…`), job `release-gate` 115246980855. Also 38376755652 on `2212844`, job 115226959552. | See the R1 notes below. Expected: the exact command runs on 1.98.0 in a PR-triggered job. | **demonstrated** |
| R2 (caller) / ledger R1 tail | "for example the existing `msrv` job" | `git diff origin/main fdd10ff -- .github/workflows/verify.yml` and the hosted job name | See the R2 notes below. | **demonstrated** |
| Ledger R2 | "A stable-only lint then fails on the PR that introduces it, not on the release tag." | The P1 gate-identity check, run hosted and locally, plus the probe run | See the ledger R2 notes below. | **demonstrated** |
| R3/R4 (caller) / ledger R3 | "A PR that adds an exact float `assert_eq!` in a `purrassist-core` test fails a hosted pull-request job with `clippy::float_cmp`." (Acceptance L23) | Probe PR #29, commit `f06cde1`, hosted run 38377011106, job 115227814445. Compared with nightly `native (ubuntu-24.04)` job 115227814496 on the same merge commit. | See the R3/R4 notes below. | **demonstrated** |
| P1 (plan) | Gate identity, guarded structurally | `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | exit 0. "Release gate identity verified: release qualification and the Verify release-gate job both run `make release-gate` on the same toolchains and environment." | **demonstrated** |
| P1/P2 fixtures | A failing fixture per rule | `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py` | exit 0, `Ran 56 tests … OK`. See the fixtures note below. | **demonstrated** |
| P2 (plan) | Toolchain pins single-sourced | `check-release.py`, as above | exit 0. "Toolchain pins verified: 8 stable occurrences match rust-version 1.98, 12 nightly occurrences match nightly-2026-09-14." The same line appears in the hosted logs. | **demonstrated** |
| P3 (plan) | Docs: the MSRV-evidence sentence, the command list, the RELEASE.md steps and paragraph | Diff read, plus `git grep` | See the P3 notes below. | **demonstrated** |
| Gate | `make hygiene` | `make hygiene` at `fdd10ff` | exit 0. Suites: 52, 56, 252, 81 and 14 tests OK. `git status` is unchanged (only `.stage/` untracked). | **demonstrated** |

**R1 notes.**

- **Run 38382758016:** the run concluded `success`, and all 8 jobs succeeded. The job env is `RUSTUP_TOOLCHAIN: 1.98.0`, and the log shows `installed - rustc 1.98.0 (88d9e12ae 2026-08-18)`.
- **Command:** under `Run make release-gate`, the log shows `cargo clippy --workspace --all-targets --locked -- -D warnings` at 10:24:58. It checks every workspace member: core, derive, wasm-harness, syntax, purrassist, host, cli and helper-census. It reaches `Finished` with no warnings.
- **Rest of the gate:** the log then shows `cargo test --workspace --locked` (`-D warnings`), `make corpus`, `test-fuzz-feedback.py`, `test-verify-published.py` and `make package`.
- **Run 38376755652:** the same sequence, with conclusion `success`.
- **Local supporting run** (independent of the cache): `CARGO_BUILD_BUILD_DIR=/tmp/audit26-build CARGO_TARGET_DIR=/tmp/audit26-target rust-validate 1.98.0 env RUSTC_WRAPPER= cargo clippy --workspace --all-targets --locked -- -D warnings`. It reported `rustc 1.98.0`, `clippy 0.1.98`, checked every member, and exited 0 (user time 1m27s).
- **What counts as proof:** the hosted runs. The local run only supports them.

**R2 notes.**

- **The rename:** in `verify.yml`, `msrv:` becomes `release-gate:`, keeping `runs-on: ubuntu-24.04` and `RUSTUP_TOOLCHAIN: 1.98.0`. The old `cargo test --workspace --locked` is replaced by `make release-gate`, which runs the same tests with `-D warnings` through `make test`; the hosted log confirms this.
- **No duplicate:** no `msrv` job remains, and `git grep -i msrv` finds only prose and one test fixture.
- **PR-triggered:** `on:` includes `pull_request:` with no paths or branches filter, and the job has no `if:` condition.

**Ledger R2 notes.** Release qualification's `package` job now has the checkout plus exactly four named steps. Its only `make` invocation is `make release-gate`. That is the same target, toolchains and env as the PR job, and the hosted logs show it. Any lint that release qualification's `make lint` reports is therefore reported first by the PR job, which has the same recipe and toolchain. The probe shows this is specific to the release toolchain:

- the same merge commit `59bf4d2` passed nightly Clippy in `native` (`cargo clippy …` reached `Finished`);
- it failed only in `release-gate`.

**R3/R4 notes.**

- **The probe commit:** `f06cde1` (parent `2212844`) changes only `crates/core/src/tool_call/from_node.rs`, +2/-2, inside `mod tests`. It turns the `to_bits()` comparison into `assert_eq!(above_midpoint.parse::<f64>().unwrap() as f32, 1.0, …)`.
- **The run:** run conclusion `failure`. `release-gate` was the only failing job, and the other 7 succeeded, including both nightly `native` jobs.
- **The log excerpt:**
  - `error: strict comparison of \`f32\` or \`f64\` --> crates/core/src/tool_call/from_node.rs:336:9`;
  - `…/rust-clippy/rust-1.98.0/index.html#float_cmp`;
  - `` = note: `-D clippy::float-cmp` implied by `-D warnings` ``;
  - `` = help: … add `#[allow(clippy::float_cmp)]` ``;
  - `make[1]: *** [Makefile:23: lint] Error 101`;
  - `make: *** [Makefile:56: release-gate] Error 2`;
  - `##[error]Process completed with exit code 2`.
- **The literal lint name:** `clippy::float_cmp` appears verbatim in the log. That settles ledger NEEDS-JUDGMENT 3, the hyphen spelling.
- **PR #29 now:** CLOSED, `mergedAt` null, draft, no closing references.

**Fixtures note.** Each new fixture edits a scratch copy and asserts a specific error string. The one "real files pass" case per class is a positive control. No assertion is tautological.

**P3 notes.**

- **QUALIFICATION.md:**
  - The command list has `RUSTUP_TOOLCHAIN=1.98.0 make release-gate`.
  - The `release-gate` sentence says it "is also the MSRV 1.98 build-and-test evidence".
  - "stable MSRV workspace tests" is gone. `git grep` finds it nowhere.
- **RELEASE.md:**
  - Step 4 is `Verify the release gate`.
  - The `check-release.py` paragraph describes gate identity and the pins.
  - The page states that the job is not a required check.
- **Old step names:** none of `Verify workspace`, `Verify corpus suites` or `Verify crate archives` remains, except as one negative fixture.
- **`check-docs.py`:** passes inside `make hygiene`.

## Failure-shape check

- **DARK:** no.
  - `validate_release_gate` and `validate_toolchain_pins` are called from `check-release.py` `main()`.
  - That runs in `make hygiene`, `make package` and the release version step, and the hosted logs print their messages.
  - The `release-gate` job runs on every `pull_request` and on pushes to `main`.
- **REFUSED, PRODUCER-WITHOUT-CONSUMER, VACUOUS OUTPUT:** not applicable.
- **DUPLICATE:** no. The `msrv` job and the five split release steps are both gone.
- **LAUNDERED-TO-FOLLOWUP:** no. Nothing the issue requires was moved elsewhere.
- **TEST-ONLY:** no. The acceptance ran through the real hosted `pull_request` Verify workflow.
- **WEAKENED TESTS:** no. The one changed assertion in `test-check-docs.py` follows the renamed step.
- **SILENT NARROWING:** no, against the issue's contract; see advisory A1.
- **Superset scope:** the PR job also runs `make corpus`, both scripts and `make package`. It still contains the issue's exact command, unmodified, as `make lint`. The added gates cost about 10–13 min of job time. Overall wall time stays below the existing ~40 min `fuzz` job. The scope is disclosed for the owner to object to.

## Open findings

None blocking.

## Advisories (non-blocking, for the owner)

**A1. Eight `purrassist-core` corpus suites are never Clippy-linted, in either lane.**

- **What I observed:** `crates/core/Cargo.toml` lines 67–105 declare eight suites with `test = false`. Cargo's `--all-targets` (`--tests`) skips `test = false` targets.
  - After my local 1.98 run, the isolated build dir holds `allocation_oracle-*` artifacts, an ordinary integration test.
  - It holds no `correction_evaluation*` artifact.
- **Consequence:** an exact float `assert_eq!` added to, say, `crates/core/tests/correction_evaluation.rs` would fail no hosted PR job with `float_cmp`.
- **Why it doesn't block #26:** the issue's own proposal names `--workspace --all-targets`, and release qualification has the same blind spot. So no release-only divergence arises, and the issue's problem class is closed. I read "a `purrassist-core` test" as a test in the targets the issue's command lints, and that is what the probe exercised.
- **If the owner wants every core test linted:** that is a change to `make lint` in both lanes, such as adding `--test <suite>` for the corpus suites. It is a new decision, not part of this PR.

**A2. PR #28's body is stale on the ready head.**

- It still opens with "**Draft:** the hosted `release-gate` job must pass on this head … before this is ready".
- It reports `test-check-release.py: 54 tests`, but the count is now 56.
- It does not link the hosted runs; the issue's handoff comment does.
- Refresh the body before Stage 3 merges it.

**A3. The structural guard pins the job's steps, env and installs, but not its trigger or conditions.**

`validate_release_gate` does not assert that:

- `verify.yml` keeps `pull_request:` in its triggers;
- the `release-gate` job has no `if:` or `continue-on-error:`.

The plan's P1 never required this, and today's state is correct. Future edits that drop PR coverage would pass `check-release.py`.

**A4. Not a required check (disclosed).**

- `main`'s ruleset types are `deletion, non_fast_forward, required_linear_history, required_signatures, pull_request`, with no required status checks.
- So a red `release-gate` job does not block a merge.
- The acceptance only requires that the PR job *fails*, so this does not block the issue.
- RELEASE.md, the PR body and the issue all state that making it required is the owner's setting.

## NEEDS-JUDGMENT dispositions (from `issue-analysis.md`)

1. **Same job or a new one:** it is the existing `msrv` job, renamed `release-gate`. That is what the issue suggested.
2. **Whole gate or Clippy only:** the PR job runs the whole gate, a superset that contains the exact command. It is disclosed, with no owner objection on record.
3. **The hyphen in `-D clippy::float-cmp`:** the literal `clippy::float_cmp` appears in the hosted log.
4. **One probe for a standing acceptance:** it is sufficient. The gate files and code are identical from the probe's parent to the head, and P1 freezes the lint recipe and the PR job's gate step.
5. **Required check:** outside the issue's contract; see A4.

VERDICT: PASS
