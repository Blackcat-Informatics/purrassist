<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `mistral-v11.v1`

Grammar: [`crates/core/src/tool_call/dialects/mistral-v11.v1.grammar`](../../../crates/core/src/tool_call/dialects/mistral-v11.v1.grammar),
built in as `DialectId::MistralV11`. The grammar and its fixtures are
first-party; no chat-template or tokenizer source is copied into this
repository.

## The format

Mistral models on the v11 and later tokenizers (Mistral Small 3.2,
Devstral, Magistral, and their fine-tunes) no longer write a JSON array of
calls. Each call is its own run of tokens:

1. the `[TOOL_CALLS]` control token;
2. the function name, as plain text;
3. on the v11 tokenizer, the `[CALL_ID]` token and the model's call id
   (9 alphanumeric characters). The v13 tokenizer (Devstral, Magistral)
   dropped this segment;
4. the `[ARGS]` token;
5. the arguments, serialized as one JSON object.

Several calls follow one another directly, each with its own `[TOOL_CALLS]`,
and the message ends with `</s>`. A tool result goes back to the model as
`[TOOL_RESULTS]`, the call id and `[TOOL_CONTENT]` (v11) or nothing (v13),
the result content, then `[/TOOL_RESULTS]`. Magistral writes its reasoning
between `[THINK]` and `[/THINK]`. Tool declarations are listed in
`[AVAILABLE_TOOLS]` as `{"type": "function", "function": {...}}` objects.

## Public sources

- `mistral-common`, the reference tokenizer, `instruct.py`:
  <https://github.com/mistralai/mistral-common/blob/2c30e17cbfe20a405b87522b1c1d51193dafe0ef/src/mistral_common/tokens/tokenizers/instruct.py>
  (commit `2c30e17cbfe20a405b87522b1c1d51193dafe0ef`, 2026-09-17):
  - The v11 instruct tokenizer encodes each call as `[TOOL_CALLS]`, the
    name, `[CALL_ID]` and the id when the call has one, `[ARGS]`, and the
    arguments serialized with `json.dumps`.
  - The v13 tokenizer drops the call id from calls and from tool results.
  - The v7 tokenizer and later encode a tool result as `[TOOL_RESULTS]`,
    the call id, `[TOOL_CONTENT]`, the content and `[/TOOL_RESULTS]`, and
    v13 as `[TOOL_RESULTS]`, the content and `[/TOOL_RESULTS]`.
  - `[THINK]` and `[/THINK]` delimit reasoning when the tokenizer defines
    them.
- The same repository's special tokens, `base.py`:
  <https://github.com/mistralai/mistral-common/blob/2c30e17cbfe20a405b87522b1c1d51193dafe0ef/src/mistral_common/tokens/tokenizers/base.py>:
  `[TOOL_CALLS]`, `[ARGS]`, `[CALL_ID]`, `[TOOL_RESULTS]`, `[TOOL_CONTENT]`,
  `[/TOOL_RESULTS]`, `[AVAILABLE_TOOLS]`, `[THINK]`, `[/THINK]`.
- Mistral Small 3.2 24B Instruct 2506 chat template, from the public
  mirror of the model (`mistralai/Mistral-Small-3.2-24B-Instruct-2506`
  ships only `tekken.json`):
  <https://huggingface.co/unsloth/Mistral-Small-3.2-24B-Instruct-2506/blob/3ed8d341b53ea3d0e4194689905477688a6cf733/chat_template.jinja>
  (revision `3ed8d341b53ea3d0e4194689905477688a6cf733`, 2025-08-26):
  - A past call renders as `[TOOL_CALLS]` + name + `[CALL_ID]` + id +
    `[ARGS]` + the arguments as JSON, one run per call.
  - A tool result renders as `[TOOL_RESULTS]` + id + `[TOOL_CONTENT]` +
    content + `[/TOOL_RESULTS]`.
- Devstral Small 2507 and Magistral Small 2509 chat templates, from the
  same mirror:
  <https://huggingface.co/unsloth/Devstral-Small-2507/blob/122dc390a9e5725cd827b31f84a5853a3ac1a6d2/chat_template.jinja>
  (revision `122dc390a9e5725cd827b31f84a5853a3ac1a6d2`, 2025-07-10) and
  <https://huggingface.co/unsloth/Magistral-Small-2509/blob/dcfa8f675a2adaea5dad53b32c0c948b47c02f55/chat_template.jinja>
  (revision `dcfa8f675a2adaea5dad53b32c0c948b47c02f55`, 2025-09-29):
  - A call renders as `[TOOL_CALLS]` + name + `[ARGS]` + arguments, with no
    call id.
  - A result renders as `[TOOL_RESULTS]` + content + `[/TOOL_RESULTS]`.
  - Magistral's system prompt asks for reasoning between `[THINK]` and
    `[/THINK]`.

## What the grammar accepts

- `call`: `[TOOL_CALLS]`, the name, an optional `[CALL_ID]` with the id, an
  `[ARGS]`, and the arguments value.
  - The id is captured. It is the call's `source_id` and, under the default
    `IdPolicy::Turn`, its id.
  - A call without one (the v13 layout) is numbered and recorded as a
    derived `tool_call.id.dedup` inference that never demotes it. So is an
    id that repeats an earlier call's.
- The arguments value is read by the self-delimiting `json` codec:
  - exact number spellings, member order and duplicates kept;
  - stringified arguments decoded and flagged
    (`tool_call.arguments.decode-string`);
  - an object left open at the end of output under a stop closed
    (`json.syntax.close-container`);
  - the codec knows the declared tool the name refers to.
- Whitespace and newlines around the name, the id and the tokens are read
  as markup. They are never part of the name or the arguments, and nothing
  is recorded for them.
- An omitted `[ARGS]` between a `[TOOL_CALLS]` name (or id) and its
  arguments is read as inserted (`tool_call.markup.missing-token`, a markup
  repair), so the call stays `Resolved`.
- Several calls: each `[TOOL_CALLS]` call is a call.
  - A further call written without its `[TOOL_CALLS]`, right after a call
    (a name, optional id, and an explicit `[ARGS]`), continues the same
    attempt through the DSL's `next`
    ([GRAMMAR.md](../GRAMMAR.md#several-calls-in-one-attempt)). The omitted
    token is recorded as inserted (`tool_call.markup.missing-token`), and
    the call stays `Resolved`.
  - The attempt reads every call it can. Anything after the last call that
    is not a name followed by `[ARGS]` (prose, a plain JSON object, a word
    cut off by a length stop) is content.
  - A length stop inside a further call's arguments truncates that call
    alone.
- A length stop right after a call's arguments: the format has no closing
  token after `[ARGS]{...}`, so a call whose arguments JSON value is complete
  is a complete call. It is `Resolved`, as under a stop, and the finish is
  `truncated_with_calls`: the generation was cut, and more calls may have
  followed. A length stop inside the value truncates the call
  (`Unresolved`, `Truncated`).
- `result`: `[TOOL_RESULTS]`, optionally the call id and `[TOOL_CONTENT]`,
  the content verbatim, `[/TOOL_RESULTS]`. Captured as `id:` (when present)
  and `content:`.
- `declaration`: one `[AVAILABLE_TOOLS]` entry
  `{"type": "function", "function": {"name": ..., "description": ...,
  "parameters": {...}}}` (description and parameters optional).
- Reasoning: `[THINK]` ... `[/THINK]` (Magistral).

## Evidence and Mistral v3

Mistral v3 (`mistral-v3.v1`) shares the `[TOOL_CALLS]` opener. With both
dialects allowed (the default), each attempts a call there and the body
decides:

- a name is read by this dialect;
- a `[` or `{` (a v3 array or object) by Mistral v3.

The other dialect's attempt is rejected at the body's first byte. A
rejected attempt costs more than any repair a call needs, so each canonical
call is read by its own dialect alone, and its candidate dialects name only
that dialect (the `mistral_v3_and_v11_canonical` golden). This dialect never
opens at a bare name: without a preceding call, a name followed by `[ARGS]`
is content.

## Printing (canonical form)

The printer (`tool_call::emit`) runs the same productions forward with
canonical choices. Every emitted call is printed in the v11 layout, one run
per call, with no whitespace:

- `call`: `[TOOL_CALLS]NAME[CALL_ID]ID[ARGS]ARGS`, one per call, in one
  production instance. `ARGS` is the compact JSON object of the argument
  tree with exact number spellings and member order. `ID` is the id the
  policy assigns: the model's id under `IdPolicy::Turn` or `Source`,
  otherwise a numbered id. The tokenizer expects 9-character alphanumeric
  ids; a numbered id is longer and is printed as it is. The printer never
  chooses an alternative holding `insert(...)`, so it always writes
  `[TOOL_CALLS]` and `[ARGS]`.
- `result`: `[TOOL_RESULTS]ID[TOOL_CONTENT]CONTENT[/TOOL_RESULTS]`, content
  verbatim.
- `declaration`: `{"type":"function","function":{"name":"NAME","description":"DESC","parameters":SCHEMA}}`
  with `SCHEMA` the tool's exact parameters schema text, verbatim.

## Deliberately not accepted

- A name followed directly by its arguments with both `[TOOL_CALLS]` and
  `[ARGS]` omitted: with neither token, nothing shows the text is a call.
- The output of tokenizers that strip the `[TOOL_CALLS]` token (a bare
  `name[ARGS]{...}` with no call before it).
- Arguments that are not one JSON value.
- Streaming deltas (no `delta` production).
