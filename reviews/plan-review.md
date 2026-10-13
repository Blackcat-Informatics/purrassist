# Final plan review (Stage 1): issue #26

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/26-lint-the-whole-workspace-with-the/.stage/lint-the-whole-workspace-with-the/plan.md` (revision 3)

Reports and dispositions examined:

- `reviews/plan-completeness.md` (pass 1 BLOCKED, pass 2 PASS): B1-B3, A1-A4, N1-N3
- `reviews/plan-constraints.md` (PASS, pass 2 PASS): A1-A3, B1-B2, the `-j` note
- `reviews/plan-design.md` (PASS, pass 2 PASS): A1-A4, P2-1, P2-2, the RUSTFLAGS fuzz-example note
- `reviews/plan-enhancement.md` (pass 1 BLOCKED, pass 2 PASS): items 1-8
- `reviews/plan-adversary.md` (passes 1-2 BLOCKED, pass 3 PASS): F1-F4, the runtime-false probe note, N1 (a)(b)(c), three pass-3 advisories
- The plan's "Review dispositions" section (plan lines 160-233)

Also read: `issue.md`, `issue-analysis.md`, `prior-art-assessment.md`, and in the worktree `verify.yml`, `release-qualification.yml`, `publish.yml:60-89`, `Makefile`, `Cargo.toml:10`, `rust-toolchain.toml`, `scripts/check-release.py`, `scripts/test-check-release.py:78-129`, `scripts/check-docs.py:738-812`, `scripts/test-check-docs.py:1121-1209`, `scripts/test-fuzz-feedback.py:21-37`, `docs/QUALIFICATION.md:239-256`, `docs/RELEASE.md:12-23,118-152`, `crates/core/src/tool_call/from_node.rs:322-343`.

## 1. Fixes are real

Every BLOCKING finding and every adopted enhancement has matching plan text. Checked one by one:

| Finding | Plan text that discharges it |
|---|---|
| Completeness B1 (R1 hosted run never collected) | Task 1 "Then" step 3 (line 137): draft PR, head SHA recorded, wait for hosted `release-gate` success. R1 row (line 94). Task 3 step 3 (line 155) reconfirms on the final head. |
| Completeness B2 / Adversary F1 (ledger id collision) | Line 90 "Ledger ids R1–R4; plan additions P1–P3"; R2 row (line 95) is the `msrv` job requirement. |
| Completeness B3 (prior-art NJ5/NJ6 undisposed) | Prior-art rows 5 and 6 (lines 84-85). N3's comment URL is at line 58. |
| Completeness A1 / Adversary F4 (docs row prose) | P3 (line 100): grep-checked phrase removal at `QUALIFICATION.md:252-253` (verified: that phrase is there today and nowhere else in docs), command-list entry, RELEASE.md 18-23 and step list, `release-qualification-steps` rule. |
| Completeness A2 / Design A1 / Enhancement 5 / Adversary F3 (`1.98` vs `1.98.0`) | Line 45 and P2 (line 99): `major.minor` against `rust-version`, with a mismatch fixture. `Cargo.toml:10` is `"1.98"`; all workflow pins are `1.98.0`. |
| Completeness A3 / Design A3 / Adversary note (probe shape) | Line 74 and Task 2 step 1 (line 142): restore the pre-#24 runtime comparison in `from_node.rs`. The current test at lines 336-340 compares `.to_bits()` of exactly that value, so the restored float form is true at runtime. |
| Completeness A4, N2 (Task 2 review/commit/push) | Task 2 steps 2 ("Then push the branch"), 6, 8 (`tasks/T2-review.md`). |
| Completeness N1 / Adversary N1(b) ("exactly one verification step") | P1 (line 98): named steps enumerated exactly, "Only the last runs `make`". The `Verify release set and version` step runs `python3 scripts/check-release.py`, not `make`, so this holds on the intended layout. |
| Adversary N1(a) (`package` missing from the target assertion) | P1: "recipe lines are exactly the five listed in Task 1, in order, so every gate including `package` is present"; Task 1 line 112 lists them verbatim. |
| Adversary N1(c) (R2 cites P2) | R2 row now cites P1. |
| Adversary pass-3 advisory 1 (non-`make` release-only step) | P1 now pins the named step list exactly, which closes that gap outright (any added step fails). |
| Constraints A1 (plain nightly `make check`) | Focused check line 129. |
| Constraints A2 (probe commit path, attribution, closing keyword) | Lines 33, 136, 143, 145. |
| Constraints A3 (RELEASE.md check-release paragraph) | P3 names `docs/RELEASE.md` lines 18-23; line 47 says `publish.yml` is what the closed-shape rules cover. |
| Constraints B1 (`test-check-docs.py:1124` pins `Verify workspace`) | Task 1 line 122. Verified: this is the only non-doc reference to the old step names outside the workflow itself. |
| Constraints B2 (MSRV evidence sentence) | P3: "stating that this lane is also the MSRV 1.98 build-and-test evidence". |
| Constraints `-j` note / Design P2-1 (gate order) | Line 112 (recipe lines, no prerequisites), line 123, P1 asserts the five lines in order. |
| Design A2 (both pin spellings, no closed-shape rules on `verify.yml`) | Lines 45-47; P2 fixtures "a bumped install line only; a bumped env only". |
| Design A4 / Enhancement 3 (`rust-validate` phantom; hosted check before push) | Line 48 demotes the wrapper; line 128 states the reproducible form; the hosted check sits after the push (Then step 3). |
| Design P2-2 (component regex must not reach `publish.yml`) | P2: "extracts only the toolchain name ... ignoring profile and components so `publish.yml`'s component-less install still matches". Verified `publish.yml:78` has no components. |
| Enhancement 1 (TRANSFORMATION, gate identity) | Governing decisions lines 38-43; Task 1 lines 111-120; P1. |
| Enhancement 2 (all pins, both toolchains, `publish.yml`) | Line 45-46; P2. Verified the occurrence set: `verify.yml` 4 stable + 7 nightly, `release-qualification.yml` 1+1 stable, 1 nightly, `publish.yml` 2 stable. |
| Enhancement 4 pass-2 advisory (lint recipe assertion) | P1 last clause; disposition line 212. |
| Enhancement 6 (do not over-claim stable-only) | R3 requires `native` to pass on the probe PR. |
| Enhancement 8 (RUSTFLAGS divergence) | Line 120 drops `RUSTFLAGS`; P1 asserts the two `env` blocks are identical. Verified the release job env is `RUSTUP_TOOLCHAIN: 1.98.0` only, and `Makefile:26,33,36` compose `-D warnings` themselves. |

No disposition says "addressed" without matching text.

## 2. Declines are justified

- Required status check: declined as an owner ruleset setting, recorded in the PR as an owner option (lines 73, 104). Technical reason present; the cost (a red job does not block merge) is implied rather than stated (see ADVISORY 4).
- `windows-engines` Clippy step: declined because it is the only Clippy run of Windows-`cfg` code (line 80). Reason present, no cost beyond a redundant host/cli pass on Windows.
- Enhancement item 4 (standing in-lane sensitivity probe): not adopted in its proposed form; the plan substitutes the one-time hosted probe plus the structural `lint`-recipe assertion. The enhancement reviewer accepted this. The residual (a later downgrade of `[workspace.lints.clippy]` would not fail anything) is not written down in the plan (see ADVISORY 3).
- No TRANSFORMATION item was declined.

## 3. Integration did not break the plan

Cross-checked each P1 assertion against Task 1 and the current files:

- Named steps "exactly `Install release toolchain`, `Install pinned coverage-feedback toolchain`, `Verify release set and version`, `Verify the release gate`, plus the checkout step" matches line 115 (steps 4-8 collapse; install and version-check steps stay) and `release-qualification.yml:24-41`.
- "Both install the same stable pin with `clippy,rustfmt` and the same nightly pin with `llvm-tools`" matches line 118 and `release-qualification.yml:28,32`.
- "The two jobs' `env` blocks are identical" matches line 120.
- Makefile `release-gate` five recipe lines matches lines 38 and 112.
- `lint` recipe matches line 113.
- R2 (job renamed, runs `make release-gate`) matches line 117.
- Governing decision line 41 ("one verification step, after its tag/version check") matches P1 and line 115.
- P3's targets exist: `QUALIFICATION.md:252-253` carries "stable MSRV workspace tests"; `RELEASE.md:18-23` is the `check-release.py` paragraph; `RELEASE.md:124-135` is the eight-step list that `check-docs.py:794-812` forces to name every step.
- `make release-gate` is coherent on the PR job: `test-fuzz-feedback.py:27-30` self-pins the nightly from `rust-toolchain.toml` and needs `llvm-tools`, which line 118 installs; `make package` and `test-verify-published.py` already run hosted elsewhere; `make check` on 1.98.0 is what the published 0.1.0 ran.
- `check-release.py` also runs in `publish.yml:85` and the release workflow, so P1/P2 failures surface in hygiene first and never later than tag time. No contradiction.

Task structure: Task 1 has files, focused checks, review, commit, push, draft PR, issue update. Task 2 has commit, push, PR, evidence record on the issue, close/delete, review. Task 3 is the final PR/handoff task. No Task 0. Task 2 depends on the Task 1 head; Task 3 step 3 handles the rerun condition.

## 4. Findings

No BLOCKING findings.

### ADVISORY 1: name the step parser P1 uses

Plan line 190: "`check-docs.py`'s existing workflow-step parser is reused for the new single step." P1 (line 98) asserts "the `release-qualification.yml` `package` job's named steps are exactly ...", which needs a step parser inside `check-release.py`, and the plan does not say whether that is an import of `check-docs.workflow_step_names` (as `test-check-docs.py` does via `CD`) or a bounded regex in `check-release.py`'s own style. An implementer has to pick.

REWRITE TO (Task 1, `scripts/check-release.py` bullet): "P1's step-list assertion loads `scripts/check-docs.py` by path (the `importlib` pattern `test-check-docs.py` uses) and calls `workflow_step_names` on the `package` job body; no second step parser is written."

### ADVISORY 2: state how P2 classifies an occurrence as stable or nightly

P2 (line 99) checks "every stable pin occurrence" and "every nightly pin occurrence" but does not say how an extracted value is sorted into the two sets, or what happens to a value that is neither.

REWRITE TO (append to P2): "A value starting with `nightly-` is a nightly occurrence; every other value is a stable occurrence. Fixture: an occurrence of `stable` or `beta` fails with its own error."

### ADVISORY 3: record enhancement item 4's residual in the declined list

The "Enhancements considered and declined" section (lines 102-105) lists two items; the in-lane sensitivity probe (enhancement item 4) is dispositioned only in the review ledger (line 188).

REWRITE TO (add a bullet): "**Standing in-lane lint-sensitivity probe:** declined. The hosted probe (Task 2) plus P1's exact `lint`-recipe assertion cover the gate command structurally; the residual is that a later edit to `[workspace.lints.clippy]` is not caught by any check and stays a review matter for that change."

### ADVISORY 4: state the cost of the required-check decline

Line 104: "**Make the job a required status check:** declined. It's an owner ruleset setting."

REWRITE TO: "declined. It's an owner ruleset setting, and the `main` ruleset has no required status checks today. Cost: until the owner adds `release-gate`, a red run is visible on the PR but does not block merge. Recorded in the PR as an owner option."

## 5. Open user decisions

None of the five lenses escalated anything, and none blocks the plan. Two items are the user's call rather than the plan's:

1. **Scope: gate identity vs. the literal issue.** The plan adopts the enhancement TRANSFORMATION: the renamed PR job runs all five release gates on 1.98.0 (`check`, `corpus`, `test-fuzz-feedback.py`, `test-verify-published.py`, `package`), and `release-qualification.yml` is restructured to one `make release-gate` step. Cost: the PR job grows from `cargo test --workspace` to roughly the release job's duration (90-minute allowance, in parallel with the existing 90-minute `fuzz` and `qualification` jobs). Choice: (a) keep the plan as written; (b) reduce Task 1 to the issue's literal ask (`msrv` runs `make check` on 1.98.0, with P2's pin rule) and drop P1's gate-identity assertions. No quoted user decision on this is in the stage inputs.
2. **Required status check.** Choice: (a) add `release-gate` to the `main` ruleset's required checks after merge; (b) leave it as the recorded owner option.

VERDICT: PASS
