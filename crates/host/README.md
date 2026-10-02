<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-host

Explicit host boundaries for `PurRAssist`: root-scoped UTF-8 files, journaled
replacement, conditional undo, shared tools, stdio MCP, and resumable helper
request/response adapters.

File transactions coordinate cooperating writers. Each replacement is atomic
for one file; batches report independent outcomes. The portable path checks are
not a sandbox against malicious concurrent directory replacement. Durable host
writes are implemented on Unix and Windows, with explicit filesystem-dependent
flush evidence. Crash/retry/undo tests exercise the transaction checkpoints;
the Windows CI lane executes the host operations on Windows.

Helper transports, endpoints, authentication, and timeouts belong to the caller.
No model is required. Returned helper proposals undergo local validation and
remain explicit guesses. No live provider qualification is claimed.

## Tool-call recovery

The `recover_tool_calls` tool (MCP `tools/call`, CLI `purrassist call` and
`purrassist recover-tool-calls`) recovers the tool calls of one model turn
against caller declarations, over twelve built-in dialects: `qwen3-coder-xml.v1`, `hermes.v1`, `openai-tool-calls.v1`, `llama3-json.v1`,
`mistral-v3.v1`, `mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`,
`kimi-k2.v1`, `anthropic-tool-use.v1`, `harmony.v1`, and `bare-json.v1`.
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

Pre-release implementation. See the
[repository](https://github.com/Blackcat-Informatics/purrassist) for the complete
contract. First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0.
