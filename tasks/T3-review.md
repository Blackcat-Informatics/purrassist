# T3 review: marker-free reasoning ranges (`reasoning_blocks`)

## Scope
Uncommitted diff vs HEAD 28b2b3a: recognizer.rs, ir.rs, mod.rs, recover.rs, host registry.rs,
fuzz_tool_call.rs, tool_call_recover.rs, tool_call_goldens.rs, host tool_call_tools.rs, 481 goldens,
mcp-responses.jsonl, README/INTEGRATION/FUZZING docs.

## Evidence
1. Text computation (recognizer.rs). `Think.text_start` is set at the three Think construction
   sites: opener (`think_opened`, from `Pending::Marker.end`), rejected-attempt cut at an opening
   marker (L1273, L1495, via `opener_end(marker,pair)` = marker + opener literal length), and
   starts_open (0, L817). `Turn::reasoning` stores `text: text_start..marker.unwrap_or(end)`.
   Closed and the cut-attempt-closed case pass `Some(closer start)`; `promote` (closed-at-opener)
   and `unterminated` pass `None`, so text ends at the opener / end of output. Markers are literal
   byte strings, so opener_end equals the matched marker end. Segmentation code (spans, closure,
   embedded, costs, store order) is untouched; only a field is added. Harmony 30..40 of 0..47,
   `[THINK]x[/THINK]` 7..8, nested `<think><think>x</think>` 7..15 are asserted in tests.
2. `reasoning_blocks` is pushed one per `ResolvedSegment::Reasoning` (recover.rs ~L3781) next to
   `push_range` (merging) of `reasoning`; `reasoning` untouched. Wire member `reasoning_blocks`
   written after `reasoning`; `TOOL_CALLS_OUTPUT` lists it as required with a strict
   `{span,text}` item, reusing the `range` ref. Host test validates the schema, and checks that
   dropping the member or adding an extra key fails validation. Rustdoc examples pass as doctests.
3. Golden diff by script (recursive strip of `reasoning_text`, old from `git show HEAD:`): 481
   changed, 479 identical to HEAD apart from added `reasoning_text` (also inside alternatives
   `readings`); 44 files have non-empty text (spot-checked: mistral cut-at-marker gives "thinking",
   starts_open ones start at 0, unterminated runs to the end). The other 2
   (harmony_python_code_holds_call_closer, qwen_value_holds_call_closers_then_text) are pure
   reorderings of `readings`/`ledger_rules` with identical per-reading content plus the new field.
   Ordering code: `whole_derivations` (recover.rs ~L5073) identifies each reading by the digest of
   `reading.value.to_node().to_json()` (the wire form) when there are 2+ readings, and ranking is
   by cost then that digest. This is the documented contract (README L75, INTEGRATION L628,
   IMPLEMENTATION L117: "cost, then a content digest"). A wire addition therefore legitimately
   changes tie order; it is not a regression. Segmentation/readings content is unchanged.
4. Digest re-pins. Experiment in /tmp (deleted): copy of the tree with the `reasoning_blocks`
   wire member removed and the four OLD pins restored: `deterministic_*` tests pass. So the new
   pins differ only by the wire addition.
5. Published fixture: only `mcp-responses.jsonl` changed (2 lines). Parsed diff: outputSchema gains
   `reasoning_blocks` property and a required entry; the recover response gains
   `"reasoning_blocks":[]` (in the structured value and its stringified copy). `call-response.json`
   has no recover_tool_calls response and needs no change. `python3 scripts/test-verify-published.py`: OK (10 tests).
6. Fuzz `check_reasoning_blocks`: checks ordered/disjoint, text within span, span within a
   reasoning range, byte-cover equality of block spans and reasoning ranges, head equals an allowed
   dialect's opener (empty only at offset 0 under starts_open), tail empty or that pair's closer,
   AsContent yields no blocks. It is non-vacuous (covers equality, exact marker bytes) and does not
   fire on valid inputs; `python3 scripts/test-fuzz.py`: OK (66 tests); clippy all-targets compiles it.
   I did not run make fuzz as instructed.
7. Tests pass: tool_call_recover (50), tool_call_goldens (22 + others, all ok), host
   tool_call_tools (22), clippy `-D warnings` clean, `cargo fmt --check` clean, recover doctests (46) pass.
   Recover tests assert exact span, text and bytes (adjacent blocks, closed, starts_open,
   closed-at-opener, unterminated, Harmony, Mistral, nested opener, AsContent, empty starts_open) via
   `tools.recover`, the production entry; host test goes through `recover_tool_calls`.
8. Docs: README L119 row, INTEGRATION "Templates that open reasoning" paragraph, FUZZING.md item (R),
   `ReasoningSegment.text`, `ReasoningBlock` and `ToolCalls::reasoning_blocks` Rustdoc all match behaviour.

## Real callers/consumers
`ToolCalls` is built in `Recovery` (recover.rs) from `ResolvedSegment::Reasoning`; consumers: wire
`to_node`, MCP `recover_tool_calls` (host registry schema), goldens harness, fuzz probe, verify-published.
`ReasoningBlock` is re-exported from `tool_call/mod.rs`.

## Findings
1. ADVISORY: `calls_key` (recover.rs ~L4935, reading de-dup key) hashes only the merged `reasoning`
   ranges, not `reasoning_blocks`. Two readings with equal merged ranges but different block splits or
   text would be folded as one, keeping the first. Not observed in any golden or test; blocks do not
   change which calls are recovered, so likely acceptable. Consider documenting or adding blocks to the key.
2. ADVISORY: the `WireValue for ToolCalls` doc-comment line listing members is now over-long
   (cosmetic; rustfmt does not wrap comments).
3. ADVISORY: the plan said to regenerate `call-response.json`; it correctly did not change (no
   recover_tool_calls response in it). No action.

No REQUIRED findings.

VERDICT: PASS

## Orchestrator dispositions
- Advisory 1 (`calls_key` hashes only the merged `reasoning` ranges): addressed. `calls_key` now keys on each block's span and text, so readings that divide the same bytes into different blocks stay distinct. Goldens, recover and rules are unchanged (100/50/41), `make corpus` passes, `correction_evaluation` passes (10), and the pinned wire digests are unaffected (`calls_key` is not on the wire).
- Advisory 2: cosmetic; left as is.
- Advisory 3: no action; `call-response.json` correctly did not change.
