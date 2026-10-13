# Stage 3 merge-plan review: PR #28 (issue #26)

## What was reviewed

- **Squash notes:** `/home/paudley/Active/purrassist/.worktrees/26-lint-the-whole-workspace-with-the.squash_notes`
- **Stage files:** validation.md (including "Stage 3 integration assessment"), review-debt.md, completion-audit-exit.md, issue.md, issue-analysis.md, plan.md, remediation-plan.md (as posted in PR comment 3), pr-comments.md, pr-checks.txt, and raw/pr-reviews.json, pr-inline-comments.json and pr-threads.json.
- **Tree at head:** verify.yml, Makefile, docs/QUALIFICATION.md, docs/RELEASE.md and scripts/check-docs.py.
- **Refs, read from .git:**
  - `refs/remotes/origin/main` and `FETCH_HEAD` are both `8f1f42d6e503…`.
  - The branch ref is `8c7ab3be5929…`.

**Limit:** this reviewer had no shell, so I did not re-run `git merge-tree`, `git rev-parse`, `git diff --stat` or `gh`.
- The tree equality `980f4bc5` and the stat totals (11 files, +1327/-38) come from validation.md and the squash notes.
- The stat is internally consistent: the per-file changes sum to 1365, which equals 1327 + 38.
- I could not confirm that the F2 closeout comment exists on issue #26. raw/issue.json predates it, and its presence rests on validation.md:120 and review-debt.md:32.

## Q1. Is the integration evidence sufficient for the candidate?

**Yes, in substance.**
- The base equals the merge base (`8f1f42d`), so the base side has no changes and no semantic reconciliation is needed.
- Hosted Verify run 38438995951 on `8c7ab3b` is green on all 8 jobs (pr-checks.txt, `state: pass`).
- Probe PR #30 (run 38430682936) ran on `fd898ed`. The audit shows the workflows, Makefile and crates are byte-identical from `fd898ed` to `8c7ab3b` (completion-audit-exit.md:400).
- The current `Makefile:59` (`$(MAKE) check` under `release-gate`) matches that probe's `Makefile:59: release-gate` error line.

**The description of the evidence overclaims, though.** See B2.

## Q2. Does every comment have an honest disposition?

**Yes.**
- The PR has 0 reviews, 0 inline comments and 0 threads, confirmed from the raw JSON.
- It has 6 conversation comments, all owner status posts, and each has a disposition in review-debt.md.
- One small gap: PR comment 6 itself says "both probe results still apply", which is the same overclaim as B2. It is the author's own post, not feedback, so this is advisory (A2).

## Q3. Do the squash notes claim anything the evidence does not show?

### B1 (BLOCKING): the requirement mapping matches neither plan.md nor the ledger

The squash notes say:
- "R2 (fails a PR with an exact float `assert_eq!` in a core test)"
- "R3 (same toolchain as release)"
- "R4 (documented)"

The plan's completeness contract (plan.md, issue.md L123–126) defines these differently:
- **R2** is "Use the existing `msrv` job".
- **R3** is "A stable-only lint fails on the PR that introduces it".
- **R4** is "Exact float `assert_eq!` … fails a hosted PR job".
- "Same toolchain" is plan item P2, and "documented" is P3.

Under the notes' labels, plan R2 is never mapped. In the ledger, R4 is a scope-out (the required check).

REWRITE TO:
```
Issue requirements (plan.md completeness contract):

- **R1 (whole-workspace Clippy on Rust 1.98.0 in a PR job):** Verify
  `release-gate`; run 38438995951 on `8c7ab3b`.
- **R2 (use the existing `msrv` job):** `msrv` is now `release-gate`, and
  the drift guard requires it.
- **R3/R4 (a stable-only lint, an exact float `assert_eq!` in a core test,
  fails a hosted PR job with `clippy::float_cmp`):** probe PR #30, run
  38430682936, on `fd898ed`, whose gate files are byte-identical to the
  final head. Probe PR #29 (run 38377011106) showed the same on the
  Task 1 gate.
- **P2 (one toolchain pin) and P3 (docs):** the drift guard;
  QUALIFICATION.md and RELEASE.md.
```

### B2 (BLOCKING): the notes claim more evidence covers the final head than it does

**Squash notes, Acceptance probes:** "The two commits after the probe head touch only the checker script, its tests and RELEASE.md, so the probe evidence still applies."
- This holds for PR #30 (`fd898ed`).
- It is false for PR #29 (`2212844`). `verify.yml` and the Makefile changed after that probe in `5e5159e` and `0c4c25e`, and its error line was `Makefile:56`.
- The audit says it "did not rely on" 38377011106 for the final head (completion-audit-exit.md:45).

REWRITE TO: "PR #30 ran on `fd898ed`; the two later commits touch only the checker script, its tests and RELEASE.md, so its result applies to the final gate. PR #29 ran on the earlier Task 1 gate and is historical."

**Squash notes, Integration:** "all of the evidence above covers it directly."
- The local 1.98.0 `make release-gate` (403 s) and the nightly `make check` (178 s) ran on `fd898ed` (validation.md:97–103), not on `8c7ab3b`.
- The probes ran on `fd898ed` and `2212844`.

REWRITE TO: "so hosted run 38438995951 and the script tests on `8c7ab3b` cover it directly; the local 1.98.0 and nightly runs and probe PR #30 ran on `fd898ed`, whose workflows, Makefile and crates are byte-identical to the candidate."

**Squash notes, Validation:** make the same distinction in the two local-run bullets.

REWRITE TO:
- "**Local, Rust 1.98.0 (at `fd898ed`):**"
- "**Local, nightly (at `fd898ed`):**"

**validation.md:130** has the same overclaim: "the probe runs 38430682936 and 38377011106, and the local single full run … all cover the exact candidate tree."

REWRITE TO: "Hosted run 38438995951 covers the exact candidate tree. Probe run 38430682936 and the local full runs cover `fd898ed`, whose gate files are byte-identical to the candidate. Run 38377011106 is historical (pre-Stage-2 gate)."

### Other numbers in the notes

These all match validation.md and the audit:
- 115 and 260 tests;
- 6 archives;
- 8 jobs;
- run IDs 38438995951, 38430682936 and 38377011106;
- `from_node.rs:336:9`;
- 8 corpus suites, which matches Makefile:34;
- 7 commits.

### A1 (ADVISORY): "Branch protection: `main` has none"

The evidence is a 404 from the classic branch-protection API, but the plan refers to "the `main` ruleset", so `main` may have a ruleset.

REWRITE TO: "**Required checks:** `main` has no branch protection and its ruleset has no required status checks, so `release-gate` is not required."

### A2 (ADVISORY): PR comment 6

PR comment 6 says "both probe results still apply". Optionally post a one-line correction to the PR.

## Q4. Is any issue requirement silently missing?

**No.** Each requirement is met or openly declined:
- **R1:** the issue's exact command is `make lint` line 1.
- **R15:** QUALIFICATION.md:259–260 has the MSRV evidence sentence.
- **R20:** `make lint` uses `--locked`.
- **R21:** `msrv` became `release-gate`.
- **R22:** `RUSTFLAGS` is dropped from the gate job (verify.yml:42–43).
- **R23:** the `windows-engines` Clippy run is kept (verify.yml:65).
- **R4/R5:** the required-check decline is disclosed.
- **R6–R13:** the probe procedure is evidenced.
- **NEEDS-JUDGMENT 2:** the broader scope (the whole release gate on PRs) is disclosed in PR comment 2. The owner has not objected.

## First-pass verdict

The first pass was blocked on two text-only corrections, B1 and B2. No re-run was needed.

## Recheck (after the coordinator's corrections)

I re-read the squash notes in full and validation.md lines 120–139 ("Stage 3 integration assessment" and the closing note).

### B1: DISCHARGED

Squash notes lines 48–61 now map each item to the plan.md contract:
- **R1:** `release-gate`, green on `8c7ab3b` in run 38438995951.
- **R2:** the `msrv` job is now `release-gate`.
- **R3 and R4:** probe PR #30 (run 38430682936, `fd898ed`), with PR #29 marked historical.
- **P1 and P2:** the drift guard.
- **P3:** QUALIFICATION.md, RELEASE.md, README and the `check-docs.py` pin rule.

Every label matches plan.md L123–129. Nothing is unmapped.

### B2: DISCHARGED

- **Squash notes lines 77 and 79:** both local runs are labelled "(at `fd898ed`)", which matches validation.md:97–103.
- **Squash notes lines 86–91:** PR #30 is tied to `fd898ed` and PR #29 is called historical, so neither run is overclaimed.
- **Squash notes lines 103–105:** the Integration paragraph now limits direct coverage to run 38438995951 and the script tests on `8c7ab3b`, and attributes the rest to `fd898ed`.
- **validation.md:130–134:** the same split, with 38377011106 called historical. This now agrees with completion-audit-exit.md:45 and :400.

The phrase "whose gate files are byte-identical" is accurate. The scripts that changed after `fd898ed` are covered by the hosted run on `8c7ab3b`.

### A1: DISCHARGED

- **Squash notes lines 94–97 and validation.md:135:** both now say "no classic branch protection" and that the ruleset has no required status checks.
- **Not independently verified:** the ruleset details (squash-only, signed, linear history) rest on the coordinator's `gh api …/rules/branches/main`, which this reviewer could not run. The claim is consistent with the plan's "the `main` ruleset has no required status checks".

### A2: STILL-OPEN (advisory, non-blocking)

PR comment 6 still says "both probe results still apply". It is the author's own status post and does not enter the squash commit. A one-line follow-up on the PR is optional.

### New text

No new findings.
- Squash notes lines 86–88 mention "on `fd898ed`" twice. That is cosmetic only.
- validation.md:138 accurately records the B1/B2 dispositions.

VERDICT: PASS
