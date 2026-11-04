<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `mistral-v11-stripped.v1`

`DialectId::MistralV11Stripped` is the governed call-framing projection of
[`mistral-v11.v1`](mistral-v11.v1.md), representing Mistral v11 names and retained ordinary IDs.
The registry stores a `GrammarRecipe::Derived` pointing to the unchanged parent
source. One positioned-AST transform suppresses its enumerated marker bytes
before the existing compiler lowers the grammar. There is no copied grammar,
replacement parser, tokenizer dependency or model execution.

## Marker inventory and provenance

This recipe represents special-token skipping. Its call-reachable inventory is
`[TOOL_CALLS]`, `[ARGS]`, `[CALL_ID]`.

[The pinned public authority](https://huggingface.co/unsloth/Mistral-Small-3.2-24B-Instruct-2506/blob/3ed8d341b53ea3d0e4194689905477688a6cf733/tokenizer_config.json) establishes the format classification.
The pinned configuration marks all three spellings `special: true`. The reference tokenizer at `mistral-common` commit `2c30e17cbfe20a405b87522b1c1d51193dafe0ef` also declares them as special tokens. Only the `[CALL_ID]` marker disappears: the ordinary characters of the ID remain. V13 formats omit the ID segment themselves, as documented by the parent grammar.

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
a declared tool or alias, including a name in a native-admitted joint ID split.
Every original boundary remains an alternative after admission. Unknown prose such as `Saved {"name":"Ada"}` stays
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
f c1 {}
````

Equivalent exact and derived readings retain their candidate dialect IDs;
exact framing has representative priority within an equal-cost invocation tie. Genuine alternative names, IDs,
arguments, repairs or segmentations remain alternatives. Selecting a profile
does not authorize converting an inferred value into a resolved one.

`name-arguments-stripped.v1` is a detector category for both
`deepseek-v3.1-stripped.v1` and `mistral-v11-stripped.v1`; it is not a shared
grammar identity. `MarkupFamily::dialects()` reports both candidates. An
explicit allow-list may select either or both.

For joined text such as `ab12{}`, every nonempty split admitted by the original
name and ID class automata remains possible, alongside the whole-name reading:
`a`/`b12`, `ab`/`12`, and `ab1`/`2`. Each inferred boundary carries
`tool_call.markup.stripped-name-id-boundary` and remains a guess. No ID bytes
are discarded. The recognizer retains one maximal joint span, validates name
prefixes and ID suffixes through native finite-state authority, then
materializes admitted alternatives under the caller's governor. Canonical
emission can retain parent-admitted whitespace between name and ID, making
that emitted surface unambiguous without inventing a missing source boundary.

The framing scan and fixed-class boundary validation remain linear in input
length. Copying all admitted names, IDs and argument values additionally costs
proportionally to the materialized output; joined runs can require quadratic
output bytes. Resource refusal remains explicit and retains original input.

## Executable coverage

Three first-party [anchor fixtures](../../../crates/core/tests/data/tool-calls/conformance/mistral-v11-stripped.v1)
cover one call, several calls and nested typed arguments. The
[`tool_call_stripped` behavioral target](../../../crates/core/tests/tool_call_stripped.rs)
exercises the recipe roster, evidence boundaries, original input, shared detector
candidates, joined-ID ambiguity and indentation work. Qualification results,
corpus measurements and native/WASM capture digests require their actual runs;
this page does not supply execution evidence.
