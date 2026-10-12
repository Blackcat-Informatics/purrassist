# Stage 3 merge-plan review: PR #20 (issue #1)

- Reviewed: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and.squash_notes` (squash notes), with `validation.md` (Stages 1-3), `review-debt.md`, `gap-analysis.md`, `plan.md`, `remediation-plan.md` (by reference from gap-analysis), `output-rules.md`, `tasks/T5-review.md`, `raw/pr-20*.json`, `pr-checks.txt`.
- Head `93c02b3`, base `origin/main` `f11b45e`.
- Tooling limit: this reviewer had no shell, so `git merge-tree` and `gh api .../rulesets/24943594` could not be re-executed. Integration was checked from the git ref and reflog files instead (below). The ruleset facts are taken from `review-debt.md` / `validation.md` and were not re-queried.

## Coverage

### 1. Integration evidence for the actual candidate

I recomputed this from the repository's ref files. I could not run `merge-tree`.
- `refs/heads/paudley/1-complete-purrassist-0-1-0-plan-and` = `93c02b3a58...`, and `refs/remotes/origin/paudley/1-complete-purrassist-0-1-0-plan-and` = `93c02b3a58...`. The pushed head equals the local head.
- `refs/remotes/origin/main` (loose ref, which overrides `packed-refs`) = `f11b45ece8...`. The worktree's `FETCH_HEAD` also records `main` at `f11b45ece8...`. The last `origin/main` reflog entry (`fccfbe7 -> f11b45e`) comes before the branch's creation.
- The branch reflog opens with `branch: Created from f11b45ece8...` and then lists exactly 9 linear `commit:` entries: `dbdc64d`, `871af51`, `f11912e`, `f0d978a`, `a42a817`, `4933cc4`, `d6fb1e0`, `feefaae`, `93c02b3`. There are no rebases or amends. The worktree `logs/HEAD` shows only no-op `reset: moving to HEAD` entries.
- So `f11b45e` is the merge base and an ancestor of `HEAD`. A three-way merge whose base equals one side always yields the other side's tree. The recorded result ("tree `8b2f718` == `HEAD^{tree}`") is therefore structurally forced and consistent. I did not verify the tree hash itself.
- Gate evidence reuse is sound. The `make check` and `make package` final run (`validation.md`, "Stage 2 final gate") is recorded on `93c02b3a5836...`, which is the merge candidate. The Stage 2 verdict's conditions (`gap-analysis.md`, "Verdict conditions") are met.
- Residual: the `origin/main` value is the Stage 3 fetch, not live state. The merge executor must re-fetch and confirm `main` is still `f11b45e` (or re-run merge-tree) immediately before merging. This is ADVISORY A4.

### 2. Feedback and check dispositions

- PR feedback: 4 owner status comments, 0 reviews, 0 inline comments, 0 threads (`raw/pr-20-merge.json`: `"reviews":[]`). Each comment has a disposition in `review-debt.md`. The notes' "no reviews, inline comments or threads" is accurate.
- Checks: 8 jobs in `pr-checks.txt`, 2 FAILURE (`windows-engines`, `fuzz`). Both have causal analysis in `review-debt.md`: the lint is in untouched `cost.rs` and `tool_call/`, and the runner's Node version is not pinned. Per the owner decision, no check is required. The notes disclose both failures, and `docs/RELEASE.md:238` does require "Verify hosted CI (`verify.yml`) for that commit". The notes' "still requires a hosted pass" is supported.

### 3. Numbers in the notes (spot-checked)

| Claim | Evidence | Result |
|---|---|---|
| 251 tests | `scripts/test-check-docs.py` has 251 `def test_` methods; `gap-analysis.md` recheck "251 tests OK"; `validation.md` G11 follow-up | holds |
| 110 test groups | `validation.md` "Stage 2 final gate": 110 `test result: ok` groups on `93c02b3` | holds (recorded) |
| 31 tools | `docs/TOOLS.md` has 31 `### ` headings, and the checker enforces exactly one per registry name; `gap-analysis.md` MCP `tools/list` returns 31 | holds |
| six archives | `validation.md`: "Verified 6 release archives" for source commit `93c02b3` | holds |
| 9 commits | reflog lists 9, and the subjects match the notes verbatim | holds |
| known limitations #6, #7, #10-#14, #16-#18 | `docs/known-limitations.json` issues 6, 7, 10, 11, 12, 13, 14, 16, 17, 18 | holds |
| 13 topics, wiki and projects off, PVR on | `tasks/T5-review.md` live output | holds |
| 248 tokens / 14 links / 2 | `validation.md` Task 2 | holds (see A3 on wording) |
| "All hygiene scripts use explicit UTF-8 and run with `EncodingWarning` as an error" | `Makefile:14`, `:19`, `:20` | **does not hold** (B1) |
| Files stat (61 files, +6145/-627) | not recomputed (no shell) | unverified |

### 4. Requirements silently missing?

- Every contract row in `plan.md` has a status in `gap-analysis.md`'s final table. The one undelivered item, the social-preview upload (row 15), is disclosed in the notes: "The social-preview upload was not done ... It is an owner step."
- Hosted CI and #6, #7, #10 and #11 are excluded explicitly, per the owner decision.
- Requirement-level coverage has no silent cut. One owner step is missing from the notes' list of remaining steps (A1).

### 5. Trailers, closing keywords and excluded wording

- Notes open with `Refs #1` and "Issue #1 stays open". The PR body (`raw/pr-20.json`) also opens `Refs #1. Issue #1 stays open`.
- A case-insensitive search for `close[sd]?|fix(e[sd])?|resolve[sd]?` followed by `#N` finds no matches in the notes or the PR raw files. The only match is a 2026-09-28 issue comment, which is not part of this merge.
- No co-author trailer, generated-by credit, agent names or agent-vendor email domain appear in the notes.
- No hosted-CI account or payment wording appears in the notes.
- `Defect-Class: documentation-drift` is a classification trailer, not a provenance or attribution claim. It has no precedent in tracked files, but no rule forbids it.

## Findings

### BLOCKING

**B1. The encoding claim overstates what the Makefile does.**
- Notes line 25 says: "All hygiene scripts use explicit UTF-8 and run with `EncodingWarning` as an error."
- `make hygiene` runs nine Python lines. Three of them run without `-X warn_default_encoding -W error::EncodingWarning`:
  - `Makefile:14` `python3 scripts/check-release.py`
  - `Makefile:19` `python3 scripts/test-resource-report.py`
  - `Makefile:20` `python3 scripts/check-resource-limits.py --schema-only ...`
- Two of those scripts still use locale-default I/O: `scripts/check-resource-limits.py:39` `DOC.read_text()` and `:246`, and `scripts/test-resource-report.py:38`, `:131`, `:211-223`, `:237` (`write_text(...)`/`read_text()` with no encoding).
- `validation.md` itself says only "EncodingWarning as an error on all seven script lines". The squash message would put a false portability claim permanently into `main`'s history.
- REWRITE TO: "`check-docs.py`, `test-check-docs.py`, `check-repository.py`, `test-repository-audit.py`, `test-check-release.py` and `test-fuzz.py` use explicit UTF-8 and run in `make hygiene` with `EncodingWarning` as an error; `check-release.py` and `fuzz.py` were also made explicit."

### ADVISORY

**A1. Remaining owner steps are incomplete.**
- Notes lines 3-5 list the follow-on steps as "tagging `rust-v0.1.0`, registry publication, downloaded-package acceptance and the GitHub release". Line 81 adds the social-preview upload.
- The notes omit the derive trusted-publisher configuration. `plan.md` "Out of this stage by owner decision" lists it, the PR body lists it, and it is a prerequisite for publication.
- REWRITE TO (append to line 81's bullet list): "- The `purrassist-derive` trusted-publisher configuration is an owner step before publication."

**A2. The live entry-point runs are not bound to a commit.**
- Notes lines 89-92: "**Live entry points:** real CLI `call match_strings`; a real MCP ... session (31 tools); the read-only publish preflight".
- The bullet sits directly under "Final full run on head `93c02b3`". The CLI and MCP runs were made on `a42a817` (`gap-analysis.md` section (f)), and the preflight was made during Task 3 (`validation.md`). The `93c02b3` delta is comment-only, and the `protocol` tests that drive both transports ran inside `make check` on `93c02b3`, so the substance holds.
- REWRITE TO: "**Live entry points (run on `a42a817`; later commits change comments, docs and checkers only, and the CLI/MCP protocol tests ran in the `93c02b3` gate):** ..."

**A3. Migration wording.**
- Notes line 67: "with 2 deliberate respellings".
- `validation.md` calls them "Deliberate removals". One token is a respelling (`DIRECTORY` to `DIR`). The other replaces the sentence that contained `make fuzz TARGET=json` with `make fuzz TARGET=<target>`.
- REWRITE TO: "with 2 deliberate removals, both restated in the new text."

**A4. Re-confirm the base at merge time.**
- The integration claim (notes line 102, and "None. The base has not advanced since the branch point.") is true as of the Stage 3 fetch.
- The merge step must re-fetch `origin/main` and confirm it is still `f11b45e` (or re-run `git merge-tree --write-tree --no-messages origin/main HEAD` and compare it with `HEAD^{tree}`) right before merging. It must also pass the expected head SHA `93c02b3` so that a moved head aborts the merge.

**A5. Hosted-CI framing (output-rules.md).**
- Notes line 93: "Hosted CI: not a gate for this work (owner decision), ..."
- `output-rules.md` says to refer to hosted CI "only as out of scope for this stage". Line 60 complies, but line 93 uses different framing. The results that follow are honest disclosure and contain no excluded topic.
- REWRITE TO: "**Hosted CI: out of scope for this work (owner decision); the `main` ruleset requires no status check.**"

VERDICT (initial review, superseded by the recheck below): BLOCKED

---

## Recheck after the notes correction

I re-read the squash notes in full (206 lines). Only the text added by the fixes was reviewed again.

### Prior findings

| Finding | Status | Evidence |
|---|---|---|
| B1 (BLOCKING): the encoding claim | **DISCHARGED** | See "B1 detail" below. |
| A1: derive trusted publisher | **DISCHARGED** | The new "## Remaining owner steps" (lines 100-105) lists: 1. the crates.io trusted publisher for `purrassist-derive`; 2. the social-preview upload; 3. tag authorization, with the hosted pass per `docs/RELEASE.md`; 4. after publication, the docs.rs homepage and `server.json`. This matches `plan.md` Task 6 and the PR body. |
| A2: live entry points bound to a commit | **DISCHARGED** (see N1) | Line 89 now reads "(run on `a42a817`; later commits changed only comments, docs and checkers, and these entry points are covered at `93c02b3` by `make check`'s protocol tests)". |
| A3: migration wording | **DISCHARGED** | Line 67 now reads "2 deliberate removals, both restated in the new text". |
| A4: base at merge time | **DISCHARGED** | See "A4 detail" below. |
| A5: hosted-CI framing | **DISCHARGED** | Line 93 now reads "Hosted CI: out of scope for this work (owner decision), and no status check is required by the `main` ruleset." |

**B1 detail.** Line 25 now names exactly `check-docs.py`, `test-check-docs.py`, `check-repository.py`, `test-repository-audit.py`, `test-check-release.py` and `test-fuzz.py`.
- These are the six flagged hygiene lines, `Makefile:12`, `:13`, `:15`, `:16`, `:17` and `:18`.
- "`check-release.py` and `fuzz.py` were also made explicit" holds. A grep over those two scripts and the six above finds no `read_text()`, no `text=True` and no `write_text` without `encoding=`. The only `open` calls are `urlopen` and `tarfile.open`, which are binary.
- "The older resource-report scripts are unchanged" holds. Neither `test-resource-report.py` nor `check-resource-limits.py` is in the notes' file list.

**A4 detail.**
- The ref files I read again agree with the coordinator's re-fetch: `refs/remotes/origin/main` = `f11b45ece8...`, the worktree `FETCH_HEAD` `main` = `f11b45ece8...`, and the branch ref = `93c02b3a58...`.
- The coordinator reports `git merge-tree --write-tree --no-messages origin/main HEAD` = `8b2f7182a3ac3675d1d222713bdebfec81d05c28` = `HEAD^{tree}`. That is structurally forced, because the base is an ancestor of the head.
- The merge must still pin the expected head `93c02b3`. That is an execution requirement, not a notes defect.

### New text

- **Closing keywords and wording.** Lines 25, 67, 89, 93 and 100-105 contain no closing keyword, attribution, or hosted-CI account or payment wording. A rerun of the case-insensitive search over the whole notes file finds nothing.
- **N1 (ADVISORY, LOW):** in line 89's parenthetical, "these entry points are covered at `93c02b3` by `make check`'s protocol tests" also takes in the third bullet, the read-only publish preflight.
  - The protocol tests do not cover the preflight. It is a crates.io ownership read that does not depend on the commit, so the claim is harmless in substance.
  - REWRITE TO (optional): "(CLI and MCP run on `a42a817` and covered at `93c02b3` by `make check`'s protocol tests; the publish preflight is a registry read that does not depend on the commit)".
  - This does not block.

No blocking finding is open. The notes' claims are supported by the recorded evidence. Issue #1 stays open (`Refs #1`, no closing keyword). The integration candidate equals the tested head tree on a base that has not moved.

VERDICT: PASS
