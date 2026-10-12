# Plan constraints review

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and/.stage/complete-purrassist-0-1-0-plan-and/plan.md`

## Governing documents

Read:

- `.goals`
- `AGENTS.md`
- `CONTRIBUTING.md`
- `docs/DEPENDENCIES.md`
- `docs/RELEASE.md`
- `vendor/tooling.json`
- `scripts/check-repository.py`
- `Makefile`
- `.gitattributes`
- `.gitignore`
- Global user rule `no-attribution.md`

Recorded absent: `.baseline` (the plan says the same at line 22), `CONSTITUTION.md`, `CLAUDE.md`, `PRECOMMIT_GUIDANCE.md`, ADR directories, nested `AGENTS.md`/`CLAUDE.md`, `REUSE.toml`/`.reuse/`.

Binding user decisions applied, as quoted by the caller:

- Ignore hosted CI.
- Never mention CI-provider account or payment matters anywhere.
- Issues #6, #7, #10 and #11 do not block 0.1.0.
- Scope: release docs, README, doc updates, GitHub repo settings (the repo is public), three-way interface framing (Rust crate, MCP, CLI) and `social-preview.png`.
- Tagging and publishing are out of this stage.
- No AI or agent attribution anywhere.

## BLOCKING

### B1. The plan narrows the "never mention CI-provider account/payment matters anywhere" decision, then publishes text that mentions them

Governing text: the user decision (caller): "never mention CI-provider account/payment matters anywhere".

Offending plan lines:

- plan.md:12: "Ignore hosted CI for this work. Documents must not mention CI-provider account matters."
- plan.md:178: "**Wording rule:** no CI-provider account wording anywhere."
- plan.md:306 (Task 6, step 5): "Post the plan to the issue and the PR."

There are two defects:

1. **The recorded decision is narrower than the real one.** Line 12 limits the decision to "Documents" and drops "payment". The user said "anywhere". That covers issue comments, PR title and body, commit messages, the T5 settings record, the handoff and the least-confident note, not only repository documents. An implementer working from line 12 could put the subject in an issue update or the handoff and still believe they comply.
2. **The plan breaks the decision itself.** Task 6 posts this plan verbatim to the public issue and PR. Lines 12 and 178 would tell readers that CI-provider account matters exist and are being kept out. That is a mention.

REWRITE TO:

- plan.md:12: "Hosted CI is out of scope for this stage." Keep the full decision only in an unpublished stage note, not in any text that will be posted. Its scope covers every artifact and every outward message: docs, commits, issue and PR comments, PR body, settings record and handoff.
- plan.md:178: delete the line from the plan. If the implementer needs a reminder, put it in the unpublished stage note.
- Task 6, step 5: "Post the plan to the issue and the PR **after removing any internal-only constraint notes**; the posted text contains no reference to CI-provider account or payment matters."
- Do not add an executable denylist of such words to `scripts/check-docs.py` or `scripts/test-check-docs.py`. The committed test strings would themselves mention the subject.

## ADVISORY

### A1. The `rsvg-convert` tooling entry must vendor a license notice with its digest

Governing text:

- `scripts/check-repository.py:374-377` reads `entry['notices']` for every `vendor/tooling.json` entry and hashes each file. An entry without `notices`, or one with a missing file, fails `make hygiene`.
- `docs/DEPENDENCIES.md:101-102`: "Tool and runtime license notices are registered in `vendor/tooling.json`."
- `CONTRIBUTING.md:14-15`: "Any external dependency, generated dataset, test tool, or vendored source needs a recorded purpose, exact identity, license, and provenance."

Plan line (Task 4): "`rsvg-convert` is registered with its version in `vendor/tooling.json`, following the existing entry schema, and justified in `docs/DEPENDENCIES.md`."

REWRITE TO: "Register `librsvg`/`rsvg-convert` in `vendor/tooling.json` with:

- `name`.
- The exact installed `version`.
- `license` (librsvg's SPDX expression, verified from upstream).
- `source` (the upstream license URL at that version tag).
- `scope`: authoring-time render of `docs/social-preview.svg` only; not invoked by `make check`; not linked or packaged.
- `notices`: the upstream license text vendored under `vendor/licenses/librsvg-<version>/`, with its sha256.

Add one paragraph to `docs/DEPENDENCIES.md` stating the same."

Note: `docs/DEPENDENCIES.md:279` ("No dependency may be added without explicit maintainer approval") falls under the Cargo inventory section, so it does not strictly cover an external authoring tool. The user asked for the PNG but did not name the renderer, so the handoff should still list the new tool registration.

### A2. The repository has no existing convention for registering the PNG's license or the SVG-to-PNG pair

Governing text:

- `CONTRIBUTING.md:14-15` (quoted above): the generated asset needs a recorded purpose, identity, license and provenance.
- `.gitattributes:11` contains only `*.png binary`. It holds no license data.
- The repository has no REUSE files.
- The generated-data registries that `scripts/check-repository.py` checks (`vendor/unicode/generated.json` at :329-332 and `vendor/entities/generated.json` at :338-340) are scoped to Unicode and entity tables. Each maps a name to a sha256 and nothing more.

Plan lines (Task 4):

- "The pair goes in the repository's generated or binary asset registry (where `check-repository.py` already registers generated data; the implementer finds it)".
- "`.gitattributes` or REUSE-style registration as the repo already does for binaries; the implementer checks".

These premises are false. Neither mechanism exists. Putting the pair into a Unicode or entity registry would misfile it.

REWRITE TO: "Create a dedicated registry `docs/assets.json` with one entry for `docs/social-preview.png`. The entry records:

- `source: docs/social-preview.svg`
- `source_sha256`
- `render_command`
- `tool: rsvg-convert@<version>`
- `license: MIT OR Apache-2.0 OR MulanPSL-2.0`
- `provenance`: derived from `docs/purrassist-logo.svg`

`scripts/check-docs.py` verifies the SVG digest and the PNG dimensions and size. `.gitattributes` is unchanged."

### A3. The CLI-equals-MCP claim depends on a test the plan has not confirmed exists

Governing text:

- `AGENTS.md`: "Capability claims require executable evidence."
- `CONTRIBUTING.md:19-20`: "Capability or performance claims need executable evidence".

Plan line (Task 2, item 3): "both return identical output (cite the CLI≡MCP test that exists in the host and cli tests; the implementer locates and names it)."

REWRITE TO: "Before writing the sentence, name the test that asserts identical CLI `call` and MCP `tools/call` output. If no test asserts byte-identical output, either add one to `crates/cli/tests` in this task, or narrow the README sentence to what the existing tests prove (a shared registry)."

### A4. Open-ended rendering choices conflict with "LOW/NO OPTIONALITY, HARD FAILS" and the reproducibility goal

Governing text: `.goals:6`: "LOW/NO OPTIONALITY, HARD FAILS - fail fast and early, no optional components ... READ CAREFULLY - DOES NOT APPLY TO CODE." That makes it binding on non-code artifacts such as this asset.

Plan line (Task 4): "Text is converted to paths, or uses a generic font family with a recorded fallback ... The implementer picks the option".

A generic font family makes the PNG depend on whatever fonts are installed.

REWRITE TO: "Text is converted to paths in the SVG, so rendering does not depend on fonts. No font fallback is used."

### A5. "Current main head" weakens the exact-commit binding in RELEASE.md

Governing text:

- `docs/RELEASE.md:124-125`: the gate "blocks on a successful `release-qualification.yml` run for the exact commit the tag points at".
- `docs/RELEASE.md:212`: "for the exact merge commit".

The plan says procedure changes are out of scope (plan line 238).

Plan line (Task 3): "**Release step:** refers to the current main head rather than 'the reviewed merge commit'."

"Current main head" is a moving target. It reads as a procedure change.

REWRITE TO: "**Release step:** refers to 'the exact `main` commit to be tagged' in place of 'the reviewed merge commit'. Every exact-commit requirement in RELEASE.md stays as it is."

This keeps the hosted-CI line as written: the user decision covers this stage's work, not the separately authorized tagging step.

### A6. `.stage/` is not ignored, and its contents must not reach a commit or a public doc

Governing text:

- `AGENTS.md`: "No user-specific services, credentials, or private infrastructure belong in source, fixtures, documentation, or publication metadata."
- `.gitignore:1-6` does not exclude `.stage/`, and git status shows `?? .stage/`.

`.stage/.../issue.md` contains local home-directory plan paths (for example `issue.md:146`) and text covered by the B1 decision. The plan writes `$STAGE/tasks/T5-repo-settings.md` (Task 5), and every task ends with "commit, push".

REWRITE TO: add to each task's commit step: "Stage explicit paths only. `git status` must show no `.stage/` path in the index before committing." Do not add `.stage/` to `.gitignore` unless the user asks for it.

## Checked and compliant

- **Hosted CI.** Hosted CI is excluded (plan:131), and no task waits on or reruns hosted lanes. The edits at QUALIFICATION:143 only correct an existing claim; they do not depend on CI.
- **Non-blocking issues.** Issues #6, #7, #10 and #11 are disclosed as known limitations, not as blockers (plan:13, 118, 122, 247).
- **Scope.** Scope matches the user's list. Tagging, publication, downloaded-package verification and the GitHub release are out of this stage (plan:133). The PR uses `Refs #1` (plan:127, 305).
- **Interface framing.** The three-interface framing (Rust, MCP, CLI) appears in the README, crate READMEs, the CHANGELOG and the social preview.
- **Core boundaries.**
  - No Cargo dependency is added. `purrassist-core` is untouched (`AGENTS.md`, `docs/DEPENDENCIES.md:249-281`).
  - The new scripts are stdlib Python, in line with `docs/DEPENDENCIES.md:87` and `:227` (Python 3.12+ for repository verification).
- **SPDX headers.**
  - New `.py`, `.md` and `.svg` files carry SPDX headers (`AGENTS.md`; `scripts/check-repository.py:303-311`; `CONTRIBUTING.md:22-23`).
  - All 39 tracked Markdown files already carry SPDX, so the new check-docs rule creates no churn in frozen or vendored data.
- **Gate.** `make check` is the gate and `make package` provides packaging evidence (plan:126, 302). Neither the commit steps nor the tool commands bypass any hook.
- **No user-specific content.**
  - The MCP client config example has no user-specific paths (plan:192).
  - The RELEASE.md derive record states "identical ownership" without naming a login, consistent with `docs/RELEASE.md:71` ("No login is hard-coded").
- **Attribution.** No agent attribution, trailer, generated-by credit or agent author identity appears in the planned commits, PR title or body, docs, image text or repo description. The implementer should keep issue and PR comments free of tool-default footers.

VERDICT: BLOCKED

---

## Pass 2 (recheck of plan revision 2)

Reviewed: the same `plan.md`, revision 2 (its "Review dispositions (revision 2)" section is at lines 347-389). Line numbers below refer to revision 2.

### Prior findings

**B1: DISCHARGED.**

- Line 14 now reads "Hosted CI is out of scope for this stage." The pass-1 line 178 wording rule is gone.
- A case-insensitive search of revision 2 for account, billing, payment, spend or similar wording finds nothing.
- The disposition text at line 351 ("the plan contains no excluded topic ... kept in an unposted stage note that covers docs, comments, the PR and commits, and no blocklist strings are committed") refers to the rule without naming its subject. Posting this plan (Task 6, step 5, line 345) therefore no longer mentions the matter.
- The banned-phrase list at line 143 contains no such terms.
- Condition: no unposted stage note exists in the stage directory yet (only `plan.md`, analysis files and `reviews/` are present). Create it before Task 1 starts. Its scope must cover issue updates, the T5 records, the handoff and the least-confident note, as well as docs, comments, the PR and commits.

**A1: DISCHARGED.**

- Line 33 records that each `vendor/tooling.json` entry needs `notices`.
- Line 155 and line 307 register `rsvg-convert` and Noto Sans in `vendor/tooling.json`, with notices vendored under `vendor/licenses/` and justified in `docs/DEPENDENCIES.md` as manual asset-rendering tools only. This satisfies `scripts/check-repository.py:374-377` and `CONTRIBUTING.md:14-15`.
- See N1 for the license spelling.

**A2: DISCHARGED.**

- Line 155 and lines 305-306 add a dedicated `docs/assets.json` that records the SVG and PNG sha256, the render command, the tool version, the font sha256 and the license.
- A REUSE-style `docs/social-preview.png.license` sidecar is added. No rule forbids a sidecar; the repository simply had no convention, and the plan now states one explicitly and checks it.

**A3: DISCHARGED.**

- Line 148 cites `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp`.
- The test exists at `crates/cli/tests/protocol.rs:743` and asserts `direct.to_json() == via_mcp.to_json()` for every registry entry (lines 765-771). That is executable evidence for the CLI≡MCP sentence.
- The plan disclaims identity for the `decode`/`match`/`validate` shortcuts (lines 148 and 174-176). This keeps the claim within what is proven.

**A4: DISCHARGED, with the coordinator's constraint applied.**

- The coordinator reports that no text-to-path tool is available. Revision 2 replaces the open choice with a single font, "Noto Sans", with no fallback (lines 155 and 300). The font file sha256, the `rsvg-convert` version and the PNG sha256 are pinned in `docs/assets.json`.
- A fresh render must reproduce the PNG sha256 (line 155, line 314). That re-render comparison is the hard-fail signal `.goals:6` asks for.
- See N2 for a gap the pin leaves open.

**A5: DISCHARGED.**

- Line 45: "Qualification and publication are bound to the exact commit that is tagged."
- Line 152 and line 265: "the exact `main` commit to be tagged".
- These are consistent with `docs/RELEASE.md:124-125`. The existing qualification sequence is kept (line 265).

**A6: PARTIALLY DISCHARGED (advisory, non-blocking).**

- Task 1 (line 207) says "commit (explicit files, no `.stage/`)".
- Tasks 2, 3 and 4 (lines 259, 292 and 317) still say only "commit, push".
- REWRITE TO: state the rule once for every task, for example under Standing goals: "Every commit stages explicit paths only; no `.stage/` path is ever in the index." Alternatively, copy the Task 1 wording into Tasks 2-4.

### New items in revision 2

**N1 (ADVISORY). Use the SPDX license identifier.** Line 155 writes the `rsvg-convert` license as "LGPL-2.1+". Existing `vendor/tooling.json` entries use SPDX identifiers (for example `"GPL-2.0-or-later"` at `vendor/tooling.json:65` and `"LGPL-2.1-or-later AND GPL-2.0-or-later"` at :78). REWRITE TO: `"license": "LGPL-2.1-or-later"` (verified against the upstream librsvg COPYING), and `"OFL-1.1"` for Noto Sans.

**N2 (ADVISORY). Silent font substitution, and the font identity.** Governing text: `.goals:6` (hard fails, no optionality) and `AGENTS.md` ("No user-specific services ... or private infrastructure belong in source ...").

- `rsvg-convert` resolves `font-family` through fontconfig. If "Noto Sans" is not installed, fontconfig substitutes another font without an error. The only thing that catches this is the re-render digest comparison, which line 155 marks "Not in `make check`".
- REWRITE TO: in Task 4 and the handoff, state that the re-render sha256 comparison is a mandatory pass/fail step whose mismatch blocks the commit.
- Record the font in `docs/assets.json` and `vendor/tooling.json` by upstream source URL, version and file sha256, not by a local system font path.

**Checked and compliant in revision 2:**

- **Identity in the plan.** Line 17 no longer names an owner login. Lines 12-20 contain no user-specific identity.
- **New core test.** The new `crates/core/tests/tool_call_dialects.rs` assertion reads `docs/` from a test, not from `crates/core/src`. The ambient-access scan (`docs/DEPENDENCIES.md:266-272`) and the core dependency rules are unaffected, and no Cargo dependency is added.
- **Bless switch.** `PURRASSIST_BLESS` with a guard when `CI` is set follows the existing precedent at `crates/core/tests/recovery_goldens.rs:190-193`.
- **New JSON registries.** `docs/known-limitations.json` and `docs/assets.json` are data files. The SPDX scan (`scripts/check-repository.py:303-311`) does not cover `.json`, consistent with existing `vendor/*.json`.
- **SPDX scan extended to Markdown.** All tracked Markdown already carries headers.
- **Out-of-scope items.** Hosted CI, tagging and publication remain out of scope (lines 160-164).
- **Known limitations.** #6, #7, #10 and #11 are recorded with `blocks_release: false` (line 145).
- **Homepage.** It now points to the repository README, avoiding a docs.rs placeholder claim (line 324).
- **Attribution.** No agent attribution, trailer or generated-by credit appears in the PR title or body, the commit steps, the description, the topics or the image text.
- **Social preview.** If the Playwright upload is impossible, the row is reported as NOT DELIVERED rather than claimed done (line 156). This is consistent with `AGENTS.md` ("Capability claims require executable evidence").

No BLOCKING findings remain. The open items (A6 partial, N1, N2) are advisory and cheap.

VERDICT: PASS
