# Gap analysis: PR #20, issue #1 (Stage 2, lightweight path)

- Branch: `paudley/1-complete-purrassist-0-1-0-plan-and`, head `a42a817`, base `origin/main` (`f11b45e`)
- Files changed: 35 (5 commits)
- Inputs: plan.md rev 4 (contract and dispositions), issue-analysis.md, validation.md, tasks/T1..T5, reviews/implementation-completeness.md, raw/pr-20*.json, output-rules.md
- Run here on 2026-10-12: every focused command in the rubric, a real CLI call, a real MCP round trip, checker mutation probes, the README migration re-check, and a PNG re-render.
- Not run here: `make check` and `make package`. Their final-head results in validation.md are reused (see "Gate applicability").

## (a) Findings

Required findings are the CRITICAL and HIGH ones, plus any finding marked "required". There are no CRITICAL or HIGH findings. One MEDIUM finding is marked required: the documented release path would publish a false statement, and nothing checks for it.

### MEDIUM

**M1 (required). The 0.1.0 release body would ship "not yet tagged".**
- Where: `CHANGELOG.md:16-18`, `docs/RELEASE.md:235-237`, `docs/RELEASE.md:245-247`.
- Problem:
  - The `[0.1.0]` section says "It is not yet tagged `rust-v0.1.0`; the date above is set when it is tagged."
  - RELEASE.md step 1 changes only the heading date.
  - Step 5 uses that section as the GitHub release body.
  - So following the procedure as written publishes a stale "not yet tagged" sentence in the 0.1.0 release, and no rule catches it. `CHANGELOG.md` is not in `STATUS_DOCS`.
- Fix: delete that sentence from the `[0.1.0]` section; the `- unreleased` heading already carries the status. Keep "The release procedure records the publication gates." Alternatively, extend step 1 to "set the date and remove the 'not yet tagged' sentence from the section".

**M2 (advisory). The checker conflicts with the documented post-publication edit.**
- Where: `scripts/check-docs.py:117` and `:346`, `docs/RELEASE.md:254`.
- Problem: RELEASE.md says to update the "not yet tagged" status lines after publication. The `workspace-version` rule requires `not yet tagged \`rust-vX.Y.Z\`` in five documents, so that edit fails the gate.
- Probe: replacing the README status with "Published as `rust-v0.1.0`." gives `[workspace-version] README.md: fact sentence not found`.
- This fails safe, but the procedure never mentions the checker.
- Fix: the rule should accept either the unpublished or the published status sentence, still matching the version, or the RELEASE.md bullet should name the rule to update in the same commit.

### LOW (advisory)

**L1. "Published in this order" before publication.**
- Where: `CHANGELOG.md:41`.
- Problem: this reads as a past-tense publication claim while 0.1.0 is unpublished. The README twin was fixed in `a42a817`.
- Fix: "Release order:".

**L2. "Every result carries" a receipt overstates.**
- Where: `CHANGELOG.md:63-65`.
- Problem: the generated reference shows that `read_file`, `undo_file`, `recover_files` and `commit_files` return no `receipt` (`docs/TOOLS.md` generated block). The wording echoes `docs/IMPLEMENTATION.md:10`, but in the release body it reads as universal.
- Fix: "every recovery, matching, Markdown and edit-plan result carries a receipt of ...".

**L3. "Compares the output byte for byte" is inexact.**
- Where: `docs/TOOLS.md:22-23`.
- Problem: the test compares the `to_json()` serializations of the parsed CLI and MCP results (`crates/cli/tests/protocol.rs:768-772`), not raw output bytes.
- Fix: "compares their JSON serializations for equality".

**L4. The asset purpose says the image is uploaded.**
- Where: `docs/assets.json:5`.
- Problem: it reads "uploaded manually in the repository settings", but the upload has not happened (T5 record, live `og:image` is the default card).
- Fix: "for manual upload in the repository settings".

**L5. Process jargon in a hand-written impact line.**
- Where: `docs/known-limitations.json:14`, rendered at `CHANGELOG.md:107`, `docs/QUALIFICATION.md:218` and `docs/FUZZING.md:858`.
- Problem: the impact text "Some stage-3 commits ..." is not the verbatim issue title (line 13), so it can be reworded.
- Fix: "Some commits of the #3 tool-call work were merged without an audit ...", then update the three rendered copies together (the equality rule enforces this).

**L6. Process jargon in lines this branch did not touch.**
- Where:
  - `docs/RESOURCES.md:1073` ("Task 17's")
  - `docs/DEPENDENCIES.md:46` ("Task 7")
  - `docs/FUZZING.md:74` ("Task 8")
  - `docs/tool-calls/INTEGRATION.md:156` ("stage-3 pass 5")
  - `docs/QUALIFICATION.md:270` ("Stage 2"; the plan requires this heading)
- These are outside the plan inventory, so they are not a branch gap.
- Fix: a one-line rewording each. Once they are clean, add `\bTask \d+\b` to `BANNED_PHRASES`.

**L7. The `Pre-release` ban also catches ordinary SemVer wording.**
- Where: `scripts/check-docs.py:72`.
- Problem: it is a case-insensitive ban. Probe: "A pre-release version such as `0.2.0-alpha.1` ..." gives `[banned-phrase]`.
- Fix: anchor it to the stale form, `Pre-release implementation`. That still flags all four former crate-README sites and README:14.

**L8. Unnamed workflow steps escape the qualification-summary rule.**
- Where: `scripts/check-docs.py:603-614`.
- Problem: only `- name:` lines are read. Probe: appending `- run: make wasm-test` to `release-qualification.yml` gives 0 errors. It would also treat a `- name:` item in a non-step list (for example matrix `include`) as a step.
- Fix: inside `steps:` blocks, collect every `- ` item and fail on an item with `run:` and no `name:`.

**L9. The sidecar rule has a fixed, case-sensitive suffix list.**
- Where: `scripts/check-repository.py:281-292`.
- Problem: probe in a temp repository: `b.PNG`, `c.webp` and `d.ttf` are not required to carry a sidecar, while `a.png` is. Today the only tracked binary outside the test data is `docs/social-preview.png`, which is covered.
- Fix: select binaries from `git ls-files --eol` (`i/-text`), or compare suffixes case-insensitively and add `.webp`, `.svgz`, `.ttf`, `.otf`, `.woff`, `.woff2`.

**L10. Windows Python portability.**
- Where: `scripts/check-docs.py:277`, `:282` and others; the new `scripts/check-repository.py:275` and `:302`.
- Problem: `read_text()` is called without `encoding='utf-8'`. Six tracked docs (for example `docs/tool-calls/README.md`) do not decode as cp1252, so a Windows Python without UTF-8 mode would raise UnicodeDecodeError. That fails closed. `scripts/test-check-docs.py` also symlinks every tracked file, which needs symlink privilege on Windows.
- `make check` runs only on Linux and macOS lanes; the Windows lane runs `cargo test` only. Linux and macOS are fine: pathlib, posix names from git, tomllib (Python 3.11 or later), and `cargo metadata --no-deps --offline` needs no registry access.
- Fix: pass `encoding='utf-8'` explicitly, matching `read_bytes` elsewhere.

**L11. Immutable package metadata links to `blob/main`.**
- Where: `crates/cli/Cargo.toml:15`, `crates/cli/README.md:96` and `:101`, `crates/host/README.md:47`, `crates/purrassist/README.md:96`, `crates/syntax/README.md:26`.
- Problem: the 0.1.0 crate pages will point at whatever `main` documents later. This follows the existing `blob/main` precedent (`crates/core/README.md:53`, `crates/cli/README.md:64`), and the plan specified the repository link.
- Optional fix: pin to `blob/rust-v0.1.0/...`. These links resolve once tagged, the same trade-off the plan already accepted for the derive docs.rs `0.1.0` pins.

**L12. The fuzz-stop rules hard-code exactly two stop issues.**
- Where: `scripts/check-docs.py:660-665` and `:680-683`.
- Problem: fixing #6 or #11 forces a checker edit along with the docs. That is acceptable coupling, but it is undocumented.
- Fix: say so in the rule's docstring, or derive the stop set from a field in `known-limitations.json`.

### Owner-gated item (not a branch gap)

- **Social-preview upload (contract row 15): NOT DELIVERED.**
- It is not classed CRITICAL as deferred work, for three reasons:
  - the plan's contract defines this exact outcome in advance and does not count it as done;
  - the action needs a signed-in owner browser, and GitHub has no API for it;
  - the PR body lists it as owner step 2 and says "not done".
- The owner's choice between uploading it and dropping it from this stage is not yet recorded.

## (b) Reviewer feedback

- None.
- `raw/pr-20.json` has 0 reviews and an empty `reviewDecision`. `raw/pr-20-inline-comments.json` is `[]`. `raw/pr-20-threads.json` has no review threads.
- The PR's 2 conversation comments are the stage's own plan and handoff posts, with no requests.
- The issue comments from 2026-10-12 are the plan and the task updates; there is no new owner instruction.

## (c) Consumer wiring

| Consumer | Status | Evidence |
|---|---|---|
| `check-docs.py`, `test-check-docs.py` in `make hygiene`, which feeds `make check` | WIRED | `Makefile:6` (`check: fmt hygiene ...`), `Makefile:16-17`. Final-head gate log shows both ran (178 tests OK). |
| New Rust tests in workspace test runs | WIRED | `cargo test --workspace --locked --no-run` lists `tests/protocol.rs` and `tests/tool_call_dialects.rs`. Neither is a `test = false` corpus suite (`crates/core/Cargo.toml:64-100`). `make test` = `cargo test --workspace --locked` (`Makefile:26`). The windows-engines job runs `cargo test -p purrassist-host -p purrassist-cli` and `-p purrassist-core` (`verify.yml:56-58`). |
| `check-repository.py` binary-sidecar rule | WIRED | `scripts/check-repository.py:371` calls `check_binary_attribution(ROOT, tracked_binaries(ROOT))`, and `REQUIRED_BINARY_ATTRIBUTION` pins `docs/social-preview.png` even if untracked. `test-repository-audit.py` gives 48 tests OK. Coverage gap: L9. |
| Markdown SPDX scan | WIRED | `scripts/check-repository.py:370`. Scope is tracked `*.md` minus `crates/core/tests/data/`. |
| Cargo metadata in packaged archives | FLOWS | The retained final-head archives `.cache/package-archives/a42a817af4f1-h6qtf08n/` were inspected. `purrassist-cli-0.1.0/Cargo.toml` has the new description, documentation, keywords and categories, and its README line 103 carries the visible `mcp-name:` bullet. The `purrassist` and `purrassist-host` archives carry the new description and keywords. |
| `docs/TOOLS.md` generated block | TEST-OWNED | `tools_reference_matches_registry` byte-pins the block (`crates/cli/tests/protocol.rs:1309-1348`). Mutation probe here: renaming `read_file` to `read_files` in the block fails with a line diff (`line 377`). The file was restored and its sha256 verified unchanged. `PURRASSIST_BLESS=1` panics under `CI`. `check-docs.py` reads only the names from the block to count the hand-written headings (`:842-866`); probe: a heading for an unknown tool fails. |

## (d) Checker correctness, robustness, portability, cost

- **Correctness.** Every rule reads its fact from the owning source and fails on zero matches. T1 to T4 recorded per-rule mutation probes. Probes here add:
  - the status-line false positive (M2);
  - the SemVer false positive (L7);
  - the unnamed-step false negative (L8);
  - the sidecar false negatives (L9).
- **Portability.**
  - `make check` lanes are Linux and macOS; names are posix from `git ls-files`.
  - `cargo metadata --no-deps --offline` needs no registry.
  - `.gitattributes` pins `eol=lf` and `*.png binary`, so the Rust byte-pinned TOOLS.md test and the example test are CRLF-safe on the Windows test lane.
  - The Windows Python risks are L10 and are not exercised by any lane.
- **Git dependence.** Git is now required by `check-repository.py` too: `tracked_markdown` and `tracked_binaries` are new uses of `git ls-files`. Its only caller is `make hygiene` in a checkout, so this is acceptable.
- **Runtime.**
  - `check-docs.py`: 0.44 s, about 0.13 s of it in `cargo metadata`, cached by manifest digest.
  - `test-check-docs.py`: 29.2 s for 178 tests (about 0.09 s tree build plus about 0.17 s per check). It is the largest single hygiene step but acceptable.
  - `check-repository.py` and its tests: about 2 s.

## (e) Claim accuracy

| Claim | Verdict | Evidence |
|---|---|---|
| Three equal interfaces (README:24-101, crate READMEs, CHANGELOG highlights, social preview) | ACCURATE | Facade re-exports core, syntax and derives (`crates/purrassist/src/lib.rs:8-76`). CLI commands `decode/validate/match/recover-tool-calls/call/mcp` (`crates/cli/src/commands.rs:36-40`). |
| `.purrassist/` created by `mcp` and `call` | ACCURATE | Real runs here: a temp root contains only `.purrassist/` after both the CLI call and the MCP session. |
| Newline-delimited JSON-RPC over stdio | ACCURATE | Real MCP round trip here. |
| CLI and MCP identical for every registered tool | ACCURATE | `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` (`protocol.rs:744-788`) finds a fixture per tool and asserts fixture count equals registry count. "Byte for byte" wording: L3. |
| Arguments validated against the input schema before the handler runs | ACCURATE | `crates/host/src/tools/registry.rs:411-421`. |
| Every `examples/*.json` runs through CLI and MCP | ACCURATE | `examples_run_through_cli_and_mcp` passes. |
| Status: merged to `main` (PR #2, `699d9ff`), not yet tagged | ACCURATE | `699d9ff` is on `origin/main`. |
| Windows run bound to `dc23bc8`; "does not cover later changes" | ACCURATE | `dc23bc8` is an ancestor of `699d9ff`. |
| No publication overclaim | ACCURATE except L1 | README:15-17, SECURITY:9, RELEASE:9-10, CHANGELOG:7-8 and :14 are all conditional or unpublished; CHANGELOG:41 is L1. |
| No hosted pass claimed for current `main` | ACCURATE | The only added hosted-CI lines are procedure steps (QUALIFICATION:257, RELEASE:238). |
| Receipt coverage | OVERBROAD | L2. |
| Asset upload state | INACCURATE (minor) | L4. |
| Release body after the documented procedure | WOULD BE INACCURATE | M1. |
| Process jargon | Mostly clean | New: L5. Pre-existing: L6. D-F3 grep returns nothing. |
| Attribution | NONE | Commit messages: no trailers and no agent names. Diff: no attribution strings; "Anthropic" appears only as dialect and product vocabulary. |
| Hosted-CI account or payment wording | NONE | No such wording in the diff, commit messages, PR body or handoff. |

## (f) Completion rubric (executed on the real entry points)

| # | Contract row | Demonstration run here | Result |
|---|---|---|---|
| 1 | Restated facts sourced; stale facts corrected | `python3 scripts/check-docs.py`; `python3 scripts/test-check-docs.py` | exit 0; 178 tests OK (29.2 s). One-time f11b45e baseline: validation.md, 17/17 sites. |
| 2 | Dialect ids have one source | `cargo test -p purrassist-core --test tool_call_dialects` | 17 passed, including `built_in_ids_equal_the_documented_dialect_pages` |
| 3 | Honest qualification and limits wording | `check-docs.py` (known-limitations, fuzz-row, baseline, archived-evidence, windows-run rules) plus test fixtures | pass |
| 4 | Tool reference is a registry projection | `cargo test -p purrassist-cli --test protocol` (8 passed, including `tools_reference_matches_registry`); mutation probe fails as expected; MCP `tools/list` names equal the generated block in order | pass |
| 5 | Examples run through CLI and MCP | `examples_run_through_cli_and_mcp` passed; real `cargo run -q -p purrassist-cli -- call match_strings --root <tmp> < examples/match-strings.json` | exit 0, top match `tea`, receipt v3, `termination: completed` |
| 6 | README presents three equal interfaces | `python3 scripts/check-repository.py` (marker copies `derive-example`, `three-interfaces-rust`); `cargo test -p purrassist --doc` (5 passed); `check-docs.py` three-interfaces and mcp-config rules; `validate_command_prints_validate_json_output` passed | pass |
| 7 | No README claim lost | Re-ran the validation.md migration script | 248 tokens and 14 links; the same 2 deliberate removals; 0 links lost |
| 8 | Crate map and READMEs release-ready | `check-docs.py` crate-map, packaged-readmes (from `cargo metadata`) and workspace-boundaries rules | pass |
| 9 | MCP Registry token | `check-docs.py` registry-name rule; packaged cli README line 103 (final-head archive) | pass |
| 10 | Crate metadata | `python3 scripts/check-release.py` (exit 0, six crates in RELEASE_ORDER); `test-check-release.py` OK; archive Cargo.toml inspected | pass |
| 11 | Release docs current | `check-docs.py` release-set, reservation and qualification-step rules. The read-only preflight output is reused from validation.md; it is a registry read, not rerun. | pass. M1 and M2 concern the future release path. |
| 12 | 0.1.0 CHANGELOG | `check-docs.py` changelog and known-limitations rules | pass (content: M1, L1, L2) |
| 13 | SECURITY.md | `check-docs.py` security-policy rule | pass |
| 14 | Social preview image | `rsvg-convert -w 1280 -h 640 docs/social-preview.svg -o <tmp>`: sha256 `a1045507...3338` equals the committed PNG and `docs/assets.json`; SVG `c7240946...cac26` matches; PNG viewed (wordmark, tagline, "Rust crate · MCP server · CLI"); `check-docs.py` social-preview rule; `check-repository.py` sidecar | pass |
| 15 | GitHub presentation | Live state from T5-review.md (description, homepage, 13 topics, wiki and projects off, private reporting enabled). Not re-queried, by instruction. | delivered except the upload (owner-gated) |
| 16 | Local gate and package | Reused from validation.md; applicability judged below | pass |
| 17 | PR and honest handoff | `raw/pr-20.json`: body starts `Refs #1` and lists the owner steps (derive trusted publisher, social-preview upload, tag authorization, homepage switch, `server.json`) | pass |

Also run here: `python3 scripts/test-repository-audit.py` (48 OK). MCP round trip with the real binary (`purrassist mcp --root <tmp>`):
- `initialize` reports serverInfo `purrassist 0.1.0` and protocol `2025-11-25`.
- `tools/list` returns 31 tools.
- `tools/call match_strings` returns `isError=false`.
- Exit status is 0 with empty stderr.

### Gate applicability (reused, not rerun)

- `make check` (validation.md, `/tmp/purr-make-check2.log`, 110 `test result: ok` groups, no warnings or failures) finished at 13:37:57. `a42a817` was recorded at 13:38:01.
- The four files that `a42a817` changes were last written at 13:36:34 and are unchanged since; the tree is clean apart from `.stage/`.
- So the run covered byte-identical content to `a42a817`. It ran on the working tree seconds before the commit was recorded.
- `make package` ran after the commit; its retained archive directory is named `a42a817af4f1-...`, "Verified 6 release archives".
- Both results are judged applicable to the final head.
- The focused checks run here on `a42a817` independently pass.

VERDICT (original review of a42a817): BLOCKED (superseded for M1 by the remediation recheck below)

---

## Remediation recheck (units 1-3)

Reviewed on 2026-10-12 against `remediation-plan.md` revision 4.
- Unit 1 is `4933cc4` and unit 2 is `d6fb1e0`, both landed.
- Unit 3 is the uncommitted checker change in the worktree: `CHANGELOG.md`, `Makefile`, `docs/RELEASE.md`, `docs/known-limitations.json`, `scripts/{check-docs,test-check-docs,check-repository,test-repository-audit,check-release,fuzz}.py`.
- G11 (unit 4) has not landed, so this is not a final completion verdict.

### Commands run

| Command | Result |
|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py` | exit 0 |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py` | 233 tests OK (42.7 s) |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-repository.py` | exit 0 |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-repository-audit.py` | 52 tests OK |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | exit 0 |
| `make hygiene` (Makefile now passes the flags to the four scripts) | exit 0, 49 s; check-repository, test-repository-audit (52), check-release, test-check-release (18), check-docs, test-check-docs (233), test-fuzz (78), test-resource-report (14), resource-limits schema |
| `cargo test -p purrassist-cli --test protocol` | 8 passed |
| `cargo test -p purrassist --doc` | 5 passed |
| `git grep -iE '\btask [0-9]+\b\|stage-3 pass\|adversary pass'` over in-scope `*.md` | empty |

Observation, outside the plan's G8 scope: `scripts/test-check-release.py` (7 sites) and `scripts/test-fuzz.py` (10 sites) still open files with no encoding. They raise under the warning flag, but the Makefile runs them without it, so hygiene is green. The libraries they test (`check-release.py`, `fuzz.py`) were made explicit and are covered by the new AST test (`Portability.test_text_is_read_and_written_with_an_explicit_encoding`).

### Independent mutation probes

These run on `test-check-docs.Tree` copies; the worktree was not modified.

| Probe | Result |
|---|---|
| A: "unreleased" inside the `### Known limitations` subsection of `[0.1.0]` | `[changelog] CHANGELOG.md:102: ... release body, which must not contain 'unreleased'` (caught) |
| B: #6 and #11 both fixed (no `fuzz_stop_targets`, docs say `make fuzz` passes) | 4 errors: "fact sentence not found: the default `make fuzz` stop ..." and "a `make fuzz` row must state ... stops" (false positive) |
| C: README status reads "Published as `rust-v0.1.0`." with a capital P, the other four in lowercase | `[workspace-version] README.md: fact sentence not found` (false positive) |
| D: `https://github.com/blackcat-informatics/purrassist/blob/main/...` (lowercase owner) in `crates/core/README.md` | 0 errors (false negative) |
| E: three stop issues written with an Oxford comma, "#11, #6, and #10", and the JSON agreeing | 4 errors, "fact sentence not found" (false positive; it fails closed) |
| F: unnamed `- run:` step in a second job of `release-qualification.yml` | `[release-qualification-steps] ...:56: a step that runs a command needs a name:` (caught) |
| G: "This follows Task 19." in `docs/RELEASE.md` | `[banned-phrase] ... 'Task 19'` (caught) |

The plan's own fixtures cover:
- the re-added sentence (wrapped included);
- the published-form, mixed-form and wrong-version status lines;
- the `blob/main`, missing-path and missing-anchor pins;
- content-selected `.PNG`, `.webp` and `.ttf`;
- the run-body `- ` line and matrix `include`;
- an unknown `fuzz_stop_targets` value;
- the symlink-refused copy path.

### Per finding

| Finding | Status | Evidence |
|---|---|---|
| M1 (required) | FIXED | The sentence was removed in `4933cc4`. `RELEASE_BODY_BANNED` (`not yet tagged`, `unreleased`, `published in this order`) is checked over the positional `## [<version>]` body (`changelog_section`). `### Known limitations` now sits inside `[0.1.0]`, so the release body carries it (A5), and RELEASE.md step 5 says so. The per-version lookup has fixtures (N3). Probe A caught. |
| M2 | FIXED | `STATUS_FORMS` accepts `not yet tagged` or `published as`, with the workspace version. A mix fails (A4). The RELEASE.md "After publication" bullet names the form and the five files. Residual: probe C, new N-1 below. |
| L1 | FIXED | `CHANGELOG.md` reads "Release order:", and the release-body rule bans "published in this order" in the version section only (N1; a README fixture passes). |
| L2 | FIXED | The receipt sentence is scoped to recovery, matching, Markdown and edit-plan results; `every result carries` is banned. |
| L3 | FIXED | `docs/TOOLS.md` reads "compares their JSON serializations for equality"; the old phrase is banned. |
| L4 | FIXED | `docs/assets.json` reads "for manual upload in the repository settings"; `uploaded manually` is banned. |
| L5 | FIXED | The #7 impact reads "Some commits of the #3 tool-call work ..." in the JSON and all three rendered copies. The title stays verbatim. |
| L6 | FIXED | RESOURCES, DEPENDENCIES, FUZZING and INTEGRATION were reworded, plus the A7 "adversary pass N" sites (INTEGRATION, MATCHING-QUALITY, qwen3 dialect page). `\btask \d+\b` is banned and probe G caught it. The `Stage 2` historical headings stay by plan. |
| L7 | FIXED | The ban is anchored to `Pre-release implementation`; a SemVer "pre-release version" fixture passes. |
| L8 | FIXED | Steps are parsed by indentation (`workflow_steps`/`block_steps`). An unnamed `run:` fails, unnamed `uses:` is ignored, a name after `run:` is associated, and run bodies and matrix items are not steps. Probe F caught. |
| L9 | FIXED | Binaries are selected from the `i/-text` column of `git ls-files -z --eol` with no suffix list. Fixtures cover `.PNG`, `.webp`, `.ttf`, `.dat` and no extension, plus text files that are not selected, and the exemption stays only `crates/core/tests/data/`. |
| L10 | FIXED | `encoding='utf-8'` on every read, write and subprocess call in the four scripts plus `check-release.py` and `fuzz.py`. The Makefile flags enforce it, an AST test guards it, and `link_or_copy` falls back to copying when symlinks are refused (tested with a mock and a full tree). |
| L11 | FIXED | Crate READMEs and the cli `documentation` pin `blob/rust-v0.1.0/...`. `check_pinned_links` requires `rust-v<workspace version>` and checks that the path and `#anchor` resolve (A2). Residual: probe D, new N-2 below. |
| L12 | FIXED (follow-up 2) | The stop set comes from `fuzz_stop_targets`, validated against `TARGETS` (A3), and 1, 2 or N issues are read. When the JSON names no stop, the documents must carry "stops on no known signature" (`NO_STOP_SENTENCE`, `stop_issues`). Probes: B1 (JSON empty and docs say none) gives 0 errors, so fixing both #6 and #11 needs no checker edit. B2 (JSON empty, docs still name #11/#6) gives 4 "extra" errors. B3 (JSON names stops, docs say none) gives 4 "missing" errors. B6 (JSON empty, rows drop any stop clause) gives 3 "fact sentence not found" errors. Residual: N-4 below. |

### New findings from the probes (LOW, advisory)

**N-1. A capital "Published as" is not accepted.**
- Where: `scripts/check-docs.py:370`.
- Problem: the status regex is case-sensitive. A sentence-initial "Published as `rust-v0.1.0`." (the natural README form, since the README status line is a sentence) is not found.
- Fix: match the form group case-insensitively, for example `(?i:not yet tagged|published as)`, or document the lowercase form as mandatory in RELEASE.md.

**N-2. Pinned-link checks miss a differently cased owner.**
- Where: `scripts/check-docs.py:1127`.
- Problem: GitHub owner and repository paths are case-insensitive, but the pattern is case-sensitive, so a `blob/main` link with a differently cased owner escapes the rule.
- Fix: compile the repository prefix with `re.IGNORECASE`, keeping the tag and path parts case-sensitive.

**N-3. An Oxford-comma stop sentence is not read.**
- Where: `scripts/check-docs.py:764`.
- Problem: "#11, #6, and #10" is not parsed. It fails closed.
- Fix: `(?:,? and #\d+)?`.

### Scope note

Unit 3 also edits `scripts/check-release.py` and `scripts/fuzz.py`, which are not in G8's list of four scripts. The edits only add explicit encodings, which `check-docs.py` needs because it loads both modules and the AST test covers them. `test-fuzz.py` and `test-check-release.py` pass, so this is in keeping with the plan and not a finding.

### Follow-up 2: leftover fixes (same uncommitted unit 3)

The worktree adds the following to unit 3:
- the no-stop sentence (L12);
- case-insensitive status forms (N-1);
- a case-insensitive repository prefix in the pinned-link rule (N-2);
- the Oxford comma (N-3);
- explicit UTF-8 in `scripts/test-check-release.py` and `scripts/test-fuzz.py`, whose `hygiene` lines (and the `fuzz` recipe's `test-fuzz.py` line) now carry the encoding flags.

| Command | Result |
|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py` | 18 tests OK |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py` | 78 tests OK |
| `... check-docs.py`, `... check-repository.py`, `... test-repository-audit.py` (same flags) | exit 0; exit 0; 52 tests OK |
| `make hygiene` | exit 0, 36 s; test-check-docs 242 tests OK; 7 flagged lines in the Makefile |

Probes ran on `test-check-docs.Tree` copies; the worktree was not modified. B1 to B6 are under L12 above.

| Finding | Probe | Result | Status |
|---|---|---|---|
| N-1 | C1: README "Merged to `main`. Published as `rust-v0.1.0`." with all five published | 0 errors | FIXED |
| N-1 | C2: capital "Published as `rust-v0.1.1`" in all five | 5 errors, "states version 0.1.1" (the version is still exact) | FIXED |
| N-1 | C3: README "Published as", the other four "not yet tagged" | mixed-forms error | FIXED |
| N-1 | C4: "NOT YET TAGGED `rust-v0.1.0`" | 0 errors (accepted, as intended) | FIXED |
| N-2 | D1: `https://github.com/BLACKCAT-informatics/PurRAssist/blob/main/...` in `crates/core/README.md` | `[pinned-links] ... names 'main'` | FIXED |
| N-2 | D2: lowercase owner, pinned to an existing path and anchor | 0 errors | FIXED |
| N-2 | D3: lowercase owner, tag written `RUST-V0.1.0` | `[pinned-links] ... names 'RUST-V0.1.0'` (the tag stays case-sensitive) | FIXED |
| N-3 | E1: "#11, #6, and #10" with the JSON agreeing | 0 errors | FIXED |
| N-3 | E2: "#11, and #6" | 0 errors | FIXED |
| L10 follow-on | the two test scripts under the flags | pass, as shown above | FIXED |

New, LOW and advisory:

**N-4. A document that states both stop forms is not caught.**
- Where: `scripts/check-docs.py:774-777`.
- Problem: the `stop_issues` docstring says a document "that names stops must not claim there is none", but the function returns as soon as it finds the issue form, so the no-stop sentence is never examined.
- Probe B5: with the JSON naming #6 and #11, the FUZZING Known limitations section stating "stops on the #11 and #6 signatures" and then "stops on no known signature" gives 0 errors.
- The reverse case is caught. B4: with the JSON empty, the doc says none and then names #11/#6, which gives an "extra [6, 11]" error, because the issue form is read first.
- Fix: when both forms occur in the text, return an error, or raise in `stop_issues` and report it, and add a fixture.

VERDICT (units 1-3): PASS. No required finding is open: M1 is fixed, M2 and L1-L12 are fixed, and N-1 to N-3 are fixed. N-4 is a LOW advisory item. The final completion verdict waits for G11 (unit 4).

---

## G11 review and final completion verdict (Stage 2)

Reviewed on 2026-10-12.
- Unit 3 is committed as `feefaae`.
- G11 is uncommitted in the worktree (15 Rust sources, `scripts/generate-unicode.py`, `scripts/check-docs.py`, `scripts/test-check-docs.py`, `vendor/unicode/generated.json`), checked against `remediation-plan.md` revision 4.

### G11 review

**Comments only.** Every hunk in the 15 `.rs` files edits a `//`, `///` or `//!` comment or the generated `reason = "..."` string of an `#![allow]` attribute. `cargo fmt --all --check`, `cargo clippy -p purrassist-core -p purrassist-host --all-targets -- -D warnings` and `RUSTDOCFLAGS=-D warnings cargo doc -p purrassist-core -p purrassist-host --no-deps` all pass.

**Accuracy against the code:**
- "idn-hostname format validator" for the `script_of`, `bidi_class`, `joining_type` and `canonical_combining_class` consumers is accurate; all of them are used only in `crates/core/src/schema/format/host.rs`.
- "derivative pattern engine" / "`Pattern::generate`" is accurate (`crates/core/src/pattern/mod.rs:467`, engines `pattern.engine.derivative-v1`/`backtrack-v1`).
- "Pattern-engine wiring" is accurate (the `pattern_ids` and `pattern_evidence` fields carry engine evidence).
- "every structural Markdown operation" (`plan_markdown_structure`) and "the witness-generation surface" are accurate.
- The `xml.rs:1300` rewording replaces the T5/F9 tag with its meaning (P4-3).
- One nit: `crates/core/src/text/mod.rs:251` now says the IDNA table is used by "the idn-hostname format validators". The consumer is the shared `is_hostname(text, idn, ..)` in `schema/format/host.rs`, which also serves `hostname` A-label checks. The surrounding sentence already says "hostname/idn-hostname format validation", so it is not misleading. Optional: name the module `schema::format::host`.

**Attouche stage names kept.** "Stage 1 (canonicalize)", "stage 2 (`not` elimination)" and "stage 5 (generation)" remain in `crates/core/src/schema/algebra/canon.rs:4`, `:186`, `:285` and `:361` and in `generate.rs:4`. The `algebra/mod.rs` reference to "this stage's bounded ... search" is kept.

**Regeneration holds.** I reran it independently, without network access, against the cached inputs, which the generator verifies against `vendor/unicode/manifest.json`.
- `python3 scripts/generate-unicode.py` regenerated the tables: "generated 5914 decompositions, ..., 932 Joining_Type ranges".
- A temporary index (`GIT_INDEX_FILE`, HEAD plus the worktree's `crates/core/src/text` and `vendor/unicode`) was used, and the real index was untouched. `git diff --exit-code crates/core/src/text vendor/unicode` exited 0 after the rerun.
- The sha256 of all five generated or pinned files is identical before and after the rerun.
- `git diff --exit-code HEAD -- crates/core/src/text/unicode_data.rs crates/core/src/pattern/unicode/unicode_data.rs vendor/unicode/manifest.json` is clean. Against HEAD, only `idna_data.rs` (2 lines: the header comment and `reason`) and its digest in `generated.json` change.
- validation.md records the negative control (with "Task 7" restored in the template, the diff is non-empty).

**`source-jargon` rule** (`scripts/check-docs.py`, `check_source_jargon`):
- One `TASK_NUMBER = r'\btask \d+\b'` constant is used both in `BANNED_PHRASES` and by the rule (P4-4), and `test_the_pattern_is_shared_with_the_markdown_ban` pins that.
- Scope is `src/**/*.rs` of the publishable packages from `cargo metadata`. The docstring states it, and states that `tests/`, `benches/` and `examples/` are out (P4-2).
- Fixtures: a failure in a published `src`, a nested path in any case, a pass outside published sources (`tests/`, `benches/`, `helper-census/src`), and ordinary words passing.
- My probes:
  - "Task 3's" in `crates/purrassist/src` is caught.
  - A `"task 4"` string literal in `crates/cli/src` is caught.
  - "TASK 9" in `crates/derive/src` is caught.
  - "Task 9" in `crates/wasm-harness/src` passes, because that crate is unpublished.
- `git grep -iE '\btask [0-9]+'` over the six published `src/` trees: 0 matches.
- Checks with `-X warn_default_encoding -W error::EncodingWarning`: `check-docs.py` exit 0, `check-repository.py` exit 0, `test-check-docs.py` 248 tests OK.

**New LOW items from this review (advisory; none required):**

- **N-5. Dangling "requirement N" numbering.** 19 "Requirement N" references remain in `crates/core/src/schema/**`, several on public items that render on docs.rs. Examples: `schema/mod.rs:44`, `:88`, `:312` and `compile.rs:331`. G11 also rewrote "Task 8 requirement 2" as "(witness-generation algebra, requirement 2)" at `recovery/proposals.rs:76`, `:203`, `:613`, `:683` and `:804`. No public document defines that numbering. The plan scoped G11 to `Task N` on purpose (P4-3: do not widen the rule), so this is outside its scope. Fix: drop the numbers or name the property ("format evidence is always recorded").
- **N-6. Wrapped references are missed.** The rule matches line by line, so a reference wrapped across doc-comment lines ("Task\n/// 8") is not caught (probe H3). Optional: strip the comment markers and collapse whitespace before matching.
- **N-7. The scan roughly doubles the test suite's runtime.** It adds about 0.2 s to every `check()` (0.22 s of a 0.25 s cached check, over 5 MB of `crates/core/src`). `test-check-docs.py` runs it 248 times: 85 s here under load 19, against 29-43 s before G11. Fix: cache the scan per file by (path, size, mtime), or by content digest as `workspace_packages` does.

### Final per-criterion table

| Criterion | Status | Evidence | Relies on the final `make check` / `make package` |
|---|---|---|---|
| M1 (required): release body free of "not yet tagged" | FIXED | `4933cc4`; release-body rule and the A5/N3 layout (`feefaae`); probe A | `make check` (hygiene) |
| M2: status rule accepts the published form | FIXED | `STATUS_FORMS`, mixed forms rejected, RELEASE.md documents it; fixtures | `make check` |
| L1: "Release order" | FIXED | `4933cc4`, plus the release-body ban on "published in this order" | `make check` |
| L2: receipt claim scoped | FIXED | `4933cc4`; `every result carries` banned | `make check` |
| L3: JSON-equality wording | FIXED | `4933cc4`; phrase banned | `make check` |
| L4: asset "for manual upload" | FIXED | `4933cc4`; `uploaded manually` banned | `make check` |
| L5: #7 impact wording | FIXED | `d6fb1e0`; JSON and three copies, equality rule | `make check` |
| L6: process jargon in docs | FIXED | `d6fb1e0` (plus A7); `\btask \d+\b` banned; probe G | `make check` |
| L7: `Pre-release implementation` anchor | FIXED | `feefaae`; SemVer fixture passes | `make check` |
| L8: workflow steps by indentation | FIXED | `feefaae`; probe F | `make check` |
| L9: content-selected binaries | FIXED | `feefaae`; `git ls-files --eol` `i/-text`; `.PNG`/`.webp`/`.ttf` fixtures | `make check` |
| L10: explicit UTF-8 and Makefile flags | FIXED | `feefaae`; six scripts plus the two test scripts; AST test; symlink fallback | `make check` (flagged hygiene lines) |
| L11: crate links pinned to `rust-v0.1.0` | FIXED | `d6fb1e0` plus `check_pinned_links` (A2) | `make check`; `make package` for the packaged READMEs and the cli `documentation` field in the archives |
| L12: stop set from JSON, including none | FIXED | `feefaae`; probes B1-B3 and B6 | `make check` |
| N-1: case-insensitive status forms | FIXED | probes C1-C4 | `make check` |
| N-2: case-insensitive repository prefix | FIXED | probes D1-D3 | `make check` |
| N-3: Oxford comma | FIXED | probes E1-E2 | `make check` |
| N-4: both stop forms rejected | FIXED | `feefaae`; `test_a_document_that_states_both_stop_forms_fails`; re-probe B5 gives `[known-limitations-section] ... both names issues and says "stops on no known signature"` | `make check` |
| G11: no `Task N` in published `src/` | FIXED (uncommitted, reviewed as in the worktree) | comment-only rewording, regeneration reproduces, `source-jargon` rule shares the pattern and has fixtures; grep empty | `make check` (full-workspace clippy, rustdoc `-D warnings`, all tests including derive/syntax/cli); `make package` (changed core and host source bytes in the archives) |
| Original contract rows 1-14 and 17 | PASS | section (f) above, rerun on `a42a817`; nothing in units 1-4 regresses them (focused checks green after each unit) | row 16 below |
| Original contract row 15 (GitHub presentation) | DELIVERED except the social-preview upload (owner-gated, see above) | T5-review.md | no |
| Original contract row 16 (local gate and package) | PENDING | The `a42a817` results are superseded by units 1-4 | yes: the single `make check` and `make package` on the final head |

Open advisory items, none required: N-5, N-6 and N-7, the `text/mod.rs:251` wording nit, and the owner's decision on the social-preview upload.

### Verdict conditions

This verdict covers the G11 worktree content reviewed here and the commits up to `feefaae`. It holds only if:
- G11 is committed with exactly that content;
- the single `make check` and `make package` on the resulting final head both exit 0, and are recorded in validation.md.

If either fails, or the committed content differs, this verdict is void and the affected rows must be rechecked.

VERDICT (G11 worktree review, superseded by the 93c02b3 recheck below): PASS

---

## Recheck at 93c02b3 (G11 plus N-5, N-6 and N-7 committed) and final verdict

Reviewed on 2026-10-12 at `93c02b3`.
- The coordinator is running the final `make check` and then `make package` on that head. I did not run them.
- Every probe here ran in temporary trees or a separate `git archive` export, so the worktree and index were not written while the gate ran.

### Focused checks at 93c02b3

| Command | Result |
|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py` | exit 0 (0.7 s) |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-repository.py` | exit 0 |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py` | 251 tests OK (69 s wall time, while the gate run held load average 20-29) |
| Regeneration in an isolated export: `git archive 93c02b3`, the cached pinned inputs, `python3 scripts/generate-unicode.py`, then `git diff --exit-code -- crates/core/src vendor/unicode` | clean; the generated data at 93c02b3 is exactly what the pinned generator produces |
| `git grep -iE '\b(task\|requirements?) [0-9]+\b'` over the six published `src/` trees | 0 matches |

### Probes

All probes ran on `test-check-docs.Tree` copies.

| Probe | Result |
|---|---|
| P1: "Task" and "8" wrapped across `///` lines (`crates/core/src`) | caught at line 1 (`'Task 8'`) |
| P2: "requirement" and "2" wrapped across `//` lines inside a function (`crates/host/src`) | caught at line 2 |
| P3: "Requirements 3" (plural) in `//!` (`crates/syntax/src`) | caught |
| P4: a code line between two comment lines ("// task", `fn f() {}`, "// 5 apples") | not joined, 0 errors (correct) |
| P5: a trailing `// Task 2` on a code line (`crates/cli/src`) | caught |
| P6: ordinary "requirements" / "required 3" | 0 errors (correct) |
| P7: cache staleness, with the same file rewritten in place at the same size ("Task 1" to "Tusk 1") | 1 error, then 0 (the mtime in the key invalidates it) |
| Scan cost: cold scan vs. a fresh linked tree with a warm cache | 0.259 s vs. 0.005 s |

### Rewording accuracy (N-5)

The 19 "Requirement N" sites now state the property itself:
- `format` annotation evidence is always recorded;
- the unsupported-vocabulary evidence;
- an unrecognized format under an active assertion vocabulary;
- only `date`/`date-time`/`time` admit a format repair, and every other violation stays an explicit no-candidate;
- witness-generation applicator recursion.

I checked each against its surrounding code; none changes meaning. `vocabulary.rs:83` now reads "the dialect treats either boolean as known", which is accurate (the field records whether the URI was declared).

The earlier nit at `text/mod.rs:251` is unchanged. It is cosmetic, because the sentence names hostname/idn-hostname validation.

### Final per-criterion table (supersedes the table above)

| Criterion | Status | Commit | Relies on the final `make check` / `make package` |
|---|---|---|---|
| M1 (required) | FIXED | `4933cc4`, `feefaae` | `make check` |
| M2 | FIXED | `feefaae` | `make check` |
| L1-L4 | FIXED | `4933cc4`, `feefaae` | `make check` |
| L5, L6 | FIXED | `d6fb1e0`, `feefaae` | `make check` |
| L7-L10, L12 | FIXED | `feefaae` | `make check` |
| L11 | FIXED | `d6fb1e0`, `feefaae` | `make check`; `make package` (pinned links and cli `documentation` in the archives) |
| N-1-N-4 | FIXED | `feefaae` | `make check` |
| G11 | FIXED | `93c02b3` | `make check` (full-workspace clippy, rustdoc `-D warnings`, all tests); `make package` (changed core/host source bytes) |
| N-5 | FIXED | `93c02b3`: 19 sites reworded, `REQUIREMENT_NUMBER` in `SOURCE_JARGON`; probes P2, P3 and P6 | `make check` |
| N-6 | FIXED | `93c02b3`: comment runs joined (`COMMENT_LINE`, `source_jargon`); probes P1, P2, P4 and P5 | `make check` |
| N-7 | FIXED | `93c02b3`: per-process cache keyed by (dev, inode, mtime_ns, size); warm scan 5 ms; staleness probe P7 | `make check` (hygiene runtime) |
| Contract rows 1-14 and 17 | PASS | section (f), plus the focused checks after every unit | row 16 |
| Contract row 15 | DELIVERED except the social-preview upload (owner-gated) | T5-review.md | no |
| Contract row 16: local gate and package | PENDING: the gate run on `93c02b3` is in progress | validation.md (to be recorded by the coordinator) | yes |

No findings are open, required or advisory. The only open item is the owner-gated social-preview upload, which the plan defines as an owner decision.

### Verdict conditions

This verdict is for `93c02b3`. It holds only if the single `make check` and then `make package` on `93c02b3` both exit 0 and are recorded in validation.md. If either fails, or the head moves, this verdict is void and the affected rows must be rechecked.

VERDICT: PASS
