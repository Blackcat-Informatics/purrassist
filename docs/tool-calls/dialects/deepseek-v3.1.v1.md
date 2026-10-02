<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `deepseek-v3.1.v1`

Grammar: [`crates/core/src/tool_call/dialects/deepseek-v3.1.v1.grammar`](../../../crates/core/src/tool_call/dialects/deepseek-v3.1.v1.grammar),
built in as `DialectId::DeepSeekV31`. The grammar and its fixtures are
first-party; no chat-template or model-card text is copied into this
repository.

## The format

DeepSeek-V3.1, V3.1-Terminus and V3.2-Exp write a turn's calls as one
*tool-calls section*:

1. `<｜tool▁calls▁begin｜>` opens the section;
2. each call is `<｜tool▁call▁begin｜>`, the function name,
   `<｜tool▁sep｜>`, the arguments as JSON, and `<｜tool▁call▁end｜>`;
3. the calls are chained with no separator;
4. `<｜tool▁calls▁end｜>` closes the section, and the message ends with
   `<｜end▁of▁sentence｜>`.

The special tokens are spelled with the fullwidth vertical line `｜`
(U+FF5C) and the lower one-eighth block `▁` (U+2581). A tool result goes
back between `<｜tool▁output▁begin｜>` and `<｜tool▁output▁end｜>`.

In thinking mode the generation prompt ends with `<think>`, so the model's
output starts inside its reasoning and closes it with `</think>`; R1-style
outputs write the whole `<think>…</think>` block. The calls carry no id.
The tools are listed in the system prompt under `## Tools`, one
`### NAME` heading per tool with `Description:` and `Parameters:` lines.

DeepSeek-V3 (0324) and R1-0528 use a different call body (a `function` type,
the name on its own line, and the arguments in a fenced JSON block); that is
the separate [`deepseek-v3.v1`](deepseek-v3.v1.md) dialect.

## Public sources

- DeepSeek-V3.1 chat template (`assets/chat_template.jinja`):
  <https://huggingface.co/deepseek-ai/DeepSeek-V3.1/blob/c0781d039fb7a1ba2abc4add0bdc293e92d2b8db/assets/chat_template.jinja>
  (revision `c0781d039fb7a1ba2abc4add0bdc293e92d2b8db`, 2025-09-05):
  - an assistant message's calls render as `<｜tool▁calls▁begin｜>`, then
    per call `<｜tool▁call▁begin｜>` + name + `<｜tool▁sep｜>` + arguments +
    `<｜tool▁call▁end｜>`, then `<｜tool▁calls▁end｜><｜end▁of▁sentence｜>`;
  - tool messages render between `<｜tool▁output▁begin｜>` and
    `<｜tool▁output▁end｜>`;
  - in thinking mode the generation prompt ends with `<think>`.
- DeepSeek-V3.1 model card (`README.md`, same revision):
  <https://huggingface.co/deepseek-ai/DeepSeek-V3.1/blob/c0781d039fb7a1ba2abc4add0bdc293e92d2b8db/README.md>:
  - the tool description format (`## Tools`, `### {name}`,
    `Description:`, `Parameters:` with the schema as JSON);
  - the call format, and that several calls are chained directly, without
    separators or spaces.
- DeepSeek-V3.1-Terminus and DeepSeek-V3.2-Exp chat templates, the same call
  rendering:
  <https://huggingface.co/deepseek-ai/DeepSeek-V3.1-Terminus/blob/19510d6dc61f79dbd925bd51ee8a9081c509a4b6/tokenizer_config.json>
  (revision `19510d6dc61f79dbd925bd51ee8a9081c509a4b6`, 2025-09-29) and
  <https://huggingface.co/deepseek-ai/DeepSeek-V3.2-Exp/blob/194c67e12b1b0d6df0ef373ddcf215bc84027409/assets/chat_template.jinja>
  (revision `194c67e12b1b0d6df0ef373ddcf215bc84027409`, 2025-11-18).
- vLLM's DeepSeek-V3.1 tool parser at release v0.10.2:
  <https://github.com/vllm-project/vllm/blob/01efc7ef781391e744ed08c3292817a773d654e6/vllm/entrypoints/openai/tool_parsers/deepseekv31_tool_parser.py>
  (commit `01efc7ef781391e744ed08c3292817a773d654e6`): the same section and
  call tokens, each call's name and arguments taken from between
  `<｜tool▁call▁begin｜>`, `<｜tool▁sep｜>` and `<｜tool▁call▁end｜>`.
- DeepSeek-R1-0528 chat template, which uses the V3 body instead:
  <https://huggingface.co/deepseek-ai/DeepSeek-R1-0528/blob/4236a6af538feda4548eca9ab308586007567f52/tokenizer_config.json>
  (revision `4236a6af538feda4548eca9ab308586007567f52`, 2025-05-29).

## What the grammar accepts

- `call`: one tool-calls section. Each call of the section is its own call,
  separated by the DSL's `next`
  ([GRAMMAR.md](../GRAMMAR.md#several-calls-in-one-attempt)):
  - the first call's span holds the section opener;
  - the last call's span holds the section closer.

  Whitespace between the tokens and around the name is markup.
- The arguments value is read by the self-delimiting `json` codec: exact
  number spellings, member order and duplicates kept; stringified arguments
  decoded and flagged (`tool_call.arguments.decode-string`); the codec knows
  the declared tool the name refers to.
- Closers: `<｜tool▁call▁end｜>` and `<｜tool▁calls▁end｜>` are closer
  classes.
  - A missing section closer at the end of output under a stop (the common
    case) is inserted (`tool_call.markup.missing-closers-at-end`).
  - A missing call closer before the next call is inserted
    (`tool_call.markup.missing-function-close`).
  - A missing section closer before another section is inserted
    (`tool_call.markup.missing-wrapper-close`).
  - A repeated closer is absorbed (`tool_call.markup.stray-closer`).
  - Before prose, a missing section closer is the guess
    `tool_call.markup.missing-wrapper-close.before-text`, admitted only
    under `Tolerant`.
- A length stop inside a call's arguments truncates that call
  (`Unresolved`, `Truncated`). The calls before it in the section are whole.
- ASCII renderings of the tokens, from detokenizers that do not keep their
  spellings, are read through declared confusables (`confusable "｜" "|"`,
  `confusable "▁" "_"`, `confusable "▁" " "`). For example,
  `<|tool_calls_begin|>`, `<|tool▁calls▁begin|>` and `<|tool calls begin|>`.
  - Each confusable spelling is recorded as
    `tool_call.markup.terminal-confusable` (a markup repair), so the call
    stays `Resolved`.
  - A confusable opener that no call follows is a rejected attempt, never
    silent content.
- A section that starts directly at `<｜tool▁call▁begin｜>`, with
  `<｜tool▁calls▁begin｜>` left out, is read with the token recorded as
  inserted (`tool_call.markup.missing-token`).
- Reasoning: `<think>` … `</think>`. A thinking-mode output that starts
  inside the block is read with `ReasoningPolicy::Separate { starts_open:
  true }`.
- `result`: `<｜tool▁output▁begin｜>CONTENT<｜tool▁output▁end｜>`, content
  verbatim; the result carries no call id.
- `declaration`: one `## Tools` entry:
  - `### NAME`;
  - an optional `Description: DESC` line followed by a blank line;
  - `Parameters: SCHEMA`.
- Calls carry no id. Every call is numbered by the id policy with no
  inference.

## Printing (canonical form)

The printer (`tool_call::emit`) runs the same productions forward with
canonical choices:

- `call`: every emitted call in one section,
  `<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>NAME<｜tool▁sep｜>ARGS<｜tool▁call▁end｜>…<｜tool▁calls▁end｜>`.
  This is the template's layout, with the canonical fullwidth spellings and
  no whitespace. `ARGS` is the compact JSON object of the argument tree with
  exact number spellings and member order.
- `result`: `<｜tool▁output▁begin｜>CONTENT<｜tool▁output▁end｜>`.
- `declaration`: `### NAME\nDescription: DESC\n\nParameters: SCHEMA`, with
  `SCHEMA` the tool's exact parameters schema text verbatim, and
  `{"type":"object"}` for a tool declared without one (the template always
  prints a schema).

## Deliberately not accepted

- The DeepSeek-V3 / R1-0528 body (`function<｜tool▁sep｜>NAME` and a fenced
  JSON block): the [`deepseek-v3.v1`](deepseek-v3.v1.md) dialect.
- Calls outside a section with both the section opener and every call
  opener lost.
- Other renderings of the tokens than the declared ASCII bar, underscore and
  space.
- Streaming deltas (no `delta` production).
