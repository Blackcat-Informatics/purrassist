# Plan constraints review: issue #26

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/26-lint-the-whole-workspace-with-the/.stage/lint-the-whole-workspace-with-the/plan.md`

## Governing documents

Read:

- `.goals` (13 lines)
- `AGENTS.md`
- `CONTRIBUTING.md`
- `docs/RELEASE.md`
- `docs/QUALIFICATION.md` (lines 225-272)
- `docs/IMPLEMENTATION.md` (lines 135-157)
- `docs/DEPENDENCIES.md` (lines 82-93)
- `README.md` (lines 279-289)
- `CHANGELOG.md` (header)
- Existing CI and check conventions: `.github/workflows/verify.yml`, `.github/workflows/release-qualification.yml`, `Makefile`, `rust-toolchain.toml`, `Cargo.toml`, `scripts/check-release.py`, the workflow and Makefile assertions in `scripts/check-docs.py` and `scripts/test-check-docs.py`, and `vendor/tooling.json`.
- The user's global attribution rule: no agent credits or attribution trailers on commits, never bypass commit verification.

Recorded absent:

- `.baseline`, `CONSTITUTION.md`, `CLAUDE.md`, `PRECOMMIT_GUIDANCE.md`, and any ADR directory (`docs/adr/`, `docs/decisions/`).
- No nested `AGENTS.md` or `CLAUDE.md` in the directories the plan touches.
- No commit hooks are installed: `.git/hooks` holds only `*.sample` files, and no `core.hooksPath` is set in the repository or global git config.

User decisions in this session: none quoted beyond the global attribution and hook rules. The plan's "Governing decisions" (plan.md:35-36) are the plan author's choices, not user decisions. None of them conflicts with a standing constraint.

## Findings

No BLOCKING findings.

### A1 (ADVISORY): Task 1's pre-commit gate leaves out the default `make check`

Governing text:

- CONTRIBUTING.md:8: "Run `make check` before committing. Never bypass commit hooks. Focused tests do not replace the full gate."
- AGENTS.md (Verification): "A focused test is not the full gate. `make check` is the local verification gate".
- rust-toolchain.toml:4: `channel = "nightly-2026-09-14"`. This is the toolchain a plain `make check` resolves to.

Plan text (plan.md:92-94): "`rust-validate 1.98.0 make check` exits 0 ... `make hygiene` exits 0."

Task 1 changes the `Makefile` `lint` recipe, which both toolchains run. The plan checks it only under 1.98.0. The repository's documented local gate is `make check` on the pinned toolchain.

REWRITE TO: "Before committing: `make check` on the pinned toolchain (rust-toolchain.toml) exits 0, and `rust-validate 1.98.0 make check` exits 0. The second is the command the new `msrv` lane runs."

### A2 (ADVISORY): Task 2 must say how the probe commit is made and that hooks stay in force

Governing text:

- AGENTS.md (Verification): "Never bypass commit hooks, including through `--no-verify`, `-n`, environment variables, or alternative commit creation paths. Fix failed checks or stop without committing."
- User global rule: "Preserve repository verification hooks. Never bypass commit verification."

Plan text (plan.md:101-103): "On a probe branch from the Task 1 head, add `assert_eq!(0.1_f64 + 0.2, 0.3);` ... Open a **draft** PR titled `probe(#26): do not merge`".

The plan does not say how the probe commit is created. The repository's hook rule names "alternative commit creation paths". A commit made through the GitHub API or the web editor would therefore fall under that rule. The probe also deliberately commits code that fails `make check`. That is justified by the issue's acceptance (issue.md:23, "A PR that adds an exact float `assert_eq!` in a `purrassist-core` test fails a hosted pull-request job with `clippy::float_cmp`"): the failing check is the evidence the issue asks for, not a failure left unfixed. The plan should state that justification so the commit cannot be read as an unexplained exception to "Fix failed checks or stop without committing".

REWRITE TO (add to Task 2, step 1):

> "Create the probe commit with a normal local `git commit`, with hooks active (no `--no-verify` or `-n`, no environment override, and no API or web-editor commit). If any hook rejects the commit, stop and report; do not work around it. The probe intentionally fails `make lint` because issue #26's acceptance (issue.md:23) requires that exact PR. It is never merged. Its commit message, PR body and issue comment carry no agent attribution. The PR body references #26 without a closing keyword."

### A3 (ADVISORY): RELEASE.md's description of `check-release.py` must cover the new drift rule

Governing text:

- AGENTS.md:5-6: "tests and public documentation must distinguish implemented behavior from plans."
- CONTRIBUTING.md:19-20: "Capability or performance claims need executable evidence; `docs/QUALIFICATION.md` records the executable evidence and its limits".

Plan text:

- plan.md:88: "`docs/QUALIFICATION.md` and `docs/RELEASE.md` where they describe CI lanes."
- plan.md:36: "`scripts/check-release.py` already validates the shape of release workflows."

Today `check-release.py` validates only `publish.yml` (scripts/check-release.py:112-120). docs/RELEASE.md:18-23 lists exactly what it proves, and that list covers only the release set and `publish.yml`. Task 1 adds assertions over `verify.yml`, `release-qualification.yml` and `Cargo.toml` `rust-version`. RELEASE.md:18-23 is not a CI-lane description, so the plan's docs scope as worded can miss it.

REWRITE TO: "`docs/RELEASE.md`: the 'Release set' paragraph (lines 18-23) that lists what `scripts/check-release.py` proves gains the drift assertion: the `verify.yml` `msrv` job's toolchain equals the release-qualification toolchain and the workspace `rust-version`, and the job runs `make check`. Also update every CI-lane description in RELEASE.md and QUALIFICATION.md:252-253 ('stable MSRV workspace tests')." Also correct plan.md:36 to "already validates the shape of `publish.yml`".

## Checked and compliant

- **Rust-first and core purity** (AGENTS.md Contracts; CONTRIBUTING.md:13): the plan adds no Rust runtime code and no `purrassist-core` dependency. The probe line is never merged.
- **Dependency and tool registration** (AGENTS.md: "Register and justify third-party dependencies, including ... build-only, test-only"): no new dependency. Clippy and rustfmt for 1.98.0 are already registered development tools (docs/DEPENDENCIES.md:86-87), and release-qualification.yml:28 already installs that exact toolchain and its components. No new action is introduced. The existing SHA-pinned checkout with `persist-credentials: false` stays in place.
- **SPDX headers** (AGENTS.md; CONTRIBUTING.md:23-24): the plan creates no new committed source file. Fixtures live inside the existing `scripts/test-check-release.py`. `validation.md` sits in the untracked `.stage/` directory.
- **`--locked` on `make lint`**: this matches the repository convention. Every other cargo recipe in the Makefile (`test`, `corpus`, `doc`, `helper-test`) and every cargo step in CI passes `--locked`.
- **`RUSTFLAGS: -D warnings` kept**: this matches the existing `msrv` and `windows-engines` jobs (verify.yml:39, 50) and CONTRIBUTING.md:18-19 ("Keep formatting and lint warnings clean").
- **Hard fails**: `.goals`:6 applies to the project and explicitly not to code. The new lane fails on any lint, and the drift rule has failing fixtures that run in `make hygiene`. Nothing is made optional.
- **Required status check declined**: no governing document requires one, and branch rulesets are an owner setting. The plan records it as an owner option rather than claiming merge blocking. That is consistent with AGENTS.md's rule against claiming unimplemented behavior.
- **Release and publication rules** (docs/RELEASE.md): the plan does not touch `publish.yml`, the tag, or the publication paths. No tag, release or publish step is involved.
- **No private infrastructure** (AGENTS.md; CONTRIBUTING.md:24-25): `rust-validate` is a local evidence tool and appears in no committed source, fixture or document.
- **Attribution** (user global rule): plan.md:31 adopts "No agent attribution" as a standing constraint. A2 extends it explicitly to the probe artifacts.

## Judgment on the Task 2 probe PR

The probe PR is appropriate, given the A2 guard. Reasons:

- The issue's acceptance (issue.md:23) is defined as a PR that fails a hosted pull-request job. AGENTS.md requires "Capability claims require executable evidence". A local run or a `workflow_dispatch` run would not demonstrate the `pull_request` event path the issue names.
- `verify.yml` runs with `permissions: contents: read` (verify.yml:9-10) and uses no secrets. A same-repository draft PR therefore exposes nothing and cannot publish: `publish.yml` and `release-qualification.yml` trigger only on `rust-v*` tags or dispatch.
- No governing document forbids draft or throwaway PRs on the public repository.
- The probe is labelled draft and do-not-merge, contains only the one test line on top of Task 1, is closed unmerged, and its branch is deleted. AGENTS.md's "Preserve unrelated work" is respected because only the probe's own branch is deleted.

Non-binding note: the probe triggers every `verify.yml` job, including the 90-minute `fuzz` and `qualification` jobs. No written rule limits this. Cancelling the jobs that are not needed once `msrv` concludes would be courteous but is not required.

VERDICT: PASS

---

## Pass 2 (plan revision 2)

Reviewed: the same `plan.md`, now headed "Revision 2". It adds a shared `make release-gate` target. `release-qualification.yml` collapses its steps 4-8 into a single step, `Verify the release gate`. The `verify.yml` `msrv` job is renamed `release-gate` and runs the same target. `check-release.py` gains gate-identity rules (P1) and single-source pin rules (P2), and the docs change is listed as P3.

The governing documents are the same as Pass 1. In addition, this pass read:

- `scripts/check-docs.py:1-40` and `:789-812`: the `release-qualification-steps` rule.
- `scripts/test-check-docs.py:1121-1209`: its tests.
- The docs that mention release-qualification steps: `docs/FUZZING.md:1069-1077`, `docs/QUALIFICATION.md:40-55`, and `README.md:327-329`.

### Prior findings

- **A1 DISCHARGED.** plan.md:127 "Plain `make check` (nightly) exits 0." is now a focused check, alongside `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` (plan.md:126).
- **A2 DISCHARGED.**
  - plan.md:33: "Hooks are never bypassed, including through alternative commit paths."
  - plan.md:134: "a normal local commit; no hooks are installed, and none is bypassed".
  - plan.md:141: "Commit it normally, with no attribution, and with a message saying it is a deliberate failing probe for #26 that must not merge."
  - plan.md:143: "with no closing keyword".
- **A3 DISCHARGED.** P3 (plan.md:100) covers RELEASE.md's `check-release.py` paragraph (lines 18-23) and the qualification-workflow step list. The plan now describes `check-release.py` as validating `publish.yml` (plan.md:47 and 171).

### Release workflow changes: checked and compliant

- **The release-qualification header contract still holds.** The header (release-qualification.yml:4-9) and RELEASE.md:147-148 say the workflow "never publishes, attests, or creates a GitHub release, and it holds no `id-token` permission and no registry credential". The plan changes only the verification steps. The trigger, `permissions: contents: read`, the install steps and the tag/version check stay (plan.md:115). `make release-gate` contains no publishing command.
- **The publish gate is unaffected.** `publish.yml`'s gate polls `release-qualification.yml` by file name and `conclusion: success` (RELEASE.md:160-175). Both the file name and the workflow conclusion are unchanged. `check-release.py:174` still finds the reference.
- **`publish.yml` is untouched.** It is only read for its pins (P2). RELEASE.md:106-107 ("Changing that shape needs a matching change to the check") is not triggered, and plan.md:47 keeps the closed-shape rules off `verify.yml`.
- **The release gate's coverage is preserved.** `make release-gate` holds the five gates release qualification runs today: `check`, `corpus`, `test-fuzz-feedback.py`, `test-verify-published.py` and `package` (release-qualification.yml:42-51). The nightly `llvm-tools` toolchain the feedback test needs is the registered one (verify.yml:96-98; FUZZING.md:1074-1077), and the PR job installs it the same way release qualification does (plan.md:118).
- **The docs stay true.** FUZZING.md:1073-1074 ("release qualification runs it too"), QUALIFICATION.md:50-51 ("`make corpus` runs ... in release qualification") and RELEASE.md:213-214 remain true, because release qualification still runs those gates through `make release-gate`.
- **Hard fails** (`.goals`:6): P1 and P2 turn drift in gates or pins into a `make hygiene` failure and a `check-release.py` failure in `publish.yml` before anything is published. That is the intended direction.
- **The probe is still appropriate.** It now restores the pre-#24 assertion in `crates/core/src/tool_call/from_node.rs` (plan.md:74 and 140). That is the same class of edit inside a core test, with the A2 guards in place. The Pass 1 judgment stands.

### B1 (ADVISORY): Task 1 must also edit `scripts/test-check-docs.py`, which pins the old step name

Governing text:

- AGENTS.md (Verification): "Fix failed checks or stop without committing. A focused test is not the full gate. `make check` is the local verification gate".
- Makefile:17: `make hygiene` runs `scripts/test-check-docs.py`.

Repository convention:

- `scripts/test-check-docs.py:1122-1125` reads the real workflow and asserts `self.assertIn('Verify workspace', names)`.
- `scripts/check-docs.py:794-812` requires every named step of `release-qualification.yml` to appear, backticked, in RELEASE.md's "Qualification workflow (release-qualification.yml)" section.

Plan text:

- plan.md:115: "`.github/workflows/release-qualification.yml`: steps 4–8 collapse into one named step, `Verify the release gate`".
- plan.md:121-122: Task 1's file list names only `check-release.py`, `test-check-release.py` and the docs.

Collapsing the steps removes `Verify workspace`, so `test_the_steps_are_read_from_the_workflow` fails and `make hygiene` and `make check` go red. The `check-docs.py` rule itself is satisfied, as long as RELEASE.md names `Verify the release gate` in backticks inside that section (P3 already requires this). Only its test pins the old name.

REWRITE TO (add to Task 1 files): "`scripts/test-check-docs.py`: `test_the_steps_are_read_from_the_workflow` asserts `Verify the release gate` instead of `Verify workspace`. The rest of `ReleaseQualificationSteps` is unchanged and must pass: each name in the step list (`Install release toolchain`, `Install pinned coverage-feedback toolchain`, `Verify release set and version`, `Verify the release gate`) must appear backticked in RELEASE.md's qualification section."

### B2 (ADVISORY): QUALIFICATION.md must keep recording the evidence behind the MSRV claim

Governing text:

- CONTRIBUTING.md:19-20: "Capability or performance claims need executable evidence; `docs/QUALIFICATION.md` records the executable evidence and its limits".
- AGENTS.md: "Capability claims require executable evidence."

Published claims:

- README.md:281: "MSRV 1.98".
- CHANGELOG.md:90: "minimum supported Rust version (MSRV) 1.98".
- docs/IMPLEMENTATION.md:154: "stable MSRV 1.98".

Plan text (plan.md:100): "`docs/QUALIFICATION.md:252-253` 'stable MSRV workspace tests' is replaced by a sentence naming the `release-gate` job and `make release-gate` on the release toolchain, and the old phrase is gone."

The renamed job is still the only evidence that the workspace builds and tests on its MSRV, because P2 holds the stable pin's `major.minor` equal to `rust-version`. If the replacement sentence names only the release toolchain, QUALIFICATION.md stops recording where the MSRV claim is proved.

REWRITE TO: "QUALIFICATION.md:252-253 names the `release-gate` job, which runs `make release-gate` on the release toolchain 1.98.0, the workspace MSRV, so the same lane is the MSRV build and test evidence. `check-release.py` holds the release pin's major.minor equal to `rust-version`."

### Non-binding note

If `release-gate`'s gates are Makefile prerequisites (plan.md:98: "lists `check corpus`"), a parallel `make -j` could run them in a different order from the release step list in RELEASE.md. `MAKEFLAGS` sets no `-j` (Makefile:3), and no rule forbids this, so it is not a finding. Sequential `$(MAKE)` recipe lines would make the documented order hold by construction.

VERDICT: PASS
