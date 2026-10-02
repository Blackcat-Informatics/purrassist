<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `kimi-k2.v1`

Grammar: [`crates/core/src/tool_call/dialects/kimi-k2.v1.grammar`](../../../crates/core/src/tool_call/dialects/kimi-k2.v1.grammar),
built in as `DialectId::KimiK2`. The grammar and its fixtures are
first-party; no chat-template or model-card text is copied into this
repository.

## The format

Kimi-K2-Instruct, K2-Instruct-0905 and K2-Thinking write a turn's calls as
one *tool-calls section*:

1. `<|tool_calls_section_begin|>` opens the section;
2. each call is `<|tool_call_begin|>`, the call's id,
   `<|tool_call_argument_begin|>`, the arguments as JSON, and
   `<|tool_call_end|>`;
3. the calls are chained with no separator;
4. `<|tool_calls_section_end|>` closes the section.

The call id has the form `functions.NAME:IDX`: the tool name between a
`functions.` prefix and a colon, then a decimal index that counts the calls
of the conversation. The id is how the model names the tool; there is no
separate name field.

Tool results go back as tool messages, `<|im_system|>tool<|im_middle|>`,
then `## Return of ID`, a newline, the content, and `<|im_end|>`. The
request's tools are listed in a `tool_declare` system message as one compact
JSON array of OpenAI tool objects. Kimi-K2-Thinking writes its reasoning
between `<think>` and `</think>` before the answer.

## Public sources

- Kimi-K2-Instruct chat template (`chat_template.jinja`):
  <https://huggingface.co/moonshotai/Kimi-K2-Instruct/blob/fd1984e2b7a3350dbf7305fe73a4ede25c14de50/chat_template.jinja>
  (revision `fd1984e2b7a3350dbf7305fe73a4ede25c14de50`, 2026-04-23):
  - an assistant message's calls render as `<|tool_calls_section_begin|>`,
    then per call `<|tool_call_begin|>` + the call's id +
    `<|tool_call_argument_begin|>` + arguments + `<|tool_call_end|>`, then
    `<|tool_calls_section_end|>`, with no separators;
  - tool messages render as `## Return of ID`, a newline and the content,
    inside the `tool` role's message;
  - the tools render as `<|im_system|>tool_declare<|im_middle|>` and the
    tool list as compact JSON.
- Kimi-K2-Instruct-0905 chat template, the same rendering:
  <https://huggingface.co/moonshotai/Kimi-K2-Instruct-0905/blob/ac6c49f04883bd0a0598b790693a72061c676629/chat_template.jinja>
  (revision `ac6c49f04883bd0a0598b790693a72061c676629`, 2026-01-30).
- Kimi-K2-Thinking chat template, the same call and tool rendering, with
  each assistant message's reasoning between `<think>` and `</think>`:
  <https://huggingface.co/moonshotai/Kimi-K2-Thinking/blob/a51ccc050d73dab088bf7b0e2dd9b30ae85a4e55/chat_template.jinja>
  (revision `a51ccc050d73dab088bf7b0e2dd9b30ae85a4e55`, 2026-01-30).
- vLLM's Kimi-K2 tool parser:
  <https://github.com/vllm-project/vllm/blob/03ce1c6ed908788cf508219b666720783a723123/vllm/tool_parsers/kimi_k2_tool_parser.py>
  (commit `03ce1c6ed908788cf508219b666720783a723123`, 2026-04-19): the same
  tokens; an id must end in a colon and digits; the tool name is the id's
  text before the first colon, after its last dot (so `get_weather:0`
  without the prefix is accepted). The streaming parser of
  <https://github.com/vllm-project/vllm/blob/b6e04390d3ea5ebc79ac70d1b76d638c56fa8ce2/vllm/entrypoints/openai/tool_parsers/kimi_k2_tool_parser.py>
  (commit `b6e04390d3ea5ebc79ac70d1b76d638c56fa8ce2`) also accepts the
  singular section tokens `<|tool_call_section_begin|>` and
  `<|tool_call_section_end|>`.
- SGLang's Kimi-K2 detector:
  <https://github.com/sgl-project/sglang/blob/ae7a516ba723e3504966d2cfb54fba8e7c9aa710/python/sglang/srt/function_call/kimik2_detector.py>
  (commit `ae7a516ba723e3504966d2cfb54fba8e7c9aa710`): ids `functions.NAME:IDX`
  or `NAME:IDX`, the name made of word characters, dots and hyphens (so a
  dotted name keeps its dots, and a colon ends it); a bare call counter as
  the id, with the tool inferred from the argument keys.

## What the grammar accepts

- `call`: one tool-calls section. Each call of the section is its own call,
  separated by the DSL's `next`
  ([GRAMMAR.md](../GRAMMAR.md#several-calls-in-one-attempt)). Whitespace
  between the tokens and around the id is markup.
- The id is a *composite id* ([GRAMMAR.md](../GRAMMAR.md#captures)): the
  whole `functions.NAME:IDX` is the call's model id, kept as its source id
  under the default id policy, and the `NAME` inside it is the tool name.
  - The `functions.` prefix may be left out (`read_file:1`), as both parsers
    accept.
  - The name may hold dots: `functions.mcp.search:0` names `mcp.search`, as
    SGLang reads it (vLLM keeps only the text after the last dot, which
    would lose the namespace).
  - The name ends at the id's *final* `:IDX`, so it may hold colons, as tool
    names of this library may (MCP-namespaced tools): `functions.ns:tool:0`
    names `ns:tool`. Both public parsers end the name at the first colon
    instead (vLLM would read `ns`, SGLang no call at all). A name read this
    way that no tool declares is an unknown tool, never coerced to a
    declared prefix of it.
  - An id without its `:IDX` (`functions.read_file`) is read with the name
    only. The call then carries no model id: the id policy numbers it, a
    derived `tool_call.id.dedup` inference. So is a call whose id repeats an
    earlier call's.
- An id that is a bare call counter (`<|tool_call_begin|>3<|tool_call_argument_begin|>`,
  which SGLang accepts) is the call's id with no name. The resolver reads the
  tool from the arguments (`tool_call.name.from-arguments`, a guess): the
  one declared tool whose required parameters are all present and which
  declares every member makes the call a `BestGuess`; several make it
  `Ambiguous`, none leave it `Unresolved` (missing name). `Structural` does
  not admit the guess, and a guess on a mutating tool is never usable.
- The arguments value is read by the self-delimiting `json` codec: exact
  number spellings, member order and duplicates kept; stringified arguments
  decoded and flagged (`tool_call.arguments.decode-string`); the codec knows
  the declared tool the name refers to.
- Closers: `<|tool_call_end|>` and `<|tool_calls_section_end|>` are closer
  classes.
  - A missing section closer at the end of output under a stop is inserted
    (`tool_call.markup.missing-closers-at-end`).
  - A missing call closer before the next call is inserted
    (`tool_call.markup.missing-function-close`).
  - A repeated closer is absorbed (`tool_call.markup.stray-closer`).
  - Before prose, a missing section closer is the guess
    `tool_call.markup.missing-wrapper-close.before-text`, admitted only
    under `Tolerant`.
- A section that starts directly at `<|tool_call_begin|>`, with
  `<|tool_calls_section_begin|>` left out, is read with the token recorded as
  inserted (`tool_call.markup.missing-token`).
- The singular spellings `<|tool_call_section_begin|>` and
  `<|tool_call_section_end|>` are read through the declared confusable
  `confusable "calls_section" "call_section"`, recorded as
  `tool_call.markup.terminal-confusable` (a markup repair).
- A length stop inside a call's arguments truncates that call
  (`Unresolved`, `Truncated`). The calls before it in the section are whole.
- Reasoning: `<think>` … `</think>`.
- `result`: `<|im_system|>tool<|im_middle|>## Return of ID\nCONTENT<|im_end|>`,
  with the result's call id and its content verbatim.
- `declaration`: one element of the `tool_declare` array,
  `{"type":"function","function":{"name":...,"description":...,"parameters":...}}`.

## Printing (canonical form)

The printer (`tool_call::emit`) runs the same productions forward with
canonical choices:

- `call`: every emitted call in one section,
  `<|tool_calls_section_begin|><|tool_call_begin|>functions.NAME:IDX<|tool_call_argument_begin|>ARGS<|tool_call_end|>…<|tool_calls_section_end|>`,
  the template's layout. `ARGS` is the compact JSON object of the argument
  tree with exact number spellings and member order. The composite id is
  printed from the call's name; `IDX` is the call's position among the
  printed calls (0, 1, …), so the ids of one printing are distinct. A
  recovered Kimi-K2 turn numbered from 0 prints its ids unchanged; other
  calls' ids are renumbered this way, since a Kimi-K2 id must hold the tool
  name.
- `result`: `<|im_system|>tool<|im_middle|>## Return of ID\nCONTENT<|im_end|>`.
- `declaration`: the compact OpenAI tool object; the description and the
  parameters appear only when the tool has them.

## Deliberately not accepted

- An id without `:IDX` whose name has a colon and no `functions.` prefix.
- Tool results whose role name is not `tool`.
- Streaming deltas (no `delta` production).
