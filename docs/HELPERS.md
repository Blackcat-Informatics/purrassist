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

## `prepare_helper`/`complete_helper` tools

`Pending` is also reachable as a two-call CLI/MCP tool pair, so a caller never
needs its own Rust process to use it. `prepare_helper` takes `provider`,
`model`, `input`, `schema`, and `max_output_tokens`, and returns the exact
`path`/`headers`/`body` the caller's own transport must send, plus an opaque
`pending` string and a receipt; it performs no network I/O itself. The caller
sends that request, then calls `complete_helper` with the same `pending`
string alongside the transport's observed `status` and response `body`; it
returns the same `AssistResult` shape every other governed tool does (a
`status`/`value`/`alternatives`/`receipt`), with any successful proposal
reported as `best_guess`.

The `pending` string is a versioned, tamper-evident envelope: it carries
`provider`/`model`/`original` (the input)/`schema`/`max_output_tokens` under a
`payload` key, and a `digest` binding that payload's exact serialized bytes.
`complete_helper` recomputes that digest before trusting anything inside and
refuses a `pending` string whose digest does not match or whose `version` it
does not recognize. This detects corruption or substitution of the opaque
string in transit or storage; it is a content hash, not an authenticated MAC,
so it does not defend against a caller who controls the string outright — that
threat has no defense here, consistent with `purrassist-core`'s prohibition on
ambient credentials or secrets. `complete_helper` reconstructs the identical
`Pending` value `prepare_helper` minted (preparation is a deterministic
function of those five fields) rather than persisting anything server-side
between the two calls; each call charges exactly one caller-owned governor and
is itself the explicit budgeting of one `HelperCalls` unit (every other tool
leaves the default budget's `helper_calls: 0` in place).
