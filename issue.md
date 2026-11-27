# Issue #307: Release PurRAssist 0.2.0

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

## Result

Release PurRAssist 0.2.0: the six release crates published to crates.io at `0.2.0` from one reviewed tag, `rust-v0.2.0`, and visible on the registry.

## What 0.2.0 contains

- The four PurRDF crates on the `=5` series (merged in #306, commit `ddcd585ca2752aaee944b5421eb43759b99dfded`), with the typed loss subjects and the changelog record of every canonical form and digest.
- The `[Unreleased]` changes since `0.1.0`, moved into a dated `[0.2.0]` section of `CHANGELOG.md`.

## Version and marker changes

- Workspace version `0.1.0` becomes `0.2.0`, with the five internal version requirements in the root `Cargo.toml` and `server.json` updated to match.
- The derive support marker renames `__DERIVE_ABI_0_1` to `__DERIVE_ABI_0_2` in `crates/core/src/tool_call/` and in `docs/tool-calls/INTEGRATION.md`. A minor bump requires the rename; the core unit test enforces it.
- Status lines in `README.md`, `SECURITY.md`, the crate READMEs, `docs/IMPLEMENTATION.md`, `docs/QUALIFICATION.md` and `docs/RELEASE.md` name `0.2.0`. `scripts/check-docs.py` and `scripts/check-release.py` pass.

## The six crates

In this order: `purrassist-core`, `purrassist-derive`, `purrassist-syntax`, `purrassist`, `purrassist-host`, `purrassist-cli`.

## Acceptance

- The release head is named, and an independent review of that exact head returns PASS.
- `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` passes on that head, with the log kept.
- `make fuzz-hosted` passes locally on that head.
- The trusted publisher for each of the six crates is re-confirmed through the crates.io API immediately before tagging.
- The tag `rust-v0.2.0` is pushed only after the head, the verdict and the gate result have been reported to coordination and the tag is authorized.
- `publish.yml` succeeds, and each of the six crates is visible on crates.io at `0.2.0`.
- `scripts/verify-published.py --version 0.2.0` passes, and the GitHub release carries the `[0.2.0]` section and that verification output.

## Blocks

Nothing that the 0.2.0 registry release depends on is outstanding. Issues 221 and 235 are not part of this release.


## Comments (2)

### paudley — 2026-11-27T19:28:01Z

Local release-gate limitation on this host (recorded so the next release does not rediscover it):

- `make release-gate` under `rust-validate 1.98.0` stops at its first step: the validator's rustup home has no rustup for its cargo path. The validator is documented for external-package validation, so the release gate's stable 1.98.0 toolchain setup is not available locally.
- `make fuzz-hosted` under the same validator fails the coverage-feedback probe: the 1.98.0 validation toolchain has no `llvm-tools` component. Installing it would change the host toolchain, which was not done.

The release gate and hosted fuzz evidence for a release head come from the hosted `verify.yml` `release-gate` and `fuzz` jobs on that head.


### paudley — 2026-11-27T20:29:36Z

Stash check for the release worktree (read-only), after a sub-agent used a bare `git stash` and `git stash pop` there.

- `git fsck --no-reflogs --unreachable` lists 495 unreachable commits. Two stash-shaped commits are from 2026-11-27, and both belong to this release work. `e607f2ed` ("WIP on paudley/307-release-purrassist-0-2-0", 14:13, parent `3576c32c`) is the sub-agent's stash. Its tree differs from `3576c32c` only in six of the seven files that commit `ad42ceac` changes: `CHANGELOG.md`, `README.md`, `docs/FUZZING.md`, `docs/QUALIFICATION.md`, `docs/RESOURCES.md` and `docs/known-limitations.json`. `809cf5dc` ("On main: preserve-agents-md-for-302-merge", 13:05) is the tagged stash used to advance `main` after the purrdf repin merge (PR #306), applied by SHA and dropped. The next newest stash-shaped unreachable commits date from 2026-11-20. The pop restored only the sub-agent's own work.
- `git stash list`, run once, shows eight entries, dated 2026-10-28 to 2026-11-19, none from today. The stack is shared by every worktree of this repository, so any checkout of it lists the same eight.


