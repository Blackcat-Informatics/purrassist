<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Integrating tool-call recovery

This page states what an integrator can rely on when tool-call recovery sits
between a model server and a tool executor, how each guarantee is enforced,
and the test or check that proves it. Test names are integration tests in
`crates/core/tests` unless another crate is named. The overview, policy
matrix, and finish classes are in [README.md](README.md).

## Guarantees

### C1. Network-free, dependency-free core

Tool-call recovery lives entirely in `purrassist-core`, which has no
third-party runtime dependency and no ambient filesystem, network, clock,
process, thread, or random-number access. It performs no I/O: the caller
passes the output bytes and the declarations, and receives values.

- **Enforced by** `scripts/check-repository.py` (run by `make hygiene` and so
  `make check`): the resolved dependency closure of `purrassist-core` over
  normal, build, and dev edges must be empty (`check_core_closure`), and its
  manifest may declare no runtime, target-specific, dev, or build
  dependencies (`check_core_dev_dependencies`); no publishable crate
  may reach a networking or async-runtime package (`check_network_denylist`);
  core source may not name `std::net`, `std::process`, `std::fs`,
  `std::env`, `std::thread`, `SystemTime`, `Instant`, `HashMap`, `HashSet`,
  `RandomState`, or standard streams (`check_ambient_access`).
- **Proved by** `scripts/test-repository-audit.py`, whose cases inject each
  forbidden dependency and token and require the audit to fail with its
  location, and require the current core source to pass.

### C2. No silent guesses

Every repair applied to a call is itemized on that call, and nothing uncertain
is reported as certain.

- **Per-call itemized repairs.** `ToolCall::repairs` holds the evidence,
  losses (each with its `markup` or `data` kind), and inferences of the
  selected reading; the receipt itemizes the same entries. Every rule id is in
  the registry ([RULES.md](RULES.md)). *Proved by*
  `tool_call_recover::per_call_repairs_itemized`,
  `tool_call_rules::every_rule_has_cost_and_policy`,
  `tool_call_rules::every_rule_declares_loss_kind`,
  `tool_call_rules::rules_doc_matches_registry`, and the captured
  `tool_call_goldens` (one golden per rule, `every_rule_has_a_golden`).
- **Guesses are labeled and demote.** A guess inference or a data loss makes a
  call at most `BestGuess`; several equally good readings make it
  `Ambiguous`, with the competing readings in `ToolCall::alternatives`.
  *Proved by* `tool_call_rules::data_loss_demotes_to_best_guess`,
  `tool_call_rules::markup_loss_does_not_demote`,
  `tool_call_rules::repeated_scalar_reports_both_readings`, and
  `tool_call_rules::per_call_alternatives_exposed`.
- **Ambiguity is evidence, never a dialect-label tie.** Dialects that share
  an opener (Qwen3-Coder XML and Hermes both open with `<tool_call>`) each
  attempt a call there. A complete call is read by the dialect its body
  evidences (a `<function=` body is Qwen3-Coder XML, a JSON object body is
  Hermes): the other dialect's attempt is rejected, and a rejection costs
  more than any repair a call needs. Whole-message readings that recover the
  same calls and differ only in which dialect's attempt was rejected (a
  dangling, cut, or malformed `<tool_call>`) are one reading: the rejected
  attempt reported is the one whose dialect read furthest into the body, and
  `RejectedCall::candidate_dialects` (wire `candidate_dialects`) names every
  dialect that attempted it. Dialects that read the same bytes into the
  same call are one reading too, and `ToolCall::candidate_dialects` (wire
  `candidate_dialects`) names all of them, so no dialect attribution is
  dropped. Readings that recover different calls with identical ledgers
  carry a passing `dialect=ID` validation per call, so an `Ambiguous`
  operation always publishes distinguishing evidence for every
  ranked-adjacent pair, and complete calls never drop out of `value`
  because of a dialect label. *Proved by*
  `tool_call_recover::body_evidence_selects_the_dialect_of_a_shared_opener`,
  `tool_call_recover::rejected_attempts_over_a_shared_opener_are_one_reading`,
  `tool_call_recover::same_call_from_two_dialects_names_both`,
  `tool_call_recover::different_calls_from_two_dialects_are_distinguished_by_dialect`,
  `tool_call_goldens::default_allow_list_never_loses_a_call` (every golden
  also runs under the default allow-list),
  `tool_call_goldens::ambiguity_always_has_distinguishing_evidence`, the
  `mixed_complete_calls_then_dangling_opener` golden, the `fuzz-tool-call`
  harness, and the host test
  `tool_call_tools::dangling_opener_after_a_call_keeps_the_call`.
- **`AmbiguityPolicy::Reject`.** Any whole-message or per-call ambiguity makes
  the operation `Unresolved` with a failed `tool_call.ambiguity` validation,
  every alternative kept in the receipt. *Proved by*
  `tool_call_recover::strict_ambiguity_rejects` and the
  `strict_ambiguity_rejected` golden.
- **Single-reading repairs only for `Mutating`/`Structural`.** `Structural`
  admits exactly the rules with a single reading; a reading that needs any
  other rule is `Unresolved` (`RuleNotAdmitted`). A guess on a `Mutating`
  tool is never `Resolved`, never usable, and never emitted. *Proved by*
  `tool_call_rules::structural_admits_only_single_reading_rules`,
  `tool_call_rules::mutating_guess_never_resolved`,
  `tool_call_recover::usable_never_yields_mutating_guess`,
  `tool_call_roundtrip::mutating_guess_never_emitted`, and
  `tool_call_goldens::no_mutating_guess_resolved_over_goldens`.
- **Measured.** The 3,234-case correction evaluation records zero incorrect
  automatic answers overall, per rule, and per dialect, and abstains on every
  case whose only correct answer is abstention
  (`correction_evaluation::tool_call_recovery_evaluation_has_zero_incorrect_corrections`;
  see [MATCHING-QUALITY.md](../MATCHING-QUALITY.md#tool-call-recovery-evaluation)).

### C3. Determinism

The same output, tool set, and request always give the same result, receipt,
and resource usage, on every platform.

- **Enforced by** portable work-unit budgets (every charge is a function of
  the bytes, the tool set, and the request, never of time or pointer width);
  a total order on readings (cost, then a content digest), independent of
  tool and dialect declaration order; canonical `DialectSet` order by
  `(id, digest)`; ordered maps only (`HashMap` is forbidden in core, see C1);
  and a recognizer that is a fold of a per-byte step, so chunking never
  changes a result.
- **Proved by** `tool_call_recover::deterministic_pinned_digest` (the wire
  form of a mixed-dialect recovery, and of an `Ambiguous` one, is identical
  for permuted tool and dialect orders and equals a pinned SHA-256 digest),
  `tool_call_rules::resolution_independent_of_tool_and_dialect_order`,
  `grammar_engine::dialect_set_order_independent`,
  `grammar_engine::push_split_equals_single_push`,
  `grammar_codecs::codec_every_split_equals_batch`, and
  `tool_call_cost::push_chunking_identical_work` (chunks of 1, 16, and 4096
  bytes charge identical usage). `make wasm-test` executes the core
  transcript, which includes a tool-call recovery with a fuzzy name and a
  stray closer, natively, under WASI, and on `wasm32-unknown-unknown`, and
  requires identical output.

### C4. Bounded cost

Recovery work is linear in the output length for a fixed dialect
allow-list, with a published bound, and every limit is explicit.

- **Enforced by** the published bound
  `work ≤ (2 + K_LEX + Σ_d K_d)·n + K_ERR·E` (each dialect contributes
  `K_THREAD` times its configuration bound per byte; `E` error configurations
  are capped by the effort's budget), linear JSON extraction, and governed
  charges for every step. Exhaustion is an `Unresolved` result naming the
  resource, never a partial value.
- **Proved by** `tool_call_cost::resources_doc_matches_cost_constants` (every
  published constant and per-dialect bound equals the code's),
  `tool_call_cost::one_mib_argument_work_bound` (1 MiB Qwen3-Coder XML,
  Hermes, Llama 3 JSON, Mistral v3, Mistral v11, DeepSeek-V3.1,
  DeepSeek-V3, Kimi-K2, Anthropic, Harmony and bare JSON arguments satisfy the bound and scale linearly from 64 KiB),
  `tool_call_cost::one_mib_argument_fits_published_budget` (a 1 MiB argument
  in every built-in dialect is `Resolved` under `ToolRecoveryRequest::sized`
  for every effort from `Quick` to `Maximum`, with peak scratch at most 16
  bytes per input byte for Qwen3-Coder XML, Hermes, Llama 3 JSON, Mistral v3,
  Mistral v11, DeepSeek-V3.1, DeepSeek-V3, Kimi-K2, Anthropic, Harmony and
  bare JSON, and at most `RECOVER_SCRATCH_PER_BYTE` for `OpenAI` stringified
  arguments),
  `tool_call_cost::work_independent_of_dialect_count_beyond_bound`,
  `json_incremental::extract_many_brackets_is_linear`, and
  `tool_call_recover::exhaustion_is_explicit_every_resource`.
- **Memory, including validation.** Every allocation recovery makes is
  charged to its governor, including the working set of each call's schema
  validation (about 0.3 to 2.7 KiB per call on the measured tool schemas,
  charged before it is allocated). A scratch limit below that working set
  is an explicit `BudgetExceeded(ScratchBytes)`, never a validation that
  succeeds on uncharged memory. *Proved by*
  `allocation_oracle::oracle_tool_call_validation_charges_its_working_set`
  and the `tool_call_recover` row of the allocation oracle.
- **Sizing.** Use `ToolRecoveryRequest::sized(effort, output)` (or
  `sized_for(effort, output, dialects)` for another allow-list); the MCP tool
  and the CLI use it by default. It adds 32 work units and 48 scratch bytes
  per output byte to the preset, and `⌈preset.candidates / 8⌉` candidates
  per call opener of the output (16 per opener at `Standard`), counted by a
  linear pre-scan (`DialectSet::call_openers`), so a busy turn does not
  exhaust a budget sized for an ordinary one. Contested spans whose
  segmentations recover the same calls are folded before whole-message
  readings are enumerated, so they add readings instead of multiplying
  them. Every limit stays finite and linear in the output length, and
  readings that genuinely multiply still exhaust explicitly. *Proved by*
  `tool_call_cost::busy_turn_resolves_every_call_under_sized` (64 calls,
  each with a markup repair and a contested stray opener: every call
  `Resolved` at `Effort::Standard`, where the fixed preset exhausts),
  `tool_call_cost::per_call_competition_scales_with_calls`,
  `tool_call_cost::crafted_reading_explosion_exhausts_explicitly`,
  `tool_call_cost::sized_budget_follows_the_published_formula`, and the host
  and CLI tests `tool_call_tools::busy_turn_resolves_every_call_by_default`
  and `tool_calls_cli::busy_turn_resolves_every_call_by_default`. See
  [RESOURCES.md](../RESOURCES.md#budgets-for-large-outputs-and-busy-turns).

### C5. Lossless declaration adapters

`ToolSet::from_openai`, `from_mcp`, and `from_anthropic` accept the wire
declarations the model was shown and lose nothing.

- **Enforced by** keeping each declaration's exact bytes and spans,
  compiling the *exact* parameters schema text (never a re-rendering) under
  the dialect its `$schema` selects: Draft 2020-12 by default, or Draft-07
  (`http://json-schema.org/draft-07/schema#`, common in MCP servers). Every
  keyword the typed projection does not model is listed per call
  (`ToolCall::keywords`, with whether repair consulted it) and enforced by
  validation against the exact schema. A malformed declaration is a hard
  error; no declaration is skipped.
- **Proved by** `tool_adapters::openai_render_back_is_byte_exact`,
  `tool_adapters::mcp_render_back_is_byte_exact`,
  `tool_adapters::anthropic_render_back_is_byte_exact`,
  `tool_adapters::draft07_mcp_tools_adapt_losslessly`,
  `tool_adapters::adapter_schema_compiled_from_exact_text_rejects_violation`,
  `tool_adapters::unmodeled_keywords_are_listed`,
  `tool_adapters::invalid_declaration_is_hard_error`, and
  `tool_call_recover::anyof_ref_enforced_on_recovered_call`.

### C6. One caller-chosen policy, stop reason as input

- **One policy for every tool.** `PolicySelection::All(policy)` applies one
  `RepairPolicy` to every tool, whatever each declares
  (MCP `"policy": "structural" | "tolerant"`, CLI `--policy`). The default,
  `PerTool`, uses each tool's explicit policy or the one its effects select.
  *Proved by* `tool_call_rules::policy_selection_all_overrides_per_tool` and
  `tool_call_recover::single_policy_override_applies_to_all`.
- **The stop reason is an input.** The caller passes `GenerationStop`; only
  `Length` reads an open call as truncation, which the turn reports as
  `Finish::Truncated` or `TruncatedWithCalls`, `finish_reason` `"length"`.
  *Proved by* `tool_call_recover::length_vs_stop_truncation`,
  `tool_call_recover::unknown_stop_is_conservative`,
  `tool_call_recover::stop_sequence_consumed_closer`, and
  `tool_call_roundtrip::finish_reason_mapping`.

### C7. `OpenAI` `tool_calls` output shape

`EmitTarget::OpenAiToolCalls` (MCP `"emit": "openai"`, CLI `--emit openai`)
and `emit::openai_tool_calls` return the `tool_calls` array itself: exactly
`{"id", "type": "function", "function": {"name", "arguments"}}` per emitted
call, with `arguments` the compact JSON string of the recovered arguments.
There is no `role` and no message envelope; the caller places the array in
its own response. Ids come from the request's `IdPolicy` and are
caller-scoped: distinct within the turn, and, with zero configuration,
distinct across the rounds of a conversation. The default, `IdPolicy::Turn`,
keeps each id the model wrote (an `OpenAI` `tool_calls[].id`) and numbers
every other call `call_<tag>_<index>`, where `<tag>` is the first 12
hexadecimal digits of the SHA-256 digest of the turn's output; a model id
that is missing where the dialect provides for one, or repeats an earlier
call's id, is numbered so and recorded as a derived `tool_call.id.dedup`
inference (see [Ids across rounds](#ids-across-rounds)).

- **Proved by** `tool_call_roundtrip::openai_shape_exact`,
  `tool_call_roundtrip::emit_option_end_to_end`,
  `tool_call_roundtrip::emission_never_emits_unresolved_silently`,
  `tool_call_recover::ids_never_collide_each_policy`,
  `tool_call_recover::default_ids_are_disjoint_across_turns`,
  `tool_call_recover::default_ids_are_deterministic_per_turn`,
  `tool_call_recover::default_ids_preserve_model_ids_and_repair_duplicates`,
  `tool_call_recover::explicit_prefix_is_unchanged`,
  `tool_call_recover::deterministic_pinned_digest` (the explicit
  `Prefix { prefix: "call_", start: 0 }` wire form is pinned to the digest
  recovery published before the default changed), the CLI tests
  `tool_calls_cli::qwen_xml_recovers_via_cli` (the emitted element has no
  `role`) and `tool_calls_cli::default_ids_are_disjoint_across_turns`, and the
  host test `tool_call_tools::default_ids_are_disjoint_across_turns`.

## Integration notes

### Read `finish` and `rejected`, not only `finish_reason`

`Finish::openai_finish_reason()` maps `Finish::Unresolved` to `"stop"`, as
the `OpenAI` wire format has no better value. `"stop"` therefore hides
rejected and unresolved calls. Read `ToolCalls::finish` (wire `finish`) and
`ToolCalls::rejected` (wire `rejected`) before deciding that a turn
completed cleanly. A rejected attempt's `dialect` is the dialect whose
grammar read furthest before failing; `candidate_dialects` lists every
dialect that attempted a call at the same opener.

### Map serving stops to `GenerationStop`

| Serving-layer stop | `GenerationStop` |
| --- | --- |
| End-of-turn or end-of-sequence token | `Stop` |
| Token budget exhausted | `Length` |
| The model ended its turn to call tools | `ToolCalls` |
| Content filter | `ContentFilter` |
| A caller stop sequence matched and was removed | `StopSequence(bytes)` |
| Repetition, degenerate output, or any other reason | `Unknown` |

`Unknown` is conservative: an open call at the end of output is malformed,
never truncated.

### Effects of adapter tools

Wire declarations do not state effects, so every adapter-built tool is
`Effects::Mutating` and therefore `Structural`. Declare read-only tools per
request with `ToolRecoveryRequest::effects(name, Effects::ReadOnly)` (MCP
`"effects": {"read_file": "read-only"}`, CLI `--effects read_file=read-only`)
to admit validated, labeled guesses for them. Naming a tool the set does not
declare is an invalid request.

### Templates that open reasoning in the prompt

Some chat templates place the opening `<think>` marker in the prompt, so the
output begins inside a reasoning block. Use
`ReasoningPolicy::Separate { starts_open: true }` (MCP
`"reasoning": "separate-open"`, CLI `--reasoning separate-open`); otherwise
the reasoning text before `</think>` is read as content.

### Ids across rounds

With no id option (Rust `IdPolicy::Turn`, the default; MCP without `ids`; CLI
without `--ids-*`), ids never collide across the turns of a conversation and
the caller threads no state:

- A call keeps the id the model wrote (an `OpenAI` `tool_calls[].id`, a
  Mistral v3 `"id"`, a Mistral v11 `[CALL_ID]`), reported as both `id` and
  `source_id`.
- Every other call is `call_<tag>_<index>`: `<tag>` is the first 12
  hexadecimal digits of the SHA-256 digest of the turn's output bytes
  (`tool_call::turn_id_prefix`; the digest is `ToolCalls::turn` and the
  receipt's source digest) and `<index>` is the call's position. An assigned
  id that another call of the turn already holds gets `_<k>` appended for
  the least free `k`.
- A missing model id (in a dialect whose markup provides for one) or a model
  id an earlier call already holds is numbered so and recorded as a derived
  `tool_call.id.dedup` inference on the call and in the receipt; it never
  demotes the call. DeepSeek-V3.1, DeepSeek-V3, Hermes, Llama 3 JSON and
  Qwen3-Coder XML markup carries no id, so their calls are numbered without an
  inference (Hermes and Llama 3 JSON keep an id a model adds as the source
  id).
- `next_id_start` is the number of calls.

The same output always gets the same ids: recovering one turn twice is the
same turn. Two different outputs share a tag with probability `2^-48`, so
among `n` distinct turns of one conversation the chance that any two share a
tag is at most `n(n-1)/2^49` (about `1.8e-11` for 100 turns, `1.8e-7` for
10,000, and `1.8e-5` for 100,000). A conversation that can repeat a
byte-identical turn and must still number it apart chains the explicit
counter instead.

`IdPolicy::Prefix { prefix, start }` numbers every call
`prefix + (start + i)`, whatever id the model wrote, exactly as before.
`ToolCalls::next_id_start` is the counter after this recovery; pass it as the
next round's `start` (MCP `"ids": {"prefix": "call_", "start": N}`, CLI
`--ids-prefix P --ids-start N`; an omitted prefix is `call_` and an omitted
start is 0). `IdPolicy::Generator` uses a caller generator (a duplicate id
is an invalid request), and `IdPolicy::Source` (MCP `"ids": "source"`, CLI
`--ids-source`) keeps the ids the output carries, deduplicated as
`<id>#<index>` with a recorded `tool_call.id.dedup` inference. Emission
under a policy (`tool_call::emit`) numbers calls exactly as recovery under
that policy does.

### The missing `</parameter>` repair

In Qwen3-Coder XML, a model sometimes omits a `</parameter>` before the next
parameter. When the call names a declared tool, the opener of a parameter
that tool declares always ends the open value: the repair
`tool_call.markup.missing-parameter-close` is grounded in the definition, has
a single reading, and is admitted under `Structural`, so the call is
`Resolved` even on a `Mutating` tool. Its evidence states that the
alternative reading (the value contains that opener as text) is not offered.
The opener of a parameter the tool does not declare is not a boundary: it
stays text of the value. When the call names no declared tool, every opener
is a boundary, recorded as the guess
`tool_call.markup.undeclared-parameter-boundary`. A typed value that is
incomplete at the boundary (`{"a":` before the next opener) is a rejected,
malformed attempt.

### Data versus markup losses

A `markup` loss (closers, framing newlines, fence markers, a closers-only
body) is itemized and never demotes a call. A `data` loss (a dropped unknown
parameter, a dropped repeated occurrence, dropped prose inside a call,
truncation, a dangling opener, an unterminated reasoning block, invalid
UTF-8) makes the call at most `BestGuess`, which a `Mutating` tool never
runs. Each loss on the wire carries its `kind`.

## Surfaces

The Rust API, the MCP tool, and the CLI run the same request builder
(`purrassist_host::tools::recover_tool_calls`); the host test
`tool_call_tools::final_result_matches_core_recover` checks the tool's result
against direct core recovery, and the CLI protocol tests check CLI and MCP
equivalence.

### MCP

The stdio MCP server (`purrassist mcp --root DIRECTORY`) lists
`recover_tool_calls`. Its arguments are `output` (the turn, a string) and
`tools` (the declarations) plus optional `tools_format` (`openai`, the
default, `mcp`, or `anthropic`), `dialects` (built-in dialect ids; default
all of them), `stop` (`stop`, `length`, `tool_calls`, `content_filter`,
`unknown`, or `{"stop_sequence": STRING}`), `tool_choice` (an `OpenAI` or
Anthropic form), `policy` (`per-tool`, `structural`, `tolerant`), `effects`,
`reasoning` (`separate`, `separate-open`, `content`), `ambiguity` (`report`,
`reject`), `ids` (omitted for the default turn-scoped ids, `"source"`, or
`{"prefix", "start"}`; see [Ids across rounds](#ids-across-rounds)), `emit` (`none`,
`openai`, or a dialect id), and `effort`. The result is in
`structuredContent` (and as text in `content`):

```json
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"recover_tool_calls","arguments":{
  "output":"<tool_call>\n<function=write_file>\n<parameter=path>\nnotes.txt\n<parameter=content>\nhello\n</parameter>\n</function>\n</tool_call>",
  "tools":[{"type":"function","function":{"name":"write_file","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}}}],
  "emit":"openai"}}}
```

Malformed options and invalid declarations are tool errors; exhaustion and
cancellation are an `Unresolved` result whose receipt names the stop.

### CLI

`purrassist recover-tool-calls` reads the turn as raw bytes from a file or
stdin (invalid UTF-8 is retained and recorded as a loss) and prints the same
result:

```sh
purrassist recover-tool-calls --tools tools.json --emit openai turn.txt
```

With `tools.json` holding the `tools` array above and `turn.txt` holding the
`output` above (a missing `</parameter>` before `<parameter=content>`), the
result is `"status": "resolved"`. The call is `write_file` with arguments
`{"path":"notes.txt","content":"hello"}`, effects `mutating`, policy
`structural`, outcome `resolved`, and its repairs list four
`tool_call.xml.framing-newline` markup losses and one
`tool_call.markup.missing-parameter-close`. The turn's `finish` is `calls`,
`openai_finish_reason` is `tool_calls`, and `emitted.output` is:

```json
[{"id":"call_a7e052aa565b_0","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"notes.txt\",\"content\":\"hello\"}"}}]
```

`a7e052aa565b` is the first 12 hexadecimal digits of the SHA-256 digest of
`turn.txt` (with no trailing newline); another turn gets another tag.

The exit status is 0 for resolved and best-guess results, 3 for ambiguous,
4 for unresolved, and 1 for invocation errors. `--help` lists every option;
the command never executes a call.
