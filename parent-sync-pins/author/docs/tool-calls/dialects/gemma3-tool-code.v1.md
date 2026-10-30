<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `gemma3-tool-code.v1`

Grammar: [`gemma3-tool-code.v1.grammar`](../../../crates/core/src/tool_call/dialects/gemma3-tool-code.v1.grammar),
built in as `DialectId::Gemma3ToolCode`. It reuses the native `pythonic`
codec; no Python interpreter or model is executed.

## Format and provenance

Google's Gemma cookbook records this prompted function-calling convention in
[its Gemma 3 notebook at commit 5c52c7250feff52e0a7cc526bf32edd646bc3d6b](https://github.com/google-gemma/cookbook/blob/5c52c7250feff52e0a7cc526bf32edd646bc3d6b/.archive/Gemma/%5BGemma_3%5DFunction_Calling_with_HF.ipynb).
The notebook selects `google/gemma-3-4b-it`. Its captured output contains a
direct `search(query="…")` invocation in a `tool_code` fence, and its prompt
and result adapter specify a singular `tool_output` fence containing returned
text. This is evidence of the textual prompted convention and recorded example;
it does not establish universal model behavior. Only textual evidence was
inspected; no model was downloaded or run.

````text
```tool_code
get_weather(city="Paris")
```
```tool_output
The temperature is 18 °C.
```
````

## Calls, results and examples

One direct keyword call occupies each `tool_code` block. Multiple blocks retain
call order. The opening fence begins a line, allowing preceding indentation.
The function may use a dotted identifier such as `mcp.search`. A newline
separates the opening fence, invocation and closing fence; LF and CRLF are
accepted. A closing fence occupies its own line, with only optional spaces or
tabs before LF, CRLF or end of input. A following `tool_code` or `tool_output`
header never doubles as that closer. Canonical emission uses LF. A complete invocation whose closing
fence is missing can use the existing costed closer repair; incomplete values
are never synthesized.

The [Pythonic codec](pythonic.v1.md) supplies string quotes and escapes, exact
numbers, booleans, nulls, lists and dictionaries. Original spans are retained.
Fence text inside a quoted argument stays inside that value. Schema typing,
guess restrictions and caller resource limits apply as for other dialects.
Positional arguments receive `pythonic_positional_rejected`. Executable Python,
`print(...)` wrappers, comprehensions, imports and arbitrary statements are not
accepted as direct tool invocations.

The grammar opts into `protocol-fences` context handling. An ordinary `python`
fence remains content. A `tool_code` example inside an
enclosing ordinary code fence also remains content, including malformed
examples. The call's own fence is its markup rather than an example boundary.
An inline mention after prose does not open a call.

A `tool_output` block is a result production. Its body is retained verbatim
between the opening newline and the newline that introduces the first
unindented triple-backtick closer; inline backticks remain result text. As with other dialects,
recovery publishes tool-result bytes as content rather than executable calls.
The first-party declaration production uses the same JSON
name/description/parameters convention as the Pythonic dialect.

## Qualification inputs

Three [conformance anchors](../../../crates/core/tests/data/tool-calls/conformance/gemma3-tool-code.v1)
cover one call, multiple blocks and nested typed arguments. Native
`tool_call_gemma3` cases cover quoted fence text, example suppression, source
spans and result payloads. The common every-pair roundtrip suite includes this
dialect through the registry. Goldens, corpus witnesses, fuzz seed discovery,
long-argument and dense-call fixtures, and portable transcript cases are present.
Capture digests, cost bounds and corpus measurements remain pending their
qualification runs.
