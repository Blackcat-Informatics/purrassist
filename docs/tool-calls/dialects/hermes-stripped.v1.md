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

Without strict tool-choice constraints, optional whitespace left by suppressed
markers has one maximal framing. Grouped calls retain the latest end and then
the earliest start in call order. This applies only within the
same derived grammar when the ordered calls, every captured source span,
arguments, outcome and complete ledgers agree. Different captures or
non-whitespace boundaries remain distinct. Strict choices retain the original
content alternatives. An intact native JSON reading also precedes a matching
projected envelope whose only changes are exact exterior prose or fence
extractions; each retains its original loss kind and source bytes.

A physically consumed canonical native Mistral V3 `[TOOL_CALLS]` marker keeps
its grouped attempt when a projected completion discards that marker as leading
prose. The recognizer remembers the earliest native frame within each weak
header and checks every materialized completion independently. Every raw
Hermes reading must contain the whole marker in a leading `Dropped` capture
with a matching `ExtractProse` edit, before every name, ID and argument capture.
Missing proof retains the entire completion. The weak attempt stays alive for
future configurations and completions; later calls with their own source
prefixes remain represented. The native attempt keeps its inherited
interpretation, malformed group members and cut-off tails. Repaired or
recipe-suppressed markers supply no physical frame proof.

Projected JSON envelopes also retain declaration-shaped counter-evidence.
Without invocation markers, arguments containing `"type":"object"` and a
`"properties"` object stay content when the named tool cannot accept them as
written. This applies through discarded prose and array framing in the Hermes,
Llama JSON and Mistral v3 projections. An exact name or explicit alias whose
original arguments already passed the selected tool's schema remains a call,
including a guess caused only by exterior extraction or example context.
The resolver reuses its completed validation and compares the original and
selected arguments semantically; it performs no second schema validation and
does not claim lexical equality. Mixed native framing, different captures or
different argument values retain their alternatives. A native invocation marker
such as `<|python_tag|>` continues to establish a call even when its arguments
resemble a declaration.

## Executable coverage

Three first-party [anchor fixtures](../../../crates/core/tests/data/tool-calls/conformance/hermes-stripped.v1)
cover one call, several calls and nested typed arguments. The
[`tool_call_stripped` behavioral target](../../../crates/core/tests/tool_call_stripped.rs)
exercises the recipe roster, evidence boundaries, original input, shared detector
candidates, joined-ID ambiguity and indentation work. Qualification results,
corpus measurements and native/WASM capture digests require their actual runs;
this page does not supply execution evidence.
