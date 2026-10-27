# Issue #44: Tool graph foundation: apply admitted metadata without losing exact MCP schemas

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github
Labels: enhancement

## Body

## Objective

Deliver the core adapter prerequisite of #33: import an exact MCP declaration while applying caller-supplied effects, repair policy and parameter aliases before normal tool-set compilation. The tool graph's admitted metadata must reach the existing recovery engine without rebuilding its narrower typed schema subset.

This is a complete public adapter capability split from the larger loader/codec delivery to keep implementation, qualification and landing within four hours. #33 retains every original loader, codec and integration requirement.

## Contract

- Add `DeclarationMetadata` and `ParameterAliases`, and `ToolSet::from_mcp_with_metadata_governed`, using the signatures in `docs/tool-graph/TOOLS_PLAN.md` as the starting contract.
- Apply metadata at the existing pending-definition assembly point. Preserve exact original declaration bytes, schema text, compiled schema, unmodeled keywords and validation semantics.
- Keep existing imports conservative with empty metadata. MCP annotations never become trusted effects authority automatically; metadata is an explicit typed caller input.
- Resolve canonical object-member paths, including members of array-item objects. Refuse unknown tools/paths, duplicate records, invalid aliases, collisions and unmodeled hint paths. Use existing typed `ToolSetError` failures and preserve resource exhaustion/cancellation.
- Govern metadata traversal and retained aliases through the existing governor. Core retains its empty dependency closure and deterministic behavior.

## Acceptance

- A real consumer imports a schema with a constraint outside the typed subset, attaches an alias and recovers a valid call through that alias. The original schema still refuses its invalid neighbor, and same-form render-back is byte-exact.
- Nested and array-item aliases use their canonical paths; ambiguous/colliding/unknown metadata fails with tool/path context. Empty metadata preserves existing behavior.
- Tiny budgets and pre-tripped cancellation return explicit resource outcomes. Existing adapter and recovery cases remain valid.
- Meaningful Rustdoc documents the public API and errors. Focused adapter checks and mandatory `make check` pass with normal hooks and signing.

Closing this issue does not close #33; the latter must wire the capability through the complete graph loader and MCP codec.


## Comments (0)

