# Independent re-review: issue 196 (MCP Extended codec), head 6a6664d

Reviewer: independent read-only review (delivery-reviewer, Opus), against base bc2a03d.

VERDICT: PASS at head 6a6664d.

Previous blocking finding (stale README and docs/IMPLEMENTATION.md calling the manifest codec planned): FIXED.
- crates/toolgraph/README.md lines 33-35 and 126-127 match the code.
- docs/IMPLEMENTATION.md lines 951-957 list #191 and #196 as delivered; tool_set, governed parsing, wasm wiring and host integration as planned.

Optional changes verified:
- decode compares with Node::equivalent_governed under the caller's governor; a stop maps to CodecError::Resource.
- encode's rustdoc names CodecError::Schema wrapping ToolSchemaError::InvalidRoot, matching manifest.rs schema_for.

Accepted decisions: the embedded _meta.security.sourceGraph is the documented contract (TOOLS.md, envelope reader); the Vocabulary, Governor and Result signatures are justified.

Non-blocking note: docs/IMPLEMENTATION.md ~959-960 still reads "graph loading and original-SHACL integration remain #33"; reword optional.
