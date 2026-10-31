<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `hermes-stripped.v1`

`DialectId::HermesStripped` is the governed call-framing projection of
[`hermes.v1`](hermes.v1.md), representing Hermes JSON without call tags.
The registry stores a `GrammarRecipe::Derived` pointing to the unchanged parent
source. One positioned-AST transform suppresses its enumerated marker bytes
before the existing compiler lowers the grammar. There is no copied grammar,
replacement parser, tokenizer dependency or model execution.

## Marker inventory and provenance

This recipe represents explicit protocol-marker suppression. Its call-reachable inventory is
`<tool_call>`, `</tool_call>`, `</tool_calls>`.

[The pinned public authority](https://huggingface.co/NousResearch/Hermes-2-Pro-Llama-3-8B/blob/f798274b30e7d2d4797c369edcc0cc7473b6e6f2/tokenizer_config.json) establishes the format classification.
The canonical call tags are classified `special: false` at this revision. This profile explicitly suppresses protocol markers. The ordinary repair spelling `</tool_calls>` is also intentionally suppressed so it does not become a mandatory canonical closer when the canonical spelling disappears; it is not attributed to a tokenizer special-token entry.

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
{"name":"f","arguments":{}}
````

Equivalent exact and derived readings retain their candidate dialect IDs;
exact framing has representative priority within an equal-cost invocation tie. Genuine alternative names, IDs,
arguments, repairs or segmentations remain alternatives. Selecting a profile
does not authorize converting an inferred value into a resolved one.

## Executable coverage

Three first-party [anchor fixtures](../../../crates/core/tests/data/tool-calls/conformance/hermes-stripped.v1)
cover one call, several calls and nested typed arguments. The
[`tool_call_stripped` behavioral target](../../../crates/core/tests/tool_call_stripped.rs)
exercises the recipe roster, evidence boundaries, original input, shared detector
candidates, joined-ID ambiguity and indentation work. Qualification results,
corpus measurements and native/WASM capture digests require their actual runs;
this page does not supply execution evidence.
