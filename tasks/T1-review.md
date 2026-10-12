# Task 1 independent review

## Reviewed scope

Uncommitted diff in the worktree plus the untracked `scripts/check-docs.py`, `scripts/test-check-docs.py` and `docs/known-limitations.json`. Inputs: plan.md "Task 1" and the Task 1 contract rows, output-rules.md, validation.md "Task 1". No source file was modified; probes ran on temp copies in /tmp.

## Evidence

- `python3 scripts/check-docs.py` -> "Documentation facts, links, wording and known limitations match their sources.", rc 0.
- `python3 scripts/test-check-docs.py` -> Ran 87 tests, OK.
- `python3 scripts/check-repository.py` -> verified, rc 0. `python3 scripts/test-repository-audit.py` -> Ran 39 tests, OK.
- `cargo test -p purrassist-core --test tool_call_dialects` -> 17 passed, 0 failed (includes `built_in_ids_equal_the_documented_dialect_pages`, tool_call_dialects.rs:675).
- `python3 scripts/check-release.py` -> release set and publish.yml verified, rc 0.
- Mutation probes (temp copies, `check()` over the mutated tree). All were detected by the named rule:
  - FUZZING "across ten" -> "nine": `[fuzz-targets]`.
  - QUALIFICATION table row "Ten targets" -> "Nine": `[fuzz-targets]` plus `[banned-phrase]`.
  - QUALIFICATION:175 "ten mutation" -> "nine" / "eight": `[fuzz-targets]` (banned-phrase too for nine).
  - Receipt wire v3 -> v4: `[receipt-version]`.
  - README link to docs/NOPE.md: `[links]`.
  - `](#known-limitations)` removed from either `make fuzz` row (QUALIFICATION:36 and 38): `[fuzz-row-limits]`.
  - "issue\nbranch" wrapped across lines: `[banned-phrase]` at the first line. "after review\n   and merge" (indented wrap): detected.
  - `.stage/x` reference: `[stage-references]`.
  - Workspace version 0.2.0 in IMPLEMENTATION: `[workspace-version]`.
  - `hermes.v1` removed from a README dialect list: `[dialect-ids]`.
  - Windows run link changed so it differs from the "Verify run N" label: `[windows-run]`.
  - A baseline cell for tool-call changed from none: `[fuzz-baseline-table]`.
  - RELEASE order changed: `[release-set]`.
  - A known-limitation issue changed to #99 in FUZZING: `[known-limitations-section]`.
  - An unmutated tree copy: no errors (the probe harness is sound).
- validation.md baseline over the f11b45e versions flags all 17 banned-phrase sites, including the wrapped ones (RELEASE 6, QUALIFICATION 7, 142, 226).

## Deliverables against the contract

- Facts sourced, not hard-coded: release order from `RELEASE_ORDER` (loaded from check-release.py), version from Cargo.toml, targets from fuzz.py `TARGETS` (plus Makefile loop/comment cross-check), receipt version parsed from contract.rs, dialect ids from the `docs/tool-calls/dialects/*.md` stems (in `check()`), known limitations from the JSON. Present and real.
- Zero-match failure: `Report.fact()` adds "fact sentence not found" when nothing matches. `DIALECT_LIST_DOCS` and `DIALECT_COUNT_DOCS` make absence an error. `FactSentencesAreRequired` (test-check-docs.py:681-696) runs an absent-sentence fixture for every fact.
- Every rule has a failing fixture: test classes cover release-set, workspace-version, fuzz-targets, receipt-version, dialect-ids/count, links, stage-references, banned-phrase, known-limitations source and section, fuzz-row-limits, baseline-table, archived-evidence, stage2-heading and windows-run.
- Whitespace-collapsed banned phrases: `BANNED_PHRASES` (check-docs.py:44-57) holds the twelve contracted regexes, matched on `collapse()`d text with `collapse_offsets()` for line numbers. Tests cover a newline wrap, an indented wrap and case insensitivity.
- Known-limitations structure: `docs/known-limitations.json` has the ten issues (#6, 7, 10, 11, 12, 13, 14, 16, 17, 18), each with issue/title/impact/blocks_release=false, validated strictly (exact keys, duplicates, one-line values, `blocks_release` must be false). QUALIFICATION and FUZZING each have exactly one `## Known limitations` heading whose issue set, titles and impacts are compared with the JSON. The CHANGELOG is accounted for when present.
- Fuzz-row links: `check_fuzz_rows` requires every `make fuzz` table row to link `#known-limitations` and to state the "#N and #N signatures" stop. Verified by probe. The rows affected by #6/#11 state the stop (QUALIFICATION:36, 38).
- Dialect assertion: tool_call_dialects.rs:675-700 compares sorted `DialectId::BUILT_IN` ids with sorted page stems. Passes.
- SPDX on tracked `*.md`: check-repository.py:254-276 plus 5 tests in test-repository-audit.py. Scope is `git ls-files` minus test data. Passes on the checkout.
- Stale facts corrected in README, RELEASE, QUALIFICATION, IMPLEMENTATION, FUZZING, CONTRIBUTING, SECURITY and the four crate READMEs. No banned phrase remains. The FUZZING baseline table has `none` rows for tool-call and tool-call-ref. The Stage 2 heading is "Historical: Stage 2 record bound to `66b3756`". The IMPLEMENTATION delivery sequence marks delivered stages.

## Real callers and consumers

- Makefile:16-17 (`hygiene`, which `check` depends on at Makefile:6) runs `scripts/check-docs.py` and `scripts/test-check-docs.py`. The markdown SPDX scan runs inside the existing `check-repository.py` hygiene line. The Rust assertion runs under `cargo test` in `make check`.
- Scope: `markdown_names()` uses `git ls-files -- '*.md'` filtered by `EXCLUDED_PREFIXES = ('.stage/', 'vendor/', 'crates/core/tests/data/')`; `.stage` is also rejected as a reference in public docs. test-check-docs.py:120 pins the scope.
- Caveat: an untracked new `.md` is outside the scope until `git add`. This matches the existing index-based scan.

## Accuracy against repo evidence

- PR #2 / `699d9ff` is "feat(1): complete 0.1.0 scope ... (#2)". The tool-call work (issue #3, PR #5, 943fa04), #8 and #15/#19 are in history, so "merged since" is true.
- The Windows sentence's run 36702475819 and commit dc23bc8 come from the issue #1 comments; dc23bc8 is an ancestor of 699d9ff; verify.yml `windows-engines` runs the core, syntax, derive, facade, host and cli tests. The sentence says the run does not cover later `main`. Nothing claims hosted CI proves current main.
- The 2026-09-28 observation for five crates and the 2026-10-12 derive record (trusted publisher pending owner) match issue-analysis R9b. Nothing claims derive is configured.
- "execution 682", #6 and #11 match FUZZING lines ~925-990. The archived-evidence commit binding is checked against the `docs/qualification/*.json` source commits.
- Implemented vs planned: #12-#18 are stated as not built in; the delivery sequence says verified publication is open.

## Output rules

- No agent attribution in the diff, scripts or JSON. The only keyword hits are "Anthropic SSE" in the #17 title (product naming).
- No hosted-CI provider account or payment wording. Hosted CI appears only as out of scope (QUALIFICATION Windows sentence) or in the existing release procedure.

## Findings (none blocking)

- N1 (minor wording): the Known limitations intros in docs/QUALIFICATION.md and docs/FUZZING.md say "The list is generated from one source". It is hand-written and verified by `check-docs.py`, not generated. Suggest "kept in step with" or "checked against". Fix in Task 2 or 3 when touching these files.
- N2 (minor wording): the QUALIFICATION Windows sentence (around lines 146-150) ends "hosted CI is outside this stage's scope". "This stage" is process-bound wording in a public document; "this record does not cover hosted CI" is cleaner.
- N3 (note): the status line mixes a PR number (#2) with issue numbers (#3, #8, #15). It is correct and the plan mandates it.
- Style: both new scripts use the `importlib` load precedent, SPDX headers, stdlib only and the same layout as the neighbouring check scripts. Matches.

## Verdict

All Task 1 contract items are present, real, wired into `make hygiene`, and mutation-resistant. Findings are cosmetic wording only.

VERDICT: PASS
