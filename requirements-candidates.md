# Requirement candidates — issue #26

Lines from the issue body and comments that matched a lexical rule, copied
verbatim with the rule that fired. This is a CANDIDATE list: rules both
over-match (a "should" in passing) and under-match (a requirement stated
without a keyword). The issue-analyzer keeps or drops each row, adds what
the rules missed, and accounts for every silent comment. Code fences are
skipped.

| ID | Source | Line | Rules | Text |
|---|---|---|---|---|
| C1.1 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 28 | list-under-requirement-heading | - Rust-first, hard fails, maximal utility. |
| C1.2 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 29 | list-under-requirement-heading | - Every tool is registered. |
| C1.3 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 30 | list-under-requirement-heading | - SPDX headers on sources. |
| C1.4 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 31 | list-under-requirement-heading | - Capability claims need executable evidence. |
| C1.5 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 32 | list-under-requirement-heading | - `make check` is the local gate. |
| C1.6 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 33 | list-under-requirement-heading | - Hooks are never bypassed, including through alternative commit paths. |
| C1.7 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 34 | list-under-requirement-heading | - No agent attribution. |
| C1.8 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 73 | scope-out, modal | \| Required check? \| Out of scope. It's an owner ruleset setting, and the `main` ruleset has no required status checks. Recorded in the PR as an owner option. \| |
| C1.9 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 104 | modal | - **Make the job a required status check:** declined. It's an owner ruleset setting. The cost: a red `release-gate` job does not block a merge, so it relies on review discipline until the owner makes it required. |
| C1.10 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 143 | list-under-requirement-heading | 1. On a branch `probe/26-float-cmp` from the Task 1 head, restore the pre-#24 runtime comparison in `crates/core/src/tool_call/from_node.rs`. |
| C1.11 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 144 | modal, list-under-requirement-heading | 2. Commit it normally, with no attribution, and with a message saying it is a deliberate failing probe for #26 that must not merge. Then push the branch. |
| C1.12 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 145 | list-under-requirement-heading | 3. Local supporting evidence: 1.98.0 Clippy reports `float_cmp` at that line. |
| C1.13 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 146 | list-under-requirement-heading | 4. Open a **draft** PR titled `probe(#26): deliberate float_cmp failure, do not merge`, with no closing keyword. |
| C1.14 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 147 | modal, list-under-requirement-heading | 5. Required: its hosted `release-gate` job fails with `clippy::float_cmp` at the probe line, while `native` passes. |
| C1.15 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 148 | list-under-requirement-heading | 6. Record the run URL and log excerpt in `validation.md` and on issue #26. |
| C1.16 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 149 | list-under-requirement-heading | 7. Close the probe PR unmerged and delete only its branch. |
| C1.17 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 150 | list-under-requirement-heading | 8. Task review: `tasks/T2-review.md` checks the evidence. |
| C1.18 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 197 | modal | - The item 4 residual and the cost of the required-check decline are recorded. |
| C1.19 | comment 1 (paudley, 2026-10-13T08:54:53Z) | 226 | modal | - B2: the MSRV-evidence sentence is required. |
| C2.1 | comment 2 (paudley, 2026-10-13T10:13:34Z) | 24 | list-under-requirement-heading | - **`release-gate` failed:** `error: strict comparison of f32 or f64` at `crates/core/src/tool_call/from_node.rs:336:9`, `-D clippy::float-cmp`. |
| C2.2 | comment 2 (paudley, 2026-10-13T10:13:34Z) | 25 | list-under-requirement-heading | - **Every other job passed**, including both nightly `native` jobs. |
| C3.1 | comment 3 (paudley, 2026-10-13T11:03:34Z) | 33 | modal | - **Not a required check:** `release-gate` isn't a required status check (`main`'s ruleset has none), so a red run doesn't block a merge. Making it required is your setting. |

## Comments that matched no rule

None.
