# Gap analysis: PR #28 for issue #26 (Stage 2, full path)

- **Issue:** #26, "CI: lint the whole workspace with the release toolchain on pull requests"
- **PR:** #28 (open, ready for review, not a draft)
- **Branch:** `paudley/26-lint-the-whole-workspace-with-the` at `fdd10ff`
- **Base:** `origin/main` at `8f1f42d`
- **Files changed:** 8 (+466 / -33)
- **Reviewer feedback:** none. There are 0 reviews, 0 review threads and 0 inline comments. The PR's two comments are the author's plan and handoff.

## Summary

The issue's requirement and its literal acceptance are met, with hosted evidence. One required gap is open:

- `check-release.py` is the drift guard the plan names as the lasting protection, but it does not pin the two gate jobs to the release toolchain.
- So both lanes can be moved to nightly while `check-release.py` and CI stay green (H1).

The fix touches only `check-release.py`, its tests and one sentence in RELEASE.md. After it, no probe rerun is needed, only a green hosted run on the new head.

Severity convention: CRITICAL and HIGH must be fixed before merge. MEDIUM and LOW are advisory.

## Checks run by this analysis

| Check | Result |
|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | Exit 0. Prints "Release gate identity verified" and "8 stable occurrences match rust-version 1.98, 12 nightly occurrences match nightly-2026-09-14". |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py` | 56 tests OK |
| `scripts/check-docs.py` | Exit 0 |
| `scripts/test-check-docs.py` | 252 tests OK |
| `make hygiene` | Exit 0 (52, 56, 252, 81 and 14 tests OK; resource schema valid) |
| 25 mutation probes on scratch copies of the six files the rules read (outside the repo; repo untouched) | See "Robustness" |
| `gh run view` of runs 38376755652, 38377011106 and 38382758016, plus job logs 115246980855 and 115227814445 | See "Hosted evidence" |
| Not run, per the brief | `make check`, `make release-gate` |

## Hosted evidence (re-verified live)

| Run | Head | Result | Evidence |
|---|---|---|---|
| 38376755652 | `2212844`, pull_request | All 8 jobs succeeded | `release-gate` job 115226959552, 10 min 34 s |
| 38377011106 | `f06cde1`, the probe (parent `2212844`) | Only `release-gate` (115227814445) failed. The other 7 jobs passed, including both nightly `native` jobs. | Log lines: 144 `rustc 1.98.0`; 363 `cargo clippy --workspace --all-targets --locked -- -D warnings`; 397-398 `strict comparison of f32 or f64` at `crates/core/src/tool_call/from_node.rs:336:9`; 407 help URL `rust-1.98.0/index.html#float_cmp`; 408 `-D clippy::float-cmp implied by -D warnings`; 409 `#[allow(clippy::float_cmp)]`; 412 `make[1]: *** [Makefile:23: lint] Error 101`; 413 `make: *** [Makefile:56: release-gate] Error 2` |
| 38382758016 | `fdd10ff`, final head, pull_request | All 8 jobs succeeded | `release-gate` job 115246980855 (13 min 16 s). Log lines: 144 rustc 1.98.0 installed; 156 nightly-2026-09-14 installed; 162 env `RUSTUP_TOOLCHAIN: 1.98.0`; 164 `make check`; 363 the clippy command, finished at 397; 2785 `make corpus`; 2869 `test-fuzz-feedback.py` OK; 2875 `test-verify-published.py` (10 tests) OK; 2881 `make package`; then "Verified 6 release archives". |

**Gate files unchanged since the probe:** `git diff --name-only 2212844 fdd10ff -- .github Makefile` is empty. `fdd10ff` changed only QUALIFICATION.md, RELEASE.md and `test-check-release.py`, so the probe still applies to the final head.

**Probe cleanup:** PR #29 is CLOSED, a draft, with `mergedAt` null, the planned title and no closing keyword. `git ls-remote --heads origin probe/26-float-cmp` returns nothing.

**Attribution:** clean. The author and committer of both commits are the user, and no trailer or attribution term appears in either commit message, the diff, the PR body or the PR comments.

## Requirements coverage (ledger R1-R24)

| # | Requirement | Status | Location | Gap |
|---|---|---|---|---|
| R1 | Workspace Clippy with `--locked -- -D warnings` on 1.98.0 in a PR job | COMPLETE | `Makefile:23`; `verify.yml:35-49`; run 38382758016 log lines 144 and 363 | - |
| R2 | A stable-only lint fails on the PR that introduces it | COMPLETE for the demonstration; standing guard PARTIAL | run 38377011106 | The guard has false negatives on this exact property (H1, M1) |
| R3 | Exact float `assert_eq!` in a core test fails a hosted PR job with `clippy::float_cmp` | COMPLETE | job 115227814445, log lines 397-413 | - |
| R4 | Required status check is out of scope | COMPLETE (recorded) | `docs/RELEASE.md:146-149` | - |
| R5 | Cost of declining the required check is recorded | COMPLETE | `docs/RELEASE.md:146-149`; issue comment 3 | - |
| R6 | Probe branch from the Task 1 head | COMPLETE | `f06cde1`, parent `2212844` | - |
| R7 | Normal commit, no attribution, "deliberate, must not merge" | COMPLETE | `tasks/T2-review.md` | - |
| R8 | Local 1.98.0 Clippy reports `float_cmp` | COMPLETE (recorded) | `validation.md:18` | - |
| R9 | Draft PR with the planned title and no closing keyword | COMPLETE | PR #29, live | - |
| R10 | `release-gate` fails with `float_cmp` while `native` passes | COMPLETE | run 38377011106 | - |
| R11 | Run URL and log excerpt in `validation.md` and on #26 | COMPLETE | `validation.md:19-34`; issue comment 3 (verbatim excerpt and both URLs) | - |
| R12 | Probe PR closed unmerged, only its branch deleted | COMPLETE | PR #29; `ls-remote` returns nothing | - |
| R13 | `tasks/T2-review.md` checks the evidence | COMPLETE | PASS | - |
| R14 | Item 4 residual and required-check cost recorded | COMPLETE | plan; `docs/RELEASE.md:146-149` | - |
| R15 | MSRV-evidence sentence | COMPLETE | `docs/QUALIFICATION.md:258-260` | - |
| R16 | Defect class is gate divergence | COMPLETE | one gate target | - |
| R17 | `make release-gate` holds the five gates | COMPLETE | `Makefile:55-60` | - |
| R18 | Release workflow runs it as its one verification step, after the version check | COMPLETE | `release-qualification.yml:33-43` | - |
| R19 | PR job `release-gate` runs the same target on the same two toolchains | COMPLETE now; guard PARTIAL | `verify.yml:35-49` | The toolchain the gate runs on is not pinned to stable (H1) |
| R20 | `make lint` gains `--locked` | COMPLETE | `Makefile:23` | - |
| R21 | `msrv` becomes `release-gate` | COMPLETE | `verify.yml:35` | - |
| R22 | Drop `RUSTFLAGS`; env equals the release job's | COMPLETE | `verify.yml:41-42`; `check-release.py:379` | - |
| R23 | `windows-engines` Clippy step kept | COMPLETE | `verify.yml:63-64` | - |
| R24 | Permanent failing fixture declined; structural rules guard against drift | PARTIAL | `scripts/check-release.py:325-460` | The structural guard misses the gate moving off the release toolchain (H1) and Clippy leaving the gate (M1) |

### Plan contract rows

| Id | Status | Note |
|---|---|---|
| Plan R2 ("with `RUSTUP_TOOLCHAIN: 1.98.0` ... checked by the `check-release.py` rule in P1", `plan.md:95`) | PARTIAL | The job and its `make` command are checked. The env value is checked only for equality with the release job, never for being the stable pin (H1). |
| P1 | COMPLETE apart from the R2 env clause | Step list, single `make`, installs, env equality, recipe and lint pinned, with a fixture for each |
| P2 | COMPLETE | Probes C1-C5 behave as specified |
| P3 | COMPLETE | QUALIFICATION.md and RELEASE.md updated; `check-docs.py` passes |

## Acceptance criteria status

| Criterion | Status | Evidence |
|---|---|---|
| Issue: a PR adding an exact float `assert_eq!` in a `purrassist-core` test fails a hosted PR job with `clippy::float_cmp` | MET | Run 38377011106, job 115227814445, log lines 397-413. Only this job failed. |
| Plan R1: on the hosted run, `release-gate` uses 1.98.0, runs the clippy command and succeeds | MET | Run 38382758016 on the final head, job 115246980855 |
| Plan R1: local `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` exits 0 | MET (recorded, not re-run here) | `validation.md:6` |

## NEEDS-JUDGMENT resolutions

1. **Rename `msrv` or add a separate job (R1 vs R21). Resolved: the rename is correct, no gap.**
   - The issue says "for example the existing `msrv` job"; the branch renames it (`verify.yml:35`).
   - The MSRV evidence is kept: `make check` builds and tests the workspace on 1.98.0 (log line 144 and the test run), and `docs/QUALIFICATION.md:258-260` says so.
   - Nothing referenced the job id `msrv`: the `main` ruleset has no required status checks.

2. **Whole release gate or only Clippy (R1 vs R16 and R19). Resolved: accept the broader scope.**
   - The issue's exact command runs (log line 363), so the literal ask is a strict subset of what the job does.
   - `.goals` (MAXIMAL UTILITY; HARD FAILS) favours running every release-only gate on pull requests: corpus, the verifier self-test, and the stable `cargo package` build.
   - The broadening is disclosed in the PR body and in issue comment 3, for the owner to object to.
   - Cost: the `release-gate` job takes 10.5 to 13.3 minutes. Wall time is unchanged, because `fuzz` (about 40 minutes) is the longest job.
   - Any owner objection is a Stage 3 review decision, not an implementation gap.

3. **`clippy::float-cmp` in the log vs `clippy::float_cmp` in the acceptance text. Resolved: met literally.**
   - The failing log contains `clippy::float_cmp` (line 409, `#[allow(clippy::float_cmp)]`) and `#float_cmp` (line 407).
   - `-D clippy::float-cmp` (line 408) is rustc's lint-level display of the same lint.

4. **Is one probe enough for a standing acceptance (R3 vs R24)? Resolved, with a condition.**
   - One hosted probe, on gate files byte-identical to the final head, meets the acceptance as written.
   - The standing property ("a stable-only lint fails the PR that introduces it") rests on the structural guard. The plan made that guard the lasting protection instead of a permanent fixture (prior-art item 6; R24).
   - That guard has false negatives on this exact property: H1 (env pin) and M1 (Clippy reachability).
   - With H1 fixed, and M1 as recommended, the standing property is guarded. Without H1, the guard does not hold what the plan says it holds.

5. **Should `release-gate` be a required check (R5)? Resolved: out of scope, and not a deferral.**
   - It is a repository-owner ruleset setting, and the issue did not ask for it.
   - `docs/RELEASE.md:146-149` states the position accurately: the ruleset has no `required_status_checks`.
   - RELEASE.md's pre-tag step 2 ("Every job must pass") still catches a red `release-gate` on the commit to be tagged.
   - This is an owner action item, not an open gap.

## Integration: consumers traced

| Consumer | Finding | Status |
|---|---|---|
| `publish.yml` `gate` job (`publish.yml:36-62`) | Polls `actions/workflows/release-qualification.yml/runs?head_sha=...` and accepts any run whose conclusion is `success` (lines 45-46). It depends only on the file name and the run conclusion; the `package` job id and its step names don't matter. Unaffected. A `continue-on-error` or `if:` on the gate step would let qualification succeed without the gate, and nothing checks for that (M2). | OK |
| `publish.yml` publish job, `check-release.py --tag` (line 85) | Now also runs `validate_release_gate` and `validate_toolchain_pins`, and imports `check-docs.py`, which needs `tomllib` (Python 3.11+; ubuntu-24.04 has 3.12). It fails closed, and it is harmless at publish time. | OK |
| `release-qualification.yml` step 3 (lines 33-41) | Same as above, run before the gate. | OK |
| `check-docs.py` `release-qualification-steps` rule (`check-docs.py:794-812`) | Requires every named step in RELEASE.md's "Qualification workflow" section. `docs/RELEASE.md:138` names `Verify the release gate`, and `check-docs.py` exits 0. | OK |
| `check-docs.py:1315` loads `check-release.py` for `RELEASE_ORDER` | `check-release.py` now loads `check-docs.py`, but only lazily, inside `validate_release_gate` (`check-release.py:253`). There is no import cycle. | OK |
| `scripts/test-check-docs.py:1124` | Asserts `Verify the release gate`. The old step names survive only in a negative fixture in `test-check-release.py`. | OK |
| Makefile targets reached by `release-gate` | `check` (fmt, hygiene, helper-test, helper-check, lint, test, doc), `corpus`, `test-fuzz-feedback.py`, `test-verify-published.py` and `package` all exist, and all appear in order in job 115246980855's log. | OK |
| `check-release.py` in `make hygiene` (`Makefile:14`) and `make package` (`Makefile:48`) | Runs at log lines 209-221 and 2885-2886. `Makefile:14` runs it without the encoding flags, which predates this PR. This analysis ran it with the flags: exit 0. | OK |
| `verify.yml:26-27`: the native job re-runs `test-check-release.py` on Linux and macOS | 56 tests pass on both in run 38382758016. | OK |
| `scripts/verify-published.py:64` loads `check-release.py` for `RELEASE_ORDER` | Only module-level constants were added. Unaffected. | OK |

**WIRING_GAPS:** none. The added lines contain no TODO, FIXME, placeholder, stub or follow-up markers.

## Robustness of the new `check-release.py` rules

Each probe is one edit to scratch copies of the Makefile, `Cargo.toml`, `rust-toolchain.toml` and the three workflows.

### False negatives: both validators PASS, but they should fail

| Probe | Edit | Effect | Finding |
|---|---|---|---|
| A1 | Remove `RUSTUP_TOOLCHAIN` from both gate jobs | Both lanes run `make release-gate` on nightly-2026-09-14 (from `rust-toolchain.toml`) | H1 |
| A2 | Set both gate jobs' `RUSTUP_TOOLCHAIN` to the nightly | Same; the nightly pin is accepted because it equals the channel | H1 |
| A3 | Drop `lint` from the `check:` prerequisites | The issue's command leaves both lanes | M1 |
| A4 | `continue-on-error: true` on the verify `release-gate` job | A red gate shows green | M2 |
| A5 | A step that writes `RUSTUP_TOOLCHAIN=nightly-...` to `$GITHUB_ENV` before the verify gate | The PR lane runs on nightly | M2 |
| A6 | Step-level `env:` (`RUSTFLAGS: --cap-lints=warn`) on the verify gate step | Lints weakened in one lane | M2 |
| A7 | `continue-on-error: true` on the release gate step | Qualification succeeds when the gate fails, and the publish gate accepts it | M2 |
| A8 | `if: false` on the release gate step | Qualification succeeds without running the gate | M2 |
| C6 | Extra verify step running cargo with a plus-version override | The pin is never read | L3 |

### False positives on legitimate edits: all fail loudly and name a rule

| Probe | Edit | Message | Finding |
|---|---|---|---|
| B1 | Give the verify gate step a `name:` | "must run `make release-gate` as its only make command; found ['run: make release-gate']", which is confusing | L1 |
| B2 | Quote the env value in one job | env inequality | L1 |
| B3 | Trailing comment on an env value | env inequality | L1 |
| B4 | Swap the order of the two installs in verify | "must install the same toolchains" | L1 |
| B5 | Add a matrix job pinning `${{ matrix.rust }}` | "stable toolchain pins differ ... `${{`"; the matrix values themselves are never read | L2 |
| B7 | Flags before the name: `... install --profile minimal 1.98.0` | The pin is read as `--profile` | L2 |
| B11 | Line continuation: `... install \` | The pin is read as `\` | L2 |
| B8 | Name the checkout step | Step-list mismatch | Intended strictness |
| B10 | Trailing comment on a step `name:` line | Step-list mismatch; the reused `check-docs.py` parser keeps the comment in the name (predates this PR) | Acceptable |

### Correct behaviour

| Probe | Edit | Result |
|---|---|---|
| B6 | Add an OS-matrix job with literal pins | PASS |
| B9 | Whole-line comments mentioning `make` and a 1.99 install, inside both jobs | PASS; comments are ignored |
| C1 | Patch bump to 1.98.1 in all three workflows | PASS |
| C2 | 1.99.0 everywhere, plus `rust-version = "1.99"` | PASS |
| C3 | 1.99.0 everywhere, `rust-version` left at 1.98 | FAIL, "does not match Cargo.toml rust-version 1.98". Intended: the gate job is the MSRV evidence, as `docs/RELEASE.md:29-30` documents. |
| C4 | New nightly in `rust-toolchain.toml` and every workflow | PASS |
| C5 | Stable and nightly bumped together, consistently | PASS. C1 and C5 leave the docs stale (L4). |

## Standing goals (.goals, AGENTS.md, ETHOS)

| Goal | Status | Notes |
|---|---|---|
| `.goals`: LOW/NO OPTIONALITY, HARD FAILS | COMPLIANT | The gate fails pull requests instead of tags. The checker raises `SystemExit` listing every failure. |
| `.goals`: MAXIMAL UTILITY | COMPLIANT | The whole release gate runs on pull requests. |
| ETHOS: one path (section 19) | COMPLIANT | One Makefile target serves both lanes. |
| AGENTS: `make check` is the local gate; packaging needs its own evidence | COMPLIANT | `check` is unchanged apart from `--locked`; `release-gate` composes it with `package`. |
| AGENTS: capability claims need executable evidence | COMPLIANT for the docs | The plan's R2 clause "checked by P1" is unsupported for the env pin (H1). |
| AGENTS: docs distinguish implemented behaviour from plans | MOSTLY | The PR body still reads as a draft plan (M3). |
| AGENTS: register dependencies; SPDX headers | COMPLIANT | Standard library only (`importlib`, `shutil`). No new files. |
| AGENTS: never bypass hooks | COMPLIANT | Normal commits; no hooks are installed. |
| No attribution | CLEAN | See "Hosted evidence". |
| SOLID: SRP | Minor concern | `check-release.py`'s module docstring (line 4) still describes only the publish set, though the script now also owns gate identity and toolchain pins (L5). |

## Validation and deferrals

- **No stubs or deferrals:** there are no stubs, no placeholders, and no work moved to a follow-up issue.
- **Declined items are justified:** the required check, the permanent failing fixture, and removing the `windows-engines` Clippy step are each justified and recorded. None is in-scope work that was deferred.
- **Local runs are recorded only:** the local 1.98.0 `make release-gate` (652 s) and the nightly `make check` appear only in `validation.md:6-7`. They were not re-run here, per the brief. The hosted final-head run covers the same gate.
- **After the H1 fix:** that fix touches only `check-release.py`, `test-check-release.py` and RELEASE.md. `verify.yml` and the Makefile stay unchanged, so no probe rerun is needed. Task 3's rule still requires a green hosted Verify run on the new head.

## Findings

### H1 [HIGH]: the drift guard does not pin the gate jobs to the release toolchain

**Where:**
- `scripts/check-release.py:379-394`: `validate_release_gate` compares the two jobs' `env` only with each other, and checks only that each job installs one stable and one nightly toolchain.
- `plan.md:95`: the R2 acceptance.
- Plan prior-art item 6 and R24: the structural guard as the lasting protection.

**Evidence:**
- Probes A1 and A2 pass both `validate_release_gate` and `validate_toolchain_pins`.
- With `RUSTUP_TOOLCHAIN` removed from both gate jobs, both run `make release-gate` on nightly-2026-09-14 from `rust-toolchain.toml`.
- With both set to the nightly, the nightly pin rule accepts it, because it equals the channel.
- Either way the stable Clippy lane is gone while `check-release.py` and CI stay green. That is issue #26's defect returning undetected.
- No other script checks this: only `test-fuzz-feedback.py` sets the variable, and only for its child process.

**Why HIGH and not CRITICAL:** no shipped document or PR text claims this check. `docs/RELEASE.md:23-31` claims "the same installs and environment", which holds. But the plan's R2 acceptance says P1 checks it, and the completeness review marked R2 delivered without testing that.

**Fix:**
1. In `validate_release_gate`, after the env-equality check, require that `_job_env(release).get('RUSTUP_TOOLCHAIN')` exists, does not start with `nightly`, and equals the job's single stable install name. Env equality then covers the verify job too. Report a failure as, for example, "release qualification and the release-gate job must run on the stable release toolchain: `RUSTUP_TOOLCHAIN` must be <stable install>, found <value>".
2. Add two fixtures to `ReleaseGateIdentity` (`scripts/test-check-release.py:261`): env removed from both jobs, and env set to the nightly in both jobs.
3. Extend `docs/RELEASE.md:25-27` to say both jobs run with `RUSTUP_TOOLCHAIN` set to that stable pin.

### M1 [MEDIUM]: Clippy's reachability from the gate is not pinned

**Where:** `Makefile:6` (`check: fmt hygiene helper-test helper-check lint test doc`). `scripts/check-release.py:406-409` pins only the `lint` recipe.

**Evidence:**
- Probe A3 (`lint` removed from `check:`) passes.
- The issue's command would then run in neither lane, while `docs/RELEASE.md:139-140` says `make check` includes Clippy.
- There is precedent for moving work out of `check` to speed it up (`fecda89`).

**Fix:** assert that `'lint'` (and `'test'`) is among `_make_recipe(makefile, 'check')[0]`, with a fixture that drops `lint` from `check`. Recommended in the same change as H1.

### M2 [MEDIUM]: step and job modifiers can bypass the gate in either lane

**Where:** `scripts/check-release.py:355-376`. The release step list is checked only by step names and `make` lines; the verify job only by `make` lines.

**Evidence:**
- Probes A4 to A8 all pass: `continue-on-error`, `if: false`, a step-level `env:`, and a `$GITHUB_ENV` write.
- A7 and A8 let release qualification conclude `success` without the gate, and `publish.yml:45-46` accepts any successful run.
- The exposure predates this PR (the old five steps had it too). But the new closed step list is the natural place to close it, and `docs/RELEASE.md:23-24` implies the job holds only the gate.

**Fix:**
- In both gate jobs, reject `continue-on-error:` and `if:` at job and step level, and reject step-level `env:`.
- Close the verify job's step list as well: checkout, the two installs, then `make release-gate`. Read it with `workflow_steps` instead of matching the literal `- run:` text; this also fixes probe B1 (L1).

### M3 [MEDIUM]: the PR body is stale and will feed the structured squash

**Where:** PR #28 body, lines 4 and 22 (live; last updated 2026-10-13T11:03:36Z).

**Evidence:**
- The body still says "**Draft:** the hosted `release-gate` job must pass on this head, and the acceptance probe ... must fail it ... before this is ready." The PR is no longer a draft, and both conditions are met.
- "`test-check-release.py`: 54 tests" is now 56.
- The body has no hosted run links.
- Merges to `main` are structured squashes built from the PR (`8f1f42d`: `GhpRsq-Merge-Style: structured-squash`), so this text can reach `main`'s history.

**Fix:** edit the body:
- remove the Draft paragraph;
- add the hosted evidence (run 38382758016, probe run 38377011106, probe PR #29);
- change the test count to 56;
- keep the scope note and the required-check note.

### L1 [LOW]: formatting false positives in the identity checks

**Where:**
- `scripts/check-release.py:292`: env values are compared as raw text, quotes and trailing comments included.
- `scripts/check-release.py:374`: literal `'- run: make release-gate'`.
- `scripts/check-release.py:392`: install lists are compared in order.

**Evidence:** probes B1 to B4 each fail on a semantically identical workflow. B1's message ("found ['run: make release-gate']") is confusing.

**Fix:** strip quotes and trailing `#` comments from env values; compare installs as sorted lists; find the verify `make` step with `workflow_steps`.

### L2 [LOW]: the pin extractor misreads non-literal or reformatted pins

**Where:** `scripts/check-release.py:249-250` (`ENV_PIN`, `INSTALL_PIN`).

**Evidence:** B5 reads `${{` as a stable pin and never reads the matrix values; B7 reads `--profile`; B11 reads `\`. All three fail, but as "stable toolchain pins differ", pointing at a fake pin.

**Fix:** accept as a pin only names matching `^(nightly-\d{4}-\d{2}-\d{2}|\d+\.\d+\.\d+)$`. Fail anything else in a pin position with "unsupported pin form at file:line; pin the toolchain literally".

### L3 [LOW]: pins outside env and install lines are not read

**Where:** `scripts/check-release.py:430-460`.

**Evidence:** C6 (a cargo plus-version override), A5 (a `$GITHUB_ENV` write) and inline `RUSTUP_TOOLCHAIN=... cmd` assignments are not scanned.

**Fix:** also read `RUSTUP_TOOLCHAIN=` assignments and cargo plus-version selectors in `run:` lines, or reject them in workflows.

### L4 [LOW]: a pin bump passes the checker but leaves docs stale

**Where:** `docs/QUALIFICATION.md:251` and `:259`; `docs/RELEASE.md:129`, `:132` and `:150`; the comment at `verify.yml:36`.

**Evidence:** probes C1 and C5 pass, and these lines would still say 1.98.0. This PR added two of these hard-coded commands.

**Fix:** have `validate_toolchain_pins`, or a `check-docs.py` rule, also read the `RUSTUP_TOOLCHAIN=` values in QUALIFICATION.md and RELEASE.md.

### L5 [LOW]: stale docstrings

**Where:**
- `scripts/test-check-release.py:6` says "Two groups" over three bullets.
- `scripts/check-release.py:4` says the script checks only "the complete publish set and dependency order".

**Fix:** change the first to "Three groups". Extend the second to cover gate identity and toolchain pins.

### L6 [LOW]: "release fuzz gate" wording beside a target named `release-gate`, and README lacks the new command

**Where:**
- `README.md:290`: "the CI and release fuzz gate".
- `Makefile:83`: "The hosted and release fuzz gate".
- `README.md:284-293`: the develop-and-verify command list has no `make release-gate`.

**Evidence:**
- `fdd10ff` fixed the equivalent sentence in QUALIFICATION.md, but these two still call `fuzz-hosted` a release gate, and `make release-gate` does not run it.
- They mean RELEASE.md's pre-tag checklist, so they are not false, but they invite the same misreading.

**Fix:**
- Reword both to "the CI `fuzz` job and the pre-tag fuzz check (docs/RELEASE.md)".
- Add `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` to README's list, with a comment saying it is the gate release qualification and the `release-gate` PR job run.

### L7 [LOW]: duplicated module loader

**Where:** `scripts/check-release.py:253-261` duplicates `load_module` in `scripts/check-docs.py:176-183` (and `_load` in `test-check-release.py`).

**Fix:** optional; acceptable at this size.

## Reviewer feedback

None to address:
- PR #28 has 0 reviews, 0 review threads and 0 inline comments. Its two comments are the author's plan and handoff.
- Issue #26's comments are all the author's.

## Priority ranking

1. **[HIGH]** H1: the gate jobs are not pinned to the stable release toolchain, so the issue's defect can return undetected.
2. **[MEDIUM]** M1: Clippy's reachability from `make release-gate` (`check: ... lint`) is not pinned.
3. **[MEDIUM]** M2: `continue-on-error`, `if:`, step env and `$GITHUB_ENV` writes can bypass or alter the gate in either lane.
4. **[MEDIUM]** M3: the PR body is stale (draft text, 54 tests) and feeds the structured squash.
5. **[LOW]** L1 to L7: formatting false positives, the pin extractor, uncovered pin forms, doc pins, docstrings, fuzz wording and README, and the duplicated loader.

## Recommended actions

1. **Fix H1 and M1 together, in `validate_release_gate`:**
   - Add the stable env-pin assertion and the `check` -> `lint` (and `test`) prerequisite assertion.
   - Add three fixtures: env removed from both jobs, env set to the nightly in both jobs, and `lint` dropped from `check`.
   - Add one clause for each to `docs/RELEASE.md:23-31`.
   - Run `check-release.py` (with the encoding flags), `test-check-release.py`, `check-docs.py` and `make hygiene`.
   - Commit normally and push.
   - Confirm a green hosted Verify run, `release-gate` included, on the new head. No probe rerun is needed, because `verify.yml` and the Makefile are unchanged.
2. **Fix M3:** refresh the PR #28 body as described under M3.
3. **Fix M2:** reject `continue-on-error`, `if:` and step-level env in both gate jobs, and close the verify job's step list. This is the same function, so it can go in the same commit as action 1.
4. **L1 to L7:** at the implementer's discretion.

VERDICT: BLOCKED
