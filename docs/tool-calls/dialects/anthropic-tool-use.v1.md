<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `anthropic-tool-use.v1`

Grammar: [`crates/core/src/tool_call/dialects/anthropic-tool-use.v1.grammar`](../../../crates/core/src/tool_call/dialects/anthropic-tool-use.v1.grammar),
built in as `DialectId::AnthropicToolUse`. The grammar and its fixtures are
first-party.

## The format

The Anthropic Messages API returns an assistant turn as a message whose
`content` is an array of blocks. A tool call is a `tool_use` block, an
object with `type` `"tool_use"`, the call's `id` (beginning `toolu_`), the
tool's `name`, and `input`, the arguments object. Text answers are `text`
blocks, and extended thinking appears as `thinking` blocks (the reasoning
and a signature) or `redacted_thinking` blocks (encrypted `data`). A tool
result goes back to the model as a `tool_result` block naming the
`tool_use_id`, with string content or an array of text blocks. The request
lists tools as objects with `name`, `description` and `input_schema`.

A whole response object wraps the message: `id` (beginning `msg_`), `type`
`"message"`, `role`, `model`, `content`, `stop_reason` (`"tool_use"` when the
turn ends to call tools), `stop_sequence` and `usage`.

## Public sources

- Anthropic Python SDK, repository `anthropics/anthropic-sdk-python`, at
  commit `18f25547f20cf5f01da69ac611e700e3bc9ebf21` (2026-09-30), the types
  generated from the API's schema:
  - `src/anthropic/types/tool_use_block.py`: `id`, `input` (an object),
    `name`, `type` `"tool_use"`, and the optional `caller` and
    `toolset_name`;
  - `src/anthropic/types/thinking_block.py` (`signature`, `thinking`,
    `type`) and `redacted_thinking_block.py` (`data`, `type`);
  - `src/anthropic/types/message.py`: the response object's members;
  - `src/anthropic/types/tool_result_block_param.py`: `tool_use_id`,
    `type` `"tool_result"`, `content` (a string or blocks), `is_error`;
  - `src/anthropic/types/tool_param.py`: a tool's `name`, `description`
    and `input_schema`;
  - `src/anthropic/types/raw_content_block_start_event.py` and
    `input_json_delta.py`: the streaming events.

  <https://github.com/anthropics/anthropic-sdk-python/tree/18f25547f20cf5f01da69ac611e700e3bc9ebf21/src/anthropic/types>
- Anthropic API reference, Messages and tool use (retrieved 2026-10-02):
  <https://docs.anthropic.com/en/api/messages>.

`input` is an object in the API and the SDK types; a JSON string holding
the object is not documented, but some proxies that translate from the
`OpenAI` shape write one. The `json` codec decodes such a string and flags
it (`tool_call.arguments.decode-string`), as for `OpenAI` arguments, and the
decoded object is validated like any arguments.

## What the grammar accepts

- `call`: one `tool_use` block, `{"type": "tool_use", "id": ID, "name":
  NAME, "input": ARGS}`, its members in any order, JSON whitespace between
  tokens. `ID` is captured as the call's model id (kept as its source id).
  The optional `caller` and `toolset_name` members are read after the four
  and are not part of the call.
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
- Where blocks are read: standalone, in a bare `content` array, in an
  assistant message `{"role": "assistant", "content": [...]}`, and in a
  whole response object. Each block is one call, in source order; the
  envelope, the array, `text` blocks and separators are content around the
  calls, so an envelope left unclosed at the end of output does not affect
  a whole block inside it.
- Evidence: a block opens at `{` but becomes a call attempt only at its
  `"type": "tool_use"` member, or, when the type was lost, at an `"id"`
  beginning `toolu_` (`commit`). An object with neither (a `{"name": ...,
  "input": ...}` in prose, a Llama 3 call, a Hermes or Mistral object) is
  never an Anthropic call attempt. A missing `id` is numbered by the id
  policy (`tool_call.id.dedup`).
- Reasoning: `thinking` and `redacted_thinking` blocks, spelled compactly or
  with one space after each colon and comma, are reasoning (the grammar
  declares one marker pair per spelling): from the block's start to its
  `signature` member, or to the end of its `data`.
- `ARGS` is read by the self-delimiting `json` codec: exact number
  spellings, member order and duplicates kept; a stringified object is
  decoded and flagged.
- Repairs, each recorded:
  - `"arguments"` or `"parameters"` written for `"input"` after the block's
    evidence is read through declared confusables
    (`tool_call.markup.terminal-confusable`). Before the evidence the key
    must be `"input"`: reading such a key there would run a second reading
    of the arguments of every Hermes, Mistral and `OpenAI` object in the
    output.
  - The block's closing brace is a closer class: a missing one at the end of
    output under a stop is inserted
    (`tool_call.markup.missing-closers-at-end`), a repeated one absorbed.
  - A length stop inside `input` truncates the call (`Unresolved`,
    `Truncated`).
- Streaming events (not in scope here; see issue #4): a
  `content_block_start` event whose `content_block` is a `tool_use` block,
  and an `input_json_delta` event, are rejected attempts spanning their
  line. They are never silently content, and the nested block with its empty
  `input` is never read as a call.
- `result`: one `tool_result` block, `{"type": "tool_result",
  "tool_use_id": ID, "content": CONTENT}` with an optional `is_error`, where
  `CONTENT` is a JSON string or an array of text blocks; captured as `id:`
  and `content:`.
- `declaration`: one request `tools` entry, `{"name": NAME, "description":
  DESC, "input_schema": SCHEMA}`, the same shape as
  `DeclarationForm::Anthropic` (this dialect adds no declaration form of its
  own; the dialect tests read every Anthropic rendering back through this
  production).

## Telling Anthropic blocks from the other JSON dialects

- Llama 3 JSON needs `"name"` and `"parameters"` (or, after
  `<|python_tag|>`, `"arguments"`), optionally with an `"id"` member first or
  last; a block's `"type": "tool_use"` member ends its attempt before it
  commits. An object with a `toolu_` id and `"parameters"`
  (`{"id": "toolu_01", "name": ..., "parameters": ...}`) is read by both: Llama
  3 JSON with no repair, Anthropic through the confusable key (cost 1), so
  it resolves once, as Llama 3 JSON. With the key before the id, only Llama
  3 JSON reads it.
- `OpenAI` `tool_calls` elements start with `"id"`, `"type": "function"` or
  `"function"`. An element that starts with `"id"` becomes a call attempt
  only at its `"type": "function"` or `"function"` member, so a response
  object's leading `"id": "msg_..."` stays content.
- Hermes needs a `<tool_call>` wrapper. A `tool_use` block inside one is
  read by Hermes, which accepts a leading provider type marker and id and
  reads `"input"` through a confusable (cost 1); the reading that rejects
  the wrapper and reads the block alone carries a rejected attempt (cost
  16). One Hermes call results, with the key recorded.

## Printing (canonical form)

- `call`: `{"type":"tool_use","id":"ID","name":"NAME","input":ARGS}`, the
  API's member order, compact; several blocks are separated by one newline.
  `ID` is the call's id under the id policy (a model's `toolu_` id is kept).
- `result`: `{"type":"tool_result","tool_use_id":"ID","content":"CONTENT"}`.
- `declaration`: `{"name":"NAME","description":"DESC","input_schema":SCHEMA}`.

## Deliberately not accepted

- Server tools (`server_tool_use`, `web_search_tool_result`, ...) and MCP
  tool blocks (`mcp_tool_use`): they are run by the API, not the caller.
- Other extra members of a `tool_use` block, and `"type"` values other than
  `"tool_use"`.
- Streaming deltas (no `delta` production): see above.
- A message's text blocks are not decoded into prose; they stay inside the
  content segments as JSON.
