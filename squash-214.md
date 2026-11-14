Test each UnsupportedConstruct sub-kind of the tool graph loader (issue 211, follow-up to PR 195).

Hosted results at head cd41a70: corpus pass; fuzz pass; native ubuntu pass; qualification pass; release-gate pass; reuse pass; wasm pass; windows-engines pass. Native macOS is cancelled before any step: the runner configuration macos-14-arm64-us-default is not supported (issue 180); main run 43528776017 shows the same.

Review: independent review PASS at cd41a70, .stage/unsupported-subkinds/reviews/cd41a70-review.md, written before the merge. make check exit 0 at cd41a70 (local, read from the log file).

Refs #191, closes #211
