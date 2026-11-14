Name the subject and predicate of an FnO-view statement refusal (issue 207, fix forward from PR 195).

Hosted results at head 1e304ea: corpus pass; fuzz pass; native ubuntu pass; qualification pass; release-gate pass; reuse pass; wasm pass; windows-engines pass. Native macOS fails as in issue 180: the job is cancelled before any step, with the runner configuration macos-14-arm64-us-default not supported; main run 43528776017 shows the same.

Review: independent review PASS at 1e304ea, .stage/fno-statement-subject/reviews/1e304ea-review.md, written before the merge. make check exit 0 at 1e304ea (local).

Refs #191, closes #207
