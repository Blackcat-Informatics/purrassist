<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `harmony.v1`

Grammar: [`crates/core/src/tool_call/dialects/harmony.v1.grammar`](../../../crates/core/src/tool_call/dialects/harmony.v1.grammar),
built in as `DialectId::Harmony`. The grammar and its fixtures are
first-party; no format documentation or renderer code is copied into this
repository.

## The format

The `OpenAI` Harmony response format (gpt-oss-20b and gpt-oss-120b) writes
an assistant turn as a sequence of messages. Each message is
`<|start|>`, a header, `<|message|>`, the body, and an end token. The header
holds the role (`assistant`), the channel after `<|channel|>` (`analysis`
for the chain of thought, `commentary` for tool calls and preambles,
`final` for the answer), and, for a tool call, the recipient `to=NAME`,
which may sit in the role section (before `<|channel|>`) or in the channel
section (after the channel name). A call to a JSON tool may name the content
type: ` <|constrain|>json`. A message ends with `<|end|>`; a tool call ends
with `<|call|>`, and the last message of a finished turn with `<|return|>`.
The prompt ends with `<|start|>assistant`, so the model's output usually
begins directly with the first message's header.

Function tools are declared in the developer message's `# Tools` section,
in a `functions` namespace, as TypeScript-like `type NAME = (_: {...}) =>
any;` declarations with comment lines; their calls go to
`functions.NAME`. The built-in tools take other recipients:
`browser.search`, `browser.open` and `browser.find` (JSON arguments, in the
analysis channel), and `python` (the body is Python code). A tool's output
returns as a message from the tool to the assistant,
`<|start|>functions.NAME to=assistant<|channel|>commentary<|message|>OUTPUT<|end|>`.

## Public sources

- `openai/harmony` at commit `abd677f7ac962629c808197caa1feb9e3e95d2b0`
  (2026-04-08):
  - `docs/format.md`: the message structure, channels, end tokens, function
    calling (`to=functions.NAME`, "the recipient might be defined in the
    role or channel section of the header", `<|constrain|>json`), tool
    messages, the `functions` namespace declarations, and the built-in
    browser and python tools;
    <https://github.com/openai/harmony/blob/abd677f7ac962629c808197caa1feb9e3e95d2b0/docs/format.md>
  - `src/encoding.rs`: the renderer (the recipient rendered in the role
    section, ` to=NAME` before `<|channel|>`; the content type with a space
    before `<|constrain|>`; `<|call|>` ending an assistant message with a
    recipient) and the parser (the channel value ends at whitespace or `<`;
    `<|constrain|>` directly after the recipient is accepted; the last
    header token is the recipient or the content type);
    <https://github.com/openai/harmony/blob/abd677f7ac962629c808197caa1feb9e3e95d2b0/src/encoding.rs>
- vLLM's gpt-oss tool parser:
  <https://github.com/vllm-project/vllm/blob/03ce1c6ed908788cf508219b666720783a723123/vllm/tool_parsers/openai_tool_parser.py>
  (commit `03ce1c6ed908788cf508219b666720783a723123`): every parsed message
  whose recipient starts with `functions.` is a tool call (arguments parsed
  as JSON when the content type is absent or holds `json`); a `final`
  message, or a `commentary` message without a recipient, is content; the
  built-in recipients are not returned as tool calls.

## What the grammar accepts

- `call`: one assistant message with a recipient, with or without the
  `<|start|>assistant` prefix:
  - the recipient in the channel section,
    `<|channel|>commentary to=functions.NAME <|constrain|>json<|message|>ARGS<|call|>`
    (the documented layout);
  - the recipient in the role section,
    `to=functions.NAME<|channel|>commentary <|constrain|>json<|message|>ARGS<|call|>`
    (the renderer's layout);
  - the content type may be ` <|constrain|>json` (with or without the space
    before it), ` json`, or left out; the channel may be `commentary`,
    `analysis` or `final`.

  Each message is its own call; several calls are several messages.
- The call markup around the arguments is special tokens and text, not a
  JSON envelope, so the grammar does not declare `json-envelope;`: the JSON
  syntax repairs (`json.syntax.*`) apply inside the JSON arguments, read by
  the `json` codec. See [GRAMMAR.md](../GRAMMAR.md).
- Commit point: a message becomes a call attempt only at its recipient (in
  the role-section layout, at the `<|channel|>` after it). A message without
  a recipient never opens one:
  - the `analysis` channel is reasoning (the declared markers
    `<|channel|>analysis<|message|>` and `<|end|>`);
  - `final` messages and `commentary` messages without a recipient
    (preambles) are content.
- Recipients:
  - `functions.NAME`: a call to `NAME`;
  - a dotted built-in recipient (`browser.search`, `browser.open`, ...): a
    call to the tool of that name, with JSON arguments; a declared tool of
    that name resolves it, otherwise it is `Unresolved` (unknown tool),
    never silent content;
  - `python`: a call to `python` whose arguments are the body as raw text
    (Python code); `Unresolved` (unknown tool, or arguments not an object)
    unless declared and fitting. Code may hold the text of a call closer
    (`print("<|call|>")`): the body is read both ending at that closer and
    holding it, up to the closer that completes the call
    (`tool_call.value.closer-in-value`). A raw body needs no repair either
    way, so when the rest parses cleanly both readings are reported (the
    call is `Ambiguous`); see
    [Closer text inside a value](qwen3-coder-xml.v1.md#closer-text-inside-a-value).
    A `<|start|>` (the next message) always ends the code.
  - any other bare `NAME`: read as `functions.NAME`, the namespace recorded
    as inserted (`tool_call.markup.missing-token`).
- `ARGS` is read by the self-delimiting `json` codec (exact number
  spellings; a stringified object decoded and flagged).
- Repairs, each recorded:
  - the call's end is a closer class: `<|end|>` or `<|return|>` for
    `<|call|>` is a variant (`tool_call.markup.closer-variant`), and a
    missing end at the end of output is inserted
    (`tool_call.markup.missing-closers-at-end`);
  - `<|channel>` and `<channel|>` are confusable spellings of `<|channel|>`
    (`tool_call.markup.terminal-confusable`);
  - text after the JSON arguments, before the end token, is dropped
    (`tool_call.extract.prose`, a data loss, so a best guess); a message has
    no outside, so text after the arguments with no end token at all is
    dropped the same way;
  - a recipient written after `<|message|>`
    (`<|channel|>commentary<|message|>to=functions.NAME {...}`) is read, the
    early `<|message|>` dropped (`tool_call.extract.prose`, a data loss: the
    body might have been a preamble that mentions a recipient);
  - a length stop inside the arguments truncates the call (`Unresolved`,
    `Truncated`).
- Calls carry no id; every call is numbered by the id policy.
- `result`: one tool message,
  `<|start|>functions.NAME to=assistant<|channel|>commentary<|message|>CONTENT<|end|>`,
  captured as `name:` and `content:`.
- `declaration`: one function of the `# Tools` section:
  `// DESC` (optional), a newline, and `type NAME = (_: SCHEMA) => any;`
  (`type NAME = () => any;` for a tool without parameters), with `SCHEMA` the
  tool's exact parameters schema as JSON. The renderer's TypeScript
  expansion of the schema is not reproduced (the exact text is what
  arguments are validated against); a multi-line description is printed as
  it is, its later lines without `// `.

## Printing (canonical form)

- `call`: `<|start|>assistant<|channel|>commentary to=functions.NAME <|constrain|>json<|message|>ARGS<|call|>`,
  the documented layout, one message per call (separated by a newline);
  `ARGS` is compact JSON.
- `result`: `<|start|>functions.NAME to=assistant<|channel|>commentary<|message|>CONTENT<|end|>`.
- `declaration`: `// DESC\ntype NAME = (_: SCHEMA) => any;`.

## Deliberately not accepted

- Content types other than `json` (`<|constrain|>` with another format).
- A recipient in both header sections, or more than one recipient.
- Messages from roles other than the assistant, except tool messages read
  by `result`.
- Streaming deltas (no `delta` production).
