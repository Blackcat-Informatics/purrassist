# Historical fit: PR #9 (issue #8), 943fa04..248d07a

Evidence: the stage files, local `git log -S`/`-G`, and code reading of the diff. Items marked (read) were not executed. Nothing was refetched from the forge.

## 1. Precedent followed or departed from

### 1.1 Codec frame model (`grammar/codec/xml.rs`)
- **Follows.** The item path reuses the frame stack from `58e9ac1`, and the object and unmodeled nesting from `5d52d98` and `7c919c9`.
  - `nesting(declared)` is a pure extension of the old `match`: `Array(Object|Unmodeled)` now yields `(scope, item=true)`.
  - `Frame.item` is consumed only in `pop`, which stays the single attach point.
  - `Depth` is charged as for object frames, and `pop` is now fallible (`Result<(), StopReason>`).
  - A resource stop thus propagates instead of falling back to a string, which fits the AGENTS.md "exhaustion is explicit" rule.
- **Mild departure.** `pop` records `ArrayItem` itself, with a zero-width edit `from..at, b""`. Before, `ArrayItem` was recorded only in `Reader::value` for a non-`[` value.
  - The rule id and cost (0, non-guess) are unchanged, so the Exact admission and the RULES.md semantics are preserved.
  - The edit's span meaning differs between the two sites: a value span versus an item-body span.
- **Unchanged hot-path invariants.** `owner()` (`rposition`), `boundary()`, `terminator`, `Held`, `Mode`, `cheapest` and `reading` are unchanged. These are the F9, F13, F14 and F19 areas, with F14 and F19 unconfirmed in #7. Existing goldens were not re-blessed for Task 1; 9 new goldens were added.
- **Documented limit, not a fix.** A member named like the array is the item's own child, never the next item (`xml_array_nested_object_same_name_member`). A recursive `$ref` item is `Unmodeled` and takes the sibling reading with `missing-parameter-close`. Nesting needs the tool known at codec time, the same as for object parameters.

### 1.2 Collect rule and pointers (`ir.rs::adopt`, `resolve.rs::collect`)
- **Follows.** `collect` (from `5d52d98`) stays the only place repeats become arrays. The change is the numbering of pointers: `ArgTree::adopt` now shifts leaf and unresolved pointers by the item count of earlier same-name occurrences, and `collect` numbers `source/{before+index}`.
- **Departs, to close a latent bug.** Before, every repeated occurrence aliased `/name/0`. Scalar repeats never exposed this, because their leaves were not item-nested. This is a behaviour change to the pointer space of any repeated array parameter, not only to object items.
  - The shift applies only when the occurrence root is an `Array` and not a stand-in.
  - `earlier` counts a non-array earlier occurrence as 1 and an array occurrence as its length. A JSON-array occurrence followed by a nested one is covered by the `_mixed` golden.
  - Nothing in the evidence shows the other pointer-keyed consumers were audited after the shift: the `decoded()` first-match lookup by `leaf.pointer == site.source`, minimal-diff re-emission "item by item" in `emit.rs`, and evidence `destination`. The prior-art hazard list named them (read).

### 1.3 Tool-choice validation path (`recover.rs`)
- **Follows.** `ToolChoice` and its `constraint()`/`Recovery::tool_choice` came from `1fbe235`. The strict form adds a `ForcedStrict` arm beside `Forced` and reuses all of the following:
  - `lookup_exact` canonicalization;
  - the per-reading check and `ToolChoiceCheck {constraint, passed, diagnostics}`;
  - the F5/F7 whole-turn failure (`Finish::Unresolved`, `NothingUsable`, empty emission);
  - the `purrassist.tool_call.v1` validation record.
- **Departs, but compatibly, from the form the issue sketched** (`Forced{name, exactly_one, allow_content}`).
  - `Forced(String)` is untouched.
  - A new variant and struct (`StrictForced`) carry the options, with a bracketed constraint grammar `forced[exactly-one,no-content,no-reasoning]:NAME`. The name is last so `:`, `;` and `,` in a name stay unambiguous.
  - A lenient `ForcedStrict` renders exactly `forced:NAME`. Two types can therefore produce the same constraint string.
- **Departs from the Anthropic doctrine of "never silently drops".** `from_anthropic` used to reject `disable_parallel_tool_use`. It now maps `true` to `ForcedStrict{exactly_one, allow_content: true, allow_reasoning: true}`. It still errors on `auto`, `any`, `none` and a non-boolean.
  - This is a deliberate narrowing of a previous hard error. Callers that relied on the error get a behaviour change.
  - It also adds a cross-surface contradiction rule in the host: `exactly_one:false` beside `disable_parallel_tool_use:true` is an error.
- **New `UnresolvedReason::ToolChoiceCardinality`** (`code()` = `tool_choice_cardinality`). It follows the `ToolChoiceForced` and `ToolChoiceNone` precedent from `1fbe235`.
- **Content semantics.** "Content" means `content` ranges only. A rejected attempt, an `unsupported_dialect` span or an embedded call is not content, and not a counted call. Docs claim "a rejected attempt already fails the turn's finish". Whether `tool_choice.passed` stays true when only a rejected attempt sits beside the single call was not shown by an executed case (read).

### 1.4 Reasoning ranges (`recognizer.rs`, `ir.rs`)
- **Follows.** It extends the `Think`/`Turn::reasoning` machinery from `b7fe57c`, keeping `ToolCalls.reasoning` (merged, markers included) intact. The new `reasoning_blocks` is a parallel, unmerged vector.
- **Departs from the prior-art plan** (an index-aligned inner range). A paired `{span, text}` struct per block was chosen. This is more self-describing and is not index-coupled to the merged `reasoning`.
- **Text start.**
  - The opener end comes from the matched `Pending::Marker.end`, as constraint 12 required.
  - The two cut-at-marker rejected-attempt sites instead use `opener_end(marker, pair)`, which is marker plus the opener literal length (read). That is exact only for literal markers.
  - Both are justified in the T3 review as equal because markers are literal byte strings.
  - The text end is the closer start, the call opener, or the end of output.
- **Digest side effect.** Tie order among `Ambiguous` readings is the digest of the wire JSON, so adding a wire member reorders ties. Two goldens reordered `readings`/`ledger_rules`. This is the documented "cost, then content digest" contract. Four digests were re-pinned and verified by restoring the old pins with the member removed.

## 2. Wire versioning: is a required field without a version bump consistent?

- **Repository doctrine.**
  - `docs/IMPLEMENTATION.md` says only: "Receipt serialization is versioned; `docs/schema/receipt.schema.json` is the JSON Schema for the current version". That versioning covers the receipt (`receipt.version`, integer, currently 3 in the fixtures). It does not cover the `ToolCalls` value.
  - `docs/schema/` holds no tool-call schema. The `ToolCalls` wire shape lives only in `crates/host/src/tools/registry.rs` (`TOOL_CALLS_OUTPUT`) and in `WireValue for ToolCalls`.
  - No doc, in CONTRIBUTING, INTEGRATION or README, states an additive-field or compatibility policy for `tool_call.recover.v1`. The operation id `tool_call.recover.v1` is the only "version", and a repo-wide grep found no bump rule.
- **History precedent.** Every prior `ToolCalls` wire member was added in place, with no bump, and always inside the unreleased #3 line:

  | Member | Commit |
  |---|---|
  | `next_id_start`, `tool_choice`, `distinguishing`, `keywords`, `openai_finish_reason` | `1fbe235`, `32ceb64` |
  | `embedded` | `2b5c98b` |
  | `candidate_dialects` | `f1f1987` |
  | `verdict`, `usable_calls`, `usable_calls_accepting_guesses` | `b65687e` |
  | `emitted` | the emission commit |

  `reasoning_blocks` (required, `additionalProperties:false`) follows that pattern.
- **Material difference.** All earlier additions happened before the operation shipped. #8 itself says the integrator already consumes v1, and PR #5 is merged to main.
  - The workspace is still `0.0.0` with no tags and no crates.io publication (`docs/RELEASE.md`: "A published 0.1.0 is not claimed"). Compatibility is therefore not yet promised.
  - This is the first post-merge, integrator-visible wire change. A strict client that validates with the old `additionalProperties:false` output schema will reject the new member. The MCP `inputSchema` is only extended, but the output schema now lists `reasoning_blocks` as required.
- **Reading.** No doctrine requires a bump, and precedent supports in-place addition pre-release. The change sets a precedent as the first post-merge addition, with no stated policy. An integrator note is arguably owed (see risks).
- **Other public-surface growth.**
  - `ToolChoice` and `UnresolvedReason` gain variants, and `ToolCalls` and `ReasoningSegment` gain fields.
  - None of these has `#[non_exhaustive]` (only `detect.rs` and `adapter.rs` use it). Downstream exhaustive `match` and struct literals break at compile time.
  - Within the pre-release line this is consistent with `1fbe235`, but the diff states no compatibility note.

## 3. Doctrine check

| Doctrine | Result |
|---|---|
| Guess vs resolved | New item rules are cost 0, non-guess, single-reading, definition-grounded. The issue example is `Resolved` under Exact (CLI run E1). The unmodeled item is typed later by `schema_typed`. |
| Resource exhaustion | `pop` is fallible, so a `Depth` stop propagates. No test in the evidence forces depth exhaustion through an array item (read). |
| Linear cost bound | `adopt` walks all leaves of the adopted value and `shift_item` allocates a `String` per leaf and unresolved. The sum over k repeats of k items with m leaves is O(k·m), not quadratic in k, but `earlier` is recomputed per adoption by scanning all members, which makes it O(k²) in member count for k repeats (read). The strict content scan is O(content bytes). `docs/RESOURCES.md` documents a bound for `ToolSet::recover`. The diff does not touch it, and no measurement was added. |
| F15 corpus truth | Truth citation present: "Issue #8 section 1" is cited in the new mutants. But the same diff narrows the existing `inline-closers-then-call` mutant, skipped when a value ends in `\n`. A second filter was added in the corpus. The justification is a code comment on framing-newline behaviour, not an issue clause or maintainer quote. The reviewer checked that base had 0 affected cases (34 of them at base), so no previously evaluated case was dropped. By F15's letter this is a truth-affecting edit without the required citation, and it is recorded only in a code comment. |
| F20 published transcripts | `mcp-responses.jsonl` was recaptured twice (strict input member; `reasoning_blocks`) and `test-verify-published.py` passed. The diff adds only the intended members. `call-response.json` has no recover response, so it is correctly unchanged. |
| AGENTS.md "implemented vs planned" | Docs state limits (recursive `$ref`, tool known at codec time, same-name member) and name proving tests. |
| Core dependency freedom, SPDX | No new dependencies. No new source files. The new goldens are unheadered JSON like the existing ones. |
| Hook discipline | `make check` exit 0 on 248d07a. The WASM comparison was not run (Node v26.11.1 against the pinned v26.10.0). |

## 4. Recurring failure patterns in the history that touch this code

- **Xml-params closer handling.** Refixed repeatedly: `9a47d3c`, `ef4abf7`, `4c05791`, `c52c3a3`, `749f53d`, `b605ce5`, `7a5a6c2`, and adversary passes 4 to 6 (`73e6a13`, `3961333`, `75c8f11`, `3813065`). Each pass found new interactions between a value's closer and the following text. The item path adds a frame kind inside this region, which makes a pass-7 finding plausible even though the new goldens pass.
- **Rejected-span fuzz oracle (F4/F11/F16).**
  - #6 is open: `make fuzz` fails on the base after about 500 executions. On the branch it fails after about 1001, with a different message at line 1056.
  - Attribution by exact replay on a probe built from 943fa04 reproduces the same message. The new `reasoning_blocks` and strict invariants did not fire in the bounded run.
  - Only the first failing input is visible in a bounded run. The new codec path was fuzzed for under about 1000 executions before the oracle abort, so the nested-array path is weakly fuzzed (read).
- **Seed-layout drift.** The control byte is full, so strict options come from a trailing byte `0xF8..=0xFF` that is consumed only when `control>>6==3` and `len>2`. This reinterprets any existing input of that shape. The claim that no earlier seed ends in such a byte is asserted in a comment, not tested. The mutator can create such inputs, so retained corpus entries of that shape change meaning.
- **Re-bless volume.** 481 goldens were mechanically re-blessed and 4 digest pins moved. This is the pattern where a mass diff hides semantic changes. The reviewer's script diff (464 identical and 2 reorders, 0 semantic) is the only guard. It is outside the repo and was not made a gate.
- **Corpus shift.** Adding the `plan_outfits` tool changed the shared base cases. Per-rule counts moved for unrelated rules (for example `json.syntax.literal-case` 77 to 55, `json.syntax.bare-string` 187 to 176, `tool_call.arguments.decode-string` 336 to 336 plus additions). `distinct_requests` (18165) is now below `cases` (18166), where before they were equal. The repo test requires unique identities and at least 500 distinct requests, so it passes, but it is a first duplicate request in this corpus.

## 5. Consumers of the changed contracts

| Consumer | Updated? | Notes |
|---|---|---|
| core: `ir.rs`, `recognizer.rs`, `recover.rs`, `resolve.rs`, `mod.rs` re-exports | Yes | `ReasoningBlock` and `StrictForced` are exported. `calls_key` keys on blocks (advisory fix applied). |
| core codec docs, `edit.rs` operator docs, `explain()` text | Yes | |
| host MCP input schema | Yes | `tool_choice_strict` is a closed object, all three members required. The `MEMBERS` list is 14. |
| host MCP output schema | Yes | `reasoning_blocks` is required. `UnresolvedReason.code` is `type:string`, so the new code needs no schema change. |
| host `recover_tool_calls` handler | Yes | Cross-checks the Anthropic contradiction. |
| CLI | Yes | Three valueless flags, each at most once, a usage error without a named choice. The CLI help string is updated. |
| wasm-harness `transcript.rs` | Not edited | The transcript contains `<think>plan</think>` and the full `result.to_node()`, so `reasoning_blocks` flows into the native-versus-WASI comparison. The pinned digests cover only the emission arrays, hence "no re-pin". `scripts/check-wasm.py` was not run, so parity of the new member is unverified. CI runs `make wasm-test`. |
| `scripts/data/verify-published` | Yes | `mcp-responses.jsonl` only. |
| `scripts/fuzz.py`, `test-fuzz.py` | Yes | Five strict seeds added. |
| Corpus and `manifest.json` | Yes | Hash, case counts and `MATCHING-QUALITY.md` tables regenerated. `tool-calls.jsonl` is a 19k-line diff. |
| Published transcripts for the CLI | Partial | `mcp-requests.jsonl` was not changed. The recorded published fixtures cover strict input by schema only, so the CLI strict flags and `reasoning_blocks` with real content are not in the replay. The CLI strict flags are covered by `tool_calls_cli` tests. |
| Docs | Yes | README, CLI README, INTEGRATION, RULES, README for tool-calls, the Qwen dialect doc, FUZZING, MATCHING-QUALITY. |
| `docs/schema` | n/a | Receipt only. There is no tool-call JSON schema outside `registry.rs`. |
| CI | Blocked | Hosted CI could not start. No hosted gate ran for this PR. |

## 6. Blast radius

- **Behavioural, previously failing inputs.** `Array(Object|Unmodeled)` parameters whose item begins with `<parameter=` used to fault as `InvalidJson`. They now resolve. This reaches every Qwen-family call with such a schema and is the intent.
- **Behavioural, previously working inputs.**
  - Anthropic `disable_parallel_tool_use` accepted, not an error.
  - The pointer renumbering of repeated array occurrences. The `collect` source pointers and evidence of repeated-array calls change.
  - Tie order among ambiguous readings (two goldens).
- **Wire.** One new required output member on every `recover_tool_calls` result, on `value` and on every alternative, plus one new input member, one new unresolved code and one new constraint-string family.
- **Code.** Xml-params is the churn-heavy region; the parser for untrusted model output is changed.

## Risks for gap analysis

1. **Required output member without a stated compatibility policy.** `reasoning_blocks` is `required` under `additionalProperties:false` in a merged operation that an integrator already uses. No doc states an additive-field rule. History precedent is pre-release only. Check whether an integrator-facing note or an explicit "additive members may appear" statement exists in INTEGRATION.md, and whether the `outputSchema` should require the member at all.
2. **Public enums and structs without `#[non_exhaustive]`.** `ToolChoice`, `UnresolvedReason`, `ToolCalls` and `ReasoningSegment` gained variants or fields. This breaks exhaustive matches and struct literals downstream, with no compatibility note in the diff.
3. **Pointer renumbering in `adopt` and `collect`.**
   - It changes sources and leaf pointers for all repeated array parameters, not only objects.
   - Pointer-keyed consumers (`decoded()` first-match, `emit.rs` item-by-item minimal-diff re-emission, evidence `destination`) are not shown to be re-audited.
   - Verify with a repeated nested-item call emitted to OpenAI and dialect targets, and with a schema-typed `Unmodeled` item that is repaired in the second occurrence.
4. **O(k²) cost in `adopt`.** The `earlier` scan is recomputed for every adoption. Check it against the linear-cost doctrine in `docs/RESOURCES.md` and the resource-limit script, with a thousands-of-items input. No performance evidence was added.
5. **Depth exhaustion through item frames.** `pop` is fallible, but no cited test forces `Resource::Depth` exhaustion through nested array items. Verify that the outcome is an explicit stop, not a string fallback.
6. **Interaction of item frames with F9, F13, F14 and F19.** These cases need adversarial probing: stray `</parameter>` after a closed item, doubled closers, text after an item closer, an indented item, two calls in a row after an unclosed last item, an item inside an object inside an item, and an unknown or renamed tool. F14 and F19 are themselves unconfirmed in #7.
7. **Rejected-span oracle (#6) masks the new path.** `make fuzz` stops at the pre-existing oracle failure after about 1000 executions. The nested-array codec path and the strict and block invariants had little fuzz exposure. Consider a fuzz seed set with nested array items, run with the F4 clause isolated.
8. **Strict check blind spots.** "Exactly one" counts `calls` only. A rejected attempt, an `unsupported_dialect` span, an embedded call and `Ambiguous` calls (per reading) are not counted. Confirm by execution that `tool_choice.passed` and `finish` behave as the docs claim in each case. Also confirm that mutating call outcomes to `Unresolved(ToolChoiceCardinality)` inside the per-reading check cannot make two readings collapse or diverge in the digest.
9. **Anthropic mapping.** `disable_parallel_tool_use:true` now yields an exactly-one choice that allows content, which is a semantic reinterpretation of a field that previously errored. Review the MCP contradiction rule and the `ForcedStrict`/`Forced` constraint-string aliasing (both render `forced:NAME` when lenient).
10. **Fuzz trailer byte.** A last byte `0xF8..=0xFF` under control bits `11` is consumed as strict options. The comment claims no earlier seed ends that way; there is no test. Retained and mutated corpus inputs may silently change meaning.
11. **F15 on the corpus narrowing.** The `inline-closers-then-call` guard and the second filter change which mutants exist, justified by a code comment only, with no issue clause or maintainer quote. The new `distinct_requests < cases` is a first. Re-check the truths of the new mutants against issue #8 section 1 only, and consider whether the narrowing needs a recorded justification.
12. **WASM parity not run.** The transcript includes a `<think>` block, so `reasoning_blocks` and the strict-choice code path enter the native-versus-WASI comparison. `make wasm-test` failed on a Node pin mismatch and hosted CI is not run, so no environment has executed it. The wire change is unverified on WASM.
13. **Published transcript coverage gap.** The F20 replay shows `reasoning_blocks:[]` and the strict input schema, but no recover call with a reasoning block or a strict choice. Real-content behaviour is covered by repo tests, not by the published-binary replay.
14. **Mass re-bless of 481 goldens.** Only an off-repo script showed the diff to be additive. Re-run an independent diff, and check the two reordered goldens (`harmony_python_code_holds_call_closer`, `qwen_value_holds_call_closers_then_text`) for any consumer that relies on the old `readings` order.
15. **Docs and claims.** Verify every capability claim has an executable proof (AGENTS.md): the "mixed", "unclosed before sibling" and "same-name member" doc statements, the `$ref` sibling reading, and the statement that the rejected attempt fails the finish. Verify that the dialect docs match the codec Rustdoc on the Unmodeled and `anyOf` cases.
16. **`opener_end` shortcut.** It recomputes the opener end from the literal length at the two rejected-attempt cut sites, instead of the matched marker end. This is correct for literal markers only. Check Anthropic's explicit JSON-fragment markers and any marker the grammar can match with variable spelling or case.
