# Delta review: issue 198 (tool_set), merge head b6a50cb

Reviewer: independent read-only delta review (delivery-reviewer, Opus), covering git diff b92d5ca HEAD (4 files, +9/-9) and both merge parents.

VERDICT: PASS at merge head b6a50cb9f9a5d1d791c4b928a13798f91074003f.

1. The false unknown-argument claim (ReadOnly changes DropWithLoss) is gone from toolgraph docs and sources. The replacement (unknown-argument handling comes from the exact schema, not from effects) matches core adapter.rs:182-196 and metadata.rs:97-98, which override only effects and policy.
2. The merge brings no behaviour change: the branch-side delta is the REUSE sidecar for worked_manifest.json; the tool_set.rs change is comment-only; nothing under codec, loader or core changed.
3. Required items from the earlier BLOCKED review remain in place: the static-proof limits wording (tool_set.rs and TOOLS.md), the render-back test asserting a verbatim slice of the golden (tests/tool_set.rs), and the subtool-closure test (a_subtool_that_can_write_revokes_read_only).
4. Optional: long unwrapped lines in TOOLS.md:49 and TOOLS_PLAN.md:407 (cosmetic).

Not verified by this review: tests and prose-check were not run (read-only delta review).
