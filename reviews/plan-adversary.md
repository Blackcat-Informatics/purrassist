# Plan completion adversary: issue #26

- Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/26-lint-the-whole-workspace-with-the/.stage/lint-the-whole-workspace-with-the/plan.md`
- Pass: 1 of 3 (no prior `plan-adversary.md`)
- Inputs read: `issue.md` (0 comments), `issue-analysis.md` (ledger R1-R4), `prior-art-assessment.md`, `.goals`, `AGENTS.md`. There is no `.baseline`, no `CONSTITUTION.md`, and no ADRs. No user scope decisions were supplied.
- Repository facts checked: `.github/workflows/verify.yml` (`on: push: [main]`, `pull_request`, `workflow_dispatch`; `msrv` at lines 35-45), `.github/workflows/release-qualification.yml:22,28`, `Makefile:6,11-23`, `Cargo.toml:10` (`rust-version = "1.98"`), `scripts/check-release.py` (today it reads neither `verify.yml` nor the toolchain), `docs/QUALIFICATION.md:252-253`, `docs/RELEASE.md:118-152`, `open.json` (no PR exists yet).

## Cleared (no finding)

- **Pre-emptive descope:** none found. The plan declines "required status check" and "drop windows-engines Clippy". The issue asks for neither: it says "fails a hosted pull-request job", not "blocks merge". The plan does not shrink the issue's command, toolchain, scope (`--workspace --all-targets`) or trigger. `make check` contains `make lint`, and Task 1 makes `make lint` the issue's exact command.
- **Planned-by-refusal:** none found. Task 2 uses a real hosted `pull_request` run of the actual edit the issue describes. The local `rust-validate` run is labelled supporting evidence only.
- **Ledger NEEDS-JUDGMENT 1-4:** each has a disposition: `--locked` added to `make lint`; job is `msrv`; required check is out of scope as literal-reading; hosted probe PR.

## Findings

### F1: UNMAPPED REQUIREMENT. The contract renumbers the ledger, and ledger R2 has no row.

Plan lines 67-71:

> `| R2 | A stable-only lint fails on the PR that introduces it | 2 | ...`
> `| R3 | Acceptance: an exact float `assert_eq!` in a core test fails a hosted PR job with `float_cmp` | 2 | ...`
> `| R4 | The PR lane can't drift from release qualification | 1 | ...`
> `| R5 | Docs describe the lanes accurately | 1 | ...`

These are the ledger rows in `issue-analysis.md`:

- R2 is "for example the existing `msrv` job".
- R3 is "A stable-only lint then fails on the PR that introduces it, not on the release tag."
- R4 is the Acceptance sentence.

So:

- Ledger R2 has no contract row. It is handled only in the NEEDS-JUDGMENT prose.
- Plan R2 and R3 carry ledger R3 and R4 under the wrong ids.
- Plan R4 and R5 are plan-added items that reuse ledger ids.

A later review that checks "R4 delivered" against the ledger would find drift-rule evidence instead of the hosted acceptance run.

REWRITE TO: key the contract by the ledger ids, quote each verbatim, and give plan-added rows a separate prefix:

| R-id | Requirement (verbatim) | Task | Executable acceptance |
|---|---|---|---|
| R1 | "Run `cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.98.0 in a pull-request job" | 1 | (current R1 acceptance, as amended by F2) |
| R2 | "for example the existing `msrv` job" | 1 | `verify.yml` job `msrv` (workflow `on: pull_request`) has `RUSTUP_TOOLCHAIN: 1.98.0` and a `run: make check` step. Its `pull_request` run on PR #N logs `cargo clippy --workspace --all-targets --locked -- -D warnings`. |
| R3 | "A stable-only lint then fails on the PR that introduces it, not on the release tag." | 2 | (current plan R2 acceptance) |
| R4 | "A PR that adds an exact float `assert_eq!` in a `purrassist-core` test fails a hosted pull-request job with `clippy::float_cmp`." | 2 | (current plan R3 acceptance) |
| P1 (plan-added) | PR lane cannot drift from release qualification | 1 | (as amended by F3) |
| P2 (plan-added) | Docs state the new lane | 1 | (as amended by F4) |

### F2: UNFALSIFIABLE CRITERION. R1's hosted evidence names a PR that does not exist until Task 3, and the probe evidence is not tied to the merged head.

Plan line 67 (R1): "The `msrv` job on the Task 1 PR runs `make check` ..."
Plan line 95 (Task 1 focused check): "The hosted `msrv` job on the PR passes, and its log shows the clippy command."
Plan line 97: "Then: task review, commit, push, issue update."
Plan line 112 (Task 3): "3. PR with `Closes #26`."
Plan line 101 (Task 2): "On a probe branch from the Task 1 head ..."

`verify.yml` runs only on `pull_request`, pushes to `main`, and `workflow_dispatch` (lines 4-8). A push to the Task 1 branch therefore runs nothing, and the PR is not opened until Task 3. That leaves "the Task 1 PR" undefined, and three readings would all count as success:

- a `workflow_dispatch` run, which is not a pull-request job;
- the probe PR's run, which is a different PR;
- a run taken after Task 3.

Also, Task 2's probe is built on "the Task 1 head". If review fixes later change `verify.yml` or `Makefile`, the R3/R4 evidence covers a lane configuration that does not merge. `docs/QUALIFICATION.md:255` says that "a green run applies to its commit only".

REWRITE TO:

- (Task 1, last line): "Then: task review, commit, push, and open PR #N as a **draft** with `Closes #26`. Its `pull_request` run is R1's hosted evidence. Then the issue update."
- (Task 3 step 3): "Mark PR #N ready for review. Its body keeps `Closes #26`."
- (R1 acceptance): "The `pull_request`-event run of job `msrv` on PR #N at head SHA S runs `make check` under `RUSTUP_TOOLCHAIN: 1.98.0`. Its log shows `cargo clippy --workspace --all-targets --locked -- -D warnings` and the job concludes `success`. It is not a `workflow_dispatch` run and not the probe PR's run. `validation.md` records the run URL and S. Locally, `rust-validate 1.98.0 make check` exits 0."
- (Task 2 step 1, appended): "The probe branch is based on PR #N's head SHA S, and `validation.md` records S. If any later commit to PR #N changes `.github/workflows/verify.yml` or `Makefile`, rebase the probe onto the new head and repeat steps 3-5 before Task 3."

### F3: UNFALSIFIABLE CRITERION. The drift rule's "equals the workspace `rust-version`" cannot hold literally, so it must be reinterpreted.

Plan line 70: "`scripts/check-release.py` asserts that the `verify.yml` `msrv` job's `RUSTUP_TOOLCHAIN` equals the toolchain `release-qualification.yml` installs and the workspace `rust-version` ..."
Plan line 35: "(`1.98.0`, which is also the workspace `rust-version`)"

`Cargo.toml:10` is `rust-version = "1.98"`, while both workflows use `1.98.0`. A literal equality check fails against the current repository. The implementer then has to choose a comparison rule after the fact, and almost any choice can be called "equals".

REWRITE TO: "P1 | PR lane cannot drift from release qualification | 1 | `scripts/check-release.py` asserts all of the following:

- the `verify.yml` `msrv` job's `RUSTUP_TOOLCHAIN` string equals the `release-qualification.yml` `package` job's `RUSTUP_TOOLCHAIN` string;
- it also equals the version in that job's `rustup toolchain install` step;
- its `major.minor` prefix equals `[workspace.package] rust-version` in `Cargo.toml` (currently `1.98`);
- the `msrv` job has a `run: make check` step.

`scripts/test-check-release.py` shows that the unmodified repository passes. Each of these fixtures makes the check exit non-zero with a named error:

- (a) `msrv` set to `RUSTUP_TOOLCHAIN: 1.97.0`;
- (b) the `msrv` `make check` step replaced by `cargo test --workspace --locked`;
- (c) `release-qualification.yml` bumped to `1.99.0` with `msrv` left at `1.98.0`.

Both scripts run in `make hygiene` (Makefile:14-15)." Also correct plan line 35 to say "`1.98.0`, whose `major.minor` is the workspace `rust-version` `1.98`".

### F4: UNFALSIFIABLE CRITERION. The docs row is prose, and its file list disagrees with Task 1.

Plan line 71: "`| R5 | Docs describe the lanes accurately | 1 | QUALIFICATION.md describes the `msrv` lane as the release gate on the release toolchain. `check-docs.py` passes. |`"
Plan line 88: "`docs/QUALIFICATION.md` and `docs/RELEASE.md` where they describe CI lanes."

`check-docs.py` passing does not test this content. "Describes ... as" can be met by any wording. `docs/RELEASE.md` does not describe the `msrv` lane anywhere, so "where they describe CI lanes" has nothing to point at. The stale public claim is concrete: `docs/QUALIFICATION.md:252-253` says "The `Verify` workflow additionally runs ... stable MSRV workspace tests ...". After Task 1, that understates the lane.

REWRITE TO: "P2 | Docs state the new lane | 1 | `docs/QUALIFICATION.md` lines 252-253 no longer say `stable MSRV workspace tests`. Instead they say the `msrv` job runs `make check`, including `cargo clippy --workspace --all-targets --locked -- -D warnings`, on Rust 1.98.0 for every pull request and main push. `grep -rn 'MSRV workspace tests' docs README.md` prints nothing. `docs/RELEASE.md` §Qualification workflow (lines 147-152) gains one sentence: `verify.yml`'s `msrv` job runs the same `make check` on the same toolchain on every pull request. If not, Task 1 removes `docs/RELEASE.md` from its file list. `make hygiene` exits 0 (it runs `check-docs.py` and `test-check-docs.py`)."

## Non-blocking observation (no verdict weight)

Plan line 101's probe `assert_eq!(0.1_f64 + 0.2, 0.3);` is also false at runtime. The probe PR's nightly `native` jobs and every test lane would fail on it even without the new lane. The R3/R4 criteria still single out the new lane, because they require `clippy::float_cmp` in the `msrv` log, so this is not a finding.

A cleaner demonstration of "stable-only lint fails the PR" uses an exact float comparison that is true at runtime. One option is to revert 598d059's change at `crates/core/src/tool_call/from_node.rs:336-340`, which compared the `f32` values rather than their bits and is the assertion that actually failed `rust-v0.1.0` on 1.98.0. Another is `assert_eq!(0.5_f64 + 0.25, 0.75);`. Either way, only the 1.98.0 Clippy lane can fail the probe.

## User decision required

None. All four findings are contract rewrites inside the issue's stated scope.

Status under the stage protocol: FINDINGS-OPEN (pass 1 of 3). F1-F4 must be DISCHARGED on recheck.

Pass 1 verdict: BLOCKED (superseded by Pass 2 below)

---

## Pass 2

- Reviewed plan: same path, "Revision 2" (plan line 3), which is restructured around a shared `make release-gate`.
- Pass: 2 of 3.
- Scope:
  - adjudicate F1-F4 and the pass 1 note;
  - audit the text added by those fixes and by the restructure: governing decisions lines 38-48, contract lines 90-100, Task 1 lines 111-136, Task 2 lines 140-147, Task 3 line 153.
  - Text cleared on pass 1 and left unchanged is not reopened.
- Repository facts rechecked:
  - `docs/RELEASE.md:18-23` is the `check-release.py` paragraph the plan cites.
  - `scripts/check-docs.py:794-812` (`release-qualification-steps`) requires RELEASE.md's qualification section to name every named step of `release-qualification.yml`.
  - `release-qualification.yml:33` has the step `Verify release set and version`.
  - `Cargo.toml:10` is `rust-version = "1.98"`.

### Prior findings

**F1: DISCHARGED.** Line 90 reads "Ledger ids R1–R4; plan additions P1–P3."

- Rows R1 (line 94), R2 (line 95, the `msrv` job), R3 (line 96) and R4 (line 97) now match ledger R1-R4 one for one.
- The plan's additions use the separate ids P1-P3.
- The requirement column paraphrases rather than quoting. No requirement is lost or merged.

**F2: DISCHARGED.**

- Line 135: "Open the PR as a **draft**, record its head SHA, and wait for the hosted `release-gate` job to succeed on that SHA (R1)."
- Line 94: R1 evidence is "On the draft PR opened at the end of Task 1 (head SHA recorded)". That is the issue PR itself, not a dispatch run and not the probe.
- Line 97 and line 153: "rerun the probe if `verify.yml` or `Makefile` changed after Task 2", and "Confirm the hosted `release-gate` job is green on the final head".

**F3: DISCHARGED.**

- Line 45 and line 99 now say every stable pin occurrence in all three workflows is identical, "with `major.minor` equal to the Cargo `rust-version`".
- Line 99's fixtures include "a `rust-version` mismatch".
- This matches `Cargo.toml:10` (`"1.98"`) against `1.98.0`.

**F4: DISCHARGED.** Line 100:

- names `docs/QUALIFICATION.md:252-253`;
- requires the phrase "stable MSRV workspace tests" to be gone, checked by grep;
- adds `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` to the command list.

The RELEASE.md step list is now checked by the existing `release-qualification-steps` rule (check-docs.py:794-812). The cited RELEASE.md:18-23 paragraph exists. A non-blocking advisory: name the grep tokens the updated 18-23 paragraph must contain, for example `verify.yml`, `rust-toolchain.toml`, `rust-version`.

**Pass 1 note (runtime-false probe): DISCHARGED.**

- Lines 74 and 140 restore the pre-#24 `assert_eq!(… as f32, 1.0, …)` in `from_node.rs`, which is true at runtime.
- R3 (line 96) and Task 2 step 5 (line 144) also require `native` to pass. That attributes the failure to the release-toolchain gate.

### New finding in text added by the fixes

#### N1: UNFALSIFIABLE CRITERION. P1's gate-identity assertions contradict Task 1 and the current workflow, so the implementer would have to define them.

Plan line 98 (P1):

> "`release-qualification.yml` has exactly one verification step and it runs `make release-gate`; ... the Makefile `release-gate` target lists `check corpus` and the three scripts."

Plan line 112 (Task 1):

> "a `release-gate` target (`check`, `corpus`, `python3 scripts/test-fuzz-feedback.py`, `python3 scripts/test-verify-published.py`, `package`)"

Plan line 115: "The install and version-check steps stay."

Plan line 95 (R2): "This is checked by the `check-release.py` rule in P2."

Evidence:

- (a) **The target's contents.** Task 1 defines the target as two make targets, two scripts and the `package` target, not "check corpus and the three scripts".
  - Under P1's wording, a checker that never requires `package` still meets "the three scripts".
  - Removing `package` from the shared gate would then drop the stable `cargo package` verify build from release qualification, and no fixture would fail. Line 38 names that build as the tag-only gap.
- (b) **"Exactly one verification step."** The version-check step that line 115 keeps is named `Verify release set and version` (release-qualification.yml:33) and runs `check-release.py`.
  - Read plainly, the workflow then has two verification steps, both named `Verify …`.
  - So the assertion as written fails on the intended layout, and the implementer has to make up a definition after the fact.
- (c) **Wrong cross-reference.** R2's acceptance cites P2, but the assertion that the `verify.yml` job runs `make release-gate` is in P1. P2 checks only the toolchain pins.

REWRITE TO (P1 acceptance): "`scripts/check-release.py` asserts all of the following:

- (1) In the `release-qualification.yml` `package` job, every `run:` step other than the two `rustup toolchain install` steps and `Verify release set and version` (`check-release.py`) is exactly one step, `Verify the release gate`, whose `run:` is `make release-gate`.
- (2) In the `verify.yml` `release-gate` job, the only `run:` step other than its `rustup toolchain install` steps is `make release-gate`.
- (3) Both jobs install the stable pin with components `clippy,rustfmt` and the nightly pin with `llvm-tools`.
- (4) The Makefile `release-gate` target runs exactly `check`, `corpus`, `python3 scripts/test-fuzz-feedback.py`, `python3 scripts/test-verify-published.py` and `package`.

`scripts/test-check-release.py` shows that the unmodified repository passes. It has one failing fixture per assertion, each naming its error, including:

- an extra `run: make corpus` step in `release-qualification.yml`;
- the `verify.yml` `release-gate` job running `make check` instead;
- `clippy` missing from one install line;
- `package` removed from the Makefile target."

REWRITE TO (R2 acceptance, last sentence): "This is checked by the `check-release.py` rules in P1 (the job runs `make release-gate`) and P2 (its stable pin)."

### Cleared in new text

- **No descope.** The `make release-gate` restructure adds work; it does not remove any. The issue's command still runs, through `make check` → `make lint` with `--locked`, in a `pull_request` job on 1.98.0.
- **No planned-by-refusal.** The R3/R4 evidence is still a real hosted probe PR (lines 140-147), and the local run is labelled a convenience (line 48).
- **Declined enhancements are unchanged** (lines 104-105). Neither is asked for by the issue.

## User decision required (pass 2)

None. N1 is a rewrite of one plan-added row inside the issue's scope. If N1 is still open on pass 3, it goes to the user as ESCALATE-TO-HUMAN with two choices: adopt the exact P1 assertions above, or remove P1's "gate identity" completion claim from the contract.

Status under the stage protocol: FINDINGS-OPEN (pass 2 of 3). F1-F4 and the note are DISCHARGED. N1 is open.

Pass 2 verdict: BLOCKED (superseded by Pass 3 below)

---

## Pass 3

- Reviewed plan: same path. The header still reads "Revision 2"; pass-2 dispositions are at lines 192-219.
- Pass: 3 of 3, the final allowed pass.
- Scope:
  - adjudicate N1;
  - audit the text its fix introduced: P1 (line 98), R2 (line 95), P2 (line 99), P3 (line 100), and Task 1 lines 112, 120 and 122-123.
  - Text already cleared is not reopened.
- Repository facts rechecked: `Makefile:26` (`test`) and `Makefile:33` (`corpus`) prepend `-D warnings` to `RUSTFLAGS` themselves, and `Makefile:36` (`doc`) sets `RUSTDOCFLAGS` `-D warnings`. `release-qualification.yml:21-22`: the `package` job `env` is `RUSTUP_TOOLCHAIN: 1.98.0` only.

### N1: DISCHARGED

- **(a) The target's contents.** Line 98 now requires that "the Makefile `release-gate` target has no prerequisites and its recipe lines are exactly the five above, in order, so every gate including `package` is present". Line 112 names the five recipe lines verbatim: `$(MAKE) check`, `$(MAKE) corpus`, `python3 scripts/test-fuzz-feedback.py`, `python3 scripts/test-verify-published.py`, `$(MAKE) package`. A missing `package` now fails P1, with "a failing fixture per assertion, each naming its error".
- **(b) "Exactly one verification step."** Line 98 now says "exactly one `release-qualification.yml` step runs `make`, and that step runs `make release-gate` (the `Verify release set and version` step runs only `check-release.py`)". That holds on the intended layout: the version check runs `python3 scripts/check-release.py`, not `make`. No definition has to be invented.
- **(c) Cross-reference.** Line 95 now reads "This is checked by the `check-release.py` rule in P1." P1 holds the "`verify.yml` `release-gate` job runs `make release-gate`" assertion.

### Text introduced by the fix

**`RUSTFLAGS` dropped from the PR job (line 120), and P1's "the two jobs' `env` blocks are identical" (line 98).** Not a descope.

- The old `msrv` job's only capability was `cargo test --workspace --locked` with `-D warnings`.
- `make test` (Makefile:26) still runs exactly that with `-D warnings` added. `make corpus` (Makefile:33) and `make doc` (Makefile:36) also deny warnings.
- The issue's command carries its own `-D warnings`, now pinned by P1's lint-recipe assertion.
- Env equality is falsifiable against `release-qualification.yml:21-22`.

**P1 lint-recipe assertion.** "The Makefile `lint` recipe is exactly `cargo clippy --workspace --all-targets --locked -- -D warnings`". This ties ledger R1's exact command to an executable check, so it strengthens R1 and creates no finding.

**P2 extractor (line 99).** "Ignoring profile and components so `publish.yml`'s component-less install still matches". This is consistent with P1, which separately requires `clippy,rustfmt` and `llvm-tools` on the two gate jobs. The criteria are falsifiable, with five named fixtures.

**P3 (line 100).** It adds "stating that this lane is also the MSRV 1.98 build-and-test evidence". That keeps the public MSRV claim, and it is grep-checkable, so it creates no finding.

**Task 1 lines 122-123.** The `test-check-docs.py` assertion rename and the sequential recipe lines create no completion gap.

**R1-R4 rows.** Unchanged in substance from pass 2. All remain mapped to tasks, with falsifiable hosted evidence.

### Non-blocking advisories (no verdict weight)

1. **P1's "exactly one step runs `make`" leaves a gap.** It would not catch a future release-only step that does not use `make`, such as an added `run: python3 scripts/…`, which would bring back gate divergence.
   - The existing `release-qualification-steps` docs rule (check-docs.py:794-812) would force RELEASE.md to name such a step, but it would not fail it.
   - Optional tightening: "every `run:` step other than the two `rustup toolchain install` steps and `Verify release set and version` is `make release-gate`".
   - This is about how strong a plan-added guard is, not about completing the issue, so it stays an advisory.
2. **P1 says "the five above".** The exact recipe strings appear at line 112, below the table. Citing "Task 1, line 112" would make the reference self-contained. The set itself is unambiguous (line 38 and line 112 agree).
3. The plan header still says "Revision 2" while carrying pass-2 dispositions. This is cosmetic.

## User decision required (pass 3)

None. Every finding from passes 1-2 (F1-F4 and N1) is DISCHARGED, and the pass 1 note is adopted. Nothing remains to escalate.

Status under the stage protocol: PASS (pass 3 of 3).

VERDICT: PASS
