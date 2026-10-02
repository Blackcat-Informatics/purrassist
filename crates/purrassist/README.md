<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `PurRAssist`

Assistance for AIs that get the details almost right. This facade exposes the
pure recovery engines and isolated syntax integration.

```rust
use purrassist::{Outcome, RecoveryRequest, Schema};
let schema = Schema::compile(r#"{"type":"object","required":["temperature"],"properties":{"temperature":{"type":"number"}},"additionalProperties":false}"#).unwrap();
let answer = schema.recover("[[{temperatur: '42',}]]", RecoveryRequest::default());
let Outcome::Resolved(value) = answer.outcome else { panic!("unexpected outcome") };
assert_eq!(value.to_json(), r#"{"temperature":42}"#);
```

## Tool-call recovery

`tool_call` recovers the tool calls of one raw model turn against the `OpenAI`,
MCP, or Anthropic declarations the model was shown (read byte for byte), with
per-call outcomes, itemized repairs, the turn's finish, and a receipt; it never
executes a call. Twelve dialects ship: `qwen3-coder-xml.v1`, `hermes.v1`, `openai-tool-calls.v1`, `llama3-json.v1`,
`mistral-v3.v1`, `mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`,
`kimi-k2.v1`, `anthropic-tool-use.v1`, `harmony.v1`, and `bare-json.v1`.

```rust
use purrassist::Outcome;
use purrassist::tool_call::{ToolRecoveryRequest, ToolSet};
let declarations = r#"[{"type":"function","function":{"name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}]"#;
let tools = ToolSet::from_openai(declarations, purrassist::Budget::default()).unwrap();
let turn = br#"[TOOL_CALLS] [{"name": "get_weather", "arguments": {"city": "Oslo"}}]"#;
let result = tools.recover(turn, &ToolRecoveryRequest::default());
let Outcome::Resolved(calls) = result.outcome else { panic!("unexpected outcome") };
assert_eq!(calls.calls[0].tool.as_deref(), Some("get_weather"));
assert_eq!(calls.calls[0].dialect.id(), "mistral-v3.v1");
```

Inspect every result's status and receipt. Validation does not turn an invented
value into a fact. Ambiguity and resource exhaustion are explicit outcomes.

Pre-release implementation with executable conformance, recovery, resource,
and portability qualification. See the
[repository](https://github.com/Blackcat-Informatics/purrassist) for capabilities,
limitations, verification, and provenance. First-party code is licensed under
MIT OR Apache-2.0 OR MulanPSL-2.0.
