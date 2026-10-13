# Plan design review (SOLID / ETHOS): issue #26

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/26-lint-the-whole-workspace-with-the/.stage/lint-the-whole-workspace-with-the/plan.md`

## Code read

- `.github/workflows/verify.yml` (all jobs; `msrv` 35-45, `windows-engines` 46-60, `native` 12-34)
- `.github/workflows/release-qualification.yml` (env line 22, install line 28, `make check` line 43)
- `Makefile` (`check` 6, `hygiene` 11-20, `lint` 22-23, `test` 26, `doc` 36, `helper-test` 94-96)
- `scripts/check-release.py` (`validate_publish_workflow` 111-235, `main` 238-275)
- `scripts/test-check-release.py` (`check_copy` 79-88, `gate_with` 110-115, fixture style)
- `scripts/check-repository.py` (subprocess and tooling.json uses, 267-440), `scripts/test-fuzz-feedback.py:28`
- `Cargo.toml` (`rust-version = "1.98"` line 10, workspace lints 25-34), `rust-toolchain.toml`
- `crates/core/src/tool_call/from_node.rs:322-343` (the #24 fix site)
- `docs/RELEASE.md:118-150, 305-312`, `docs/QUALIFICATION.md:44, 145`
- `vendor/tooling.json` (no toolchain entries), repository grep for `rust-validate` (no matches)

## Judgments the caller asked for

**Full `make check` in `msrv` rather than clippy alone: sound.** Every tool `make check` needs on
1.98.0 is what the plan installs. `Makefile:6-36` runs `cargo fmt` (rustfmt), nine Python scripts
(python3 is on `ubuntu-24.04`; the `native` job already calls it unconditionally at
`verify.yml:27`), `cargo run -p purrassist-helper-census` (a workspace member the current `msrv`
job already compiles via `cargo test --workspace`), clippy, test and doc. No `make check` target
or hygiene script invokes nightly: the only nightly requirement in `scripts/` is
`test-fuzz-feedback.py:28`, which is outside `make check`. The decisive evidence is that
`release-qualification.yml:28,43` already runs this exact `make check` on
`1.98.0 --profile minimal --component clippy,rustfmt` and passed for the published 0.1.0
(`docs/RELEASE.md:305-312`). Runtime becomes that of the `native` ubuntu job, which already runs
`make check` with no `timeout-minutes`; no new failure mode. Running only clippy would leave
`make check` on 1.98.0 unexercised by PRs, which is the gap the issue reports.

**Drift rule in `check-release.py`: sound placement, with two specification gaps (A1, A2).**
`check-release.py:111-235` is the only script that validates workflow shape, it already uses the
job-body bounding technique (`text.split('\n  publish:\n')` then `re.search(r'\n  [^\s#]')`,
lines 129-137), and both it and its tests run in `make hygiene` (`Makefile:14-15`) and in
`make package` (`Makefile:48`). `check-repository.py` never parses workflows, and
`vendor/tooling.json` registers neither toolchain, so there is no existing single source of truth
to reuse instead.

**RUSTFLAGS superset: sound.** `release-qualification.yml` sets no `RUSTFLAGS`; the plan keeps
`-D warnings` in `msrv`. The only direction this can differ is PR-stricter. In practice the
surface is near zero: `make lint` already passes `-D warnings` to clippy-driver, and
`Makefile:26,33,36` already prepend `-D warnings` for test, corpus and doc; the only step gaining
anything is `helper-test`'s `cargo run` (`Makefile:95`), whose crate `make lint` has already
compiled under `-D warnings`. Non-path dependencies are compiled with `--cap-lints allow`, so
the flag cannot fail on upstream code. No finding.

**Task boundaries: sound**, with one sequencing nit (A4). Task 1 holds every change that must
land together (workflow, `--locked` in `make lint`, the rule and its fixtures, docs), Task 2 is a
disposable probe that merges nothing, Task 3 is handoff.

## Findings

No BLOCKING findings.

### A1 (ADVISORY, correctness of the rule as specified)

Plan line 70: "`scripts/check-release.py` asserts that the `verify.yml` `msrv` job's
`RUSTUP_TOOLCHAIN` equals the toolchain `release-qualification.yml` installs and the workspace
`rust-version`".

`Cargo.toml:10` is `rust-version = "1.98"`; both workflows say `1.98.0`
(`verify.yml:38,44`, `release-qualification.yml:22,28`). A literal equality rule fails on the
current repository on its first run, and the implementer will then loosen it ad hoc with no
specified invariant.

REWRITE TO: state the invariant as "the release toolchain is the MSRV": require
`toolchain == f'{rust_version}.0'` or `toolchain.split('.')[:2] == rust_version.split('.')`,
and take `rust_version` from the `cargo metadata` call `main()` already makes
(`check-release.py:242-245`, field `packages[name]['rust_version']`), asserting all workspace
packages agree, rather than re-reading `Cargo.toml` with a second parser.

### A2 (ADVISORY, a drift the rule would miss)

Plan line 70 checks one string in `verify.yml` against "the toolchain
`release-qualification.yml` installs". Each workflow spells the toolchain twice: `env.RUSTUP_TOOLCHAIN`
(`verify.yml:38`, `release-qualification.yml:22`) and the `rustup toolchain install` line
(`verify.yml:44`, `release-qualification.yml:28`). `RUSTUP_TOOLCHAIN` is what `make check`
actually runs under; rustup auto-installs a missing toolchain on first use. So a bump of the
install line alone passes the rule as written while the job keeps linting on the old compiler,
which is exactly the drift R4 exists to stop. Separately, `validate_publish_workflow`'s
closed-shape checks (exactly two jobs, no `${{ }}`-free assumption, lines 155-172) cannot be
applied to `verify.yml`, which has seven jobs and a matrix expression at line 17.

REWRITE TO: add `validate_release_lanes(verify_path, qualification_path, rust_version)` that
(1) bounds the `msrv` job body with the same split-and-bound technique as lines 129-137 and
bounds the `package` job body of release-qualification.yml the same way; (2) extracts from each
body both `RUSTUP_TOOLCHAIN:\s*(\S+)` and `rustup toolchain install (\S+) --profile minimal
--component clippy,rustfmt` and requires all four values equal and equal to the MSRV per A1;
(3) requires `^\s*- run: make check\s*$` in the `msrv` body; (4) raises `SystemExit` with one
message per failure, as the existing function does. Apply none of the closed-shape rules
(job list, anchors, escapes) to `verify.yml`. Call it from `main()` after
`validate_publish_workflow()`. Tests follow `check_copy`'s temp-copy style with two copies; the
plan's three fixtures plus "install line bumped, env unchanged" and "env bumped, install line
unchanged".

### A3 (ADVISORY, the probe as written may not trip the lint)

Plan line 101: "add `assert_eq!(0.1_f64 + 0.2, 0.3);` in a `purrassist-core` unit test module."

Both operands are compile-time constants. Clippy's `float_cmp` has historically exempted
comparisons where both sides are constant-evaluable (named constants go to the separate
`float_cmp_const` restriction lint), and the real #24 failure was a runtime value against a
literal: the fixed form at `from_node.rs:336-340` compares `(... .parse::<f64>().unwrap() as
f32).to_bits()` to `1.0_f32.to_bits()`, whose pre-fix form compared the `as f32` result to
`1.0_f32`. If the probe does not fire, Task 2 step 2 fails locally and the probe has to be
redesigned mid-task. The local pre-check at step 2 is the right guard; the shape should not
depend on it.

REWRITE TO: mirror the #24 shape with a runtime left operand, for example
`assert_eq!("0.1".parse::<f64>().unwrap() + 0.2, 0.3);` or
`assert_eq!(f64::from_node(&node("0.1")).unwrap() + 0.2, 0.3);` in the same test module.

### A4 (ADVISORY, acceptance names a tool the repository does not have, and one check is out of order)

Plan lines 67 and 92: "Locally, `rust-validate 1.98.0 make check` exits 0." `rust-validate`
appears nowhere in the repository, so a reviewer on another machine cannot reproduce the
executable acceptance. Plan line 95 lists "The hosted `msrv` job on the PR passes" among Task
1's focused checks, which precede "commit, push" on line 97; it cannot be satisfied before the
push.

REWRITE TO: express the local acceptance as `RUSTUP_TOOLCHAIN=1.98.0 make check` (the exact
mechanism `verify.yml:38` uses; `RUSTUP_TOOLCHAIN` overrides `rust-toolchain.toml`), keeping
`rust-validate` as a personal convenience only. Move the hosted-job check to a "after push"
line in Task 1 or into Task 3's completeness review.

VERDICT: PASS

---

## Pass 2 (plan revision 2: shared `make release-gate`, renamed `release-gate` job, all-occurrence pin rules)

Additional code read for this pass: `scripts/test-fuzz-feedback.py:21-51` (reads the nightly
from `rust-toolchain.toml` and sets its own `RUSTUP_TOOLCHAIN`, 300 s subprocess timeout),
`scripts/fuzz.py:2157-2185, 2289-2317` (llvm-tools resolution; `RUSTFLAGS` inheritance),
`scripts/test-verify-published.py:1-22` (offline local-registry fixture),
`scripts/package-workspace.py:6-56` (`rustup which cargo`, fresh `CARGO_HOME`),
`scripts/check-docs.py:24-26, 128-133, 790-799` (`release-qualification-steps` rule),
`.github/workflows/publish.yml:72,78`, `docs/QUALIFICATION.md:239-256`, `docs/RELEASE.md:14-23`.

### Prior findings

- A1 (major.minor vs `1.98.0`): DISCHARGED. Plan line 45 compares `major.minor` to `rust-version`.
- A2 (both spellings in both workflows; no closed-shape rules on `verify.yml`): DISCHARGED. Plan
  lines 45-47 and P2's fixtures ("a bumped install line only; a bumped env only").
- A3 (probe shape): DISCHARGED. Plan line 74 restores the pre-#24 runtime comparison at
  `from_node.rs`, which is the exact shape that failed run 38351629747.
- A4 (`rust-validate`, hosted check after push): DISCHARGED. Plan lines 48, 94, 135.

### Collapsing five named steps into one: no finding, with the evidence

The question is whether the operator can still tell which gate failed. They can, from the log
rather than the step list: make stops at the first failing target and prints
`make: *** [Makefile:<line>: <target>] Error <n>` naming the innermost target (`fmt`, `lint`,
`corpus`, `package`, or the script line of `release-gate`), and every recipe in `Makefile:8-102`
echoes its command (no `@` prefix except the fuzz loops), so the failing command precedes that
line. The previous shape gave no more than this: each old step ran one `make` target or one
script, and the first failure stopped the job either way (`release-qualification.yml:42-51`).
What is lost is only GitHub's per-step timing and the step-name list that `check-docs.py`
regenerates into `docs/RELEASE.md` (`check-docs.py:24-26, 794-799`), and that rule still passes
on a single named step. The alternative that keeps five steps in both workflows reintroduces
the step-list drift P1 exists to remove, so the trade is correct. One ordering point is a real
finding (P2-1 below).

### PR job runtime and nightly install: sound

- Nightly need: `test-fuzz-feedback.py:27-30` reads the channel from `rust-toolchain.toml` and
  overrides `RUSTUP_TOOLCHAIN` for its subprocess, so the job's `RUSTUP_TOOLCHAIN: 1.98.0` does
  not break it, and `fuzz.py:2157-2170` requires the `llvm-tools` component on that nightly with
  the version pinned in `vendor/tooling.json`. The plan installs exactly
  `release-qualification.yml:32`'s line. No Node is needed: the `patch` target is
  non-differential (`test-fuzz-feedback.py:22-26`).
- Runtime: the set is what `release-qualification.yml` already runs within `timeout-minutes: 90`
  (line 20), and the plan gives the PR job the same allowance (plan line 119). `make package` and
  `make corpus` already run on every PR in the `qualification` and `corpus` jobs on nightly, so the
  hosted cost roughly adds one more `make check`-plus-package on stable. Accepted cost, no failure
  mode.
- `RUSTFLAGS: -D warnings` now reaches one build release qualification never applies it to:
  `fuzz.py:2291-2293` appends `-C instrument-coverage` to the inherited `RUSTFLAGS`, so the
  nightly coverage build of the `purrassist-core` fuzz example compiles under `-D warnings`.
  `purrassist-core` and its examples already compile warning-free under nightly `-D warnings`
  (the `native` job's `make test`, `Makefile:26`, builds examples), so this cannot fail today;
  it is the one place the "strict superset" is not empty and should be named in the plan's
  RUSTFLAGS sentence (line 120) so a future nightly-only warning in a fuzz example is understood
  as PR-lane-only.

### New findings

No BLOCKING findings.

#### P2-1 (ADVISORY, gate order inside `release-gate` is unspecified and prerequisites would invert it)

Plan line 112: "a `release-gate` target (`check`, `corpus`, `python3 scripts/test-fuzz-feedback.py`,
`python3 scripts/test-verify-published.py`, `package`)". If `check corpus package` are written as
prerequisites and the two scripts as recipe lines, make runs all prerequisites before the recipe,
so `package` (six `cargo package` verify builds with fresh `CARGO_HOME`, `package-workspace.py:26`,
the slowest gate) runs before the two cheap scripts, and the documented order in
`docs/RELEASE.md` no longer matches execution. P1's rule as phrased ("lists `check corpus` and the
three scripts") would accept either order.

REWRITE TO: make the order explicit and recipe-only:

```make
release-gate:
	$(MAKE) check
	$(MAKE) corpus
	python3 scripts/test-fuzz-feedback.py
	python3 scripts/test-verify-published.py
	$(MAKE) package
```

and have P1's Makefile assertion match these five lines in this order.

#### P2-2 (ADVISORY, the component assertion must not reach `publish.yml`)

Plan line 98 (P1): "both install the same stable pin with `clippy,rustfmt`"; line 45 (P2): the
stable pin is checked in `publish.yml` too. `publish.yml:78` is
`rustup toolchain install 1.98.0 --profile minimal` with no components, correctly: that job only
runs `cargo publish`. If the implementer writes one regex requiring `--component clippy,rustfmt`
and applies it to every install line, the rule fails on the real `publish.yml` and the fix is
either to loosen the rule or to install Clippy in the publish job.

REWRITE TO: two extractors: pin-only (`rustup toolchain install (\S+)`) applied to all three
workflows for P2 equality, and the full component form applied only to the `verify.yml`
`release-gate` job and the `release-qualification.yml` `package` job for P1.

VERDICT: PASS
