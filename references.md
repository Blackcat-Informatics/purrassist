# Referenced artifacts — issue #1

Everything the issue names, checked against disk. Facts only: whether a
file is a viable port source, or may be relicensed, is decided in the plan.
Paths resolve against the main checkout, then against a sibling checkout
only when that repo is named (first path component, or in the prose just
before the reference). "not here; exists in sibling(s)" is NOT resolved.

## Paths

| Reference | Exists | Resolved in | Kind | Lines | Licence header |
|---|---|---|---|---|---|
| `docs/IMPLEMENTATION.md` | yes | repo | file | 145 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/QUALIFICATION.md` | yes | repo | file | 251 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `README.md` | yes | repo | file | 431 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/core/src/matching.rs` | yes | repo | file | 1260 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/core/tests/string_selection.rs` | yes | repo | file | 326 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/core/tests/matching_quality.rs` | yes | repo | file | 379 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `AGENTS.md` | yes | repo | file | 32 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `verify.yml` | NO | repo | — | — | — |
| `check-repository.py` | NO | repo | — | — | — |
| `check-release.py` | NO | repo | — | — | — |
| `package-workspace.py` | NO | repo | — | — | — |
| `measure-resources.py` | NO | repo | — | — | — |
| `fuzz-json.py` | NO | repo | — | — | — |
| `check-wasm.py` | NO | repo | — | — | — |
| `vendor/tooling.json` | yes | repo | file | 166 | "license": "MIT", |
| `src/lib.rs` | NO | not here; exists in sibling(s): lillith_defense, lillith_draws, nyarlathotep, nyarlathotep-campaign-controller, nyarlathotep-campaign-controller-current (+2 more) | — | — | — |
| `publish.yml` | NO | repo | — | — | — |
| `docs/MATCHING-QUALITY.md` | yes | repo | file | 475 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `matching_quality.rs` | NO | repo | — | — | — |
| `crates/core/src/contract.rs` | yes | repo | file | 1895 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `schema/compile.rs` | NO | repo | — | — | — |
| `build.rs` | NO | repo | — | — | — |
| `pattern.rs` | NO | repo | — | — | — |
| `schema/recovery.rs` | NO | repo | — | — | — |
| `recovery/proposals.rs` | NO | repo | — | — | — |
| `schema/evaluate.rs` | NO | repo | — | — | — |
| `helpers.rs` | NO | repo | — | — | — |
| `crates/core/tests/allocation_oracle.rs` | yes | repo | file | 1511 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/core/src/provenance.rs` | yes | repo | file | 855 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/core/src/wire.rs` | yes | repo | file | 472 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `edits/fuzzy.rs:123` | NO | repo | — | — | — |
| `edits/unified.rs:85` | NO | repo | — | — | — |
| `crates/syntax/src/lib.rs:340` | yes | repo | file | 912 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/schema/receipt.schema.json` | yes | repo | file | 246 | — |
| `receipt.schema.json` | NO | repo | — | — | — |
| `crates/syntax/Cargo.toml` | yes | repo | file | 35 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/syntax/src/alloc.rs` | yes | repo | file | 602 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `malloc/calloc/realloc/free` | NO | repo | — | — | — |
| `allocation.rs` | NO | repo | — | — | — |
| `docs/RESOURCES.md` | yes | repo | file | 1154 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `scripts/generate-unicode.py` | yes | repo | file | 501 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `vendor/dependencies.json` | yes | repo | file | 597 | — |
| `crates/core/src/pattern/` | yes | repo | dir | — | — |
| `built-ins/RegExp/property-escapes/generated` | NO | repo | — | — | — |
| `crates/core/src/schema/format/` | yes | repo | dir | — | — |
| `vocabularies.rs` | NO | repo | — | — | — |
| `schema_conformance.rs` | NO | repo | — | — | — |
| `optional/format/` | NO | repo | — | — | — |
| `optional/format-assertion.json` | NO | repo | — | — | — |
| `formats.rs` | NO | repo | — | — | — |
| `crates/core/src/schema/algebra/` | yes | repo | dir | — | — |
| `if/then/else` | NO | repo | — | — | — |
| `unevaluatedProperties/Items` | NO | repo | — | — | — |
| `tests/recursive_schemas.rs` | NO | repo | — | — | — |
| `tests/corruption_oracle.rs` | NO | repo | — | — | — |
| `crates/core/src/patch.rs` | yes | repo | file | 593 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `patch_laws.rs` | NO | repo | — | — | — |
| `markdown_structure.rs` | NO | repo | — | — | — |
| `markdown_sections.rs:246-258` | NO | repo | — | — | — |
| `crates/host/src/tools/registry.rs` | yes | repo | file | 423 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `tools/list` | NO | repo | — | — | — |
| `crates/cli/tests/protocol.rs` | yes | repo | file | 898 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/core/tests/preservation_properties.rs` | yes | repo | file | 1497 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `scripts/fuzz-json.py` | NO | repo | — | — | — |
| `crates/core/Cargo.toml` | yes | repo | file | 121 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `.cache/` | yes | repo | dir | — | — |
| `scripts/test-fuzz.py` | yes | repo | file | 1345 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/FUZZING.md` | yes | repo | file | 1076 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/core/tests/data/evaluation/` | yes | repo | dir | — | — |
| `tests/correction_evaluation.rs` | NO | repo | — | — | — |
| `docs/qualification/resources-linux-x86_64.json` | yes | repo | file | 585 | — |
| `crates/host/tests/crash_recovery.rs` | yes | repo | file | 374 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `prepared.json` | NO | repo | — | — | — |
| `crates/host/examples/portable.rs` | yes | repo | file | 72 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/host/tests/helpers.rs` | yes | repo | file | 231 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/wasm-harness` | yes | repo | dir | — | — |
| `scripts/run-wasm.mjs` | yes | repo | file | 24 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `scripts/check-wasm.py` | yes | repo | file | 93 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `.mjs` | NO | repo | — | — | — |
| `docs/DEPENDENCIES.md` | yes | repo | file | 323 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `check-resource-limits.py` | NO | repo | — | — | — |
| `.github/workflows/publish.yml` | yes | repo | file | 118 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `rust-lang/crates-io-auth-action` | NO | repo | — | — | — |
| `scripts/verify-published.py` | yes | repo | file | 614 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `release-qualification.yml` | NO | repo | — | — | — |
| `docs/RELEASE.md` | yes | repo | file | 226 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `scripts/check-packages.py` | yes | repo | file | 98 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `scripts/test-verify-published.py` | yes | repo | file | 501 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/HELPERS.md` | yes | repo | file | 65 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/EDIT-APPLICATION.md` | yes | repo | file | 232 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/CANCELLATION.md` | yes | repo | file | 112 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `.worktrees/1-release-qualification` | NO | repo | — | — | — |
| `/api/v1/trusted_publishing/github_configs` | NO | absolute | — | — | — |
| `MATCHING-QUALITY.md` | NO | repo | — | — | — |
| `.git/hooks` | yes | repo | dir | — | — |
| `SECURITY.md` | yes | repo | file | 50 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `property-escapes/generated` | NO | repo | — | — | — |
| `format-assertion.json` | NO | repo | — | — | — |
| `verify-published.py` | NO | repo | — | — | — |
| `Cargo.lock` | yes | repo | file | 326 | — |
| `paudley/1-release-qualification` | NO | repo | — | — | — |
| `CONSTITUTION.md` | NO | not here; exists in sibling(s): gmeow-ontology, lillith_decodes, lillith_memetics | — | — | — |
| `paudley/366-centralize-overlapping-helpers-one-home` | NO | repo | — | — | — |
| `UPSTREAM.json` | NO | repo | — | — | — |
| `.cache/package-archives/1a8509cbfb2e-v9bmb44g` | NO | repo | — | — | — |
| `a.txt` | NO | repo | — | — | — |
| `scripts/check-release.py` | yes | repo | file | 251 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `prior-art-assessment.md` | NO | repo | — | — | — |
| `docs/tool-calls/RULES.md` | yes | repo | file | 158 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/core/tests/tool_call_rules.rs` | yes | repo | file | 2264 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `CONTRIBUTING.md` | yes | repo | file | 24 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `CONTRIBUTING.md:20` | yes | repo | file | 24 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `scripts/fuzz.py` | yes | repo | file | 2598 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `validation.md` | NO | repo | — | — | — |
| `.stage/` | NO | not here; exists in sibling(s): katamari, lillith_browser, lillith_decodes, purrdf | — | — | — |
| `vendor/` | yes | repo | dir | — | — |
| `crates/core/tests/data/` | yes | repo | dir | — | — |
| `crates/core/tests/tool_call_dialects.rs` | yes | repo | file | 1154 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `check-docs.py` | NO | repo | — | — | — |
| `docs/known-limitations.json` | NO | repo | — | — | — |
| `docs/TOOLS.md` | NO | repo | — | — | — |
| `tools/call` | NO | repo | — | — | — |
| `temperature.schema.json` | NO | repo | — | — | — |
| `temperature.input.txt` | NO | repo | — | — | — |
| `crates/purrassist/README.md` | yes | repo | file | 80 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `.purrassist/` | yes | repo | dir | — | — |
| `docs.rs/.../latest` | NO | repo | — | — | — |
| `crates/cli/README.md` | yes | repo | file | 88 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `.github/workflows/release-qualification.yml` | yes | repo | file | 51 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `CHANGELOG.md` | NO | not here; exists in sibling(s): HA, cf-config, gmeow-gts, llama-cpp-rs-logit-loom, logit-loom (+5 more) | — | — | — |
| `docs/social-preview.svg` | NO | not here; exists in sibling(s): gmeow-gts, gmeow-ontology, logit-loom, purrdf, purrdf-408-wip-source (+1 more) | — | — | — |
| `docs/social-preview.png` | NO | not here; exists in sibling(s): gmeow-gts, gmeow-ontology, logit-loom, purrdf, purrdf-408-wip-source (+1 more) | — | — | — |
| `docs/assets.json` | NO | repo | — | — | — |
| `docs/social-preview.png.license` | NO | repo | — | — | — |
| `tasks/T5-repo-settings.md` | NO | repo | — | — | — |
| `check-packages.py` | NO | repo | — | — | — |
| `server.json` | NO | repo | — | — | — |
| `test-check-release.py` | NO | repo | — | — | — |
| `scripts/check-docs.py` | NO | repo | — | — | — |
| `scripts/test-check-docs.py` | NO | repo | — | — | — |
| `SECURITY.md:5` | yes | repo | file | 50 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/QUALIFICATION.md#known-limitations` | NO | repo | — | — | — |
| `scripts/check-repository.py` | yes | repo | file | 390 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `vendor/licenses/` | yes | repo | dir | — | — |
| `tasks/T5-review.md` | NO | repo | — | — | — |
| `test-check-docs.py` | NO | repo | — | — | — |
| `test-repository-audit.py` | NO | repo | — | — | — |
| `examples/` | yes | repo | dir | — | — |
| `OFL.txt` | NO | repo | — | — | — |
| `io.github.Blackcat-Informatics/purrassist` | NO | repo | — | — | — |

### History: `docs/IMPLEMENTATION.md`

- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `5398ba175 fix(tool_call): report every length stop as finish_reason "length"`
- `ce66b7baf docs(tool_call): tighten the public surface and finish the doc wiring`
- `b3e16ab46 feat(tool_call): bare-json.v1 dialect`
- `937091368 feat(tool_call): harmony.v1 dialect`
- `e8c8072ed feat(tool_call): anthropic-tool-use.v1 dialect`
- `33a6ce87b feat(tool_call): kimi-k2.v1 dialect`
- `cf75220d6 feat(tool_call): deepseek-v3.v1 dialect`
- `0ec3f3a53 feat(tool_call): deepseek-v3.1.v1 dialect`
- `ca1123604 feat(tool_call): mistral-v11.v1 dialect`

### History: `docs/QUALIFICATION.md`

- `6cbb6282f docs(derive): near-miss nested keys, patch compatibility rule, qualification evidence (#15)`
- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `381306500 fix(tool-call): closers inside values are judged by what follows, never by layout (adversary pass 6, F14, F19)`
- `fecda8934 test: split make check into focused tests; corpus and fuzz get their own lanes`
- `75c8f1134 fix(tool-call): keep stray closers and call bodies out of values and content (adversary pass 5, F13–F18)`
- `396133344 feat(tool-call): weigh closer text and call markup inside values by evidence`
- `73e6a137c fix(tool-call): never let a repair override a reading that needs none`
- `b605ce55a fix(tool_call): resynchronize past any failing call, read misspelled tokens, cut calls at opening reasoning markers`
- `7a5a6c2d5 fix(tool_call): read written markup, resynchronize groups, keep rejected fences out of prose`
- `7c919c9bc feat(tool_call): type unmodeled-schema XML text by the exact schema`

### History: `README.md`

- `d0a99e991 test(derive): close remaining evidence and doc gaps (#15)`
- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `69c6b5cd7 fix(tool-call): strict choice counts rejected attempts and survives reading merges (#8)`
- `28b2b3a05 feat(tool-call): strict forced tool choice — exactly one call, no content, no reasoning (#8)`
- `5398ba175 fix(tool_call): report every length stop as finish_reason "length"`
- `ce66b7baf docs(tool_call): tighten the public surface and finish the doc wiring`
- `4d1592023 fix(cli): derive the recover-tool-calls exit code from what is usable`
- `efa99f1d1 feat(tool_call): report unparsed call markup from unsupported or disallowed dialects`
- `b3e16ab46 feat(tool_call): bare-json.v1 dialect`
- `937091368 feat(tool_call): harmony.v1 dialect`

### History: `crates/core/src/matching.rs`

- `227d4ebcb feat(contract): receipt v3 with byte-exact sources and wire-value results`
- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `23caff46d feat: derive every tool surface from one operation registry`
- `8e360f3c1 fix: make scratch charges independent of pointer width`
- `974ccac6e feat: publish receipts from a provenance derivation forest`
- `48d4e942c feat: enforce scratch accounting through budgeted containers`
- `2e286923d feat: resolve strings through a refinement lattice with protected tokens`
- `be82a33d2 feat: select strings with explicit ambiguity and evidence`
- `a95c9b7b8 feat: expose governed alias-aware string matching`
- `d0fa500e2 feat: propagate caller-owned cancellation through governed engines and tools`

### History: `crates/core/tests/string_selection.rs`

- `227d4ebcb feat(contract): receipt v3 with byte-exact sources and wire-value results`
- `2e286923d feat: resolve strings through a refinement lattice with protected tokens`
- `be82a33d2 feat: select strings with explicit ambiguity and evidence`

### History: `crates/core/tests/matching_quality.rs`

- `2e286923d feat: resolve strings through a refinement lattice with protected tokens`

### History: `AGENTS.md`

- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `vendor/tooling.json`

- `c1fa81dac build(tooling): repin Node to v26.11.1`
- `300f7cffe test: make profiling prerequisites explicit and retain startup diagnostics`
- `22d33135e test: preserve malformed HTML and correct independent email retries`
- `53f23662a test: generalize guided fuzzing and preserve strict differential evidence`
- `095776765 build: enforce qualification tooling and archive provenance`
- `788270396 build: add trusted-publishing release workflow and published-package verification`
- `24787e1a3 test: compare helper transcripts and browser WASM with native output`
- `ea50c8965 test: add dependency-free coverage-guided JSON qualification`
- `b2c9cba01 docs: record clean-commit resource measurements and runtime provenance`
- `c2f5d18f2 test: measure allocation and memory costs with external profilers`

### History: `docs/MATCHING-QUALITY.md`

- `e93eaf883 fix(tool-call): $ref and anyOf scopes declare their schema's member names (#8)`
- `c168175df feat(tool-call): read arrays of objects written as repeated nested parameters (#8)`
- `381306500 fix(tool-call): closers inside values are judged by what follows, never by layout (adversary pass 6, F14, F19)`
- `75c8f1134 fix(tool-call): keep stray closers and call bodies out of values and content (adversary pass 5, F13–F18)`
- `396133344 feat(tool-call): weigh closer text and call markup inside values by evidence`
- `73e6a137c fix(tool-call): never let a repair override a reading that needs none`
- `b605ce55a fix(tool_call): resynchronize past any failing call, read misspelled tokens, cut calls at opening reasoning markers`
- `7a5a6c2d5 fix(tool_call): read written markup, resynchronize groups, keep rejected fences out of prose`
- `7c919c9bc feat(tool_call): type unmodeled-schema XML text by the exact schema`
- `3415a3b14 feat(tool_call): tolerate JSON syntax slips in call envelopes`

### History: `crates/core/src/contract.rs`

- `c2f191658 perf(provenance): derivation and forest indexes stay logarithmic (#8)`
- `e93eaf883 fix(tool-call): $ref and anyOf scopes declare their schema's member names (#8)`
- `e7a301968 fix(tool_call): size candidates to the turn and charge validation memory`
- `b35ec82a7 feat(json): push-based tolerant JSON, byte-step patterns, linear extraction`
- `2d9183c7f refactor(search): extract a generic best-first search engine`
- `227d4ebcb feat(contract): receipt v3 with byte-exact sources and wire-value results`
- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `61d6986de fix: release scratch meter handles when retaining charges`
- `23caff46d feat: derive every tool surface from one operation registry`
- `de8e8fda0 docs: shorten the ScratchString summary line for stable clippy`

### History: `crates/core/tests/allocation_oracle.rs`

- `e7a301968 fix(tool_call): size candidates to the turn and charge validation memory`
- `2e2365d26 feat(tool_call): correction evaluation, published cost bound, and bench`
- `b35ec82a7 feat(json): push-based tolerant JSON, byte-step patterns, linear extraction`
- `66b375677 fix(pattern): govern compilation and witness storage`
- `7e8f7f5a9 test: count oracle allocations per thread`
- `48d4e942c feat: enforce scratch accounting through budgeted containers`

### History: `crates/core/src/provenance.rs`

- `c2f191658 perf(provenance): derivation and forest indexes stay logarithmic (#8)`
- `b7fe57c39 feat(grammar): weighted grammar engine and tool-call recognizer`
- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `2b43d223a fix: restore rustdoc and syntax-oracle gates`
- `23caff46d feat: derive every tool surface from one operation registry`
- `974ccac6e feat: publish receipts from a provenance derivation forest`

### History: `crates/core/src/wire.rs`

- `1fbe235e4 feat(tool_call): tool_call.recover.v1 recovery operation`
- `227d4ebcb feat(contract): receipt v3 with byte-exact sources and wire-value results`
- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `23caff46d feat: derive every tool surface from one operation registry`
- `974ccac6e feat: publish receipts from a provenance derivation forest`
- `b87ac5404 fix: bind ambiguous JSON recovery evidence to each alternative`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `crates/syntax/src/lib.rs:340`

- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `2bd5d02d1 fix: preserve governed resource outcomes through host operations`
- `23caff46d feat: derive every tool surface from one operation registry`
- `65ef9081e feat: bound tree-sitter allocations through the governor`
- `974ccac6e feat: publish receipts from a provenance derivation forest`
- `d0fa500e2 feat: propagate caller-owned cancellation through governed engines and tools`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `docs/schema/receipt.schema.json`

- `227d4ebcb feat(contract): receipt v3 with byte-exact sources and wire-value results`
- `23caff46d feat: derive every tool surface from one operation registry`
- `974ccac6e feat: publish receipts from a provenance derivation forest`

### History: `crates/syntax/Cargo.toml`

- `ed9f9ddf2 perf: measure and enforce source-bound resource limits`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `crates/syntax/src/alloc.rs`

- `2bd5d02d1 fix: preserve governed resource outcomes through host operations`
- `65ef9081e feat: bound tree-sitter allocations through the governor`

### History: `docs/RESOURCES.md`

- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `8bca69ee4 test(fuzz): reach nested-parameter array items and scope graphs; pin the strict trailer claim (#8)`
- `3743aedf3 perf(grammar): Mistral v3 call arrays cost linear, charged time (#8)`
- `bdb102a30 perf(grammar): consecutive chained calls segment incrementally (#8)`
- `8fb63a424 perf(tool-call)!: raise the published per-byte recovery budget to cover dense markup (#8)`
- `3a19a2d47 perf(tool-call): repeated parameters cost linear time and scratch (#8)`
- `381306500 fix(tool-call): closers inside values are judged by what follows, never by layout (adversary pass 6, F14, F19)`
- `fecda8934 test: split make check into focused tests; corpus and fuzz get their own lanes`
- `75c8f1134 fix(tool-call): keep stray closers and call bodies out of values and content (adversary pass 5, F13–F18)`
- `396133344 feat(tool-call): weigh closer text and call markup inside values by evidence`

### History: `scripts/generate-unicode.py`

- `3ed24fc2f feat: evaluate every JSON Schema format and report unsupported vocabularies`
- `4dfd1b432 feat: match ECMA-262 patterns with a symbolic derivative engine`
- `2e286923d feat: resolve strings through a refinement lattice with protected tokens`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `vendor/dependencies.json`

- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `2e2365d26 feat(tool_call): correction evaluation, published cost bound, and bench`
- `34e8ac54d feat(schema): draft-07 dialect for validation, witnesses, and recovery`
- `a2da56a81 build: port structural helper census and strict ledger tooling`
- `095776765 build: enforce qualification tooling and archive provenance`
- `3ed24fc2f feat: evaluate every JSON Schema format and report unsupported vocabularies`
- `4dfd1b432 feat: match ECMA-262 patterns with a symbolic derivative engine`
- `b53ce9fbd feat: qualify filesystem durability on Windows`
- `2e286923d feat: resolve strings through a refinement lattice with protected tokens`
- `fa30c00ee feat: decode Markdown entities from pinned standards data`

### History: `crates/core/src/pattern/`

- `9a47d3c02 fix(grammar): read multi-byte terminals after class runs from their first byte`
- `8496413da test(fuzz): tool-call fuzz target and WASM parity for every dialect`
- `b7fe57c39 feat(grammar): weighted grammar engine and tool-call recognizer`
- `b35ec82a7 feat(json): push-based tolerant JSON, byte-step patterns, linear extraction`
- `66b375677 fix(pattern): govern compilation and witness storage`
- `a3b361e04 fix(pattern): preserve assertion context during witness search`
- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `856d2ecdb docs: qualify bounded search and allocation scale evidence`
- `bd1b656f0 fix: repair Markdown structure and anchored pattern semantics`
- `a45e2c8d0 feat: generate JSON Schema witnesses with an algebraic pipeline`

### History: `crates/core/src/schema/format/`

- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `a45e2c8d0 feat: generate JSON Schema witnesses with an algebraic pipeline`
- `3ed24fc2f feat: evaluate every JSON Schema format and report unsupported vocabularies`

### History: `crates/core/src/schema/algebra/`

- `34e8ac54d feat(schema): draft-07 dialect for validation, witnesses, and recovery`
- `d982fb755 fix(schema): honor format vocabularies during witness search`
- `1e94beaeb fix(schema): scope witness constraints to applicable types`
- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `2b43d223a fix: restore rustdoc and syntax-oracle gates`
- `a45e2c8d0 feat: generate JSON Schema witnesses with an algebraic pipeline`

### History: `crates/core/src/patch.rs`

- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `7690c4a66 feat: unify edits as a patch algebra with structural Markdown operations`

### History: `crates/host/src/tools/registry.rs`

- `69c6b5cd7 fix(tool-call): strict choice counts rejected attempts and survives reading merges (#8)`
- `248d07a0b feat(tool-call): marker-free reasoning blocks beside the reasoning ranges (#8)`
- `28b2b3a05 feat(tool-call): strict forced tool choice — exactly one call, no content, no reasoning (#8)`
- `b65687e31 feat(tool_call): publish a usability verdict beside the reading's status`
- `347cb0e24 fix(tool_call): read caller declarations byte-exact on CLI and MCP`
- `efa99f1d1 feat(tool_call): report unparsed call markup from unsupported or disallowed dialects`
- `b3e16ab46 feat(tool_call): bare-json.v1 dialect`
- `937091368 feat(tool_call): harmony.v1 dialect`
- `e8c8072ed feat(tool_call): anthropic-tool-use.v1 dialect`
- `33a6ce87b feat(tool_call): kimi-k2.v1 dialect`

### History: `crates/cli/tests/protocol.rs`

- `32ceb6442 feat(host): recover_tool_calls MCP tool and recover-tool-calls CLI`
- `e4bda6e78 fix: preserve and report multiline setext heading edits`
- `ba2a35cc3 feat: expose witness generation as a tool`
- `23caff46d feat: derive every tool surface from one operation registry`
- `be82a33d2 feat: select strings with explicit ambiguity and evidence`
- `a95c9b7b8 feat: expose governed alias-aware string matching`
- `4103c50a0 feat: apply source-bound edit plans through shared tools`
- `ad69fbda9 feat: service live MCP cancellation with bounded tool workers`
- `cf8433858 feat: inspect and edit source-bound Markdown reference definitions`
- `97f69cdae feat: edit nested Markdown table columns with structural validation`

### History: `crates/core/tests/preservation_properties.rs`

- `227d4ebcb feat(contract): receipt v3 with byte-exact sources and wire-value results`
- `4f055c5c8 test: qualify edit preservation with generated adversarial documents`

### History: `crates/core/Cargo.toml`

- `fecda8934 test: split make check into focused tests; corpus and fuzz get their own lanes`
- `8496413da test(fuzz): tool-call fuzz target and WASM parity for every dialect`
- `2e2365d26 feat(tool_call): correction evaluation, published cost bound, and bench`
- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `53f23662a test: generalize guided fuzzing and preserve strict differential evidence`
- `ed9f9ddf2 perf: measure and enforce source-bound resource limits`
- `ea50c8965 test: add dependency-free coverage-guided JSON qualification`
- `c2f5d18f2 test: measure allocation and memory costs with external profilers`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `scripts/test-fuzz.py`

- `8bca69ee4 test(fuzz): reach nested-parameter array items and scope graphs; pin the strict trailer claim (#8)`
- `69c6b5cd7 fix(tool-call): strict choice counts rejected attempts and survives reading merges (#8)`
- `8496413da test(fuzz): tool-call fuzz target and WASM parity for every dialect`
- `22d33135e test: preserve malformed HTML and correct independent email retries`
- `53f23662a test: generalize guided fuzzing and preserve strict differential evidence`

### History: `docs/FUZZING.md`

- `8bca69ee4 test(fuzz): reach nested-parameter array items and scope graphs; pin the strict trailer claim (#8)`
- `69c6b5cd7 fix(tool-call): strict choice counts rejected attempts and survives reading merges (#8)`
- `248d07a0b feat(tool-call): marker-free reasoning blocks beside the reasoning ranges (#8)`
- `28b2b3a05 feat(tool-call): strict forced tool choice — exactly one call, no content, no reasoning (#8)`
- `fcbab863e fix(tool-call): cover call bodies after markup debris in rejected attempts (fuzz)`
- `fb522afce ci: gate pull requests on the published-transcript replay (adversary pass 6, F20)`
- `75c8f1134 fix(tool-call): keep stray closers and call bodies out of values and content (adversary pass 5, F13–F18)`
- `396133344 feat(tool-call): weigh closer text and call markup inside values by evidence`
- `73e6a137c fix(tool-call): never let a repair override a reading that needs none`
- `b605ce55a fix(tool_call): resynchronize past any failing call, read misspelled tokens, cut calls at opening reasoning markers`

### History: `crates/core/tests/data/evaluation/`

- `e93eaf883 fix(tool-call): $ref and anyOf scopes declare their schema's member names (#8)`
- `c168175df feat(tool-call): read arrays of objects written as repeated nested parameters (#8)`
- `381306500 fix(tool-call): closers inside values are judged by what follows, never by layout (adversary pass 6, F14, F19)`
- `75c8f1134 fix(tool-call): keep stray closers and call bodies out of values and content (adversary pass 5, F13–F18)`
- `396133344 feat(tool-call): weigh closer text and call markup inside values by evidence`
- `73e6a137c fix(tool-call): never let a repair override a reading that needs none`
- `b605ce55a fix(tool_call): resynchronize past any failing call, read misspelled tokens, cut calls at opening reasoning markers`
- `7a5a6c2d5 fix(tool_call): read written markup, resynchronize groups, keep rejected fences out of prose`
- `7c919c9bc feat(tool_call): type unmodeled-schema XML text by the exact schema`
- `3415a3b14 feat(tool_call): tolerate JSON syntax slips in call envelopes`

### History: `docs/qualification/resources-linux-x86_64.json`

- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `300f7cffe test: make profiling prerequisites explicit and retain startup diagnostics`
- `79e5d3705 test: record final frozen-source resource qualification`
- `4ca11dcec test: bind final resource measurements to repaired qualification inputs`
- `275de19db test: refresh source-bound portable resource qualification`
- `ed9f9ddf2 perf: measure and enforce source-bound resource limits`
- `b2c9cba01 docs: record clean-commit resource measurements and runtime provenance`

### History: `crates/host/tests/crash_recovery.rs`

- `1af659394 test(files): qualify conditional undo after every crash checkpoint`
- `9b493b33e fix(files): preserve temporary artifacts without proven ownership`
- `b1034e488 fix: flush retried journal pieces through writable handles`
- `0b7c7de05 fix: make Windows durability code lint-clean and fail explicitly`
- `b53ce9fbd feat: qualify filesystem durability on Windows`

### History: `crates/host/examples/portable.rs`

- `24787e1a3 test: compare helper transcripts and browser WASM with native output`

### History: `crates/host/tests/helpers.rs`

- `227d4ebcb feat(contract): receipt v3 with byte-exact sources and wire-value results`
- `2bd5d02d1 fix: preserve governed resource outcomes through host operations`
- `974ccac6e feat: publish receipts from a provenance derivation forest`
- `48d4e942c feat: enforce scratch accounting through budgeted containers`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `crates/wasm-harness`

- `5398ba175 fix(tool_call): report every length stop as finish_reason "length"`
- `ccd54b1ee test(fuzz): guard the F5/F7 verdict contract in the tool-call probe`
- `b3e16ab46 feat(tool_call): bare-json.v1 dialect`
- `937091368 feat(tool_call): harmony.v1 dialect`
- `e8c8072ed feat(tool_call): anthropic-tool-use.v1 dialect`
- `33a6ce87b feat(tool_call): kimi-k2.v1 dialect`
- `cf75220d6 feat(tool_call): deepseek-v3.v1 dialect`
- `0ec3f3a53 feat(tool_call): deepseek-v3.1.v1 dialect`
- `ca1123604 feat(tool_call): mistral-v11.v1 dialect`
- `1f80b3bb7 feat(tool_call): mistral-v3.v1 dialect`

### History: `scripts/run-wasm.mjs`

- `24787e1a3 test: compare helper transcripts and browser WASM with native output`

### History: `scripts/check-wasm.py`

- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `51ed930ab ci: enforce complete qualification and exact tool identities`
- `24787e1a3 test: compare helper transcripts and browser WASM with native output`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `docs/DEPENDENCIES.md`

- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `34e8ac54d feat(schema): draft-07 dialect for validation, witnesses, and recovery`
- `f218e8401 feat(audit): enforce a dependency-free, network-free, ambient-free core`
- `3246d6579 refactor: consolidate duplicate helpers and enforce strict census`
- `300f7cffe test: make profiling prerequisites explicit and retain startup diagnostics`
- `22d33135e test: preserve malformed HTML and correct independent email retries`
- `53f23662a test: generalize guided fuzzing and preserve strict differential evidence`
- `a2da56a81 build: port structural helper census and strict ledger tooling`
- `095776765 build: enforce qualification tooling and archive provenance`
- `788270396 build: add trusted-publishing release workflow and published-package verification`

### History: `.github/workflows/publish.yml`

- `8d8d9a25a ci(release): publish.yml header names the derive crate and the preflight (#15)`
- `b7475b4fd build(release): publish preflight checks crate names and owners before publishing (#15)`
- `e97a97367 feat(derive): zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)] (#15)`
- `51ed930ab ci: enforce complete qualification and exact tool identities`
- `788270396 build: add trusted-publishing release workflow and published-package verification`

### History: `scripts/verify-published.py`

- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `ed81ffe5f fix: bound installed acceptance and prove file transaction undo`
- `095776765 build: enforce qualification tooling and archive provenance`
- `868d19613 fix: locate packaged archives wherever cargo writes them and keep lockfiles`
- `788270396 build: add trusted-publishing release workflow and published-package verification`

### History: `docs/RELEASE.md`

- `4d8a48e82 docs(derive): scope the nested near-miss rule to the matching policy (#15)`
- `6cbb6282f docs(derive): near-miss nested keys, patch compatibility rule, qualification evidence (#15)`
- `d0a99e991 test(derive): close remaining evidence and doc gaps (#15)`
- `b7475b4fd build(release): publish preflight checks crate names and owners before publishing (#15)`
- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `fb522afce ci: gate pull requests on the published-transcript replay (adversary pass 6, F20)`
- `1a8509cbf fix: isolate release archive verification from shared Cargo caches`
- `42b44609c docs: reconcile implemented capabilities and qualification contracts`
- `788270396 build: add trusted-publishing release workflow and published-package verification`
- `a68beebbc build: isolate same-version package verification`

### History: `scripts/check-packages.py`

- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `095776765 build: enforce qualification tooling and archive provenance`
- `788270396 build: add trusted-publishing release workflow and published-package verification`
- `0a8412b78 build: reject unsafe crate archive entries`
- `de8db9d7b build: qualify release archives and publish order`
- `fa30c00ee feat: decode Markdown entities from pinned standards data`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `scripts/test-verify-published.py`

- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `ed81ffe5f fix: bound installed acceptance and prove file transaction undo`
- `868d19613 fix: locate packaged archives wherever cargo writes them and keep lockfiles`
- `788270396 build: add trusted-publishing release workflow and published-package verification`

### History: `docs/HELPERS.md`

- `2bd5d02d1 fix: preserve governed resource outcomes through host operations`
- `23caff46d feat: derive every tool surface from one operation registry`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `docs/EDIT-APPLICATION.md`

- `1af659394 test(files): qualify conditional undo after every crash checkpoint`
- `9b493b33e fix(files): preserve temporary artifacts without proven ownership`
- `bd1b656f0 fix: repair Markdown structure and anchored pattern semantics`
- `42b44609c docs: reconcile implemented capabilities and qualification contracts`
- `7690c4a66 feat: unify edits as a patch algebra with structural Markdown operations`
- `9994d4d61 fix: open Windows replacement handles with delete access`
- `b53ce9fbd feat: qualify filesystem durability on Windows`
- `4103c50a0 feat: apply source-bound edit plans through shared tools`

### History: `docs/CANCELLATION.md`

- `1fbe235e4 feat(tool_call): tool_call.recover.v1 recovery operation`
- `42b44609c docs: reconcile implemented capabilities and qualification contracts`
- `ad69fbda9 feat: service live MCP cancellation with bounded tool workers`
- `d0fa500e2 feat: propagate caller-owned cancellation through governed engines and tools`

### History: `SECURITY.md`

- `b08161e66 docs: state unsafe exception and allocator evidence scope precisely`
- `b53ce9fbd feat: qualify filesystem durability on Windows`
- `65ef9081e feat: bound tree-sitter allocations through the governor`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `Cargo.lock`

- `4085bf31a feat(purrassist): re-export the tool derives through the facade (#15)`
- `e97a97367 feat(derive): zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)] (#15)`
- `a2da56a81 build: port structural helper census and strict ledger tooling`
- `24787e1a3 test: compare helper transcripts and browser WASM with native output`
- `b53ce9fbd feat: qualify filesystem durability on Windows`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `scripts/check-release.py`

- `b7475b4fd build(release): publish preflight checks crate names and owners before publishing (#15)`
- `e97a97367 feat(derive): zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)] (#15)`
- `788270396 build: add trusted-publishing release workflow and published-package verification`
- `de8db9d7b build: qualify release archives and publish order`

### History: `docs/tool-calls/RULES.md`

- `e93eaf883 fix(tool-call): $ref and anyOf scopes declare their schema's member names (#8)`
- `c168175df feat(tool-call): read arrays of objects written as repeated nested parameters (#8)`
- `381306500 fix(tool-call): closers inside values are judged by what follows, never by layout (adversary pass 6, F14, F19)`
- `75c8f1134 fix(tool-call): keep stray closers and call bodies out of values and content (adversary pass 5, F13–F18)`
- `396133344 feat(tool-call): weigh closer text and call markup inside values by evidence`
- `73e6a137c fix(tool-call): never let a repair override a reading that needs none`
- `b605ce55a fix(tool_call): resynchronize past any failing call, read misspelled tokens, cut calls at opening reasoning markers`
- `7a5a6c2d5 fix(tool_call): read written markup, resynchronize groups, keep rejected fences out of prose`
- `749f53d76 fix(tool_call): keep calls around a malformed one, read misspelled closers, cut calls at reasoning markers`
- `7c919c9bc feat(tool_call): type unmodeled-schema XML text by the exact schema`

### History: `crates/core/tests/tool_call_rules.rs`

- `917553ef1 fix(tool-call): reasoning markers inside JSON strings of repeated parameters stay value text (#8)`
- `31115823b fix(tool-call): linear item numbering keyed by canonical name; decode evidence by span (#8)`
- `e93eaf883 fix(tool-call): $ref and anyOf scopes declare their schema's member names (#8)`
- `c168175df feat(tool-call): read arrays of objects written as repeated nested parameters (#8)`
- `381306500 fix(tool-call): closers inside values are judged by what follows, never by layout (adversary pass 6, F14, F19)`
- `396133344 feat(tool-call): weigh closer text and call markup inside values by evidence`
- `b605ce55a fix(tool_call): resynchronize past any failing call, read misspelled tokens, cut calls at opening reasoning markers`
- `749f53d76 fix(tool_call): keep calls around a malformed one, read misspelled closers, cut calls at reasoning markers`
- `7c919c9bc feat(tool_call): type unmodeled-schema XML text by the exact schema`
- `a539e96ce fix(tool_call): keep calls whose arguments hold invalid UTF-8`

### History: `CONTRIBUTING.md`

- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `CONTRIBUTING.md:20`

- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `scripts/fuzz.py`

- `8bca69ee4 test(fuzz): reach nested-parameter array items and scope graphs; pin the strict trailer claim (#8)`
- `69c6b5cd7 fix(tool-call): strict choice counts rejected attempts and survives reading merges (#8)`
- `4b80ade59 fix(tool-call): reasoning text never reverses; redacted thinking is opaque (#8)`
- `28b2b3a05 feat(tool-call): strict forced tool choice — exactly one call, no content, no reasoning (#8)`
- `fcbab863e fix(tool-call): cover call bodies after markup debris in rejected attempts (fuzz)`
- `75c8f1134 fix(tool-call): keep stray closers and call bodies out of values and content (adversary pass 5, F13–F18)`
- `396133344 feat(tool-call): weigh closer text and call markup inside values by evidence`
- `73e6a137c fix(tool-call): never let a repair override a reading that needs none`
- `b605ce55a fix(tool_call): resynchronize past any failing call, read misspelled tokens, cut calls at opening reasoning markers`
- `7a5a6c2d5 fix(tool_call): read written markup, resynchronize groups, keep rejected fences out of prose`

### History: `vendor/`

- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `c1fa81dac build(tooling): repin Node to v26.11.1`
- `2e2365d26 feat(tool_call): correction evaluation, published cost bound, and bench`
- `34e8ac54d feat(schema): draft-07 dialect for validation, witnesses, and recovery`
- `300f7cffe test: make profiling prerequisites explicit and retain startup diagnostics`
- `22d33135e test: preserve malformed HTML and correct independent email retries`
- `53f23662a test: generalize guided fuzzing and preserve strict differential evidence`
- `a2da56a81 build: port structural helper census and strict ledger tooling`
- `095776765 build: enforce qualification tooling and archive provenance`
- `788270396 build: add trusted-publishing release workflow and published-package verification`

### History: `crates/core/tests/data/`

- `a49e81bc5 feat(tool-call): project a root $ref declaration's parameters (#8)`
- `7136da715 test(receipt): refresh the invalid-UTF-8 tool-call receipt's usage (#8)`
- `917553ef1 fix(tool-call): reasoning markers inside JSON strings of repeated parameters stay value text (#8)`
- `31115823b fix(tool-call): linear item numbering keyed by canonical name; decode evidence by span (#8)`
- `69c6b5cd7 fix(tool-call): strict choice counts rejected attempts and survives reading merges (#8)`
- `4b80ade59 fix(tool-call): reasoning text never reverses; redacted thinking is opaque (#8)`
- `e93eaf883 fix(tool-call): $ref and anyOf scopes declare their schema's member names (#8)`
- `248d07a0b feat(tool-call): marker-free reasoning blocks beside the reasoning ranges (#8)`
- `28b2b3a05 feat(tool-call): strict forced tool choice — exactly one call, no content, no reasoning (#8)`
- `c168175df feat(tool-call): read arrays of objects written as repeated nested parameters (#8)`

### History: `crates/core/tests/tool_call_dialects.rs`

- `dd95ec822 test: make fixture path matching portable to Windows`
- `b3e16ab46 feat(tool_call): bare-json.v1 dialect`
- `937091368 feat(tool_call): harmony.v1 dialect`
- `e8c8072ed feat(tool_call): anthropic-tool-use.v1 dialect`
- `33a6ce87b feat(tool_call): kimi-k2.v1 dialect`
- `cf75220d6 feat(tool_call): deepseek-v3.v1 dialect`
- `0ec3f3a53 feat(tool_call): deepseek-v3.1.v1 dialect`
- `ca1123604 feat(tool_call): mistral-v11.v1 dialect`
- `1f80b3bb7 feat(tool_call): mistral-v3.v1 dialect`
- `1ba176c97 feat(tool_call): llama3-json.v1 dialect`

### History: `crates/purrassist/README.md`

- `d0a99e991 test(derive): close remaining evidence and doc gaps (#15)`
- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `ce66b7baf docs(tool_call): tighten the public surface and finish the doc wiring`
- `42b44609c docs: reconcile implemented capabilities and qualification contracts`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `crates/cli/README.md`

- `28b2b3a05 feat(tool-call): strict forced tool choice — exactly one call, no content, no reasoning (#8)`
- `b65687e31 feat(tool_call): publish a usability verdict beside the reading's status`
- `347cb0e24 fix(tool_call): read caller declarations byte-exact on CLI and MCP`
- `4d1592023 fix(cli): derive the recover-tool-calls exit code from what is usable`
- `20b380b07 fix(tool_call): scope default call ids to the turn`
- `32ceb6442 feat(host): recover_tool_calls MCP tool and recover-tool-calls CLI`
- `cf8433858 feat: inspect and edit source-bound Markdown reference definitions`
- `97f69cdae feat: edit nested Markdown table columns with structural validation`
- `c68140502 feat: insert and remove nested Markdown table body rows`
- `e54dce716 feat: plan source-preserving nested Markdown table cell edits`

### History: `.github/workflows/release-qualification.yml`

- `fecda8934 test: split make check into focused tests; corpus and fuzz get their own lanes`
- `51ed930ab ci: enforce complete qualification and exact tool identities`
- `788270396 build: add trusted-publishing release workflow and published-package verification`
- `de8db9d7b build: qualify release archives and publish order`

### History: `SECURITY.md:5`

- `b08161e66 docs: state unsafe exception and allocator evidence scope precisely`
- `b53ce9fbd feat: qualify filesystem durability on Windows`
- `65ef9081e feat: bound tree-sitter allocations through the governor`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `scripts/check-repository.py`

- `d0a99e991 test(derive): close remaining evidence and doc gaps (#15)`
- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `2e2365d26 feat(tool_call): correction evaluation, published cost bound, and bench`
- `f218e8401 feat(audit): enforce a dependency-free, network-free, ambient-free core`
- `095776765 build: enforce qualification tooling and archive provenance`
- `24787e1a3 test: compare helper transcripts and browser WASM with native output`
- `4dfd1b432 feat: match ECMA-262 patterns with a symbolic derivative engine`
- `2e286923d feat: resolve strings through a refinement lattice with protected tokens`
- `fa30c00ee feat: decode Markdown entities from pinned standards data`
- `6788ccb9e feat: index Markdown containers and source-bound context`

### History: `vendor/licenses/`

- `c1fa81dac build(tooling): repin Node to v26.11.1`
- `300f7cffe test: make profiling prerequisites explicit and retain startup diagnostics`
- `a2da56a81 build: port structural helper census and strict ledger tooling`
- `095776765 build: enforce qualification tooling and archive provenance`
- `788270396 build: add trusted-publishing release workflow and published-package verification`
- `24787e1a3 test: compare helper transcripts and browser WASM with native output`
- `b53ce9fbd feat: qualify filesystem durability on Windows`
- `ea50c8965 test: add dependency-free coverage-guided JSON qualification`
- `c2f5d18f2 test: measure allocation and memory costs with external profilers`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `examples/`

- `be82a33d2 feat: select strings with explicit ambiguity and evidence`
- `a95c9b7b8 feat: expose governed alias-aware string matching`
- `4103c50a0 feat: apply source-bound edit plans through shared tools`
- `922b36fa9 feat: plan source-preserving Markdown list item edits`
- `4bd905573 feat: inspect and replace nested Markdown section bodies`
- `cf8433858 feat: inspect and edit source-bound Markdown reference definitions`
- `97f69cdae feat: edit nested Markdown table columns with structural validation`
- `c68140502 feat: insert and remove nested Markdown table body rows`
- `e54dce716 feat: plan source-preserving nested Markdown table cell edits`
- `792cd882e feat: add governed caller-owned embedding matching`

## Identifiers (files containing each at HEAD)

- `main`: 254
- `be82a33`: 0
- `resolve_string`: 7
- `ghprsq`: 0
- `unsafe`: 20
- `xtask`: 0
- `std::fs::rename`: 0
- `sleep`: 4
- `not`: 756
- `resolve`: 737
- `rank`: 54
- `match_strings`: 8
- `charge`: 221
- `release_scratch`: 6
- `resolve_governed`: 16
- `Resolved`: 273
- `CancellationToken`: 32
- `ScratchString`: 5
- `try_reserve`: 3
- `Drop`: 39
- `ScratchLease`: 10
- `Pattern::compile_governed`: 13
- `to_json`: 114
- `finish`: 576
- `Violation`: 6
- `active`: 27
- `retain_source`: 15
- `with`: 805
- `and`: 1148
- `Pattern::is_match`: 6
- `usage.scratch`: 0
- `BudgetExceeded`: 90
- `AlternativeReceipt`: 6
- `output`: 282
- `EditPlan`: 37
- `AppliedEdit`: 6
- `tree_sitter::set_allocator`: 4
- `ParseOptions`: 1
- `std::alloc::System`: 3
- `Tree`: 82
- `Send`: 10
- `std::sync::Once`: 4
- `Parser::new`: 2
- `Language`: 21
- `inspect`: 40
- `apply_checked_governed`: 5
- `QueryCursor`: 2
- `Cancelled`: 47
- `peak_scratch_bytes`: 29
- `parse`: 255
- `ast`: 261
- `derive`: 214
- `backtrack`: 31
- `generate`: 148
- `unicode`: 496
- `avoid`: 26
- `NoCandidate`: 28
- `date`: 812
- `time`: 174
- `true`: 460
- `false`: 410
- `Schema::compile`: 52
- `pattern`: 135
- `patternProperties`: 29
- `propertyNames`: 16
- `additionalProperties`: 71
- `dependentSchemas`: 19
- `contains`: 186
- `Ambiguous`: 96
- `Patch`: 12
- `apply`: 148
- `invert`: 21
- `compose`: 35
- `commute`: 9
- `Stale`: 6
- `apply_edit`: 9
- `apply_file`: 14
- `rebase_from`: 7
- `insert_section`: 10
- `remove_section`: 10
- `move_section`: 10
- `rename_heading`: 11
- `renumber_list`: 8
- `TableRowEdit::Move`: 7
- `TableColumnEdit::Move`: 7
- `add_reference`: 7
- `relabel_reference`: 8
- `remove_reference`: 8
- `unlink_reference`: 7
- `call`: 807
- `plan_markdown_structure`: 9
- `plan_markdown_section`: 8
- `recover_files`: 8
- `commit_files`: 6
- `effort`: 70
- `plan_replacement`: 7
- `plan_diff`: 6
- `inspect_code`: 10
- `node`: 168
- `CARGO_TARGET_DIR`: 4
- `FlushFileBuffers`: 6
- `FILE_FLAG_BACKUP_SEMANTICS`: 2
- `Busy`: 5
- `NotApplied`: 5
- `Pending`: 13
- `cdylib`: 2
- `portable_main`: 2
- `qualification`: 35
- `CARGO_HOME`: 5
- `mcp`: 46
- `success`: 73
- `purrassist`: 321
- `rank_governed`: 9
- `paths`: 75
- `match_paths`: 6
- `matching_quality`: 3
- `string_selection`: 5
- `numeric_identity_guard_is_explicit_and_budgeted`: 1
- `ScratchMeter`: 8
- `ScratchVec`: 24
- `ScratchMap`: 14
- `total_scratch_charged`: 17
- `usage`: 91
- `prepare_helper`: 8
- `complete_helper`: 6
- `distinguishing`: 24
- `RECEIPT_VERSION`: 5
- `usage.total_scratch_charged`: 6
- `Usage::merge`: 0
- `core.hooksPath`: 0
- `wasm`: 38
- `size_of`: 25
- `ScratchCharge`: 21
- `de8e8fd`: 0
- `Pattern::generate`: 8
- `b53ce9f`: 0
- `b1034e4`: 0
- `MoveFileExW`: 6
- `DELETE`: 7
- `ERROR_ACCESS_DENIED`: 3
- `Bidi_Class`: 3
- `Joining_Type`: 3
- `Guess`: 101
- `decode`: 156
- `validate_json`: 11
- `oracle_pattern_match`: 1
- `Patch::invert`: 4
- `TextEdit`: 27
- `diff`: 205
- `rebase`: 14

## Workspace packages named

- `purrassist`
- `purrassist-cli`
- `purrassist-core`
- `purrassist-derive`
- `purrassist-helper-census`
- `purrassist-host`
- `purrassist-syntax`
- `wasm-harness`

## URLs

- https://github.com/Blackcat-Informatics/purrassist/blob/main/.goals
- https://github.com/Blackcat-Informatics/purrassist/blob/main/docs/IMPLEMENTATION.md
- https://github.com/Blackcat-Informatics/purrassist/blob/main/docs/QUALIFICATION.md
- https://github.com/
- https://crates.io/api/v1/crates/
- https://crates.io/api/v1/crates/$c/0.0.0
- https://json-schema.org/draft/2020-12/vocab/format-assertion
- https://example.invalid/vocab/x
- https://crates.io/api/v1/crates/$c/0.1.0
- https://github.com/Blackcat-Informatics/purrassist/actions/runs/36702475819
- https://github.com/Blackcat-Informatics/purrassist/actions/runs/36707816916
- https://github.com/Blackcat-Informatics/purrassist/actions/runs/36732604500
- https://github.com/Blackcat-Informatics/purrassist/actions/runs/36734534499
- https://github.com/Blackcat-Informatics/purrassist/pull/2
- https://github.com/Blackcat-Informatics/purrassist
- https://github.com/Blackcat-Informatics/purrassist#readme

TRUNCATED (paths, identifiers): more references of
those kinds than the limit; the rest were not checked. Other kinds are
complete.
