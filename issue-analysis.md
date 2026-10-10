# Issue Analysis: #8

**Title:** Tool-call recovery: arrays of nested-parameter objects, strict forced cardinality, marker-free reasoning range
**State:** OPEN  **Repo:** Blackcat-Informatics/purrassist  **Comments:** 0  **ADRs:** none cited
**Base:** origin/main 943fa04 (#5 merged)  **Worktree:** /home/paudley/Active/purrassist/.worktrees/8-tool-call-recovery-arrays-of-nested
**Sources:** stage `issue.md`, `brief.json`, `prior-art.md`. Code was read at the base. Baselines were executed at the base with a scratch probe outside the repo (`/tmp/i8probe`, `ToolSet::recover`).

## Summary
A downstream integrator evaluated `tool_call.recover.v1` against Qwen3-family production failures. The issue lists three gaps:
1. **Functional.** Qwen3-Coder XML writes arrays of objects as repeated nested `<parameter=>` elements, including inside an object parameter. These must resolve to a typed array of objects.
2. **Enhancement.** Add a strict forced tool choice: exactly one call to `name`, no content and, optionally, no reasoning. It is enforced in the `tool_call.tool-choice` validation.
3. **Ergonomics.** Expose the marker-free (inner) reasoning range next to the existing outer range.

The issue also lists ten behaviours that are already satisfied. None of them may regress.

## Production entry points (used by every check below)
- **Core:** `purrassist_core::tool_call::ToolSet::recover(output, &ToolRecoveryRequest)`, which uses `recover_governed`. Golden harness: `crates/core/tests/tool_call_goldens.rs`. Fixtures: `crates/core/tests/data/tool-call-goldens/*.json`. The harness's `request()` reads `tool_choice` only through `ToolChoice::from_openai`. It reads `reasoning` as `separate`, `starts_open` or `as_content`. It captures `content` text but **not** `reasoning`.
- **CLI:** `purrassist recover-tool-calls` (`crates/cli/src/commands.rs`). Exit 0 means usable and the choice passed. Exit 4 means the choice failed or nothing is usable. Today `--tool-choice` accepts only `auto|none|required|NAME`.
- **MCP:** tool `recover_tool_calls` (`crates/host/src/tools/tool_calls.rs`). The input schema is at `crates/host/src/tools/registry.rs:176`. The output schema `TOOL_CALLS_OUTPUT` (registry.rs:44) uses `additionalProperties:false` on `tool_calls`, so any new member must be added there.
- **Wire:** `impl WireValue for ToolCalls` (recover.rs ~L2548). The `tool_choice` object has the members `{constraint, passed, diagnostics}`. Receipt validation is `{validator:"purrassist.tool_call.v1", constraint:ToolChoice::constraint()}` (recover.rs ~L3351).

---

## 1. Arrays of objects as repeated nested `<parameter=>` elements

### Current behaviour (EXECUTED; this corrects the issue's code-reading prediction)
The issue's exact input and schema (qwen3-coder-xml.v1, adapter tool from OpenAI JSON) do **not** fault as invalid JSON. Instead they are mis-recovered:
- `outfits` = `["<parameter=wearer>Alice", "<parameter=wearer>Bob"]`. Each item is read through the repair `json.syntax.bare-string`.
- `garments` is hoisted to the top level twice through `tool_call.parameter.pass-through`, plus `tool_call.markup.stray-closer` ×2 and `tool_call.parameter.collect`.
- The call ends `Unresolved{SchemaValidation}`, the turn finish is `unresolved`, and the operation status is `BestGuess`.

Root cause:
- xml.rs ~L929-950 pushes a nested scope only for `ValueShape::Object` and `Unmodeled`.
- For `ValueShape::Array(Object)` (~L1062-1081), the item goes to `value(Some(Object))` and then to the tolerant JSON `typed()` path.
- `wearer` and `garments` are not boundaries, because no open scope declares them. `boundary()` and `owner()` only look at open frames.

The fix must therefore open the **item's** object scope so that the member names delimit values. Rejecting the item as JSON is not enough.

### Explicit requirements
- R1.1: When an array's item shape is `Object` or `Unmodeled` and the item body (after skipping whitespace) starts with `<parameter=`, read its members as a nested object, exactly as the object path does. This includes `tool_call.markup.nested-parameter` and `Depth` charging.
- R1.2: Combine this with the existing repeated-name collection. Each repetition contributes one item, in output order (`tool_call.xml.array-item` per item, `tool_call.parameter.collect` at resolution).
- R1.3: The shape also works nested inside an object parameter: `<parameter=plan><parameter=cast>[..]</parameter><parameter=outfits>…</parameter><parameter=outfits>…</parameter></parameter>`.
- R1.4: The result is `Resolved` with a single reading. The declared array type makes the repetition unambiguous: no `repeated` or `repeated-choice` alternatives, no guess rules.
- R1.5: Five goldens are required (A1.1-A1.5 below).

### Implicit requirements
- R1.6: A nested object that pops out of an array-item scope must be adopted **as an item**, through `wrap_item` and `ArrayItem`, so that leaves and unresolved pointers re-root under `/outfits/0/wearer`, not as a bare object member. `pop()` (xml.rs ~L829) currently adopts the frame tree directly.
- R1.7: A repeated `<parameter=outfits>` opener while an item is still open is grounded by `owner()`. It pops the item with `tool_call.markup.missing-parameter-close`, the same as the existing object path.
- R1.8: A single nested item with no repetition gives a one-element array.
- R1.9: `Unmodeled` items (for example `items:{"anyOf":[...]}` or `$ref`) give an untyped object that `tool_call.value.schema-typed` types later. The issue names `Unmodeled` explicitly.
- R1.10: The item schema's `required:["wearer"]` is still enforced. An item without `wearer` must not become `Resolved`.
- R1.11: The JSON-array path (`<parameter=outfits>[{...},{...}]</parameter>`) and scalar repeated items (`repeated_array_param_is_array.json`, `xml_array_item.json`) stay byte-identical in their goldens.
- R1.12: The codec doc (xml.rs L34-71 "Values"), `docs/tool-calls/RULES.md` rows for `nested-parameter` and `array-item`, and `docs/tool-calls/dialects/qwen3-coder-xml.v1.md` are updated. Per AGENTS, public docs must distinguish implemented behaviour from planned behaviour.

### Acceptance checks (all through `ToolSet::recover` via golden fixtures, dialect `qwen3-coder-xml.v1`, `tools_openai` = issue schema)
- **A1.1 Top level.** Input: the issue's block, verbatim.
  - Golden outcome `resolved`, call `resolved`, `finish:"calls"`, `verdict:"run"`.
  - `arguments` == `{"outfits":[{"wearer":"Alice","garments":["a red coat"]},{"wearer":"Bob","garments":["a grey scarf"]}]}`.
  - Rules include `tool_call.markup.nested-parameter`×2, `tool_call.xml.array-item`×2 and `tool_call.parameter.collect`.
  - Rules include **none** of `json.syntax.bare-string`, `tool_call.parameter.pass-through` or `tool_call.markup.stray-closer`. `failed_validations:[]`, no call alternatives.
  - The same input through the CLI `recover-tool-calls --tools F --dialect qwen3-coder-xml.v1` exits 0 with identical `value.calls[0].arguments`. MCP `recover_tool_calls` returns the same `/value/calls/0/arguments`.
- **A1.2 Inside an object.** Schema: `plan:{type:object, properties:{cast:{type:array,items:{type:string}}, outfits:<issue array>}}`.
  - Input: `<parameter=plan><parameter=cast>["Alice","Bob"]</parameter><parameter=outfits>…Alice…</parameter><parameter=outfits>…Bob…</parameter></parameter>`.
  - Result: `{"plan":{"cast":["Alice","Bob"],"outfits":[{…Alice…},{…Bob…}]}}`, Resolved, single reading.
- **A1.3 Mix.** Item 1 is `<parameter=outfits>{"wearer":"Alice","garments":["a red coat"]}</parameter>`. Item 2 is nested elements for Bob.
  - Result: the same `outfits` array in the same order. Both items have identical tree shape and typing: the nested and inline-JSON paths produce the same tree.
- **A1.4 Unclosed last item.** As A1.1, but Bob's item `</parameter>` is missing, so it ends at `</function>`.
  - Result: the same arguments, plus `tool_call.markup.parameter-closed-by-element`, still `resolved`. This mirrors `nested_parameter_object_unclosed.json`.
  - Recommended extra case: the *first* item is unclosed and is followed by `<parameter=outfits>`. Expect `tool_call.markup.missing-parameter-close`, same arguments.
- **A1.5 Negative (String items).** Schema `tags:{type:array,items:{type:string}}`, input `<parameter=tags>\n<parameter=wearer>Alice</parameter>\n</parameter>`.
  - Executed baseline that must be preserved: `{"tags":["<parameter=wearer>Alice</parameter>"]}`, `resolved`, rules `framing-newline`×2, `array-item`, `tool_call.value.closer-in-value`. No nested-parameter edit.
  - Pin it as a golden. An item without an inner closer currently gives `["<parameter=wearer>Alice"]`, resolved.
- **A1.6 Unmodeled items** (R1.9). Items `{"anyOf":[{"type":"object",…}]}` with nested elements give objects typed by the exact schema (`tool_call.value.schema-typed`). Keyword `used:true`.
- **A1.7** A missing required `wearer` in a nested item makes the call not Resolved (`MissingRequired` or `SchemaValidation`).
- **A1.8** Existing goldens, `every_rule_has_a_golden`, `tool_call_rules`, the corpus zero-incorrect evaluation and fuzz all stay green. `make check` passes.

---

## 2. Strict forced tool choice

### Current behaviour (EXECUTED)
`ToolChoice::Forced("ls")` with two `ls` calls gives `passed:true`. With `Sure, listing.\n` prose plus one call it also gives `passed:true`. The enforcement is in `Recovery::tool_choice` (recover.rs ~L4050). The `Forced` arm checks only that there is at least one call and that every call goes to `name`. Constraint: `tool_choice=forced:NAME`.

### Explicit requirements
- R2.1: A variant or option, for example `Forced { name, exactly_one: bool, allow_content: bool }`, meaning exactly one call to `name` and no content outside it.
- R2.2: Optionally, no reasoning either. The suggested signature omits a reasoning flag, but the body asks for it.
- R2.3: The check is recorded in the tool-choice validation "like the existing checks": `ToolChoiceCheck{choice,passed,diagnostics}`, the wire `tool_choice` object, and the receipt validation `constraint`.
- R2.4: Goldens: (a) two calls under the strict form; (b) one call plus prose under the strict form.

### Implicit requirements
- R2.5: A failed strict check follows the existing whole-turn contract: `finish:"unresolved"`, `verdict:"nothing_usable"`, `emitted_calls()` empty, `openai_finish_reason:"stop"`, CLI exit 4.
- R2.6: The existing `Forced(String)` semantics, the `from_openai`/`from_anthropic` behaviour, the constraint string and the goldens (`forced_tool_choice_violated*.json`) stay unchanged. `ToolChoice` is a public exhaustive enum, so changing `Forced` into a struct variant breaks doc examples, the fuzzer (`examples/fuzz_tool_call.rs:192`) and host tests. An additive variant is the safer choice.
- R2.7: A strict choice naming an undeclared tool is a request error (`invalid_input`), as at recover.rs ~L3126.
- R2.8: The new constraint string is distinct and deterministic, and it cannot be confused with a tool name. See the risks table.
- R2.9: Callers of the CLI and MCP need a way to request the strict form. The issue is silent on this. AGENTS requires the complete capability, and the MCP input schema has `additionalProperties:false`.
- R2.10: Calls embedded in reasoning are excluded from the count, as documented for `ToolCalls::embedded`.

### Acceptance checks
- **A2.1 Two calls.** Tools `ls` and `cat`, strict `exactly_one`.
  - Input: `<tool_call>\n{"name": "ls", "arguments": {}}\n</tool_call>\n<tool_call>\n{"name": "ls", "arguments": {}}\n</tool_call>`.
  - Result: `tool_choice.passed:false`, constraint = the strict string, a diagnostic naming the cardinality (2 calls), `finish:"unresolved"`, `verdict:"nothing_usable"`, `usable_calls_accepting_guesses` unchanged but nothing emitted. The receipt validation is `{constraint:<strict>, passed:false}`.
  - CLI exit 4. MCP `/value/tool_choice/passed == false`.
  - Baseline at 943fa04: this passes under `Forced`.
- **A2.2 One call plus prose.** Strict with `allow_content:false`. Input: `Sure, listing.\n<tool_call>\n{"name": "ls", "arguments": {}}\n</tool_call>`.
  - Result: `passed:false`, a diagnostic citing the content range or bytes, and the consequences in R2.5.
  - Baseline: passes under `Forced`.
- **A2.3 Positive control.** Exactly one call whose only surrounding content is whitespace (`"\n"`, which today *is* a `content` range; see the goldens `dangling_trailing_tool_call` and `length_vs_stop_same_text`).
  - Result: `passed:true`, verdict `run`. Without this check the strict form is unusable.
- **A2.4 Orthogonality.**
  - `exactly_one` + `allow_content:true` + prose + one call: passes.
  - `exactly_one:false` + `allow_content:false` + two calls and no prose: passes.
  - A call to another tool still fails with `ToolChoiceForced` as today.
- **A2.5 Reasoning flag** (if implemented). `<think>x</think>\n<tool_call>…</tool_call>` fails with reasoning disallowed and passes with it allowed.
- **A2.6 Regression.** `forced_tool_choice_violated.json`, `forced_tool_choice_violated_beside_a_usable_call.json` and `tool_choice_none_with_calls.json`, plus the host `tool_choice_json_forms` test (crates/host/tests/tool_call_tools.rs:222), stay unchanged.
- **A2.7 Request error.** A strict choice with an undeclared name returns `invalid_input` termination, with no calls.

---

## 3. Marker-free reasoning range

### Current behaviour (code plus EXECUTED)
`ToolCalls::reasoning: Vec<Range<usize>>`, documented as "markers included". It comes from `ReasoningSegment.span` (ir.rs ~L507, built at recognizer.rs ~L473 as `think.start..end`) through `push_range` (recover.rs ~L3052, called ~L3526).

`push_range` **merges adjacent ranges**. Executed results:
- `<think>a</think><think>b</think>\n<tool_call>…` gives `reasoning=[0..32]`.
- `a</think><think>b</think>…` with `starts_open` gives `[0..25]`.

Under `starts_open`, `Think.start` is 0 and `pair` is `None`. A block can end in three ways:
- Closed (`think_closed`, which knows the closer start).
- ClosedAtOpener (no closer; the block ends at the call opener).
- Unterminated (no closer; the block ends at end of output).

Marker pairs vary in length and form:
- `<think>`/`</think>`
- `[THINK]`/`[/THINK]`
- Harmony `<|channel|>analysis<|message|>`/`<|end|>`
- Anthropic **explicit** pairs, e.g. `{"type":"thinking","thinking":"` … `","signature":"`, where the inner bytes are JSON-escaped.

### Explicit requirements
- R3.1: Expose the inner range (the reasoning text without markers) next to the outer range on `ToolCalls`.
- R3.2: Under `ReasoningPolicy::Separate { starts_open: true }` the inner range starts at the block start (no opener is stripped).

### Implicit requirements
- R3.3: The outer `reasoning` field and its wire shape stay unchanged. The change is additive in Rust, on the wire, in the MCP output schema (required + additionalProperties) and in the CLI JSON.
- R3.4: Each inner range lies within an outer range and contains neither its pair's opener nor its closer.
- R3.5: Defined behaviour for every closure:
  - Closed: `[opener_end, closer_start)`.
  - ClosedAtOpener: `[opener_end, outer_end)`.
  - Unterminated: `[opener_end, output_end)`.
  - starts_open: `opener_end = outer_start`.
- R3.6: Bytes are verbatim offsets into the output. They are not unescaped, even for Anthropic explicit pairs, and the docs must say so.
- R3.7: Wire-digest pins are updated only for the added field: `PINNED_DIGEST` in `crates/core/tests/tool_call_recover.rs` and the WASM transcript digests in `crates/wasm-harness/src/transcript.rs`. The published-transcript CI replay (fb522af) must pass.

### Acceptance checks (default dialects via `ToolSet::recover`; byte offsets computed from the literal input)
- **A3.1 Closed block.** `<think>abc</think>\n<tool_call>\n{"name": "ls", "arguments": {}}\n</tool_call>` gives outer `[0..18]` and inner `[7..10]` (text `abc`).
- **A3.2 starts_open.** `abc</think>\n<tool_call>…` with `Separate{starts_open:true}` gives outer `[0..11]` and inner `[0..3]`.
- **A3.3 Closed at opener.** `<think>abc<tool_call>…` (rule `tool_call.reasoning.unclosed`) gives outer `[0..10]` and inner `[7..10]`.
- **A3.4 Unterminated.** `<think>abc` (rule `tool_call.reasoning.unterminated`) gives outer `[0..10]` and inner `[7..10]`.
- **A3.5 Adjacent blocks.** `<think>a</think><think>b</think>` gives outer `[0..32]` (merged, executed) and inner `[7..8, 23..24]`. A rule that pairs index `i` of the outer list with index `i` of the inner list breaks here (see risks).
- **A3.6 Other pairs.**
  - `[THINK]x[/THINK]` (mistral-v11) gives inner `x`.
  - Harmony analysis channel gives the text between `<|message|>` and `<|end|>`.
  - Anthropic thinking block gives the escaped string bytes.
- **A3.7 Wire.** The MCP `recover_tool_calls` output validates against the updated `TOOL_CALLS_OUTPUT`. CLI JSON carries the new member.
- **A3.8** `AsContent` gives no outer and no inner ranges. An empty output under `starts_open` gives no ranges, as today.
- **A3.9** The fuzz invariant holds: inner ⊆ outer, and inner is free of markers.

---

## Explicitly out of scope
- Streaming recognizer, the remaining dialects and `purrassist-derive` (tracked in #4).
- At-most-one for `auto`/`required` (not asked). `disable_parallel_tool_use` is currently rejected by `from_anthropic`. Mapping `{"type":"tool","name":N,"disable_parallel_tool_use":true}` onto strict exactly-one is optional and is a design decision, not a requirement.

## Already satisfied: must not regress (each has goldens)
1. Reasoning that starts open: `reasoning_starts_open*.json`, `deepseek3*_reasoning_starts_open.json`.
2. Adapter tools default to `Mutating`, with a request override (`effects`).
3. Closers missing at EOF under `Stop` are repaired only when the call is complete (`tool_call.markup.missing-closers-at-end`).
4. Ids chain through `next_id_start` (`IdPolicy::Prefix`).
5. Losses cap a call at `BestGuess`.
6. `ToolChoice::from_openai`.
7. An unclosed JSON value followed by the next parameter.
8. A JSON body with one stray closer (`json_body_stray_closer_trailing_text`).
9. A dangling trailing `<tool_call>` (`dangling_trailing_tool_call.json`).
10. Length versus stop truncation (`length_vs_stop_same_text*.json`).

Gate: the whole `tool_call_goldens` suite plus `make check`.

## Dependencies and related
- Depends on #5 (merged, the base). #3 (parent epic, OPEN). #4 (part 2, OPEN; the scope boundary). #1 (0.1.0 qualification, OPEN; the capability claims in docs/QUALIFICATION.md may need updating).
- No ADRs. No recurrence data (first recorded instance; no `GhpRsq-Defect-Class` trailers).
- Note: `.stage/.../plan.md` already exists. This analysis was produced independently, and the risks below include points the plan should confirm.

## Risks and ambiguities

| Risk / ambiguity | Likelihood | Impact | Mitigation |
|---|---|---|---|
| **What counts as "content"** under `allow_content:false`. Today whitespace between calls and after reasoning is a `content` range. Tool results the model wrote count as content. Rejected attempts and `unsupported_dialect` bytes "stay content". | HIGH | HIGH | Allow whitespace-only ranges (define "whitespace": ASCII or Unicode White_Space?). Decide explicitly whether rejected or unparsed markup and model-written tool results fail the check. Golden A2.3. |
| **What counts as "one call"**: `calls` of any outcome (the current `Required` semantics) versus usable calls; rejected attempts; embedded calls (excluded by documented contract). | MED | HIGH | Count `value.calls` (all outcomes), excluding embedded calls. State whether a rejected attempt plus one call passes. Document it. |
| Marking surplus calls: leave outcomes intact (like `Required`), or make calls after the first `Unresolved` with a new reason? A new `UnresolvedReason` is a wire and enum addition. | MED | MED | Either works, because the failed check already empties `emitted_calls`. Avoid "first wins": the check is a contract failure, not a selection. |
| Constraint string collision: MCP tool names may contain `:`, `;` or `,`, and `forced:NAME` puts the name last. | MED | MED | Put the flags before the name (e.g. `tool_choice=forced-strict[exactly-one,no-content]:NAME`), or keep the name last and document that the flags are fixed tokens before it. Add a test with an odd name. |
| `reasoning` merges adjacent ranges (`push_range`), so an index-aligned parallel inner vector is **wrong** for adjacent blocks (A3.5). | HIGH | MED | Either stop merging reasoning (a wire change) or make the inner vector per block and document that it does not align by index. Alternatively, add a per-block struct `{outer, inner}`. Fuzz invariant: every inner range lies within some outer range. |
| Inner range for ClosedAtOpener and Unterminated has no closer to strip. The closure is a guess (`reasoning.unclosed`) or a loss. | MED | LOW | Inner end = outer end. Document that the inner range inherits the block's guess or loss status. |
| `starts_open` blocks close at the closer of *any* non-explicit pair, so the closer length differs by pair. | LOW | MED | Record `closer_start` in `think_closed` and do not compute it from `pair`. |
| Anthropic explicit pairs: the inner bytes are JSON-escaped, and the closer `","signature":"` leaves the signature outside the block. | MED | LOW | Use verbatim offsets and document "not unescaped". Add a golden for the explicit pair. |
| `marker-cut`: a call cut at the closer marker. The marker bytes belong to the block. | LOW | LOW | Inner end = marker start. Covered by the existing `tool_call_recover.rs:1320` test plus an inner assertion. |
| Array item scope: when the item object scope is pushed, the names of its members (e.g. `wearer`) become boundaries for *sibling* values inside the item. A declared item property whose name equals the array's own name, or a top-level parameter name, is resolved innermost-first by `owner()`. | MED | MED | Add a test where an item property shares a name with a top-level parameter. Confirm `rposition` (innermost) ownership. |
| Mixed JSON-array plus nested items in one parameter (`[{…}]` then `<parameter=outfits><parameter=…>`) should concatenate through `collect`. Not asked, but reachable. | LOW | LOW | Add a rule-level test, or document it. |
| `Unmodeled` item through the verbatim path: `verbatim(Array(Unmodeled))` is true, so the nested detection must run before `reading()` or `terminator()` consume closer text. | MED | MED | Detect `<parameter=` before calling `reading()`, as the object path does. Test A1.6. |
| Adversarial depth (arrays of objects of arrays of objects …) and repetition count. | LOW | HIGH | Keep the `Resource::Depth` and `Work` charges. Exhaustion stays explicit (AGENTS contract). Add a scale test or corpus mutant. |
| Golden harness cannot express strict choice (`from_openai` only) or inner reasoning (not captured). | HIGH | MED | Extend the harness `request()` with a new key. Add a captured field and re-bless it with a diff that adds only that field. |
| Public API break if `Forced(String)` is reshaped. | MED | MED | Add a new variant or option instead. Update every exhaustive match (fuzzer, host, emit). |
| CLI and MCP exposure is unspecified by the issue. | MED | MED | Add explicit options. The MCP schema uses `additionalProperties:false`. A strict flag without a forced choice is a request error. |

## Technical notes: affected areas
- `crates/core/src/grammar/codec/xml.rs`: `parameter()` nested detection (~L929), `value()` Array arm (~L1062), `pop()` (~L829, adopt as item), `verbatim()` (~L1655), module docs.
- `crates/core/src/tool_call/resolve.rs`: repeat collection (R3.4d). Check that collecting wrapped object items does not trigger `repeated` or `repeated-choice`.
- `crates/core/src/tool_call/recover.rs`: `ToolChoice` (~L368), `constraint()` (~L527), `Recovery::run` forced lookup (~L3126), `tool_choice()` (~L4050), `ToolCalls` (~L1338) plus wire (~L2548), `segment()` (~L3523).
- `crates/core/src/tool_call/recognizer.rs`: `Think`, `think_opened`, `think_closed`, `unterminated`, `reasoning()` (~L413-485).
- `crates/core/src/tool_call/ir.rs`: `ReasoningSegment` (~L507).
- `crates/host/src/tools/tool_calls.rs` and `registry.rs` (input and output schemas), `crates/cli/src/commands.rs` (flags, HELP), `crates/core/examples/fuzz_tool_call.rs`, `crates/wasm-harness/src/transcript.rs`.
- Docs: `docs/tool-calls/{README,INTEGRATION,RULES}.md`, `docs/tool-calls/dialects/qwen3-coder-xml.v1.md`, `README.md`, `crates/cli/README.md`, `docs/IMPLEMENTATION.md`.

## Related PRs
- #5 (merged): `feat(3): tool-call recovery`, the base. No open PR closes #8.
