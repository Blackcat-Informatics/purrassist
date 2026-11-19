# Issue #236: Tool-call recovery: OpenAI chat-completions streaming reassembly (second unit of #17, stacked on the Anthropic unit)

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

Second unit of #17: the OpenAI chat-completions delta reading of provider streaming event logs, reassembled through the same ToolCallStream as the Anthropic reading (PR for issue 17, Anthropic only, is in review). This unit is STACKED on the issue 17 branch and must merge after it.

Scope: tool_calls[i] deltas with id, function.name and function.arguments fragments spread across chunks, reassembled into the same per-call outcomes, repairs, losses and receipts as the batch reading; identical for every chunking; malformed, truncated and out-of-order events are typed faults, never silent drops; arguments through the existing JSON repair, schema typing and strict-choice checks; zero third-party dependencies in core; cost charged to the governor.

Not in scope: the CLI, MCP and batch input surfaces; provider fixtures with provenance; fuzz seeds; WASM parity.


## Comments (0)

