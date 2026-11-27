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


## Comments (0)

