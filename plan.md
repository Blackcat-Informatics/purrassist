# Plan — Issue #1 (resumed): release-ready 0.1.0 documentation and public repository presentation

Revision 4: this revision folds in every lens review through its final pass. Dispositions are at the end.

## Issue summary

Issue #1 asks that PurRAssist 0.1.0 be finished, qualified and released. The implementation it named (R1–R6,
R10, R12) merged in PR #2 (`699d9ff`, 2026-09-30). The squash message kept the issue open for tagging,
registry publication and downloaded-package acceptance. Main has since gained tool-call recovery (#3, #8)
and `purrassist-derive` (#15), so the release set is now six crates at workspace version `0.1.0`.

Owner decisions in this session (binding):

- Hosted CI is out of scope for this stage.
- Open issues #6, #7, #10 and #11 do not block 0.1.0. They are disclosed as known limitations.
- The remaining scope is: release docs, an updated `README.md`, and other doc updates. Tagging and publishing are not part of this stage.
- Publish the `purrassist-derive` 0.0.0 name reservation. **Done** on 2026-10-12: crates.io returns HTTP 200 and the owner is identical to the other five. The owner configures its trusted publisher.
- The repository is public. Set its GitHub description and related settings.
- Present PurRAssist as **one engine with three equal interfaces: the Rust crates, the MCP server and the CLI**.
- Add a social-preview image.

## Baseline defaults

There is no `.baseline` file.

## Standing goals and constraints

From `.goals` and `AGENTS.md`:

- Rust-first, maximal utility and portability.
- Hard fails, and low or no optionality, including in non-code work.
- `purrassist-core` has no third-party runtime dependencies.
- Every dependency and tool is registered and justified. Each `vendor/tooling.json` entry needs `notices`.
- Every source file has an SPDX header.
- Capability claims need executable evidence.
- Docs distinguish implemented behaviour from plans.
- No user-specific services, credentials or private infrastructure.
- Hooks are never bypassed.
- `make check` is the local gate; packaging needs its own evidence.

## Governing decisions

- `docs/RELEASE.md` is the procedure. It defines six crates in the `RELEASE_ORDER` of `scripts/check-release.py`.
- `publish.yml` is the only workflow that publishes.
- Qualification and publication are bound to the exact commit that is tagged.
- Tagging `rust-v0.1.0` stays a separately authorized, irreversible step.

## Prior art and recurrence (from `prior-art-assessment.md`)

**Earlier documentation passes:**

- Task 19 (`42b4460`, `b08161e`, `856d2ec`) reconciled the docs.
- Its acceptance was a one-time grep that was never wired into `make check`.

**Recurrence:**

- 11 commits explicitly fix stale or overclaimed docs. Unlabelled fixes folded into other commits are UNKNOWN.
- The fuzz-target count went stale twice ("nine"; there are ten).
- The receipt wire version went stale (v2; it is 3).
- #15 missed a six-crate mention until `8d8d9a2`.

**Why earlier attempts did not hold:**

- Prose restates facts that no machine check reads.
- The only machine-checked Markdown is the derive-example copy and `docs/tool-calls/RULES.md`, which is pinned to `rules::RULES` by `crates/core/tests/tool_call_rules.rs`.

**What holds:**

- Generated projections pinned by tests (the RULES.md precedent).
- A fact checker that takes facts from their sources.

## Stale or missing content (verified; line numbers on `f11b45e`)

**`docs/RELEASE.md`:**

- Lines 5–7: "reviewed merge commit" intro.
- Lines 43–62 and 212–218: derive "no placeholder record yet" or "not yet done".
- Line 83: "five of six".
- Lines 100–104: the qualification summary omits steps it runs.
- Line 182: "Task 10" jargon.

**`README.md`:**

- Lines 14–17: "follows review and merge".
- Line 387: "nine" targets. Line 426: JSON-only fuzz wording.
- It has no install section and no crate map.
- It has no three-interface framing.
- About 250 lines of per-tool reference sit in the overview, and `read_file` and `undo_file` are missing.
- It calls the `examples/*.json` requests executable, but no test runs them.

**`docs/IMPLEMENTATION.md`:**

- Lines 5–7: "issue branch" and "follows review and merge".
- Lines 99–108: the delivery sequence does not mark delivered stages.
- The workspace boundaries do not mark the internal crates.

**`docs/QUALIFICATION.md`:**

- Lines 7–8, 224 and 226: "reviewed merge commit" and "after review and merge".
- Line 23: "receipt wire v2".
- Lines 33 and 168: "Nine targets" and "nine mutation targets".
- Line 143: Windows "proved by the hosted lane".
- `make fuzz` is presented as passing, although issues #11 and #6 make it stop.
- Lines 240–251: the Stage 2 section is unlabelled and bound to `66b3756`.

**`docs/FUZZING.md`:**

- Lines 877–881: "10,000 iterations per target", but the two tool-call targets have no baseline.
- Line 967: a `.stage` report pointer.

**Crate READMEs, `CONTRIBUTING.md` and `SECURITY.md`:**

- The core, host, cli and purrassist READMEs say "Pre-release implementation", which would ship in the archives.
- `CONTRIBUTING.md:20` points to a removed section.
- `SECURITY.md` is pre-release wording and has no supported-versions table.

**Crate metadata:**

- All crates share the same non-MCP keywords.
- `purrassist-cli` is categorized as `parsing`.
- There is no `homepage`.
- The `purrassist-cli` documentation link points to docs.rs, but the crate is a binary with `doc = false`.
- The derive README's `latest` docs.rs links resolve to 0.0.0 until publication.

**Missing:**

- There is no CHANGELOG.

**GitHub:**

- The description, homepage and topics are empty.
- Wiki and Projects are enabled but unused.
- There is no social preview.
- Private vulnerability reporting is off.
- GitHub detects the license as Apache-2.0 only.

## Completeness contract

Every row's checks run inside `make check`, except where marked.

| Requirement | Task | Executable acceptance |
|---|---|---|
| Restated facts are sourced, not hand-maintained; stale facts corrected | 1 | `python3 scripts/check-docs.py` exits 0. `python3 scripts/test-check-docs.py` has, for every rule, a fixture copy that violates it and must fail with that rule's named error, plus a fixture where the sentence the rule reads is absent, which must fail ("fact sentence not found"; every fact regex fails on zero matches). Rules: release set and order from `RELEASE_ORDER`; workspace version; fuzz-target count from `scripts/fuzz.py` `TARGETS`; receipt version from `RECEIPT_VERSION`; dialect ids from the `docs/tool-calls/dialects/*.md` stems in every file that lists them; relative links resolve; SPDX on tracked `*.md` (extending the existing scan in `check-repository.py`); no `.stage` references in public docs; a case-insensitive banned-phrase list, matched after collapsing all whitespace runs (including newlines) to one space, with a fixture where a phrase wraps across lines, covering these regexes, applied to the collapsed text: `issue branch`, `nine (mutation \|coverage-guided )?targets`, `wire v2`, `reviewed merge commit`, `follows? (the )?review and merge`, `after review and merge`, `no placeholder record`, `not yet done`, `five of six`, `Pre-release`, `stage report`, `proved by the [^.]*hosted`. **One-time baseline check (Task 1 focused check, recorded in `validation.md`, not in `make check`):** run the rule over the `f11b45e` versions of README, RELEASE, QUALIFICATION, IMPLEMENTATION and FUZZING (via `git show` into a temp directory). It must flag every stale site in the inventory, including the wrapped sites RELEASE 6–7, QUALIFICATION 7–8, 142–143 and 226–227. Scope: all tracked `*.md` outside `.stage/`, `vendor/` and `crates/core/tests/data/`. |
| Dialect ids have one source | 1 | A new Rust assertion in `crates/core/tests/tool_call_dialects.rs` checks that `DialectId::BUILT_IN.map(id)` equals the sorted `docs/tool-calls/dialects/*.md` stems. `cargo test -p purrassist-core --test tool_call_dialects` passes. |
| Honest qualification and limits wording | 1 | `check-docs.py` structure rules, each with a failing fixture: QUALIFICATION and FUZZING contain a `## Known limitations` heading. Its issue set equals the set of issue references in `docs/known-limitations.json` (one source: number, title, one-line impact, `blocks_release: false`). Task 3 extends the same rule to the CHANGELOG when that file is created. The FUZZING baseline table has one row per `TARGETS` entry, and a target without a baseline says `none`. QUALIFICATION's archived-evidence paragraphs name the commit each report is bound to (the `source.commit` of each `docs/qualification/*.json`, read by the checker). The Windows sentence cites the run id it claims. Every row of QUALIFICATION's `make fuzz` results table links to `#known-limitations`; a fixture with the link removed fails. The rows for the targets affected by #6 and #11 state the stop instead of "invariants hold". |
| Tool reference is a generated projection of the registry | 2 | The new test `tools_reference_matches_registry` in `crates/cli/tests/protocol.rs` renders, from `Tools::definitions()` (name, description, input properties with type and required, output top-level properties) in registry order, the block between `<!-- tools-reference:begin -->` and `<!-- tools-reference:end -->` in `docs/TOOLS.md`. It asserts byte equality and prints a diff on failure; `PURRASSIST_BLESS=1` rewrites it and panics if `CI` is set, following the `recovery_goldens` precedent. Each generated entry shows the MCP `tools/call` name, `purrassist call NAME --root DIR < args.json`, and its example file when one exists. Hand-written per-tool narrative (moved from the README) sits outside the block, under `### <tool>` headings. `check-docs.py` counts the hand-written `### <tool>` headings outside the generated block, against the tool names inside it (the block itself is byte-pinned by the Rust test). There must be exactly one heading per name, and no headings for unknown names. |
| Every documented example request actually runs through both CLI and MCP | 2 | The new test `examples_run_through_cli_and_mcp` in `crates/cli/tests/protocol.rs` has an `EXAMPLES` table (file → tool). It asserts that every `examples/*.json` is either a listed tool request or a listed supporting input (`temperature.schema.json` and `temperature.input.txt`, consumed by the temperature request). No file may be an orphan and that each runs through `run_cli` and `run_mcp` in fresh roots with identical, output-schema-valid results. The temperature decode is also checked through `decode_json` in both transports. |
| README presents three equal interfaces | 2 | The README has a `three-interfaces` marker block with Rust, MCP and CLI subsections. The Rust snippet is a byte copy of a doctested block in `crates/purrassist/README.md`. That check is generalized from `check_derive_example` into one marker-copy rule in `check-repository.py` covering both `derive-example` and `three-interfaces`. The MCP client config JSON in the block is parsed by `check-docs.py` (valid JSON, `command` = `purrassist`, `args` = `["mcp","--root",…]`). The README states that MCP creates `.purrassist/` journals in the root. The CLI≡MCP sentence cites `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp`. The convenience commands `decode` and `match` are documented as shortcuts with their own output, and no identity is claimed for them. `purrassist validate` keeps its true claim that it prints exactly the `validate_json` tool output, now backed by a new test, `validate_command_prints_validate_json_output` in `crates/cli/tests/protocol.rs`. |
| No README claim is lost in the move | 2 | Task 2 runs a one-time migration check as a focused check and records it in `validation.md`; there is no `git` in `make check`. It extracts every backticked token and link target from `git show f11b45e:README.md` and asserts that each appears in the new README ∪ `docs/TOOLS.md`, or is listed in a deliberate-removal table in `validation.md`, each entry with its reason (for example a stale count). The Task 2 reviewer must accept every removal. The command and its output are recorded in the Task 2 review. |
| Crate map and READMEs are release-ready | 2 | `check-docs.py` checks that the README crate-map table lists exactly `RELEASE_ORDER` in order. Each packaged README, derived from publishable `cargo metadata` packages rather than a hand list, has no banned phrase and no `docs.rs/.../latest` link. The IMPLEMENTATION workspace-boundaries list equals the workspace members, with the unpublished ones marked internal. |
| MCP Registry ownership token in the published 0.1.0 CLI README (presence only; listing requires a post-publication submission) (crates.io versions are immutable; crates.io strips HTML comments) | 2 | `crates/cli/README.md` has a visible Links bullet: `` MCP Registry name: `mcp-name: io.github.Blackcat-Informatics/purrassist` ``. `check-docs.py` derives the expected name `io.github.<owner>/purrassist` from the owner segment of the workspace `repository` URL, verbatim and case-sensitive. It asserts that the token appears outside any HTML comment and code fence, in the packaged cli README, with failing fixtures for a missing token and a commented-out token. The `server.json` file and registry submission are owner steps after publication, listed in the handoff and in RELEASE.md. |
| Crate metadata is correct | 2 | No `homepage` is set: Cargo's `redundant_homepage` lint rejects a homepage equal to `repository`, and crates.io already links the repository. `purrassist-cli` gets the description "PurRAssist CLI and stdio MCP server", the categories `command-line-utilities` and `development-tools`, the keywords `mcp`, `cli`, `json`, `recovery` and `agents`, and a documentation link to the repository `docs/TOOLS.md`. The facade gets the description "Rust interface to PurRAssist intent recovery for imperfect AI output" and the `mcp` keyword is added to host and cli. `check-docs.py` asserts that the cli documentation link is not docs.rs. `python3 scripts/check-release.py` passes. Archives are audited by `make package` (Task 6). |
| Release docs are current | 3 | The banned phrases are absent. RELEASE.md names the six crates in `RELEASE_ORDER` (checked). The derive reservation is recorded with its date. Trusted-publisher state is a dated observation with derive owner-pending. Qualification is bound to "the exact `main` commit to be tagged". The qualification summary lists every step the workflow runs, checked against the `run:` step names parsed from `.github/workflows/release-qualification.yml`. Not in `make check`: `python3 scripts/check-release.py --preflight-owners --user-agent 'purrassist-release (https://github.com/Blackcat-Informatics/purrassist)'` exits 0 (a read-only crates.io check) and is recorded in `validation.md`. |
| 0.1.0 CHANGELOG | 3 | `CHANGELOG.md` uses Keep a Changelog headings: `## [Unreleased]`, then `## [0.1.0] - unreleased`. `check-docs.py` asserts that the version equals the workspace version, that all six crates and all three interfaces are named, and that the Known limitations set equals `docs/known-limitations.json`. RELEASE.md's release step names this section as the GitHub release body. |
| SECURITY.md | 3 | `check-docs.py` checks for a `Supported versions` table containing the workspace `major.minor`. It also checks that both reporting channels are named: the email, and GitHub private vulnerability reporting. |
| Social preview image | 4 | `docs/social-preview.svg` (SPDX, 1280×640, logo inlined, single font "Noto Sans" with no fallback) and `docs/social-preview.png` are registered in the new `docs/assets.json`: SVG sha256, PNG sha256, render command, `rsvg-convert` version, the font file sha256, and the license. The PNG license is recorded by a REUSE-style `docs/social-preview.png.license` sidecar, and the SPDX scan accepts sidecars for binaries. `rsvg-convert` (`LGPL-2.1-or-later`) and Noto Sans (`OFL-1.1`, recorded by upstream URL, version and file sha256, not a local path) are registered in `vendor/tooling.json` with vendored notices and justified in `docs/DEPENDENCIES.md` as manual asset-rendering tools only. `check-docs.py` checks that the PNG IHDR is 1280×640, that the size is under 1,000,000 bytes, and that both digests equal the registry; there are failing fixtures for each. Not in `make check`: a fresh render under the recorded tool and font must reproduce the PNG sha256. The logo's `feDropShadow` rendering is verified visually. |
| GitHub presentation | 5 | Recorded in `tasks/T5-repo-settings.md` and checked by a reviewer. `gh repo view --json description,homepageUrl,repositoryTopics,hasWikiEnabled,hasProjectsEnabled` shows the set values. `gh api repos/Blackcat-Informatics/purrassist/private-vulnerability-reporting` returns `{"enabled":true}`. The social preview is uploaded through the Settings UI with the Playwright browser tools, verified by the public page's `og:image` changing from the default `opengraph.githubassets.com` card. If the browser upload is impossible, the row is **NOT DELIVERED** and goes to the owner as an explicit decision (upload it themselves, or drop it from this stage); it is not counted as done. |
| Local gate and package | 6 | `make check` and `make package` exit 0 on the final head (single full run). |
| PR and honest handoff | 6 | The PR body says `Refs #1` (the issue stays open for the tag and publication) and lists the remaining owner steps. The plan is posted to the issue and the PR. |

**Out of this stage by owner decision:**

- Hosted CI.
- The derive trusted-publisher configuration.
- The tag, publication, downloaded-package verification and GitHub release.

## Enhancements considered and declined

- **Combined `LICENSE` file for GitHub license detection.**
  - Declined: GitHub's detector can't represent an SPDX `OR` expression. A synthesized file would duplicate three license texts that `check-packages.py` audits by hash, and it would mislead tools that read `LICENSE` as a single license.
  - The README license section and Cargo `license` remain authoritative, and the handoff states the display limitation.
- **`server.json` MCP-registry manifest file.**
  - Declined for this stage: its correctness is proven only by registry submission, which comes after publication.
  - The irreversible prerequisite, the visible `mcp-name:` token in the immutable 0.1.0 `purrassist-cli` README, *is* adopted in Task 2.
  - Writing and submitting `server.json` is listed in RELEASE.md and the handoff as a post-publication step.
- **Test that `purrassist decode` output is identical to `call decode_json`.**
  - Declined: they are deliberately different shapes. `decode` prints the recovery result and has exit codes; the tool returns the registry output schema.
  - The docs stop implying identity, and the README temperature request is exercised through `decode_json` in both transports instead.
- **Generate all prose counts too.**
  - Partly adopted: structural lists (tools, crates, dialects, fuzz targets, known limitations) are generated or sourced.
  - Free prose numbers stay checked facts, because generating narrative text would make it unreadable.

## Task 1: Doc fact checker, known-limitations source, stale-fact corrections

**Files:**

- New, stdlib only, SPDX headers, loaded with the `importlib` precedent of `test-check-release.py`: `scripts/check-docs.py` and `scripts/test-check-docs.py`.
- `Makefile` `hygiene` runs both.
- New: `docs/known-limitations.json` (#6, #7, #10, #11, #12, #13, #14, #16, #17, #18, with titles from the 2026-10-12 observation: all open).
- `crates/core/tests/tool_call_dialects.rs`: the dialect-stem assertion.
- `check-repository.py`: the SPDX scan is extended to `*.md`.
- Corrections in `docs/QUALIFICATION.md`, `docs/IMPLEMENTATION.md`, `docs/FUZZING.md`, `CONTRIBUTING.md`, and the README fact lines at 14–17, 387 and 426. The stale facts are listed in the inventory above.
- **Every banned-phrase site is corrected in this task, so `check-docs.py` is green when the rule lands.** This includes:
  - `docs/RELEASE.md` lines 5–7, 43–62, 83 and 212–218: minimal factual corrections, namely the derive reservation minted on 2026-10-12, a dated trusted-publisher observation, and "the exact `main` commit to be tagged".
  - `SECURITY.md:5`: the pre-release sentence is replaced with a factual status line.
  - The four crate READMEs' "Pre-release implementation" lines: replaced with a one-line 0.1.0 status.

  Tasks 2 and 3 then do the structural rewrites of these files.

**Fixes:**

- **Status lines:** "Merged to `main` (PR #2, `699d9ff`); tool-call recovery (#3, #8) and `purrassist-derive` (#15) merged since; not yet tagged `rust-v0.1.0`."
- **QUALIFICATION:**
  - Every archived-evidence paragraph names its bound commit, and the Stage 2 section is headed "Historical: Stage 2 record bound to `66b3756`".
  - The Windows sentence cites its exact run.
  - A `## Known limitations` section is added, including that the default `make fuzz` stops on #11 and #6 signatures.
- **IMPLEMENTATION:** the delivery sequence (lines 99–108) marks the stages already delivered.
- **FUZZING:** a per-target baseline table (`none` for the tool-call targets), and the `.stage` pointer is removed.

**Focused check:**

- `python3 scripts/check-docs.py && python3 scripts/test-check-docs.py && python3 scripts/check-repository.py`.
- `cargo test -p purrassist-core --test tool_call_dialects`.

Then: task review, commit (explicit files, no `.stage/`), push, issue update.

## Task 2: Three-interface README, generated tool reference, executed examples, crate READMEs and metadata

**README order:**

1. Logo, tagline and value statement.
2. Status, linking `docs/QUALIFICATION.md#known-limitations`. Task 3 adds the CHANGELOG link when the file exists, keeping the relative-link rule green at every commit.
3. **One engine, three interfaces** (marker block):
   - **Rust:** `cargo add purrassist` plus the doctested copy.
   - **MCP:** `cargo install purrassist-cli`, `purrassist mcp --root DIR`, the client config JSON, and the `.purrassist/` note.
   - **CLI:** `purrassist decode …` and `purrassist call TOOL --root DIR`.
   - The CLI≡MCP sentence with its test name.
4. Crate map: the six crates in order, with roles, plus the internal crates.
5. Capabilities, condensed with no claim lost.
6. Use the checkout.
7. Develop and verify.
8. Conformance.
9. License and family. It notes that GitHub shows only one of the three licenses.

**`docs/TOOLS.md`:**

- The generated block comes from `tools_reference_matches_registry`, blessed once.
- The narrative sections are moved from the README, plus new ones for `read_file` and `undo_file`.

**Tests in `crates/cli/tests/protocol.rs`:**

- `tools_reference_matches_registry`.
- `examples_run_through_cli_and_mcp`, including the temperature request via `decode_json`.
- `validate_command_prints_validate_json_output`.

**Crate READMEs:** each of the six gets a 0.1.0 status line and its interface role:

- `purrassist` is the Rust interface.
- `purrassist-cli` is the CLI and MCP server.
- `purrassist-host` serves both.
- core, syntax and derive are the shared engines.

The derive README's `latest` links are pinned to `0.1.0`.

The cli README gets the visible `mcp-name:` Links bullet.

**Other files:**

- Crate metadata as in the contract.
- IMPLEMENTATION workspace boundaries.
- `check-repository.py` marker-copy generalization.
- `check-docs.py` README, crate-map, packaged-README and config rules, each with failing fixtures.
- The one-time migration check, recorded in `validation.md`.

**Focused check:**

- `python3 scripts/check-docs.py && python3 scripts/test-check-docs.py && python3 scripts/check-repository.py && python3 scripts/check-release.py`.
- `cargo test -p purrassist --doc`.
- `cargo test -p purrassist-cli --test protocol`.

Then: task review, commit (explicit files, no `.stage/`), push, issue update.

## Task 3: Release docs, CHANGELOG, SECURITY

**`docs/RELEASE.md`:**

- The intro and release step are bound to "the exact `main` commit to be tagged". The existing qualification sequence is kept.
- A six-reservation record: derive minted on 2026-10-12 with identical ownership.
- Trusted publishing as a dated observation: five confirmed on 2026-09-28, derive owner-pending. Re-confirm through the API immediately before tagging.
- A complete qualification-workflow summary.
- The "Task 10" jargon is removed.
- The release body comes from the CHANGELOG `[0.1.0]` section plus the `verify-published.py` output, and the date is set at tagging.

**`CHANGELOG.md`:** Keep a Changelog format with:

- Highlights by interface.
- The six crates.
- Capability areas with doc links.
- Compatibility: MSRV 1.98 and the derive/core ABI marker rule.
- Known limitations rendered from `docs/known-limitations.json`, checked for equality.

**`SECURITY.md`:**

- Record the before-value (`gh api repos/Blackcat-Informatics/purrassist/private-vulnerability-reporting`) in `tasks/T5-repo-settings.md`.
- Then enable GitHub private vulnerability reporting (`gh api -X PUT repos/Blackcat-Informatics/purrassist/private-vulnerability-reporting`) and verify that it reads back `{"enabled":true}`. Only then write the sentence naming it. If enabling fails, SECURITY.md names the email only and the failure is reported.
- A Supported versions table: 0.1.x is supported once published, and the 0.0.0 reservations contain no code.
- Both reporting channels.

**`check-docs.py`:** the matching rules, with failing fixtures.

**Focused check:**

- `python3 scripts/check-docs.py && python3 scripts/test-check-docs.py && python3 scripts/check-release.py && python3 scripts/test-check-release.py`.
- The read-only preflight-owners run, recorded in `validation.md`.

Then: task review, commit (explicit files, no `.stage/`), push, issue update.

## Task 4: Social preview

**Assets:**

- `docs/social-preview.svg`: 1280×640, 40 px safe margin.
- It inlines the logo artwork and shows the "PurRAssist" wordmark, the tagline, and "Rust crate · MCP server · CLI".
- Palette from the logo, single font "Noto Sans" with no fallback.
- The PNG is rendered with `rsvg-convert -w 1280 -h 640 docs/social-preview.svg -o docs/social-preview.png`.

**Registration:**

- `docs/assets.json`.
- `scripts/check-repository.py`: the SPDX scan accepts a `.license` sidecar for binary files.
- `docs/social-preview.png.license`.
- `vendor/tooling.json` entries for `rsvg-convert` and Noto Sans with notices under `vendor/licenses/`.
- A `docs/DEPENDENCIES.md` justification.
- `check-docs.py` asset rules with failing fixtures.

**Focused check:**

- `python3 scripts/check-docs.py && python3 scripts/test-check-docs.py && python3 scripts/check-repository.py`.
- **Mandatory pass/fail before commit:** re-render to a temp file under the recorded tool and font, and compare its sha256 with the registry. A mismatch (for example a silent font substitution) blocks the commit.
- View the PNG.

Then: task review, commit (explicit files, no `.stage/`), push, issue update.

## Task 5: GitHub repository presentation (outward-facing, owner-requested, no commit)

**`gh repo edit Blackcat-Informatics/purrassist`:**

- **Description:** "Recovers intent from imperfect AI output — JSON, tool calls, names, paths and Markdown edits — with auditable receipts. Rust crate, MCP server and CLI."
- **Homepage:** `https://github.com/Blackcat-Informatics/purrassist#readme`. docs.rs shows the 0.0.0 placeholder until publication, so it is listed in the handoff for change after release.
- **Topics:** `rust`, `mcp`, `mcp-server`, `model-context-protocol`, `cli`, `json-schema`, `json-repair`, `tool-calling`, `llm`, `ai-agents`, `fuzzy-matching`, `markdown`, `wasm`.
- **Unused features:** `--enable-wiki=false --enable-projects=false`.

**Other settings:**

- Private vulnerability reporting: already enabled in Task 3; re-verify it here.
- Social-preview upload via Playwright, as in the contract row.

**Records:** before and after values go in `tasks/T5-repo-settings.md`. An independent reviewer verifies them against live `gh` output and writes `tasks/T5-review.md`. A summary is posted to the issue.

## Task 6: Final gate, completeness review, PR and handoff

1. Run `make check`, then `make package`, once on the final head and record the results in `validation.md`.
2. Get a completeness review against this plan.
3. Do the artifact inventory.
4. Open the PR with the title "docs(release): release-ready 0.1.0 documentation and three-interface presentation". The body says `Refs #1`, lists changes and validation, and lists the remaining owner steps:
   - the derive trusted publisher;
   - the social preview, if it wasn't uploaded;
   - the homepage switch to docs.rs after publication;
   - authorization for `rust-v0.1.0`.
5. Post the plan to the issue and the PR, plus a least-confident note.

## Review dispositions (revision 2)

**Constraints:**

- B1: fixed with neutral scope wording; the plan contains no excluded topic. The rule is kept in an unposted stage note that covers docs, comments, the PR and commits, and no blocklist strings are committed.
- A1: notices for the new tooling entries.
- A2: the dedicated `docs/assets.json` plus the sidecar.
- A3: named tests.
- A4: a single font with no fallback (no text-to-path tool is installed, so the font is pinned instead).
- A5: exact-commit wording.
- A6: explicit-file commits in every task.

**Final review, pass 1:**

- B1: every banned-phrase correction moves into Task 1.
- B2: the README status links QUALIFICATION until the CHANGELOG exists.
- Advisories adopted: the CHANGELOG rule moves to Task 3, a deliberate-removal table for the migration check, the sidecar edit is listed in Task 4, and the vulnerability-reporting before-value is recorded.

**Adversary, pass 2:**

- A3(b): QUALIFICATION fuzz rows link to Known limitations, and affected rows state the stop.
- N1: whitespace-collapsed matching, broadened patterns, and a one-time baseline check over the `f11b45e` files.
- Note on `validate`: the claim is kept, with a new test.
- Note on the migration test: already a one-time check.

**Enhancement, pass 2:**

- Item 2 is narrowed and adopted: the `mcp-name:` token goes in Task 2, verified against the MCP Registry package-types documentation.
- Item 11: resolved by the one-time migration check.

**Completeness, pass 2:**

- N1: whitespace-collapsed matching.
- N2: the delivery-sequence fix moves under IMPLEMENTATION.
- N3: reporting is enabled before the SECURITY sentence.
- N4: resolved by the one-time migration check.

**Design, pass 2:**

- B3: the migration check becomes a one-time Task 2 focused check, with no `git` in `make check`.
- A8 and A9: adopted.

**Constraints, pass 2:**

- N1: SPDX ids.
- N2: the font is recorded by upstream identity, and the re-render comparison is a blocking step before commit.

**Completeness:**

- B1: QUALIFICATION 7–8/224/226 and the IMPLEMENTATION sequence are fixed in Task 1.
- B2: the banned-phrase rule with an explicit file scope.
- B3: every rule has a failing fixture.
- B4: example execution, plus generated per-tool forms.
- Advisories: SECURITY channels, the issue states verified on 2026-10-12, a literal description, the homepage, a Task 5 review, the IMPLEMENTATION boundaries, the RELEASE summary and jargon, the PR owner-step list, and the named digest file are all adopted.

**Design:**

- B1: dialect ids come from the doc stems plus a Rust assertion.
- B2: the PNG digest, tool version and font digest are pinned.
- A1: the generalized marker-copy rule.
- A2: packaged READMEs come from metadata.
- A3: zero-match failure.
- A4: superseded, because the census now uses the registry in Rust.
- A5: the Rust entry-point column is dropped from the generated block; the narrative names the facade module only where it already did.
- A6 and A7: adopted.

**Enhancement:**

- Item 1 (TRANSFORMATION): the generated, pinned TOOLS.md block is adopted.
- Item 2 (`server.json`): declined, see above.
- Items 3 and 6–10: adopted.
- Item 4: declined as an identity claim; docs corrected and the request exercised.
- Item 5: adopted.

**Adversary:**

- A1–A8: adopted as written (A6 as the one-time migration check, A7 as `docs/known-limitations.json`, A8 as the NOT DELIVERED rule).
