GBNF compiler for tool argument grammars (issue 223, second part of #36; partial, #36 stays open).

Hosted results at head 8ea43a2: corpus pass; fuzz pass; native ubuntu pass; qualification pass; release-gate pass; reuse pass; wasm pass; windows-engines pass. Native macOS fails as in issue 180: cancelled before any step, runner configuration macos-14-arm64-us-default not supported; main run 43528776017 shows the same.

Review: independent pass PASS at 8ea43a2, .stage/tool-gbnf-gate/reviews/8ea43a2-review.md, written before the merge. Local full make check at 8ea43a2: exit 0 (.stage/tool-gbnf-gate/make-check-8ea43a2.log).

Merge form: the full form with the Stage record (.stage/tool-gbnf-gate), because the PR has one.

Refs #36, #223
