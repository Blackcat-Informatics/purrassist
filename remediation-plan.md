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
| G20 | HIGH (user decision) | The published per-byte default budget (32 work, 48 scratch) is below dense repeated markup's measured cost (52-61 work, up to 262 peak scratch per byte), so the default budget exhausts at about 1,000 nested items | R12 |
| G21 | HIGH (user decision) | Mistral v11 whitespace-chained calls are re-segmented at every call (superlinear) | R13 |
| G22 | HIGH (user decision) | Mistral v3 call arrays: quadratic wall time behind linear charged work (the governor under-counts) | R14 |
| G23 | LOW (user decision: document) | Call-free brace-heavy text costs 895 work/byte | R12 docs |
| G24 | MEDIUM (user decision: issue #10, accepted) | Restarted DeepSeek-V3.1 sections: quadratic charged work and super-quadratic wall time (pre-existing) | documented, #10 |
| G25 | MEDIUM | Residual about 2.2×-per-doubling wall time for many repeated nested items, with linear charged work (investigate) | R15 |
| G26 | MEDIUM (user decision: issue #11, accepted) | Repair search under-charges on a mutated free-form-item turn; fuzz timeout | documented, #11 |
| G12 | LOW | Hosted CI never ran. External; the MSRV compile was verified locally by the gap analyzer. | — (reported) |

## Fixes

**Execution order:** R1 (done) → R2 → R3 → R4 → R5 → R10 → R12 → R13 → R14 → R6 → R15 → R11 → R7 → R8 → R9.
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
- Golden `anthropic_redacted_thinking_closer_overlaps_opener`: input `{"type": "redacted_thinking", "data": "}x`. Expected (corrected in R3 review: the input is 41 bytes and the opener is 0..39): one block, span `0..41`, text `39..41` (`}x`), closure unterminated (`tool_call.reasoning.unterminated`, a guess, so the outcome is `best_guess`), `reasoning_text == ["}x"]`, asserted by name. The `separate` default and `separate-open` both give `text.start <= text.end` for this input.
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

**Also (R5 review advisory 2).** Leaves keep their written spelling (an alias repetition is `/ex/1`), so `marker_cut` must key by leaf span, not by resolving a pointer. A test covers an alias-spelled repetition holding a reasoning marker in its JSON string.

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

**Test runtime (R5 review advisory 1).** After the fix, `repeated_parameters_cost_is_linear` must run in a few seconds in debug (it was 48 s, caused by the quadratic scratch).

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

### R12: `perf(tool-call)!: raise the published per-byte recovery budget to cover dense markup` (G20, user decision)
**User decision (verbatim).** Question: published default budget below dense repeated Qwen3 XML cost, "What should I do?" Answer: **"Raise the published constants"**.

**Fix.**
- Re-measure the per-byte `work` and peak `scratch` of recovery on dense markup:
  - repeated nested items, JSON-array items and string items, in qwen3-coder-xml;
  - plus the densest shapes of the other dialects (many small calls in hermes, openai-tool-calls, mistral and harmony);
  - up to the 1 MiB acceptance size, under the full default dialect set.
- Raise `RECOVER_WORK_PER_BYTE` and `RECOVER_SCRATCH_PER_BYTE` (and any per-byte constant `sized` uses for candidates/depth if measured as binding) to the measured maximum plus a stated margin, so every measured dense shape up to 1 MiB completes under plain `sized(Effort::Standard, output)`.
- Update the published worst-case bound, `docs/RESOURCES.md`, INTEGRATION.md C4, any resource qualification JSON/schema that pins the constants (`docs/qualification/resources-*.json`, `scripts/check-resource-limits.py`), and the cost tests that assert them.
- Name the margin and the measurement table in RESOURCES.md.

**Test.**
- `tool_call_cost.rs` `repeated_parameters_cost_is_linear` uses plain `sized(Effort::Standard, output)`, with no raised per-byte override, at k=8000 for all three shapes.
- A new dense-markup case per dialect at roughly 1 MiB completes under plain `sized`.
- The existing 1 MiB test still passes.

**Check.**
- `cargo test -p purrassist-core --test tool_call_cost` passes.
- `python3 scripts/check-resource-limits.py --schema-only` (or the full check) passes.
- `make hygiene` passes.
- Goldens unchanged, apart from receipts that record budgets. Any budget-bearing golden or digest that changes is listed, and digests are re-pinned only under the Digest rule, with `receipt.usage` and budget fields excluded and justified.

### R13: `perf(grammar): consecutive chained calls segment incrementally` (G21, user decision)
**User decision (verbatim).** Answering whether to fix three newly measured superlinear paths: **"Fix (1) and (2) here, document (3)"**.

**Gap.** Mistral v11 calls separated only by whitespace form one `next`-chained attempt, and the engine re-segments the whole chain at every call. `fields()` cloned argument trees 17,860 times for 143 calls. Charged work and peak scratch per byte grow with the number of calls: 141 work and 2,009 scratch per byte at 409 calls, and 1,139 and 27,354 at 6,553 calls. The source is `tasks/R12-measurements.md`.

**Root cause first.** Use instrumentation, then the escalation ladder, as in R10.

**Fix.** Segment the chain incrementally, extending per call rather than re-running, so each call's segmentation and field extraction cost is O(call). The readings, calls, repairs and attempts must be identical.

**Test.**
- Mistral v11 with a newline after each call, k = 256…4096 calls: charged work and peak scratch grow linearly in bytes (within the cost-test tolerance) and complete under plain `sized`.
- The goldens are unchanged without a re-bless, `make corpus` passes, and the round-trip tests pass.

**Docs.** Remove the item from RESOURCES.md "Not covered" and add the shape to `dense_markup_fits_published_budget`.

### R14: `perf(grammar): Mistral v3 call arrays cost linear, charged time` (G22, user decision)
**Gap.** A Mistral v3 `[TOOL_CALLS]` array of thousands of calls charges linear work but takes quadratic wall time: 0.18 s at 16 KiB, 1.8 s at 64 KiB, and no finish at 1 MiB within 300 s. The governor under-counts real work.

**Root cause first**, as above.

**Fix.**
- Make the real work linear.
- Every remaining loop is charged in proportion to the work it actually does. Charged work must track wall time.
- The readings must be identical.

**Test.**
- An array of k = 1k…16k calls: charged work linear.
- A deterministic structural check, a counter or the charged ratio, rather than wall time.
- 1 MiB completes under plain `sized`, in the corpus lane.
- The goldens are unchanged and `make corpus` passes.

**Docs.** Remove the item from RESOURCES.md and add the shape to the dense tests.

### Brace-heavy call-free text (G23, user decision: document)
RESOURCES.md keeps "a megabyte of `{` costs 895 work/byte (linear), explicit `BudgetExceeded(Work)`" as a documented not-covered shape. It is done in R12.

### G24: restarted DeepSeek-V3.1 sections (user decision: file an issue, accept for now)
**User decision (verbatim).** "File the deepseek issues as a new issue, and accept it for now." It was filed as **#10**, with a reproduction and measurements: quadratic charged work, super-quadratic wall time, identical on `main` 943fa04. RESOURCES.md lists it as not covered and cites #10. No code change in #8.

### R15: `perf(tool-call): residual superlinear wall time for many repeated items` (G25, investigate)
**Gap (observed by the R6 review and the R6 implementer).** With repeated nested items and no markers, release wall time grows about 2.2× per doubling at k = 8k → 16k → 32k (2.17, 4.76, 8.29 s), while charged work is linear. The R6 reviewer saw about 36 s at 32k+ in a debug build. The contract requires charged work to track real work, and the user asked for repeated-parameter cost to be fixed (R10).

**Root cause first.** Profile in release at k = 8k, 16k, 32k and 64k, and name the stage whose time grows faster than its charge. If it is only a cache or allocator effect (time per charged unit drifts but no step is superlinear in operations), record the evidence and close the gap as a false positive. Otherwise fix it (linear operations, charged), and add a deterministic operation-count test.

**Check.** A release timing table up to 64k, with the ratio of time to charged work stable, or the gap closed with profile evidence.

### G26: slow repair search on a mutated free-form-item turn (user decision: issue, accept for now)
**User decision.** Chosen option: "File an issue, accept for now (Recommended)". Filed as **#11**: 192 bytes, about 240-280 ns per charged work unit, about 5 s in the fuzz probe; identical on `main`. FUZZING.md and RESOURCES.md cite #11. `make fuzz` at the default timeout will fail on `tool-call-ref` there, and on `tool-call` for #6, until those are fixed.

## Declined / not in scope (with reasons)
- **Performance advisory 2** (per-item pointer allocation is uncharged). This is pre-existing and O(1) per item, bounded by the charged items.
- **The #6 rejected-span oracle fuzz failure.** It is tracked by #6 and attributed by exact base replay; R7 isolates it for campaigns.

## R9 compatibility items accumulated during remediation
- R2: the new public `tool_call::scope` module (`ScopeGraph`, `ScopeNode`, `ScopeMember`, `Nesting`); the new fields `ArgumentShape.scopes`, `ParameterShape.nesting`, `ArgTree.collected` and `CollectedMember`; `EditOperator::UnmodeledScopeBoundary` (`OPERATOR_COUNT` 46). Free-form object parameters and items (`additionalProperties:true`, no properties) now close by the guess rule `unmodeled-scope-boundary` instead of `missing-parameter-close`. The `Budget::depth` charge counts the pushed frame, so a whole call holds about (d-1)/3 nested item levels.
- R5: `ArgTree` gains a private field, so external struct-literal construction no longer compiles; `ArgTree::value` and `empty_object` are no longer `const`; alias repetitions share item indices while leaves keep the written spelling; `decode-typed` evidence cites each item's own span.
- R11: root `$ref`/`allOf`/`anyOf` declarations now project parameters, where Draft-07 root `$ref` used to project none (behaviour change). `assert_draft07_root_rules` expectations changed. The golden runner accepts `tools_mcp`. The receipt validation log is indexed (usage only). The root-union declaration ceiling under the default budget is about 610 one-property branches, down from about 670.
