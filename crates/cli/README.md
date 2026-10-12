<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-cli

The `purrassist` executable exposes schema-guided JSON recovery, validation,
matching, shared tool calls, and a newline-delimited stdio MCP server.

**Status: 0.1.0.** This crate is the CLI and MCP interface of `PurRAssist`, one
engine with three equal interfaces (the Rust crates, the MCP server and the
CLI): `cargo install purrassist-cli` installs the `purrassist` executable.

```sh
purrassist decode --schema schema.json --input response.txt
purrassist match raeder reader writer
purrassist recover-tool-calls --tools tools.json --emit openai turn.txt
purrassist mcp --root /path/to/workspace
```

To use the server from an MCP client that launches stdio servers, configure the
command `purrassist` with the arguments `mcp`, `--root` and the workspace
directory. `mcp` and `call` create a `.purrassist/` journal directory in that
root.

`recover-tool-calls` reads one model turn as raw bytes from `FILE` or stdin
(16 MiB limit; invalid UTF-8 is kept and recorded as a
`tool_call.input.invalid-utf8` loss) and recovers its tool calls against the
declarations in `--tools` (`--tools-format openai`, `mcp`, or `anthropic`),
whose bytes the adapters read exactly as written (their render-back and schema
digests refer to the file's own bytes).
It prints the same result as the `recover_tool_calls` tool: per-call outcomes,
itemized repairs and losses, rejected attempts, the tool-choice check, the
turn's finish, an optional `--emit` re-emission (`openai` for the `tool_calls`
array, or a dialect id), and a receipt. By default each call keeps the id
the model wrote (an `OpenAI` `tool_calls[].id`), and every other call is
numbered `call_<tag>_<index>`, where `<tag>` is the first 12 hexadecimal
digits of the output's SHA-256 digest: different turns never share ids
without any caller state, the same output always gets the same ids, and a
missing or repeated model id is numbered and recorded as a
`tool_call.id.dedup` inference. `--ids-prefix P`/`--ids-start N` number every
call `P(N + index)` instead (prefix `call_`, start 0 when omitted; pass the
result's `next_id_start` as the next round's `--ids-start`), and
`--ids-source` keeps the output's ids, deduplicated as `ID#INDEX`; combining
`--ids-source` with either numbering option is an invocation error.
`--tool-choice NAME` forces the tool `NAME` (at least one call, all to
`NAME`); adding `--tool-choice-exactly-one`, `--tool-choice-no-content`
(content other than ASCII whitespace fails) and `--tool-choice-no-reasoning`
makes the forced choice strict, and is an invocation error without
`--tool-choice NAME` (a tool name, not `auto`, `none` or `required`). A failed
choice exits 4. `--help`
lists every option; repeated single-valued options are rejected. The exit
code is `tool_call::exit_code` of the result, so it always agrees with the
printed `value.verdict`: 0 for `run` (the tool-choice check passed and at
least one call is usable, `usable_accepting_guesses`, even beside rejected
attempts or unparsed call markup, which the JSON itemizes) and `no_calls`
(clean content, finish `content_only`); 3 for `ambiguous` (nothing usable
and a call is ambiguous) or an `Ambiguous` operation; 4 for `nothing_usable`
(unresolved calls, only rejected or unparsed call markup, a `truncated`
turn, only guesses that may not be used, such as a mutating tool's, or a
failed tool-choice check even beside usable calls) or an `Unresolved`
operation (an exhausted budget, invalid input). The JSON's `status` is the
certainty of the reading of the turn, not whether anything can run: a turn
whose only call names an undeclared tool is `status: "resolved"` with
`verdict: "nothing_usable"` and exits 4 (see the tool-calls
[INTEGRATION.md](https://github.com/Blackcat-Informatics/purrassist/blob/rust-v0.1.0/docs/tool-calls/INTEGRATION.md#reading-a-result-status-finish-and-verdict)).
Invocation errors exit 1. The printed JSON is the same whatever the exit
code. It
never executes a call.

`call TOOL --root DIRECTORY` accepts a JSON argument object on stdin. Tools
include `decode_json`, `recover_tool_calls`, `match_strings`, `match_embeddings`, `plan_replacement`, `plan_diff`,
`inspect_code`, `render_markdown`, `inspect_markdown`, `plan_markdown_table_cell`, `plan_markdown_table_row`,
`read_file`, `apply_file`, and `undo_file`. Planning tools return
source-bound proposals with explicit uncertainty; they do not write files.

`render_markdown` accepts exact `source`, optional `dialect` (`commonmark` or
`gfm`), optional `extensions`, and `effort`. The returned receipt retains the
source; the result identifies the profile and contains HTML. An explicit extension
array replaces dialect defaults. GFM tag filtering is not a general sanitizer.

`plan_markdown_table_cell` accepts `source`, a nested table `path`, zero-based
`row`/`column`, and replacement inline `markdown`. Row zero is the header.
The proposal preserves surrounding bytes and is reparsed before it is returned.
`plan_markdown_table_row` uses a table `path`, `action` (`insert` or `remove`),
and zero-based body `index`. Insertion supplies `cells` matching the header width;
an index equal to the row count appends. The header and delimiter are not body rows.
`plan_markdown_table_column` selects a table `path` and column `index` with action
`insert`, `remove`, or `align`. Insert supplies `header`, `alignment`, and one
`cells` value per body row; align supplies `alignment`. Plans preserve existing
cell spelling and validate the resulting table before returning.
`inspect_markdown` also returns reference occurrences and their original field
spans. `plan_markdown_reference` takes `path`, `occurrence`, decoded `destination`,
and explicit `title` (string or `null`). It preserves the selected label and all
unselected definitions, with a structural-validation receipt and conditional undo.

Version 0.1.0. Capabilities, known limitations and verification are recorded in
the [qualification record](https://github.com/Blackcat-Informatics/purrassist/blob/rust-v0.1.0/docs/QUALIFICATION.md).
First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0.

## Links

- Tool reference: [docs/TOOLS.md](https://github.com/Blackcat-Informatics/purrassist/blob/rust-v0.1.0/docs/TOOLS.md)
- Repository: <https://github.com/Blackcat-Informatics/purrassist>
- MCP Registry name: `mcp-name: io.github.Blackcat-Informatics/purrassist`
