<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `openai-tool-calls.v1`

Grammar: [`crates/core/src/tool_call/dialects/openai-tool-calls.v1.grammar`](../../../crates/core/src/tool_call/dialects/openai-tool-calls.v1.grammar),
built in as `DialectId::OpenAiToolCalls`. The grammar and its fixtures are
first-party.

## Public sources

- `OpenAI` API reference, Chat Completions: the chat completion object and
  its assistant message (`role`, `content`, `tool_calls`), and the request
  `tools` and tool-message shapes:
  <https://platform.openai.com/docs/api-reference/chat> (retrieved
  2026-10-01).
- `OpenAI` OpenAPI specification, repository `openai/openai-openapi`,
  `openapi.json`, schemas `ChatCompletionMessageToolCall`,
  `ChatCompletionResponseMessage`, `ChatCompletionRequestToolMessage`,
  `ChatCompletionTool` and `FunctionObject`:
  <https://github.com/openai/openai-openapi/blob/40496dbddd5b66bbe0a77d38bcb435884b7b8370/openapi.json>
  (commit `40496dbddd5b66bbe0a77d38bcb435884b7b8370`, 2026-10-01). A tool
  call has `id`, `type` (`"function"`), and `function` with `name` and
  `arguments`, where `arguments` is a string of JSON text. A tool message has
  `role` `"tool"`, `tool_call_id`, and `content` that is a string or an
  array of text parts. A tool is `{"type": "function", "function": {name,
  description, parameters, strict}}`.

## What the grammar accepts

- `call`: one `tool_calls` element,
  `{"id": ID, "type": "function", "function": {"name": NAME, "arguments": ARGS}}`,
  with its three members in any order and JSON whitespace between tokens.
  `"id"` may be missing (an id-less element is still a call; the recovery
  operation's id policy assigns its id) and `"type"` may be missing, but
  when present it is `"function"`. `"name"` and `"arguments"` come in either
  order inside `"function"`. `ID` is captured (`id:`).
- `ARGS` is read by the self-delimiting `json` codec. The canonical form, a
  string of JSON, is decoded and flagged (`arguments_were_stringified`,
  `tool_call.arguments.decode-string`); spans inside it map back through the
  escapes to the source bytes. A plain JSON object is read as it is.
- The three text forms of the output: a single element; a bare `tool_calls`
  array; an assistant message object `{"role": "assistant", "content": ...,
  "tool_calls": [...]}`. Each element is one call, in source order; the
  envelope, brackets and separators are content around the calls.
- Openers: a call attempt opens only where an object's first member is
  `"id"`, `"function"`, or the whole member `"type": "function"` (compact
  or with one space after the colon). An element that starts with `"id"`
  becomes a call attempt only at its `"type": "function"` or `"function"`
  member (`commit`), so another object whose first member is `"id"` (an
  Anthropic response, `{"id": "msg_...", "type": "message", ...}`) stays
  content. The opener literals spell each start
  compactly (`{"id"`), with one space (`{ "id"`), and pretty-printed on a
  new line indented by 0 to 16 spaces in steps of two or by 1 to 4 tabs. A
  message envelope, a tool message, a content part (`{"type": "text", ...}`)
  or other JSON in content therefore never opens a call attempt; an echoed
  request `tools` entry (`{"type": "function", "function": {"name": ...,
  "parameters": ...}}`) does open one, and is rejected at its `parameters`
  member.
- `result`: one request tool message,
  `{"role": "tool", "tool_call_id": ID, "content": CONTENT}`, where
  `CONTENT` is a JSON string or an array of `{"type": "text", "text": ...}`
  parts; captured as `id:` and `content:`. It opens on the same
  compact, spaced and indented starts followed by `"role": "tool"` (or the
  fully compact `{"role":"tool"`).
- `declaration`: one request `tools` entry,
  `{"type": "function", "function": {"name": ..., "description": ...,
  "parameters": {...}, "strict": ...}}` with the last three optional.
- This dialect is the output and round-trip target: its `declaration` and
  `result` productions are the request shapes a printer emits.

## Printing (canonical form)

The printer (`tool_call::emit`) runs the same productions forward with
canonical choices: the first alternative carrying the most values, optional
whitespace omitted. Every printed form is compact JSON.

- `call`: `{"id":"ID","type":"function","function":{"name":"NAME","arguments":"ARGS"}}`
  where `ARGS` is the compact JSON of the argument tree (exact number
  spellings) carried as a JSON string; several elements are separated by
  one newline. Each line is byte-identical to the corresponding element of
  `emit::openai_tool_calls`, the `tool_calls` array value itself (no `role`,
  no message envelope). An id the `call-id` class does not accept (such as
  a deduplicated source id `call#0`) cannot be printed in this dialect; the
  call is reported as skipped, never printed with another id.
- `result`: `{"role":"tool","tool_call_id":"ID","content":"CONTENT"}`, the
  content as a JSON string (the compact `{"role":"tool"` start is the first
  `tool-open` alternative).
- `declaration`: the canonical `OpenAI` `tools` entry
  `{"type":"function","function":{"name":...,"description":...,"parameters":...}}`.

## Message versus array detection

No detection step is needed: the grammar recognizes elements wherever they
occur, so the same calls come out of a single element, a bare array, or a
message envelope. What surrounds the elements is content.

## Known variants handled by closer classes

The dialect has no closer classes: JSON has no alternative closer
spellings. Malformed JSON inside `ARGS` is repaired by the `json` codec and
recorded with its JSON rule ids (covered by the repair goldens).

## Deliberately not accepted

- Extra members (for example a streaming `index`), and a `"type"` other
  than `"function"`.
- An element without `function`, the legacy `function_call` message field,
  and non-function (custom) tool calls.
- Pretty-printed starts with other indentation or CRLF line ends (the
  object is content, not a call attempt).
- The message's `content` text is not decoded into prose; it stays inside
  the content segment as JSON.
- Reasoning markers: the format carries no in-band reasoning; in a set with
  dialects that declare `<think>`, the set's markers apply to the whole turn.
- Streaming deltas (no `delta` production).
