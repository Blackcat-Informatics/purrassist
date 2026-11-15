# Issue #17: Tool-call recovery: reassemble provider streaming events (Anthropic SSE tool_use, OpenAI tool_calls deltas)

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

## Objective

Recover tool calls from **provider streaming event logs**, not only from the model's raw text:
- Anthropic Messages SSE: `content_block_start` with a `tool_use` block, followed by `input_json_delta` fragments, then `content_block_stop`.
- OpenAI chat-completions streaming: `tool_calls[i]` deltas, with `id`, `function.name` and `function.arguments` fragments spread across chunks.

Each event log is reassembled into calls with the same per-call outcomes, repairs, losses and receipts as the batch reading of the equivalent complete message. The result is identical however the event stream is chunked.

## Maintainer decision (from #4)

> **Update (2026-10-02, stage 2 of #3, Spec B addition):** the maintainer directed that provider streaming event logs belong to Spec B. The question was "Anthropic streaming SSE events (`content_block_start` / `input_json_delta`) … Parsing them into calls is streaming work … Where should that work go?", and the answer was "Add to #4 streaming". Spec B therefore also covers reassembling Anthropic Messages SSE events (`content_block_start` with a `tool_use` block, followed by `input_json_delta` fragments) and, for symmetry, OpenAI streaming `tool_calls` deltas into calls, with chunking-invariance against batch. PR #5 reports these events as rejected attempts (never silent content) until then.

Today (PR #5 onward), these event logs are reported as rejected attempts and are never silently treated as content.

## Scope

- **Input surfaces.** An event-log input for the batch API, the CLI and MCP, and for the streaming API. Malformed, truncated and out-of-order events are explicit outcomes, never silent drops.
- **Chunking invariance.** The result is identical across every split of the event log, the same contract as streaming text.
- **Reuse.** Arguments go through the existing JSON repair, schema typing and strict-choice checks.
- **Evidence.** Fixtures from real provider logs with provenance notes, goldens, negatives, fuzz seeds, and WASM parity.

**Depends on #16** (streaming recognizer core).

## Standing constraints (from #3)

Zero third-party dependencies in core (and in derive), network-free core, deterministic linear cost charged to the governor, identical native and WASM output, original input retained, and a guess never reported as resolved.

Split out of #4. Base: `main` at `fccfbe7`. #4 is now a tracking issue.


## Comments (1)

### canblmz1 — 2026-10-14T10:26:23Z

For Spec B, a useful negative pair is a parseable, schema-valid tool argument object followed by token-limit termination, versus the same bytes followed by confirmed successful termination. A repaired value should not make those execution outcomes equivalent. A further case is a delta arriving after its own content-block stop: retaining the anomaly matters even if later bytes happen to close the JSON.

I maintain [prefix-safe-json](https://github.com/canblmz1/prefix-safe-json), whose [portable conformance corpus](https://github.com/canblmz1/prefix-safe-json/tree/main/conformance) addresses this boundary. Would you be interested in comparing a few fixtures with your own event-log implementation as development tests? Your zero-dependency Rust/WASM constraint makes a runtime JS dependency inappropriate; the corpus can be used independently and its cases are normalized events, not a drop-in raw SSE parser test.

If the comparison is useful, a fixture-level result or mismatch would help improve the corpus. This is an optional execution-authority comparison alongside the recovery contract, not a claim that Purrassist's current repair behavior is incorrect.


