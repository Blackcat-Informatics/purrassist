<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `bare-json.v1`

Grammar: [`crates/core/src/tool_call/dialects/bare-json.v1.grammar`](../../../crates/core/src/tool_call/dialects/bare-json.v1.grammar),
built in as `DialectId::BareJson`. The grammar and its fixtures are
first-party; no chat-template or model-card text is copied into this
repository.

## The format

Many models, fine-tunes and prompt-engineered agents write a tool call as a
plain JSON object with no tag, special token or envelope around it:

```json
{"name": "get_weather", "arguments": {"city": "Oslo"}}
```

It is the function object of an `OpenAI` `tool_calls` element written
without its element, and the call object Hermes-style models put inside
`<tool_call>` tags written without the tags. It appears as the whole answer,
among prose ("Let me check. {...} One moment."), inside a markdown
`` ```json `` fence, or as the elements of a JSON array of calls. The
arguments are an object, or (as in the `OpenAI` function object) a string
holding the JSON object.

## Public sources

- `OpenAI` OpenAPI specification, repository `openai/openai-openapi`,
  `openapi.json`, schemas `ChatCompletionMessageToolCall` and its
  `function` object:
  <https://github.com/openai/openai-openapi/blob/40496dbddd5b66bbe0a77d38bcb435884b7b8370/openapi.json>
  (commit `40496dbddd5b66bbe0a77d38bcb435884b7b8370`, 2026-10-01): the
  function object of a tool call has `name` and `arguments`, where
  `arguments` is a string of JSON text.
- Hermes function-calling reference code, `schema.py`:
  <https://github.com/NousResearch/Hermes-Function-Calling/blob/ea3c4723e4cefdac760d483ccac6c8a428c95ab8/schema.py>
  (commit `ea3c4723e4cefdac760d483ccac6c8a428c95ab8`, 2025-12-22): the call
  object has exactly two fields, `arguments` (an object) and `name`.
- vLLM's Llama 3 JSON tool parser:
  <https://github.com/vllm-project/vllm/blob/8c1557a79c539ffe82d004d2a0c8d7b5e71159ce/vllm/tool_parsers/llama_tool_parser.py>
  (commit `8c1557a79c539ffe82d004d2a0c8d7b5e71159ce`, 2026-09-16): output
  that starts with `{` is read as call objects, each with `"name"` and
  either `"parameters"` or `"arguments"`; the `"arguments"` spelling without
  any tag is the shape this dialect reads.

## What the grammar covers, and what it leaves to other dialects

This dialect reads exactly the call objects no other built-in dialect
reads, so no other dialect's canonical form has a second bare JSON reading:

| Object | Read by |
| --- | --- |
| `{"name": N, "arguments": A}`, either order, optional `"id"` first or last | `bare-json.v1` |
| `{"name": N, "parameters": A}`, optional `"type": "function"` and `"id"` | `llama3-json.v1` |
| `{"type": "function", ...}`, `{"id": ..., "type": "function", "function": {...}}`, `{"function": {...}}` | `openai-tool-calls.v1` (or `llama3-json.v1`) |
| `{"type": "tool_use", ...}`, or a block whose leading `"id"` begins `toolu_` | `anthropic-tool-use.v1` |
| `<tool_call>{...}</tool_call>`, `[TOOL_CALLS] [{...}]`, `<\|python_tag\|>{...}` | `hermes.v1`, `mistral-v3.v1`, `llama3-json.v1` |

The call objects inside another dialect's wrapper have the bare shape, so
both dialects read them; the wrapper's attempt commits at its opener and
covers the object, so the wrapped dialect's call wins on evidence and the
call's `candidate_dialects` names that dialect alone (the goldens
`bare_json_in_hermes_wrapper` and the test
`every_canonical_fixture_in_one_mixed_turn`, which recovers the canonical
fixture of all fifteen dialects in one turn). A leading `"id"` beginning
`toolu_` is not read here: such a block is Anthropic's, whose `"input"` key
it may have written as `"arguments"`. A trailing id, which Anthropic
evidence cannot precede, may have any spelling.

## What the grammar accepts

- `call`: one JSON object with JSON whitespace between its tokens: a
  `"name"` member and an `"arguments"` member in either order, an optional
  `"id"` member first or last, then the closing brace. The id is kept as the
  call's source id; the format does not provide for ids (`optional-id;`), so
  a call without one is numbered without an inference, and printing writes
  none. The arguments value is read by the `json-args` codec: the
  self-delimiting `json` codec (exact number spellings, member order and
  duplicates kept, each JSON repair recorded) restricted to an object, or a
  string that is empty or holds stringified arguments (decoded and flagged
  `tool_call.arguments.decode-string`).
- The call's JSON envelope (`json-envelope;`) tolerates the JSON syntax
  slips the `json` codec repairs inside arguments, under the same rules: a
  trailing comma (`json.syntax.trailing-comma`), single quotes as in a
  Python-repr dict (`json.syntax.string`), bare keys
  (`json.syntax.bare-key`), a missing comma or colon
  (`json.syntax.missing-comma`, `json.syntax.missing-colon`), a comment
  (`json.syntax.comment`), and raw control characters beside the quotes of
  the name string (deleted, `json.syntax.string`). Each is a non-guess
  repair: the call keeps the outcome it would have with the slip inside its
  arguments. See [GRAMMAR.md](../GRAMMAR.md) and the `envelope_*` goldens.
- Several calls: each object is its own call wherever it occurs. The
  brackets and commas of a JSON array, fence lines, and any prose around
  the objects are content. A call in a fence carries the guess
  `tool_call.context.fenced-example`, as in every dialect.
- `result`: a complete `{"tool_call_id": ID, "content": "..."}` object.
- `declaration`: a bare function object `{"name": ..., "description": ...,
  "parameters": {...}}` (description and parameters optional).
- No reasoning markers.

## Evidence: plain JSON stays content

A bare call has no opener of its own: it starts with `{`, like every JSON
object. The attempt commits only when the object is complete (`commit`
after the closing brace; [GRAMMAR.md](../GRAMMAR.md#commit-points)): a
name string, an object or stringified-object arguments value, at most an id,
and nothing else. Until then a failing attempt leaves no trace, so every
other object stays content byte for byte and nothing is rejected:

- a record that happens to have a name, `{"name": "Ada", "born": 1815}`;
- a nameless `{"arguments": {...}}`;
- `{"name": "pi", "arguments": 3}` and `{"name": "Ada", "arguments": "she
  argued"}` (the `json-args` codec faults on a number, an array, `null`, or
  a string that is not stringified arguments);
- an object with any other member (`"description"`, `"type"`, ...);
- an object cut short, by a length stop or otherwise.

A complete object is still not a call when it is shaped like a tool
declaration (`declaration-content;`): when its arguments, as written, are
an object whose `"type"` is `"object"` and whose `"properties"` is an
object (a JSON Schema), the object is content unless the named tool is
declared and resolves those arguments as written, with no guess and no loss.
A schema pasted for a tool that does not take a schema is then content
(`bare_json_schema_is_content`), while a tool whose own parameters are
`type` and `properties` still receives its call
(`bare_json_schema_for_schema_tool`). The same rule applies to untagged
Llama 3 JSON objects (`{"name": ..., "parameters": SCHEMA}`,
`llama3_declaration_is_content`). A pasted declaration with a description,
or a bare schema, is not call-shaped at all
(`bare_json_pasted_declaration_is_content`).

## Printing (canonical form)

- `call`: `{"name":"NAME","arguments":ARGS}`, `ARGS` the compact JSON object
  of the argument tree with exact number spellings and member order. Several
  calls are separated by one newline.
- `result`: `{"tool_call_id":"ID","content":"CONTENT"}`, the content a JSON
  string.
- `declaration`: `{"name":"NAME","description":"DESC","parameters":SCHEMA}`
  with `SCHEMA` the tool's exact parameters schema text, verbatim.

## Deliberately not accepted

- The `"parameters"` key: `{"name", "parameters"}` objects are Llama 3
  JSON's, and a `"parameters"` value is the shape of a pasted declaration.
- A `"type"` member, and any member other than `"name"`, `"arguments"`
  and `"id"`.
- A missing or doubled final brace: the object is a call only when it is
  complete, so it has no closer to repair. A cut-off object is content
  (with finish `truncated` under a length stop), not a truncated call.
- Arguments that are not an object or stringified object.
- Streaming deltas (no `delta` production).
