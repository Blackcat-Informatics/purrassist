<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->

# Recorded provider event logs

- `anthropic-parallel-weather.sse` contains the UTF-8 bytes of the first
  interaction's `response.body.content` string in the Pydantic AI
  cassette. The cassette records a live Anthropic Messages request with
  two parallel weather tool calls. The fixture contains the response body
  only. Its SHA-256 is
  `8c0641597662061a7516ebc2b3d574514b21856d70411761d4fb504d4b92455a`.
  The original MIT license is in `LICENSE-Pydantic-AI`.
  [Cassette](https://github.com/pydantic/pydantic-ai/blob/c432cba022a22b50ae1078ee516b1fc8583d1019/tests/cassettes/test_capability_pending_messages/test_parallel_tool_enqueues_reach_anthropic.yaml)
  · [Recording commit](https://github.com/pydantic/pydantic-ai/commit/c432cba022a22b50ae1078ee516b1fc8583d1019)
- `openai-weather.sse` contains the OpenAI Chat Completions response
  snapshot used by `test_parse_pydantic_tool`. The SDK snapshots the
  response body directly when recording live requests. The bytes match
  the source snapshot, with SHA-256
  `c6aa7e397b7123c3501f25df3a05d4daf7e8ad6d61ffa406ab5361fe36a8d5b1`.
  The original Apache-2.0 license is in `LICENSE-OpenAI-Python`.
  [Response snapshot](https://github.com/openai/openai-python/blob/9feadd8274809fff9ff1e36a0c90d45566ed46e2/.inline-snapshot/external/c6aa7e397b7123c3501f25df3a05d4daf7e8ad6d61ffa406ab5361fe36a8d5b1.bin)
  · [Recording commit](https://github.com/openai/openai-python/commit/9feadd8274809fff9ff1e36a0c90d45566ed46e2)
