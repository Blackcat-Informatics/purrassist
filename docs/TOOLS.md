<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Tool reference

Every operation PurRAssist exposes to a host is one entry of a single registry
(`crates/host/src/tools/registry.rs`). The stdio MCP server's `tools/list` and
`tools/call`, and the CLI's `call` command, both dispatch through it.

- **MCP:** `purrassist mcp --root DIR` serves the tools over newline-delimited
  stdio; a client invokes one with `tools/call`, passing the tool `name` and an
  `arguments` object.
- **CLI:** `purrassist call TOOL --root DIR` reads a JSON argument object from
  stdin, invokes the same operation used by MCP, and prints the result as JSON.
- **Rust:** the engines behind the tools are public APIs of the
  [`purrassist`](../crates/purrassist/README.md) crate.

Both `mcp` and `call` open the root as a file store, which creates a
`.purrassist/` journal directory in `DIR` if it is absent, even for tools that
read no file. The selected input and output schemas are prepared before the
handler runs, and arguments must satisfy the input schema. Its complete returned
node must satisfy the registered output schema before successful Rust, CLI or
MCP release. This validates the declared envelope, including array roots;
fields declared as `true`, such as some operation values, remain unconstrained.
The CLI and MCP results of every tool are the same, as the test
`every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` runs each
tool through both with a fixture and compares their JSON serializations for
equality. The
convenience commands `decode`, `validate`, `match` and `recover-tool-calls` are
documented under the tool each is related to.

## Reference

<!-- tools-reference:begin -->
Generated from `Tools::definitions()` by the test `tools_reference_matches_registry`; do not edit by hand. Regenerate with
`PURRASSIST_BLESS=1 cargo test -p purrassist-cli --test protocol tools_reference_matches_registry`.
`required` is the schema's top-level `required` list; requirements that depend on another argument (`oneOf`) are in the tool's input schema, which `tools/list` returns.

- **`match_embeddings`**: Match caller-supplied vectors in a named embedding space; unique cosine winners are best guesses, ties remain ambiguous, and no model is loaded
  - MCP: `tools/call` with name `match_embeddings`
  - CLI: `purrassist call match_embeddings --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `space`: string, required
    - `query`: array of number, required
    - `candidates`: array of object, required
    - `minimum_score`: number
    - `minimum_margin`: number
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/match-embeddings.json`](../examples/match-embeddings.json)
- **`render_markdown`**: Render exact Markdown source to HTML with a source-retaining receipt, dialect, and explicit extension profile; preserves raw HTML/URI schemes except selected GFM tag filtering and is not a sanitizer
  - MCP: `tools/call` with name `render_markdown`
  - CLI: `purrassist call render_markdown --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `dialect`: one of "commonmark", "gfm"
    - `extensions`: array
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/render-markdown.json`](../examples/render-markdown.json)
- **`inspect_markdown`**: Index exact Markdown block ranges, source line maps, nested containers, and sections
  - MCP: `tools/call` with name `inspect_markdown`
  - CLI: `purrassist call inspect_markdown --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
- **`markdown_context`**: Extract exact block-aligned Markdown context and enclosing sections
  - MCP: `tools/call` with name `markdown_context`
  - CLI: `purrassist call markdown_context --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `start`: integer, required
    - `end`: integer, required
    - `neighbors`: integer
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
- **`plan_markdown_list_item`**: Insert, replace, or remove a root or nested Markdown list item with structural validation and exact source identity
  - MCP: `tools/call` with name `plan_markdown_list_item`
  - CLI: `purrassist call plan_markdown_list_item --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `path`: array of integer, required
    - `action`: one of "before", "after", "replace", "remove", required
    - `marker`: string
    - `body`: string
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/markdown-list-item.json`](../examples/markdown-list-item.json)
- **`plan_markdown_section`**: Plan a structurally validated Markdown section body replacement selected by root heading index or nested path
  - MCP: `tools/call` with name `plan_markdown_section`
  - CLI: `purrassist call plan_markdown_section --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `heading`: integer
    - `path`: array of integer
    - `body`: string, required
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/markdown-section.json`](../examples/markdown-section.json)
- **`plan_markdown_reference`**: Plan replacement of a selected Markdown reference definition's decoded destination and title
  - MCP: `tools/call` with name `plan_markdown_reference`
  - CLI: `purrassist call plan_markdown_reference --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `path`: array of integer, required
    - `occurrence`: integer, required
    - `destination`: string, required
    - `title`: string or null, required
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/markdown-reference.json`](../examples/markdown-reference.json)
- **`plan_markdown_table_column`**: Plan a nested GFM column insertion, removal, or alignment change with structural validation
  - MCP: `tools/call` with name `plan_markdown_table_column`
  - CLI: `purrassist call plan_markdown_table_column --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `path`: array of integer, required
    - `action`: one of "insert", "remove", "align", required
    - `index`: integer, required
    - `header`: string
    - `alignment`: one of "default", "left", "right", "center"
    - `cells`: array of string
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/markdown-table-column.json`](../examples/markdown-table-column.json)
- **`plan_markdown_table_row`**: Plan insertion or removal of a GFM body row by nested table path and zero-based body index
  - MCP: `tools/call` with name `plan_markdown_table_row`
  - CLI: `purrassist call plan_markdown_table_row --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `path`: array of integer, required
    - `action`: one of "insert", "remove", required
    - `index`: integer, required
    - `cells`: array of string
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/markdown-table-row.json`](../examples/markdown-table-row.json)
- **`plan_markdown_table_cell`**: Plan a source-preserving GFM cell edit by nested block path; row zero is the header
  - MCP: `tools/call` with name `plan_markdown_table_cell`
  - CLI: `purrassist call plan_markdown_table_cell --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `path`: array of integer, required
    - `row`: integer, required
    - `column`: integer, required
    - `markdown`: string, required
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/markdown-table-cell.json`](../examples/markdown-table-cell.json)
- **`plan_markdown_task`**: Plan a single checkbox-state edit selected by root block index or nested block path
  - MCP: `tools/call` with name `plan_markdown_task`
  - CLI: `purrassist call plan_markdown_task --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `block`: integer
    - `path`: array of integer
    - `checked`: boolean, required
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
- **`plan_markdown_structure`**: Plan a typed structural Markdown operation: section insert/remove/move/rename, list move/renumber, table row/column move, or reference add/relabel/remove/unlink
  - MCP: `tools/call` with name `plan_markdown_structure`
  - CLI: `purrassist call plan_markdown_structure --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `op`: one of "section_insert", "section_remove", "section_move", "section_rename", "list_move", "list_renumber", "table_row_move", "table_column_move", "reference_add", "reference_relabel", "reference_remove", "reference_unlink", required
    - `heading`: integer
    - `position`: one of "start", "after"
    - `after_heading`: integer
    - `new_level`: integer
    - `title`: string or null
    - `text`: string
    - `path`: array of integer
    - `to`: integer
    - `start`: integer
    - `index`: integer
    - `label`: string
    - `destination`: string
    - `new_label`: string
    - `occurrence`: integer
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
- **`decode_json`**: Recover schema-valid JSON with effort bounds, ambiguity, and inference receipts
  - MCP: `tools/call` with name `decode_json`
  - CLI: `purrassist call decode_json --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `input`: string, required
    - `schema`: object or boolean, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `allow_guesses`: boolean
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Examples: [`examples/temperature.input.txt`](../examples/temperature.input.txt), [`examples/temperature.schema.json`](../examples/temperature.schema.json)
- **`recover_tool_calls`**: Recover the tool calls of one model turn against caller tool declarations (OpenAI, MCP, or Anthropic form) over built-in call dialects, with per-call outcomes, itemized repairs and losses, rejected attempts (including call markup of dialects not built in or not allowed, reported as unsupported_dialect), the tool-choice check, the turn's finish, optional re-emission, and a receipt; never executes a call. status is the certainty of the reading of the turn, not whether anything can run: read value.verdict (run, no_calls, ambiguous, nothing_usable) and value.usable_calls for that; both usable counts are 0 whenever value.tool_choice.passed is false, so a failed tool choice never leaves a call to run
  - MCP: `tools/call` with name `recover_tool_calls`
  - CLI: `purrassist call recover_tool_calls --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `output`: string, required
    - `tools`: array or object or string, required
    - `tools_format`: one of "openai", "mcp", "anthropic"
    - `dialects`: array
    - `stop`: one of several shapes
    - `tool_choice`: string or object
    - `tool_choice_strict`: object
    - `policy`: one of "per-tool", "structural", "tolerant"
    - `effects`: object
    - `reasoning`: one of "separate", "separate-open", "content"
    - `ambiguity`: one of "report", "reject"
    - `ids`: one of several shapes
    - `emit`: one of "none", "openai", "anthropic-tool-use.v1", "bare-json.v1", "deepseek-v3.1.v1", "deepseek-v3.v1", "harmony.v1", "hermes.v1", "kimi-k2.v1", "llama3-json.v1", "mistral-v11.v1", "mistral-v3.v1", "openai-tool-calls.v1", "qwen3-coder-xml.v1"
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: one of several shapes, required
    - `alternatives`: array, required
    - `receipt`: object, required
- **`project_result`**: Validate the complete original JSON result against its output schema, apply the closed context field mask, and emit an exact-byte checked JSON Lines artifact; structured refusals retain original source, requested controls, stage and typed cause. The byte limit covers only decoded artifact text, excluding protocol and receipt framing; no tool is executed or authorized
  - MCP: `tools/call` with name `project_result`
  - CLI: `purrassist call project_result --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `result_json`: string, required
    - `output_schema`: object or boolean, required
    - `format`: string, required
    - `context`: object, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `status`: one of "resolved", "unresolved", required
    - `original_json`: string, required
    - `requested`: object, required
    - `stage`: one of "context", "schema_serialization", "schema_compilation", "result_parsing", "validation", "selection", "serialization", "byte_limit", "complete", required
    - `error`: one of several shapes, required
    - `validation`: one of several shapes, required
    - `transformations`: array, required
    - `artifact`: one of several shapes, required
    - `receipt`: object, required
- **`validate_json`**: Validate a JSON instance against a schema, reporting violations, always-evaluated format/pattern evidence, and unsupported-vocabulary evidence
  - MCP: `tools/call` with name `validate_json`
  - CLI: `purrassist call validate_json --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `schema`: object or boolean, required
    - `instance`: any, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `valid`: boolean, required
    - `violations`: array of object, required
    - `format_evidence`: array of object, required
    - `pattern_evidence`: array of object, required
    - `unsupported_vocabularies`: array of object, required
    - `receipt`: object, required
- **`generate_json_witness`**: Generate a witness instance for a JSON Schema location, prove unsatisfiability with refutation evidence, or report explicit search exhaustion; never claims unsatisfiability from a governor hard stop
  - MCP: `tools/call` with name `generate_json_witness`
  - CLI: `purrassist call generate_json_witness --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `schema`: object or boolean, required
    - `resources`: object
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `outcome`: one of "witness", "unsatisfiable", "exhausted", required
    - `witness`: any, required
    - `refutation`: array of object, required
    - `receipt`: object, required
- **`match_strings`**: Rank caller-identified strings and aliases using exact, Unicode, lexical, distance, and optional English phonetic evidence
  - MCP: `tools/call` with name `match_strings`
  - CLI: `purrassist call match_strings --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `query`: string, required
    - `candidates`: array, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `english_phonetics`: boolean
    - `limit`: integer
  - Output:
    - `matches`: array, required
    - `receipt`: object, required
  - Example: [`examples/match-strings.json`](../examples/match-strings.json)
- **`resolve_string`**: Select a caller-identified string with explicit exact, guess, ambiguity, and weak-match outcomes
  - MCP: `tools/call` with name `resolve_string`
  - CLI: `purrassist call resolve_string --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `query`: string, required
    - `candidates`: array, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `english_phonetics`: boolean
    - `minimum_score`: integer
    - `minimum_margin`: integer
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/resolve-string.json`](../examples/resolve-string.json)
- **`match_paths`**: Resolve caller-supplied relative paths with context, explicit ambiguity, and guess receipts; never opens or authorizes a matched path
  - MCP: `tools/call` with name `match_paths`
  - CLI: `purrassist call match_paths --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `query`: string, required
    - `paths`: array of string, required
    - `context`: string
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `minimum_score`: integer
    - `minimum_margin`: integer
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
- **`resolve_intent`**: Resolve explicit semantic domain rules against caller concepts; unknown words fail, conflicting interpretations remain ambiguous, and no model is contacted
  - MCP: `tools/call` with name `resolve_intent`
  - CLI: `purrassist call resolve_intent --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `input`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
    - `domain`: object, required
    - `concepts`: array of object, required
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
- **`plan_replacement`**: Plan exact or bounded fuzzy text replacement; approximate selections remain guesses and ties remain ambiguous
  - MCP: `tools/call` with name `plan_replacement`
  - CLI: `purrassist call plan_replacement --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `needle`: string, required
    - `replacement`: string, required
    - `max_edits`: integer
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `plans`: array, required
    - `receipt`: object, required
- **`plan_diff`**: Plan a single-file unified diff with relocatable exact context and source-bound edit ranges
  - MCP: `tools/call` with name `plan_diff`
  - CLI: `purrassist call plan_diff --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `patch`: string, required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `plans`: array, required
    - `receipt`: object, required
- **`inspect_code`**: Inspect grammar acceptance, source-bound declaration ranges, JSX rename repair options, and parse allocator accounting without executing code
  - MCP: `tools/call` with name `inspect_code`
  - CLI: `purrassist call inspect_code --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `language`: one of "python", "rust", "javascript", "typescript", "tsx", required
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
- **`read_file`**: Read a bounded root-relative UTF-8 file and its exact digest
  - MCP: `tools/call` with name `read_file`
  - CLI: `purrassist call read_file --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `path`: string, required
  - Output:
    - `path`: string, required
    - `source`: string, required
    - `digest`: string, required
- **`apply_edit`**: Apply one caller-selected source-bound edit plan to a buffer, optionally validate code syntax, optionally rebase a stale plan onto an intervening change, and return text, exact inverse, and a receipt; no file writes
  - MCP: `tools/call` with name `apply_edit`
  - CLI: `purrassist call apply_edit --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `source`: string, required
    - `plan`: object, required
    - `rebase_from`: object
    - `language`: one of "text", "python", "rust", "javascript", "typescript", "tsx"
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
  - Example: [`examples/apply-edit.json`](../examples/apply-edit.json)
- **`apply_file`**: Conditionally apply one selected edit plan or a replacement buffer, optionally rebase a stale plan onto an intervening change, validate supported code grammar, journal it, and return undo identity
  - MCP: `tools/call` with name `apply_file`
  - CLI: `purrassist call apply_file --root DIR < args.json`
  - Writes files under the root: yes
  - Input:
    - `path`: string, required
    - `expected_digest`: string, required
    - `replacement`: string
    - `plan`: object
    - `rebase_from`: object
    - `effort`: one of "exact", "quick", "standard", "deep", "maximum"
  - Output:
    - `commit`: any, required
    - `receipt`: object, required
- **`undo_file`**: Undo a journaled file replacement only if its exact output is still present
  - MCP: `tools/call` with name `undo_file`
  - CLI: `purrassist call undo_file --root DIR < args.json`
  - Writes files under the root: yes
  - Input:
    - `transaction`: string, required
  - Output:
    - `transaction`: string, required
    - `path`: string, required
    - `before`: string, required
    - `after`: string, required
    - `durability`: object, required
- **`recover_files`**: Reconcile prepared journals without overwriting targets; remove only bounded compatible temporary bytes beside a validated journal's recorded target, preserving uncertain artifacts with reasons
  - MCP: `tools/call` with name `recover_files`
  - CLI: `purrassist call recover_files --root DIR < args.json`
  - Writes files under the root: yes
  - Input:
    - none
  - Output: an array of objects with:
    - `kind`: one of "finalized", "not_applied", "conflict", "corrupt", "orphan_removed", "orphan_unknown", "orphan_preserved", required
    - `id`: string, required
    - `reason`: string or null, required
- **`commit_files`**: Independently journal and commit each of several file replacements, never as one atomic multi-file transaction
  - MCP: `tools/call` with name `commit_files`
  - CLI: `purrassist call commit_files --root DIR < args.json`
  - Writes files under the root: yes
  - Input:
    - `files`: array of object, required
  - Output: an array of objects with:
    - `index`: integer, required
    - `path`: string, required
    - `status`: one of "committed", "error", required
    - `commit`: any, required
    - `error`: string or null, required
- **`prepare_helper`**: Prepare a non-streaming completion request for a caller-selected provider/model/schema/input, performing no network I/O; the caller's own transport sends the returned request
  - MCP: `tools/call` with name `prepare_helper`
  - CLI: `purrassist call prepare_helper --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `provider`: one of "openai_responses", "anthropic_messages", "chat_completions", required
    - `model`: string, required
    - `input`: string, required
    - `schema`: object or boolean, required
    - `max_output_tokens`: integer, required
  - Output:
    - `provider`: string, required
    - `path`: string, required
    - `headers`: array of object, required
    - `body`: string, required
    - `pending`: string, required
    - `receipt`: object, required
- **`complete_helper`**: Resume a prepared helper request with its caller-observed HTTP status and body, returning a recovered outcome with a receipt; refusals and non-completed responses remain unsuccessful
  - MCP: `tools/call` with name `complete_helper`
  - CLI: `purrassist call complete_helper --root DIR < args.json`
  - Writes files under the root: no
  - Input:
    - `pending`: string, required
    - `status`: integer, required
    - `body`: string, required
  - Output:
    - `status`: one of "resolved", "best_guess", "ambiguous", "unresolved", required
    - `value`: any, required
    - `alternatives`: array, required
    - `receipt`: object, required
<!-- tools-reference:end -->

## Common behavior

Registry admission and output validation each use explicit default budgets,
separately from handler engines. Admission observes cancellation. After a handler
returns, bounded output validation completes without late cancellation so a
completed transaction is not reported as cancelled. Successful payload bytes
and existing engine receipts are unchanged; engine receipts do not include
registry validation costs.

If a returned result violates its schema or output validation exhausts a native
resource, Rust receives an error string containing a JSON diagnostic with code
`registered_output_refused`. CLI `call` exits nonzero, writes the diagnostic to
stderr and emits no successful stdout result. MCP emits text with `isError: true`
and no successful `structuredContent`. The diagnostic retains `original_output`,
selected tool and schema identity, reason or full validation report, and separate
`admission` and `output_validation` boundary receipts with their budgets and usage.
It explicitly records that the handler returned and effects may already have
occurred. Inspect the retained transaction/undo identity before deciding what
to do; this failure performs no retry or rollback. Original duplicate members
and numeric spelling survive in the diagnostic. Error materialization follows
the existing host policy, outside these schema-phase budgets.

The Markdown operations take exact `source` and optional `effort`, retain
admitted source, and return receipts. Edit plans in `value` bind to the source
digest and do not write files. Block indices come from inspection of the same
source version. Markdown editing operations retain the original source and
support exact undo.

Each example request under `examples/` is run through both the CLI and MCP, in
fresh roots, by the test `examples_run_through_cli_and_mcp`, which also
requires the results to match and to validate against the tool's output
schema. Run one with, for example:

```sh
purrassist call match_strings --root . < examples/match-strings.json
```

## Tool notes

Each tool has one heading, in registry order. The entries above are generated;
these notes are hand-written.

### `match_embeddings`

`match_embeddings` takes a caller-declared `space`, a numeric `query` vector,
and `candidates` containing unique `id`/`vector` pairs. No model is loaded.
`minimum_score` defaults to `0.75`; `minimum_margin` defaults to `0.05`.
A unique qualifying cosine winner is a best guess, ties/near ties remain
ambiguous, and weak matches remain unresolved. Scores are not probabilities.
Vectors must be finite, nonzero, and dimensionally consistent. JSON decimals are
converted to `binary64`; overflow and nonzero underflow are rejected. Admitted
requests retain their structured JSON input in receipts, with separate ledgers
for ambiguous alternatives. Admission failures can return a tool error before
retention. `embeddings::EmbeddingIndex` provides reusable normalized vectors to
Rust callers. See `examples/match-embeddings.json` for an illustrative input.

### `render_markdown`

`render_markdown` takes `source`, optional `dialect` (`commonmark` by default or
`gfm`), optional `extensions`, and `effort`. Omitting extensions uses the dialect's
default profile; an explicit array replaces it, including `[]` to disable all.
Names are `autolinks`, `strikethrough`, `tables`, `task_lists`, and `tag_filter`.
Results include HTML, source/output digests, renderer/specification identities,
effective extensions, and a receipt retaining the original source. Tag filtering
is not a general HTML or URL sanitizer. For example:

```sh
purrassist call render_markdown --root . < examples/render-markdown.json
```

### `inspect_markdown`

`inspect_markdown` returns nested blocks, original line maps, root sections, nested `section_sites`, and
reference-definition occurrences with original field spans.

### `markdown_context`

`markdown_context` takes UTF-8 byte offsets `start`/`end` and optional
neighboring-block count (`neighbors`).

### `plan_markdown_list_item`

`plan_markdown_list_item` selects an item by `path` and accepts `before`, `after`,
`replace`, or `remove`. Insertions require an explicit `marker` (`-`, `+`, `*`,
`1.`, or `1)` through nine-digit ordinals) and logical `body`; replacement uses
`body` and retains the selected marker. Existing items are never renumbered.
New lines receive container prefixes and continuation indentation; required
separators follow the anchor's newline style and EOF convention. Removing the
only item can leave an empty container line. Every plan reparses the intended
item and checks outside siblings and ancestors. See `examples/markdown-list-item.json`.

### `plan_markdown_section`

`plan_markdown_section` selects either a root `heading` index or a nested `path`
from `section_sites`. Root bodies are inserted verbatim. Nested bodies omit outer
container markers; new lines receive quote prefixes and list-continuation spaces.
Reparsing preserves the heading, section boundary, ancestors, and outside blocks.
Supply line endings and separators explicitly. Bodies that capture following
sections, introduce peer headings, or change structure under container prefixes
are rejected. A trailing blank body line and a leading tab are layout-recovered
(reindented to the container level, or to the number of spaces the tab would
occupy from column zero) rather than rejected. Identical logical bodies preserve
original prefix spelling and yield empty plans. See `examples/markdown-section.json`.

### `plan_markdown_reference`

`plan_markdown_reference` selects a definition by its inspection `path` and
`occurrence`, including shadowed duplicate labels. Supply decoded `destination`
and an explicit `title` (string or `null` to remove it). The plan encodes these
values safely for Markdown syntax, preserves label spelling and surrounding
source, and reparses every definition to check that unselected values stay exact.
Multiline destination/title regions can collapse to one physical line; newline
and tab values use entities. NUL values are rejected because Markdown cannot
retain them. Equal decoded fields produce an empty plan. See
`examples/markdown-reference.json`.

### `plan_markdown_table_column`

`plan_markdown_table_column` takes a table `path`, zero-based column `index`, and
`action` (`insert`, `remove`, or `align`). Insertion requires `header`, `alignment`
(`default`, `left`, `right`, or `center`), and one `cells` value per body row;
alignment changes require only `alignment`. Removing the final column is rejected.
Insert/remove preserves existing cell spelling, makes implicit padding explicit,
keeps ignored extra cells beyond the new header width, and adds outer pipes when
absent. Alignment changes preserve delimiter margins. Every proposal is reparsed
and supports exact conditional undo. See `examples/markdown-table-column.json`.

### `plan_markdown_table_row`

`plan_markdown_table_row` inserts or removes body rows. Supply a table `path`,
`action` (`insert` or `remove`), and zero-based body `index`; insertion also requires
`cells` with exactly the header width. Inserting at the row count appends.
New rows use canonical `| cell |` spacing, the delimiter line's container prefix
and newline style, and preserve EOF's final-newline convention. Existing bytes
outside the edit remain exact. The header and delimiter are not body rows. Cell
and row plans include structural validation in their receipts.

### `plan_markdown_table_cell`

`plan_markdown_table_cell` selects a table by `path`, with zero-based `row` and
`column` (row zero is the header). Its `markdown` replaces one explicitly present
cell; surrounding whitespace, pipes, container prefixes, and line endings remain
exact. Leading/trailing replacement whitespace is trimmed. Escape literal pipes
as `\|`; line breaks and proposals that change table structure are rejected after
reparsing. Columns beyond the header width are not editable through this
operation, but editing an implicit padded cell of a body row shorter than the
header materializes every implicit cell up to it instead of refusing.
For an executable nested-table example:

```sh
purrassist call plan_markdown_table_cell --root . < examples/markdown-table-cell.json
```

### `plan_markdown_task`

`plan_markdown_task` sets the supplied `checked` state using either a root
`block` index or a nested `path` from inspection (for example `[0,0,1]`).
Exactly one selector is required; paths follow successive child indices.

### `plan_markdown_structure`

The Rust `markdown::Document` API's structural operations — `insert_section`,
`remove_section`, `move_section` (with optional re-leveling of the moved
section's own heading), `rename_heading`, `ListItemEdit::Move`,
`renumber_list`, `TableRowEdit::Move`, `TableColumnEdit::Move`, and
`add_reference`/`relabel_reference`/`remove_reference`/`unlink_reference`
(which rewrite every shortcut/collapsed/full-reference use resolving to the
affected definition) — are exposed through the host tool
`plan_markdown_structure`, selected by a typed `op` (`section_insert`,
`section_remove`, `section_move`, `section_rename`, `list_move`,
`list_renumber`, `table_row_move`, `table_column_move`, `reference_add`,
`reference_relabel`, `reference_remove`, `reference_unlink`). Each is governed,
digest-bound, reparse-checked, and provides an exact inverse.

### `decode_json`

`decode_json` takes `input` (the model's text) and a `schema`, and optional
`effort` and `allow_guesses`. It searches composed corrections, validates the
complete result against the whole schema, and returns the outcome (`resolved`,
`best_guess`, `ambiguous`, or `unresolved`) with a receipt of what changed. The
tool compiles the schema and recovers under one shared governor, so the total
preparation and recovery budget is bounded together. The example pair
`examples/temperature.schema.json` and `examples/temperature.input.txt` is the
schema and the model reply that the README decodes with `purrassist decode`; the test
`examples_run_through_cli_and_mcp` decodes them through both the CLI and MCP.

The `purrassist decode` command is a shortcut with its own output: it recovers
`--input` (or stdin) against `--schema` and prints the answer and receipt as
JSON, with `--effort` and `--no-guesses` options and the exit codes 0 for
resolved or best guess, 3 for ambiguous and 4 for unresolved. It does not print
the `decode_json` tool's output and makes no claim to.

### `recover_tool_calls`

`recover-tool-calls` reads one model turn as raw bytes from a file or stdin
and recovers its tool calls against the declarations in `--tools`
(`--tools-format openai`, `mcp`, or `anthropic`). It prints the same result as
the `recover_tool_calls` MCP tool: per-call outcomes, itemized repairs and
losses, rejected attempts, the tool-choice check, the turn's finish and its
`OpenAI` `finish_reason`, an optional `--emit` re-emission (`openai` for the
`tool_calls` array, or a dialect id), and a receipt. It never executes a call.
The `finish_reason` is `"length"` whenever the caller's stop reason is a length stop (whatever the
finish: a length stop is never reported as a clean finish, and the usable
calls are still emitted), otherwise `"tool_calls"` exactly when the `OpenAI`
emission holds a call, and `"stop"` otherwise;
neither says whether anything else went wrong, so read `finish`, `verdict`
and `rejected` as well. The command exits 0 when at least one call is usable (`usable_accepting_guesses`) and the
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
code; the code only summarizes it. `--tool-choice NAME` forces a tool; with
`--tool-choice-exactly-one`, `--tool-choice-no-content` and
`--tool-choice-no-reasoning` (a usage error without `--tool-choice NAME`) the
turn must hold exactly one call, no content other than ASCII whitespace, and
no reasoning block, or the tool-choice check fails like any violated choice.
A rejected call attempt of any reason counts as a call beyond the one and as
content, and `--tool-choice-no-reasoning` with `--reasoning content` (where
reasoning is never recognized) is invalid input.
The [integration guide](tool-calls/INTEGRATION.md)
lists every option, the integrator guarantees, and the evidence for each. Call
markup of a dialect that is not built in yet, or not in the request's
allow-list, is reported as a rejected attempt (`unsupported_dialect`), never
silent content. See [tool-call recovery](tool-calls/README.md).

### `validate_json`

`validate_json` exposes whole-instance validation itself: given `schema` and
`instance`, it returns `valid`, every `violations` entry, always-evaluated
format/pattern evidence, and any unsupported-vocabulary evidence, alongside a
receipt. The shipped CLI's `purrassist validate --schema FILE [--input FILE]`
command emits exactly this tool's output (and exits 0 when the instance is
valid and 4 when it is not); the test `validate_command_prints_validate_json_output`
compares the two.

### `generate_json_witness`

`generate_json_witness` exposes the same witness-generation algebra as
`Schema::generate_witness`: given `schema` and optional `resources`, it returns
`outcome` (`witness`, `unsatisfiable`, or `exhausted`), the generated `witness`
value or non-empty `refutation` evidence, and a receipt; a governor hard stop
while generating is reported as `exhausted` with `budget_exceeded` in the
receipt's termination, never as `unsatisfiable`.

### `match_strings`

`match_strings` accepts plain candidate strings or objects with stable `id`,
`text`, and caller-declared `aliases`. Optional `effort`, `english_phonetics`,
and `limit` control ranking. Its results retain each identity, score, aliases,
and comparison signals; scores are ranking units, not proof of semantic
equivalence. See [the example](../examples/match-strings.json).

The `purrassist match QUERY CANDIDATE...` command is a shortcut with its own
output: it ranks the candidates (identified by their position) and prints
`matches` and a receipt, exiting 4 when nothing matches. It does not print the
`match_strings` tool's output and makes no claim to.

### `resolve_string`

`resolve_string` applies caller-adjustable `minimum_score` and `minimum_margin`
thresholds to all candidates, even when ranking would normally return only its
first ten. A unique byte-exact spelling is `resolved`; a unique accepted
approximation is `best_guess`; close alternatives remain `ambiguous` with
separate digest-bound inference ledgers; weak matches and exhaustion are
`unresolved`. Normalization equivalence is an explicit lattice — NFC,
case-fold, width/compatibility-fold, diacritic-strip, UTS-39 confusable
skeleton, and depunctuation, each a named ranking signal — resolving a query
at the finest level where it has a unique acceptable candidate. Protected
token classes (decimal digit runs, negation markers, semver components, ISO
dates, hex digests, and units) must match for automatic selection unless a
caller-declared alias matches the query exactly; a discarded protected-class
mismatch appears in the receipt. Aliases count as guesses when selected.
Thresholds are policy on ranking units, not calibrated confidence.
The [curated quality set](MATCHING-QUALITY.md) records correction and
abstention outcomes across the full lattice and every protected class.

### `match_paths`

`match_paths` takes a `query`, caller-supplied `paths`,
optional directory `context`, effort, and score/margin thresholds. It returns
`resolved`, `best_guess`, `ambiguous`, or `unresolved` with methods and a receipt.
Ambiguous paths carry separate evidence and inference ledgers bound to each
serialized alternative.
It matches names only; choosing a path does not grant file access.

### `resolve_intent`

`resolve_intent` takes `input`, an explicit `domain` (identity, phrase rules, ignored words), and
`concepts` (identities, labels, attribute maps). It applies the declared rules,
retains the request, and returns concept interpretations with evidence. Unknown
words are errors; effort bounds work without changing domain meanings.
Ambiguous semantic answers carry separate ordered receipt ledgers with output
digests, supporting rules, derived values, and validation references. Duplicate
outputs retain the first supporting rule derivation; rejected branches do not
contribute evidence to accepted answers.

### `plan_replacement`

`plan_replacement` plans an exact or bounded fuzzy replacement of `needle` by
`replacement` in `source`; `max_edits` is the largest Levenshtein edit distance
a fuzzy match may have, and `effort` selects its search budget preset. Approximate selections remain guesses, ties
remain ambiguous, and nothing is written.

### `plan_diff`

`plan_diff` plans a single-file unified diff (`patch`) against `source` with
relocatable exact context; the `effort` argument selects its search budget
preset. Nothing is written. See [plan application](EDIT-APPLICATION.md).

### `inspect_code`

`inspect_code` accepts an `effort` argument selecting its traversal budget
preset. It reports each JSX/TSX rename issue's repair options and the governed
parse's allocator accounting (retained/peak/overshoot bytes). Grammar
acceptance does not prove program correctness.

### `read_file`

`read_file` reads one file by root-relative `path` and returns its `path` as
given, its `source`, and the `digest` of its exact bytes (`sha256:` and 64 hex
digits). The digest is what `apply_file` and `commit_files` take as
`expected_digest`, so a write is rejected if the file changed after it was
read. The store opened for the root admits files of at most 16 MiB; a path must
be relative, must not contain `.` or `..` components, empty components, a
backslash, a colon or a NUL, must not name `.purrassist`, and must resolve to a
regular file through no symlink. A file that is not valid UTF-8 is rejected.
Reading takes no writer lock and writes nothing.

### `apply_edit`

`apply_edit` takes one selected plan and the original `source` and returns the
edited text and a source-bound inverse, with no file write. A stale plan is
reconciled against an intervening change through an explicit `rebase_from`
input. See [plan application](EDIT-APPLICATION.md) and `examples/apply-edit.json`.

### `apply_file`

`apply_file` also accepts a `plan` instead of a full replacement buffer, with
its expected digest and journaled undo checks. File tools reject stale versions
and record transaction identities for undo. The portable filesystem layer
coordinates cooperating writers; it is not a sandbox against malicious
directory replacement or an atomic multi-file transaction. See
[plan application](EDIT-APPLICATION.md).

### `undo_file`

`undo_file` takes the `transaction` identity returned in the `commit` of
`apply_file` (or of a `commit_files` entry) and restores the file only if it
still holds exactly the committed output. It inverts the patch the journal
recorded and applies the inverse to the file's current text; if the file has
been modified since, the digest check fails and the call returns a stale error
without touching the file. The undo is itself journaled as a new conditional
transaction, so its result has the shape of any commit: `transaction`, `path`,
`before`, `after`, and `durability`. Tampered or incomplete journals are
rejected.

### `recover_files`

`recover_files` reconciles prepared journals against actual file versions
without overwriting anything, reporting each transaction's exact outcome and
removing evidence-bearing orphan temporaries.

### `commit_files`

`commit_files` independently journals and commits several file replacements in
one call — never as one atomic multi-file transaction; each entry reports its
own commit or error.

### `prepare_helper`

`prepare_helper` and `complete_helper` expose optional external-model helper
proposals (see [helpers](HELPERS.md)) as a two-call tool pair: the first
returns the exact request to send and an opaque, tamper-evident `pending`
string with no network I/O performed here.

### `complete_helper`

`complete_helper` resumes with the caller-observed HTTP status/body once the
caller's own transport has sent the request prepared by `prepare_helper`.


### `project_result`

`project_result` is a read-only data-processing operation. The same handler is
available through `purrassist_host::tools::project_result`, CLI `call`, and MCP
`tools/call`. For example:

```sh
purrassist call project_result --root . <<'JSON'
{"result_json":" {\"orders\":[1.00e2,true]} ","output_schema":{"type":"object"},"format":"purrassist-toolgraph-v2","context":{"outputProjectionJq":".orders[]?","maxOutputBytes":12,"outputEnforcement":"exact-bytes","historyRetentionTurns":0,"intentTags":[],"loadingStrategy":"always"}}
JSON
```

The reply's `artifact.text` is `1.00e2` followed by LF and `true` followed by LF:
12 UTF-8 bytes, with media type `application/x-ndjson`. The measurement counts
this decoded artifact text. JSON escaping, receipt and requested-control fields,
the CLI envelope LF and MCP wrappers are outside its ceiling. The protocol
reply can therefore exceed `maxOutputBytes` while its artifact meets that bound.

`original_json` retains the exact request string, including whitespace, duplicate
members and numeric spelling. `requested.context_json` retains all context
occurrences; `requested.output_schema_json` and `requested.effort` retain the
requested schema and optional effort. `requested.masks` lists every projection value without
choosing a duplicate winner. `stage` identifies the last preparation or pipeline
stage attempted. Validation is null until available. Success has status
`resolved`; a processing refusal has status `unresolved`, a stable typed `error`,
artifact null and a receipt preserving the native resource stop where applicable.
A syntactically valid refusal reply follows the existing CLI `call` exit policy.
Invocation validation and cancellation before handler entry retain the existing
transport-error policy.

The complete original is schema-validated before masking. This operation neither
recovers invalid JSON nor executes or authorizes a tool. History retention,
loading and intent values remain metadata; graph, confinement and actual release
policies remain separate requirements.
