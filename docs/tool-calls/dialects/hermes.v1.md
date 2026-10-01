<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `hermes.v1`

Grammar: [`crates/core/src/tool_call/dialects/hermes.v1.grammar`](../../../crates/core/src/tool_call/dialects/hermes.v1.grammar),
built in as `DialectId::Hermes`. The grammar and its fixtures are
first-party; no chat-template or model-card text is copied into this
repository.

## Public sources

- Hermes 2 Pro model card:
  <https://huggingface.co/NousResearch/Hermes-2-Pro-Llama-3-8B/blob/f798274b30e7d2d4797c369edcc0cc7473b6e6f2/README.md>
  (revision `f798274b30e7d2d4797c369edcc0cc7473b6e6f2`, 2024-09-14): a call
  is a JSON object inside `<tool_call>` tags, the published example lists
  `"arguments"` before `"name"`, and tool output returns inside
  `<tool_response>` tags.
- Hermes function-calling reference code, `schema.py`:
  <https://github.com/NousResearch/Hermes-Function-Calling/blob/ea3c4723e4cefdac760d483ccac6c8a428c95ab8/schema.py>
  (commit `ea3c4723e4cefdac760d483ccac6c8a428c95ab8`, 2025-12-22): the call
  object has exactly two fields, `arguments` (an object) and `name`; a tool
  signature is `{"type": "function", "function": {name, description,
  parameters}}`.
- Qwen3 chat template (a Hermes-style template) in `tokenizer_config.json`:
  <https://huggingface.co/Qwen/Qwen3-8B/blob/b968826d9c46dd6066d109eabc6255188de91218/tokenizer_config.json>
  (revision `b968826d9c46dd6066d109eabc6255188de91218`, 2025-07-26): the same
  call object with `"name"` first, arguments that may already be a string,
  and `<think>`/`</think>` reasoning.

## What the grammar accepts

- `call`: `<tool_call>`, optional whitespace, one JSON object holding a
  `"name"` member and an `"arguments"` member in either order, optional
  whitespace, `</tool_call>`. JSON whitespace is allowed between tokens.
- The arguments value is read by the self-delimiting `json` codec: exact
  number spellings, member order and duplicates kept. A JSON string holding
  JSON (stringified arguments) is decoded and flagged
  (`arguments_were_stringified`, `tool_call.arguments.decode-string`). When
  `"name"` comes first, the codec knows the declared tool.
- Text before a call (a preamble) and between calls is content.
- Inside the wrapper, text before the JSON object (a preamble such as
  `Sure:` or a code-fence opener such as ```` ```json ````) and a code-fence
  closer after it are dropped (`dropped:` captures): a fence marker is a
  markup loss (`tool_call.extract.fence`), any other dropped text a data loss
  (`tool_call.extract.prose`) that keeps the call at most a best guess. Other
  text after the object is never dropped, so trailing prose is not
  swallowed into a call.
- `result`: `<tool_response>CONTENT</tool_response>`.
- `declaration`: one tool signature object
  `{"type": "function", "function": {"name": ..., "description": ...,
  "parameters": {...}}}` (description and parameters optional), the shape of
  one entry of the system prompt's tool list.
- Reasoning: `<think>` ... `</think>`.

## The `"parameters"` key

Not accepted. The Hermes call schema (`FunctionCall` in `schema.py`) has
only `arguments` and `name`, and both the model card and the Qwen3 template
emit `"arguments"`. A `"parameters"` key belongs to other JSON call formats;
a call spelled with it is not a Hermes call and is rejected by this grammar
rather than reinterpreted.

## Known variants handled by closer classes

- `</tool_calls>` is read as `</tool_call>` (closer-class variant).
- Repeated or missing closers: the engine's weighted repair search may
  delete a stray closer spelling or insert a missing closer, keeping the
  least-cost reading (repairs, covered by the repair goldens rather than the
  conformance anchors).

## Deliberately not accepted

- Members other than `name` and `arguments` in the call object (for example
  an `id`).
- Several call objects, or a JSON array of calls, inside one wrapper.
- A call object without the `<tool_call>` wrapper.
- Streaming deltas (no `delta` production).
