# Merge-plan review: PR #9 (closes #8), head 42899ab, base 943fa04

Stage 3's single plan review. Reviewed on 2026-10-10 against the integration tree e77f41d.

## Inputs reviewed

- `/home/paudley/Active/purrassist/.worktrees/8-tool-call-recovery-arrays-of-nested.squash_notes` (687 lines, read in full)
- `validation.md` (Stage 1, Stage 2, Stage 2 exit, Stage 3 sections, read in full)
- `review-debt.md` (VERDICT: CLEAR), `pr-checks.txt`, `pr-comments.md` (all 19 comments), `raw/pr-reviews-final.json` (`[]`), `raw/pr-inline-comments-final.json` (`[]`), `raw/pr-threads-final.json` (totalCount 0), `raw/pr-9.json` (body grep only)
- `completion-audit-exit.md` (PASS, on 42899ab) and `completion-audit.md` (FINDINGS-OPEN, on 248d07a)
- `issue.md`, `plan.md`, `remediation-plan.md`, `squash-notes-draft.md`
- `tasks/T1-review.md`, `T2`, `T3`, `G-R1`, `R2`-`R15` (18 review files)
- `AGENTS.md`, `.goals`, `docs/IMPLEMENTATION.md`, `CONTRIBUTING.md`
- Merge-tree result as reported by the caller: `git merge-tree --write-tree --no-messages 943fa04 42899ab` exit 0, tree e77f41d == `42899ab^{tree}`; base == merge base; no semantic assessment dispatched
- Stage 3 command guidance for squash notes (`Defect-Class: TODO` must become "a real repo defect class or `none`"; no `GhpRsq-*` trailers)

Not available to me: `git` itself. The `## Files` stat block (556 files, +25,259/-9,669) and the 22 commit hashes could not be independently regenerated; the hash list was checked against the five most recent commits in the worktree's git status and against the hashes cited in `validation.md`, `pr-comments.md` and the task reviews.

## Focus 1: Is integration evidence sufficient for the actual candidate tree?

Yes.

- The PR base `main` is 943fa04, which is the merge base, and `merge-tree` yields e77f41d, which is exactly HEAD's tree. The candidate is therefore byte-identical to the tree that every piece of head evidence was produced on. Nothing from the base side enters the result, so there is no base-side change whose interaction with the branch could be assessed. Declining to dispatch a semantic assessment over an empty base delta is the correct call; dispatching one would have produced a vacuous report.
- Head evidence bound to 42899ab and reusable without re-running: `make check` exit 0; `make wasm-test` exit 0 (native, WASI core/syntax/host, browser WASM); bounded fuzz on both targets with 6 known #6 inputs attributed to base by identical panics and 0 branch-only failures; the Stage 2 exit completion audit (PASS), which itself ran `tool_call_goldens` (120), `correction_evaluation` (10, 18,176 cases, 0 incorrect), `tool_call_scale` (4), `test-verify-published.py` (10), an MSRV 1.98 `cargo check --workspace --all-targets --locked` with `-D warnings`, and a replay of all 727 retained fuzz inputs on the head.
- The gaps `make check` leaves (corpus lane, published replay) were closed by the auditor on the same head. That matters because the last recorded `make corpus` run was at R11, three code commits before the head.
- Completion-audit reuse rationale holds: HEAD is unchanged since the audit, the integration tree equals the head tree, and there is no new PR feedback (0 reviews, 0 inline comments, 0 threads). A fresh auditor would re-execute the same binary on the same inputs.
- Residual limits are stated honestly in `validation.md`: hosted CI not run; `make fuzz` in gate form not run on the head.

ADVISORY (A1): before `ghprsq` runs, re-confirm that `origin/main` is still 943fa04. The "no drift" finding is a snapshot; a push to `main` between this review and the merge would invalidate the identical-tree argument and require a fresh merge-tree.

## Focus 2: Does every comment, thread and check have an honest disposition?

Yes.

- 19 conversation comments, all by the workflow's own account, each dispositioned in `review-debt.md` with evidence. I re-read all 19 in `pr-comments.md`. The only question-like text is C2's conditional ("If you want abstention there, that is a policy change to decide") and C4's quoted user question; both are closed (the recursive-`$ref` case C2 raised was reclassified as defect G2 and fixed in e93eaf8, so the abstention question is moot rather than merely declined). No comment asks for a reply that is still owed.
- 0 review submissions, 0 inline comments, 0 review threads: nothing to disposition; the raw files confirm this.
- Checks: 8 Verify jobs show `fail` in 2-6 s without having started. `review-debt.md`, `validation.md` and the exit audit all say "did not run" / "never started", never "passed". The user decision to merge on local evidence is recorded verbatim.

ADVISORY (A2): PR comment C19 (already posted on GitHub) says of the 19 fix commits, including 342f2d4, 7136da7, c6ce650 and 42899ab, "Each one had an independent task review that passed." That is the same inaccuracy as the squash-notes claim in R1 below. It cannot be fixed by editing the notes; consider a one-line correction comment on PR #9 (or leave it, since the squash notes become the durable record).

## Focus 3: Do the squash notes claim anything the evidence does not show?

Verified as accurate against `validation.md`, the audits and the task reviews:

- Corpus 18,176 cases, 0 incorrect, abstention recall 100% (validation R2; audit re-ran on HEAD).
- Fuzz 1,408 (`tool-call`) and 1,101 (`tool-call-ref`) executions; 4 + 2 = 6 known #6 inputs; all 6 panic identically on a base 943fa04 probe; 0 branch-only failures (validation settle phase; audit replayed them).
- Budget constants 32 -> 176 work, 48 -> 368 scratch, new evidence per 7 bytes (R12 review; constants cross-checked in cost.rs/RESOURCES/INTEGRATION/README by the R12 reviewer).
- `OPERATOR_COUNT` 46, the three new variants, the new public fields, `const fn` loss on `ArgTree::value`/`empty_object`, and empty `usable()`/`usable_accepting_guesses()`/`emitted_calls()` on a failed choice (R2, R4, R5, R9 reviews).
- 22 commits listed, in the right order, hashes consistent with the stage record.
- Scope-decision quotes match the verbatim quotes in `remediation-plan.md` and the exit audit.
- `make check` exit 0 and `make wasm-test` exit 0 on 42899ab; MSRV 1.98 `cargo check` locally (auditor).
- F1, F2, F3 closed; already-satisfied behaviours identical to base; review debt clear; 19 status comments.
- No template markers, `TODO`/`TBD`, angle-bracket placeholders or `GhpRsq-*` trailers remain in the notes (grep over the file: no matches).

Claims the evidence does not support as written:

### R1 (REQUIRED): "Each of the 22 commits has an independent review that passed." (line 81)

Eighteen commits have a dedicated task review: c168175 (T1), 28b2b3a (T2), 248d07a (T3), c1fa81d (G-R1), e93eaf8 (R2), 4b80ade (R3), 69c6b5c (R4), 3111582 (R5), 3a19a2d (R10), 8fb63a4 (R12), bdb102a (R13), 3743aed (R14), 917553e (R6), c2f1916 (R15), a49e81b (R11), 8bca69e (R7), f93f0b4 (R8), 2484700 (R9). Four commits have none in `tasks/`:

- 342f2d4 `fix(tool-call): no seeded hashing in core` (HashMap -> BTreeMap; covered by `check-repository.py`, `ir` lib tests, goldens, clippy per validation.md; the R12 review's scope line names it only as its base);
- 7136da7 `test(receipt): refresh the invalid-UTF-8 tool-call receipt's usage` (Digest-rule refresh, usage-only; checked by the orchestrator, not a reviewer; the R15 review names it only as its base);
- c6ce650 `test(verify-published): refresh the replayed recover_tool_calls usage` (usage-only recapture from a clean worktree; orchestrator-checked);
- 42899ab `refactor(tool-call): one by-name lookup for tools and scope members` (helper-check fix during settle; "behaviour unchanged" rests on `make check` exit 0 and the exit audit, not on a review).

All four are covered by same-head gates (`make check`, full-crate tests, the exit completion audit on 42899ab), which is adequate evidence for merging. The claim must still be corrected because the sentence is false as written.

`REWRITE TO:` "Eighteen of the 22 commits carry an independent task review that passed (`tasks/*-review.md`; five of them first blocked on real defects that were fixed before commit). The other four (the `BTreeMap` substitution 342f2d4, the two usage-only fixture refreshes 7136da7 and c6ce650, and the final `by_name` dedup 42899ab) have no dedicated review; they are covered by the full-crate test runs, `make check` on 42899ab and the exit completion audit."

### R2 (REQUIRED): evidence attributed to 42899ab that was produced on an earlier tree (lines 65-71)

The list is headed "All on 42899ab, whose tree e77f41d is the integration tree". Three bullets do not meet that heading:

1. Line 68: "The base corpus on the head code reproduces the base tally." This was done at R2 time (tree of e93eaf8; `validation.md` R2/A4 and the R2 review, "by tally arithmetic", plus the implementer's per-case comparison). It was not repeated on 42899ab. What was done on 42899ab is the auditor's golden-set differential against base (463 identical, 2 reorder-only, 1 intended `usable_calls` change) and `correction_evaluation` with 0 incorrect.
   `REWRITE TO:` "Correction corpus on 42899ab: 18,176 cases, 0 incorrect, abstention recall 100%. At R2 (e93eaf8) the base corpus run on the then-head code reproduced the base tally; on 42899ab every modified golden was compared with base and only the intended changes remain."
2. Line 70: "llvm-cov shows mutations reaching the nested-item and scope-graph code." Coverage was measured on the R7 working tree before 8bca69e was committed (`validation.md` R7, `tasks/R7-campaign.md`). The settle-phase fuzz on 42899ab re-ran the campaigns (same execution counts, artefacts timestamped after the head commit) but no coverage figure is recorded for the head, and 42899ab itself changed `scope.rs` (`ScopeNode::member` now goes through `by_name`).
   `REWRITE TO:` "llvm-cov on the R7 tree showed mutated executions reaching the nested-item codec branches and the scope-graph runtime; the same bounded campaigns re-ran clean on 42899ab."
3. Line 71: "`make fuzz` in its gate form stops on #6 and #11, both pre-existing on main." `validation.md` line 219 records this as NOT RUN on 42899ab ("It would stop on #6 ... and on #11"), and the exit audit lists it as "expected to FAIL". The notes state an observation that was not made on this head.
   `REWRITE TO:` "`make fuzz` in its gate form (no `--continue-on-known`, default timeout) was not run on 42899ab; it would stop on #6 (`tool-call`) and #11 (`tool-call-ref`), both pre-existing on main and tracked."

### R3 (REQUIRED): hosted-CI sentence says more than "it did not run" (line 72)

> `- Hosted CI did not run. There is no branch protection or ruleset, so no required checks are configured, and the user decided to merge on local evidence.`

The second sentence does not state a cause for the non-run, but it attaches an explanatory clause to the CI statement, and the user's constraint for GitHub-facing text is "only that it did not run". A commit message is GitHub-facing. The clause also adds nothing a reader of the merge needs; the user decision is the operative fact.

`REWRITE TO:` "- Hosted CI did not run. The user decided to merge on local evidence."

Line 45 ("Hosted CI did not run; the user decided to merge on local evidence.") is already in the compliant form. The same configuration clause appears in `validation.md` (Stage 3 section) and `review-debt.md`; those are stage files under the untracked `.stage/` directory, not GitHub-facing, so no change is required there. The PR body (as captured in `raw/pr-9.json`) contains no CI reason; its only CI sentence is the Stage 1 handoff line "Hosted CI or a pinned Node must confirm it." The 19 PR comments say only "did not run" / "never started" / "has not run".

### R4 (REQUIRED): `Defect-Class: none` contradicts the body (line 686)

The notes' own "Why more changed than the issue named" section lists six fixes of pre-existing cost defects, each with a root cause: the repeated-parameter quadratic (G18, on base too), Mistral v11 re-segmentation (G21), Mistral v3 uncharged quadratic wall time (G22), two uncharged quadratics in provenance (G25), and the quadratic receipt validation log. Two further instances of the same class were filed as #10 and #11. Pre-existing `marker_cut` pointer resolution (G7) and `decoded`-by-pointer evidence (G6) were also fixed. The trailer exists so that recurrence becomes countable; `none` is the value for a change that fixes no repo defect, and this change fixes a recurring one. Sibling repositories in the same workflow use `Defect-Class: performance` for exactly this shape, and more specific hyphenated classes are also in use.

`REWRITE TO:` "Defect-Class: performance" (or a single more specific class such as "uncharged-superlinear-work" if the repo wants finer counting; one class per trailer, matching precedent).

### Lesser claim-precision items (ADVISORY)

- A3, line 22: "Each was fixed with a root cause, an identity oracle (goldens unchanged without a re-bless, plus differentials against the previous engine) and a test that fails on the old code". Differential CLI runs against the previous head are recorded for R6, R11, R13, R14 and R15. R5 (item numbering) and R10 (repeated-parameter collection) relied on goldens-unchanged, corpus and pinned digests, with no differential run; the R5 reviewer did not execute the mutation checks ("plausible by reading"). Soften to "goldens unchanged without a re-bless and, for the engine and provenance changes, differentials against the previous engine".
- A4, line 78: "re-executed every in-scope #8 criterion and every Stage 2 gap input". The exit audit re-executed the gap inputs the remediation plan's "Settle and exit" section named (G2 set, G3 p7, G6 p6, G7 p12/p12j, G14, G16, plus R11 and cost spot checks). Test-level gaps (G5, G8, G9, G13, G17, G25) were verified through their tests, not re-executed inputs. "every Stage 2 gap input the remediation plan named" is exact.
- A5: the notes omit the corpus-generator truth correction from T1 (two inline-closer mutants now skip newline-final values; the T1 orchestrator promised to name it in the PR body, and the PR body does). One line under "Design decisions and declines" or "Compatibility" would make the squash record self-contained.
- A6: the `## Files` stat and the "556 files changed" line could not be regenerated here; the caller should confirm they came from `git diff --stat 943fa04..42899ab` on the current head.

## Focus 4: Is any #8 requirement or accepted Stage 2 gap silently missing or moved to follow-up without a recorded user decision?

No.

- Issue #8 ask 1 (nested-parameter array items, including inside an object, mixed with JSON items, unclosed last item, String-item negative, and the five goldens the issue lists): all VERIFIED by the exit audit on the shipped CLI and MCP, with the issue's exact `plan_outfits` bytes returning the exact expected JSON, Resolved, single reading.
- Ask 2 (strict forced choice; goldens for two calls and call-plus-prose; recorded in the `tool_call.tool-choice` validation): VERIFIED, including the Stage 2 fix for rejected second attempts (F2).
- Ask 3 (marker-free reasoning range beside the outer range): VERIFIED, including `starts_open` text from byte 0 and the G14 reversed-range fix.
- Already-satisfied list: all ten items re-run head against base, identical.
- Stage 2 gaps G1-G26: every FIXED gap maps to a commit and a passing review; G12 (hosted CI) is reported, not claimed; G23 is documented by user decision; G24 -> #10 and G26 -> #11 under verbatim user decisions, each cited in `docs/RESOURCES.md` (and `FUZZING.md` for #11), and the auditor re-ran both repros on head and base with identical results. The remediation plan's two declines (performance advisory 2; #6) carry reasons and appear in the notes.
- Plan-review REQUIRED findings F1-F13: C19 reports all 13 resolved; F1/F2/F3/F5/F6/F9/F10/F12 are traceable in the remediation plan and the R2/R4/R7/R3 reviews.

ADVISORY (A7): the R6 review's ADVISORY 1 (a bare unquoted reasoning marker inside a JSON value is repaired to a string by `json.syntax.bare-string` and then not cut, pre-existing at HEAD for single occurrences; "consider a note or follow-up") has no recorded disposition in `tasks/R6-review.md` (the orchestrator dispositioned only ADVISORY 3) or anywhere else in the stage directory. It is not a #8 requirement and is pre-existing, so it does not block the merge, but it should get a one-line disposition (document in INTEGRATION.md's "reasoning markers are never call content" guarantee, or file an issue) so it is not lost.

## Focus 5: Defect-Class value and title

- Defect-Class: see R4 above (REQUIRED).
- Title: `feat(8): tool-call recovery: nested-parameter array items, strict forced choice, marker-free reasoning blocks (#9)`. Concrete, names all three delivered behaviours, carries the PR reference, and matches the Stage 2 draft subject. Two ADVISORY points:
  - A8: the squash absorbs `8fb63a4 perf(tool-call)!: ...` and the body lists twelve Rust API breaks under "Compatibility". Conventional-commit practice (which the branch follows, including the `!` marker) would carry the breaking marker on the squash subject: `feat(8)!:` or `feat(tool-call)!:`. No repo document mandates this (CONTRIBUTING.md is silent on commit format), so it is consistency, not a rule.
  - A9: the branch's own commits use `(tool-call)` as the scope and `(#8)` as a trailing reference; the squash uses the issue number as the scope. If `stagectl squash-notes` seeded `feat(8):`, keep it; otherwise `feat(tool-call):` would match the history on `main`. Also confirm that `ghprsq` does not itself append a second `(#9)`.

## Focus 6: Leftover template markers

None. Grep of the notes for `TODO`, `TBD`, `FIXME`, `XXX`, `{{`, `[[`, `<placeholder>`-style angle-bracket tokens, `PLACEHOLDER`, `fill in`, `<timestamp>`: no matches. No `GhpRsq-*` line is present. The draft's `TODO (Stage 3: ...)` under Defect-Class was replaced (by `none`; see R4 for the value itself). The draft's "PENDING: hosted CI, Stage 3 merge-tree and preflight" line was correctly replaced by the Integration and Hosted CI bullets.

## Design and standing-goals check (brief)

Nothing at Stage 3 reopens Stage 1/2 design decisions, but for the record: no new optionality was introduced beyond what the issue asked for (`StrictForced`'s three booleans are the integrator's requested contract options, surfaced identically on Rust, CLI and MCP, with the degenerate all-lenient form collapsing to `Forced`); failure is fail-closed and whole-turn; `purrassist-core` stays dependency-free and ambient-free (the one `HashMap` regression was caught and replaced); every exhaustion is explicit; guesses are never Resolved; charged work tracks real work on every measured shape except the two user-accepted issues. SPDX header confirmed on the new `scope.rs` by the R2 reviewer. These are consistent with `.goals` and `AGENTS.md`.

## Findings summary

REQUIRED (all in the squash notes; none in the code or the merge mechanics):

- R1: line 81, 22-commit review claim is false (18 of 22). Rewrite given.
- R2: lines 68, 70, 71 attribute R2-time / R7-tree / not-run evidence to 42899ab. Rewrites given.
- R3: line 72, trim the hosted-CI bullet to "did not run" plus the user decision. Rewrite given.
- R4: line 686, `Defect-Class: none` contradicts the six pre-existing defect fixes in the body; use a real class (`performance` per precedent).

ADVISORY:

- A1: re-confirm `origin/main` == 943fa04 immediately before `ghprsq`.
- A2: PR comment C19 carries the same 22-commit claim; optional correction comment.
- A3: soften "plus differentials against the previous engine" (not done for R5, R10).
- A4: "every Stage 2 gap input" -> "every gap input the remediation plan named".
- A5: add one line on the T1 corpus-mutant truth correction.
- A6: confirm the `## Files` stat was regenerated from the current head.
- A7: record a disposition for R6 review ADVISORY 1 (bare unquoted marker repaired to string).
- A8: consider the `!` breaking marker on the squash subject.
- A9: scope `(8)` vs `(tool-call)`; confirm `ghprsq` does not double the `(#9)`.

## Verdict

The integration evidence is sufficient for the candidate tree, every comment and check has an honest disposition, no #8 requirement or accepted gap is missing or laundered, and the code is ready to merge on the recorded user decision. The squash notes, which become the durable record, overstate review coverage, misattribute three pieces of evidence to the head, carry one clause too many on hosted CI, and understate the defect class. Fix R1-R4 in `${WORKTREE}.squash_notes` and re-run the leftover-marker sweep; no re-validation of the tree is needed.

VERDICT: NEEDS-REVISION

## Recheck

Scope: the coordinator's edits to `${WORKTREE}.squash_notes` for R1-R4 and A3, and the reported edits to the Stage 2 summary comments on issue #8 and PR #9. Re-read the notes (lines 1-120 and the trailer) and grepped them for `Defect-Class`, `GhpRsq`, `TODO`, `TBD`, `branch protection`, `ruleset`, `required checks`.

- **R1 (line 81): resolved.** The sentence now reads "Eighteen of the 22 commits carry an independent task review that passed (`tasks/*-review.md`); five of those reviews first blocked on real defects that were fixed before commit. The other four (the `BTreeMap` substitution 342f2d4, the usage-only fixture refreshes 7136da7 and c6ce650, and the `by_name` dedup 42899ab) have no dedicated review. They are covered by the full-crate test runs, `make check` on 42899ab and the exit completion audit." The count "five" is correct: R3 (stale golden description), R6 (uncharged quadratic), R9 (missing Rust API list), R10 (wrong doc numbers) and R15 (no revert-detecting test) each carry a first `VERDICT: BLOCKED` followed by a recheck PASS. The four unreviewed hashes match my list. Light re-wording relative to my text (sentence split, "the two" dropped) changes no claim.
- **R2 (lines 68, 70, 71): resolved.** Line 68 now attributes the base-corpus tally replay to R2 (e93eaf8) and the golden comparison to 42899ab; line 70 attributes llvm-cov to the R7 tree and says the campaigns re-ran clean on 42899ab; line 71 says the gate-form `make fuzz` "was not run on 42899ab. It would stop on #6 (`tool-call`) and #11 (`tool-call-ref`), both pre-existing on main and tracked." Each matches `validation.md` (R2/A4, R7, settle phase line 219) exactly. The "All on 42899ab" heading is now true for every bullet beneath it.
- **R3 (line 72): resolved.** "- Hosted CI did not run. The user decided to merge on local evidence." No other line in the notes mentions branch protection, rulesets or required checks (grep: no matches). Line 45 is unchanged and compliant.
- **R4 (line 686): resolved.** `Defect-Class: performance`. It is the only `Defect-Class` line; no `GhpRsq-*` trailer was added. Given that this repo has no prior trailers, this value becomes the repo's first countable class, which is consistent with the body's six pre-existing cost fixes and with the sibling-repo precedent I cited.
- **A3 (line 22): resolved, one nit.** Now "an identity oracle (goldens unchanged without a re-bless and, for the engine and provenance changes, CLI differentials against the previous head)". Accurate for R6, R11, R13 and R14, which were CLI-to-CLI differentials. R15's identity evidence was a release-build comparison of full `Usage` plus an FNV hash of outcome and alternatives against a detached previous-head worktree, which is a differential against the previous head but not literally through the CLI. Non-blocking; "differentials against the previous head" (without "CLI") would be exact.
- **C19 edits on issue #8 and PR #9: not independently verifiable here.** I have no forge access and the local `pr-comments.md` capture predates the edit. The coordinator's description (replace the 22-commit claim with the 18-plus-4 statement) is the right correction; if the workflow archives PR comments, refresh `pr-comments.md` so the stage record matches what is posted. Not a merge condition.
- **New text introduced by the edits:** none beyond the sentences quoted above; no template markers, no `TODO`/`TBD`, no `GhpRsq-*` lines (grep confirmed). The title and the commit list are unchanged.

All four REQUIRED findings are closed. Remaining advisories (A1, A2's archive refresh, A4-A9, and the A3 nit) are non-blocking and need no further review round.

VERDICT: PASS
