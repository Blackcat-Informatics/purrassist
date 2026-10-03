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
  `Ambiguous`, with the competing readings in `ToolCall::alternatives`. A
  non-repeatable parameter given more than once is never a silent last-wins:
  when the declared type, enumeration or schema accepts only one of the
  values, that reading is chosen on evidence (a `BestGuess`); when every
  value is valid the call is `Ambiguous`, and either way the selected
  reading records `tool_call.parameter.repeated-choice`, naming the
  occurrence its `arguments` keep and why (the blocked alternative, or that
  it is merely the first-ranked reading), citing the alternatives.
  *Proved by* `tool_call_rules::data_loss_demotes_to_best_guess`,
  `tool_call_rules::markup_loss_does_not_demote`,
  `tool_call_rules::repeated_scalar_reports_both_readings`,
  `tool_call_goldens::repeated_scalar_choice_is_labelled`, and
  `tool_call_rules::per_call_alternatives_exposed`.
- **Envelope repairs keep parity with argument repairs.** The JSON call
  envelope of Hermes, Llama 3 JSON, Mistral v3, Anthropic, `OpenAI` and bare
  JSON (`json-envelope` grammars) tolerates the JSON syntax slips the `json`
  codec repairs inside arguments: a trailing comma, single quotes (a
  Python-repr dict), bare keys, a missing comma or colon, a comment, and raw
  control characters beside the quotes of the name string. Each is recorded
  under the same `json.syntax.*` rule, cost and policy admission as the
  same slip inside arguments, and like it is a non-guess single-reading
  repair: it never demotes a call, on a `Mutating` tool either, and the
  name read is the declared name itself (a control character is deleted and
  kept in the edit's original bytes, never matched as a near-miss name).
  Literal case (`True`, `None`) only occurs in argument values. The
  tolerance never makes a call of prose: an object without a call's members
  stays content, with no rejected attempt. Mistral v11, DeepSeek, Kimi-K2 and
  Harmony have no JSON envelope. *Proved by*
  `tool_call_goldens::envelope_json_syntax_is_tolerated` (every slip on all
  six dialects, a Python-repr dict, the prose negative, and the
  mutating-tool parity of the same slip in the envelope and in the
  arguments) and the correction corpus's `envelope` mutations (384 cases,
  0 incorrect; [MATCHING-QUALITY.md](../MATCHING-QUALITY.md)).
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
- **Call markup is never silent content.** Call markup that no dialect of
  the allow-list reads (a family not built in yet: Pythonic call lists,
  Gemma 3 `` ```tool_code `` blocks, GLM-4.5 `<arg_key>` calls, and the
  stripped renderings of built-in dialects no grammar reads; or a built-in
  dialect the allow-list leaves out, `[TOOL_CALLS]` under a Hermes-only
  request) is reported as a `RejectedCall` with
  `Rejection::UnsupportedDialect { family }` (wire `reason.code`
  `unsupported_dialect`, with `family`, `built_in`, and a `detail` saying
  "not built in yet" or "built in but not in the allow-list"), a
  `tool_call.markup.unparsed-dialect` evidence entry in its repairs and the
  receipt, and the finish `Unresolved`. Its bytes also stay content, and a
  span a call, a rejected attempt or reasoning already holds is never
  reported again. The detector (`detect_unparsed`) is linear and never
  parses; Pythonic lists need a call-shaped `NAME(` with a keyword argument
  at the start of the turn or a line, so prose with parentheses, math and
  Python code blocks are not flagged. *Proved by*
  `tool_call_goldens::unparsed_markup_is_reported` (one golden per family,
  five built-in dialects outside the allow-list, and negatives), the CLI
  test `tool_calls_cli::unparsed_markup_is_reported_via_cli`, and the host
  test `tool_call_tools::unparsed_markup_over_mcp`.
- **Measured.** The 13,815-case correction evaluation records zero incorrect
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
- **Schema-typed text.** A parameter whose schema the typed model does not
  express (pydantic `Optional[int]`'s `anyOf` with `null`, a `$ref` to
  `$defs` or Draft-07 `definitions`, `oneOf`, a type array) arrives in
  Qwen3-Coder XML as text, which the exact schema types itself: each
  verbatim string is also read as the JSON value it spells (a number with
  its exact spelling, a boolean, `null`, an array or object written inline,
  and nested `<parameter=...>` elements read as an object's members), and
  the exact validator chooses. The verbatim string is kept wherever it
  validates, so `12345` under `anyOf[string, integer]` or `Optional[str]`
  stays a string as for a declared string; the JSON reading is taken where
  it removes the schema's violations at that value (`5` under
  `Optional[int]` is `5`); with no validating reading the text stays
  verbatim with its `schema_validation` diagnosis. Each decision is the
  definition-grounded, non-guess `tool_call.value.schema-typed` (like
  `tool_call.value.decode-typed` for a declared type), so the call is
  `Resolved` under `Structural` and on a mutating tool, and the keyword that
  drove it is reported `used`. `$ref` recursion is resolved by the governed
  validator (a reference recurring without progress is the schema's
  existing invalid-input outcome). There is no cap on how many values are
  typed: they are decided in batched rounds (every value the violations
  implicate is tried at once, and only values interacting through a shared
  ancestor are tried one at a time), so a call costs a handful of
  validations whatever its size, each charged to the request's governor;
  running out is the request's explicit `BudgetExceeded`, never a quiet
  verbatim fallback. JSON arguments arrive typed and are never retyped. *Proved by* `tool_call_goldens::unmodeled_schemas_type_xml_text`
  (`Optional[int]`, `Optional[str]` with numeric text, `anyOf[string,
  integer]`, `$ref` scalar, `$ref` object inline and as nested parameters,
  `oneOf` constants, a recursive `$ref`, a type array, an unmatched value,
  and each declaration rendered back byte for byte),
  `tool_call_recover::anyof_ref_enforced_on_recovered_call`, and the
  correction corpus's adapter cases.
- **Which bytes.** The adapters read exactly the text they are given, and
  the render-back (`ToolDeclaration::text`, `parameters_text`) and the
  schema digest (each call's schema validation `constraint`, which binds
  the parameters text's exact bytes) refer to that text:
  - Rust: the `&str` passed to `ToolSet::from_*`.
  - CLI `recover-tool-calls --tools FILE`: the file's bytes, passed through
    unparsed (whitespace, member order, number spellings and string
    escapes as written).
  - MCP `recover_tool_calls` with `tools` as a string holding the
    declaration JSON: that string's characters, byte for byte.
  - MCP `recover_tool_calls` with `tools` as a structured value: JSON-RPC
    has already parsed it, so the adapters read its compact
    re-serialization, which keeps member order, duplicate members and
    number spellings (`1.50e3` stays `1.50e3`) but not the caller's
    whitespace or string escape spellings. Pass the string form when the
    digests must match the caller's own bytes.
- **Proved by** `tool_adapters::openai_render_back_is_byte_exact`,
  `tool_adapters::mcp_render_back_is_byte_exact`,
  `tool_adapters::anthropic_render_back_is_byte_exact`,
  `tool_adapters::draft07_mcp_tools_adapt_losslessly`,
  `tool_adapters::adapter_schema_compiled_from_exact_text_rejects_violation`,
  `tool_adapters::unmodeled_keywords_are_listed`,
  `tool_adapters::invalid_declaration_is_hard_error`, and
  `tool_call_recover::anyof_ref_enforced_on_recovered_call`, the CLI test
  `tool_calls_cli::tools_file_bytes_reach_the_adapter_exactly` (a
  declaration file with unusual spacing, member order and `1.50e3`: the
  receipt's schema digest is that of the file's parameters bytes), and the
  host test `tool_call_tools::raw_declaration_string_is_read_byte_exact`
  (the string form byte-exact; the structured form compact with member
  order and number spellings kept).

### C6. One caller-chosen policy, stop reason as input

- **One policy for every tool.** `PolicySelection::All(policy)` applies one
  `RepairPolicy` to every tool, whatever each declares
  (MCP `"policy": "structural" | "tolerant"`, CLI `--policy`). The default,
  `PerTool`, uses each tool's explicit policy or the one its effects select.
  *Proved by* `tool_call_rules::policy_selection_all_overrides_per_tool` and
  `tool_call_recover::single_policy_override_applies_to_all`.
- **The stop reason is an input.** The caller passes `GenerationStop`; only
  `Length` reads an open call as truncation, which the turn reports as
  `Finish::TruncatedWithCalls` when a call may run and `Finish::Truncated`
  otherwise. A length stop is never reported as a clean finish: under it
  the `finish_reason` is `"length"` whatever the finish, `Unresolved`
  included (a failed tool choice, a rejected attempt beside a usable call),
  and the usable calls are still emitted. Under any other stop the finish
  is `Calls` only when a call may run; calls of which none may (each a
  guess on a mutating tool) finish `Unresolved`, and `finish_reason` is
  `"tool_calls"` exactly when the `OpenAI` emission holds a call and
  `"stop"` otherwise
  ([Reading a result](#reading-a-result-status-finish-and-verdict)).
  *Proved by* `tool_call_recover::length_vs_stop_truncation`,
  `tool_call_recover::unknown_stop_is_conservative`,
  `tool_call_recover::stop_sequence_consumed_closer`, and
  `tool_call_roundtrip::finish_reason_mapping`,
  `tool_call_goldens::finish_and_finish_reason_agree_with_the_verdict`, the
  goldens `length_stop_required_choice_on_content` and
  `length_stop_rejected_attempt_beside_usable_call`, and the tool-call fuzz
  probe's `check_verdict`.

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

`ToolCalls::openai_finish_reason()` (wire `openai_finish_reason`) is
`"length"` under a length stop, otherwise `"tool_calls"` exactly when the
`OpenAI` emission holds a call, and `"stop"`, so it never contradicts the
emitted array or hides a budget cut, but the `OpenAI` wire format has no
value for "something else was wrong": `"stop"`, `"tool_calls"` and
`"length"` all hide rejected and unresolved calls. Read `ToolCalls::finish` (wire `finish`) and
`ToolCalls::rejected` (wire `rejected`) before deciding that a turn
completed cleanly, and branch on `verdict` (not `status`) for whether
anything can run ([Reading a result](#reading-a-result-status-finish-and-verdict)).
A rejected attempt's `dialect` is the dialect whose
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
the reasoning text before `</think>` is read as content. The block ends
exactly after its closer (`</think>`, `[/THINK]`, Harmony's `<|end|>`),
never at a closer of a block that only exists after its own opener
(Anthropic's thinking blocks, which close with `"}`, JSON a call's arguments
also contain): under the default allow-list a call inside the block is never
swallowed. Without a closer, the block is closed at the first call opener
(`tool_call.reasoning.unclosed`, a guess) and the calls are recovered; with
one, the calls inside are offered as `embedded` alternatives
(`tool_call.reasoning.embedded-call`). *Proved by*
`tool_call_goldens::reasoning_never_swallows_calls` (Hermes, Mistral v3,
Llama 3, bare JSON, Qwen3-Coder XML and Anthropic calls, unclosed and
closed, under the default allow-list, plus the `separate` and `as_content`
modes) and the tool-call fuzz probe's reasoning invariant (a call the turn
recovers with reasoning as content never vanishes under a reasoning mode).

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
  demotes the call. DeepSeek-V3.1, DeepSeek-V3, Harmony, Hermes, Llama 3
  JSON, bare JSON and Qwen3-Coder XML markup carries no id, so their calls
  are numbered without an inference (Hermes, Llama 3 JSON and bare JSON keep
  an id a model adds as the source id: `optional-id`).
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
UTF-8 inside the call) makes the call at most `BestGuess`, which a
`Mutating` tool never runs. Each loss on the wire carries its `kind`.

### Invalid UTF-8

The output's bytes are retained exactly, and every maximal run of bytes
that are not well-formed UTF-8 is a `tool_call.input.invalid-utf8` data loss
of the turn. Where the bytes fall decides what else happens:

- **Outside every call** (content): the bytes stay content. A call
  elsewhere in the turn keeps its own outcome; the operation is at most a
  best guess.
- **Inside an argument value, an id, or a parameter name**, in every
  dialect and codec (`json`, `json-args`, `xml-params`, `raw`): the call is
  kept. Each maximal malformed subpart is read as U+FFFD (the Unicode
  Standard's substitution of maximal subparts), and each run is recorded on
  the call as `tool_call.value.invalid-utf8`: a data loss whose evidence span
  is the run's exact byte range and whose original retains the bytes
  (`\xE9` on the wire). The value keeps everything else it decodes, and
  offsets after the run are unchanged. Hermes
  `{"path": "caf\xE9.txt"}` reads as `{"path": "caf\u{FFFD}.txt"}`. The
  call is `BestGuess`, never `Resolved`. A read-only tool's call is usable
  when guesses are accepted, and a `Mutating` tool's call is never usable.
  This is never a silent replacement.
- **Inside a tool name**: every built-in name grammar is an ASCII identifier
  (the providers' own name rule), so no name can hold the bytes and no
  declared tool could match them. The attempt is a rejected attempt with
  reason `invalid_utf8` at the first malformed byte's offset, its bytes
  retained, never a crash or a silently dropped call. A custom grammar whose
  name capture admits such bytes reads them as U+FFFD (recorded as above),
  and name resolution then decides: no exact match is possible, so only a
  near-miss match under `Tolerant` (a guess) can name a tool.

*Proved by* `tool_call_goldens::invalid_utf8_in_arguments_keeps_the_call`
(Hermes, Kimi-K2 and `OpenAI` stringified arguments through `json`,
Qwen3-Coder XML through `xml-params`, Harmony's python body through `raw`,
several runs, the `Mutating` guard, tool and parameter names),
`grammar_codecs::invalid_utf8_in_argument_is_replaced_and_recorded`, and
the codecs' unit tests.

## Surfaces

The Rust API, the MCP tool, and the CLI run the same request builder
(`purrassist_host::tools::recover_tool_calls`); the host test
`tool_call_tools::final_result_matches_core_recover` checks the tool's result
against direct core recovery, and the CLI protocol tests check CLI and MCP
equivalence.

### MCP

The stdio MCP server (`purrassist mcp --root DIRECTORY`) lists
`recover_tool_calls`. Its arguments are `output` (the turn, a string) and
`tools` (the declarations: the array or object itself, or a string holding
the raw declaration JSON, which the adapters read byte for byte; see
[C5](#c5-lossless-declaration-adapters)) plus optional `tools_format` (`openai`, the
default, `mcp`, or `anthropic`), `dialects` (built-in dialect ids; default
all of them), `stop` (`stop`, `length`, `tool_calls`, `content_filter`,
`unknown`, or `{"stop_sequence": STRING}`), `tool_choice` (an `OpenAI` or
Anthropic form), `policy` (`per-tool`, `structural`, `tolerant`), `effects`,
`reasoning` (`separate`, `separate-open`, `content`), `ambiguity` (`report`,
`reject`), `ids` (omitted for the default turn-scoped ids, `"source"`, or
`{"prefix", "start"}`; see [Ids across rounds](#ids-across-rounds)), `emit` (`none`,
`openai`, or a dialect id), and `effort`. The result is in
`structuredContent` (and as text in `content`); branch on `value.verdict`
for what can run, not on `status` (see
[Reading a result](#reading-a-result-status-finish-and-verdict)):

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

The command's exit code is `tool_call::exit_code` of the result: the
selected reading's `value.verdict` mapped to 0 (`run`, `no_calls`), 3
(`ambiguous`) or 4 (`nothing_usable`), 3 for an `Ambiguous` operation and 4
for an `Unresolved` one (see [Reading a result](#reading-a-result-status-finish-and-verdict)),
so the exit code and the printed JSON can never disagree. It exits 0 when
the tool-choice check passed and at least one call is usable
(`usable_accepting_guesses`), even if other attempts in the turn were
rejected or are unparsed call markup (the JSON itemizes them, so a script
keyed on the exit code never discards good calls because of one stray
opener), or the turn is clean content (finish `content_only`). It exits 3
when nothing is usable and a call or the operation is ambiguous, and 4 when
nothing is usable (unresolved calls, only rejected or unparsed call markup,
a `truncated` turn, only guesses that may not be used, such as a mutating
tool's, or an `Unresolved` operation: an exhausted budget, invalid input) or
the tool-choice check failed, even with usable calls: a violated tool choice
is a contract failure the caller must see. Invocation errors exit 1. The
printed JSON is the same whatever the exit code. `--help` lists every
option; the command never executes a call. *Proved by* `tool_calls_cli::exit_code_reflects_what_is_usable`
(resolved calls, usable calls beside a stray rejected opener, clean
content, `truncated_with_calls`, `truncated`, a hallucinated name, a missing
required argument, each violated tool choice, including `none` with a usable
call, unparsed markup, an ambiguous call),
`tool_calls_cli::busy_turn_resolves_every_call_by_default`, and
`tool_calls_cli::exit_code_without_a_value` (rejected ambiguity, an exhausted
budget).

The `recover_tool_calls` MCP tool has no exit code. Like every other tool in
the registry, a completed recovery is a successful tool result (`isError`
false) whatever it recovered, and `isError` is true only when the request
itself is invalid. Its `value.verdict` is the field the CLI's exit code
summarizes; `status` is not (see below).

### Reading a result: status, finish and verdict

`status` and the usability of the calls are different questions, answered
by different fields:

- **`status`** is the certainty of the recovered *reading of the turn*,
  with the same meaning as in every other operation's result: `resolved`
  (one reading of the output), `best_guess` (a reading chosen among others,
  or carrying a guess or a data loss), `ambiguous` (competing readings,
  none selected: `value` is null and each reading is in `alternatives`), or
  `unresolved` (no reading: an exhausted budget, invalid input, rejected
  ambiguity). It is **not** a verdict on the calls: a turn read with full
  certainty may hold nothing to run. A turn whose only call names an
  undeclared tool, misses a required argument, or violates the tool choice
  is `status: "resolved"` (that is unambiguously what the model wrote) with
  `value.finish: "unresolved"` and `value.verdict: "nothing_usable"`.
- **`value.verdict`** is the usability verdict, one field to branch on
  (`ToolCalls::verdict`). `value.usable_calls` counts the calls that may run
  as is (`Resolved`, `ToolCalls::usable`), and
  `value.usable_calls_accepting_guesses` also counts best guesses on
  read-only tools (`ToolCalls::usable_accepting_guesses`).
- **`value.finish`** classifies the turn: `calls` or
  `truncated_with_calls` only when at least one call may run (so they imply
  `verdict: "run"`), `content_only`, `truncated`, or `unresolved` when
  something is wrong somewhere: a rejected attempt (even beside calls that
  run), an unresolved or ambiguous call, a failed tool-choice check, or
  calls of which none may run (each a guess on a mutating tool). Per-call
  `outcome` and `value.tool_choice` give the reasons.
- **`value.openai_finish_reason`** is what an `OpenAI`-compatible relay
  sends: `"length"` whenever the caller's stop reason is a length stop
  (whatever the finish, so a relay never hides the budget cut; the usable
  calls are still emitted), otherwise `"tool_calls"` exactly when the
  `OpenAI` emission holds a call, and `"stop"`. The
  emission prints exactly `ToolCalls::emitted_calls`: the calls usable
  accepting read-only guesses, and none when the tool-choice check failed;
  so a relay never sends `"tool_calls"` with no tool call, nor `"stop"`
  beside one.

A failed tool-choice check is a contract failure of the whole turn: even
beside a call that would run on its own (the forced tool called, plus an
extra call), the verdict is `nothing_usable`, the finish `unresolved`, the
emission empty (the call skipped as `tool_choice_failed`), and
`finish_reason` `"stop"` (`"length"` under a length stop). `usable_calls` still counts each call's own
usability.

| `status` | `value.verdict` | `value.finish` | `openai_finish_reason` | Exit | What the caller should do |
| --- | --- | --- | --- | --- | --- |
| `resolved`, `best_guess` | `run` | `calls`, `truncated_with_calls`, or `unresolved` (a rejected or unresolved attempt beside the calls that run) | `"tool_calls"` (`"length"` under a length stop) | 0 | Run the usable calls: `usable_calls` (strict) or, accepting read-only guesses, `usable_calls_accepting_guesses` (what the emission prints); report `rejected` |
| `resolved`, `best_guess` | `no_calls` | `content_only` | `"stop"` | 0 | Use the content; nothing to run |
| `resolved`, `best_guess` | `ambiguous` | `unresolved` | `"stop"` (`"length"` under a length stop) | 3 | Choose among the ambiguous call's `alternatives`, or re-ask |
| `resolved`, `best_guess` | `nothing_usable` | `unresolved` (calls that may not run, a failed tool choice, unresolved calls, rejected attempts) or `truncated` | `"stop"` (`"length"` under a length stop) | 4 | Run nothing; re-ask or report the per-call reasons and the tool-choice diagnostics |
| `ambiguous` | none (`value` is null; each of `alternatives` carries its own `verdict`) | none | none | 3 | Choose a reading, or re-ask |
| `unresolved` | none (`value` is null) | none | none | 4 | Read `receipt.termination`; run nothing |

*Proved by* `tool_call_goldens::verdict_and_exit_code_agree_over_goldens`
(over every golden, pinned and under the default allow-list: the wire
`verdict` and counts are `ToolCalls::verdict` and the usable-call counts,
and the exit code agrees with the rule above, restated independently),
`tool_call_goldens::finish_and_finish_reason_agree_with_the_verdict` (the
finish, the emission and `finish_reason` agree with the verdict as the
table states, over every golden), the goldens
`mutating_guess_invalid_utf8_finish`, `mutating_guess_near_miss_name_finish`,
`mutating_guess_beside_usable_call_finish`,
`mutating_guess_cut_by_length_finish` and
`forced_tool_choice_violated_beside_a_usable_call`,
`length_stop_required_choice_on_content` and
`length_stop_rejected_attempt_beside_usable_call` (each with an `OpenAI`
emission), and the CLI tests above.
