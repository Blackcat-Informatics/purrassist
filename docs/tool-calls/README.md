<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Tool-call recovery

Tool-call recovery reads one model turn, the raw output bytes of an assistant
message, and recovers the tool calls it makes against the tools the model was
offered. It returns every call with its own outcome, itemized repairs and
losses, an id, and the turn's finish, plus a receipt; it never executes a
call. The operation is `tool_call.recover.v1`, implemented in
`purrassist_core::tool_call` and exposed through the Rust API, the
`recover_tool_calls` MCP tool, and the `purrassist recover-tool-calls` CLI
command.

Twelve dialects are built in:

| Dialect | Shape | Reference |
| --- | --- | --- |
| `qwen3-coder-xml.v1` | `<tool_call><function=NAME><parameter=P>VALUE</parameter></function></tool_call>`, or the bare `<function=NAME>` element | [dialect](dialects/qwen3-coder-xml.v1.md) |
| `hermes.v1` | `<tool_call>{"name": NAME, "arguments": {...}}</tool_call>` | [dialect](dialects/hermes.v1.md) |
| `openai-tool-calls.v1` | `tool_calls` elements `{"id": ID, "type": "function", "function": {...}}` as text | [dialect](dialects/openai-tool-calls.v1.md) |
| `llama3-json.v1` | `{"name": NAME, "parameters": {...}}`, optionally after `<\|python_tag\|>` | [dialect](dialects/llama3-json.v1.md) |
| `mistral-v3.v1` | `[TOOL_CALLS] [{"name": NAME, "arguments": {...}, "id": ID}, ...]` | [dialect](dialects/mistral-v3.v1.md) |
| `deepseek-v3.1.v1` | `<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>NAME<｜tool▁sep｜>{...}<｜tool▁call▁end｜>...<｜tool▁calls▁end｜>` | [dialect](dialects/deepseek-v3.1.v1.md) |
| `deepseek-v3.v1` | `<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>NAME` + `` ```json `` block + `<｜tool▁call▁end｜>...<｜tool▁calls▁end｜>` | [dialect](dialects/deepseek-v3.v1.md) |
| `kimi-k2.v1` | `<\|tool_calls_section_begin\|><\|tool_call_begin\|>functions.NAME:IDX<\|tool_call_argument_begin\|>{...}<\|tool_call_end\|>...<\|tool_calls_section_end\|>` | [dialect](dialects/kimi-k2.v1.md) |
| `anthropic-tool-use.v1` | `tool_use` content blocks `{"type": "tool_use", "id": ID, "name": NAME, "input": {...}}`, standalone or in a message or response | [dialect](dialects/anthropic-tool-use.v1.md) |
| `harmony.v1` | gpt-oss messages `<\|channel\|>commentary to=functions.NAME <\|constrain\|>json<\|message\|>{...}<\|call\|>`, the analysis channel as reasoning | [dialect](dialects/harmony.v1.md) |
| `mistral-v11.v1` | `[TOOL_CALLS]NAME[CALL_ID]ID[ARGS]{...}` (`[CALL_ID]` optional), repeated per call | [dialect](dialects/mistral-v11.v1.md) |
| `bare-json.v1` | an unwrapped `{"name": NAME, "arguments": {...}}` object: the whole turn, in prose, in a code fence, or an array element; never a declaration-shaped object | [dialect](dialects/bare-json.v1.md) |

Callers add their own dialects with the grammar DSL
([GRAMMAR.md](GRAMMAR.md)). Still tracked in issue #4: the `pythonic.v1`,
`gemma3-tool-code.v1` and `glm-4.5.v1` dialects, the `*-stripped` variants,
streaming recognition, and `purrassist-derive`.

## Design

- **Weighted, invertible grammars.** Each dialect is a declarative grammar.
  Its `call` and `result` productions compile into byte-level recognizer
  programs, and the same productions run forward as a printer. Recognition
  and printing are inverses: a printed call recognizes to the same tool,
  equivalent arguments, and no new loss.
- **Best-first repair search.** Well-formed input is recognized
  deterministically on a linear fast path. Where every configuration of a
  call attempt fails, a best-first search over (position, configuration)
  applies weighted edit operators: insert a missing closer, delete a stray
  one, accept a declared variant or confusable spelling. The least-cost
  repairs are kept and every applied operator is itemized. Argument codecs
  (`json`, `json-args`, `xml-params`, `raw`) read argument bodies with their own recorded
  repairs.
- **Factorized ambiguity.** The turn is a least-cost segmentation into
  content, reasoning, calls, tool results, and rejected attempts. When the
  recognizer knows the tool set, a call none of whose readings names a
  declared tool (exactly or by alias) weighs `UNDECLARED_NAME` (4, one
  guessed edit) more in this selection, so between competing readings of
  one span the one that names a declared tool wins unless it needs more
  repair than that; the weight is not a repair and records nothing.
  Equal-cost segmentations of one span are packed into a local choice;
  independent choices are never multiplied out during recognition. Each call
  is then resolved against the tool set (tool name, parameter names, repeats,
  typing, schema validation; a call that names no tool is read as the one
  declared tool its arguments fit, a guess, `tool_call.name.from-arguments`), and its readings are ranked by cost and a
  content digest, a total order independent of declaration order. Several
  equally good readings of one call make that call `Ambiguous`; the
  segmentations of each contested span that recover the same calls are
  folded into one, then the remaining whole-message readings are
  enumerated, charged as candidates before the first is built, and
  reported as an `Ambiguous` operation only
  when they recover different calls and the receipt holds evidence that
  tells them apart (below).
- **Evidence-based dialect detection.** Every allowed dialect attempts a
  call at its opener, so dialects that share one (`<tool_call>` opens both
  Qwen3-Coder XML and Hermes calls) compete over the same bytes. The body
  is the evidence: a `<function=` body is read by Qwen3-Coder XML and a JSON
  object body by Hermes, while the other dialect's attempt is rejected at
  the body's first byte. A rejected attempt costs more than any repair a
  call needs, so the least-cost segmentation reads a complete or repaired
  call in the dialect its body evidences. When no dialect reads a call there
  (a dangling, cut, or malformed `<tool_call>`), the segmentations differ
  only in which dialect's attempt was rejected; they recover the same calls
  with the same repairs, so they are one reading. Its rejected attempt is
  the one whose dialect read furthest into the body before failing, and
  `RejectedCall::candidate_dialects` names every dialect that attempted a
  call at that opener. Likewise, dialects that read the same bytes into the
  same call (same span, tool, arguments, outcome, and repairs) give one
  reading, and `ToolCall::candidate_dialects` names every one of them
  (always including the call's own `dialect`). Readings that recover
  different calls with identical ledgers are a genuine ambiguity; the
  dialect that read each call is then the evidence that tells them apart,
  attested as a passing `dialect=ID` validation of the call
  (`purrassist.tool_call.v1`). An `Ambiguous` operation therefore always
  carries non-empty `distinguishing` evidence for each ranked-adjacent pair
  of readings in its receipt.

Every rule a call can record is listed with its cost, guess flag, and loss
kind in [RULES.md](RULES.md).

## The API at a glance

| Item | Role |
| --- | --- |
| `ToolDefinition`, `Param`, `ObjectSpec` | Typed builders: name, aliases, `Effects`, `RepairPolicy`, `UnknownParameters`, parameters. |
| `ToolSet::compile`, `from_openai`, `from_mcp`, `from_anthropic` | A governed, validated tool set. Wire declarations are kept byte for byte and validated against their exact schema (Draft 2020-12 or Draft-07). |
| `DialectId`, `DialectSet` | One dialect (built-in or `DialectId::custom`), and one model's dialect allow-list. |
| `ToolRecoveryRequest` | Effort, budget, dialects, `ReasoningPolicy`, `GenerationStop`, `ToolChoice`, `IdPolicy`, `PolicySelection`, effects overrides, `AmbiguityPolicy`, `EmitTarget`. |
| `ToolSet::recover`, `recover_governed` | Recover one turn; returns `AssistResult<ToolCalls>`. |
| `ToolCalls` | `calls`, `rejected` (each `RejectedCall` with its `dialect` and `candidate_dialects`), `embedded`, `content`, `reasoning`, `finish`, `tool_choice`, `next_id_start`, `turn` (the output's digest), `emitted`; `usable()`, `usable_accepting_guesses()`, `verdict()` (wire `verdict`, with `usable_calls` and `usable_calls_accepting_guesses`), `reemit_with`. |
| `IdPolicy` | `Turn` (default: the model's own ids, every other call `call_<12 hex of the output's SHA-256>_<index>`), `Prefix { prefix, start }`, `Generator`, `Source`; see [call ids](#call-ids). |
| `ToolCall` | `id`, `source_id`, `tool`, `arguments`, `dialect`, `candidate_dialects`, `outcome`, `repairs`, `alternatives`, `keywords`; `arguments_as::<T>()`. |
| `tool_call::emit` | Print calls in any dialect or as the `OpenAI` `tool_calls` array; print tool results and declarations. |

Recovering a Qwen3-Coder XML call and printing it as Hermes markup and as the
`OpenAI` `tool_calls` array (the `tool_call::emit` module doctest):

```rust
use purrassist_core::tool_call::{emit, DialectId, IdPolicy, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
use purrassist_core::{Budget, Outcome};
let tools = ToolSet::compile([ToolDefinition::new("read").param("path", Param::string().required())], Budget::default()).unwrap();
let output = b"<tool_call>\n<function=read>\n<parameter=path>\nsrc/a b.rs\n</parameter>\n</function>\n</tool_call>";
let Outcome::Resolved(calls) = tools.recover(output, &ToolRecoveryRequest::default()).outcome else { unreachable!() };
let hermes = emit::emit(&tools, &calls, &DialectId::Hermes, &IdPolicy::default())?;
assert_eq!(hermes.output, b"<tool_call>\n{\"name\":\"read\",\"arguments\":{\"path\":\"src/a b.rs\"}}\n</tool_call>");
assert_eq!((hermes.calls.as_slice(), hermes.skipped.len()), (&[0][..], 0));
let array = emit::openai_tool_calls(&calls, &IdPolicy::default())?;
assert_eq!(
    array.output.to_json(),
    r#"[{"id":"call_03cf7be79285_0","type":"function","function":{"name":"read","arguments":"{\"path\":\"src/a b.rs\"}"}}]"#,
);
# Ok::<(), emit::EmitError>(())
```

The id `call_03cf7be79285_0` is the default numbering: `call_`, the first 12
hexadecimal digits of the SHA-256 digest of the output, and the call index.

A guess is usable only when the caller accepts guesses, and only on a
read-only tool (the `ToolCalls::usable_accepting_guesses` doctest):

```rust
use purrassist_core::tool_call::{Effects, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
use purrassist_core::{Budget, Outcome};
let tools = ToolSet::compile([ToolDefinition::new("read_file").effects(Effects::ReadOnly)], Budget::default()).unwrap();
let result = tools.recover(br#"<tool_call>{"name": "read_fiel", "arguments": {}}</tool_call>"#, &ToolRecoveryRequest::default());
let Outcome::BestGuess(calls) = result.outcome else { unreachable!() };
assert!(calls.usable().is_empty());
assert_eq!(calls.usable_accepting_guesses()[0].tool.as_deref(), Some("read_file"));
```

### Call ids

Ids are caller-scoped and never collide across the rounds of a conversation,
with zero configuration. By default (`IdPolicy::Turn`):

- A call keeps the id the model wrote in its markup (an `OpenAI`
  `tool_calls[].id`, a Mistral v3 `"id"`, a Mistral v11 `[CALL_ID]`);
  `ToolCall::source_id` reports it.
- Every other call is numbered `call_<tag>_<index>`, where `<tag>` is the
  first 12 hexadecimal digits (48 bits) of the SHA-256 digest of the turn's
  output bytes (`tool_call::turn_id_prefix`; the digest is
  `ToolCalls::turn` and the receipt's source). An assigned id that another
  call of the turn already holds gets `_<k>` appended for the least free
  `k`.
- A model id that is missing where the dialect's markup provides for one, or
  that repeats an earlier call's id, is numbered as above and recorded as a
  derived `tool_call.id.dedup` inference on the call and in the receipt; it
  never demotes the call. Dialects whose markup carries no id (DeepSeek-V3.1,
  DeepSeek-V3, Harmony, Hermes, Llama 3 JSON, bare JSON, Qwen3-Coder XML) are
  numbered without an inference; Hermes, Llama 3 JSON and bare JSON still
  keep an id a model adds as the call's source id (`optional-id`).

The same output always gets the same ids (C3); different outputs get
different tags except with probability `2^-48` per pair, so among `n`
distinct turns of one conversation a shared tag has probability at most
`n(n-1)/2^49`: about `1.8e-11` for 100 turns, `1.8e-7` for 10,000, and
`1.8e-5` for 100,000. A conversation that can repeat a byte-identical turn and
must number it apart uses `IdPolicy::Prefix { prefix, start }` and passes
each result's `next_id_start` as the next round's `start`.
`IdPolicy::Generator` uses a caller generator, and `IdPolicy::Source` keeps
the output's ids, deduplicated as `<id>#<index>`. Emission under a policy
numbers calls exactly as recovery under that policy does.

## Policy matrix

### Effects select the defaults

| Effects | Default `RepairPolicy` | Default `UnknownParameters` |
| --- | --- | --- |
| `Mutating` (the default for every definition that does not state its effects) | `Structural` | `Error` |
| `ReadOnly` | `Tolerant` | `DropWithLoss` |

A definition may set its policy and unknown-parameter handling explicitly.
Tools built from wire declarations (`from_openai`, `from_mcp`,
`from_anthropic`) declare no effects, so they are `Mutating` and
`Structural`; their unknown-parameter handling follows the exact schema
(`Error` where `additionalProperties: false` closes the scope, `PassThrough`
otherwise). The request overrides effects per tool
(`ToolRecoveryRequest::effects`; naming an undeclared tool is a request
error), and `PolicySelection::All(policy)` applies one policy to every tool,
whatever it declares.

### What each policy admits

| Repair | `Structural` | `Tolerant` |
| --- | --- | --- |
| Canonical parsing and records (framing newlines, nested parameters, stringified arguments, aliases, truncation, ids, invalid UTF-8) | admitted | admitted |
| Single-reading repairs (missing, stray, and variant closers, confusables, the missing `</parameter>` before a declared parameter, non-guessed JSON syntax repairs in arguments and in a JSON call envelope, enum case/whitespace normalization) | admitted | admitted |
| Multiple-reading rules, the guesses (fuzzy tool and parameter names, approximate enum values, guessed JSON repairs, calls inside reasoning or code examples, a wrapper closed before text, repeated parameters) | not admitted: the call is `Unresolved` (`RuleNotAdmitted`) | admitted: the call is at most `BestGuess` |
| A near-miss tool name | never matched; exact names and aliases only | a guess-flagged `tool_call.name.match` |
| A near-miss parameter name | never renamed and never dropped in place of the rename (`UnknownParameter`) | renamed by `json.key.match`, a guess |

`Structural` admits exactly the single-reading rules: repairs that leave no
other reading of the input. A required argument is never invented under any
policy, and a value is never coerced (a declared string is never parsed, and
numbers keep their spelling).

### Outcomes

`CallOutcome::classify` is the single place that decides a call's outcome:

| Call outcome | When |
| --- | --- |
| `Resolved` | One reading, every rule admitted, no guess, no data loss. Markup losses never demote. |
| `BestGuess` | One reading with a guess inference or a data loss. |
| `Ambiguous` | Several distinct readings of the call (a different tool, arguments, or blocker) at the least cost, whatever the policy. |
| `Unresolved { reason }` | No usable reading: an unknown tool, a missing required argument, a schema validation failure, a malformed or truncated call, a rule the policy does not admit, a repair under `Effort::Exact`, or a tool-choice violation. |

**Markup and data losses.** A `markup` loss discards bytes that carry no
argument data: closers, framing newlines, fence markers, a closers-only body.
It is itemized and never demotes a call. A `data` loss discards bytes that may
carry data: a dropped unknown parameter, a dropped repeated occurrence,
dropped prose inside a call, truncation, a dangling opener, an unterminated
reasoning block, bytes that are not UTF-8 (inside a call they are read as
U+FFFD and recorded as `tool_call.value.invalid-utf8`, with the original
bytes kept; see [INTEGRATION.md](INTEGRATION.md#invalid-utf-8)). It makes the
call that records it at most `BestGuess`. Text outside every call is never
dropped: it stays content.

### Usability and emission

| Call | `usable()` | `usable_accepting_guesses()` | Emitted |
| --- | --- | --- | --- |
| `Resolved` | yes | yes | yes, unless the turn fails its tool-choice check (`ToolChoiceFailed`) |
| `BestGuess`, read-only tool | no | yes | yes, unless the turn fails its tool-choice check (`ToolChoiceFailed`) |
| `BestGuess`, mutating tool | no | no | no (`GuessOnMutatingTool`) |
| `Ambiguous` | no | no | no (`Ambiguous`) |
| `Unresolved` | no | no | no (`Unresolved`, with its reason) |

"Read-only" means the tool's effective effects after request overrides.
Emission (`ToolRecoveryRequest::emit`, `tool_call::emit`) prints exactly the
calls `ToolCalls::emitted_calls()` selects (those `usable_accepting_guesses()`
selects, and none when the turn fails its tool-choice check), because
emitted output carries no outcome and a downstream executor may run whatever
it holds. The same selection decides `verdict()` (`Run` exactly when it is
not empty) and `openai_finish_reason()`. Every call or
rejected attempt that is not printed is listed in `Emitted::skipped`, so
nothing is dropped silently. Calls written inside closed reasoning blocks
(`ToolCalls::embedded`) are never selected, never receive an id, and never
affect the finish.

**Mutating guards.** A guess on a `Mutating` tool is never `Resolved`, never
selected by either usability method, and never emitted. With default
settings a `Mutating` tool is `Structural`, so a guess is not admitted at all,
and an unknown parameter is an error rather than a drop.

**Ambiguity.** `AmbiguityPolicy::Report` (the default) reports competing
whole-message readings as an `Ambiguous` operation and an ambiguous call as a
per-call `Ambiguous` outcome. Whole-message readings compete only when they
recover different calls: readings that differ only in which dialect read
a call (`ToolCall::candidate_dialects`) or which dialect's attempt at a
shared opener was rejected (`RejectedCall::candidate_dialects`) are one
reading, so complete calls never drop out of the value because of a dialect
label. `AmbiguityPolicy::Reject` makes any ambiguity,
whole-message or per-call, an `Unresolved` operation with a failed
`tool_call.ambiguity` validation and every alternative kept in the receipt.

**Status is not a usability verdict.** The operation's `status`
(`Resolved`, `BestGuess`, `Ambiguous`, `Unresolved`) is the certainty of the
reading of the turn, as in every other operation; it does not say whether
anything can run. A turn whose only call names an undeclared tool is read
with certainty (`Resolved`) and has nothing to run. Branch on
`ToolCalls::verdict()` (wire `value.verdict`): `run`, `no_calls`, `ambiguous`
or `nothing_usable`, with `usable_calls` and `usable_calls_accepting_guesses`
counting what may run; the CLI's exit code is `tool_call::exit_code` of it.
See [INTEGRATION.md](INTEGRATION.md#reading-a-result-status-finish-and-verdict)
for the status × verdict table.

## Finish classes

| `Finish` | When | `ToolCalls::openai_finish_reason()` |
| --- | --- | --- |
| `Calls` | The turn made calls, nothing is wrong with any of them, and at least one may run (usable accepting read-only guesses). | `"tool_calls"` |
| `ContentOnly` | No call, nothing wrong. | `"stop"` |
| `TruncatedWithCalls` | A `Length` stop after at least one call that may run (a cut-off call never may). | `"length"` |
| `Truncated` | A `Length` stop with no call that may run. | `"length"` |
| `Unresolved` | A rejected attempt (other than one a length stop cut off), including call markup no allowed dialect reads, an `Unresolved` (other than by truncation) or `Ambiguous` call, a failed tool-choice check (even beside calls that would run), or calls of which none may run (each a guess on a mutating tool). | `"length"` under a length stop; otherwise `"tool_calls"` when the emission still holds a call (a rejected or unresolved attempt beside calls that run), `"stop"` otherwise |

`Calls` and `TruncatedWithCalls` imply the verdict `run`. The
`finish_reason` (`tool_call::openai_finish_reason`, the one mapping) is
`"length"` whenever the caller's stop reason is a length stop (whatever the
finish: a length stop is never reported as a clean finish, and the usable
calls are still emitted), otherwise `"tool_calls"` exactly when the `OpenAI`
emission holds a call, and `"stop"` otherwise: the finish, `finish_reason`, emission and verdict never
contradict each other, and a length stop is never reported as a clean
finish.

Call markup that no dialect of the allow-list reads is never silent
content. A linear detector (`detect_unparsed`) scans the turn's content for
the distinctive openers and shapes of the families no built-in dialect
parses yet (`pythonic.v1` call lists `[f(a=1)]` at the start of the turn or
a line or after `<|python_start|>`, `gemma3-tool-code.v1` `` ```tool_code ``
blocks, `glm-4.5.v1` `<arg_key>`/`<arg_value>` calls, and the stripped
renderings `kimi-k2-stripped.v1`, `harmony-stripped.v1`,
`deepseek-v3-stripped.v1` and `name-arguments-stripped.v1`), and for the built-in dialects the
allow-list leaves out. Each hit is a `RejectedCall` with
`Rejection::UnsupportedDialect { family }` (its `dialect` is the family's
built-in dialect, or `None`; it has no `candidate_dialects`), recorded as
`tool_call.markup.unparsed-dialect` evidence; its bytes stay content, and
the finish is `Unresolved`. Stripped renderings that a built-in grammar
reads (a Mistral v3 array or a Hermes object without its tokens is a bare
JSON call, a Qwen3-Coder `<function=NAME>` element without `<tool_call>` is
a Qwen3-Coder XML call, a Llama 3 object without `<|python_tag|>` is an untagged Llama 3
call) are recovered, not reported.

Every `finish_reason` of `Finish::Unresolved` (`"stop"`, `"tool_calls"`,
`"length"`) hides rejected and unresolved calls: read `ToolCalls::finish`,
`ToolCalls::verdict` and `ToolCalls::rejected`, never only the
`finish_reason`. Truncation never upgrades malformed output.

## Termination semantics

The caller passes why the model stopped as `GenerationStop`; it decides what
an open call at the end of output means.

| Stop | An open call at the end of output |
| --- | --- |
| `Length` | Truncation: the call is a dangling reading of what was captured, `Unresolved` (`Truncated`) with a `tool_call.truncated` data loss; unless something else is wrong with the turn, the finish is `Truncated` or `TruncatedWithCalls`; the `finish_reason` is `"length"` whatever the finish. A call that ends with a self-delimiting arguments value (Mistral v11's `[ARGS]{...}`, with no closing token after it) is complete when that value is complete at the cut: it is not cut off, so it is read as under a stop, and the finish is `TruncatedWithCalls`. |
| `Stop`, `ToolCalls`, `ContentFilter`, `Unknown` | Malformed: completed only by inserting the missing closers at the end (`tool_call.markup.missing-closers-at-end`) when the call is otherwise complete (its name parsed, no value open, arguments complete, every required parameter present); otherwise a rejected attempt with a `tool_call.markup.dangling-opener` data loss and the finish `Unresolved`. |
| `StopSequence(bytes)` | Like `Stop`, and when `bytes` equal the closer the call expects next, that closer is reinserted (`tool_call.stop.consumed-closer`). |

Serving-layer repetition or degenerate-output stops map to
`GenerationStop::Unknown`, which never reads an open call as truncation.

## Effort

`Effort::Exact` permits canonical parsing only: no rule that costs something
or guesses (`Rule::is_repair`) is applied. A call that needs one is
`Unresolved` with `UnresolvedReason::RepairNotPermitted` naming the rule, its
span kept and no repaired value reported; a turn whose segmentation itself
needs a repair (a reasoning block closed at a call opener) is an `Unresolved`
operation with a failed `effort=exact` validation. Zero-cost parsing (framing
newlines, nested parameter elements, stringified arguments, typed values,
aliases) and records (truncation, ids, invalid UTF-8) still apply; the
`Exact` column of [RULES.md](RULES.md) classifies every rule. `Quick`,
`Standard` (the default), `Deep`, and `Maximum` select increasing finite
budget presets and do not change which rules apply.

## Budgets

Every step is charged to one `Governor` with portable amounts, so charges
depend only on the output, the tool set, and the request. Exhaustion,
cancellation, and an invalid request are an `Unresolved` operation whose
receipt names the stop (`BudgetExceeded` with the resource, `Cancelled`, or
`InvalidInput`), never a partial value or a panic.

Fixed presets are sized for ordinary turns. `ToolRecoveryRequest::sized(effort,
output)` (and `sized_for(effort, output, dialects)` for another allow-list)
sizes a budget for the output in two phases: a linear pre-scan counts its
call openers `m` (`DialectSet::call_openers`), then the budget is the
preset's work plus `RECOVER_WORK_PER_BYTE · n` (32 per byte), its scratch
bytes plus `RECOVER_SCRATCH_PER_BYTE · n` (48 per byte), at least `n` input
bytes, and its candidates plus `⌈preset.candidates / 8⌉ · m`
(`RECOVER_OPENER_CANDIDATE_DIVISOR`; 16 per opener at `Standard`); every
other limit is the preset's. The MCP tool and the CLI use it. Competing
segmentations of a contested span that recover the same calls are folded
before whole-message readings are enumerated, so a turn with many calls
spends candidates linearly; readings that genuinely multiply still exhaust
explicitly. Schema validation's working memory is charged to the same
governor. The published work bound, its constants, and 1 MiB measurements
are in [RESOURCES.md](../RESOURCES.md#tool-call-recovery-cost-bound).

## Further reading

- [INTEGRATION.md](INTEGRATION.md): integrator guarantees, how each is
  enforced and tested, and the MCP and CLI surfaces.
- [RULES.md](RULES.md): every rule, generated from and checked against the
  registry.
- [GRAMMAR.md](GRAMMAR.md): the dialect DSL and a custom-dialect walkthrough.
- [dialects/](dialects/): public sources, accepted forms, printing, and
  deliberate exclusions of each built-in dialect.
- [MATCHING-QUALITY.md](../MATCHING-QUALITY.md#tool-call-recovery-evaluation):
  the 13,788-case correction evaluation, per rule and per dialect.
- [RESOURCES.md](../RESOURCES.md#tool-call-recovery-cost-bound): the cost
  bound, budgets for large outputs, and throughput.
