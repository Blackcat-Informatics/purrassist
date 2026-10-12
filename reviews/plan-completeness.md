# Plan completeness review

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and/.stage/complete-purrassist-0-1-0-plan-and/plan.md`

Inputs I read in full: `issue.md` (the body and all 63 comments), `issue-analysis.md`, `prior-art-assessment.md`, and the binding session decisions the caller passed on. I checked the worktree at `f11b45e` for:

- The entry points: `crates/cli/src/commands.rs` provides `decode --schema/--input`, `call TOOL --root`, and `mcp --root`.
- The `check-release.py --preflight-owners --user-agent` usage.
- The `include_str!` READMEs (`core`, `host`, `purrassist`, `derive`).
- The `examples/` directory.
- `docs/purrassist-logo.svg`.
- The stale phrases, using a grep over tracked `*.md`.

## Requirement → task → check

| # | Requirement (source) | Task | Acceptance check | Status |
|---|---|---|---|---|
| U1 | Ignore hosted CI; never mention CI-provider account matters in documents (session decision) | 1 (wording rule) | None. Task 1 states the rule, but no check enforces it. | PARTIAL (A2) |
| U2 | #6/#7/#10/#11 do not block; disclose them as known limitations (session decision, analysis D-F2) | 1, 3 | `check-docs.py` requires the issue numbers in QUALIFICATION, FUZZING and CHANGELOG | MAPPED |
| U3 | Release docs current: derive reservation recorded, trusted publishing (TP) state, release step, release notes (session decision, analysis RELEASE inventory) | 3 | `check-release.py --preflight-owners …` exits 0; `check-docs.py` checks the order and the derive wording | MAPPED (A7) |
| U4 | Updated README (session decision; analysis README inventory) | 2 | marker block, crate map, tool census, doctest, CLI/MCP runs | MAPPED; check insufficient (B4) |
| U5 | Other doc updates: QUALIFICATION, IMPLEMENTATION, FUZZING, CONTRIBUTING, SECURITY, crate READMEs (session decision; analysis inventory) | 1, 2, 3 | `check-docs.py`, the stale grep, reviewer read | PARTIAL (B1, B2) |
| U6 | Repo is public; set the GitHub description and other settings (session decision) | 5 | `gh repo view --json …` and the PVR API | MAPPED (A4, A5) |
| U7 | Three-way interface: Rust crate, MCP, CLI (session decision) | 2 (+3, 4, 5) | `three-interfaces` marker block; CHANGELOG names all three interfaces; Cargo descriptions | MAPPED |
| U8 | Add `social-preview.png` (session decision) | 4 (+5 upload) | IHDR 1280×640, under 1,000,000 bytes, SVG digest | MAPPED (A9) |
| U9 | Derive 0.0.0 reservation already published; record it (session decision, R9b) | 3 | Preflight owners plus the RELEASE.md wording rule | MAPPED |
| U10 | Tag and publication are out of this stage (session decision) | 6 handoff | The owner steps are listed | MAPPED |
| R1–R6, R10, R12 | Implementation (issue body) | n/a | Already done in `699d9ff`, `943fa04`, `fccfbe7`, `f11b45e` (analysis class a) | SATISFIED |
| R7 | Full `make check` gate (release acceptance bullet 1) | 6 | `make check` exits 0 on the final head | MAPPED |
| R8a | Archive and notice audit, now six crates (release acceptance bullet 2) | 2, 6 | `make package` exits 0; the audit covers six | MAPPED |
| R8b, R9a, R9c | Downloaded-package verification, hosted CI, publish/release | out of stage | User decision; issue stays open (`Refs #1`) | OUT OF STAGE (user) |
| R9b | Derive trusted publisher | owner | Recorded as owner-pending in RELEASE.md and the handoff | MAPPED |
| R9d | Do not describe the qualification workflow as a publish workflow | 3 | Implicit: RELEASE.md rewrite keeps the procedure | MAPPED (implicit) |
| R11 | Issue branch + PR | 6 | PR body says `Refs #1` | MAPPED |
| R13 | Never bypass hooks | every commit | standing constraint | MAPPED |
| D-F1 | Fact validator in hygiene; **each rule has at least one failing fixture** (analysis) | 1–4 | `test-check-docs.py` has fixtures only for the Task 1 rules | PARTIAL (B3) |
| D-F2 | Known-limitations disclosure (analysis) | 1 | issue-number rule | MAPPED |
| D-F3 | Stale-phrase regression grep (analysis) | 1 | The plan's grep is narrower than D-F3 and misses live stale text | INSUFFICIENT (B2) |
| D-F4 | Read-only registry owner preflight (analysis) | 3 | `check-release.py --preflight-owners` | MAPPED |
| D-F5 / D-F6 | `make check` / `make package` (analysis) | 6 | single full run | MAPPED |
| D-F7 | Every status statement separates implemented / qualified / pending (analysis) | 1, 3 | reviewer read | PARTIAL (B1) |
| D-F8 | PR body says `Refs #1` **and lists the remaining category (c) items** (analysis) | 6 | `Refs #1` only | PARTIAL (A8) |
| D-Q3 | 0.1.0 release notes (analysis) | 3 | CHANGELOG `## 0.1.0` rules | MAPPED (A3) |
| D-Q4 | QUALIFICATION Windows claim (analysis) | 1 | reviewer read | MAPPED |

Structure: Tasks 1–6, with no Task 0. Tasks 1–4 each have a focused check, a task review, commit, push and an issue update. Task 5 is no-commit by design; it posts an issue update but has no review step (A5). Task 6 runs the gate, creates the PR, posts the plan and hands off.

## Findings

### B1. BLOCKING (UNMAPPED / SILENT CUT): QUALIFICATION "Release acceptance after review and merge" and IMPLEMENTATION's delivery sequence are never corrected

The plan's own inventory lists this item (plan line 90):

> "Line 226 still refers to "reviewed merge commit"."

No task corrects it.

- **Task 1's QUALIFICATION corrections** (plan lines 168–173) cover ten targets, receipt v3, archived evidence, Windows, `make fuzz` stops and Stage 2. They do not cover this heading or sentence.
- **Task 3** (plan line 238) changes only RELEASE.md's release step.

The worktree confirms the stale text is live:

- `docs/QUALIFICATION.md:224`: "## Release acceptance after review and merge"
- `docs/QUALIFICATION.md:226-227`: "reserves publication for the reviewed merge commit. It requires the full gate and hosted CI on that commit"

Task 1 also rewords CONTRIBUTING.md:20 to point away from QUALIFICATION, which leaves this heading untouched.

The issue analysis's IMPLEMENTATION inventory (L99-108, "Required delivery sequence") says no stage is marked done. Its final line, "Every stage remains required", mixes implemented work with plans, contrary to AGENTS.md and D-F7. The plan's IMPLEMENTATION correction (line 167) covers only the status line.

REWRITE TO: insert the following into Task 1 **Corrections → QUALIFICATION**, after "The "Stage 2" section is relabeled as historical, bound to its commit.":

```
  - The "Release acceptance after review and merge" section (heading and lines 224–237) is retitled "Release acceptance" and states: implementation merged to main (`699d9ff`, then `943fa04`, `fccfbe7`, `f11b45e`); publication happens from a separately authorized main commit tagged `rust-v0.1.0`; the remaining steps (derive trusted-publisher confirmation, tag, publish, downloaded-package verification, GitHub release) are pending. No sentence refers to "the reviewed merge commit". The hosted-CI procedural step is kept as procedure text only, not as a result claim.
```

Also add this under **Corrections → IMPLEMENTATION status**:

```
- **IMPLEMENTATION delivery sequence (lines 99–108):** stages 1–4 are marked delivered (merged to main) and stage 5 lists what is done (conformance, fuzzing, portability, performance, packaging, documentation, 0.0.0 registry records) separately from what is pending (derive trusted publisher, tag, verified publication).
```

### B2. BLOCKING (UNFALSIFIABLE / insufficient check): the stale-phrase grep misses stale text that exists today

Plan line 117 has this grep:

> "`git grep -nE 'issue branch\|nine (coverage-guided )?targets\|wire v2\|no placeholder record yet'` on the public docs is empty."

It is narrower than analysis D-F3, so it can return empty while stale text remains:

- **"nine mutation targets"** (`docs/QUALIFICATION.md:168`): `nine (coverage-guided )?targets` does not match it.
- **"reviewed merge commit" / "after review and merge"** (QUALIFICATION:224/226): not covered (see B1).
- **"follows review and merge"** (README.md:17, IMPLEMENTATION.md:7): not covered.
- **"stage report"** (FUZZING.md:967): not covered. The `.stage/` link rule does not match the prose "see its stage report".

The plan also never names the file set ("the public docs").

REWRITE TO (replace the grep sentence in contract row 1):

```
`git grep -nE 'issue branch|nine (coverage-guided |mutation )?targets|Nine targets|wire v2|no placeholder record|reviewed merge commit|after review and merge|follows review and merge|stage report|not yet (done|reserved)' -- README.md CONTRIBUTING.md SECURITY.md CHANGELOG.md 'docs/*.md' 'crates/*/README.md'` prints nothing (exit 1). The same phrase list is a `check-docs.py` forbidden-phrase rule, so it stays enforced in `make hygiene`.
```

### B3. BLOCKING (UNFALSIFIABLE): the rules added in Tasks 2–4 have no failing fixture

Analysis D-F1 requires: "Each rule has at least one failing fixture." The contract (row 1) lists negative mutations only for Task 1's rules: wrong count, "nine", broken link, `.stage/` link, missing SPDX, and "Pre-release".

Tasks 2–4 add rules only as "`check-docs.py` gains …" or "checks …":

- three-interfaces marker block
- tool census against `tools/list`
- crate map equals `RELEASE_ORDER`
- CHANGELOG version, crates, interfaces and limitation set
- SECURITY supported-versions table
- RELEASE.md order and derive wording
- PNG IHDR, size and SVG digest
- the receipt-version, dialect-list and known-limitations rules from Task 1

None of these requires a negative test. A rule whose matcher never fires still passes `check-docs.py` (exit 0), so "check-docs.py exits 0" proves nothing for it.

REWRITE TO: append to each of Task 1, Task 2, Task 3 and Task 4 **check-docs.py rules** (and to contract rows 1–6):

```
Every rule added in this task gets at least one mutation in `scripts/test-check-docs.py`. The mutation applies to a temporary copy of the real file and asserts that `check-docs.py` exits non-zero with that rule's named error. Required mutations:
- **Task 1:** receipt version changed to the previous value; a dialect removed from the README sentence; a known-limitations section missing #10.
- **Task 2:** marker block missing the MCP example; one tool's entry deleted from `docs/TOOLS.md` and one duplicated; a crate dropped from the README crate map; "Pre-release" put back into `crates/syntax/README.md`.
- **Task 3:** CHANGELOG heading version ≠ workspace version; #7 removed from CHANGELOG Known limitations; the SECURITY table without `0.1`; RELEASE.md crate list out of `RELEASE_ORDER`; the "no placeholder record" sentence put back.
- **Task 4:** a PNG with a 1200×630 IHDR; one byte changed in the SVG.
The task's focused check runs `python3 scripts/test-check-docs.py` and shows each of these cases failing for its named reason.
```

### B4. BLOCKING (UNFALSIFIABLE / TEST-ONLY gap): the TOOLS.md examples for 31 tools are never run

Task 2 (plan line 203) requires, for each of 31 tools, "… arguments, outcomes and an example file". Contract row 3 claims "The CLI example commands in the README and TOOLS.md run", but it runs only `call match_strings` and a `tools/list` round trip.

The worktree's `examples/` has request files for about 11 tools: apply-edit, five markdown-*, match-embeddings, match-strings, render-markdown, resolve-string, plus temperature. So about 20 example files would be new and unexecuted. A documented example that fails, or whose outcome differs from the documented one, would pass every listed check. That conflicts with AGENTS.md: "Capability claims require executable evidence."

REWRITE TO: replace the last sentence of contract row 3 and add it to Task 2's focused check:

```
`scripts/test-check-docs.py` builds `purrassist-cli` once. For every example file that `docs/TOOLS.md` references (one per `tools/list` tool; the test fails if any tool has none), it runs `purrassist call <tool> --root <fresh temp copy of the example's declared root>` with that file on stdin. It asserts the exit code and the top-level `outcome`/status field that TOOLS.md documents for that example. It also sends the same arguments as an MCP `tools/call` through `purrassist mcp --root <same root>` and asserts the result is byte-identical to the CLI output. The README `three-interfaces` CLI and MCP snippets run the same way, and `decode --schema examples/temperature.schema.json --input examples/temperature.input.txt` exits 0.
```

### A1. ADVISORY: the SECURITY.md private-vulnerability-reporting mention has no committing task

Task 5 says "SECURITY.md mentions both", but Task 5 is "no commit". Task 3 edits SECURITY.md before PVR is enabled and does not add that mention. As written, SECURITY.md never mentions PVR.

REWRITE TO (in Task 5): "After PVR is enabled, add one sentence to SECURITY.md naming GitHub private vulnerability reporting alongside the email address. Run `check-docs.py`, then commit and push it as `docs(security): name private vulnerability reporting (#1)`." Alternatively, drop the "SECURITY.md mentions both" clause.

### A2. ADVISORY: the CI-provider account wording rule (U1) is not checked

Plan line 178, "no CI-provider account wording anywhere", has no executable check. Public docs contain no such text today; I grepped for billing, spending, payment and runner capacity. The risk is new text written in this stage.

REWRITE TO: "**Wording rule:** `check-docs.py` rejects `(?i)billing|spending limit|payment|plan limits|runner capacity` in the public docs set, with a failing fixture in `test-check-docs.py`."

### A3. ADVISORY: the CHANGELOG cites #12–#14 and #16–#18 without checking they are open

The analysis records their states as "unverified", and the plan hard-codes them into the CHANGELOG rule.

REWRITE TO (Task 3): "Before writing Known limitations, run `gh issue view N --json state -q .state` for 6, 7, 10, 11, 12, 13, 14, 16, 17 and 18. List only the open ones. Record the output in the task log and the issue update."

### A4. ADVISORY: plan line 16 can be read as a literal description

> 'Set the GitHub description "and other things".'

That reads as the literal description string "and other things", while Task 5 sets a full sentence plus homepage and topics.

REWRITE TO: "Set the GitHub repository description and the other repository presentation settings (homepage, topics, unused features, social preview)."

Also, `https://docs.rs/purrassist` currently serves the 0.0.0 "Reserved name" page. Consider `https://github.com/Blackcat-Informatics/purrassist#readme` until 0.1.0 is published, or state this in the handoff.

### A5. ADVISORY (STRUCTURE): Task 5 has no task review

Task 5 records values and posts to the issue, but has no review step.

REWRITE TO: append to Task 5: "Then: task review of the before/after record against this task's list, then the issue update."

### A6. ADVISORY: D-F1 sub-rule for IMPLEMENTATION "Workspace boundaries" is not adopted

The rule requires that section to name the publishable set and label the `publish = false` members (`wasm-harness`, `purrassist-helper-census`). The plan checks only the README crate map.

REWRITE TO (Task 2, crate map rule): "…and IMPLEMENTATION's Workspace boundaries list names exactly `RELEASE_ORDER`, plus the two `publish = false` members labeled internal."

### A7. ADVISORY: two RELEASE.md items from the analysis are not addressed

- L100-104 omits the steps `release-qualification.yml` actually runs: `make corpus`, `test-fuzz-feedback.py`, `test-verify-published.py`.
- L183-184 contains the plan jargon "Task 10's registry replacement".

REWRITE TO (add to Task 3 docs/RELEASE.md): "**Accuracy:** the `release-qualification.yml` summary lists every step the workflow runs (`.github/workflows/release-qualification.yml:42-51`), and the 'Task 10' example is replaced with a public description."

### A8. ADVISORY: D-F8 PR body content

REWRITE TO (Task 6 step 4): "…body `Refs #1`, a list of the remaining out-of-stage items (derive trusted publisher, tag `rust-v0.1.0`, publication, `verify-published.py --version 0.1.0`, GitHub release), and the `--preflight-owners` output."

### A9. ADVISORY: Task 4 does not name the asset registry file

There is no generic generated-asset registry today. The only one is the `vendor/unicode/generated.json` and `vendor/entities/generated.json` pattern that `check-repository.py:329-340` reads.

REWRITE TO: "Register the pair in a new `vendor/assets/generated.json` (SVG sha256, render command, `rsvg-convert` version), checked by `check-docs.py` the same way `check-repository.py` checks `vendor/unicode/generated.json`."

## Summary

The plan covers every binding session decision: README, release docs, other docs, the three-interface presentation, the social preview, GitHub settings, the derive reservation record, and the non-blocking disclosure of #6, #7, #10 and #11. Structure is sound.

Four blocking gaps remain:

1. Live stale QUALIFICATION and IMPLEMENTATION status text is not assigned to any task (B1).
2. The stale-phrase grep cannot catch that text (B2).
3. The new fact-checker rules in Tasks 2–4 have no failing fixtures, so they can pass vacuously (B3).
4. The per-tool TOOLS.md examples, about 20 of them new, are never executed (B4).

Pass 1 verdict: BLOCKED (superseded by Pass 2 below)

---

## Pass 2 (recheck of plan revision 2)

Reviewed: the same `plan.md`, revision 2, including its dispositions (lines 347–389). The coordinator reports that #6, #7, #10–#14 and #16–#18 were verified OPEN on 2026-10-12 via `gh`.

I re-verified against the worktree:

- **Banned phrases.** A case-insensitive grep finds them only at the stale sites the plan lists:
  - `SECURITY.md:5`
  - README:14
  - the four crate READMEs
  - IMPLEMENTATION:5
  - RELEASE:83 and RELEASE:218
  - QUALIFICATION:224
  - FUZZING:967

  No legitimate text would be banned.
- **Test helpers.** `run_cli` (`crates/cli/tests/protocol.rs:672`), `run_mcp` (`:695`) and `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` (`:743`) exist.
- **Registry source.** `Tools::definitions()` exists (`crates/host/src/tools.rs:53`).
- **Dialect test.** `crates/core/tests/tool_call_dialects.rs` exists, and there are 12 `docs/tool-calls/dialects/*.md` stems.

### Prior findings

| Finding | Status | Evidence |
|---|---|---|
| B1 (QUALIFICATION release-acceptance section; IMPLEMENTATION delivery sequence) | **DISCHARGED** | Inventory lines 93–94 and 99 now list QUALIFICATION 7–8/224/226 and IMPLEMENTATION 99–108. Task 1 line 190 makes "the stale facts listed in the inventory above" the correction set. The heading "after review and merge" is in the banned list, so `check-docs.py` cannot exit 0 until the section is rewritten. See N2 for a misfiled bullet. |
| B2 (stale grep too narrow, no file scope) | **DISCHARGED** with residual N1 | Contract row 1 (line 143) has a case-insensitive banned-phrase rule in `make check`. It covers every phrase I listed, plus `not yet done`, `five of six`, `Pre-release` and `proved by the hosted`. Its scope is explicit: all tracked `*.md` except `.stage/`, `vendor/` and `crates/core/tests/data/`. |
| B3 (no failing fixtures for Task 2–4 rules) | **DISCHARGED** | Row 143: "for every rule, a fixture copy that violates it and must fail with that rule's named error, plus a fixture where the sentence the rule reads is absent". Tasks 2, 3 and 4 each repeat "with failing fixtures" (lines 251, 285, 309). Row 155 adds fixtures for each asset digest. |
| B4 (TOOLS.md examples never run) | **DISCHARGED** | Row 146: the per-tool forms come from the registry and are byte-pinned by `tools_reference_matches_registry`, and an example file is listed only "when one exists". Row 147: `examples_run_through_cli_and_mcp` requires every `examples/*.json` (no orphans) to run through both transports with identical, output-schema-valid results; temperature runs via `decode_json`. Both entry points exist. |
| A1 (SECURITY PVR mention) | DISCHARGED | Task 3 / row 154: `check-docs.py` requires both channels. See N3 for ordering. |
| A2 (CI-provider wording check) | DISCHARGED (accepted rationale) | The constraints disposition B1 keeps the rule in an unposted stage note and commits no blocklist strings. No such text exists in public docs today. |
| A3 (#12–#18 states) | DISCHARGED | `docs/known-limitations.json` uses the 2026-10-12 observation (all open, per the coordinator). The CHANGELOG, QUALIFICATION and FUZZING sets must equal it. |
| A4 (literal description; docs.rs homepage) | DISCHARGED | Line 18 is reworded. The homepage is `…/purrassist#readme`, and the docs.rs switch is an owner step. |
| A5 (Task 5 review) | DISCHARGED | Line 333: an independent reviewer writes `tasks/T5-review.md` from live `gh` output. |
| A6 (IMPLEMENTATION workspace boundaries) | DISCHARGED | Row 150: the list equals the workspace members, with the unpublished ones marked internal. |
| A7 (RELEASE summary and jargon) | DISCHARGED | Row 152 checks the summary against the `run:` steps parsed from `release-qualification.yml`. Task 3 removes the "Task 10" jargon. |
| A8 (PR owner-step list) | DISCHARGED | Task 6 step 4 lists the owner steps. The preflight output goes to `validation.md`. |
| A9 (asset registry file) | DISCHARGED | `docs/assets.json` plus the `.license` sidecar. |

### New findings (revision 2 text only)

None are blocking.

**N1. ADVISORY (UNFALSIFIABLE residue): banned phrases split across hard-wrapped lines escape a line-based matcher**

Both live instances of "reviewed merge commit" are split across a line break:

- `docs/RELEASE.md:6-7`: "performed on the reviewed / merge commit"
- `docs/QUALIFICATION.md:226-227`: "for the reviewed merge / commit"

Row 143 does not say how matching treats line breaks. A per-line regex would miss both instances and any future wrapped regression. The fixtures would still pass if they put the phrase on one line.

Today's fixes are assigned explicitly anyway: Task 3 rewrites the RELEASE intro, and the QUALIFICATION:224 heading is caught. That is why this is advisory.

REWRITE TO (append to row 143's banned-phrase sentence): "Matching runs on each file's text with every run of whitespace, including newlines and Markdown blockquote/list continuation prefixes, collapsed to one space. `test-check-docs.py` includes a fixture where `reviewed merge` and `commit` sit on consecutive lines, and it must fail."

**N2. ADVISORY: the delivery-sequence fix is filed under QUALIFICATION**

Task 1 line 196 places "The delivery sequence marks delivered stages." under **QUALIFICATION**. The text is `docs/IMPLEMENTATION.md:99-108` (inventory line 94). Only a reviewer verifies it.

REWRITE TO: move that bullet under a new **IMPLEMENTATION** fix: "IMPLEMENTATION lines 99–108: stages 1–4 marked delivered (merged to `main`); stage 5 separates done items from pending ones (derive trusted publisher, tag, verified publication); 'Every stage remains required' removed."

**N3. ADVISORY: SECURITY.md names PVR before Task 5 enables it**

Task 3 commits SECURITY.md naming GitHub private vulnerability reporting, and `check-docs.py` enforces that. Task 5 enables PVR later. If the `gh api -X PUT` call fails, the shipped doc names a channel that does not exist. Only the social preview has a NOT DELIVERED rule.

REWRITE TO (append to Task 5 "Other settings"): "If enabling PVR fails, this row is NOT DELIVERED. Revert the SECURITY.md PVR sentence and the matching `check-docs.py` requirement in a follow-up commit, then report the failure to the owner."

**N4. ADVISORY: the migration test pins the README to `f11b45e` permanently**

Row 149 freezes every backticked token and link target from `git show f11b45e:README.md` inside `make hygiene`. Two problems follow:

- It fails if a stale token must be removed deliberately (for example, the JSON-only `make fuzz TARGET=json` framing at README:426).
- It fails in any checkout without that commit.

REWRITE TO: "The migration test reads `git show f11b45e:README.md`, subtracts a reviewed `REMOVED_TOKENS` list (each entry with a one-line reason), and asserts that the remainder appears in README ∪ `docs/TOOLS.md`. It runs in Task 2's focused check and in `test-check-docs.py` only while that commit is reachable; otherwise it prints `skipped: f11b45e not present` and does not count as a pass."

### Pass 2 summary

All four prior blocking findings and all nine advisories are discharged. If every task is done as written in revision 2, every requirement in the issue, the analysis and the binding session decisions is delivered, and each has an executable check through a named, existing entry point.

The social-preview upload is honestly gated as NOT DELIVERED if the browser upload fails. Tag, publish, downloaded-package verification and hosted CI stay out of the stage by owner decision. Four advisories remain (N1–N4); N1 is the most important.

VERDICT: PASS
