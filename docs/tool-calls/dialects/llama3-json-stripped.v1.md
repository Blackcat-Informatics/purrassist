<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `llama3-json-stripped.v1`

`DialectId::Llama3JsonStripped` is the governed call-framing projection of
[`llama3-json.v1`](llama3-json.v1.md), representing Llama 3 JSON without the Python marker.
The registry stores a `GrammarRecipe::Derived` pointing to the unchanged parent
source. One positioned-AST transform suppresses its enumerated marker bytes
before the existing compiler lowers the grammar. There is no copied grammar,
replacement parser, tokenizer dependency or model execution.

## Marker inventory and provenance

This recipe represents special-token skipping. Its call-reachable inventory is
`<|python_tag|>`, `<|eom_id|>`, `<|eot_id|>`.

[The pinned public authority](https://huggingface.co/unsloth/Llama-3.1-8B-Instruct/blob/4699cc75b550f9c6f3173fb80f4703b62d946aa5/tokenizer_config.json) establishes the format classification.
The pinned configuration marks all three spellings `special: true`. Checked Llama 3.2 and 3.3 configurations at `006f5dcd1393c3add266de40994ba96225e9689d` and `99cd0d2c829e92a67c844f9144c2509632e5c87f` retain that classification. The markers are format evidence; no model files or weights are used by recovery.

The projection changes call framing only. Parent result, declaration and
reasoning conventions remain inherited; erased result framing does not create
a wildcard parser for ordinary prose, and erased reasoning markers do not
invent a reasoning boundary. Unicode confusable spellings remain the generic
costed repair of retained terminals. Only the enumerated exact bytes are
suppressed.

## Retained evidence and ambiguity

A source line may start with spaces or tabs. Weak forms require retained
call evidence before they become attempts; a bare word does not commit a call.
A derived call starts at the first non-whitespace byte on its source line.
Indentation stays outside that call, so it cannot overlap the preceding call
and is not repeatedly rescanned.
JSON argument forms use the native codec. Harmony keeps its recipient-based
commit and raw Python-body behavior; Llama keeps its retained name/key evidence.
Deleting a marker cannot preserve a commit that occurred before any remaining
input byte.

Example surface, with ordinary source bytes retained:

````text
{"name":"f","parameters":{}}
````

Equivalent exact and derived readings retain their candidate dialect IDs;
exact framing has representative priority within an equal-cost invocation tie. Genuine alternative names, IDs,
arguments, repairs or segmentations remain alternatives. Selecting a profile
does not authorize converting an inferred value into a resolved one.

## Executable coverage

Three first-party [anchor fixtures](../../../crates/core/tests/data/tool-calls/conformance/llama3-json-stripped.v1)
cover one call, several calls and nested typed arguments. The
[`tool_call_stripped` behavioral target](../../../crates/core/tests/tool_call_stripped.rs)
exercises the recipe roster, evidence boundaries, original input, shared detector
candidates, joined-ID ambiguity and indentation work. Qualification results,
corpus measurements and native/WASM capture digests require their actual runs;
this page does not supply execution evidence.
