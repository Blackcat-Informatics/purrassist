# T1 review (issue #26, plan revision 3)

## Scope
Uncommitted diff in the worktree: Makefile, release-qualification.yml, verify.yml, check-release.py,
test-check-release.py, test-check-docs.py, QUALIFICATION.md, RELEASE.md. No source modified by this review.

## Deliverables vs plan (all match)
- Makefile: `release-gate` has no prerequisites; recipe is `$(MAKE) check`, `$(MAKE) corpus`, test-fuzz-feedback.py,
  test-verify-published.py, `$(MAKE) package`, in that order; `.PHONY` updated; `lint` has `--locked`.
- release-qualification.yml: checkout plus exactly four named steps (two installs, version check, `Verify the release gate`
  running `make release-gate`). env unchanged (`RUSTUP_TOOLCHAIN: 1.98.0`), timeout 90.
- verify.yml: `msrv` is now `release-gate`; env identical to release (RUSTFLAGS dropped); stable install with
  `clippy,rustfmt`; nightly-2026-09-14 install with `llvm-tools`; `make release-gate`; `timeout-minutes: 90`.
- check-release.py: `validate_release_gate` (P1: exact step names, one unnamed checkout first, `make` exactly once and in the
  gate step, verify job runs only `make release-gate`, env equality, install equality and components, Makefile
  prerequisites/recipe/lint) and `validate_toolchain_pins` (P2: name-only extractor, nightly* vs stable, identical stable pins,
  major.minor vs rust-version, nightly vs rust-toolchain.toml) over all three workflows. `workflow_steps` is imported from
  check-docs.py via importlib, not duplicated. Both rules are called from main().
- test-check-release.py: scratch-tree fixtures, one failing case per assertion, each asserting its error text, including every
  P2 fixture named in the plan (install-only bump, env-only bump, publish-only bump, rust-version mismatch, nightly drift).
- test-check-docs.py: assertion now names `Verify the release gate`.
- Docs: QUALIFICATION.md command list gains `RUSTUP_TOOLCHAIN=1.98.0 make release-gate`; the old "stable MSRV workspace
  tests" phrase is gone (no remaining "msrv" job, "Verify workspace" or old phrase in docs/scripts/.github); the new sentence
  names the `release-gate` job and states it is the MSRV 1.98 build-and-test evidence. RELEASE.md step list is 1-4 and the
  check-release paragraph describes gate identity and pins.

## Evidence (run with -X warn_default_encoding -W error::EncodingWarning)
- scripts/check-release.py: rc 0 ("Release gate identity verified", "8 stable occurrences ... 12 nightly occurrences").
- scripts/test-check-release.py: rc 0. scripts/check-docs.py: rc 0. scripts/test-check-docs.py: 252 tests OK.
- make hygiene: rc 0.
- Mutation probes on temp copies (repo untouched, git status unchanged): bumped verify env pin -> "stable toolchain pins
  differ"; publish.yml-only bump -> same; removed `$(MAKE) package` -> "recipe must be exactly"; extra release step -> "exactly
  the named steps"; `make release-gate && make doc` -> "must run make exactly once"; nightly install without llvm-tools ->
  caught.
- Not run: the full `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` (outside this review's listed checks; the plan assigns it to
  the focused checks and the hosted run).

## Real callers/consumers
check-release.py main() runs both rules and is invoked by `make hygiene`/`make package`, release-qualification.yml's version
step, and the native verify job's test-check-release.py. check-docs.py::workflow_steps is the shared parser. publish.yml
requires a successful release-qualification run, so the shared gate now guards it.

## Hosted release-gate viability (ubuntu-24.04)
- test-fuzz-feedback.py reads the `rust-toolchain.toml` channel (nightly-2026-09-14), requires the `nightly-` prefix and sets
  RUSTUP_TOOLCHAIN to it for its subprocess; the job installs exactly that nightly with llvm-tools, and P2 pins the install to
  that channel. This works despite the job-level 1.98.0 override.
- python3 and make are preinstalled on the hosted image; the same steps already ran there in the release job.
- package-workspace.py uses `rustup which cargo` (honours RUSTUP_TOOLCHAIN=1.98.0, which is installed) and
  `git rev-parse HEAD` (works on the depth-1 PR checkout).
- corpus is plain cargo test; test-verify-published already runs in the native job; make check was exercised by the
  successful release run on the same image and toolchains. PR differences (merge-ref checkout, branch ref type) do not
  affect any gate step; `--tag` appears only in the version-check step, which is outside the shared gate.
- Residual (not blocking): the first hosted run is the only real proof; Task 1 step 3 already requires it.

## Findings
- Nit (non-blocking): docs/QUALIFICATION.md around line 259-260, "engines/files/CLI, executed WASM comparisons, and Linux
  qualification. Exact commit and run evidence is recorded in issue #1 and the PR;" is a 136-column line after the reflow.
  Rewrap for consistency.
- No agent attribution and no stale wording found in the diff.

VERDICT: PASS
