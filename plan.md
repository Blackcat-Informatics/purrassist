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
