<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `harmony-stripped.v1`

`DialectId::HarmonyStripped` is the governed call-framing projection of
[`harmony.v1`](harmony.v1.md), representing Harmony recipient and retained header text.
The registry stores a `GrammarRecipe::Derived` pointing to the unchanged parent
source. One positioned-AST transform suppresses its enumerated marker bytes
before the existing compiler lowers the grammar. There is no copied grammar,
replacement parser, tokenizer dependency or model execution.

## Marker inventory and provenance

This recipe represents special-token skipping. Its call-reachable inventory is
`<|start|>`, `<|channel|>`, `<|constrain|>`, `<|message|>`, `<|call|>`, `<|end|>`, `<|return|>`.

[The pinned public authority](https://github.com/openai/harmony/blob/abd677f7ac962629c808197caa1feb9e3e95d2b0/src/tiktoken_ext/public_encodings.rs) establishes the format classification.
The reference encoding lists these spellings in `O200kHarmony` special tokens, with ranks 200006, 200005, 200003, 200008, 200012, 200007 and 200002 respectively. The Apache-2.0 reference source was inspected as format evidence; no source implementation or model is embedded.

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
commentary to=functions.f json{}
````

Equivalent exact and derived readings retain their candidate dialect IDs;
exact framing has representative priority within an equal-cost invocation tie. Genuine alternative names, IDs,
arguments, repairs or segmentations remain alternatives. Selecting a profile
does not authorize converting an inferred value into a resolved one.

The multiple-call anchor retains two whole-turn interpretations: resolved
calls `f` then `g`, or one best-guess call `f` whose trailing text contains the
second apparent invocation. The latter records `tool_call.extract.prose` with
the exact second-line source span and original bytes as a data loss. It does
not invent a guess inference for that non-guess operator. The complete input
remains available, and a mutating best guess remains unusable even when guesses
are accepted. The Standard result is ambiguous; neither reading is discarded.
The unchanged Exact preset allows one candidate, so this competing segmentation
returns explicit candidate exhaustion instead of choosing or truncating a
reading.

## Executable coverage

Three first-party [anchor fixtures](../../../crates/core/tests/data/tool-calls/conformance/harmony-stripped.v1)
cover one call, several calls and nested typed arguments. The
[`tool_call_stripped` behavioral target](../../../crates/core/tests/tool_call_stripped.rs)
exercises the recipe roster, evidence boundaries, original input, shared detector
candidates, joined-ID ambiguity and indentation work. Qualification results,
corpus measurements and native/WASM capture digests require their actual runs;
this page does not supply execution evidence.
