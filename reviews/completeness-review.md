# Completeness review: issue #8 (commits 943fa04..248d07a)

Reviewer: an independent completeness pass. It did not change the source. Throwaway files lived only in `/tmp/cr8` and were deleted afterwards.
Binary: `cargo build -q -p purrassist-cli` built `/opt/.cargo/slots/89efd6bfa4d1c105/0/target/debug/purrassist`. The target dir came from `cargo metadata`.
Tool file `/tmp/cr8/tools.json` (OpenAI form) holds three declarations:
- `plan_outfits`, with the issue's exact `outfits` schema;
- `read_file(path: string, required)`;
- `plan(plan: {cast: string[], outfits: <issue array>})`.

## Per-requirement table

| # | Requirement (issue / plan contract) | Evidence | Status |
|---|---|---|---|
| 1.1 | Issue example: repeated nested `<parameter=outfits>` items at top level give exactly `{"outfits":[{Alice…},{Bob…}]}`, Resolved, one reading | CLI run E1 (exit 0, exact JSON, status/call resolved, no alternatives, rules nested-parameter x2, array-item x2, collect x1, nothing else). Golden `xml_array_nested_object_items` plus named test | MET |
| 1.2 | Same shape inside an object parameter | CLI run E2 (exit 0, `{"plan":{"cast":[..],"outfits":[{..},{..}]}}` resolved). Golden `xml_array_nested_object_items_in_object` | MET |
| 1.3 | Mix of a JSON-object item and a nested-element item | Golden `xml_array_nested_object_items_mixed` plus named test (passes in the focused run) | MET |
| 1.4 | Unclosed last item | CLI run E3 (exit 0, resolved, `parameter-closed-by-element` x1). Golden `xml_array_nested_object_item_unclosed` | MET |
| 1.5 | Negative: `Array(String)` keeps the nested tags verbatim | CLI run E4 (`{"tags":["<parameter=wearer>Alice</parameter>"]}` resolved, no nesting). Golden `xml_array_string_item_keeps_nested_tags` | MET |
| 1.6 | `Unmodeled` items nest (issue ask: "Object, or Unmodeled") | Golden `xml_array_unmodeled_items_nested`, plus `xml_array_recursive_ref_items_are_siblings` for the documented `$ref` limit | MET |
| 1.7 | Each repetition is one item, in order, combined with collect. Per-item pointers | E1 ordering. CLI run E5: a missing `wearer` in item 2 gives `missing_required` at `/outfits/1/wearer` (exit 4). Named span test `/outfits/1/wearer` = `Bob` | MET |
| 1.8 | Required members are enforced inside items | E5 (call Unresolved, finish unresolved, exit 4) | MET |
| 1.9 | Plan extras: unclosed-before-sibling, same-name-member | Goldens present and named-tested (T1 review, focused run 100/100) | MET |
| 2.1 | Strict forced: exactly one call. Golden for two calls | CLI run E6 (`--tool-choice-exactly-one`): exit 4, finish unresolved, nothing_usable, call 1 `tool_choice_cardinality`, emission `[]`. The plain `--tool-choice` control (E6b) exits 0. Golden `forced_strict_two_calls` | MET |
| 2.2 | Strict forced: no content. Golden for one call plus prose | CLI run E7 (`--tool-choice-no-content`): exit 4, diagnostic "content at byte 0". The plain control exits 0. Golden `forced_strict_call_plus_prose`. Whitespace-only content passes (E8, golden `forced_strict_single_call_passes`). U+00A0 negative named test | MET |
| 2.3 | Optionally no reasoning | CLI run E9 (`--tool-choice-no-reasoning` with `<think>`): exit 4, "reasoning at bytes 0..22". MCP run M3. Golden `forced_strict_reasoning_rejected` | MET |
| 2.4 | Recorded in the tool-choice validation like the existing checks | E6/E7 receipts: `{validator: purrassist.tool_call.v1, constraint: tool_choice=forced[exactly-one]:read_file, passed: false}`. Goldens' `failed_validations` | MET |
| 2.5 | Surfaces: CLI flags, MCP `tool_choice_strict`, Anthropic `disable_parallel_tool_use` | E10 (the flags without NAME, or with `auto`, exit 1 with a message). MCP M1 (Anthropic `disable_parallel_tool_use` fails two calls), M2 (strict beside `auto` is an `isError` request error), M3 | MET |
| 3.1 | Inner (marker-free) reasoning range next to the outer range | CLI runs E11-E15: `reasoning_blocks[i].text` slices are `plan it` (closed), `first thoughts\n` from 0 (separate-open), `a` and `b` for two adjacent blocks under one merged `reasoning` 0..32, `still going ` (closed at opener), `never closed` (unterminated). MCP M3 shows `text 7..8` | MET |
| 3.2 | Outer range unchanged, wire and schema updated | `reasoning` unchanged in E11-E15. `TOOL_CALLS_OUTPUT` requires `reasoning_blocks`. Published fixture recaptured | MET |
| OOS | "Already satisfied" list must not regress | A script diff of all modified goldens against 943fa04 shows 464 identical apart from the added `reasoning_text` and 2 pure reorders (tie order is the wire-JSON digest, explained in T3). There are 0 semantic diffs. Starts-open, length-vs-stop, dangling `<tool_call>`, one-stray-closer, `next_id_start` and the other goldens still pass (focused run: goldens 100, recover 50, rules 41) | MET |
| Doc | Rustdoc and examples for new public items | `ToolChoice::ForcedStrict` (enum doc example), `StrictForced` (struct example with a real recover, plus examples on every builder method), `ReasoningBlock` (example), `ToolCalls::reasoning_blocks` (example), `UnresolvedReason::ToolChoiceCardinality` (`code()` doctest). `ReasoningSegment::text` has a field doc but no example (see A1) | MET (advisory) |
| SPDX | Headers on new source files | No new source files. The new files are JSON goldens only, which are unheadered like the existing goldens. `make hygiene` passed per validation.md | MET |

## Executed commands and observations

All commands ran in `/tmp/cr8` with `B=<binary> recover-tool-calls --tools tools.json`.

| ID | Command / input | Observed (trimmed) | Expected | Exit |
|---|---|---|---|---|
| E1 | `$B a.txt` (the issue's exact plan_outfits block) | status resolved, finish calls, verdict run. `{"outfits":[{"wearer":"Alice","garments":["a red coat"]},{"wearer":"Bob","garments":["a grey scarf"]}]}`. Rules nested-parameter 2, array-item 2, collect 1. Alternatives `[]` | issue JSON, Resolved | 0 |
| E2 | `plan` with cast plus two nested `outfits` items | `{"plan":{"cast":["Alice","Bob"],"outfits":[{"wearer":"Alice"},{"wearer":"Bob","garments":["a grey scarf"]}]}}`, resolved | same shape in object | 0 |
| E3 | Bob's item unclosed, ends at `</function>` | `[{"wearer":"Alice"},{"wearer":"Bob"}]`, resolved, `parameter-closed-by-element` x1 | resolved, closed by element | 0 |
| E4 | `tags: string[]` holding `<parameter=wearer>Alice</parameter>` | `{"tags":["<parameter=wearer>Alice</parameter>"]}`, resolved | verbatim string | 0 |
| E5 | item 2 has only `garments` | call unresolved `missing_required` "`/outfits/1/wearer` is missing", finish unresolved | not Resolved, per-item pointer | 4 |
| E6 | two `read_file` calls, `--tool-choice read_file --tool-choice-exactly-one [--emit openai]` | constraint `tool_choice=forced[exactly-one]:read_file`, passed false, call 1 `tool_choice_cardinality`, finish unresolved, nothing_usable, `emitted.output: []` | fail, nothing emitted | 4 |
| E6b | the same without the strict flag | `tool_choice=forced:read_file` passed, run | old semantics kept | 0 |
| E7 | prose plus call, `--tool-choice-no-content` | `forced[no-content]` failed, "content at byte 0 is not allowed…", nothing_usable. The control without the flag exits 0 | fail | 4 |
| E8 | `\n` + call + `\n`, all three strict flags | `forced[exactly-one,no-content,no-reasoning]:read_file` passed, run | pass | 0 |
| E9 | `<think>plan it</think>` + call, `--tool-choice-no-reasoning` | `forced[no-reasoning]` failed, "reasoning at bytes 0..22…", nothing_usable | fail | 4 |
| E10 | `--tool-choice-exactly-one` with no choice, and with `--tool-choice auto --tool-choice-no-content` | "…need --tool-choice NAME (a tool name, not auto, none or required)" | usage error | 1 / 1 |
| E11 | `<think>plan it</think>` + call, `--reasoning separate` | reasoning `[0..22]`, block span 0..22, text 7..14 = `plan it`. Call resolved | marker-free text | 0 |
| E12 | `first thoughts\n</think>` + call, `--reasoning separate-open` | reasoning `[0..23]`, span 0..23, text 0..15 = `first thoughts\n` | starts at 0, ends at closer | 0 |
| E13 | `<think>a</think><think>b</think>` | reasoning `[0..32]` (merged). Blocks 0..16/7..8 `a` and 16..32/23..24 `b` | two blocks, one merged range | 0 |
| E14 | `<think>still going <tool_call>…` | block span 0..19, text 7..19 (ends at the call opener). The call is `rule_not_admitted (tool_call.reasoning.unclosed)` under Structural, which is base behaviour | text to opener | 4 |
| E15 | `<think>never closed` | span 0..19, text 7..19 (end of output) | to EOF | 0 |
| M1-M3 | `purrassist mcp --root /tmp/cr8/root` over stdio: `tools/call recover_tool_calls` | M1: Anthropic `{"type":"tool","name":"read_file","disable_parallel_tool_use":true}` with two calls gives `forced[exactly-one]` failed, unresolved. M2: `tool_choice:"auto"` + `tool_choice_strict` gives `isError` "tool_choice_strict requires a forced tool_choice". M3: forced + `{exactly_one:true, allow_content:true, allow_reasoning:false}` with `<think>x</think>` gives `forced[exactly-one,no-reasoning]` failed, `reasoning_blocks [{span 0..16, text 7..8}]` | as documented | n/a |
| T | `cargo test -q -p purrassist-core --test tool_call_goldens --test tool_call_recover --test tool_call_rules` | 100 / 50 / 41 passed | green | 0 |
| G | Python diff of every modified golden against `git show 943fa04:` (strip `reasoning_text`, order-insensitive compare) | 466 modified: 464 identical, 2 reorder-only, 0 semantic diffs | additive only | 0 |
| C | Base vs branch evaluation corpus: count `inline-closers-then-call` cases whose input has a value-final newline before the inline closer | base 34 cases / 0 affected. Branch 38 / 0 | the narrowing drops no case evaluated at base | 0 |

## Failure-shape checks

- **DARK.** None. Every new path runs through `ToolSet::recover`, the CLI and MCP, as shown in E1-E15 and M1-M3.
- **REFUSED.** None. All three asks are implemented.
- **PRODUCER-WITHOUT-CONSUMER.** None:
  - `ReasoningSegment.text` feeds `ToolCalls::reasoning_blocks` (recover.rs ~L3782), which feeds the wire form (~L2845), the MCP schema, the goldens' `reasoning_text`, the fuzz `check_reasoning_blocks` and `calls_key`.
  - `StrictForced` is consumed by the enforcement code, the constraint, `from_anthropic`, the CLI, MCP, the goldens and the fuzz.
- **TEST-ONLY.** None. The CLI and MCP surfaces were exercised directly.
- **WEAKENED TESTS.** None found. In the test files, the removed lines are only:
  - import reshuffles;
  - the golden harness's `from_openai` call, now generalized to `tool_choice()`, which is stricter: a stray `tool_choice_strict` without a choice asserts;
  - four `PINNED_DIGEST` re-pins, which T3 verified as wire-addition-only;
  - the `correction_evaluation` constants, 16438/11261/5177 to 18166/12621/5545, still summing to 0 incorrect.

  The corpus narrowings (`closer_in_value_mutations` and `inline_closers_pair`) skip only values that end in `\n`. By the dialect's framing-newline rule, such text does not spell its truth. Check C shows the base corpus had no such case, so no case evaluated at base was removed.
- **VACUOUS OUTPUT.** None. Arguments, block texts and diagnostics carry the expected values.
- **SILENT NARROWING.** None found. The docs state these limits explicitly:
  - nesting needs a known tool at parse time;
  - String items never nest;
  - the same-name-member and recursive-`$ref` readings are stated;
  - non-ASCII whitespace counts as content;
  - tool results count as content;
  - `disable_parallel_tool_use` is rejected outside `type: tool`.

  The issue's example option shape `Forced { name, exactly_one, allow_content }` is realized as the additive `ForcedStrict(StrictForced{..})`, which also has `allow_reasoning`. That is a superset, and the decision is recorded in the plan.

## Findings

REQUIRED: none.

ADVISORY:
- A1. `ReasoningSegment` is public (re-exported from `tool_call`) and gains the `text` field. The field doc is accurate, but neither the struct nor the field has an example; the struct had none before this work either. The examples on `ReasoningBlock` and `ToolCalls::reasoning_blocks` cover the user-facing path. Consider a short example if AGENTS' "meaningful examples" rule is applied per struct.
- A2. The `inline_closers_pair` narrowing `(!head.ends_with('\n') || head.ends_with(">\n"))` is a heuristic. Name both corpus narrowings in the PR body, as the T1 disposition already plans.
- A3. Per validation.md, `scripts/check-wasm.py` (native/WASI transcript comparison) was not run per task. It relies on the pending `make check`. Record the `make check` outcome in validation.md before the PR.
- A4. The bounded tool-call fuzz still fails on the pre-existing #6 rejected-span oracle. validation.md attributes it with a base replay. I did not re-run the fuzz, as instructed. Report it as a residual risk in the PR.
- A5. Under a failed strict check, the top-level `status` stays `resolved` while `finish` is `unresolved` and `verdict` is `nothing_usable`. This matches the pre-existing `Forced` behaviour (golden `forced_tool_choice_violated_beside_a_usable_call`) and the documented "status is reading certainty" contract. No action is needed, but integrators should be pointed at `verdict`.

VERDICT: PASS
