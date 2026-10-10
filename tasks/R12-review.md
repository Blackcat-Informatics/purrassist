# R12 review (G20, "Raise the published constants")

Scope: uncommitted diff vs 342f2d4 (cost.rs, recover.rs, tool_call_cost.rs, tool_call_cost_support, tool_call_scale.rs, RESOURCES.md, INTEGRATION.md, README.md, verify-published fixture).

## Evidence
1. Re-measured at 64 KiB (dense_markup test, --nocapture), work / peak scratch / evidence per byte:
   kimi 138.3 / 130.5 / 0.051; deepseek-v3 79.7 / 293.4 / 0.044; qwen nested 59.0 / 267.1 / 0.064; qwen calls 71.3 / 280.1 / 0.101.
   All match the R12-measurements table (1 MiB) within 1%. Margins: 176/138.3 = +27%, 368/293.4 = +25%, evidence 1/7=0.143 vs 0.101 = +41%. Claims hold.
2. Formula consistent across cost.rs, recover.rs Rustdoc (incl. doctest), RESOURCES.md constants table and formula block, INTEGRATION.md, README (176/368/⌈n/7⌉). resources_doc_matches_cost_constants and sized_budget_follows_the_published_formula pass and cover the new constant.
3. Analytical bound (29959n + 8E) text untouched and still correct; the doc explicitly says the per-byte constants are measured sizing, not a replacement. Remaining "32"/"48" greps: RESOURCES.md:622 and tool_call_cost.rs:523 concern the unparsed-markup scan (unrelated); RESOURCES.md:878 is deliberate history. No qualification JSON/script pins the constants (check-resource-limits schema-only passes).
4. Temporarily set old 32/48: repeated_parameters_cost_is_linear and dense_markup_fits_published_budget (all shapes) fail; restored (cost.rs diff is as before). Both use plain sized, no override.
5. Fixture diff: only the receipt budget (work 1001152->1006336, scratch 67110592->67122112, evidence 65536->65542) in both the text and structuredContent copies; arithmetic matches the formula for the 36-byte input.
6. Limits (v11 whitespace-chained superlinear work and scratch, v3 many-call array quadratic wall time, `{` 895 work/byte) are stated with figures, and as exhaustions not successes.
7. Tests run green: tool_call_cost, tool_call_goldens, tool_call_recover (115/56 passed); scripts/test-verify-published.py OK; make hygiene OK; clippy -D warnings clean. The 1 MiB corpus test was not run (19 s, corpus lane).

## Findings
REQUIRED: none.

ADVISORY
- A1. The `sized` Rustdoc and README/INTEGRATION say dense markup "recovers under this budget"; they do not mention the not-covered list (only RESOURCES.md does). One sentence pointing to "Dense markup / Not covered" in the sized Rustdoc would avoid overclaiming.
- A2. Mistral v3 many-call arrays: charged work is linear while wall time is quadratic (1.8 s at 64 KiB release, 1 MiB did not finish in 300 s), so charged work is not proportional to real work there. RESOURCES.md states it, but calls it a "tracked limitation" with no issue reference; the budget does not protect wall time for that shape. Name the tracking issue or say plainly that the budget does not bound wall time for it.
- A3. The dense tests deliberately avoid the v3 array and v11 whitespace-chain shapes, so those limits have no executable evidence in-tree (they are measurement-only claims).

VERDICT: PASS

## Orchestrator dispositions
- A1: fixed. Pointers to the RESOURCES.md not-covered list were added to the `sized` Rustdoc, INTEGRATION.md and the tool-calls README. Doctests, `RUSTDOCFLAGS=-Dwarnings cargo doc` and `make hygiene` (exit 0) pass.
- A2 and A3: superseded by user decision (verbatim, answering whether to fix the three new superlinear paths): "Fix (1) and (2) here, document (3)". The Mistral v11 whitespace chain becomes R13 and the Mistral v3 array wall time becomes R14, each with tests. The `{` case stays documented.
