<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `deepseek-v3.1-stripped.v1`

`DialectId::DeepSeekV31Stripped` is the governed call-framing projection of
[`deepseek-v3.1.v1`](deepseek-v3.1.v1.md), representing DeepSeek V3.1 name followed by JSON.
The registry stores a `GrammarRecipe::Derived` pointing to the unchanged parent
source. One positioned-AST transform suppresses its enumerated marker bytes
before the existing compiler lowers the grammar. There is no copied grammar,
replacement parser, tokenizer dependency or model execution.

## Marker inventory and provenance

This recipe represents explicit protocol-marker suppression. Its call-reachable inventory is
`<｜tool▁calls▁begin｜>`, `<｜tool▁call▁begin｜>`, `<｜tool▁sep｜>`, `<｜tool▁call▁end｜>`, `<｜tool▁calls▁end｜>`.

[The pinned public authority](https://huggingface.co/deepseek-ai/DeepSeek-V3.1/blob/c0781d039fb7a1ba2abc4add0bdc293e92d2b8db/tokenizer.json) establishes the format classification.
The complete added-token array at this revision marks the call, section and separator spellings `special: false`. This is an explicit marker-suppression profile, not a claim that ordinary Hugging Face special-token skipping removes them. Later configuration files that omit token flags do not establish a different classification.

The projection changes call framing only. Parent result, declaration and
reasoning conventions remain inherited; erased result framing does not create
a wildcard parser for ordinary prose, and erased reasoning markers do not
invent a reasoning boundary. Unicode confusable spellings remain the generic
costed repair of retained terminals. Only the enumerated exact bytes are
suppressed.

## Retained evidence and ambiguity

A source line may start with spaces or tabs. Weak forms require retained
call evidence before they become attempts; a bare word does not commit a call.
When declared tools are supplied, a whitespace-separated plain name must name
a declared tool or alias. Unknown prose such as `Saved {"name":"Ada"}` stays
content; contiguous `unknown{}` retains unknown-tool behavior.
A derived call starts at the first non-whitespace byte on its source line.
Indentation stays outside that call, so it cannot overlap the preceding call
and is not repeatedly rescanned.
JSON argument forms use the native codec. Harmony keeps its recipient-based
commit and raw Python-body behavior; Llama keeps its retained name/key evidence.
Deleting a marker cannot preserve a commit that occurred before any remaining
input byte.

Example surface, with ordinary source bytes retained:

````text
f{}
````

Equivalent exact and derived readings retain their candidate dialect IDs;
exact framing has representative priority within an equal-cost invocation tie. Genuine alternative names, IDs,
arguments, repairs or segmentations remain alternatives. Selecting a profile
does not authorize converting an inferred value into a resolved one.

`name-arguments-stripped.v1` is a detector category for both
`deepseek-v3.1-stripped.v1` and `mistral-v11-stripped.v1`; it is not a shared
grammar identity. `MarkupFamily::dialects()` reports both candidates. An
explicit allow-list may select either or both.

## Executable coverage

Three first-party [anchor fixtures](../../../crates/core/tests/data/tool-calls/conformance/deepseek-v3.1-stripped.v1)
cover one call, several calls and nested typed arguments. The
[`tool_call_stripped` behavioral target](../../../crates/core/tests/tool_call_stripped.rs)
exercises the recipe roster, evidence boundaries, original input, shared detector
candidates, joined-ID ambiguity and indentation work. Qualification results,
corpus measurements and native/WASM capture digests require their actual runs;
this page does not supply execution evidence.
