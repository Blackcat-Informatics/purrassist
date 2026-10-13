# Completion audit, Stage 2 exit gate: issue #26 / PR #28

This report was written by an independent auditor, not the implementer. It follows the rubric in `~/.claude/commands/includes/_completion-adversary.md` with STAGE=2.

## Scope, sources and authority

- **Repository and PR:** Blackcat-Informatics/purrassist, PR #28 (OPEN, ready, MERGEABLE, `Closes #26`).
- **Worktree:** `.worktrees/26-lint-the-whole-workspace-with-the`, on branch `paudley/26-lint-the-whole-workspace-with-the`.
- **Head and base:**
  - The first pass audited `fd898ed`.
  - The first recheck audited `a3bbed0`, whose only parent is `fd898ed`.
  - The final recheck audits the current head, `8c7ab3b`. Its only parent is `a3bbed0`, and `git ls-remote` shows the remote branch is also `8c7ab3b`. See the two recheck sections at the end.
  - The base is `origin/main` = `8f1f42d`, which is also the merge-base. The remote `main` is unchanged.
- **Diff under review:** `origin/main...fd898ed`, 11 files, +1270/-37. It covers the two workflows, Makefile, README, `crates/core/Cargo.toml`, QUALIFICATION.md, RELEASE.md, `check-release.py`, `check-docs.py` and both test files.
- **Contracts judged:**
  - the issue body and its acceptance;
  - `plan.md` (Stage 1, revision 3) for R1–R4 and P1–P3;
  - `remediation-plan.md` (Stage 2, revision 3) for G1–G8, covering H1, M1–M3, L1–L7 and A1.
- **Other sources read:** `issue-analysis.md` (24 rows), `gap-analysis.md`, `completion-audit.md`, `validation.md`, `tasks/G1/G3/G5-review.md` and `reviews/remediation-plan-review.md`.
- **Context layers:** `.goals` and `AGENTS.md`. There is no `.baseline`, `CONSTITUTION.md` or ADR.
- **`.deficiencies`:** the root file has no #26 entry, and the worktree has no such file.
- **User scope decisions:** none. The whole release gate running on pull requests is the plan's own disclosed adoption. I judged it as a superset that contains the issue's exact command.
- **What I executed:**
  - the four permitted validators and their tests;
  - `make hygiene`;
  - read-only `gh` queries, plus job logs of runs 38430626503 and 38430682936;
  - 40 probes (39 adversarial plus the real-files baseline) on scratch copies outside the repository, and three toy Makefiles (no cargo) to confirm GNU Make 4.4.1 semantics.
- **Cleanup:** all scratch files and logs were deleted. `git status` shows only `.stage/` untracked, before and after.
- **What I did not execute:**
  - No local cargo or `make release-gate` run. The local 1.98.0 `make release-gate` (403 s) and the corpus-suite `float_cmp` demonstration are reused from `validation.md`.
  - No release-qualification run. It triggers only on a tag or a dispatch, and neither is authorized.
  - No push, PR or repository change.

## Why the hosted evidence applies to `fd898ed`

- **Run 38430626503** (Verify, `pull_request`):
  - Its `headSha` is exactly `fd898ed91d38…`.
  - It checked out `5754c78` = "Merge fd898ed… into 8f1f42d…".
  - `main` is still `8f1f42d`, and the branch has no later commit.
- **Probe run 38430682936** (`pull_request`, PR #30):
  - It ran on `81aa453`, whose only parent is `fd898ed`.
  - `git diff fd898ed 81aa453` touches only `crates/core/src/tool_call/from_node.rs` (+2/-2).
  - It checked out `3c989e7` = "Merge 81aa453… into 8f1f42d…".
  - So every gate file (Makefile, both workflows and the scripts) is byte-identical to the final head's.
- **Earlier runs:** 38376755652, 38377011106 and 38382758016 predate the Stage 2 changes to `verify.yml` and the Makefile. I did not rely on them for the final head.

## Criteria

**Ledger mapping.** Plan R1–R4 correspond to issue ledger R1 (with its "msrv" clause), R2 and R3.

### Plan R1: workspace Clippy on Rust 1.98.0 in a pull-request job

**Requirement:** `cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.98.0 in a pull-request job.

**Evidence:** run 38430626503, job `release-gate` 115411547933, read with `gh run view --log --job`.

**Observed:**
- Job env: `RUSTUP_TOOLCHAIN: 1.98.0`.
- Log line 144: `installed - rustc 1.98.0`.
- Log line 372: the exact command.
- Log line ~405: `Finished` in 33.39 s, after checking all 8 members.
- Step 5, `Run make release-gate`, concluded `success`, and all 8 jobs passed.
- The probe run on the same gate files prints the help URL `rust-clippy/rust-1.98.0/…#float_cmp`, which proves that 1.98.0 Clippy is the binary that runs.

**Exit:** the run and job conclude success.

**Status: demonstrated.**

### Plan R2: the existing `msrv` job

**Evidence:** the `verify.yml` diff, plus `check-release.py`.

**Observed:**
- `msrv:` became `release-gate:` and runs `make release-gate`. No `msrv` job remains.
- The checker exits 0 and prints "Release gate identity verified".

**Exit:** 0.

**Status: demonstrated.**

### Plan R3 and R4: a stable-only lint fails the PR that introduces it

**Requirement:** a PR that adds an exact float `assert_eq!` in a `purrassist-core` test fails a hosted PR job with `clippy::float_cmp`.

**Evidence:**
- Probe PR #30, commit `81aa453`, run 38430682936.
- Job `release-gate` 115411746395, compared with nightly job `native (ubuntu-24.04)` 115411746296 on the same merge commit.

**Observed:**
- `release-gate` was the only job that failed. Step 5 failed with:

  ```
  error: strict comparison of `f32` or `f64`
     --> crates/core/src/tool_call/from_node.rs:336:9
  … rust-1.98.0/index.html#float_cmp
  = note: `-D clippy::float-cmp` implied by `-D warnings`
  = help: … add `#[allow(clippy::float_cmp)]`
  make[1]: *** [Makefile:23: lint] Error 101
  make: *** [Makefile:59: release-gate] Error 2
  ```

- The other 7 jobs succeeded. The nightly `native` log shows both Clippy lines reaching `Finished`.
- PR #30 is CLOSED, a draft, with `mergedAt` null and no closing references.
- `git ls-remote --heads origin 'probe/*'` is empty.

**Exit:** the job's exit code is 2, which is the expected failure.

**Status: demonstrated.**

### Remediation fixes

#### H1 (G1): the gate jobs are pinned to the stable release toolchain

**Commands:**
- Caller-mandated probes H1a and H1a2: `RUSTUP_TOOLCHAIN` removed from both gate jobs.
- Caller-mandated probes H1b and H1b2: the nightly, bare and quoted, set in both gate jobs.
- `validate_release_gate(root)` on scratch copies.

**Observed:**
- H1a: FAIL, with "package job must set `env`" and "env must be exactly RUSTUP_TOOLCHAIN, found []" for both jobs.
- H1b: FAIL, with "RUSTUP_TOOLCHAIN must be the stable release toolchain, not nightly-2026-09-14" for both jobs.
- Unit tests `test_env_removed_from_both_jobs_fails` and `test_nightly_env_in_both_jobs_fails` pass.

**Expected:** FAIL, naming the cause.

**Status: demonstrated.**

#### N1 (G1): the checkout

**Command:** `ref: main` added to the verify checkout and the release checkout, as the first `with:` key, and in the flow form `with: {…, ref: main}`.

**Observed:** all four FAIL, with "step 1 `with:` must be exactly persist-credentials: false". Unit tests cover a tag-pinned or different action, and `uses` on a later step.

**Status: demonstrated.**

#### M2 and B1–B3 (G1): the allowlists

**Commands:** 14 probes:
- `continue-on-error` on the job;
- a quoted `'if': false` on the job;
- `- if: false` as the step's opening key;
- `continue-on-error` on the release gate step;
- a step `env:`;
- a YAML merge key;
- a `$GITHUB_ENV` write;
- a workflow-level `env:`;
- `make release-gate RUSTUP_TOOLCHAIN=nightly…`;
- a block-scalar `|| true`;
- an install step that smuggles in a second command;
- `pull_request` removed;
- a `paths` filter, and a `types` filter.

**Observed:** all FAIL, each with a message naming the key or rule. The must-pass probes also behave as required: swapped install order, a quoted env value with a trailing comment, and the real files all PASS.

**Status: demonstrated.**

#### M1 (G2): Clippy and the tests reachable from the gate

**Commands:** `lint` dropped from `check:`; `test` dropped from it; a target-specific `lint: export RUSTUP_TOOLCHAIN := nightly…`.

**Observed:** FAIL, with "check must depend on `lint`", "… on `test`", and "lint recipe must be exactly …, found None".

**Status: demonstrated.**

#### A1 (G3): corpus suites linted on 1.98.0

**Hosted, at `fd898ed`:**
- Line 407 of job 115411547933 runs `cargo clippy -p purrassist-core --locked --test correction_evaluation … -- -D warnings` on 1.98.0.
- Cargo prints `Checking purrassist-core`, which means new units were checked after line 1, and then `Finished`.

**Reused local evidence:** `validation.md` records a temporary `assert_eq!(…parse::<f64>().unwrap(), 0.5)` in `correction_evaluation.rs:673`:
- the workspace line passed it;
- the second line failed it with `float_cmp`, `make: *** [Makefile:24: lint] Error 101`;
- the change was reverted.

**Rule probes:** a suite removed, a new `test = false` target, `CORPUS_SUITES +=` and a dropped `-D warnings` each FAIL. The unit tests also cover `|| true`, a leading `-`, `?=` and `override`.

**Status: demonstrated.** The failing demonstration is local and reused; the command's execution is hosted.

#### M3 (G4): the PR #28 body

**Command:** `gh pr view 28 --json body`.

**Observed:**
- The body has no "Draft".
- It links runs 38376755652, 38382758016, 38430626503, 38377011106 and 38430682936.
- It gives the counts 110 and 260.
- It keeps the scope note, the corpus-lint note and the not-required-check note.

**Status: demonstrated.**

#### L2 and L3 (G5): pin extraction

**Commands:** a `cargo +1.97.0` step, a `${{ matrix.rust }}` install, a 1.97.0 pin in a new `extra.yml`, and an install with flags first plus a line continuation.

**Observed:**
- The first three FAIL, with "pins differ / does not match rust-version 1.98" and "toolchain pins must be literal".
- The last one PASSES, as required.
- The unit tests cover inline assignments, `default`/`override`, `stable`, `$VAR`, and `apt-get` and `install-qualification-tool` lines that must not be read as pins.

**Status: demonstrated.**

#### L4 (G6): docs follow the pin

**Command:** stable pin set to 1.98.1 in every workflow, then the gate, pin and `check-docs` rules run.

**Observed:**
- The gate and pin rules PASS.
- `check-docs` `release-toolchain` FAILS at `RELEASE.md:142`, `:145` and `:164`, and at `QUALIFICATION.md:251`.
- Hosted, the check-docs rule passes on the real files (log line 236).

**Status: demonstrated.**

#### L1 (G1): formatting false positives

**Observed:** a named gate step, a quoted env value, a trailing comment and swapped install order all PASS.

**Status: demonstrated.**

#### L5 (G8): docstrings

**Observed:**
- The `check-release.py` module docstring and the function docstrings are updated.
- `scripts/test-check-release.py:6` still says "Two groups, neither of which touches … this checkout" above three bullets. See F1.

**Status: partially fixed.** The remainder is advisory (LOW).

#### L6 (G8): wording

**Command:** `grep -rn 'release fuzz gate' README.md Makefile docs/`, then `grep -c 'make release-gate' README.md`.

**Observed:** the first grep exits 1 (no matches); the second finds 1 line.

**Status: demonstrated.**

#### L7: the duplicated loader

Left as is, with a reason. The gap analysis marked this fix optional.

**Status: accepted.**

#### G7: the probe rerun

**Observed:** it ran on the final head and failed only `release-gate`, as shown in the R3/R4 section. It is recorded in `validation.md` and the PR body.

**Status: demonstrated.** Recording it on issue #26 is still outstanding; see F2.

### Local checks and gate

| Check | Command | Observed | Exit |
|---|---|---|---|
| Release validator | `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | Release set; publish.yml; "Release gate identity verified"; "8 stable occurrences match rust-version 1.98, 12 nightly … nightly-2026-09-14" | 0 |
| Release validator tests | the same flags on `scripts/test-check-release.py` | `Ran 110 tests … OK` | 0 |
| Docs validator and tests | `check-docs.py` and `test-check-docs.py`, with the same flags | "Documentation facts … match their sources"; `Ran 260 tests … OK` | 0 / 0 |
| Gate | `make hygiene` | 52, 110, 260, 81 and 14 tests OK; resource schema valid | 0 |

## Open findings (first pass, at `fd898ed`)

**None blocking.** Each of the following is a real, small item. Under the run's own convention (`gap-analysis.md`: "MEDIUM and LOW are advisory"), none blocks.

**Status at `a3bbed0`:** F1 and F3 are fixed, and the Makefile advisory is addressed. F2 is still a closeout item. See the recheck section.

**F1 (LOW, residual of L5): `scripts/test-check-release.py:6`.**
- It still reads "Two groups, neither of which touches the network or this checkout:" above three bullets.
- The gate and pin tests do read this checkout: `test_real_files_pass` calls `CR.validate_release_gate()` on ROOT.
- `remediation-plan.md` says "Every finding is fixed, except L7", which overstates L5 by this line.
- **Fix:** "Three groups, none of which touches the network; the gate and pin groups also check the real files."

**F2 (closeout): the G7 rerun is not yet on issue #26.**
- G7 step 4 says "Record the run in `validation.md` and on the issue".
- Issue #26's last comment (16:27:53Z) predates the result of run 38430682936 / PR #30.
- **Fix:** the Stage 2 final summary to the issue must name PR #30, the run URL and the log excerpt.

**F3 (LOW): `docs/RELEASE.md:43` is ambiguous.**
- "These checks run in `make check` and `make package`" now follows the `check-docs.py` sentence.
- `check-docs.py` runs only in `make check` (`hygiene`), not in `make package`.
- **Fix:** attribute the sentence to `check-release.py`, or say that check-docs runs in `make check`.

## Advisory: Makefile directives the drift guard does not see (beyond the contract)

The remediation contract scoped its allowlists to the workflow files, plus the exact `release-gate`, `check`, `lint` and `CORPUS_SUITES` text in the Makefile. That contract holds.

Four one-line Makefile directives outside it pass both `validate_release_gate` and `validate_toolchain_pins` on scratch copies. Toy Makefiles under GNU Make 4.4.1 confirmed their effect:

| Directive (after `MAKEFLAGS += --no-print-directory`) | Toy result | Likely hosted effect (reasoned, not executed) |
|---|---|---|
| `unexport RUSTUP_TOOLCHAIN` | The recipe saw an empty variable; rc 0 | Recipes fall back to `rust-toolchain.toml` (the nightly, with clippy and rustfmt), so both lanes run green on nightly. This is the H1 consequence by another route. |
| `.IGNORE: lint` | `[lint] Error 1 (ignored)`; rc 0 | Clippy failures are ignored in both lanes. |
| `MAKEFLAGS += -i` | Not run as a toy; this is GNU Make's documented global form of `.IGNORE` | Every recipe error is ignored. |
| `export RUSTUP_TOOLCHAIN := nightly-…` | The recipe saw the override | The gate jobs' nightly has only `llvm-tools`, so `cargo fmt` or `cargo clippy` would fail loudly. Not silent. |

**Why these are advisory, not blocking:**
- Each is a deliberate directive whose only effect is to defeat the gate. Each is visible in review.
- None is claimed by the plan, RELEASE.md or the PR body. Their claims are scoped to the gate jobs and the recipe text, and those claims hold.
- The issue's acceptance and the remediation criteria are all demonstrated.

**For the owner:** `unexport RUSTUP_TOOLCHAIN` is the one with a plausible convenience motive: making the repository's toolchain file win over a shell setting. If the owner wants the standing guard to cover it, a single rule closes all four, with one fixture each. The rule rejects any Makefile line that names `RUSTUP_TOOLCHAIN`, `.IGNORE`, `unexport`, or `-i`/`--ignore-errors` in `MAKEFLAGS`.

## Failure-shape check

- **DARK:** no.
  - `validate_release_gate`, `validate_toolchain_pins` and `check_release_toolchain` run from `check-release.py main()` and `check-docs.py`, through `make hygiene` → `make check` → `make release-gate`.
  - Hosted log lines 206–237 at `fd898ed` print their messages.
- **TEST-ONLY:** no. The acceptance ran through the real hosted `pull_request` Verify workflow.
- **WEAKENED TESTS:** no. 110 and 260 tests ran; every new rule has a must-fail fixture asserting its message.
- **DUPLICATE:** no. The `msrv` job and the five split release steps are gone.
- **REFUSED, PRODUCER-WITHOUT-CONSUMER, VACUOUS OUTPUT:** not applicable.
- **LAUNDERED-TO-FOLLOWUP:** no. No required work moved elsewhere. The required check stays an owner setting, which the issue did not ask for.
- **SILENT NARROWING:** no. The issue's exact command runs unmodified as `make lint` line 1, on 1.98.0, in a pull-request job. The scope is a disclosed superset.
- **Attribution:** clean.
  - All 5 commits and the probe commit are authored and committed by Patrick_Audley.
  - `%(trailers)` is empty on every commit.
  - The diff contains no attribution text.

## Limits

- **Release qualification:** `release-qualification.yml` at `fd898ed` has not run, because it triggers only on a tag or a dispatch. Its equivalence to the pull-request lane rests on `check-release.py`'s structural rules, which ran hosted at `fd898ed`, and on the PR job running the identical `make release-gate`.
- **Required check:** `release-gate` is not a required status check. This is disclosed in RELEASE.md and the PR, and it is an owner setting.
- **Local 1.98.0 runs:** the local `make release-gate` and the corpus-suite `float_cmp` demonstration are the implementer's recorded runs. This audit did not re-run them.

## Recheck of `a3bbed0` (the delta on `fd898ed`)

### The delta

- `a3bbed0` has one parent, `fd898ed`. It is authored and committed by Patrick_Audley, with no trailers.
- `git diff --stat fd898ed a3bbed0` touches only three files:
  - `docs/RELEASE.md` (+5/-2);
  - `scripts/check-release.py` (+13);
  - `scripts/test-check-release.py` (+33/-2).
- `git diff fd898ed a3bbed0 -- .github Makefile crates Cargo.toml Cargo.lock rust-toolchain.toml scripts/check-docs.py` is empty.
- The new rule is in `validate_release_gate`, at `check-release.py:666-678`. It scans each Makefile line with comments stripped, and it fails on:
  - `RUSTUP_TOOLCHAIN` or `unexport`;
  - a leading `.IGNORE`;
  - a `MAKEFLAGS` assignment whose value has a dash flag cluster containing `i`, or `--ignore-errors`.

### Commands, at `a3bbed0`

| Command | Observed | Exit |
|---|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | All four "verified" lines, including "Release gate identity verified" | 0 |
| The same flags on `scripts/test-check-release.py` | `Ran 115 tests … OK`, up from 110. New: unexport, set, `.IGNORE`, `-i`/`--ignore-errors`/`-ki`, and the real `MAKEFLAGS` line passes | 0 |
| `check-docs.py` | "Documentation facts … match their sources" | 0 |
| `make hygiene` | 52, 115, 260, 81 and 14 tests OK; schema valid | 0 |

### My four Makefile edits, re-tried on scratch copies

`validate_release_gate(root)`; scratch files deleted afterwards.

| Edit | Result |
|---|---|
| 1. `unexport RUSTUP_TOOLCHAIN` | FAIL: "Makefile:4: must not set, export or unexport RUSTUP_TOOLCHAIN" |
| 2. `.IGNORE: lint` | FAIL: "Makefile:4: must not declare .IGNORE" |
| 3. `MAKEFLAGS += -i` | FAIL: "Makefile:4: MAKEFLAGS must not ignore errors (-i/--ignore-errors)" |
| 4. `export RUSTUP_TOOLCHAIN := nightly-2026-09-14` | FAIL: the RUSTUP_TOOLCHAIN message |
| Variants: a plain `RUSTUP_TOOLCHAIN :=` without `export`; `override undefine RUSTUP_TOOLCHAIN`; a bare `unexport`; `.IGNORE :`; `MAKEFLAGS+=-ki`; `--ignore-errors`; `-i` appended to the real line; `export MAKEFLAGS := -i` | All FAIL with the matching message |
| Must pass: the real Makefile; `MAKEFLAGS += --no-print-directory -j4 --output-sync=line`; a comment naming `RUSTUP_TOOLCHAIN`, `.IGNORE` and `unexport` | All PASS |

### Residual (advisory, LOW, not blocking)

Two deliberately evasive spellings still pass the checker. Toy Makefiles under GNU Make 4.4.1 show both ignore errors (`Error 1 (ignored)`, rc 0):

- **The POSIX bare-letter form:** `MAKEFLAGS = i --no-print-directory`. This is a documented spelling: the first word without a dash is read as flag letters.
- **Indirection:** `IGN := -i` followed by `MAKEFLAGS += $(IGN)`.

Neither spelling has any innocent purpose, and a textual checker cannot close indirection in general. Allowlisting the exact real line, `MAKEFLAGS += --no-print-directory`, as the only permitted `MAKEFLAGS` line would close both, in keeping with the plan's allowlist principle. This is the owner's option, not a contract gap.

### Findings status

| Finding | Status at `a3bbed0` |
|---|---|
| F1 | Fixed. `test-check-release.py:6` now says "Three groups, none of which touches the network; the gate and pin groups read scratch copies…". |
| F3 | Fixed. `RELEASE.md:46-47` says "`check-release.py` runs in `make check` and `make package`; `check-docs.py` runs in `make check`", which matches the Makefile. The new Makefile-guard sentence matches the code. |
| F2 | Still a closeout item. The coordinator states that the Stage 2 final summary to issue #26 will include PR #30 and run 38430682936. That summary must be posted before Stage 2 reports done. |

### Does the verdict depend on the hosted run on `a3bbed0`?

At the time of writing, Verify run 38438344049 on `a3bbed0` is `in_progress`, with all 8 jobs in progress.

**What does not depend on it:**
- Every criterion's behavior.
- The workflows, Makefile, crates, toolchain files and `check-docs.py` are byte-identical to `fd898ed`. So the hosted R1 evidence (run 38430626503) and the R3/R4 probe (run 38430682936) still apply to the gate `a3bbed0` runs.
- The delta itself is validated locally above.

**What does depend on it:**
- The changed `check-release.py` and `test-check-release.py` run inside `make hygiene` in every hosted `make check`, including the `release-gate` job and the `native` jobs. So the plan's exit condition, "a hosted run on the final head with every job green", is not yet met for `a3bbed0`.
- This PASS therefore applies to `a3bbed0` on condition that run 38438344049 concludes `success` on every job. Stage 2 must not report done, and Stage 3 must not proceed, until it does.
- If any job fails, this verdict does not cover `a3bbed0`, and the failure returns to remediation.

`a3bbed0` is no longer the head; it is superseded by `8c7ab3b` below. Its hosted run is not the final-head evidence.

## Final recheck: `8c7ab3b` (the delta on `a3bbed0`)

### The delta

- `8c7ab3b` has one parent, `a3bbed0`. It is authored and committed by Patrick_Audley, with no trailers.
- `git diff --stat a3bbed0 8c7ab3b` touches only three files:
  - `docs/RELEASE.md` (+2/-1);
  - `scripts/check-release.py` (+10/-3);
  - `scripts/test-check-release.py` (+7/-6).
- `git diff a3bbed0 8c7ab3b -- .github Makefile crates Cargo.toml Cargo.lock rust-toolchain.toml scripts/check-docs.py` is empty. These gate files are therefore still byte-identical to `fd898ed`.
- **The change:** `ALLOWED_MAKEFLAGS = 'MAKEFLAGS += --no-print-directory'`. Any Makefile line that mentions `MAKEFLAGS` (comments stripped) and is not exactly that line is rejected.
  - This replaces the earlier `-i` pattern.
  - The `MAKEFLAGS` fixture now covers: `-i`, `--ignore-errors`, `-ki`, `MAKEFLAGS = i --no-print-directory`, and `IGN := -i` + `MAKEFLAGS += $(IGN)`.
- **RELEASE.md:44-46** now says "any `MAKEFLAGS` setting other than `MAKEFLAGS += --no-print-directory`", which matches the code.

### Commands, at `8c7ab3b`

| Command | Observed | Exit |
|---|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | "Release gate identity verified", and "Toolchain pins verified: 8 stable … 12 nightly …" | 0 |
| The same flags on `scripts/test-check-release.py` | `Ran 115 tests … OK` | 0 |
| `check-docs.py` | "Documentation facts … match their sources" | 0 |
| `make hygiene` | 52, 115, 260, 81 and 14 tests OK; schema valid | 0 |

### My two spellings and regressions, re-tried on scratch copies

`validate_release_gate(root)`; scratch files deleted afterwards.

| Edit | Result |
|---|---|
| `MAKEFLAGS = i --no-print-directory` (POSIX bare-letter) | FAIL: "Makefile:3: MAKEFLAGS may only be set by `MAKEFLAGS += --no-print-directory`, so the gate cannot ignore errors" |
| `IGN := -i` + `MAKEFLAGS += $(IGN)`, with the real line kept | FAIL: the same message at Makefile:5 |
| The same indirection with the real line removed | FAIL at Makefile:4 |
| Regressions: `unexport RUSTUP_TOOLCHAIN`; `.IGNORE: lint`; `MAKEFLAGS += -i`; `export RUSTUP_TOOLCHAIN := nightly-…` | All FAIL with their messages |
| Variants: `-i` appended to the real line; `$(eval MAKEFLAGS += -i)`; `override MAKEFLAGS := -i` | All FAIL |
| Must pass: the real Makefile; the real line with trailing whitespace; a comment naming `MAKEFLAGS` | All PASS |

**Status:** the last advisory is closed. No open findings remain against the delta.

### Limits of a text-based guard (not a finding)

Two further spellings pass the checker. Toy Makefiles under GNU Make 4.4.1 show that both make the gate's sub-makes ignore errors (`Error 1 (ignored)`, rc 0):

- `export GNUMAKEFLAGS := -i`
- `MAKE := make -i`, which works because the gate recipe is `$(MAKE) check`.

Like the indirection already closed, each is a deliberate evasion with no innocent purpose, visible in review. A Makefile is a programming language, so a line-text guard can always be outrun by obfuscation. The guard now covers every plausible or documented direct spelling. If the owner wants more, one option is to allowlist the Makefile's top-level variable assignments. That is optional and does not affect this verdict.

### F2 and the hosted run on the final head

**F2 (closeout):** still pending. The Stage 2 final summary to issue #26 must name PR #30, run 38430682936 and its log excerpt.

**Hosted run:** at the time of writing, Verify run 38438995951 on `8c7ab3b` is `in_progress`, with all 8 jobs in progress.

**What does not depend on it:**
- Every criterion's behavior.
- The gate files are unchanged since `fd898ed`. So run 38430626503 (R1, success) and probe run 38430682936 (R3/R4, `release-gate` failing alone with `float_cmp`) still apply to the gate `8c7ab3b` runs.
- The deltas are validated locally above.

**What does depend on it:**
- The changed `check-release.py` and `test-check-release.py` run inside `make hygiene` in every hosted `make check`, including the `release-gate` and `native` jobs. So the plan's exit condition, "a hosted run on the final head with every job green", requires this run.
- If any job fails, this verdict does not cover `8c7ab3b`, and the failure returns to remediation.
- Stage 2 must not report done, and Stage 3 must not proceed, until the run is green and F2 is posted.

**The verdict applies to the final head `8c7ab3b` on two conditions:**
1. Verify run 38438995951 concludes `success` on all 8 jobs.
2. The F2 closeout summary is posted to issue #26.

VERDICT: PASS
