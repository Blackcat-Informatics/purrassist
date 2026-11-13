Merge PR 199: ToolGraph::tool_set bridge to the governed MCP adapter (issue 198, third piece of #33).

Review: delta review PASS at merge head b6a50cb (.stage/tool-graph-toolgraph-tool-set-bridge-to/reviews/toolset-b6a50cb-delta-review.md), covering the commit 84c15d1 correction and the merge of main.
make check exit 0 at 84c15d1 (local).
Hosted checks at merge: corpus, native ubuntu, qualification, release-gate, reuse, wasm, windows-engines, fuzz all green; native macOS fails as in issue 180 (operator runner), not a gate for this change.
Stacked on PR 197 (merged as 8fc97e9); the merge of main brings it in.
