# Merge-plan review: PR #22 (issue #1), head 8ce1ca6, base main e0a7ecc

- Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and.squash_notes`
- Supporting evidence read: `validation.md`, `review-debt.md` (PASS), `gap-analysis.md` (final PASS at cbecc8c), `pr-checks.txt`, `AGENTS.md`.
- Source read on the head: `scripts/test-fuzz.py`, `scripts/fuzz.py` (`main`), `Makefile` (`fuzz`, `fuzz-hosted`, `hygiene`), `.github/workflows/verify.yml`, `docs/RELEASE.md` step 2, `docs/QUALIFICATION.md`, `docs/FUZZING.md` lines 864-953, crate `Cargo.toml`/README layout.

## Tooling limitation (read first)

This reviewer had only file read, search and write tools. It had **no shell**, so it did **not** execute any of the three commands the caller named:

- `git merge-tree --write-tree --no-messages origin/main HEAD`
- `gh run view 38333691731 --repo Blackcat-Informatics/purrassist --json jobs,headSha`
- `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py`

Each is covered below by direct evidence from the repository's own files and by the recorded evidence. None of the output below is presented as if those commands were run here. If the merge procedure requires this reviewer to have run them, rerun them with a shell before merging. The evidence below suggests they will pass.

## 1. Integration (merge-tree)

Direct evidence from git metadata files:
- `.git/worktrees/1-complete-purrassist-0-1-0-plan-and/logs/HEAD`: the branch starts at `e0a7ecc519dc...` and is a linear chain `2864e75 -> f07b15b -> 7e30c67 -> cbecc8c -> 8ce1ca6`, all ordinary commits with no merges and no rebases.
- `.git/logs/refs/remotes/origin/main` ends at `e0a7ecc519dc...`. The worktree's `FETCH_HEAD` records `e0a7ecc` for `main` and `8ce1ca6` for the PR branch. `refs/remotes/origin/paudley/1-complete-...` is `8ce1ca6`.

Because `origin/main` (`e0a7ecc`) is HEAD's direct ancestor, `merge-tree` must return HEAD^{tree}. That supports the notes' "The merge-tree tree equals the head tree". Caveat: this only holds while remote `main` is still `e0a7ecc` as of the last fetch. Confirm with a fresh fetch at merge time. Conflicts: none possible.

## 2. Hosted run 38333691731 on the final head

Not queried live here. Corroboration:
- `review-debt.md` records live read-only queries against head `8ce1ca6f76cf...` with all eight checks COMPLETED/SUCCESS. Those checks are native ubuntu and macos, msrv, windows-engines, wasm, corpus, fuzz and qualification.
- `pr-checks.txt` shows the same eight checks as SUCCESS, `state: pass`.
- `validation.md` line 20 names the run, the 8 jobs and the six #6 executions (tool-call 600/1002/1078/1129, tool-call-ref 905/1044), with no mocked lines.
- Ruleset 24943594 requires no status checks. Merge is squash-only and requires signed commits.

## 3. Notes claim nothing beyond the evidence

| Notes claim | Evidence | Status |
|---|---|---|
| 1.98.0 Clippy: exactly three `too_long_first_doc_paragraph` errors before the change, exit 0 after | gap-analysis Change 1: negative control on base `e0a7ecc` reports exit 101 with exactly cost.rs:93, emit.rs:1130 and ir.rs:600; positive run exits 0 | Supported |
| "Local `make check` runs nightly" | `rust-toolchain.toml` channel `nightly-2026-09-14` | Supported. The "which is why" is a reasonable inference, not a separate claim. |
| 107 test groups OK on the lane's `cargo test` commands | validation.md line 5 | Supported |
| Local campaigns: 1,408 inputs with 4 retained #6 panics; 1,101 inputs with 2, past #11 at 682 | validation.md lines 11-12; gap-analysis hosted section | Supported |
| Six inputs replayed through base `943fa04`, all reproduce #6 | gap-analysis recheck table (ff4c7266 ... e806e7b8) | Supported |
| Run 38330704651 on `7e30c67`: every job passed, fuzz in 33m23s, same six inputs | gap-analysis final recheck; validation.md line 19 | Supported |
| Run 38333691731 on `8ce1ca6`: all eight jobs, same six named inputs | validation.md line 20; review-debt.md; pr-checks.txt | Supported by records, not re-queried here |
| `make check` exit 0 on `7e30c67` (110 groups) | validation.md line 17 | Supported. The notes correctly do not claim `make check` on the final head. |
| `make package` exit 0 on `f07b15b`; later commits touch no crate file | validation.md line 16. The files list shows crate changes only in `2864e75`. Crates package their own `crates/*/README.md` (none touched), not the root README. The hosted `qualification` job also runs `make package` on `8ce1ca6`. | Supported |
| `make hygiene` exit 0 on the final head | validation.md line 19 | Supported |
| "PR feedback: no comments, reviews or threads" | review-debt.md: all counts 0 | Supported |
| Mocked self-test campaigns no longer print look-alike lines | Code read (section 4) and validation.md line 20 ("no mocked lines") | Supported |

No claim exceeds the recorded evidence. "Hosted Verify run ... every job passed" (7e30c67) and "passed all eight jobs" (8ce1ca6) agree with the records.

## 4. Correctness of 8ce1ca6

**`scripts/test-fuzz.py` `_campaign` (lines 326-363):**
- An `ExitStack` redirects `sys.stdout` or `sys.stderr` to a `StringIO`, but only when the stream is still the interpreter original (`sys.stdout is sys.__stdout__`).
- `fuzz.py` resolves its streams at call time:
  - `print(..., flush=True)` for the `Campaign artifacts`, `Known issue #6 panic`, `Failure retained` and `Passed` lines;
  - `print(..., file=sys.stderr)` for the `Probe crashed` line and the panic tail (fuzz.py lines 2502-2607).
- `fuzz.py` has no module-level stream capture and no `logging` or `os.write` path (grep). All mocked-campaign output is therefore swallowed.

**The three capture tests still see their output:**
- `test_a_crash_prints_...` and `test_an_unknown_crash_...` patch `sys.stderr` with their own `StringIO`. The identity check then skips the stderr redirect, so their assertions still see the text. stdout is still redirected.
- `test_a_retained_known_panic_...` is the mirror case: it patches stdout, and stderr is redirected.

**Exceptions propagate correctly.** `CalledProcessError` is caught inside the `with`, so the redirects unwind cleanly. Any other exception propagates normally and unwinds the stack.

**Encoding and the CI run:**
- The new code adds no `open` or `read_text` calls without an explicit encoding, so `-W error::EncodingWarning` cannot newly fire. The existing `read_text(encoding='utf-8')` calls are explicit.
- This exact command (`python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py`) is the first line of both `make fuzz-hosted` (Makefile line 79) and `make hygiene` (line 18). The hosted `fuzz` job on `8ce1ca6` and the recorded `make hygiene` on the final head both executed it successfully.

**`docs/QUALIFICATION.md` lines 233-237:**
- New text: "The 10,000-mutation baseline qualification runs in FUZZING.md stay strict." This is the gap-analysis A2 cross-reference.
- It is accurate against FUZZING.md lines 901-913 and 942-943 ("The default `make fuzz` and qualification runs never use the option and stay strict").
- It is consistent with QUALIFICATION's command list (`make fuzz-hosted`, line 246) and RELEASE.md step 2.

Both gap-analysis advisories (A1 log noise, A2 two meanings of "qualification") are discharged.

## 5. No requirement silently missing

The PR's scope is to make hosted `verify.yml` green for the release procedure. All three gap-analysis requirements are COMPLETE with executable evidence:
1. The doc splits (comments only) make 1.98 Clippy clean.
2. The `fuzz` job installs the pinned Node.
3. The hosted fuzz gate `make fuzz-hosted` runs all ten targets. Only #6 signatures are retained, and every other failure stays fatal. #11 gets a 120 s timeout. RELEASE.md requires base attribution. The docs are consistent and checked.

The accepted limitations #6 and #11 stay visible as non-blocking. The notes say "keeps the accepted 0.1.0 limitations visible", and the default `make fuzz` is unchanged and still stops on them. Issue #1's wider 0.1.0 work (tag, publication, release) is explicitly left open, not claimed.

## 6. Trailers, closing keywords, attribution, wording

- `Refs #1.` with "Issue #1 stays open until the tag, publication and release." No `Closes`, `Fixes` or `Resolves` keywords; checked case-insensitively.
- The only trailer is `Defect-Class: none`. There are no `GhpRsq` trailers, no co-author trailer, no agent credit or generated-by text, and no agent email.
- There is no wording about hosted-CI accounts, billing, payments, quotas or minutes. The only duration mentioned is the fuzz job's 33m23s runtime.

## Findings

BLOCKING: none.

ADVISORY: none against the notes text. The only caveat is procedural and is recorded above. The three commands were not executed by this reviewer for lack of a shell. Section 1 needs a fresh `origin/main` at merge time.

VERDICT: PASS
