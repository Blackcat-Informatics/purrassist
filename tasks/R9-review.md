# R9 review (G10, G13)

## Checks run
- `python3 scripts/test-verify-published.py`: 10 tests OK. `make hygiene`: OK (14 tests). `python3 scripts/check-repository.py`: OK.
- Replay diff: exactly 2 added lines per file (ids 20, 21), no other change.
  - id 20 (`exactly_one`, `allow_content:false`, reasoning allowed): constraint `tool_choice=forced[exactly-one,no-content]:list_dir`, passed, `reasoning_blocks` non-empty, verdict `run`, usable 1. Matches the claim.
  - id 21 (also `allow_reasoning:false`): constraint `forced[exactly-one,no-content,no-reasoning]:list_dir`, passed false, diagnostic names reasoning 0..49, verdict `nothing_usable`, usable 0. Matches the claim.
  - Fixture holds a non-empty `reasoning_blocks` and a `tool_choice=forced[` constraint (plan check met).

## (1) Completeness of R9-compat.md
Independently derived from `git diff 943fa04` (pub items, variants, fields, const fn, derives, CLI/registry/MCP). All of these appear in R9-compat.md: ForcedStrict, ToolChoiceCardinality, UnmodeledScopeBoundary and OPERATOR_COUNT 46, ToolCalls.reasoning_blocks, ReasoningSegment.text, ArgumentShape.scopes, ParameterShape.nesting, ArgTree.collected, ArgTree::value and empty_object losing const, StrictForced and its builders, ReasoningBlock, CollectedMember, Nesting, ScopeGraph, ScopeMember, ScopeNode, the three cost constants, the NodeId and Derivation derive changes (A19: cfg(test)/ignored), CLI flags, MCP `tool_choice_strict`, output schema, descriptions. No omissions found.

## (2) INTEGRATION.md `## Compatibility`, spot checks (all accurate)
usable_calls zero on failed choice (recover.rs `if !self.tool_choice.passed`, replay id 21); reasoning_blocks in output (replay); constraint family (doctests); budgets 176/368/7 (cost.rs; `div_ceil`); unmodeled-scope-boundary guess (RULES.md/edit.rs); root `$ref` projection and anchors `#c5-...`, `#cli`, `#strict-forced-tool-choice` exist; disable_parallel mapping (recover.rs from_anthropic); `(depth - 1) / 3` about 42 (INTEGRATION L789); 610/670 (R11 review, validation.md); reasoning closer at/after text start (recognizer.rs invariant). 

## Findings
REQUIRED 1. The plan says the public-type changes "go in both INTEGRATION.md and the PR body". The INTEGRATION.md section covers the wire/CLI/behaviour/budget changes but lists NO Rust API break (A1-A8, A13-A16): `ToolChoice::ForcedStrict`, `UnresolvedReason::ToolChoiceCardinality`, `EditOperator::UnmodeledScopeBoundary` (only the rule id appears), new pub fields `ToolCalls.reasoning_blocks`, `ReasoningSegment.text`, `ArgumentShape.scopes`, `ParameterShape.nesting`, `ArgTree.collected`, `ArgTree::value/empty_object` no longer const, the new types, and the `usable()`/`emitted_calls()` empty-on-failed-choice change. It says "Rust API details are in the pull request description", yet the current PR #9 body does not contain them (grep for ToolChoiceCardinality/ArgTree/OPERATOR_COUNT/ScopeGraph/RECOVER_WORK_PER_BYTE: 0 hits; it names only 3 breaks). Either add a "Rust API" list to INTEGRATION.md Compatibility or update the PR body from R9-compat.md sections A/A' before declaring R9 done (PR body edit is a pending action; the plan's "grep confirms ... the PR body" check is unmet).

ADVISORY: the section does not state the C9 limits (local `$ref` only, 64 hops); covered elsewhere in C5.

VERDICT: BLOCKED

## Recheck
- `### Rust API` holds all of A1-A12 (Breaking) and A13-A18 (Added); A19 correctly omitted. Compared item by item with my derived list: complete. Root `$ref` limits sentence present. Lists are well-formed markdown; "budget items above" refers to the preceding Budgets list; anchor `C5` exists. `check-repository.py` passes.
- ADVISORY (not blocking): the intro paragraph (INTEGRATION.md L1413) still says "Rust API details are in the pull request description", now stale since the subsection exists; reword to "see Rust API below".
- The PR body still needs the same lists at handoff (coordinator states it will).

VERDICT: PASS
