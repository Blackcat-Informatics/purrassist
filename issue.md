# Issue #173: reuse: reach full REUSE 3.3 compliance and add the REUSE badge

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github
Labels: documentation, enhancement

## Body

## Goal

Bring this repository to full conformance with the [REUSE Specification 3.3](https://reuse.software/spec-3.3/) and show it with the REUSE badge in the README. This is one of four matching issues: Blackcat-Informatics/purrdf, rust-agentic-foundations, gmeow-ontology and purrassist. The licensing choices do not change; only how they are declared and checked.

## Current state

- No `LICENSES/` directory. The root has `LICENSE-MIT`, `LICENSE-APACHE` and `LICENSE-MULAN`; confirm the intended expression against `Cargo.toml`.
- No `REUSE.toml`. About 502 of 2,115 tracked files carry an SPDX identifier (rough signal; `reuse lint` is the authority).
- No `reuse lint` in `verify.yml`, `publish.yml`, `release-qualification.yml` or `mcp-registry.yml`. No REUSE badge in `README.md`.

## Work

- Create `LICENSES/` with the SPDX texts for each license in use.
- Add `REUSE.toml` covering `vendor/`, `.purrassist/`, `Cargo.lock`, `server.json`, docs and other files that cannot carry headers.
- Add a `reuse` job to `verify.yml`, a local `Makefile` target, and the badge to the README.

## Acceptance criteria

1. Record the baseline `uvx reuse@6.2.0 lint` result in this issue (missing copyright or license, bad licenses, unused licenses, deprecated dep5).
2. Every file has copyright and license information: an inline SPDX header where the format allows, otherwise a `REUSE.toml` annotation (`precedence`, globs) for lockfiles, binaries, generated and vendored content. No `.reuse/dep5`.
3. `LICENSES/` holds exactly one `<SPDX-ID>.txt` (or `LicenseRef-*.txt`) per license in use, no unused licenses, no non-license files.
4. `reuse lint` exits 0 and runs as a required CI job with a pinned version, plus a local gate target.
5. Register the repository at https://api.reuse.software, then add the badge to the README badge row:
   `[![REUSE status](https://api.reuse.software/badge/github.com/Blackcat-Informatics/purrassist)](https://api.reuse.software/info/github.com/Blackcat-Informatics/purrassist)`
   Confirm it reads "compliant" once the change is on `main`.
6. CONTRIBUTING or the PR template states the SPDX header requirement for new files.

## Sibling issues

- Blackcat-Informatics/rust-agentic-foundations#194
- Blackcat-Informatics/purrdf#1262
- Blackcat-Informatics/gmeow-ontology#2687


## Comments (1)

### paudley — 2026-11-11T18:25:35Z

## Baseline `reuse lint` (reuse 6.2.0, spec 3.3)

Run on a clean export of tracked files at `main` (`218ec2b`, 1,927 files). Result: **not compliant** (exit 1).

- Files with copyright information: 959 / 1,927
- Files with license information: 501 / 1,927
- **Missing copyright and license info: 1,426 files**, mostly `crates/core` (1,339), `docs/qualification` (35), `vendor/licenses` (14), `examples/` (13), `scripts/` (6), root files (4), `vendor/unicode` (3), `vendor/entities` (2) and the `vendor/*.json` files
- **Missing license texts (5):** `Apache-2.0`, `MIT`, `MulanPSL-2.0`, `BSD-3-Clause`, `Unicode-3.0`. There is no `LICENSES/` directory.
- Bad, deprecated, unused licenses: 0
- **Invalid SPDX expressions: 7, in 4 files.** These are false positives where scripts quote header text in string literals: `scripts/generate-entities.py`, `scripts/generate-python-names.py`, `scripts/generate-unicode.py`, `scripts/test-repository-audit.py`. Fix with `REUSE-IgnoreStart` / `REUSE-IgnoreEnd`.

`LICENSES/` needs five texts, not the three in the description: `BSD-3-Clause` and `Unicode-3.0` are also referenced.


