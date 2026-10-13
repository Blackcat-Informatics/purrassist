# Plan completeness review: issue #26

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/26-lint-the-whole-workspace-with-the/.stage/lint-the-whole-workspace-with-the/plan.md`

# Pass 1 (plan revision 1)

Inputs read in full: `issue.md` (body, 0 comments), `issue-analysis.md` (R1-R4, 4 NEEDS-JUDGMENT), `prior-art-assessment.md` (7 NEEDS-JUDGMENT), `.github/workflows/verify.yml`, `.github/workflows/release-qualification.yml`, `Makefile`, `scripts/check-release.py`, plus `Cargo.toml` lints/rust-version and the `docs/QUALIFICATION.md` / `docs/RELEASE.md` lane text.

Source facts confirmed:
- `verify.yml` `msrv` (lines 35-45) runs on `pull_request` with `RUSTUP_TOOLCHAIN: 1.98.0`. It runs only `cargo test --workspace --locked` and installs no clippy or rustfmt. So the entry point exists, and the plan's change to it is real work.
- `verify.yml` triggers on `push` to `main`, `pull_request` and `workflow_dispatch`. Pushing a feature branch with no PR open runs **no** Verify job.
- `release-qualification.yml` line 28 installs `1.98.0` with `clippy,rustfmt` and runs `make check` (line 43). It sets no `RUSTFLAGS`.
- `Makefile` line 23: `lint` is `cargo clippy --workspace --all-targets -- -D warnings`, without `--locked`.
- `scripts/check-release.py` currently validates only `publish.yml` (`validate_publish_workflow`). It has no `verify.yml` or `release-qualification.yml` rule, so the R4 drift rule is new code.
- `Cargo.toml` line 10: `rust-version = "1.98"`, not `"1.98.0"`. Line 33 sets `pedantic = warn`.
- No doc mentions an `msrv` lane today.

## Requirement -> task -> check

Ledger R-ids (`issue-analysis.md`) are used as the primary key. The plan's own contract ids are given in brackets because they do not match (see B2).

| Ledger R-id | Requirement (verbatim source) | Plan row | Task | Acceptance check | Status |
|---|---|---|---|---|---|
| R1 | "Run `cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.98.0 in a pull-request job" (body L19) | plan R1 | 1 | Hosted `msrv` job on "the Task 1 PR" passes, and its log shows the exact clippy command. Also local `rust-validate 1.98.0 make check`. | **Check not executable as sequenced.** Task 1 opens no PR, so no hosted run exists until Task 3, and Task 3 never checks it (B1). |
| R2 | "for example the existing `msrv` job" (body L19) | none (only an NJ row) | 1 | Covered in substance by the NJ row choosing `msrv`, but it has no contract row (B2) | UNMAPPED (traceability) |
| R3 | "A stable-only lint then fails on the PR that introduces it, not on the release tag." (body L19) | plan R2 | 2 | Probe draft PR's hosted `msrv` job fails, and its log contains `clippy::float_cmp` at the probe file:line | OK (real entry point, real input, specific output) |
| R4 | "A PR that adds an exact float `assert_eq!` in a `purrassist-core` test fails a hosted pull-request job with `clippy::float_cmp`." (body L23) | plan R3 | 2 | As above, with the run URL recorded in `validation.md` and on #26 | OK (see A3 on probe fidelity) |
| (plan-added) | PR lane cannot drift from release qualification | plan R4 | 1 | `check-release.py` rule plus three failing fixtures in `test-check-release.py`, run by `make hygiene` | OK (see A2 on the `1.98` vs `1.98.0` comparison) |
| (plan-added) | Docs describe lanes | plan R5 | 1 | "QUALIFICATION.md describes the msrv lane ..." plus `check-docs.py` passes | Weak (A1) |

NEEDS-JUDGMENT coverage:

| Ledger | Item | Plan disposition |
|---|---|---|
| analysis NJ1 | `--locked` vs `make lint` | NJ row: add `--locked` to `make lint`. OK |
| analysis NJ2 | which job | NJ row: `msrv`. OK |
| analysis NJ3 | required status check | NJ row plus declined enhancement: owner option, not in scope. OK |
| analysis NJ4 | how to demonstrate | NJ row: hosted probe PR. OK |
| prior-art NJ1 | windows-engines Clippy: replace, beside, or move | Declined enhancement: kept. OK |
| prior-art NJ2 | `--locked` form | Same as analysis NJ1. OK |
| prior-art NJ3 | which toolchain | Governing decision: 1.98.0. OK |
| prior-art NJ4 | count event 4? | "3 counted, plus 1 UNKNOWN". OK |
| prior-art NJ5 | is commit-message evidence sufficient for the recurrence claim? | **No disposition** (B3) |
| prior-art NJ6 | should a test or fixture for the acceptance be part of the planned work, and where? | **No explicit disposition** (B3) |
| prior-art NJ7 | licence | NJ row. OK |

Structure: Task 1 onward, no Task 0, final Task 3 creates the PR, posts the plan and hands off. OK. Gaps: Task 2's review/commit/push (A4), and Task 1's hosted check timing (B1).

## Findings

### B1. BLOCKING (UNMAPPED check / STRUCTURE): R1's hosted acceptance has no PR to run on and is never collected

Plan R1 row: "The `msrv` job on the Task 1 PR runs `make check` with `RUSTUP_TOOLCHAIN: 1.98.0`, and its log shows `cargo clippy --workspace --all-targets --locked -- -D warnings` running and passing."

Task 1 focused check: "The hosted `msrv` job on the PR passes, and its log shows the clippy command."

Task 3 is where the PR is created: "3. PR with `Closes #26`."

`verify.yml` runs only on pushes to `main`, on `pull_request`, and on manual dispatch. When Task 1 pushes the feature branch, no Verify run starts. Task 2's probe PR does run `msrv`, but it is designed to fail, so it cannot show the clippy command passing on the clean change. Task 3 opens the PR but has no step that waits for `msrv` or records its log. As written, the issue's central requirement (the clippy command running in a pull-request job on 1.98.0) has no task that observes it passing on a hosted runner.

REWRITE TO (Task 1, replace the last focused-check bullet and the "Then:" line):
> - After the push, open the PR for this branch now as a **draft** titled for #26 with `Closes #26` in the body. Wait for the hosted `Verify` / `msrv` job on that PR. Required: the job concludes `success`; its log contains the exact line `cargo clippy --workspace --all-targets --locked -- -D warnings`; and its toolchain step shows `1.98.0`. Record the run URL and that log line in `validation.md` and on issue #26.
>
> Then: task review, commit, push, issue update (with the run URL).

REWRITE TO (Task 3, replace "3. PR with `Closes #26`."):
> 3. Mark the Task 1 draft PR (`Closes #26`) ready for review. Confirm that the hosted `msrv` job on its final head concludes `success` and that its log shows `cargo clippy --workspace --all-targets --locked -- -D warnings`. Record the run URL.

### B2. BLOCKING (UNMAPPED / traceability): plan R-ids collide with the requirements ledger

The plan's contract renumbers the requirements. Ledger R2 ("for example the existing `msrv` job") has no contract row. Plan "R2" is ledger R3, plan "R3" is ledger R4, and plan "R4" is a plan-derived drift rule. Ledger R4 is the issue's acceptance criterion. Every later reviewer, and Task 3's "Completeness review against this plan", will match `R4` to the wrong requirement.

REWRITE TO (completeness contract):
> | R1 | Run `cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.98.0 in a PR job | 1 | (B1 text) |
> | R2 | "for example the existing `msrv` job" | 1 | The `verify.yml` `msrv` job (triggered by `pull_request`) is the job that runs it. `check-release.py` fails if `msrv` stops running `make check` (fixture P1). |
> | R3 | A stable-only lint fails on the PR that introduces it | 2 | Probe draft PR's hosted `msrv` job fails, and its log contains `clippy::float_cmp` at the probe's file:line. |
> | R4 | Acceptance: an exact float `assert_eq!` in a core test fails a hosted PR job with `float_cmp` | 2 | As R3, with the probe in `crates/core`. Run URL in `validation.md` and on the issue. |
> | P1 | (plan-derived) PR lane can't drift from release qualification | 1 | (current R4 text) |
> | P2 | (plan-derived) Docs describe the lanes accurately | 1 | (A1 text) |

### B3. BLOCKING (missing NEEDS-JUDGMENT dispositions): prior-art NJ5 and NJ6

The plan's NJ table covers prior-art NJ1-4 and NJ7, but not:
- NJ5: "Is that sufficient evidence for the planner's recurrence claim?"
- NJ6: "should a test or fixture for it be part of the planned work, and where should it live?"

NJ6 bears on delivery. The plan's answer is implied: the probe is never merged, and the persistent guard is the drift rule plus the lane itself. That answer is not stated.

REWRITE TO (add rows to the NJ dispositions table):
> | Recurrence evidence sufficiency (prior-art NJ5) | Sufficient for motivation only. The rust-v0.1.0 failure is evidenced by commit `598d059`'s message and the issue body. No CI log is in the repository, and the plan makes no claim that depends on more. |
> | Persistent test or fixture for the acceptance (prior-art NJ6) | No float probe is merged; it would fail the gate by design. The acceptance is demonstrated once, by the hosted probe PR (Task 2). The lasting guards are the `msrv` lane itself and the `check-release.py` drift rule with its failing fixtures in `scripts/test-check-release.py` (P1), which run in `make hygiene` on every PR. |

### A1. ADVISORY (UNFALSIFIABLE): docs check is prose

Plan R5: "QUALIFICATION.md describes the `msrv` lane as the release gate on the release toolchain. `check-docs.py` passes." `check-docs.py` passes whether or not the sentence exists. No doc names `msrv` today. Task 1 also lists `docs/RELEASE.md` "where they describe CI lanes", but RELEASE.md describes only the release workflow's own steps.

REWRITE TO:
> `docs/QUALIFICATION.md` gains a sentence naming the `msrv` job in `.github/workflows/verify.yml`, stating that it runs `make check` on Rust `1.98.0` (the release toolchain) on every pull request. `grep -n 'msrv' docs/QUALIFICATION.md` shows that sentence, and it contains `make check` and `1.98.0`. `make hygiene` (including `check-docs.py`) exits 0. RELEASE.md is edited only if a grep for `verify.yml` or `pull request` finds a lane description that is now wrong.

### A2. ADVISORY: the drift-rule comparison as worded would fail on the real files

Plan R4: "asserts that the `verify.yml` `msrv` job's `RUSTUP_TOOLCHAIN` equals the toolchain `release-qualification.yml` installs and the workspace `rust-version`". `Cargo.toml` has `rust-version = "1.98"`, and the workflows use `1.98.0`. Strict equality with `rust-version` fails on the current tree.

REWRITE TO:
> asserts that the `verify.yml` `msrv` job's `RUSTUP_TOOLCHAIN` equals `release-qualification.yml`'s `RUSTUP_TOOLCHAIN` and its `rustup toolchain install` argument, that this toolchain equals the workspace `rust-version` or extends it with `.<patch>`, that `msrv` installs `clippy` and `rustfmt`, and that both jobs run `make check`.

### A3. ADVISORY: probe fidelity

Task 2 step 1: "add `assert_eq!(0.1_f64 + 0.2, 0.3);`". Both operands are compile-time constants. The #24 failure compared a runtime value (`above_midpoint.parse::<f64>().unwrap() as f32`) in `crates/core/src/tool_call/from_node.rs`. Step 2's local run will reveal whether `float_cmp` fires on an all-constant comparison, but the plan has no fallback if it does not. A probe that reproduces the historical failure is the more faithful test.

REWRITE TO:
> 1. On a probe branch from the Task 1 head, in `crates/core/src/tool_call/from_node.rs` test `f32_rounds_the_spelling_once_to_nearest`, re-introduce the pre-#24 exact comparison: `assert_eq!(above_midpoint.parse::<f64>().unwrap() as f32, 1.0_f32);`. If step 2 shows that `float_cmp` does not fire, stop and report; do not substitute a different lint.

### A4. ADVISORY (STRUCTURE): Task 2 lacks an explicit task review, commit and push

Task 2 must commit and push to open its PR, but it never says so, and it has no independent task review. It does have an issue update (step 5).

REWRITE TO (append to Task 2):
> 7. Commit the probe only on `probe/26-float-cmp` and push that branch; never push it to the feature branch. Then: task review of the recorded evidence, and an issue update (step 5).

## Pass 1 summary

The plan delivers every issue requirement in substance. It uses the right entry point (the `pull_request` `msrv` job), the exact command (through `make lint` with `--locked`), and a real hosted demonstration of the acceptance criterion with a falsifiable log check. The remaining problems are three BLOCKING gaps. The passing hosted run of the new lane is never collected, because Task 1 opens no PR and Task 3 does not check it. The contract's R-ids do not match the ledger. Two prior-art NEEDS-JUDGMENT items have no disposition.

Pass 1 verdict: BLOCKED (superseded by Pass 2 below).

# Pass 2 (plan revision 2)

Rechecked: the full revision-2 `plan.md`, restructured around `make release-gate`. Following the recheck rule, only text introduced or changed by the revision was reviewed for new findings.

New source claims in revision 2, verified in the worktree:
- `docs/QUALIFICATION.md:252-253` contains "stable MSRV workspace tests". The P3 grep target is real.
- `docs/RELEASE.md:18-23` is the `check-release.py` paragraph ("It runs in `make check` and `make package`"). Lines 118-135 hold the eight-step qualification list that collapses under Task 1.
- `scripts/check-docs.py` `check_release_qualification` (rule `release-qualification-steps`, line 795) requires every named step of `release-qualification.yml` to appear in RELEASE.md's qualification section. P3's "`check-docs.py`'s qualification-steps rule passes on the new step names" is therefore a real, falsifiable gate on the renamed step `Verify the release gate`.
- `publish.yml` pins `1.98.0` at lines 72 and 78, so P2's three-workflow pin check has real occurrences to cover. `rust-toolchain.toml` exists for the nightly source.
- The probe is restored from the pre-#24 comparison in `from_node.rs` (current lines 336-339 compare `.to_bits()`). The float form is true at runtime, as the plan states, so the only gate that can fail the probe is Clippy.

## Prior findings

| Finding | Status | Evidence in revision 2 |
|---|---|---|
| B1: R1 hosted check had no PR | **DISCHARGED** | Task 1 "Then" step 3: "Open the PR as a **draft**, record its head SHA, and wait for the hosted `release-gate` job to succeed on that SHA (R1)". Contract R1: "Its log shows `cargo clippy --workspace --all-targets --locked -- -D warnings` running and passing, and the job concludes `success`." Task 3 step 3 re-confirms green on the final head. `make lint` is not `@`-silenced, so the recipe line is echoed into the job log. |
| B2: R-id collision with ledger | **DISCHARGED** | "Ledger ids R1–R4; plan additions P1–P3." R2 now has its own row ("Use the existing `msrv` job"), with a check: the job is renamed `release-gate`, and `check-release.py` (P1/P2) asserts it. |
| B3: prior-art NJ5/NJ6 undisposed | **DISCHARGED** | The `prior-art-assessment.md` table now has rows 1-7. Row 5 says the evidence is sufficient: `598d059` plus a run id recorded on issue #1. Row 6 says the lasting guard is the `check-release.py` rules and their tests, and the hosted probe is the one-time acceptance. |
| A1: docs check prose | **DISCHARGED** | P3: the grep-checked replacement of the "stable MSRV workspace tests" phrase, a new command-list entry, RELEASE.md lines 18-23 and the step list, and the `release-qualification-steps` rule. |
| A2: `1.98` vs `1.98.0` equality | **DISCHARGED** | P2: "`major.minor` equal to the Cargo `rust-version`", with a "`rust-version` mismatch" fixture. |
| A3: probe fidelity | **DISCHARGED** | Task 2 step 1 restores the pre-#24 runtime comparison in `crates/core/src/tool_call/from_node.rs`. |
| A4: Task 2 review/commit/push | **DISCHARGED** | Step 2 commits, step 4 opens the PR (which requires a push), step 6 updates the issue, and step 8 is the task review (`tasks/T2-review.md`). Push is implicit rather than written; see N2. |

## Requirement -> task -> check (revision 2)

| Id | Requirement | Task | Check | Status |
|---|---|---|---|---|
| R1 | Workspace clippy with `--locked` on 1.98.0 in a PR job | 1 (+3) | Hosted `release-gate` job on the Task 1 draft PR, at a recorded head SHA: log shows the exact clippy command, and the job concludes `success`. Local `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` exits 0. | OK |
| R2 | The existing `msrv` job | 1 | The `msrv` job becomes `release-gate` running `make release-gate` on `1.98.0`, enforced by `check-release.py` | OK |
| R3 | Stable-only lint fails on the introducing PR | 2 | Probe PR's hosted `release-gate` job concludes `failure` with `clippy::float_cmp` at the probe line, while `native` passes | OK |
| R4 | Exact float `assert_eq!` in a core test fails a hosted PR job with `float_cmp` | 2 (+3 rerun) | As R3. Run URL and excerpt go in `validation.md` and on #26, and the probe is rerun if gate files change after Task 2. | OK |
| P1 | Gate identity | 1 | `check-release.py` assertions, each with a named failing fixture | OK (see N1) |
| P2 | Single-sourced pins | 1 | Five named failing fixtures | OK |
| P3 | Docs | 1 | Grep-checked sentences plus the `release-qualification-steps` rule | OK |

NEEDS-JUDGMENT: all 4 `issue-analysis.md` items and all 7 `prior-art-assessment.md` items have dispositions.

Structure: Tasks 1-3, no Task 0. Task 1 has focused checks, review, commit, push, draft PR and issue update. Task 2 has commit, PR, issue record and review. Task 3 confirms green, marks the PR ready with `Closes #26`, posts the plan and hands off. OK.

## New findings (revision-2 text only)

### N1. ADVISORY (falsifiability of P1): "exactly one verification step" is ambiguous

P1: "`release-qualification.yml` has exactly one verification step and it runs `make release-gate`". Task 1 keeps the step `Verify release set and version`, which runs `check-release.py --tag`. Under a name-based reading, that step is also a "verification step", so the assertion either fails on the intended tree or needs an unstated definition.

REWRITE TO:
> `release-qualification.yml`'s `package` job has exactly one step whose `run:` invokes `make`, and that command is `make release-gate`. Its only other `run:` steps are the two `rustup toolchain install` steps and the `Verify release set and version` step.

### N2. ADVISORY: Task 2 does not state the push

Task 2 step 2: "Commit it normally ...". Step 4 opens the PR, which requires a pushed branch, so delivery is not at risk.

REWRITE TO (step 2, append):
> Push only `probe/26-float-cmp` to the remote; never push the probe commit to the feature branch.

### N3. ADVISORY: the run id cited for prior-art NJ5 is not in the stage inputs

Row 5: "Sufficient: commit `598d059` plus the hosted run id recorded on issue #1". The id `38351629747` appears only in `plan.md`, not in `brief.json`, `prior-art.md` or the assessment. This affects motivation only, not delivery.

REWRITE TO:
> Sufficient: commit `598d059`'s message, and release-qualification run 38351629747 as recorded in the issue #1 comment `<comment URL>`. Task 3's artifact inventory links that comment.

## Pass 2 summary

All three BLOCKING findings and all four advisories from Pass 1 are discharged. Every ledger requirement R1-R4 maps to a task with a falsifiable check on a hosted `pull_request` run of the named `release-gate` job, or on the shipped `make release-gate` target. All 11 NEEDS-JUDGMENT items have dispositions. There are no stubs, deferrals, or acceptance by refusal. The three new findings are advisory.

VERDICT: PASS
