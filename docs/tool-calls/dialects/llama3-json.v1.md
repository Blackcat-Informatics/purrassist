<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `llama3-json.v1`

Grammar: [`crates/core/src/tool_call/dialects/llama3-json.v1.grammar`](../../../crates/core/src/tool_call/dialects/llama3-json.v1.grammar),
built in as `DialectId::Llama3Json`. The grammar and its fixtures are
first-party; no chat-template or model-card text is copied into this
repository.

## The format

Llama 3.1, 3.2 and 3.3 Instruct models call a custom (JSON-described) tool
by writing one JSON object as their answer: a `"name"` member holding the
function name, then a `"parameters"` member holding the arguments object.
The chat templates render a past call exactly that way, with a single space
after each colon and comma, and end the assistant message with `<|eot_id|>`
(or `<|eom_id|>` in the `ipython` environment). Meta's prompt-format guide
shows a second shape for the same calls: the `<|python_tag|>` special token,
then the object pretty-printed with four-space indentation and a leading
`"type": "function"` member, then `<|eom_id|>`. Tool output comes back to the
model as an `ipython` message.

The templates support one call per message. Serving stacks that parse Llama
3 output (vLLM's `llama3_json` parser) also accept several objects separated
by `; `, the tag before the first one only, and the `"arguments"` key in
place of `"parameters"`.

## Public sources

- Llama 3.1 8B Instruct chat template (`tokenizer_config.json`), from the
  public mirror of Meta's gated repository:
  <https://huggingface.co/unsloth/Llama-3.1-8B-Instruct/blob/4699cc75b550f9c6f3173fb80f4703b62d946aa5/tokenizer_config.json>
  (revision `4699cc75b550f9c6f3173fb80f4703b62d946aa5`, 2025-05-09): the
  system or first user message asks for `{"name": function name,
  "parameters": dictionary of argument name and its value}`; a past call is
  rendered as that object with `", "` and `": "` separators and the
  arguments serialized as JSON; one call per message; tool results are
  `ipython` messages ending in `<|eot_id|>`; built-in tools are called with
  `<|python_tag|>` and Python call syntax; tool declarations are listed as
  JSON objects indented by four spaces.
- Llama 3.3 70B Instruct and Llama 3.2 3B Instruct chat templates, from the
  same mirror:
  <https://huggingface.co/unsloth/Llama-3.3-70B-Instruct/blob/99cd0d2c829e92a67c844f9144c2509632e5c87f/tokenizer_config.json>
  (revision `99cd0d2c829e92a67c844f9144c2509632e5c87f`, 2025-11-25; the call
  rendering is identical to 3.1) and
  <https://huggingface.co/unsloth/Llama-3.2-3B-Instruct/blob/006f5dcd1393c3add266de40994ba96225e9689d/tokenizer_config.json>
  (revision `006f5dcd1393c3add266de40994ba96225e9689d`, 2025-06-02; the same
  `"name"`/`"parameters"` object).
- Meta's Llama 3.1 prompt-format guide in `llama-models`:
  <https://github.com/meta-llama/llama-models/blob/8d29d93fa5700a60532e0061a02ffa89d0acd3fc/models/llama3_1/prompt_format.md>
  (commit `8d29d93fa5700a60532e0061a02ffa89d0acd3fc`, 2024-09-25):
  `<|python_tag|>` marks a tool call in the response, `<|eom_id|>` ends a
  message that expects a tool result, the JSON tool-calling example answers
  with the tag, a pretty-printed object with `"type": "function"`, `"name"`
  and `"parameters"`, and `<|eom_id|>`; built-in tools use
  `<|python_tag|>NAME.call(...)`.
- vLLM's Llama 3 JSON tool parser:
  <https://github.com/vllm-project/vllm/blob/8c1557a79c539ffe82d004d2a0c8d7b5e71159ce/vllm/tool_parsers/llama_tool_parser.py>
  (commit `8c1557a79c539ffe82d004d2a0c8d7b5e71159ce`, 2026-09-16): output
  that starts with `<|python_tag|>` or `{` holds call objects separated by
  `; `, each with `"name"` and either `"parameters"` or `"arguments"`.

## What the grammar accepts

- `call`: one JSON object, with JSON whitespace between its tokens:
  an optional leading `"type": "function"` member, then a `"name"` member and
  a `"parameters"` member in either order, then the closing brace. The
  arguments value is read by the self-delimiting `json` codec: exact number
  spellings, member order and duplicates kept; a JSON string holding JSON
  (stringified arguments) is decoded and flagged
  (`tool_call.arguments.decode-string`). When `"name"` comes first, the codec
  knows the declared tool.
- `<|python_tag|>` before the object, optionally followed by whitespace. After
  the tag the `"arguments"` key is read like `"parameters"`.
- Several calls: each object is its own call, wherever it occurs. What
  separates them (`; `, a newline, nothing, the brackets and commas of a JSON
  array) and any text around them is content, as is a trailing `<|eom_id|>`
  or `<|eot_id|>`.
- The closing brace of the call object is a closer class: a missing final
  brace is inserted at the end of output under a non-length stop
  (`tool_call.markup.missing-closers-at-end`) or before the next call
  (`tool_call.markup.missing-wrapper-close`), and a repeated one after the call
  is absorbed as a stray closer (`tool_call.markup.stray-closer`), never
  leaking into content. A value cut short before `<|eom_id|>`, `<|eot_id|>` or
  another `<|python_tag|>` ends there.
- `result`: an `ipython` message,
  `<|start_header_id|>ipython<|end_header_id|>\n\nCONTENT<|eot_id|>`.
- `declaration`: one tool signature object
  `{"type": "function", "function": {"name": ..., "description": ...,
  "parameters": {...}}}` (description and parameters optional).
- No reasoning markers: Llama 3 has no in-band reasoning block.

## Evidence before a call: the commit point

A plain call has no distinctive opener: it starts with `{`, like any JSON
object. The grammar opens an attempt at every `{` and *commits* it only on
call-shaped evidence, using the DSL's `commit` ([GRAMMAR.md](../GRAMMAR.md#commit-points)):

- the `"parameters"` key after a `"name"` member, or
- the `"name"` key after a `"parameters"` member, or
- the `<|python_tag|>` token, after which the attempt is committed at once.

Before it commits, an attempt that fails leaves no trace: the bytes stay
content and nothing is rejected, so `{"name": "Ada", "age": 36}` in prose is
content. After it commits, the attempt is a call: it is repaired, truncated
by a length stop, or reported as a rejected attempt, never silently dropped.
`<|python_tag|>` followed by a built-in tool call in Python syntax
(`brave_search.call(query="...")`) is therefore a rejected attempt rather
than content.

Without the tag, only `"parameters"` is evidence. The `{"name": ...,
"arguments": ...}` objects inside Hermes calls and OpenAI `tool_calls`
elements are never read a second time as Llama 3 calls, and a bare object
with `"arguments"` is left to a dialect that owns that shape. A Llama 3 call
inside another dialect's call (inside a Hermes wrapper, for example) is part
of that call: the outer call holds its bytes.

## Printing (canonical form)

The printer (`tool_call::emit`) runs the same productions forward with
canonical choices: the first alternative carrying the most values (the
untagged object, `"name"` before `"parameters"`, no `"type"` member). The
spaces after the colons and the comma are the `sp` production, which the
grammar prints as one space (`layout sp " ";`); every other optional
whitespace is omitted.

- `call`: `{"name": "NAME", "parameters": ARGS}`, the template's layout byte
  for byte except `ARGS`, which is the compact JSON object of the argument
  tree with exact number spellings and member order. Several calls are
  separated by one newline.
- `result`: `<|start_header_id|>ipython<|end_header_id|>\n\nCONTENT<|eot_id|>`,
  content verbatim; the result carries no call id.
- `declaration`: `{"type":"function","function":{"name":"NAME","description":"DESC","parameters":SCHEMA}}`
  with `SCHEMA` the tool's exact parameters schema text, verbatim. The
  template indents each signature by four spaces; the printer writes it
  compact, like the Hermes entry.

## Deliberately not accepted

- Built-in tool calls in Python call syntax after `<|python_tag|>`, and code
  for the code interpreter: a rejected attempt, not reinterpreted.
- The `"arguments"` key without `<|python_tag|>`; bare `{"name", "arguments"}`
  objects belong to another shape.
- Members other than `"type"`, `"name"` and `"parameters"` (or
  `"arguments"` after the tag), and a `"type"` other than `"function"` or
  anywhere but first.
- Llama 3.2's Pythonic list `[func(a=1), ...]` for lightweight models (a
  different format) and Llama 3.1's `<function=NAME>{...}</function>` custom
  system-prompt format.
- A missing final brace before text that is not another call: the
  insertion is then a guess (`tool_call.markup.missing-wrapper-close.before-text`),
  admitted only under `Tolerant`.
- Streaming deltas (no `delta` production).
