# Review debt: PR #19 (issue #15, purrassist-derive), head 637018b (addendum: 8d8d9a2)

Audited at head `637018bc4d83ce0200b2e3e87215a84ce4596197` (confirmed equal to the PR `headRefOid`).

## Inventory

- Reviews (`raw/pr-reviews.json`): 0.
- Inline comments (`raw/pr-inline-comments.json`): 0.
- Review threads (`raw/pr-threads.json`): 0.
- PR conversation comments: 10, all by `paudley` (the implementer). No bot comments.
- Issue #15 comments: 14 (reconciled at Stage 3 with `gh issue view 15 --json comments`), all by `paudley` (the implementer). There is no feedback from anyone else.

Reviews, inline comments and threads are empty, so no disposition is owed for them. Every comment is a workflow post by the implementer. None asks for a change. Each one is therefore dispositioned as an informational record, with the commitments it makes checked.

## Comment dispositions

| # | Comment | Kind | Disposition | Commitments and their state |
|---|---|---|---|---|
| 1 | Stage 1 plan | Plan | Informational, no reviewer request. Implemented by `0fb0454`, `e97a973`, `4085bf3` and `534f862`. | Tasks 1-5 are done. The external crates.io name reservation and trusted-publisher setup are declined by the plan, with a reason (maintainer steps), and documented in `docs/RELEASE.md` as required before tagging. |
| 2 | Handoff: least-confident areas | Disclosure | Informational. The three disclosed weaknesses were later handled: error-code assertions on `compile_fail` doctests (G7, `d0a99e9`), and the bridge span residual disclosed in the PR body. | No open commitment. |
| 3 | Stage 2 remediation plan | Plan | Informational. All items were executed (below). Declined items carry stated reasons: `=0.1.0` pinning, hiding `Members`/`Slot`, the `arguments(ObjectSpec)` setter, and a UI harness. | Item G8 is assigned to Stage 3 (open, see below). G2 is "reported; validation is local". |
| 4 | R1 hygiene | Fix report | Addressed in `1e17df7`. Evidence: standalone builds, a fully shadowed expansion, an independent review PASS. | None. |
| 5 | R2 / G1 nested unknown policy | Fix report | Addressed in `64097ec`. Evidence: real recover to conversion tests, and the no-key test fails on the previous code. | None. |
| 6 | R3 support-surface hardening | Fix report | Addressed in `d46b996`. | "`make check` runs once, on the settled head": fulfilled (run on `d0a99e9`, see `validation.md`). |
| 7 | R4 crate override and ABI marker | Fix report | Addressed in `9534a4f`. | None. |
| 8 | R5 publish preflight | Fix report | Addressed in `b7475b4`. | None. |
| 9 | R6 evidence and doc gaps | Fix report | Addressed in `d0a99e9`. | None. |
| 10 | Stage 2 summary | Summary | Informational. Its Stage 3 obligations are listed below. | See the commitments section. |

All commit hashes above were resolved with `git log` in this worktree. Commits `6cbb628`, `4d8a48e` and `637018b` answer the exit audit's findings (`completion-audit-exit.md`, final VERDICT: PASS).

## Issue #15 comments

Fourteen comments (count reconciled at Stage 3), all by the implementer. They duplicate the PR posts (plan, task reports, handoff, remediation plan, R1-R6, Stage 2 summary). No external feedback and no request to answer.

## Checks

State of every check on head `637018b` (`gh pr checks 19`, run 38122901971):

| Check | State |
|---|---|
| native (ubuntu-24.04) | did not run |
| native (macos-14) | did not run |
| msrv | did not run |
| windows-engines | did not run |
| wasm | did not run |
| corpus | did not run |
| fuzz | did not run |
| qualification | did not run |

- Branch protection on `main` is absent (`gh api repos/Blackcat-Informatics/purrassist/branches/main/protection` returns 404 "Branch not protected"). No check is formally required.
- Confirmed with `gh run view 38122901971`: its `headSha` is `637018bc...`. All eight jobs have 0 steps and ran for 3-5 seconds (started 07:53:32Z, completed 07:53:35Z-07:53:37Z). No step executed. Hosted CI did not run on the PR head, and there is no hosted evidence.
- The local qualification in `validation.md` is the evidence. It binds the current head indirectly rather than directly:
  - The full qualification (`make check`, `make package` with 6 archives, `rust-validate 1.98.0 cargo test --workspace --locked`, `make wasm-test`) ran on `d0a99e9`.
  - The three later commits (`6cbb628`, `4d8a48e`, `637018b`) change only Rustdoc and Markdown prose. Derive doctests (55 + 50), rustdoc `-D warnings`, `check-repository.py` and `make hygiene` were rerun on them and pass.
  - `validation.md` records that the integration candidate tree equals the head tree (merge-tree exit 0, base `fccfbe7` unchanged).
  - The full gate was therefore not re-run on `637018b`. The reuse rationale is recorded and holds for prose-only changes.

## Unfulfilled commitments

- **G8 (open, expected Stage 3 obligation):** append a D2 resolution record to `.deficiencies` with the merge SHA. `.deficiencies` lives in the main checkout, outside git. D2 currently reads "purrassist-derive crate not built". This cannot be done before the merge SHA exists. It is a post-merge obligation, not a blocker for merging.
- The other Stage 3 obligations in comment 10 are in progress in this stage: merge-tree assessment (recorded in `validation.md`), this review-debt audit, and the squash notes (draft at `squash-notes-draft.md`).
- Maintainer-only steps before any release tag (not merge blockers, documented in `docs/RELEASE.md`): reserve the `purrassist-derive` name on crates.io and configure its trusted publisher.
- No other unfulfilled commitment found in comments 1-10 or the issue comments.

## Summary

- Unaddressed review feedback: none (0 reviews, 0 inline comments, 0 threads, no external issue feedback).
- Declined items all have recorded reasons in comment 3.
- Hosted CI did not run; local qualification is the only evidence.

First-pass verdict (superseded by the final line): CLEAR. No outstanding reviewer feedback exists. The only open items are the post-merge G8 `.deficiencies` D2 record and the maintainer pre-tag steps, both already tracked as Stage 3 obligations. The lack of hosted CI is recorded, and branch protection does not require it.


## Stage 3 addendum (head 8d8d9a2)
- `8d8d9a2` fixes the stale `publish.yml` header comment found by the merge-plan review (F1). No new comments, reviews or threads; the disposition is unchanged.

VERDICT: CLEAR
