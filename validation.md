# Validation index — issue #1, PR #22 (CI fixes and hosted fuzz gate)

## windows-engines
- Reproduced with `RUSTFLAGS='-D warnings' rust-validate 1.98.0 cargo clippy -p purrassist-host -p purrassist-cli --all-targets --locked -- -D warnings`. It failed with 3 `too_long_first_doc_paragraph` errors (cost.rs:93, tool_call/emit.rs:1130, tool_call/ir.rs:600). After `2864e75` (comment-only splits) it exits 0.
- The lane's three `cargo test` commands on 1.98.0 with `-D warnings`: exit 0, 107 test groups OK.

## fuzz
- Cause 1: the job never installed Node, so the pattern oracle failed the pinned-version check. Fixed by the install step (`f07b15b`).
- Cause 2: the default `make fuzz` stops on the accepted #6/#11 limitations. Fixed by `make fuzz-hosted` (the latest commit): `--continue-on-known` on both tool-call targets, and a 120 s timeout on `tool-call-ref`. Crash panics and classifications are now printed to the log.
- Local evidence on this head:
  - `fuzz.py --target tool-call --continue-on-known`: passed 1,408 inputs, 4 known #6 retained, 8m25s.
  - `fuzz.py --target tool-call-ref --continue-on-known --timeout 120`: passed 1,101 inputs, 2 known #6 retained, 6m19s.
- `test-fuzz.py`: 81 OK. `test-check-docs.py` Makefile-loop tests: OK.

## Gates
- `make check` and `make package` on `f07b15b`: both exit 0 (110 test groups; six archives verified). Rerun on the final head is recorded below.
- Final head `7e30c67`: `make check` exit 0 (110 test groups). `make package` was not rerun after `f07b15b`: the later commit touches no crate file.
- Docs follow-up (latest commit): FUZZING.md and README describe `make fuzz-hosted`, and RELEASE.md step 2 requires base replay of retained #6 inputs. The six local #6 inputs on `7e30c67` were replayed on base `943fa04` by the reviewer, and all reproduce their signature. make hygiene exit 0; test-check-docs.py OK.
- Final reviewer recheck (gap-analysis.md): VERDICT PASS. Hosted run 38330704651 on `7e30c67`: every job passed (windows-engines; fuzz via make fuzz-hosted in 33m23s with the six known #6 inputs; #11 passed). Last commit: mocked-campaign output silenced in test-fuzz.py (0 crash/known lines in its log), QUALIFICATION cross-reference; make hygiene exit 0.
- Hosted Verify run 38333691731 on the final head `8ce1ca6`: all 8 jobs passed (msrv, fuzz, wasm, corpus, qualification, windows-engines, native ubuntu, native macos). The fuzz log retains the same six named #6 inputs (tool-call 600/1002/1078/1129; tool-call-ref 905/1044) with no mocked lines.
- Pre-merge commands run by the coordinator: merge-tree = HEAD^{tree} (origin/main e0a7ecc); run 38333691731 head 8ce1ca6 success; test-fuzz.py OK with EncodingWarning as an error. merge-plan-review.md: VERDICT PASS.
