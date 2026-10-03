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
- The bare `<function=NAME>...</function>` element, without the wrapper:
  `<tool_call>` and `</tool_call>` are special tokens that detokenizers
  commonly strip, so the missing wrapper tokens are read as inserted, each
  recorded as `tool_call.markup.missing-token` (a markup repair: a call that
  needs nothing else is `Resolved`). A `</tool_call>` after the bare element
  is read as its closer. Each bare element is its own call, and the attempt
  opens at `<function=`: an element that does not parse (no `>`, an empty
  name, text between parameters, a nested `<function=`) is a rejected
  attempt, never silent content. Inside a wrapper the wrapped reading
  covers the element.
- The body (the `xml-params` codec, typed by the declared tool `NAME`):
  `<parameter=P>` elements. A string, enumeration or unmodeled parameter is
  verbatim text with exactly one framing newline consumed on each side
  (recorded as `tool_call.xml.framing-newline`, a markup loss that is not a
  repair). Integer, number, boolean, array and object parameters are parsed
  as JSON with their exact spellings. An object parameter may hold nested
  `<parameter=...>` elements instead of inline JSON; both give the same
  tree. A body that is one whole JSON object is accepted as the arguments.
- The `<tool_call>` opener is shared with [Hermes](hermes.v1.md). With both
  dialects allowed (the default), each attempts a call there and the body
  decides: a `<function=` body is read by this dialect, and the other dialect's attempt
  is rejected at the body's first byte. An attempt neither dialect reads
  (dangling, cut off, or malformed) is one rejected attempt in the dialect
  that read furthest, with both dialects in its `candidate_dialects`; see
  [the README](../README.md#design).
- `result`: `<tool_response>CONTENT</tool_response>`, the rendering of a tool
  message.
- `declaration`: one `<function>` entry of the tool list: `<name>`, an
  optional `<description>`, and a `<parameters>` block (kept verbatim).
- Reasoning: `<think>` ... `</think>`.

## Printing (canonical form)

The printer (`tool_call::emit`) runs the same productions forward with
canonical choices: the first alternative carrying the most values, closer
classes as their canonical spelling, and whitespace as the grammar's
printing layout. The grammar declares `layout ws "\n";`, so every `ws`
between tags prints as one newline: the layout the published chat template
renders, so a printed call replayed into chat history is byte-identical to
what the template would produce for the same call. (`layout` only affects
printing; recognition accepts any whitespace as before.) Recognizing a
printed call gives back the same tool, an equivalent argument tree, and no
edit but the framing newlines.

- `call`: `<tool_call>\n<function=NAME>\n` + elements + `</function>\n</tool_call>`,
  that is `<tool_call>\n<function=NAME>\n<parameter=P>\nVALUE\n</parameter>\n</function>\n</tool_call>`
  for one parameter; several calls are separated by one newline.
- The body: one `<parameter=P>` element per top-level argument member, each
  `<parameter=P>` `\n` VALUE `\n` `</parameter>` `\n`, after one leading
  newline. A string value of a parameter the codec reads verbatim (declared
  string, enumeration or unmodeled, or undeclared) is written verbatim
  between the two framing newlines; every other value is inline compact
  JSON with exact number spellings. Nested objects are written as inline
  JSON, not as nested parameter elements (both read to the same tree; inline
  JSON is the single canonical choice). A repeatable parameter is one element
  per item.
- The body is checked by decoding it with the codec under the tool's shape.
  When that does not give back the same tree with framing newlines as the
  only edits (for example an unmodeled parameter holding a non-string, or a
  value containing `</parameter>`), the body is instead one whole JSON object
  (`tool_call.markup.json-body`), which reads independently of the shape.
  JSON text that would contain parameter markup or a call closer writes
  every `<` as `\u003c`.
- `result`: `<tool_response>CONTENT</tool_response>`, content verbatim (a
  content containing `</tool_response>` cannot be printed). The tool message
  carries no call id.
- `declaration`: `<function>\n<name>NAME</name>\n<description>DESC</description>\n<parameters>SCHEMA</parameters>\n</function>`,
  where `SCHEMA` is the tool's exact parameters schema text, verbatim (the
  `<parameters>` body is read by the `raw` codec, so either the exact schema
  text or the template's per-parameter XML expansion is accepted). The
  template's XML expansion is not printed: it renders nested schemas lossily,
  while the exact text is what arguments are validated against.

## Known variants handled by closer classes

- `</functions>` is read as `</function>` and `</tool_calls>` as
  `</tool_call>` (closer-class variants, recorded as
  `tool_call.markup.closer-variant`).
- `</parameters>` is read as `</parameter>` by the codec.
- Repeated or missing closers: every closer-class spelling is also known to
  the engine's weighted repair search, which may delete a stray closer
  (`tool_call.markup.stray-closer`) or insert a missing one mid-message
  (`tool_call.markup.missing-function-close`,
  `tool_call.markup.missing-wrapper-close` where the next call's opener or
  the end of output follows; before text that no closer follows up to the
  next opener or the end of output, the text may be content or inside a
  wrapper that never closes, so the insertion is the guess
  `tool_call.markup.missing-wrapper-close.before-text`, never usable under
  `Structural`) or at the end of output
  (`tool_call.markup.missing-closers-at-end`, usable only when the call is
  otherwise complete), keeping the least-cost reading. A closer spelling
  immediately after a complete call (`</function></function>`,
  `</tool_call></tool_call>`) is absorbed into that call as a stray closer,
  never left as content. A stray `</parameter>` between parameters is
  deleted by the codec, a body of only closers is no arguments
  (`tool_call.markup.closers-only-empty`), a declared parameter's opener ends
  an unclosed value (`tool_call.markup.missing-parameter-close`), and the
  end of the function element ends one
  (`tool_call.markup.parameter-closed-by-element`).

These are repairs and are covered by the repair goldens, not by the
conformance anchors, which hold canonical markup only.

## Deliberately not accepted

- Several `<function=...>` elements inside one `<tool_call>` wrapper: the
  template always wraps each call separately.
- Attributes or quoting in the opener (`<function name="x">`,
  `<parameter name="p">`): these belong to other XML dialects.
- Text between parameters other than whitespace, text after a whole-JSON
  body, and text between `</function>` and a later closer such as
  `</tool_call>` (the call is malformed).
- Python-style literals in typed values (`True`, `None`) are not canonical;
  the JSON codec's literal-case repair applies and is recorded.
- Streaming deltas (no `delta` production).
