<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `mistral-v3.v1`

Grammar: [`crates/core/src/tool_call/dialects/mistral-v3.v1.grammar`](../../../crates/core/src/tool_call/dialects/mistral-v3.v1.grammar),
built in as `DialectId::MistralV3`. The grammar and its fixtures are
first-party; no chat-template or tokenizer source is copied into this
repository.

## The format

Mistral models on the v3 tokenizer (Mistral 7B Instruct v0.3, Mistral Nemo,
and their fine-tunes) call tools by writing the `[TOOL_CALLS]` control token
and then one JSON array that holds every call of the turn. Each array
element is an object with the function's `"name"`, its `"arguments"` object
and, since the v3 tokenizer, a 9-character alphanumeric `"id"`. The
Mistral 7B v0.3 template puts one space after the token; the Nemo template
puts none. The template renders the elements with JSON's `", "` and `": "`
separators and ends the message with `</s>`. A tool result goes back to the
model between `[TOOL_RESULTS]` and `[/TOOL_RESULTS]` as an object with the
result's `"content"` and the `"call_id"` it answers. Tool declarations are
listed between `[AVAILABLE_TOOLS]` and `[/AVAILABLE_TOOLS]`, each as a
`{"type": "function", "function": {...}}` object.

Models do not always write the id, and serving stacks assign one when it is
missing.

## Public sources

- Mistral 7B Instruct v0.3 chat template (`tokenizer_config.json`):
  <https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3/blob/c170c708c41dac9275d15a8fff4eca08d52bab71/tokenizer_config.json>
  (revision `c170c708c41dac9275d15a8fff4eca08d52bab71`, 2025-12-03): an
  assistant message's calls are rendered as `[TOOL_CALLS] [`, each call's
  function object serialized as JSON with `, "id": "..."` appended before its
  closing brace, `, ` between calls, and `]` plus the end-of-sequence token
  after the last; call ids must be 9 alphanumeric characters; tool results
  are `[TOOL_RESULTS] {"content": ..., "call_id": "..."}[/TOOL_RESULTS]`;
  declarations are `[AVAILABLE_TOOLS] [...]` with one
  `{"type": "function", "function": {...}}` object per tool.
- Mistral Nemo Instruct 2407 chat template:
  <https://huggingface.co/mistralai/Mistral-Nemo-Instruct-2407/blob/04d8a90549d23fc6bd7f642064003592df51e9b3/tokenizer_config.json>
  (revision `04d8a90549d23fc6bd7f642064003592df51e9b3`, 2025-07-28): the
  same shapes with no space after `[TOOL_CALLS]`, `[TOOL_RESULTS]` and
  `[AVAILABLE_TOOLS]`.
- `mistral-common`, the reference tokenizer, `instruct.py`:
  <https://github.com/mistralai/mistral-common/blob/2c30e17cbfe20a405b87522b1c1d51193dafe0ef/src/mistral_common/tokens/tokenizers/instruct.py>
  (commit `2c30e17cbfe20a405b87522b1c1d51193dafe0ef`, 2026-09-17): the v2
  and v3 instruct tokenizers encode an assistant's calls as the
  `[TOOL_CALLS]` token followed by the JSON list of `{"name", "arguments"}`
  objects; v3 adds each call's `"id"` and encodes a tool result as
  `{"content", "call_id"}`. The v11 and later tokenizers use a different,
  non-JSON layout (a separate dialect).
- vLLM's Mistral tool parser at release v0.10.0:
  <https://github.com/vllm-project/vllm/blob/6d8d0a24c02bfd84d46b3016b865a44f048ae84b/vllm/entrypoints/openai/tool_parsers/mistral_tool_parser.py>
  (commit `6d8d0a24c02bfd84d46b3016b865a44f048ae84b`): the text before
  `[TOOL_CALLS]` is content, the rest is parsed as a JSON list of
  `{"name", "arguments"}` objects, and a missing id is replaced by a random
  9-character alphanumeric one.

## What the grammar accepts

- `call`: `[TOOL_CALLS]`, optional whitespace, then a JSON array of call
  objects. JSON whitespace is allowed between tokens.
- The array written without its brackets: call objects after the token,
  separated by a comma or only by whitespace (one per line). Each omitted
  bracket is recorded as an inserted token
  (`tool_call.markup.missing-token`, a markup repair), so the calls stay
  `Resolved`. The list runs through the last call-shaped object: an object
  that holds a name and an arguments member continues it, and anything else
  ends it. Text after the last call, a plain JSON object in prose included,
  is content. The attempt reads every call it can
  ([GRAMMAR.md](../GRAMMAR.md#several-calls-in-one-attempt)), so this is
  decided by the objects themselves, not by competing equal readings.
- Each array element is its own call. The DSL's `next`
  ([GRAMMAR.md](../GRAMMAR.md#several-calls-in-one-attempt)) separates them:
  the first call's span holds the token and `[`, each element's span ends
  with the `, ` after it, and the last call's span holds `]`. Text before the
  token, and `</s>` after the array, are content.
- A call object holds `"name"` and `"arguments"` in either order, and an
  optional `"id"` first or last. The id is captured: it is the call's
  `source_id` and, under the default `IdPolicy::Turn`, its id. A call
  without one is numbered and recorded as a derived `tool_call.id.dedup`
  inference that never demotes it. The same applies to an id that repeats
  an earlier call's id.
- `"parameters"` is read like `"arguments"`. This is the Llama 3 key, and
  the `[TOOL_CALLS]` token is evidence enough that the object is a call.
- The arguments value is read by the self-delimiting `json` codec: exact
  number spellings, member order and duplicates kept. A JSON string holding
  JSON (stringified arguments) is decoded and flagged
  (`tool_call.arguments.decode-string`). When `"name"` comes first, the codec
  knows the declared tool.
- The element's `}` and the array's `]` are closer classes:
  - A missing one is inserted at the end of output under a non-length stop
    (`tool_call.markup.missing-closers-at-end`), or before the next
    `[TOOL_CALLS]` (`tool_call.markup.missing-wrapper-close`).
  - A repeated one after the call is absorbed as a stray closer
    (`tool_call.markup.stray-closer`).
  - A length stop inside the last object truncates that call alone. The
    calls before it are whole and usable.
- `result`: `[TOOL_RESULTS]`, optional whitespace,
  `{"content": CONTENT, "call_id": "ID"}`, `[/TOOL_RESULTS]`, with `CONTENT`
  a JSON string. It is captured as `content:` and `id:`.
- `declaration`: one `[AVAILABLE_TOOLS]` entry
  `{"type": "function", "function": {"name": ..., "description": ...,
  "parameters": {...}}}` (description and parameters optional).
- No reasoning markers: the format carries no in-band reasoning.

## Evidence and the other JSON dialects

The `[TOOL_CALLS]` token is the evidence. It opens the attempt, so every
array element belongs to that attempt, and no other dialect's reading of
the same bytes competes with it. In particular, a Llama 3 JSON object
(`{"name", "parameters"}`) inside the array is part of the Mistral call,
and the Mistral call's candidate dialects name Mistral alone. Without the
token, `{"name", "arguments"}` objects are not Mistral calls: this dialect
never opens at `{`.

## Printing (canonical form)

The printer (`tool_call::emit`) runs the same productions forward with
canonical choices. Every emitted call goes into one array, in the
template's layout: one space after the token and after each separator, the
id last:

- `call`: `[TOOL_CALLS] [{"name": "NAME", "arguments": ARGS, "id": "ID"}, ...]`.
  `ARGS` is the compact JSON object of the argument tree with exact number
  spellings and member order. `ID` is the id the policy assigns: the model's
  id under `IdPolicy::Turn` or `Source`, otherwise a numbered id. Each call
  is printed alone first, so an id the `call-id` class does not accept (such
  as a deduplicated source id `call#0`) skips that call, never the others.
  The template requires 9-character alphanumeric ids; a numbered id is
  longer and is printed as it is.
- `result`: `[TOOL_RESULTS] {"content": "CONTENT", "call_id": "ID"}[/TOOL_RESULTS]`,
  the content as a JSON string.
- `declaration`: `{"type":"function","function":{"name":"NAME","description":"DESC","parameters":SCHEMA}}`
  with `SCHEMA` the tool's exact parameters schema text, verbatim (compact,
  like the Hermes entry).

Editing a value of one call of an array (`ToolCalls::reemit_with`) splices
only that value. A call that shares its array with others is never
reprinted whole.

## Deliberately not accepted

- Elements with other members (a `"type"`, an `"index"`), and an element
  missing `"name"` or `"arguments"`.
- A missing comma between elements of a bracketed array.
- The Mistral v11+ layout `[TOOL_CALLS]NAME[ARGS]{...}`
  (`mistral-v11.v1`), and the output of tokenizers that strip the
  `[TOOL_CALLS]` token.
- Streaming deltas (no `delta` production).
