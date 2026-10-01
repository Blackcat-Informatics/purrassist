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
declarations in `--tools` (`--tools-format openai`, `mcp`, or `anthropic`).
It prints the same result as the `recover_tool_calls` tool: per-call outcomes,
itemized repairs and losses, rejected attempts, the tool-choice check, the
turn's finish, an optional `--emit` re-emission (`openai` for the `tool_calls`
array, or a dialect id), and a receipt. Call ids are numbered from
`--ids-prefix`/`--ids-start` (default `call_0`, `call_1`, ...), or with
`--ids-source` taken from the output itself (deduplicated); combining
`--ids-source` with either numbering option is an invocation error. `--help`
lists every option; repeated single-valued options are rejected. It exits 0 for resolved or best-guess
results, 3 for ambiguous, 4 for unresolved, and 1 for invocation errors. It
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
