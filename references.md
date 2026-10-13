# Referenced artifacts — issue #26

Everything the issue names, checked against disk. Facts only: whether a
file is a viable port source, or may be relicensed, is decided in the plan.
Paths resolve against the main checkout, then against a sibling checkout
only when that repo is named (first path component, or in the prose just
before the reference). "not here; exists in sibling(s)" is NOT resolved.

## Paths

| Reference | Exists | Resolved in | Kind | Lines | Licence header |
|---|---|---|---|---|---|
| `CONSTITUTION.md` | NO | not here; exists in sibling(s): gmeow-ontology, lillith_decodes, lillith_memetics | — | — | — |
| `CLAUDE.md` | NO | not here; exists in sibling(s): cf-config, fenris, gmeow-extensions, gmeow-ontology, katamari (+9 more) | — | — | — |
| `PRECOMMIT_GUIDANCE.md` | NO | repo | — | — | — |
| `AGENTS.md` | yes | repo | file | 32 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `release-qualification.yml` | NO | repo | — | — | — |
| `scripts/test-fuzz-feedback.py` | yes | repo | file | 55 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `scripts/test-verify-published.py` | yes | repo | file | 501 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `verify.yml` | NO | repo | — | — | — |
| `publish.yml` | NO | repo | — | — | — |
| `rust-toolchain.toml` | yes | repo | file | 6 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `scripts/check-release.py` | yes | repo | file | 279 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `prior-art-assessment.md` | NO | repo | — | — | — |
| `issue-analysis.md` | NO | repo | — | — | — |
| `Cargo.lock` | yes | repo | file | 326 | — |
| `crates/core/src/tool_call/from_node.rs` | yes | repo | file | 364 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `crates/host/src/files/windows.rs` | yes | repo | file | 372 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `check-release.py` | NO | repo | — | — | — |
| `validation.md` | NO | repo | — | — | — |
| `check-docs.py` | NO | repo | — | — | — |
| `scripts/test-check-release.py` | yes | repo | file | 281 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/QUALIFICATION.md:252-253` | yes | repo | file | 289 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/RELEASE.md` | yes | repo | file | 336 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `.github/workflows/release-qualification.yml` | yes | repo | file | 51 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `.github/workflows/verify.yml` | yes | repo | file | 139 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `scripts/test-check-docs.py` | yes | repo | file | 1822 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `probe/26-float-cmp` | NO | repo | — | — | — |
| `tasks/T2-review.md` | NO | repo | — | — | — |
| `test-check-docs.py` | NO | repo | — | — | — |
| `crates/core/src/tool_call/from_node.rs:336:9` | yes | repo | file | 364 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |
| `docs/QUALIFICATION.md` | yes | repo | file | 289 | SPDX: MIT OR Apache-2.0 OR MulanPSL-2.0 |

### History: `AGENTS.md`

- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `scripts/test-fuzz-feedback.py`

- `53f23662a test: generalize guided fuzzing and preserve strict differential evidence`

### History: `scripts/test-verify-published.py`

- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `ed81ffe5f fix: bound installed acceptance and prove file transaction undo`
- `868d19613 fix: locate packaged archives wherever cargo writes them and keep lockfiles`
- `788270396 build: add trusted-publishing release workflow and published-package verification`

### History: `rust-toolchain.toml`

- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `scripts/check-release.py`

- `e34c66542 build(release): reject write-all anywhere in publish.yml (#1)`
- `edcc413ec build(release): reject every YAML tag form in publish.yml (#1)`
- `afc180a7d build(release): reject escapes, tags and block-scalar permissions in publish.yml (#1)`
- `b1c964563 build(release): hold publish.yml to a closed shape (#1)`
- `44e5dd1b5 build(release): catch every form of OIDC grant outside the publish job (#1)`
- `f6283b907 build(release): only the publish job may request an OIDC token (#1)`
- `feefaaebc build(docs): harden the documentation and attribution checks (#1)`
- `b7475b4fd build(release): publish preflight checks crate names and owners before publishing (#15)`
- `e97a97367 feat(derive): zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)] (#15)`
- `788270396 build: add trusted-publishing release workflow and published-package verification`

### History: `Cargo.lock`

- `4085bf31a feat(purrassist): re-export the tool derives through the facade (#15)`
- `e97a97367 feat(derive): zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)] (#15)`
- `a2da56a81 build: port structural helper census and strict ledger tooling`
- `24787e1a3 test: compare helper transcripts and browser WASM with native output`
- `b53ce9fbd feat: qualify filesystem durability on Windows`
- `46e9d1a6e feat: establish intent recovery workspace and verification foundation`

### History: `crates/core/src/tool_call/from_node.rs`

- `fac0d98fb test(core): compare the double-rounding result by bits (#1)`
- `d46b996d2 refactor(tool-call): harden the derive support surface before first publish (#15)`
- `0fb045418 feat(tool-call): core support for derived tool definitions (#15)`
- `fcf234265 feat(tool_call): typed tool definitions, governed ToolSet, FromNode`

### History: `crates/host/src/files/windows.rs`

- `f8d8cd3d2 fix: report exhausted Windows rename contention as busy`
- `9994d4d61 fix: open Windows replacement handles with delete access`
- `0b7c7de05 fix: make Windows durability code lint-clean and fail explicitly`
- `b53ce9fbd feat: qualify filesystem durability on Windows`

### History: `scripts/test-check-release.py`

- `e34c66542 build(release): reject write-all anywhere in publish.yml (#1)`
- `edcc413ec build(release): reject every YAML tag form in publish.yml (#1)`
- `afc180a7d build(release): reject escapes, tags and block-scalar permissions in publish.yml (#1)`
- `b1c964563 build(release): hold publish.yml to a closed shape (#1)`
- `44e5dd1b5 build(release): catch every form of OIDC grant outside the publish job (#1)`
- `f6283b907 build(release): only the publish job may request an OIDC token (#1)`
- `feefaaebc build(docs): harden the documentation and attribution checks (#1)`
- `b7475b4fd build(release): publish preflight checks crate names and owners before publishing (#15)`

### History: `docs/QUALIFICATION.md:252-253`

- `f58736a5d docs(release): record the published 0.1.0 and add the MCP Registry manifest (#1)`
- `8ce1ca6f7 test(fuzz): keep mocked campaign findings out of CI logs (#1)`
- `7e30c67c2 ci(fuzz): a hosted fuzz gate that completes every target (#1)`
- `d6fb1e0fa docs: version-pinned crate links and no internal process references (#1)`
- `dbdc64dcc docs(release): source restated documentation facts and disclose known limits (#1)`
- `6cbb6282f docs(derive): near-miss nested keys, patch compatibility rule, qualification evidence (#15)`
- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `381306500 fix(tool-call): closers inside values are judged by what follows, never by layout (adversary pass 6, F14, F19)`
- `fecda8934 test: split make check into focused tests; corpus and fuzz get their own lanes`
- `75c8f1134 fix(tool-call): keep stray closers and call bodies out of values and content (adversary pass 5, F13–F18)`

### History: `docs/RELEASE.md`

- `03be6d5d8 ci(mcp-registry): main-only, bounded runs; state the real crates.io control (#1)`
- `798104f2e ci(mcp-registry): submit server.json through GitHub OIDC (#1)`
- `e19c8d842 docs(release): publication, not the tag, is what cannot be undone (#1)`
- `f58736a5d docs(release): record the published 0.1.0 and add the MCP Registry manifest (#1)`
- `cbecc8c1b docs(fuzz): describe the hosted gate and keep base attribution for its #6 inputs (#1)`
- `7e30c67c2 ci(fuzz): a hosted fuzz gate that completes every target (#1)`
- `f07b15b16 ci(verify): install pinned Node for fuzzing; define the release fuzz gate (#1)`
- `2ce68e5c7 docs(release): name the YAML tag forms the release check rejects (#1)`
- `e34c66542 build(release): reject write-all anywhere in publish.yml (#1)`
- `edcc413ec build(release): reject every YAML tag form in publish.yml (#1)`

### History: `.github/workflows/release-qualification.yml`

- `fecda8934 test: split make check into focused tests; corpus and fuzz get their own lanes`
- `51ed930ab ci: enforce complete qualification and exact tool identities`
- `788270396 build: add trusted-publishing release workflow and published-package verification`
- `de8db9d7b build: qualify release archives and publish order`

### History: `.github/workflows/verify.yml`

- `7e30c67c2 ci(fuzz): a hosted fuzz gate that completes every target (#1)`
- `f07b15b16 ci(verify): install pinned Node for fuzzing; define the release fuzz gate (#1)`
- `b7475b4fd build(release): publish preflight checks crate names and owners before publishing (#15)`
- `4085bf31a feat(purrassist): re-export the tool derives through the facade (#15)`
- `fb522afce ci: gate pull requests on the published-transcript replay (adversary pass 6, F20)`
- `fecda8934 test: split make check into focused tests; corpus and fuzz get their own lanes`
- `8496413da test(fuzz): tool-call fuzz target and WASM parity for every dialect`
- `300f7cffe test: make profiling prerequisites explicit and retain startup diagnostics`
- `51ed930ab ci: enforce complete qualification and exact tool identities`
- `788270396 build: add trusted-publishing release workflow and published-package verification`

### History: `scripts/test-check-docs.py`

- `f58736a5d docs(release): record the published 0.1.0 and add the MCP Registry manifest (#1)`
- `2fb4c3b76 docs(release): first-release wording for shipped Rustdoc and the release-body rule (#1)`
- `04be6e023 release: date the 0.1.0 changelog section (#1)`
- `cbecc8c1b docs(fuzz): describe the hosted gate and keep base attribution for its #6 inputs (#1)`
- `7e30c67c2 ci(fuzz): a hosted fuzz gate that completes every target (#1)`
- `93c02b3a5 docs(core): describe features, not internal task and requirement numbers (#1)`
- `feefaaebc build(docs): harden the documentation and attribution checks (#1)`
- `f0d978a37 docs(assets): reproducible 1280x640 social preview for the repository (#1)`
- `f11912e6f docs(release): 0.1.0 changelog, current release procedure and security policy (#1)`
- `871af5187 docs(readme): one engine, three interfaces, and a registry-pinned tool reference (#1)`

### History: `crates/core/src/tool_call/from_node.rs:336:9`

- `fac0d98fb test(core): compare the double-rounding result by bits (#1)`
- `d46b996d2 refactor(tool-call): harden the derive support surface before first publish (#15)`
- `0fb045418 feat(tool-call): core support for derived tool definitions (#15)`
- `fcf234265 feat(tool_call): typed tool definitions, governed ToolSet, FromNode`

### History: `docs/QUALIFICATION.md`

- `f58736a5d docs(release): record the published 0.1.0 and add the MCP Registry manifest (#1)`
- `8ce1ca6f7 test(fuzz): keep mocked campaign findings out of CI logs (#1)`
- `7e30c67c2 ci(fuzz): a hosted fuzz gate that completes every target (#1)`
- `d6fb1e0fa docs: version-pinned crate links and no internal process references (#1)`
- `dbdc64dcc docs(release): source restated documentation facts and disclose known limits (#1)`
- `6cbb6282f docs(derive): near-miss nested keys, patch compatibility rule, qualification evidence (#15)`
- `534f862e3 build(release): register purrassist-derive for packaging, audit and docs (#15)`
- `381306500 fix(tool-call): closers inside values are judged by what follows, never by layout (adversary pass 6, F14, F19)`
- `fecda8934 test: split make check into focused tests; corpus and fuzz get their own lanes`
- `75c8f1134 fix(tool-call): keep stray closers and call bodies out of values and content (adversary pass 5, F13–F18)`

## Identifiers (files containing each at HEAD)

- `msrv`: 1
- `clippy::float_cmp`: 0
- `env.RUSTUP_TOOLCHAIN`: 0
- `major.minor`: 8
- `channel`: 53
- `float_cmp`: 1
- `de8e8fd`: 0
- `too_long_first_doc_paragraph`: 0
- `pull_request`: 1
- `main`: 260
- `cfg`: 74
- `success`: 75
- `failure`: 135
- `native`: 138
- `Makefile`: 6
- `package`: 76
- `make`: 155
- `env`: 180
- `lint`: 19
- `workflow_steps`: 2
- `nightly`: 40
- `RUSTFLAGS`: 3
- `fdd10ff`: 0
- `f32`: 33
- `f64`: 40
- `PATH`: 12

## Workspace packages named

- `purrassist-cli`
- `purrassist-core`
- `purrassist-host`

## URLs

- https://github.com/Blackcat-Informatics/purrassist/issues/1#issuecomment-6139102863
- https://github.com/Blackcat-Informatics/purrassist/actions/runs/38376755652
- https://github.com/Blackcat-Informatics/purrassist/actions/runs/38382758016
- https://github.com/Blackcat-Informatics/purrassist/actions/runs/38377011106
