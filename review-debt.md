# Review debt: PR #9 (closes issue #8), head 42899ab

Sources: pr-comments.md (19 conversation comments), raw/pr-reviews-final.json (0 reviews),
raw/pr-inline-comments-final.json (0 inline comments), raw/pr-threads-final.json (0 threads, totalCount 0),
pr-checks.txt, validation.md (Stage 2 exit and Stage 3 sections), completion-audit-exit.md (PASS).

All 19 comments are authored by `paudley` (the workflow's own account). No bot, reviewer, or third-party
author appears. All are status posts (plan, handoff, task summaries, completion note). None asks a
reviewer for input that is still unanswered.

## Items

| id | author | type | summary | disposition | evidence |
|---|---|---|---|---|---|
| C1 | paudley | PR comment | Stage 1 plan for #8 (three gaps, task workflow). | Addressed: status post, no request. | Plan executed; Stage 2 exit audit PASS (completion-audit-exit.md). |
| C2 | paudley | PR comment | Stage 1 handoff "least confident". Contains three concerns and one conditional ("if you want abstention there, that is a policy change to decide"). | Addressed, all sub-items. (1) Same-name/recursive-`$ref` Resolved: gap analysis reclassified the recursive `$ref` case as a defect (G2) and fixed it in e93eaf8 (C5); no one requested abstention, so the conditional policy change is legitimately declined. (2) Native/WASM comparison: fixed by R1 (c1fa81d, C3); `make wasm-test` exit 0. (3) Fuzz red on #6: attributed to base 943fa04, bounded campaigns on both targets, 0 branch-only failures (C16). The other notes (re-pinned digests, golden reorder, pointer fix, `verdict` branching, CLI naming limit) are informational and are covered in INTEGRATION.md compatibility section (2484700, C18). | gap-analysis.md G2; completion-audit-exit.md G2 rows; validation.md. |
| C3 | paudley | PR comment | R1/G1: Node repinned to v26.11.1 (c1fa81d); wasm-test passes. | Addressed: status post. | `make wasm-test` exit 0 on 42899ab. |
| C4 | paudley | PR comment | Stage 2 remediation plan (G1-G26, R1-R15). Only "?" is a quoted, already-answered user question ("Fix them in this PR"). | Addressed: plan fully executed (R1-R15 each committed and reviewed); no open question. | C3, C5-C18; commits listed in C19. |
| C5 | paudley | PR comment | R2/G2 CRITICAL: `$ref`/`anyOf` scopes declare member names (e93eaf8). | Addressed: status post of a fix. | completion-audit-exit.md G2 rows VERIFIED. |
| C6 | paudley | PR comment | R3/G14: reasoning text never reverses (4b80ade). | Addressed: status post of a fix. | Mutation check recorded; exit audit PASS. |
| C7 | paudley | PR comment | R4/G3 strict choice fail-closed (69c6b5c). | Addressed: status post of a fix. | Exit audit re-executed on CLI and MCP. |
| C8 | paudley | PR comment | R5/G5, A5, G6 linear item numbering (3111582). | Addressed: status post of a fix. | Counter and cost tests. |
| C9 | paudley | PR comment | R10/G18 repeated parameters linear. | Addressed: status post of a fix. | Cost test; measured. |
| C10 | paudley | PR comment | HashMap to BTreeMap (342f2d4); R12/G20 budget constants raised (8fb63a4). | Addressed: status post of fixes. | `make check` exit 0 (includes hygiene). |
| C11 | paudley | PR comment | R13/G21 Mistral v11 chained calls linear (bdb102a). | Addressed: status post of a fix. | Measured; goldens unchanged. |
| C12 | paudley | PR comment | R14/G22 Mistral v3 arrays linear (3743aed); DeepSeek restart filed as #10, user-accepted. | Addressed (fix); #10 legitimately declined for this PR by explicit user decision, tracked in its own issue. | Issue #10; completion-audit-exit.md decision (5). |
| C13 | paudley | PR comment | R6/G7 reasoning markers inside JSON strings (917553e). | Addressed: status post of a fix. | Repro inputs resolve. |
| C14 | paudley | PR comment | R15/G25 residual superlinear time (c2f1916); receipt fixture refresh (7136da7). | Addressed: status post of fixes. | Revert-detecting unit tests. |
| C15 | paudley | PR comment | R11/G19 root `$ref` projects parameters (a49e81b). | Addressed: status post of a fix. | Built CLI check; exit audit. |
| C16 | paudley | PR comment | R7/G4 fuzzer reaches new code (8bca69e); #11 slow repair search accepted by user. | Addressed (fuzz target and campaigns); #11 legitimately declined for this PR by explicit user decision, tracked in its own issue. | Issue #11; 0 branch-only failures; 727 retained inputs replayed. |
| C17 | paudley | PR comment | R8/G8 reading identity test (f93f0b4). | Addressed: status post of a fix. | Mutation check. |
| C18 | paudley | PR comment | R9/G10, G13 compatibility statement and published replay (2484700). | Addressed: status post of a fix. | INTEGRATION.md Compatibility section. |
| C19 | paudley | PR comment | Stage 2 complete: exit audit PASS on 42899ab; lists unrun items (hosted CI; `make fuzz` gate form stops on pre-existing #6 and #11). | Addressed: completion note, no request. Unrun items are covered in the checks section below. | completion-audit-exit.md VERDICT: PASS. |
| R-* | none | Review submissions | 0 reviews. | Nothing to disposition. | raw/pr-reviews-final.json is `[]`. |
| I-* | none | Inline comments | 0 inline comments. | Nothing to disposition. | raw/pr-inline-comments-final.json is `[]`. |
| T-* | none | Review threads | 0 threads, none unresolved. | Nothing to disposition. | raw/pr-threads-final.json totalCount 0. |

Questions or requests needing an answer: none. C2's conditional abstention offer and C4's quoted question
are the only question-like text; both are resolved above.

## Checks

- Hosted CI did not run: all 8 Verify jobs (corpus, fuzz, msrv, native macos-14, native ubuntu-24.04,
  qualification, wasm, windows-engines) show `fail` in 2-6 s without having run.
- No required checks are configured (no branch protection, no rulesets).
- User decision: merge on local evidence.
- Local validation per validation.md on 42899ab: `make check` exit 0; `make wasm-test` exit 0; correction
  corpus 0 incorrect over 18,176 cases; bounded fuzz on `tool-call` and `tool-call-ref` with #6 attribution
  (`--continue-on-known`, 0 branch-only failures); exit completion audit PASS on 42899ab. Integration tree
  equals head tree (no base drift), so this evidence applies to the merge candidate.
- Known and unrun: `make fuzz` in its gate form stops on #6 and #11, both pre-existing on main and tracked.

## Open required items

Count: 0

VERDICT: CLEAR
