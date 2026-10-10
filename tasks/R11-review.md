# R11 review: project a root $ref declaration's parameters (G19)

Reviewed: uncommitted R11 tree vs HEAD c2f1916 (adapter.rs, scope.rs, toolset.rs, resolve.rs, recover.rs, tool_adapters.rs, tool_call_goldens.rs, 8 new mcp_* goldens, INTEGRATION.md). Read-only; scratch in /tmp (HEAD worktree removed, pruned).

## Gates (all on the R11 tree)
- `cargo test -q -p purrassist-core`: PASS (all suites + doctests).
- `cargo test -q -p purrassist-host -p purrassist-cli`: PASS.
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS.
- `make hygiene`: PASS.

## Evidence per check
(a) Draft-07 vs 2020-12 sibling semantics: correct. Draft-07: a `$ref` node is "overridden" (siblings noted as unmodeled evidence, not projected); root siblings of an `allOf` member still apply. 2020-12: node and target apply together. CLI probes: Draft-07 `$ref` + sibling `properties:{extra}` + closed target: `{q}` Resolved, `{q,extra}` Unresolved (exact schema rejects `extra`, projection does not declare it) so projection and validation agree. 2020-12 `$ref` + sibling `properties`/`required:[extra]`: call with q, extra, n Resolved; call missing `extra` Unresolved (required from the sibling honored).
(b) `required`: taken only from applying (conjunctive) schemas, never union branches. Probe: root `anyOf` of `{a required}` / `{b required}`: call `{b:3}` Resolved, exit 0, `b` typed via exact schema (`/anyOf/1/properties/b`). A `oneOf` of $ref branches with `k` int in A and string in B: names Unmodeled, calls validated by the exact schema. No false required. PASS.
(c) Closed scope: Error only when an applying schema closes it (and has no `patternProperties`/RESHAPING member) or every union branch does. Rejecting names outside the union of declared names is sound: a closing schema in a conjunction already rejects them. Draft-07 target `additionalProperties:false` + ignored sibling `properties` probe: no valid call rejected. PASS.
(d) Render-back byte-exact: `assert_render_back` checks, per declaration, all three wire forms (OpenAI, MCP incl. Draft-07, Anthropic) cross-rendered, `json_schema()` text, `parameters_text()`, and projected names. Used for FastMCP Draft-07, 2020-12 merged, 2020-12 target-alone, allOf, anyOf union, unprojectable set, nested-ref. PASS.
(e) Differential: git status shows no existing golden modified (only 8 new files). HEAD vs tree CLI, all 491 tracked goldens that carry `tools_openai`: outputs (exit code, stdout, stderr) identical after excluding `receipt.usage` only. No tracked golden used root `$ref`/allOf/anyOf in a tool schema, so the only diff is usage (see ADVISORY 1).
(f) FastMCP repro, `recover-tool-calls --tools fast.json --tools-format mcp`: Qwen3 XML gives `{"query":"rust","limit":5,"tags":["a","b"]}` Resolved exit 0; Hermes JSON gives `{"query":"rust","limit":5,"tags":["a"]}` Resolved exit 0. Typed (integer, array). PASS.
(g) Cost: `follow` charges Work per hop and ScratchBytes per node, `seen` is capped at `REF_HOPS`=64 and cycle-checked, no recursion (iterative through `follow`/`follow_ref`... bounded by the cap). Probes: 10k-hop chain and 10k-node cycle end in explicit `budget_exceeded: Work` in 0.03 s (the schema engine compile also charges); chains of 50/63/64/65 hops Resolved (65+ degrade to PassThrough). No unbounded recursion. PASS.

## Adjudication of implementer deviations
1. No prompt-form change: ACCEPTED. `render_declaration` for adapter tools prints the exact `parameters` text; asserted by `root_ref_prompt_form_declaration_prints_the_exact_schema` and the byte-exact render-back. The plan expectation was wrong; the doc (INTEGRATION.md) names it.
2. Unresolvable/external root `$ref` is an invalid declaration (schema compile rejects), PassThrough pins cover non-object, type-array, cyclic, `$ref:"#"`: ACCEPTED; documented and pinned.
3. Union/multi-declared names Unmodeled and not required; required from applying schemas; closed scope rule: ACCEPTED; sound per (b)/(c).

## Findings
REQUIRED: none.

ADVISORY:
1. `receipt.usage.work` rises for every adapter tool (plan() runs for all), e.g. a 50-branch union 82,279 to 93,969 work. Any pinned receipt/usage in published fixtures must be refreshed (orchestrator already handling scripts/data/verify-published).
2. Wide root unions cost more: a root `anyOf` of 500 one-property object branches Resolves at HEAD (755k work) but exhausts the default 1,009,962 Work budget on the tree (explicit `budget_exceeded`, not a silent failure; `RootPlan::add` dedupe and `declaring` are linear per name so growth is superlinear). Reasonable limit for realistic tools; note in RESOURCES/INTEGRATION if the budget story is touched in R12.
3. INTEGRATION.md claim that an external/missing root `$ref` is "invalid" was not independently re-run by me beyond the implementer's pin test (`unprojectable_root_ref_stays_pass_through_with_its_evidence`, which passes).

VERDICT: PASS

## Recheck

Gates after the follow-ups: full `cargo test -q -p purrassist-core` PASS; `python3 scripts/test-verify-published.py` OK; `root_union_projection_cost_is_linear` PASS.

O(n^2) fixes read: `RootPlan::add` now dedups through a `BTreeSet<(bool,String)>` (`known`); `RootLocations::declaring` is a map lookup (`declared`); `relative()` probes a `BTreeSet` `describing` at each `/` boundary, longest first (O(depth log n)). Sound. Residual: `RootPlan::merge` still uses `overridden.contains` (Vec scan), only reachable for Draft-07 `$ref` nodes (not per branch of a wide union in practice); negligible.

My exact probe: MCP tool `t`, schema `{"anyOf":[{"type":"object","properties":{"p<i>":{"type":"string"}}} x n]}` (distinct 2-4 char names, one string property per branch, no descriptions, no root type), recover-tool-calls --tools-format mcp, one hermes call `{"p1":"x"}`, default effort (budget work 1,010,208).
- n=500: HEAD resolves (work 755,079); tree STILL exhausts (budget_exceeded, work 1,010,161). n=600/650: HEAD resolves (904,679 / 979,479), tree exhausts. n=700: both exhaust. Tree ceiling is about 410 branches vs HEAD about 670 (uncapped `--effort deep`: 400 branches tree 991,439 vs HEAD 605,479; 500: 1,338,139 vs 755,079).
- Cause is NOT toolset construction: with a no-call input the tree is linear and about +6% over HEAD (n=500: 803,281 vs 748,651), matching the coordinator's 5-9%. With a call present, the extra work is superlinear: (tree minus no-call) = 31k, 98k, 349k, 535k at n=100/200/400/500 versus a flat ~6k at HEAD, roughly 1.8*n^2. Every branch's unmodeled keywords (`type`, `properties`, ~2n of them) are now "on path" for any call, so `Recovery::keywords` (recover.rs ~4572-4636) and its downstream consumers (validations/ledger/emission, ~3742) process ~2n entries per call, and something in that path is charged quadratically. The new test only measures `from_mcp_governed` (no call), so it cannot see this.
- The overrun is an explicit `budget_exceeded` (bounded, not silent), and 500 branches is a pathological tool, but the coordinator's claim that 500 one-property branches resolves under the default budget holds only without a call; with a call (the real use) it does not, and the cost is superlinear in the number of branches.

Findings: REQUIRED: none (bounded, explicit outcome, no wrong result). ADVISORY (strengthened): (1) find and linearize the per-call cost of a wide union's keyword evidence (or record branch keywords once per call rather than per branch/keyword), and extend `root_union_projection_cost_is_linear` to recover a call so it covers it; or (2) document that a root union of more than about 400 branches exhausts the default budget on a call.

VERDICT: PASS

## Recheck 2

Gates: full `cargo test -q -p purrassist-core` PASS (pinned digests, receipt fixtures); `python3 scripts/test-verify-published.py` OK; `cargo test -p purrassist-core --test tool_adapters root_union` (includes `root_union_call_recovery_cost_is_linear`) PASS.

Probe (same shape as Recheck 1, one hermes call, default budget 1,010,208), HEAD vs tree work:
- n=500: both resolve (755,079 vs 835,636, +10.7%).
- n=600: both resolve (904,679 vs 1,004,726; close to the cap).
- n=650: HEAD resolves (979,479); tree exhausts (`budget_exceeded`, 1,000,687).
- n=700: both exhaust.
The 500-branch case now resolves as at HEAD. The tree's ceiling is about 600-620 branches versus about 670 at HEAD, a roughly 10% work overhead that is linear in n (the superlinear per-call term is gone). Exhaustion remains an explicit `budget_exceeded`.

`Log` change read (recover.rs ~3328-3355): the index key is now `((start,end), constraint)`. Validation equality (`entries[index] == *validation`) implies equal constraint, so every possible duplicate is still found in the same sub-list; sub-lists keep insertion order, so the first-equal index returned is the same as before; `entries` is still append-only in call order, so receipt validation order and serialization are unchanged. Only the Work charge (`known.len()+1`) shrinks, which shows up only in `usage`.

Differential: HEAD CLI vs tree CLI over all 491 tracked goldens (receipts compared without `usage`): exit code, stdout and stderr identical for every one. No difference other than usage.

Findings: REQUIRED none. ADVISORY: the ~10% constant overhead on wide unions lowers the default-budget ceiling from about 670 to about 610 branches; acceptable and stated.

VERDICT: PASS
