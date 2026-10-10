# Completion audit, Stage 2 EXIT: PR #9, issue #8

## Scope and authority

- **Auditor:** a fresh, independent completion auditor (STAGE=2 exit). I changed no source. This report is the only file I wrote in the repo. Scratch lived in `/tmp/cax8`, plus a detached base worktree at `/tmp/cax8/base` (943fa04); both were deleted after the audit.
- **Reviewed:** `943fa04..42899ab`, 22 commits.
  - PR #9 is open, and its `headRefOid` is `42899ab791b6…`.
  - The worktree was clean apart from the untracked `.stage/`.
- **Inputs read:**
  - `issue.md` (the body, all 3 asks, the listed goldens, the already-satisfied list, and 5 comments) and `plan.md`.
  - `remediation-plan.md` (G1-G26, R1-R15, accumulated compatibility items) and `validation.md` (read in full).
  - `gap-analysis.md` (including the Appendix) and the first `completion-audit.md` (F1-F3).
  - Verdict lines of `reviews/*.md` and `tasks/*.md`.
  - `AGENTS.md`, `.goals`, `docs/IMPLEMENTATION.md`.
  - `gh issue view 10` and `gh issue view 11`.
  - Root `.deficiencies`: no #8 entry. The worktree has no `.deficiencies`.
- **Binding user decisions (verbatim):**
  - (1) "you may not modify the local system.  just change the node version pinned to match ours."
  - (2) "Fix them in this PR", on the repeated-parameter quadratic and Draft-07 root `$ref`.
  - (3) "Raise the published constants".
  - (4) "Fix (1) and (2) here, document (3)".
  - (5) "File the deepseek issues as a new issue, and accept it for now." This became #10.
  - (6) "File an issue, accept for now (Recommended)". This became #11.
- **Entry points I executed:**
  - **HEAD CLI:** `purrassist recover-tool-calls`, a release build from 42899ab. The target dir came from `cargo metadata`; the binary was copied to `/tmp/cax8/pa-head`.
  - **HEAD MCP:** stdio `purrassist mcp --root /tmp/cax8/root`, then `tools/list` and `tools/call recover_tool_calls`.
  - **Base CLI:** built from a detached `git worktree add` of 943fa04 in `/tmp`, then removed.
  - **Fuzz probes:** `fuzz-tool-call` probes from HEAD and from base, built with the dev profile, opt-level 1 and debug assertions (the `scripts/fuzz.py` build flags without coverage instrumentation).
- **Not run, as instructed:** `make check`, `make fuzz` and `make wasm-test`. No system install: the MSRV toolchain was already present in the sanctioned `rust-validate` home, so nothing was installed.

## Per-criterion results

Unless noted:
- `H` is `pa-head recover-tool-calls --tools tools.json`.
- `tools.json` holds the gap-analysis Appendix tools, plus `plan_event` (object `plan{cast, outfits}`) and `tag` (`tags: string[]`).
- `g2tools.json` holds:
  - `rm(path, keep: $ref k{path})`;
  - `rmfree(path, keep: free-form object items)`;
  - `save(tags, notes: $ref note{text, tags})`;
  - `create_doc(title, sections: $ref sec{title, body})`;
  - `rtree`;
  - an `anyOf` item tool.

The "MCP" rows were run through stdio MCP with the same output and tools.

### Issue section 1: arrays of objects written as repeated nested `<parameter=>` elements

| Criterion | Command | Observed | Expected | Exit | Status |
|---|---|---|---|---|---|
| The issue's exact `plan_outfits` example | `H a1.txt` (issue bytes, issue schema), and MCP | Arguments are byte-identical to the issue JSON after compact serialization. Status `resolved`. Top-level and per-call alternatives `[]`. Rules: `nested-parameter`×2, `array-item`×2, `collect`×1. Base: `unresolved`, exit 4, items read as strings | `{"outfits":[{"wearer":"Alice","garments":["a red coat"]},{"wearer":"Bob","garments":["a grey scarf"]}]}`, Resolved, single reading | 0 | VERIFIED |
| The same shape inside an object parameter | `H a2.txt`, and MCP with `emit: openai` | `{"plan":{"cast":["Alice","Bob"],"outfits":[{Alice…},{Bob…}]}}`, resolved. The OpenAI emission carries the exact arguments | Nested inside the object, Resolved | 0 | VERIFIED |
| One JSON-object item mixed with one nested-element item | `H a3.txt`, and MCP | The same two items in source order, resolved | One item per repetition, in order | 0 | VERIFIED |
| Unclosed last item | `H a4.txt` (the item is ended by `</function>`), and MCP | `[{"wearer":"Alice"},{"wearer":"Bob","garments":["a grey scarf"]}]`, resolved, with `parameter-closed-by-element` | Ends with its enclosing text | 0 | VERIFIED |
| Negative: `String` items | `H a5.txt`, and MCP | `{"tags":["<parameter=wearer>Alice</parameter>","plain"]}`, resolved | Nested tags stay a verbatim string | 0 | VERIFIED |
| Required members enforced; per-item pointer | `H a7.txt` (item 2 lacks `wearer`) | Call `unresolved missing_required`: "`/outfits/1/wearer` is missing". `nothing_usable` | Not Resolved | 4 | VERIFIED |
| G2: recursive `$ref` tree (p9r), default effort and `--effort exact` | `H p9r.txt`, `H --effort exact p9r.txt`, and MCP | `{"children":[{"name":"a","children":[{"name":"b"}]}]}`, resolved. Rules are only `nested-parameter`×2 and `array-item`×2. Equal to the inline twin p9i. Base: exit 4 | Nested, Resolved, also under `exact` | 0 / 0 | VERIFIED |
| G2: F1 repro 1, a well-formed recursive tree with a sibling | `H --tools g2tools.json tr3.txt` | `[{"name":"a","children":[{"name":"b"}]},{"name":"c"}]`, resolved | The faithful reading | 0 | VERIFIED |
| G2: p10_ref_plan, colliding member names | `H p10_ref_plan.txt`, and MCP | `{"name":"party","outfits":[{"name":"Alice","size":3},{"name":"Bob","size":4}]}`, resolved, integer sizes. Equal to the inline twin. Base: exit 4 | As written | 0 | VERIFIED |
| G2: security `rm`/`keep` with a `$ref` item | `H --tools g2tools.json rmkeep.txt`, and MCP | `{"path":"/tmp/x","keep":[{"path":"/home/user/important"}]}`, resolved | `path` stays inside the item | 0 | VERIFIED |
| G2: F1 repro 2 (an outer array name collides) and repro 3 (abstention narrowing) | `H --tools g2tools.json save.txt` and `doc.txt` | `{"tags":["work"],"notes":[{"text":"buy milk","tags":["home"]}]}`, and `{"title":"Report","sections":[{"title":"Intro","body":"hello"}]}`, both resolved | Each name stays in its scope | 0 / 0 | VERIFIED |
| G2: opaque free-form item | `H --tools g2tools.json opaque.txt`, and MCP | `best_guess`, call `unresolved rule_not_admitted` (`unmodeled-scope-boundary`), `nothing_usable`, usable 0/0, arguments `{"keep":[{}],"path":"/home/user/important"}` | Labelled guess, never run on a mutating tool | 4 | VERIFIED |
| Free-form item without a colliding name nests | `H --tools g2tools.json opaque_nocoll.txt` | `{"path":"/tmp/x","keep":[{"label":"mine"}]}`, resolved | Unmodeled items nest | 0 | VERIFIED |
| G7: p12 and p12j, `<think>` inside a JSON string of a repeated item | `H p12.txt`, `H p12j.txt`, and MCP | Bob's `"garments":["a <think> scarf"]`, resolved, `reasoning: []`, `reasoning_blocks: []`. Base: both exit 4 | Stays value text | 0 / 0 | VERIFIED |
| G6: per-item evidence spans | `H --policy tolerant p6.txt` | `decode-typed` `/xs/0` spans 43..45 and `/xs/1` spans 74..78 | Each item's own span | 4 (mutating best guess; spans are the point) | VERIFIED |
| G16: depth guard | `cargo test --release -p purrassist-core --test tool_call_recover nested_item_depth_is_a_stack_guard`. CLI: a 5,000-deep recursive tree | Test: 1 passed. CLI: `budget_exceeded: Depth`, no panic | Explicit exhaustion | 0 / 4 | VERIFIED |
| Goldens present and passing | `cargo test --release -p purrassist-core --test tool_call_goldens`, plus reading the files | 120 passed. All 5 issue-listed goldens exist with named assertions. `xml_array_nested_object_items` equals the issue JSON | Present and passing | 0 | VERIFIED |
| Zero incorrect corrections, including the new `$ref` cases | `cargo test --release -p purrassist-core --test correction_evaluation` (HEAD) | 10 passed, including `tool_call_recovery_evaluation_has_zero_incorrect_corrections`, which asserts `incorrect == 0` over 18,176 cases. The corpus holds 10 `file_tasks` `$ref`-scope cases (colliding, outer-array, recursive, `$ref`-object, opaque→Unresolved) | 0 incorrect | 0 | VERIFIED |

### Issue section 2: strict forced tool choice

`S` is `--tool-choice read_file --tool-choice-exactly-one --tool-choice-no-content`.

| Criterion | Command | Observed | Expected | Exit | Status |
|---|---|---|---|---|---|
| Exactly one: two calls fail | `H S --emit openai s1.txt`. MCP: OpenAI forced plus `tool_choice_strict {true,false,true}` | `forced[exactly-one,no-content]:read_file` with `passed: false`. Call 1 is `tool_choice_cardinality`. `nothing_usable`, usable 0/0, `emitted.output: []`. The plain `--tool-choice read_file` control exits 0 with 2 calls | Fails, emits nothing | 4 | VERIFIED |
| No content: call plus prose fails | `H S s2.txt`, and MCP | "content at byte 0 is not allowed…", `nothing_usable` | Fails | 4 | VERIFIED |
| Whitespace-only content passes | `H S --tool-choice-no-reasoning s3.txt`. MCP with all three options | `passed: true`, `run`, 1 usable | Passes | 0 | VERIFIED |
| U+00A0 is content | `H S s4.txt` (`\n` + call + `\n` + U+00A0) | Fails at byte 76 | Fails | 4 | VERIFIED |
| Optional no-reasoning | `H --tool-choice read_file --tool-choice-no-reasoning r1.txt`. MCP `allow_reasoning:false` | "reasoning at bytes 0..34 is not allowed", `nothing_usable` | Fails | 4 | VERIFIED |
| Recorded in the tool-choice validation | Receipt `validations` for s1, p7, r1nr and s3 | `{"validator":"purrassist.tool_call.v1","constraint":"tool_choice=forced[…]:read_file","passed":…}` | Recorded like the existing checks | n/a | VERIFIED |
| G3: p7 (a valid call beside a rejected second attempt) | `H --dialect hermes.v1 S --tool-choice-no-reasoning --emit openai p7.txt`, and MCP | `passed: false`, with diagnostics "1 rejected call attempt beyond the one call…" and "rejected call attempt at bytes 75..163…". `nothing_usable`, usable 0/0, `emitted.output: []`. Non-strict `Forced` is unchanged from base (exit 0) | Fails | 4 | VERIFIED |
| Anthropic `disable_parallel_tool_use` | MCP `{"type":"tool","name":"read_file","disable_parallel_tool_use":true}` on two calls, call plus prose, and p7. Also `false`, and on `auto` | Two calls: `forced[exactly-one]` fails. Prose: passes. p7: fails, usable 0. `false`: `forced:read_file` passes. `auto`: `isError` "accepted only for type tool" | Exactly one, content allowed | n/a | VERIFIED |
| G15: strict no-reasoning with `--reasoning content` | `H --tool-choice read_file --tool-choice-no-reasoning --reasoning content r1.txt`, and MCP | `status: unresolved`, `invalid_input`: "tool choice forbids reasoning … never recognized" | `invalid_input` | 4 | VERIFIED |
| Request errors | CLI strict flags with no NAME, or with `auto`. MCP `tool_choice_strict` beside `auto`, and `exactly_one:false` beside `disable_parallel_tool_use:true` | CLI: exit 1 with a message. MCP: `isError` with a specific message | Explicit errors | 1 / n/a | VERIFIED |

### Issue section 3: marker-free reasoning range

| Criterion | Command | Observed (bytes sliced from the input) | Expected | Exit | Status |
|---|---|---|---|---|---|
| Closed block | `H r1.txt` (also with `--reasoning separate`) | Span 0..34, text 7..26 = `I should read a.txt` | Text without markers | 0 | VERIFIED |
| Starts open | `H --reasoning separate-open r2.txt`, and MCP | Span 0..23, text 0..15 = `first thoughts\n` | Starts at 0 and ends at the closer | 0 | VERIFIED |
| Adjacent blocks | `H r3.txt`, and MCP | `reasoning [0..32]` (merged). Blocks 0..16 / text 7..8 = `a`, and 16..32 / text 23..24 = `b` | Two blocks, never merged | 0 | VERIFIED |
| Closed at a call opener | `H r4.txt` | Span 0..19, text 7..19 = `still going ` (ends at the opener) | Ends at the opener | 4 (pre-existing outcome, same as the first audit) | VERIFIED |
| Unterminated, and starts-open unterminated | `H r5.txt`, `H --reasoning separate-open r6.txt` | Text 7..19 = `never closed`; text 0..33 = the whole output | Ends at end of output | 0 / 0 | VERIFIED |
| G14: `redacted_thinking` `"}x` | `H --dialect anthropic-tool-use.v1 g14.txt` (41 bytes), and MCP; also `separate-open` | Span 0..41, text 39..41 = `}x`, never reversed. `separate-open`: text 0..41 | No reversed range; span 0..41, text 39..41 | 0 | VERIFIED |
| Wire and schema | MCP `tools/list` | `reasoning_blocks` is required in `$defs/tool_calls` of the output schema. The input schema has `tool_choice_strict` with 3 required booleans | Published | n/a | VERIFIED |

### R11 (user scope): Draft-07 root `$ref` MCP declarations

| Criterion | Command | Observed | Expected | Exit | Status |
|---|---|---|---|---|---|
| FastMCP Draft-07 root `$ref` with Qwen3 XML and Hermes calls | `H --tools fastmcp.json --tools-format mcp r11q.txt` and `r11h.txt`, and MCP `tools_format: mcp` | Both `{"query":"garden plans","limit":5,"exact":true,"filter":{"tag":"home","min_score":0.5}}`, resolved, typed. The nested `$ref` member nests. Base on r11q: exit 4, all strings | Resolved typed arguments | 0 / 0 | VERIFIED |
| Repair parity with the inline twin | `querry` misspelling, root-`$ref` against inline, mutating and `--effects …=read-only` | Identical: mutating gives `missing_required` (guess not admitted); read-only gives `best_guess` renamed to `query` | Same as inline | 4 / 0 | VERIFIED |

### Cost claims (user scope R10/R12/R13), spot-checked

| Criterion | Command | Observed | Expected | Exit | Status |
|---|---|---|---|---|---|
| Repeated nested items k=2000 against 8000 under the default sized budget | `H rep2000.txt`, `H rep8000.txt` (issue item shape) | Both completed and resolved, with 2000 and 8000 items. Work per byte 52.16 and 51.74. Total scratch per byte 3199 and 3179. Peak per byte 191.0 and 188.9. Wall time 0.47 s and 1.42 s (3.0× for 4.03× the bytes) | Completes; charged cost per byte stable | 0 / 0 | VERIFIED |
| Mistral v11 chained calls, 1024 (also 256 and 4096) | `H --tools wtools.json m11_{256,1024,4096}.txt` | All completed, k calls resolved, 0 rejected. Work per byte 67.89, 67.76 and 67.75. Peak per byte about 219. Wall time 0.04, 0.14 and 0.57 s | Completes under the default budget; linear | 0 | VERIFIED |
| 1 MiB published cost bound | `cargo test --release -p purrassist-core --locked --test tool_call_scale` (HEAD) | 4 passed: `one_mib_dense_markup_fits_published_budget`, `one_mib_argument_fits_published_budget`, `one_mib_argument_work_bound`, `work_independent_of_dialect_count_beyond_bound` (55 s). The worktree is still clean afterwards | Pass | 0 | VERIFIED |

### Already satisfied: no regressions (HEAD against base 943fa04, same input)

Comparison ignores `reasoning_blocks` and `receipt`.

| Item | Command | Observed | Status |
|---|---|---|---|
| Length versus stop truncation | `t1` (complete body, no closer) and `t2` (cut mid-value), each with `--stop stop` and `--stop length` | Identical to base. t1/stop: resolved, `missing-closers-at-end`, exit 0. t1/length: `truncated`, `length`, exit 4. t2/stop: rejected `unterminated`, exit 4. t2/length: `truncated`, exit 4 | VERIFIED |
| Ids chained through `next_id_start` | `--ids-prefix call_ --ids-start 0 --emit openai s1.txt`, then `--ids-start 2 s3.txt`; also the default and `--ids-source`; MCP `ids:{prefix,start:5}` | Identical to base: `call_0`, `call_1`, next 2; then `call_2`, next 3. MCP gives `call_5`, `call_6`, next 7 | VERIFIED |
| Starts open; Mutating default with request override; `from_openai` forced; unclosed JSON value then the next parameter; stray closer; dangling `<tool_call>` | `open`, `effro`, `forced`, `unjson`, `stray`, `dang` differentials | All identical to base, with the same exit codes (0/3/0/3/3/0) | VERIFIED |
| Losses cap at BestGuess, and the whole golden set | A script compared every modified golden with `git show 943fa04:`, after deep-stripping `reasoning_text` | 466 modified: 463 identical, 2 reorder-only (`harmony_python_code_holds_call_closer`, `qwen_value_holds_call_closers_then_text`), and 1 `usable_calls` 1→0 (`forced_tool_choice_violated_beside_a_usable_call`, the A3 design) | VERIFIED |

### Gates and evidence reuse

| Gate | Evidence | Status |
|---|---|---|
| `make check` | Not re-run, as instructed. `validation.md` records exit 0 on 42899ab, and 42899ab is the reviewed head. It qualifies as same-head evidence. I independently ran `tool_call_goldens` (120), `correction_evaluation` (10) and the G16 test on HEAD; all pass | VERIFIED (reused, same head) |
| `make wasm-test` | Not re-run, as instructed. `validation.md` records exit 0 on 42899ab (native, WASI core/syntax/host and browser WASM). `vendor/tooling.json` pins node 26.11.1, `node --version` is v26.11.1, and the license sits at `vendor/licenses/node-26.11.1/` | VERIFIED (reused, same head) |
| Published replay | `python3 scripts/test-verify-published.py` on HEAD: 10 tests OK. Against base, the fixture diff changes `tools/list` (`recover_tool_calls` description, input schema and output schema), id 19 only in `usage`/`budget`/`reasoning_blocks`, and adds ids 20 and 21. Id 20: `forced[exactly-one,no-content]` passed, with non-empty blocks. Id 21: no-reasoning fails, usable 0 | VERIFIED |
| MSRV 1.98 | `rust-validate 1.98.0 cargo check --workspace --all-targets --locked`, `RUSTFLAGS=-D warnings`, on a `git archive HEAD` copy with a fresh target dir. rustc 1.98.0 confirmed | VERIFIED (compile only; MSRV `cargo test` not run) |
| Tool-call fuzz | Not re-run, as instructed. `validation.md` records bounded campaigns on 42899ab with `--continue-on-known`: 0 crashes, 0 branch-only failures. The `.cache` artefacts are timestamped after the HEAD commit. I replayed them myself:<br>- The 6 retained `known-6` inputs panic **identically**, same message and spans, on my HEAD probe (`:1156`) and base probe (`:988`), including the 2 `tool-call-ref` inputs replayed through the base fixed-tool probe.<br>- All 474 + 253 retained corpus inputs replay `ok` on the HEAD probe (`tool-call`, and `tool-call-ref` with `--tools ref`). | VERIFIED for "no branch-only failure in the retained campaign"; the gate form is FAILING on known, tracked issues (see Limits) |
| Hosted CI | All 8 jobs on 42899ab show `failure` and never started | UNVERIFIED (not run) |

## First-audit findings: disposition

- **F1: CLOSED.** The recursive `$ref` tree and the colliding names now resolve to the faithful readings:
  - p9r, plus `--effort exact`;
  - tr3 (repro 1);
  - save (repro 2);
  - create_doc (repro 3, formerly an abstention);
  - p10_ref_plan and rm/keep.

  Each is Resolved with no `missing-parameter-close` or `stray-closer`. The free-form (opaque) scope is a labelled guess that never runs on a mutating tool. The golden `xml_array_recursive_ref_items_are_siblings` was removed; `xml_array_recursive_ref_items_nest` and 8 other scope goldens were added. The corpus now covers `$ref` scopes, and zero incorrect was re-verified by me on HEAD. The dialect doc's "sibling" limit is gone; the scope graph and opaque rule are documented in `docs/tool-calls/dialects/qwen3-coder-xml.v1.md` (lines 44-90).
- **F2: CLOSED.** The p7 shape fails both exactly-one and no-content: exit 4, `nothing_usable`, usable 0/0, empty emission. This holds through the CLI, MCP and the Anthropic form. "What counts" in `docs/tool-calls/INTEGRATION.md` (around line 1268) now states that rejected attempts of any reason count. New goldens: `forced_strict_rejected_second_attempt` and `forced_strict_anthropic_unsupported_dialect_attempt`.
- **F3: CLOSED.** In c1fa81d the node pin is 26.11.1 (version, source, notices path and binary) and the local Node matches. `make wasm-test` is recorded as exit 0 on 42899ab. I did not re-run it, as instructed.

## Failure-shape review

- **DARK: none.**
  - The scope graph, opaque-boundary rule, `ForcedStrict`, `reasoning_blocks` and root-`$ref` projection are all reached from the shipped CLI and stdio MCP (above).
  - `ArgTree.collected` is consumed by `resolve.rs:2283`.
- **REFUSED: none.**
  - Every requested input is answered with the requested value.
  - The opaque case is a labelled guess, not counted as delivered nesting.
  - The non-colliding free-form item nests.
- **PRODUCER-WITHOUT-CONSUMER: none.** `ReasoningSegment.text` feeds `reasoning_blocks`, then the wire form, the MCP schema, the goldens' `reasoning_text` and the published replay ids 20 and 21.
- **DUPLICATE: none.** `Forced` and `ForcedStrict` coexist under the issue's "a variant or option" and the plan's recorded decision. `reasoning` and `reasoning_blocks` coexist by recorded decision; neither the issue nor `.goals` asks for replacement.
- **LAUNDERED-TO-FOLLOWUP: none.**
  - #10 (restarted DeepSeek-V3.1 sections) and #11 (repair-search under-charge on mutated free-form items) are user-authorized acceptances (decisions 5 and 6).
  - Neither is a #8 requirement: #8's three asks do not include those cost shapes.
  - I re-executed both repros on HEAD and base, with identical results:
    - #10 at k=8 and 16 (`--effort maximum`): work 499,439 against 498,956 and 1,866,831 against 1,865,916, with equal wall times;
    - #11: default `budget_exceeded` in about 0.23 s on both; `--effort maximum` completes in about 1.0-1.1 s, work 6,928,437 against 6,928,147.
  - Both are pre-existing and cited in `docs/RESOURCES.md` and `docs/FUZZING.md`.
  - The headline works end to end without them.
- **TEST-ONLY: none.** Every criterion above was executed on the CLI or MCP.
- **WEAKENED TESTS: none found.** I checked every modified existing test, golden and fixture across `943fa04..HEAD`:
  - **Goldens:** see the regression table (463 identical, 2 reorder-only, 1 intended `usable_calls` 1→0). Between 248d07a and HEAD, only 5 `forced*` goldens changed, and only in `usable_calls`. The removed `_are_siblings` was this PR's own wrong golden (F1).
  - **Corrected assertions:** `tool_adapters::assert_draft07_root_rules` replaces `parameters.len() == 0` with an assertion that `q` is projected and `ignored` is not, plus an extra validation assertion. This is a corrected expectation under user decision 2.
  - **Counts and constants:** `correction_evaluation` count constants rose (incorrect is still asserted 0). `tool_call_cost` still asserts depth and helper calls, and asserts evidence entries to the new published formula.
  - **Fuzz probe:** `emitted == usable` is now unconditional, plus a usable-empty check on failure.
  - **`test-fuzz.py`:** generalised for `tool-call-ref`.
  - **Corpus:** the inline-closer mutant narrowing (newline-final values) removed 0 base cases (first audit).
  - **Fixtures:** the receipt fixture refresh is usage-only (work, peak and total scratch). The published fixture refresh is usage, budget and additions only. No `#[ignore]` was added and no `#[test]` or assertion was removed in `src`.
- **VACUOUS OUTPUT: none.** `reasoning_blocks` is non-empty in the published replay (ids 20 and 21) and in goldens with reasoning. The `$ref` corpus cases evaluate non-trivial truths.
- **SILENT NARROWING: none open.** Three documented limits remain, each with a golden or doc and none contradicting an ask:
  - nothing nests when the tool is unknown at parse time (pre-existing for objects, documented);
  - opaque scopes close at an outer-declared name only as a guess;
  - a remote or anchor `$ref` stays opaque.

## Observations (no action required for #8)

- **The PR #9 body is stale; update it at handoff.** It still says:
  - "the same input reads as the next item for a recursive `$ref` schema" (now false: F1 fixed this);
  - validation on 248d07a, "15 new goldens", and a corpus of 18166.

  It also lacks the R2-R15 compatibility items. Those are now in `docs/tool-calls/INTEGRATION.md` `## Compatibility`, line 1406. This is not counted against the code.
- **Open-`anyOf` item typing.** A non-discriminated, open-`anyOf` item keeps `<parameter=a>1</parameter>` as the string `"1"`, because a branch that does not declare `a` accepts it. This is identical to the pre-existing object path on base (`objany`: `{"x":{"a":"1"}}` at both base and HEAD), so it is consistent with "exactly as the object path does". It is pre-existing typing policy, not a #8 defect.
- **Coverage of `make check`.** `make check` does not run the corpus lane or `test-verify-published.py`. I ran `correction_evaluation`, `tool_call_scale` and `test-verify-published.py` on HEAD to cover the R7-R9 and 42899ab commits, which came after the last recorded `make corpus` (R11).

## Limits

- **Hosted CI:** not run (verified on 42899ab). The macOS, Windows and hosted MSRV-test jobs have not run anywhere. MSRV compilation was verified locally.
- **`make fuzz` (gate form):** expected to FAIL. The `tool-call` target stops on the #6 rejected-span oracle, and `tool-call-ref` times out on #11 at the default 10 s. Both are pre-existing on `main` and tracked. Only `--continue-on-known` campaigns (and my replays) show the branch clean.
- **Not re-run by me:** `make check` and `make wasm-test` (reused same-head evidence).
- **No live services:** no models, deployment or publication were involved.

VERDICT: PASS
