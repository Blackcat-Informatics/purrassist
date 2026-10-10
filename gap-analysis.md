# Gap analysis: PR #9 (closes #8), 943fa04..248d07a

**Branch:** `paudley/8-tool-call-recovery-arrays-of-nested` · **Base:** `origin/main` = 943fa04 · **Head:** 248d07a · **Files changed:** 512 (29 non-golden)

## Method

- **Inputs.** Every stage file named in the brief:
  - `issue.md`, `issue-analysis.md`, `prior-art-assessment.md`;
  - `historical-fit.md`, `plan.md`, `validation.md`;
  - `tasks/T1..T3-review.md`, `reviews/plan-review.md`, `reviews/completeness-review.md`;
  - `branch-under-review.md`, `review-brief.json`, `raw/`.

  Plus `AGENTS.md`, `.goals`, `docs/IMPLEMENTATION.md` and `docs/tool-calls/{README,INTEGRATION,RULES}.md`. The forge was not refetched. PR #9 has 0 reviews, 0 inline comments and 0 threads. Hosted CI did not start.
- **Execution.** Everything ran against the shipped entry points:
  - **Head CLI:** `target/debug/purrassist`, built with `cargo build -p purrassist-cli`; the target dir came from `cargo metadata`.
  - **Base CLI:** built from `git archive 943fa04` under `/tmp`.
  - **Rust probe:** a scratch crate under `/tmp` that depends on `crates/core` by path. It drives `ToolSet::recover`, `Recognizer` and `ToolCalls::reemit_with`.
  - **WASM:** a core-only replay of `scripts/check-wasm.py`.
  - **MSRV:** `rust-validate 1.98.0` on a `/tmp` copy of HEAD.
- **Cleanup.** All `/tmp` experiments were deleted afterwards.
- **Not run, as instructed:** `make check` and `make fuzz`.
- **Reproduction.** Every acceptance check uses the inputs in the appendix, with `T=/tmp/ga8-tools.json` and `B=target/debug/purrassist`.
- **Parallel reviews.** `reviews/performance-review.md` (BLOCKED; REQUIRED 1) is referenced, not duplicated. `reviews/security-review.md` did not exist yet when this report was written.

## Gap table

| ID | Sev | Category | Evidence | Fix | Acceptance check (real entry point) |
|---|---|---|---|---|---|
| **G1** | HIGH | Validation (user decision) | **The gate never ran.** `make wasm-test` stops at the Node identity check in `scripts/check-wasm.py:16-20`. `vendor/tooling.json:22-32` pins 26.10.0; the local Node is v26.11.1. `docs/IMPLEMENTATION.md:107-122` requires an *executed* native/WASM comparison that includes the tool-call transcript, and hosted CI is not run.<br>**User decision (verbatim):** "you may not modify the local system. just change the node version pinned to match ours."<br>**Executed here (core leg only, identity check skipped).** The native, wasm32-wasip1 (`node scripts/run-wasi.mjs`) and wasm32-unknown-unknown (`run-wasm.mjs`) transcripts are byte-identical: sha256 `608b3fed…cfe5a`, 296226 bytes. They carry `reasoning_blocks` 25 times, from the transcript's `<think>` turn.<br>**Still not run:** the syntax/host WASI legs (they need the WASI sysroot download) and the gate itself. The executed leg lowers the risk; the gate stays open.<br>**License:** the local Node 26.11.1 `LICENSE` sha256 is `6b85ee19…4fc128`, identical to `vendor/licenses/node-26.10.0/LICENSE`. | Change the `vendor/tooling.json` `node` entry:<br>- `version` → `26.11.1`<br>- `source` → `…/nodejs/node/v26.11.1/LICENSE`<br>- notice path → `vendor/licenses/node-26.11.1/LICENSE` (same sha256 `6b85ee19…`)<br>- `binary_source` → `https://nodejs.org/dist/v26.11.1/node-v26.11.1-linux-x64.tar.xz`<br>- `binary_sha256` → `3883bfc73f9a680ca4eab04b196068aaaab1373ffa77d8fc1a4408222495b651`<br>Then:<br>- `git mv vendor/licenses/node-26.10.0 vendor/licenses/node-26.11.1`.<br>- Leave `docs/qualification/fuzz-*-pattern-*.json` alone. They are historical run records, and no script compares them to the pin.<br>- Record the run in `validation.md`. | `make wasm-test` exits 0 and prints "Native, WASI (core/syntax/host), and browser-WASM (core, executed) acceptance transcripts all match." `make hygiene` exits 0 (the `check-repository.py` tooling-notice hash). |
| **G2** | **CRITICAL** | Requirement, correctness, unsupported claim | **Root cause.** An `Unmodeled` item frame (`$ref` or `anyOf` items, which issue §1 names explicitly) has scope `None` and declares no member names. So `owner()` gives any opener an outer scope declares to that outer scope. The item path inherits this from the object path. Two executed manifestations follow.<br>**(a) Recursive `$ref`: a new incorrect automatic answer.** Input p9r is well-formed, balanced nesting, with a zero-repair child reading under the exact schema.<br>- `B recover-tool-calls --tools $T p9r.txt`: exit 0, Resolved, verdict `run`, `{"children":[{"name":"a"},{"name":"b"}]}`. It uses `missing-parameter-close` and `stray-closer` (cost 8), and this flattened tree would be emitted and run.<br>- The same bytes with the inline schema (p9i): `{"children":[{"name":"a","children":[{"name":"b"}]}]}`, zero repairs.<br>- `--effort exact` on p9r: exit 4, `repair_not_permitted`. Canonical input is refused.<br>- **Base 943fa04** on p9r: exit 4, Unresolved `schema_validation`. The engine used to abstain; it now answers wrongly with Resolved certainty.<br>- The golden `xml_array_recursive_ref_items_are_siblings` (`tool_call_goldens.rs:1678-1697`) pins this as intended.<br>**(b) A `$ref` item member that shares an outer parameter's name.**<br>- p10_ref_plan: exit 4, `{"name":"Bob","outfits":[{},{}],"size":"4"}`. The items' members are hijacked to the top level.<br>- The inline twin p10_inline_plan: exit 0, correct.<br>- This is the common pydantic/FastMCP `$defs` shape (`name`, `id` and `type` are often shared).<br>- Nothing tests it: `xml_array_unmodeled_items_nested` uses non-colliding names, and the corpus mutant covers only modeled `ValueShape::Object` items (`tool_call_corpus/mod.rs:2698-2706`).<br>**Unsupported claims.**<br>- `qwen3-coder-xml.v1.md:45-53` says, for unmodeled items, that "every repetition is one object item", with no qualification.<br>- `:60-62` and `xml.rs:78-80` say a recursive `$ref` "has no such members". This is false: the schema declares `children`; only the codec cannot see it.<br>- RULES.md calls `missing-parameter-close` "single reading", but a zero-repair reading exists here.<br>- The handoff called (a) "intent genuinely ambiguous". By the engine's own cost model (0 against 8) it is not. | Give every `Unmodeled` frame (array items **and** object parameters: the same root cause, pre-existing for objects) the member names its **exact schema** declares:<br>- resolve `$ref` one hop per frame (frames open lazily, so recursion is safe);<br>- take the union of `properties` keys across the object branches of `anyOf`/`oneOf`/`allOf`;<br>- use the names only in `owner()`/`boundary()`. Typing stays with `schema_typed`.<br>Then:<br>- re-bless `xml_array_recursive_ref_items_are_siblings` to the child reading and rename it;<br>- add a golden for the outer-name collision;<br>- add a `$ref`-item tool to the correction corpus;<br>- rewrite `qwen3-coder-xml.v1.md:44-62` and `xml.rs:62-80`.<br>Abstention (a multiple-reading rule, never Resolved) is the only acceptable fallback. Keeping Resolved is not. | 1. `B recover-tool-calls --tools $T p9r.txt` exits 0. `value.calls[0].arguments` is `{"children":[{"name":"a","children":[{"name":"b"}]}]}`, the only rules are `nested-parameter`×2 and `array-item`×2 (no `missing-parameter-close` or `stray-closer`), and the result equals the p9i run.<br>2. The same command with `--effort exact` is Resolved, exit 0.<br>3. `B recover-tool-calls --tools $T p10_ref_plan.txt` exits 0 with `{"name":"party","outfits":[{"name":"Alice","size":3},{"name":"Bob","size":4}]}`, equal to the p10_inline_plan result.<br>4. `make corpus` with the `$ref` mutant reports 0 incorrect. |
| **G3** | HIGH | Requirement (issue §2), misleading doc | **Strict "exactly one call, no content outside it" passes when a second, malformed call sits beside the first.**<br>- `B recover-tool-calls --tools $T --dialect hermes.v1 --tool-choice read_file --tool-choice-exactly-one --tool-choice-no-content --emit openai p7.txt`: exit 0, `tool_choice.passed: true`, verdict `run`, one call emitted.<br>- The rejected attempt at 75..163 holds a second `{"name": "read_file", …} junk junk {{{`.<br>**Doc conflict.** `INTEGRATION.md:1155-1157` says "A rejected attempt already fails the turn's finish". That is true of `finish` (`unresolved`), but the verdict table (`INTEGRATION.md:1249`) gives `run` and exit 0, and the handoff tells integrators to branch on `verdict`.<br>**Same hole:** an `unsupported_dialect` attempt under the Anthropic exactly-one form, which allows content.<br>**Code reading.**<br>- `strict()` (`recover.rs:4362`) reads `value.content`, which is not part of `calls_key` (`recover.rs:4942`).<br>- `same_calls` (`:4975`) merges readings that differ only in how the rejected/content bytes divide, and keeps one member. So a reading that fails no-content can be dropped in favour of one that passes.<br>- The check runs per reading at `:3606`, before the merge. | **Core fix (`Recovery::strict`).** Under `exactly_one` or `!allow_content`, give every `value.rejected` entry a diagnostic, whatever its reason (including `unsupported_dialect`).<br>**Merge.** The strict result must not depend on which merged member survives. Either put the strict outcome into the reading key, or re-run the check after `same_calls`.<br>**Docs.** Fix `INTEGRATION.md:1155-1157` and the `StrictForced` Rustdoc.<br>**Fuzz.** Extend `check_strict`: if the check passed with `exactly_one` or no-content, `rejected` is empty.<br>**Golden.** Add `forced_strict_rejected_attempt_beside_call`. | The p7 command above exits 4 with `tool_choice.passed: false` and a rejected-attempt diagnostic, verdict `nothing_usable`, and `emitted.output: []`. MCP `recover_tool_calls` with the Anthropic `{"type":"tool","name":"read_file","disable_parallel_tool_use":true}` on the same output also gives `passed: false`. |
| **G4** | HIGH | Validation (fuzz) | **The coverage-guided tool-call fuzz target cannot reach the new codec path.**<br>- Its fixed tool set (`examples/fuzz_tool_call.rs:145-183`) has `tags: Param::array(Param::string())` and object parameters, but no `Array(Object\|Unmodeled)`.<br>- So these never execute: the item arms of `nesting()` (`xml.rs:1660-1663`), the item branch of `pop` (`:857-865`), and the `adopt` shift.<br>- None of the 403 seeds holds an array item written as nested elements. The only 2 nested-opener seeds are object parameters.<br>**Worse than risk 7 assumed.** The historical-fit risk estimated "<1000 executions"; the actual count is zero.<br>**Risk area.** This is parsing of untrusted input in the most-refixed region of the codec (adversary passes 4-6). | Add a read-only tool with `Param::array(Param::object(…))`, and a tool declaring `$ref` items if the fixed `ToolSet` can mix builder and declaration tools.<br>Add seeds for each issue §1 shape: top level, inside an object, mixed, unclosed last item, unclosed item before a sibling, same-name member, and string items.<br>Run a bounded campaign with the #6 oracle failure isolated or fixed, so it does not mask the new path. | `python3 scripts/test-fuzz.py` passes with the new seeds. Each new seed piped to `cargo run -q -p purrassist-core --example fuzz_tool_call` prints `ok`. The llvm-cov report of `scripts/fuzz.py --target tool-call` shows `xml.rs:857-865` and `:1660-1663` executed. |
| **G5** | HIGH | Resources (published linear bound) | `reviews/performance-review.md` REQUIRED 1. `ArgTree::adopt` (`ir.rs:321-341`) rescans every member on each adoption, so it is O(k²) and uncharged: `from_parameters` takes 1.9 s at k=32000. It is new in this PR (linear on base), and the `Governor` cannot see it. AGENTS.md: "Resource exhaustion is an explicit outcome". | Apply the performance review's fix: keep a running item count per name in `ArgTree`, so `earlier` is O(1). | The performance review's `from_parameters` microbench grows linearly (at most about 2.2× per doubling up to k=32000). A new repeated-parameter case in `tool_call_cost.rs` asserts that the work and scratch ratios stay close to the byte ratio. |
| **G6** | MEDIUM | Provenance, overbroad claim | **PR-body claim:** repeated occurrences "are now numbered across occurrences, so errors and evidence point at their own item". That fails when the tool is known only after name resolution.<br>**Executed:** `B recover-tool-calls --tools $T --policy tolerant p6.txt` (`<function=Nums>`, items `11` and `2222`):<br>- The `decode-typed` evidence for `/arguments/xs/1` spans bytes 43..45 (`11`), not 74..78 (`2222`).<br>- The decoded node is rebased to the wrong offset as well.<br>**Cause:** `Resolver::decoded` (`resolve.rs:2449-2466`) takes the first leaf with `leaf.pointer == site.source`, and non-array occurrences all have source `/xs` (`resolve.rs:2315`).<br>**Base:** identical. The bug is pre-existing and unchanged, but it is a remaining instance of the defect class the PR says it fixed. | Find the leaf by span: `leaf.origin == XmlText && value.span.as_ref() == Some(&leaf.raw)`, as `unmodeled_sites` already does (`resolve.rs:3085`). Then narrow the PR-body claim, or make it true. | The p6 command gives `decode-typed` spans of 74..78 for `/xs/1` and 43..45 for `/xs/0`. A new golden (repeated scalar items under a near-miss tool name) asserts each span by name. |
| **G7** | MEDIUM | Correctness (pointer consumer) | **A reasoning-marker spelling inside a JSON string of any item breaks the call, in the issue's own shape.**<br>- p12 (Bob's `garments: ["a <think> scarf"]`): exit 4, Unresolved `{}`, with a reasoning range 238..303 cut out of the call.<br>- A single occurrence (p12s) resolves correctly.<br>**Cause:** `marker_cut` (`recognizer.rs:1404-1446`) resolves `leaf.pointer` with `root.pointer(…)`. In the raw tree a repeated parameter is a duplicate member, and `Node::get` returns `None` for duplicates (`json/mod.rs:134-147`). So a JSON leaf of a repeated parameter is never seen as "inside a string".<br>**Base:** p12j (repeated JSON-array occurrences) fails the same way. The bug is pre-existing, but the new feature now reaches it. | Decide "inside a JSON string" without resolving the pointer through duplicate members. Either carry string-ness on `ArgLeaf` from the JSON codec, or find the node by span within its occurrence. | `B recover-tool-calls --tools $T p12.txt` and `… p12j.txt` each exit 0, Resolved, with Bob's `"garments":["a <think> scarf"]` and `reasoning: []`. Add a golden. |
| **G8** | MEDIUM | Maintainability, validation | **`calls_key` (`recover.rs:4935-4963`) changed after the T3 review, and nothing reviews or tests the change.**<br>- It now keys on `reasoning_blocks`.<br>- No review covers the change.<br>- The disposition records unchanged goldens, recover and rules counts (100/50/41), so no test distinguishes the old key from the new.<br>- The doc claim "so readings that divide the same bytes into different blocks stay distinct" has no executable evidence.<br>**Effect:** this changes how readings are de-duplicated. Such readings now stay separate, which can turn a turn `Ambiguous`. | Add a focused in-crate test with two readings that have identical calls and identical merged `reasoning` but different blocks. If no input can produce such readings, say so and why in the doc comment. Get a focused review. | `cargo test -p purrassist-core --lib calls_key` (the new test) passes. It fails when `calls_key` is reverted to hash `value.reasoning`. |
| **G9** | LOW | Validation (fuzz seed meaning) | The claim "no existing seed changes meaning" (a trailer byte `0xF8..=0xFF` under the forced header) is true today. Executed over `scripts/fuzz.py` `TOOL_CALL_SEEDS`: 403 seeds, 63 forced, 5 with a trailer, and those 5 are exactly `TOOL_CALL_STRICT`. No test in `scripts/test-fuzz.py` pins it: grep for `STRICT\|0xF8\|trailer` finds nothing. | Add a test to `test-fuzz.py`: every seed with `control>>6==3` and a last byte of 0xF8 or more is in `TOOL_CALL_STRICT`. | `python3 scripts/test-fuzz.py` passes with the new test, and fails when `b'\xe6\x23x\xf8'` is appended to `TOOL_CALL_REGRESSIONS`. |
| **G10** | LOW | Docs, compatibility | **The PR body names three breaks and omits others:**<br>- `UnresolvedReason` gains `ToolChoiceCardinality` (`resolve.rs:484`) and is not `#[non_exhaustive]`.<br>- The new public `StrictForced` and `ReasoningBlock` have public fields and no `#[non_exhaustive]`.<br>**INTEGRATION.md has no compatibility statement for:**<br>- wire: `reasoning_blocks` is *required* under `additionalProperties:false` (`registry.rs:44`); a new reason code `tool_choice_cardinality`; a new constraint family `forced[…]`;<br>- behaviour: Anthropic `disable_parallel_tool_use:true` used to be an error and is now accepted; repeated-array evidence pointers are renumbered; tie order changed in 2 Ambiguous goldens.<br>These breaks are accepted pre-release, but they must be stated. | Complete the list in the PR body. Add a short "Compatibility" note to INTEGRATION.md: additive members may appear in `tool_call.recover.v1` before 0.1.0, plus the changes above. | `grep -n ToolChoiceCardinality` finds it in the PR body, and `grep -n -i compatib docs/tool-calls/INTEGRATION.md` finds the note. |
| **G11** | LOW | Docs | `reasoning_blocks[].text` for an Anthropic `redacted_thinking` block is opaque encrypted data: the golden `anthropic_redacted_thinking` captures `reasoning_text: ["EmwKAhgB"]`. The docs describe only JSON-escaped `thinking` (`recover.rs:1529-1530`, `ir.rs` `ReasoningSegment::text`). `ReasoningSegment`, which is public and gains a field, has no example (completeness review A1). | Add one sentence to the `ReasoningBlock` and `ReasoningSegment::text` Rustdoc, and to INTEGRATION.md ("Templates that open reasoning"). Add an example. | A named assertion in `tool_call_goldens` checks that the redacted golden's `reasoning_text` is `["EmwKAhgB"]`. `cargo test -p purrassist-core --doc ReasoningSegment` passes. |
| **G12** | LOW | Validation (CI) | No hosted job ran (msrv, macOS, Windows, corpus, fuzz, qualification): not run. **MSRV compilation was verified here:** `rust-validate 1.98.0` (an existing sanctioned validation toolchain, not installed by this review) ran `cargo check --workspace --all-targets --locked` with `RUSTFLAGS=-D warnings` on a `/tmp` copy of 248d07a, exit 0. That includes `const fn nesting` (`xml.rs:1656`). The msrv job's `cargo test` run and the macOS/Windows matrix were not executed. | Run the hosted matrix when it runs. Optionally run the MSRV tests locally. | `rust-validate 1.98.0 cargo test --workspace --locked` exits 0, or the hosted `msrv`, `native (macos-14)` and `windows-engines` jobs are green. |
| **G13** | LOW | Validation (F20 published replay) | `mcp-responses.jsonl` was recaptured, but the replayed `recover_tool_calls` request has no reasoning block and no strict choice. The published-binary replay shows only `reasoning_blocks: []` and the new input-schema member. | Add one replay request with `<think>…</think>`, a call and `tool_choice_strict`, then recapture. | `python3 scripts/test-verify-published.py` passes, and the fixture's response holds a non-empty `reasoning_blocks` and a `tool_choice=forced[…]` constraint. |

Pending and referenced, not duplicated: `reviews/security-review.md` (deep nesting, panics, untrusted input). The code reading on depth is under FP11.

## Historical-fit risks (all 16)

| # | Risk | Disposition |
|---|---|---|
| 1 | Required output member with no compatibility policy | Real, LOW, folded into **G10**. Pre-release precedent allows adding members in place; only the statement is missing. |
| 2 | Public types without `#[non_exhaustive]` | Real, LOW: **G10**. The PR body also omits `UnresolvedReason`. |
| 3 | Pointer renumbering; pointer-keyed consumers not audited | Every consumer audited. Two are real, both pre-existing and unchanged by the renumbering: **G6** (`decoded()` takes the first match) and **G7** (`marker_cut` calls `root.pointer` on duplicate members). False positives: `reemit_with`, evidence `destination`, and per-item `schema-typed` pointers (FP2, FP3). |
| 4 | O(k²) `adopt` | Real: **G5**, owned by the performance review. |
| 5 | Depth exhaustion through item frames | No gap found by code reading (FP11). Deferred to the security review. |
| 6 | Interactions with F9, F13, F14 and F19 | 14 adversarial probes; none gave an incorrect Resolved answer (FP6). |
| 7 | Fuzz failure #6 masks the new path | Worse than stated: the path is unreachable. **G4**. |
| 8 | Blind spots in the strict check | Real: **G3** (rejected attempts; the content check depends on which reading the merge keeps). Ambiguous per-reading checks and the cardinality mutation are false positives (FP10). |
| 9 | Anthropic mapping and constraint aliasing | By design and documented: FP9. |
| 10 | Fuzz trailer byte | The claim holds today, but no test pins it: **G9**. |
| 11 | F15 corpus narrowing; duplicate request | False positive (FP5). The remaining corpus gap (no `$ref` items) is folded into **G2**'s acceptance. |
| 12 | WASM parity not run | **G1**. Core parity of `reasoning_blocks` was executed and matches (FP8). |
| 13 | Published transcript coverage | **G13**. |
| 14 | Mass re-bless of 481 goldens | Re-verified independently: FP4. |
| 15 | Docs and claims need proof | Unsupported or misleading claims in **G2** (dialect doc, codec doc, RULES "single"), **G3** (INTEGRATION "already fails the finish") and **G6** (PR-body pointer claim). The mixed, unclosed-before-sibling and same-name (inline) claims have goldens. |
| 16 | `opener_end` shortcut | FP1. |

## Seed items adjudicated

- **G1 Node repin.** Confirmed: the gate is blocked, and the user's decision supplies the fix. The core leg has already been verified by execution.
- **`calls_key` change.** Real, MEDIUM (**G8**): no test, no review, and a doc claim without evidence. No incorrect outcome was found.
- **Same-name member vs recursive `$ref`.**
  - The *inline* reading is correct under the codec's existing rules (FP7).
  - The *recursive `$ref`* reading is a defect, not a policy choice: **G2**, CRITICAL. The input is balanced and has a zero-repair child reading under the exact schema, yet the engine reports a cost-8 repaired reading as Resolved. The base abstained.
  - "A validated guess is still a guess" applies with extra force: this reading is not even the best guess.
- **Narrowed corpus mutants and duplicate request.** FP5 (evidence below).
- **Fuzz trailer claim.** True when executed; the missing pin is **G9**.
- **Consumers of the pointer renumbering.** **G6** and **G7** are real (pre-existing); the others are false positives.
- **`<think>` in the wasm-harness and `reasoning_blocks` parity.** Executed: core native, WASI and unknown-unknown outputs are byte-identical (FP8). The full gate remains **G1**.
- **Public API breaks.** **G10**.

## False positives (with evidence)

- **FP1: the `opener_end` shortcut is exact.**
  - The marker matcher's literals are exactly the flattened `(open, close)` pairs (`dialect.rs:489-496`).
  - `Pending::Marker.start = at+1-len(literal)` (`recognizer.rs:903`).
  - So `opener_end(marker, pair) = start + len(markers[pair].0) = Pending::Marker.end`.
  - All markers are literal bytes, including Anthropic's JSON-fragment markers.
- **FP2: re-emission with repeated items is correct.** `reemit_with` works by span, not by pointer (`emit.rs:1526-1575`). Probe results:
  - Editing `/outfits/1/wearer` splices in place and round-trips.
  - Replacing `/outfits` with 1 or 3 items regenerates the whole body as one JSON object, and it round-trips.
  - The scalar `tags` case also round-trips.
  - `--emit openai`, `qwen3-coder-xml.v1` and `hermes.v1` of the issue example all print the issue's exact JSON.
- **FP3: evidence destinations are per item.** `destination(scope.raw, &site.pointer)` uses the resolved pointer. Executed evidence:
  - The `$ref` probe shows `schema-typed` at `/arguments/outfits/0/size` and `/outfits/1/size`, each with its item's own span.
  - Recognizer leaves are `/outfits/0/wearer` (76..81, `Alice`) and `/outfits/1/wearer` (196..199, `Bob`).
  - Scalar items are `/tags/0` and `/tags/1`.
- **FP4: the mass re-bless is additive.** An independent script stripped `reasoning_text` from every file that `git diff --diff-filter=M 943fa04 248d07a` reports:
  - 466 modified;
  - 464 identical;
  - 2 differ only in order (`harmony_python_code_holds_call_closer`, `qwen_value_holds_call_closers_then_text`);
  - 0 semantic changes.

  Tie order follows the documented rule: "cost, then content digest".
- **FP5: the F15 narrowing changes no base truth.**
  - Both narrowed sites emit the variant `inline-closers-then-call`. The completeness review's check C found 34 such cases at base, none with a value-final newline, so no case evaluated at base was dropped.
  - The PR body names both narrowings, and the dialect's framing-newline rule (`qwen3-coder-xml.v1.md`) grounds them.
  - The duplicate request (18165 distinct against 18166 cases) comes from two seeds drawing the same one-item witness with the same truth. The test requires unique ids and at least 500 distinct requests, and no doc claims every case is distinct.
- **FP6: item frames keep the F9, F13, F14 and F19 rules.** 14 executed probes; none gave an incorrect Resolved answer:

  | Probe | Result |
  |---|---|
  | Doubled closer after an item | stray closer; Resolved, correct |
  | Text after an item closer | rejected |
  | Text inside an item after a member | Ambiguous |
  | Closer text in a member | Ambiguous |
  | Indented items | Resolved |
  | Unclosed last item, then a second call | two calls |
  | Empty item, misspelled member, misspelled opener | Unresolved |
  | Mixed JSON and nested items; JSON array, then nested items | correct order |
  | String-array members inside an item | collected |

  EOF behaviour matches the object path at both base and head: an unclosed value is unterminated, and a closed member is repaired.
- **FP7: the inline same-name reading is correct.** p9i is balanced nested XML read with zero repairs. The sibling reading would cost `missing-parameter-close` plus `stray-closer`. Resolved is consistent with the ranking in RULES.md.
- **FP8: `reasoning_blocks` keeps WASM parity in the core leg.** See G1's evidence: the three targets are byte-identical, with `reasoning_blocks` present.
- **FP9: the Anthropic mapping is intended.** `disable_parallel_tool_use:true` maps to exactly-one. A lenient `ForcedStrict` renders `forced:NAME` and checks exactly what `Forced` checks. Both were decided in the plan, are documented (`INTEGRATION.md:1149-1151, 1174-1178`) and are doctested.
- **FP10: the cardinality mutation cannot make readings diverge.** `tool_choice` runs per reading (`recover.rs:3606`) before `same_calls` (`:4542`). The mutation depends only on call order, so identical calls get identical outcomes. The content-dependent case is real and is part of G3.
- **FP11: no depth gap through item frames, by code reading.**
  - `Depth` is charged when an item frame is pushed, as for objects, and `pop` propagates `StopReason`.
  - Members of an `Unmodeled` frame never nest (they have no declared shape), so the schema's static depth bounds nesting.
  - The security review has the final word.
- **FP12: a tool known only late simply does not nest.** p5 (`plan_outfit`) is Unresolved under both Structural and Tolerant (schema validation), which is safe. This is documented (`qwen3-coder-xml.v1.md:54-56`) and matches object parameters.
- **FP13: `status: resolved` after a failed strict check.** This is pre-existing `Forced` behaviour, documented with `verdict` as the signal to act on.
- **FP14: wire versioning.** No doctrine requires an operation-id bump, and every earlier `ToolCalls` member was added in place before release. The missing statement is G10.
- **FP15: MSRV 1.98 compiles.** `const fn nesting` with `match &**item` on a `Box` compiles under 1.98.0 with `-D warnings` across the whole workspace, all targets (see G12).

## Not run

- `make check` and `make fuzz` (as instructed).
- The full `make wasm-test`: the syntax and host WASI legs need the WASI sysroot download, and the gate needs G1.
- MSRV `cargo test` (compilation was verified).
- Hosted CI.


## Appendix: reproduction

`/tmp/ga8-tools.json` (OpenAI form):

```json
[{"type":"function","function":{"name":"plan_outfits","description":"p","parameters":{"type":"object","properties":{"outfits":{"type":"array","items":{"type":"object","properties":{"wearer":{"type":"string"},"garments":{"type":"array","items":{"type":"string"}}},"required":["wearer"]}}},"required":["outfits"]}}},{"type":"function","function":{"name":"tree","description":"t","parameters":{"type":"object","properties":{"children":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"children":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}},"required":["name"]}}},"required":["children"]}}},{"type":"function","function":{"name":"nums","description":"n","parameters":{"type":"object","properties":{"xs":{"type":"array","items":{"type":"integer"}},"tags":{"type":"array","items":{"type":"string"}}},"required":["xs"]}}},{"type":"function","function":{"name":"read_file","description":"r","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}},{"type":"function","function":{"name":"rtree","description":"rt","parameters":{"type":"object","$defs":{"node":{"type":"object","properties":{"name":{"type":"string"},"children":{"type":"array","items":{"$ref":"#/$defs/node"}}},"required":["name"]}},"properties":{"children":{"type":"array","items":{"$ref":"#/$defs/node"}}},"required":["children"]}}},{"type":"function","function":{"name":"ref_plan","description":"x","parameters":{"type":"object","$defs":{"outfit":{"type":"object","properties":{"name":{"type":"string"},"size":{"type":"integer"}},"required":["name"]}},"properties":{"name":{"type":"string"},"outfits":{"type":"array","items":{"$ref":"#/$defs/outfit"}}},"required":["name","outfits"]}}},{"type":"function","function":{"name":"inline_plan","description":"x","parameters":{"type":"object","properties":{"name":{"type":"string"},"outfits":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"size":{"type":"integer"}},"required":["name"]}}},"required":["name","outfits"]}}}]
```

Inputs. Write each with `printf '<string>' > NAME.txt`; `\n` is a newline.

- `p6.txt`: `<tool_call>\n<function=Nums>\n<parameter=xs>\n11\n</parameter>\n<parameter=xs>\n2222\n</parameter>\n</function>\n</tool_call>`
- `p7.txt`: `<tool_call>\n{"name": "read_file", "arguments": {"path": "a"}}\n</tool_call>\n<tool_call>\n{"name": "read_file", "arguments": {"path": "b"}} junk junk {{{\n</tool_call>`
- `p9r.txt`: `<tool_call>\n<function=rtree>\n<parameter=children><parameter=name>a</parameter><parameter=children><parameter=name>b</parameter></parameter></parameter>\n</function>\n</tool_call>`
- `p9i.txt`: the same as `p9r.txt` with `<function=tree>`.
- `p10_ref_plan.txt`: `<tool_call>\n<function=ref_plan>\n<parameter=name>\nparty\n</parameter>\n<parameter=outfits>\n<parameter=name>Alice</parameter>\n<parameter=size>3</parameter>\n</parameter>\n<parameter=outfits>\n<parameter=name>Bob</parameter>\n<parameter=size>4</parameter>\n</parameter>\n</function>\n</tool_call>`
- `p10_inline_plan.txt`: the same as `p10_ref_plan.txt` with `<function=inline_plan>`.
- `p12.txt`: the issue's `plan_outfits` example, with Bob's garments `["a <think> scarf"]`.
- `p12j.txt`: `<tool_call>\n<function=plan_outfits>\n<parameter=outfits>\n[{"wearer":"Alice"}]\n</parameter>\n<parameter=outfits>\n[{"wearer":"Bob","garments":["a <think> scarf"]}]\n</parameter>\n</function>\n</tool_call>`

Results observed at head 248d07a:

| Input | Flags | Result |
|---|---|---|
| issue example | — | exit 0, Resolved, the issue's JSON |
| p9r | — | exit 0, `[{a},{b}]` |
| p9r | `--effort exact` | exit 4, `repair_not_permitted` |
| p9i | — | exit 0, `[{a,children:[{b}]}]` |
| p10_ref_plan | — | exit 4, `{"name":"Bob","outfits":[{},{}],"size":"4"}` |
| p10_inline_plan | — | exit 0, correct |
| p12, p12j | — | exit 4, `{}` |
| p6 | `--policy tolerant` | `decode-typed` for `/xs/1` at 43..45 |
| p7 | `--dialect hermes.v1` plus the strict flags | exit 0, `passed: true`, verdict `run` |

At base 943fa04: p9r exits 4 (`schema_validation`), p12j exits 4 with `{}`, and p6 shows the same aliasing.

VERDICT: GAPS-OPEN
