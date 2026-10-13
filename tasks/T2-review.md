# T2 review: hosted acceptance probe (issue #26, R3 and R4)

## Scope

Plan Task 2 and contract rows R3 and R4, with R1 as the baseline. The recorded evidence in `validation.md` was checked against live read-only GitHub state for Blackcat-Informatics/purrassist. Nothing was modified except this file.

## Evidence (all re-queried live)

- Run 38377011106: `headSha` is `f06cde1cf3d95b0f21c860d92bf2d50485491a1d`, conclusion `failure`. Of eight jobs, only `release-gate` (115227814445) failed. wasm, native (macos-14), native (ubuntu-24.04), fuzz, windows-engines, corpus and qualification all succeeded. The nightly `native` jobs passing shows the failure came from the release-toolchain gate (R3).
- The `release-gate` log has `error: strict comparison of f32 or f64` at `crates/core/src/tool_call/from_node.rs:336:9`, the note `-D clippy::float-cmp implied by -D warnings`, and a help link under `rust-1.98.0` (the toolchain is evidenced in the log itself). The failure path is `make[1]: *** [Makefile:23: lint] Error 101`, then `make: *** [Makefile:56: release-gate] Error 2`, so it is the lint step of the shared gate, not an unrelated failure.
- PR #29: state CLOSED, `mergedAt` null, `isDraft` true, title `probe(#26): deliberate float_cmp failure, do not merge`, base main, head `probe/26-float-cmp`, `closingIssuesReferences` empty, no closing keyword in the body.
- `git ls-remote --heads ... probe/26-float-cmp` returns nothing, and the branches API returns 404. The branch is deleted.
- Commit f06cde1: parent is 2212844 (the Task 1 head). It changes only `crates/core/src/tool_call/from_node.rs`, +2/-2, swapping `(... as f32).to_bits()` / `1.0_f32.to_bits()` for `... as f32` / `1.0` in the one assertion. Message states it is a deliberate failing probe that must not merge. A grep of the message for co-authored, generated, claude and anthropic finds nothing. Author and committer are the user.
- R1 baseline: run 38376755652 on 2212844, conclusion success, all eight jobs success. `release-gate` 115226959552 ran with `RUSTUP_TOOLCHAIN: 1.98.0` (rustc 1.98.0 installed), and its log shows `cargo clippy --workspace --all-targets --locked -- -D warnings` under `make release-gate`. This also holds.
- Issue #26 has the recording comment (issuecomment-6143289107, "Tasks 1 and 2 done"), matching R4's requirement to record on the issue. `validation.md` has the run id, job id and log excerpt.

## Findings

1. The entry point is real. The probe ran as a pull-request Verify workflow on GitHub-hosted runners, through the same `make release-gate` the release uses. Comparing it with the passing R1 run on the parent commit isolates the one-assertion delta as the cause.
2. Minor: `validation.md` gives the run id but not the full run URL (it can be derived from the id; the issue comment may carry it). Not blocking.
3. Residual, already declared in the plan: the demonstration holds for the current `verify.yml` and `Makefile`. Task 3 must rerun it if either changes after 2212844. The probe commit's parent is 2212844, so this is currently satisfied.
4. No attribution, no closing keyword, branch removed, PR unmerged. These all match plan steps 2, 4 and 7.

## Judgement

R3 is demonstrated: a stable-only lint failed the hosted PR's `release-gate` job while the nightly jobs passed. R4 is demonstrated: an exact float `assert_eq!` in a core test failed a hosted PR job with `float_cmp` at the probe line, and the evidence is recorded in `validation.md` and on the issue.

VERDICT: PASS
