# Prior-art assessment: issue #8

This covers arrays of nested-parameter objects, strict forced cardinality, and a marker-free reasoning range.

**Inputs.** `issue.md`, `prior-art.md`, `brief.json`, local git history (167 commits, base `943fa04`), and the worktree code and docs. Nothing was refetched from the forge.

**How issues #6 and #7 are known.** Their content comes only from the squash commit body of `943fa04` (PR #5):

- #6 tracks the unverified fuzz-oracle F4 call-body clause in `fcbab86`, and that `make fuzz` fails on the tool-call target.
- #7 tracks the fixes for F14, F19 and F20 (`3813065` and `fb522af`), which no adversary pass has confirmed.

**Unexecuted claims.** Statements marked "(code reading)" were not executed.

## 1. Precedent in this codebase

### 1.1 Nested-object frame model (`crates/core/src/grammar/codec/xml.rs`)

| Mechanism | Where | Introduced |
|---|---|---|
| `struct Frame { scope, tree, name }`, the frame stack, `owner()` (innermost declaring scope, using `rposition`), `boundary()` (an opener ends a value if *any* open scope declares it), `pop()` (the single attach point, `adopt(name, tree)` into the parent) | `xml.rs` L342, L810-L843 | `58e9ac1` feat(grammar): json and xml-params argument codecs |
| Nesting for `ValueShape::Object` (`EditOperator::NestedParameter`, `tool_call.markup.nested-parameter`) | `Reader::parameter` L929-L955 | `5d52d98` (rule registry, call resolution) |
| Nesting for `ValueShape::Unmodeled` (`Some(None)` scope, an untyped frame typed later by `schema_typed`) | same match | `7c919c9` feat: type unmodeled-schema XML text by the exact schema |
| Goldens `nested_parameter_object_closed` and `nested_parameter_object_unclosed` (the latter ends at `</function>` via `parameter-closed-by-element`) | `tests/data/tool-call-goldens/` | `2b5c98b` |
| Golden `schema_typed_ref_object_nested` | | `7c919c9` |

**Implication.** The frame model already has everything an object item needs: scope typing, `MissingParameterClose` when a sibling's opener arrives, closing at end via `closed_at_end()`, `Depth` charging, and the "closing open nested objects, then stray" pricing in `cheapest()`. The only missing step is attaching a frame as an *item*.

### 1.2 `tool_call.xml.array-item` and `tool_call.parameter.collect`

**Codec side.**

- In `Reader::value` (L1062-L1076), `Array(item)` sends a value that is not a `[`-prefixed array to `value(Some(item))`. It then records `ArrayItem` and wraps the result as `[item]` with `wrap_item` (L1622), which re-roots leaves under `/0`.
- `verbatim()` (L1655) treats an array item as verbatim when the item is verbatim. That line came from `3961333`.

**Resolution side.**

- `Resolver::parameter` (resolve.rs ~L2180) routes `Array` (or repeatable) parameters with more than one occurrence to `collect` (L2266).
- `collect` flattens each occurrence's `Value::Array` items in order. It records `tool_call.parameter.collect` when there is more than one occurrence or the single one is not an array, then types each item with `typed(item)`.
- For an item of type `Object`, `typed` goes through `object()` → `assign` → `parameter`, so repeats *inside* an item are collected as well.
- Introduced in `5d52d98`. Goldens `xml_array_item` and `repeated_array_param_is_array` come from `2b5c98b`; both use scalar string items only.
- `repeated-choice` (non-array repeats → first and last readings) was added in `2abe8c8`.

**Schema typing.** `schema_sites` (resolve.rs L3013) already walks `Array` items whose item shape is `Object` or `Unmodeled`. Untyped item frames will therefore be schema-typed per item without new walking code (code reading).

**Rule registry** (`docs/tool-calls/RULES.md` L65, L67, L106). `nested-parameter`, `array-item` and `collect` are all cost 0, non-guess, single-reading and definition-grounded. That makes them admitted under `Effort::Exact`, and the corpus treats them as `Need::Exact` (`tool_call_corpus/mod.rs` L2634-L2700).

**Why the gap went unseen.**

- The corpus "object parameters as nested elements" mutant only iterates members whose declared shape is `ValueShape::Object`.
- The "array parameters as plain items" mutant only accepts all-string items.
- The printer writes arrays and nested objects as inline JSON, so round-trip tests never produce the repeated nested form.
- A scan of every golden whose tools are declared in `tools_openai` found none with `<parameter=ARR>` followed by `<parameter=` where `ARR` is an array of objects. No existing golden pins the current `InvalidJson` behaviour, so the change should need no re-bless (to be confirmed by running the suite).

### 1.3 Tool-choice validation (`crates/core/src/tool_call/recover.rs`)

- `ToolChoice { Auto, None, Required, Forced(String) }` (L368), `constraint()` (L526), and enforcement in `Recovery::tool_choice` (L4050) all came from `1fbe235`.
- `UnresolvedReason::ToolChoiceForced` and `ToolChoiceNone` were also added in `1fbe235`.
- The check is recorded as a `Validation { validator: "purrassist.tool_call.v1", constraint: "tool_choice=…" }` (L3351-L3360).
  - The issue calls this the "`tool_call.tool-choice` validation". There is no such id: it is the `TOOL_CALL_VALIDATOR` constraint string.
- A forced name is resolved to its canonical name, or rejected as a request error, by `lookup_exact` in `Recovery::run` (L3126). That match covers only `ToolChoice::Forced`.
- The check runs **per reading** (`fn reading` → `self.tool_choice(&mut reading.value, forced)`).
- `Finish` is `Unresolved` whenever `!tool_choice.passed` (L4196).
- Wire: `tool_choice {constraint, passed, diagnostics}` (L2604; MCP output schema `crates/host/src/tools/registry.rs` L44).
- Parsers: `from_openai` and `from_anthropic` (L457, L497). The host and CLI surfaces came from `32ceb64` (`crates/host/src/tools/tool_calls.rs` L175, `crates/cli/src/commands.rs` L319).
- Goldens: `forced_tool_choice_violated` (`2b5c98b`), `forced_tool_choice_violated_beside_a_usable_call` (`78bb239`), `tool_choice_none_with_calls`, and `length_stop_required_choice_on_content`.
- The verdict, finish and emission contract for a failed choice (F5/F7) came from `b65687e`, `78bb239` and `ccd54b1`.

### 1.4 Reasoning segment computation (`crates/core/src/tool_call/recognizer.rs`, `ir.rs`)

- The following came from `b7fe57c`:
  - `Think { start, pair: Option<u32>, promoted, embedded }` (L187);
  - `Turn::reasoning(think, end, marker, closure, edits)` (L418), which stores `ReasoningSegment { span: think.start..end, … }`;
  - `think_opened`, `think_closed`, `promote` (`ClosedAtOpener`) and `unterminated`;
  - `starts_open` seeding `Think { start: 0, pair: None }` (L809).
- `ToolCalls.reasoning: Vec<Range<usize>>`, documented "markers included" (recover.rs L1375), came from `1fbe235`.
- Related fixes:
  - `b8fe388` stopped a starts-open block from swallowing calls;
  - `749f53d`, `b605ce5` and `7a5a6c2` added `tool_call.reasoning.marker-cut`.
- Some of the data the inner range needs already exists but is discarded or kept elsewhere:
  - `Pending::Marker { start, end, pair, open }` (L231) carries the opener **end**, but `think_opened` drops it (`..` at L1012-L1017);
  - the closing marker's **start** is passed to `reasoning()` as `marker` and kept in `Turn::blocks` (L480-L483).
- Marker spellings vary by dialect:
  - `<think>`/`</think>` (Hermes, Qwen, DeepSeek, Kimi);
  - `[THINK]`/`[/THINK]` (Mistral v11);
  - `<|channel|>analysis<|message|>`/`<|end|>` (Harmony);
  - Anthropic `explicit` JSON-fragment markers (`{"type":"thinking","thinking":"` … `","signature":"`, and `redacted_thinking … "}`).
  - For Anthropic, the bytes between the markers are JSON-escaped string content. For `redacted_thinking` they are opaque data, not reasoning text.

## 2. Recurrence

- **Trailer-based: UNKNOWN.** The repository has no `GhpRsq-Defect-Class` trailers (`brief.json` `defect_classes: {}`), so recurrence cannot be counted from them.
- **History- and issue-based.** This is the first recorded instance of each of the three asks:
  - no commit subject or body mentions arrays of nested objects, exact-one or cardinality forcing, or inner or marker-free reasoning ranges (§3);
  - the linked and related items (#1, #3, #4, #5) do not carry these asks in their titles;
  - #4 explicitly holds other scope (streaming, dialects, derive).
- **Count.** This is not "attempt 1". The recurrence count is UNKNOWN.
- **Related defect lineage (not recurrence of these asks).** The xml-params codec around closer handling has been refixed repeatedly:
  - `9a47d3c`, `ef4abf7`, `4c05791`, `c52c3a3`, `749f53d`, `b605ce5` and `7a5a6c2`;
  - adversary passes 4-6 (`73e6a13`, `3961333`, `75c8f11`, `3813065`).
  - The new item path sits in this high-churn region.

## 3. Prior attempts at these exact asks

None found.

How this was searched:

- `git log --all -i --grep` for "array of object", "nested.*item", "exactly.one", "cardinality", "inner range", "marker-free", "without markers", "strip.*think" and "reasoning text". There were three hits, and none was relevant: `749f53d` matched "exactly one closer", `227d4eb`, and `4f055c5`.
- `git log -S` for `reasoning_text`, `exactly_one` and `tool_choice_cardinality`: no commits.
- `git log -G'Array\(item\)'` on `xml.rs`: only `5d52d98` (introduces the single-item wrap) and `3961333` (verbatim item detection). Neither touches object items.
- Branches: only `main`, `origin/paudley/3-tool-call-recovery` and `origin/paudley/1-release-qualification`. Worktrees: `1-release-qualification.artifacts`, plus this one.
- The current Stage-1 `plan.md` in `$STAGE` is this issue's own plan, not prior art.

So no earlier attempt exists that could have failed to hold.

## 4. Adversary-pass findings and issues that constrain the change

- **F9** (`73e6a13`). A canonical `</parameter>` always ends a Qwen value first, and variant or misspelled closers are fallbacks. Typed values and enumerations end at their first closer.
  - Nested item members typed as JSON (for example `garments`) must keep this.
- **F13** (`75c8f11`). A doubled or stray `</parameter>` after a string value is a stray closer, not value text.
  - With item frames, a `</parameter>` after a member value must pop the **item frame** (closing the item), not become stray or text. A further `</parameter>` at top level is stray.
- **F14 and F19, and "Newlines have zero meaning here"** (`3813065`; tracked unconfirmed in #7).
  - Text after an unpaired closer is `Ambiguous` in *any* layout, via `Mode::Hold` versus `Mode::EndFirst`, with `text-after-closer` versus `text-between-parameters`.
  - Paired markup and whole calls stay value text.
  - Closers followed only by closers or whitespace are stray.
  - Two calls in a row are never one call.
  - The item path runs inside `terminator`, `cheapest`, `Held` and `reading()`, so it inherits all of this. Indentation in the issue example (`  <parameter=wearer>`) must not matter.
  - Goldens to keep: `qwen_indented_*`, `qwen_prose_after_*`, `qwen_value_holds_*` and `qwen_inline_closers_then_next_call_are_two_calls`.
- **F15** (`75c8f11`). A corpus truth changes only when an issue clause or a maintainer quote is cited.
  - New corpus mutants for repeated nested items need a truth grounded in issue #8 §1 ("Resolved, single reading").
- **F4, F11 and F16, the rejected-span oracle** (`fuzz_tool_call.rs` L45-L70, L867; F4 call-body clause changed in `fcbab86` and **unverified, #6**).
  - A rejected attempt never covers prose, and a call body after markup debris is covered as rejected markup, never as content.
  - Today a repeated nested item makes the codec fault (`InvalidJson` stand-in, or member tags leaking as top-level parameters). Turning it into a `Resolved` call changes which bytes are call versus rejected. The new path must not make a failing nested item extend a rejected span past where its grammar read.
- **F5 and F7** (`check_verdict`, fuzz L595-L640). Whenever `!tool_choice.passed`:
  - `emitted_calls()` is empty;
  - finish is `Unresolved`;
  - verdict is `NothingUsable`;
  - and a call is `Skipped` as `ToolChoiceFailed` only then.
  - The strict forms must flow through this existing path, not a new one.
- **T2, and the range invariants** (fuzz L76-L90).
  - No call, embedded call or rejected attempt holds marker bytes.
  - Content, reasoning and call ranges never overlap, and an embedded call lies inside a reasoning range.
  - An inner range must be a subset of its outer range and exclude marker bytes.
- **S1.** A reasoning mode never makes call markup vanish. Exposing inner ranges must not change segmentation.
- **F20** (`fb522af`; #7). PRs are gated on `scripts/test-verify-published.py` (the published CLI/MCP transcript replay). Any wire addition (a `reasoning_text` field, new `constraint` strings, new MCP or CLI options) must update those fixtures. The `PINNED_DIGEST` in `tool_call_recover.rs` L732 and the WASM transcripts move with it.
- **Issue #6.** `make fuzz` failed on the tool-call target at merge. Any fuzz evidence for #8 needs a baseline on `943fa04` first, so that pre-existing failures are not attributed to this change.

## 5. Hazards found while reading (code reading; not executed)

1. **Duplicate source pointers across repeats.**
   - In `Resolver::slots` (resolve.rs ~L2822), every occurrence of a repeated name gets the same `source` (`/outfits`).
   - `wrap_item` then puts every item's leaves under `/outfits/0/...`, and `collect` builds item sources as `{source}/{index}` from each occurrence's own array, so every item is `/outfits/0`.
   - `decoded()` (resolve.rs ~L2490) finds a leaf by `leaf.pointer == site.source`, which is the first match. It is only reached for XML text the codec left untyped, so codec-typed nested items are unaffected.
   - Pointer-keyed consumers (evidence `destination`, minimal-diff re-emission "item by item" in `emit.rs` ~L1308, and leaf-to-argument mapping) can still misattribute the second item. `unmodeled_sites` matches by span and is safe.
   - The new goldens should assert that each item's spans and evidence point at its own occurrence.
2. **Nesting requires a known tool at codec time.**
   - When `frames[0].scope` is `None` (unknown or hallucinated name, renamed later), nothing nests. Every opener is an `undeclared-parameter-boundary` guess, and every value is a string.
   - This is already true for object parameters, and the new path inherits it. The docs must not claim otherwise.
3. **A member that shares the array's name.**
   - `owner()` uses `rposition`. If the item shape declares a member with the same name as the array parameter (for example a recursive `children`), the next sibling opener after an *unclosed* item nests inside it instead of starting a new item.
   - Unmodeled item scopes (`None`) declare nothing, so they pop correctly.
   - This needs a golden, or a documented limit.
4. **Strict choice and content semantics.**
   - `ToolCalls.content` contains model-written tool results and inter-call gaps. "No content" must be defined as no non-whitespace content (or a stated rule).
   - Embedded calls in reasoning are not `calls`, so they do not count toward cardinality.
   - `Ambiguous` and `Unresolved` calls do count, as recognized calls.
   - The check runs per reading, so an `Ambiguous` turn's readings may differ.
5. **The fuzz seed layout is full.**
   - Control byte bits 6-7 already encode four choices (`fuzz_tool_call.rs` L192-L196).
   - Adding a strict choice by re-laying out the bits silently changes the meaning of existing regression seeds (`TOOL_CALL_REGRESSIONS`, `scripts/fuzz.py`).
6. **API surface.**
   - `ToolChoice` and `ToolCalls` are not `#[non_exhaustive]` and have public fields. A new variant or field breaks downstream exhaustive matches and struct literals, so it needs a stated compatibility note.
   - The `from_openai` mapping is confirmed satisfied by the integrator and must not change.

## Constraints for the implementation

**Item 1: arrays of nested-parameter objects**

1. Reuse the frame model.
   - Nest when the declared shape is `Array(item)`, `item` is `Object(_)` or `Unmodeled`, and the first non-space bytes are `<parameter=`.
   - Mark the frame as an item. At its single attach point (`pop`), record `ArrayItem` and adopt `wrap_item(tree)`.
   - Every one of these paths must close an item frame correctly: `owner`, `boundary`, `cheapest`/`closed_at_end`, `MissingParameterClose`, and `parameter-closed-by-element`.
2. Keep `collect` as the only place repeats become arrays, in source order.
   - A JSON-object item, a nested item, and a `[..]` occurrence must interleave in order.
   - Record only cost-0, non-guess, single-reading rules (`nested-parameter`, `array-item`, `collect`). The issue's example must then be `Resolved` and admitted under `Effort::Exact` and `Structural`.
3. `Array(String)` and other verbatim items must not nest.
   - Nested tags stay verbatim text, under the existing closer-in-value rules (F13, F14, paired markup). That includes `Ambiguous` where the rules dictate.
   - Item member names are declared only in the item scope, so they must not become boundaries outside an open item frame.
4. Leave `terminator`, `Held`, `Mode`, `reading` and `cheapest` semantics unchanged (F9, F13, F14, F19, unconfirmed per #7).
   - Every existing golden must pass without re-blessing.
   - New corpus mutants must cite issue #8 §1 as their truth (F15).
5. Charge `Resource::Depth` for item frames as for object frames. Exhaustion is an explicit `StopReason`, never a fallback to string.
6. Add the goldens the issue lists: top level, inside an object, mixed JSON and nested, unclosed last item, and the `String` negative. Also add:
   - an unclosed item followed by a sibling;
   - an `Unmodeled` item (schema-typed);
   - an assertion that each item's leaves and evidence spans point at its own occurrence (§5.1).
7. Update the codec Rustdoc (xml.rs L36-L66), the `RULES.md` rows, and `qwen3-coder-xml.v1.md` ("An object parameter may hold nested…").
   - State that nesting needs the tool known at codec time.
   - State the same-name member limit, or fix it.

**Item 2: strict forced tool choice**

8. Add the strict form without changing the meaning of `Forced(String)`.
   - Extend the canonical-name and request-error resolution at `Recovery::run` L3126 to cover it.
   - Record it in the existing `purrassist.tool_call.v1` validation with a distinct, documented `constraint` string. There is no rule id `tool_call.tool-choice`.
9. Use the existing failure path: diagnostics → `passed = false` → `Finish::Unresolved` → `NothingUsable` → empty emission.
   - Any per-call demotion uses an `UnresolvedReason` with a stable `code()`.
   - The fuzz `check_verdict` invariants must hold unchanged.
10. Define and document the rules:
    - what "content" means (non-whitespace; whether model-written tool results count);
    - that cardinality counts `calls` only, any outcome, per reading;
    - that the optional no-reasoning check applies to `reasoning` blocks.
11. Golden-test strict two calls, strict call plus prose, strict reasoning (when enabled), and a strict single call that passes.
    - Update the CLI and MCP surfaces and their published transcripts (F20).
    - Add fuzz coverage without reinterpreting existing seeds (§5.5).

**Item 3: marker-free reasoning range**

12. Expose the inner range as a separate, index-aligned or paired value. Do not change the meaning or wire shape of the existing `reasoning` ranges.
    - Take the inner start from the matched opener's `end` (`Pending::Marker.end`), not from re-measuring a spelling. Under `starts_open` (no opener) it is `0`.
    - Take the inner end from the closing marker's start (`marker`), the outer end for `ClosedAtOpener`, and end of output for `Unterminated`.
13. Invariants, to be enforced by the fuzz probe:
    - inner is a subset of outer;
    - inner holds no marker bytes of its pair;
    - inner never splits UTF-8;
    - inner is aligned with `reasoning`.
14. Document that the inner bytes are verbatim, not unescaped. Anthropic `thinking` is JSON-escaped, and `redacted_thinking` is opaque data.
    - Update the wire output schema (`registry.rs` L44, `additionalProperties: false`), `PINNED_DIGEST`, the WASM transcripts and the published transcripts together, and confirm the diff is only the new field.

**All items**

15. Run under `make check`. Get a fuzz baseline on `943fa04` before attributing any failure, since #6 had `make fuzz` failing at merge.
16. Keep `purrassist-core` free of dependencies and ambient access.
17. Add SPDX headers and Rustdoc with examples and error semantics.
18. Make no capability claim without an executable golden or test.
