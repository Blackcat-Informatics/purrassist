Adds ToolGraph::tool_set, the bridge from a loaded tool graph to the core governed MCP adapter: aliases become parameter aliases, effects come from a static read of the execution-profile tree and subtool closure (not security admission; implementation binding not inspected), and policy is Tolerant only when a hint permits inference. Unknown-argument handling comes from the exact schema.

STACKED: this branch is based on paudley/196-tool-graph-mcp-extended-codec-encode (PR 197). Merge PR 197 first; this PR merges after it.

Review: independent review BLOCKED then fixed; the final text correction is at 84c15d1; make check exit 0 at 84c15d1 (local).

Refs #33, closes #198
