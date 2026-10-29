<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-core

Pure Rust intent recovery with zero third-party Cargo dependencies. Includes
schema-guided JSON repair, exact decimal arithmetic, Unicode matching, semantic
domain rules, source-bound edits, and evidence receipts.

**Status: 0.1.0.** This crate is one of the shared engines of `PurRAssist`, one
engine with three equal interfaces: the Rust crates (`purrassist`), the MCP
server and the CLI (`purrassist-cli`) all run it.

```rust
use purrassist_core::{Outcome, schema::{Schema, recovery::RecoveryRequest}};
let schema = Schema::compile(r#"{"type":"integer"}"#).unwrap();
let result = schema.recover("[[42]]", RecoveryRequest::default());
assert!(matches!(result.outcome, Outcome::Resolved(_)));
assert_eq!(result.receipt.sources[0].text(), Some("[[42]]"));
```

## Tool-call recovery

`tool_call` recovers the tool calls of one raw model turn against typed tool
definitions or lossless `OpenAI`, MCP, and Anthropic declarations: each call
gets its tool, schema-validated arguments, an outcome (`Resolved`,
`BestGuess`, `Ambiguous`, `Unresolved`), and its itemized repairs; markup no
allowed dialect reads is reported, never dropped. Thirteen dialects ship as
weighted, invertible grammars: `qwen3-coder-xml.v1`, `hermes.v1`, `openai-tool-calls.v1`, `llama3-json.v1`,
`mistral-v3.v1`, `mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`,
`kimi-k2.v1`, `anthropic-tool-use.v1`, `harmony.v1`, `glm-4.5.v1`, and `bare-json.v1`.

```rust
use purrassist_core::tool_call::{CallOutcome, Effects, Param, ToolDefinition, ToolRecoveryRequest, ToolSet};
use purrassist_core::{Budget, Outcome};
let tools = ToolSet::compile(
    [ToolDefinition::new("read_file").effects(Effects::ReadOnly).param("path", Param::string().required())],
    Budget::default(),
).unwrap();
let turn = br#"Reading it. <tool_call>{"name": "read_file", "arguments": {"path": "a.rs"}}</tool_call>"#;
let result = tools.recover(turn, &ToolRecoveryRequest::default(), purrassist_core::tool_call::DialectSet::built_in());
let Outcome::Resolved(calls) = result.outcome else { panic!("unexpected outcome") };
assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
assert_eq!(calls.calls[0].arguments.to_json(), r#"{"path":"a.rs"}"#);
```

Recovery may resolve, guess, preserve ambiguity, or exhaust its explicit budget.
Guesses remain labeled after validation. The core never reads files, contacts
models, or consults ambient clocks or randomness.

Version 0.1.0. Markdown parsing and rendering are tested against
all 652 `CommonMark` and 672 GFM specification examples, with source-preserving
structural edits and explicit resource outcomes. See the
[implementation contract](https://github.com/Blackcat-Informatics/purrassist/blob/rust-v0.1.0/docs/IMPLEMENTATION.md).

First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0. Unicode and HTML entity tables,
schema meta-schemas, and test corpora retain the upstream notices included with them.


The `output` module provides the complete governed field-mask/context codec and
validated JSON Lines result pipeline. Validate the full original against its
schema before projecting; exact byte ceilings apply to the complete artifact.
Original input, transformations and typed refusal stages remain available.
