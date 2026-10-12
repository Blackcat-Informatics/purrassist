# Merge-plan review: PR #21 (issue #1), base main 3037418

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and.squash_notes`

The initial review was at head `edcc413`. The recheck below is at head `e34c665`.

Evidence read: `validation.md`, `review-debt.md`, `gap-analysis.md` (initial review and rechecks 1-4),
`docs/RELEASE.md`, `scripts/check-release.py`, `scripts/test-check-release.py`,
`.github/workflows/publish.yml`, `Makefile` (hygiene target), `AGENTS.md`, the worktree reflog,
`refs/remotes/origin/main`, `FETCH_HEAD` and `COMMIT_EDITMSG`.

## Execution limit

This review session has no shell tool. I could not run `git merge-tree`, the Python scripts or
`make hygiene`. My results come from reading the code and the git files. The coordinator's
command output, now recorded in validation.md, supplies the executed evidence.

## Initial review (head edcc413)

### Coverage

| Check | Result |
|---|---|
| Integration | Supported by ancestry; I did not recompute it. `refs/remotes/origin/main` = `3037418`, and `FETCH_HEAD` agrees. The reflog runs straight from `3037418` through each listed commit, so `origin/main` is the merge base and the merge-tree tree equals `HEAD^{tree}`. |
| R7 regex (check-release.py:165) | It matches `!<tag:yaml.org,2002:str>` and `!str`. It does not match the shell line at publish.yml:53 (`! echo ... .status!="completed"`): the first `!` is followed by a space, and the second follows `s` and precedes `=`. publish.yml has no backslash. |
| R7 tests | Subtests cover `!<...>` and `!str`. `test_real_workflow_passes` covers the shell `! echo` line. There are 29 tests: 18 in `WorkflowValidation` and 11 in `Preflight`. |
| R8 re-wrap | Discharged. The RELEASE.md paragraph is reflowed to about 80 columns. |
| Requirements | Coherent: RELEASE.md:50, 61-62, 93-104 and 252-253. No "pending" or "configured first" text remains in the other docs. Nothing is silently missing. |
| Trailers, closing keywords, attribution, hosted-CI wording | None found. The notes use `Refs #1` only. `Defect-Class:` follows the project's own precedent (issue.md:3306). There is no wording about accounts or payments. |
| Tag-time risk statement (notes:36) | Accurate, and it does not claim the control (tag protection) exists. |

### Findings

- **B1 (BLOCKING).** Notes line 18 said "YAML tags of any form", and RELEASE.md:101 said "no ... YAML tags". Neither held: `permissions: ! write-all` and `permissions: !1 write-all` passed every rule.
- **A1 (ADVISORY).** Final-head command evidence was not recorded, and the tree hash `2370742` was stale.
- **A2 (ADVISORY).** The tag-time residual was not stated in RELEASE.md.

Initial verdict: BLOCKED. The recheck below supersedes it.

## Recheck (head e34c665)

The reflog shows `e34c665` ("build(release): reject write-all anywhere in publish.yml (#1)") committed directly on `edcc413`. The base is still `3037418`.

### Prior findings

- **B1: DISCHARGED.**
  - check-release.py:170-172 now rejects `^[^#\n]*\bwrite-all\b`, that is, any `write-all` outside a comment. Both bypasses match: `permissions: ! write-all` and `permissions: !1 write-all`. publish.yml contains no `write-all`, so the real workflow still passes.
  - `test_write_all_fails` (test-check-release.py:125-132) adds both forms as subtests. The count stays at 29 test methods, which matches the notes.
  - A tag in front of an `id-token` key was already caught, because the id-token pattern matches the key anywhere on a non-comment line. No tag form can now spell an OIDC grant that passes the check.
  - Notes lines 18-19 and RELEASE.md:100-103 now list `!!`, verbatim and named tags, and "no `write-all` anywhere outside a comment".
- **A1: DISCHARGED.**
  - validation.md:44-58 records the commands and their output: check-release.py, test-check-release.py (29 OK), check-docs.py (all with `EncodingWarning` as an error) and `make hygiene` (exit 0). It also records `merge-tree` = `8c8f96f` = `HEAD^{tree}` at `e34c665`, with origin/main `3037418`.
  - validation.md:21 now ties `2370742` to `e50d49a` and points to the final-head section.
  - The ancestry in the reflog is consistent with the recorded merge-tree result.
- **A2: DECLINED by the coordinator.** That is acceptable. The finding was advisory and outside what the owner asked for. Notes line 36 still states the accepted risk accurately.

### New text

- The notes list 9 squashed commits, with `e34c665` added. The file stat (+132/-9) is consistent with the added subtests and rule.
- The `e34c665` commit message has no attribution, no closing keyword and no hosted-CI wording. Notes line 3 still says "Refs #1", and issue #1 stays open.

### New finding

- **C1 (ADVISORY).** "named YAML tags" is broader than the tag rule. The phrase appears in notes line 18 ("`!!`, verbatim or named YAML tags") and in RELEASE.md:101.
  - The tag rule at check-release.py:165 rejects `!` followed by `!`, `<` or a letter. Other shorthand or named-handle tags pass it, such as `!1`, `!_x`, `!-x` and `!0!x`. For example, `permissions: !1 read` passes the check.
  - None of these can spell a grant, because `write-all` and `id-token` are now caught whatever tag precedes them. The security claim therefore holds, and only the description of the tag rule is loose. This does not block.
  - REWRITE TO (optional), in both places: "`!!`, verbatim or letter-named YAML tags".

## Result

B1 and A1 are discharged. A2 was declined, which is acceptable because it was advisory. The only remaining item is advisory wording (C1). Every grant-bearing tag form is rejected, and the executed final-head evidence is recorded.

VERDICT: PASS
