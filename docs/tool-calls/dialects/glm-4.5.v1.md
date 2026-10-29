<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# GLM 4.5 key/value calls

`glm-4.5.v1` uses the `kv-pairs` codec:

```text
<tool_call>get_weather
<arg_key>city</arg_key><arg_value>Paris</arg_value>
<arg_key>days</arg_key><arg_value>3</arg_value>
</tool_call>
```

The name precedes alternating key and value elements. Keys are text. A declared
string is verbatim, including leading/trailing whitespace and newlines; there
is no framing newline inside a value. Other declared types use inline JSON,
including nested objects and arrays. Tag-looking text inside a JSON string
remains JSON content. Unknown or unmodeled declarations retain source text for
the existing resolver's schema typing. Repeated keys remain separate occurrences:
array parameters collect, and repeated scalars follow the ordinary ambiguity
policy rather than silently choosing the final occurrence.

Multiple call wrappers may occur in one turn. `<think>...</think>` delimits
reasoning. The result production uses `<tool_response>...</tool_response>`;
tool declarations use the JSON function objects contained in `<tools>` in the
source template. Original source spans and bytes remain the evidence authority.

Missing key closers before a value opener and missing value closers at an outer
call boundary use the existing costed markup repairs. A missing value closer
before another key is structural for complete typed JSON; an inferred text
boundary is a guess. Unique one-edit tag misspellings use the existing markup
misspelling rule. Malformed alternating pairs are rejected. There is no nested
key/value syntax, XML entity decoding, or whole-JSON replacement body.

The inverse emitter writes declared strings verbatim and other values as JSON.
It checks the result through the same codec and rejects unrepresentable delimiter
collisions or a body that would require repair or change the argument reading.
It does not invent an escaping convention for raw GLM strings. Tool identifiers
must fit the grammar's identifier class; the format carries no call ID field.

## Provenance

The format was checked against the textual `zai-org/GLM-4.5` chat template at
immutable revision `cbb2c7cfb52fa128a9660cb1a7a78e017899e115`:
[chat_template.jinja](https://huggingface.co/zai-org/GLM-4.5/blob/cbb2c7cfb52fa128a9660cb1a7a78e017899e115/chat_template.jinja).
Its argument loop prints string values directly and applies `tojson` to other
values. Its reasoning and tool-response delimiters establish the corresponding
productions. These are first-party grammar and fixture implementations derived
from that published format; the template itself is not vendored. No model was
loaded or executed, and no other GLM version was checked.

The conformance directory contains single-call, multiple-call and nested typed
argument anchors. The shared every-pair roundtrip and fuzz-anchor loaders include
this dialect. Dedicated goldens retain malformed-tag evidence; corpus witnesses
and the portable transcript cover canonical and repaired calls.
