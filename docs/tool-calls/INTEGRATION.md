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
- **A rejected attempt keeps its whole call markup, and only markup.** A
  parser's rejected attempt (`RejectedCall::span`, `raw`) is decided in
  three steps:
  1. *Where its grammar stopped reading*: the end of the last token it read
     whole, which is where the furthest token it could not read begins. A
     literal it matched only in part is left out (it may be prose or begin
     the next opener), so `reason.at` (the first byte no reading could
     consume) lies inside the span, at its end, or past it only inside
     such a partial literal (read up to `at`, perhaps through one repaired
     byte): `<function=f<oops` is rejected over `<function=f` (`0..11`)
     with `at` 12, past the `<` that began a literal.
  2. *Never inside a literal*: when the bytes there complete a literal
     spelling of the dialect that began inside the span (the grammar read a
     closer variant that is a prefix of `</function>`), the span runs to
     its end: `pre <function=f><function=f>…</function> post` keeps
     `</function>` whole and leaves ` post` as content.
  3. *To its closer*: it runs on to the end literal of the `call`
     alternative it opened (of any alternative sharing its opener, as an
     Anthropic block and a streaming event share `{`; the wrapper or
     section closer in any declared or confusable spelling, at nesting
     depth one: another opener of that
     alternative opens a level, unless the alternative chains several calls
     that each open with it, as a section-less Kimi-K2 attempt does; a JSON
     object or array call nests by its brackets outside strings, and only
     over bytes JSON reads: brackets pair by kind (T6), so a `}` closes
     the innermost open `{` and any bracket a slip left open inside it,
     and a `}` or `]` with none of its kind open is a slip inside the
     markup that closes nothing (a stray `}` in a Mistral list never ends
     the list's markup before its `]`); a bracket closing nothing the scan
     opened, with none open, closes a structure around the attempt and
     ends the scan, and past
     where its grammar stopped, prose, as defined below, ends it too, so a
     bracket after it closes nothing the attempt read; issue #3 review
     finding F16: the tail of a cut special token, a word followed by `|` or `｜`
     (`t_begin|>`), is markup, not prose, and a call's member past the
     failure (`"name"`, then `:`) is the attempt's call body: outside any
     bracket its object's `{` was lost and the scan closes it by its `}`,
     and a body left open runs to its last JSON byte before prose, or to
     the next segment or the end of output that cuts it; a call body after
     the attempt's markup, however that markup ends, is covered the same
     way when only markup debris comes between (whitespace, separators, the
     `:` of a member key the attempt completed, as the `"function"` key a
     cut `OpenAI` arguments string runs into, stray punctuation such as the
     `>` of a cut tag, or JSON text holding no call's member; never prose,
     read with strings opening and closing where the markup ended), and a
     member's spelling begins a body wherever it is written, since a slip
     that broke a string before it leaves no telling which quote opens a
     string, so no call body is left as assistant text) when that
     closer
     comes before the next call opening the same alternative, a
     reasoning block, or reported unparsed markup, plus any repeated
     closers right after it. A call or attempt inside the broken wrapper (a
     bare JSON object read from its body, an inner element's own failure)
     is kept, and the wrapper's rejected span covers it; its brackets nest
     with the attempt's, and when it closes the bracket the attempt left
     open (a Mistral group's last call holds the array's `]`), no later
     bracket closes the attempt, which ends as unclosed. Without that
     closer it runs to the end of the last literal before the next segment
     that ends a call of its dialect: a chained call's own closer
     (Kimi-K2's `<|tool_call_end|>`, DeepSeek's `<｜tool▁call▁end｜>`) or
     another alternative's (`</function>` after a broken `<tool_call>`).
     Either literal may be written with one slip, as a call's closer may
     (`</tool_cal>` ends a broken `<tool_call>` attempt as it would end a
     call, when it misspells no other closer of the dialect).

  Without such a literal, an attempt that left a JSON value open (its
  bytes up to where its grammar stopped reading open more brackets than
  they close, outside strings; bytes a resync read on past the failure are
  not parsed as JSON and open nothing) runs to the bracket that balances
  it, brackets paired by kind as in step 3 (T6: in `{"a": [1, 2} done.`
  the `}` balances the object, closing the `[` a slip left open, and is
  the attempt's, not content), unless prose comes first: a bare word
  (beginning at a letter or `_` that no letter, digit or `_` precedes, as
  `_dir`; not `true`, `false` or `null`) outside a string followed by another word or
  sentence punctuation rather than JSON punctuation is no JSON structure,
  and the attempt then ends where its grammar stopped reading.

  Otherwise the span ends at step 2 and everything after it stays content
  (`Sure, I could call <function=f but actually no tool is needed. The
  answer is 42.` keeps `<function=f` as the attempt and the answer as
  content), so no call token is left as assistant text and no prose is
  taken. Unparsed markup (`unsupported_dialect`) is not extended: its bytes
  stay content. The literals are derived per grammar when it is compiled
  (`CompiledGrammar::call_ends`, with each alternative's `closers` and
  `call_closers`, and `CompiledGrammar::call_literals`). *Proved by* the
  negative corpus (`tool_call_negative`, wrapped and unwrapped, each
  asserting that content holds no call markup, and
  `tool_call_negative::stray_closers_after_rejected_attempt`),
  `tool_call_recover::rejected_wrapper_covers_the_call_recovered_inside_it`,
  `tool_call_recover::rejected_span_never_splits_a_character_a_class_cannot_consume`
  (with and without the closing brace), the goldens (which record each
  reading's content beside its rejected spans, every one also under the
  default allow-list), among them `rejected_opener_then_prose_stays_content`,
  `rejected_opener_then_prose_then_call`,
  `rejected_attempt_with_closer_covers_its_markup`,
  `rejected_attempt_stray_closers_absorbed`,
  `kimi_chained_rejected_attempt_runs_to_section_closer`,
  `kimi_call_closer_ends_rejected_attempt`,
  `deepseek31_call_closer_ends_rejected_attempt`,
  `deepseek3_call_closer_ends_rejected_attempt`,
  `kimi_sectionless_rejected_call_before_sibling`,
  `hermes_rejected_attempt_misspelled_closer`,
  `llama3_rejection_stops_before_prose`,
  `llama3_rejection_stops_at_enclosing_bracket`,
  `openai_rejection_covers_call_body_after_completed_key`,
  `hermes_rejection_covers_call_body_after_stray_byte`,
  `mistral3_rejection_covers_call_body_after_its_bracket`,
  `llama3_rejection_covers_call_member_after_broken_string`,
  `mistral3_group_bad_good_good_then_bracket`,
  `mistral3_group_bad_good_good_then_array`,
  `anthropic_rejection_shares_opener_closers`,
  `deepseek31_rejection_balances_only_read_brackets`,
  `mistral3_rejected_stray_brace_in_list`,
  `mistral3_rejected_mismatched_bracket_closes_object` and
  `rejected_span_completes_literal_bare`, `_newlines` and `_wrapped`, and
  four `fuzz-tool-call` invariants: content after a rejected attempt that
  does not end with a literal ending a call of its dialect holds none; an
  attempt runs past where its grammar failed only to such a literal or to
  complete a literal it began; a rejected span never ends inside a literal
  it began; and `at` lies inside the span, at its end, or past it by at
  most the longest literal spelling (inside a partial literal).
- **A malformed call does not discard the calls around it.** In an attempt
  that reads several calls (a production with `next`: a Kimi-K2 or DeepSeek
  v3/v3.1 section, a section-less chain of Kimi-K2 or DeepSeek calls, a
  Mistral v3 list, chained Mistral v11 calls), a call the grammar could not
  read never costs the calls around it, whichever call of the attempt it
  is. The attempt *resynchronizes* past it: beside the repairs the failure
  proposes (which cost less and win where they read the call), it scans on
  for a literal that ends a call of its alternative (the call's own closer,
  a list separator or closer, the section closer), never inside a JSON
  string of the skipped call (a multi-byte special token at any bracket
  depth, a JSON element's `}` at the call's bracket depth; an opener inside
  such a string is the string's text, so no call is read from it and no
  attempt opened there reads one), or for the next call's opener (a
  multi-byte opener,
  or a JSON element's `{` where the failing element's member was due, so an
  element left open ends there), and reads on, so later calls of the same
  attempt are read and the malformed call is a rejected part of the
  attempt. The split is recorded as `tool_call.markup.group-split` (a markup
  repair, single reading, so it does not demote and is admitted under
  `Structural`): on the last call read before the malformed one, at the
  boundary, or, when the malformed call is the attempt's first, on the
  first call read after it, which then also carries any token read as
  omitted at the attempt's start (a section opener the model left out),
  since that token opens every call of the attempt. A malformed first
  call's rejected part runs from the attempt's start (the group opener
  included, as a first call's span always does). The scan gives up at
  another attempt's opener (other than the next call's opener it reads)
  and at the end of output; the attempt then ends at its failure, the
  calls before it recovered. An attempt that read no call at all (every call malformed) is
  one rejected attempt over all the markup it read, never one rejection
  per call nor overlapping rejections; a resynchronization at a single byte
  (a JSON bracket or separator, which prose writes too) extends no such
  rejection, so prose after the failure stays content. Mistral v11 calls each carry their
  own `[TOOL_CALLS]` opener and no wrapper, so a malformed v11 call is its
  own rejected attempt and the calls after it are read by their own
  attempts, needing no repair. The same holds under a length stop.
  *Proved by* the goldens `kimi_group_good_bad_good`,
  `kimi_sectionless_group_good_bad_good`,
  `kimi_chained_rejected_attempt_runs_to_section_closer`,
  `kimi_group_split_without_resync`, `deepseek3_group_good_bad_good`,
  `deepseek31_group_good_bad_good`, `mistral3_group_good_bad_good`,
  `mistral3_group_good_bad_good_length_stop`,
  `mistral11_group_good_bad_good`, the `*_group_bad_good_good`,
  `*_group_bad_good_bad_good`, `*_group_bad_bad_good` and `*_group_all_bad`
  goldens of every dialect with `next` (Mistral v3, Kimi-K2 with and
  without a section, DeepSeek v3 and v3.1, Mistral v11),
  `kimi_sectionless_rejected_call_before_sibling`,
  `kimi_group_bad_call_without_closer`,
  `kimi_group_bad_first_call_without_closer`,
  `kimi_group_bad_call_unbalanced_json`,
  `mistral3_group_bad_element_without_closer`,
  `kimi_resync_ignores_closer_inside_string` and
  `deepseek31_unclosed_section_then_section`, each also under the default
  allow-list, and the `tool_call.markup.group-split` rows of the correction
  evaluation (`middle`, `first`, `middle-unclosed`, `unread-arguments` and
  `unread-arguments-first` variants).
- **Body evidence decides a shared opener.** When dialects share an opener
  (`[TOOL_CALLS]` for Mistral v3 and v11, `<tool_call>` for Hermes and
  Qwen3-Coder XML) and one dialect's attempt fails at the body's first
  bytes while another's reads on and reads a whole call there, the failed
  attempt's rejection is recorded (it names the dialect among the opener's
  candidates) but is not a reading of the opener: a reading that rejected
  the opener there and read the bytes after it as other calls (Mistral v3
  list elements as bare JSON) would rest on strictly less grammar evidence
  than the attempt that read the list. The engine holds such a rejection
  until every attempt at the opener has ended. When the attempt that read
  further reads no whole call either, both rejections stay readings (an
  inner well-formed object inside a broken wrapper is still recovered).
  The same holds when the attempt that read further ends after the
  rejected one (it resynchronized past a malformed first call). The
  rejected dialect stays among the opener's candidate dialects: it did
  attempt the opener. Among rejected attempts that read equally far, the
  one whose dialect's opener is written in its canonical spelling names the
  attempt (a DeepSeek attempt that read Kimi-K2's `<|tool_call_begin|>`
  through a confusable spelling does not). *Proved by* the
  `mistral3_group_good_bad_good` and `mistral3_group_bad_good_good` goldens
  under the default allow-list,
  `kimi_sectionless_rejected_call_before_sibling`,
  `kimi_sectionless_group_all_bad`,
  `kimi_sectionless_bad_call_canonical_attribution` and
  `tool_call_recover::rejected_wrapper_covers_the_call_recovered_inside_it`.
- **A written token is never read as omitted.** A token a grammar may read
  as omitted (`insert("...")`, recorded as `tool_call.markup.missing-token`)
  is never inserted where it is written: Mistral v11's `[ARGS]` after a
  name. Nor is it inserted before a value where the grammar reads other
  written markup at that position instead (v11's `[CALL_ID]`, the optional
  segment before `[ARGS]`): the value would read nothing, and the markup
  would be left as text. A malformed arguments value after a written `[ARGS]` goes through
  the json codec's repairs; a JSON arguments value no repair reads makes
  the attempt a rejected attempt (in every dialect), failing where the
  value stopped decoding, and its markup runs to its closer, the next
  opener, or the bracket that balances the value it opened, never into the
  text after it. *Proved by* the goldens `mistral11_args_written_repairable`,
  `mistral11_args_written_unreadable`,
  `mistral11_args_written_unreadable_between_calls`,
  `mistral11_call_id_written_args_cut`,
  `mistral11_group_good_bad_good`, `mistral11_prose_after_unclosed_arguments`,
  `mistral3_prose_after_unclosed_arguments`, `hermes_stray_word_in_arguments`,
  `mistral11_bare_key_with_colon_kept`, `mistral3_rejection_stops_before_prose`
  and `mistral11_bracket_balance_stops_at_prose`, each also under the
  default allow-list, and the `json.syntax.bare-key` rows of the correction
  evaluation.
- **No parameter is invented from text.** In tool-call arguments a bare
  (unquoted) word is an object key only when its `:` follows it: `{path:
  "a"}` keeps its key (`json.syntax.bare-key`), but a stray word
  (`{"a": "y" oops}`) or prose after an unclosed arguments object (`{"path":
  "a.txt" oops. Sorry, my answer is 42`) is never read as a key with a
  missing value: the value is unreadable where the word begins, the
  attempt is rejected there, and the prose stays content. Issue #3's hard
  limits: no parameter name from positional text, no trailing free text as
  an argument.
- **A misspelled closer still closes its call.** A closer token one edit
  away from exactly one closer of the grammar (one byte inserted, deleted or
  substituted, or two adjacent bytes swapped, between the closer's
  delimiters, which it keeps: `</functio>`, `</paramete>`, `</tool_cal>`,
  `<|tool_call_ed|>`, `</fucntion>`) is read as that closer
  (`tool_call.markup.closer-misspelled`, the original bytes recorded; a
  markup repair, single reading, so it does not demote, like the other
  closer repairs), and the prose after it stays content. A token one edit
  away from two closers (DeepSeek's `<｜tool▁callz▁end｜>`, one edit from
  both `<｜tool▁call▁end｜>` and `<｜tool▁calls▁end｜>`) is not respelled.
  A spelling the grammar declares (a closer variant, a `confusable`) is read
  by its own rule first. This is a generic engine operator over every
  closer class, distinct from the declared confusable spellings of
  `tool_call.markup.terminal-confusable`. *Proved by* the goldens
  `qwen_function_closer_misspelled`, `qwen_parameter_closer_misspelled`,
  `qwen_function_closer_transposed`, `hermes_call_closer_misspelled`,
  `kimi_call_closer_misspelled`, `deepseek31_closer_misspelled` and
  `deepseek31_closer_misspelled_ambiguous` (not admitted), each also under
  the default allow-list, and the unit tests of `grammar::misspelling`.
- **A repair never overrides a reading that needs none.** Every repair is
  a fallback: a misspelling, a resynchronization, a bracket balance, a
  group split, a closer variant or confusable spelling is read only where
  the reading of the same bytes that needs no repair fails, and no
  canonical markup is ever deleted to make a repair fit. A value's text
  near a closer (`<Parameter name="a">1</Parameter>`, `</parameterx>`,
  `</paramete>`, the declared variants `</functions>` and `</tool_calls>`,
  or a closer written inside a JSON string) is the value's text: an
  `xml-params` value runs to a canonical `</parameter>` (a variant or
  misspelled closer, or a misspelled `<parameter=`, ends it only when no
  canonical closer comes before the next parameter opener or the body's
  end; which canonical closer is weighed by the next rule), a body
  delimited by a closer variant or confusable spelling also reads on past
  it, and a delimiter written inside a string of a JSON value
  not yet complete ends the value only as a fallback (the value ending by
  itself needs no repair). So canonical input is `Resolved` with its whole
  value, under `Effort::Exact` too. *Proved by*
  `tool_call_recover::canonical_values_with_near_closer_text_resolve`
  (every closer of Qwen3-Coder XML, Hermes, Kimi-K2 and DeepSeek v3/v3.1,
  case-variant, one-slip and over-long, pinned, under the default
  allow-list and under `Effort::Exact`), the goldens
  `qwen_value_holds_case_variant_parameter_closer`,
  `qwen_value_holds_overlong_parameter_closer`,
  `qwen_value_holds_misspelled_parameter_closers`,
  `qwen_value_holds_function_closer_variant`,
  `qwen_value_holds_wrapper_closer_variant`,
  `hermes_value_holds_wrapper_closer`, `kimi_value_holds_call_closer` and
  `deepseek31_value_holds_call_closer`, and the `canonical` `near-closer`
  rows of the correction evaluation.
- **Closer text inside a value is weighed by evidence.** A verbatim value
  (an `xml-params` string, unmodeled or undeclared value; a `raw` body such
  as Harmony's Python code) may hold the exact text of a closer inside its
  real content: a `write_file` content with `<parameter>v</parameter>`, or
  one that documents a call (`<function=g>...</function>`). For such text the
  maintainer's decision on issue #3 applies ("Pick by evidence, else
  ambiguous"): reading A ends the value at the first closer, reading B
  holds it and ends at the closer that actually completes the call, and the
  two are compared by the existing cost machinery (the rule costs of the
  repairs each needs). The reading that parses the rest of the call without
  repairs the other does not need wins and stays `Resolved`; when both
  parse cleanly (closers mentioned inline and the call then left unclosed;
  Python code holding `<|call|>` in a string) the call is `Ambiguous` with
  both values, never usable on a mutating tool. Reading B records
  `tool_call.value.closer-in-value` (no repair and no guess: it is chosen
  only over a reading that needs more repair), spanning from the first
  closer it holds to where it ends.

  The decision covers closer text inside real value content only; it does
  not override issue #3 §3 ("`</parameter>`, `</parameters>`, and repeated
  or stray closers are all one class"), its negative corpus ("text between
  parameters" and "trailing text inside the wrapper" must be rejected or
  stay unresolved), or its hard limit ("Never accept trailing free text as
  an argument"). So reading B is offered only where the closers it holds
  sit inside the value's text:

  - closers followed by nothing but closers and whitespace up to where the
    value would end hold no value text: they are stray closers, read as
    markup (`tool_call.markup.stray-closer`), and reading B is not offered
    (`a.py\n</parameter>\n</parameter>`, `</parameter></parameters>`,
    `<|call|><|call|>`);
  - where a closer sits on its line carries no meaning (maintainer, issue #3
    review: "Newlines have zero meaning here"): a closer after a newline,
    mid-line or indented, in a value framed by newlines or not, is read the
    same way. A closer that closes an element the value opened
    (`<parameter>v</parameter>`, `<tool_calls>…</tool_calls>`,
    `<functions>f</functions>`) is paired markup, text of the value. An
    *unpaired* closer (a `</parameter>` or a closer of the call that closes
    no element the value opened and no call it holds) followed by text
    before the closer that would end the value is never read into the value
    alone: the reading holding the text (`tool_call.value.text-after-closer`)
    and the reading ending the value at that closer and dropping the text as
    text between parameters (`tool_call.markup.text-between-parameters`, a
    data loss) cost the same, so the call is `Ambiguous` with both values,
    never usable on a mutating tool (`a.py</parameter> I will now write the
    file.\n</parameter>`, `a.py\n</parameter>\nI will now write the
    file.\n</parameter>`, `print(1)\n </parameter>\nDone.\n</parameter>`,
    `End with </function></tool_call> then stop.\n</parameter>` all read
    alike). Both readings are guesses. When a string's declared pattern
    rejects either value, neither is a reading: the value ends at its first
    closer and the text after it makes the call malformed (rejected);
  - a value holding call markup holds whole calls: the markup of a call the
    value holds (`<function=g>...</function>`, wrapped or not) is that
    call's own, so its parameter openers (even a name the outer tool
    declares) and closers are text of the value, and no reading ends the
    value inside it; a reading whose value would end inside a call another
    attempt opened (two calls in a row) is no reading, and neither is one
    holding a call opener after an unpaired closer (the closers that
    complete one call, then the next call's opener, whatever whitespace or
    none between: `x</parameter></function></tool_call>\n<tool_call>...`),
    so consecutive calls are never read as one;
  - a misspelled closer the value holds is text (T5/F9: no repair reads it
    as one).

  Closers left after a call once the value has ended (`</parameter>` and
  the call's own closers, after `</tool_call>`) are that call's stray
  closers, recorded and never assistant text, so the reading that leaves
  them costs their repair. Inside a function body the `xml-params` codec
  weighs every `</parameter>` before the next declared parameter in one
  pass over the value; for the call's own closers (`</function>`,
  `</tool_call>`, `<|call|>` and their declared variants) the engine reads
  the arguments slot on past each closer beside the reading that ends
  there. A value never holds a declared parameter's opener outside a call
  it holds, a misspelled one after a closer, or an opening reasoning
  marker; a string's declared pattern must admit it; typed values and
  enumerations end at their first closer. The extra reading is bounded and
  linear: the readings that end at later closers are read only if they
  complete the call, and only the last of them is reported, so a 1 MiB
  value holding 1,000 closers recovers in linear work under
  `ToolRecoveryRequest::sized`. A JSON value is self-delimiting: closer
  text inside its strings was never a closer, its reading past that text is
  the reading as written, and a rejected call reports where that reading
  stopped, as before.
  *Proved by* the goldens `qwen_value_holds_parameter_element`,
  `qwen_value_holds_call_closers`, `qwen_value_holds_call_closers_then_text`,
  `qwen_value_holds_call_closers_at_end`,
  `qwen_value_holds_call_closers_then_long_text`,
  `qwen_value_never_runs_past_reasoning_marker`,
  `qwen_value_holds_call_closers_unclosed`, `qwen_value_then_next_parameter`,
  `qwen_value_holds_wrapper_closers_adversary`,
  `qwen_value_holds_function_closers_adversary`,
  `qwen_value_closer_then_misspelled_opener`,
  `qwen_value_closer_text_rejected_by_pattern`,
  `qwen_bare_function_value_holds_function_closer`,
  `harmony_python_code_holds_call_closer`,
  `hermes_rejection_fails_past_closer_text_in_json_string`,
  `qwen_value_holds_call_markup`,
  `qwen_value_holds_several_documented_calls`,
  `qwen_value_holds_wrapped_call_then_text`,
  `qwen_value_documents_call_with_declared_parameter`,
  `qwen_consecutive_calls_of_different_tools`; for the scope,
  `effort_exact_refuses_repair`,
  `qwen_doubled_closer_after_value_is_stray`,
  `qwen_doubled_closer_variant_after_value_is_stray`,
  `harmony_doubled_call_closer_is_stray`,
  `qwen_value_ends_with_closing_sequence`,
  `qwen_value_holds_closers_then_whole_call`,
  `qwen_value_holds_closers_then_undeclared_call`,
  `qwen_value_holds_text_between_closers`,
  `qwen_prose_after_value_closer_is_ambiguous`,
  `qwen_prose_after_last_value_closer_is_ambiguous`,
  `qwen_same_line_closer_then_text_is_ambiguous`,
  `qwen_same_line_closer_then_prose_is_ambiguous`,
  `qwen_indented_closer_then_text_is_ambiguous`,
  `qwen_unframed_closer_then_text_is_ambiguous`,
  `qwen_unframed_value_holds_parameter_element`,
  `qwen_inline_closers_then_next_call_are_two_calls`,
  `qwen_same_line_doubled_closer_is_stray` and
  `qwen_indented_doubled_closer_is_stray` (each also under the default
  allow-list), `tool_call_goldens::qwen_closer_text_held_by_evidence`,
  `tool_call_goldens::qwen_text_after_unpaired_closer_is_ambiguous`,
  `tool_call_goldens::qwen_inline_closers_then_next_call_are_two_calls`,
  `tool_call_goldens::qwen_doubled_closer_is_stray_in_any_layout`,
  `tool_call_goldens::qwen_inline_call_closers_unclosed_are_ambiguous`,
  `tool_call_goldens::qwen_closing_sequence_after_value_is_stray`,
  `tool_call_recover::exact_effort_never_repairs`,
  `tool_call_cost::closer_text_in_values_is_linear`, and the
  `tool_call.value.closer-in-value`, `tool_call.value.text-after-closer`
  and `tool_call.markup.stray-closer` (`parameter`, `text-between`,
  `trailing-text`, `closing-sequence`, `closers-then-call`,
  `inline-closers-then-call`) rows of the correction evaluation.
- **A misspelled token still reads as its token.** The opener-side mirror
  of a misspelled closer: a markup token other than a closer (spelled with
  a leading `<` or `[`), the attempt's own opener included, one edit away
  from exactly one literal of the grammar (the next call's opener in a
  group, `<|tool_call_begn|>`; an argument separator,
  `<|tool_call_argument_begn|>`, `<｜tool▁sp｜>`; `[TOOL_CALS]` chaining a
  Mistral v11 call; `<functon=` inside `<tool_call>`; Qwen3-Coder XML's
  `<paramter=`, read by the `xml-params` codec) is read as that literal
  (`tool_call.markup.token-misspelled`, the original bytes recorded; a
  markup repair, single reading, so it does not demote). A token another
  literal of the grammar may spell exactly (a section opener where a call
  opener was due) is never a misspelling, nor is a short token (an inner
  word under three bytes) or one as close to two literals. An attempt's own
  opener written with one slip (`[TOOL_CALL]`, `<tool_cal>`,
  `<｜tool▁call▁begn｜>`) is read the same way, never left as silent
  content: as a canonical opener would, the attempt is then a call or a
  rejected attempt. Prose naming markup without one-slip delimited
  openers (`tool_call`, `TOOL_CALLS`, `<tool_calling>`) stays content.
  *Proved by* the goldens `kimi_call_opener_misspelled`,
  `kimi_argument_separator_misspelled`, `mistral11_call_opener_misspelled`,
  `qwen_function_opener_misspelled`, `qwen_parameter_opener_misspelled`,
  `mistral11_first_opener_misspelled`, `deepseek31_first_opener_misspelled`,
  `hermes_first_opener_misspelled`, `kimi_first_opener_misspelled` and
  `prose_similar_to_openers_stays_content`,
  each also under the default allow-list, and the
  `tool_call.markup.token-misspelled` rows of the correction evaluation.
- **Reasoning markers are never call content.** The bytes of a reasoning
  block's markers belong to the block: a call found inside the block that
  ran into its closing marker is cut at it, and so is a call outside
  reasoning that read an opening marker as markup (inside a capture or its
  arguments, not as the data of a string value it read: a JSON string or a
  verbatim parameter may hold `<think>`) (`tool_call.reasoning.marker-cut`,
  a data loss): a name, id or capture reaching into the marker is dropped,
  and arguments read from it are unresolved, so the call is never usable (a
  Mistral v11 call with no `[ARGS]` that read `[/THINK]` or `[THINK]` as its
  arguments). A rejected attempt that ran over an opening marker ends at
  it, and the block the marker opens is recognized (up to its closing
  marker, even one the attempt read over). A reasoning block is never text
  inside a call's wrapper: a complete call followed by a block reads with
  its wrapper closer inserted before the block
  (`tool_call.markup.missing-wrapper-close`, the only reading, as before
  another call's opener), and a closer the call or attempt left written
  right after the block is its markup, never content: on a call, recorded
  as a stray closer (`tool_call.markup.stray-closer`); for a rejected
  attempt, or a call begun inside the block (the block holds its opener
  unclosed), reported as a rejected attempt of the dialect over the closer
  alone. A closer after a block that holds no unclosed opener stays content.
  *Proved by* `hermes_closer_after_cut_reasoning`,
  `hermes_rejected_attempt_closer_after_reasoning`,
  `hermes_closer_after_reasoning_block` and
  `hermes_reasoning_mentions_closer_stays_content`, and the
  `reasoning_marker_not_call_arguments` and
  `embedded_calls_once_in_span_order` goldens,
  `mistral11_call_cut_at_opening_marker`,
  `mistral11_rejected_attempt_cut_at_opening_marker`,
  `hermes_rejected_attempt_cut_at_opening_marker` and
  `mistral11_marker_inside_string_not_cut`,
  `tool_call_recover::calls_an_attempt_reads_inside_a_closed_block_stay_embedded`,
  and the `fuzz-tool-call` invariant that no call, embedded call or
  rejected attempt holds bytes of a reasoning range's opener or closer.
- **Call markup is never silent content.** Call markup that no dialect of
  the allow-list reads (a family not built in yet: the stripped renderings of built-in dialects no grammar reads; or a built-in
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
- **Measured.** The 18,202-case correction evaluation records zero incorrect
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

Batch recovery uses a shared incremental intake kernel: source metadata,
request validation and recognizer construction precede byte intake. For each
byte, source admission and copying precede UTF-8 classification and recognition.
This replaces whole-source retention followed by whole-source encoding scans;
resource counters and refusal precedence intentionally change with that shared
ordering. After the first refusal there are no further copies, admissions or
interpreter steps. The digest still identifies every supplied byte, while
`receipt.sources` contains either the complete admitted source or nothing.
No retained prefix is presented as the full source. A complete source can still
be retained when finalization or resolution refuses. See
[the resource contract](../RESOURCES.md) for ownership and growth details.
The public streaming API, stable-prefix events and progress surfaces remain
unfinished under #16.

- **Enforced by** the published bound
  `work ≤ W_intake + (K_LEX + Σ_d K_d)·n + K_ERR·E + W_aux` (each dialect contributes
  `K_THREAD` times its configuration bound per byte; `E` error configurations
  are capped by the effort's budget; `W_intake` includes source/range growth,
  and `W_aux` retains the separately admitted codec, history and resolution
  terms), linear JSON extraction, and governed
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
  and the CLI use it by default. It adds 176 work units, 368 scratch bytes
  and one evidence entry per 7 bytes per output byte to the preset (the
  measured maximum of the densest markup up to 1 MiB, plus a margin of
  25% to 40%; `RECOVER_WORK_PER_BYTE`, `RECOVER_SCRATCH_PER_BYTE` and
  `RECOVER_EVIDENCE_BYTES_PER_ENTRY`; the shapes `docs/RESOURCES.md` lists as
  not covered still exhaust explicitly), and `⌈preset.candidates / 8⌉` candidates
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
  `tool_call_cost::sized_budget_follows_the_published_formula`,
  `tool_call_cost::chained_mistral_v11_calls_cost_is_linear` (256 to 4,096
  Mistral v11 calls separated by whitespace alone, one chained attempt:
  work and scratch linear in the bytes),
  `tool_call_cost::dense_markup_fits_published_budget` and
  `tool_call_scale::one_mib_dense_markup_fits_published_budget` (about 1 MiB
  of tiny calls in every built-in dialect, Mistral v11 calls chained by
  whitespace included, and of repeated nested, JSON-array
  and string items recovers under plain `sized(Effort::Standard, output)`),
  and the host
  and CLI tests `tool_call_tools::busy_turn_resolves_every_call_by_default`
  and `tool_calls_cli::busy_turn_resolves_every_call_by_default`. See
  [RESOURCES.md](../RESOURCES.md#budgets-for-large-outputs-and-busy-turns).
- **Repeated parameters.** A parameter repeated k times (`<parameter=xs>`
  occurrences that resolve as array items) is inside the linear bound: its
  `work` and `total_scratch_charged` grow with the output, not with k times
  the output, because resolution appends each single-reading item to the
  partial array in place. Output that is only repeated markup costs about 52
  to 61 work and 104 to 262 peak scratch bytes per byte, above the
  `sized` allowance for ordinary output, so `sized` alone exhausts
  explicitly rather than quietly: on `Work` at k = 1,000 nested items
  (3.66 million needed against 2.99 million), about 1,500 JSON items and
  1,100 string items; on peak `ScratchBytes` between 4,000 and 8,000 nested
  items (130 MB against 86.7 MiB at 8,000). *Proved by*
  `tool_call_cost::repeated_parameters_cost_is_linear` (1,000 to 8,000
  occurrences as nested items, JSON arrays and strings; both counters within
  1.05x of the byte ratio). See
  [RESOURCES.md](../RESOURCES.md#tool-call-recovery-cost-bound).

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
  and nested `<parameter=...>` elements read as an object's members, or as
  the object items of an array whose items are unmodeled), and
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
  verbatim fallback. JSON arguments arrive typed and are never retyped.
  - **Scope graph (member names of unmodeled scopes).** Typing is the
    exact schema's, but which parameter a nested `<parameter=NAME>` opener
    belongs to is decided by names, so the adapters also compile, once per
    tool and charged to the governor, a *scope graph* from the exact
    schema (`ArgumentShape::scopes`; each `ParameterShape::nesting` refers
    into it): one node per distinct schema location (a `$defs` or
    `definitions` entry, an inline object, an `anyOf`/`oneOf`/`allOf`
    union), holding the member names it declares and, per member, whether
    its value nests as an object or as an array of objects. A recursive
    `$ref` is a back-edge by node index. So a `$ref` item or object
    parameter whose member shares the name of an outer parameter (the
    common pydantic `$defs` shape: `name`, `id`, `tags`) keeps that member
    inside its own scope, and a balanced recursive tree nests exactly as
    its inline twin does; neither is flattened or hoisted into a
    `Resolved` call. A scope whose schema names no member (a free-form
    object, a `$ref` to a non-object) has unknown membership: closing it at
    an enclosing scope's opener is the guess
    `tool_call.markup.unmodeled-scope-boundary`, a best guess that is never
    usable on a mutating tool. Only local `$ref` pointers are resolved.
    *Nesting limit.* `Budget::depth` guards nested parameters: the codec
    fits `depth - 1` object levels, and a whole recovered call fits about
    `(depth - 1) / 3` nested item levels (about 42 at the CLI and MCP value,
    128), because depth is charged again where the value is read back;
    anything deeper is the explicit `BudgetExceeded(Depth)`, never a partial
    value (`tool_call_recover::nested_item_depth_is_a_stack_guard`).
    *Proved by* `tool_call_goldens::xml_array_recursive_ref_items_nest`,
    `xml_array_ref_items_member_shares_outer_name`,
    `xml_array_ref_items_keep_path_not_hijacked`,
    `xml_array_ref_items_outer_array_collision`,
    `xml_object_ref_param_member_shares_outer_name`,
    `xml_array_anyof_items_union_nest` and
    `xml_unmodeled_opaque_scope_boundary`, and the correction corpus's
    `file_tasks` cases.
  - **Root `$ref` and composed roots.** The FastMCP/pydantic shape has no
    `properties` at the root, only `{"$ref":"#/definitions/Args",
    "definitions":{...}}` (Draft-07) or `{"$ref":"#/$defs/Args",
    "$defs":{...}}` (2020-12). The projection follows a root `$ref` to a
    local pointer (transitively through further local `$ref`s) and projects
    the target object's parameters, so repair knows the tool's parameter
    names and types exactly as for an inline schema. **Draft-07 sibling
    rule:** a `$ref` overrides its siblings, so the root's `properties`,
    `type` and the like are ignored for projection, as they are for
    validation, and stay listed as unmodeled keywords beside `$ref` and
    `definitions`; in **2020-12** the siblings apply with the target, so
    their parameters are projected too. A root `allOf` of one schema that
    resolves the same way is projected with the root; a root `anyOf` or
    `oneOf` whose every branch is an object schema (or a `$ref` to one)
    projects the *union of the branches' names*. A name declared by one
    applying schema is typed from it; a name a union branch declares, or
    that several schemas declare, stays unmodeled (not marked required): its
    typing is the exact schema's (`tool_call.value.schema-typed`), as for
    the scope graph's unions. A nested `$ref` member goes through the scope
    graph with the pointer below the target. An external or missing `$ref`
    makes the declaration invalid (the exact schema cannot compile); a
    cyclic `$ref`, a target that is not an object schema (a `type` array, a
    string type) and a union with a non-object branch are not followed: the
    projection is the root's own, which for a Draft-07 root `$ref` is no
    parameter, `PassThrough`, with every root keyword listed. Validation
    still uses the exact declaration and its draft, and rendering back
    embeds the parameters text verbatim in every wire form and prompt form
    (the prompt forms print the exact schema, so a root-`$ref` tool's
    printed declaration does not change). *Proved by*
    `tool_adapters::root_ref_projects_the_target_object`,
    `root_ref_prompt_form_declaration_prints_the_exact_schema`,
    `root_ref_defs_2020_12_merges_siblings_with_the_target`,
    `root_all_of_single_ref_projects_the_target`,
    `root_any_of_objects_projects_the_union_of_names`,
    `unprojectable_root_ref_stays_pass_through_with_its_evidence`,
    `root_ref_nested_ref_members_flow_into_the_scope_graph` and
    `draft07_mcp_tools_adapt_losslessly` (the Draft-07 sibling rule), and
    `tool_call_goldens::mcp_root_ref_calls_resolve_with_typed_arguments`
    (`mcp_root_ref_qwen3_xml_resolved`, `mcp_root_ref_hermes_json_resolved`,
    `mcp_root_defs_2020_12_resolved`, `mcp_root_all_of_ref_resolved`,
    `mcp_root_any_of_union_resolved`),
    `mcp_root_ref_misspelled_parameter_repairs_like_inline` and
    `mcp_root_ref_unprojectable_stays_pass_through`.
  - *Typing proved by* `tool_call_goldens::unmodeled_schemas_type_xml_text`
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
(`tool_call.reasoning.embedded-call`), including those an attempt reading
several calls (Mistral v11 `[TOOL_CALLS]` elements) read inside the block
before running on past its closer; a call two attempts read is offered
once, and the embedded calls are in span order. *Proved by*
`tool_call_goldens::reasoning_never_swallows_calls` (Hermes, Mistral v3,
Llama 3, bare JSON, Qwen3-Coder XML and Anthropic calls, unclosed and
closed, under the default allow-list, plus the `separate` and `as_content`
modes),
`tool_call_recover::calls_an_attempt_reads_inside_a_closed_block_stay_embedded`,
the `embedded_calls_once_in_span_order` and
`embedded_calls_once_with_arguments` goldens,
and the tool-call fuzz probe's reasoning invariant (a call the turn
recovers with reasoning as content never vanishes under a reasoning mode).

The merged `reasoning` ranges include the markers and join adjacent blocks
(`<think>a</think><think>b</think>` is one range, `0..32`). To read the text,
use `reasoning_blocks` (wire `[{"span": {start, end}, "text": {start, end}}]`):
one entry per block in source order, never merged
(here `text` is `7..8` and `23..24`), where `text` excludes both markers.
Under `starts_open` the block has no opener, so its `text` begins at `0` (the
`span` and the `text` start together); a closed block's `text` ends at its
closer, a block closed at a call opener ends at the opener, and an
unterminated block ends at the end of output. The bytes are exactly those of
the output: Anthropic thinking text stays JSON-escaped (`\n`, `\"`), never
unescaped. The text of an Anthropic `redacted_thinking` block is not
reasoning prose: it is the opaque encrypted `data` payload, verbatim
(`EmwKAhgB` in the `anthropic_redacted_thinking` golden), and consumers must
not interpret or display it. A closer is recognized only at or after the start
of the block's text, so `text` is never a reversed range: in
`{"type": "redacted_thinking", "data": "}x` the `"}` begins inside the opener
(its final `"`), does not close the block, and the block is unterminated with
`text` `39..41` (`}x`). *Proved by* the `reasoning_blocks_*` tests in
`tool_call_recover`, the goldens' captured `reasoning_text`,
`tool_call_goldens::anthropic_redacted_thinking_text_is_the_opaque_payload`,
`anthropic_redacted_thinking_closer_overlaps_opener`,
`reasoning_block_ranges_slice_over_every_golden`, and the tool-call fuzz
probe's block invariants.

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
Anthropic form; the Anthropic `{"type":"tool","name":N,"disable_parallel_tool_use":true}`
is the strict form with exactly one call), `tool_choice_strict`
(`{"exactly_one", "allow_content", "allow_reasoning"}`, all three booleans
and all required: the options of a [strict forced choice](#strict-forced-tool-choice),
valid only beside a forced `tool_choice`), `policy` (`per-tool`, `structural`, `tolerant`), `effects`,
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

`--tool-choice NAME` forces the tool `NAME`; the flags
`--tool-choice-exactly-one`, `--tool-choice-no-content` and
`--tool-choice-no-reasoning` (each at most once, and a usage error, exit 1,
without `--tool-choice NAME`, a tool name rather than `auto`, `none` or
`required`) make it a [strict forced choice](#strict-forced-tool-choice).

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

### Strict forced tool choice

`ToolChoice::Forced(name)` stays what it was: at least one call, and every
call to `name` (its canonical name or an alias). It accepts two calls to
`name`, and prose around the call. `ToolChoice::ForcedStrict(StrictForced)`
adds three options on top of exactly those checks:

| Option | Field | Constraint text | Violation |
| --- | --- | --- | --- |
| exactly one call | `exactly_one` | `exactly-one` | Each call after the first, in `calls`, is `Unresolved` with `tool_choice_cardinality` (unless it already was unresolved), every entry of `rejected` (of any reason, an unsupported dialect's included) is one more call beyond the one, and the choice records one diagnostic stating both counts. |
| no content | `allow_content: false` | `no-content` | Any `content` range holding a byte that is not ASCII whitespace (`u8::is_ascii_whitespace`: space, tab, line feed, carriage return, form feed), and any rejected attempt (its bytes are outside every call), is a diagnostic: at most one for the content (the first byte, and a count of the other ranges) and one for the attempts. |
| no reasoning | `allow_reasoning: false` | `no-reasoning` | Any reasoning block is a diagnostic: one, naming the first block and a count of the others. Under `reasoning: "content"` reasoning is never recognized, so this combination is an `invalid_input` request error (MCP and CLI included) instead of a check that passes vacuously. |

`StrictForced::exactly_one(name)` is the usual form (exactly one call, no
content, reasoning allowed). The constraint recorded in the receipt's
`tool_call.recover.v1` validation and in `value.tool_choice.constraint` lists
the active options in brackets with the name last, so a name holding `:`, `;`
or `,` stays unambiguous: `tool_choice=forced[exactly-one,no-content,no-reasoning]:read`.
A `ForcedStrict` with all three options lenient (`exactly_one` false,
content and reasoning allowed) checks exactly what `Forced` checks and
renders exactly `tool_choice=forced:NAME`.

What counts:

- **One call** counts the entries of `calls`, recognized calls of any
  outcome, as `required` does. A rejected attempt of any reason, a
  malformed or truncated second call or markup of a dialect that is not
  supported, is a call attempt beyond the one call and fails the check (and
  so the verdict, not only the finish), as it fails no-content. Calls
  embedded in reasoning are never counted (they are never
  tool-choice-checked); `allow_reasoning` governs the block that holds them.
- **Content** is whitespace-insensitive only for ASCII whitespace, so the
  newlines a model writes between or around calls pass. Non-ASCII
  whitespace such as U+00A0 or U+2003 is content, and so are tool results
  the model wrote.
- An undeclared `name` is an `invalid_input` request error, as for `Forced`.

A failed strict check is the same whole-turn contract failure as any failed
tool choice: `finish` `unresolved`, `verdict` `nothing_usable`, the emission
empty, exit 4, even when the single call would run on its own.

Requesting it:

- Rust: `ToolChoice::ForcedStrict(StrictForced::exactly_one("read"))`, with
  `allow_content(true)`, `allow_reasoning(false)` and `with_exactly_one(false)`
  to change an option.
- Anthropic wire form: `{"type":"tool","name":N,"disable_parallel_tool_use":true}`
  (`ToolChoice::from_anthropic`) is `ForcedStrict` with exactly one call and
  content and reasoning allowed; `disable_parallel_tool_use: false` is
  `Forced`. `disable_parallel_tool_use` on `auto`, `any` or `none` stays an
  error: at-most-one for a free choice is not offered.
- MCP: `tool_choice_strict` beside a forced `tool_choice`. It is a tool
  error beside a choice that is not forced, and when `exactly_one` is false
  beside an Anthropic `disable_parallel_tool_use: true`.
- CLI: `--tool-choice NAME` with `--tool-choice-exactly-one`,
  `--tool-choice-no-content`, `--tool-choice-no-reasoning`.

*Proved by* the goldens `forced_strict_two_calls`,
`forced_strict_call_plus_prose`, `forced_strict_single_call_passes`,
`forced_strict_reasoning_rejected`,
`forced_strict_anthropic_disable_parallel`,
`forced_strict_anthropic_one_call_with_prose_passes`,
`forced_strict_rejected_second_attempt` and
`forced_strict_anthropic_unsupported_dialect_attempt` and their named
assertions in `tool_call_goldens` (including
`forced_strict_non_ascii_whitespace_is_content`),
`tool_call_recover::strict_forced_constraint_strings`,
`tool_call_recover::strict_forced_cardinality_content_and_reasoning`,
`tool_call_recover::strict_forced_counts_rejected_attempts_of_any_reason`,
`tool_call_recover::strict_forced_contested_second_body_is_order_independent`,
the unit test `merge_fails_closed_in_either_order` (`recover.rs`),
`tool_call_recover::strict_forced_diagnostics_are_capped`,
`tool_call_recover::strict_no_reasoning_under_reasoning_as_content_is_invalid_input`,
`tool_call_recover::failed_tool_choice_has_no_usable_calls`,
`tool_call_tools::strict_choice_counts_a_rejected_second_attempt`,
`tool_call_tools::failed_tool_choice_zeroes_the_usable_counts`,
`tool_call_tools::strict_no_reasoning_beside_content_reasoning_is_invalid_input`,
`tool_calls_cli::strict_forced_flags_count_a_rejected_second_attempt`,
`tool_calls_cli::strict_no_reasoning_with_content_reasoning_is_invalid_input`,
`tool_call_recover::tool_choice_from_openai_and_anthropic_json`,
`tool_call_tools::tool_choice_strict_member`,
`tool_call_tools::tool_choice_strict_member_errors`,
`tool_calls_cli::strict_forced_tool_choice_flags` and
`tool_calls_cli::strict_forced_flags_need_a_tool_name`, and the fuzz
invariants of `docs/FUZZING.md`.

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
  read-only tools (`ToolCalls::usable_accepting_guesses`). Both are 0
  whenever `value.tool_choice.passed` is false, so they always agree with
  the empty emission and a client reading either never runs a call from a
  failed turn.
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
`finish_reason` `"stop"` (`"length"` under a length stop). `usable_calls` and
`usable_calls_accepting_guesses` are 0: nothing of the turn may run, and a
call's own `outcome` still says what it would have been. A strict forced choice fails the same way when its extra
conditions fail (see [Strict forced tool choice](#strict-forced-tool-choice)).

| `status` | `value.verdict` | `value.finish` | `openai_finish_reason` | Exit | What the caller should do |
| --- | --- | --- | --- | --- | --- |
| `resolved`, `best_guess` | `run` | `calls`, `truncated_with_calls`, or `unresolved` (a rejected or unresolved attempt beside the calls that run) | `"tool_calls"` (`"length"` under a length stop) | 0 | Run the usable calls: `usable_calls` (strict) or, accepting read-only guesses, `usable_calls_accepting_guesses` (what the emission prints); report `rejected` |
| `resolved`, `best_guess` | `no_calls` | `content_only` | `"stop"` | 0 | Use the content; nothing to run |
| `resolved`, `best_guess` | `ambiguous` | `unresolved` | `"stop"` (`"length"` under a length stop) | 3 | Choose among the ambiguous call's `alternatives`, or re-ask |
| `resolved`, `best_guess` | `nothing_usable` | `unresolved` (calls that may not run, a failed tool choice, unresolved calls, rejected attempts) or `truncated` | `"stop"` (`"length"` under a length stop) | 4 | Run nothing (`usable_calls` and `usable_calls_accepting_guesses` are 0 when the tool-choice check failed); re-ask or report the per-call reasons and the tool-choice diagnostics |
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

## Compatibility

`tool_call.recover.v1` is a pre-0.1.0 wire value: additive members may appear
in it, and in the MCP output schema, before 0.1.0. Parse by member name and
ignore members you do not know; the MCP output schema sets
`additionalProperties: false`, so validate against the schema your server
lists (`tools/list`), not a copy. This release changes the following against
the previous head (`main`). Rust API changes are listed under "Rust API"
below.

Wire and MCP:

- `value.reasoning_blocks` is new and **required** in the output schema:
  `[{"span": {start, end}, "text": {start, end}}]`, one entry per reasoning
  block, never merged (`value.reasoning` keeps the merged ranges).
- A strict forced choice has a constraint string of the family
  `tool_choice=forced[exactly-one,no-content,no-reasoning]:NAME` (only the
  active options, the name last); a choice with no active option is still
  `tool_choice=forced:NAME`.
- A new per-call unresolved reason code, `tool_choice_cardinality`: a call
  after the first beneath an `exactly_one` choice.
- The MCP input has a new optional member, `tool_choice_strict`, and
  `tool_choice` accepts the Anthropic `disable_parallel_tool_use` boolean
  (`true` means exactly one call). Both are described in
  [Strict forced tool choice](#strict-forced-tool-choice).
- `value.usable_calls` and `value.usable_calls_accepting_guesses` are 0
  whenever `value.tool_choice.passed` is false; they used to count calls
  beside `verdict: nothing_usable`.
- Evidence pointers of a repeated array parameter number its items across all
  occurrences (`/xs/0`, `/xs/1`, ...), where each occurrence used to restart
  at 0, and `decode-typed` evidence cites each item's own span. Alias
  spellings of one parameter share one sequence.
- Ambiguous readings are ordered among equal costs by the digest of their
  wire JSON, which now includes `reasoning_blocks`; the order can differ from
  the previous release.
- A new rule id, `tool_call.markup.unmodeled-scope-boundary` (46 operators
  in all; see [RULES.md](RULES.md)).
- Receipt `budget` and `usage` numbers differ for the same input (see
  Budgets below); decisions, values and spans do not.

CLI:

- `recover-tool-calls` has the flags `--tool-choice-exactly-one`,
  `--tool-choice-no-content` and `--tool-choice-no-reasoning` (see
  [CLI](#cli)).

Behaviour:

- A repeated `<parameter=NAME>` whose body holds nested `<parameter=`
  elements is read as the object items of an array-of-object parameter.
- `$ref`, `anyOf` and `oneOf` item and parameter scopes declare their
  schema's member names, so a recursive tree nests and an inner member that
  shares an outer parameter's name stays inside its item.
- Free-form object parameters and items (`additionalProperties: true`, no
  declared members) close by the guess `unmodeled-scope-boundary` rather than
  `missing-parameter-close`: a `best_guess`, never run on a mutating tool.
- A strict choice with `allow_reasoning: false` beside reasoning `content`
  is `invalid_input`; any rejected call attempt counts against `exactly_one`
  and `allow_content: false`.
- Anthropic `disable_parallel_tool_use` used to be rejected; it is accepted.
- A root `$ref`, a root `allOf` of one schema and a root `anyOf`/`oneOf` of
  objects now project their parameters (a Draft-07 root `$ref`, the
  FastMCP/pydantic shape, used to project none), see
  [C5](#c5-lossless-declaration-adapters).
- A reasoning closer is recognized only at or after the block's text start,
  and a reasoning marker inside a JSON string of a repeated parameter stays
  value text.
- A whole call nests about `(depth - 1) / 3` item levels (about 42 at the
  CLI and MCP value); deeper input is the explicit `BudgetExceeded(Depth)`.

Budgets (see [RESOURCES.md](../RESOURCES.md)):

- The per-byte constants behind `ToolRecoveryRequest::sized` are
  `RECOVER_WORK_PER_BYTE` 176 (was 32), `RECOVER_SCRATCH_PER_BYTE` 368 (was
  48) and the new `RECOVER_EVIDENCE_BYTES_PER_ENTRY` 7, so the dense
  repeated markup the previous constants exhausted on now recovers. An
  exhausted budget stays an explicit outcome.
- Repeated parameters, Mistral chained calls and call arrays cost linear
  time. The default budget admits about 610 one-property root-union
  branches (about 670 before).

### Rust API

The Rust types are not `#[non_exhaustive]`, so new variants and fields break
exhaustive matches and struct literals.

Breaking:

- `ToolChoice` gains the variant `ForcedStrict(StrictForced)`. An exhaustive
  `match` no longer compiles.
- `UnresolvedReason` gains `ToolChoiceCardinality` (`code()` is
  `tool_choice_cardinality`, with a `Display` text). Exhaustive matches break.
- `EditOperator` gains `UnmodeledScopeBoundary`
  (`tool_call.markup.unmodeled-scope-boundary`, appended last);
  `grammar::edit::OPERATOR_COUNT` is 46 (was 45) and `OPERATORS.len()` is 46.
  Exhaustive matches and code that hardcodes 45 break.
- `ToolCalls` gains the pub field `reasoning_blocks: Vec<ReasoningBlock>`.
  Struct-literal construction breaks. The doc of `ToolCalls::reasoning`
  changes: adjacent ranges are merged (`<think>a</think><think>b</think>` is
  `0..32`); the per-block view is `reasoning_blocks`.
- `ReasoningSegment` gains the pub field `text: Range<usize>` (marker-free
  text range; invariant `text.start <= text.end`, `text` inside `span`).
  Struct literals break.
- `ArgumentShape` gains `scopes: ScopeGraph` and `ParameterShape` gains
  `nesting: Nesting`. Struct literals break.
- `ArgTree` gains the pub field `collected: Vec<CollectedMember>` and a
  private field, so external struct-literal construction no longer compiles.
  Derives are unchanged (the private running state never takes part in
  equality).
- `ArgTree::value` and `ArgTree::empty_object` are no longer `const fn`.
- `ToolCalls::usable()`, `usable_accepting_guesses()` and `emitted_calls()`
  return an empty list when `tool_choice.passed` is false (a violated tool
  choice means nothing of the turn runs). `emitted_calls()` is now exactly
  `usable_accepting_guesses()`. Callers that read `usable()` beside a failed
  check used to get the `Resolved` calls.
- `ToolChoice::from_anthropic` accepts `disable_parallel_tool_use` on
  `{"type":"tool",...}`: `true` is `ForcedStrict { exactly_one: true,
  allow_content: true, allow_reasoning: true }`, `false` is `Forced`. A
  non-boolean value, or the member on `auto`/`any`/`none`, is a
  `ToolChoiceError`. It used to be an error for every type.
- Request error: `ForcedStrict { allow_reasoning: false }` with
  `ReasoningPolicy::AsContent` is an `invalid_input` outcome (reasoning is
  never recognized, so the option could not be enforced).
- Published constants change value (see the budget items above):
  `cost::RECOVER_WORK_PER_BYTE`, `cost::RECOVER_SCRATCH_PER_BYTE`.
  `ToolRecoveryRequest::sized_budget` / `sized` / `sized_for` also grow
  `evidence_entries` (signatures unchanged).

Added:

- `tool_call::StrictForced { name, exactly_one, allow_content, allow_reasoning
  }` (`Clone, Debug, PartialEq, Eq`) with `exactly_one(name)`,
  `allow_content(bool)`, `allow_reasoning(bool)`, `with_exactly_one(bool)`
  (the last three `const fn`).
- `tool_call::ReasoningBlock { span, text }` (`Clone, Debug, PartialEq, Eq`).
- `tool_call::CollectedMember { pointer, key_span, occurrences }` (`Clone,
  Debug, PartialEq, Eq`).
- New public module surface `tool_call::{Nesting, ScopeGraph, ScopeMember,
  ScopeNode}`. `Nesting` is `None | Object(usize) | ArrayOfObject(usize)`
  (`Default` is `None`). `ScopeMember { name, nesting, array }`.
  `ScopeNode::{members, member, declares, is_open, is_opaque}`.
  `ScopeGraph::{node, len, is_empty}`.
- `cost::RECOVER_EVIDENCE_BYTES_PER_ENTRY: u64 = 7`.
- Docs: `Budget::depth` documents the stack guard and the `(depth - 1) / 3`
  nested-item capacity; `ToolChoice::constraint` documents the `forced[...]`
  family; `ToolCalls::usable` has a doctest.

Added for typed (derived) tool definitions, none of which breaks existing code:

- Traits in `tool_call`, implemented by the derives and by core for the
  supported field types: `ToolArguments: FromNode` (`tool_definition()`,
  `object_spec()`); `ToolParameter: FromNode` (`parameter()`,
  `const REQUIRED`, `absent()`, `const DEPTH`), implemented for `String`, the
  twelve integer types, `f32`, `f64`, `json::Number`, `bool`,
  `Vec<T: ToolItem>`, `Option<T: ToolItem>` (so `Option<Option<T>>` is
  rejected) and `Box<T: ToolItem>` (forwards `REQUIRED`, `absent()` and
  `DEPTH`; it does not enable recursion, and a cycle through it is still
  E0391). Only `parameter()` is required of a hand-written impl: `REQUIRED`
  defaults to `true`, `absent()` to `None` and `DEPTH` to `0`, and `REQUIRED`
  must be `false` exactly when `absent()` is `Some(..)`. `FromNode` is also
  implemented for `f32` (the spelling is parsed once with `str::parse::<f32>`,
  so reading is round-to-nearest and never double-rounded through `f64`;
  writing is exact; infinity is rejected with `expected: "number within the
  finite f32 range"`) and for `Box<T: FromNode>`. The marker `ToolItem: ToolParameter` (every
  `ToolParameter` except `Option<T>`, so `Vec<Option<T>>` does not compile);
  `ToolRepeatable` (`repeated()`, for `Vec<T>` and `Option<Vec<T>>`);
  `ToolPattern` (`with_pattern(&'static str)`, for `String`, `Option<String>`,
  `Vec<String>` and `Option<Vec<String>>`); and `ToolExtra`, a **sealed**
  marker (private supertrait) implemented only for `json::Node`, built by the
  generated code through its `#[doc(hidden)]` `__from_members` constructor.
- Nested derived structs state their unknown-member handling explicitly:
  `ToolArguments::object_spec()` (reached only through
  `ToolParameter::parameter()`, as a field) always calls
  `unknown_parameters`, with the struct's `unknown` value or
  `UnknownParameters::DropWithLoss` when it declares none. It does not
  inherit the enclosing tool's handling, so under a top-level
  `unknown = pass` an undeclared nested member is dropped with a recorded
  `tool_call.parameter.drop-unknown` loss (a `BestGuess` call) and the derived
  `from_node` succeeds. A nested struct without `unknown` that formerly
  inherited `error` (a mutating tool) and left the call `Unresolved` now
  drops the member and the call is a `BestGuess`; declare `unknown = error` on
  it to stay strict. `tool_definition()` is unchanged: a top-level tool
  keeps the effects-based default. A hand-built tool that embeds a derived
  `parameter()` keeps the nested handling; a hand-built nested `ObjectSpec`
  without `unknown_parameters` still inherits the tool's. Near misses of
  declared names follow core's matching policy. Under `Structural` (the
  default for mutating tools) a near miss is never dropped, and the call is
  `Unresolved` with `UnknownParameter`. Under `Tolerant` (the default for
  read-only tools) it is renamed as a guess to a free declared name it nearly
  matches, or dropped with a recorded loss when no such name is free.
- `tool_call::max_depth(&[usize]) -> usize` (`const fn`): the nesting depth
  of a derived type, evaluated at compile time to reject indirect recursion.
- `tool_call::Members<'a>` (`new(declared, order, node)`, `is_present(index)`,
  and `Iterator<Item = Slot<'a>>`) and `tool_call::Slot<'a>` (`Declared`,
  `Duplicate`, `Undeclared`; `#[non_exhaustive]`, so a match outside core needs
  a wildcard arm, and generated code reports a `FromNodeError` for it instead
  of panicking): a single-pass classifier of an object's members
  (O(n log k)) that tells a duplicate and an undeclared member from an absent
  one, which `Node::get` cannot.
- `FromNodeError::{mismatch(expected, &Node), missing(name), duplicate(name),
  unknown(name), within(segment)}`: constructors for the exact
  `pointer`/`expected`/`found` shape; `within` prefixes a JSON Pointer segment.
- `tool_call::is_valid_name` is now a `const fn` (same behavior).
- `#[doc(hidden)]` `tool_call::{__derive_number, __sorted, __valid_name,
  __DERIVE_ABI_0_1}`:
  support for generated code only (a number built from a spelling that the
  macro already validated, a `const` check that the declared-name table is in
  strictly ascending byte order, which generated code asserts at compile time
  so release builds cannot misclassify members, and name validation at
  compile time). The `Members` classifier is documented to report absent
  members through per-field slots and `ToolParameter::absent()`;
  `is_present` remains available to hand-written conversions. They are not part of the
  stable API, and their preconditions are documented on the items.
- New crate `purrassist-derive` (zero dependencies, built-in `proc_macro`
  only) with `#[derive(ToolDefinition)]` and `#[derive(ToolEnum)]`, and the
  facade re-exports `purrassist::{ToolDefinition, ToolEnum}` (the derive macros
  plus the `#[doc(hidden)]` `purrassist::__core`, the core crate under a name
  the generated code reaches through `::purrassist`, so users of the facade
  need no direct core dependency). Core never depends on the derive crate.
- Using the derives, three rules (also in the `purrassist-derive` README and
  crate documentation):
  - **Same version.** `purrassist-derive` and `purrassist-core` must share
    major.minor (patch may differ). Every expansion references the
    `#[doc(hidden)]` marker `tool_call::__DERIVE_ABI_<major>_<minor>`
    (`__DERIVE_ABI_0_1` for 0.1.x), named from the derive's own version at
    build time, so a mismatched pair fails to compile with an error naming
    `__DERIVE_ABI_*`. Cargo accepts the pair (the internal requirements are
    caret requirements, deliberately not `=` pins), so the compile error is
    the guard. The marker is not API and is renamed with each major or minor
    version; a core unit test fails until it is.
  - **Crate path.** Generated code names `::purrassist_core` (the
    `purrassist-derive` derives) or `::purrassist::__core` (the facade's), so
    depend on `purrassist-core` or `purrassist` under that name, or add
    `#[tool(crate = path)]` to the struct or enum. The path replaces the whole
    prefix, in both flavors and for `ToolDefinition` and `ToolEnum`, and must
    be one at which the modules `tool_call` and `json` resolve as in core:
    `::purrassist_core`, a renamed dependency, a re-export such as
    `crate::reexports::pa`, `::purrassist` (the facade re-exports both
    modules at its root) or `::purrassist::__core`. An empty path, a trailing
    `::`, a literal, generics and a repeated `crate =` are compile errors.
  - **Convert only usable calls.** `FromNode::from_node` reads a call's
    arguments exactly but does not decide whether the call may run; the
    arguments of an unresolved call are only the best tree reached. Convert
    the arguments of the calls in `ToolCalls::usable()` (outcome
    `CallOutcome::Resolved`), or of `ToolCalls::usable_accepting_guesses()`
    when best guesses of read-only tools are acceptable, never those of a
    mutating tool's best guess or an unresolved call.
  *Proved by* `crates/derive/tests/crate_override.rs`, the facade's
  `derive_facade::the_override_accepts_the_facade_root_a_hidden_core_and_a_reexport`,
  the derive's `write` unit tests on the prefix and the marker, the core test
  `tool_call::derive_support::tests::the_derive_abi_marker_is_named_after_the_crate_version`
  and the `compile_fail` doctests for a malformed or repeated `crate` path. A
  version-skewed derive/core pair cannot be built in-tree, so the skew error
  itself is not exercised.

Root `$ref` projection follows local references only (`#/definitions/…` or
`#/$defs/…`), up to 64 hops, with a cycle check; see C5.
