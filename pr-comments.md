# PR #9 — comments and review threads

19 comment(s). **Every one needs a disposition** in
`review-debt.md`: addressed (with the commit hash) or declined (with the
reason posted in reply). Bot comments count.

## 1. paudley — 

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

## 2. paudley — 

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

## 3. paudley — 

**Stage 2, R1 / G1: Node repinned to v26.11.1** (commit c1fa81d), per the user's decision.

- **What changed:** the `vendor/tooling.json` node entry now pins the version, license source/notice, binary source and SHA-256 (from the official SHASUMS256.txt) for v26.11.1. The upstream LICENSE is byte-identical to the old notice, so the vendored copy was only renamed.
- **Result:** `make wasm-test` now runs and **passes**. The native, WASI (core/syntax/host) and browser-WASM transcripts all match. This is the comparison Stage 1 could not run.
- **Other checks:** `check-repository.py` and `test-fuzz.py` pass. Independent review: PASS.
- **Hosted CI** did not run; validation is local.

## 4. paudley — 

# Stage 2 remediation plan: PR #9 (issue #8)

Sources:
- `gap-analysis.md` (G1-G13, GAPS-OPEN)
- `completion-audit.md` (FINDINGS-OPEN: F1=G2, F2=G3, F3=G1)
- `reviews/security-review.md` (BLOCKED: R1=G2, R2=G14; advisories A1=G3, A2=G15, A4=G5, A5=G5, A6=G16)
- `reviews/performance-review.md` (BLOCKED: REQUIRED 1=G5; advisory: diagnostics cap = G17)
- `historical-fit.md` (all 16 risks mapped in gap-analysis.md)

User decisions (verbatim):
- "you may not modify the local system.  just change the node version pinned to match ours."
- On the pre-existing repeated-parameter quadratic and Draft-07 root-`$ref` projection, the question was "Should I also open a GitHub issue to track them?" The answer was **"Fix them in this PR"**. Both are now in scope as R10 and R11.

Each R-commit below is one coherent fix with its own focused check, task review `tasks/RN-review.md` (R1 used `tasks/G-R1-review.md` before this rename), push, and a post to #8 and PR #9. A goldens re-bless must change only the files the fix names. Any other golden change is investigated, never accepted blind.

## Gap list

| ID | Sev | Gap | Commit |
|---|---|---|---|
| G1 | HIGH | Node pin 26.10.0 ≠ local 26.11.1, so `make wasm-test` never ran (user decision: repin) | R1 |
| G2 | CRITICAL | `$ref`/`anyOf` (Unmodeled) item and parameter scopes declare no names. Balanced recursive trees are flattened and a colliding member name is hijacked, both reported **Resolved** (a new wrong answer; base abstained). A golden pins the wrong reading. | R2 |
| G16 | LOW | Depth charge admits one extra nesting level (security A6) | R2 |
| G14 | HIGH | `ReasoningBlock::text` can be reversed (`39..38`) when the closer overlaps the opener (Anthropic `redacted_thinking` `"}`), so slicing panics | R3 |
| G11 | LOW | `redacted_thinking` text is opaque data, undocumented; `ReasoningSegment` has no example | R3 |
| G3 | HIGH | Strict exactly-one / no-content passes beside a rejected second attempt; the strict result depends on which merged reading survives `same_calls` | R4 |
| G15 | MEDIUM | `allow_reasoning:false` is unenforceable under `ReasoningPolicy::AsContent` (security A2) | R4 |
| G17 | LOW | Strict diagnostics grow with the number of extra calls and are uncharged (performance advisory) | R4 |
| G5 | HIGH | `ArgTree::adopt` is O(k²) and uncharged (new in this PR) | R5 |
| A5 | LOW | Alias and canonical repetitions of one array number items differently | R5 |
| G6 | MEDIUM | `Resolver::decoded` finds a leaf by pointer, so evidence for `/xs/1` points at item 0's bytes when the tool is known only after name resolution (pre-existing; the PR claims the class is fixed) | R5 |
| G7 | MEDIUM | A reasoning-marker spelling inside a JSON string of a repeated parameter breaks the call: `marker_cut` resolves the pointer through duplicate members and `Node::get` returns `None` (pre-existing; the feature now reaches it) | R6 |
| G4 | HIGH | The tool-call fuzz target has no `Array(Object|Unmodeled)` parameter, so the new path has zero fuzz executions | R7 |
| G9 | LOW | The fuzz trailer-byte claim is untested | R7 |
| G8 | MEDIUM | The `calls_key` change was unreviewed and is not distinguished by any test | R8 |
| G13 | LOW | The published replay has no reasoning block or strict choice | R9 |
| G10 | LOW | Compatibility statement incomplete (PR body, INTEGRATION.md) | R9 |
| A3 | MEDIUM | `usable_calls` is non-zero beside `verdict: nothing_usable` when the tool-choice check fails, and the MCP description tells clients to read it (security A3) | R4 |
| A6-doc | LOW | `Budget::depth` is the stack guard for recursive schema evaluation; this is undocumented (security A6) | R2 |
| G18 | HIGH (user scope) | Wall time and charged scratch are quadratic in repeated parameter occurrences (on base too). Default budgets exhaust near 600 items, which caps #8's feature. | R10 |
| G19 | HIGH (user scope) | A Draft-07 schema whose root is a `$ref` (FastMCP/pydantic shape) projects no parameters (`adapter.rs` ~L905, `PassThrough`) | R11 |
| G12 | LOW | Hosted CI never ran; the MSRV compile was verified locally by the gap analyzer. | — (reported) |

## Fixes

**Execution order:** R1 (done) → R2 → R3 → R4 → R5 → R10 → R6 → R11 → R7 → R8 → R9.
- R10 follows R5, because both touch repeated-parameter cost and the R5 cost test is tightened by R10.
- R11 follows R2, because root projection feeds the scope graph.
- R7's fuzz campaigns run after every code fix.
- R9 runs last, because it enumerates the head's public and wire changes.

### R1: `build(tooling): repin Node to v26.11.1` (G1)
- In `vendor/tooling.json`, change the `node` entry:
  - `version` → `26.11.1`;
  - `source` → `https://raw.githubusercontent.com/nodejs/node/v26.11.1/LICENSE`;
  - notices key → `vendor/licenses/node-26.11.1/LICENSE` (hash unchanged, `6b85ee19…4fc4128`; the upstream LICENSE is byte-identical);
  - `binary_source` → `https://nodejs.org/dist/v26.11.1/node-v26.11.1-linux-x64.tar.xz`;
  - `binary_sha256` → `3883bfc73f9a680ca4eab04b196068aaaab1373ffa77d8fc1a4408222495b651` (from the official `SHASUMS256.txt`, not GPG-verified).
- `git mv vendor/licenses/node-26.10.0 vendor/licenses/node-26.11.1`.
- Historical `docs/qualification/*pattern*.json` records stay as they are.
- No local tool install (user decision).
- **Status: DONE** (c1fa81d). `make wasm-test` ran to completion, exit 0, all legs. The `.cache/wasi` sysroot fetch is the script's own repo-local step and is in scope. `check-repository.py` and `test-fuzz.py` pass. Review: `tasks/G-R1-review.md` PASS.

### R2: `fix(tool-call): $ref and anyOf scopes declare their schema's member names` (G2, G16)
**Design: a scope graph** (plan review F1).
- Compile, once per tool, a finite scope graph from the exact schema. There is one node per distinct schema location: a `$defs`/`definitions` entry, an inline object, or an `anyOf`/`oneOf`/`allOf` branch union.
- Each node holds its accepted name literals and, per member, `(nesting: None | Object(node) | ArrayOfObject(node), typing: unchanged)`.
- Cycles are back-edges by node index, with no unrolling. Cost is bounded by the schema's size and charged at compile time.
- `Frame.scope` becomes an enum `{Declared(&ObjectShape), Schema(&ScopeNode), Opaque}`. `nesting` takes the member's scope entry, so the inner `children` of a `$ref` item opens an item frame on the same node. `declares`, `owner`, `boundary`, `owners` and `cheapest` consult the enum.
- `ValueShape::Unmodeled` stays a unit variant. The graph lives beside `ArgumentShape` or on `ParameterShape`, so the public `ValueShape` API and its doctests do not change. If they must change, R9 lists the break.
- Depth is charged per frame push (existing), which bounds traversal of cycles at read time.
- **Union nesting (A3):** under an `anyOf`/`oneOf`/`allOf` scope, a member nests when its body begins with `<parameter=` and any object branch declares it as object or array-of-object. Typing stays schema-typed.
- The names of the single-`$ref` object parameter case (pre-existing, same root cause) come from the same graph.

**Opaque scopes** (plan review F2).
- Add `EditOperator::UnmodeledScopeBoundary` (`tool_call.markup.unmodeled-scope-boundary`): cost 2, guess yes, readings multiple, grounding syntax, repair yes. `OPERATOR_COUNT` becomes 46, with a RULES.md row and one golden per rule.
- `cheapest`'s `settle` prices pops through `Opaque` frames at this operator's cost.
- The guessed reading is recorded as the single guessed reading. The balanced closer that follows is evidence against the pop, which raises its cost, but the reading is not discarded. The call is therefore `best_guess`, and never runs on a mutating tool.
- **Golden `xml_unmodeled_opaque_scope_boundary`:** tool `rm(path: string, keep: array of {"type":"object","additionalProperties":true})`, input `<parameter=keep>\n<parameter=path>\n/home/user/important\n</parameter>\n</parameter>` inside the normal markup.
  - Expected (single guessed reading; review note): `{"keep":[{}],"path":"/home/user/important"}` with `unmodeled-scope-boundary` plus `stray-closer`. If the hand derivation from the cost model differs, `tasks/R2-review.md` must reconcile it before capture.
  - Required either way: the outcome is `best_guess`, the inference `tool_call.markup.unmodeled-scope-boundary` is present, the verdict is `nothing_usable` under default `Mutating` effects, and the arguments are asserted by exact JSON.

**G16** (plan review F3). The Depth charge counts the frame being pushed. A named test in `tool_call_recover.rs` uses `Budget { depth: d, .. }` with `ToolSet::recover` on nested array items `d` levels deep: that resolves, and `d + 1` levels returns `BudgetExceeded(Depth)` explicitly, with no string fallback and no panic. The same test is the evidence for historical-fit risk 5 and security P1.

**A6-doc.** Add one sentence to the `Budget::depth` Rustdoc: it is the stack guard for recursive schema evaluation, and MCP and the CLI fix it at their documented value.

**Goldens.** Exact JSON is written before capture.
- Replace `xml_array_recursive_ref_items_are_siblings` with `xml_array_recursive_ref_items_nest`.
  - Input p9r; expected `{"children":[{"name":"a","children":[{"name":"b"}]}]}`, **Resolved**.
  - Rules: `nested-parameter`×2 and `array-item`×2 (plus schema-typed if recorded). No `missing-parameter-close` and no `stray-closer`.
- `xml_array_ref_items_member_shares_outer_name` (p10_ref_plan) → `{"name":"party","outfits":[{"name":"Alice","size":3},{"name":"Bob","size":4}]}` (size is an integer per the schema), Resolved.
- `xml_array_ref_items_keep_path_not_hijacked` (rm/keep with `keep: array of $ref{path}`) → `{"path":<top-level as written>,"keep":[{"path":"/home/user/important"}]}`, with the exact input and JSON written before capture.
- `xml_array_ref_items_outer_array_collision` (completion-audit F1 repro 2: `save(tags: string[], notes: array of $ref note{text, tags})`). Each note's `tags` stays inside its note, and the outer `tags` is unaffected.
- `xml_object_ref_param_member_shares_outer_name` (the single `$ref` object parameter).
- `xml_unmodeled_opaque_scope_boundary` (above).

**Corpus.**
- Add a `$ref`-item mutant with a colliding member name to `tool_call_corpus/mod.rs`, citing #8 §1 and this gap (F15). Regenerate, and update `docs/MATCHING-QUALITY.md` and the `correction_evaluation.rs` constants. **Zero incorrect** is required.
- **A4:** also run the base corpus (`git show 943fa04:` of `tool-calls.jsonl`) on the head code and confirm the correct/incorrect/abstained tallies are identical to base, apart from cases whose truth the fix intentionally changes. Each such case is listed.

**Docs.**
- `qwen3-coder-xml.v1.md`: remove the "$ref → sibling" limit; describe scope-graph names and the opaque-scope guess.
- Codec module docs.
- INTEGRATION.md schema-typed section.
- RULES.md row.

**Draft-07 root `$ref`** is now in scope as R11 (user decision).

**Check.**
- Through the built CLI `recover-tool-calls`:
  - p9r, p10_ref_plan, rm/keep and the outer-array collision give the exact expected values, Resolved, exit 0.
  - p9r with `--effort exact` gives Resolved, exit 0.
  - The opaque input gives `best_guess`, verdict `nothing_usable`, exit 4, with the exact arguments.
- The named depth test passes.
- `cargo test -p purrassist-core --test tool_call_goldens --test tool_call_rules --test correction_evaluation --test tool_call_recover --test tool_call_roundtrip` passes, `make corpus` passes, and the A4 base-corpus tally comparison is recorded.

### R3: `fix(tool-call): reasoning text never reverses; redacted thinking is opaque` (G14, G11)
**Fix.** A reasoning closer is recognized only when it starts at or after the block's `text_start`. Apply this in `recognizer.rs` `think_closed` and in every closing path (`promote`, `unterminated`, rejected-attempt cut). `Turn::reasoning` keeps the invariant `text.start <= text.end` with `text` ⊆ `span`, and uses `debug_assert!` plus construction-site guarantees. It does not clamp silently.

**Tests and fuzz.**
- Golden `anthropic_redacted_thinking_closer_overlaps_opener`: input `{"type": "redacted_thinking", "data": "}x`. Expected: one block, span `0..40`, text `39..40` (`x`), closure unterminated (`tool_call.reasoning.unterminated`), `reasoning_text == ["x"]`, asserted by name. The `separate` default and `separate-open` both give `text.start <= text.end` for this input.
- A fuzz seed for that input.
- The `check_reasoning_blocks` invariant asserts `text.start <= text.end`.

**G11.**
- Rustdoc on `ReasoningBlock`, `ReasoningSegment::text` and INTEGRATION.md states that redacted thinking text is opaque encrypted data.
- `ReasoningSegment` gets a doctest.
- A named assertion that `anthropic_redacted_thinking` `reasoning_text == ["EmwKAhgB"]`.

**Check.** The CLI on the security input exits with no panic. Slicing every `reasoning_blocks[].text` of the output succeeds (a test does this over every golden). `cargo test -p purrassist-core --test tool_call_goldens --test tool_call_recover`, `--doc`, and `python3 scripts/test-fuzz.py` pass.

### R4: `fix(tool-call): strict choice counts rejected attempts and survives reading merges` (G3, G15, G17)
**Exactly-one and no-content.**
- Under `exactly_one`, every `value.rejected` entry of any reason (including `unsupported_dialect`) is a cardinality diagnostic.
- Under `!allow_content`, rejected-attempt bytes are non-call bytes and fail no-content.

**Merge independence** (plan review F5). The merge fails closed.
- In `merge`, the merged reading's `tool_choice.passed` is false if any member's is false.
- Its diagnostics are the deduplicated union of the members' diagnostics, then capped per G17.
- The extra-call `ToolChoiceCardinality` outcomes come from the failing member.
- **Named test:** an output whose second `<tool_call>` body is contested by two dialects (one reads a rejected attempt, the other content), under `forced[exactly-one]` with content allowed. It asserts `passed: false` and identical diagnostics regardless of reading order.

**A3: `usable_calls`.** When `tool_choice.passed` is false, `usable_calls` and `usable_calls_accepting_guesses` are 0. That matches the empty emission, so a client reading either field never runs a call from a failed turn. Update INTEGRATION.md (~L1243 and the verdict table), the `ToolCalls::usable`/`verdict` Rustdoc, the `recover_tool_calls` registry description, the fuzz `check_verdict`, and the probe's documented A13 invariant (`fuzz_tool_call.rs` ~L37-39). Add a named host test, and re-bless the goldens that capture `usable_calls` for failed-choice turns. Those are only the `forced_*` and `tool_choice_none_*` files, and each change is listed.

**Fuzz probe and seeds** (plan review F6).
- For the invalid combination (strict no-reasoning under AsContent), the probe asserts the request-error variant and prints `ok`.
- Seed 4 of `TOOL_CALL_STRICT` (`…\xfc`) is re-encoded with a `Separate` reasoning control byte (for example `0xc6`), so it tests what its comment claims.
- One seed is added that draws the error combination.
- `test-fuzz.py` asserts that both run `ok`. The seed change is recorded as a meaning correction, not a preservation.

**G15.** `ForcedStrict { allow_reasoning: false }` under `ReasoningPolicy::AsContent` is a request error (`invalid_input`), surfaced through MCP and the CLI.

**G17.** Diagnostics are capped: at most one cardinality diagnostic that states the count of extra calls, plus one per other failed option. Any per-entry work is charged.

**Docs.** Fix `INTEGRATION.md` ~L1155-1157 and the `StrictForced` Rustdoc.

**Fuzz.** `check_strict`: if `passed` and (`exactly_one` or `!allow_content`), then `rejected.is_empty()`.

**Goldens.**
- `forced_strict_rejected_second_attempt` (p7 + all strict flags): passed false, verdict `nothing_usable`, nothing emitted, exit 4.
- `forced_strict_anthropic_unsupported_dialect_attempt`.
- Named tests for the G15 request error and the diagnostics cap.

**Check.** p7 through the built CLI with the strict flags exits 4, `passed: false`, nothing emitted. MCP gives the same. The focused core/host/CLI suites pass.

### R5: `fix(tool-call): linear, charged item numbering keyed by canonical name; decode evidence by span` (G5, A5, G6)
**G5.** `ArgTree` keeps a running per-name item count, so `adopt`'s `earlier` is O(1). Any remaining per-member work is charged to the Governor.

**A5.** Numbering is keyed by the canonical parameter name, so alias and canonical repetitions share one sequence. `collect` in `resolve.rs` must agree.

**G6.** `Resolver::decoded` (`resolve.rs` ~L2449) finds its leaf by span (`leaf.origin == XmlText && value.span == Some(leaf.raw)`), the same way `unmodeled_sites` does (~L3085). Pointer equality is no longer used.

**Tests.**
- New repeated-parameter case in `crates/core/tests/tool_call_cost.rs`, for k = 1k…8k nested items and one-item JSON arrays, under a budget large enough not to exhaust:
  - Assert that the `work` ratio tracks the byte ratio (linear, within the file's tolerance).
  - Assert that the `total_scratch_charged` ratio is ≤ (k ratio)² × tolerance, with a comment naming the pre-existing quadratic. **R10 tightens this to linear.**
- `adopt` linearity is checked deterministically, not by wall time. Either a `#[cfg(test)]` counter of members visited by `adopt` shows O(k) at k=32k, or the per-adopt step is charged to `Resource::Work` and the charged ratio is asserted.
- Golden `xml_repeated_scalar_items_near_miss_tool_spans` (p6, `--policy tolerant`): `decode-typed` evidence for `/xs/1` spans 74..78, and for `/xs/0` 43..45.
- Alias test (Rust API) for A5.

**Check.** p6 through the CLI shows the spans. The cost test passes. Focused suites pass.

### R6: `fix(tool-call): reasoning markers inside JSON strings of repeated parameters stay value text` (G7)
**Fix.** `marker_cut` (`recognizer.rs` ~L1404-1446) decides "inside a JSON string" without resolving the pointer through duplicate members. Either carry string-ness on `ArgLeaf` from the JSON codec, or find the node by span within its occurrence.

**Must not regress.** The F14/F19 and "reasoning markers are never call content" goldens, and the INTEGRATION.md guarantees ~L561-606.

**Goldens.** p12 (the issue example, Bob's garments `["a <think> scarf"]`) and p12j (repeated JSON-array occurrences). Each is Resolved with the literal value and `reasoning: []`.

**Check.** p12 and p12j through the CLI exit 0 with the exact values. `tool_call_goldens`, `tool_call_rules`, `tool_call_recover` and `make corpus` pass.

### R7: `test(fuzz): reach nested-parameter array items; pin the strict trailer claim` (G4, G9)
**Fuzz tool set and seeds.**
- Add a read-only tool with `Param::array(Param::object(…))` to `examples/fuzz_tool_call.rs`.
- `$ref` items (plan review F9, option b): the probe takes an argument (`fuzz-tool-call --tools ref`) that selects a second fixed toolset, compiled with `ToolSet::from_openai`, with `$ref` and `anyOf` items, colliding member names, and an opaque item.
- That toolset is registered in `scripts/fuzz.py` as its own target `tool-call-ref`, with its own seeds and strict-trailer handling. It is added to the `make fuzz` target loop and to `docs/FUZZING.md`.
- The existing `tool-call` target and its seeds stay byte-for-byte unchanged, apart from the R4 seed-4 meaning correction.
- Add seeds for every §1 shape (top level, in-object, mixed, unclosed last, unclosed before sibling, same-name, string items, `$ref` items, colliding names) to `scripts/fuzz.py`.

**G9.** A `test-fuzz.py` test: every seed with a forced header (`control>>6 == 3`) and a last byte ≥ 0xF8 is in `TOOL_CALL_STRICT`.

**Check.**
- `python3 scripts/test-fuzz.py` passes.
- Each new seed piped to the built `fuzz-tool-call` example prints `ok`.
- **#6 isolation** (plan review F10). `scripts/fuzz.py` gains a documented option, `--continue-on-known` with the #6 panic signatures (`rejected attempt … runs past where it failed`, `… beyond a literal past its end`). The campaign continues past a matching failure and retains every such input for #6. The option is never used by `make fuzz`.
- Run bounded campaigns of `--target tool-call` and `--target tool-call-ref` with the option. Report the number of mutated executions that reached the item branches of `pop` and `nesting` (`xml.rs` ~857-865 and ~1660-1663) and the scope-graph paths, from the llvm-cov report.
- **Attribution.** Every retained input, matching or not, is replayed through a probe built from base `943fa04`. An input that reproduces on base is #6's. Any other input is this branch's to fix before the campaign is claimed clean. Counts (retained, reproduced on base, branch-only) are recorded in `validation.md`.
- A seeds-only fallback is permitted only after isolation is shown infeasible, and is then recorded in `validation.md` as G4 still open.

### R8: `test(tool-call): reading identity distinguishes reasoning block splits` (G8)
**Test.** An in-crate test (`recover.rs` `#[cfg(test)]`) builds two `Reading`s with identical calls and identical merged `reasoning`, but different `reasoning_blocks`. It asserts different `calls_key`s. A focused task review covers the `calls_key` change itself.

If no recognizer input can produce such readings, the test still pins the key function. The doc comment then states that the distinction is defensive and why.

**Check.** `cargo test -p purrassist-core --lib calls_key` passes, and fails when `calls_key` is reverted to hash `value.reasoning`. That second condition is demonstrated once, with the exact command and failing assertion recorded in `validation.md`, and then reverted.

### R9: `docs(tool-call): compatibility note; published replay exercises reasoning and strict choice` (G13, G10)
**G13.** Add to `scripts/data/verify-published/mcp-requests.jsonl` one `recover_tool_calls` request with `<think>…</think>`, a call and `tool_choice_strict`. Then run `python3 scripts/verify-published.py --capture` and review the diff.

**G10.**
- Add a "Compatibility" note to `docs/tool-calls/INTEGRATION.md`. It says additive members may appear in `tool_call.recover.v1` before 0.1.0, and lists:
  - `reasoning_blocks` (required in the output schema);
  - the `tool_choice_cardinality` reason;
  - the `forced[…]` constraint family;
  - Anthropic `disable_parallel_tool_use` now accepted;
  - repeated-array evidence pointers renumbered;
  - tie order of ambiguous readings follows the wire digest.
- R9 enumerates every public-type and wire change present on the head at the time it runs, from `git diff 943fa04 -- crates/core/src` for `pub` items. That includes the R2-R7 additions (`EditOperator::UnmodeledScopeBoundary`, scope types, `ArgTree`/`ArgLeaf` fields, the `usable_calls` change, the request-error combination). They go in both INTEGRATION.md and the PR body.
- **Repeated parameters:** after R10, `docs/RESOURCES.md` states the linear bound for repeated parameter occurrences, with the R10 cost test as its evidence. After R11, INTEGRATION.md states root-`$ref` projection.

**Check.** `python3 scripts/test-verify-published.py` passes, and the fixture holds a non-empty `reasoning_blocks` and a `tool_choice=forced[` constraint. A grep confirms the note and the PR body.

## Settle and exit
- On the final head: `make check` once, `make wasm-test`, and one bounded tool-call fuzz with failure attribution.
- Update `validation.md`.
- A fresh completion auditor (opus/xhigh) re-executes the issue criteria on the shipped CLI/MCP, plus the G2 inputs (p9r, p10, rm/keep, outer-array collision, opaque), G3 (p7), G6 (p6 spans), G7 (p12/p12j), G14, and the G16 depth test.
- G12 (hosted CI did not run) is reported as not run. It is never claimed as passed.

### R10: `perf(tool-call): repeated parameters cost linear time and scratch` (G18, user scope)
**Root cause first** (escalation ladder; no fix by inference).
- Profile `ToolSet::recover` on k = 1k/2k/4k repeated `<parameter=xs>` occurrences: nested items, one-item JSON arrays, and string items, with qwen3-coder-xml.
- Use temporary instrumentation (Governor scratch-lease call sites with sizes, and per-opener lease totals) to name the step whose per-opener cost is O(input).
- Candidates the reviews point at: per-opener re-scans of the remaining body in `reading`/`terminator`/`cheapest`/`select` (`xml.rs`); non-codec per-member work (`adopt` leaf re-prefixing, `record`, `collect`/`Node::get` over duplicate members); recognizer re-reads; and `same_calls`/`calls_key` re-serialization.
- **Conditional user decision:** if the root cause is a lookahead that a correctness rule (closer-in-value, F14/F19) requires, with no decision-preserving linear replacement, stop and ask the user whether to accept a documented non-linear bound asserted by the R10 test or a cited semantic change.
- If it is not identified after reading and instrumenting, a `root-cause-analyst` (opus/high) gets the observed-vs-expected data, not a theory.
- All probes are removed before commit.

**Fix.**
- Make the identified step amortized O(1) or O(item) per opener, for example by caching the next-opener/closer index, scanning forward once, or bounding the lookahead to the current value. The fix must keep every reading and every repair decision identical; recovery semantics do not change.
- If the step serves a correctness rule (closer-in-value weighing, F14/F19), the replacement must provably return the same decisions. The full goldens set, `make corpus` and the F14/F19 tests are the regression oracle.

**Digest rule** (applies to R4 G17, R5 and R10 charging changes). The four `PINNED_*_DIGEST` constants hash `result.to_node()`, including the receipt's charged usage. They are re-pinned only after `to_node()` minus `receipt.usage` is shown byte-identical to the previous head's for each pinned input. The demonstration is recorded in the task review.

**Test.** `tool_call_cost.rs`, repeated-parameter case for k = 1k…8k (nested items, JSON-array items, string items):
- the `work` **and** `total_scratch_charged` ratios track the byte ratio, within the file's linear tolerance;
- k = 8k completes under `sized(Effort::Standard, output)`;
- a k=16k wall-time smoke stays under 4× the k=4k time, recorded and not asserted.

**Docs.** `docs/RESOURCES.md` and the INTEGRATION.md cost-bound section name repeated parameters as covered by the linear bound.

**Check.**
- The cost test passes.
- `make corpus` passes.
- Goldens are unchanged, with no re-bless.
- The bounded fuzz under R7 isolation gives no branch-only failures.

### R11: `feat(tool-call): project a root $ref declaration's parameters` (G19, user scope)
**Fix.** In `adapter.rs` (~L905), a parameters schema whose root is `{"$ref": "#/definitions/X"}` or `{"$ref":"#/$defs/X"}`, followed transitively, projects the target object's properties as the tool's parameters. Projection covers:
- Draft-07 `definitions` and 2020-12 `$defs`;
- a root `allOf: [{"$ref": …}]` with a single object target;
- sibling keywords alongside the root `$ref`, under the rules of each draft;
- nested `$ref` members, which go through the R2 scope graph.

Validation still uses the exact declaration and its draft. Render-back stays byte-exact (IMPLEMENTATION.md acceptance: "byte-exact declaration render-back for all three wire forms, including Draft-07 MCP schemas").

**Not projected:**
- unresolvable or external `$ref`, and roots that resolve to a non-object, keep today's `PassThrough` with its existing evidence;
- a root `anyOf`/`oneOf` of objects **projects the union of names** with typing left to the exact schema, consistent with R2 unions (decided: maximal utility, same design). A golden tests it.

**Conditions for `tasks/R11-review.md`:**
- `tool_adapters::assert_draft07_root_rules` (~L1143-1157) pins the behaviour R11 changes. Its new expectations are listed: Draft-07 sibling `properties` are ignored for projection but kept as keyword evidence.
- The prompt-form `render_declaration` change (the projected spec now lists parameters) is named and asserted.
- `render_canonical` embeds `parameters` verbatim, so byte-exact render-back is asserted, not assumed.

**Goldens and tests.**
- A FastMCP/pydantic-shape MCP declaration (`{"$ref":"#/definitions/Args","definitions":{"Args":{…}}}`, `$schema` Draft-07), read via `from_mcp`:
  - a Qwen3 XML call → Resolved, with typed arguments;
  - a Hermes JSON call → Resolved;
  - a misspelled parameter name → repaired as for an inline schema.
- 2020-12 `$defs` root.
- Root `allOf:[{$ref}]`.
- An unresolvable root `$ref` stays `PassThrough` (pinned).
- Render-back byte-exactness for each declaration.
- The published-declaration tests (`adapter` round trips) pass.

**Check.**
- The CLI `recover-tool-calls --tools <mcp declaration file>` on the FastMCP input gives exact Resolved arguments, exit 0.
- `cargo test -p purrassist-core --test tool_call_goldens --test tool_call_recover` plus the adapter/declaration tests pass.
- `make corpus` passes.

## Declined / not in scope (with reasons)
- **Performance advisory 2** (per-item pointer allocation is uncharged). This is pre-existing and O(1) per item, bounded by the charged items.
- **The #6 rejected-span oracle fuzz failure.** It is tracked by #6 and attributed by exact base replay; R7 isolates it for campaigns.

## 5. paudley — 

**Stage 2, R2 / G2 (CRITICAL): `$ref` and `anyOf` scopes declare their schema's member names** (commit e93eaf8)

**Gap.** A `$ref` or `anyOf` array item, or a single `$ref` object parameter, was read with a scope that declared no member names. Wrong readings came back `Resolved` where the base had abstained:
- a balanced recursive tree was flattened;
- an item member that shared an outer parameter's name was hoisted to the top level;
- in the security review's `rm`/`keep` case, the path to *keep* became the delete's argument.

The Stage 1 golden `xml_array_recursive_ref_items_are_siblings` pinned the wrong reading as a "limit". It is removed.

**Fix.**
- Each adapter tool compiles a finite scope graph from its exact schema: one node per `$defs` entry, inline object or union. Recursive references are back-edges, the graph is compiled iteratively, and the cost is charged.
- Codec frames are scoped by this graph, so nested `$ref` items keep their members.
- A free-form (opaque) scope that closes because of an outer name is now the guess rule `tool_call.markup.unmodeled-scope-boundary`. Such a call is at most `best_guess` and never runs on a mutating tool.
- `$ref`-scope array merges record `tool_call.parameter.collect`, the same as the inline form.
- The depth charge now counts the frame being pushed.

**Evidence** (built CLI):
- The recursive tree comes back nested and `Resolved`, also under `--effort exact`.
- Colliding names keep their items.
- `rm`/`keep` keeps the path inside its item.
- The opaque case gives `best_guess` with `nothing_usable` (exit 4).
- Adversarial schemas (cyclic, 3000-long ref chains, 5000-deep) exhaust explicitly in under 0.1 s.
- The corpus has 18,176 cases and **0 incorrect**, and the base corpus run on the head code gives the base tally.
- The focused suites and `make corpus` pass. Independent review: PASS, and PASS again on recheck.

## 6. paudley — 

**Stage 2, R3 / G14 (HIGH): reasoning text never reverses** (4b80ade)

- **Gap:** for Anthropic `{"type": "redacted_thinking", "data": "}x`, the closer `"}` overlapped the opener's final quote. That gave `reasoning_blocks[].text` = `39..38`, so slicing it panicked.
- **Fix:** a closer is accepted only when it starts at or after the block's text, on every closing path. The text range is debug-asserted, never clamped.
- **Result:** the input is now one unterminated block, span `0..41`, text `39..41` (`}x`), `best_guess`.
- **G11:** the docs now say redacted thinking text is opaque encrypted data, and thinking text stays JSON-escaped.
- **Mutation check:** the new tests and the fuzz seed fail on the previous head.
- **No regression:** 27 cross-dialect CLI probes give identical output to the previous head, and no existing golden changed.
- **Independent review:** PASS, after one description fix.

## 7. paudley — 

**Stage 2, R4 / G3 (HIGH), G15, G17, A3: strict choice** (commit 69c6b5c)

**Gap.** One valid call plus a malformed second attempt passed strict exactly-one/no-content: verdict `run`, exit 0, the call emitted.

**Fix.**
- Under exactly-one, rejected attempts of any reason count against the single call, and their bytes fail no-content.
- Merged readings fail closed: if any member fails the check, the merged reading fails, with the union of diagnostics.
- Diagnostics are capped and charged.
- A failed tool-choice check now zeroes `usable_calls` and `usable_calls_accepting_guesses`, matching the empty emission (A3).
- Strict no-reasoning under `--reasoning content` is an `invalid_input` request error, because it cannot be enforced (G15).

**Evidence.**
- Built CLI on the gap's input: exit 4, `passed: false`, `nothing_usable`, usable 0/0, nothing emitted. A single valid strict call still passes. Non-strict `Forced` is unchanged.
- MCP stdio confirms the same results.
- Each new guard fails its named test when removed (mutation checks).
- Goldens: five `forced_*` files changed only `usable_calls` from 1 to 0, and two were added.
- Independent review: PASS.

## 8. paudley — 

**Stage 2, R5 / G5 (HIGH), A5, G6** (commit 3111582)

- **G5:** the item numbering added in this PR rescanned every member on each repeat, which was O(k²) and uncharged. It is now O(1) per adoption; a counter test checks this at 32k occurrences.
- **A5:** alias and canonical spellings of one array parameter now share one item sequence.
- **G6:** typed-decode evidence for the second repeated scalar item cited the first item's bytes. It now cites its own span. Built CLI: `/xs/0` 43..45, `/xs/1` 74..78.
- **Cost test:** a new test checks that charged work grows linearly with 1k–8k repeated parameters. Charged scratch is still 36× for an 8× input; R10 (next) fixes that pre-existing quadratic, per the user's decision.
- **Checks:** `make corpus` passes, and the independent review passes.

## 9. paudley — 

**Stage 2, R10 / G18: repeated parameters are linear** (user decision: "Fix them in this PR")

- **Root cause:** found by instrumenting the scratch charges, not by inference. It predates this branch: `Resolver::collect` and the typed-array branch re-measured and cloned the whole partial array for every item, which is O(k²).
- **Fix:** items now extend in place. Readings, decisions and goldens are unchanged (no re-bless), and the pinned digests are unchanged.
- **Measured** at 8,000 nested items: scratch dropped from 13.99 GB to 1.54 GB and wall time from 9.0 s to 1.8 s.
- **Cost test:** asserts that work and scratch grow linearly with bytes.
- **Remaining limit:** the published default budget is still below the per-byte cost of dense markup, so it exhausts explicitly at about 1,000 nested items. Per the user's decision, R12 raises the published constants.
- **Review:** independent review PASS after a correction to the doc numbers.

## 10. paudley — 

**Stage 2: HashMap fix (342f2d4) and R12 / G20 (8fb63a4)**

**342f2d4: HashMap fix.** R5 had introduced a `HashMap` into `purrassist-core`. Its default hasher is seeded from ambient randomness, which the core contract forbids. It is now a `BTreeMap`. The commit hook doesn't run `make hygiene`, so every later fix now runs it explicitly.

**8fb63a4: published per-byte budget raised (user decision: "Raise the published constants").**
- **Measured:** all 12 dialects at 1 MiB on dense markup. The maximum is 138 work per byte and 293 peak scratch per byte.
- **New constants:**
  - work per byte: 32 → **176**;
  - scratch per byte: 48 → **368**;
  - evidence: new, ⌈n/7⌉ entries.

  Margins are 25–41% over the measured maxima.
- **Effect:** every measured dense shape up to 1 MiB recovers under the default `sized` budget. Tests run the dense shapes at 64 KiB and, in the corpus lane, at 1 MiB, and they fail with the old constants.
- **Breaking change:** sized budgets grow. In the published transcript, only the budget numbers changed.
- **Review:** independent review PASS.

**Next, by user decision ("Fix (1) and (2) here, document (3)"):**
- **R13:** Mistral v11 calls chained by whitespace are re-segmented at every call, so cost grows faster than linear. They will segment incrementally.
- **R14:** a Mistral v3 array of thousands of calls takes quadratic wall time while charged work stays linear. Work will become linear and fully charged.
- **Brace-heavy call-free text** stays a documented explicit exhaustion.

## 11. paudley — 

**Stage 2, R13 / G21: Mistral v11 chained calls are linear** (bdb102a; user decision: "Fix (1) and (2) here, document (3)")

- **Root cause:** found by instrumentation. Every completion re-segmented the whole chain, re-reading and cloning all earlier calls. Fixing that exposed two uncharged per-byte scans.
- **Fix:** each chained call is closed once and shared, dominated completions are not materialized, and held completions are indexed.
- **Measured at 1,024 calls:** work dropped from 275 to 64 per byte and peak scratch from 3,420 to 218 per byte. 1 MiB of chained calls recovers under the default budget.
- **Results unchanged:** all goldens, plus about 14,000 differential runs, are byte-identical apart from usage counters.
- **Margin:** the DeepSeek dense rows were re-measured, and the published scratch margin is now 22%.
- **Review:** independent review PASS.
- **New finding, pre-existing:** restarted DeepSeek chains are superlinear (489 work per byte at 64 sections, 1,666 at 256). This is identical on the previous head and held for a user decision.

## 12. paudley — 

**Stage 2, R14 / G22: Mistral v3 call arrays are linear and fully charged** (3743aed)

- **Root cause:** `Engine::hold_failures` re-filtered every held failure at every byte, and charged none of it. Charged work looked linear while wall time was quadratic: 24 s at 4,096 calls, and no finish at 1 MiB within 300 s.
- **Fix:** held failures are indexed so each byte examines only what can change, and every examination is charged.
- **Measured:** 1 MiB (15,195 calls) now recovers in about 6 s, at 79.6 work per byte, within the published budget.
- **Results unchanged:** all goldens, plus 2,715 differential cases, are identical apart from usage counters.
- **Digests:** pinned digests were re-pinned under the usage-only rule, with all 7 pinned results shown byte-identical once `usage` is stripped.
- **Review:** independent review PASS, and PASS again on recheck.

**Restarted DeepSeek-V3.1 sections** were filed as #10 and accepted for #8 as a documented limit, per the user's decision. Measured on the built CLI: quadratic charged work and super-quadratic wall time, identical on `main`.

## 13. paudley — 

**Stage 2, R6 / G7: reasoning markers inside JSON strings of repeated or alias parameters stay value text** (917553e)

- **Bug:** `<think>` inside a JSON string of the second repeated item (for example the issue's example with `["a <think> scarf"]`) cut the call, returning `Unresolved {}`. The cause was a marker lookup by pointer, which fails on duplicate members and alias spellings.
- **Fix:** a per-reading index of string-data spans, searched by binary search. It is charged, and its scratch is a lease that is always released.
- **Results:** the repro inputs now resolve with the literal value. Markers outside string data still cut.
- **Wall time:** with a marker in every item, wall time equals the marker-free baseline. The review caught an uncharged quadratic in the first version, and that is fixed here.
- **Review:** independent review PASS after the fix.

**Next:** R15 investigates a residual of about 2.2× wall time per doubling of repeated items, with linear charged work, before R11, R7, R8 and R9.

## 14. paudley — 

**Stage 2, R15 / G25: residual superlinear wall time fixed** (c2f1916), plus receipt fixture refresh (7136da7)

**R15.**
- **Gap:** many repeated items still took superlinear wall time while charged work stayed linear.
- **Cause:** two provenance steps were quadratic and uncharged. Derivation membership used a linear scan, and the forest index was a sorted vector with an O(n) insert.
- **Fix:** both are now ordered sets or maps, with identical charges and results.
- **Measured:** time per charged unit at 64k items dropped from 136 ns to 37 ns, flat across sizes.
- **Tests:** two unit tests fail if either step reverts; the reviewer verified this by reverting.
- **Review:** independent review PASS, after the test was added.

**Receipt fixture (7136da7).** A receipt fixture test had been failing on the branch since earlier charging changes, because my per-fix focused checks did not include it.
- **Refresh:** only its usage numbers were refreshed, after confirming the receipt is otherwise field-for-field identical.
- **Process change:** every fix now runs the whole core test crate.

## 15. paudley — 

**Stage 2, R11 / G19: root `$ref` declarations project their parameters** (a49e81b; user decision: "Fix them in this PR")

**Gap.** Draft-07 schemas whose root is a `$ref` (the FastMCP/pydantic shape) projected no parameters, so calls were passed through without typing or repair.

**Fix.**
- The parser now follows a root `$ref` to its local definition (Draft-07 or 2020-12 sibling rules), a root `allOf` of one `$ref`, and a root `anyOf`/`oneOf` of objects (union of names).
- Validation still uses the exact schema.
- Render-back is byte-exact in all three wire forms.

**Built CLI.** A FastMCP declaration with qwen and hermes calls resolves with exact typed arguments, exit 0.

**Review fixes.** The review loop found and fixed two quadratics: uncharged O(n²) in the new projection code, and a pre-existing O(n²) scan in the receipt validation log that wide unions exposed. A call over a 500-branch union dropped from 537k to 35k work.

**Equivalence.** A differential run over 491 goldens is identical to the previous head apart from usage counters.

**Limit.** The default-budget ceiling for root unions moves from about 670 to about 610 one-property branches; exhaustion stays explicit.

**Also committed:** c6ce650 refreshes the published MCP transcript, whose usage figures had moved. The diff is usage-only and was captured from a clean checkout.

**Review:** independent review PASS, after two rechecks.

## 16. paudley — 

**Stage 2, R7 / G4 (HIGH), G9: the fuzzer reaches the new code** (8bca69e)

**Coverage.** A new `tool-call-ref` target (101 seeds) uses a second tool set with `$ref`, `anyOf` and nested-object items, colliding names, recursion, free-form items and a root `$ref`. Measured with llvm-cov, mutated executions now reach the nested-item codec branches (214 hits on `pop`'s item branch) and the scope graph (121–136 hits). The old target reaches none of them.

**Campaigns** (bounded, `--continue-on-known`):
- `tool-call`: 1,408 executions.
- `tool-call-ref`: 1,101 executions.
- **0 branch-only failures.** All 6 #6-signature inputs reproduce on a base `943fa04` probe.

**Accepted limit (user decision).** Issue #11 was filed for a pre-existing slow repair-search input (192 bytes; about 5 s with the probe's budget; under-charged work). `make fuzz` at the default timeout will stop there, and `tool-call` still stops on #6.

**Review.** Independent review PASS.

## 17. paudley — 

**Stage 2, R8 / G8: reading identity distinguishes reasoning block splits** (f93f0b4)

- **Gap:** Stage 1 changed the reading identity key to use reasoning blocks, but no test told the old key from the new one.
- **Fix:** a unit test on real recognized readings checks three cases:
  - equal keys for identical readings;
  - different keys for a different block split;
  - different keys for a text-only change.
- **Mutation check:** reverting the key to the merged ranges fails the test, and both the implementer and the reviewer demonstrated this.
- **Defensive by design:** no input can produce such readings today, because every dialect shares one reasoning-marker matcher. The doc comment says so.
- **Review:** independent review PASS.

## 18. paudley — 

**Stage 2, R9 / G10, G13: compatibility statement and published replay** (2484700)

- **Compatibility statement:** `docs/tool-calls/INTEGRATION.md` now has a Compatibility section covering wire/MCP, CLI, behaviour and budget changes, plus a Rust API subsection with 12 breaking changes and 6 additions. The reviewer derived the list independently from the diff and found no omissions.
- **Published replay:** the MCP replay now exercises `reasoning_blocks` and the strict choice end to end, in one passing case and one failing on reasoning.
- **Review:** independent review PASS.

All remediation fixes are committed. The settle phase is next: one full `make check`, `make wasm-test`, bounded fuzz with attribution against the base, and a fresh completion audit.

## 19. paudley — 

## Stage 2 complete for #8 / PR #9: exit audit PASS on `42899ab`

**Gaps resolved:**
- G1–G26 from the gap analysis, completion audit, security and performance reviews.
- All 13 REQUIRED plan-review findings.
- One CRITICAL defect (G2: `$ref` items misread as `Resolved`).
- Every gap is FIXED except G12 (hosted CI, external) and the two user-accepted issues, #10 and #11.

**Fix commits:**
c1fa81d, e93eaf8, 4b80ade, 69c6b5c, 3111582, 342f2d4, 3a19a2d, 8fb63a4, bdb102a, 3743aed, 917553e, 7136da7, c2f1916, c6ce650, a49e81b, 8bca69e, f93f0b4, 2484700, 42899ab. Eighteen of them had a dedicated independent task review that passed. The other four (342f2d4, 7136da7, c6ce650, 42899ab: a `BTreeMap` swap, two usage-only fixture refreshes and a helper dedup) are covered by full-crate tests, `make check` and the exit audit. Several of those reviews first blocked on real defects, which were fixed before commit: uncharged quadratics in two of the fixes, a missing test that would detect a revert, and stale numbers in the docs.

**Scope decisions (verbatim):**
- Node repin.
- "Fix them in this PR".
- "Raise the published constants".
- "Fix (1) and (2) here, document (3)".
- DeepSeek: issue #10, accepted.
- Slow repair search: issue #11, accepted.

**Local validation on 42899ab:**
- `make check` and `make wasm-test` exit 0.
- Corpus: 0 incorrect over 18,176 cases.
- Fuzz: `tool-call` and `tool-call-ref` pass with `--continue-on-known`. The 6 #6 inputs reproduce on base, and there are 0 branch-only failures.
- Fresh completion audit: **PASS**, with every criterion re-executed on the shipped CLI and MCP.

**Hosted CI:** never started. All validation above is local.

**Unrun or known:** `make fuzz` in its gate form stops on #6 and on #11, both pre-existing on `main`.


**Stage 3 readiness:**
- Ready to run Stage 3 (merge-tree assessment, preflight, squash).
- Any new commit invalidates the local gates.
- Hosted CI has not run.

