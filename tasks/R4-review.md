# R4 review (G3 HIGH, G15, G17, A3)

Scope: uncommitted diff vs 4b80ade plus two new goldens. Read-only review; mutation runs were done on a /tmp copy (deleted).

## Evidence
- Suites pass: core `tool_call_goldens` + `tool_call_recover` (114/56), the `merge_fails_closed_in_either_order` unit test, host `tool_call_tools` (25), CLI `tool_calls_cli` (26), `test-fuzz.py` (68), `test-verify-published.py` (10). `cargo clippy --workspace --all-targets -D warnings` is clean. `cargo fmt --check` is clean.
- Built CLI on p7 (tools: read_file):
  - `--dialect hermes.v1` + exactly-one/no-content/no-reasoning + `--emit openai`: exit 4, resolved/`nothing_usable`, `passed:false`, usable 0/0, `emitted.output == []`. Diagnostics: "1 rejected call attempt beyond the one call..." and "rejected call attempt at bytes 75..163 ...". At HEAD this was exit 0, `run`.
  - A single valid call plus trailing whitespace, all strict flags: exit 0, `run`, usable 1/1, 1 emitted. No over-blocking.
  - Non-strict `Forced` on p7, hermes only: exit 0, `run`, passed true, 1/1, same as HEAD (HEAD binary built from a temporary worktree and compared). Forced does not fail on rejected attempts, so it is unchanged.
  - p7 with all dialects is genuinely Ambiguous (hermes reads 1 call + a rejected attempt, qwen/bare-json reads 2 calls). At HEAD the hermes alternative passed and read as `run`. Now both alternatives fail with usable 0/0 under exactly-one, no-content, or both (verified for each flag set), exit 3.
- Goldens: only the five `forced_*` files changed, each solely `usable_calls` strict/accepting_guesses 1->0, plus two new files. `tool_choice_none_with_calls` already had 0/0. The PINNED_* digests are not touched and their tests pass, so the digest rule is satisfied trivially.
- Published fixture: line 2 differs only in `tools[13].description` and `tools[13].inputSchema...tool_choice_strict.description` (checked by a JSON tree walk).
- Fuzz seed bit math, checked against the probe header and `request()`:
  - `0xe6` has bits 4-5 = `10` (AsContent). `0xc6` has bits 4-5 = `00` (Separate) and bits 6-7 = `11` (forced).
  - Trailer `f9` = exactly_one, `fa` = no content, `fc` = no reasoning.
  - Seeds 3 and 4 (index) are `0xc6`. Seed 5 keeps `0xe6`+`0xfc` and draws the request error. Seeds 6 and 7 are p7 attempts.
  - Mutation: reverting `\xc6` to `\xe6` makes `test-fuzz.py` fail ("seed 3 must test no-reasoning under separate reasoning"). The probe run test executes all 8 strict seeds to `ok`.
- Mutations on a /tmp copy; each fails named tests:
  - fail_closed disabled: `merge_fails_closed_in_either_order`.
  - rejected not counted: the two new goldens, `tool_call_recover::strict_choice_counts...` and the CLI test.
  - usable() guard removed: 7 goldens, host `failed_tool_choice_zeroes_the_usable_counts`, `strict_choice_counts_a_rejected_second_attempt`.
  - G15 check removed: the core, host and CLI invalid-input tests.
- Docs checked against code: INTEGRATION verdict table, strict option table, "What counts", usable-count paragraphs; README; docs/tool-calls/README; FUZZING; Rustdoc (`StrictForced`, `usable`, `usable_accepting_guesses`, `emitted_calls`, `Verdict`); the registry description and the `tool_choice_strict` schema description. All consistent.
- Diagnostics cap (G17): `strict()` emits at most one cardinality diagnostic (both counts), one content diagnostic ("and N more"), one rejected-attempt diagnostic and one reasoning diagnostic. It charges Work per visited entry and ScratchBytes per diagnostic. Merge charges per diagnostic and for the union. `strict_forced_diagnostics_are_capped` covers it.

## Adjudication of the deviations
(a) Accepted, with a caveat on the wording. `equivalent_options` folds contested-span options with the same calls and reasoning, and `fold_options` keeps the member with the greatest `body_evidence`. Any rejected attempt has positive reach (Malformed `at` is past the opener, Unterminated and UnsupportedDialect use the span end), while content has (0,0). So in the fold the reading that carries a rejected attempt always wins, which is the fail-closed direction. "Unreachable" is therefore true only as a consequence of that evidence ordering, not as a structural invariant. The merge-level fail-closed rule is the right defence in depth, and the forced unit test exercises it meaningfully in both orders (mutation-verified). The real hermes-vs-qwen p7 case differs in calls, so it is not a merge and is covered separately by the CLI and test checks above.
(b) Accepted. It takes the same `invalid_input` path as an undeclared forced tool name (`recognized()`, adjacent check), and the CLI exit is 4. Documented in INTEGRATION, the README and the registry description.
(c) Accepted. `run` = `recognized` + `conclude` in the same statement order. The only new code in `recognized` is the G15 check, which sits after the forced-name lookup and before `encoding`.

## Findings
REQUIRED: none.

ADVISORY:
1. `crates/cli/tests/tool_calls_cli.rs`: the new tests were inserted between the doc comment "The `--tool-choice-*` flags are a usage error without a tool name..." and its test. That comment now heads `strict_forced_flags_count_a_rejected_second_attempt`, and `strict_forced_flags_need_a_tool_name` lost its doc. Move the comment back.
2. The `merge` Rustdoc and the implementer's "folding makes it unreachable" claim rest on the evidence ordering (see (a)). One sentence naming that dependency would keep it honest. The second `merge` call site (the `indistinguishable` groups in `conclude`) can join readings whose calls differ. There the merged check fails closed, but the cardinality outcomes belong to the kept reading's calls. This is safe (usable 0) but unexercised by a test.
3. Count overlap: the qwen alternative of p7 reports "1 extra call and 1 rejected call attempt" although the bare-json call (87..136) sits inside the rejected span 75..163. It is over-counted but fail-closed, and the diagnostic is accurate as to what each count means.
4. Some Rustdoc and docs lines are long single lines (`emitted_calls`, FUZZING.md, README docs/tool-calls). Cosmetic.

VERDICT: PASS

## Orchestrator dispositions
- Advisory 1: fixed. The `strict_forced_flags_need_a_tool_name` doc comment is moved back onto its test; `tool_calls_cli` passes (26).
- Advisory 2: fixed. The `merge` Rustdoc now states that the keep order favours the attempt-reading member and that failing closed does not rely on it. The second merge call site is unchanged and safe per the review.
- Advisory 3: noted. The double count still fails closed.
