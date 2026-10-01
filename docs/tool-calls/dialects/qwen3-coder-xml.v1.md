<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `qwen3-coder-xml.v1`

Grammar: [`crates/core/src/tool_call/dialects/qwen3-coder-xml.v1.grammar`](../../../crates/core/src/tool_call/dialects/qwen3-coder-xml.v1.grammar),
built in as `DialectId::Qwen3CoderXml`. The grammar and its fixtures are
first-party; no chat-template text is copied into this repository.

## Public sources

- Qwen3-Coder model repository, chat template `chat_template.jinja`:
  <https://huggingface.co/Qwen/Qwen3-Coder-480B-A35B-Instruct/blob/9d90cf8fca1bf7b7acca42d3fc9ae694a2194069/chat_template.jinja>
  (revision `9d90cf8fca1bf7b7acca42d3fc9ae694a2194069`, last modified
  2025-08-21). The template documents the call markup an assistant turn
  carries, how a tool message is rendered back to the model, and how the
  system prompt lists tools.
- Qwen3 model repository, chat template in `tokenizer_config.json`:
  <https://huggingface.co/Qwen/Qwen3-8B/blob/b968826d9c46dd6066d109eabc6255188de91218/tokenizer_config.json>
  (revision `b968826d9c46dd6066d109eabc6255188de91218`, 2025-07-26), for the
  `<think>`/`</think>` reasoning markers of the Qwen3 family.

## What the grammar accepts

- `call`: `<tool_call>`, optional whitespace, `<function=NAME>`, a body read
  by the `xml-params` codec, `</function>`, optional whitespace,
  `</tool_call>`. Each call has its own wrapper; calls follow one another
  with any content (usually a newline) between them.
- The body (the `xml-params` codec, typed by the declared tool `NAME`):
  `<parameter=P>` elements. A string, enumeration or unmodeled parameter is
  verbatim text with exactly one framing newline consumed on each side
  (recorded as `tool_call.xml.framing-newline`, a markup loss that is not a
  repair). Integer, number, boolean, array and object parameters are parsed
  as JSON with their exact spellings. An object parameter may hold nested
  `<parameter=...>` elements instead of inline JSON; both give the same
  tree. A body that is one whole JSON object is accepted as the arguments.
- `result`: `<tool_response>CONTENT</tool_response>`, the rendering of a tool
  message.
- `declaration`: one `<function>` entry of the tool list: `<name>`, an
  optional `<description>`, and a `<parameters>` block (kept verbatim).
- Reasoning: `<think>` ... `</think>`.

## Known variants handled by closer classes

- `</functions>` is read as `</function>` and `</tool_calls>` as
  `</tool_call>` (closer-class variants, recorded as
  `tool_call.markup.closer-variant`).
- `</parameters>` is read as `</parameter>` by the codec.
- Repeated or missing closers: every closer-class spelling is also known to
  the engine's weighted repair search, which may delete a stray closer
  (`tool_call.markup.stray-closer`) or insert a missing one
  (`tool_call.markup.missing-closer`), keeping the least-cost reading; a
  duplicate closer after a call that is already complete is content. A stray
  `</parameter>` between parameters is deleted by the codec, and a declared
  parameter's opener ends an unclosed value
  (`tool_call.markup.missing-parameter-close`).

These are repairs and are covered by the repair goldens, not by the
conformance anchors, which hold canonical markup only.

## Deliberately not accepted

- Several `<function=...>` elements inside one `<tool_call>` wrapper: the
  template always wraps each call separately.
- Attributes or quoting in the opener (`<function name="x">`,
  `<parameter name="p">`): these belong to other XML dialects.
- Text between parameters other than whitespace, and text after a whole-JSON
  body (the call is malformed).
- Python-style literals in typed values (`True`, `None`) are not canonical;
  the JSON codec's literal-case repair applies and is recorded.
- Streaming deltas (no `delta` production).
