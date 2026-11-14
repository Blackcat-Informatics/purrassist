Refuse a declared named graph that holds no statements instead of dropping it (issue 209, follow-up to PR 195).

Hosted results at head 754a438: corpus pass; fuzz pass; native ubuntu pass; qualification pass; release-gate pass; reuse pass; wasm pass; windows-engines pass. Native macOS fails as in issue 180: the job is cancelled before any step, with the runner configuration macos-14-arm64-us-default not supported; main run 43528776017 shows the same.

Review: independent review PASS at 754a438, .stage/empty-named-graph/reviews/754a438-review.md, written before the merge. make check exit 0 at 754a438 (local).

Refs #191, closes #209
