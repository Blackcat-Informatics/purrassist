# R6 review (G7 + R5-review advisory 2)

## Scope
Uncommitted diff vs HEAD 3743aed: `recognizer.rs` (`marker_cut` now calls new `span_is_string`),
tests `tool_call_goldens.rs`, `tool_call_rules.rs`, 2 new goldens. Read-only review; scratch in /tmp.

## Evidence
- `cargo test -q -p purrassist-core --test tool_call_goldens --test tool_call_rules --test tool_call_recover`: 117/56/44 pass.
  `make hygiene` passes; `cargo clippy -p purrassist-core --all-targets -D warnings` clean.
- Differential CLI probe, HEAD binary (built from `git archive HEAD`) vs tree, 576 inputs: 8 reasoning-marker spellings
  (`<think>`, `</think>`, `<thinking>`, Harmony channel, `<think>x</think>`, `<think>`, `\"<think>`, `<|begin_of_thought|>`)
  x hermes/mistral/deepseek/qwen JSON-body/qwen XML (values, keys, numbers, nested arrays, truncated strings, between params,
  nested-parameter items, objects), with and without a leading `<think>pre</think>`.
  Hermes, mistral, deepseek, qwen JSON-body and all non-repeated qwen inputs: byte-identical. Differences are only qwen
  repeated-parameter inputs: marker in a JSON string of an item (xs, rows nested, rows JSON, `\"<think>` in a string,
  Harmony marker, `<think>x</think>`) went exit 4 (cut) -> exit 0 resolved with the literal value (intended).
  Marker in a key, between occurrences, after the value, or in a truncated string stays cut (unchanged).
  Three repeated cases with a bare unquoted marker as a value (`["b", <think> 3]`, `{"a":<think>}`, `2<think>`) changed from
  cut to "bare-string repair reads it as a string"; the single-occurrence form already did that at HEAD (verified identical),
  so repeated now matches single. See ADVISORY 1.
- Edge cases: stringified-JSON hermes arguments (marker in string stays text; outside string cut) identical to HEAD;
  one-string array, three occurrences, bare scalar JSON string occurrences all correct. Leaf raw spans of JSON strings
  include the quotes and equal the node span (the tests above pass), so "exactly that span" matches. Unresolved/stand-in nodes
  without a span never match `within`, and the function returns false (marker treated as not in a string, cut as before).
  One latent oddity: if a node had the same span as its only child, the outer node is checked first and a non-string outer
  returns false; no such wrapper was found to reach this (the `wrap_item` and one-element-array probes were correct).
- Goldens: only the 2 new files; both named tests assert exact `to_json()`, `reasoning` empty, `reasoning_blocks` empty, 0
  marker-cut rules; HEAD binary cuts both inputs (exit 4) so fail-without-fix is confirmed. The rules test covers the
  alias-spelled repetition (`ex` for `rows`) and asserts Resolved, exact value, no marker-cut, no Reasoning segment.

## Cost measurement
Charged governor work (receipt `work`) is linear in k for qwen repeated nested items with a `<think>` per item
(k=1k..8k: 19.3M, 37.8M, 74.7M, 148.7M) and unchanged from HEAD, because `span_is_string` charges nothing.
Wall time (debug CLI, loaded machine): single array of k objects each with a marker, k=32k/64k/128k:
HEAD 6.2/14.1/39.1 s, tree 6.8/17.7/54.1 s. perf at k=64k: `marker_cut::{closure#1}` is 40.7% of the tree's run; at HEAD it
is absent from the top entries. So the change adds a dominant, uncharged term that is quadratic in (markers x siblings).

## Findings
REQUIRED 1. `span_is_string` finds the containing child by a linear scan (`items.iter().find(within)`, and the same over object
members) at every level, once per marker in `marker_cut`, charging nothing to the Governor. For an array of k items with a
marker in each this is O(k^2) uncharged work; HEAD's pointer lookup indexed arrays directly. Measured: 40% of runtime at 64k
items, +38% wall at 128k vs HEAD, and the extra term grows quadratically. This violates the bounded-cost contract (C4: work
must be charged/linear). Fix: children are in document order with disjoint spans, so binary-search by `span.start`
(`partition_point`) for arrays and object members, giving O(depth log width) per marker; and/or walk the tree once per
reading rather than per marker (collect string spans once). Charge the walk if it stays superlinear. Add a cost test
(k repeated/array items each holding a marker, work/byte bounded) next to the existing linear-cost tests.

ADVISORY 1. A bare unquoted marker as a JSON value (`{"a":<think>}`, `2<think>`) is repaired to a string by
`json.syntax.bare-string`, and the node is a `Value::String`, so it is now (and for single occurrences already was at HEAD)
treated as inside a string and not cut. The INTEGRATION.md "reasoning markers are never call content" guarantee arguably
wants these cut. Pre-existing, not introduced by R6; consider a note or follow-up.

ADVISORY 2. The quadratic wall time on 32k+ nested repeated items (push_ledger/call_derivations, ~36 s with no markers at all,
identical at HEAD) is pre-existing and unrelated to R6.

VERDICT: BLOCKED

## Recheck
Design: `marker_cut` builds a `DataIndex` (sorted string-data leaf spans with prefix-max end) lazily once per reading and answers
each marker with one `partition_point`; `marker_cut` now takes the Governor.

- Semantics: a marker is held iff some data leaf (Raw/XmlText always; Json/StringifiedJson only when a String node has exactly the
  leaf's span) has start <= marker < end. The prefix max over starts <= marker is exactly that existential test, so nested or
  overlapping leaf ranges, gaps and numbers (non-string leaf, no string node with its span) behave as the old per-leaf scan.
  Wrappers sharing a child's span, and spanless generated nodes (skipped), are handled; the one difference from the old pointer
  lookup is intended (duplicate members and alias spellings now resolve). `found` keeps the minimum (marker, pair) as `.min()` did.
- Differential probes rebuilt (HEAD vs tree, 174 inputs, hermes/mistral/qwen, 6 marker spellings), receipt/usage stripped: the only
  differences are the 20 repeated-parameter inputs from the first review (marker in a JSON string -> resolved; bare unquoted marker
  -> bare-string repair, same as single occurrences). All other outputs identical, including markers in keys, numbers, gaps,
  trailing positions, truncated strings, and after the closing bracket.
- Tests: goldens 117, rules 56, recover 44, cost 18 (incl. `reasoning_marker_in_every_repeated_item_is_linear`), lib recognizer 2
  (both data_index unit tests) all pass; clippy `-D warnings` clean.
- Wall time (debug CLI): nested repeated items with a marker per item vs a marker-free baseline, k=8k/16k/32k: 2.77/7.4/24.0 s vs
  2.83/7.96/28.1 s (parity; the remaining superlinearity is the pre-existing one, unrelated). Single array with a marker per item
  k=32k/64k/128k: 3.5/7.6/17.1 s (HEAD 6.2/14.1/39.1 s, previous draft 6.8/17.7/54.1 s): about linear.
- Charging: build charges Work for nodes+leaves and 32 B per item of scratch; each lookup charges 1 Work; scratch is released after
  the reading loop on the normal and found paths.

ADVISORY 3. If `DataIndex::build` or a charge returns `Err` mid-loop (`?`), indexes already built are not released. The error is a
sticky stop that ends the recovery, so no result is produced from the leaked reservation; but a guard/lease (`lease_scratch`) would
make release unconditional. Also the sort is O(n log n) charged as n Work; negligible.

REQUIRED 1 is resolved.

VERDICT: PASS

## Orchestrator disposition
- ADVISORY 3: fixed. `DataIndex` holds a `ScratchLease` (RAII), so release is unconditional on every path; the manual release loop is removed and the charges are unchanged. Cost (18), goldens (117), recover (56), `--lib recognizer`, `make hygiene` (exit 0), clippy and fmt pass (implementer).
