# Security review: PR #9 (tool-call recovery, arrays of nested objects, strict forced choice, reasoning blocks)

## Scope

Diff `943fa04..HEAD` (HEAD `248d07a`) in the worktree
`.worktrees/8-tool-call-recovery-arrays-of-nested`. The review covered production code that
reads untrusted model output or untrusted MCP/CLI request input:

- `crates/core/src/grammar/codec/xml.rs`: array-item frames (`nesting()`, `Frame.item`,
  `pop()` now returning `Result`, `wrap_item`).
- `crates/core/src/tool_call/ir.rs`: `ArgTree::adopt` pointer shift and `shift_item`.
- `crates/core/src/tool_call/resolve.rs`: `collect` item numbering and `ToolChoiceCardinality`.
- `crates/core/src/tool_call/recover.rs`: `ToolChoice::ForcedStrict`, `StrictForced`,
  `from_anthropic` `disable_parallel_tool_use`, `Recovery::strict`, `ReasoningBlock`, wire
  `reasoning_blocks`, `calls_key`.
- `crates/core/src/tool_call/recognizer.rs`: reasoning `text` ranges (`text_start`,
  `opener_end`, `think_opened`, `cut_rejected`).
- `crates/host/src/tools/tool_calls.rs`, `crates/host/src/tools/registry.rs`: the
  `tool_choice_strict` member and its schema.
- `crates/cli/src/commands.rs`: the `--tool-choice-*` flags.
- `crates/core/examples/fuzz_tool_call.rs` `check_reasoning_blocks`, only to check whether it
  masks a production issue.

Golden runners and tests were out of scope. The exception is one golden that locks in a
production misreading (finding R1).

## Method

- Read the full source diff, then followed every new path into its callers and callees:
  frame push and pop, the `owner`/`boundary` scope logic, `Governor::charge` semantics for
  `Depth`, `Node::get` duplicate handling, and the recognizer marker matcher.
- Built the PR CLI (`purrassist`, dev profile: opt-level 3, overflow and debug assertions on)
  and a main-branch baseline (`git archive 943fa04`, built under `/tmp`). Copied both binaries
  to `/tmp` so concurrent builds could not change them.
- Built two small library probes in `/tmp` against `purrassist-core` (by path). They control
  the Budget (including `depth: u32::MAX`) and the thread stack size, and they slice
  `ReasoningBlock::text` the way the Rustdoc example does.
- Wrote a Python fuzzer that drives the CLI with random sequences of every built-in reasoning
  marker, call opener, full call and punctuation fragment, under `--reasoning
  separate|separate-open` and `--stop stop|length`. It checks the documented `reasoning_blocks`
  invariants: span and text inside the output, `span.start <= text.start <= text.end <=
  span.end`, an opener present outside starts_open, every block inside a merged range, and
  equal coverage. It ran 4 seeds x 3000 cases.
- Probed the MCP schema and handler (`purrassist call recover_tool_calls`, the same
  `Tools::call` path as MCP) and the CLI flags with malformed, duplicate and contradictory input.
- Did not run `make check` or `make fuzz`. Removed all scratch files under `/tmp` afterwards.

## Executed probes and observed results

| # | Probe | Observed |
|---|---|---|
| P1 | Deep `<parameter=outfits>` item nesting under a recursive inline schema, default budget. Schema/output depth 5/6, 100/100, 200/200, 10/10000. Alternating object/array schema 300. | Depth 6 resolves correctly. Schemas deeper than about 32 levels: `ToolSet` compile returns `BudgetExceeded(Depth)` (MCP: `budget_exceeded: Depth`, exit 4). Output deeper than the schema (10k): `budget_exceeded: Work`, 0.37 s. No panic, no overflow. |
| P2 | Unmodeled (`$ref`) recursive items, 10k and 100k openers, default budget | `budget_exceeded: Work` (0.41 s, 3.7 s, about 95 MB RSS). An unmodeled frame has `scope: None`, so nothing nests below it: depth stays at most 2. |
| P3 | Library probe, `Budget{depth: u32::MAX, work/scratch huge}`, inline depth 500/1000/1300/2000/5000, recover plus `to_node().to_json()` | Depth 500 to 1000: resolves on an 8 MB stack. Depth 1300 on 8 MB: stack overflow in `schema::evaluate::Evaluator::eval` (gdb), which is old code. At every stack size where compile succeeds (64 MB, depth 5000), recover and wire serialization also succeed. The new codec, `adopt`, `wrap_item`, `collect` and wire code do not recurse. Reaching this requires the caller to remove the Depth limit, which MCP and CLI never do (depth is fixed at 128). |
| P4 | Wide repetition: N items of array-of-object, array-of-string and `$ref` items (100 to 32000). Default, maximum and huge budgets. | Default: N ≥ 600 gives `ScratchBytes`/`Work` exhaustion. Maximum effort, 20k items: `ScratchBytes` after 4.1 s. Huge budget: time and charged scratch both grow quadratically (8000 items: 8.5 s, 12.6 GB charged scratch), and the charge stops it at 16k. The new uncharged `earlier` scan in `adopt` is not the dominant cost. Exhaustion is always explicit. |
| P5 | `expect` reachability in `pop()`/`parameters()` | Every `pop` call site is guarded (`frames.len() > 1`, or `frames.len() - 1 > owner` with `owner <= len - 1`). Item frames always carry `name: Some`. `Reader::run` takes `self` by value, so a stop that leaves frames open is never reused. No reachable panic. |
| P6 | Integer arithmetic in `shift_item`, `adopt`, `collect` | `shift_item` parses with `parse::<usize>()` (failure leaves the pointer unchanged) and uses `saturating_add`. The `earlier` and `before + index` sums are bounded by allocated item counts. No overflow path. |
| P7 | MCP `tool_choice_strict`: null, string, missing members, extra member, duplicate top-level `tool_choice_strict`, duplicate `tool_choice`, duplicate inner member, beside `auto`, with no choice, beside an undeclared forced name, `exactly_one:false` beside Anthropic `disable_parallel_tool_use:true`, `disable_parallel_tool_use` non-boolean, duplicated, or on `any` | All rejected with a tool error. Registry schema and `duplicate-members` validation run before the handler, so a duplicate key cannot silently drop strictness through `Node::get` returning `None`. An undeclared name gives `invalid_input`. No panic. The handler's checks match the schema. |
| P8 | CLI: strict flag without `--tool-choice`, with `auto`, repeated, in either order, `--flag=value`, repeated `--tool-choice`, undeclared NAME | All usage errors (exit 1) or `invalid_input` (exit 4). Flag order does not matter. |
| P9 | Strict-check bypass attempts: reasoning-embedded call; NBSP content; `--reasoning content` with `allow_reasoning:false`; a second call attempt that is malformed, truncated, or in an unsupported dialect | Embedded call: rejected by no-reasoning. NBSP: rejected by no-content. `reasoning: content` plus allow_content: passes, see A2. Malformed or truncated second attempt: `exactly_one` and `no-content` both pass, verdict `run`, see A1. Unsupported dialect: caught by no-content only. |
| P10 | Reasoning-range fuzzer (12,000 cases) and starts_open edge cases (empty output, `</think>`, `<think>`, `</think><think>`, `<think></think>`, think then call) | Edge cases are all in bounds. The fuzzer found one class of reversed text range, finding R2. |
| P11 | Recursive `$ref` items written with balanced nested markup, PR versus main | PR: silently resolves a different call. Main: unresolved. See R1. |

## Findings

### R1. REQUIRED: unmodeled (`$ref`) array items silently misread as siblings, or their members hoisted to the top level, reported `resolved` / verdict `run`

- Location: `crates/core/src/grammar/codec/xml.rs:948-956` (`owner()` pops the item frame when
  an outer scope declares the opener's name) together with the new `nesting()` at
  `xml.rs:1656-1667`, which makes array-of-unmodeled items open `scope: None` frames.
- Rule classification: `crates/core/src/grammar/edit.rs:25` classifies `MissingParameterClose`
  as `guess: no`, `grounding: definition`. An unmodeled scope's membership is unknown, not
  empty, so the definition does not ground the pop.
- Golden that locks it in: `crates/core/tests/data/tool-call-goldens/xml_array_recursive_ref_items_are_siblings.json`
  asserts `outcome: resolved`, `verdict: run` for a balanced nested tree that is read as two
  siblings.

Untrusted model output, with markup that is well formed and fully balanced:

```
<tool_call>
<function=acls>
<parameter=rules>
<parameter=allow>
/home
</parameter>
<parameter=rules>
<parameter=deny>
/home/secret
</parameter>
</parameter>
</parameter>
</function>
</tool_call>
```

Here `rules: array of $ref r` and `r` declares `allow`, `deny` and `rules`. The PR returns
`{"rules":[{"allow":"/home"},{"deny":"/home/secret"}]}` with status `resolved`, verdict `run`,
no inference and no alternative. The scoped deny becomes a top-level deny. The only signal is a
`stray-closer` markup loss for a closer that was not stray. Main (`943fa04`) fails closed on
the same input: `schema_validation`, `nothing_usable`.

A worse variant hoists a member out of the item into the top-level arguments. With
`rm(path: string, keep: array of $ref k)`, where `k` declares `path`:

```
<parameter=keep>
<parameter=path>
/home/user/important
</parameter>
</parameter>
```

The PR returns `{"keep":[{}],"path":"/home/user/important"}`, `resolved`, verdict `run`. The
path the model asked to keep becomes the argument of a destructive call. Main returns
`unresolved` / `nothing_usable` for the array form.

The same root cause already exists on main for a single unmodeled object parameter
(`node: $ref` gives `{"node":{},"path":...}`, resolved). This PR extends it to arrays, where
repetition is legal, so the misreading now always validates and runs.

This is reachable from untrusted output and violates the AGENTS.md contract: "a validated guess
is still a guess (never silently Resolved)". Documenting it as a "limit" does not satisfy that
contract while the result is `resolved` / `run`.

Fix suggestions, any one of which is sufficient:

1. When the innermost open frame has `scope: None` and `owner()` would pop it because an outer
   frame declares the name, do not treat the pop as definition-grounded. Record it as a guess
   with a competing reading (keep the opener as a member of the unmodeled object), so the call
   is `BestGuess` or `Ambiguous` and never `Resolved`.
2. Resolve the `$ref` target's property names at compile time, as a member-name set for the
   unmodeled item, so `declares()` can answer for unmodeled scopes.
3. At minimum, when a reading that popped through an unmodeled frame later deletes a "stray"
   closer, demote it to a guess. Balanced closers are direct evidence against the pop.

Then change the golden `xml_array_recursive_ref_items_are_siblings` to expect a guess or
ambiguity, and add the hoisting case as a golden.

### R2. REQUIRED: reversed `ReasoningBlock::text` range (start > end) from crafted output; documented slicing panics

- Location: `crates/core/src/tool_call/recognizer.rs:1672` (the `think_closed` guard
  `think.start > start` lets a closing marker that overlaps the opening marker close the block)
  and `recognizer.rs:478` (`text: think.text_start..marker.unwrap_or(end)`, built without
  clamping).
- Minimal input (Anthropic spaced redacted-thinking opener, whose final `"` begins the closer
  `"}`): `{"type": "redacted_thinking", "data": "}x`
- Observed (CLI, default options): `status: resolved`,
  `reasoning_blocks: [{"span":{"start":0,"end":40},"text":{"start":39,"end":38}}]`.
- The library probe ran the Rustdoc pattern `&output[block.text.clone()]` and panicked:
  `slice index starts at 39 but ends at 38`.
- The fuzzer found this in 6 of 12,000 cases, all the same class. Nothing else violated the
  invariants.

This breaks the documented invariant ("text ... inside `span`", "ends where the closing marker
begins") and is a panic or denial of service in any consumer that slices `output` by `text`,
including the Rustdoc example. The fuzz harness `check_range` would assert on it, so it does not
mask the bug. It just has not generated this marker overlap yet.

Fix: require a closer to start at or after the block's text start. In `think_closed` use
`if !closes || start < think.text_start { return Ok(()) }`, and apply the same check to the
`close` argument in `cut_rejected` and the promoted (closed-at-opener) paths. As a backstop,
build `text` as `think.text_start..marker.unwrap_or(end).max(think.text_start)`. Add the input
above as a golden and as a fuzz seed.

### A1. ADVISORY: strict `exactly_one` / `no-content` ignore rejected call attempts

- Location: `crates/core/src/tool_call/recover.rs:4362-4402`.

`<good call>\n<tool_call>{"name": "ls", "argu]]] </tool_call>` (and a truncated second attempt)
under `forced[exactly-one,no-content]` gives `tool_choice.passed: true`, verdict `run`, and an
emitted call, with finish `unresolved`. Rejected attempts are neither counted calls nor content
ranges. A rejected attempt in an unsupported dialect, however, stays content and fails
no-content, which is inconsistent.

INTEGRATION.md says "A rejected attempt already fails the turn's finish". The verdict and the
emission still run the call. Only one call can run, so this is not a cardinality bypass. But a
strict mode is expected to fail closed. Suggest treating any `rejected` attempt as a strict
diagnostic (cardinality or content), or documenting that the verdict stays `run`.

### A2. ADVISORY: `allow_reasoning: false` is not enforced when `reasoning: "content"`

Under `reasoning: content`, reasoning is never recognized, so `<think>plan</think><call>` with
`{exactly_one, allow_content: true, allow_reasoning: false}` passes. This needs the caller's own
option combination, so it is not attacker-controlled. Suggest rejecting that combination as
`invalid_input`, or documenting it beside the option.

### A3. ADVISORY: `usable_calls` counts calls of a turn whose strict check failed

`usable_calls: 1` comes back with verdict `nothing_usable` for single-call turns that fail only
no-content or no-reasoning. The source is `recover.rs:2868`, `ToolCalls::usable()`. This is
documented ("`usable_calls` still counts each call's own usability") and already applied to
`Forced`. However, the MCP tool description tells clients to "read value.verdict ... and
value.usable_calls", and strict mode makes this mismatch the common case. Suggest stating in
the registry description that `verdict` alone gates execution, or zeroing `usable_calls` when
`tool_choice.passed` is false.

### A4. ADVISORY: uncharged O(n²) scan in `ArgTree::adopt`

- Location: `crates/core/src/tool_call/ir.rs:325-344`.

The `earlier` sum rescans every same-named member on each array occurrence, and the scan is not
charged. P4 shows it is dominated by work that is charged (scratch charging is itself quadratic
and stops it), so this is not exploitable under any budget. Suggest keeping a running per-name
item count, or charging `Work` for the scan.

### A5. ADVISORY: alias and canonical occurrences number items differently

`adopt` counts earlier occurrences by exact key. `resolve::collect` numbers items across all
occurrences, including aliases. Mixing alias and canonical repetitions of one array parameter
can therefore give a leaf pointer that differs from the resolved source pointer, which
misattributes `decode-typed` evidence spans (`resolve.rs:2465`). Adapters create no aliases, so
only Rust-API definitions are affected. Suggest counting earlier occurrences by canonical name
in `adopt`.

### A6. ADVISORY: off-by-one in the Depth charge, and recursion outside this diff

- `xml.rs:975` charges `frames.len()` before the push, so nested frames can reach `depth + 1`.
  This is harmless.
- With a caller budget of `depth: u32::MAX`, the recursion in `schema::evaluate` (in recovery)
  and in declaration compilation overflows the stack at about 1300 levels on an 8 MB thread.
  This is not new in this diff and is not reachable through MCP or CLI. It is noted for the
  Rust API documentation (`Budget::depth` is the stack guard).

## Verdict rationale

R1 and R2 are reachable from untrusted model output.

- R1 turns a well-formed nested call into a different, executable call reported `resolved` /
  `run`, including hoisting an item member into a destructive top-level argument. Main fails
  closed on the same input.
- R2 emits a reversed range that panics consumers using the documented slicing pattern.

Both violate stated contracts and must be fixed before merge. Everything in the brief's checks
1 to 4 apart from these two passed: depth, recursion, `expect` panics, arithmetic, and the
MCP/CLI strict parsing and schema consistency.

VERDICT: BLOCKED
