# PR 169 duplicate charge removal

New head: fa4016f1a3ab42f0f59c0f6a8af2e33d259cedb6 (pushed with `git push origin HEAD`, exit 0). Previous head a9d12e3.

Verification by reading: `merge_live` (engine.rs ~6334) charged `size(live.len())` immediately before
`merge_noting(live, ...)`, whose first statement charges `size(threads.len()) * K_THREAD` (K_THREAD = 1,
cost.rs) over the same unmodified `live`. The ownership census runs inside that pass through the `note`
callback ("in the same charged pass"). Same thread set, same quantity: a double count. Removed only that charge.
The other `size(live.len())` charges (6385, 6505...) are separate passes and were left alone.

| Measurement | Before | After |
|---|---|---|
| 64-call busy turn, full pass (sized) work | 1,315,123 | 1,252,779 |
| Standard preset | stops BudgetExceeded(Work), usage work 1,000,000 | stops BudgetExceeded(Work), usage work 999,938 |
| Native Hermes, Exact, first Work stop | 32 | 33 |
| hermes-stripped first Work stop (extra check) | 25 | 25 |
| Busy candidates / evidence entries | 193 / 713 | 193 / 713 |

Identical before/after (hash of Debug output): busy outcome 3cd1dea97b7d93d3, evidence 641c1b2956f15ee2,
alternatives 7904dd65836c8ff4. Hermes probe n=1..31 and 33..100: outcome, evidence, alternatives, candidates identical;
only n=32 differs (stopped on Work before, Completed after), as intended. Logs and probe: .stage/169-dup/.

Moved fields: pinned digests in tool_call_recover.rs (four) changed only in usage.work (proved by substituting the
old work value into the new wires and reproducing the old digests: 31500->30509, 31497->30506, 8448->8374, 7120->7046).
Receipt fixture v3-tool-call-invalid-utf8.json: only usage.work 23005 -> 22367 (field-by-field diff).
No preset limit or assertion expectation changed. Prose updated: busy-turn work in a tool_call_cost.rs comment and
docs/RESOURCES.md, and native Hermes 32 -> 33 in docs/RESOURCES.md.

Exit codes: clippy `--workspace --all-targets --locked -- -D warnings` = 0; tests = 0
(receipt_wire 7, tool_call_cost 20, tool_call_goldens 123, tool_call_recover 59, all passed); commit hook = 0; push = 0.
(First test run before fixture update failed: receipt_wire and deterministic_pinned_digest, both usage.work only.)

Files changed: crates/core/src/grammar/engine.rs, crates/core/tests/tool_call_recover.rs,
crates/core/tests/tool_call_cost.rs (comment), crates/core/tests/data/receipts/v3-tool-call-invalid-utf8.json, docs/RESOURCES.md.
