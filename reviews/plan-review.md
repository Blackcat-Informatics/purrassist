# Plan review: issue #8 Stage 1 plan

**Reviewed plan:** `/home/paudley/Active/purrassist/.worktrees/8-tool-call-recovery-arrays-of-nested/.stage/tool-call-recovery-arrays-of-nested/plan.md`
**Base:** `943fa04` (origin/main, #5 merged)
**Reviewed:** 2026-10-09

## Coverage

Read in full: `plan.md`, `issue.md`, `issue-analysis.md`, `prior-art-assessment.md`, `.goals`, `AGENTS.md`, `docs/IMPLEMENTATION.md`, `docs/tool-calls/README.md`, `docs/tool-calls/INTEGRATION.md` (all 1174 lines).

Verified against source at the base (line numbers are the base's):

- `crates/core/src/grammar/codec/xml.rs`: module docs L34-103, `Frame` L342, `boundary`/`owner`/`pop`/`closed_at_end` L810-852, `parameters` L855-899, `parameter` L902-1006 (nesting match L934-955 precedes `reading()` L958, so the plan's "detect before reading" holds), `value` Array arm L1062-1077, `wrap_item` L1622, `verbatim` L1655.
- `crates/core/src/tool_call/resolve.rs`: `UnresolvedReason` + `code()` L424-515, `collect` L2266-2339 (item sources are `{occurrence.source}/{index}`), `slots` L2745-2834 (every occurrence of a key gets the same `source`, confirming the `/NAME/0` aliasing hazard the plan's row 8 covers).
- `crates/core/src/tool_call/recover.rs`: `ToolChoice` L368-378, `from_openai`/`from_anthropic` L457-519, `constraint` L526-533, `ToolCalls` L1338-1397, wire L2548-2627, `push_range` L3052 (merges adjacent ranges, confirming the non-alignment the plan documents), forced lookup L3126-3139, `tool_choice` L4050-4097.
- `crates/core/src/tool_call/recognizer.rs`: `Think` L187, `Pending::Marker{start,end,..}` L231 (opener end exists and is dropped by `think_opened` L1174), `reasoning()` L418 (closer start arrives as `marker`), `promote` L1131, `think_closed` L1629, `unterminated` L1677, edit wording L2323.
- `crates/core/src/tool_call/ir.rs` L507 `ReasoningSegment` (pub fields); `mod.rs` L77-81 re-exports it publicly.
- `crates/core/tests/tool_call_goldens.rs`: `CAPTURED` L59-74, `request()` L317-362 (`reasoning` is an input key read with `as_str`), `reading_fields` L544-586.
- `crates/host/src/tools/tool_calls.rs` L175-187, `registry.rs` L44 (`TOOL_CALLS_OUTPUT`, `additionalProperties:false` on `tool_calls`), `crates/cli/src/commands.rs` L317-328, `crates/core/examples/fuzz_tool_call.rs` L170-208 (control byte bits 6-7 hold the four choices, as the plan says).
- `crates/wasm-harness/src/transcript.rs` L94-182; `crates/core/tests/tool_call_recover.rs` (`PINNED_DIGEST` at L732, not L652); `scripts/verify-published.py`, `scripts/test-verify-published.py` L436-465, `scripts/data/verify-published/mcp-{requests,responses}.jsonl`; `docs/tool-calls/RULES.md` rows L65/L67/L106; `docs/tool-calls/dialects/qwen3-coder-xml.v1.md` L42-43, L82-83; `docs/FUZZING.md` exists.

Not executed: nothing was built or run. Findings below rest on code reading and on the executed baselines the issue analysis reports.

## Issue-to-plan mapping (completeness)

| Issue ask | Plan row(s) | Status |
|---|---|---|
| §1 nested items at top level, Resolved, single reading | 1, 8 | mapped, executable |
| §1 same shape inside an object parameter | 2 | mapped |
| §1 mix of JSON-object item and nested item | 3 | mapped |
| §1 unclosed last item | 4 (+7 extra) | mapped |
| §1 `String` item negative | 5 | mapped |
| §1 `Unmodeled` item (named in the ask) | 6 | mapped |
| §2 exactly one call, no content | 10, 11 | mapped |
| §2 optionally no reasoning | 12 | mapped |
| §2 recorded in the tool-choice validation | 13 | mapped (the plan correctly uses the `purrassist.tool_call.v1` constraint string; there is no rule id `tool_call.tool-choice`) |
| §2 goldens: two calls; one call plus prose | 10, 11 | mapped |
| §3 inner range next to the outer range; `starts_open` | 14 | mapped |
| "already satisfied" list must not regress | 9 | mapped (whole golden set) |

No issue requirement is unmapped. No planned-by-refusal. The two declined items (at-most-one for `auto`/`required`; a struct-of-ranges `reasoning`) are not issue asks, so they are not pre-emptive descopes.

## Findings

### Broader review (completeness, constraints, design, utility)

**1. REQUIRED. The planned captured golden field `reasoning` collides with the existing `reasoning` fixture input key.**
Plan line 117: "Goldens gain a captured `reasoning` field (inner texts), blessed once."
`tool_call_goldens.rs` L344 reads `golden.get("reasoning").and_then(Node::as_str)` as the request's `ReasoningPolicy` (`"separate" | "starts_open" | "as_content"`), and `CAPTURED` (L59-74) is the list of members a bless overwrites. Adding `reasoning` to `CAPTURED` makes `PURRASSIST_BLESS=1` overwrite the policy string with an array in every fixture that sets it (`reasoning_starts_open*`, `deepseek3*_reasoning_starts_open`, every `as_content` fixture); on the next run `as_str` returns `None`, the policy silently falls back to the default, and the "already satisfied" row 9 behaviours (reasoning that starts open) are no longer exercised. The runner's "bless cannot weaken a named assertion" guard does not cover an input key being clobbered.
`REWRITE TO:` "Goldens gain a captured `reasoning_text` field (one inner range per block as text, bytes when not UTF-8, mirroring `content`), added to `CAPTURED`; the input key `reasoning` (the policy) is untouched. Blessed once; the diff is checked to add only that field."

**2. REQUIRED. The committed published-transcript fixtures (F20) are omitted from the artifacts the wire and schema changes move.**
Plan lines 115-117 list the wire addition, `TOOL_CALLS_OUTPUT`, `PINNED_DIGEST` and the WASM transcript; plan line 94 adds MCP input options. Neither Task 2 nor Task 3 nor Cross-cutting names `scripts/data/verify-published/mcp-responses.jsonl`, which holds a full `recover_tool_calls` response (`mcp-requests.jsonl` L20) and the `tools/list` schemas. `scripts/test-verify-published.py` L436-465 replays those committed fixtures against the current workspace CLI and fails when they are stale ("regenerate with `python3 scripts/verify-published.py --capture`"); it is run by `.github/workflows/verify.yml` L27 (the PR gate from `fb522af`) and, per its docstring, by `make hygiene`. Prior-art constraint 14 names this explicitly. As planned, Task 2's input-schema change and Task 3's output change each make the fixtures stale, and the plan's per-task commit sequence ("implement → focused tests → task review → commit → push") commits stale fixtures twice before the single `make check` at the end. AGENTS: "Implement the complete accepted capability"; IMPLEMENTATION.md: "`make check` runs formatting, lint, optimized build/test, Rustdoc, and repository hygiene with warnings as errors."
`REWRITE TO:` add to Task 2 "Wire and surfaces" and to Task 3 "Wire": "Regenerate `scripts/data/verify-published/{mcp-responses.jsonl,call-response.json}` with `python3 scripts/verify-published.py --capture` and review the diff: Task 2 adds only the new input-schema members to `tools/list`; Task 3 adds only `reasoning_text` to the `recover_tool_calls` response and the output schema. Include `python3 scripts/test-verify-published.py` in each task's focused check."

**3. REQUIRED. A third public API break is unnamed.**
Plan line 127: "`ToolChoice` gains a variant and `ToolCalls` gains a field. Neither type is `#[non_exhaustive]`." Plan line 109 also adds a field to `ReasoningSegment` ("Expose `ReasoningSegment::text`"), which is a public struct with all-public fields (`ir.rs` L507-518) re-exported at `tool_call/mod.rs` L77-81; downstream struct literals and exhaustive destructuring break. The PR body under-reports the break as written.
`REWRITE TO:` "`ToolChoice` gains a variant, `ToolCalls` gains a field, and `ReasoningSegment` gains a field. None of the three types is `#[non_exhaustive]`. The crate is unreleased (0.1.0 qualification in #1), so the breaks are accepted and all three are named in the PR body."

**4. REQUIRED. "Whitespace" in the no-content rule is undefined.**
Plan line 79: "'Content' means any `content` range that holds a non-whitespace byte." Plan line 86: "Inter-call framing whitespace passes." The issue analysis lists this as a HIGH/HIGH risk ("define 'whitespace': ASCII or Unicode White_Space?") and the plan does not answer it. The consequence is concrete: a turn framed with U+00A0 or U+3000 passes or fails depending on the implementer's choice, and the docs the plan promises (INTEGRATION.md "contract-failure paragraph") cannot state the contract. The core is byte-oriented and dependency-free; the ASCII rule is the one the codec already uses for framing and is decidable per byte.
`REWRITE TO:` "'Content' means any `content` range holding a byte that is not ASCII whitespace (`u8::is_ascii_whitespace`: space, `\t`, `\n`, `\r`, `\x0C`). Non-ASCII whitespace is content. The golden `forced_strict_single_call_passes` uses `\n`; a second named assertion checks that U+00A0 between the call and the end of output fails `no-content`. The rule is stated in the `StrictForced` Rustdoc and INTEGRATION.md."

**5. ADVISORY (design). `reasoning_text` as a flat, non-aligned range list loses the per-block outer span that the engine already has, and the stated reason for declining a per-block struct does not apply.**
Plan line 143 declines "a struct-of-ranges `reasoning`" because it would "break the existing `reasoning` wire shape and the MCP `range` `$ref`". That is true of *replacing* `reasoning`; it is not true of *adding* a per-block field beside it. Consequence of the chosen design: `push_range` merges adjacent blocks (`recover.rs` L3052), so a caller holding an inner range cannot recover its own block's outer span from the public result at all (the merged `0..32` of `<think>a</think><think>b</think>` is not either block's span), and must search by containment, which the plan itself names as the cost. An additive `reasoning_blocks: [{span, text}]` (one entry per `ReasoningSegment`, unmerged `span`, inner `text`) would give strictly more, be self-describing (no "not index-aligned" caveat to document), and reuse the `range` `$ref` twice; `reasoning` stays as is. Also, the name `reasoning_text` for a list of byte ranges reads as a list of strings; if the flat list is kept, `reasoning_inner` says what it is. Not blocking: the plan's field satisfies the issue's ask.

**6. ADVISORY. The strict-choice surfaces have four spellings and two undefined interactions.**
Rust `StrictForced{exactly_one, allow_content, allow_reasoning}` (line 77); MCP `tool_choice_exactly_one`, `tool_choice_allow_content`, `tool_choice_allow_reasoning` (line 94); CLI `--tool-choice-exactly-one`, `--tool-choice-no-content`, `--tool-choice-no-reasoning` (line 95); golden `tool_choice_strict` (line 96). Undefined: (a) an MCP request carrying both the Anthropic `{"type":"tool","name":N,"disable_parallel_tool_use":true}` form (line 93) and `tool_choice_exactly_one:false`, or a strict option beside an `OpenAI` form, which `tool_calls.rs` L175-187 routes through one `tool_choice` parser; (b) the constraint string of a `ForcedStrict` whose three options are all at their lenient values (line 78 gives only the all-active example), which is semantically `Forced` and should either be the same string `tool_choice=forced:NAME` or be rejected by the builder. Suggest: one MCP member `tool_choice_strict: {exactly_one, allow_content, allow_reasoning}` mirroring the golden key, a request error when it accompanies a non-forced choice or contradicts the Anthropic flag, and a stated constraint string for the degenerate case.

**7. ADVISORY. Two fuzz statements need a sharper form before they are implemented.**
Line 97: "a strict pass implies at most one call and no non-whitespace content" holds only when `exactly_one` and `!allow_content` are set; with the options drawn from a separate seed byte the invariant must be conditioned on them (and a third clause for `!allow_reasoning`: no `reasoning` range). Line 118: "the inner ranges partition the reasoning blocks (each block has exactly one inner range)" is checkable only if the probe can count blocks, and `ToolCalls` exposes merged outer ranges, not blocks; state the counting method (count opening markers inside outer ranges, with `starts_open` adding one block at offset 0; or run the `Recognizer` and count `Segment::Reasoning` entries).

**8. ADVISORY. Required-member enforcement inside nested items (analysis R1.10 / A1.7) is not in the contract.**
`collect` types each item through `typed(item)` (`resolve.rs` L2320), which should enforce `required:["wearer"]`, but no row checks it, and the new path is the first that produces object items from the codec's nested frames. Add one named assertion: an item lacking `wearer` is `Unresolved{MissingRequired | SchemaValidation}`, never `Resolved`.

**9. ADVISORY. Small factual drifts.**
Line 61 "Add the seven goldens": the contract table lists eight Task 1 goldens (rows 1-7 and 9; row 8 is a named test). Line 116 "`PINNED_DIGEST` (`tool_call_recover.rs:652`)": it is at L732. Line 116 "the WASM transcript digests": `transcript.rs` L94-96 and L161-163 pin digests of the emitted `tool_calls` arrays only, which `reasoning_text` does not change; the full `result.to_node()` is embedded (L169, L179) and compared across targets, not pinned, so expect no re-pin there and say so, or name the artifact that actually moves.

### Completion rubric (plan-only; four grounds)

**R1. REQUIRED. UNFALSIFIABLE CRITERION, row 9 (plan line 37).**
> "Item type with a member named like the array, item unclosed: behaviour pinned and documented | 1 | golden `xml_array_nested_object_same_name_member` (innermost-scope rule; the outcome is whatever the cost model yields, never a silent Resolved misnesting: the expected value is asserted by hand)"

"Whatever the cost model yields" cannot fail: a bless records whatever comes out and the hand assertion is written afterwards from that output. The clause also contradicts itself: under the innermost-scope rule (`owner()` uses `rposition`, `xml.rs` L819-826) the second `<parameter=children>` opener after an unclosed item *is* declared in the item's scope, so it is adopted as the item's member with no repair and the call is `Resolved`. That is the rule's reading, not a "misnesting", and the plan must say whether it accepts it as the documented limit (prior-art constraint 7: "State the same-name member limit, or fix it") or requires a different outcome.
`REWRITE TO:` "Item type with a member named like the array, item unclosed: innermost-scope rule | 1 | golden `xml_array_nested_object_same_name_member`: schema `children: array of {name: string, children: <same array>}`, input `<parameter=children><parameter=name>a</parameter><parameter=children><parameter=name>b</parameter></parameter></parameter>` (first item unclosed before the inner opener). Expected and asserted by name: one top-level item `{name:"a", children:[{name:"b"}]}`, `Resolved`, rules `nested-parameter`×2, `array-item`×2, `collect`×0 (single occurrence each), no `missing-parameter-close`. The limit (a sibling item intended by the model is read as a child when the item is unclosed and the item type declares the array's own name) is stated in `qwen3-coder-xml.v1.md` and the codec docs."
(If the maintainer wants the sibling reading instead, the row must say so and name the rule that grounds it; either way the expected value is written before the golden is captured.)

No PRE-EMPTIVE DESCOPE, UNMAPPED REQUIREMENT or PLANNED-BY-REFUSAL found. The plan's hedges at line 67 ("fix the re-rooting there if needed") and line 68 (unknown-tool caveat) sit under falsifiable rows (8 and the existing object-path behaviour) and are not rubric findings.

**Completion rubric result: FINDINGS-OPEN** (R1).

## What is sound (so it is not re-litigated)

- Task 1's mechanism is the right one and is grounded line by line: nesting is detected in `parameter()` before `reading()` (so `Unmodeled` items, which `verbatim()` treats as text, are caught); `pop()` is the single attach point; `owner()`/`boundary()` give sibling-opener and item-member scoping for free; `collect` already flattens `Value::Array` occurrences in source order, so JSON and nested items interleave; the three rules recorded are cost 0, non-guess, definition-grounded, so `Resolved` under `Structural` and `Effort::Exact` follows. Row 8 (per-occurrence pointers) correctly targets the `slots` same-`source` hazard.
- Task 2 reuses the existing failure path (diagnostics → `passed=false` → `Finish::Unresolved` → `NothingUsable` → empty emission), keeps `Forced(String)` byte-identical, counts `calls` of any outcome as `Required` does, excludes embedded calls as documented, and places flags before the name in the constraint string. The separate fuzz seed byte preserves existing regression seeds.
- Task 3 takes the inner start from `Pending::Marker.end` and the inner end from `think_closed`'s `start`, which are exactly the two values the recognizer already has, and defines all four closures. Not aligning by index is correct given `push_range`.
- Cross-cutting: the #6 fuzz baseline on `943fa04` before attributing failures, F15 truth citation for the new corpus mutant, and naming the API breaks are all right.

## Verdict

Blocking: findings 1, 2, 3, 4 and R1. Each is a bounded edit to the plan text; none changes the design.

VERDICT: NEEDS-REVISION

## Orchestrator dispositions (pass 1)

- 1 REQUIRED: addressed. The captured golden field is `reasoning_text`; the input key `reasoning` is untouched.
- 2 REQUIRED: addressed. Tasks 2 and 3 regenerate the verify-published fixtures with `--capture`, review the diff, and run `test-verify-published.py` as a focused check.
- 3 REQUIRED: addressed. All three API breaks are named.
- 4 REQUIRED: addressed. Whitespace means `u8::is_ascii_whitespace`, with a U+00A0 negative assertion.
- R1 REQUIRED: addressed. The row is rewritten verbatim from the review, with the expected value written before capture.
- 5 ADVISORY: adopted. `reasoning_blocks: [{span,text}]` is additive; `reasoning` is unchanged.
- 6 ADVISORY: adopted. One `tool_choice_strict` object on MCP and in the goldens; request error on a non-forced choice or a contradiction with `disable_parallel_tool_use`; the degenerate case renders `forced:NAME`.
- 7 ADVISORY: adopted. The fuzz invariants are conditioned on the drawn options; blocks are counted directly from the unmerged `reasoning_blocks`.
- 8 ADVISORY: adopted. A required-member assertion is added.
- 9 ADVISORY: adopted. Eight goldens, `PINNED_DIGEST` ~L732, no WASM re-pin expected.

## Recheck (pass 2)

Scope: the revised `plan.md` (155 lines), adjudicating findings 1-4, R1, adopted advisories 5-9, and the new text their fixes introduced (Task 3 `reasoning_blocks`, the `tool_choice_strict` MCP member, the degenerate-options rule, contract rows 9-10). Unchanged cleared text was not reopened. Nothing was executed; evidence is the revised plan text checked against the same source regions as pass 1.

| Finding | Status | Evidence in the revised plan |
|---|---|---|
| 1 (golden key collision) | **DISCHARGED** | Line 122: captured field is `reasoning_text`, added to `CAPTURED`; "the input key `reasoning` (the policy) is untouched". Row 14 (line 44) names `golden captured reasoning_text`. No remaining use of `reasoning` as a captured member. |
| 2 (verify-published fixtures) | **DISCHARGED** | Task 2 line 101 and Task 3 line 121 regenerate `scripts/data/verify-published/{mcp-responses.jsonl,call-response.json}` with `--capture` and bound the expected diff (Task 2: the `tools/list` input-schema member; Task 3: `reasoning_blocks` in the response and output schema). `python3 scripts/test-verify-published.py` is in both focused checks (lines 109, 133), so each task's commit carries fresh fixtures. |
| 3 (third API break) | **DISCHARGED** | Line 138 names `ToolChoice`, `ToolCalls` and `ReasoningSegment`, states none is `#[non_exhaustive]`, and commits to naming all three in the PR body. Line 113 states the field (`text: Range<usize>`). |
| 4 (whitespace) | **DISCHARGED** | Line 80: `u8::is_ascii_whitespace` enumerated; non-ASCII whitespace is content; U+00A0 named negative assertion; rule placed in the `StrictForced` Rustdoc and INTEGRATION.md. Line 88 ("Inter-call framing whitespace passes") and row 11 (line 41) are consistent with that definition. |
| 5 (per-block struct) | **DISCHARGED** (adopted; new text reviewed) | Lines 113-114, 119: `ToolCalls.reasoning_blocks: Vec<ReasoningBlock>` with `ReasoningBlock { span, text }`, one entry per block, unmerged, `reasoning` unchanged; wire `[{span, text}]` reusing the `range` `$ref`, required under `additionalProperties:false`. Row 14 asserts `reasoning_blocks[i].text` bytes for all five closures. Lines 115-118 keep the four closure rules. Line 154 records the decision with the correct reason. The merged-list example (line 114) is right: `0..32` outer, texts `7..8` and `23..24`. No new unfalsifiable text. |
| 6 (strict surfaces) | **DISCHARGED** (adopted; new text reviewed) | Line 96: one MCP member `tool_choice_strict: {exactly_one, allow_content, allow_reasoning}` (booleans, all required), request error when the choice is not forced or when `exactly_one:false` sits beside the Anthropic `disable_parallel_tool_use` flag. Line 98: golden key has the same shape. Line 97: CLI flags are a usage error without `--tool-choice NAME`. Line 81: all-lenient `ForcedStrict` renders exactly `tool_choice=forced:NAME`, consistent with line 79's "encodes the active options". The interactions pass 1 called undefined are now defined. |
| 7 (fuzz invariants) | **DISCHARGED** | Line 99: strict invariants conditioned on the drawn options (exactly one call iff `exactly_one`; no non-ASCII-whitespace content byte iff `!allow_content`; no reasoning range iff `!allow_reasoning`). Lines 123-129: block invariants are all computable from `ToolCalls` alone (ordered/disjoint; `text ⊆ span`; each `span` inside one `reasoning` range; union of block spans equals union of `reasoning` ranges; no marker bytes at `text`'s edges), and block counting is direct from the unmerged list. |
| 8 (required members in items) | **DISCHARGED** | New row 10 (line 38): named assertion that an item lacking `wearer` makes the call `Unresolved` (missing required / schema validation), never `Resolved`. |
| 9 (drifts) | **DISCHARGED** | Line 62 "eight goldens" matches the table (rows 1-7 and 9; rows 8 and 10 are named assertions). Line 120: `PINNED_DIGEST` ~L732; WASM transcript pins only emitted arrays, no re-pin expected, "its test is run to confirm". |
| R1 (row 9 unfalsifiable) | **STILL-OPEN** (narrowed; see below) | Line 37 now states the expected value, rules and limit before capture, which removes the unfalsifiable clause. But the schema it names cannot produce that expected value. |

### R1, still open: the row's schema contradicts its expected value

This defect is in my pass-1 `REWRITE TO:` text, which the plan adopted verbatim; the orchestrator's disposition is accurate and the fault is mine.

Line 37 reads: "schema `children: array of {name: string, children: <same array>}`". A *recursive* schema is expressible only as a `$ref` in OpenAI/MCP/Anthropic JSON and not at all through the Rust builder (`Param`/`ObjectSpec` build a finite value tree). A `$ref` item is projected as `ValueShape::Unmodeled` (INTEGRATION.md C5 "Schema-typed text": "a `$ref` to `$defs` … nested `<parameter=...>` elements read as an object's members"; prior-art §1.1; golden `schema_typed_ref_object_nested`). An `Unmodeled` item frame has scope `None` (`xml.rs` L936), which declares nothing (`declares()` L1664-1666), so at the inner `<parameter=children>` opener `owner("children")` (L819-826, `rposition`) finds only the top frame and `parameter()` L915-923 pops the open item with `missing-parameter-close`; a second item frame opens for `b`; the final `</parameter>` at top level is `DeleteStrayCloser` (L887-888). With the schema as written the walk yields the **sibling** reading `[{name:"a"},{name:"b"}]` plus `missing-parameter-close` and `stray-closer`, not the row's `{name:"a", children:[{name:"b"}]}`. Prior-art hazard 3 says exactly this: "Unmodeled item scopes (`None`) declare nothing, so they pop correctly." As written, the golden's named assertion would fail, and the implementer would have to change either the schema or the expectation without plan guidance.

With a non-recursive, inline two-level object schema the walk gives the row's expected value exactly: top frame declares `children` (Array(Object{name, children})); the first `<parameter=children>` nests (`nested-parameter` 1) with item scope declaring `name` and `children` (Array(Object{name})); `name` → `a`; the inner `<parameter=children>` has `owner` = the item frame (it declares `children`), so no pop, and it nests (`nested-parameter` 2) with scope `{name}`; `name` → `b`; `</parameter>` pops the inner frame as an item (`array-item` 1, `wrap_item`, adopted as the item's `children` member); `</parameter>` pops the outer item (`array-item` 2) into the top frame. Resolution sees one occurrence per `children`, each already a `Value::Array`, so `collect` records nothing (`resolve.rs` L2276-2279). Rules `nested-parameter`×2, `array-item`×2, `collect`×0, no `missing-parameter-close`, `Resolved`. The expected value, the rule counts and the documented limit in the row all stand; only the schema spelling is wrong.

Offending line 37 fragment:
> "schema `children: array of {name: string, children: <same array>}`"

`REWRITE TO:` "schema (inline, non-recursive, in `tools_openai`): `children: {type: array, items: {type: object, properties: {name: {type: string}, children: {type: array, items: {type: object, properties: {name: {type: string}}, required: [name]}}}, required: [name]}}` — no `$ref`, so both item scopes are modeled `Object` shapes and the inner `children` opener is owned by the open item. (A recursive `$ref` item is `Unmodeled`, declares nothing, and takes the sibling reading with `missing-parameter-close`; the dialect doc states both, and no golden for the `$ref` form is required.)"

Rubric ground: the row is no longer UNFALSIFIABLE; it is now falsifiable but unsatisfiable as specified, which leaves the criterion unable to serve as acceptance. It stays REQUIRED because it is the one row whose expected value the implementer cannot reach without a decision the plan does not record. The fix is one phrase.

### New text introduced by the fixes: checked, nothing further

- Task 3 (lines 113-133): `ReasoningBlock` is a new public type and will need the `tool_call` re-export; that is implementation detail, not a plan gap. The `reasoning_text` golden capture name beside the `reasoning_blocks` wire name is deliberate (the golden captures text, as `content` does) and is not a collision.
- Task 2 (lines 80-81, 95-101): consistent with `tool_calls.rs` L175-187 routing one `tool_choice` parser; the contradiction rule covers the only ambiguous combination.
- Rows 9-10 (lines 37-38): row 10 is falsifiable as written; row 9 is covered above.

**Completion rubric result (pass 2): FINDINGS-OPEN** (R1 only, narrowed to the schema spelling).

Still open: R1 (one-phrase schema fix in row 9, text supplied above). Everything else is discharged.

VERDICT: NEEDS-REVISION

## Recheck (pass 3, final)

Scope: R1 only, against `plan.md` line 37 as revised.

**R1: DISCHARGED.** Line 37 now carries, in order: the inline non-recursive schema verbatim from the pass-2 `REWRITE TO:` (`children: {type: array, items: {type: object, properties: {name: {type: string}, children: {type: array, items: {type: object, properties: {name: {type: string}}, required: [name]}}}, required: [name]}}`, declared in `tools_openai`, no `$ref`); the statement that both item scopes are therefore modeled `Object` shapes and the open item owns the inner `children` opener; the `$ref` note (an `Unmodeled` item declares nothing and takes the sibling reading with `missing-parameter-close`; the dialect doc states both cases; no `$ref` golden required); the input; the expected value `{name:"a", children:[{name:"b"}]}`, `Resolved`; the rule counts `nested-parameter`×2, `array-item`×2, `collect`×0, no `missing-parameter-close`; the documented limit; and "The expected value is written before capture."

The pass-2 walk-through of that exact schema through `parameter()` (`xml.rs` L915-955), `pop()` (L829-840 as the plan extends it), and `collect` (`resolve.rs` L2276-2279) produces precisely the stated value and counts, so the row is now falsifiable and satisfiable as specified, and the criterion can serve as acceptance. No rubric ground applies.

Cosmetic, not a finding: the row contains "is required., input" (a stray period before the comma). It does not affect any assertion.

**Completion rubric result (pass 3): PASS.** No findings open.

VERDICT: PASS
