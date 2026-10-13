# Prior-art inventory: issue #26: CI: lint the whole workspace with the release toolchain on pull requests

Evidence: prior-art.md, references.md, brief.json, issue.md, plus read-only git and file reads of this worktree (verify.yml, release-qualification.yml, Makefile, git log). The forge was not queried. Coverage, copied:
- issue #26 + 0 comment(s) via github
- link crawl: 2 issue(s) within 2 hop(s) of `#N` references
- forge search on ['whole', 'workspace', 'release']: 3 other item(s)
- GhpRsq-Defect-Class trailers: 3 distinct class(es) in the last 200 commits
- prior attempts: 0 commit(s) citing #26 (second parents walked), 0 other branch(es) named for it
- PRs citing #26: 0
- requirement candidates: 0 line(s); 0 comment(s) matched no rule
- referenced artifacts: 0 path(s) (0 on disk), 2 identifier(s), 3 workspace package(s), 0 URL(s)

Note: references.md lists "None named" for paths. The issue body names no file paths. The artifacts below were added to this inventory by the assessment task, not by the issue or brief.

## Referenced artifacts
| Reference | Resolved | Exists | Lines | Licence header | Symbol ranges |
|-----------|----------|--------|-------|----------------|---------------|
| .github/workflows/verify.yml | this repo | yes | 139 | SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 (line 2) | n/a (YAML); job ranges below |
| .github/workflows/release-qualification.yml | this repo | yes | 51 | SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 (line 2) | n/a (YAML); job ranges below |
| Makefile (lint target) | this repo | yes | 102 | not read | Makefile:22-23 |
| commit 598d059 (#24 fix, on main; fac0d98 is the same change on the #1 branch) | this repo | yes (598d059 is an ancestor of HEAD) | n/a | n/a | n/a |

Job and step ranges, from grep and sed output:
- verify.yml `msrv`: lines 35-44. Env sets the 1.98.0 toolchain and RUSTFLAGS -D warnings (lines 37-38). Steps: checkout (40-43), a toolchain-install step for 1.98.0 (44), `cargo test --workspace --locked` (45). No Clippy step.
- verify.yml `windows-engines`: lines 46-60. Env sets the 1.98.0 toolchain and RUSTFLAGS -D warnings (lines 48-49). Steps: toolchain-install for 1.98.0 (55), `cargo test -p purrassist-core -p purrassist-syntax --locked` (56), `cargo test -p purrassist-derive -p purrassist --locked` (57), `cargo test -p purrassist-host -p purrassist-cli --locked` (58), a step adding the clippy component (59), `cargo clippy -p purrassist-host -p purrassist-cli --all-targets --locked -- -D warnings` (60).
- verify.yml `native` (comparison): toolchain-install for the pinned nightly (line 24), then `make check` (line 25).
- release-qualification.yml: env sets RUSTUP_TOOLCHAIN 1.98.0 for the `package` job (line 14). Steps include "Verify workspace" running `make check`, then `make corpus` and `make package`.
- Makefile: `check: fmt hygiene helper-test helper-check lint test doc` (line 6); `lint:` at line 22; the recipe `cargo clippy --workspace --all-targets -- -D warnings` (line 23, no `--locked`).
- Fix commit 598d059 `test(core): compare the double-rounding result by bits (#24)`: its message states the rust-v0.1.0 release-qualification run on 464306c failed on `clippy::float_cmp` in `f32_rounds_the_spelling_once_to_nearest` (`crates/core/src/tool_call/from_node.rs:336`). The fix changes only that test. The source for the failure is the commit message and the issue #26 body; no CI log is in the repository.

## Prior attempts at this issue
- Commits: none found citing #26 (per prior-art.md and brief.json; not re-searched in this task).
- Branches: none (brief.json `prior_attempts.branches` is empty; the own branch is `paudley/26-lint-the-whole-workspace-with-the`).
- PRs: none found (per prior-art.md; the forge was not queried).

Earlier commits that changed verify.yml's Clippy or msrv/windows-engines jobs (`git log -p -- .github/workflows/verify.yml`):
- 46e9d1a 2026-09-28 `feat: establish intent recovery workspace and verification foundation`: added the `msrv` job (tests only, no Clippy) and the `windows-engines` job (tests only, no Clippy).
- 0b7c7de 2026-09-28 `fix: make Windows durability code lint-clean and fail explicitly`: added the clippy-component step and `cargo clippy -p purrassist-host -p purrassist-cli --all-targets --locked -- -D warnings` to `windows-engines`.
- No other verify.yml commit in the history matched a Clippy or msrv line in `git log -p`.
- The stable Clippy fixes for windows-engines (2864e75, 76d5e73) did not change verify.yml.

## Linked issues
| # | Relation | State | Title |
|---|----------|-------|-------|
| 24 | mentions | MERGED | test(core): compare the double-rounding result by bits |
| 1 | 2-hop | CLOSED | Complete PurRAssist 0.1.0 plan and release qualification |

Related items (forge, per prior-art.md): #4 CLOSED, #1 CLOSED, #3 CLOSED (tool-call recovery items).

## Governing ADRs
- The issue cites no ADRs: NOT FOUND

## Recurrence
Copied tallies (`GhpRsq-Defect-Class` trailers, last 200 commits, per prior-art.md and brief.json):
- `none`: 6 prior instance(s)
- `documentation-drift`: 2 prior instance(s)
- `performance`: 1 prior instance(s)

Stable-only lint breaks, counted from git log and commit messages (not from trailers):

| Event | Commit (date) | Lane that broke | Lint | Stable-only per message? |
|-------|---------------|-----------------|------|--------------------------|
| 1 | 598d059 / fac0d98 fix; failure on the rust-v0.1.0 run of 464306c (2026-10-12) | release qualification (`make check` on 1.98.0) | `clippy::float_cmp` | yes: message says local `make check` uses nightly and did not catch it |
| 2 | de8e8fd (2026-09-28) | windows-engines (1.98.0 Clippy) | `too_long_first_doc_paragraph` (ScratchString) | yes: message says the nightly lint set does not flag it |
| 3 | 2864e75 (2026-10-12), fixed in 76d5e73 (#22) | windows-engines (1.98.0 Clippy) | `too_long_first_doc_paragraph` x3 (K_PRINT, render_tool_result_governed, CallSegment) | yes: message says stable Clippy rejects; 3037418 body (line 97) also records the failure on main |
| 4 | 0b7c7de (2026-09-28) | windows-engines (Clippy with -D warnings, first added here) | several, not named as stable-only | UNKNOWN: message lists const-eligible helpers, an inverted condition, a redundant clone, duplicate match arms, and a byte-to-FILE_RENAME_INFO cast; lint names not given |

Counts: release breaks by a stable-only lint: 1 (event 1). CI breaks by a stable-only lint: 2 (events 2 and 3). Event 4 is a CI Clippy break with UNKNOWN stable-only attribution, counted separately. The grep for `clippy` across all refs returns 12 commits, which includes non-breakage commits. The grep for `float_cmp|too_long_first_doc|stable clippy|stable Clippy` also matched 7ba482d and 3037418; their bodies were not counted as separate events (7ba482d's body was not read). Any recurrence figure beyond these four events is UNKNOWN.

## NEEDS-JUDGMENT
1. verify.yml `windows-engines` (line 60) already runs `cargo clippy -p purrassist-host -p purrassist-cli --all-targets --locked -- -D warnings` on 1.98.0, and `msrv` (lines 35-45) runs only `cargo test --workspace --locked` with RUSTFLAGS -D warnings: should the workspace Clippy run replace the windows-engines Clippy step, sit beside it, or move into `msrv` as the issue's example suggests?
2. Makefile line 23 is `cargo clippy --workspace --all-targets -- -D warnings` (no `--locked`), while the issue's proposed command includes `--locked`: which form should the PR job run, and should `make lint` change?
3. Commit messages 598d059 and 76d5e73 say local `make check` runs on nightly, and verify.yml `native` pins a nightly (line 24), but release-qualification.yml sets RUSTUP_TOOLCHAIN 1.98.0 (line 14) for `make check`: which toolchain should the PR job lint with, given that the issue asks for 1.98.0?
4. Event 4 (0b7c7de): the lint names are not recorded, so it is UNKNOWN whether it was a stable-only lint. Should it count toward the recurrence figure?
5. The rust-v0.1.0 failure (event 1) is evidenced only by commit 598d059 and the issue #26 body; no CI run log is in the repository. Is that sufficient evidence for the planner's recurrence claim?
6. The issue's acceptance (a float `assert_eq!` in a purrassist-core test fails a hosted PR job with `clippy::float_cmp`) is not checked by any existing job: should a test or fixture for it be part of the planned work, and where should it live?
7. Licence: the Makefile header was not read; no port source is involved; the repository licence row is absent from references.md, so the repository licence is UNKNOWN for comparison.
