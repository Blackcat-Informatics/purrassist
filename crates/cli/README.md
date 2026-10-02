<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-cli

The `purrassist` executable exposes schema-guided JSON recovery, validation,
matching, shared tool calls, and a newline-delimited stdio MCP server.

```sh
purrassist decode --schema schema.json --input response.txt
purrassist match raeder reader writer
purrassist recover-tool-calls --tools tools.json --emit openai turn.txt
purrassist mcp --root /path/to/workspace
```

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
`--ids-source` with either numbering option is an invocation error. `--help`
lists every option; repeated single-valued options are rejected. It exits 0 when at least one call is usable (`usable_accepting_guesses`) and the
tool-choice check passed, even if other attempts in the turn were rejected or
are unparsed call markup (the JSON itemizes them, so a script keyed on the
exit code never discards good calls because of one stray opener), and for
clean content (finish `content_only`). It exits 3 when nothing is usable and
the operation or a call is ambiguous, and 4 when nothing is usable (unresolved
calls, only rejected or unparsed call markup, a `truncated` turn, only guesses
that may not be used, such as a mutating tool's, or an `Unresolved` operation:
an exhausted budget, invalid input) or the tool-choice check failed, even with
usable calls: a violated tool choice is a contract failure the caller must
see. Invocation errors exit 1. The printed JSON is the same whatever the exit
code; the code only summarizes it. It
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

Pre-release implementation. See the
[repository](https://github.com/Blackcat-Informatics/purrassist) for current
limitations and verification. First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0.
