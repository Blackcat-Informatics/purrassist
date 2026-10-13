# Review: Stage 2 remediation plan, issue #26 / PR #28

- **Plan reviewed:** `.stage/lint-the-whole-workspace-with-the/remediation-plan.md`
- **Evidence read:** `gap-analysis.md`, `completion-audit.md`, `historical-fit.md`, `plan.md`; worktree `Makefile`, `.github/workflows/{verify,release-qualification,publish,mcp-registry}.yml`, `scripts/check-release.py`, `scripts/test-check-release.py` (structure), `scripts/check-docs.py` (`load_module`, `workflow_steps`, `block_steps`), `crates/core/Cargo.toml`, `docs/RELEASE.md`, `docs/QUALIFICATION.md`, `README.md`.
- **Focus applied first:** does each fix close its gap on the real entry point (`make hygiene`/`make check` -> `check-release.py`; the hosted `release-gate` job), are checks falsifiable, does any fix narrow scope or add false positives.

## Coverage

| Source finding | Plan item | Closed on the real entry point? |
|---|---|---|
| H1 | G1 | Yes for `RUSTUP_TOOLCHAIN`. The RELEASE.md clause from H1's fix (item 3) is dropped (B7). |
| M1 | G2 | Yes for `lint`. `test` from M1's fix is dropped (A3). |
| M2 | G3 | Partly. The denylist is narrower than M2's fix and misses equivalent bypasses (B1). M2's "close the verify job's step list" is dropped (B2). |
| M3 / A2 | G5 | Yes (A8 has refinements). |
| L1 | G6 | Yes. |
| L2 | G6 | No. The stated parse rule misreads the plan's own fixture (B4). |
| L3 | G6 | Partly. Inline `RUSTUP_TOOLCHAIN=` assignments from the gap's L3 are dropped (A5). |
| L4 | G7 | It adds a false positive on a historical record (B6). README and the `verify.yml:36` comment are not covered (A6). |
| L5, L6, L7 | G8 | The L6 check is vacuous (B8). The L7 mechanism is unspecified (A7). |
| A1 (audit) | G4 | Yes for selection. The second lint line is not pinned (B5). |
| A3 (audit) | (none) | **Missing.** The `pull_request:` trigger is not pinned (B3). |
| A4 (audit) | "Not a branch gap" | Correct. |

## Answers to the caller's specific questions

**G3 against the real `release-qualification.yml`.** As worded, the bans are scoped to the two gate jobs and target YAML keys, so the tag/version step passes:

- it has no `if:` key, no step `env:`, and no `GITHUB_ENV` or `GITHUB_PATH` token;
- `if [[ "$GITHUB_REF_TYPE" == tag ]]` is shell inside a `run: |` block, and `$GITHUB_REF_TYPE`/`$GITHUB_REF_NAME` are different names.

Two things are still wrong:

- **Detection is unspecified.** A substring or loose regex such as `\bif\b` or `GITHUB_[A-Z_]+` would false-positive on that step. Scoping by file instead of by job would false-positive on `verify.yml`'s `>> "$GITHUB_PATH"` lines in `native`, `wasm`, `fuzz` and `qualification`, on `publish.yml:83` (`>> "$GITHUB_ENV"`) and on `mcp-registry.yml:28` (job `if:`). See A4.
- **The version step's `shell: bash` points at an unlisted bypass.** `shell:` on the *gate* step (`shell: true {0}` is a valid custom shell) makes the step succeed without running `make`. The plan neither bans it nor allowlists it per step. See B1.

**G4: does `cargo clippy --test <name>` lint `test = false` targets?** Yes:

- Explicit target selection ignores the `test` default-selection flag; the flag only controls what `--tests`/`--all-targets` pick up.
- The repository already relies on this for the same eight targets: `Makefile:33` (`cargo test -p purrassist-core --locked $(foreach suite,$(CORPUS_SUITES),--test $(suite))`), run by the hosted `corpus` job.
- None of the eight `[[test]]` entries (`crates/core/Cargo.toml:67-105`) has `required-features`.
- The other workspace manifests contain no `test = false`.

The selection mechanism is sound. The gaps are that the new line is not pinned (B5) and the suite list is duplicated (A1).

**G6 literal-pin rule against existing workflows.** No false positive today:

- every `RUSTUP_TOOLCHAIN:` value and every install name in `verify.yml`, `release-qualification.yml` and `publish.yml` is literal;
- `${{ matrix.os }}` (`verify.yml:17`) and the `${{ github.* }}` uses in `publish.yml` are not in pin positions.

The rule as written is wrong in another way: the "first argument after the install verb that doesn't start with `-`" rule reads `minimal` from `--profile minimal 1.98.0`. That is the B7 probe the plan says will now pass (B4).

## Findings

### B1 BLOCKING: G3's denylist is narrower than M2's fix and leaves equivalent bypasses open

**Plan text:**

> - a step-level `env:` that sets `RUSTUP_TOOLCHAIN` or `RUSTFLAGS`;

**Narrowing.** The gap-analysis M2 fix says "reject step-level `env:`" (all of it). Limiting the ban to two variable names leaves other variables that weaken or redirect the build:

- `CARGO_ENCODED_RUSTFLAGS: --cap-lints=warn` (it overrides `RUSTFLAGS`), `CARGO_BUILD_RUSTFLAGS`, `CARGO_TARGET_<triple>_RUSTFLAGS`;
- `RUSTC`, `RUSTC_WRAPPER`, `CARGO`;
- `MAKEFLAGS: -i`.

No step in either gate job has an `env:` today, so the full ban costs nothing.

**Uncovered modifiers that bypass the gate:**

- **Step `shell:`.** On the gate step, `shell: true {0}` gives success without the gate. Step `working-directory:` can point the gate at another Makefile.
- **Job `defaults: run: shell/working-directory`.** Same effect, applied to every step.
- **Workflow-level `env:` and `defaults:`.** `_job_env` (`check-release.py:281-294`) reads only the 4-space job block, so a top-level `env: RUSTFLAGS: --cap-lints=warn` in `verify.yml` reaches the PR gate unseen.
- **Job-level env keys other than `RUSTUP_TOOLCHAIN`.** G1 checks only that key. The existing check is only env *equality* (`check-release.py:379`), so adding `RUSTFLAGS: --cap-lints=warn` to both jobs passes. This is gap probe A6 moved up one level and mirrored in both files.

**REWRITE TO:**

> In both gate jobs, use allowlists, not denylists.
>
> - **Workflow level:** neither `verify.yml` nor `release-qualification.yml` may have top-level `env:` or `defaults:`.
> - **Job keys:** exactly `runs-on`, `timeout-minutes`, `env`, `steps`.
> - **Job `env`:** exactly `{RUSTUP_TOOLCHAIN: <stable pin>}`.
> - **Step keys:** read at the step key column with `check-docs.py`'s `block_steps`.
>   - checkout: `uses`, `with`;
>   - installs: `run`, optionally `name`;
>   - version step: `name`, `shell: bash`, `run`;
>   - gate step: `run`, optionally `name`.
> - **Tokens:** no `GITHUB_ENV` or `GITHUB_PATH` token anywhere in either gate job.
>
> Anything else fails with "gate job `<job>` may not set `<key>`".
>
> Fixtures, each of which must fail:
> - `shell: true {0}` on the gate step;
> - job `defaults:`;
> - workflow-level `env: RUSTFLAGS`;
> - job-level `RUSTFLAGS` added to both jobs;
> - step `env: CARGO_ENCODED_RUSTFLAGS`.
>
> The real files must pass.

### B2 BLOCKING: G3 drops M2's "close the verify job's step list"

**Plan text:** G3 lists only the four bans. The gap-analysis M2 fix's second bullet ("Close the verify job's step list as well: checkout, the two installs, then `make release-gate`") does not appear in G3 or anywhere else in the plan.

**Consequence:**

- The release job's step list is closed (`check-release.py:357-362`). The verify job's is not: `check-release.py:372-376` checks only its `make` lines.
- So an extra verify step that runs before the gate can weaken the PR lane only, for example:
  - writing `.cargo/config.toml` with `[build] rustflags = ["--cap-lints=warn"]`;
  - `sed`-editing the Makefile.
- It passes every planned rule: no `if:`, no env, no `GITHUB_ENV`.
- That is lane divergence, the defect class of #26.

**REWRITE TO:**

> G3 also closes the verify `release-gate` step list. Read with `workflow_steps`, it is exactly:
> - the checkout;
> - the stable install;
> - the nightly install;
> - one step whose only command is `make release-gate`.
>
> Fixture: an extra `run:` step before the gate in `verify.yml` fails.

### B3 BLOCKING: completion-audit A3 (the trigger) is not addressed, though the plan says "Every finding is fixed"

**Plan text:**

> Every finding is fixed. None is deferred, and there are no user scope decisions.

**Evidence.** Audit A3 has two parts:

- "`verify.yml` keeps `pull_request:` in its triggers": nothing in the plan covers this.
- no `if:`/`continue-on-error:` on the job: G3 covers this.

**Consequence:** removing `pull_request:` from `verify.yml:7`, or adding a `paths:`/`branches:` filter to it, removes or narrows the PR lane. The issue's property ("fails on the PR that introduces it") is then gone, while `check-release.py` stays green.

**REWRITE TO:**

> **G3 (add):** `validate_release_gate` requires that `verify.yml`'s `on:` contains `pull_request:` with no `branches`, `branches-ignore`, `paths` or `paths-ignore` filter.
>
> Fixtures that must fail:
> - `pull_request:` removed;
> - `pull_request: {paths: ['docs/**']}`.

### B4 BLOCKING: G6's L2 parse rule misreads the plan's own "flags before the name" fixture

**Plan text:**

> - **L2:** a pin is the first argument after the install verb that doesn't start with `-`, with `\` line continuations joined. A non-literal pin (`${{ … }}`) is an explicit error

and the check:

> These fixtures now pass: … flags before the name; …

**Consequences:**

- **The rule contradicts its fixture.** For `install --profile minimal 1.98.0`, the first token without a leading `-` is `minimal`. `minimal` is literal and doesn't start with `nightly`, so it becomes a stable pin, and the fixture fails with "stable toolchain pins differ". The same happens with `--component clippy 1.98.0`.
- **The grammar check is dropped.** The rule omits the gap-analysis L2 fix's grammar check (`^(nightly-\d{4}-\d{2}-\d{2}|\d+\.\d+\.\d+)$`).
- **Other non-literal pins are silently skipped.** "Non-literal" is defined only as `${{ … }}`. A shell-variable pin such as `install "$TOOLCHAIN"` is not read at all today, because `INSTALL_PIN`'s `[^\s'"]+` cannot start at a quote. It stays silently skipped.
- **Only one pin is read per line.** `install 1.98.0 1.97.0` installs two toolchains; the rule reads the first only.

**REWRITE TO:**

> **L2:** after joining `\` continuations, tokenize the arguments after `rustup toolchain install` and skip options and their values:
> - `--profile`, `-c`/`--component` and `-t`/`--target` take a value, in both the separate and the `=` form;
> - other `--flags` take none.
>
> Every remaining positional, with quotes stripped, is a pin and must match `^(nightly-\d{4}-\d{2}-\d{2}|\d+\.\d+\.\d+)$`. A `RUSTUP_TOOLCHAIN:` value must match the same pattern. Anything else in a pin position fails with "toolchain pins must be literal: `<token>` at `<file>:<line>`". That covers `${{ … }}`, `$VAR`, and bare channel names `stable`, `beta` and `nightly`.
>
> Fixtures that pass: `--profile minimal 1.98.0`; `--component=clippy,rustfmt 1.98.0`.
>
> Fixtures that fail: `"$TOOLCHAIN"`; `stable`; a second positional `1.97.0`.

### B5 BLOCKING: G4's second lint line is not pinned, so the new capability has the same hole H1 had

**Plan text:**

> - P1 asserts the first line is exactly the issue's command.
> - A new rule derives the set of `test = false` `[[test]]` targets … and requires each to appear as `--test <name>` in the lint recipe.

**Consequence.** Nothing pins the second line's other flags. Each of these meets both planned rules while the corpus-suite lints no longer fail:

- `cargo clippy -p purrassist-core --test a … ` with no `-- -D warnings`;
- the same line with a leading `-` (make ignores the failure);
- the same line with `|| true`.

`LINT_RECIPE` (`check-release.py:245`) is a tuple compared with `!=` (`:407`). The plan needs to say the tuple becomes two exact lines.

**REWRITE TO:**

> `LINT_RECIPE` becomes exactly two lines:
> 1. the issue's command;
> 2. `cargo clippy -p purrassist-core --locked <suite args> -- -D warnings`.
>
> The derived-set rule checks that `<suite args>` names exactly the `test = false` targets (A1 has the form).
>
> Fixtures that must fail:
> - line 2 without `-- -D warnings`;
> - line 2 with a leading `-`;
> - line 2 with `|| true`.

### B6 BLOCKING: G7's `Rust <x>` rule false-positives on a historical record and would force rewriting it on every bump

**Plan text:**

> a `check-docs.py` rule requires every `RUSTUP_TOOLCHAIN=<x>` and `Rust <x>` stable-version mention in `docs/RELEASE.md` and `docs/QUALIFICATION.md` to equal the stable pin.

**Evidence:**

- `docs/RELEASE.md:324` is part of the 0.1.0 publication record that `historical-fit.md` §2 says is "retained and not edited". It reads "`464306c` failed release qualification (stable Rust 1.98 Clippy".
  - `1.98` is not equal to the pin `1.98.0`, so the rule fails on the real file today, unless "equal" is quietly redefined.
  - After a bump to 1.99.0, the rule would demand editing history.
- The two-component form also appears as "MSRV 1.98" (`QUALIFICATION.md:260`). It is a `rust-version` fact, not a pin.

The plan's own check ("The real files pass") would fail, or the implementer would improvise.

**REWRITE TO:**

> The rule checks:
> - every `RUSTUP_TOOLCHAIN=<x>`, and every three-component `Rust <x.y.z>`, against the stable pin;
> - every two-component `Rust <x.y>` against Cargo `rust-version`.
>
> It applies only outside dated publication-record sections. Name the excluded RELEASE.md heading explicitly, or scope the rule to the "Qualification workflow" section and the QUALIFICATION.md command list.
>
> Fixtures:
> - a pin bump without a doc bump fails;
> - the real files pass, including `RELEASE.md:324`.

### B7 BLOCKING: the docs that describe the guard are not updated for the new rules

**Evidence:**

- The gap-analysis H1 fix, item 3, and its recommended action 1 both require RELEASE.md clauses for the env pin and for `check -> lint`. The plan's G1 and G2 omit them.
- G4 makes RELEASE.md false in two places:
  - `docs/RELEASE.md:28` says "`make lint` is the `--locked` Clippy command" (one command);
  - `:139-140` describe Clippy as only `--workspace --all-targets --locked -- -D warnings`.
- Comments go stale too:
  - `Makefile:28-30` ("`make check` does not");
  - `crates/core/Cargo.toml:66` ("keeps them out of … `make check`").

  After G4, `make check` lints these suites; it still doesn't run them.
- AGENTS.md: "tests and public documentation must distinguish implemented behavior from plans".

**REWRITE TO (add to commits 1 and 2):**

> Update `docs/RELEASE.md:23-31` to state each new rule:
> - the gate jobs' `RUSTUP_TOOLCHAIN` equals the stable pin;
> - `check` requires `lint` and `test`;
> - the gate jobs have a closed key, env and step shape;
> - `verify.yml` keeps an unfiltered `pull_request:` trigger;
> - `make lint` also lints the `test = false` suites.
>
> Update `:139-140` to describe both lint lines.
>
> Reword the `Makefile:28-30` and `crates/core/Cargo.toml:66` comments: the suites are linted by `make check` but run only by `make corpus`.
>
> Check: `check-docs.py` passes, and `grep -n 'the --locked Clippy command' docs/RELEASE.md` returns nothing.

### B8 BLOCKING: G8's L6 check is vacuous

**Plan text:**

> **Check:** `grep -n 'release gate' README.md Makefile` shows no `fuzz-hosted` mention.

**Evidence:** the offending lines say "release *fuzz* gate":

- `README.md:290`: "the CI and release fuzz gate";
- `Makefile:83`: "The hosted and release fuzz gate".

The pattern `release gate` matches neither, so the check passes on the unfixed tree. Nothing checks the README `make release-gate` line either.

**REWRITE TO:**

> `grep -nE 'release (fuzz )?gate' README.md Makefile` returns no line that describes `fuzz-hosted`, which today means `README.md:290` and `Makefile:83` are gone, and `grep -c 'make release-gate' README.md` is 1.

## Advisory findings

### A1 ADVISORY: G4 duplicates the suite list; derive-check `CORPUS_SUITES` instead

**Plan text:** "with one `--test` per suite" plus a rule that each name appears in the lint recipe.

**Problem:**

- `Makefile:31` already defines `CORPUS_SUITES`, and nothing checks it against `Cargo.toml`: `grep CORPUS_SUITES scripts/` is empty.
- A new `test = false` suite missing from `CORPUS_SUITES` is never *run* by `make corpus` either.
- A second literal list adds a third copy of the same fact.

**Option:**

- Check that `CORPUS_SUITES` equals the derived set.
- Write lint line 2 as `cargo clippy -p purrassist-core --locked $(foreach suite,$(CORPUS_SUITES),--test $(suite)) -- -D warnings`, the same form as `corpus`, and pin it literally (B5).

**Cost:** same size. **Payoff:** one list, and `make corpus` drift is caught too.

If a non-core member ever gains `test = false` targets, require a per-package line. The planned `-p purrassist-core` line cannot select another package's test.

### A2 ADVISORY: G4's effect on the nightly lane and on `make check` time

`native` runs `make check` on nightly, so it will now lint the corpus suites on nightly too. Check-mode Clippy on eight integration tests is cheap compared with running them (the `fecda89` concern was run time, not compile time). Still:

- record the before and after `make check` time in `validation.md`;
- disclose the broadening in the PR body (A8).

### A3 ADVISORY: G2 pins `lint` only

The gap-analysis M1 fix says "`'lint'` (and `'test'`)". `make test` is the "MSRV 1.98 build-and-test evidence" (`QUALIFICATION.md:259-260`), and dropping it from `check:` passes today.

**REWRITE TO:**

> P1 asserts that `check`'s prerequisites include `lint` and `test`, with a fixture for each.

Optionally, reject `.IGNORE` and `-i`/`--ignore-errors`/`-k` in `MAKEFLAGS` in the Makefile. These would also make Clippy failures non-fatal in every lane.

### A4 ADVISORY: specify G3's detection and add per-lane fixtures

- Detect keys at the job column (4 spaces) and the step key column (`block_steps`), never inside `run:` bodies.
- Detect `GITHUB_ENV`/`GITHUB_PATH` as exact tokens.
- Scope everything to the two gate jobs.
- The planned fixtures omit gap probe A7, `continue-on-error: true` on the release gate *step*. That is the probe that lets `publish.yml:46` accept a run where the gate failed. Add it, and the equivalent for the verify gate step.
- Add a positive fixture: the unmodified version step (`shell: bash`, `$GITHUB_REF_TYPE`) passes.

### A5 ADVISORY: G6 L3 omits part of the gap's L3 and the full workflow set

- **Inline assignments.** The gap's L3 names inline `RUSTUP_TOOLCHAIN=… cmd` assignments in `run:` lines. The plan omits them.
- **Install aliases.** `rustup install <x>` is an alias of the install verb and is not read.
- **Workflow files.** `WORKFLOW_FILES` (`check-release.py:239`) lists three files. "In all workflows" should mean a glob of `.github/workflows/*.y*ml`, so that `mcp-registry.yml` and any future workflow are scanned.

The glob introduces no false positive today: `mcp-registry.yml` has no Rust pins, and its `GITHUB_PATH` write and job `if:` sit outside the gate jobs.

### A6 ADVISORY: G7's doc set

- G8 adds a README `make release-gate` line. If it is written as `RUSTUP_TOOLCHAIN=1.98.0 make release-gate`, as the gap-analysis L6 fix suggests, README.md must join G7's file set, or the plan creates a new stale pin.
- The gap's L4 also lists the `verify.yml:36` comment ("stable 1.98.0"). The checker ignores comments, so either reword that comment so it carries no version, or include it.

### A7 ADVISORY: G8 L7 has no implementable mechanism

**Plan text:**

> the duplicated module loader is replaced by the existing one, with one definition used by both call sites.

**Problems:**

- `check-release.py` cannot call `check-docs.load_module` without first loading `check-docs.py`, which needs a loader. That is circular.
- Loaders also exist in `test-check-release.py:33`, `verify-published.py:53`, `test-fuzz.py:18` and `test-repository-audit.py:16`, so "both call sites" is ambiguous.
- The gap analysis rated L7 "optional; acceptable at this size".

**Fix:** either name the mechanism (a shared SPDX-headed `scripts/` helper imported through `sys.path[0]`, and which callers move to it), or record L7 as consciously not changed, with the gap's justification. Leaving the implementer to improvise is the worse option.

### A8 ADVISORY: G5 body contents

**Problems:**

- "Link the R1, probe and final-head runs" does not say which probe. The final-head evidence is the new `probe/26-float-cmp-2` run, so link both probe runs.
- The gap's M3 fix also keeps the required-check note.
- The body should disclose that `make lint`, and therefore `make check` in every lane, now also lints the corpus suites. That is a scope broadening the owner should see, like the whole-gate broadening.

**Fix:** run the check after the final push. Add "mentions the required-check position and the corpus-suite lint" to the check.

## Notes, not findings

- The "Commit boundaries" item says "because G3 and G4 change `verify.yml` and the `Makefile`". As planned, G3 changes only `check-release.py`, while G4 and G8 change the Makefile. The probe rerun is required either way, so the conclusion stands.
- The G1, G2 and G4 checks are falsifiable as written: named-error fixtures, `rust-validate 1.98.0 make lint` exits 0, and a local corpus-suite probe that must fail line 2. The final-head hosted probe on `probe/26-float-cmp-2` is correctly required.

Pass 1 verdict: BLOCKED

---

## Pass 2 (remediation plan revision 2)

Scope of this pass:

- the prior findings, each marked DISCHARGED or STILL-OPEN;
- only the new text from revision 2, checked against the worktree files.

Facts re-checked in the worktree:

- `docs/RELEASE.md:318` is exactly `## 0.1.0 publication record`.
- `release fuzz gate` occurs only at `README.md:290` and `Makefile:83` (`.stage/` excluded).
- Root `Cargo.toml:5` lists `members` literally, with no globs, so a `tomllib` walk of the members is direct.
- Under the new allowlists, both gate jobs today have exactly the keys `runs-on`, `timeout-minutes`, `env` and `steps`, and an env of only `RUSTUP_TOOLCHAIN`. Both workflows' top-level keys are within `{name, on, permissions, jobs}`, and `verify.yml:7` is a bare `pull_request:`. So the real files pass the new rules.

### Prior findings

| Id | Status | Evidence in revision 2 |
|---|---|---|
| B1 | DISCHARGED, apart from N1 | G1 lines 13-15 and 23-57 add allowlists at three levels: workflow keys ⊆ {`name`, `on`, `permissions`, `jobs`, `concurrency`} with no `env`/`defaults`; job keys exactly {`runs-on`, `timeout-minutes`, `env`, `steps`}; env exactly `RUSTUP_TOOLCHAIN`. Step keys ⊆ {`name`, `uses`, `with`, `run`}, plus `shell: bash` on the version step only. Fixtures (lines 61-63) cover job `RUSTFLAGS` in both jobs, step `CARGO_ENCODED_RUSTFLAGS`, workflow `env`, job `defaults`, `shell:` on the gate step, and step and job `continue-on-error`. The step-key allowlist also excludes `working-directory`. The `with:` contents are not constrained (N1). |
| B2 | DISCHARGED | Lines 42-52 give closed step lists for both jobs. Line 63 has a fixture: an extra step that writes `.cargo/config.toml`. |
| B3 | DISCHARGED | Line 30 requires `pull_request:` with no keys under it. Line 64 has fixtures for removing it and for adding `paths:`. Flow-style spelling is A9. |
| B4 | DISCHARGED for the reported defects | Line 116 skips value-taking flags by name. Line 128 adds the format regex. Line 129 makes `${{ … }}` and `$VAR` errors. The line 133 fixture (`--profile minimal 1.98.0`) now matches the rule. Residuals are in A10. |
| B5 | DISCHARGED | Lines 76-83: `lint` is exactly two lines, the second reusing `CORPUS_SUITES`. `CORPUS_SUITES` equals the derived `test = false` set (line 84). Fixtures (lines 88-92) cover dropping `-- -D warnings`, appending `\|\| true`, a leading `-`, a removed suite, and an unlisted new target. |
| B6 | DISCHARGED | Line 144 restricts the rule to `RUSTUP_TOOLCHAIN=` and patch-form `Rust x.y.z`. Line 146 excludes `## 0.1.0 publication record`, which exists verbatim at RELEASE.md:318. The real patch-form mentions (RELEASE.md:129, :132, :150; QUALIFICATION.md:251, :259) all equal 1.98.0, so the real files pass. |
| B7 | DISCHARGED, with A11 | Lines 150-154 enumerate updates to RELEASE.md (around 28 and 139-140), the `Makefile:28-30` comment and the `crates/core/Cargo.toml:66` comment. |
| B8 | DISCHARGED | Line 177's `grep -rn 'release fuzz gate' README.md Makefile docs/` matches `README.md:290` and `Makefile:83` today, so the check is falsifiable. No file under `docs/` contains the phrase, so no historical text is forced to change. The README `make release-gate` line has no check (A11). |
| A1 | DISCHARGED | `CORPUS_SUITES` is reused and derive-checked (lines 79 and 84). |
| A2 | Partly | The PR body discloses the corpus lint (line 107). Recording the `make check` timing is not mentioned; that is acceptable. |
| A3 | DISCHARGED | G2 pins both `lint` and `test` (lines 70-72). |
| A4 | DISCHARGED | Keys are read structurally (line 21) and scoped to the gate jobs. The step-level `continue-on-error` fixture is at line 62. |
| A5 | DISCHARGED | Lines 116-124 cover plain `install`, inline `RUSTUP_TOOLCHAIN=`, `cargo +X`, and every workflow file. No false positive: `publish.yml` and `mcp-registry.yml` contain none of these forms with a non-pin value. |
| A6 | Partly | README joins the doc set (line 144). The `verify.yml:36` comment ("stable 1.98.0") is still unmentioned (A11). |
| A7 | DISCHARGED | Line 179 records L7 as left as is, with a reason, as the gap analysis allowed. |
| A8 | DISCHARGED | Lines 101-110 link both probe runs and add the corpus-lint and required-check notes. |

### New findings on the revision 2 text

#### N1 BLOCKING: the allowlist leaves the checkout step's `uses:` and `with:` contents open, so the PR lane can check out different code

**Plan text:**

> - Step keys ⊆ {`name`, `uses`, `with`, `run`}, plus `shell: bash` on `Verify release set and version` only.

and, for the release job, only:

> 1. the checkout;

**Consequence:**

- `with:` is allowed but its contents are not. `actions/checkout` takes `ref:` and `repository:` inputs.
- `with: { persist-credentials: false, ref: main }` on the verify `release-gate` checkout makes the PR lane lint and test `main` instead of the PR head. Every planned rule still passes, and the PR's new stable-only lint never meets the 1.98.0 Clippy.
- `uses:` on the checkout is also unconstrained. It could name a different action.
- This is lane divergence, the issue's defect class. It contradicts line 15's claim that the allowlist "closes every known and future modifier bypass".
- The verify list (line 43) hints at `with: persist-credentials: false`, but it does not say "exactly". The release list (line 48) says nothing about it.

**REWRITE TO (G1, gate step lists):**

> In both gate jobs, the checkout step's `uses` matches `actions/checkout@<40-hex SHA>`, and its `with` is exactly `{persist-credentials: false}`. No other step has `uses` or `with`.
>
> Fixtures that must fail:
> - `ref: main` added to the verify checkout's `with:`;
> - `uses:` changed to another action.

#### A9 ADVISORY: flow-style trigger spellings

**Plan text:**

> - `on` contains `pull_request:` with no keys under it

An indentation reader can miss `pull_request: {paths: [docs/**]}` on one line, which has no keys *under* it, or `on: [push, pull_request]`.

**REWRITE TO:**

> the `pull_request:` line has nothing after the colon except an optional comment, and `on:` is a block mapping. Any other spelling fails.

Optionally, add the one-line flow-map fixture.

#### A10 ADVISORY: B4 residuals in the extraction rule

**Plan text:**

> flags and their values are skipped (`--profile X`, `--component X`, `-c X`, `--target X`, `-t X`). The pin is the first remaining argument.

**Problems:**

- **The `=` forms are unspecified** (`--component=clippy`).
- **Boolean flags are unspecified** (`--force`, `--no-self-update`, `--allow-downgrade`). They must not consume the next token.
- **Only the first positional is read.** `rustup toolchain install 1.98.0 1.97.0` hides the second pin.

**REWRITE TO:**

> - Only the five named options consume a value, and only in the separate form.
> - A `--opt=value` token, or any other dash token, consumes nothing.
> - Every remaining positional is a pin.
>
> Fixtures: `--component=clippy 1.98.0` passes, and a second positional `1.97.0` fails.

#### A11 ADVISORY: small doc gaps in G6 and G8

- **RELEASE.md:29-30** says "every stable pin in the three workflows". After G5 scans every workflow file, "three" is stale. Add it to the B7 list.
- **G2 clause.** The B7 RELEASE.md update should also state the G2 rule: `check` requires `lint` and `test`.
- **`verify.yml:36` comment.** It still hard-codes "stable 1.98.0" and is invisible to both checkers. Reword it so it carries no version.
- **README check.** Add `grep -c 'make release-gate' README.md` = 1 to G8's check.

#### A12 ADVISORY: `CORPUS_SUITES` must have a single definition

The derive check should require exactly one `CORPUS_SUITES :=` line, and no `+=`, `override` or second assignment. Otherwise the checked text and the value Make expands can differ.

### Pass 2 summary

- B1–B8 and the pass 1 advisories are discharged, apart from A2's timing note and A6's comment, which carry forward as advisories.
- One new blocking gap remains: N1, the checkout `with:`/`uses:` contents. The rewrite is one rule and two fixtures.
- A9–A12 are advisory.

Pass 2 verdict: BLOCKED

---

## Pass 3 (remediation plan revision 3)

Scope of this pass:

- the pass 2 findings, each marked DISCHARGED or STILL-OPEN;
- only the new text from revision 3, checked against the worktree workflows.

### Prior findings

| Id | Status | Evidence in revision 3 |
|---|---|---|
| N1 | DISCHARGED | Line 54: in both gate jobs, `uses:` appears only on the checkout step and must match `actions/checkout@<40-hex SHA>`, and `with:` is exactly `{persist-credentials: false}`. No other step may have `uses:` or `with:`. Line 66 adds fixtures: `ref: main`; a tag-pinned or different action, in either job. The real files pass: both checkouts are `actions/checkout@11d5960a326750d5838078e36cf38b85af677262` with only `persist-credentials: false` (`release-qualification.yml:24-26`, `verify.yml:44-46`), and no other gate step has `uses:` or `with:`. |
| A9 | DISCHARGED | Line 30 requires the bare `pull_request:` form and rejects the flow forms. Line 65 has a flow-form fixture. `verify.yml:7` is bare. |
| A10 | DISCHARGED | Line 119: the value-taking flags consume their argument, `--opt=value` and boolean flags consume nothing, and every positional is a pin. On every real install line, only the toolchain name remains as a positional (for example `--target wasm32-wasip1,wasm32-unknown-unknown` is consumed at `verify.yml:73` and `:123`). |
| A11 | DISCHARGED | Line 155 changes "three workflows" to "every workflow" and adds the `check` -> `lint`/`test` clause. Line 156 removes the version from the `verify.yml:36` comment. Line 183 adds the README `make release-gate` grep. That grep is falsifiable: README's command list has no such line today. |
| A12 | DISCHARGED | Line 86: exactly one `CORPUS_SUITES :=`, and no `+=`, `override` or `?=` assignment. |
| A2 (timing) | DISCHARGED | Line 203 records the before and after `make check` time, with a 348 s nightly baseline. |
| A6 (`verify.yml:36`) | DISCHARGED | Line 156. |

### New findings on the revision 3 text

#### A13 ADVISORY: anchor the "plain `install`" verb to `rustup`, because every workflow is now scanned

**Plan text (line 119):**

> After the install verb (`toolchain install` or plain `install`), value-taking flags consume their next argument …

**Risk:** G5 now scans every workflow file and treats every positional as a pin. Read literally, "plain `install`" also matches:

- `verify.yml:131`: `sudo apt-get install --yes --no-install-recommends libarchive-tools`. `libarchive-tools` would become a malformed pin and fail the real file.
- `verify.yml:32`, `:75`, `:107`, `:125`, `:127`: `scripts/install-qualification-tool.py`. A `\binstall\b` match hits inside the hyphenated file name.

The existing `INSTALL_PIN` requires the `rustup` prefix, so a careful implementation is fine. The real-file pass inside `make hygiene` would also catch the mistake before commit.

**REWRITE TO:**

> After `rustup toolchain install` or `rustup install` (the `rustup` token required) …
>
> Add a positive control: the unmodified `verify.yml`, with its `apt-get install` and `install-qualification-tool.py` lines, passes.

#### Note, not a finding

Lines 203-205 split the Exit list: a stray paragraph sits ahead of the first bullet. This is cosmetic.

### Pass 3 summary

- Every blocking finding from passes 1 and 2 is discharged, and every advisory is adopted or discharged.
- The new text adds no blocking issue.
- A13 is a precision note on the extractor's verb anchor. The real-file run in `make hygiene` already guards it.
- The fixes close their gaps on the real entry points: `check-release.py` through `make hygiene`/`make check`, `make lint` through `make release-gate` in both lanes, and the hosted `release-gate` job with the G7 probe rerun.
- The acceptance checks are falsifiable.

VERDICT: PASS
