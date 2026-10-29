<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-host

Explicit host boundaries for `PurRAssist`: root-scoped UTF-8 files, journaled
replacement, conditional undo, shared tools, stdio MCP, and resumable helper
request/response adapters.

**Status: 0.1.0.** This crate is the filesystem and helper integration behind
two of the three equal interfaces of `PurRAssist`: the CLI and the MCP server
(`purrassist-cli`) both serve its tool registry. Rust users reach the engines
through the `purrassist` crate.

File transactions coordinate cooperating writers. Each replacement is atomic
for one file; batches report independent outcomes. The portable path checks are
not a sandbox against malicious concurrent directory replacement. Durable host
writes are implemented on Unix and Windows, with explicit filesystem-dependent
flush evidence. Crash/retry/undo tests exercise the transaction checkpoints;
the Windows CI lane executes the host operations on Windows.

Helper transports, endpoints, authentication, and timeouts belong to the caller.
No model is required. Returned helper proposals undergo local validation and
remain explicit guesses. No live provider qualification is claimed.

## Registered result contracts

The shared `Tools::call` and `call_with_cancellation` path prepares the selected
entry's exact input and output schemas before invoking its handler once.
Complete returned nodes, including array roots, must satisfy the registered
output schema before successful Rust, CLI or MCP release. Validation applies
to the declared envelope; unconstrained operation values remain unconstrained.

Admission observes cancellation. After a handler returns, a separate bounded
output-validation phase completes without late cancellation, preserving actual
completed file transactions. Successful bytes and engine receipts are unchanged;
registry costs are separate from handler receipt accounting.

Post-handler schema or native resource failures use the existing String error
transport, containing a JSON diagnostic with `registered_output_refused`, the
original complete output, tool/schema identity, reason/report, separate boundary
receipts and explicit effect context. An invalid result is not released as a
successful CLI/MCP result. A mutation may already have completed: its retained
commit/undo identity remains available, and refusal performs no retry or rollback.
Diagnostic materialization follows existing host policy outside schema budgets.
This adds no graph authorization, confinement or proof-bound release claim.

## Tool-call recovery

The `recover_tool_calls` tool (MCP `tools/call`, CLI `purrassist call` and
`purrassist recover-tool-calls`) recovers the tool calls of one model turn
against caller declarations, over fourteen built-in dialects: `qwen3-coder-xml.v1`, `hermes.v1`, `openai-tool-calls.v1`, `llama3-json.v1`,
`mistral-v3.v1`, `mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`,
`kimi-k2.v1`, `anthropic-tool-use.v1`, `harmony.v1`, `glm-4.5.v1`, `bare-json.v1`, and `pythonic.v1`.
Its result is the recovery's wire form: per-call outcomes, itemized repairs,
rejected and unparsed attempts, the turn's finish, an optional re-emission, and
a receipt. It never executes a call.

```rust
use purrassist::json::{self, Node};
let options = json::parse(r#"{"tools":[{"type":"function","function":{"name":"ls","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}]}"#).unwrap();
let turn = br#"<tool_call>{"name": "ls", "arguments": {"path": "src"}}</tool_call>"#;
let result = purrassist_host::tools::recover_tool_calls(turn, &options).unwrap();
let node = result.to_node();
assert_eq!(node.pointer("/value/finish").and_then(Node::as_str), Some("calls"));
assert_eq!(node.pointer("/value/calls/0/dialect").and_then(Node::as_str), Some("hermes.v1"));
```

Version 0.1.0. Capabilities, known limitations and verification are recorded in
the [qualification record](https://github.com/Blackcat-Informatics/purrassist/blob/rust-v0.1.0/docs/QUALIFICATION.md).
First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0.
