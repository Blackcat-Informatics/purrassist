Add built-in `gemma3-tool-code.v1` recovery for line-start `tool_code` fences containing one direct keyword call per block. Reuse the native Pythonic codec for exact values and source spans, preserve ordinary fenced examples as content, retain `tool_output` as content, and repair a missing closing fence through the existing costed rule.

Register the grammar across recognition, canonical printing, default selection, conformance, evaluation, fuzz seeds and portable transcripts. Document the pinned Gemma cookbook convention and its evidence limits. Core gains no runtime dependency. Expose every core built-in in the host dialect and emission selectors, correcting the missing Gemma, GLM and Pythonic entries.

Validation:

- Independent source, golden, corpus and complete CLI/MCP fixture reviews passed.
- Golden suite: 123 passed. Correction corpus: 18,218 cases, zero incorrect results, required-abstention recall 5,435/5,435.
- All 18 runtime cost tests and four scale tests passed; dense recovery handles 29,127 calls under the ordinary sized Standard budget. The fifteen-dialect bound is `30292·n + 8·E`.
- Executed native/WASI core, syntax and host transcripts and browser-WASM core transcripts match. Exact CLI/MCP capture and replay passed: discovery advertises all fifteen dialects, and operation receipts have only the reviewed resource-counter changes. Requests, seeds and semantic values are preserved.
- All 26 host tool-call cases passed, including default/explicit Gemma selection, admission of every built-in selector, and rejection of unknown neighbors.
- All 423 fixed tool-call fuzz seeds passed; this is a zero-mutation seed run, not the full mutation/reference campaign. Mandatory `make check` passed both Clippy stages, all 141 logged Rust result groups (1,913 passing cases), Rustdoc and documentation generation.

Closes #14.
