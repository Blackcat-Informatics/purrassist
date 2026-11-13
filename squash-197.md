Merge PR 197: MCP Extended manifest codec for purrassist-toolgraph (issue 196, second piece of #33).

Review: independent review PASS at head 6a6664d (.stage/tool-graph-mcp-extended-codec-encode/reviews/codec-6a6664d-review.md); make check exit 0 at 6a6664d (local).
Head 1af9b46 adds only the REUSE .license sidecar for tests/data/worked_manifest.json (no code change); REUSE lint passes locally.
Hosted checks at merge: corpus, native ubuntu, qualification, release-gate, reuse, wasm, windows-engines, fuzz all green; native macOS fails as in issue 180 (operator runner), not a gate for this change.
