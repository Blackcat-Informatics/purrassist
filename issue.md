# Issue #198: Tool graph: ToolGraph::tool_set bridge to the governed MCP adapter (third piece of #33)

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

## Objective

Third bounded delivery of #33 (parent): the bridge from a loaded `ToolGraph` to the core recovery engine, `ToolGraph::tool_set(budget) -> ToolSet`, on top of the loader (#191, merged d808673) and the codec (#196).

## Scope

- `ToolGraph::tool_set(budget)` uses the existing core MCP adapter with the metadata augmentation specified in PR #42: `ToolSet::from_mcp_with_metadata_governed` (purrassist-core) and its `DeclarationForm`. Add a narrow adapter API only if the existing one cannot carry the augmentation; keep the exact schema and original declaration.
- Each reified alias becomes a `ParameterSpec::alias`.
- `readOnlyHint` and the security model select `Effects`; `ReadOnly` only when the graph proves no write capability.
- The policy follows from the tolerances.
- Recovery stays on the existing engine; no lossy typed schema subset is rebuilt.
- Metadata collision is refused.

## Excluded (stays on #33)

Governed source parsing (purrdf 4.0.2 `from_dataset_with_control` and `parse_dataset_controlled` are the next piece), the wasm harness wiring, host integration.

## Acceptance

- Alias recovery, exact unmodeled-schema validation, byte-exact declaration render-back and metadata-collision refusal are exercised through the augmented existing adapter, on the worked example.
- `cargo test -p purrassist-toolgraph --locked` for the new target; clippy with `-D warnings`; the `wasm32-unknown-unknown` build of the crate; `make check` on the head.


## Comments (0)

