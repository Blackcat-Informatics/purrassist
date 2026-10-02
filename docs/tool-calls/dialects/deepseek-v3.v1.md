<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `deepseek-v3.v1`

Grammar: [`crates/core/src/tool_call/dialects/deepseek-v3.v1.grammar`](../../../crates/core/src/tool_call/dialects/deepseek-v3.v1.grammar),
built in as `DialectId::DeepSeekV3`. The grammar and its fixtures are
first-party; no chat-template or model-card text is copied into this
repository.

## The format

DeepSeek-V3, V3-0324, R1 and R1-0528 write a turn's calls as one
*tool-calls section*, with the same special tokens as DeepSeek-V3.1
([`deepseek-v3.1.v1`](deepseek-v3.1.v1.md)) but a different call body:

1. `<｜tool▁calls▁begin｜>` opens the section;
2. each call is `<｜tool▁call▁begin｜>`, the call's type (always
   `function`), `<｜tool▁sep｜>`, the function name, a newline, the
   arguments as JSON in a fenced block (a `` ```json `` line, the JSON, a
   newline and a closing `` ``` ``), and `<｜tool▁call▁end｜>`;
3. calls after the first are each preceded by a newline;
4. `<｜tool▁calls▁end｜>` closes the section, and the message ends with
   `<｜end▁of▁sentence｜>`.

The special tokens are spelled with the fullwidth vertical line `｜`
(U+FF5C) and the lower one-eighth block `▁` (U+2581). Tool results go back
inside `<｜tool▁outputs▁begin｜>` … `<｜tool▁outputs▁end｜>`, one
`<｜tool▁output▁begin｜>CONTENT<｜tool▁output▁end｜>` per result, separated
by newlines.

R1 writes a `<think>` … `</think>` reasoning block before its answer; the
original R1 template ends the generation prompt with `<think>` and a
newline, so that model's output starts inside the block. The calls carry no
id. The chat templates do not render the request's tool list; vLLM's
DeepSeek-V3 and R1 tool templates add it to the system prompt, the R1 one as
one fenced JSON block per tool holding the OpenAI tool object.

The original DeepSeek-V3 template (revision below) closes the section after
every call but the first when it renders several; each such section after
the first then starts at `<｜tool▁call▁begin｜>`, which the grammar reads
with the section opener recorded as inserted. The V3-0324, R1 and R1-0528
templates write one section.

## Public sources

- DeepSeek-V3-0324 chat template (`tokenizer_config.json`, `chat_template`):
  <https://huggingface.co/deepseek-ai/DeepSeek-V3-0324/blob/e9b33add76883f293d6bf61f6bd89b497e80e335/tokenizer_config.json>
  (revision `e9b33add76883f293d6bf61f6bd89b497e80e335`, 2025-03-27):
  - an assistant message's calls render as `<｜tool▁calls▁begin｜>`, the
    first call `<｜tool▁call▁begin｜>` + type + `<｜tool▁sep｜>` + name +
    newline + `` ```json `` + newline + arguments + newline + `` ``` `` +
    `<｜tool▁call▁end｜>`, each later call the same after a newline, then
    `<｜tool▁calls▁end｜><｜end▁of▁sentence｜>`;
  - tool messages render as `<｜tool▁outputs▁begin｜>`, then per result
    `<｜tool▁output▁begin｜>` + content + `<｜tool▁output▁end｜>` (later ones
    after a newline), then `<｜tool▁outputs▁end｜>`.
- DeepSeek-R1-0528 chat template, the same call and output rendering:
  <https://huggingface.co/deepseek-ai/DeepSeek-R1-0528/blob/4236a6af538feda4548eca9ab308586007567f52/tokenizer_config.json>
  (revision `4236a6af538feda4548eca9ab308586007567f52`, 2025-05-29).
- DeepSeek-R1 chat template, the same call rendering, with the generation
  prompt ending in `<think>` and a newline:
  <https://huggingface.co/deepseek-ai/DeepSeek-R1/blob/56d4cbbb4d29f4355bab4b9a39ccb717a14ad5ad/tokenizer_config.json>
  (revision `56d4cbbb4d29f4355bab4b9a39ccb717a14ad5ad`, 2025-03-27).
- DeepSeek-V3 chat template, the same call body, with a section closer
  after every later call:
  <https://huggingface.co/deepseek-ai/DeepSeek-V3/blob/e815299b0bcbac849fa540c768ef21845365c9eb/tokenizer_config.json>
  (revision `e815299b0bcbac849fa540c768ef21845365c9eb`, 2025-03-27).
- vLLM's DeepSeek-V3 tool parser at release v0.10.2:
  <https://github.com/vllm-project/vllm/blob/01efc7ef781391e744ed08c3292817a773d654e6/vllm/entrypoints/openai/tool_parsers/deepseekv3_tool_parser.py>
  (commit `01efc7ef781391e744ed08c3292817a773d654e6`): the same section and
  call tokens; each call is the type, `<｜tool▁sep｜>`, the name up to a
  newline, and the arguments between a `` ```json `` line and a newline
  followed by `` ``` `` and `<｜tool▁call▁end｜>`. The type is not checked.
- vLLM's DeepSeek-V3 and R1 tool chat templates at the same commit, which
  list the request's tools in the system prompt (the R1 one as a fenced JSON
  block per tool) and render calls with the body above:
  <https://github.com/vllm-project/vllm/blob/01efc7ef781391e744ed08c3292817a773d654e6/examples/tool_chat_template_deepseekv3.jinja>
  and
  <https://github.com/vllm-project/vllm/blob/01efc7ef781391e744ed08c3292817a773d654e6/examples/tool_chat_template_deepseekr1.jinja>.

## What the grammar accepts

- `call`: one tool-calls section. Each call of the section is its own call,
  separated by the DSL's `next`
  ([GRAMMAR.md](../GRAMMAR.md#several-calls-in-one-attempt)):
  - the first call's span holds the section opener;
  - the last call's span holds the section closer.

  Whitespace between the tokens, around the type and the name, and around
  the fence lines is markup.
- The arguments value is read by the self-delimiting `json` codec: exact
  number spellings, member order and duplicates kept; stringified arguments
  decoded and flagged (`tool_call.arguments.decode-string`); the codec knows
  the declared tool the name refers to. A JSON string value may hold a code
  fence of its own: a raw newline is never inside a JSON string, so only a
  newline followed by `` ``` `` ends a value cut short.
- The fenced block, each repair recorded so the call stays `Resolved`:
  - a missing opening fence line, a fence without its `json` info string,
    and a missing closing fence are read with the token recorded as inserted
    (`tool_call.markup.missing-token`); so is a block with neither fence;
  - a repeated opening or closing fence line is dropped
    (`tool_call.extract.fence`, a markup repair);
  - an opening fence naming another language (`` ```javascript ``) is
    dropped (`tool_call.extract.fence`) and the `json` fence read as
    inserted; the arguments are still read as JSON.
- The type:
  - a missing type is read as an inserted `function`
    (`tool_call.markup.missing-token`);
  - a type other than `function` (`tool`, `Function`) is dropped and the
    call read as a function call. It is recorded as
    `tool_call.extract.prose`, a data loss, so the call is a best guess,
    not `Resolved`, and runs only where the caller accepts guesses.
- Closers: `<｜tool▁call▁end｜>` and `<｜tool▁calls▁end｜>` are closer
  classes.
  - A missing section closer at the end of output under a stop is inserted
    (`tool_call.markup.missing-closers-at-end`).
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
- ASCII renderings of the tokens are read through the same declared
  confusables as DeepSeek-V3.1 (`confusable "｜" "|"`, `confusable "▁" "_"`,
  `confusable "▁" " "`), each recorded as
  `tool_call.markup.terminal-confusable` (a markup repair). A confusable
  opener that no call follows is a rejected attempt, never silent content.
- A section that starts directly at `<｜tool▁call▁begin｜>`, with
  `<｜tool▁calls▁begin｜>` left out, is read with the token recorded as
  inserted (`tool_call.markup.missing-token`).
- Reasoning: `<think>` … `</think>`. An R1 output that starts inside the
  block is read with `ReasoningPolicy::Separate { starts_open: true }`.
- `result`: `<｜tool▁output▁begin｜>CONTENT<｜tool▁output▁end｜>`, content
  verbatim; the result carries no call id.
- `declaration`: one fenced tool entry, `` ```json ``, a newline, the
  OpenAI tool object, a newline and `` ``` ``. The object is read with the
  OpenAI member order (`type`, then `function` with `name`, an optional
  `description` and optional `parameters`) or with its keys sorted (as the
  vLLM template's `tojson` writes it).
- Calls carry no id. Every call is numbered by the id policy with no
  inference.

## Telling DeepSeek-V3 from DeepSeek-V3.1

The two dialects share every special token, so a section of either opens an
attempt in both when both are allowed. The bodies decide:

- A V3 call has the type and then the name after `<｜tool▁sep｜>`. V3.1
  reads `function` as the name and expects JSON after the separator, where a
  name and a fence follow, and rejects the attempt; the V3 reading is
  selected.
- A V3.1 call has JSON right after `<｜tool▁sep｜>`, where V3 needs a name,
  so V3 rejects it at once. This holds for a V3.1 call to a tool literally
  named `function` too: what follows the separator is never a name.

- A V3 call cut off inside its arguments leaves V3.1 a repaired reading
  too (`function` as the name, the rest as guessed JSON) that can need less
  repair than the V3 reading, which inserts the closing fence and closers.
  `function` names no declared tool while the V3 reading's name does, and a
  call whose name denotes no declared tool weighs `UNDECLARED_NAME` more in
  reading selection, so the V3 reading is selected with its repairs
  recorded (`deepseek3_unclosed_arguments_at_stop`). Under a stop, a value
  completed only by end-of-output repairs inside unterminated markup is
  `Unresolved` (`Malformed`), as in every dialect with closers.

Each canonical form therefore resolves as its own dialect alone, also with
both sections in one turn
(`deepseek3_and_v31_in_one_turn`, `deepseek31_tool_named_function`).

## Printing (canonical form)

The printer (`tool_call::emit`) runs the same productions forward with
canonical choices:

- `call`: every emitted call in one section,
  `<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>NAME\n```json\nARGS\n```<｜tool▁call▁end｜>`,
  later calls after a newline, then `<｜tool▁calls▁end｜>`. This is the
  V3-0324 and R1-0528 templates' layout byte for byte, with `ARGS` the
  compact JSON object of the argument tree (exact number spellings and
  member order).
- `result`: `<｜tool▁output▁begin｜>CONTENT<｜tool▁output▁end｜>`.
- `declaration`: `` ```json `` + newline +
  `{"type":"function","function":{"name":"NAME","description":"DESC","parameters":SCHEMA}}`
  + newline + `` ``` ``, with `SCHEMA` the tool's exact parameters schema
  text verbatim; the description and the parameters appear only when the
  tool has them.

## Deliberately not accepted

- The DeepSeek-V3.1 body (`NAME<｜tool▁sep｜>ARGS` with no fence): the
  `deepseek-v3.1.v1` dialect.
- Arguments that are not JSON inside the fence.
- Calls outside a section with both the section opener and every call
  opener lost.
- Other renderings of the tokens than the declared ASCII bar, underscore and
  space.
- Streaming deltas (no `delta` production).
