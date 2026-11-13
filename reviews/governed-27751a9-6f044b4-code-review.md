# Independent code review: issue 200, whole delta 27751a9..6f044b4

Reviewer: independent read-only review (delivery-reviewer, Opus), whole delta git diff 27751a9 6f044b4 (includes the merge of origin/main). No builds were run by the reviewer; it read the saved make check log for 6f044b4 (EXIT=0).

VERDICT: PASS at head 6f044b47a6b3e080dabf740541be1ebd2e9aa0cd.

(1) Storage: PASS. storage_stop (control.rs:144-161) asks the governor first; a recorded scratch exhaustion still reports Exhausted(ScratchBytes). Otherwise Parse with code storage (native cause) or storage-accounting (rejected release). No BudgetExceeded is guessed. Both call sites use it (control.rs:210-215, load.rs:775-777). Test: unrecorded_storage_failures_keep_their_cause_and_work_is_carried.
(2) Media type: PASS. only_turtle_and_trig_media_types_are_loaded loads TriG (same IRIs as Turtle) and asserts unsupported-format for application/n-triples and application/x-unknown. Both refusal sites use one helper.
(3) Conversion: PASS. NATIVE_UNITS_PER_WORK = 64 (control.rs:101). total = carried.saturating_add(units); carried = total % 64; charge total / 64 (control.rs:129-135). A zero-length charge still calls Governor::charge, which checks cancellation first (contract.rs:1442-1443); the native parser calls charge(0) (purrdf-rdf 4.0.2 parse.rs:674). A one-unit charge charges nothing until the carry reaches 64 (unit test). Within one Work instance the charged total is floor(sum/64); across the two instances (parse and FnO reader) the total is at most floor(total/64). Overflow: saturating_add cannot wrap; Governor::charge uses checked_add.
(4) Default budget: PASS. No file under crates/core changed; Budget::default() is Effort::Standard.budget() (1,000,000, 128). mcp_manifest.rs is identical to origin/main and uses Budget::default(). PR 199's tests/tool_set.rs (branch origin/paudley/198-tool-graph-toolgraph-tool-set-bridge-to) uses Budget::default() at line 62.
(5) Fallbacks and expect: PASS. Rejection uses unwrap_or_else(PoisonError::into_inner), which reads the value and swallows nothing. The one production expect (load.rs:123-124) has a real invariant (RetainedParse::dataset is None only on Err, which classify refuses). RefCell borrow in Work::charge is covered by a stated invariant. u64::try_from(len).unwrap_or(u64::MAX) fails closed into an input-bytes refusal.

Findings (none blocking):
1. control.rs:412 comment says a zero charge observes a stop; no assertion covers charge(0) on a stopped governor (behaviour correct by construction).
2. TOOLS.md:30 and graph.rs say "carrying the remainder" but do not state that the final sub-64 tail is never charged (at most 63 native units per Work instance). Stated in the code comment only.
3. graph.rs module-doc rates paragraph has an over-long line; prose-check was not run by the reviewer.
4. The admitted-size table is an empirical claim from the author's probes, not verifiable read-only.
