# T2 review: strict forced tool choice

Reviewed: uncommitted working tree vs c168175 (Task 1). Files: core `recover.rs`, `resolve.rs`, `mod.rs`; host `tool_calls.rs`, `registry.rs`, `tools.rs`; CLI `commands.rs`; core goldens (6 new `forced_strict_*.json`) and tests; fuzz example, `scripts/fuzz.py`, `docs/FUZZING.md`; published fixture; INTEGRATION.md, tool-calls README, README, CLI README.

## Evidence
- `cargo test -q` for core `tool_call_goldens`+`tool_call_recover` (99 ok), host `tool_call_tools` (43 ok), CLI `tool_calls_cli` (21 ok), `--doc tool_call` (204 ok): pass.
- `python3 scripts/test-verify-published.py` OK; `python3 scripts/test-fuzz.py` OK (66 tests).
- `cargo clippy -q --workspace --all-targets -- -D warnings`: clean. `make hygiene`: exit 0.
- `git status`: no tracked file under `crates/core/tests/data` modified; only the 6 new untracked `forced_strict_*` goldens. Forced(String) arm, constraint, and checks are unchanged (`Forced(_) | ForcedStrict(_)` share the pre-existing block; recover.rs ~L4260).
- 5 new strict fuzz seeds run through the built `fuzz-tool-call` example: all exit 0.

## Contract rows (production entry points)
1. Core (`Recovery::strict`, recover.rs ~L4296): exactly-one marks calls after the first `Unresolved{ToolChoiceCardinality}` (code `tool_choice_cardinality`, resolve.rs) + diagnostic; no-content scans `value.content` with `u8::is_ascii_whitespace`; no-reasoning flags every `reasoning` range. Failure flows through existing path: goldens `forced_strict_two_calls`, `_call_plus_prose`, `_reasoning_rejected`, `_anthropic_disable_parallel` assert finish unresolved, verdict nothing_usable, empty emission, `failed_validations` holds the constraint (helper `assert_strict_failed`). `_single_call_passes` (newline framing) passes. U+00A0 negative (`forced_strict_non_ascii_whitespace_is_content`) plus U+2003 in `strict_forced_cardinality_content_and_reasoning`. Embedded call in `<think>` not counted (same test, `embedded.len()==1`, passes with exactly-one). Undeclared name still `invalid_input` for the strict form (resolve at L3315 matches both variants).
2. MCP: `tool_call_tools::tool_choice_strict_member(_errors)`; CLI: `strict_forced_tool_choice_flags`, `strict_forced_flags_need_a_tool_name`. Manually confirmed with the built binary: `--tool-choice read_file --tool-choice-no-content` with a trailing `<tool_response>` block exits 4, finish unresolved, nothing_usable (tool results are content).
3. constraint(): `forced[exactly-one,no-content,no-reasoning]:NAME`, name last, active options only; all-lenient gives `tool_choice=forced:NAME` (doc test + `strict_forced_constraint_strings`).
4. from_anthropic: `disable_parallel_tool_use` true -> ForcedStrict{exactly_one, content ok, reasoning ok}; false -> Forced; non-bool errors; on auto/any/none still an explicit error. Host: `tool_choice_strict` requires all 3 booleans, unknown members rejected, errors beside non-forced choice and when `exactly_one:false` contradicts the Anthropic flag. Registry schema (required + `additionalProperties:false`) matches the parser.
5. CLI: flags with `auto`/`none`/`required`/no choice -> exit 1 with message; repeated flag and repeated `--tool-choice` -> "unknown or repeated option"; order-independent. A tool literally named `required` cannot be forced from the CLI (pre-existing: the word maps to Required); the flag error message and docs say "a tool name, not auto, none or required", so the ambiguity is stated, not hidden.
6. Fixture diff: one line of `mcp-responses.jsonl` changed; verified by character diff it adds only the `disable_parallel_tool_use` clause in the `tool_choice` description and the `tool_choice_strict` member.
7. Docs: INTEGRATION.md new "Strict forced tool choice" section (options table, counting rules, wire forms, proof list) is accurate against the code; Rustdoc on `ToolChoice`, `StrictForced` (builder methods, whitespace rule, error semantics) has runnable examples (doctests pass).

## Fuzz deviation judgment
Seeds live only in `scripts/fuzz.py` (`TOOL_CALL_SEEDS`: conformance markup + witnesses + repairs + negatives, `TOOL_CALL_REGRESSIONS`, and the new `TOOL_CALL_STRICT`); no committed corpus files. Computed over all 403 seeds: the only seeds with forced header (`control>>6==3`) and a final byte >= 0xF8 are the 5 new strict seeds. Claim verified; earlier seeds keep their meaning (non-forced and non-trailer inputs parse as before: `output = bytes[2..end]`, end = len unless trailer).
Coverage cost: under forced choice, a mutated input ending in 0xF8..0xFF no longer exercises "invalid-UTF-8 tail byte at end of the turn" for that byte (the byte is consumed as a trailer; the turn then ends before it). 0xC0/0xC1/0xF0..0xF7 and interior 0xF8+ bytes, and the same tail under auto/none/required (3 of 4 choices), remain; only 8 of 256 byte values on 1 of 4 choices are affected, and the same bytes are still exercised mid-turn. Judged acceptable. It departs literally from "a separate later seed byte" only in that the trailer shares the input's end, which is the sole way to add a later byte without a format change; documented in the example header and FUZZING.md. Invariants (`check_strict`): count>1 under exactly_one implies not passed; passed implies exactly one call, no non-ASCII-whitespace content byte, no reasoning range; conditioned on drawn options, consistent with plan; run only on resolved outcomes (correct).

## Findings
1. ADVISORY. "Tool results count as content" is documented (INTEGRATION.md, recover.rs L1562) but no named test pins it; I only verified it by hand. Add one assertion (a Hermes `<tool_response>` after the call under no-content).
2. ADVISORY. A tool named `required`/`auto`/`none` cannot be strictly forced via CLI (inherited ambiguity); the message states it. Acceptable; no action required.
3. ADVISORY. Under exactly-one, the diagnostic count equals calls-1; a turn with a rejected attempt already fails finish, so no double counting issue found.

No REQUIRED findings.

VERDICT: PASS

## Orchestrator dispositions
- Advisory 1: addressed with the named test `forced_strict_tool_result_is_content`. A model-written `<tool_response>` after the single call gives exactly one diagnostic, a content diagnostic, with no rejected attempt; the turn is unresolved and nothing is emitted.
- Advisory 2: no action. This is pre-existing CLI behaviour, and the error and docs state it.
- Advisory 3: no action. It is the correct behaviour.
