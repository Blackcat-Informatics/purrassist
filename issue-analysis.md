# Requirements ledger: issue #26: CI: lint the whole workspace with the release toolchain on pull requests

Evidence: issue.md, requirements-candidates.md, brief.json (coverage: copied from brief.json)
- issue #26 + 3 comment(s) via github
- link crawl: 5 issue(s) within 2 hop(s) of `#N` references
- forge search on ['whole', 'workspace', 'release']: 3 other item(s)
- GhpRsq-Defect-Class trailers: 3 distinct class(es) in the last 200 commits
- prior attempts: 0 commit(s) citing #26 (second parents walked), 1 other branch(es) named for it
- PRs citing #26: 2
- requirement candidates: 22 line(s); 0 comment(s) matched no rule
- referenced artifacts: 30 path(s) (16 on disk), 26 identifier(s), 3 workspace package(s), 4 URL(s)

Line references: "issue.md L<n>" is the absolute line in issue.md. Candidate line numbers in requirements-candidates.md are relative to each comment body; they map to issue.md as: comment 1 +29, comment 2 +274, comment 3 +304.

## Requirements
| R-id | Kind | Source | Quote (verbatim) | From candidate |
|------|------|--------|------------------|----------------|
| R1 | prose | body (issue.md L19) | "Run `cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.98.0 in a pull-request job, for example the existing `msrv` job." | prose |
| R2 | prose | body (issue.md L19) | "A stable-only lint then fails on the PR that introduces it, not on the release tag." | prose |
| R3 | prose | body, Acceptance (issue.md L23) | "A PR that adds an exact float `assert_eq!` in a `purrassist-core` test fails a hosted pull-request job with `clippy::float_cmp`." | prose |
| R4 | scope-out | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L102 | "Out of scope. It's an owner ruleset setting, and the `main` ruleset has no required status checks." | C1.8 |
| R5 | scope-out | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L133 | "**Make the job a required status check:** declined. It's an owner ruleset setting. The cost: a red `release-gate` job does not block a merge, so it relies on review discipline until the owner makes it required." | C1.9 |
| R6 | list-item | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L172 | "On a branch `probe/26-float-cmp` from the Task 1 head, restore the pre-#24 runtime comparison in `crates/core/src/tool_call/from_node.rs`." | C1.10 |
| R7 | modal | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L173 | "Commit it normally, with no attribution, and with a message saying it is a deliberate failing probe for #26 that must not merge. Then push the branch." | C1.11 |
| R8 | list-item | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L174 | "Local supporting evidence: 1.98.0 Clippy reports `float_cmp` at that line." | C1.12 |
| R9 | list-item | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L175 | "Open a **draft** PR titled `probe(#26): deliberate float_cmp failure, do not merge`, with no closing keyword." | C1.13 |
| R10 | modal | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L176 | "Required: its hosted `release-gate` job fails with `clippy::float_cmp` at the probe line, while `native` passes." | C1.14 |
| R11 | list-item | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L177 | "Record the run URL and log excerpt in `validation.md` and on issue #26." | C1.15 |
| R12 | list-item | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L178 | "Close the probe PR unmerged and delete only its branch." | C1.16 |
| R13 | list-item | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L179 | "Task review: `tasks/T2-review.md` checks the evidence." | C1.17 |
| R14 | prose | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L226 | "The item 4 residual and the cost of the required-check decline are recorded." | C1.18 |
| R15 | modal | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L255 | "B2: the MSRV-evidence sentence is required." | C1.19 |
| R16 | decision | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L67 | "**The defect class is gate divergence**, not one lint." | prose |
| R17 | decision | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L69 | "A single `make release-gate` target holds those five gates." | prose |
| R18 | decision | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L70 | "The release workflow runs `make release-gate` as its one verification step, after its tag/version check." | prose |
| R19 | decision | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L71 | "A pull-request job named `release-gate` runs the same target with the same two toolchains." | prose |
| R20 | decision | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L100 | "`make lint` gains `--locked`." | prose |
| R21 | decision | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L147 | "the `msrv` job becomes `release-gate`;" | prose |
| R22 | decision | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L150 | "it drops `RUSTFLAGS: -D warnings`, so its environment equals the release job's." | prose |
| R23 | scope-out | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L135 | "declined. It's the only Windows-`cfg` Clippy run." | prose |
| R24 | scope-out | comment 1 (paudley, 2026-10-13T08:54:53Z), issue.md L134 | "declined. The hosted probe (Task 2) plus Task 3's rerun when gate files change cover the one-time acceptance, and the structural P1/P2 rules guard against drift." | prose |

Kinds: checkbox, modal, list-item, scope-out, dependency, question, decision, prose.

## Candidate dispositions
| Candidate | Disposition |
|-----------|-------------|
| C1.1 | dropped: quoted from another source (.goals / AGENTS.md standing constraints listed in comment 1, issue.md L57) |
| C1.2 | dropped: quoted from another source (.goals / AGENTS.md, issue.md L58) |
| C1.3 | dropped: quoted from another source (.goals / AGENTS.md, issue.md L59) |
| C1.4 | dropped: quoted from another source (.goals / AGENTS.md, issue.md L60) |
| C1.5 | dropped: quoted from another source (.goals / AGENTS.md, issue.md L61) |
| C1.6 | dropped: quoted from another source (.goals / AGENTS.md, issue.md L62) |
| C1.7 | dropped: quoted from another source (.goals / AGENTS.md, issue.md L63) |
| C1.8 | kept → R4 |
| C1.9 | kept → R5 |
| C1.10 | kept → R6 |
| C1.11 | kept → R7 |
| C1.12 | kept → R8 |
| C1.13 | kept → R9 |
| C1.14 | kept → R10 |
| C1.15 | kept → R11 |
| C1.16 | kept → R12 |
| C1.17 | kept → R13 |
| C1.18 | kept → R14 |
| C1.19 | kept → R15 |
| C2.1 | dropped: quoted from another source (CI log output quoted in comment 2, issue.md L298) |
| C2.2 | dropped: progress report, covered context under the owner rule (comment 2 result: other jobs passed; not a new requirement) |
| C3.1 | dropped: duplicate of R5 (restates the required-check decline) |

Lexical kept: 12. Lexical dropped: 10. Prose additions: 12 (R1–R3 from the body; R16–R24 from comment 1 plan decisions and scope items not caught by the rules).

## Comment coverage
| Comment | Author | Rows / disposition |
|---------|--------|--------------------|
| 1 | paudley (2026-10-13T08:54:53Z) | R4–R24. Plan body: problem and design context (issue.md L30–L66, L68–L99, L101–L146, L151–L169, L190–L255 excluding R14, R15) is covered context under the owner rule; standing-constraint quotes dropped as quoted from another source. |
| 2 | paudley (2026-10-13T10:13:34Z) | no requirement: progress report on Tasks 1–2 (probe result). C2.1 and C2.2 dropped as listed above. |
| 3 | paudley (2026-10-13T11:03:34Z) | no requirement: handoff report. Its scope question and open owner decision are in NEEDS-JUDGMENT 2 and 5; C3.1 duplicates R5. |

## Dependencies (from brief.json relations)
- #24 mentions [state not recorded in brief.json]
- #1 mentions [CLOSED]
- #28 mentions [OPEN]
- #29 mentions [CLOSED]
- #26 closes (self-entry under link_relations; state OPEN)
- related (brief related list, not in link_relations): #4 [CLOSED]
- related (brief related list, not in link_relations): #3 [CLOSED]

## NEEDS-JUDGMENT
1. R1 vs R21: "for example the existing `msrv` job." (body, issue.md L19) / "the `msrv` job becomes `release-gate`;" (comment 1, issue.md L147): is the PR job the existing msrv job, renamed and changed to run `make release-gate`, or a separate job?
2. R1 vs R16 and R19: "Run `cargo clippy --workspace --all-targets --locked -- -D warnings` on Rust 1.98.0 in a pull-request job" (body, issue.md L19) / "this is broader than the issue's literal ask." (comment 3, paudley, 2026-10-13T11:03:34Z, issue.md L333): should the PR job run the whole release gate or only the workspace Clippy command the issue names? Comment 3 asks the owner to object.
3. R3 vs comment 2: "fails a hosted pull-request job with `clippy::float_cmp`." (body, issue.md L23) / "-D clippy::float-cmp" (comment 2, paudley, 2026-10-13T10:13:34Z, issue.md L298 and L324): does the hyphenated log name satisfy the literal acceptance text `clippy::float_cmp`?
4. R3 vs R24: "fails a hosted pull-request job" (body, issue.md L23) / "the claim that a stable-only lint fails the PR is re-demonstrated only when gate files change." (comment 1, issue.md L134): does one probe run satisfy a standing acceptance, given the plan limits re-demonstration to gate-file changes?
5. R5 vs comment 3: "declined. It's an owner ruleset setting." (comment 1, issue.md L133) / "Making it required is your setting." (comment 3, paudley, 2026-10-13T11:03:34Z, issue.md L338): the owner decision on making `release-gate` a required check is open in the thread; no answer is recorded.

## Self-check
Quotes checked: 24. Misses: 0. Comments covered: 3/3.
Quote misses: 0
