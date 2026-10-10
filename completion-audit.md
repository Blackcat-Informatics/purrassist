# Completion audit (Stage 2): PR #9, issue #8

- **Auditor:** an independent completion auditor (STAGE=2). It made no source changes. Scratch files were kept in `/tmp/ca8`, and the files under `/tmp/ca8` were deleted after the audit.
- **Scope:** commits `943fa04..248d07a` (head `248d07a`, base `origin/main` `943fa04`, no base drift).
- **Inputs read:**
  - `issue.md` (title, body, all 3 asks, the listed goldens, the already-satisfied list, and 5 comments);
  - `plan.md` (the Stage 1 contract, identical to the plan posted on the issue);
  - `reviews/completeness-review.md`, `validation.md`, `historical-fit.md`;
  - `AGENTS.md`, `.goals`, `docs/IMPLEMENTATION.md`;
  - `raw/pr-9.json`.
  - `.deficiencies` holds no entry for #8.
- **User scope decision (verbatim):** "you may not modify the local system. just change the node version pinned to match ours."
- **Entry points exercised:**
  - CLI: `purrassist recover-tool-calls` (binary built from `248d07a` with `cargo build -p purrassist-cli`; target dir from `cargo metadata`).
  - stdio MCP: `purrassist mcp --root DIR`, then `tools/call recover_tool_calls` and `tools/list`.
- **Base comparison:** `943fa04` was built from `git archive` in `/tmp` to provide a base differential.
- **Not run (as instructed):** `make check` and `make fuzz`.
- **Excluded:** no live model, server or deployment actions.

## Stage 1 demonstrations: do they qualify?

`reviews/completeness-review.md` qualifies as evidence:
- it ran on the same head (`248d07a`);
- it used the real CLI binary and the stdio MCP server;
- its inputs were representative.

I re-ran its core cases myself, and they reproduce. It did not exercise the two failure shapes below (F1, F2), so its PASS does not carry over to those criteria.

## Per-criterion results

Unless noted, `B` is `/tmp/ca8/purrassist recover-tool-calls --tools tools.json`. `tools.json` holds:
- `plan_outfits` with the issue's exact schema;
- `plan_event` (object `plan{cast, outfits}`);
- `tag` (`tags: string[]`);
- `read_file`.

### Ask 1: arrays of objects written as repeated nested `<parameter=>` elements

| Criterion | Command | Observed | Expected | Exit | Status |
|---|---|---|---|---|---|
| Issue's exact `plan_outfits` example | `B a1.txt` (issue bytes) and MCP `{output, tools}` | Arguments equal the issue JSON byte for byte after compact serialization. Status `resolved`, call `resolved`. Top-level and call `alternatives` are `[]`. Rules: nested-parameter ×2, array-item ×2, collect ×1. The MCP `value` equals the CLI `value`. Base `943fa04`: `unresolved`, exit 4 | `{"outfits":[{"wearer":"Alice","garments":["a red coat"]},{"wearer":"Bob","garments":["a grey scarf"]}]}`, Resolved, one reading | 0 | VERIFIED |
| Same shape inside an object parameter | `B --emit openai a2.txt` (`<parameter=plan><parameter=cast>[..]</parameter><parameter=outfits>…</parameter><parameter=outfits>…</parameter></parameter>`) and MCP | `{"plan":{"cast":["Alice","Bob"],"outfits":[{Alice…},{Bob…}]}}`, resolved, emitted | Nested inside the object, Resolved | 0 | VERIFIED |
| A JSON-object item mixed with a nested-element item | `B a3.txt` | Same two items in order, resolved | One item per repetition, in order | 0 | VERIFIED |
| Unclosed last item (ended by `</function>`) | `B a4.txt` | `[{"wearer":"Alice"},{"wearer":"Bob","garments":["a grey scarf"]}]`, resolved, `parameter-closed-by-element` ×1 | Closed by its enclosing element | 0 | VERIFIED |
| Negative: `String` items | `B a5.txt` | `{"tags":["<parameter=wearer>Alice</parameter>","plain"]}`, resolved | Nested tags stay a verbatim string | 0 | VERIFIED |
| Per-item order and pointers; required members enforced | `B a7.txt` (item 2 lacks `wearer`) | Call `unresolved` `missing_required` "`/outfits/1/wearer` is missing", finish `unresolved` | Not Resolved, pointer to item 1 | 4 | VERIFIED |
| `Unmodeled` items nest "exactly as the object path does" | `--tools tools_ref.json a6.txt` (non-recursive `$ref`) | `[{"wearer":"Carol","garments":["boots"]},{"wearer":"Dan"}]`, resolved | Nested | 0 | VERIFIED for this shape |
| `Unmodeled` items: well-formed input is never resolved to a wrong value | `--tools tools_tree.json tr3.txt`, `--tools tools_tags.json tg_save.txt`, and MCP (see F1) | Incorrect `Resolved` values. The calls are emitted, verdict `run`. Base `943fa04` abstained (exit 4) on the same bytes | The faithful reading, or abstention | 0 | **FAILED (F1)** |
| Issue-listed goldens exist and pin the expected values | Read the golden files. `cargo test -p purrassist-core --test tool_call_goldens` | All 5 asked goldens are present (plus 4 plan extras). `xml_array_nested_object_items` equals the issue JSON. 100/100 pass | Present and passing | 0 | VERIFIED |

### Ask 2: strict forced tool choice

| Criterion | Command | Observed | Expected | Exit | Status |
|---|---|---|---|---|---|
| Exactly one: two calls fail | `B --tool-choice read_file --tool-choice-exactly-one --tool-choice-no-content --emit openai s1.txt`. MCP: OpenAI forced plus `tool_choice_strict {true,false,true}` | `forced[exactly-one,no-content]:read_file` passed=false. Call 1 `unresolved tool_choice_cardinality`, finish `unresolved`, verdict `nothing_usable`, `emitted.output: []`. The plain `--tool-choice read_file` control exits 0 | Fails, emits nothing | 4 | VERIFIED |
| No content: one call plus prose fails | The same flags on `s2.txt` (prose, then the call), and MCP | "content at byte 0 is not allowed…", nothing_usable, `[]`. The plain control exits 0 | Fails | 4 | VERIFIED |
| Whitespace-only content passes | `s3.txt` (`\n` + call + `\n`). MCP with all three options | passed=true, run, 1 emitted | Passes | 0 | VERIFIED |
| Optionally no reasoning | `B --tool-choice read_file --tool-choice-no-reasoning r1.txt`. MCP `allow_reasoning:false` | "reasoning at bytes 0..34 is not allowed", nothing_usable | Fails | 4 | VERIFIED |
| Recorded in the tool-choice validation | Receipt of the s1/s2 runs | `{"validator":"purrassist.tool_call.v1","constraint":"tool_choice=forced[exactly-one,no-content]:read_file","passed":false,…}` | Recorded like the existing checks | n/a | VERIFIED |
| Anthropic `disable_parallel_tool_use` (plan) | MCP `{"type":"tool","name":"read_file","disable_parallel_tool_use":true}`: (a) two calls, (b) one call plus prose, (c) `false` | (a) `forced[exactly-one]` failed, cardinality. (b) passes. (c) `forced:read_file` passes with 2 calls | Exactly-one with content allowed; `false` means Forced | n/a | VERIFIED |
| Request errors | CLI flags without NAME, or with `auto`. MCP strict beside `auto` or `required`, `exactly_one:false` beside `disable_parallel_tool_use`, partial object, and `disable_parallel_tool_use` on `auto` | CLI: exit 1 with message. MCP: `isError:true` with a specific message each time | Explicit errors | 1 / n/a | VERIFIED |
| The strict contract holds when the second call attempt is malformed | `B --tool-choice read_file --tool-choice-exactly-one --tool-choice-no-content --emit openai rj.txt`. MCP with all three options | passed=**true**, verdict `run`, 1 call emitted. Only `finish` is `unresolved` | Two attempts plus non-whitespace bytes outside the call fail "exactly one call … and no content outside it" | 0 | **FAILED (F2)** |

### Ask 3: marker-free reasoning ranges

| Criterion | Command | Observed | Expected | Exit | Status |
|---|---|---|---|---|---|
| A `<think>` block plus a call: `text` bytes exactly | `B r1.txt` and `B --reasoning separate r1.txt` (`<think>I should read a.txt</think>\n<tool_call>…`) | `reasoning [0..34]`. Block span 0..34, text 7..26 = `I should read a.txt`. Call resolved | Text without markers | 0 | VERIFIED |
| `--reasoning separate-open` turn | `B --reasoning separate-open r2.txt` (`first thoughts\n</think>\n<tool_call>…`), and MCP `reasoning:"separate-open"` | Span 0..23, text 0..15 = `first thoughts\n`. Call resolved. The `separate` control reads the text as content | Text starts at 0 and ends at the closer | 0 | VERIFIED |
| Adjacent blocks, closed at an opener, unterminated, starts-open unterminated | `r3`–`r6` | One merged `reasoning` range 0..32 with blocks `a` (7..8) and `b` (23..24). `still going ` (7..19). `never closed` (7..19). Starts-open text 0..32 | As in plan Task 3 | 0/4/0/0 | VERIFIED |
| Harmony and Anthropic | MCP over `harm.txt` and `anth.txt` | Harmony text = `Need to read a.txt.`. Anthropic thinking text = `line1\n\"quoted\"` (JSON-escaped, verbatim) | As documented | n/a | VERIFIED |
| Wire and MCP schema | MCP `tools/list` | The output schema requires `reasoning_blocks` (`{span,text}` with the `range` `$ref`). The input schema has `tool_choice_strict` with 3 required booleans | Published | n/a | VERIFIED |

### Already satisfied: no regressions

Each line compares the head binary against the base binary on the same input, ignoring `reasoning_blocks` and `receipt`:

| Item | Command | Observed | Status |
|---|---|---|---|
| Length versus stop truncation | `t1.txt` (complete body, no closer) and `t2.txt` (cut mid-value), each with `--stop stop` and `--stop length` | t1/stop: resolved, `missing-closers-at-end`, exit 0. t1/length: `truncated`, ofr `length`, exit 4. t2/stop: rejected `unterminated`, exit 4. t2/length: `truncated`, exit 4. All 4 equal base | VERIFIED |
| Ids through `next_id_start` | `B --ids-prefix call_ --ids-start 0 --emit openai s1.txt`, then `--ids-start 2 s3.txt`, and MCP `ids:{prefix,start}` | `call_0`, `call_1`, `next_id_start` 2, then `call_2`, `next_id_start` 3. Equal to base | VERIFIED |
| Closers missing at EOF are repaired only when the call is complete; reasoning that starts open; Mutating default plus `--effects` override; `from_openai`; unclosed JSON value then the next parameter; one stray closer; a dangling `<tool_call>` | Differential runs `t1`, `r2`/`r6`, `effects-ro`, `forced-plain-*`, `unjson`, `stray`, `dang` | All equal base, same exit codes. Repeated scalar arrays differ only in the `array-item` explanation wording (planned) | VERIFIED |
| Losses cap at BestGuess, and the whole golden set | Recursive diff of every modified golden against `git show 943fa04:` after stripping `reasoning_text` | 466 modified: 464 identical, 2 reorder-only (`Ambiguous` tie order), 0 semantic differences | VERIFIED |
| Evaluation corpus | Base corpus (16438 cases) through the head code's `correction_evaluation` in a `/tmp` copy | 11261 correct, **0 incorrect**, 5177 abstained (identical to the base tallies), abstention recall 100%. The only failure is the expected case-count constant. The narrowing removed 0 base cases (34 inline-closers cases with a value-final newline: 0) | VERIFIED |

### Gates

| Gate | Evidence | Status |
|---|---|---|
| Focused suites | core goldens 100, recover 50, rules 41, correction_evaluation 10, roundtrip 15; host `tool_call_tools` 22; CLI `tool_calls_cli` 24; doctests `tool_call` 206. All pass on `248d07a` | VERIFIED |
| Native/WASM comparison | `python3 scripts/check-wasm.py` in a `git archive 248d07a` copy, with `vendor/tooling.json` node pinned to 26.11.1 (the local `node --version`, per the user decision) and the verified WASI sysroot copied in. Core, syntax and host native = WASI; core native = wasm32-unknown-unknown; exit 0 | VERIFIED (scratch); the repo pin change is still pending, see F3 |
| `make check` | Not run by me, as instructed. `validation.md` reports exit 0 on `248d07a` | UNVERIFIED (independently) |
| Hosted CI | All 8 jobs never started | UNVERIFIED (not run) |
| Tool-call fuzz | Details below | UNVERIFIED: the campaign cannot finish while #6 is open; no regression found |

Tool-call fuzz details:
- **Retained failure.** The retained failing input (`.cache/fuzz-tool-call-rjgczyq9/current.bin`) panics identically on a head probe (`fuzz_tool_call.rs:1056`) and on a base probe (`:988`), both built in `/tmp`. The attribution to #6 is confirmed.
- **Corpus replay.** All 876 retained corpus inputs from both campaigns pass on both probes.
- **Seeds.** The 5 strict seeds and the issue-example seed pass on the head probe.

## Failure-shape review

- **DARK:** none. Every new path is reached through the CLI and MCP (above).
- **REFUSED:** none.
- **PRODUCER-WITHOUT-CONSUMER:** none.
  - `ReasoningSegment.text` feeds `reasoning_blocks`, then the wire form, the MCP schema and the goldens.
  - `StrictForced` reaches the CLI, MCP and Anthropic surfaces.
- **DUPLICATE:** none. The issue asks for "a variant or option"; `Forced` and `ForcedStrict` coexist by the plan's recorded decision.
- **LAUNDERED-TO-FOLLOWUP:** none.
- **TEST-ONLY:** none.
- **WEAKENED TESTS:**
  - No assertion was weakened. The corpus regeneration rotated 8830 base cases out, but evaluating the base corpus on the head code gives the identical tally.
  - However, the golden `xml_array_recursive_ref_items_are_siblings` **pins an incorrect `Resolved` answer** as the expected value (part of F1).
- **VACUOUS OUTPUT:** none. 43 goldens carry non-empty `reasoning_text`.
- **SILENT NARROWING:**
  - F2: rejected attempts escape both strict options.
  - F1: undocumented beyond the array's own name. Any `Unmodeled`-item member named like an outer-scope parameter escapes its item.

## Findings

### F1 (HIGH, required): `Unmodeled` array items give incorrect `Resolved` answers on well-formed input (a regression from abstention)

**Where:**
- `crates/core/src/grammar/codec/xml.rs`, `nesting()` (~L1655): `Array(Unmodeled)` opens an item frame with scope `None`.
- `owner()` (~L840) gives an opener to the innermost frame that *declares* the name. A `None` scope declares nothing, so any name that an outer scope declares closes the item. The codec records `missing-parameter-close`, and the item's real closer becomes `stray-closer`.
- With arrays, `parameter.collect` absorbs the escaped element as extra items. The result is a single non-guess reading, so the call is `Resolved`.

**Repros** (CLI and MCP give the same result; base `943fa04` returns `unresolved` with exit 4 on the same bytes):

1. Recursive `$ref` tree, well-formed (every element closed).
   - Input: `tools_tree.json` `tree(children: array of $ref node{name, children: array of $ref node})` with
     `<parameter=children>\n<parameter=name>a</parameter>\n<parameter=children>\n<parameter=name>b</parameter>\n</parameter>\n</parameter>\n<parameter=children>\n<parameter=name>c</parameter>\n</parameter>`
   - Result: `{"children":[{"name":"a"},{"name":"b"},{"name":"c"}]}`, `resolved`, effects `mutating`, policy `structural`, verdict `run`, **emitted**, exit 0.
   - Truth: `[{"name":"a","children":[{"name":"b"}]},{"name":"c"}]`.
2. A `$ref` item member named like another top-level array parameter (the Pydantic `$defs` shape).
   - Input: `save(tags: string[], notes: array of $ref note{text, tags: string[]})` with
     `<parameter=tags>\nwork\n</parameter>\n<parameter=notes>\n<parameter=text>\nbuy milk\n</parameter>\n<parameter=tags>\nhome\n</parameter>\n</parameter>`
   - Result: `{"tags":["work","home"],"notes":[{"text":"buy milk"}]}`, `resolved`, emitted, exit 0.
   - The inline-schema twin of this tool resolves correctly to `{"tags":["work"],"notes":[{"text":"buy milk","tags":["home"]}]}`.
3. **Related narrowing:** when the colliding outer parameter is not an array, the well-formed `$ref` item abstains instead (`create_doc`: `title` both top-level and in the item, `unresolved`).

**Consequence:**
- These are incorrect automatic answers, on mutating tools under the default `structural` policy.
- They contradict `docs/IMPLEMENTATION.md` ("a correction evaluation with zero incorrect automatic answers") and the `Resolved` definition in `docs/tool-calls/README.md` ("One reading…").
- The issue's ask is `Unmodeled` items "exactly as the object path does". The object path abstains (`Ambiguous` via `repeated-choice`) on the analogous recursive input; the array path returns a wrong `Resolved`.
- The docs, PR body and handoff misdescribe the pinned golden: its input is balanced, not an "unclosed item".
- The broader collision (any outer-scope name) is not documented.
- No corpus case covers `$ref` items, so the zero-incorrect evaluation cannot see this.

**Required fix:** take one of these routes.
- Give `Unmodeled` item frames their members: resolve local `$ref`/`$defs` to the referenced object's properties, and use the union of the branches for `anyOf`/`oneOf`. Same-name members are then children, as in the inline case.
- Or, while an `Unmodeled` frame is open, never commit to the escape reading as a single non-guess reading: offer both readings (`Ambiguous`) or abstain.

Then:
- Change `xml_array_recursive_ref_items_are_siblings` (its value and description).
- Add goldens for repros 1 and 2.
- Add a `$ref`-item corpus mutant so the path is under the zero-incorrect evaluation.
- Correct the dialect doc, codec doc and PR text.

Accepting incorrect answers here would need an explicit user decision. The handoff raised this shape as a policy question, and no answer is recorded.

### F2 (MEDIUM, required): a strict forced choice passes when the second call attempt is rejected

**Where:** `crates/core/src/tool_call/recover.rs`, `strict()` (~L4360).
- `exactly_one` counts only `value.calls`.
- `allow_content` inspects only `value.content`.
- Rejected-attempt bytes are in neither.

**Repro:** `rj.txt` = one valid `read_file` Hermes call, then `\n<tool_call>\n{"name": "read_file", "arguments": {"path": \n</tool_call>`. Run it with all three strict options (CLI flags, or MCP `tool_choice_strict {true,false,false}`).
- `tool_choice.passed: true`, verdict `run`, CLI exit 0.
- The first call is emitted.
- Only `finish` is `unresolved`.

**Consequence:**
- The issue's contract is "exactly one call to `name`, and no content outside it". This output has a second call attempt and non-whitespace bytes outside the call, yet it runs.
- The documented rationale ("A rejected attempt already fails the turn's finish", INTEGRATION.md "What counts" and the `StrictForced` Rustdoc) holds only for `finish`. The same docs tell integrators to branch on `verdict` and the exit code, and those report success.

**Required fix:**
- Under `exactly_one`, count rejected attempts of allowed dialects toward cardinality. Alternatively or additionally, treat their bytes as content under `!allow_content`.
- A failed check must then give `nothing_usable`, exit 4 and an empty emission.
- Add goldens for both forms.
- Correct the "What counts" text.

### F3 (REQUIRED by the user's decision): the Node pin change has not been applied, so `make wasm-test` cannot run in the repo

**State at head `248d07a`:** `vendor/tooling.json` still pins node `26.10.0`. The local Node is `v26.11.1`, and the user directed: "just change the node version pinned to match ours."

**Required change:** update the node entry in `vendor/tooling.json`:
- `version`;
- `source`;
- the `notices` path and hash, with `vendor/licenses/node-26.11.1/LICENSE`;
- `binary_source`;
- `binary_sha256`.

Then run `make wasm-test` and `make check` on the new head. `check-repository.py` validates tooling notices.

**Expected outcome:** my scratch run of `248d07a` under that pin passed every transcript comparison, so the code is expected to pass. The repo-level gate is still unrun.

### Observations (no action)

- **Starts-open text.** Under `starts_open`, a literal `<think>` the model writes inside the already-open block stays in `text` (golden `reasoning_starts_open_think_closer_repro`). This is verbatim by design and documented.
- **Untracked file.** An untracked `crates/core/tests/zz_perf_tmp.rs` (no SPDX header, not part of the PR) was present at the start of the audit. Its owner removed it during the audit. I did not touch it.

VERDICT: FINDINGS-OPEN
