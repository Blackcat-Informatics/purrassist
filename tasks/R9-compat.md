# R9 compatibility list: head f93f0b4 against main 943fa04 (PR #9 / issue #8)

Derived from `git diff 943fa04 -- crates/core/src crates/host/src crates/cli/src`, cross-checked
against `remediation-plan.md` (R2, R5, R11 accumulated items) and `validation.md`.
No public type lost a derive or a trait impl. The public enums and structs this diff touches are not `#[non_exhaustive]` (that attribute exists only on `Dialect`, `MarkupFamily` and `DeclarationForm`), so every added variant or field below is a source break for exhaustive matches and struct literals.
`tool_call.recover.v1` is pre-0.1.0: additive members may appear.

## A. Rust API breaks (source-incompatible for downstream code)

A1. `ToolChoice` gains the variant `ForcedStrict(StrictForced)`. An exhaustive `match` no longer compiles.
A2. `UnresolvedReason` gains `ToolChoiceCardinality` (`code()` is `tool_choice_cardinality`, with a `Display` text). Exhaustive matches break.
A3. `EditOperator` gains `UnmodeledScopeBoundary` (`tool_call.markup.unmodeled-scope-boundary`, appended last); `grammar::edit::OPERATOR_COUNT` is 46 (was 45) and `OPERATORS.len()` is 46. Exhaustive matches and code that hardcodes 45 break.
A4. `ToolCalls` gains the pub field `reasoning_blocks: Vec<ReasoningBlock>`. Struct-literal construction breaks. The doc of `ToolCalls::reasoning` changes: adjacent ranges are merged (`<think>a</think><think>b</think>` is `0..32`); the per-block view is `reasoning_blocks`.
A5. `ReasoningSegment` gains the pub field `text: Range<usize>` (marker-free text range; invariant `text.start <= text.end`, `text` inside `span`). Struct literals break.
A6. `ArgumentShape` gains `scopes: ScopeGraph` and `ParameterShape` gains `nesting: Nesting`. Struct literals break.
A7. `ArgTree` gains the pub field `collected: Vec<CollectedMember>` and a private field, so external struct-literal construction no longer compiles. Derives are unchanged (the private running state never takes part in equality).
A8. `ArgTree::value` and `ArgTree::empty_object` are no longer `const fn`.
A9. `ToolCalls::usable()`, `usable_accepting_guesses()` and `emitted_calls()` return an empty list when `tool_choice.passed` is false (a violated tool choice means nothing of the turn runs). `emitted_calls()` is now exactly `usable_accepting_guesses()`. Callers that read `usable()` beside a failed check used to get the `Resolved` calls.
A10. `ToolChoice::from_anthropic` accepts `disable_parallel_tool_use` on `{"type":"tool",...}`: `true` is `ForcedStrict { exactly_one: true, allow_content: true, allow_reasoning: true }`, `false` is `Forced`. A non-boolean value, or the member on `auto`/`any`/`none`, is a `ToolChoiceError`. It used to be an error for every type.
A11. Request error: `ForcedStrict { allow_reasoning: false }` with `ReasoningPolicy::AsContent` is an `invalid_input` outcome (reasoning is never recognized, so the option could not be enforced).
A12. Published constants change value (see section D): `cost::RECOVER_WORK_PER_BYTE`, `cost::RECOVER_SCRATCH_PER_BYTE`. `ToolRecoveryRequest::sized_budget` / `sized` / `sized_for` also grow `evidence_entries` (signatures unchanged).

## A'. Rust API additions (source-compatible)

A13. `tool_call::StrictForced { name, exactly_one, allow_content, allow_reasoning }` (`Clone, Debug, PartialEq, Eq`) with `exactly_one(name)`, `allow_content(bool)`, `allow_reasoning(bool)`, `with_exactly_one(bool)` (the last three `const fn`).
A14. `tool_call::ReasoningBlock { span, text }` (`Clone, Debug, PartialEq, Eq`).
A15. `tool_call::CollectedMember { pointer, key_span, occurrences }` (`Clone, Debug, PartialEq, Eq`).
A16. New public module surface `tool_call::{Nesting, ScopeGraph, ScopeMember, ScopeNode}`. `Nesting` is `None | Object(usize) | ArrayOfObject(usize)` (`Default` is `None`). `ScopeMember { name, nesting, array }`. `ScopeNode::{members, member, declares, is_open, is_opaque}`. `ScopeGraph::{node, len, is_empty}`.
A17. `cost::RECOVER_EVIDENCE_BYTES_PER_ENTRY: u64 = 7`.
A18. Docs: `Budget::depth` documents the stack guard and the `(depth - 1) / 3` nested-item capacity; `ToolChoice::constraint` documents the `forced[...]` family; `ToolCalls::usable` has a doctest.
A19. Internal only (no API change): `ScratchOrdMap` is `pub(crate)`; `NodeId`'s counting impls exist only under `cfg(test)`; `Derivation` gains a private `seen` set (equality ignores it); `Forest` keeps its public API; `RootLocations`, `RootPlan`, `Chained`, `HeldSet`, `HeldFailures`, `DataIndex`, `Extras` are crate-private.

## B. Wire and schema (`tool_call.recover.v1`, MCP tool `recover_tool_calls`)

B1. `value.reasoning_blocks` is a new member and is **required** in the output schema (`additionalProperties: false`, so a client validating against the old output schema rejects the new result): `[{"span": {start, end}, "text": {start, end}}]`, one entry per block, never merged. `value.reasoning` keeps merged ranges.
B2. Constraint string family `tool_choice=forced[exactly-one,no-content,no-reasoning]:NAME` (options listed in that order, only the active ones; name last so `:`, `;` and `,` in a name stay unambiguous). A strict choice with every option lenient renders `tool_choice=forced:NAME`, as before. This string is a receipt `validations[].constraint` and `value.tool_choice.constraint`.
B3. New per-call unresolved reason code `tool_choice_cardinality`: each call after the first beneath an `exactly_one` choice.
B4. `value.tool_choice.diagnostics` text for a strict choice: one cardinality diagnostic that states the extra-call count and the rejected-attempt count (capped, not one per entry), plus one per other failed option. Rejected call attempts of any reason (an `unsupported_dialect` included) count against `exactly_one` and `allow_content: false`.
B5. `value.usable_calls` and `value.usable_calls_accepting_guesses` are 0 when `tool_choice.passed` is false, so they agree with `verdict: nothing_usable` and the empty emission. They used to be non-zero beside it.
B6. A failed strict check is merge-independent: when readings merge, the merged reading fails if any member fails, with the deduplicated union of diagnostics.
B7. Evidence pointers of a repeated array parameter number items across all occurrences (`/xs/0`, `/xs/1`, ... where each occurrence used to restart at 0). Alias and canonical spellings of one parameter share one sequence; leaves keep the written spelling. `decode-typed` evidence cites each item's own span (item 1 used to cite item 0's bytes).
B8. `tool_call.parameter.collect` is now also recorded for occurrences merged under a `$ref` or `anyOf` scope the typed shape does not declare (one entry per merged member, citing the first occurrence's name span).
B9. The tie order of ambiguous readings (`alternatives`, `readings`, `ledger_rules`) follows the digest of the wire JSON, which now includes `reasoning_blocks`; two goldens reordered. Order among equal-cost readings can differ from main.
B10. New rule id `tool_call.markup.unmodeled-scope-boundary` (cost 2, guess, multiple readings, syntax grounding) in the rule registry and `docs/tool-calls/RULES.md`; 46 operators.
B11. Receipt `budget` and `usage` numbers differ from main for the same input: the published per-byte constants changed (section D) and charging was made linear and complete (R5, R6, R10, R11, R13, R14, R15). Decisions, values and spans are byte-identical apart from `usage` (differentially verified); the published transcript and the receipt fixture were refreshed.
B12. MCP input schema: new optional member `tool_choice_strict` (`{"exactly_one", "allow_content", "allow_reasoning"}`, three booleans, all required, valid only beside a forced `tool_choice`); `tool_choice` additionally accepts the Anthropic `disable_parallel_tool_use` boolean for `type: "tool"`. The description text of `tool_choice`, `value.usable_calls` and the tool itself changed.
B13. MCP tool errors: a malformed `tool_choice_strict`, `tool_choice_strict` beside a non-forced choice, and `exactly_one: false` beside `disable_parallel_tool_use: true` are `invalid_input` tool errors.
B14. `tools_format` parameter schemas whose root is a `$ref`, `allOf` of one schema, or `anyOf`/`oneOf` of objects now project their parameters; the adapter's render-back is byte-exact in all three wire forms (prompt forms print the exact schema and are unchanged).

## C. Behaviour

C1. The feature (issue #8): a repeated `<parameter=NAME>` whose body holds nested `<parameter=` elements is read as object items of an array-of-object parameter, instead of sibling parameters or a missing close.
C2. `$ref`/`anyOf`/`oneOf`/`allOf` item and parameter scopes declare their schema's member names (scope graph): recursive trees nest, and a nested member that collides with an outer name is no longer hijacked by the outer scope. Main's wrong `Resolved` reading of balanced recursive trees is replaced by the nested reading (golden `xml_array_recursive_ref_items_nest` replaces `..._are_siblings`).
C3. Free-form object parameters and items (`additionalProperties: true`, no properties, or an unmodeled `$ref`/`anyOf` declaring no name) close at an enclosing scope's opener by the guess rule `tool_call.markup.unmodeled-scope-boundary` (a best guess that never runs on a mutating tool) instead of `missing-parameter-close`.
C4. Strict forced tool choice (`exactly_one`, `allow_content`, `allow_reasoning`) is new and fail-closed: a failed check makes the turn `Unresolved`, emits nothing, and exits 4.
C5. Strict `allow_reasoning: false` with reasoning `content` is `invalid_input` (exit 4; MCP `invalid_input`), not a pass.
C6. Anthropic `disable_parallel_tool_use: true` is accepted and means exactly one call; it used to be rejected as an unrepresentable member.
C7. Marker-free reasoning: a reasoning closer is recognized only at or after the block's text start, so `text.start <= text.end` always holds (Anthropic `redacted_thinking` with a `"}` closer overlapping the opener used to give `39..38`). Redacted-thinking `text` is the opaque encrypted payload, not prose.
C8. A reasoning-marker spelling inside a JSON string of a repeated or alias parameter stays value text (p12: exit 4 with `{}` on main, now Resolved with the literal value and `reasoning: []`).
C9. Root `$ref`/`allOf`/`anyOf` declarations project parameters (a Draft-07 root `$ref`, the FastMCP/pydantic shape, used to project none): typing and `required` now come from the followed schemas; a name declared by a union branch or by several schemas stays schema-typed and not required. Local `$ref` only, 64 hops, cycle-checked. Draft-07 ignores a `$ref`'s siblings; 2020-12 applies them. Validation still uses the exact schema.
C10. `Budget::depth` charges the frame being pushed (one level fewer than main admitted): the `xml-params` codec fits `depth - 1` object levels and a whole recovered call about `(depth - 1) / 3` nested item levels (about 42 at the CLI/MCP value 128); deeper is the explicit `BudgetExceeded(Depth)`.
C11. Mistral v11 chained calls and Mistral v3 call arrays are linear in time and charged work with identical decisions.
C12. Documented non-covered shapes still end in explicit exhaustion or are slow: restarted DeepSeek-V3.1 sections (issue #10, same on main) and a mutated free-form-item repair search (issue #11, same on main).

## D. Resources and budgets

D1. `cost::RECOVER_WORK_PER_BYTE` 32 -> **176**.
D2. `cost::RECOVER_SCRATCH_PER_BYTE` 48 -> **368**.
D3. New `cost::RECOVER_EVIDENCE_BYTES_PER_ENTRY` = **7**: the sized budget adds `ceil(n / 7)` evidence entries. Margins on the dense-markup maxima are 25-27% (work, scratch) and 41% (evidence); the DeepSeek scratch margin is 22%.
D4. The sized default budget now recovers dense repeated markup (about 1,000 nested items used to exhaust Work; k = 8k repeated parameters, 1 MiB of Mistral v11/v3 calls and 1 MiB of each dialect recover under `sized`). Exhaustion stays an explicit `BudgetExceeded`.
D5. Repeated parameter occurrences cost linear time and scratch (R10; wall time at k = 8000 nested items 9.0 s -> 1.8 s; scratch 13.99 GB -> 1.54 GB).
D6. Default-budget ceiling for one-property root-union declarations: about 670 -> about 610 branches (about 10% constant overhead of the root projection); exhaustion is explicit.
D7. Accepted and documented: call-free brace-heavy text costs 895 work per byte (G23).

## E. CLI and MCP

E1. CLI `recover-tool-calls` flags `--tool-choice-exactly-one`, `--tool-choice-no-content`, `--tool-choice-no-reasoning`: each at most once, only beside `--tool-choice NAME` (a tool name, not `auto|none|required`), else a usage error (exit 1). They set `tool_choice_strict`. `--help` text changed.
E2. CLI/MCP exit and verdict: a failed strict check is exit 4, `passed: false`, verdict `nothing_usable`, `usable_calls` 0, nothing emitted (p7 with all flags was exit 0 and `run` on the prior head).
E3. MCP `tools/list` changes: `tool_choice_strict` input member and extended `tool_choice` description; output schema requires `value.reasoning_blocks`; the `recover_tool_calls` description says `usable_calls` is zero when the check fails.
E4. The published replay (`scripts/data/verify-published`) exercises reasoning and strict choice (two new requests, ids 20 and 21); its transcript carries the new budget numbers and descriptions.

## F. Not integrator-visible (listed for completeness)

F1. Build tooling: Node pin v26.11.1 (`vendor/tooling.json`, notices directory renamed).
F2. Fuzzing: target `tool-call-ref`, `--continue-on-known`, new seeds, `make fuzz` loop; `fuzz-tool-call --tools ref`.
F3. Goldens: 481 files re-blessed for `reasoning_text`; `forced_*` goldens' `usable_calls` 1 -> 0; many new goldens; corpus 18,176 cases, 0 incorrect.
