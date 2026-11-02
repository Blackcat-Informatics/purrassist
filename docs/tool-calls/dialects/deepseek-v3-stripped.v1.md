<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `deepseek-v3-stripped.v1`

`DialectId::DeepSeekV3Stripped` is the governed call-framing projection of
[`deepseek-v3.v1`](deepseek-v3.v1.md), representing DeepSeek V3 / R1 fenced JSON.
The registry stores a `GrammarRecipe::Derived` pointing to the unchanged parent
source. One positioned-AST transform suppresses its enumerated marker bytes
before the existing compiler lowers the grammar. There is no copied grammar,
replacement parser, tokenizer dependency or model execution.

## Marker inventory and provenance

This recipe represents explicit protocol-marker suppression. Its call-reachable inventory is
`<｜tool▁calls▁begin｜>`, `<｜tool▁call▁begin｜>`, `<｜tool▁sep｜>`, `<｜tool▁call▁end｜>`, `<｜tool▁calls▁end｜>`.

[The pinned public authority](https://huggingface.co/deepseek-ai/DeepSeek-V3-0324/blob/e9b33add76883f293d6bf61f6bd89b497e80e335/tokenizer.json) establishes the format classification.
The complete added-token array at this revision marks the call, section and separator spellings `special: false`. The requested marker-suppression profile removes them explicitly; ordinary Hugging Face special-token skipping is not established for these spellings. R1-0528 at `4236a6af538feda4548eca9ab308586007567f52` has the same classification.

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
functionf
```json
{}
```
````

Equivalent exact and derived readings retain their candidate dialect IDs;
exact framing has representative priority within an equal-cost invocation tie. Genuine alternative names, IDs,
arguments, repairs or segmentations remain alternatives. Selecting a profile
does not authorize converting an inferred value into a resolved one.

For canonical grouped calls, uncaptured ASCII whitespace between a retained
closing fence and the next retained `function` belongs to the preceding call.
The engine applies the same preference for the latest boundary to owned
outgoing parts before admitting turn paths as candidates. It changes no argument or capture
span, numeric spelling, ledger, content or native grammar. Repaired, locally
ambiguous and rejected boundaries, and boundaries that overlap captured data,
retain their alternatives.
Shared histories and cached chains stay immutable, and the original engine
input window supplies the byte checks for both incremental and batch callers.
If normalization refuses, the engine preserves that refusal, drops the owned
payloads and refunds their recorded scratch admissions. This includes copied
parts, each original chain's last part and the remaining held completions;
shared cached chains retain their admissions.

## Executable coverage

Three first-party [anchor fixtures](../../../crates/core/tests/data/tool-calls/conformance/deepseek-v3-stripped.v1)
cover one call, several calls and nested typed arguments. The
[`tool_call_stripped` behavioral target](../../../crates/core/tests/tool_call_stripped.rs)
exercises the recipe roster, evidence boundaries, original input, shared detector
candidates, joined-ID ambiguity and indentation work. Qualification results,
corpus measurements and native/WASM capture digests require their actual runs;
this page does not supply execution evidence.
