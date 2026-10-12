# Review: Stage 2 remediation plan (issue #1, PR #20)

- Reviewed plan: `.stage/complete-purrassist-0-1-0-plan-and/remediation-plan.md`
- Against: `gap-analysis.md`, `plan.md`, `output-rules.md`, `AGENTS.md`, and the worktree files `scripts/check-docs.py`, `scripts/test-check-docs.py`, `scripts/check-repository.py`, `scripts/test-repository-audit.py`, `CHANGELOG.md`, `docs/RELEASE.md`, `docs/known-limitations.json`, `scripts/fuzz.py`, the crate READMEs, `crates/cli/Cargo.toml`, `.gitattributes`, `.github/workflows/release-qualification.yml` and `Makefile`.
- Entry point: `make check` -> `hygiene` (`Makefile:11-17`) runs `check-repository.py`, `test-repository-audit.py`, `check-docs.py` and `test-check-docs.py`. Every fix in the plan lands in one of those four scripts or in a document they read, so each fix is on the real entry point.
- Working-tree note: `CHANGELOG.md` already has the G1 sentence removed, "Release order:" (L1) and the scoped receipt sentence (L2). I reviewed the plan text, not those edits.

## Coverage against gap-analysis.md

| Gap | Plan item | Mapped |
|---|---|---|
| M1 (required) | G1 | yes |
| M2 | G2 | yes |
| L1-L4 | G3 | yes |
| L5, L6 | G4 | yes. The QUALIFICATION heading is kept, with a stated reason that `stage2-heading` enforces. |
| L7 | G5 | yes |
| L8 | G6 | yes, but broadened past the gap's fix (see B1) |
| L9 | G7 | yes |
| L10 | G8 | yes |
| L11 | G9 | yes |
| L12 | G10 | yes |
| Owner-gated social-preview upload | "Not a branch gap" | yes |

Every gap-analysis finding has a plan item.

## BLOCKING

### B1. G6 would fail the real workflow and turn `make hygiene` red

Plan text:
> "Every step item must have a `name:`; an unnamed step is an error."

The first step of the real `release-qualification.yml` has no name:

```
    steps:
      - uses: actions/checkout@11d5960a... # v4
        with:
          persist-credentials: false
```

The existing test `test_only_job_steps_are_read` (`scripts/test-check-docs.py:671-674`) deliberately includes an unnamed `- uses: x` step and expects it to be skipped. As written, G6 makes `check-docs.py` fail on the current tree, which creates a new false positive. The implementer then has two ways out, and the plan sanctions neither:

- edit a release-gating workflow to add a name, which then also has to appear in the RELEASE.md summary; or
- quietly narrow the rule.

The gap analysis proposed the narrower rule: fail on an item that has `run:` and no `name:`.

REWRITE TO:
> "Parse only the items of each job's `steps:` list, meaning list items at the indentation of that list's first `- ` and not lines inside a `run: |` block scalar. A step item that has `run:` and no `name:` key (in any key order) is an error. An unnamed `uses:` step, such as `actions/checkout`, is not a qualification step and is ignored. This matches `test_only_job_steps_are_read`, which stays. Items outside `steps:`, such as matrix `include`, are ignored. Update the module docstring line that says 'from the `name:` of every step'."

Add these checks:
- The real workflow still passes with its unnamed checkout step.
- `- run: make wasm-test` appended to a steps block fails.
- `- run: x` followed by `  name: Late` (name after run) is read as the step `Late`.
- A `run: |` body line that starts with `- name: Fake` does not add a step.

### B2. The G8 acceptance check cannot fail

Plan text:
> "`PYTHONUTF8=0 python3 -X utf8=0` runs of the checkers pass."

On the Linux and macOS hosts that run `make check`, the locale encoding is already UTF-8. Bare `read_text()` therefore decodes the same way with or without UTF-8 mode, and this command passes on the unfixed code. The second check, `grep -n "read_text()" scripts/check-docs.py`, only catches that exact spelling. It misses `open(...)` and `write_text(...)` in the test helper, and it misses `subprocess.run(..., text=True)` / `check_output(..., text=True)`. Those also decode with the locale encoding:
- `git ls-files` in `markdown_names`, `tracked_markdown` and `tracked_binaries`;
- `cargo metadata` in `workspace_packages`.

REWRITE TO:
> "**Check:** `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py`, the same flags for `scripts/test-check-docs.py`, and the new `check-repository.py` functions under `test-repository-audit.py`, all exit 0. Before the fix they fail. Every `subprocess` call with `text=True` in these scripts also passes `encoding='utf-8'`. A test forces `os.symlink` to raise `OSError` and shows that the tree builder falls back to copying."

As an alternative falsifier, `LC_ALL=C python3 -X utf8=0 scripts/check-docs.py` fails today on the non-ASCII docs.

## ADVISORY

### A1. G7: name the `i/` column, fix the fixture bytes, and add a negative fixture

Plan text:
> "Select tracked binaries from `git ls-files --eol` (index attribute `-text`), with no suffix list."

`git ls-files --eol` has two different columns:
- `i/-text` is git's content detection on the index blob: NUL, lone CR, or mostly non-printable bytes.
- `attr/-text` comes from `.gitattributes`.

Here `.gitattributes` binds `binary` case-sensitively for ten suffixes only. An `attr/` reading would therefore miss exactly the L9 cases (`b.PNG`, `.webp`, `.ttf`).

Two concrete traps:
- The existing fixture writes `docs/a.png` as `b'\x89PNG'` (`test-repository-audit.py:326`). That content has no NUL and no CR, so git reports it as `i/none`, and `test_tracked_binaries_are_the_index_minus_test_data_plus_the_required_ones` would fail under the new selection. The fixtures need real binary bytes.
- The plan lists only failing fixtures. A selector that returns every tracked file would pass all three of them.

Also, the sentence "Existing tracked binaries under excluded test-data and vendor paths keep their current treatment" describes an exemption that does not exist. Only `crates/core/tests/data/` is exempt today (`check-repository.py:282`), and `vendor/` holds only text. If someone reads that sentence as adding a `vendor/` exemption, it becomes an unrecorded narrowing.

REWRITE TO:
> "Select a tracked file as binary when its `git ls-files -z --eol` entry has `i/-text`, or when its attribute column contains `-text`. Keep `REQUIRED_BINARY_ATTRIBUTION`. The exemption stays exactly `BINARY_ATTRIBUTION_EXEMPT = ('crates/core/tests/data/',)`, with no new prefix. Record the old and new `tracked_binaries(ROOT)` lists in the commit summary. Any newly selected file gets a sidecar or a named, justified exemption. Fixtures contain NUL bytes. Add a negative fixture in which a tracked `.svg` and a `.md` are not selected."

### A2. G9: the pinned-link trade-off is acceptable, but the rule should verify the paths

Judgment on the trade-off:
- crates.io and docs.rs render these READMEs only after `publish.yml`, and that runs only after `rust-v<version>` is pushed. Published pages therefore never show the 404.
- The 404 window affects only people browsing `crates/*/README.md` on GitHub `main` before the tag, and again after each version bump until the next tag.
- Pinning prevents a 0.1.0 crate page from silently describing later `main`. The derive docs.rs pins already set the same precedent.
- I accept the trade-off.

The gap is in checking. Once the links are absolute, nothing verifies them: `check_links` skips URLs (`check-docs.py:483`). A typo in a pinned path would ship as a permanent 404 in immutable metadata.

REWRITE TO (add to G9 Fix and Check):
> "The rule matches `github.com/<owner>/<repo>/(blob|tree|raw)/<ref>/<path>` in every packaged README and in each publishable package's `documentation`. `<ref>` must be `rust-v<workspace version>`. `<path>`, and its `#fragment` for `.md` targets, must resolve in the working tree through the same logic as `check_links`. The repository root URL and links to `issues/` or `security/` are allowed. Fixtures that fail: a `blob/main` README link; a `blob/main` `documentation` value; a `blob/rust-v0.0.9` link; a pinned link to a missing path. In `docs/RELEASE.md`, say that these links resolve once the tag exists."

`crates/cli/README.md:64` (`INTEGRATION.md#reading-a-result-...`) is a `blob/main` link that the plan's site list does not name. The rule above would catch it.

### A3. G10: the fix must keep the exact-key schema check

Plan text:
> "Add a `fuzz_stop_targets` list field to the entries in `docs/known-limitations.json` (#6 and #11 today)."

`load_limitations` rejects any entry whose keys are not exactly `{'issue', 'title', 'impact', 'blocks_release'}` (`check-docs.py:537`). The plan never says whether the field is required on every entry or optional, or how it is validated. The easiest way to make the gate pass is to loosen that guard.

REWRITE TO:
> "Every entry carries `fuzz_stop_targets`, a list (empty for no stop) of names in `fuzz.TARGETS`. The exact-key check becomes `{issue, title, impact, blocks_release, fuzz_stop_targets}`. The stop set is the issues with a non-empty list. The QUALIFICATION and FUZZING sentences and the fuzz row must name exactly that set, in any order, in the form `#A, #B and #C`. Add a fixture with an unknown target name, and a fixture with a missing key."

### A4. G2: the five status docs can disagree

Plan text:
> "the `workspace-version` status rule accepts either the unpublished sentence … or the published sentence"

The rule checks each document on its own. After publication, a README that says "published as" alongside a SECURITY.md still saying "not yet tagged" would pass.

REWRITE TO: "… and all `STATUS_DOCS` must use the same form. A fixture with mixed forms fails."

### A5. G1: name the section by version and consider the release body's content

- Write the rule as "the `[<workspace version>]` section body", which matches `check_changelog`'s use of `version`, rather than hard-coding `[0.1.0]`.
- Separately, `## Known limitations` in `CHANGELOG.md` is a sibling `##` section. So "the `[0.1.0]` section" used as the GitHub release body (RELEASE.md step 5) leaves the known limitations out of the release.
- This is optional, but useful: make step 5 read "the `[0.1.0]` section and the `## Known limitations` section". That costs one line and gives the published release its limits.

### A6. G3: the wording fixes have no checks that can fail

`tools_reference_matches_registry` byte-pins only the generated block. The plan's own note says the L3 edit is outside that block, so the test cannot detect it.

REWRITE TO: "Check: `git grep -n` finds no match for the old strings ('Published in this order', 'Every result carries', 'byte for byte', 'uploaded manually') in the edited files, and finds the new strings."

### A7. G4: the jargon scope is uneven

The plan rewords "stage-3 pass 5" (`docs/tool-calls/INTEGRATION.md:156`). The same kind of reference remains at:
- `INTEGRATION.md:436` ("pass 6");
- `docs/MATCHING-QUALITY.md:303` and `:321` ("adversary pass 6");
- `docs/tool-calls/dialects/qwen3-coder-xml.v1.md:237`.

None of these were in L6. Either reword them in the same commit, or state that they stay and why.

On the new ban `\btask \d+\b`: it has no false positive on legitimate text today. The only matches in scope are the three sites G4 rewords (`RESOURCES.md:1073`, `DEPENDENCIES.md:46`, `FUZZING.md:74`), and a multiline search shows no line-wrapped "Task\nN". `BANNED_SAMPLES` must gain the "see Task 12" entry, because `test_samples_cover_the_exact_list` enforces that, and the `Pre-release` sample must change to the G5 form.

### A8. The narrowed fix is not named in the header

Plan text:
> "one gets a narrowed fix with its reason"

Name it: "(G4: the QUALIFICATION `Historical: Stage 2` heading stays)".

## Constraints

- `output-rules.md`: the plan has no hosted-CI account or payment wording and no attribution.
- `AGENTS.md`: "Capability claims require executable evidence." That is the basis for B2 and A6.
- `make check` and `make package` run once on the final head, so the gate is not replaced by focused tests.
- No new dependencies.

Pass 1 verdict: BLOCKED (superseded by Pass 2 below)

---

## Pass 2 (revision 2)

I rechecked revision 2 against the worktree: `README.md`, `docs/TOOLS.md`, `scripts/check-docs.py:78` and `:537`/`:645`, `scripts/check-repository.py`, `scripts/test-repository-audit.py`, and a grep of the in-scope docs for the new banned phrases and for `pass N`.

### Prior findings

| Finding | Status | Evidence in revision 2 |
|---|---|---|
| B1 (G6 rejects unnamed `uses:`) | **DISCHARGED** | G6 now fails only "a step item with a `run:` and no `name:`"; unnamed `uses:` steps are ignored and `test_only_job_steps_are_read` is kept. Indentation parsing and all four fixtures are present (`run: |` body line, name after run, matrix `include`, real workflow passes). |
| B2 (G8 check cannot fail) | **DISCHARGED** | The check is now `-X warn_default_encoding -W error::EncodingWarning`, `subprocess` gets `encoding='utf-8'`, and a test covers the symlink fallback. This fails before the fix on any OS. The fix scope still has a gap (N2). |
| A1 (G7 column, bytes, negative fixture) | **DISCHARGED** | G7 names the `i/-text` column, uses NUL-bearing fixtures, adds a negative text-file fixture, replaces the `b'\x89PNG'` content, and says "No vendor exemption is invented". The optional `attr/` union was not adopted, which is acceptable: real PNG, WebP and TTF content is classified `i/-text`. |
| A2 (G9 path verification) | **DISCHARGED** | Pinned paths and anchors are checked, there are fixtures for a missing path, a missing anchor and `documentation`, and `crates/cli/README.md:64` is named. I still accept the trade-off for the reasons in A2. |
| A3 (G10 schema) | **DISCHARGED** | The key is added to the exact-key check as optional, with a list-of-known-targets value, and is "not loosened otherwise". Minor: the "an unknown target fails" fixture appears in Fix but not in the Check list. Add it there. |
| A4 (status docs disagree) | **DISCHARGED** | All five docs must use one form, with a fixture. |
| A5 (release body omits limits) | **DISCHARGED**, with a new note (N3) | The section moves to `### Known limitations` inside `[0.1.0]`. I checked the moved text against G1's ban: no limitation title or impact contains "unreleased" or "not yet tagged". The `#known-limitations` anchor slug is unchanged. |
| A6 (wording checks) | **DISCHARGED**, but the new ban creates a false positive (N1) | The four banned phrases are added, each with a fixture. |
| A7 (pass N jargon) | **DISCHARGED** | "the four remaining" sites match my grep: MATCHING-QUALITY `:303` and `:321`, qwen3-coder-xml `:237`, INTEGRATION `:436`. There is no ban, with the reason given. |
| A8 (narrowed fix not named) | **DISCHARGED** | The header names G6 and G4 as narrowed. |

### New findings in revision 2's text

#### N1 (BLOCKING). The A6 ban `published in this order` fails on `README.md:105`, a line no fix edits

`README.md:105` currently reads:

> "Six crates make up the release set, published in this order:"

This is the README twin of L1. Commit `a42a817` ("describe the release set without claiming publication") deliberately reworded it to this form, and gap-analysis L1 treated it as fixed. `banned_matches` is case-insensitive over all in-scope docs, so the new ban flags this line. Commit 3 would then fail `check-docs.py`, and the plan lists no edit to `README.md`. The two quick ways out are to drop the ban (the fix shrinks back to the unfalsifiable form) or to edit README under no named gap. That is the same class of problem as B1. I grepped the other three phrases: `uploaded manually`, `every result carries` and `compares the output byte for byte` have no remaining hits. Plain "byte for byte" appears legitimately in many docs, but the anchored phrase is safe.

Plan text:
> "**A6:** new banned phrases make the wording fixes falsifiable: `published in this order`, `uploaded manually`, `compares the output byte for byte`, and `every result carries`."

REWRITE TO (pick one):
> "**L1:** `CHANGELOG.md` says "Release order:", and `README.md:105` says "Six crates make up the release set, in release order:". Both land in commit 1. The ban `published in this order` then holds on the whole tree."

or
> "Ban `published in this order` only in the `[<version>]` CHANGELOG section (the release body), as part of the G1 rule. `README.md:105` stays as `a42a817` worded it."

#### N2 (ADVISORY). G8's fix scope is narrower than its check

Plan text:
> "Every `read_text`/`open` in `check-docs.py`, `test-check-docs.py` and the new `check-repository.py` code passes `encoding='utf-8'`."

The check, however, runs all of `check-repository.py` and `test-repository-audit.py` with `-W error::EncodingWarning`. Their older code also uses the implicit encoding and would raise:
- `check-repository.py`: `path.read_text()` at `:173`, `:217`, `:368`, `:375`, `:390`, `:413` and others, and `check_output(..., text=True)` at `:402-405`;
- `test-repository-audit.py`: for example `write_text(self.SIDECAR)` at `:329`.

The check is right. The fix has to cover every implicit-encoding call in all four scripts. Do not drop those two scripts from the check.

Optional, low cost: put the same `-X warn_default_encoding -W error::EncodingWarning` flags on the four `python3` lines of `Makefile` `hygiene`. Then `make check` keeps the portability on the real entry point, instead of relying on a one-time focused run.

#### N3 (ADVISORY). A5: find the CHANGELOG known-limitations block inside the version section

Today `check_known_limitations` requires `text.splitlines().count('## Known limitations') == 1` in each holder and reads `sections(text)[heading]` (`check-docs.py:645-648`). `sections()` is a dict keyed by the heading line. At 0.2.0 the changelog will have two `### Known limitations` lines, the older one further down the file, so the dict would return the 0.1.0 block and a file-wide count would fail. For `CHANGELOG.md`, search within the `[<workspace version>]` section body, which the plan already says ("requires that subsection inside the version section"). Implement it that way, not as a file-wide count.

#### N4 (ADVISORY, editorial)

- **Commit count.** Commit boundaries lists three commits, but the next line says "After all four". Change it to "After all three".
- **Header bullet.** The G4 header bullet swallowed the sentence "Validation uses focused checks per gap. The single full `make check` and `make package` run once after the fixes settle." Move that sentence out of the bullet.
- **Commit ordering.** Commit 3 carries the A5 `CHANGELOG.md` move and all the new bans together, so each intermediate commit should still pass hygiene. Commit 1 must therefore contain every rewording that commit 3's bans depend on, which now includes N1. Commit 2 must contain the G4 and A7 rewording.

### Pass 2 summary

- Every pass 1 finding is discharged.
- One new BLOCKING finding (N1): revision 2 adds a banned phrase that fails on a line the plan does not edit. Either rewrite fixes it in one line.
- N2 to N4 are advisory.

Pass 2 verdict: BLOCKED (superseded by Pass 3 below)

---

## Pass 3 (revision 3, final allowed pass)

I rechecked revision 3 against the worktree. Commits 1 and 2 (`4933cc4`, `d6fb1e0`) are already in the tree. I grepped the tree for:
- `Task N`, `pass N`, `blob/main` and the A6 phrases;
- the `blob/rust-v0.1.0` pins in `crates/`;
- the #7 impact in `docs/known-limitations.json`.

### Prior findings

| Finding | Status | Evidence |
|---|---|---|
| N1 (`published in this order` fails on `README.md:105`) | **DISCHARGED** | This takes the second rewrite option. G3/N1: "not a global ban … The G1 release-body rule bans it only inside the CHANGELOG version section … a README fixture with the phrase passes." `README.md:105` stays as `a42a817` worded it, and the global `BANNED_PHRASES` no longer contain it. The other three A6 phrases have no hits in the tree, so they are safe as global bans. |
| N2 (G8 fix narrower than check) | **DISCHARGED** | The G8 Fix now covers "all four scripts … the whole of `check-repository.py` (including the older code at `:173`, `:368`, `:402-405` and `:413`) and the whole of `test-repository-audit.py`". The four `Makefile` `hygiene` lines run with `-X warn_default_encoding -W error::EncodingWarning`, so `make check` keeps the protection. Because `-W` populates `sys.warnoptions`, `unittest.main()` does not override the filter. The scripts already need Python 3.11+ for `tomllib`, and `EncodingWarning` exists from 3.10, so the flags are valid on every lane that runs `make check`. |
| N3 (CHANGELOG block located file-wide) | **DISCHARGED** | The G1/N3 text locates the subsection "by heading position, not by a file-wide count or a dict lookup", with a fixture covering both the 0.2.0 and 0.1.0 sections. A fixture with the subsection outside the version section must fail. |
| N4 commit count | **DISCHARGED** | "After all three". |
| N4 commit ordering | **DISCHARGED** | Verified in the tree. No `Task N` or `pass N` remains in in-scope Markdown. All eight crate links and the cli `documentation` are pinned to `blob/rust-v0.1.0`, including `crates/cli/README.md:64`. The #7 impact is reworded in the JSON and its three rendered copies. No `uploaded manually`, `every result carries` or `compares the output byte for byte` remains. Commit 3's bans will therefore hold on the tree it lands on. |
| N4 header bullet | **STILL-OPEN (editorial only)** | Plan lines 9-10 still read "- **G4:** keeps the plan-mandated historical "Stage 2" heading. Validation uses focused checks per gap. The / single full `make check` and `make package` run once after the fixes settle." The validation sentence still sits inside the G4 bullet. It has no effect on behavior, and the meaning is recoverable. |

### New text in revision 3

- **ADVISORY (editorial):** G1's bullets are out of order. The plan reads "**A5:** the release body must contain the known limitations.", then the N3 bullet, then "The A5 bullet follows: Move the CHANGELOG …". All the content is present. Merging the first and third bullets would make the instruction read in order. This is no defect.
- **ADVISORY (carried from A3):** the G10 Check list still omits the "unknown target fails" fixture that the Fix promises. Add it to the fixtures when implementing.
- **Observation, outside the gap's scope, no finding:** "Task N" and "adversary/stage-3 pass N" remain in Rust and Python comments. Examples: `crates/core/src/schema/mod.rs:130`, `crates/core/src/text/idna_data.rs:4`, `crates/core/tests/tool_call_goldens.rs:2685`, `scripts/fuzz.py:32`. Gap-analysis L6 and the `check-docs.py` scope are Markdown only, so this is not a plan gap. Some of these comments are `///` docs on public items and would appear on docs.rs. That would be a follow-up, not part of this stage.

### Pass 3 summary

- Every BLOCKING finding from passes 1 and 2 is discharged.
- The remaining items are editorial or fixture-list notes and do not affect whether the plan works.
- Every fix still lands on `make hygiene` → `make check`, each acceptance check can fail, no fix narrows scope, and nothing from the gap analysis is missing.

Pass 3 verdict: PASS (superseded by Pass 4 below)

---

## Pass 4 (revision 4: G11 and the commit list only)

This pass checks only G11, the commit list, and the two tidy-ups. Evidence:
- `Task N` counts over `crates/{core,derive,syntax,purrassist,host,cli}/src`, including wrapped lines;
- `scripts/generate-unicode.py`, `vendor/unicode/generated.json` and the package `include`/`exclude` keys;
- other source digests registered anywhere in the repository;
- the remaining process tags in published `src`.

### Earlier notes that revision 4 closes

| Item | Status | Evidence |
|---|---|---|
| G1 bullet order (pass 3 editorial) | **DISCHARGED** | The bullets now read A5 statement, then the A5 move, then N3. |
| G10 unknown-target fixture (pass 3, carried from A3) | **DISCHARGED** | G10 Check: "A fixture with an unknown `fuzz_stop_targets` value (not in `scripts/fuzz.py` `TARGETS`) fails." |
| N4 header bullet | **DISCHARGED** | Lines 9-11 now end the G4 bullet at "heading." The validation sentence is its own paragraph. |

### G11 assessment

**Scope and authorization.**
- G11 goes beyond the gap analysis. It comes from my pass-3 observation, and the coordinator adopted it explicitly.
- It narrows nothing. It does not touch behavior: the changes are comments, Rustdoc, and one `reason = "…"` string literal in a lint attribute.
- It is consistent with "process jargon out of shipped text", the principle L5 and L6 apply.
- I accept the addition.

**The figures check out.**
- I count exactly 26 `Task N` occurrences in the published crates' `src`, across 14 files.
- Of those, 24 are in `purrassist-core`: `schema/recovery/proposals.rs` 6, `text/mod.rs` 4, `schema/recovery.rs` 3, `idna_data.rs` 2, and others.
- The other 2 are in `purrassist-host`: `tools/witness.rs` and `tools/markdown/structure.rs`.
- `derive`, `syntax`, `purrassist` and `cli` have none.
- A multiline search finds no `Task` / `N` split across comment lines, so a per-line rule is complete today.

**The "stage N" exemption is correct.** Every "stage 1–5" in `src` is in `schema/algebra/{canon,merge,generate}.rs`. Those name the steps of the Attouche et al. witness pipeline ("Stage 1 (canonicalize) and stage 2 (`not` elimination) of the Attouche, …"). That is domain terminology.

**Regeneration is sound.** `generate-unicode.py` reads its inputs from `.cache/unicode/`. With `--fetch`, it downloads missing inputs from unicode.org. It checks every input against the sha256 pinned in `vendor/unicode/manifest.json`, and it rewrites `vendor/unicode/generated.json` itself (`:492-493`), so the digest is not edited by hand. `check-repository.py:389-392` verifies those digests on `make check`.

**No other live digest breaks.**
- Source-file digests also appear in `docs/qualification/helper-census-linux-x86_64.json`. That is an archived report bound to `git_head_observed_after_run` `6bcc257…`, and no rule compares it with the current tree.
- The evaluation manifest registers no `crates/*/src` file.
- Comment edits therefore break no registered digest.

#### G11 findings

**P4-1 (ADVISORY). Make the regeneration check able to fail.**

Plan text:
> "`python3 scripts/check-docs.py` and `python3 scripts/check-repository.py` pass (generated-data digests)."

The generator writes `generated.json` itself, so this check passes even if someone edits `idna_data.rs` by hand and then refreshes the digest. It does not show that the committed file is generator output.

REWRITE TO (add):
> "After the commit, `python3 scripts/generate-unicode.py` (with `--fetch` if `.cache/unicode/` is empty; inputs are pinned by sha256) followed by `git diff --exit-code crates/core/src vendor/unicode` is clean. In the commit's diff, `idna_data.rs` changes only its header comment and the `reason = "…"` line, `generated.json` changes only the `idna_data.rs` digest, and the two `unicode_data.rs` files are byte-identical."

That last condition also guards against a nondeterministic or upstream-drifted regeneration slipping in under a comment fix.

**P4-2 (ADVISORY). State the scope boundary that matches the stated reason.**

Plan text:
> "26 `Task N` references … ship in the immutable 0.1.0 archives".

That is true, but no published crate sets `include` or `exclude`, so `tests/`, `benches/` and `examples/` ship in the archives too. Those directories hold many more `Task N` and "adversary pass 6" comments, for example:
- `crates/core/tests/recovery_goldens.rs:3`;
- `crates/core/benches/latency.rs:5`;
- `crates/core/examples/fuzz_tool_call.rs:436`;
- `crates/cli/tests/protocol.rs:448`.

The `src`-only scope is the right line, because only `src` renders as Rustdoc. The plan should say so, so that a later reader does not count it as a silent cut.

REWRITE TO: "Scope is `src/` because it renders on docs.rs. Comments in packaged `tests/`, `benches/` and `examples/` are developer labels and stay. That is out of scope for 0.1.0."

**P4-3 (ADVISORY). One process tag in `src` is not `Task N`.**

`crates/core/src/grammar/codec/xml.rs:1300` reads "… repair reads it as one: T5/F9 …". It is a `//` comment, not Rustdoc, so it does not render. Either reword it with the 26, or let the scope sentence from P4-2 cover it explicitly. Do not widen the `source-jargon` regex to `T\d/F\d`; that would invite false positives.

**P4-4 (ADVISORY, design). Where the rule lives.**
- `check-docs.py`'s docstring scopes it to "every tracked `*.md` file". Adding a rule over `.rs` changes that scope.
- `check-repository.py` already walks crate sources (attribution, ambient access) and would be the more natural home.
- Either placement works. If it stays in `check-docs.py`, add it to the docstring list.
- Share one compiled `\btask \d+\b` pattern between the Markdown ban (G4) and `source-jargon`, so the two cannot drift.

### Commit list

| # | Content | Hygiene at that commit |
|---|---|---|
| 1 `4933cc4` | G1 sentence removal, G3 text | landed |
| 2 `d6fb1e0` | G4 and A7 rewording, G9 pins | landed |
| 3 | all checker rules, fixtures, A5 move, Makefile flags | the bans hold on the tree (verified in pass 3) |
| 4 | G11 rewording, regenerated `idna_data.rs` and `generated.json`, `source-jargon` rule with fixtures | the rule lands with the rewording, so it holds at that commit |

The order is sound. Each commit can pass the hooks on its own. "After all four" now matches. One cosmetic issue: commit 4 appears in prose instead of as item 4 of the numbered list.

The G11 Check runs `cargo clippy` and `cargo doc` only for core and host. That is enough, because those are the only crates touched, and the final `make check` covers the rest.

### Pass 4 summary

- G11 is a sound, authorized addition with no blocking defect.
- Its counts and exemption match the tree, regeneration goes through the pinned generator, and no live digest breaks.
- P4-1 to P4-4 are advisory. P4-1 is the one worth adopting before implementation, because it is the only way to show that the generated file really came from the generator.
- The commit list is consistent, and each commit passes hygiene on its own.

VERDICT: PASS
