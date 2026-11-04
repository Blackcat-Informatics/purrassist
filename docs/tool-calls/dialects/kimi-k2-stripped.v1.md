<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `kimi-k2-stripped.v1`

`DialectId::KimiK2Stripped` is the governed call-framing projection of
[`kimi-k2.v1`](kimi-k2.v1.md), representing Kimi K2 composite ID and JSON.
The registry stores a `GrammarRecipe::Derived` pointing to the unchanged parent
source. One positioned-AST transform suppresses its enumerated marker bytes
before the existing compiler lowers the grammar. There is no copied grammar,
replacement parser, tokenizer dependency or model execution.

## Marker inventory and provenance

This recipe represents explicit protocol-marker suppression. Its call-reachable inventory is
`<|tool_calls_section_begin|>`, `<|tool_call_begin|>`, `<|tool_call_argument_begin|>`, `<|tool_call_end|>`, `<|tool_calls_section_end|>`.

[The pinned public authority](https://huggingface.co/moonshotai/Kimi-K2-Instruct/blob/fd1984e2b7a3350dbf7305fe73a4ede25c14de50/tokenizer_config.json) establishes the format classification.
The five protocol spellings are classified `special: false` in the pinned Hugging Face configuration. The 0905 revision `ac6c49f04883bd0a0598b790693a72061c676629` has the same configuration, and Thinking revision `a51ccc050d73dab088bf7b0e2dd9b30ae85a4e55` preserves those flags. Kimi also supplies a custom tiktoken decoder; its vocabulary mechanism is not evidence that Hugging Face special-token skipping deletes these markers. This profile explicitly suppresses them.

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
functions.f:0{}
````

Equivalent exact and derived readings retain their candidate dialect IDs;
exact framing has representative priority within an equal-cost invocation tie. Genuine alternative names, IDs,
arguments, repairs or segmentations remain alternatives. Selecting a profile
does not authorize converting an inferred value into a resolved one.

## Executable coverage

Three first-party [anchor fixtures](../../../crates/core/tests/data/tool-calls/conformance/kimi-k2-stripped.v1)
cover one call, several calls and nested typed arguments. The
[`tool_call_stripped` behavioral target](../../../crates/core/tests/tool_call_stripped.rs)
exercises the recipe roster, evidence boundaries, original input, shared detector
candidates, joined-ID ambiguity and indentation work. Qualification results,
corpus measurements and native/WASM capture digests require their actual runs;
this page does not supply execution evidence.
