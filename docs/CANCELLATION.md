<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Caller-owned cancellation

`CancellationToken` is a monotonic signal shared by explicitly cloned handles.
Attach it with `Governor::with_cancellation`. Another owner can cancel without
borrowing the governor, including while a worker owns it. Engines check the
signal at governor checks and resource charges. Ordinary `Governor::new` creates
no token allocation and consults no external cancellation state. The core starts
no threads and accesses no clock, process, filesystem, or network state.

```rust
use purrassist::{Budget, CancellationToken, Governor, Outcome, Schema, RecoveryRequest};

let token = CancellationToken::new();
let mut governor = Governor::with_cancellation(Budget::default(), token.clone());
let schema = Schema::compile_governed(r#"{"type":"integer"}"#, &mut governor)?;
token.cancel(); // This handle may instead belong to the caller's execution loop.
let result = schema.recover_governed("[[42]]", RecoveryRequest::default(), &mut governor);
assert!(matches!(result.outcome, Outcome::Unresolved));
assert_eq!(result.receipt.stopped, purrassist::StopReason::Cancelled);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Cancellation does not reset usage, erase completed evidence, or roll back
effects. An exhaustion already recorded by a governor keeps its original reason
when cancellation arrives later. Local `Governor::cancel` affects that governor;
token cancellation affects every attached governor. Token cancellation cannot
be cleared; retries use a new token. Cloned governors retain their individual
accounting snapshots but share an attached token.

JSON compilation/recovery, Markdown, paths, embeddings, and intent resolution
already accept caller-owned governors. The same access is available for
`matching::rank_governed`, `edits::locate_governed`,
`edits::locate_fuzzy_governed`, `edits::plan_unified_governed`,
`EditPlan::apply_governed`, and `syntax::apply_checked_governed`.
Convenience methods delegate to these engines. Governed receipts describe the
supplied limits and cumulative usage, including work performed before the call.

`Tools::call_with_cancellation` carries one signal through tool argument schema
compilation/validation and the selected engine. Argument validation and operation
execution retain their separate budgets. Cancellation during validation returns
an error; engines that return assistance receipts record cancellation there.
Cancellation remains cooperative: it does not forcibly interrupt an individual
allocation, serialization, hashing call, or blocking filesystem operation.

File operations check before entry. `apply_file` checks again after in-memory
edit/syntax validation and before the store's journaled commit. Once commit or
undo begins, it runs through its existing durability/recovery protocol and
returns its actual outcome. Cancelling after that boundary is not proof that the
file was untouched and does not request an automatic undo. A caller can inspect
the transaction receipt and use the existing conditional undo operation.

Tests cover cross-owner cancellation of a live governor after successful work,
sticky exhaustion precedence, independent accounting, cancelled planning and
matching, source-preserving file refusal before commit, and retry with a fresh
token. The native/WASI transcript also executes token cancellation; browser-WASM
compilation checks the shared-token implementation on that target.

## MCP transport

The shipped `purrassist mcp` CLI keeps reading frames while up to four admitted
workers execute tools. `notifications/cancelled` identifies an active call by its
string or mathematical integer ID; strings and numbers remain distinct. Invalid
notifications, unknown IDs, already-completed calls, and initialize cancellation
are ignored. Duplicate active IDs receive a JSON-RPC invalid-request error.
A fifth active tool request receives error `-32000` and may be retried after a
completion. Cancellation does not free a slot until that worker completes.

A tool that observes cancellation emits no response. A tool that already
completed, including a journaled file transaction, retains its actual response
if cancellation arrives before the controller consumes its completion. Clients
must tolerate that race. MCP task-augmented requests are rejected: the internal
worker API does not advertise or implement the separate MCP tasks protocol.
These semantics follow the pinned
[MCP 2025-11-25 cancellation contract](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/cancellation).

Embedding callers use `Session::dispatch` to receive either an immediate response
or opaque `Work`. They execute work in their own executor, continue dispatching
control frames, then pass its opaque `Completion` to `Session::complete`. Tickets
are session-bound; a foreign completion is rejected. Dropping a session signals
all its outstanding calls. Admitted work must be run and completed, including
cancelled work, to reclaim its slot. The convenience `handle` and generic `serve`
methods remain synchronous and do not read additional frames during a tool call.

Only the CLI starts threads: one process-owned stdin reader and at most four tool
workers. Its event channel holds at most four queued frames/completions, and each
input frame is capped at 16 MiB. Those are admission bounds, not a measured total
allocator bound. Responses can arrive out of request order; stdout writes remain
serialized. Normal stdin EOF drains admitted calls. Input/output failure requests
cancellation, drops the event receiver to unblock result senders, and joins every
tool worker before the CLI exits. A stdin reader still blocked after output
failure ends with the owning process; no detached reader exists in the library
session API. File I/O and output backpressure remain cooperative boundaries, so
there is no guaranteed maximum cancellation latency.

Deterministic tests exercise the event loop while a real tool worker waits for a
control response, numeric ID equivalence, malformed and late notifications,
admission saturation, session-bound completions, frame fragmentation, and early
versus late file cancellation. A real subprocess test closes stdout while keeping
stdin open and requires the CLI to exit. Five additional live stdio sessions
cancelled a JSON/schema operation, answered a following ping without a cancelled
tool response, and drained cleanly at EOF. Those observations are not latency or
stress benchmarks.
