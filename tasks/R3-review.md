# R3 review (G14 reversed reasoning text range, G11 docs)

## Scope
Uncommitted diff vs HEAD e93eaf8 plus the new golden
`anthropic_redacted_thinking_closer_overlaps_opener.json`. Spec: remediation-plan R3,
security-review R2, plan-review F12.

## Evidence
- Byte arithmetic: input is 41 bytes; opener `{"type": "redacted_thinking", "data": "` is 39
  bytes (0..39); the only `"}` is at 38..40; text starts at 39 and holds `}x`. So span 0..41,
  text 39..41 is correct. The plan's 0..40 / 39..40 was HEAD's own span (HEAD printed span 0..40,
  text 39..38) and is wrong. Implementer's expectation is right; the plan is adjudicated as in error.
- Unterminated is guess-flagged, so best_guess is correct (HEAD said resolved).
- Mutation (HEAD recognizer + new test file/golden): 3 failures:
  `anthropic_redacted_thinking_closer_overlaps_opener` (left 0..40 right 0..41),
  `tool_call_goldens_match_captured_runs` ("slice index starts at 39 but ends at 38"),
  `reasoning_block_ranges_slice_over_every_golden`
  (`ReasoningBlock { span: 0..40, text: 39..38 }`). HEAD CLI JSON: `"text":{"start":39,"end":38}`;
  new CLI: `"span":0..41,"text":39..41`, status best_guess, rule tool_call.reasoning.unterminated.
  HEAD fuzz probe on seed `\x00\x52...`: panic "reasoning block text range 39..38"; new: `ok`.
- Closing paths: think_closed (`start < text_start` rejects), cut_rejected closer filter
  (`start >= opener_end`), promote (`start < text_start` -> None), unterminated (end is output end,
  >= text_start), all funnel through `Turn::reasoning`, which has a `debug_assert!` on
  start <= text.start <= text.end <= end and no clamp.
- Goldens: `git status crates/core/tests/data/tool-call-goldens` shows only the one new untracked
  file; no existing golden changed. 112 goldens tests pass.
- Behaviour-change probes (27 inputs through built CLI, HEAD vs new, hermes `<think>` with
  hermes/qwen/mistral/deepseek calls, harmony analysis+commentary incl. adjacent channel,
  anthropic thinking/redacted_thinking/tool_use adjacent and nested, separate-open): all
  identical output (status, calls, reasoning_blocks, content, rejected). No legitimate call
  suppressed. Reasoning: reasoning openers and call openers begin with different bytes, so a call
  opener cannot start strictly inside a reasoning opener in any registered dialect; the `promote`
  guard is unreachable in practice, defensive only.
- All-goldens test iterates `all_goldens()` chained with `all_default_runs()` and every reading of
  Ambiguous alternatives; asserts checked > 0. Meaningful (it failed on HEAD).
- Fuzz seeds `\x00\x52` and `\x10\x52` (bits 4-5 = 1 = starts open) match the header conventions;
  last byte `x` is not a 0xF8.. trailer; existing seeds unchanged.
- Docs: redacted thinking described as opaque encrypted data in ir.rs, recover.rs, INTEGRATION.md
  (text 39..41 `}x` consistent with code). ReasoningSegment doctest present; doctests pass (51).
- Ran: tool_call_goldens + tool_call_recover (112 + 51 pass), --doc 376 pass, test-fuzz.py 66 OK,
  clippy -D warnings clean.

## Findings
REQUIRED
1. The golden's `description` field is stale: it says "span 0..40, text 39..40" and "the one
   byte after the opener". Actual captured values are span 0..41, text 39..41 (`}x`). Fix the
   description in
   `crates/core/tests/data/tool-call-goldens/anthropic_redacted_thinking_closer_overlaps_opener.json`.
   (Documentation accuracy; also the remediation plan text should note 0..41/39..41.)

ADVISORY
1. The `promote` guard is unreachable with the current dialects (no probe could exercise it); a
   unit test constructing it is not possible without a custom dialect, so it stays defensive.
2. The separate-open arm of the test asserts only ordering, not exact ranges; adequate.

VERDICT: BLOCKED

## Orchestrator disposition
- REQUIRED (stale golden description): fixed. The description now states span 0..41, text 39..41 (`}x`). The plan's R3 expectation is corrected with the same byte arithmetic. `tool_call_goldens` passes (112). The description is an input field, so no re-bless was needed.

## Recheck
- The golden is untracked, so `git diff` cannot show it; I compared against my earlier read instead.
  The description now says span 0..41, text 39..41 (`}x`), a 39-byte opener. The captured fields I
  re-read (input, outcome best_guess, reasoning_text `["}x"]`, ledger_rules
  `tool_call.reasoning.unterminated`) match what I saw before the edit.
- The remediation plan R3 now states 0..41 / 39..41 / best_guess / `["}x"]`.
- `tool_call_goldens`: 112 passed.
- REQUIRED item 1 is resolved. No remaining required findings.

VERDICT: PASS
