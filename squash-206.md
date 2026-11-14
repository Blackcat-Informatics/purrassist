Adds a deterministic generated round-trip and ledger-soundness test over the vocabulary (issue 205, sixth piece of #33).

Hosted results at head d5a297a: corpus pass; fuzz pass; native ubuntu pass; qualification pass; release-gate pass; reuse pass; wasm pass; windows-engines pass. Native macOS fails as in issue 180: the job is cancelled before any step, with the runner configuration macos-14-arm64-us-default not supported; main run 43528776017 shows the same (macOS cancelled after one second, zero steps).

Review: independent review PASS at d5a297a, .stage/tool-generated-round-trip/reviews/d5a297a-review.md (the review file is in the 205 worktree stage record). make check exit 0 at d5a297a (local).

Closes #205
