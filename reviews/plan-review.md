# Plan final review (Stage 1)

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and/.stage/complete-purrassist-0-1-0-plan-and/plan.md` (revision 3, dispositions at lines 354–424).

Inputs examined:

- `issue-analysis.md`, `prior-art-assessment.md`, `output-rules.md` (local, unposted).
- `reviews/plan-completeness.md` (pass 2, PASS), `reviews/plan-constraints.md` (pass 2, PASS), `reviews/plan-design.md` (pass 3, PASS), `reviews/plan-enhancement.md` (pass 3, PASS), `reviews/plan-adversary.md` (pass 3, PASS).
- Worktree at `f11b45e`: `Makefile` (hygiene and check targets), `examples/*`, `README.md:14–17, 387, 426`, and a grep for the banned phrases over the public Markdown set.
- Binding owner decisions as quoted by the caller: hosted CI out of scope; #6/#7/#10/#11 non-blocking; scope = release docs, README, doc updates, GitHub repo settings (public repo), three-way interface framing, `social-preview.png`; tagging/publishing out of stage; derive 0.0.0 reservation already published.

## 1. Fixes are real (every BLOCKING finding and adopted enhancement located in plan text)

| Report | Finding | Disposition | Plan text | Verified |
|---|---|---|---|---|
| Completeness | B1 QUALIFICATION 7–8/224/226, IMPLEMENTATION 99–108 | adopted | Inventory lines 93–94, 99; Task 1 lines 192, 198, 201 | yes |
| Completeness | B2 stale grep too narrow, no scope | adopted | Row 143: case-insensitive list, whitespace-collapsed, explicit scope | yes |
| Completeness | B3 no failing fixtures for Task 2–4 rules | adopted | Row 143 ("for every rule"), Tasks 2/3/4 lines 256, 292, 316 | yes |
| Completeness | B4 TOOLS.md examples never run | adopted | Rows 146–147; Task 2 lines 236–237 | yes |
| Completeness | N1 wrapped phrases | adopted | Row 143 collapsing clause, wrap fixture, baseline check | yes |
| Completeness | N2 delivery-sequence misfiled | adopted | Task 1 line 201 under IMPLEMENTATION | yes |
| Completeness | N3 PVR enabled before SECURITY names it | adopted | Task 3 line 288 with read-back and fallback | yes |
| Completeness | N4 migration test pins f11b45e in gate | "resolved by one-time check" | Row 149, Task 2 line 257 | half: see A2 |
| Constraints | B1 excluded topic in posted text | adopted | Line 14; no such wording anywhere in the plan; `output-rules.md` exists and covers docs, commits, issue/PR comments, T5 records, handoff | yes |
| Constraints | A1 notices for tooling | adopted | Row 156, Task 4 line 314 | yes |
| Constraints | A2 asset registry | adopted | Row 156 `docs/assets.json` + sidecar | yes |
| Constraints | A4 single font, no fallback | adopted | Row 156, Task 4 line 307 | yes |
| Constraints | A5 exact-commit wording | adopted | Line 45, row 153, Task 3 line 271 | yes |
| Constraints | A6 explicit-file commits every task | adopted | Lines 209, 265, 299, 324 | yes |
| Constraints | N1 SPDX ids | adopted | Row 156 `LGPL-2.1-or-later`, `OFL-1.1` | yes |
| Constraints | N2 re-render mandatory; font by upstream identity | adopted | Row 156; Task 4 line 321 | yes |
| Design | B1 dialect source | adopted | Row 144, Task 1 line 190 | yes |
| Design | B2 PNG digest pinned | adopted | Row 156 | yes |
| Design | B3 git in gate | adopted | Row 149, Task 2 line 257; `hygiene` wiring (line 188) names only the two new scripts | yes |
| Design | A1 one marker-copy rule | adopted | Row 148, Task 2 line 255 | yes |
| Design | A2 packaged READMEs from metadata | adopted | Row 150 | yes |
| Design | A3 zero-match fails | adopted | Row 143 | yes |
| Design | A5 Rust entry-point column dropped | adopted | Row 146 lists name, description, properties only; line 411 | yes |
| Design | A7 one SPDX scanner | adopted | Row 143, Task 1 line 191 | yes |
| Design | A8 heading census direction | adopted | Row 146 last two sentences | yes |
| Design | A9 supporting inputs in EXAMPLES | adopted | Row 147 | yes |
| Enhancement | 1 TRANSFORMATION generated TOOLS.md | adopted | Row 146, Task 2 lines 229–236 | yes |
| Enhancement | 2 `server.json` | declined, narrowed `mcp-name:` token adopted | Lines 172–175; row 151; Task 2 line 249. Row title now says "presence only" (adversary pass-3 note). Namespace derived from `repository` URL owner segment (enhancement pass-3 refinement) | yes |
| Enhancement | 3 named CLI≡MCP test | adopted | Row 148 | yes |
| Enhancement | 4 decode identity | declined with reason | Lines 176–178; row 148 keeps `validate` claim with new test | yes |
| Enhancement | 5 crate metadata | adopted | Row 152 | yes |
| Enhancement | 6 MCP config + `.purrassist/` | adopted | Row 148 | yes |
| Enhancement | 7 derived limitations set | adopted | `docs/known-limitations.json`, rows 145, 154 | yes |
| Enhancement | 8 packaged README links; derive `latest` | adopted | Row 150, Task 2 line 247 | yes |
| Enhancement | 9 sidecar, renderer check | adopted | Row 156 | yes |
| Enhancement | 10 Keep a Changelog headings | adopted | Row 154 | yes |
| Adversary | A1–A8, N1 | adopted | Rows 143, 145 (fuzz-row links, stop wording), 147, 149, 154, 157 (NOT DELIVERED) | yes |

## 2. Declines are justified

- **Combined `LICENSE`** (lines 169–171): technical reason (detector cannot express `OR`; duplicate texts audited by hash would mislead) and visible cost (GitHub shows one license; stated in README and handoff). Justified.
- **`server.json`** (lines 172–175): reason (correctness is only proven by submission, which is post-publication); cost (registration is an owner step); the irreversible prerequisite is adopted inside the stage. Justified.
- **`decode` identity test** (lines 176–178): reason (deliberately different shapes); cost (docs stop claiming identity; the temperature request is exercised via `decode_json` instead). Justified.
- **Generate all prose counts** (lines 179–181): partial adoption with a readability reason; the declined TRANSFORMATION residue is explained (structural lists are generated, narrative numbers are checked facts). Justified.

## 3. Integration check: findings

### B1. BLOCKING: Task 1's global banned-phrase rule cannot exit 0 at the end of Task 1

Row 143 (Task 1) bans, case-insensitively over "all tracked `*.md` outside `.stage/`, `vendor/` and `crates/core/tests/data/`", the phrases `Pre-release`, `five of six`, `no placeholder record`, `not yet done`, `reviewed merge commit`, and the one-time baseline check must prove the rule flags the RELEASE.md sites. Task 1's focused check (line 206) then requires `python3 scripts/check-docs.py` to exit 0 on the working tree.

But Task 1's correction set (line 192) is:

> "Corrections in `docs/QUALIFICATION.md`, `docs/IMPLEMENTATION.md`, `docs/FUZZING.md`, `CONTRIBUTING.md`, and the README fact lines at 14–17, 387 and 426."

At `f11b45e` the rule also fires on files Task 1 does not touch (verified by grep):

- `docs/RELEASE.md:6–7` (`reviewed merge commit`), `:43` (`no placeholder record`), `:83` (`five of six`), `:218` (`not yet done`) — fixed in Task 3.
- `SECURITY.md:5` (`pre-release`) — fixed in Task 3.
- `crates/{core,host,cli,purrassist}/README.md` (`Pre-release`) — fixed in Task 2 (row 150).

So the Task 1 focused check fails as written, or the implementer must silently do Task 2/3 work in Task 1 or narrow the rule. Neither choice is in the plan. This was introduced by integrating Completeness B2 / Adversary A1 (the rule grew from a narrow grep to a global rule) without moving the matching corrections.

REWRITE TO (Task 1, Files, replace the "Corrections in …" bullet):

```
- Corrections at every site the banned-phrase rule flags, so the rule is green at the end of this task: `docs/QUALIFICATION.md`, `docs/IMPLEMENTATION.md`, `docs/FUZZING.md`, `CONTRIBUTING.md`, the README fact lines 14–17, 387 and 426, the `docs/RELEASE.md` phrase sites (6–7, 43–62, 83, 212–218: wording only, replaced with the exact-commit and dated-observation sentences Task 3 keeps), `SECURITY.md:5`, and the status lines of `crates/{core,host,cli,purrassist}/README.md` (neutral "0.1.0" wording; Task 2 adds the interface roles). Task 3 retains the structural RELEASE.md work (reservation record, trusted-publisher observation, qualification-workflow summary, release body) and the SECURITY.md table and channels.
```

And in Task 3 (`docs/RELEASE.md` bullets), prefix: "The phrase corrections landed in Task 1; this task restructures the sections."

### B2. BLOCKING: Task 2's README links `CHANGELOG.md`, which Task 3 creates, under Task 1's relative-link rule

Task 2 README order, item 2 (line 216): "Status, linking the CHANGELOG known limitations." Row 143 (Task 1, in `make check`) requires "relative links resolve". `CHANGELOG.md` is created in Task 3 (row 154, line 278). Task 2's focused check (line 261) runs `check-docs.py` and must exit 0, so the link is either a dangling link that fails the gate or the implementer has to invent a CHANGELOG stub in Task 2. The plan does not say which.

REWRITE TO (Task 2, README order item 2):

```
2. Status, linking `docs/QUALIFICATION.md#known-limitations` (created in Task 1). Task 3 adds the `CHANGELOG.md#known-limitations` link to the same status line when the file lands, and its focused check re-runs the link rule.
```

### A1. ADVISORY: row 145 states a Task 3 dependency inside a Task 1 row

Row 145 (Task 1): "Its issue set equals the CHANGELOG's (Task 3) and the set of issue references in `docs/known-limitations.json`." The CHANGELOG does not exist until Task 3, so the CHANGELOG half of this equality cannot be implemented or fixture-tested in Task 1.

REWRITE TO: "Its issue set equals the set in `docs/known-limitations.json`. (The CHANGELOG's equality with the same file is row 154 and lands in Task 3.)"

### A2. ADVISORY: the one-time migration check has no path for a deliberately dropped token

Row 149: "asserts that each appears in the new README ∪ `docs/TOOLS.md`." Completeness N4's first half (a token removed on purpose, for example from the JSON-only sentence at README:426, which Task 1 rewrites before Task 2 runs the check against `f11b45e`) is not addressed; the disposition (line 383) says "resolved" but only the git-in-gate half was. As written, a single intentional removal makes the recorded check fail with no documented outcome.

REWRITE TO (append to row 149): "A token that is absent on purpose is listed in the Task 2 review with a one-line reason; the recorded output must be empty after subtracting that list, and the list is part of the record."

### A3. ADVISORY: Task 4 does not list the `check-repository.py` edit it depends on

Row 156: "the SPDX scan accepts sidecars for binaries." That is a change to `scripts/check-repository.py`, but Task 4's Registration list (lines 312–316) names only `docs/assets.json`, the sidecar, `vendor/tooling.json`, `docs/DEPENDENCIES.md` and `check-docs.py` rules.

REWRITE TO (Task 4, Registration, add): "- `check-repository.py`: the SPDX scan accepts a `<file>.license` sidecar for tracked binaries, with a `test-repository-audit.py` case for a PNG without one."

### A4. ADVISORY: Task 3 changes a repository setting but Task 5 owns the before/after record

Task 3 line 288 runs `gh api -X PUT …/private-vulnerability-reporting`. Task 5 line 340 records "before and after values" and Task 5 line 337 only "re-verifies". The before value for PVR would never be recorded.

REWRITE TO (Task 3, SECURITY.md first bullet, append): "Record the `GET` value before the `PUT` in `tasks/T5-repo-settings.md` so Task 5's before/after record is complete."

## 4. Coherence (read as the implementer)

- Task order 1 → 6, no Task 0, Task 6 is the PR task. Each of Tasks 1–4 has files, focused check, review, commit (explicit files), push, issue update. Task 5 is no-commit by design with an independent review and issue post. Task 6 runs the single full gate, completeness review, inventory, PR, handoff.
- Every contract row maps to a task whose text names the deliverable, including the revision-3 row 151 (Task 2 line 249, focused check line 261).
- Acceptance checks run on shipped entry points: `check-docs.py`/`test-check-docs.py` in `hygiene`, named tests in `crates/cli/tests/protocol.rs` and `crates/core/tests/tool_call_dialects.rs` under `make test`, `check-release.py`, `make package`, `gh repo view`/`gh api`. Example requests are self-contained (inline `source`), so row 147's "fresh roots" is sufficient.
- The two ordering contradictions above (B1, B2) are the only places an implementer must guess. Everything else is specific enough to execute.
- Out-of-stage items match the owner decisions exactly (lines 161–165). No silent omission: `read_file`/`undo_file` narrative, the derive reservation record, the known-limitations disclosure, the social preview, the repo settings and the three-interface framing are all mapped.

## 5. Open user decisions

None required now. Two conditional decisions are already framed as concrete choices in the plan and need an answer only if the condition occurs:

1. If the Playwright social-preview upload is impossible (row 157): (a) the owner uploads `docs/social-preview.png` via Settings and the implementer records the `og:image` check, or (b) the owner drops the upload from this stage, quoted in the handoff.
2. If enabling private vulnerability reporting fails (Task 3 line 288): SECURITY.md names the email only and the failure is reported; the owner decides whether to enable it themselves before the PR merges.

## Pass 1 verdict

The five lens reviews are integrated faithfully; every adopted item has matching plan text and every decline has a reason and a visible cost. Two task-order contradictions introduced by the integration (B1, B2) would make Task 1's and Task 2's own focused checks fail as written and must be fixed before implementation. The four advisories are cheap clarifications.

Pass 1 verdict: BLOCKED (superseded by Pass 2 below)

---

# Pass 2 (recheck of the revised plan)

Reviewed: the same `plan.md`, re-read from disk after the coordinator's correction notice. The on-disk text now contains the "Final review, pass 1" dispositions (lines 374–378) and the edits adjudicated below. Line numbers are for the current on-disk text. The header still reads "Revision 3" (line 3); this is cosmetic and does not affect the plan.

## Prior findings

### B1. DISCHARGED

Task 1, Files, lines 193–198:

> "**Every banned-phrase site is corrected in this task, so `check-docs.py` is green when the rule lands.** This includes: `docs/RELEASE.md` lines 5–7, 43–62, 83 and 212–218: minimal factual corrections … `SECURITY.md:5`: the pre-release sentence is replaced with a factual status line. The four crate READMEs' "Pre-release implementation" lines: replaced with a one-line 0.1.0 status. Tasks 2 and 3 then do the structural rewrites of these files."

That covers every site my grep found at `f11b45e` (RELEASE 6–7/43/83/218, SECURITY:5, four crate READMEs). The RELEASE corrections are the same sentences Task 3 keeps (line 277–279: exact-commit wording, derive minted 2026-10-12, dated trusted-publisher observation), so Task 1 and Task 3 do not contradict each other. Task 2's "0.1.0 status line and its interface role" for six READMEs (line 246) extends, and does not conflict with, Task 1's one-line status for four. Task 1's focused check (line 212) can now exit 0.

### B2. DISCHARGED

Task 2 README order item 2, line 222:

> "Status, linking `docs/QUALIFICATION.md#known-limitations`. Task 3 adds the CHANGELOG link when the file exists, keeping the relative-link rule green at every commit."

`docs/QUALIFICATION.md#known-limitations` is created in Task 1 (line 206), so the link resolves at Task 2's focused check. Task 3's focused check (line 303) re-runs `check-docs.py`, which covers the added link.

### A1. DISCHARGED

Row 145 now reads "Its issue set equals the set of issue references in `docs/known-limitations.json` … Task 3 extends the same rule to the CHANGELOG when that file is created." Row 154 holds the CHANGELOG equality. No Task 1 rule depends on a Task 3 file.

### A2. DISCHARGED

Row 149 now reads "… or is listed in a deliberate-removal table in `validation.md`, each entry with its reason (for example a stale count). The Task 2 reviewer must accept every removal." The intentional-removal path exists, is recorded, and is reviewed.

### A3. DISCHARGED

Task 4 Registration, line 320: "`scripts/check-repository.py`: the SPDX scan accepts a `.license` sidecar for binary files." The edit is now in the task's file list, and the focused check (line 328) runs `check-repository.py`. The `test-repository-audit.py` negative case I suggested is not named; row 143's "for every rule, a fixture" convention is stated for `check-docs.py`, not `check-repository.py`, so this is a residual optional note, not a finding.

### A4. DISCHARGED

Task 3 SECURITY.md, line 294: "Record the before-value (`gh api repos/Blackcat-Informatics/purrassist/private-vulnerability-reporting`) in `tasks/T5-repo-settings.md`." Task 5's before/after record (line 348) is now complete for this setting.

## Integration check of the new text

- Lines 193–198 add no new rule or file beyond those already in rows 143, 150, 153 and 155; they only move corrections earlier. No contract row changed task number.
- Line 222 introduces no new check; the relative-link rule (row 143) already covers it.
- Lines 294–295 keep the N3 ordering (enable, read back, then write the sentence) and the hard fallback.
- The dispositions section (lines 374–378) names each pass-1 finding and the plan text matches each disposition.

No new BLOCKING or integration-introduced issue found. Unchanged text cleared by the five lens reviews and by pass 1 is not reopened.

## Open user decisions

None required now. The two conditional decisions from pass 1 (social-preview upload fallback; private-vulnerability-reporting enable failure) are unchanged and are stated in the plan as concrete choices.

## Pass 2 summary

Both blocking task-order contradictions are resolved in the plan text, all four advisories are adopted, and the revision introduced no new contradiction. The plan is coherent end to end: every task is a self-contained behaviour unit whose focused check can pass at its own commit, acceptance runs on shipped entry points, Task 6 is the PR task, and nothing in scope is silently omitted.

VERDICT: PASS
