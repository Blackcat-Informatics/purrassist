# Gap analysis: paudley/1-complete-purrassist-0-1-0-plan-and

- Base: `origin/main` `e0a7ecc`. Head: `f07b15b` (commits `2864e75`, `f07b15b`).
- Five files changed: `verify.yml`, `cost.rs`, `emit.rs`, `ir.rs` and `RELEASE.md`.
- `output-rules.md` does not exist in this stage directory (only `issue.md`, `open.json` and empty subdirectories), so this report uses the gap-analyzer format.

## Change 1: Rustdoc first paragraphs (`2864e75`): PASS

- **Comments only.** All three hunks change `///` lines and nothing else.
- **Meaning preserved.** Each original sentence is split at its main clause. The second clause becomes the next paragraph with its subject restated: "The printer drives ...", "Recognizing the bytes gives back ...", "Readings are ranked by ...". All intra-doc links are kept.
- **Stable 1.98.0 Clippy, run exactly as `windows-engines` runs it.** Command: `RUSTFLAGS='-D warnings' rust-validate 1.98.0 cargo clippy -p purrassist-host -p purrassist-cli --all-targets --locked -- -D warnings`.
  - The run used a fresh `CARGO_TARGET_DIR` and a fresh `CARGO_BUILD_BUILD_DIR`, because the shared `build-dir` in the cargo config otherwise replays cached results.
  - It checked `purrassist-core`, `-syntax`, `-derive`, `purrassist`, `-host` and `-cli`. Exit 0, no diagnostics.
- **Negative control.** The same command on a temporary worktree of base `e0a7ecc` exits 101. It reports exactly three `too_long_first_doc_paragraph` errors, at `cost.rs:93`, `emit.rs:1130` and `ir.rs:600`. These are the three sites this commit fixes, so no fourth site remains.
- **Hosted run 38328660473 (head `f07b15b`), still running when written.** In `windows-engines`, steps 4 and 5 (`cargo test` for core/syntax and for derive/purrassist) succeeded, and step 6 was running. On `93c02b3` and `e0a7ecc` only the Clippy step failed.
- **Residual.** Windows-only `cfg` code cannot be linted locally without the MSVC target, which was not installed as instructed. Before this branch, that job failed only at the last step, the core lint.

## Change 2: Node for the `fuzz` job (`f07b15b`): PASS

- **The installer's stdout is only the bin directory.** `scripts/install-qualification-tool.py` ends with `print(install(...))`. Its subprocess output goes to a log file or `check_output`, and it checks `node --version` against the pin. Appending stdout to `$GITHUB_PATH` is correct and matches the `native`, `wasm` and `qualification` jobs.
- **Step order is correct.**
  - The Node step comes after checkout and the toolchain install (`vendor/tooling.json` must exist) and before `make fuzz`.
  - The two things `make fuzz` runs before the target loop do not need real Node:
    - `scripts/test-fuzz.py` mocks `subprocess.run` for every `node_pattern_test` case.
    - `scripts/test-fuzz-feedback.py` runs the `patch` target, which has no differential.
  - In `fuzz.py`, `TARGETS[...]['differential'] == 'pattern'` holds only for `pattern`, so `tool_identity` asks for `node` only there. The new comment is accurate.
- **Hosted evidence.**
  - Run 38324072291 on `main` failed only at `== fuzz target: pattern ==` with `ValueError: node version differs from inventory 26.11.1`; json, markdown and schema had passed.
  - In run 38328660473, step 4 "Install pinned Node for the pattern differential oracle" completed successfully.
- **"all ten targets"** matches the Makefile loop and `TARGETS`, and `check-docs.py` passes. The comment describes what is configured, not what completes (see G3).
- **Time budget.** On `main` each early target took about 4 minutes, and tool-call stops at execution 600 (see below). The 90-minute `timeout-minutes` is not at risk.

## Change 3: the release fuzz gate in `docs/RELEASE.md` step 2: BLOCKED

The sources agree with the gate's intent in several ways:
- The Makefile loop order is json, markdown, schema, pattern, unified, fuzzy, matching, patch, tool-call, tool-call-ref.
- Each target runs as `python3 scripts/fuzz.py --target $target || exit 1`.
- `make fuzz` never passes `--continue-on-known`, so a #6 panic always ends the job.
- FUZZING.md's Known limitations describe the #11 `tool-call-ref` timeout at execution 682.

The text is still not precise enough to apply from the job log, and it is not consistent with the documents it relies on.

### Local evidence on this head

I ran `python3 scripts/fuzz.py --target tool-call` with the default seed, the same command `make fuzz` uses.
- It stops at execution 600.
- The last progress line is `501 executions; 422 corpus entries; ...`.
- The panic in `probe.stderr` is `rejected attempt 0..394 runs past where it failed (52) without its closer: ...` from `fuzz_tool_call.rs:1156`.
- `fuzz.known_failure()` classifies it as `issue-6-rejected-span-runs-past-failure`.
- Artifacts: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and/.cache/fuzz-tool-call-4xrscpos/`.

The campaign is deterministic: fixed seed, `Random(seed)`, coverage-driven corpus. The hosted job should therefore end the same way, and `tool-call-ref` never runs.

### Gaps

**G1 [CRITICAL]: The #6 panic signature never appears in the job log, so the gate cannot be applied as written.**
- `probe()` runs the harness with `capture_output=True`, writes stderr only to `<campaign>/probe.stderr`, then calls `result.check_returncode()`.
- The traceback shows only `str(CalledProcessError)`, verified both locally and in a standalone check: `Command '['.../probe']' returned non-zero exit status 101.`
- `failure.txt` holds `repr(error)`, which includes the stderr bytes, but it stays in `.cache/`. The `fuzz` job uploads no artifacts and prints neither file.
- So "a stop only on a known issue #6 panic signature" cannot be confirmed from the log. Any probe panic, including a new regression, produces an identical log.
- A strict reader can never satisfy the gate. A lax reader accepts every exit-101 crash in `tool-call`. The lax reading weakens the gate beyond the owner's decision.

**G2 [HIGH]: FUZZING.md contradicts the new gate.**
- `docs/FUZZING.md` lines 919-921: "The full local gate and hosted smoke campaigns on the exact final commit provide separate final-integration evidence; hosted smoke must execute 1,000 mutations for each of all ten targets."
- The new RELEASE.md text accepts a hosted run that stops at about execution 600 of `tool-call` and never starts `tool-call-ref`.
- `docs/QUALIFICATION.md` line 257, "It requires the full gate and hosted CI on that commit", also states no exception.
- `check-docs.py` passes because it has no rule for this sentence.

**G3 [HIGH]: The gate accepts zero hosted executions of `tool-call-ref` without saying so.**
- Because of the `|| exit 1` loop and the deterministic #6 stop at `tool-call` execution 600, `tool-call-ref` is never run. The gate's "#11 timeout in `tool-call-ref`" branch cannot be reached at current code.
- The last roughly 400 `tool-call` mutations are not run either. FUZZING.md's own "Isolating issue #6" section warns that a plain campaign "can hide a new failure behind it".
- The owner accepted #6 and #11 as non-blocking: `make fuzz` fails there. The documents do not record an owner decision that the release commit carries no fuzz evidence for `tool-call-ref`, the only target that reaches the nested-array item branches and the scope-graph paths (FUZZING.md lines 973-978).
- Either the gate requires the isolation runs that FUZZING.md already documents, or RELEASE.md states this hole and the owner explicitly accepts it.

**G4 [MEDIUM]: The #6 allowance is narrower than the single source.**
- `docs/known-limitations.json` gives #6 `fuzz_stop_targets: ["tool-call", "tool-call-ref"]`, and QUALIFICATION.md lines 228-231 say the same.
- The gate allows #6 only in `tool-call` and #11 only in `tool-call-ref`. If a future `tool-call` campaign passes and `tool-call-ref` then hits a #6 panic before execution 682, the gate blocks the tag, contradicting the accepted limitation.
- This is stricter, not weaker, but it is inaccurate against the source the docs checker treats as authoritative.

**G5 [MEDIUM]: "The issue #11 timeout" is not defined in log terms.**
- The log shows `subprocess.TimeoutExpired: Command '[.../fuzz-tool-call-ref-XXXX/probe', ...]' timed out after 10.0 seconds`. The last progress line before it is `601 executions; ...`, because progress prints at index % 100 == 0 and the #11 input is execution 682.
- The gate should require exactly that: the probe command, not `llvm-profdata` or `llvm-cov`, timing out after 10.0 s between the 601 and 701 progress lines.
- As written, any timeout anywhere in `tool-call-ref` would fit.

**G6 [LOW]: Step 2 also says to "run the ... fuzz ... checks listed in `QUALIFICATION.md`" locally.**
- Local `make fuzz` fails the same way, but the exception sentence speaks only of "Every job" and "Its log". Say that the same rule applies to the local run.

## Requirements coverage

| # | Requirement | Status | Gap |
|---|---|---|---|
| 1 | Short first doc paragraphs; comments only; meaning kept; 1.98 Clippy clean | COMPLETE | None. Positive and negative control both run. |
| 2 | Pinned Node in the `fuzz` job, correct output and order; comment says ten | COMPLETE | None. The hosted Node step succeeded. |
| 3 | Release fuzz gate accurate, applicable from the job log, no weakening beyond the owner's decision | PARTIAL | G1-G6 |

## ETHOS / one-path notes

- **Fail fast (§2-9):**
  - The gate tells a human to judge a failure from evidence the job does not emit (G1).
  - The fix belongs in the tooling (print the classified panic) rather than in a manual reading of artifacts that are lost when the runner ends.
- **No rationalized shortcuts (§21):**
  - G3 silently accepts that `tool-call-ref` never runs.
  - Either require evidence or record the owner's explicit acceptance.

## Recommended actions

1. **Fix G1 by making the stop visible in the log.** Either option is enough:
   - (a) In `fuzz.py`'s `except subprocess.CalledProcessError` path, before `raise`, print the probe stderr and the `known_failure()` classification to stdout. Example output: `probe panic: <stderr>` and `known signature: issue-6-rejected-span-runs-past-failure` or `known signature: none`. Add a `test-fuzz.py` case for both outcomes.
   - (b) Add an `if: failure()` step to the `fuzz` job that prints `.cache/fuzz-*/failure.txt` and `.cache/fuzz-*/probe.stderr`.
   - Option (a) is better: the classification then comes from the same `KNOWN_FAILURES` regexes, not from a human's reading.
2. **Fix G1 and G5 by restating the gate in log terms.**
   - json, markdown, schema, pattern, unified, fuzzy, matching and patch each print `Passed N inputs`.
   - Under `== fuzz target: tool-call ==`, the run ends with the probe's exit status 101, and the printed classification names an issue-6 signature.
   - If `tool-call` passes, `tool-call-ref` ends either with an issue-6 signature or with the probe's `TimeoutExpired` after 10.0 s following the `601 executions` line (execution 682).
3. **Resolve G3.** Choose one:
   - Require two local runs on the tag commit, with every `known-6/` input attributed by the base-replay procedure in FUZZING.md's "Isolating issue #6", and record the reports:
     - `make fuzz TARGET=tool-call FUZZ_ARGS='--continue-on-known'`;
     - `make fuzz TARGET=tool-call-ref FUZZ_ARGS='--continue-on-known --timeout 120'`.
   - Or state in RELEASE.md that hosted CI stops at `tool-call` execution 600 and never runs `tool-call-ref`, and get the owner's explicit acceptance of that. This is the owner's call, not the implementer's.
4. **Fix G2.** Make FUZZING.md lines 919-921 and QUALIFICATION.md line 257 agree with whatever RELEASE.md step 2 finally says.
5. **Fix G4.** Allow a #6 signature in either tool-call target, matching `known-limitations.json`.
6. **Fix G6.** Say that the same fuzz exception applies to the local `make fuzz` run.

VERDICT: BLOCKED

---

# Recheck at 7e30c67 (`ci(fuzz): a hosted fuzz gate that completes every target`)

The original verdict above is superseded by this section.

## What 7e30c67 resolves

- **G1 (the panic is not in the log): RESOLVED.**
  - On any crash that is not retained, `fuzz.py` now prints the following to stderr before re-raising:
    - `Probe crashed at execution N; known issue signature: <name|none>`;
    - the last 40 lines of `probe.stderr`.
  - A retained #6 panic prints `Known issue #6 panic at execution N (<signature>); input retained` to stdout. This matches the text that RELEASE.md quotes.
  - Three new `test-fuzz.py` tests cover all three outputs. I reran `test-fuzz.py`: 81 OK.
- **G3 (`tool-call-ref` never runs): RESOLVED.** `make fuzz-hosted` (the CI job's command, checked with `make -n`) runs all ten targets in `TARGETS` order.
  - `tool-call` gets `--continue-on-known`. `tool-call-ref` gets `--continue-on-known --timeout 120`. Other targets get no extra arguments.
  - Each target still runs as `|| exit 1`. Any crash other than #6, any timeout, any mismatch or any LLVM diagnostic still fails the job.
  - The `case` syntax is POSIX, so it works under `/bin/sh`.
- **G4 (#6 allowed only in `tool-call`): RESOLVED.** Both tool-call targets now retain #6, matching `fuzz_stop_targets` in `known-limitations.json`.
- **G5 (#11 not defined in log terms): RESOLVED.** The #11 input now has to complete under the 120 s timeout instead of being recognized in the log. This is what FUZZING.md's "Known slow input (issue #11)" section already prescribed.
- **G6 (local run): RESOLVED.** QUALIFICATION's command list now uses `make fuzz-hosted`, and RELEASE.md says the default `make fuzz` still stops.
- **G2 (FUZZING.md's ten-target rule):** RESOLVED for the sentence at lines 919-923, which now holds. Three other FUZZING.md sentences went stale (R1 below).
- **RELEASE.md step 2** is back to "Every job must pass" and describes `fuzz-hosted` accurately against the Makefile and `fuzz.py`.
- **Docs check:** `check-docs.py` passes, and `test-check-docs.py` passes all 252 tests. The new fixture makes the hosted loop fail when `tool-call-ref` is dropped. The change from `count=1` to `count=0` in the existing fixture is correct: the `replace` helper passes `count` to `re.sub`, so 0 replaces both loops.

## Independent evidence

- **Hosted run 38328660473 (head `f07b15b`):**
  - `windows-engines` succeeded, including the 1.98.0 Clippy step. This closes the residual noted in Change 1.
  - native Linux/macOS, MSRV, WASM, corpus and qualification also succeeded.
  - Its `fuzz` job ran the old `make fuzz` and was still running when I checked.
- **Attribution of the retained #6 inputs.** The coordinator's local campaigns left four inputs from `tool-call` (`.cache/fuzz-tool-call-6c_8wif0/known-6/`) and two from `tool-call-ref` (`.cache/fuzz-tool-call-ref-ckqo2k6h/known-6/`).
  - I followed FUZZING.md's attribution procedure. I built `--example fuzz-tool-call` from base `943fa04` in a temporary worktree, with the same `opt-level=1`, debug-assertions and overflow-checks settings, and piped each input to it.
  - All six inputs panic on the base with exit 101 and a byte-identical `rejected attempt A..B runs past where it failed (N)` message:

    | Input | Execution |
    |---|---|
    | `ff4c7266` | 600 |
    | `833c3935` | 1002 |
    | `8097dc89` | 1078 |
    | `4c0954c3` | 1129 |
    | `2bfd4b4d` | ref 905 |
    | `e806e7b8` | ref 1044 |

  - All six are therefore #6's, not branch-only. I removed the worktree and build directory afterwards.

## Remaining gaps

**R1 [HIGH, required]: Three FUZZING.md sentences now contradict the shipped gate.** `check-docs.py` does not catch any of them.

1. Lines 896-898: "**Smoke** (`verify.yml` on pull requests, main-branch pushes, and manual runs): `make fuzz` runs the corpus-feedback test once and all ten targets ..."
   - The CI now runs `make fuzz-hosted`.
2. Lines 939-941, in "Isolating issue #6": "The option is never used by `make fuzz`, the smoke gate, or a qualification run; they stay strict."
   - The smoke gate now uses the option, and so does the qualification run, since QUALIFICATION.md's command list says `make fuzz-hosted`.
   - Leaving this sentence in makes the public docs claim a strictness the gate no longer has.
3. Lines 1066-1067: "`make fuzz` (the CI `fuzz` job) runs this feedback test ..."
   - It should name `make fuzz-hosted`, or both targets.

**R2 [MEDIUM, should fix with R1]: The hosted gate passes without the attribution FUZZING.md requires.**
- FUZZING.md lines 943-955 say the attribution procedure runs "after any campaign that used the option", and that a branch-only input "must be fixed before the campaign is called clean".
- `make fuzz-hosted` is green after a signature match alone. A future regression that panics with the same rejected-span text would pass CI unnoticed.
- For the current code this has been checked: all six inputs are attributed above, and the campaign is deterministic, so the tag commit retains the same inputs unless the engine or harness changes.
- When fixing R1's second sentence, state the rule explicitly. Either:
  - RELEASE.md step 2 requires replaying the inputs the hosted log names (reproducible locally with the same targets) through the `943fa04` probe; or
  - FUZZING.md states that the hosted and release gate accept a signature match, and attribution belongs to issue #6 work.
- The first option keeps the release no weaker than the documented procedure. The second needs the owner to accept it explicitly.

**R3 [LOW]: Minor wording issues.**
- The `fuzz` recipe comment in the Makefile ("so the gate stays deterministic across all ten targets") still calls `make fuzz` "the gate".
- The README development section lists only `make fuzz`; mentioning `make fuzz-hosted` there is optional.
- For targets with no extra arguments, the echo prints a trailing space: `== fuzz target: json  ==`.

**R4 [INFO, pending]: Hosted job duration.** Local runs took 8m25s for `tool-call` and 6m19s for `tool-call-ref`. Hosted targets took about 4 minutes each on `main`. Expect about 55-65 minutes against `timeout-minutes: 90`. Hosted run 38330704651 (head `7e30c67`) was running when checked. Its `fuzz` job is the evidence that the margin holds and that `fuzz-hosted` passes on the runner.

## Updated coverage

| # | Requirement | Status |
|---|---|---|
| 1 | Doc splits, 1.98 Clippy clean | COMPLETE: local positive and negative controls, plus hosted `windows-engines` success |
| 2 | Pinned Node in the `fuzz` job | COMPLETE: hosted install step succeeded |
| 3 | Release fuzz gate accurate, applicable from the log, no weakening | PARTIAL: mechanism correct and tested; R1 (stale FUZZING.md statements) and R2 (attribution rule) open |

## Required before PASS

1. **R1:** fix the three FUZZING.md sentences (lines 896-898, 939-941 and 1066-1067) so they describe `make fuzz-hosted` and its use of `--continue-on-known` accurately.
2. **R2:** state the attribution rule for the hosted and release gate in the same edit. Either RELEASE.md step 2 requires base replay of the inputs the hosted log names, or FUZZING.md records the owner-accepted signature-only criterion.

VERDICT: BLOCKED

---

# Final recheck at cbecc8c (`docs(fuzz): describe the hosted gate and keep base attribution for its #6 inputs`)

This section supersedes both earlier verdicts.

## R1-R3 resolved

- **R1 (stale FUZZING.md sentences): RESOLVED.**
  - Lines 896-900: Smoke now names `make fuzz-hosted` and its flags.
  - Lines 939-947: the default `make fuzz` and qualification runs stay strict; `make fuzz-hosted` uses the option on the two tool-call targets, names each retained panic, and its inputs go through attribution before a run counts as clean.
  - Lines 1069-1073: both make targets are named.
- **R2 (attribution rule): RESOLVED.**
  - RELEASE.md step 2 now requires a local `make fuzz-hosted` on the tagged commit and a base-probe replay of every retained #6 input, each reproducing its signature on the base.
  - This is at least as strict as FUZZING.md's procedure. It drops the procedure's fallback of judging a `tool-call-ref` input on the branch's fixed-set probe, which is acceptable because it is stricter. No owner acceptance is needed.
- **R3 (minor wording): RESOLVED.**
  - The Makefile comment says "run", not "gate".
  - The README lists `make fuzz-hosted`.
  - The echo is now `$$target$${args:+ $$args}`. Under `sh` this prints `== fuzz target: json ==` and `== fuzz target: tool-call-ref --continue-on-known --timeout 120 ==`, and `make -n` shows the right recipe.
  - The `all\s+ten\s+targets` fixture matches across a line wrap.
- **Checks:** `check-docs.py` passes, and `test-check-docs.py` reports OK.

## Hosted evidence

- **Run 38330704651 (head `7e30c67`): conclusion success, every job passed.**
  - `windows-engines`: success (01:08:40-01:15:56), including the 1.98.0 Clippy step.
  - `fuzz` (`make fuzz-hosted`): success in 33m23s, against the 90-minute limit.
  - The eight non-tool-call targets each printed `Passed N inputs`, with no crash lines, mismatches or timeouts.
  - `tool-call --continue-on-known`: `Known issue #6 panic at execution` 600, 1002, 1078 and 1129, then `Passed 1408 inputs`.
  - `tool-call-ref --continue-on-known --timeout 120`: known #6 panics at executions 905 and 1044, then `Passed 1101 inputs`. It ran past the #11 input at execution 682 with no timeout.
  - Native Linux/macOS, MSRV, WASM, corpus and qualification also succeeded.
- **The hosted panics are the six inputs I attributed.** The hosted execution indices are identical to the coordinator's local runs. The stream is seeded and deterministic, so these are the same six inputs I replayed through the `943fa04` probe in the previous section. All six reproduced their #6 signature byte-for-byte on the base. For this code, the release attribution step is already satisfied in substance. The release procedure still requires redoing it on the tagged commit.
- **Run 38331969956 (head `cbecc8c`): in progress when checked.**
  - Since `7e30c67`, the only executed change is the Makefile echo, which I checked above. The rest is docs and a docs-check fixture.
  - Before the tagged-commit release gate, confirm that this run finished green.
  - The hosted log on `7e30c67` shows the trailing space this commit removes: `== fuzz target: json  ==`.
- **Run 38328660473 (head `f07b15b`, old `make fuzz`): `fuzz` failed as predicted.**
  - All eight non-tool-call targets passed.
  - `tool-call` stopped with only `CalledProcessError ... exit status 101` in the log, which confirms G1.
  - `windows-engines` succeeded.

## Advisories (non-blocking)

- **A1 [LOW]: misleading lines in the job log.** Before the first `== fuzz target` header, the `test-fuzz.py` self-test step prints lines such as `Probe crashed at execution 1; known issue signature: none` and `Known issue #6 panic at execution 0 ...` from mocked campaigns.
  - Someone searching the job log for `Probe crashed` will get false hits from the self-tests.
  - Fix: patch `sys.stdout` and `sys.stderr` in the older `_campaign` crash tests, as the three new tests do.
- **A2 [LOW]: two meanings of "qualification".** FUZZING.md uses "qualification runs" for the 10,000-mutation baseline campaigns, which stay strict. QUALIFICATION.md's "Qualification commands" list contains `make fuzz-hosted`, which uses the option. Each reading is correct in its own document, but a cross-reference would remove the ambiguity.

## Final coverage

| # | Requirement | Status | Evidence |
|---|---|---|---|
| 1 | Doc splits; comments only; 1.98 Clippy clean | COMPLETE | Local positive and negative controls; hosted `windows-engines` success on `f07b15b` and `7e30c67` |
| 2 | Pinned Node in the `fuzz` job | COMPLETE | Hosted install step and `pattern` passed (1011 inputs) |
| 3 | Fuzz gate accurate, log-applicable, no weakening | COMPLETE | `make fuzz-hosted` green on hosted `7e30c67`; #6 panics named in the log; other failures still fatal; base attribution required by RELEASE.md and done for the current six inputs; docs consistent and checked |

VERDICT: PASS
