# Squash notes DRAFT (Stage 2). Not final; Stage 3 seeds and finalizes `${WORKTREE}.squash_notes`.

Subject: feat(8): tool-call recovery: nested-parameter array items, strict forced choice, marker-free reasoning blocks (#9)

## Behaviour
- **Qwen3-Coder XML arrays of objects** written as repeated nested `<parameter=>` elements resolve as arrays, including inside objects and when mixed with JSON items.
  - `$ref` and `anyOf` schemas compile into a scope graph, so recursive trees nest and colliding member names stay in their items.
  - Opaque free-form scopes close by the guess rule `tool_call.markup.unmodeled-scope-boundary`.
- **`ToolChoice::ForcedStrict`:** exactly-one (rejected attempts count), no-content (non-ASCII-whitespace bytes), no-reasoning.
  - A failure makes the turn `nothing_usable` with usable counts 0.
  - It is exposed through MCP `tool_choice_strict`, the CLI flags, and Anthropic `disable_parallel_tool_use`.
- **`ToolCalls.reasoning_blocks`:** per-block span plus marker-free text; never a reversed range.
- **Root `$ref`/`allOf`/`anyOf` declarations** project parameters (FastMCP/pydantic shape).
- **Linear, charged cost:**
  - repeated parameters;
  - Mistral v11 chained calls and Mistral v3 call arrays;
  - provenance derivation and forest indexes;
  - the receipt validation log.
- **Published per-byte budget:** work 176, scratch 368, evidence ⌈n/7⌉.
- **Tooling:** Node pinned to v26.11.1; new fuzz target `tool-call-ref`.

## Decisions (verbatim user decisions)
- Node repin.
- "Fix them in this PR" (repeated-parameter quadratic, root $ref).
- "Raise the published constants".
- "Fix (1) and (2) here, document (3)".
- DeepSeek: issue #10, accepted.
- Slow repair search: issue #11, accepted.

## Evidence (to be refreshed at Stage 3)
- `make check` exit 0 and `make wasm-test` exit 0 on 42899ab.
- Corpus: 0 incorrect over 18,176 cases.
- Fuzz: both targets pass with `--continue-on-known`; the 6 #6 inputs reproduce on base.
- Exit completion audit: PASS.
- **PENDING:** hosted CI, Stage 3 merge-tree and preflight.

## Defect-Class
TODO (Stage 3: a real repo defect class or `none`)
