# Implementation completeness review

Branch `paudley/1-complete-purrassist-0-1-0-plan-and`, commits `f11b45e..f0d978a`
(dbdc64d, 871af51, f11912e, f0d978a), against `plan.md` revision 4: its
completeness contract, Tasks 1-6, the review dispositions, and the owner
decisions in the issue summary. Read-only review on 2026-10-12. `make check` and
`make package` were not run here; the coordinator is running them.

## Checks run here

| Command | Result |
|---|---|
| `python3 scripts/check-docs.py` | exit 0 ("Documentation facts, links, wording and known limitations match their sources.") |
| `python3 scripts/test-check-docs.py` | 178 tests, OK |
| `python3 scripts/check-repository.py` | exit 0 |
| `python3 scripts/test-repository-audit.py` | 48 tests, OK |
| `python3 scripts/check-release.py` / `test-check-release.py` | exit 0 / OK |
| `python3 scripts/check-release.py --preflight-owners --user-agent 'purrassist-release (...)'` (read-only) | "6 crates exist and share owners paudley", exit 0 |
| `cargo test -p purrassist-core --test tool_call_dialects` | 17 passed, including `built_in_ids_equal_the_documented_dialect_pages` |
| `cargo test -p purrassist-cli --test protocol -- tools_reference_matches_registry examples_run_through_cli_and_mcp validate_command_prints_validate_json_output every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` | 4 passed |
| `cargo test -p purrassist --doc` | 5 passed (the facade README is the crate doc, `crates/purrassist/src/lib.rs:5`) |
| `rsvg-convert -w 1280 -h 640 docs/social-preview.svg -o $TMP/r.png` | sha256 `a1045507...3338`, equal to the committed PNG and to `docs/assets.json`; font files match the recorded digests |
| `gh issue list --state open` | open issues are exactly #1 plus the ten in `docs/known-limitations.json` |
| Visual check of `docs/social-preview.png` | logo with glow, wordmark, tagline, "Rust crate · MCP server · CLI"; renders correctly |

## Completeness contract

| # | Requirement | Status | Evidence |
|---|---|---|---|
| 1 | Restated facts are sourced; stale facts corrected | DELIVERED | `scripts/check-docs.py:326-399` (release set from `RELEASE_ORDER`, workspace version, fuzz targets from `TARGETS` plus the Makefile, `RECEIPT_VERSION`), `:436-464` (dialect ids from the page stems, every file that lists them), `:478-498` (links), `:501-505` (`.stage`), `:62-75` + `:509-519` (the 12 banned regexes, exactly the plan list, matched on whitespace-collapsed text). SPDX on tracked `*.md`: `scripts/check-repository.py:272` with tests in `scripts/test-repository-audit.py` (`MarkdownAttribution`). Fixtures: `scripts/test-check-docs.py` per rule plus `FactSentencesAreRequired` (`:1232`) and the wrapped-phrase cases (`:400`, `:413`). Scope is git-tracked `*.md` minus `.stage/`, `vendor/`, `crates/core/tests/data/` (`check-docs.py:52`, `:1038-1041`). One-time baseline over `f11b45e`: recorded in `validation.md` (Task 1); all 17 inventory sites flagged, including the wrapped RELEASE 6-7, QUALIFICATION 7-8, 142-143 and 226-227. |
| 2 | Dialect ids have one source | DELIVERED | `crates/core/tests/tool_call_dialects.rs:680` `built_in_ids_equal_the_documented_dialect_pages`; passes. |
| 3 | Honest qualification and limits wording | DELIVERED | `docs/known-limitations.json` (10 issues, `blocks_release: false`, observed 2026-10-12, all live-open). `## Known limitations` in `docs/QUALIFICATION.md:211`, `docs/FUZZING.md:851`, `CHANGELOG.md:100`, checked by `check-docs.py:638-667`. FUZZING baseline table has `none` rows for `tool-call`/`tool-call-ref` (`docs/FUZZING.md:1009-1010`, rule `:686-712`). Archived-evidence commits read from `docs/qualification/*.json` (`:715-740`); Stage 2 section heading `docs/QUALIFICATION.md:270` (rule `:743-758`). Windows sentence cites Verify run 36702475819 and full commit `dc23bc8...` and says it does not cover later `main` (`docs/QUALIFICATION.md:145-149`, rule `:761-778`). Both `make fuzz` rows link `#known-limitations` and state the #11/#6 stop (`docs/QUALIFICATION.md:36`, `:38`, rule `:670-683`, fixture `FuzzRows`). |
| 4 | Tool reference is a generated projection of the registry | DELIVERED | `crates/cli/tests/protocol.rs:1309` `tools_reference_matches_registry` (byte equality, line diff, `PURRASSIST_BLESS=1`, panics under `CI`); block `docs/TOOLS.md:29-509` with MCP name, CLI form and example links per tool. 31 hand-written `### <tool>` headings (`docs/TOOLS.md:533-872`) checked one-per-name by `check-docs.py:842-866`; `read_file` (`:813`) and `undo_file` (`:841`) added. |
| 5 | Every example request runs through CLI and MCP | DELIVERED | `crates/cli/tests/protocol.rs:914` `EXAMPLES`, `:1002` `examples_run_through_cli_and_mcp`: no orphans, fresh roots, identical and output-schema-valid results, temperature request through `decode_json` in both transports. |
| 6 | README presents three equal interfaces | DELIVERED | `README.md:24-101` marker block with `### Rust crate`, `### MCP server`, `### CLI`. Rust snippet is a byte copy under `three-interfaces-rust` markers (`README.md:37-57` = `crates/purrassist/README.md:12-32`, doctested), enforced by the generalized `check_marker_copies` (`scripts/check-repository.py:312-350`, names `derive-example` and `three-interfaces-rust`). MCP config parsed by `check-docs.py:924-944`. `.purrassist/` note `README.md:83-85` (true for `mcp` and `call`: `crates/host/src/files.rs:324-333`, `crates/cli/src/commands.rs:386`, `:397`). CLI-MCP sentence names `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` (`README.md:94-98`; existence checked at `check-docs.py:905`). `decode`/`match` called shortcuts with their own output (`README.md:98-99`). `validate` claim backed by `protocol.rs:1073` `validate_command_prints_validate_json_output`. Deviation (accepted): the copy rule uses a nested `three-interfaces-rust` marker, since only the Rust snippet is a copy. |
| 7 | No README claim lost in the move | DELIVERED | One-time migration check and output in `validation.md` (Task 2): 248 tokens, 14 link targets, 2 deliberate removals with reasons, 0 links lost. Accepted in `tasks/T2-review.md:42`. |
| 8 | Crate map and READMEs release-ready | DELIVERED | `README.md:103-118`; `check-docs.py:869-883` (equals `RELEASE_ORDER`, internal crates named). Packaged READMEs from `cargo metadata` (`:803-834`, `:947-962`): no banned phrase, no `docs.rs/.../latest` (derive links pinned to `0.1.0`, `crates/derive/README.md:93-95`). IMPLEMENTATION boundaries list equals workspace members with internal crates marked (`docs/IMPLEMENTATION.md:88-104`, rule `:965-986`). |
| 9 | MCP Registry ownership token in the cli README | DELIVERED | `crates/cli/README.md:103` visible Links bullet `` MCP Registry name: `mcp-name: io.github.Blackcat-Informatics/purrassist` ``; `check-docs.py:989-1006` derives the owner from the workspace `repository` URL and strips comments and fences; fixtures `RegistryName` (missing, commented-out, fenced, case-sensitive). `server.json` listed as post-publication in `docs/RELEASE.md:255-264`. |
| 10 | Crate metadata | DELIVERED | `crates/cli/Cargo.toml:6-15` (description, keywords `mcp cli json recovery agents`, categories `command-line-utilities development-tools`, documentation = `docs/TOOLS.md`); `crates/purrassist/Cargo.toml:6` description; `mcp` keyword on host (`crates/host/Cargo.toml:12`) and cli. No `homepage`. Non-docs.rs link checked at `check-docs.py:995-998`. `check-release.py` passes. Archive audit is Task 6. |
| 11 | Release docs are current | DELIVERED | `docs/RELEASE.md:5-10` (exact `main` commit to be tagged), `:14-17` (six crates, checked), `:37-62` (reservation table, derive minted 2026-10-12; rule `check-docs.py:589-600`), `:83-96` (dated trusted-publisher observation, derive owner-pending), `:103-120` (all 8 named steps of `release-qualification.yml`; rule `:603-630`), "Task 10" removed (`:205`). Preflight recorded in `validation.md` (Task 3) and re-run here, exit 0. |
| 12 | 0.1.0 CHANGELOG | DELIVERED | `CHANGELOG.md:10` `## [Unreleased]`, `:14` `## [0.1.0] - unreleased`; six crates, three interface highlights, MSRV 1.98 and the ABI marker rule (`:88-98`), Known limitations (`:100-115`); rule `check-docs.py:555-574` + `:638-667`. RELEASE names it as the release body (`docs/RELEASE.md:245-247`). README links it (`README.md:18`, rule `:633-635`). |
| 13 | SECURITY.md | DELIVERED | `SECURITY.md:5-10` table with `0.1.x` and `0.0.0`, both channels (`:16-19`); rule `check-docs.py:577-586`. Reporting was enabled (13:15:49 -06:00, `tasks/T5-repo-settings.md`) before the commit that names it (f11912e, posted 19:23Z). |
| 14 | Social preview image | DELIVERED | `docs/social-preview.svg` (SPDX, 1280x640, logo inlined with no external `href`, one `font-family="Noto Sans"`), `docs/social-preview.png` (112,623 bytes), `docs/assets.json` (both digests, command, tool and library versions, font upstream, version and file digests, license), sidecar `docs/social-preview.png.license`, sidecar scan `scripts/check-repository.py:295` with `REQUIRED_BINARY_ATTRIBUTION`. `vendor/tooling.json` entries with notices under `vendor/licenses/`; `docs/DEPENDENCIES.md:325-335`. Asset rule `check-docs.py:1009-1035` with fixtures `SocialPreview`. Re-render reproduced byte-identically (here and in `validation.md`). |
| 15 | GitHub presentation | DELIVERED except the social-preview upload | Description, homepage, 13 topics, wiki and projects off, and private vulnerability reporting `{"enabled":true}` are recorded before and after in `tasks/T5-repo-settings.md` and checked against live state in `tasks/T5-review.md` (PASS). Social-preview upload is **NOT DELIVERED**: the browser session got a 404 on Settings and is not signed in. Live `og:image` is still the default card. Per the plan this is not counted as done. It goes to the owner: upload `docs/social-preview.png` under Settings > General > Social preview, or drop it from this stage. Accepted as an owner step. |
| 16 | Local gate and package | PENDING (Task 6) | The coordinator is running `make check` and `make package`. `make hygiene` now runs `check-docs.py` and `test-check-docs.py` (`Makefile:16-17`). |
| 17 | PR and honest handoff | PENDING (Task 6) | No PR exists yet for the branch. The plan is posted to issue #1 (2026-10-12T18:35:44Z), with one update per task. The PR body must say `Refs #1` and list the owner steps: derive trusted publisher, social-preview upload, homepage switch to docs.rs after publication, and `rust-v0.1.0` authorization. |

## Owner decisions

| Decision | Status |
|---|---|
| Hosted CI out of scope | Respected. No doc claims a hosted pass on current `main`. The only cited run is bound to `dc23bc8...` and says it does not cover later `main` (`docs/QUALIFICATION.md:145-149`). Other hosted-CI mentions describe workflow configuration or future release steps (`docs/RELEASE.md:238`, `docs/QUALIFICATION.md:248-252`, `:256`), and `:251` says "a green run applies to its commit only". |
| #6, #7, #10, #11 do not block; disclosed | Done (with #12-#14 and #16-#18), `blocks_release: false`, enforced by `check-docs.py:548-549`. |
| Scope limited to docs; no tag or publication | Respected. No tag, and every status line says "not yet tagged". |
| Derive 0.0.0 reservation recorded | `docs/RELEASE.md:50`; preflight confirms six crates with identical owners. |
| Repository public settings | Delivered (row 15). |
| One engine, three equal interfaces | README block, crate maps, every crate README status line, CHANGELOG highlights, repository description, social preview. |
| Social-preview image | Asset delivered; upload left as an owner step (row 15). |

## Flags requested

- **Agent attribution:** none in the four commit messages (all authored by
  Patrick_Audley, no trailers), the changed files, or the recent issue #1
  comments.
- **Hosted-CI provider accounts or payments:** no such wording in the changed
  docs, the commit messages, the T5 records, or the recent issue comments.
- **Capability claims without executable evidence:** none found in the
  changed docs. The new claims are each backed: CLI-MCP identity, `validate`
  identity, example execution, tool reference, `.purrassist/` creation (code
  path), the reproducible render, and the open issue set.
- **Publication overclaim (fix before the PR; wording, not an omission):**
  - `README.md:105` reads "Six crates are published, in this release order".
    0.1.0 is not published; only the 0.0.0 name reservations are. This
    conflicts with the status line at `README.md:15-17`. Suggested wording:
    "Six crates make up the release set, published in this order".
  - `CHANGELOG.md:7-8` says "All six published crates share one version". This
    is weaker and arguably refers to the publishable set. Consider "All six
    release crates".
- **Process jargon in public docs (advisory; none of it was introduced as new
  process wording):**
  - `docs/QUALIFICATION.md:270` "Historical: Stage 2 record bound to
    `66b3756`". The plan requires this exact heading, and the `stage2-heading`
    rule enforces it.
  - "stage-3" in the #7 entry (`docs/known-limitations.json`, rendered in
    QUALIFICATION, FUZZING and CHANGELOG). It is the verbatim live issue title.
  - These two lines predate the branch, are outside the plan inventory, and no
    banned phrase covers them:
    - `docs/FUZZING.md:986` "on `main` as on this branch"
    - `docs/RESOURCES.md:213` "during this task"

    They are worth a one-line cleanup, but they do not block.

## Minor observations (non-blocking)

- `workflow_step_names` (`check-docs.py:603-614`) reads `name:` lines. A
  future `run:` step without a `name:` would not be required in the RELEASE
  summary. Today every `run:` step is named
  (`.github/workflows/release-qualification.yml:27-51`).
- `## [Unreleased]` is present but not enforced, which the
  `test_a_missing_unreleased_heading_still_needs_the_version` fixture
  documents.
- The derive README links pin `docs.rs/purrassist-core/0.1.0`, as the plan
  says. They will 404 until publication.

## Required omissions

None in Tasks 1-5. Every contract row for Tasks 1-4 is DELIVERED with
executable evidence. Task 5 is DELIVERED except the honestly recorded social-
preview upload, which is an owner step. Task 6 rows are pending the
coordinator's single gate run and the PR.

VERDICT: PASS
