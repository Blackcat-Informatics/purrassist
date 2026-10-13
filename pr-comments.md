# PR #28 — comments and review threads

6 comment(s). **Every one needs a disposition** in
`review-debt.md`: addressed (with the commit hash) or declined (with the
reason posted in reply). Bot comments count.

## 1. paudley — 

# Plan — Issue #26: lint the whole workspace with the release toolchain on pull requests

Revision 3. This revision folds in the five lens reviews and their rechecks; dispositions are at the end. The enhancement audit's
TRANSFORMATION, *gate identity*, restructures the plan.

## Issue summary

Release qualification runs `make check` on the release toolchain, stable Rust 1.98.0. Its `make lint` is
`cargo clippy --workspace --all-targets -- -D warnings`. Pull-request CI never runs that combination:

- the native jobs run `make check` on the pinned nightly;
- `windows-engines` runs 1.98.0 Clippy on host and cli only;
- `msrv` runs only `cargo test --workspace --locked` on 1.98.0.

So the first `rust-v0.1.0` tag failed release qualification on `clippy::float_cmp` in a `purrassist-core` test
(fixed in #24). The issue asks for a pull-request job running
`cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.98.0, and accepts the change when a PR
adding an exact float `assert_eq!` in a core test fails a hosted pull-request job with `clippy::float_cmp`.

## Baseline defaults

There is no `.baseline` file. There is also no `CONSTITUTION.md`, `CLAUDE.md`, `PRECOMMIT_GUIDANCE.md` or ADR directory, and no commit hooks are installed.

## Standing goals and constraints

From `.goals` and `AGENTS.md`:

- Rust-first, hard fails, maximal utility.
- Every tool is registered.
- SPDX headers on sources.
- Capability claims need executable evidence.
- `make check` is the local gate.
- Hooks are never bypassed, including through alternative commit paths.
- No agent attribution.

## Governing decisions

- **The defect class is gate divergence**, not one lint. `release-qualification.yml` runs five gates on the release toolchain: `make check`, `make corpus`, `scripts/test-fuzz-feedback.py`, `scripts/test-verify-published.py` and `make package`. Pull-request CI runs none of them on that toolchain. `make package`'s stable `cargo package` verify build of all six crates happens only at tag time.
- **The fix is gate identity by construction.**
  - A single `make release-gate` target holds those five gates.
  - The release workflow runs `make release-gate` as its one verification step, after its tag/version check.
  - A pull-request job named `release-gate` runs the same target with the same two toolchains.
  - Neither side can drift from the other's step list.
- **Toolchain pins have one source each.**
  - The stable pin is checked against every workflow occurrence (`env.RUSTUP_TOOLCHAIN` and every `rustup toolchain install` line, in `verify.yml`, `release-qualification.yml` and `publish.yml`). Its `major.minor` is checked against the workspace `rust-version` (`"1.98"`).
  - The nightly pin is checked against `rust-toolchain.toml`'s `channel`.
  - Both checks live in `scripts/check-release.py`, the existing workflow validator, which runs in `make hygiene` and `make package`. Its `publish.yml` closed-shape rules are not applied to `verify.yml`.
- **Reproducible local command:** `RUSTUP_TOOLCHAIN=1.98.0 make release-gate`. It is documented in QUALIFICATION.md's command list. The local `rust-validate` wrapper is only a convenience used in this session, not repository evidence.

## Prior art and recurrence (`prior-art-assessment.md`)

**Prior attempts:** none. There are no commits, branches or PRs citing #26.

**Recurrence of stable-only lint breaks:** 3 counted, plus 1 UNKNOWN.

| Event | What broke | Evidence |
|---|---|---|
| Release | `float_cmp` | Commit `598d059` and the hosted release-qualification run 38351629747, recorded in the issue #1 comment https://github.com/Blackcat-Informatics/purrassist/issues/1#issuecomment-6139102863 (the PR #24 squash notes) |
| CI | Stable-only lint | `de8e8fd` |
| CI | `too_long_first_doc_paragraph` | `2864e75`, fixed in `76d5e73` |
| UNKNOWN | Clippy break, lint names not recorded | `0b7c7de`; not counted |

**Why the earlier fixes did not hold:** each fixed code; none aligned the pull-request gate with the release gate.

## NEEDS-JUDGMENT dispositions

### `issue-analysis.md`

| Item | Decision and evidence |
|---|---|
| `--locked` in the issue's command but not in `make lint` | `make lint` gains `--locked`. The issue's exact command then runs inside `make check` → `make release-gate` in both lanes. `Cargo.lock` is committed, and every other CI cargo invocation already uses `--locked`. |
| Which job | The issue's example, the `msrv` job, is renamed `release-gate`. Its `cargo test --workspace --locked` is subsumed by `make check`'s tests on the same toolchain. `verify.yml` runs on `pull_request`, so this is a pull-request job. |
| Required check? | Out of scope. It's an owner ruleset setting, and the `main` ruleset has no required status checks. Recorded in the PR as an owner option. |
| Acceptance demonstration | A real hosted pull-request run (Task 2): a probe draft PR from the Task 1 head that restores the pre-#24 runtime comparison in `crates/core/src/tool_call/from_node.rs` (`assert_eq!(… as f32, 1.0, …)`). That assertion is true at runtime, so only the 1.98.0 Clippy gate can fail it. |

### `prior-art-assessment.md`

| Item | Decision and evidence |
|---|---|
| 1. Where the workspace Clippy run goes | Inside the `release-gate` job via `make check`. The `windows-engines` host/cli Clippy step stays: it's the only Clippy run on Windows `cfg` code (`crates/host/src/files/windows.rs`). |
| 2. Which form of the lint command | `make lint` with `--locked`, as above. |
| 3. Which toolchain | The stable release pin `1.98.0`, single-sourced as above. |
| 4. Event `0b7c7de` | Not counted (UNKNOWN). |
| 5. Evidence for the release break | Sufficient: commit `598d059` plus the hosted run id recorded on issue #1. |
| 6. A lasting fixture for the acceptance | The lasting guard is structural: the `check-release.py` rules and their tests. A deliberately failing lint can't live in the tree. The hosted probe (Task 2) is the one-time executable acceptance, recorded on the issue. |
| 7. Licence | The repository is `MIT OR Apache-2.0 OR MulanPSL-2.0` (Cargo.toml). No port source is involved. |

## Completeness contract

Ledger ids R1–R4; plan additions P1–P3.

| Id | Requirement | Task | Executable acceptance |
|---|---|---|---|
| R1 | `cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.98.0 in a pull-request job | 1 | On the draft PR opened at the end of Task 1 (head SHA recorded), the hosted `release-gate` job runs on `1.98.0`. Its log shows `cargo clippy --workspace --all-targets --locked -- -D warnings` running and passing, and the job concludes `success`. Locally, `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` exits 0. |
| R2 | Use the existing `msrv` job | 1 | `verify.yml` no longer has an `msrv` job that runs only tests. That job is the `release-gate` job, with `RUSTUP_TOOLCHAIN: 1.98.0`, running `make release-gate`. This is checked by the `check-release.py` rule in P1. |
| R3 | A stable-only lint fails on the PR that introduces it | 2 | The probe PR's hosted `release-gate` job concludes `failure` with `clippy::float_cmp` in its log at the probe line. The `native` jobs (nightly) still pass, showing the failure came from the release-toolchain gate. |
| R4 | Exact float `assert_eq!` in a core test fails a hosted PR job with `float_cmp` | 2 | As R3. The run URL and log excerpt are recorded in `validation.md` and on issue #26. If `verify.yml` or `Makefile` change after Task 2, the probe is rerun on the final head. |
| P1 | Gate identity: release and PR run the same steps | 1 | `scripts/check-release.py` asserts all of the following: the `release-qualification.yml` `package` job's named steps are exactly `Install release toolchain`, `Install pinned coverage-feedback toolchain`, `Verify release set and version` and `Verify the release gate`, plus the checkout step. Only the last runs `make`, and it runs `make release-gate`, so no release-only step can be added outside the shared gate without failing this check; the `verify.yml` `release-gate` job runs `make release-gate`; both install the same stable pin with `clippy,rustfmt` and the same nightly pin with `llvm-tools`; the two jobs' `env` blocks are identical; the Makefile `release-gate` target has no prerequisites and its recipe lines are exactly the five listed in Task 1, in order, so every gate including `package` is present; and the Makefile `lint` recipe is exactly `cargo clippy --workspace --all-targets --locked -- -D warnings`. The step list is parsed with the indentation-based steps reader already in `check-docs.py` (`workflow_steps`), imported rather than duplicated. `scripts/test-check-release.py` has a failing fixture per assertion, each naming its error. |
| P2 | Toolchain pins single-sourced | 1 | `check-release.py` extracts only the toolchain name from every `RUSTUP_TOOLCHAIN:` value and every `rustup toolchain install` line, ignoring profile and components so `publish.yml`'s component-less install still matches. A pin starting with `nightly` is a nightly occurrence, and any other pin is a stable occurrence. It asserts every stable pin occurrence in all three workflows is identical, with `major.minor` equal to the Cargo `rust-version`, and every nightly pin occurrence equals `rust-toolchain.toml`. Failing fixtures: a bumped install line only; a bumped env only; a `publish.yml`-only bump; a `rust-version` mismatch; a nightly drift in one job. |
| P3 | Docs | 1 | These sentences change, checked by grep: `docs/QUALIFICATION.md:252-253` "stable MSRV workspace tests" is replaced by a sentence naming the `release-gate` job and `make release-gate` on the release toolchain, stating that this lane is also the MSRV 1.98 build-and-test evidence. The old phrase is gone. QUALIFICATION.md's command list gains `RUSTUP_TOOLCHAIN=1.98.0 make release-gate`. `docs/RELEASE.md`'s qualification-workflow step list and its `check-release.py` paragraph (lines 18-23) describe the new step and rules. `check-docs.py`'s qualification-steps rule passes on the new step names. |

## Enhancements considered and declined

- **Make the job a required status check:** declined. It's an owner ruleset setting. The cost: a red `release-gate` job does not block a merge, so it relies on review discipline until the owner makes it required.
- **Permanent out-of-tree failing-lint fixture (enhancement item 4):** declined. The hosted probe (Task 2) plus Task 3's rerun when gate files change cover the one-time acceptance, and the structural P1/P2 rules guard against drift. The residual: the claim that a stable-only lint fails the PR is re-demonstrated only when gate files change.
- **Remove `windows-engines`' Clippy step:** declined. It's the only Windows-`cfg` Clippy run.

## Task 1: One release gate, run on pull requests and in release qualification

**Files:**

- `Makefile`:
  - a `release-gate` target with no prerequisites, whose recipe lines run in this order: `$(MAKE) check`, `$(MAKE) corpus`, `python3 scripts/test-fuzz-feedback.py`, `python3 scripts/test-verify-published.py`, `$(MAKE) package`;
  - `lint` gains `--locked`;
  - the `.PHONY` list.
- `.github/workflows/release-qualification.yml`: steps 4–8 collapse into one named step, `Verify the release gate`, running `make release-gate`. The install and version-check steps stay.
- `.github/workflows/verify.yml`:
  - the `msrv` job becomes `release-gate`;
  - it installs `1.98.0` with `clippy,rustfmt` and the nightly with `llvm-tools`, the same as release qualification;
  - it runs `make release-gate`, with `timeout-minutes: 90`;
  - it drops `RUSTFLAGS: -D warnings`, so its environment equals the release job's. Every gated cargo step already denies warnings: Clippy with `-D warnings` and the workspace lints. This removes the last lane divergence.
- `scripts/check-release.py` and `scripts/test-check-release.py`: P1 and P2, with fixtures.
- `scripts/test-check-docs.py`: the real-workflow assertion that names `Verify workspace` (around line 1124) becomes `Verify the release gate`.
- The `release-gate` recipe lines run in that documented order, with no prerequisites, so `make -j` can't reorder them.
- Docs: P3.

**Focused checks:**

- `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` exits 0, run locally through the session's `rust-validate 1.98.0` wrapper.
- Plain `make check` (nightly) exits 0.
- `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` and `scripts/test-check-release.py` pass.
- `make hygiene` exits 0.

**Then:**

1. Task review.
2. Commit (a normal local commit; no hooks are installed, and none is bypassed) and push.
3. Open the PR as a **draft**, record its head SHA, and wait for the hosted `release-gate` job to succeed on that SHA (R1).
4. Issue update.

## Task 2: Hosted acceptance probe

1. On a branch `probe/26-float-cmp` from the Task 1 head, restore the pre-#24 runtime comparison in `crates/core/src/tool_call/from_node.rs`.
2. Commit it normally, with no attribution, and with a message saying it is a deliberate failing probe for #26 that must not merge. Then push the branch.
3. Local supporting evidence: 1.98.0 Clippy reports `float_cmp` at that line.
4. Open a **draft** PR titled `probe(#26): deliberate float_cmp failure, do not merge`, with no closing keyword.
5. Required: its hosted `release-gate` job fails with `clippy::float_cmp` at the probe line, while `native` passes.
6. Record the run URL and log excerpt in `validation.md` and on issue #26.
7. Close the probe PR unmerged and delete only its branch.
8. Task review: `tasks/T2-review.md` checks the evidence.

## Task 3: PR ready and handoff

1. Completeness review against this plan.
2. Artifact inventory.
3. Confirm the hosted `release-gate` job is green on the final head, and rerun the probe if `verify.yml` or `Makefile` changed after Task 2.
4. Mark the PR ready (`Closes #26`) and post the plan.
5. Least-confident note.
6. Report the Stage 2/3 gates.

## Review dispositions (revision 2)

**Completeness:**

- B1: the draft PR is opened in Task 1, and its hosted pass is recorded.
- B2: the contract is keyed to the ledger's R1–R4, with the plan's additions as P1–P3.
- B3: both prior-art items are resolved.
- Advisories adopted: the grep-checkable docs check, the major.minor comparison, the runtime probe, and Task 2's commit, push and review.

**Constraints:**

- A1: plain nightly `make check` is added to the focused checks.
- A2: the probe is a normal commit, with no attribution, no closing keyword, and a stated deliberate failure.
- A3: the RELEASE.md `check-release.py` paragraph is updated, and the plan says `check-release.py` validates `publish.yml` (and now the pins).

**Design:**

- A1 and A2: the major.minor comparison, and checking every pin occurrence without applying `publish.yml`'s closed-shape rules to `verify.yml`.
- A3: the runtime probe.
- A4: the reproducible `RUSTUP_TOOLCHAIN=… make` command, and the hosted pass moved after the push.

**Enhancement:**

- Item 1 (TRANSFORMATION): `make release-gate` gives gate identity, adopted.
- Item 2: all pins, including `publish.yml` and the nightly, adopted.
- Item 3: the reproducible command and QUALIFICATION.md's command list, adopted.
- Item 5: major.minor, adopted.
- Items 4, 6, 7:
  - The probe is the executable gate-sensitivity check.
  - The wording says "release-toolchain gate", not just "stable-only".
  - `check-docs.py`'s existing workflow-step parser is reused for the new single step.

**Final review (PASS) advisories:**

- The reused step parser is named.
- The stable-versus-nightly classification is stated.
- The item 4 residual and the cost of the required-check decline are recorded.
- Gate-identity scope: adopted as the TRANSFORMATION under the fold-in rules, and stated in the PR for the owner to object to.

**Adversary, pass 3 (PASS):**

- The release step list is pinned exactly, so no release-only step can be added outside the gate.
- The cross-reference is fixed.
- The header is bumped.

**Adversary, pass 2:**

- N1 (a): P1 now names all five recipe lines, including `package`.
- N1 (b): P1 asserts that exactly one step runs `make`.
- N1 (c): R2's acceptance now cites P1.

**Design, pass 2:**

- P2-1: `release-gate` has recipe lines only, in order.
- P2-2: the pin extractor ignores components.
- The superset note is moot, since `RUSTFLAGS` is dropped.

**Enhancement, pass 2:**

- Item 4: the lint-recipe assertion is adopted.
- Item 8: `RUSTFLAGS` is dropped from the PR job, and P1 asserts env equality.

**Constraints, pass 2:**

- B1: the `test-check-docs.py` assertion is added to Task 1.
- B2: the MSRV-evidence sentence is required.
- The `-j` note is handled with sequential recipe lines.

**Completeness, pass 2:**

- N1: P1 now says exactly one step runs `make`.
- N2: the probe push is explicit.
- N3: the run id is traced to its issue #1 comment.

**Adversary:**

- F1: the ledger-keyed ids.
- F2: the draft PR and head SHA in Task 1, and a probe rerun if gate files change.
- F3: the exact-pin and major.minor rules with named fixture errors.
- F4: the grep-checked doc sentences.
- Note: the runtime-true probe.

## 2. paudley — 

## Handoff for #26: PR #28 is ready for review

**What changed:**

- `make release-gate` (check, corpus, the coverage-feedback test, the published-verifier test, package) is now the one release gate.
- Release qualification runs it as its single verification step.
- The pull-request job `release-gate`, formerly `msrv`, runs the same target with the same toolchains and environment.
- `make lint` is exactly `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `check-release.py` keeps the two lanes identical and single-sources every toolchain pin. It has 56 tests, with failing fixtures for each rule.

**Evidence:**

- **R1, the gate passes on a pull request:** [run 38376755652](https://github.com/Blackcat-Informatics/purrassist/actions/runs/38376755652) on `2212844`, and [run 38382758016](https://github.com/Blackcat-Informatics/purrassist/actions/runs/38382758016) on the final head `fdd10ff`. All eight jobs pass. The `release-gate` job runs on `RUSTUP_TOOLCHAIN: 1.98.0` and executes `cargo clippy --workspace --all-targets --locked -- -D warnings` inside `make check`, then corpus, both scripts and package.
- **R3/R4, acceptance:** probe PR #29 restored the pre-#24 exact float `assert_eq!` in a `purrassist-core` test. In [run 38377011106](https://github.com/Blackcat-Informatics/purrassist/actions/runs/38377011106) only `release-gate` failed:

  ```
  error: strict comparison of `f32` or `f64`
     --> crates/core/src/tool_call/from_node.rs:336:9
      = note: `-D clippy::float-cmp` implied by `-D warnings`
  error: could not compile `purrassist-core` (lib test) due to 1 previous error
  make: *** [Makefile:56: release-gate] Error 2
  ```

  The nightly `native` jobs passed. The probe PR was closed unmerged and its branch deleted.

**What I'm least confident about:**

- **Scope:** this is broader than the issue's literal ask. Pull requests now run the whole release gate, not just workspace Clippy, because `make corpus`, the published-verifier test and the stable `cargo package` build had the same release-only blind spot. Object on the PR if you want only the narrow change.
- **Cost:** the PR job now takes about 10–11 minutes, up from a test-only run.

**What you may not know:**

- **Not a required check:** `release-gate` isn't a required status check (`main`'s ruleset has none), so a red run doesn't block a merge. Making it required is your setting.
- **Release qualification changed shape:** it now shows one `Verify the release gate` step instead of five. A failing gate shows up as `make: *** [Makefile:N: <target>]` in that step's log.
- **Local reproduction:** `RUSTUP_TOOLCHAIN=1.98.0 make release-gate`. If your Rust setup puts 1.98 binaries ahead of the rustup proxies on `PATH`, the coverage-feedback test can't switch to its pinned nightly. Rustup proxies, as on hosted runners, handle it.

**Not yet run:** the Stage 2 gap analysis and the Stage 3 merge gates.

## 3. paudley — 

# Stage 2 remediation plan — issue #26, PR #28 (full path)

Revision 3. This revision folds in `reviews/remediation-plan-review.md` passes 1 and 2 (B1–B8, N1, A9–A12).

**Sources:**

- `gap-analysis.md` (BLOCKED on H1; M1–M3, L1–L7).
- `completion-audit.md` (PASS; A1–A4).
- `historical-fit.md`.

Every finding is fixed, except L7, which is left as is with a stated reason. There are no user scope decisions.

**Design principle (B1):** the gate jobs are described by *allowlists*, not ban lists. Anything not explicitly
allowed in a gate job, or at a workflow's top level, fails. This closes every known and future modifier
bypass (`continue-on-error`, `if:`, `defaults:`, `shell:`, step `env:`, workflow `env:`, extra steps) in one rule.

## Gaps and fixes

### G1 — H1, M2, B1, B2, B3: allowlisted gate jobs pinned to the release toolchain

`scripts/check-release.py` `validate_release_gate` reads YAML keys structurally (indentation within each workflow, the same reader family as `workflow_steps`). It asserts the following.

**Workflow level, both files:**

- Top-level keys ⊆ {`name`, `on`, `permissions`, `jobs`, `concurrency`}.
- No workflow-level `env:` or `defaults:`.

**`verify.yml` trigger (B3):**

- `on` contains `pull_request:` with nothing after the colon except an optional comment, and no indented keys under it. The flow forms `pull_request: {…}` and `on: [push, pull_request]` are rejected as unsupported shapes (A9).

**Gate job keys:** the `release-qualification.yml` `package` job and the `verify.yml` `release-gate` job each have keys exactly {`runs-on`, `timeout-minutes`, `env`, `steps`}. That means no `if`, `continue-on-error`, `defaults`, `strategy`, `container` or `services`.

**Gate job env (H1):**

- Exactly the single key `RUSTUP_TOOLCHAIN`.
- Its value, after stripping quotes and trailing comments, is a stable pin (not nightly) equal to that job's single stable install pin.
- Together with P2, that pin's major.minor equals Cargo `rust-version`.

**Gate step lists (B2):**

- `verify.yml` `release-gate`, exactly:
  1. the checkout step (`uses:` the pinned checkout, `with: persist-credentials: false`);
  2. a stable install;
  3. a nightly install;
  4. a step whose only command is `make release-gate`.
- `release-qualification.yml` `package`, exactly:
  1. the checkout;
  2. `Install release toolchain`;
  3. `Install pinned coverage-feedback toolchain`;
  4. `Verify release set and version`;
  5. `Verify the release gate`.
- Step keys ⊆ {`name`, `uses`, `with`, `run`}, plus `shell: bash` on `Verify release set and version` only.
- **N1:** in both gate jobs, `uses:` appears only on the checkout step and must match `actions/checkout@<40-hex SHA>`. Its `with:` must be exactly `{persist-credentials: false}`. No other step may have `uses:` or `with:`.
- No step `env:`, `if:` or `continue-on-error:`.
- No `run:` text containing `GITHUB_ENV` or `GITHUB_PATH`.
- The verify gate step may be named or unnamed (L1).
- Install comparison is per toolchain kind (stable/nightly) and order-independent (L1).

**Fixtures (each must fail with a named error in `test-check-release.py`):**

- **Env:** env removed from both jobs; nightly in both jobs; env differs from the job's stable install; a job-level `RUSTFLAGS` added to both jobs; a step-level `env:` (`CARGO_ENCODED_RUSTFLAGS`).
- **Keys and modifiers:** a workflow-level `env:`; a job-level `defaults:`; `continue-on-error: true` on the job and on a step; `if: false` on the gate step; `shell:` on the gate step.
- **Steps:** an extra step in the verify gate job (writes `.cargo/config.toml`); an `echo … >> "$GITHUB_ENV"` step.
- **Trigger:** `pull_request:` removed from `verify.yml`; a `paths:` filter added; the flow form `pull_request: {paths: [x]}`.
- **Checkout (N1):** `ref: main` added to the verify checkout's `with:`; a different action (`actions/checkout@v4` by tag, or another action) in either gate job.

These fixtures must pass: a named verify gate step; a quoted env value; a trailing comment on the env value; swapped install order.

### G2 — M1: Clippy and tests reachable from the gate

P1 asserts that the `Makefile` `check` target's prerequisites include both `lint` and `test`.

**Fixtures:** `lint` dropped from `check:`, and `test` dropped from `check:`, each fail.

### G3 — A1, B5: corpus suites linted on the release toolchain

**Fix:** `make lint` has exactly two recipe lines:

1. the issue's exact command, `cargo clippy --workspace --all-targets --locked -- -D warnings`;
2. `cargo clippy -p purrassist-core --locked $(foreach suite,$(CORPUS_SUITES),--test $(suite)) -- -D warnings`, which reuses the Makefile's existing `CORPUS_SUITES` list (also used by `make corpus`).

**Rules in P1:**

- the `lint` recipe is exactly those two lines;
- the Makefile has exactly one `CORPUS_SUITES :=` definition and no `+=`, `override` or `?=` assignment to it, so the checked text equals what Make expands (A12);
- `CORPUS_SUITES` equals the set of `test = false` `[[test]]` targets in the workspace members' `Cargo.toml` (parsed with `tomllib`). `cargo clippy --test <name>` does lint `test = false` targets: verified, since the `correction_evaluation` artifact appears with `profile.test=true`.

**Fixtures:** each fails:

- the second line weakened by dropping `-- -D warnings`;
- `|| true` appended;
- a leading `-`;
- a suite removed from `CORPUS_SUITES`;
- a new `test = false` target not in `CORPUS_SUITES`.

**Local evidence:**

- `make lint` on 1.98.0 exits 0.
- A temporary exact float `assert_eq!` in a corpus suite is reported by line 2 on 1.98.0. It is recorded and then reverted.

### G4 — M3: PR #28 body

Rewrite the PR body to cover:

- the final state (ready);
- the current test counts;
- links to the R1 run, both probe runs (the original, plus the rerun from G7) and the final-head run;
- the scope note;
- that `make check` now also lints the corpus suites;
- the not-required-check note.

**Check:** `gh pr view 28 --json body` has no "Draft", and it links every run.

### G5 — L2, L3, B4: pin extraction

**Extraction:**

- After the install verb (only when preceded by the `rustup` token: `rustup` + `toolchain` + `install`, or `rustup` + `install`; `apt-get install`, `scripts/install-qualification-tool.py` and other installers are never read as pins, A13), value-taking flags consume their next argument (`--profile X`, `--component X`, `-c X`, `--target X`, `-t X`). `--opt=value` forms and boolean flags consume nothing. **Every** remaining positional argument is a pin (A10).
- `\` line continuations are joined.
- Extraction also reads:
  - `rustup default X`;
  - `rustup override set X`;
  - `cargo +X`;
  - inline `RUSTUP_TOOLCHAIN=X` assignments in `run:` text;
  - env keys.
- It covers every file in `.github/workflows/`, not a fixed list.

**Validation:**

- A pin must match `^(nightly-\d{4}-\d{2}-\d{2}|\d+\.\d+\.\d+)$`.
- A `${{ … }}` or `$VAR` pin is an explicit error: "toolchain pins must be literal".

**Fixtures:**

- These pass: flags before the name (`--profile minimal 1.98.0`); a line continuation; the unmodified real `verify.yml`, including its apt-get and install-qualification-tool lines (A13).
- These fail:
  - a `${{ matrix.rust }}` pin;
  - a `"$TC"` pin;
  - `cargo +1.97.0`;
  - an inline `RUSTUP_TOOLCHAIN=1.97.0`;
  - a pin in a new workflow file;
  - a malformed pin (`stable`).

### G6 — L4, B6, B7: docs stay true when pins and checks change

**`check-docs.py` rule:** every `RUSTUP_TOOLCHAIN=<x>` occurrence and every patch-form `Rust <maj.min.patch>` occurrence in `README.md`, `docs/RELEASE.md` and `docs/QUALIFICATION.md` must equal the stable pin.

- The `## 0.1.0 publication record` section of RELEASE.md is excluded as a dated historical record. Its "stable Rust 1.98 Clippy" is not patch-form in any case.
- The pin comes from `release-qualification.yml` through `check-release.py`'s extractor, loaded with the existing `load_module` helper.
- **Fixtures:** a workflow-pin bump without the docs fails; the real files pass; the publication-record line is ignored.

**B7 / A11 doc updates:**

- `docs/RELEASE.md:29-30` says "every workflow" rather than "three workflows", and the `check-release.py` paragraph mentions that `check` must depend on `lint` and `test`.
- The `verify.yml:36` comment no longer hard-codes "stable 1.98.0"; it says "the release toolchain pinned in release-qualification.yml".

- `docs/RELEASE.md`: the `check-release.py` paragraph (around line 28) describes the allowlisted gate jobs, the pinned env, the PR trigger and the corpus-suite rule. The qualification-workflow section (around lines 139–140) says `make lint` runs workspace Clippy plus the corpus suites.
- The `Makefile` comment at lines 28–30 describes the corpus list's two users.
- The `crates/core/Cargo.toml:66` comment notes that `test = false` suites are linted by `make lint` through `CORPUS_SUITES`.

### G7 — the probe rerun (Stage 1 rule: `verify.yml` and `Makefile` change)

This runs after G1–G6 land:

1. Open a fresh draft PR from `probe/26-float-cmp-2`, based on the final head, with the same restored pre-#24 assertion.
2. Required: only `release-gate` fails, with `clippy::float_cmp` at the probe line, while the nightly jobs pass.
3. Close it unmerged and delete its branch.
4. Record the run in `validation.md` and on the issue.

### G8 — L5, L6, L7, B8

**L5:** `check-release.py`'s module docstring and the two stale function docstrings describe:

- the release set;
- the publish workflow;
- gate identity and its allowlists;
- toolchain pins;
- the corpus-suite rule.

**L6:** `README.md:290` and the `Makefile:83` comment stop calling `fuzz-hosted` a "release fuzz gate", and say "the CI and pre-tag fuzz gate" instead. The README lists `make release-gate`.

**Check (B8):** `grep -rn 'release fuzz gate' README.md Makefile docs/` is empty, and `grep -n 'make release-gate' README.md` finds the new README line.

**L7 (left as is):** `check-release.py` and `check-docs.py` each define a small module loader. Making one import the other's loader would create a circular import, since each already loads the other. The two are four-line `importlib` helpers with the same behaviour, so the duplication is left with this reason recorded.

### Not a branch gap

A4: the required status check is an owner setting, still disclosed.

## Commit boundaries and publication

1. **G1 + G2:** allowlisted gate identity and env pin, with fixtures.
2. **G3:** corpus suites linted, with rules and fixtures.
3. **G5 + G6 + G8:** pin extraction, doc-pin rule, docs, docstrings and wording.
4. **G7:** probe rerun (no commit on the PR branch).
5. **G4:** PR body (no commit).

Each commit is pushed and summarized on the issue and the PR.

**Exit:**

- Record the before and after `make check` wall time in `validation.md`. The before figure is 348 s on nightly, from Task 1.

- a hosted run on the final head with every job green;
- `make release-gate` once on 1.98.0;
- nightly `make check` once;
- a fresh completion auditor.

## 4. paudley — 

## Stage 2 fix 1 (`5e5159e`): G1 + G2, allowlisted gate jobs

**Gap H1 (HIGH):** the gate-identity check compared the two gate jobs only with each other. Removing `RUSTUP_TOOLCHAIN` from both jobs, or setting both to the nightly, passed every rule and silently dropped the 1.98.0 lint. Job and step modifiers could also bypass the gate (M2).

**Fix:** both gate jobs are now described by allowlists:

- workflow top-level keys;
- a bare `pull_request:` trigger;
- job keys exactly `runs-on`, `timeout-minutes`, `env` and `steps`;
- env exactly `RUSTUP_TOOLCHAIN`, stable and equal to the job's stable install;
- closed step lists, with an allowlist of step keys;
- a SHA-pinned checkout with only `persist-credentials: false`;
- no `GITHUB_ENV` or `GITHUB_PATH` writes.

`check` must keep `lint` and `test` (M1).

**Checks:**

- `test-check-release.py`: 84 tests OK, up from 56. Each new rule has a fixture that must fail, and the legitimate edits have fixtures that must pass.
- `make hygiene` exits 0.
- An independent review passed. Its adversarial probes, including H1's two bypasses, `ref: main` on the checkout, `if: ${{ false }}` and a top-level `env:`, all now fail.

## 5. paudley — 

## Stage 2 fixes 2 and 3

### `0c4c25e`: G3, corpus suites linted on the release toolchain

**Gap A1:** `cargo clippy --workspace --all-targets` skips the eight `test = false` corpus suites, so a stable-only lint in one of them failed no job.

**Fix:** `make lint` adds a second line that lints those suites by name, reusing `CORPUS_SUITES`. `check-release.py` requires both lines verbatim and a single plain `CORPUS_SUITES` definition equal to the `test = false` targets declared in `Cargo.toml`.

**Evidence on Rust 1.98.0:** an exact float `assert_eq!` in `correction_evaluation` passes the workspace line and fails the second line with `clippy::float_cmp` (recorded, then reverted).

### `fd898ed`: G5, G6 and G8, pin extraction, the docs-pin rule and wording

**Pin extraction:**

- Pins are read only after the `rustup` token.
- Value flags consume their argument, and every remaining positional is a pin. Continuations are joined.
- `rustup default` and `override`, `cargo +<pin>`, inline `RUSTUP_TOOLCHAIN=` and env keys are read in every workflow file.
- Non-literal (`${{ }}`, `$VAR`) and malformed pins are rejected.

**Docs-pin rule:** `check-docs.py`'s `release-toolchain` rule holds the docs' release-toolchain mentions to the pin, ignoring the dated publication record.

**Docs and wording:**

- RELEASE.md describes everything `check-release.py` now enforces.
- The README lists `make release-gate`.
- "Release fuzz gate" is now "CI and pre-tag fuzz gate".

**Checks:**

- `test-check-release.py`: 110 tests OK. `test-check-docs.py`: 260 OK.
- `make hygiene` passes.
- Independent reviews of each unit passed, with adversarial probes.

## 6. paudley — 

## Stage 2 complete for PR #28: ready for Stage 3

**Path:** full. The change restructures `release-qualification.yml`, which `publish.yml` gates on.

### Gaps resolved

**Gap analysis (initially BLOCKED):**

- **H1 (HIGH):** the drift guard didn't pin the gate jobs to the release toolchain. Removing `RUSTUP_TOOLCHAIN` from both jobs, or setting both to the nightly, would have silently reintroduced #26.
- **M1–M3:** Clippy's reachability from the gate, step and job modifier bypasses, and the stale PR body.
- **L1–L6:** robustness, pin forms, doc drift, docstrings and wording.

**Completion audit:** **A1** found that the eight `test = false` corpus suites were never linted by `--all-targets` on any lane.

**Fixes:**

| Commit | What it fixes |
|---|---|
| `5e5159e` | Allowlisted gate jobs: workflow and job keys, an env of exactly `RUSTUP_TOOLCHAIN` pinned to the stable release toolchain, closed step lists, a SHA-pinned checkout with only `persist-credentials: false`, a bare `pull_request:` trigger, and no `if`, `continue-on-error`, step `env`, `defaults`, `shell` or `GITHUB_ENV`/`GITHUB_PATH` writes. `check` must keep `lint` and `test`. |
| `0c4c25e` | A second `make lint` line lints the corpus suites by name, from `CORPUS_SUITES`, which is checked against `Cargo.toml`'s `test = false` targets. |
| `fd898ed` | Pins are read in every rustup form in every workflow file; non-literal and malformed pins are rejected. `check-docs.py` holds the docs to the pin, excluding the dated publication record. Docs, docstrings and wording updated. |
| `a3bbed0`, `8c7ab3b` | From the exit audit: the Makefile may not set or unexport `RUSTUP_TOOLCHAIN` or declare `.IGNORE`, and only `MAKEFLAGS += --no-print-directory` is allowed. |

L7 (a duplicated four-line module loader) is left as is, because sharing it would create a circular import. The reason is recorded.

### Validation

**Local:**

- Rust 1.98.0 `make release-gate`: exit 0, six archives verified.
- Nightly `make check`: exit 0, 110 test groups.
- `test-check-release.py`: 115 tests. `test-check-docs.py`: 260.
- A temporary exact float assertion in a corpus suite is caught by the new lint line on 1.98.0.

**Hosted:**

- **Final head `8c7ab3b`:** all eight jobs pass ([run 38438995951](https://github.com/Blackcat-Informatics/purrassist/actions/runs/38438995951)).
- **Gate head `fd898ed`:** all eight pass ([run 38430626503](https://github.com/Blackcat-Informatics/purrassist/actions/runs/38430626503)).
- **Acceptance probe rerun on `fd898ed`, PR #30:** it restored the pre-#24 exact float `assert_eq!`. **Only `release-gate` failed**, with `clippy::float_cmp` at `from_node.rs:336:9` ([run 38430682936](https://github.com/Blackcat-Informatics/purrassist/actions/runs/38430682936)). Closed unmerged.
- **Earlier probe, PR #29:** [run 38377011106](https://github.com/Blackcat-Informatics/purrassist/actions/runs/38377011106), the same result.
- The commits after `fd898ed` touch only the checker script, its tests and RELEASE.md, so both probe results still apply.

**Completion audits:** independent, both PASS. They demonstrated R1–R4 on hosted runs and probed every remediation adversarially on scratch copies.

**Recorded limit:** deliberately evasive Makefile spellings such as `MAKE := make -i` can still defeat any text-only check. All plausible direct spellings are rejected.

### Separate items

- **Hosted CI:** green on the final head.
- **Not run:** release qualification itself (it runs only on a tag or manual dispatch).
- **Owner setting:** `release-gate` is not a required status check.

**Stage 3 readiness:** ready.

