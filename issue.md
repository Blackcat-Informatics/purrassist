# Issue #200: Tool graph: load through purrdf's governed parse and FnO catalog entry points (fourth piece of #33)

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

## Objective

Fourth bounded delivery of #33 (parent): load tool graphs through purrdf's governed entry points instead of the ungoverned ones, so that the caller's work, storage and cancellation limits bound source parsing and FnO catalog construction, and so that diagnostics can name the exact source occurrence.

## Scope

- `ToolGraph::load` parses through `purrdf_rdf::parse_dataset_controlled` (exact ordered source occurrences retained) instead of `parse_dataset`.
- The FnO catalog is built through `FnoCatalog::from_dataset_with_control` (purrdf-core 4.0.2) instead of `FnoCatalog::from_dataset`.
- The caller's `Governor` supplies the admission and progress authority that purrdf's `Admission` and `Progress` traits require. Map a purrdf refusal or stop to the existing typed `LoadError` (Exhausted or Cancelled). Nothing is swallowed.
- Parse errors and loader diagnostics that refer to a statement name the exact source occurrence where purrdf provides it.
- Update the rustdoc and docs/tool-graph/TOOLS.md: the governed path replaces the stated "ungoverned entry points" limit; any remaining limit is named.

## Excluded

tool_set (PR 199, issue 198), the MCP codec (PR 197, issue 196), wasm harness wiring, host integration.

## Acceptance

- Existing tests in crates/toolgraph/tests/graph_load.rs still pass unchanged in their expectations, except where a diagnostic now carries an occurrence.
- A test that a work or storage limit smaller than the worked example refuses with the typed exhaustion error and does not publish a partial graph.
- A test that cancellation is honoured by the governed parse.
- `cargo test -p purrassist-toolgraph --locked --test graph_load`, clippy with -D warnings, the `wasm32-unknown-unknown` build of the crate, and make check on the head.


## Comments (0)

