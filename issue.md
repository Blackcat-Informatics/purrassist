# Issue #8: Tool-call recovery: arrays of nested-parameter objects, strict forced cardinality, marker-free reasoning range

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

## Summary

A downstream integrator evaluated `tool_call.recover.v1` (merged in #5) against the failures it sees in production from Qwen3-family models. Almost every case resolves as intended. This issue tracks what is still missing.

## 1. Array of objects written as repeated nested `<parameter=>` elements (gap, functional)

Qwen3-Coder XML writes an object-typed argument as nested `<parameter=>` tags. It writes an array of objects by **repeating** the nested element, once per item:

```
<tool_call>
<function=plan_outfits>
<parameter=outfits>
  <parameter=wearer>Alice</parameter>
  <parameter=garments>["a red coat"]</parameter>
</parameter>
<parameter=outfits>
  <parameter=wearer>Bob</parameter>
  <parameter=garments>["a grey scarf"]</parameter>
</parameter>
</function>
</tool_call>
```

Schema: `outfits: {"type":"array","items":{"type":"object","properties":{"wearer":{"type":"string"},"garments":{"type":"array","items":{"type":"string"}}},"required":["wearer"]}}`

Expected (Resolved, single reading, since the declared array type makes the repetition unambiguous):

```json
{"outfits":[{"wearer":"Alice","garments":["a red coat"]},{"wearer":"Bob","garments":["a grey scarf"]}]}
```

The same shape also occurs **nested inside an object parameter**, e.g. `<parameter=plan><parameter=cast>[..]</parameter><parameter=outfits>…</parameter><parameter=outfits>…</parameter></parameter>`.

**Current behaviour (by code reading; not executed).**
- In `crates/core/src/grammar/codec/xml.rs` (~L929-938), nested members are read only when the parameter's declared shape is `ValueShape::Object` or `Unmodeled`.
- For `ValueShape::Array(item)` with an object `item` (~L1062-1081), a value that isn't a JSON array is treated as one item. That item goes to `value(Some(Object))`, which goes to `typed(..)`, which parses it as JSON. So `<parameter=wearer>…` faults as invalid JSON.
- `repeated_array_param_is_array.json` and `xml_array_item.json` cover only scalar items.

**Asks**
- When an array's item shape is `Object`, or `Unmodeled`, and the item body starts with `<parameter=`, read the members as a nested object, exactly as the object path does.
- Combine with the existing repeated-name collection, so each repetition contributes one item in order.
- Add goldens:
  - repeated nested-object items at top level;
  - the same inside an object parameter;
  - a mix of one JSON-object item and one nested-element item;
  - an unclosed last item (ends with its enclosing text, like `nested_parameter_object_unclosed`);
  - a negative case where the item shape is `String`, so the nested tags stay a verbatim string, or are rejected, per the existing string rules.

## 2. Forced tool choice with exact cardinality (enhancement, small)

`ToolChoice::Forced(name)` means "at least one call, and only to that tool". Some integrators need the stricter contract that OpenAI-style forced choice is often taken to mean: **exactly one call to `name`, and no content outside it**. Optionally, no reasoning either.

**Ask:** a variant or option, e.g. `Forced { name, exactly_one: bool, allow_content: bool }`, recorded in the `tool_call.tool-choice` validation like the existing checks. Add goldens for two calls, and for one call plus prose, under the strict form.

## 3. Reasoning range without markers (ergonomics, small)

`ToolCalls` reasoning ranges include the `<think>` / `</think>` markers, so every caller strips them. This holds even under `ReasoningPolicy::Separate { starts_open: true }`, where there is no opener.

**Ask:** expose the inner range, the reasoning text without markers, next to the outer range.

## Out of scope / already satisfied

The integrator confirmed these against goldens:
- reasoning that starts open;
- adapter tools defaulting to `Mutating`, with a request override;
- closers missing at EOF under `Stop` are repaired only when the call is complete;
- ids chained through `next_id_start`;
- losses cap a call at `BestGuess`;
- `ToolChoice::from_openai`;
- the unclosed-JSON-value-then-next-parameter case;
- a JSON body with one stray closer;
- a dangling trailing `<tool_call>`;
- length-versus-stop truncation.

Streaming, the remaining dialects and derive remain tracked in #4.


## Comments (5)

### paudley — 2026-10-09T07:08:33Z

# Issue #8 — Stage 1 plan: nested-parameter array items, strict forced choice, marker-free reasoning ranges

## Context

A downstream integrator ran `tool_call.recover.v1` (from #5) against real Qwen3 failures. Three gaps remain:

1. **Functional gap.** Qwen3-Coder XML writes an array of objects by repeating a nested `<parameter=NAME>…<parameter=member>…</parameter>…</parameter>` element, once per item. Today `crates/core/src/grammar/codec/xml.rs` pushes a nested frame only for `ValueShape::Object` and `Unmodeled` (`Reader::parameter`, ~L934-954). `Array(Object)` falls through to `value()` → `typed()`. That route parses JSON, so `<parameter=wearer>Alice` becomes an `InvalidJson` stand-in, and the member tags leak out as stray top-level parameters.
2. **Strict forced choice.** `ToolChoice::Forced(name)` means "at least one call, and only to `name`". Integrators also need "exactly one call, no prose outside it", and optionally no reasoning either.
3. **Reasoning ranges without markers.** `ToolCalls.reasoning` includes `<think>`/`</think>`, even under `starts_open`, so every caller strips the markers by hand.

## Stage 1 workflow (after plan approval)

- `stagectl open --issue 8 --json` → `$WORKTREE`, `$BRANCH`, `$STAGE`.
- `brief`.
- Run issue-analyzer and prior-art agents in parallel.
- Write `$STAGE/plan.md` from this file.
- Run the `gate prior-art` and `gate plan` gates.
- One `plan-reviewer` (fable/high) with the completion-adversary rubric.
- Post the plan to #8.
- Then for each task: implement (sonnet) → focused tests → task review `tasks/TN-review.md` → commit → push → issue comment.
- `make check` runs once, before the PR, and its result goes in `validation.md`.

Prior art: #3/#5 established the nested-object frame model, the repeated-name `collect` rule, and the tool-choice validation. No earlier attempt at any of these three asks is known. Recurrence of the array-of-objects shape in the wild is UNKNOWN beyond the integrator's report.

## Completeness contract

| Requirement | Task | Executable acceptance |
|---|---|---|
| Repeated nested-element items at the top level → `[{Alice…},{Bob…}]`, Resolved | 1 | golden `xml_array_nested_object_items` via `recover_tool_calls`, plus a named assertion in `tool_call_goldens.rs` |
| Same shape inside an object parameter | 1 | golden `xml_array_nested_object_items_in_object` |
| Mix of a JSON-object item and a nested-element item | 1 | golden `xml_array_nested_object_items_mixed` |
| Unclosed last item, ended by `</function>` | 1 | golden `xml_array_nested_object_item_unclosed` (`parameter-closed-by-element`) |
| `Array(String)` item: nested tags stay a verbatim string | 1 | golden `xml_array_string_item_keeps_nested_tags` |
| `Unmodeled` item shape nests too | 1 | golden `xml_array_unmodeled_items_nested` |
| An unclosed item followed by a sibling item stays two items | 1 | golden `xml_array_nested_object_item_unclosed_before_sibling` (`missing-parameter-close`, two items in order) |
| Each item's evidence points at its own occurrence (no `/outfits/0` aliasing across repeats) | 1 | named test in `tool_call_goldens.rs`: the leaf for `/outfits/1/wearer` spans the source bytes `Bob`, and `/outfits/0/wearer` spans `Alice` |
| Item type with a member named like the array, item unclosed: innermost-scope rule | 1 | golden `xml_array_nested_object_same_name_member`: schema (inline, non-recursive, in `tools_openai`): `children: {type: array, items: {type: object, properties: {name: {type: string}, children: {type: array, items: {type: object, properties: {name: {type: string}}, required: [name]}}}, required: [name]}}`. There is no `$ref`, so both item scopes are modeled `Object` shapes and the open item owns the inner `children` opener. A recursive `$ref` item is `Unmodeled` and declares nothing, so it takes the sibling reading with `missing-parameter-close`; the dialect doc states both cases, and no golden for the `$ref` form is required. Input `<parameter=children><parameter=name>a</parameter><parameter=children><parameter=name>b</parameter></parameter></parameter>` (first item unclosed before the inner opener). Expected and asserted by name: one top-level item `{name:"a", children:[{name:"b"}]}`, `Resolved`, rules `nested-parameter`×2, `array-item`×2, `collect`×0, no `missing-parameter-close`. The limit (a sibling item intended by the model is read as a child when the item is unclosed and the item type declares the array's own name) is stated in `qwen3-coder-xml.v1.md` and the codec docs. The expected value is written before capture. |
| Required members are enforced inside nested items | 1 | named assertion: an item lacking `wearer` makes the call `Unresolved` (missing required / schema validation), never `Resolved` |
| Already-satisfied behaviours do not regress | 1-3 | the whole existing golden set and `tool_call_recover`/`tool_call_rules` suites pass unchanged except the documented additive fields |
| Strict forced: exactly one call | 2 | golden `forced_strict_two_calls` → `finish: unresolved`, `failed_validations` holds the strict constraint |
| Strict forced: no content | 2 | golden `forced_strict_call_plus_prose`; whitespace-only content passes (golden `forced_strict_single_call_passes`) |
| Strict forced: optionally no reasoning | 2 | golden `forced_strict_reasoning_rejected` |
| Recorded in the `tool_call.tool-choice` validation | 2 | the constraint string appears in the receipt validation and in goldens' `failed_validations` |
| Inner (marker-free) reasoning range next to the outer range | 3 | `tool_call_recover.rs` tests for two adjacent blocks, a closed block, `starts_open` (inner `0..closer_start`), closed-at-opener and unterminated, each asserting `reasoning_blocks[i].text` bytes exactly; wire `reasoning_blocks`; MCP output schema; golden captured `reasoning_text`; fuzz invariants |

## Task 1: Arrays of nested-parameter objects (Qwen3-Coder XML)

**Codec (`crates/core/src/grammar/codec/xml.rs`)**

- In `Reader::parameter`, extend the nesting match. Treat `Array(item)` as nesting when `item` is `Object(inner)` (frame scope `Some(inner)`) or `Unmodeled` (`None`), and the body's first non-space bytes are `<parameter=`.
- Add `item: bool` to `Frame`.
- In `pop()` (the single attach point), handle item frames this way:
  - record `EditOperator::ArrayItem` over the item's span;
  - adopt `wrap_item(tree)`, so the item is re-rooted under `/0`.
- Collection needs no change. The existing duplicate-member collection plus `resolve.rs::collect` (`tool_call.parameter.collect`) turns repeated occurrences into one array in source order, so a JSON-object item and a nested item mix naturally.
- `owner`, `boundary` and the `cheapest` pop pricing work unchanged. Item members are declared only in the item frame's scope, so `Array(String)` keeps its verbatim-string rule.
- Rules recorded are only `nested-parameter`, `array-item` and `collect`, all non-guess readings, so the call stays **Resolved** as the issue expects.
- Update the module docs at L36-46 and L94-103 to describe the item case.

**Tests and docs**

- Add the eight goldens in `crates/core/tests/data/tool-call-goldens/`. Capture them with `PURRASSIST_BLESS=1`, then hand-check every captured field against the issue's expected JSON.
- Add named assertions in `crates/core/tests/tool_call_goldens.rs`. Add a unit test next to `repeated_array_param_is_array` in `tool_call_rules.rs`.
- Add a corpus mutant "array-of-object parameters as repeated nested elements" in `crates/core/tests/tool_call_corpus/mod.rs`. This puts the shape under the zero-incorrect-answer evaluation.
- Update the `docs/MATCHING-QUALITY.md` tallies that `correction_evaluation.rs` asserts.
- Update the `nested-parameter`/`array-item` rows in `docs/tool-calls/RULES.md` and the edit-wording text in `recognizer.rs:2323`. Update `docs/tool-calls/dialects/qwen3-coder-xml.v1.md`.

- **Per-occurrence pointers.** Fix the hazard where every wrapped item is rooted at `/NAME/0` before `collect` renumbers it. Each item's leaves and unresolved pointers must end at `/NAME/i`. Read `resolve.rs::collect` and fix the re-rooting there if needed.
- **Docs caveat.** When the tool is unknown at parse time, no scope is declared, so nesting does not occur. The docs state this; they do not claim nested arrays then.

**Focused check:** `cargo test -p purrassist-core --test tool_call_goldens --test tool_call_rules --test correction_evaluation`, plus `make corpus`.

## Task 2: Strict forced tool choice

**API (`crates/core/src/tool_call/recover.rs`)**

- Keep `Forced(String)`, which stays non-breaking with unchanged semantics.
- Add `ToolChoice::ForcedStrict(StrictForced)`, where `StrictForced { name: String, exactly_one: bool, allow_content: bool, allow_reasoning: bool }`. It provides the builder `StrictForced::exactly_one(name)`, which defaults to exactly one call, with no content and reasoning allowed.
- `constraint()` encodes the active options with the tool name last, so names holding `:`, `;` or `,` stay unambiguous. Example: `tool_choice=forced[exactly-one,no-content,no-reasoning]:read`.
- **What counts.** "One call" counts entries in `ToolCalls.calls` (recognized calls of any outcome, as `Required` does). Rejected attempts already fail the turn's finish. Embedded calls inside reasoning are governed by `allow_reasoning`, not by cardinality, which matches the existing rule that embedded calls are never tool-choice-checked. "Content" means any `content` range holding a byte that is not ASCII whitespace (`u8::is_ascii_whitespace`: space, `\t`, `\n`, `\r`, `\x0C`). Non-ASCII whitespace is content. The golden `forced_strict_single_call_passes` uses `\n`; a second named assertion checks that U+00A0 between the call and the end of output fails `no-content`. The rule is stated in the `StrictForced` Rustdoc and INTEGRATION.md. Tool results the model wrote are content, so they violate `no-content`.
- **Degenerate options.** A `ForcedStrict` with all three options lenient is semantically `Forced`; its constraint string is exactly `tool_choice=forced:NAME`.
- Add Rustdoc with examples and error semantics.

**Enforcement (`fn tool_choice`, ~L4050)**

- Run the `Forced` checks first: at least one call, and every call is to `name`.
- If `exactly_one` is set, every call after the first gets `Unresolved{ToolChoiceCardinality}` (a new `UnresolvedReason`, code `tool_choice_cardinality`) and a diagnostic.
- If `!allow_content`, any non-whitespace `content` range is a diagnostic. Inter-call framing whitespace passes.
- If `!allow_reasoning`, any reasoning block is a diagnostic.
- A failed check flows through the existing whole-turn contract: `finish` becomes `unresolved`, `verdict` becomes `nothing_usable`, and the emission is empty.
- The undeclared-name request error (L3126) also covers the strict variant.

**Wire and surfaces**

- `ToolChoice::from_anthropic` accepts `{"type":"tool","name":N,"disable_parallel_tool_use":true}` and maps it to strict exactly-one, with content and reasoning allowed. `disable_parallel_tool_use` stays rejected on other types, with an explicit error, because at-most-one-for-auto is not in scope.
- Host MCP (`crates/host/src/tools/tool_calls.rs`, input schema in `registry.rs:176`) gets one member `tool_choice_strict: {exactly_one, allow_content, allow_reasoning}` (all booleans, all required in the object), the same shape as the golden key. It is a request error when the choice is not forced, or when it contradicts the Anthropic `disable_parallel_tool_use` flag (`exactly_one: false` beside that flag).
- CLI (`crates/cli/src/commands.rs`) gets the matching (these flags are a usage error without `--tool-choice NAME`) `--tool-choice-exactly-one`, `--tool-choice-no-content` and `--tool-choice-no-reasoning` flags, with help text.
- The golden runner `request()` gets the input key `tool_choice_strict` with the same object shape.
- The fuzzer (`examples/fuzz_tool_call.rs:192`) generates the strict variant. The choice bits in the seed byte are full, so the strict options come from a separate later seed byte. Existing regression seeds keep their meaning. Its invariants are conditioned on the drawn options: a passed strict check implies exactly one call when `exactly_one` is set, no non-ASCII-whitespace content byte when `allow_content` is false, and no reasoning range when `allow_reasoning` is false.

- Regenerate `scripts/data/verify-published/{mcp-responses.jsonl,call-response.json}` with `python3 scripts/verify-published.py --capture` and review the diff: it adds only the new input-schema member to `tools/list`.

**Tests and docs**

- Add goldens for two calls, one call plus prose, reasoning rejected, the single-call pass, and the Anthropic `disable_parallel_tool_use` form.
- Add host and CLI tests.
- Update the `ToolChoice` docs in `docs/tool-calls/INTEGRATION.md` (options, CLI, and the contract-failure paragraph), `docs/tool-calls/README.md`, `README.md` and `crates/cli/README.md`.

**Focused check:** `cargo test -p purrassist-core --test tool_call_goldens --test tool_call_recover`, `-p purrassist-host --test tool_call_tools`, `-p purrassist-cli --test tool_calls_cli`, and `python3 scripts/test-verify-published.py`.

## Task 3: Marker-free reasoning ranges

- Record the opener end (`Pending::Marker.end`; 0 under `starts_open`) and the closer start (`think_closed`'s `start`) on `Think`/`ReasoningSegment` (`recognizer.rs` `think_opened`, `think_closed`, `promote`, `unterminated`; `ir.rs:492`). `ReasoningSegment` gains a `text: Range<usize>` field.
- `ToolCalls` gains `reasoning_blocks: Vec<ReasoningBlock>`, where `ReasoningBlock { span: Range<usize>, text: Range<usize> }` has one entry per block, in source order, never merged. `span` includes the markers and `text` excludes them. `reasoning` (merged outer ranges) is unchanged. Merging matters: `<think>a</think><think>b</think>` gives one `reasoning` range `0..32` but two blocks with texts `7..8` and `23..24`. The `text` doc says: bytes verbatim, not unescaped (Anthropic thinking text stays JSON-escaped).
- The text range ends as follows:
  - a closed block ends at the closer's start;
  - a block closed at a call opener ends at the opener (equal to the span end);
  - an unterminated block ends at end of output.
- Wire: add `"reasoning_blocks": [{"span":{start,end},"text":{start,end}}]` in `WireValue for ToolCalls` (~L2598) and to `TOOL_CALLS_OUTPUT` in `registry.rs:44`. That schema uses `additionalProperties:false`, so the field is listed as required, with the `range` `$ref` reused.
- Re-pin `PINNED_DIGEST` (`tool_call_recover.rs` ~L732), justified by the wire addition only. The WASM transcript (`crates/wasm-harness/src/transcript.rs`) pins only the emitted `tool_calls` arrays, so no WASM re-pin is expected; its test is run to confirm.
- Regenerate `scripts/data/verify-published/{mcp-responses.jsonl,call-response.json}` with `python3 scripts/verify-published.py --capture`. Review the diff: it adds only `reasoning_blocks` to the `recover_tool_calls` response and the output schema.
- Goldens gain a captured `reasoning_text` field: one entry per block as text (bytes when not UTF-8), mirroring `content`. It is added to `CAPTURED`; the input key `reasoning` (the policy) is untouched. Bless once and check that the diff adds only that field.
- Fuzz invariants (`examples/fuzz_tool_call.rs`):
  - blocks are ordered and disjoint;
  - each `text` ⊆ its `span`;
  - each `span` lies within one `reasoning` range;
  - the union of the block spans equals the union of the `reasoning` ranges;
  - `text` holds neither the opener nor the closer at its edges.
  Block counting is direct: `reasoning_blocks` is unmerged.
- Tests cover two adjacent blocks, a closed `<think>`, `starts_open`, closed-at-opener, unterminated, and Harmony analysis-channel markers.
- Update docs: `docs/tool-calls/README.md:119`, the INTEGRATION.md "Templates that open reasoning" section, and `docs/FUZZING.md`.

**Focused check:** the `tool_call_recover`, `tool_call_goldens`, `tool_call_tools` and wasm-harness transcript tests, `python3 scripts/test-verify-published.py`, and one bounded fuzz run compared to the baseline signature.

## Cross-cutting

- **Fuzz baseline.** #6 records `make fuzz` failing at the #3 merge. Before Task 3's bounded fuzz run, run the same bounded fuzz on the base `943fa04` and record it in `validation.md`. A failure present on the base is attributed to #6, not to this work, and is reported, not hidden.
- **Public API break.** `ToolChoice` gains a variant, `ToolCalls` gains a field, and `ReasoningSegment` gains a field. None of the three types is `#[non_exhaustive]`. The crate is unreleased (0.1.0 qualification in #1), so the breaks are accepted and all three are named in the PR body.
- **Truth changes.** Corpus truths change only with a cited issue clause (F15). The new mutant cites #8 §1.

## Task 4: PR and handoff

1. Run `make check` once. Record the result in `validation.md`.
2. Run the completeness review of the implementation against this contract.
3. Run the artifact inventory (haiku).
4. Open the PR with `stagectl pr-create`. The body says `Closes #8` and lists the validation and residual risks. Post the plan on the PR.
5. Post a "least confident about" note to both the issue and the PR.
6. Report which Stage 2/3 gates have not run.

## Declined / decided

- **Keep `Forced(String)` and add a separate strict variant**, instead of turning `Forced` into a struct. This keeps existing call sites and goldens, and keeps the OpenAI form untouched. The cost is two variants with overlapping meaning, so the docs must state the relation.
- **No at-most-one for `auto`/`required`.** It was not asked for, and it would turn `disable_parallel_tool_use` into a general cardinality feature. It stays an explicit error.
- **Additive `reasoning_blocks` instead of replacing `reasoning`** (adopted from plan review advisory 5). Replacing `reasoning` would break its wire shape and the MCP `range` `$ref`; adding a field breaks neither. The cost is two overlapping fields; `reasoning` stays the merged view.


### paudley — 2026-10-09T07:31:26Z

**Task 1 done — arrays of objects as repeated nested `<parameter=>` elements** (commit c168175, pushed)

- **Codec:** an `Array(Object|Unmodeled)` value that starts with `<parameter=` now opens an item frame. Closing the frame records `xml.array-item` and wraps the object as one item; `parameter.collect` assembles the items in source order.
- **The issue's `plan_outfits` example** now resolves to `{"outfits":[{"wearer":"Alice","garments":["a red coat"]},{"wearer":"Bob","garments":["a grey scarf"]}]}` as `Resolved`. Before the change, the call ended `Unresolved` with the items read as strings.
- **Fixed a pointer bug:** every repeated occurrence was rooted at `/NAME/0`, so an error in item 2 was reported as `/xs/0/...`. Pointers are now numbered across occurrences.
- **Nine goldens:** top level, inside an object, a JSON/nested mix, an unclosed last item, an unclosed item before its sibling, string items that keep the tags verbatim, `$ref` items, and the same-name-member limit for both inline and recursive-`$ref` schemas. Each has a named assertion.
- **Corpus:** a new array-of-objects tool and mutant. Totals are 18166 cases, 0 incorrect, abstention recall 100%. Two existing inline-closer mutants now skip values that end in a newline: their recorded truth was wrong because the framing newline is consumed.
- **Validation:** `tool_call_goldens` (93), `tool_call_rules` (41), `correction_evaluation` (10), `tool_call_roundtrip` (15) and `tool_call_recover` (41) pass, as do `make corpus` and clippy with `-D warnings`. Independent task review: PASS.


### paudley — 2026-10-09T07:45:10Z

**Task 2 done: strict forced tool choice** (commit 28b2b3a, pushed)

- **New choice:** `ToolChoice::ForcedStrict(StrictForced { name, exactly_one, allow_content, allow_reasoning })`. `StrictForced::exactly_one(name)` sets exactly one call, no content, and reasoning allowed. `Forced(String)` is unchanged.
- **Options:**
  - Exactly-one: each call after the first is `Unresolved` with `tool_choice_cardinality`.
  - No-content: any content byte that is not ASCII whitespace fails the check. U+00A0 counts as content, and so do tool results the model wrote.
  - No-reasoning: any reasoning block fails the check.
- **When the check fails,** the existing contract failure applies to the whole turn: `finish: unresolved`, `verdict: nothing_usable`, and nothing is emitted.
- **Constraint string:** `tool_choice=forced[exactly-one,no-content,no-reasoning]:NAME`. The name comes last, so names containing `:` or `;` stay unambiguous. If no options are set, it renders as `forced:NAME`.
- **Surfaces:**
  - Anthropic `{"type":"tool","name":N,"disable_parallel_tool_use":true}` maps to exactly-one.
  - MCP takes `tool_choice_strict: {exactly_one, allow_content, allow_reasoning}`.
  - The CLI takes `--tool-choice-exactly-one`, `--tool-choice-no-content` and `--tool-choice-no-reasoning`.
  - Each of these is a request error unless the choice is forced.
- **Goldens:** two calls, one call plus prose, a single call with only whitespace (passes), reasoning rejected, and the Anthropic form with two calls and with prose (passes). Named tests cover U+00A0 and a model-written `<tool_response>`.
- **Validation:**
  - Core goldens/recover/rules pass (100/43/41), as do the doctests (204), host (21) and CLI (24).
  - `test-verify-published` passes; the MCP transcript was recaptured for the new input member.
  - `test-fuzz.py`, `make hygiene` and clippy with `-D warnings` pass.
  - Independent task review: PASS.


### paudley — 2026-10-09T08:04:18Z

**Task 3 done — marker-free reasoning ranges** (commit 248d07a, pushed)

- **New field.** `ToolCalls.reasoning_blocks: Vec<ReasoningBlock { span, text }>`. It holds one entry per block, in source order, never merged.
  - `span` includes the markers; `text` excludes them.
  - Under `starts_open`, `text` starts at byte 0.
  - `text` ends at the closer, at the call opener that closed the block, or at the end of output.
  - `reasoning` is unchanged. It still merges adjacent ranges, which is why an index-aligned list could not work: `<think>a</think><think>b</think>` is one `reasoning` range `0..32` but two blocks, with texts `7..8` and `23..24`.
- **Wire.** `reasoning_blocks` is added to the wire value and to the MCP output schema. Goldens capture a new `reasoning_text` field. Readings are now deduplicated on block identity.
- **Validation.**
  - Exact-range tests pass for adjacent blocks, a closed block, `starts_open`, closed-at-opener, unterminated, Harmony, Mistral and a nested opener.
  - The core, host and CLI suites pass, as do `make corpus`, `make hygiene` and `test-verify-published`.
  - The four pinned digests were re-pinned. Restoring the old pins passes once `reasoning_blocks` is removed, so the re-pin is caused by the wire addition alone.
  - Independent task review: PASS.
- **Fuzz.** The bounded tool-call fuzz still fails, on the rejected-span oracle. Replaying the exact retained input through a base `943fa04` probe gives the identical panic, so this is the pre-existing #6 failure. The new invariants did not fire.


### paudley — 2026-10-09T08:09:58Z

**Handoff: what I'm least confident about, and what you should know**

**Least confident**
1. **Same-name-member and recursive-`$ref` readings both come out `Resolved`.**
   - Suppose an item is left unclosed and the item type declares the array's own name. An inline schema then reads the next opener as a *child*, and a recursive `$ref` schema reads it as a *sibling*. Both readings are `Resolved`.
   - The model's intent is genuinely ambiguous in that shape. The cost model ranks the reading; it does not abstain.
   - Goldens pin the behaviour and the dialect doc states it. If you want abstention there, that is a policy change to decide.
2. **The native/WASM comparison is unverified for this branch.**
   - `make wasm-test` refused the local Node (v26.11.1 against the v26.10.0 pin) before comparing anything.
   - The wire value gained `reasoning_blocks`. The native `portable-core` pins pass, and they cover only the emitted `tool_calls` arrays, so I expect no drift. Hosted CI or a pinned Node must confirm it.
3. **Tool-call fuzz is still red, on #6.**
   - The failure was attributed by replaying the exact retained input through a probe built from base `943fa04`, which gave the identical panic.
   - Only one bounded campaign ran. While #6 stays open, a new failure in this branch's code could hide behind the old one.

**What you may not know**
- **Re-pinned digests.** The four pinned result digests and the published MCP transcript were re-pinned for the additive wire field, and the change was checked by stripping the field. Downstream consumers that hash the full result will see new digests.
- **Golden reordering.** Two `Ambiguous` goldens reorder tied readings, because the tie-break is the wire-JSON digest.
- **Pointer bug fixed.** Every repeated parameter occurrence used to point at `/NAME/0`; pointers are now numbered across occurrences. Evidence pointers for repeated scalar array items also changed (`/tags/0`, `/tags/1`).
- **Strict choice at the top level.** A failed strict check leaves the top-level `status: resolved`, with `verdict: nothing_usable`, the same as plain `Forced`. Integrators must branch on `verdict`.
- **CLI naming limit.** A tool literally named `auto`, `none` or `required` cannot be strictly forced from the CLI. This was already true of `--tool-choice`; use MCP or the Rust API instead.


