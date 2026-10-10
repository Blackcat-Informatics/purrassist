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
