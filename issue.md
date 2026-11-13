# Issue #185: Main's tool-call fuzz target fails: a found attempt end splits a dialect literal

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

On main (0870203, reproduced at 960e3db and 06f29ab) the tool-call fuzz target fails on oracle F7 at `crates/core/examples/fuzz_tool_call.rs:652`: a rejected span ends inside a dialect literal (`"in`, the anthropic early-input-member literal).

Cause (read from the code): `bracket_step` (`crates/core/src/tool_call/recover.rs`, around line 2228) closes an attempt with `Flow::Found(walk.json_end)`, and the `Found` path in `markup_end` does not apply `complete_literal`. `complete_literal`'s own documentation says a rejected span never splits such a literal, and `unclosed_end` already applies it. So the engine breaks its own invariant.

Acceptance: the saved failing input is pinned as a regression test (exact bytes, sha256 edffe988bd847d8f7cf563505d2a6bb06575eab938fcd2a993d23f6e3d039495 is the GLM input from a different issue; this input's bytes are at `.stage/169-fuzz-diagnosis` evidence and must be copied by the fixer from the worktree `.cache/fuzz-tool-call-499t0xfe/current.bin` of PR 169); a test at the literal boundary; the fuzz target passes `--timeout 120`; the rejected-span suites pass.


## Comments (0)

