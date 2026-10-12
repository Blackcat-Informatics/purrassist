# Plan Completion Adversary: Stage 1 review

- **Reviewed plan:** `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and/.stage/complete-purrassist-0-1-0-plan-and/plan.md`
- **Pass:** 1 of 3. Pass 2 and pass 3 (the final Stage 1 pass) are appended below; the pass-3 verdict is the final line of this file.
- **Inputs read:**
  - `issue.md` (body, the original plan comment, and the final comments through 2026-09-30T21:35:17Z)
  - `issue-analysis.md`, `prior-art-assessment.md`, `brief.json`, `open.json`
  - The worktree at `f11b45e`
- **Baseline:** none (`.baseline` is absent). There are no ADRs.

## Binding user decisions applied

- Hosted CI is ignored.
- Documents never mention CI-provider account matters.
- #6, #7, #10 and #11 do not block 0.1.0.
- The scope is:
  - release docs, README and other doc updates;
  - GitHub repository settings for the now-public repo;
  - the three-interface framing (Rust crate, MCP, CLI);
  - `social-preview.png`.
- Tagging and publishing are outside this stage.
- The `purrassist-derive` 0.0.0 reservation is already published.

I do not count anything these decisions exclude as a descope.

## Findings

### A1. UNMAPPED REQUIREMENT: the stale "review and merge" release wording has no correcting task

Plan text:

> Line 226 still refers to "reviewed merge commit". (plan:90, stale inventory)

> **Release step:** refers to the current main head rather than "the reviewed merge commit". (plan:238, Task 3)

Task 1's QUALIFICATION corrections (plan:168-173) list ten targets, wire v3, archive labels, Windows, `make fuzz` stops and the Stage 2 relabel. None of them corrects the release-acceptance text. Task 3 names only RELEASE.md's "Release step". The repository still says:

- `docs/QUALIFICATION.md:7-8`: "Publication and downloaded-package acceptance follow the review and merge sequence in RELEASE.md."
- `docs/QUALIFICATION.md:224`: "## Release acceptance after review and merge"
- `docs/QUALIFICATION.md:226`: "reserves publication for the reviewed merge commit"
- `docs/RELEASE.md:5-7`: "Publication acceptance is performed on the reviewed merge commit". This is the intro, not the release step.
- `docs/RELEASE.md:210`: "## Release step (after merge)"

The plan identifies the QUALIFICATION:226 defect and then no task fixes it. No check catches it either, because the row-1 grep omits the phrase.

REWRITE TO (Task 1, QUALIFICATION bullet, add): "Release acceptance: lines 7-8, the heading at 224 and the sentence at 226 refer to 'the exact `main` commit authorized for tagging', not to 'the review and merge sequence' or 'the reviewed merge commit'. The documented gate sequence is otherwise unchanged. `CONTRIBUTING.md`'s repointed reference uses the new heading."

REWRITE TO (Task 3, RELEASE.md, add): "Intro (lines 5-7) and the `Release step (after merge)` heading and sentence: the same replacement."

REWRITE TO (Task 1 checker, add): "`check-docs.py` has a forbidden-phrase rule, matched case-insensitively over `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, `docs/*.md`, `docs/tool-calls/*.md` and `crates/*/README.md`. It covers `reviewed merge commit`, `after review and merge`, `follows? (the )?review and merge`, `issue branch`, `no placeholder record`, `stage report` and `nine (coverage-guided |mutation )?targets`. `test-check-docs.py` reinserts each phrase and asserts the named failure."

### A2. UNFALSIFIABLE CRITERION: the row-1 regression grep and fact checker are vacuous for known stale text

Plan text:

> `git grep -nE 'issue branch\|nine (coverage-guided )?targets\|wire v2\|no placeholder record yet'` on the public docs is empty. (row 117)

> Prose counts are written as words or digits; the checker normalizes "ten"/"10". (plan:158)

> taken from `DialectId::BUILT_IN` source and compared with the README dialect sentence. (plan:160)

Evidence:

- The grep is case-sensitive and covers only "nine targets" and "nine coverage-guided targets". It misses both stale QUALIFICATION occurrences:
  - `docs/QUALIFICATION.md:33`: "Nine targets (including `tool-call`)" (capital N)
  - `docs/QUALIFICATION.md:168`: "the nine mutation targets"

  The criterion is therefore met with both sentences still stale.
- In the raw plan text the alternations are written `\|` (a Markdown table escape). Pasted verbatim into `git grep -E`, they are not reliably alternation.
- The checker does not say which sentences it reads for the fuzz count. "Normalizes" does not locate anything.
- The row claims "Restated doc facts can't drift", but the dialect set is compared only with the README. It is also restated at:
  - `crates/purrassist/README.md:21`
  - `crates/host/README.md:24`
  - `crates/core/README.md:23`
  - `docs/QUALIFICATION.md:27,115`
  - `docs/IMPLEMENTATION.md:43-46`
  - `docs/tool-calls/README.md:14`

REWRITE TO (row 117): "`check-docs.py` keeps an explicit `FACT_SITES` table. Each restated fact has a file and a locator, and a locator that matches nothing fails (it never skips):

- fuzz-target count: the README `make fuzz` comment, the QUALIFICATION capability row and FUZZING link sentence, and the FUZZING smoke paragraph, all compared with `len(fuzz.TARGETS)`;
- dialect count word and id list: every file above, compared with `DialectId::BUILT_IN`;
- receipt version: every `receipt wire v<N>`, compared with `RECEIPT_VERSION`;
- release set and order: RELEASE.md and the README crate map, compared with `RELEASE_ORDER`.

The one-time regression check is run from a code block, not a table cell: `git grep -niE 'issue branch|nine (coverage-guided |mutation )?targets|wire v2|no placeholder record|reviewed merge commit|review and merge' -- README.md CONTRIBUTING.md SECURITY.md 'docs/*.md' 'crates/*/README.md'` prints nothing."

### A3. UNFALSIFIABLE CRITERION: the qualification-honesty corrections are accepted by issue-number presence plus "a reviewer reads"

Plan text:

> `check-docs.py` requires a known-limitations section in QUALIFICATION and FUZZING that names #6, #7, #10 and #11 (each issue number present). A reviewer reads every changed sentence against `docs/qualification/*.json`. (row 118)

A section listing four issue numbers passes even if every claim the row names stays false:

- `docs/QUALIFICATION.md:142-143`: "Real Windows execution is proved by the `windows-engines` hosted lane".
- `docs/FUZZING.md:878-881`: "10,000 iterations per target … the same mutation-count floor for every target". There is no `fuzz-tool-call*-linux-x86_64.json` baseline.
- `docs/QUALIFICATION.md:33,35` present `make fuzz` as passing.
- The QUALIFICATION "Stage 2 local integration record" is unlabeled, although its evidence is bound to `66b3756` (`docs/qualification/resources-stage2-linux-x86_64.json` `/source/commit`).

"A reviewer reads" is not an observation on a named entry point.

REWRITE TO (row 118, acceptance): "`check-docs.py` asserts each of the following, and each has a failing mutation in `test-check-docs.py`:

- (a) The target set of FUZZING's 'Recorded 10,000-mutation baselines' table equals the set of `docs/qualification/fuzz-<t>-linux-x86_64.json` files with `<t>` in `fuzz.TARGETS`. Every `TARGETS` entry without such a file (today `tool-call` and `tool-call-ref`) is named in FUZZING's known-limitations section as having no baseline.
- (b) The README `make fuzz` comment and QUALIFICATION's two `make fuzz` rows each name #11 or link to the known-limitations anchor.
- (c) The phrase `proved by the \`windows-engines\` hosted lane` is absent, and QUALIFICATION's Windows sentence contains a 7-40 hex commit id.
- (d) The headings of QUALIFICATION's 'Stage 2 local integration record' and FUZZING's 'Stage 2 repair qualification' contain the commit read from `resources-stage2-linux-x86_64.json` `/source/commit`.
- (e) The QUALIFICATION and FUZZING known-limitations sections name #6, #7, #10 and #11."

### A4. UNFALSIFIABLE CRITERION: the rules added in Tasks 2-4 have no failing fixture

Plan text:

> `python3 scripts/test-check-docs.py` mutates temporary copies (a wrong count, "nine" targets, a broken relative link, a `.stage/` link, a missing SPDX header, a "Pre-release" crate README) and asserts each one fails with a named error. (row 117)

Rows 119-124 add about a dozen more `check-docs.py` rules:

- the `three-interfaces` marker block;
- the TOOLS.md exactly-one census;
- the crate map;
- the README Rust byte copy;
- the CHANGELOG version, crates, interfaces and limitations;
- the SECURITY table;
- the RELEASE.md order and derive-unreserved wording;
- the PNG IHDR and size;
- the SVG digest.

None of them gets a negative test. Their focused checks only run `test-check-docs.py`, which still contains only the six Task 1 mutations. A rule whose locator never matches passes on every input, so "`check-docs.py` checks X" proves nothing without a failing twin.

REWRITE TO (append to rows 119, 120, 121, 122, 123 and 124): "In the same task, `test-check-docs.py` gains one mutation per new rule, and each must fail with that rule's named error:

- the `three-interfaces` block without its MCP example;
- a TOOLS.md entry deleted, and another duplicated;
- a crate dropped from, or reordered in, the README crate map;
- one byte changed in the README Rust copy;
- the CHANGELOG heading changed to `0.1.1`;
- a known-limitation issue removed;
- the SECURITY table without `0.1`;
- two RELEASE.md crates swapped;
- `has no placeholder record` reinserted;
- a 1280×641 PNG;
- a PNG over 1,000,000 bytes;
- one byte appended to the SVG.

`test-check-docs.py` also asserts that every rule registered in `check-docs.py` has at least one mutation. A rule without one fails."

### A5. UNFALSIFIABLE CRITERION: the "three equal interfaces" evidence is one sample, may be skipped, and does not test the operation shown

Plan text:

> The CLI example commands in the README and TOOLS.md run: `cargo run -p purrassist-cli -- call match_strings --root . < examples/match-strings.json` exits 0, and so does one MCP `tools/list` round trip through `purrassist mcp` (scripted in `test-check-docs.py` against the built binary). (row 119)

> One operation (temperature decode) shown as: Rust … MCP … CLI (plan:190-193)

> For each of the 31 tools: MCP name, `purrassist call` form, Rust entry point (crate path), arguments, outcomes and an example file. (plan:203)

Evidence:

- **(a) The MCP round trip may never run.** `make hygiene` (Makefile:11-18) runs only Python, before `test`, and nothing in it builds `purrassist`. "Against the built binary" leaves open whether the script builds the binary or skips, and a skip passes vacuously.
- **(b) One sample stands in for every example.** The claim covers every CLI example in README and TOOLS.md. Only 12 `examples/*.json` exist for 31 tools, so the new per-tool example files would never be executed.
- **(c) The operation shown is never exercised over MCP.** The README block's MCP form is a `decode_json` call, but the round trip is `tools/list`. Nothing shows that Rust, CLI and MCP return the same value for the operation the README shows.
- **(d) The census source is not proved fresh.** It is the committed `scripts/data/verify-published/mcp-responses.jsonl`. Its freshness test, `scripts/test-verify-published.py` `CommittedFixtureFreshnessTests`, is not in `make check`; it runs only in `.github/workflows/verify.yml:29` and `release-qualification.yml:49`. This stage's evidence therefore never shows the census is current.
- **(e) Rust entry points are checked for presence only.** "Names its Rust entry point" can be met by any text.
- **(f) Crate README roles are mostly unchecked.** Row 120 claims the crate READMEs "state the three-interface roles", but checks only two Cargo descriptions.

REWRITE TO (row 119, acceptance):

"`test-check-docs.py` builds the CLI itself (`cargo build --locked -q -p purrassist-cli`). A failed build or missing binary is a test failure, never a skip. Against that binary it:

1. Sends `initialize` and the README `three-interfaces` block's MCP `tools/call decode_json` request through `purrassist mcp --root <tmp>`. It asserts that the result value is `{"temperature":42}` and equals the `value` printed by the block's CLI command, `purrassist decode --schema examples/temperature.schema.json --input examples/temperature.input.txt`. The Rust form is the doctested facade-README copy asserting the same value.
2. Takes the live `tools/list` names and asserts that they equal both the committed fixture census and the TOOLS.md entry set.
3. Runs every `examples/*.json` that TOOLS.md references through `purrassist call <tool> --root <seeded tmp>` and asserts exit 0 with no tool error. Where no runnable example exists, the TOOLS.md entry points to its `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` fixture (`crates/cli/tests/protocol.rs`) instead of claiming an example file.

`check-docs.py` also asserts:

- the final identifier of each TOOLS.md Rust entry point exists as a `pub` item (`fn`, `struct`, `enum`, `trait`, `mod`, `type` or `const`) in the named crate's `src/`;
- each of the six packaged READMEs contains a role sentence naming the interface(s) it serves.

Task 2's focused check adds `python3 scripts/test-verify-published.py`. The README cites the CLI≡MCP test by name: `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp`."

### A6. UNFALSIFIABLE CRITERION: the README-to-TOOLS.md move is accepted by "the reviewer diffs claims"

Plan text:

> the per-tool reference moves out of the README with no claim dropped (the reviewer diffs claims). (plan:203)

About 240 lines of dense contract text (README.md:133-375 at `f11b45e`) move. "The reviewer diffs claims" can be declared satisfied whatever was lost.

REWRITE TO: "Before the move, Task 2 records every backticked token and relative link in README.md:133-375 at `f11b45e` in `$STAGE/tasks/T2-claims-before.txt`. After the move, a script prints every recorded token present in neither `README.md` nor `docs/TOOLS.md`. The printed set is empty, or each remaining token is justified in `$STAGE/tasks/T2-claims.md` as a deliberate factual correction. The output is recorded there."

### A7. UNFALSIFIABLE CRITERION: the known-limitations issue set is hard-coded, with unverified issue states and an unread #7

Plan text:

> its Known limitations names #6, #7, #10, #11, #12, #13, #14, #16, #17 and #18. (row 122)

> its known-limitation issue set is complete. (plan:257)

The plan's own rule is "facts are taken from their sources, never hard-coded" (plan:155). The issue analysis records:

- `issue-analysis.md:156`: "#12, #13, #14, #16, #17, #18: cited in docs as "Still tracked"; **states unverified**"
- `issue-analysis.md:155`: "#7: … its content is not in the gathered evidence"

"Complete" has no reference set. If any listed issue is closed, the CHANGELOG ships a fixed issue as a 0.1.0 limitation and the check still passes. The "one-line impact" for #7 cannot be written from the gathered evidence.

REWRITE TO (Task 3, first step, and row 122): "Task 3 first runs `gh issue view <n> --repo Blackcat-Informatics/purrassist --json number,state,title,body` for 6, 7, 10, 11, 12, 13, 14, 16, 17 and 18, and records the output in `$STAGE/tasks/T3-issue-states.md`. It then:

- removes any closed issue from every 'Still tracked' list (README, IMPLEMENTATION, QUALIFICATION, `docs/tool-calls/README.md`) and from the CHANGELOG;
- writes #7's one-line impact from its issue body.

`check-docs.py` derives the expected set as the union of issue numbers in the 'Still tracked' sentences and in the QUALIFICATION and FUZZING known-limitations sections. It asserts that the CHANGELOG `Known limitations` issue set equals that union; a missing or extra number fails."

### A8. PLANNED-BY-REFUSAL / PRE-EMPTIVE DESCOPE: the social-preview upload may be "delivered" as a handoff note

Plan text:

> The social preview is uploaded through the Settings UI, verified by the `og:image` of the public repo page no longer being the default `opengraph.githubassets.com` card; if browser upload isn't possible, this is an explicit owner step in the handoff. (row 125)

> Otherwise hand it to the owner as an explicit step with the exact file path. (plan:296)

The user asked: "Add a `social-preview.png`" (plan:18). Repository settings for the public repo are in scope. No user decision authorizes turning the upload into an owner step. As written, the contract row passes when nothing is uploaded.

REWRITE TO (row 125, social-preview clause): "Acceptance: the `og:image` meta content of `https://github.com/Blackcat-Informatics/purrassist` is a `repository-images.githubusercontent.com` URL, not `opengraph.githubassets.com`. The URL is recorded in `$STAGE/tasks/T5-repo-settings.md`. If no authenticated browser session is available, the row is reported **NOT DELIVERED**, and before Task 6's PR handoff the implementer asks the user to choose:

- (a) the user uploads `docs/social-preview.png` through Settings → Social preview, after which the implementer runs and records the `og:image` check; or
- (b) the user explicitly descopes the upload from this stage, and that decision is quoted in the handoff.

A handoff note alone does not satisfy this row."

## Rows and decisions checked and cleared

- **Row 121, release docs and derive reservation:** cleared, subject to A1 and A4. The `check-release.py --preflight-owners --user-agent …` argument form matches `scripts/check-release.py:211`, and it is a real read-only observation.
- **Row 123, SECURITY.md:** cleared, subject to A4.
- **Rows 126 and 127, gate, package and PR:** cleared. `make check` and `make package` run once on the final head. The PR body uses `Refs #1`, which keeps #1 open.
- **Out of stage:** hosted CI, the derive trusted publisher, and tag, publish, download verification and the GitHub release. Each matches a quoted user decision.
- **Declined enhancements:** none hides requested work.
  - The combined-LICENSE file is not a requested deliverable, and the display limitation is disclosed.
  - Fresh tool-call campaigns are consistent with the "#6/#10/#11 do not block" decision.
  - The image generator choice is a design matter.
- **No CI-provider account wording:** none exists in the repository today. I propose no checker text naming such terms, so the review itself complies with the user decision.

## Note outside this reviewer's grounds (not counted)

`Homepage: https://docs.rs/purrassist` (plan:292) resolves to the 0.0.0 "Reserved name." placeholder until publication. This is for the design and constraints reviewers.

## Required user decision

None at pass 1. A8's choice between (a) owner upload and (b) explicit descope may be settled by the user now, or when Task 5 finds no browser session.

Reviewer protocol status: FINDINGS-OPEN (pass 1 of 3; 8 open findings A1-A8).

Pass 1 verdict: BLOCKED (superseded by pass 2 below)

---

# Pass 2 (plan revision 2)

- **Reviewed plan:** the same path, now "Revision 2" (plan:3), with dispositions at plan:347-388.
- **Pass:** 2 of 3.
- **Scope of this recheck:**
  - the eight pass-1 findings;
  - the text introduced to fix them;
  - unchanged text cleared at pass 1 is not reopened.
- **Line numbers:** `plan:N` refers to revision 2; repository line numbers are at `f11b45e`.

## Adjudication of pass-1 findings

### A1. DISCHARGED

The stale release wording is now mapped to a task:

- The inventory lists RELEASE.md "Lines 5–7: 'reviewed merge commit' intro" (plan:76) and QUALIFICATION.md "Lines 7–8, 224 and 226" (plan:99).
- Task 1's files bullet corrects "the stale facts … listed in the inventory above" (plan:190).
- Task 3 binds "The intro and release step … to 'the exact `main` commit to be tagged'" (plan:265).
- The banned-phrase rule (plan:143) adds `reviewed merge commit`, `follows review and merge` and `after review and merge`.

Whether that rule actually detects these sites is a separate defect in the new text, raised as N1.

### A2. DISCHARGED

- The one-time grep is replaced by a case-insensitive banned-phrase rule in `make check`. That rule includes `nine (mutation |coverage-guided )?targets` (plan:143), which matches `QUALIFICATION.md:33` "Nine targets" and `:168` "nine mutation targets". Both sit on single lines.
- Every fact regex now "fails on zero matches", and each rule has a fixture in which the sentence it reads is missing (plan:143).
- Dialect ids come from one source in "every file that lists them", and a Rust test asserts `DialectId::BUILT_IN` against the `docs/tool-calls/dialects/*.md` stems (plan:144).

### A3. STILL-OPEN, on sub-item (b) only

Revision 2 adopts sub-items (a), (c), (d) and (e) (plan:145):

- one baseline row per `TARGETS` entry, with `none` where there is no baseline;
- the Windows sentence cites its run id;
- archived-evidence paragraphs name the `source.commit` that the checker reads from each report;
- a `## Known limitations` heading whose issue set equals `docs/known-limitations.json`.

The disposition says "A1–A8: adopted as written" (plan:388). Sub-item (b) is nevertheless absent. No rule ties QUALIFICATION's `make fuzz` evidence rows to the known limitations. Today `docs/QUALIFICATION.md:35` reads:

> | Tool-call fuzzing | `make fuzz` (`TARGET=tool-call` alone) | … no panic, budget respected, mutating-guess guards and span invariants hold …

Issue #6 is the rejected-span oracle failure on that same target (`docs/FUZZING.md:902-903`). The inventory names the defect: "`make fuzz` is presented as passing, although issues #11 and #6 make it stop" (plan:99-103). However, every row-145 check passes whether or not the table rows change, because a separate `## Known limitations` section satisfies all of them. The capability table, the part a reader actually uses as evidence, can keep claiming "span invariants hold".

REWRITE TO (append to row 145): "`check-docs.py` asserts that every QUALIFICATION capability-table row whose evidence cell contains `make fuzz` (today 'Coverage-guided/differential probes' and 'Tool-call fuzzing') links to `#known-limitations` in its scope cell. `test-check-docs.py` has a fixture with that link removed from one row, and it must fail with the rule's named error."

### A4. DISCHARGED

- Row 143 now requires "for every rule, a fixture copy that violates it and must fail with that rule's named error, plus a fixture where the sentence the rule reads is absent".
- Rows 145 and 155, and Tasks 2-4 (plan:251, 285 and 309), each require failing fixtures for the rules they add.
- The census and examples rules moved into Rust tests, which fail on their own.

### A5. DISCHARGED

- **(a) Round trip may never run:** resolved. Execution moved into `crates/cli/tests/protocol.rs`, which runs under `make check`'s `test` target; the binary is built by Cargo, not assumed.
- **(b) One sample for every example:** resolved. `examples_run_through_cli_and_mcp` requires every `examples/*.json` to be listed, with no orphans, and runs each through `run_cli` and `run_mcp` (plan:147).
- **(c) Operation shown never run over MCP:** resolved. The temperature request runs through `decode_json` in both transports. The identity claim between `decode` and `decode_json` is withdrawn with a reason (plan:174-176), not left untested.
- **(d) Census freshness:** resolved. The census now comes from the live registry `Tools::definitions()` in `tools_reference_matches_registry` (plan:146), not from the committed fixture.
- **(e) Presence-only Rust entry points:** resolved. The Rust entry-point column is dropped (plan:375), so the unverifiable claim no longer exists. The Rust interface keeps its doctested copy in the README block (plan:148).
- **(f) Crate README roles:** the per-README role sentence (plan:237-242) is still checked only by review. It is an omission risk, not a false claim, so I do not hold the finding open for it.

### A6. DISCHARGED

Row 149 adds a migration test. It reads the pre-move README with `git show f11b45e:README.md` and requires every backticked token and link target to appear in the new README ∪ `docs/TOOLS.md`.

### A7. DISCHARGED

- `docs/known-limitations.json` is the single source (number, title, one-line impact, `blocks_release: false`).
- Its issue states were observed on 2026-10-12, all open (plan:187).
- QUALIFICATION, FUZZING and CHANGELOG sets must equal it (plan:145 and 153).

### A8. DISCHARGED

Row 156 now reads: "If the browser upload is impossible, the row is **NOT DELIVERED** and goes to the owner as an explicit decision (upload it themselves, or drop it from this stage); it is not counted as done."

## New finding on text introduced by the fixes

### N1. UNFALSIFIABLE CRITERION: the banned-phrase rule, as written, misses four of the stale sites it was added to catch

Plan text (row 143):

> a case-insensitive banned-phrase list covering `issue branch`, `nine (mutation \|coverage-guided )?targets`, `wire v2`, `reviewed merge commit`, `follows review and merge`, `after review and merge`, `no placeholder record`, `not yet done`, `five of six`, `Pre-release`, `stage report`, `proved by the hosted`.

The plan does not say how matching handles Markdown line wraps, and two of the phrases do not match the repository's actual wording. Checked against `f11b45e`:

| Site | Actual text | Why the listed phrase misses it |
|---|---|---|
| `docs/QUALIFICATION.md:226-227` | "the reviewed merge⏎commit" | wrapped across a newline |
| `docs/RELEASE.md:6-7` | "on the reviewed⏎merge commit" | wrapped across a newline |
| `docs/QUALIFICATION.md:7-8` | "follow the review⏎and merge sequence" | wrapped, and "follow the" is not `follows` |
| `docs/QUALIFICATION.md:142-143` | "is proved by the⏎`windows-engines` hosted lane" | wrapped, and a word sits between "the" and "hosted" |

With per-line matching, or with the phrases exactly as listed, `check-docs.py` exits 0 even if all four sentences stay unchanged. Row 143 says "stale facts corrected", and its executable acceptance would then be met while those facts remain stale.

The fixtures do not close the gap. A fixture written as a single-line copy of each phrase passes the test without proving that real wrapped text is caught. The Windows run-id rule (row 145) covers the 142-143 claim separately, but the banned phrase gives false assurance for the other three sites.

REWRITE TO (row 143, banned-phrase clause):

"Banned phrases are matched against each file's text after every run of whitespace, including newlines and wrap indentation, is collapsed to one space. The list uses `follows? (the )?review and merge` in place of `follows review and merge`, and `proved by the [^.]{0,40}hosted` in place of `proved by the hosted`.

`test-check-docs.py` adds two tests:

- **(i) Wrap fixture:** a fixture whose banned phrase is split across a line break must fail.
- **(ii) Baseline test:** it runs the rule on `git show f11b45e:<file>` for `README.md`, `docs/QUALIFICATION.md`, `docs/RELEASE.md`, `docs/IMPLEMENTATION.md`, `docs/FUZZING.md`, `SECURITY.md` and the four packaged READMEs that say 'Pre-release'. It asserts at least one hit at each of these sites:
  - `QUALIFICATION.md` 7-8, 23, 33, 142-143, 168, 224 and 226-227;
  - `RELEASE.md` 6-7, 43, 83 and 218;
  - `README.md` 14, 17 and 387;
  - `IMPLEMENTATION.md` 5 and 7;
  - `FUZZING.md` 967;
  - `SECURITY.md` 5;
  - `crates/{core,host,cli,purrassist}/README.md` 'Pre-release'."

## Notes outside this reviewer's grounds (not counted)

- **The `purrassist validate` identity claim would be dropped.**
  - Row 148 says the convenience commands "`decode`, `match`, `validate` … no identity is claimed for them".
  - `README.md:149-150` currently says "`purrassist validate` command emits exactly this tool's output", and `crates/cli/src/commands.rs:90-97` implements exactly that by calling `purrassist_host::tools::validate_json`.
  - Dropping the sentence would lose a true claim. That conflicts with row 149's title ("No README claim is lost"), and the token-based migration test would not notice.
  - Choose one: keep the sentence and add an equality test, or record its removal as deliberate. This is for the design and completeness reviewers.
- **The migration test can fail without the base commit.** Row 149 makes the test read `f11b45e` permanently inside `make check`, so any clone that lacks that commit fails the gate. This is a design consideration.
- **The homepage note from pass 1 is resolved** (plan:324).

## Required user decision

None at pass 2. The next pass is 3, the last Stage 1 pass. Anything still open after it goes to the user as `ESCALATE-TO-HUMAN`, stated as concrete choices.

Reviewer protocol status: FINDINGS-OPEN (pass 2 of 3). Open: A3 (sub-item b) and N1. Discharged: A1, A2, A4, A5, A6, A7 and A8.

Pass 2 verdict: BLOCKED (superseded by pass 3 below)

---

# Pass 3 (final Stage 1 pass)

- **Reviewed plan:** the same path, re-read from disk.
  - Its header still reads "Revision 2" (plan:3). It nevertheless contains the "Adversary, pass 2" dispositions (plan:366-371) and the pass-2 changes adjudicated below.
  - This pass reviews the text on disk.
- **Pass:** 3 of 3. This is the last Stage 1 pass.
- **Scope of this recheck:**
  - A3(b) and N1;
  - the two pass-2 notes;
  - the text introduced to resolve them.
  - Unchanged text that earlier passes cleared is not reopened.
- **Line numbers:** `plan:N` refers to the current on-disk text; repository line numbers are at `f11b45e`.

## Adjudication of open findings

### A3. DISCHARGED

Row 145 now adds:

> Every row of QUALIFICATION's `make fuzz` results table links to `#known-limitations`; a fixture with the link removed fails. The rows for the targets affected by #6 and #11 state the stop instead of "invariants hold". (plan:145)

This is the rule I requested, and it has a failing fixture. Row 143's "every fact regex fails on zero matches" stops it from passing vacuously if it finds no `make fuzz` rows. The "span invariants hold" wording in the `Tool-call fuzzing` row (`docs/QUALIFICATION.md:35`) must also be replaced by a statement of the stop. A reader of the table is therefore sent to the known-limitations section.

### N1. DISCHARGED

Row 143 now:

- matches the banned phrases "after collapsing all whitespace runs (including newlines) to one space, with a fixture where a phrase wraps across lines";
- uses `follows? (the )?review and merge` and `proved by the [^.]*hosted`;
- adds a one-time baseline check over the `f11b45e` versions of README, RELEASE, QUALIFICATION, IMPLEMENTATION and FUZZING. That check must flag "every stale site in the inventory, including the wrapped sites RELEASE 6–7, QUALIFICATION 7–8, 142–143 and 226–227" (plan:143).

I checked the four sites against `f11b45e` after whitespace collapse:

| Site | Collapsed text | Matching phrase |
|---|---|---|
| `QUALIFICATION.md:226-227` | "the reviewed merge commit" | `reviewed merge commit` |
| `RELEASE.md:6-7` | "the reviewed merge commit" | `reviewed merge commit` |
| `QUALIFICATION.md:7-8` | "follow the review and merge sequence" | `follows? (the )?review and merge` |
| `QUALIFICATION.md:142-143` | "proved by the `windows-engines` hosted lane" | `proved by the [^.]*hosted` (no period in between) |

All four now match.

The baseline check is a one-time Task 1 focused check recorded in `validation.md`, not the permanent test I proposed. That is enough for this stage's acceptance: it is a recorded observation on real input. The permanent wrap fixture keeps the matching mode under test in `make check`.

The baseline does not cover `SECURITY.md` or the crate READMEs. Their stale phrases ("pre-release" at `SECURITY.md:5`, and "Pre-release" in the core, host, cli and purrassist READMEs) are each on a single line. They are covered by the permanent case-insensitive rule and its per-phrase fixtures.

### Pass-2 notes resolved

- **`validate` claim:** the true claim is kept and backed by a new test, `validate_command_prints_validate_json_output` in `crates/cli/tests/protocol.rs` (plan:148 and 238).
- **Migration check:** it is now a one-time Task 2 focused check. It runs `git show f11b45e:README.md`, has no `git` in `make check`, and its command and output are recorded in `validation.md` and the Task 2 review (plan:149 and 257).
  - A6 stays DISCHARGED. My pass-1 A6 rewrite was itself a one-time recorded script.

### Earlier discharges

A1, A2, A4, A5, A6, A7 and A8 remain DISCHARGED. Their supporting text is still present:

- rows 143-149;
- row 153, the exact-commit wording;
- row 154, CHANGELOG equal to `docs/known-limitations.json`;
- row 157, the NOT DELIVERED rule for the social-preview upload.

## New text since pass 2: checked, no findings

- **Row 151, the `mcp-name:` token in the cli README:** the acceptance is falsifiable. The token must appear outside any HTML comment or code fence in the packaged README, with failing fixtures for a missing token and a commented-out token.
- **Task 3, SECURITY.md (plan:288):** reporting is enabled before SECURITY.md names it, with a read-back. The fallback ("names the email only") would make row 155's two-channel check fail. That is a hard, reported failure, not a silent pass.
- **Task 4 (plan:321):** the re-render sha256 comparison blocks the commit, and the outcome is observable.
- **Row 147:** the supporting inputs (`temperature.schema.json`, `temperature.input.txt`) are listed explicitly, so the no-orphan assertion stays well defined.

## Note outside this reviewer's grounds (not counted)

Row 151 is titled "MCP Registry eligibility". Its acceptance proves only that the token is present. `server.json` and registry submission remain post-publication owner steps (plan:172-175).

- The handoff should report the delivered item as "registry name token present in the cli README", not as eligible or listed.
- The namespace string is permanent once 0.1.0 ships, so the enhancement and design reviewers should confirm it.

## Required user decision

None. No finding remains open, so there is nothing to escalate.

Reviewer protocol status: PASS (pass 3 of 3). Pass-1 findings A1-A8 and pass-2 finding N1 are all discharged.

VERDICT: PASS
