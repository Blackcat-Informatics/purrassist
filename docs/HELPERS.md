<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Optional helper proposals

`purrassist_host::helpers::Pending` separates preparing a request from consuming
a response. The caller supplies an exact model identity, token limit, and a
positive helper-call budget. Defaults permit zero helper calls.

Adapters implement the public [OpenAI Responses format](https://developers.openai.com/api/docs/guides/migrate-to-responses),
[Anthropic Messages format](https://platform.claude.com/docs/en/api/messages/create),
and [chat-completions format supported by llama.cpp](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md).
OpenAI requests disable response storage. Requests contain the schema and input
explicitly supplied by the caller; no files, environment credentials, model
catalogs, endpoints, or host context are discovered automatically.

The application owns authentication, endpoint selection, TLS, transport timeout,
response-size admission, and sending the request. The adapter itself performs
no I/O and starts no server. It does not bundle an HTTP/TLS stack or provider SDK.
Its request `Debug` output omits the prompt body.

Complete the pending request with an HTTP status and non-streaming JSON response.
The adapter extracts visible answer text, refuses error/refusal/unfinished
responses, and passes the proposal through the first-party recovery engine and
complete local schema. It never relies on a provider's restricted JSON Schema
subset as the final validator. Any accepted helper proposal is a `BestGuess` or
an explicit ambiguity, with provider/model and request/response digests recorded.

Fixed transcripts exercise all three wire formats, status errors, refusal,
truncation, helper admission, validation, and guess labeling. No live provider
qualification or model-backed evaluation has been performed.
