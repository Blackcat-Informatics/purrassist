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

The current stdio MCP loop is synchronous and does not yet service cancellation
notifications while a tool runs. The token/host APIs provide the execution path
for that remaining transport work; they do not establish MCP cancellation support
or guarantee a cancellation latency bound.
