# Historical fit: PR #28 (issue #26), head `fdd10ff`

Scope: `git diff origin/main...HEAD`, 2 commits (`2212844`, `fdd10ff`), 8 files. Read-only git, files and `gh` views.

## Verdict

The change fits history and doctrine. It reverses no deliberate earlier split, and it extends the one precedent that matters (`fecda89`, which already kept release-only gates out of `make check`). Three points need the reviewer's attention:

1. PR-time cost rises by one long Ubuntu job (about 11 to 13 minutes), duplicating work three existing jobs already do.
2. The new `release-gate` job is not a required check, so it blocks nothing by itself.
3. `scripts/check-release.py` now hard-codes the job name `release-gate` and the shape of `verify.yml`. That is a new coupling, and it is deliberate and tested.

## 1. Precedent

### release-qualification.yml (5 commits)

| Commit | Date | Change and stated reason |
|---|---|---|
| `de8db9d` | 2026-09-28 | Created with three steps: `make check` then `make package`. No reason given for the split. |
| `7882703` | 09-28 | Header comment: "Qualification only", "runs the full local gate on stable Rust". Establishes that publish.yml gates on this workflow. |
| `51ed930` | 09-29 | Added the nightly install for the coverage probe. The comment says "The workspace gate remains stable". |
| `fecda89` | 10-07 | Added `make corpus`, `test-fuzz-feedback.py` and `test-verify-published.py` as separate steps. |
| `2212844` | 10-13 | (this PR) Collapsed steps 4 to 8 into `make release-gate`. |

**Did the old step split record a rationale that this PR contradicts? No.**

- `fecda89` is the only message that explains the step list. Its reason for the split was speed of `make check` ("ran for 30+ minutes; it now runs in about a minute"). Corpus suites became `test = false` targets with their own lanes, and "release qualification runs make corpus, the fuzz-feedback probe and the published-crate verifier self-test".
- That is a split of the Make targets, not of the workflow steps. Workflow steps were simply the place the extra gates were listed. The PR keeps `make check`, `make corpus` and `make package` as separate targets and only puts a list in the Makefile instead of the YAML.
- The cost is lost per-step visibility in the Actions UI. A failure now reads `make: *** [Makefile:56: release-gate] Error 2`, and the failing gate is found from the log (the probe run's log showed the clippy error above it). Nothing in history says per-step granularity was wanted.
- The same pattern already exists in the repo: `check: fmt hygiene helper-test ...` is a list of gates in the Makefile.

### verify.yml `msrv` job

- Added in `46e9d1a` (the foundation commit) as a tests-only job on 1.98.0 with `RUSTFLAGS: -D warnings`. It was never touched again; `git log -p` shows no Clippy line ever added.
- The `windows-engines` Clippy step (added in `0b7c7de`) was the only 1.98.0 Clippy. That was a Windows-`cfg` fix, not a stable-lane decision.
- Replacing `msrv` loses no recorded rationale. The MSRV claim (`rust-version = "1.98"`) is still evidenced, since `make release-gate` builds and tests on 1.98.0. QUALIFICATION.md says so in its changed sentence.
- It also drops `RUSTFLAGS: -D warnings` from the job. `make test` already sets `-D warnings` itself (`RUSTFLAGS="$${RUSTFLAGS:-} -D warnings" cargo test`), and `check-release.py` now requires the two lanes' `env` blocks to be equal.

### Makefile

- `check`, `lint`, `test` and `package` are unchanged except for `--locked` on `lint`. `fecda89` had earlier fixed `check` at "about a minute"; the new target does not touch that.
- `release-gate` is deliberately a recipe list with no prerequisites. The comment says this keeps order under `make -j`, and `check-release.py` pins it.
- `make fuzz-hosted` was added in `7e30c67`/`cbecc8c` as the "release fuzz gate", and docs said the "release gate" runs it. It never ran in release-qualification.yml. `fdd10ff` corrects QUALIFICATION.md. This is a documentation-drift repair, and the repo has two earlier `documentation-drift` classes on record.

### check-release.py

- It grew in `b7475b4` (publish preflight), then in a run of #1 commits (`44e5dd1`, `f6283b9`, `b1c9645`, `afc180a`, `edcc413`, `e34c665`) that hold `publish.yml` to a closed shape.
- This PR adds 233 lines on the same pattern: text-level, standard-library-only workflow checks with fixture tests. The plan restricts the closed-shape rules to `publish.yml` and applies only the identity and pin rules to `verify.yml`. That is consistent with precedent.

## 2. Doctrine

- **AGENTS.md:** "`make check` is the local verification gate; packaging and publication require their own evidence." The PR does not redefine `make check`. It adds a distinct `release-gate` that composes `check` with `corpus`, the two self-tests and `package`.
- **AGENTS.md: "Capability claims require executable evidence"; "tests and public documentation must distinguish implemented behavior from plans".** Evidence for the claim exists in `validation.md`:
  - hosted passing run 38376755652 (R1);
  - a probe PR #29 whose `release-gate` job failed with `clippy::float_cmp` while `native` stayed green (R3, R4), at run 38377011106;
  - a final green run on `fdd10ff`, run 38382758016.
  
  The claim in RELEASE.md that the job is "not yet a required status check" is accurate (see section 4).
- **AGENTS.md: "Never bypass commit hooks".** No hook-related change.
- **`.goals`:** "LOW/NO OPTIONALITY, HARD FAILS" is served: the release lane's gates now run on every PR instead of at the tag. "MAXIMAL UTILITY" is served; nothing in `.goals` weighs against the CI minutes.
- **RELEASE.md:**
  - Its step 2, the pre-tag checklist, still requires every `verify.yml` job to pass on the commit to be tagged. The new job is covered automatically by "Every job must pass".
  - The PR updates the `check-release.py` paragraph and the qualification step list, which `check-docs.py` requires to name every step.
  - The 0.1.0 publication record (lines 324 to 330) cites the float_cmp failure as the reason the publish gate stopped. That history is retained and not edited.

## 3. Recurring failures in this area

Counted from the prior-art assessment and checked against `git log`:

| Event | Where it broke | Fix did not change the gate? |
|---|---|---|
| `0b7c7de` (2026-09-28), UNKNOWN stable-only attribution | windows-engines Clippy, first added | Not counted |
| `de8e8fd` (09-28), `too_long_first_doc_paragraph` | windows-engines 1.98.0 Clippy | Code-only fix |
| `2864e75` (10-12), fixed in `76d5e73` (#22), same lint x3 | windows-engines 1.98.0 Clippy | Code-only fix |
| `598d059` (#24, 10-12), `clippy::float_cmp` | release qualification at the tag | Code-only fix |

- Stable-only lint breaks: 3 counted, plus 1 UNKNOWN. All three fixes touched code and none aligned the PR gate with the release gate. This PR is the first to do so.
- Windows-lane Clippy failures happened twice on 1.98.0 only because the Windows lane was the one place stable Clippy ran. The third time (`float_cmp`) there was no PR lane at all, and the tag failed. That is the same defect class (lane divergence) and supports the "gate identity" framing.
- Two related drifts: `fuzz` failed in CI on a Node mismatch (fixed in `76d5e73`/`f07b15b`), and `fdd10ff` here corrects the "release gate runs fuzz-hosted" claim. Both are doc/CI divergence, not Clippy.

## 4. Consumers of the changed artifacts

| Consumer | Effect | Status |
|---|---|---|
| `publish.yml` `gate` job | Polls `actions/workflows/release-qualification.yml/runs?head_sha=...` and tests `.conclusion=="success"`. It depends on the file name, not on step names or job id. Both are unchanged (`package` job, same file). | Unaffected. |
| `check-docs.py` `release-qualification-steps` (lines ~795 to 812) | Reads every named step of release-qualification.yml and requires `` `<step>` `` in the RELEASE.md "Qualification workflow" section. `Verify the release gate` appears in the updated list. A `run:` step without `name:` is an error; the new step is named. | Satisfied. The old five names are no longer required. |
| `scripts/test-check-docs.py` | Line 1124 asserted `Verify workspace`; changed to `Verify the release gate`. A grep of the repo for the five old step names finds only `scripts/test-check-release.py:303`, a mutation fixture that edits the new text. | Updated. |
| `scripts/test-check-release.py` | Rewritten with 56 tests and fixtures per rule. Line 315 uses `msrv` as a mutation target (renames the job back), not as a live reference. | New. |
| Docs naming `msrv` | Grep over README, CHANGELOG, docs, CONTRIBUTING, crate READMEs and scripts finds only prose uses of "MSRV 1.98" (README:281, CHANGELOG:90, IMPLEMENTATION.md:154, QUALIFICATION.md:260) and the `verify.yml` comment. None refer to a job id `msrv`. `CONTRIBUTING.md` has no reference to `msrv` or `release-gate`. | Clean. |
| Branch protection / ruleset | Repo ruleset 24943594 (`main`, active) has rules `deletion`, `non_fast_forward`, `required_linear_history`, `required_signatures`, `pull_request`. No `required_status_checks`, and `branches/main/protection` returns 404. No reference to a job named `msrv` anywhere. Renaming breaks nothing. | Clean. |
| Required-check consequence | Because no status checks are required, a red `release-gate` job does not block merge. RELEASE.md now says so. #3037418's record already states the same of earlier jobs ("no status check is required by the `main` ruleset"). | Known and documented; owner decision. |
| Local contributors | `make lint` gains `--locked`. `Cargo.lock` is committed and every other cargo call in CI is `--locked`, so a stale lockfile now fails `make lint` locally. | Low risk. |
| Other callers of `make lint` | `make check`, hence `native` (nightly), `release-qualification` and the new job. | Unchanged semantics apart from `--locked`. |

## 5. Blast radius

**If `release-gate` fails.** On a PR it shows red and, per the ruleset, does not block merge. At a tag, release qualification fails exactly as it did for `float_cmp`. `publish.yml`'s `gate` job exhausts its 200 x 30 s polling bound only if no run exists; if all runs complete as failures it exits 1. That is the behavior that saved 0.1.0 and is unchanged. One new failure mode: since both lanes now run the identical `make release-gate`, a gate that is flaky on the PR lane is flaky at the tag. The old split meant a flaky PR lane could not hold up a tag. This is a consequence of "identity" and cannot be avoided. The probe at least shows it fails for the intended reason.

**If check-release.py is wrong.** `make hygiene`/`make check`/`make package` and `native` jobs would fail on every PR until fixed, since `check-release.py` runs inside both. The checker hard-codes: the job name `release-gate`, `env` equality, the exact install lines, and the Makefile recipe lines. Renaming the job or adding a sixth gate needs a coordinated edit of the Makefile, the checker and its fixtures. That is intended (it is the point of the rule), but a future maintainer who edits only the Makefile gets a clear error naming the rule.

**PR-time cost.** Hosted timings from `gh run view`:

- Run 38382758016 on `fdd10ff`: `release-gate` 10:21:10 to 10:34:26 UTC, 13 min 16 s. Run 38376755652 earlier: 10 min 34 s. The `fuzz` job took about 41 min.
- Wall time is unchanged, because all jobs start together and `fuzz` is the longest. The `release-gate` job is the second longest job (after `fuzz`), longer than `native` (about 7 to 10 min), `qualification` (about 11 min), and `windows-engines` (about 7.5 min).
- Runner minutes: roughly +11 to +13 Linux minutes per PR push, billed per job. The replaced `msrv` job was one test run (not timed in the evidence; I could not verify the old duration). It should be a few minutes less. Net increase is likely about 8 to 10 minutes per run, but this is an estimate.
- Overlap: the job re-runs `make check` (also run by `native` on nightly), `make corpus` (also the `corpus` job), `test-verify-published.py` (also in `native`), and `make package` (also in `qualification`). That duplication is the price of identity; the alternative was to run Clippy alone (the issue's literal ask), which would have left tag-only gates `corpus`, `package` and the two probes unexercised on 1.98.0.
- Any push to a PR re-runs this job. Whether `concurrency` cancellation is configured was not established; no `concurrency:` block appears in the displayed `verify.yml`, so superseded pushes continue to burn minutes. That is pre-existing for all jobs.
- Timeout: 90 minutes matches `release-qualification.yml` and `fuzz`/`qualification`; observed runtime is ~12 min, so large headroom.
- Cache: no cache is used in any job. Cold build on 1.98.0 plus a nightly install (two toolchain downloads) accounts for most of the time.

## 6. Residual risks for the reviewer

- **Step visibility.** Failures in the release lane no longer have per-gate steps in the Actions UI.
- **Nightly installed twice in the release lane** (`llvm-tools`), consistent with `51ed930`, and pinned equal to `rust-toolchain.toml` by the new check.
- **Not required.** The owner has to add the required check; until then the protection is by convention and review.
- **`windows-engines` Clippy step remains** and is not redundant, since it is the only Windows `cfg` lint.
