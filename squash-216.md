Apply the newer compiler clippy rewrite in failed_prefix framing (issue 215, the first of two sites; the second is PR 218).

Hosted results at head db4e925: corpus pass; fuzz pass; native ubuntu pass; qualification pass; release-gate pass; reuse pass; wasm pass; windows-engines pass. Native macOS fails as in issue 180: cancelled before any step, runner configuration macos-14-arm64-us-default not supported; main run 43528776017 shows the same.

Review: independent review PASS at db4e925, .stage/failed-prefix-lint/reviews/db4e925-review.md, written before the merge. My own reading of the merging diff agrees: one hunk, failed_prefix.rs only.

Local gate: make check at db4e925 exits 2 only on the helper-census site (crates/helper-census/src/source.rs:962, on main, outside this change), which PR 218 fixes. The keep-going workspace clippy shows no site left in failed_prefix.rs with this change.

Refs #215, closes #215
