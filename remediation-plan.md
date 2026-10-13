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
