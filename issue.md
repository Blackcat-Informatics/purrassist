# Issue #196: Tool graph: MCP Extended codec encode/decode with LossLedger (second piece of #33)

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

## Objective

Second bounded delivery of #33 (parent): the bidirectional MCP Extended Tool Manifest codec for `purrassist-toolgraph`, on top of the typed `ToolGraph` loaded by #191 (merged as d808673).

## Scope

- `codec::mcp::encode(&ToolGraph) -> (ManifestJson, LossLedger)` emitting `name`, `description`, `inputSchema`, `outputSchema`, `annotations`, `_meta.security`, `_meta.executionProfile`, `_meta.contextManagement`.
- `codec::mcp::decode(&ManifestJson, &Vocabulary) -> (ToolGraph, LossLedger)`, the reverse.
- Round-trip law for the worked example: `decode(encode(g))` equals `g` except for exactly the ledgered losses. An unledgered loss is a bug.
- Output is byte-deterministic (fixed key order; nothing time- or hash-seed-dependent).
- Input and output schemas come from `purrdf_shapes::json_schema::compile_tool_arguments` (the plain-argument profile used by `examples/plain_tool_schemas.rs`), keeping `CompiledSchema.losses` in the ledger. No local JSON-LD key rewriting and no second compiler.
- Output byte limit and exact-bytes enforcement keep the #47 contract: `maxOutputBytes`, `outputEnforcement: "exact-bytes"`, outer format `purrassist-toolgraph-v2`; legacy `maxOutputTokens` is rejected.

## Excluded (stays on #33)

`ToolGraph::tool_set`, the core adapter augmentation, governed source parsing (purrdf 4.0.2 `from_dataset_with_control` and `parse_dataset_controlled` are the next step, not this issue), and wasm-harness wiring.

## Acceptance

- The MCP Extended JSON golden for the worked example is byte-identical across two encodes.
- The round-trip law holds for the worked example.
- `cargo test -p purrassist-toolgraph --locked` for the new codec test targets; `cargo clippy -p purrassist-toolgraph --all-targets --locked -- -D warnings`; `cargo build -p purrassist-toolgraph --locked --target wasm32-unknown-unknown`; `make check` on the head.


## Comments (0)

