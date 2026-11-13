# Issue #16: Tool-call recovery: streaming recognizer (ToolCallStream, stable-prefix events, CLI --stream, MCP progress)

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

## Objective

Add **streaming tool-call recognition**. `ToolSet::stream(..)` returns a `ToolCallStream`. It accepts output bytes as they arrive and emits events, never emitting anything a later byte could contradict. It finishes with exactly the `AssistResult<ToolCalls>` that batch `recover` returns for the same bytes, including `usage`, however the input is chunked.

## Design decisions

**D6 Streaming = longest stable prefix.** 
- After prefix `p`, an event about bytes `[0,k)` may be emitted only if its interpretation is
  identical for every completion of `p` under the compiled grammar. That is, the
  configuration set at `k` has a unique interpretation (the commitment frontier).
- "No leaked markup", "never retracted", "`CallName` only once unique" and the hold-back
  bound are theorems of this rule.
- `feed(chunk)` is defined as a fold of a byte step. Any skip-ahead fast path must be proven
  equivalent by the invariance tests.
- Governor charges are functions of (bytes, configurations, events) only. They never depend
  on feed count, caller buffers or chunk boundaries.
- Events are charged per event. The recognizer's logical state is charged by logical length.
- Batch `recover` and the stream both run the same `Recognizer`, so `usage` is
  chunk-invariant (A4 compares the whole `AssistResult`).

**D7 Hold-back and candidate bytes.** 
- `hold_back_bound()` is computed from the compiled lexical layer. It is the maximum
  viable-prefix length of any opener, including reasoning openers and closers and Harmony
  channel tokens, over the request's `DialectSet`.
- Invariant: outside candidate regions, `held_bytes() <= hold_back_bound()`.
- Inside a candidate region, bytes are `candidate_bytes()`. They are charged, and released
  as content only on rejection.
- A bare-JSON candidate decides at its discriminating key. A bare-JSON or Pythonic dialect
  whose discriminating key can arrive late (`{"arguments": <1 MiB>, "name": …}`) is
  documented as having an unbounded `candidate_bytes()`, because the key's position is
  model-controlled. Both values are exposed.

**D19 Constrained-decoding hook (the issue's "trigger point").** 
- `CallStarted.offset` marks the trigger point.
- `ToolCallStream::viable(&[u8]) -> bool` and `allowed_next_bytes() -> ByteSet` are
  derivatives of the compiled recognizer.
- The stream state is `Clone`.
- A token-mask engine is not built, because the issue declares constrained decoding itself
  out of its scope (see Declined).

## Specification (#4 Spec B, streaming core)

It builds on the
`Recognizer::push`/`finish` engine shipped in PR #5. Its scope:
- D6 (commitment frontier) and D7 (hold-back and candidate bytes).
- D19: `ByteSet`, `viable` and `allowed_next_bytes`.
- `StreamEvent`:
  `ContentDelta`, `ReasoningDelta`, `CallStarted{index, offset, dialect_candidates}`,
  `CallName{index, tool}`, `ArgumentDelta{index, raw}`,
  `ParameterComplete{index, name, value}`, `CallComplete{index, outcome}`,
  `CallRejected{index, span, reason}`, `Revision{index, parameter, rule, before, after}`.
- `delta` productions and incremental emitters (R6.10), including OpenAI streaming
  `tool_calls[i].function.arguments` deltas, with the test `incremental_concat_equals_batch`.
- CLI `--stream` (R7.4): stdin read in 8 KiB reads, one NDJSON line per event, a final
  `{"type":"result",…}` line, and a flush after each line. Test:
  `stream_ndjson_final_equals_batch` with 1-byte writes.
- MCP progress (R7.5):
  - `CallContext { cancellation, progress: Option<ProgressSink> }` in `Handler`;
  - `Event::Notify(Node)` in `mcp_stdio.rs`;
  - `params._meta.progressToken` validated;
  - unchanged output without a token.
  - Tests: `progress_notifications_when_token_present`, `no_notifications_without_token`,
    `final_result_identical_to_batch`.
- The `tool-call-stream` fuzz target, where one input byte is a split seed and stream must
  equal batch, including `usage`. The WASM transcript gains a streaming section.

**API (`tool_call/stream.rs`).**
- `ToolSet::stream(ToolRecoveryRequest) -> ToolCallStream` and `stream_governed(.., Governor)`.
- `feed(&[u8], &mut impl Extend<StreamEvent>)`, plus a convenience `feed_collect(&[u8]) -> Vec<StreamEvent>`.
  The `Vec` is charged per event.
- `finish(GenerationStop) -> AssistResult<ToolCalls>`.
- `held_bytes`, `candidate_bytes`, `hold_back_bound`, `viable`, `allowed_next_bytes`,
  `cancel`, and `Clone`.

**Behavior.**
- `ParameterComplete` carries the settled typed value. It is never issued for `Mutating`
  tools before `CallComplete`.
- Revisions are recorded as evidence.
- An identity change is emitted as `CallRejected` followed by a new `CallStarted`.
- A UTF-8 sequence split across feeds holds at most 3 bytes.

**Checks:** `cargo test -p purrassist-core --test tool_call_streaming` passes these tests:
- `every_single_split_equals_batch` (A4): the whole `AssistResult`, including `usage`, over
  the anchors, goldens and negatives
- `seeded_multisplit_equals_batch` (A4)
- `split_openers` (A5): `<tool_`|`call>`, `[TOOL_`|`CALLS]`, `<|python_`|`tag|>`, DeepSeek
  tokens split mid-UTF-8, and a 4-byte char split at every offset
- `no_markup_in_content_delta` (A6)
- `events_consistent_with_final_result`: at every split, each emitted `ContentDelta`,
  `CallName` and `ParameterComplete` agrees with the final `ToolCalls`
- `events_consistent_with_prefix_length_batch`
- `hold_back_bound_holds` (A7, after every feed)
- `candidate_bytes_reported`
- `content_never_retracted`
- `revision_events_for_inferred_closer` (A8)
- `length_mid_call_follows_section4`
- `mutating_never_usable_before_complete`
- `cancel_between_feeds`
- `exhaustion_ends_stream_explicitly` (receipt retains the source digest)
- `per_byte_work_linear`: a 1 MiB argument has the same `work` as batch, and the work ratios
  stay within 1.05× of the byte ratio across 64 KiB, 256 KiB and 1 MiB
- `viable_and_allowed_next_bytes`

## Batch parity with features added since #4 was written

Streaming runs the same `Recognizer` as batch, so stream-equals-batch must also hold for what #8 (PR #9) added:
- the scope graph for nested-parameter array items (`$ref`, `anyOf`, opaque scopes);
- strict forced tool choice;
- `reasoning_blocks` (marker-free text ranges);
- the raised per-byte sized budget (`RECOVER_*_PER_BYTE`).

The A4 invariance tests run over the goldens and anchors that cover these.

Reassembling **provider streaming event logs** (Anthropic SSE, OpenAI deltas as *input*) is #17, which builds on this one.

## Standing constraints (from #3)

Zero third-party dependencies in core (and in derive), network-free core, deterministic linear cost charged to the governor, identical native and WASM output, original input retained, and a guess never reported as resolved.

Split out of #4. Base: `main` at `fccfbe7`. #4 is now a tracking issue.


## Comments (0)

