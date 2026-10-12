## Issue Analysis: #1

**Title:** Complete PurRAssist 0.1.0 plan and release qualification
**State:** OPEN (kept open deliberately for tag/publish/download acceptance)
**Author:** not recorded in the gathered `issue.md` (all 63 comments are by `paudley`)
**Created:** not recorded in gathered evidence (first comment 2026-09-28T19:06:22Z)
**Labels:** not recorded in gathered evidence
**Base for this stage:** `origin/main` = `f11b45e` (brief `base_oid`), workspace version `0.1.0`

Evidence used: `issue.md` (body + 63 comments), `brief.json`, `prior-art.md`, `open.json`,
and the worktree at `f11b45e`. Nothing was refetched from the forge.

### Summary

Issue #1 is the umbrella for finishing PurRAssist 0.1.0: implementation (R1-R6), release
qualification (R7-R10), and workflow rules (R11-R14). That implementation landed in PR #2,
squash-merged as `699d9ff` on 2026-09-30. The squash message (final comment, 2026-09-30T21:35:17Z)
says that "tagging, registry publication and downloaded-package acceptance remain separately
authorized" and that the issue stays open. Since that merge, main has gained tool-call recovery
(`943fa04`, PR #5 for issue #3; `fccfbe7`, PR #9 for issue #8) and the `purrassist-derive` crate
(`f11b45e`, PR #19 for issue #15). That makes six release crates instead of five.

**Stage 1 scope (decided by the user):** bring the public and release documentation to an
accurate, release-ready state for 0.1.0 with six crates. The docs must reflect what is
implemented now and keep it separate from plans. This stage does **not** tag, publish, verify
downloads, or involve hosted CI. Open issues #6, #7, #10 and #11 do not block 0.1.0, but the
docs must disclose them as known limitations.

---

### Requirement traceability

Legend:
- **(a)** Already satisfied by merged work. Branch commits were squashed into `699d9ff` unless stated otherwise.
- **(b)** In scope now (documentation), with an acceptance check.
- **(c)** Outside this stage by user decision.

| # | Requirement (source) | Class | Evidence / disposition |
|---|---|---|---|
| R1 | Composed JSON recovery + witnesses, regex coverage, recursive-schema/resource audit, explicit unsupported vocabulary/format behaviour (body "Remaining implementation" bullet 1) | (a) | Task 6 `4dfd1b4` (comment 2026-09-29T00:33:10Z), Task 7 `3ed24fc` (01:30:56Z), Task 8 `a45e2c8` (03:56:54Z), Stage 2 G1 `1e94bea`, G2 `d982fb7`, G3 `a3b361e`, G11 `66b3756`. Merged in `699d9ff`. Docs: QUALIFICATION.md:15-18, 56-81. |
| R2 | Markdown section/list/table/reference editing, adversarial preservation, Rust/CLI/MCP access (bullet 2) | (a) | Task 9 `7690c4a`, Task 10 `23caff4`, Task 11 `4f055c5`, `e4bda6e`, G9 `2a7dccb`, G10 `6da7f3b`. The registry now has 31 tools (census in `scripts/data/verify-published/mcp-responses.jsonl`). |
| R3 | Resource accounting across all paths; explicit exhaustion/cancellation (bullet 3) | (a) | Task 3 `48d4e94`, `8e360f3`, Task 5 `65ef908`, `2bd5d02`, G4 `4bbd47b`, G11 `66b3756`. |
| R4 | Alternative-specific ledgers (bullet 4) | (a) | Task 4 `974ccac`, G6 `32c8cf4`, G7 `0de9dc3`. |
| R5 | Oracles, goldens, ambiguity corpora, incorrect-correction evaluation, fuzzing beyond JSON, measurements (bullet 5) | (a), with a doc caveat in (b) | `2e28692`, `2c2c582`, `d88cbce`, `79e5d37`, `53f2366`, `22d3313`. The **post-merge tool-call targets** (`tool-call`, `tool-call-ref`) have no 10,000-mutation baseline: FUZZING.md:974-983 lists 8 rows only. The default `make fuzz` is documented to stop on `tool-call-ref` (#11, FUZZING.md:957-967) and on #6 signatures (FUZZING.md:900-916). The docs must state this; see D-Q2 and D-F2. |
| R6 | Windows durability and crash/retry/stale/undo; native/WASM with fixed transcripts (bullet 6) | (a) | Windows `b53ce9f`…`de8e8fd`, `f8d8cd3`; crash/orphan G8 `9b493b3`, G12 `1af6593`; WASM Task 15 `24787e1`. The wording of the "proved by hosted lane" claim needs care (D-Q4). |
| R7 | Official suites, syntax oracles, properties, bounded fuzzing, portability, full `make check` (release-acceptance bullet 1) | (a) for `699d9ff` source; (b) `make check` as this stage's gate | Final local gates at `34fb2ac` (comment 2026-09-30T21:26:55Z). This stage changes docs only, so the gate is `make check`, which includes hygiene, README doctests via `include_str!` and Rustdoc with `-D warnings`. |
| R8a | Audit release archives and license/source notices (bullet 2, written as "five") | (a) for five crates at `34fb2ac`; (b) re-evidence for six | Five archives audited at `34fb2ac` (same comment, table). The derive crate is now packaged (`534f862`, `b7475b4`) and `check-packages.py:67` treats it with core. Crate README edits change archive bytes, so `make package` (local only, no publication) is the acceptance check for the doc claim "Six Cargo-verified archives" (QUALIFICATION.md:38). |
| R8b | Verify installed CLI/MCP from **downloaded** packages | (c) | Tooling exists and is tested offline (`ed81ffe`, `868d196`; `scripts/verify-published.py` iterates `RELEASE_ORDER`, line 594). The real run needs publication. |
| R9a | Hosted CI on the final commit | (c) | User decision: ignore hosted CI for this work. |
| R9b | Registry records and trusted publishing in dependency order | (a) for 5 crates; (a) this session for the derive name; (c) derive trusted publisher | Five 0.0.0 records (comment 2026-09-28T19:07:06Z). Trusted publishing was verified for those five (2026-09-28T19:24:20Z). The `purrassist-derive` 0.0.0 record was published this session (owner `paudley`, identical to the other five). The owner will configure its trusted publisher. **The docs must say the derive record exists and that its trusted-publisher confirmation is still a manual pre-tag step.** They must not claim it is configured. |
| R9c | Verify publication and downloads, then release | (c) | Task R. Tag `rust-v0.1.0` is not authorized in this stage. |
| R9d | "Do not describe the current qualification-only workflow as a publish workflow" | (a); keep in (b) | RELEASE.md:98-116 and 118-163 keep `release-qualification.yml` and `publish.yml` separate. `scripts/test-check-release.py` validates publish.yml structure. Any rewrite must keep this distinction. |
| R10 | Core has zero third-party runtime deps and no ambient I/O; every dependency registered | (a) | `check-repository.py` `check_core_closure`/`check_derive_closure`/`check_ambient_access` (lines 61, 70, 169) run in `make hygiene`. Docs: DEPENDENCIES.md:9-17 (derive). |
| R11 | Issue branch + reviewable PR | (b) | Current branch `paudley/1-complete-purrassist-0-1-0-plan-and`. The PR should use `Refs #1`, **not** `Closes #1`, because the issue stays open for (c). |
| R12 | Preserve uncommitted main-checkout WIP | (a) | Carried and finished in `2e28692`; main-checkout hashes were preserved throughout (comments 2026-09-30T15:10:34Z and 21:35:17Z). This stage must not touch the main checkout. |
| R13 | Never bypass hooks | (b) | Every commit in this stage. |
| R14 | Merge via owner's `ghprsq` | (b)/(c) | Applies when this stage's PR merges. |
| T19 | Documentation truthfulness (plan Task 19, issue.md:463-470) | (a) at `42b4460`/`b08161e`/`856d2ec`; **(b) now stale** | Accurate at `699d9ff`. Made stale by `943fa04`, `fccfbe7`, `f11b45e` and this session's derive reservation. **This is the core of the stage.** See the stale-document inventory. |
| TR | Task R release ceremony (issue.md:486-498) | (c) | Includes the GitHub release whose notes must link the `verify-published.py` output. A release-notes **draft** may be in scope (D-Q3). |

---

### Stale-document inventory (current main `f11b45e`)

#### docs/RELEASE.md (highest priority; it is the release procedure)
- **L33** heading "Placeholder name records (already minted)" and **L35-41**: "for the original five crates". Now six records exist.
- **L43-62**: "**`purrassist-derive` has no placeholder record yet.** Its name has not been reserved on crates.io … must be completed before any release tag is pushed … Until both are done the release must not be tagged." **Stale.** The record now exists. Only the trusted-publisher configuration and its confirmation remain.
- **L83** heading "Trusted publishing (configured for five of six crates)" and **L85-87** "all except `purrassist-derive`, whose trusted publisher is not yet configured". This is still true as of this session. It must stay unclaimed until the owner confirms, but it should be reworded so the only outstanding item is derive's trusted publisher, not its name.
- **L212-218** "Release step (after merge)": "The `purrassist-derive` name reservation and trusted publisher are not yet done and must exist first". **Stale** for the name. "after merge" and the "exact merge commit" (L212) refer to PR #2's merge, which has happened. The release commit will be a later main commit.
- **L100-104**: the summary of `release-qualification.yml` leaves out steps it actually runs: `make corpus`, `scripts/test-fuzz-feedback.py` and `scripts/test-verify-published.py` (`.github/workflows/release-qualification.yml:42-51`). This is a minor accuracy gap.
- **L183-184**: "e.g. Task 10's registry replacement". This is internal plan jargon in a public doc.
- Correct as is: L10-19 (six-crate order matches `RELEASE_ORDER`, `scripts/check-release.py:19-26`), L21-31 (derive/core ABI rule), L64-81 (preflight), L118-163 (publish.yml six-crate order).

#### README.md
- **L14-17**: "**Pre-release 0.1.0 implementation.** … Publication acceptance follows review and merge; 0.1.0 is not claimed as published." The "follows review and merge" part is stale because the merge happened. The status should say: implemented on main, 0.1.0 not yet published, publication follows an authorized tag.
- **No crate list anywhere.** README never names `purrassist-core`, `-syntax`, `-host`, the `purrassist` facade, or the CLI crate as packages (only `-cli` in `cargo run -p` lines 123-130, and `-derive` in passing at L112). It needs a six-crate table in `RELEASE_ORDER` with roles. That table should note that the two other workspace members (`wasm-harness`, `purrassist-helper-census`) are `publish = false` tooling and that the 0.0.0 crates.io versions are name reservations, not usable releases.
- **L387**: "`make fuzz` # nine coverage-guided targets". **Wrong.** `Makefile:58-62` runs **ten** (`… tool-call tool-call-ref`), and FUZZING.md:7-8 and :875 say ten. The line also implies a passing gate, which #11 contradicts (see R5).
- **L426-427**: "Coverage-guided JSON campaigns are available with `make fuzz TARGET=json`". This is stale JSON-only framing from before Task 12.
- **L113-115** "Still tracked: … (#12) … (#13) … (#14) … (#18) … (#16) … (#17)". The states of these issues are **not in the gathered evidence**. Confirm them before keeping the list. The same list appears in IMPLEMENTATION.md:58-60, QUALIFICATION.md:118-121 and docs/tool-calls/README.md:36-39.
- **L98**: "Journaled single-file replacement and conditional undo". This undersells the shipped `recover_files`/`commit_files` (documented at L305-309). Optional fix.
- Constraint: the README derive example (L23-53) must stay byte-identical to `crates/purrassist/README.md`. `scripts/check-repository.py:254-282` enforces this.

#### docs/IMPLEMENTATION.md
- **L5-7**: "Status: accepted implementation delivered on the issue branch. … registry publication acceptance follows review and merge". **Stale.** It was delivered on main via `699d9ff`, then extended by `943fa04`/`fccfbe7`/`f11b45e`.
- **L87-97**: lists the six published crates correctly, but omits the two unpublished workspace members (`Cargo.toml:6` lists eight).
- **L99-108**: "Required delivery sequence". Stage 5 ("… registry setup, and verified publication") is the only stage still open. The doc does not say which stages are done, so implemented work and plans are not separated.

#### docs/QUALIFICATION.md
- **L33**: "Nine targets (including `tool-call`)". **Wrong.** There are ten (`tool-call-ref` is missing).
- **L168**: "describes the nine mutation targets". **Wrong** (ten).
- **L33/L35/L212**: these lines present `make fuzz` as passing evidence. FUZZING.md:957-967 says the default `make fuzz` exceeds its timeout on `tool-call-ref` (#11), and #6 signatures stop plain tool-call campaigns (FUZZING.md:900-916). The known limitations (#6, #10, #11, and #7 if applicable) must appear here.
- **L143**: "Real Windows execution is proved by the `windows-engines` hosted lane". Hosted CI is out of scope, and `G4`/`G8`/`G12` changed host file code after the last hosted Windows execution recorded in the comments. The wording should describe what the lane covers, not claim it proves the current source (D-Q4).
- **L224-237** "Release acceptance after review and merge": "reserves publication for the reviewed merge commit" is stale. Publication will be from an authorized main commit after `f11b45e`.
- **L240-251** "Stage 2 local integration record": process jargon, and its evidence is bound to `66b3756` (`docs/qualification/resources-stage2-linux-x86_64.json` `/source/commit`). That predates tool-call recovery and derive. It needs an explicit historical label (for example "as of `699d9ff`"), or it reads as current.
- Correct as is: L34 derive evidence row, L38 "Six Cargo-verified archives" (a capability statement; it needs `make package` evidence after README edits).

#### docs/FUZZING.md
- **L877-881**: "Qualification … 10,000 iterations per target … evidence committed under `docs/qualification/`". There is no baseline for `tool-call`/`tool-call-ref` (table L974-983 has 8 rows, and `docs/qualification/` has no `fuzz-tool-call*`). The doc must say which targets have qualification baselines.
- **L967**: "The #8 campaign used `--timeout 120`; see its stage report." This is a dangling reference to non-public `.stage` material.

#### Packaged crate READMEs (these become the crates.io/docs.rs landing text for 0.1.0)
- `crates/core/README.md:46`, `crates/host/README.md:41`, `crates/cli/README.md:86`, `crates/purrassist/README.md:76`: "Pre-release implementation." If tagged unchanged, 0.1.0 ships calling itself pre-release. Use neutral wording that stays true both before and after publication (version + link to the qualification record and its limits).
- `crates/cli/README.md:60-63`: "Tools include …" lists 14 of 31 tools. That is acceptable as "include", but pointing to `tools/list` or the full set is better.
- Core, host, facade and derive READMEs are `include_str!` crate docs (`crates/{core,purrassist,host}/src/lib.rs:5`, `crates/derive/src/lib.rs:481`), so any code-block edit is doctested.

#### Other
- `CONTRIBUTING.md:20`: "`docs/QUALIFICATION.md` separates local checks from remaining release work". No section has that name now (it is "Release acceptance after review and merge"). Keep the wording and the heading consistent.
- `SECURITY.md:5`: "pre-release and has no supported published release yet". **Accurate now.** Leave it, and note it as a post-publication flip.
- No `CHANGELOG*` exists at the root or in any crate. No release-notes source exists for Task R's GitHub release (D-Q3).

---

### Acceptance criteria (Stage 1, documentation)

- [ ] **D-F1 Release-doc fact check (new, executable).** Add a validator wired into `make hygiene`, either `validate_release_docs()` in `scripts/check-release.py` or a new `scripts/check-docs.py`. Add offline negative tests in `scripts/test-check-release.py` (already in hygiene) or a new hygiene-wired test. It derives every fact from the repository, never from hard-coded counts:
  - the `RELEASE_ORDER` sequence appears in order in RELEASE.md's release-set and publish-order sentences and in the README crate table;
  - the README crate table and IMPLEMENTATION "Workspace boundaries" name exactly the publishable set from `cargo metadata --no-deps`, and every `publish = false` member they mention is labeled unpublished;
  - every stated version equals the single workspace version (`0.1.0`);
  - the fuzz-target number words in README, QUALIFICATION and FUZZING equal the number of targets in the `Makefile` `fuzz` loop (10), and FUZZING's target list equals that loop;
  - the dialect number word ("Twelve") equals the count of `docs/tool-calls/dialects/*.md` (12);
  - any stated tool count equals the `tools/list` census in `scripts/data/verify-published/mcp-responses.jsonl` (31);
  - RELEASE.md's placeholder section names all six crates;
  - no packaged crate README contains "Pre-release";
  - no public doc references `.stage` or a "stage report";
  - every issue number that FUZZING.md/RESOURCES.md cite as an open defect (#6, #10, #11) also appears in the release known-limitations text.

  Each rule has at least one failing fixture.
- [ ] **D-F2 Known limitations disclosed.** QUALIFICATION.md (and release notes, if added) states that the default `make fuzz` is expected to stop at `tool-call-ref` under its default timeout (#11), that #6 signatures affect plain tool-call campaigns, the #10 quadratic DeepSeek section growth, and that tool-call targets have no 10,000-mutation baseline. D-F1 enforces this.
- [ ] **D-F3 Stale-phrase regression grep** returns nothing:
  `grep -rnE 'has no placeholder record|five of six|not yet (done|reserved)|delivered on the issue branch|reviewed merge commit|nine (coverage|mutation)' README.md docs/*.md crates/*/README.md CONTRIBUTING.md`
- [ ] **D-F4 Registry fact evidence (read-only network GET; no publication):**
  `python3 scripts/check-release.py --preflight-owners --user-agent 'purrassist-release (https://github.com/Blackcat-Informatics/purrassist)'` exits 0. This shows that all six names exist with identical owners and supports the new RELEASE.md statement about the derive reservation. Record the output in the PR. Trusted-publisher state is not visible to this check and stays a manual statement.
- [ ] **D-F5 Doctests and Rustdoc:** `make check` exits 0. It covers hygiene (including D-F1, `check-repository.py`'s derive-example drift check and SPDX headers on any new file such as `CHANGELOG.md`), doctests of the four `include_str!` READMEs, and `RUSTDOCFLAGS=-D warnings` docs.
- [ ] **D-F6 Packaged artifacts:** `make package` exits 0 and produces six verified archives with the registered notice audit. Crate README edits change archive contents. This is local evidence only and makes no registry claim.
- [ ] **D-F7 Truthfulness:** every status statement separates (i) implemented on main, (ii) locally qualified with its source identity, and (iii) still pending (derive trusted publisher, tag, publish, download verification, GitHub release). No doc claims publication, configured derive trusted publishing, or a hosted-CI result for current main.
- [ ] **D-F8 PR hygiene:** the PR body uses `Refs #1` and lists the remaining category (c) items. It is merged via `ghprsq` with hooks intact.

---

### Dependencies

#### Depends on (all merged)
- #2: feat(1): complete 0.1.0 scope and release qualification (MERGED, `699d9ff`)
- PR #5 / issue #3: tool-call recovery (merged, `943fa04`)
- PR #9 / issue #8: arrays of nested-parameter objects, strict forced choice, marker-free reasoning (merged, `fccfbe7`; #8 CLOSED)
- PR #19 / issue #15: `purrassist-derive` (merged, `f11b45e`; #15 CLOSED)

#### Blocks
- Task R (tag `rust-v0.1.0`, publish, `verify-published.py --version 0.1.0`, GitHub release). Out of this stage.

#### Related (non-blocking by user decision; disclose as known limitations)
- #6: tool-call fuzz rejected-span oracle failures (FUZZING.md:900-956)
- #10: quadratic charged work for restarted DeepSeek-V3.1 sections (RESOURCES.md:939-951)
- #11: slow `tool-call-ref` input; default `make fuzz` timeout (FUZZING.md:957-967, RESOURCES.md:952-958)
- #7: non-blocking per user; its content is not in the gathered evidence
- #12, #13, #14, #16, #17, #18: cited in docs as "Still tracked"; **states unverified** in this evidence set

---

### Context from discussion

#### Key decisions
- Name reservations: 0.0.0 placeholders before the real release (plan Task 1, user-authorized; comment 2026-09-28T19:07:06Z).
- Trusted publisher: repo `Blackcat-Informatics/purrassist`, workflow `publish.yml`, environment `crates-io` (comments 2026-09-28T19:07:06Z and 19:24:20Z). Derive must use the same values.
- Release tooling stays in Python under the Makefile (reviewer suggestion declined; issue.md:128-131).
- Publication only via `publish.yml` on `rust-v*` tags, gated on `release-qualification.yml`. Qualification never publishes (RELEASE.md:98-163).
- The issue stays open until real registry download acceptance; local packages and offline verifier tests are never described as publication (comments 2026-09-30T00:59:56Z, 02:02:42Z, final).
- This session: hosted CI is ignored; #6/#7/#10/#11 do not block 0.1.0; Stage 1 is docs only.

#### Rejected approaches
- `workflow_run` trigger for publish gating (RELEASE.md:133-138): it ties trigger semantics to another workflow.
- A Rust `xtask` for release tooling (issue.md:128-131).
- Std-only Windows durability (issue.md:132-133).

#### Open questions
- **D-Q1:** RELEASE.md:212 and QUALIFICATION.md:227 list "hosted CI on that commit" as a pre-tag step. The user said to ignore hosted CI for this work. Should this stage leave those procedural lines unchanged (recommended: change no CI text except factual result claims), or change the documented release procedure? This is the user's decision.
- **D-Q2:** Should the docs only disclose that `make fuzz` (default) is known to stop on `tool-call-ref`/#6? Or should this stage also run `make fuzz TARGET=tool-call-ref FUZZ_ARGS='--timeout 120'` to commit fresh tool-call evidence? Recommended: disclose only. Fresh campaigns are qualification work, not docs.
- **D-Q3:** Add release notes for 0.1.0? This would be `CHANGELOG.md` at the root or `docs/RELEASE-NOTES-0.1.0.md`, with an SPDX header, an "unreleased" label, no date, and a known-limitations section. Task R's GitHub release needs notes, but none exist.
- **D-Q4:** QUALIFICATION.md:143 Windows claim. Recommended rewording: what the lane executes and which local checks cover Windows code (cross-compiled Clippy and tests compile), without asserting a hosted pass for current main.
- Are #12-#14 and #16-#18 still open? The "Still tracked" lists depend on it, and this analysis could not refetch.

---

### Risk assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Docs claim derive trusted publishing is configured before the owner confirms | MED | HIGH (a misleading release procedure; a tag would fail at auth) | Word it as a manual pre-tag confirmation; D-F7 review; D-F4 only proves names and owners |
| Docs keep presenting `make fuzz` as a passing gate while #11/#6 make it stop | HIGH (current text) | MED | D-F2 disclosure; D-F1 issue-number rule |
| Crate READMEs ship "Pre-release implementation." inside 0.1.0 archives | HIGH if missed | MED (permanent crates.io text; versions are immutable) | D-F1 rule; D-F6 `make package` |
| README edits break the derive-example byte-equality or doctests | MED | LOW (caught by gate) | Edit outside the markers; D-F5 |
| Hard-coded counts drift again (targets, dialects, tools, crates) | HIGH over time | MED | D-F1 derives counts from Makefile, dialect docs, MCP census and `cargo metadata` |
| Historical evidence (Stage 2 reports bound to `66b3756`) is read as covering tool-call/derive | MED | MED | Label historical sections with their source identity; do not rewrite report bytes |
| A new doc validator is too phrase-specific and brittle | MED | LOW | Prefer positive structural assertions (order, counts, set equality); keep the phrase list short and tested |
| Heading renames break inbound links (CONTRIBUTING.md:20, cross-doc anchors) | LOW | LOW | Grep for anchors before renaming; keep CONTRIBUTING consistent |
| The PR accidentally closes #1 | LOW | MED | `Refs #1` (D-F8) |
| "Still tracked" issue numbers are stale | MED | LOW | Confirm states before finalizing (open question) |
| docs.rs `latest` links in derive README resolve to the 0.0.0 placeholder until publication | HIGH (pre-publish) | LOW | Acceptable; it resolves after publication. Optionally pin to `0.1.0` |

---

### Technical notes

#### Affected areas
- `docs/RELEASE.md`, `README.md`, `docs/IMPLEMENTATION.md`, `docs/QUALIFICATION.md`, `docs/FUZZING.md` (L877-881, L967)
- `crates/{core,host,cli,purrassist}/README.md` (status lines; packaged and partly doctested)
- `CONTRIBUTING.md:20`; optional new `CHANGELOG.md` / release notes
- `scripts/check-release.py` or a new `scripts/check-docs.py`, its tests, and the `Makefile` `hygiene` target (L11-15) if a new script is added

#### Potential challenges
- Keep every status sentence true both before and after publication, or mark clearly what changes at publication.
- Derive counts in the validator from authoritative sources:
  - `RELEASE_ORDER` (check-release.py:19)
  - the Makefile fuzz loop (L58-62)
  - `docs/tool-calls/dialects/`
  - the MCP `tools/list` fixture
  - `cargo metadata --no-deps --offline --locked` (as check-release.py:213 does)
- The validator must run offline in `make hygiene`. Only D-F4 touches the network, and it runs once by hand.

### Related PRs
- PR #2: feat(1): complete 0.1.0 scope and release qualification (MERGED, `699d9ff`)
- PR #5: tool-call recovery for issue #3 (merged, `943fa04`)
- PR #9: tool-call recovery follow-ups for issue #8 (merged, `fccfbe7`)
- PR #19: purrassist-derive for issue #15 (merged, `f11b45e`)
