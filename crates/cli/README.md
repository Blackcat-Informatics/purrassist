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
include `decode_json`, `match_strings`, `plan_replacement`, `plan_diff`,
`inspect_code`, `read_file`, `apply_file`, and `undo_file`. Planning tools return
source-bound proposals with explicit uncertainty; they do not write files.

Pre-release implementation. See the
[repository](https://github.com/Blackcat-Informatics/purrassist) for current
limitations and verification. First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0.
