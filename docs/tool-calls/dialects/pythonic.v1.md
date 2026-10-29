<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `pythonic.v1`

Grammar: [`pythonic.v1.grammar`](../../../crates/core/src/tool_call/dialects/pythonic.v1.grammar),
built in as `DialectId::Pythonic`, with the native `pythonic` literal codec.
The grammar and examples are first-party; no model code is executed.

## Format and provenance

Meta's Llama 3.2 text prompt-format guide identifies the 1B and 3B Instruct
models' function-calling format as a list of keyword calls:
`[get_weather(city='Paris'), get_weather(city='Oslo')]`.
The pinned source is
[Meta's guide at commit 57d9b434514159a812a61e38c992c276ef5432dc](https://github.com/meta-llama/llama-models/blob/57d9b434514159a812a61e38c992c276ef5432dc/models/llama3_2/text_prompt_format.md).
It documents an unmarked list and an end-to-end example prefixed with
`<|python_tag|>`, JSON tool declarations, and `ipython` tool results ending
with `<|eot_id|>`. The additional `<|python_start|>` opener implements the
accepted recovery contract; this token is not attributed to that guide.
Only textual format evidence was inspected; no model was downloaded or run.

## Accepted literals and boundaries

An unmarked `[` must begin the turn or a line, allowing preceding spaces
and tabs. Either explicit marker may appear after prose. At least one
call is required. Tool names may be dotted identifiers and must match a declared
tool; every argument must have an ASCII identifier keyword.
Positional arguments produce `pythonic_positional_rejected`. Fenced,
inline-code and blockquote examples remain content, including malformed
examples. A rejected example cannot hide a later closing fence or call.

The codec accepts single, double and triple-quoted strings, `r` and `u`
string prefixes, standard Python escapes, exact decimal integers and
floats, underscored numbers, binary/octal/hexadecimal integers, `True`,
`False`, `None`, lists and dictionaries with string keys. Nested literals
and quoted delimiters remain inside their argument. Original byte spans
are retained. Schema validation and ordinary recovery policies apply to
the resulting typed values. Canonical emission uses quoted strings,
Python boolean/null spellings, lists and dictionaries; source splicing
reprints changed Python arguments using the same codec.

`\N{name}` uses Unicode 17.0 canonical names and aliases, plus algorithmic
Hangul and ideograph names. The generated tables and checksums are recorded
in [`vendor/unicode/python-names.json`](../../../vendor/unicode/python-names.json)
and [`DEPENDENCIES.md`](../../DEPENDENCIES.md). Named sequences are refused:
Python's literal escape does not accept a name resolving to multiple code
points. Lone surrogate escapes are refused because the value contract uses
Unicode scalar strings. Unknown escapes retain their backslash.

No expression evaluation, function invocation, comprehensions, unpacking,
formatted or byte strings, tuples, sets, adjacent-literal concatenation,
or non-string dictionary keys are supported. Unterminated strings and
incomplete numeric literals are rejected; a missing outer closer does not
synthesize a missing value. Literal nesting is bounded and all parsing,
name lookup and integer conversion consume the caller's resource budget.
Nondecimal integer conversion uses O(n²) arbitrary-precision digit work,
admitted before each multiplication pass. Canonical decimal parsing and
finite-state framing remain linear; the grammar scan bound does not cover
that typed conversion. Exhaustion is an explicit resource outcome.

## Executable evidence

Three [conformance anchors](../../../crates/core/tests/data/tool-calls/conformance/pythonic.v1)
cover one call, multiple calls, and nested typed arguments with multiline
strings. `tool_call_pythonic` exercises literal semantics, positional and
expression refusal, source spans, and context boundaries. The common
roundtrip suite exercises every dialect pair. Golden, corpus, fuzz, cost
and portable-harness inputs include this dialect; recorded measurements
and capture digests are maintained by their qualification runs.
