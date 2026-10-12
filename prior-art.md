# Prior art — issue #1: Complete PurRAssist 0.1.0 plan and release qualification

Assembled by `stagectl brief`. Every hash and path below came from this
repository; nothing here is recalled or inferred. Read the Coverage
section at the bottom before concluding anything is absent — an empty
section can mean 'nothing found' or 'could not look'.

## Governing ADRs

The issue cites no ADRs.

## Linked issues (2-hop `#N` crawl)

- #1: Complete PurRAssist 0.1.0 plan and release qualification  [OPEN]  (closes)
- #2: feat(1): complete 0.1.0 scope and release qualification  [MERGED]  (mentions)
- #3: Tool-call recovery: typed tool definitions, multi-dialect parsing, schema-guided repair, streaming  [CLOSED]  (mentions)
- #8: Tool-call recovery: arrays of nested-parameter objects, strict forced cardinality, marker-free reasoning range  [CLOSED]  (mentions)
- #15: purrassist-derive: zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)]  [CLOSED]  (mentions)
- #6: Tool-call fuzz: finish the rejected-span oracle failures left at the #3 merge  [OPEN]  (mentions)
- #7: Audit the unreviewed #3 stage-3 commits and close the F14/F19 rule's known limits  [OPEN]  (mentions)
- #10: Tool-call recovery: restarted DeepSeek-V3.1 sections cost quadratic work and super-quadratic time  [OPEN]  (mentions)
- #11: Tool-call recovery: repair search under-charges work on mutated free-form-item turns (seconds for 192 bytes)  [OPEN]  (mentions)
- #12: Tool-call recovery: add the pythonic.v1 dialect and pythonic codec  [OPEN]  (mentions)
- #13: Tool-call recovery: add the glm-4.5.v1 dialect and kv-pairs codec  [OPEN]  (mentions)
- #14: Tool-call recovery: add the gemma3-tool-code.v1 dialect  [OPEN]  (mentions)
- #16: Tool-call recovery: streaming recognizer (ToolCallStream, stable-prefix events, CLI --stream, MCP progress)  [OPEN]  (mentions)
- #17: Tool-call recovery: reassemble provider streaming events (Anthropic SSE tool_use, OpenAI tool_calls deltas)  [OPEN]  (mentions)
- #18: Tool-call recovery: special-token-stripped dialect variants (*-stripped.v1)  [OPEN]  (mentions)
- #5: feat(3): tool-call recovery — typed definitions, grammar-driven parsing (12 dialects), schema-guided repair  [MERGED]  (2-hop)
- #4: Tool-call recovery, part 2: streaming recognizer, remaining dialects, purrassist-derive  [CLOSED]  (2-hop)
- #9: Tool-call recovery: arrays of nested-parameter objects, strict forced choice, marker-free reasoning blocks  [MERGED]  (2-hop)

## Prior attempts at this issue

Commits citing it (including inside squash merges):

- `1a8509cbf fix: isolate release archive verification from shared Cargo caches`
- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `61d6986de fix: release scratch meter handles when retaining charges`
- `ba2a35cc3 feat: expose witness generation as a tool`
- `2b43d223a fix: restore rustdoc and syntax-oracle gates`
- `23caff46d feat: derive every tool surface from one operation registry`
- `a45e2c8d0 feat: generate JSON Schema witnesses with an algebraic pipeline`
- `868d19613 fix: locate packaged archives wherever cargo writes them and keep lockfiles`
- `788270396 build: add trusted-publishing release workflow and published-package verification`
- `24787e1a3 test: compare helper transcripts and browser WASM with native output`
- `4f055c5c8 test: qualify edit preservation with generated adversarial documents`
- `7690c4a66 feat: unify edits as a patch algebra with structural Markdown operations`
- `7e8f7f5a9 test: count oracle allocations per thread`
- `3ed24fc2f feat: evaluate every JSON Schema format and report unsupported vocabularies`
- `de8e8fda0 docs: shorten the ScratchString summary line for stable clippy`
- `b1034e488 fix: flush retried journal pieces through writable handles`
- `4dfd1b432 feat: match ECMA-262 patterns with a symbolic derivative engine`
- `9994d4d61 fix: open Windows replacement handles with delete access`
- `58bcb73f9 fix: release the writer lock synchronously on every platform`
- `0b7c7de05 fix: make Windows durability code lint-clean and fail explicitly`
- `b53ce9fbd feat: qualify filesystem durability on Windows`
- `65ef9081e feat: bound tree-sitter allocations through the governor`
- `8e360f3c1 fix: make scratch charges independent of pointer width`
- `974ccac6e feat: publish receipts from a provenance derivation forest`
- `48d4e942c feat: enforce scratch accounting through budgeted containers`
- `2e286923d feat: resolve strings through a refinement lattice with protected tokens`

Other branches named for it:

- None.

PRs citing it:

- #20: docs(release): release-ready 0.1.0 documentation and three-interface presentation  [OPEN]  (this stage's own PR)
- #2: feat(1): complete 0.1.0 scope and release qualification  [MERGED]

## Related items on the forge

- #15: purrassist-derive: zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)]  [CLOSED]
- #8: Tool-call recovery: arrays of nested-parameter objects, strict forced cardinality, marker-free reasoning range  [CLOSED]

## Recurrence

Counted from `GhpRsq-Defect-Class` trailers — these are tallies, not estimates:

- `none`: 1 prior instance(s)
- `performance`: 1 prior instance(s)

## Coverage

What was searched, and what was not:

- issue #1 + 69 comment(s) via github
- link crawl: 18 issue(s) within 2 hop(s) of `#N` references
- forge search on ['Complete', 'PurRAssist', '0.1.0']: 2 other item(s)
- GhpRsq-Defect-Class trailers: 2 distinct class(es) in the last 200 commits
- prior attempts: 26 commit(s) citing #1 (second parents walked), 0 other branch(es) named for it
- PRs citing #1: 2
- requirement candidates: 65 line(s); 36 comment(s) matched no rule
- referenced artifacts: 150 path(s) (68 on disk), 150 identifier(s), 8 workspace package(s), 16 URL(s); TRUNCATED (paths, identifiers): only the first entries of those kinds were checked
