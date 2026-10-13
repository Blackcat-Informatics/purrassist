# G1 review (G1 + G2, issue #26)

## Scope
Uncommitted diff of `scripts/check-release.py` and `scripts/test-check-release.py`, against remediation-plan.md G1 and G2 (binding), gap-analysis H1, M1, M2, and the remediation-plan review. No source files were modified. Probes ran on temp copies under /tmp and were removed. `git status` is unchanged (the two modified files plus `.stage/`).

## Evidence
- `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py`: exit 0, "Release gate identity verified", "Toolchain pins verified".
- Same flags on `scripts/test-check-release.py`: 84 tests OK (56 before).
- `make hygiene`: exit 0 (includes check-release, the other helper tests and the resource-limit schema check).

### (1) Rules and fixtures
Every G1 and G2 rule is present, and each fixture asserts its specific message with `fails(...)`:
- Workflow top-level allowlist and no `env:` or `defaults:` (`_workflow_shape_errors`): `test_workflow_level_env_or_defaults_fails` covers both workflows and both keys.
- Bare `pull_request:` trigger (`_pull_request_errors`): trigger removed, `paths:` filter, flow `{paths: [x]}`, flow `on: [push, pull_request]`. The commented bare trigger passes.
- Gate job keys exactly {runs-on, timeout-minutes, env, steps}: `defaults`, job and step `continue-on-error`, `if`, `shell`.
- Job env exactly `RUSTUP_TOOLCHAIN`, stable, equal to the job's stable install: env removed from both jobs, nightly in both, 1.98.1 against a 1.98.0 install, job-level `RUSTFLAGS`, step-level `CARGO_ENCODED_RUSTFLAGS`.
- Step lists: the verify gate is exactly 4 steps, and the release job names are checked by the existing rule plus the new plain-install check. Extra step writing `.cargo/config.toml`, `GITHUB_ENV` write, `GITHUB_PATH` write in the version step.
- Step keys, `shell: bash` only on `Verify release set and version`, and N1: `ref: main`, `@v4` by tag, a different action, a 39-hex SHA, and `uses` on a later step.
- Must-pass fixtures: named verify gate step, quoted env, trailing comment on env, swapped install order, commented bare trigger.
- G2: `check` without `lint` and `check` without `test` each fail with their own message.

### (3) Adversarial probes
Run through `validate_release_gate(root)` on scratch copies:

| Probe | Result |
|---|---|
| Real files | pass |
| H1: env removed from both jobs | FAIL: job must set `env`; env must be exactly RUSTUP_TOOLCHAIN, found [] |
| H1: nightly in both jobs | FAIL: must be the stable release toolchain, not nightly-2026-09-14 (both jobs named) |
| Extra step writing `.cargo/config.toml` | FAIL: exactly the steps |
| `if: ${{ false }}` on the verify job and an `if` on the release job | FAIL: job may not set `if` |
| `ref: main` on the checkout | FAIL: step 1 `with:` must be exactly persist-credentials: false |
| `pull_request:` with `types: [labeled]` | FAIL: trigger must have no filters |
| Top-level `env:` | FAIL: may not set the top-level key `env` |
| Step with `$GITHUB_ENV` in a block scalar | FAIL: must not write GITHUB_ENV or GITHUB_PATH |
| Trailing extra step; `working-directory` on the gate step; `make release-gate \|\| true`; extra `uses` step in the release job; `continue-on-error` on the release gate step; removed `timeout-minutes`; job env as a flow mapping; `steps: &x`; `steps: [`; install with `; true` or `&& make x`; a quoted `"env"` key in a step; a stray `? complex` key in the job | FAIL, each with a message that names the cause |
| Renamed verify gate step | pass (required) |
| Comment line inside steps (verify and release) | pass (required) |

All the scratch trees were deleted.

## Findings
No blocking findings.

Advisory, none requiring a change for G1:
1. A1 (not in G1 scope): extra triggers beside the bare `pull_request:` are not restricted. Adding `pull_request_target:` passes. This is not in the plan's trigger rule and does not weaken the gate on pull requests. A later unit could allowlist the `on:` keys.
2. Code quality: `_job_steps` and `_job_keys` are a second indentation reader beside `check-docs.py`'s `workflow_steps`. This is justified, because `workflow_steps` does not expose `with`, `env`, `if` or unrecognised keys, and the docstrings say so. The `by_kind` helper is defined inline inside `validate_release_gate`, and the first-step `uses/with` exemption in `_gate_job_errors` is a slightly dense conditional (`key == 'uses' and index == 0 or ...`). Both are readable but could be tidied.
3. Error messages name the file and job, the offending key and the allowed set. Every new helper has a docstring. `validate_release_gate`'s docstring was updated to list the new rules.
4. Several messages for one defect are repeated (for example the extra-step case also trips the exactly-the-steps, make-command and GITHUB_ENV rules). That is noisy but not wrong.

VERDICT: PASS
