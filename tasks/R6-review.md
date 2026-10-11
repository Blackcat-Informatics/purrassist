# R6 review (PR #19, issue #15, HEAD b7475b4 plus uncommitted working tree)

## Reviewed scope
Spec: remediation-plan.md "### R6"; gap-analysis.md G4, G6, G7, G9. A-1 verify only; A-2 out of scope.
Files in the uncommitted diff: README.md, crates/purrassist/README.md, crates/derive/src/lib.rs,
crates/derive/tests/derive.rs, crates/core/src/tool_call/derive_support.rs, docs/RELEASE.md,
scripts/check-repository.py, scripts/test-repository-audit.py. No source was modified by this review.
Falsification used scratch copies in /tmp (/tmp/scr, /tmp/rv) only.

## Evidence
- `cargo test -q -p purrassist-core -p purrassist-derive -p purrassist --locked`: exit 0, every
  `test result` line is ok (includes derive.rs 32 passed and all doctests).
- `cargo test -q -p purrassist-derive --doc --locked -- --list | grep -i readme`:
  `crates/derive/src/../README.md - ReadmeDoctests (line 16): test`. The derive README has one code
  block (a complete program), so the example runs.
- `python3 scripts/check-repository.py`: exit 0 ("... core and derive boundaries verified").
- `python3 scripts/test-repository-audit.py`: Ran 34 tests, OK (includes the 5 new DeriveExample tests).
- `make hygiene`: exit 0, all sub-scripts OK.
- Facade doc: crates/purrassist/src/lib.rs line 5 has `#![doc = include_str!("../README.md")]`, so the
  marked example in crates/purrassist/README.md is doctested (facade doctests listed at lines 12/29/46).
- Root README block vs doctested block: extracted between markers, equal (True), 712 bytes, exactly
  two fences (one complete `rust` block with `use`, struct, asserts and the `# Ok::<..>(())` line).
- Drift falsification: copied README.md and crates/purrassist/README.md to /tmp/rv, changed `let` to
  `lEt` inside the markers; `check_derive_example(Path('/tmp/rv'))` returned
  `['README.md: derive example differs from the doctested copy in crates/purrassist/README.md; copy it over byte for byte']`.
  On the real tree it returns `[]`. The function accepts a root argument.
- E0277 spot checks (nightly rustc 1.100.0, scratch copy /tmp/scr, separate target dir):
  - Changed the added codes at derive_support.rs lines 245 (ToolRepeatable) and 271 (ToolPattern) to
    E0308: both doctests failed with "Some expected error codes were not found: [E0308]".
  - Changed lines 374 and 380 (Option<T>/ToolParameter) to E0308: both failed. So all four added
    E0277 annotations are enforced and correct. Repo tree unchanged afterwards (git status shows only the
    eight expected files plus .stage).
- Plain compile_fail audit (G7): in the scratch copy, replaced every plain `compile_fail` in
  crates/derive/src/lib.rs with the bogus code E9999 and collected rustc output. The only plain
  doctest whose output contained any rustc error code was # 32 (`Option<Self>`): its first error is
  the derive's `compile_error!` ("field `a` refers to `A` itself ...") plus an incidental
  `E0072 recursive type`. All other plain cases are `compile_error!`-only.
  Judgement on doctest 32: leaving it plain is correct. The intended failure is the derive's message;
  pinning E0072 would let the doctest pass if the derive stopped emitting its error (E0072 comes from
  rustc alone), so the pin would assert the wrong thing. Plain is no weaker than the other
  compile_error cases, whose residual is already scoped in the plan.
- Codes present overall: E0277 x13, E0391 x3, E0080 x4, E0433 x1; A-1 doctest 30 is
  `compile_fail,E0277` and passes.
- Line width: scanned added lines of `git diff origin/main...HEAD` plus the working tree in all touched
  .md/.rs/.py/.yml files, excluding tables, URLs and code. The four prose lines added to RELEASE.md by
  the branch (>100 cols) are the ones the working tree rewraps; none remain. lib.rs has no prose over 100
  (only the code line at 56, tables, and URL link definitions). Remaining >100 hits in diffs are code
  (Rust/Python/test strings, workflow shell lines).

## Requirement check
- G4: ExplicitOverDocEnum added to derive.rs with doc comment + `#[tool(description)]` + variant docs
  (one with `#[param(description = "Beta  value.")]` showing whitespace collapse, one undocumented), and
  the assertion expects `"Explicit enum text.\n- A: Alpha value.\n- B: Beta value."`. Passes. Meets spec.
- G6: README doctest wiring present (cfg(doctest) struct); drift check implemented, wired into `main()`,
  5 negative/positive tests; root and facade copies identical. Meets spec.
- G7: codes added where a rustc code exists; compile_error-only cases plain. Meets spec.
- G9: lib.rs enum-folding paragraph and RELEASE.md paragraphs rewrapped; content unchanged in meaning.
- A-1: verified (see above).

## Findings (non-blocking)
1. docs/RELEASE.md line 92 (105 cols) is over 100 but exists unchanged on origin/main, so it is not added
   by this PR and is outside the G9 list. Optional tidy.
2. crates/derive/src/write.rs line 110 (110 cols) is a `//` comment enumerating tokens in backticks (code
   illustration, added earlier in the branch). Treated as code; optional tidy.
3. check_derive_example checks that the marker blocks are equal, and it does not check that the block is
   the only example or that markers are in the doctested file beyond the exact-one rule. Adequate for the
   spec.

No required behavior is missing; all requested commands pass.

VERDICT: PASS
