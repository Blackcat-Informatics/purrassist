# Remediation plan review: PR #9 / issue #8 (Stage 2)

**Reviewed:** `.stage/tool-call-recovery-arrays-of-nested/remediation-plan.md`
**Against:** `gap-analysis.md`, `completion-audit.md`, `reviews/security-review.md`, `reviews/performance-review.md`, `historical-fit.md`; contract `issue.md`, `plan.md`, `validation.md`; rules `AGENTS.md`, `.goals`, `docs/IMPLEMENTATION.md`, `docs/tool-calls/INTEGRATION.md`.
**Source read:** `crates/core/src/grammar/codec/xml.rs` (`Frame`, `boundary`, `owner`, `pop`, `parameter`, `cheapest`, `nesting`, `declares`), `grammar/edit.rs` (operator table), `tool_call/{shape.rs,toolset.rs,adapter.rs,ir.rs,recover.rs,recognizer.rs,resolve.rs}`, `examples/fuzz_tool_call.rs`, `scripts/fuzz.py`, `scripts/check-wasm.py`, `scripts/verify-published.py`, `vendor/tooling.json`, `docs/RESOURCES.md`, `docs/tool-calls/dialects/qwen3-coder-xml.v1.md`.
**User decision honoured by the plan:** "you may not modify the local system. just change the node version pinned to match ours." (R1 repins; no install.)

## Coverage: every finding in the five sources, mapped

| Source | Finding | Plan item | Status |
|---|---|---|---|
| gap-analysis | G1 Node pin | R1 | mapped; see F11 (descope of legs) |
| gap-analysis / audit F1 / security R1 | G2 Unmodeled scopes | R2 | mapped; design insufficient, see F1, F2 |
| gap-analysis / audit F2 / security A1 | G3 strict vs rejected attempts | R4 | mapped; merge rule not concrete, see F5 |
| gap-analysis | G4 fuzz cannot reach item path | R7 | mapped; pre-emptive descope, see F9, F10 |
| gap-analysis / perf REQUIRED 1 / security A4 | G5 O(k²) adopt | R5 | mapped; test contradicts the declined item, see F7 |
| gap-analysis | G6 `decoded()` by pointer | R5 | mapped, falsifiable (p6 spans) |
| gap-analysis | G7 marker in JSON string of repeated parameter | R6 | mapped, falsifiable (p12, p12j) |
| gap-analysis | G8 `calls_key` untested | R8 | mapped, falsifiable |
| gap-analysis | G9 fuzz trailer claim | R7 | mapped, falsifiable |
| gap-analysis | G10 compatibility note | R9 | mapped; list is stale against R2-R6, see F13 |
| gap-analysis | G11 redacted thinking docs | R3 | mapped |
| gap-analysis | G12 hosted CI | reported | legitimately external |
| gap-analysis | G13 published replay | R9 | mapped; `--capture` works offline from the workspace CLI |
| audit F3 | Node pin unapplied | R1 | mapped; working tree already holds the change, see F11 |
| security R2 | reversed `text` range | R3 | mapped; golden expectation is either/or, see F12 |
| security A2 | `allow_reasoning:false` under `AsContent` | R4 (G15) | mapped; probe and seed interaction missing, see F6 |
| security A3 | `usable_calls` with failed strict check | none | **UNMAPPED**, see F4 |
| security A5 | alias vs canonical numbering | R5 | mapped |
| security A6 (off-by-one) | Depth charge | R2 (G16) | mapped; unfalsifiable as written, see F3 |
| security A6 (second part) | `Budget::depth` is the stack guard, Rust API doc | none | **UNMAPPED**, see F4 |
| perf ADVISORY 2 | per-item pointer allocation uncharged | none | unmapped (pre-existing, minor), see F4 |
| perf ADVISORY 3 | pre-existing quadratic; add cost case | Declined + R5 test | partly mapped; see F7, F8 |
| perf ADVISORY 4, 5 | informational, no action | none | no action required |
| perf ADVISORY 6 | strict diagnostics uncharged/unbounded | R4 (G17) | mapped |
| historical-fit risks 1-16 | all adjudicated in gap-analysis | via G-items | mapped; risk 5 (depth exhaustion through item frames) has no named test, see F3 |
| Declined: #6 oracle failure | tracked by #6, attributed by base replay | Declined | legitimately out of scope |
| Declined: pre-existing quadratic | not in the diff | Declined | out of scope for the fix; tracking and documentation required, see F8 |

Entry points: R2, R3, R4, R5, R6 check through the built CLI (`recover-tool-calls`), R4 also through MCP; R7 through the fuzz probe; R8 is library-level by nature. Good.

## Findings

### REQUIRED

**F1. G2's design (name sets for Unmodeled scopes) cannot produce the golden it promises; the data model is unspecified and self-contradictory.**
Quoted: "an Unmodeled scope declares them for scoping only: `declares`, `owner`, `boundary` and `cheapest` pricing"; "Typing stays schema-typed (`tool_call.value.schema-typed`), unchanged."; "The name sets are computed when tools are compiled"; "Recursive references resolve lazily per opened frame. They are never expanded eagerly".
Evidence: `xml.rs:957-961` derives a member's `declared` shape from `scope.parameter(canonical).map(|p| &p.value)`; `xml.rs:970` decides nesting with `nesting(declared)`. A scope that carries only names yields `declared = None` for every member, so `nesting(None) = None`. Trace of p9r under the plan's design: top-level `children` is `Array(Unmodeled)` and opens an item frame whose scope declares `{name, children}`; `<parameter=name>a</parameter>` binds; the inner `<parameter=children>` is owned by the item frame (good) but is read as a **verbatim string value**, not a nested frame; its value ends at the `<parameter=name>` boundary (`missing-parameter-close`), `name` repeats inside the item, and the final `</parameter>` is a stray closer. The result is neither `{"children":[{"name":"a","children":[{"name":"b"}]}]}` nor the promised rule set (`nested-parameter`x2, `array-item`x2, no `missing-parameter-close`). Nesting needs, per member of an Unmodeled scope, the member's own nesting kind (object / array-of-object / opaque) and its scope, recursively. The plan also says both "computed when tools are compiled" and "never expanded eagerly" without saying how a cyclic `$ref` graph is represented behind `Frame<'t>::scope: Option<&'t ObjectShape>` (`xml.rs:359-368`) and the `const fn nesting(Option<&ValueShape>)` signature (`xml.rs:1656`), nor what happens to the public unit variant `ValueShape::Unmodeled` (`shape.rs:201`, asserted by doctests).
`REWRITE TO:` "Compile, once per tool, a finite **scope graph** from the exact schema: one node per distinct schema location (`$defs`/`definitions` entry or inline object/branch union), each node holding its accepted name literals plus, per member, `(nesting: None | Object(node) | ArrayOfObject(node), typing: unchanged)`. Cycles are back-edges by node index (no unrolling); cost is bounded by the schema's size and charged at compile time. `Frame.scope` becomes an enum `{Declared(&ObjectShape), Schema(&ScopeNode), Opaque}`; `nesting` takes the member's scope entry, so the inner `children` of a `$ref` item opens an item frame on the same node. `declares`/`owner`/`boundary`/`owners`/`cheapest` consult the enum. `ValueShape::Unmodeled` stays a unit variant; the graph lives beside `ArgumentShape` (or on `ParameterShape`) so the public `ValueShape` API and its doctests do not change (if it must change, R9 lists the break). Depth is charged per frame push (existing), which bounds traversal of cycles at read time." Keep the acceptance checks (p9r, `--effort exact`, p10_ref_plan, rm/keep) as written; add the rule-list assertion from gap-analysis acceptance 1.

**F2. The opaque-scope guess is unimplementable as one of its two options, and its golden is unfalsifiable (UNFALSIFIABLE CRITERION).**
Quoted: "Record it with a guess-flagged operator: a new `EditOperator`, or `MissingParameterClose` with the guess flag when the frame's scope is unknown."; "Add `xml_unmodeled_opaque_scope_close_is_a_guess`: an opaque item with a colliding name -> BestGuess, never Resolved."; check: "The opaque case gives `best_guess`."
Evidence: `OperatorRule.guess` is a static column of the `const OPERATOR_TABLE` (`edit.rs:415-419`), and RULES.md has one row per rule id; `MissingParameterClose` cannot be guess-flagged conditionally. `cheapest` (`xml.rs:1368-1372`) prices every pop at `MissingParameterClose` or `UndeclaredParameterBoundary` by the top-level scope alone, so the guessed pop through an opaque frame is unpriced. The golden names no input and no expected arguments; "gives `best_guess`" cannot fail for the wrong reading (hoisted `{"keep":[{}],"path":...}` as BestGuess versus `{"keep":[{"path":...}]}` as BestGuess are both "best_guess").
`REWRITE TO:` "Add `EditOperator::UnmodeledScopeBoundary` (`tool_call.markup.unmodeled-scope-boundary`, cost 2, guess yes, readings multiple, grounding syntax, repair yes); `OPERATOR_COUNT` 46; RULES.md row; one golden per rule (IMPLEMENTATION.md). `cheapest`'s `settle` prices pops through `Opaque` frames at this operator's cost. Input for the golden: `rm(path: string, keep: array of {"type":"object","additionalProperties":true})` with `<parameter=keep>\n<parameter=path>\n/home/user/important\n</parameter>\n</parameter>`. Expected: status `best_guess`, arguments `<state them>`, inference `unmodeled-scope-boundary`, verdict `nothing_usable` under the default `Mutating` effects, and the alternative reading `<state it>` in `alternatives` or recorded as the single guessed reading (decide, and justify against the balanced closer being evidence against the pop, security R1 fix 3)."

**F3. G16 and historical-fit risk 5 have no falsifiable check (UNFALSIFIABLE CRITERION).**
Quoted: "**G16:** the Depth charge counts the frame being pushed; no extra level is admitted." No test is named.
`REWRITE TO:` "Named test in `tool_call_recover.rs` (or `tool_call_cost.rs`): `Budget { depth: d, .. }` with `ToolSet::recover` on nested array items `d` levels deep resolves; `d + 1` levels returns `BudgetExceeded(Depth)` explicitly (no string fallback, no panic). The same test is the executable evidence for historical-fit risk 5 and security P1."

**F4. Findings dropped from the mapping (UNMAPPED REQUIREMENT).**
Quoted header: "advisories A1=G3, A2=G15, A4=G5, A5=G5, A6=G16". Security **A3** (`usable_calls: 1` beside `verdict: nothing_usable`; MCP description tells clients to read both) is absent from the gap list and from every R-commit. Security **A6's second part** ("noted for the Rust API documentation (`Budget::depth` is the stack guard)") is absent. Performance **ADVISORY 2** (pointer allocation per item uncharged) is absent.
`REWRITE TO:` add rows: "A3 -> R4: either zero `usable_calls`/`usable_calls_accepting_guesses` when `tool_choice.passed` is false (and update INTEGRATION.md ~L1243 and the fuzz `check_verdict`), or change the `recover_tool_calls` registry description to say `verdict` alone gates execution; pick one, with a named host test." "A6-doc -> R2: one sentence on `Budget::depth` Rustdoc: it is the stack guard for recursive schema evaluation; MCP/CLI fix it at 128." "perf ADVISORY 2 -> Declined (pre-existing, O(1) per item, bounded by charged items) with that reason stated."

**F5. R4's merge-independence requirement is not concrete and neither offered option guarantees independence.**
Quoted: "Either include the strict check's verdict in the reading key, or re-run the check after `same_calls` on the surviving merged reading. The implementer picks one and documents why."
Evidence: the strict check runs per reading at `recover.rs:3606`; `same_calls` at `:4542` keys on calls/embedded/blocks/messages only (`calls_key`, `:4942-4968`); `merge` (`:5005-5057`) keeps the member with the largest `body_evidence` and copies its `tool_choice` unchanged, discarding the others' results. After R4's fix, under Anthropic `disable_parallel_tool_use` (exactly-one, content allowed) a member that reads the second attempt as `rejected` fails while a member that reads the same bytes as `content` passes; "re-run on the survivor" still depends on which survives. Option 1 (verdict in the key) stops merging such readings and can turn one-reading turns `Ambiguous`, a behaviour change the plan does not assess against goldens.
`REWRITE TO:` "Fail closed in `merge`: the merged reading's `tool_choice.passed` is false if any member's is false; its diagnostics are the deduplicated union (then capped per G17); extra-call `ToolChoiceCardinality` outcomes come from the failing member. Named test: an output whose second `<tool_call>` body is contested by two dialects (one reads a rejected attempt, the other content) under `forced[exactly-one]` with content allowed, asserting `passed:false` and identical diagnostics regardless of reading order. Fuzz invariant in `check_strict`: `passed` implies `rejected.is_empty()` when `exactly_one` or `!allow_content`."

**F6. G15 collides with the fuzz probe and the existing strict seeds; the plan does not say how.**
Quoted (R4): "`ForcedStrict { allow_reasoning: false }` under `ReasoningPolicy::AsContent` is a request error (`invalid_input`)"; (R7): "with existing seed meaning preserved".
Evidence: every `TOOL_CALL_STRICT` seed starts with control byte `0xe6` (`fuzz.py:878-882`); bits 4-5 of `0xe6` are `10`, which `request()` maps to `ReasoningPolicy::AsContent` (`fuzz_tool_call.rs:205-210`). Seed 4 (`...\xfc`, "reasoning, no reasoning") therefore draws exactly the combination G15 turns into a request error, and today it never tests no-reasoning at all. The probe calls `tools.recover(output, &request)` (`:1684`) and expects the completion line `ok`.
`REWRITE TO:` "R4 states the probe's behaviour for the invalid combination (assert the request error variant, print `ok`), re-encodes seed 4 with a Separate reasoning control byte (for example `0xc6`) so it tests what its comment claims, adds one seed that draws the error combination, and `test-fuzz.py` asserts both run `ok`. Record the seed change as a meaning correction, not a preservation."

**F7. R5's cost test asserts a property the plan declines to deliver.**
Quoted (R5): "Assert that the charged work/scratch ratios track the byte ratio (within the file's existing tolerance)"; (Declined): "Pre-existing quadratic wall time and charged scratch for many repeated parameters on base 943fa04 ... It is unchanged by this PR".
Evidence: the performance review measured `total_scratch_charged` 0.55G -> 20.0G for k 1000 -> 8000 (bytes 87k -> 710k): a 36x scratch ratio for an 8.2x byte ratio, before and after the PR. The test as written fails on the code the plan ships. The second bullet ("A unit or bench test of `ArgTree::from_parameters` at k=32k completing under a generous bound. Or document why only the charged ratio is asserted.") is a wall-time assertion the previous bullet forbids, with an escape clause.
`REWRITE TO:` "Assert `work` ratio ~ byte ratio (linear). Assert `total_scratch_charged` ratio <= (k ratio)^2 x tolerance, with a comment naming the tracked issue from F8. Replace the k=32k wall-time bullet with a deterministic check: a `#[cfg(test)]` counter of members visited by `adopt` (or an equivalent probe) that is O(k) at k=32k, or charge the per-adopt step to `Resource::Work` and assert the charged ratio."

**F8. The declined pre-existing quadratic is dropped without tracking or documentation (PLANNED-BY-REFUSAL).**
Quoted: "It is reported to the user as a residual; no follow-up issue is created to stand in for #8 work."
Evidence: this is not #8 work, so a tracked issue is the right home, and refusing one leaves the defect only in a chat residual. It limits the capability #8 ships: arrays of objects are *always* repeated parameters in this dialect, and security P4 shows default budgets exhaust at roughly 600 items (performance review: near k=1000). RESOURCES.md L757-763 states every limit stays "finite and linear in `n`" and is silent on repeated parameters; AGENTS.md requires documentation to distinguish implemented behaviour and capability claims to have evidence. Declining the **fix** inside #8 is acceptable; declining to record it is not.
`REWRITE TO:` "Open a tracked issue (or `.deficiencies` entry) carrying the performance review's table and the P4 numbers. R9 adds one sentence to `docs/RESOURCES.md` and `qwen3-coder-xml.v1.md`: wall time and charged scratch grow quadratically in the number of repeated parameter occurrences; under default budgets a few hundred items exhaust explicitly (`BudgetExceeded(ScratchBytes)`). The fix stays out of #8."

**F9. R7's second toolset has no viable selector; "unused header bit" does not exist and would break the seed-meaning promise.**
Quoted: "Add a `$ref`-item declaration tool if the fixed `ToolSet` can mix them. Otherwise add a second probe toolset selected by an unused header bit, documented, with existing seed meaning preserved."
Evidence: `ToolSet` offers `compile` (builder) and `from_openai`/`from_mcp`/`from_anthropic` (adapters) with no combining API (`toolset.rs:566`, `adapter.rs:468-543`); only adapters produce `Unmodeled`. The control byte is full (`fuzz_tool_call.rs:14-20`); split bits 0-1 are effort, bit 2 ambiguity, bits 3-7 feed `check_chunking(split >> 3)` (`:1689`). Every bit is in use.
`REWRITE TO:` "Either (a) add a public `ToolSet` API that combines builder-compiled and adapter-compiled tools (listed in R9 as an additive API change, with Rustdoc and a doctest), and add the `$ref` tools to the fixed set; or (b) give the probe an argument (`fuzz-tool-call --tools ref`) selecting a second fixed toolset, registered in `scripts/fuzz.py` as its own target with its own seeds and strict trailer handling, leaving the existing target and seeds byte-for-byte unchanged. State which."

**F10. R7 pre-authorises a weaker evidence standard for G4 (PRE-EMPTIVE DESCOPE).**
Quoted: "If the #6 oracle failure stops the campaign first, the failing input is replayed against a base-built probe for attribution, and the coverage claim is instead established by running the new seeds alone under coverage. That is recorded honestly."
Evidence: G4 is "zero fuzz executions" on the new path; the probe has no switch to isolate the F4 oracle (`fuzz_tool_call.rs:292-513, 943`), and the campaign aborts near 1000 executions. Seeds under coverage prove the seeds execute the branches, not that mutation reaches them; this is the historical-fit risk 7 restated, not closed.
`REWRITE TO:` "Isolate the #6 oracle for this campaign: `scripts/fuzz.py` gains a documented option to continue past a failure whose panic message matches the #6 signature (`rejected attempt ... runs past where it failed` / `beyond a literal past its end`), retaining every such input for #6; or the probe gains a documented `--without-f4` argument used only by this campaign and never by `make fuzz`. Report mutated executions that reached `xml.rs:857-865` and `:1660-1663`. The seeds-only fallback is permitted only after isolation is shown infeasible, and is then recorded in `validation.md` as G4 still open."

**F11. R1 pre-authorises stopping `make wasm-test` legs for a step that does not touch the local system (PRE-EMPTIVE DESCOPE), and ignores the working-tree state.**
Quoted: "`make wasm-test` exits 0, with the native, WASI and unknown-unknown transcripts identical, including the syntax/host legs. If a leg needs a sysroot download into `.cache`, I record it and stop that leg; I do not install system tooling."
Evidence: `scripts/check-wasm.py:51-59` fetches the digest-verified WASI sysroot into `ROOT/.cache/wasi` itself; this is the repository's own cache and documented workflow, not a system install, so the user decision does not forbid it. The two sentences contradict each other. The completion audit already ran all legs this way (exit 0). Also, the working tree already holds the repin: `vendor/tooling.json` reads `26.11.1` with the new `binary_sha256`, and `vendor/licenses/node-26.11.1/LICENSE` exists, uncommitted.
`REWRITE TO:` "Run `make wasm-test` to completion; the `.cache/wasi` sysroot fetch is the script's own step and is in scope. R1 commits exactly the repin diff already present in the working tree after `git status` shows nothing else, and `python3 scripts/check-repository.py` confirms the notice hash."

**F12. R3's golden expectation is either/or (UNFALSIFIABLE CRITERION).**
Quoted: "Expected: no reversed range; the block is either unterminated (text to end of output) or closed at a non-overlapping closer. The exact value is hand-derived before capture."
Evidence: the opener `{"type": "redacted_thinking", "data": "` is 39 bytes; the only `"}` starts at 38, which the fix (`start < think.text_start` in `think_closed`, `recognizer.rs:1672`) rejects; no later closer exists.
`REWRITE TO:` "Expected: one block, span `0..40`, text `39..40` (`x`), closure unterminated (`tool_call.reasoning.unterminated`), `reasoning_text == ["x"]`; asserted by name. The `--reasoning separate` default and `separate-open` both give `text.start <= text.end` for this input."

**F13. R9's compatibility list predates R2-R6 and will be stale when it runs.**
Quoted: the R9 list (`reasoning_blocks`, `tool_choice_cardinality`, `forced[...]`, `disable_parallel_tool_use`, pointer renumbering, tie order) and "Update the PR body breaks list: `UnresolvedReason::ToolChoiceCardinality`, plus `StrictForced` and `ReasoningBlock` with public fields."
Evidence: R2 adds an `EditOperator` variant (public enum, not `#[non_exhaustive]`) and possibly a scope type; R5 adds a field to `ArgTree`, whose fields are all `pub` (`ir.rs:156-171`); R6 may add a field to `ArgLeaf` (`ir.rs:136-144`); R4 adds a request-error combination and the `usable_calls` decision from F4; R7 may add a `ToolSet` API.
`REWRITE TO:` "R9 enumerates every public-type and wire change present on the head at the time it runs (`git diff 943fa04 -- crates/core/src` for `pub` items), including the R2-R7 additions, in both INTEGRATION.md and the PR body."

### ADVISORY

**A1.** R2 golden `xml_array_ref_items_member_shares_outer_name` writes `"size":"3"` then says "with the types the schema gives"; the schema says integer and gap-analysis acceptance 3 says `"size":3`. Write the exact JSON before capture.

**A2.** The Draft-07 root `$ref` case (`adapter.rs:905-913`: no parameters projected, `PassThrough`) is the same root-cause class and the common FastMCP/pydantic shape (`{"$ref":"#/definitions/Args","definitions":{...}}`). State whether R2 covers it or why it is out of #8.

**A3.** Define nesting under an `anyOf`/`oneOf`/`allOf` union scope: a member nests when its body begins with `<parameter=` and any object branch declares it as object or array-of-object; typing stays schema-typed.

**A4.** R2's corpus regeneration rotates shared cases (historical fit section 4). Add to R2's check the base-corpus-on-head-code tally comparison the completion audit performed (identical correct/incorrect/abstained counts), so "zero incorrect" is not only about the new mutant.

**A5.** Completion-audit F1 repro 2 (`save(tags: string[], notes: array of $ref note{text, tags})`, outer **array** collision collected silently as Resolved) is a distinct shape from p10 (scalar collision) and rm/keep. Add it as a golden or state that p9r covers the outer-array class.

**A6.** R8's "demonstrated once, recorded, and then reverted" mutation check: record the exact command and failing assertion in `validation.md`.

**A7.** "Settle and exit" re-executes G2/G3/G7/G14 inputs; add G6 (p6 spans), the G16 depth test, and the F2 opaque input to the fresh auditor's list.

**A8.** Task-review naming "`tasks/GN-review.md` (named after the R-commit, `G-R1`...)" is ambiguous; use `tasks/R1-review.md` ... `tasks/R9-review.md`.

**A9.** R6 leaves two implementation routes open; acceptable because p12/p12j pin exact values, but if `ArgLeaf` gains a field it is a public break for F13.

## Verdict rationale

The plan maps the gap list faithfully and its CLI/MCP acceptance checks for G3, G6, G7, G14 are falsifiable on the real entry points. It cannot be executed as written: the G2 design (F1) does not yield its own golden, the opaque-scope guess is under-specified in both operator and expected value (F2), R4's merge rule does not achieve independence (F5), R5's test contradicts the Declined section (F7), three advisories are dropped (F4), and three items pre-authorise weaker evidence or scope (F10, F11, plus the tracking gap in F8).

VERDICT: NEEDS-REVISION

## Recheck (pass 2)

Re-read the revised `remediation-plan.md` in full. Verified independently: the branch ref `refs/heads/paudley/8-tool-call-recovery-arrays-of-nested` is `c1fa81dacdb19f4ff7b20f408be7733021c6a9de`; `validation.md` L60-63 records `make wasm-test` exit 0 on native, WASI (core/syntax/host) and browser-WASM legs under the repin, with the sysroot fetched into the gitignored `.cache/wasi`; `tasks/G-R1-review.md` exists (PASS). Per the coordinator, F8's tracking half and A2's tracking are "pending user decision" (an outward-facing issue creation), not plan defects.

| Finding | Status | Evidence in the revised plan |
|---|---|---|
| F1 scope-graph design | DISCHARGED | R2 "Design: a scope graph": one node per schema location, per-member `(nesting: None \| Object(node) \| ArrayOfObject(node), typing: unchanged)`, back-edges by index, `Frame.scope` enum `{Declared, Schema, Opaque}`, `nesting` takes the member's scope entry, `ValueShape::Unmodeled` unchanged (break listed in R9 if forced), union nesting rule, single-`$ref` object parameter from the same graph. This produces the p9r trace the original design could not. |
| F2 opaque guess operator and golden | DISCHARGED (with a derivation note) | `EditOperator::UnmodeledScopeBoundary` with cost 2 / guess / multiple / syntax / repair, `OPERATOR_COUNT` 46, RULES row, golden per rule, `cheapest`'s `settle` prices `Opaque` pops at it. Golden `xml_unmodeled_opaque_scope_boundary` has the exact tool and input; outcome `best_guess`, inference id, verdict `nothing_usable`, exit 4, and "arguments are asserted by exact JSON" are required. The plan commits to the pop as "the single guessed reading", so the hand derivation recorded in `tasks/R2-review.md` must come out as `{"keep":[{}],"path":"/home/user/important"}` with `unmodeled-scope-boundary` (2) plus `stray-closer` (3) for the balanced closer. If the implementer derives anything else, the design sentence is wrong and the R2 review must say which gave way. The sentence "raises its cost, but the reading is not discarded" means the stray-closer charge; a single reading has nothing to be discarded against. Not blocking. |
| F3 G16 depth test | DISCHARGED | Named test: `Budget { depth: d }`, `d` nested item levels resolve, `d + 1` -> `BudgetExceeded(Depth)`, no fallback, no panic; cited as evidence for historical-fit risk 5 and security P1. |
| F4 unmapped A3 / A6-doc / perf ADVISORY 2 | DISCHARGED | Gap list rows A3 (R4) and A6-doc (R2); perf ADVISORY 2 in Declined with its reason. R4's A3 fix zeros both usable counts when `tool_choice.passed` is false, updates INTEGRATION.md, Rustdoc, registry description and the fuzz `check_verdict`, adds a host test, and lists each re-blessed golden (`forced_*`, `tool_choice_none_*`). |
| F5 merge independence | DISCHARGED | "The merge fails closed": any member failing fails the merged reading; deduplicated union of diagnostics capped per G17; cardinality outcomes from the failing member; named two-dialect contested-attempt test under `forced[exactly-one]` with content allowed; fuzz invariant `passed && (exactly_one \|\| !allow_content) => rejected.is_empty()`. |
| F6 G15 vs probe and seeds | DISCHARGED | Probe asserts the request-error variant and prints `ok`; seed 4 re-encoded with a `Separate` control byte (`0xc6`: bits 4-5 = `00`, bits 6-7 = `11` forced, verified); one new seed draws the error combination; `test-fuzz.py` asserts both; recorded as a meaning correction. |
| F7 R5 test vs Declined | DISCHARGED | `work` ratio linear; `total_scratch_charged` ratio <= (k ratio)^2 x tolerance with a comment naming the pre-existing quadratic; adopt linearity by a `#[cfg(test)]` visit counter or by charging the step and asserting the charged ratio; no wall-time assertion. |
| F8 quadratic: documentation half | DISCHARGED | R9 adds the sentence to `docs/RESOURCES.md` and `qwen3-coder-xml.v1.md` (quadratic in repeated occurrences; a few hundred items exhaust explicitly under default budgets); Declined states the cap. Tracking half: pending user decision, per the coordinator. |
| F9 second fuzz toolset | DISCHARGED | Option (b): `fuzz-tool-call --tools ref`, `ToolSet::from_openai` with `$ref`/`anyOf`/colliding/opaque items, registered as target `tool-call-ref` with its own seeds and trailer handling, in the `make fuzz` loop and `docs/FUZZING.md`; existing target byte-for-byte unchanged except the F6 seed-4 correction. Nit (advisory): extend the G9 trailer test to the new target's seed list. |
| F10 #6 isolation | **STILL-OPEN** (narrow) | The isolation mechanism now exists (`--continue-on-known` with the #6 signatures, never used by `make fuzz`; mutated-execution coverage of `pop`/`nesting` item branches and scope-graph paths reported; fallback only after infeasibility, recorded as G4 open). But the fix text replaced the original per-failure attribution with a message-signature filter: "Any failure not matching the #6 signature is this branch's to fix." A failure on the **new** item or scope-graph path that trips the same F4 oracle produces the same message class and would be filed under #6 unseen, which is exactly the masking G4 names. Evidence: the F4 oracle (`fuzz_tool_call.rs:943`) checks every rejected attempt regardless of which codec path read it; the retained #6 input already reproduces on base only because it was replayed there (`validation.md` L41). `REWRITE TO:` "Every input retained by `--continue-on-known` is replayed through a probe built from base `943fa04` (the same attribution as `validation.md` L41). One that reproduces on base is #6's; one that does not is this branch's to fix before the campaign is claimed clean. The counts of each are recorded in `validation.md`." |
| F11 R1 descope / working tree | DISCHARGED | R1 marked DONE at `c1fa81d` (confirmed as the branch head); `make wasm-test` exit 0 on all legs; the `.cache/wasi` fetch stated as the script's repo-local step; `check-repository.py` and `test-fuzz.py` pass; review PASS. |
| F12 R3 golden expectation | DISCHARGED | Exact: one block, span `0..40`, text `39..40` (`x`), unterminated, `reasoning_text == ["x"]`, asserted by name, for both `separate` and `separate-open`. |
| F13 R9 compatibility list | DISCHARGED | R9 enumerates every `pub` and wire change from `git diff 943fa04 -- crates/core/src` at run time, naming the R2-R7 additions, in INTEGRATION.md and the PR body. |
| A1 size integer | DISCHARGED | `"size":3` / `"size":4`, "size is an integer per the schema". |
| A2 root `$ref` | Declined with reason; tracking pending user decision | Reason given (root projection, `adapter.rs ~L905`, not #8 §1 item scoping; pre-existing). The reason is legitimate for #8's scope. |
| A3 union nesting | DISCHARGED | Rule stated in R2. |
| A4 base-corpus tally | DISCHARGED | R2 Corpus: base `tool-calls.jsonl` on head code, tallies identical apart from intentionally changed truths, each listed; in R2's check. |
| A5 outer-array collision golden | DISCHARGED | `xml_array_ref_items_outer_array_collision` (audit F1 repro 2) added; exact JSON written before capture per the R2 Goldens header. |
| A6 R8 mutation record | DISCHARGED | Exact command and failing assertion recorded in `validation.md`. |
| A7 auditor input list | DISCHARGED | Settle: p9r, p10, rm/keep, outer-array, opaque, p7, p6 spans, p12/p12j, G14, G16 depth test. |
| A8 review naming | DISCHARGED | `tasks/RN-review.md`, with R1's earlier name noted. |
| A9 `ArgLeaf` break | DISCHARGED | Named in R9's enumeration. |

New text introduced by the fixes was read for new debt: the A3 change to the usable counts alters the documented `usable()` semantics and the fuzz probe's A13 invariant ("`usable()` holds only `Resolved` calls that name a tool", `fuzz_tool_call.rs:37-39`); R4 names `check_verdict` but the probe's doc header and A13 wording must also change. Advisory, not blocking: fold into R4's doc list.

One item remains open: F10's attribution rule. It is a one-sentence change to R7 (base-probe replay of every retained matching input), and it is required because signature matching alone cannot tell a #6 failure from a new-path failure with the same oracle message.

VERDICT: NEEDS-REVISION

## Recheck (pass 3)

Re-read the revised `remediation-plan.md` in full (user decision "Fix them in this PR" recorded verbatim at L12; execution order at L45-49; R10 and R11 added; Declined reduced to perf ADVISORY 2 and #6). Source re-checked for the new material: `tool_call_recover.rs:639-641, 692-739` (`wire_digest` hashes `result.to_node()`; the comment at L693-696 says the pinned digests cover the receipt's charged usage), `tool_call_goldens.rs:59-75` (`CAPTURED` holds no usage member), `tool_adapters.rs:1123-1167` (`assert_draft07_root_rules` pins `wrapped.parameters.len() == 0`, `PassThrough`, and the keyword list for a resolvable root `$ref`), `adapter.rs:574-599` (`render_canonical` embeds `parameters` verbatim), `toolset.rs:475-504, 1034-1049` (`render_declaration` for prompt forms renders from the projected spec), `xml.rs:1144-1240` (`terminator` scans per value and breaks at the next declared opener), `commands.rs:225-226` (`--tools`/`--tools-format` accept MCP declarations).

### F10

**DISCHARGED.** R7 Check, "Attribution" (L206): every retained input, matching or not, is replayed through a base-`943fa04` probe; reproduces-on-base is #6's; anything else is this branch's to fix before the campaign is claimed clean; counts (retained / reproduced on base / branch-only) are recorded in `validation.md`. This is the attribution the pass-2 rewrite asked for, and it closes the masking hole. The A13 doc sites were added to R4 (L142). The opaque golden now states the derived value with a reconcile rule (L79).

### R10 (G18): repeated parameters cost linear time and scratch

| Concern | Status | Evidence / note |
|---|---|---|
| Closes the gap on the real entry point | PASS | The cost test drives `ToolSet::recover` (the `tool_call_cost.rs` convention) with Qwen3-Coder XML repeated occurrences, the shape #8 makes common. The fuzz campaigns under R7 cover the mutated space. |
| Falsifiable checks | PASS, one wording fix | `work` and `total_scratch_charged` ratios within the file's linear tolerance for k = 1k..8k across three item kinds; `make corpus` passes; goldens unchanged. Wording: "k = 8k completes under the **default** budget for a document of that size, or the default budget scales by the published linear rule" is an either/or. `REWRITE TO:` "k = 8k resolves under `ToolRecoveryRequest::sized(Effort::Standard, output)`, the published linear rule (RESOURCES.md); the preset `Effort::Standard` budget is not asserted." |
| No scope narrowing | PASS | The fix is in scope by the user decision; the escalation ladder forbids fixing by inference; there is no fallback that accepts a residual quadratic. |
| Design fits the code | PASS with two conditions | Root-cause-first with scratch-lease instrumentation is the right method: `terminator` (`xml.rs:1144-1240`) breaks at the next declared opener, so the per-opener codec scan is O(value) for repeated declared parameters, which means the quadratic may not be where the candidate list points. **Condition 1 (advisory, method):** add to the candidate list the per-member work outside the codec scan: `ArgTree::adopt` leaf re-prefixing and `leaves` growth (`ir.rs:345-349`), `Reader::record`'s edit insertion, and `resolve::collect` / `Node::get` duplicate-member lookups, so the instrumentation is not biased toward the codec. **Condition 2 (required for the no-semantic-change guarantee):** the four `PINNED_*_DIGEST` constants (`tool_call_recover.rs:732-739`) hash `result.to_node()`, which includes the receipt's charged usage (comment at L693-696). R10 changes charged scratch by design, so these digests **will** move while "Goldens are unchanged, with no re-bless" stays true (`CAPTURED` has no usage member). R10's Check must add: "The four pinned digests are re-pinned only after showing that `to_node()` with `receipt.usage` removed is byte-identical to the pre-R10 head for each pinned run; the comparison is recorded in `validation.md`." The same rule applies to any earlier R-commit whose charging changes (R4 G17 "per-entry work is charged", R5 "remaining per-member work is charged"), and the task review of each must state whether the pins moved and why. |

**User decision needed only if R10's analysis reaches it.** If the root cause turns out to be a lookahead that a correctness rule (closer-in-value weighing, F14/F19) semantically requires and no decision-preserving linear replacement exists, the plan has no instruction. Phrased as the decision: "Accept an amortized bound that is not linear (for example O(n log n), or linear with a documented bounded lookahead) for repeated parameters, documented in RESOURCES.md with the R10 test asserting that bound; or accept a cited semantic change to the closer-text rule." The plan should name this trigger so the implementer stops and asks rather than choosing.

### R11 (G19): project a root `$ref` declaration's parameters

| Concern | Status | Evidence / note |
|---|---|---|
| Closes the gap on the real entry point | PASS | Check runs the built CLI `recover-tool-calls --tools <mcp declaration>` on the FastMCP input (the CLI accepts MCP declarations via `--tools`/`--tools-format`, `commands.rs:225-226`); `from_mcp` is the adapter entry. Hermes and Qwen3 forms, misspelled-name repair, 2020-12 `$defs`, `allOf:[{$ref}]`, and the unresolvable-`$ref` negative are each named. |
| Falsifiable checks | PASS, one omission to add | Exact Resolved arguments and exit 0 on the CLI; render-back byte-exactness per declaration; the adapter round trips. **Omission:** `tool_adapters::assert_draft07_root_rules` (`tool_adapters.rs:1143-1157`) pins today's behaviour for a *resolvable* Draft-07 root `$ref` (`wrapped.parameters.len() == 0`, `PassThrough`, keyword list `$schema, definitions, $ref, properties, type`). R11 changes it and the plan does not list it. `REWRITE TO:` "`assert_draft07_root_rules` is rewritten: `wrapped` projects the target's `properties` (named), `unknown_parameters` follows the target's `additionalProperties`, the sibling `properties`/`type` beside the Draft-07 root `$ref` stay **ignored for projection** (Draft-07 rule) and stay listed as unmodeled keywords with `$ref` and `definitions` (retained evidence); the `{"q":1}` validation assertion is kept." |
| No scope narrowing | PASS | "Not projected" covers only the genuinely undeterminable cases (unresolvable/external `$ref`, non-object target), each keeping today's `PassThrough` with its evidence. Validation still uses the exact declaration. |
| Design fits the code | PASS with one note | Projection at `adapter.rs ~L905` through the existing `object()` path on the resolved target is consistent with how `spec()` already follows `$ref` for validation (`exact_subschema`, `toolset.rs:1176-1190`), and nested `$ref` members flow into the R2 scope graph (hence R11 after R2). The render-back constraint is satisfied by construction for the three wire forms because `render_canonical` embeds the retained `parameters` text verbatim (`adapter.rs:574-599`). **Note (required to state, not a defect):** prompt-form `render_declaration` (Qwen3-Coder XML, Hermes, ...) renders from the projected spec (`toolset.rs:1034-1049`), so a root-`$ref` tool's printed declaration changes from no properties to the projected ones. That is the intended utility gain, but R11 must name it as an expected behaviour change, add one assertion (`render_declaration(DeclarationForm::Qwen3CoderXml)` lists the projected parameters for the FastMCP fixture), and R9 lists it. |

**User decision (the plan leaves this to the implementer; the coordinator asked that remaining criterion questions be phrased as decisions).** R11 "Not projected" second bullet: a root `anyOf`/`oneOf` of objects "projects the union of names ... or stays `PassThrough`. The implementer decides". Decision: "Project the union of names (typing left to the exact schema, as R2 unions do), or keep `PassThrough`?" Recommendation: project the union, for consistency with R2 and `.goals` maximal utility; either way the chosen behaviour gets a golden, and the plan should record the choice rather than defer it.

### Plan-text inconsistency to remove

R2 L106 still reads "**Draft-07 root `$ref`** (plan review A2). Declined for #8 ... tracking per the user's decision". It contradicts L12, the G19 row and R11. Delete the paragraph (or replace with "see R11").

### Conditions carried into task reviews

1. R10: add the pinned-digest re-pin rule (usage-only, verified by stripping `receipt.usage`) to R10's Check, and have `tasks/R4-review.md`/`tasks/R5-review.md` state whether the pins moved.
2. R10: replace the "default budget ... or" sentence with the `sized(Effort::Standard, output)` wording.
3. R10: extend the candidate list to the non-codec per-member work (advisory).
4. R11: list `assert_draft07_root_rules` with its new expectations; name the prompt-form render change and assert it; record the root `anyOf`/`oneOf` decision.
5. Remove the stale R2 L106 paragraph.

Items 1, 2, 4 and 5 are determinate text additions with no design latitude; `tasks/R10-review.md` and `tasks/R11-review.md` must verify them. Two genuine decisions are phrased above for the user (R10 fallback trigger; R11 root union). With those recorded, the plan closes every mapped gap on the real entry points with falsifiable checks and no narrowing.

VERDICT: PASS
