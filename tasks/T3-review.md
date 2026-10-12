# T3 review: release docs, CHANGELOG, SECURITY

## Scope
Uncommitted diff against 871af51: CHANGELOG.md (new), README.md, SECURITY.md,
docs/RELEASE.md, scripts/check-docs.py, scripts/test-check-docs.py.

## Evidence
- `python3 scripts/check-docs.py && test-check-docs.py && check-repository.py && check-release.py && test-check-release.py`: all pass (docs facts OK; 18 release tests OK; repository and publish.yml verified).
- Mutations on a temp copy (all detected, copy deleted, worktree unchanged):
  - CHANGELOG `[0.1.0]` -> `[0.2.0]`: "newest version heading is [0.2.0] but workspace is 0.1.0".
  - `purrassist-host` dropped from CHANGELOG: "does not name the crate `purrassist-host`".
  - `Verify corpus suites` removed from RELEASE.md: "does not name the workflow step".
  - PVR advisory link removed from SECURITY.md: "does not name the reporting channel".
- Failing fixtures exist in scripts/test-check-docs.py:522-745 for version, crate, interface, table row, both channels, reservation table (missing/unknown/reordered), qualification steps, README link.
- Qualification steps in RELEASE.md (the numbered list under "Qualification workflow") match the 8 step names in release-qualification.yml:27-50, with the 90-minute allowance, Rust 1.98.0, tag/manual split and nightly-2026-09-14 coverage toolchain.
- Release-step order (docs/RELEASE.md "Release step"): date commit, verify exact commit, reconfirm 6 trusted publishers, tag, release body. This is coherent with publish.yml:36, whose gate waits for release-qualification.yml success on the tagged SHA (triggered by the tag).
- SECURITY: PVR before-value `false`, after `true` recorded in T5-repo-settings.md, so naming the channel is supported.
- MCP Registry page (fetched): cargo example uses registryType cargo, identifier, version, stdio transport; mcp-name must be visible text because crates.io strips HTML comments. RELEASE.md "After publication" matches; crates/cli/README.md:103 carries a visible `mcp-name: io.github.Blackcat-Informatics/purrassist` bullet.
- CHANGELOG claims verified: CLI subcommands (crates/cli/src/commands.rs:36-40), four active tool calls (crates/host/src/mcp.rs:215), twelve dialects and Draft 2019-09/draft-06 rejection (README.md:154,311), MSRV 1.98 and edition 2024 (Cargo.toml:9-10), six crates in RELEASE_ORDER. Known limitations are the 10 JSON entries, equality enforced by check-docs.
- Overclaim scan: every status line says "not yet tagged"; 0.1.x "Yes, once published"; no hosted pass on current main asserted. Grep for attribution, billing/payment/account/provider wording, "Task N", plan/stage jargon in the four docs: clean, except finding 1.

## Consumers
check-docs.py runs in `make check`; README links CHANGELOG.md; release body in RELEASE.md step 5 consumes the `[0.1.0]` section; the crates.io/GitHub security tab consumes SECURITY.md.

## Findings
1. Low, non-blocking: CHANGELOG.md:107 says "stage-3 commits" (text copied from the issue #7 title in docs/known-limitations.json). It is internal workflow jargon in a public file, but it is bound by the equality check to the JSON. Fix would be to reword the JSON entry (also surfaces in QUALIFICATION.md/FUZZING.md) - optional.
2. Cosmetic: docs/RELEASE.md:205 is a single long line after the "Task 10" removal; reflow.
3. Note: the MCP Registry server.json needs a top-level `version` as well as the package `version`; RELEASE.md says "the published version" only for the package. Optional clarification; the doc defers to the registry's current schema.
No blocking issues found.

VERDICT: PASS
