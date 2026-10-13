# G3 review (remediation unit 2, issue #26)

## Scope
Uncommitted diff of Makefile, scripts/check-release.py, scripts/test-check-release.py on 5e5159e. Source files not modified; probes ran on temp copies, all removed.

## Evidence
1. Static checks
   - `lint` recipe is exactly two lines: the issue's workspace command, then `cargo clippy -p purrassist-core --locked $(foreach suite,$(CORPUS_SUITES),--test $(suite)) -- -D warnings`. LINT_RECIPE matches byte for byte.
   - CORPUS_SUITES has one plain `:=` definition (Makefile:32). `make corpus` (line 34) is untouched and uses the same variable; the diff does not touch it.
   - The 8 listed suites equal the 8 `test = false` [[test]] targets in crates/core/Cargo.toml. `recovery` is a [[test]] without `test = false` and is correctly excluded.
   - The 12 new fixtures plus the real-file check each assert a specific message: "lint recipe must be exactly", "missing [...]", "unknown [...]", or "must define CORPUS_SUITES exactly once". They cover: no `-D warnings`, `|| true`, leading `-`, missing line, removed suite, new test=false target, test=true target passes, unknown suite, `+=`, a second `:=`, `override`, and `?=`.
2. Runs
   - `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` exits 0.
   - `scripts/test-check-release.py`: Ran 96 tests, OK.
   - `make hygiene`: schema valid, exit 0.
   - `make lint` (nightly): both clippy lines pass.
3. Probes (`validate_release_gate` on temp copies)
   - Real files: PASS.
   - New `[[test]] name="x" test=false` in crates/core/Cargo.toml, CORPUS_SUITES unchanged: FAIL, "missing ['x']". As required.
   - Suites reordered: PASS. Order is not semantically relevant to the check or to Make, so a set comparison is right. Duplicates are still rejected.
   - Lint line wrapped with `\`: FAIL. The recipe is compared exactly, so a wrap is rejected. Strict, which is acceptable for an exact-recipe gate. It is not a false pass.
   - Trailing `# note` on the CORPUS_SUITES line: PASS. The comment is stripped, and Make drops it too (the value is unchanged apart from trailing whitespace).
   - `$(EXTRA)` inside CORPUS_SUITES: FAIL, "unknown ['$(EXTRA)']". Variable references cannot slip past the literal check.
4. Corpus overlap and harm
   - The workspace `--all-targets` line does not build `test = false` targets (per the plan, verified there). The second line therefore covers new ground: it adds only the 8 corpus test artifacts.
   - The overlap that remains is rechecking the already-built core lib deps from cache (0.17s locally), which is harmless.
   - `make corpus` still compiles the same suites with `-D warnings` through RUSTFLAGS. That is a test build, not a lint, so there is no conflict.

## Findings
- No blockers.
- Minor, non-blocking: a `test = false` suite declared in a non-core crate would pass the set check but fail `-p purrassist-core --test` loudly (cargo error), as would `make corpus`. That is a loud failure, not a silent one.
- Minor: the regex check for non-`:=` assignments does not catch an assignment via `include`d files or `$(eval ...)`. The Makefile has none; this is out of scope for G3.

VERDICT: PASS
