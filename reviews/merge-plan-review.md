# Merge-plan review: PR #19 (issue #15, purrassist-derive), Stage 3

**Candidate (first pass):** head `637018bc4d83ce0200b2e3e87215a84ce4596197`, base `origin/main` `fccfbe70ffccf4b1a7151ee1702a5d003fd59660`. The recheck at the end of this file covers head `8d8d9a2`.
**Reviewed:** 2026-10-11.

## Method and limits

This review had file access only (no shell), so no `git` or `gh` command was run. Where the task asked for a command, the equivalent on-disk state was read instead, and the limit is recorded as a finding (F2):

- `.git/refs/remotes/origin/main` and `.git/FETCH_HEAD` both read `fccfbe70…` for `branch 'main'`; the reflog `.git/logs/refs/remotes/origin/main` ends at `fccfbe7` (push 1791660169).
- The branch reflog `.git/logs/refs/heads/paudley/15-purrassist-derive-zero-dependency` shows the branch created from `fccfbe7` and exactly 15 commits, `0fb0454` through `637018b`, in the order the squash notes list.
- Code facts were checked by reading `crates/derive/Cargo.toml`, `.github/workflows/publish.yml`, `.github/workflows/verify.yml`, `scripts/check-release.py`, `Makefile`, `crates/purrassist/src/lib.rs`, `crates/derive/README.md`, `crates/derive/src/lib.rs`, `docs/RELEASE.md`, `docs/tool-calls/INTEGRATION.md` and the ledger `/home/paudley/Active/purrassist/.deficiencies`.

Inputs read in full: `validation.md`, `review-debt.md`, `completion-audit-exit.md`, the squash notes, `squash-notes-draft.md`, `plan.md`, `remediation-plan.md`, `issue.md`, `pr-comments.md`, `gap-analysis.md`, `historical-fit.md`, `raw/pr-19.json`, `pr-checks.txt`.

## 1. Integration evidence for the actual candidate

| Claim (validation.md, Stage 3) | Check | Result |
|---|---|---|
| `origin/main` = `fccfbe7` = merge-base of `637018b` | Local `origin/main` ref and `FETCH_HEAD` read `fccfbe7`; the branch reflog starts at `fccfbe7` with no merge or rebase entries, so the merge-base is the branch point | Consistent (not re-fetched by this reviewer; see F2) |
| `git merge-tree` exit 0; candidate tree `a2a05fd4` equals `637018b^{tree}` | Follows by construction when the base side is empty; not re-executed here | Accepted on validation.md's record |
| Full qualification on `d0a99e9` binds the candidate | `d0a99e9` -> `637018b` is three commits recorded as prose-only, with derive doctests (55 + 50), rustdoc `-D warnings`, `check-repository.py` and `make hygiene` rerun on each. The audit's two rechecks independently confirmed the prose-only diffs for `6cbb628` and `4d8a48e`. The `637018b` wording ("nearly matches") is present in all three affected docs (`README.md:57`, `lib.rs:216`, `INTEGRATION.md:1594`) | Sound |

Note on evidence shape: `make check`, `make package`, workspace MSRV and `make wasm-test` were last run at `d0a99e9`, four commits back. The reuse rationale is recorded, consistent with how the audit treated the same commits, and holds for Rustdoc/Markdown-only changes. The one gap is the base-moved check (F2), which only the orchestrator can close immediately before `ghprsq`.

## 2. Dispositions of comments and threads

- Reviews 0, inline comments 0, threads 0: nothing is owed there.
- Ten PR conversation comments, all implementer workflow posts; `review-debt.md` gives each a disposition and traces each commitment. I checked the commitments that are checkable from files: R3's "`make check` runs once, on the settled head" (run on `d0a99e9`, validation.md), the G7 disclosure condition (present in the squash notes under "Design decisions and declines"), A-2 whitespace-collapse statement (present: "whitespace collapsed"), the G8 Stage 3 obligation (open, post-merge). All honest.
- Issue #15 comments are all by the implementer. `review-debt.md`'s count (9) and its parenthetical enumeration do not agree (F3), but no disposition depends on the count.
- Declined items (`=` pins, hiding `Members`/`Slot`, the `arguments(ObjectSpec)` setter, the diagnostics UI harness, the external crates.io steps) each carry a reason in `remediation-plan.md` or `plan.md`, and the squash notes restate them.

## 3. Squash notes against the evidence

| Squash-note claim | Evidence | Verdict |
|---|---|---|
| Zero dependencies, no `syn`/`quote` | `crates/derive/Cargo.toml`: `proc-macro = true`, `[lints] workspace`, only `[dev-dependencies] purrassist-core`; audit row "Zero dependencies" DEMONSTRATED | Supported |
| Re-exported through the facade; facade users need no direct core dependency | `crates/purrassist/src/lib.rs:21,55,76`; host `derive_facade` test with no core dependency (validation Task 3, audit 3c) | Supported |
| Derived definition equals the hand-built one; descriptions, defaults, effects, policy, unknown handling, `repeat`, `pattern`, nested, `Option`, `Vec`, `Box`, integers, `f32`/`f64`, `bool`, `String` | audit rows 2a, 4 and the criteria table; R3 for `f32`/`Box` | Supported |
| Single-pass `FromNode`, exact `FromNodeError` pointers | FN1 counter test (validation Task 1); audit criteria table | Supported |
| Nested default drop-with-recorded-loss; near-miss rule per policy (structural unresolved; tolerant renamed to a free near-matching name, else dropped with loss) | audit rows 1a-1d, Recheck 1 `nearmiss.rs`, Recheck 2 `nearmiss2.rs`; docs at head carry the same wording | Supported |
| Every rejected construct pinned by a `compile_fail` doctest with a passing twin | audit criteria row (five rejection classes built standalone), R6 report (58 blocks built standalone), Task 2 review | Supported |
| Hygiene against shadowing | audit rows 2a-2d | Supported |
| `#[tool(crate = path)]`; `__DERIVE_ABI_<major>_<minor>` marker; skew fails naming the marker | audit rows 3a-3d (simulated skew gives one E0425 naming `__DERIVE_ABI_0_1`) | Supported |
| Core support items listed; core stays dependency-free with no ambient access | audit criteria table; `check-repository.py` exit 0 | Supported |
| Release order core -> derive -> syntax -> purrassist -> host -> cli | `scripts/check-release.py:19-26`; `publish.yml:101` loop | Supported |
| Preflight runs before trusted-publishing authentication; missing name stops the job before anything is published; tested offline | `publish.yml:90-94` (Preflight at 90, Authenticate at 92, loop at 95); `Makefile:15` runs `test-check-release.py`; audit row 5 | Supported |
| RELEASE.md records the patch-compatibility rule | `docs/RELEASE.md:24-28`; audit Recheck 2 LOW-2 | Supported |
| Five verbatim user decisions | `plan.md` "User decisions" and `remediation-plan.md` R2 | Supported, verbatim |
| Validation: full qualification on `d0a99e9`, later commits prose-only with the listed reruns | validation.md Stage 2 full qualification and Stage 2 exit | Supported |
| Exit audit PASS after two rechecks, with the listed demonstrations | `completion-audit-exit.md` (BLOCKED, BLOCKED, PASS at `4d8a48e`); demonstrations 1-5 | Supported |
| Every Stage 1 task and Stage 2 remediation had an independent review, all PASS | validation.md: T1-T4 PASS, R1-R6 PASS. (`e4850a8` had no per-task review; it was covered by the completeness recheck, as gap-analysis seed (d) records. The sentence is about tasks and remediations, so it is accurate.) | Supported |
| Review debt CLEAR | `review-debt.md` VERDICT: CLEAR | Supported |
| Hosted CI did not run | validation.md, review-debt.md | Supported; stated with no reason, as policy requires |
| "Commits squashed (15)" and the list | Branch reflog: 15 commits, same hashes and subjects, same order | Supported |
| "47 files changed, 9869 insertions(+), 115 deletions(-)" | Not re-computed here (no shell). Gap analysis had 42 files / +7379 / -92 at `e4850a8`, so the growth is plausible. One line is a copy-detection artifact (F4) | Unverified, plausible |
| "Conflicts: None. The base did not advance" | Consistent with the refs read; see F2 | Supported pending the final fetch |
| No attribution strings | None present in the squash notes | Compliant |

No claim in the squash notes is contradicted by the evidence.

## 4. Issue #15 requirements and remediation items

The exit audit's criteria table covers every issue bullet with execution evidence, and every R1-R6 item has a review PASS. Deviations are recorded with reasons and disclosed: release order (D-10), the dev edge to core (D-11), trait errors instead of `compile_error!` for type misuse (D-13), and variant descriptions composed into the enum description.

One requirement is partly unmet and nowhere recorded (F1): the issue says "The `publish.yml` loop and comment are updated." The loop (line 101) and the step name (line 95) are updated, but the file's header comment at `publish.yml:7` still reads "Publishes core -> syntax -> purrassist -> host -> cli". `check-release.py validate_publish_workflow()` checks the preflight step's comment (line 190), not the header, and the plan's stale-text grep (`five crates|...`) does not match this phrasing. No other stale five-crate text was found in `docs/`, `scripts/` or `.github/workflows/` (the remaining "five" hits in RELEASE.md are intentional: "the original five crates", "configured for five of six crates").

Remediation items: G1, G3-G7, G9, F-1..F-6b, A-1, A-2 all have evidence. G2 is "did not run", recorded. G8 is the post-merge obligation (section 5).

## 5. Stage 3 decisions

**Reusing the completion audit (PASS at `4d8a48e`).** Sound. `637018b` implements the auditor's own optional nit in prose; the three affected docs now agree with the audit's `nearmiss2.rs` observation ("a free declared name it nearly matches"). Nothing in source, tests, criteria or feedback changed after the audit. The audit's demonstrations ran against the `d0a99e9` tree, which the prose-only commits do not alter.

**Not dispatching a semantic assessment.** Sound. With `origin/main` equal to the merge-base, the base side of the three-way comparison is empty; there is no second change set whose interaction with shared APIs, consumers, schemas, dependencies or toolchain could be assessed. The reason is recorded in validation.md, which is the right place.

**Merging while hosted CI did not run.** Acceptable under the user's standing decision for this repository, as #9 did. The facts that make it acceptable here are recorded: `main` has no branch protection and no required checks; the local gate (`make check`, `make package`, workspace MSRV 1.98.0, `make wasm-test`) exited 0 on `d0a99e9`; the later commits are prose-only with the text-affected checks rerun; the audit executed the integration paths from downstream crates natively and under WASI. The squash notes and validation.md describe CI only as "did not run", with no reason, as required. The residual that no local run reproduces is the Windows job's `cargo test -p purrassist-derive -p purrassist --locked` (`verify.yml:57`) on an MSVC host; gap analysis G2 already names it and it is not new to Stage 3.

**Post-merge `.deficiencies` D2 record.** Correct and still owed. The ledger at `/home/paudley/Active/purrassist/.deficiencies` has D2 "purrassist-derive crate not built" and no resolution record. It is append-only and git-excluded, so it cannot be part of the PR, and it needs the merge SHA, so it cannot be written before the merge. Concrete format in F7.

## Findings

### F1. `publish.yml` header comment still lists five crates (LOW, non-blocking)

- **Where:** `/home/paudley/Active/purrassist/.worktrees/15-purrassist-derive-zero-dependency/.github/workflows/publish.yml:7`: `# on `rust-v*` tags. Publishes core -> syntax -> purrassist -> host -> cli via`.
- **Why it matters:** issue #15 asks for the loop and comment to be updated; the loop is, the header is not. It is the only stale release-set text found in the tree. It is a comment, so it changes no behavior and no gate checks it.
- **Fix (preferred, before squash):** one prose-only commit changing the line to `# on `rust-v*` tags. Publishes core -> derive -> syntax -> purrassist -> host -> cli via`. Rerun `python3 scripts/check-release.py`, `python3 scripts/test-check-release.py`, `make hygiene` and `actionlint .github/workflows/publish.yml`; record them in validation.md as a fourth prose-only commit; add the commit to the squash notes' list (16) and change "The three later commits" to "four" in the Validation paragraph. The completion-audit and qualification reuse rationale extends unchanged (a YAML comment is not a build, test, packaging or doc input).
- **Alternative:** merge as is and fix in the next release-tooling change, recording the miss in the squash notes under "Residual" so it is not silent.

### F2. The base-moved check was not re-executed by this reviewer (LOW, procedural)

- **Where:** task focus item 1 asked for `git fetch` and `git rev-parse origin/main`. No shell was available; the on-disk refs show `fccfbe7`, but a fetch may be stale.
- **Fix:** immediately before `ghprsq`, run `git fetch origin && git rev-parse origin/main` in the worktree. Proceed only if it prints `fccfbe70ffccf4b1a7151ee1702a5d003fd59660`. If it does not, stop: the Stage 3 integration assessment, the squash notes' "Conflicts: None", and the no-semantic-assessment decision all depend on that equality and must be redone against the new base.

### F3. `review-debt.md` issue-comment count and enumeration disagree (LOW)

- **Where:** `review-debt.md:11,34`: "Issue #15 comments: 9" but the enumeration lists plan, four task reports, handoff, remediation plan, R1-R6 and the Stage 2 summary (14 items). `raw/issue.json` is the Stage 1 snapshot (6 comments) and cannot settle it.
- **Why it matters:** none of the comments is external feedback, so no disposition changes. It is a record-accuracy nit.
- **Fix:** run `gh issue view 15 --repo Blackcat-Informatics/purrassist --json comments --jq '.comments | length'` and make the sentence match (either correct the count or state which posts were mirrored to the issue).

### F4. Squash notes "Files" block carries a copy-detection artifact (INFO)

- **Where:** squash notes line `crates/{purrassist => derive}/Cargo.toml | 16 +-` alongside `crates/purrassist/Cargo.toml | 1 +`. Git's rename/copy detection paired the new derive manifest with the facade manifest. Nothing was moved.
- **Fix (optional):** regenerate the block with `git diff --stat --no-renames origin/main HEAD` so it reads `crates/derive/Cargo.toml | 25 +` (or similar), and re-check the totals line. If left as is, it is cosmetic.

### F5. Trailer key in the squash notes (INFO)

- **Where:** squash notes end with `Defect-Class: none`; repository history records the class in a `GhpRsq-Defect-Class` trailer (`prior-art-assessment.md:394-399`). The draft template used the same short key, so this is likely the `ghprsq` input convention.
- **Fix:** confirm `ghprsq` emits `GhpRsq-Defect-Class: none` from this field. If it expects the full key in the notes, change the line before running it.

### F6. `raw/pr-19.json` is a Stage 2 entry snapshot (INFO)

- **Where:** `raw/pr-19.json` has `headRefOid` `e4850a8` and 2 comments; `review-debt.md` audited head `637018b` with 10 comments from live `gh` output.
- **Fix:** none required for the merge. If the stage directory is archived, note in `review-debt.md` that the raw file predates the audit, or refresh it with `gh pr view 19 --json ...` for the record.

### F7. Post-merge D2 record: concrete text (INFO, obligation confirmed)

- **Where:** `/home/paudley/Active/purrassist/.deficiencies` (append-only, git-excluded).
- **Fix:** after `ghprsq` reports the merge SHA, append in the ledger's existing style:

  ```
  =============================================================================
  ## From #15 — purrassist-derive, merged (recorded 2026-10-11)
  PR #19 merged as <merge sha> on 2026-10-11.

  D2. Resolved. purrassist-derive ships: #[derive(ToolDefinition)] and #[derive(ToolEnum)],
      facade re-exports (purrassist::ToolDefinition / ToolEnum), release registration
      (RELEASE_ORDER core, derive, syntax, purrassist, host, cli; publish preflight).
      Remaining external (maintainer, before the first release tag): crates.io name
      reservation (0.0.0 placeholder) and trusted publisher for purrassist-derive; see
      docs/RELEASE.md. Hosted CI did not run on the PR head; validation was local.
  ```

  Then `grep -n 'D2' /home/paudley/Active/purrassist/.deficiencies` must show the record with the SHA (gap-analysis G8 acceptance).

## Summary (first pass)

The integration evidence binds the candidate: the candidate tree equals the tested head tree, the base is the merge-base, and the full gate plus prose-only reruns cover every commit. Every comment has an honest disposition, and there are no reviews, threads or external feedback. The squash notes make no claim the evidence does not support; the one unverified figure is the diffstat total, which is plausible and cosmetic. The Stage 3 decisions (audit reuse, no semantic assessment, merge on local evidence under the standing decision, post-merge D2 record) are sound and recorded.

One issue #15 item is partly unmet and unrecorded: the `publish.yml` header comment (F1). It is a comment with no behavioral effect and a one-line fix; it does not block the merge, but it should either be fixed in a prose-only commit before squashing or named in the squash notes so it is not silent. The orchestrator must perform the final `git fetch` / `rev-parse` check that this reviewer could not run (F2).

First-pass verdict (superseded): PASS

## Recheck (head 8d8d9a2)

Scope: only the findings above, against the coordinator's integration report. As in the first pass, no shell was available; each item was rechecked by reading the files on disk. The final `VERDICT:` line at the end of this file supersedes the first-pass verdict.

**Head.** The branch reflog `.git/logs/refs/heads/paudley/15-purrassist-derive-zero-dependency` now ends with `637018b -> 8d8d9a25a83994e7a557a4d8ae26c30fc25233e9`, subject "ci(release): publish.yml header names the derive crate and the preflight (#15)". The branch ref reads `8d8d9a2`. `.git/refs/remotes/origin/main` still reads `fccfbe7`.

| Finding | Recheck evidence | Verdict |
|---|---|---|
| F1 stale `publish.yml` header | `publish.yml:7-8` now reads "After a crates.io name and owner preflight, publishes core -> derive -> syntax -> purrassist -> host -> cli via crates.io trusted publishing". A tree-wide search for `core -> syntax` / `syntax -> purrassist` outside `.stage/` finds only the new line 8 (with `derive` in place). The step name (line 95) and loop (line 101) are unchanged. `validation.md:122` records the commit as comment-only with `check-release.py`, `test-check-release.py`, actionlint and `make hygiene` passing, and that the base is still the merge-base so the candidate tree equals the head tree. The reported tree `66a18b6c` = `8d8d9a2^{tree}` is consistent with an empty base side; not re-executed here. The qualification and completion-audit reuse rationale extends to a YAML comment, as the first pass said | DISCHARGED |
| F2 base-moved check | Not performed here; the coordinator will fetch and compare immediately before `ghprsq`. Still required: proceed only on `fccfbe70ffccf4b1a7151ee1702a5d003fd59660` | OPEN BY DESIGN (pre-ghprsq step, not a blocker) |
| F3 issue-comment count | `review-debt.md:11,34` now says 14 issue comments, reconciled with `gh issue view 15 --json comments`, all by the implementer; this matches the enumeration (plan, four task reports, handoff, remediation plan, R1-R6, summary). The file has the 8d8d9a2 addendum, one "First-pass verdict (superseded by the final line): CLEAR" line and a single final `VERDICT: CLEAR` | DISCHARGED |
| F4 Files block artifact | The squash notes' Files block now lists `crates/derive/Cargo.toml | 24 +` and `crates/purrassist/Cargo.toml | 1 +`, no `{purrassist => derive}` line; `publish.yml` is `16 +-`; totals `47 files changed, 9887 insertions(+), 110 deletions(-)`. The commit list has 16 entries with `8d8d9a25a` first, and the Validation paragraph says "The four later commits change only Rustdoc/Markdown prose and one workflow comment" with `check-release.py`, `test-check-release.py` and actionlint added to the rerun list. No other squash-note claim changed | DISCHARGED |
| F5 trailer key | The coordinator reports that the previous merge `fccfbe7` carries both `Defect-Class: performance` and `GhpRsq-Defect-Class: performance`, so `ghprsq` maps the notes line. Consistent with `prior-art-assessment.md:394-399` (trailer read from history) and the draft template's short key. Not re-read from the object store here | DISCHARGED |
| F6 stale raw snapshot | `raw/pr-19.json` now has `"headRefOid":"8d8d9a25a83994e7a557a4d8ae26c30fc25233e9"` and ten `createdAt` entries, matching the ten PR comments in `review-debt.md` | DISCHARGED |
| F7 D2 record | Post-merge by nature; the ledger is unchanged and the obligation is acknowledged | OPEN BY DESIGN (post-merge) |

**Residual nits, non-blocking:**
- `review-debt.md:1,3` still title the audit "head 637018b"; the addendum covers `8d8d9a2`, so the record is complete but a reader must reach line 75 to see it. Optional: add "(addendum: 8d8d9a2)" to the title.
- `validation.md:122` states the candidate tree equals the head tree for `8d8d9a2` but does not record the tree hash (`66a18b6c`) the coordinator reported. Optional: add it, matching the `a2a05fd4` record for `637018b`.

**Squash notes at `8d8d9a2`:** every first-pass claim remains supported; the changed lines (commit count 16, "four later commits", the extended rerun list, the regenerated Files block) match the reflog, `validation.md:122` and `publish.yml`. No attribution strings; hosted CI described only as "did not run".

**Stage 3 decisions:** unchanged and still sound. The audit reuse extends to `8d8d9a2` because a workflow comment is not a source, test, doc-build, packaging or criteria input, and nothing else changed. The base side remains empty, so no semantic assessment is needed. Branch protection is absent and the standing decision applies.

VERDICT: PASS
