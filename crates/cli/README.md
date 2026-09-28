<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-cli

The `purrassist` executable exposes schema-guided JSON recovery, validation,
matching, shared tool calls, and a newline-delimited stdio MCP server.

```sh
purrassist decode --schema schema.json --input response.txt
purrassist match raeder reader writer
purrassist mcp --root /path/to/workspace
```

`call TOOL --root DIRECTORY` accepts a JSON argument object on stdin. Tools
include `decode_json`, `match_strings`, `match_embeddings`, `plan_replacement`, `plan_diff`,
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

Pre-release implementation. See the
[repository](https://github.com/Blackcat-Informatics/purrassist) for current
limitations and verification. First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0.
