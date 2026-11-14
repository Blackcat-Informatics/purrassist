Use the worked example as the Parse test valid neighbour (issue 210, follow-up to PR 195).

Hosted results at head 925a3cc: corpus pass; fuzz pass; native ubuntu pass; qualification pass; release-gate pass; reuse pass; wasm pass; windows-engines pass. Native macOS fails as in issue 180: cancelled before any step, runner configuration macos-14-arm64-us-default not supported; main run 43528776017 shows the same.

Review: independent review PASS at 925a3cc, .stage/parse-neighbour/reviews/925a3cc-review.md, written before the merge. My own reading of the test diff agrees: one line added, six removed, in graph_load.rs; the Parse assertion is unchanged.

Local make check at 925a3cc: exit 0.

Refs #191, closes #210
