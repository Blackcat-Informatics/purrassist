# G5 review (G5 + G6 + G8, issue #26)

## Scope
Uncommitted diff on 0c4c25e: scripts/check-release.py, scripts/check-docs.py, both test files,
docs/RELEASE.md, README.md, Makefile, verify.yml comment, crates/core/Cargo.toml comment.
Checked against remediation-plan.md G5, G6, G8 (A10, A11, A13, B6, B7, B8, L7). No source modified;
all probes ran on temp copies, since deleted (git status shows only the unit's files).

## Evidence
1. Rules and fixtures (all present, each with a specific error message):
   - Extraction: `_logical_lines` (continuations, comments), `_rustup_pins` (install only after the
     `rustup` token, VALUE_FLAGS consume the next arg, `--opt=value` and booleans consume nothing, every
     positional is a pin, `default`, `override set`, cargo plus-version, inline toolchain assignment), env
     keys via ENV_PIN, `workflow_files()` covers every .yml/.yaml in `.github/workflows/`.
   - Validation: "toolchain pins must be literal" for `$`; "must be nightly-YYYY-MM-DD or X.Y.Z" otherwise.
   - Passing fixtures: flags before the name, continuation, real workflows, apt-get / install-qualification-tool /
     `cargo install` lines. Failing fixtures: matrix, `"$TC"`, cargo plus-version, inline assignment,
     default/override, new workflow file (`extra.yml:5`), `stable`, non-literal env pin, continued 1.97.0.
   - G6: `check_release_toolchain` plus 8 `ReleaseToolchain` tests (real pass, bump without docs, stale env
     assignment, stale patch mention, publication record ignored, mention just after the record read,
     none found, ambiguous pins).
   - G8: README lists `make release-gate`; Makefile/README fuzz wording changed; docstrings rewritten; L7
     left as is (reason recorded in the plan).
2. Commands: check-release.py, test-check-release.py (14 tests OK), check-docs.py, test-check-docs.py
   (260 tests OK) all exit 0 with `-X warn_default_encoding -W error::EncodingWarning`. `make hygiene` passes.
   `grep -rn 'release fuzz gate' README.md Makefile docs/` is empty (rc 1); `make release-gate` found at README.md:293.
   No stale `WORKFLOW_FILES`, `INSTALL_PIN` or "three workflows" remain anywhere.
3. Probes:
   - `sudo apt-get install -y libarchive-tools` (also after `&&`), install-qualification-tool line, `cargo install`: no pins. PASS.
   - `--profile=minimal 1.98.0` and `--profile=minimal --component=clippy 1.98.0 && cargo test`: pin 1.98.0 only. PASS.
   - cargo plus nightly-2026-09-14: read as a nightly pin; matching the channel passes, nightly-2026-09-15 fails
     ("does not match rust-toolchain.toml channel"). PASS.
   - New `.yaml` workflow with stable 1.97.0: fails ("stable toolchain pins differ", lists new.yaml:5). The same
     pin 1.98.0 passes. PASS.
   - Pin bump 1.98.0 to 1.98.1 in all workflows with rust-version left at 1.98 (so check-release passes) and docs
     untouched: check-docs fails with five `[release-toolchain]` errors (RELEASE.md:142,145,164;
     QUALIFICATION.md:251,259). PASS.
   - Publication record: "stable Rust 1.98 Clippy" untouched without failing; the test also shows a 1.97.3 mention
     and an env-assignment inside the record are ignored. PASS.
4. Doc accuracy: the RELEASE.md check-release paragraph matches `_gate_job_errors`, `_pull_request_errors`,
   `validate_release_gate` (check prerequisites lint+test, two LINT_RECIPE commands, CORPUS_SUITES equality),
   and `validate_toolchain_pins`. The module docstring, Makefile, Cargo.toml and verify.yml comments are accurate.
   The qualification-section text about `make lint` is consistent with the Makefile.

## Findings (none blocking)
- N1 (nit): `_unquote` in check-release.py has no docstring, unlike its neighbours.
- N2 (nit): the Rust-version regex in check-docs treats `Rust 1.98.0-rc.1` as a plain 1.98.0 match; irrelevant today.
- N3 (observation): a bare cargo plus `nightly` (no date) is extracted as pin `nightly` and then rejected by
  PIN_FORMAT, which is the intended strictness.

## Verdict
All required rules, fixtures, commands, probes and doc claims check out.

VERDICT: PASS
