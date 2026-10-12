# Stage 2 remediation plan — issue #1, PR #20 (lightweight path)

Revision 4. This revision folds in `reviews/remediation-plan-review.md` (pass 1: B1, B2, A1–A8; pass 2: N1–N4; pass 3 notes) and adds G11 from the pass-3 observation.

Source: `gap-analysis.md` (combined reviewer, VERDICT BLOCKED on M1). All findings are fixed and none is deferred.

Narrowed fixes, with reasons:
- **G6:** fails only `run:` steps with no `name:`. Unnamed `uses:` steps such as checkout are legitimate and stay ignored.
- **G4:** keeps the plan-mandated historical "Stage 2" heading.

Validation uses focused checks per gap. The single full `make check` and `make package` run once after the fixes settle.

## Gaps and fixes

### G1 — M1 (required): the release body would ship "not yet tagged"

**Fix:**

- Remove the sentence "It is not yet tagged `rust-v0.1.0`; the date above is set when it is tagged." from the `CHANGELOG.md` `[0.1.0]` section. The `- unreleased` heading carries the status.
- Add a `check-docs.py` rule: the `[0.1.0]` section body (the release body) must not contain "not yet tagged" or "unreleased" outside the heading line. Add a failing fixture.
- **A5:** the release body must contain the known limitations.
- The A5 bullet follows: Move the CHANGELOG `## Known limitations` section to `### Known limitations` inside `[0.1.0]`. The changelog rule then requires that subsection inside the version section, and its issue set still equals the JSON. Add a fixture with the subsection outside the version section, which must fail.
- **N3:** the subsection is located inside the *current workspace version's* section by heading position, not by a file-wide count or a dict lookup. A fixture with two version sections (0.2.0 and 0.1.0), each with its own `### Known limitations`, passes, and only the current version's set is compared with the JSON.

**Check:**

- `python3 scripts/check-docs.py && python3 scripts/test-check-docs.py`.
- A fixture that re-adds the sentence fails with `[changelog]`.

### G2 — M2: the status rule contradicts the documented post-publication edit

**Fix:**

- In `check-docs.py`, the `workspace-version` status rule accepts either the unpublished sentence (`not yet tagged \`rust-vX.Y.Z\``) or the published sentence (`published as \`rust-vX.Y.Z\``). Both must still name the workspace version.
- Document the accepted published form in `docs/RELEASE.md`'s "After publication" bullet.
- **A4:** all five status docs must use the same form, so a mix of published and unpublished fails, with a fixture.

**Check:**

- A fixture with each form passes.
- A fixture with the wrong version, or with neither form, fails.

### G3 — L1, L2, L3, L4: wording

**Fix:**

- **L1:** `CHANGELOG.md` says "Release order:".
- **L2:** the receipt sentence is scoped to recovery, matching, Markdown and edit-plan results.
- **L3:** `docs/TOOLS.md` says "compares their JSON serializations for equality".
- **L4:** `docs/assets.json` says "for manual upload in the repository settings".

**Check:**

- `check-docs.py`, `check-repository.py`, and `cargo test -p purrassist-cli --test protocol tools_reference_matches_registry` (for the text outside the generated block).
- **A6:** new banned phrases make the wording fixes falsifiable: `uploaded manually`, `compares the output byte for byte`, and `every result carries`. Each has a failing fixture.
- **N1:** `published in this order` is not a global ban, because the README's "the release set, published in this order" legitimately describes the intended publish order. The G1 release-body rule bans it only inside the CHANGELOG version section, where it would read as a claim of publication. It has a failing fixture, and a README fixture with the phrase passes.

### G4 — L5, L6: process jargon in public docs

**Fix:**

- Reword the impact in `docs/known-limitations.json` #7 to "Some commits of the #3 tool-call work were merged without an audit …", and update its three rendered copies.
- Reword `docs/RESOURCES.md` "Task 17's", `docs/DEPENDENCIES.md` "Task 7", `docs/FUZZING.md` "Task 8" and `docs/tool-calls/INTEGRATION.md` "stage-3 pass 5" by naming what they refer to (the commit or feature).
- **A7:** reword the four remaining "adversary pass N"/"pass N" process references in public docs, located with `git grep -nE 'pass [0-9]+'` on the in-scope docs. Each one is judged; ordinary usage such as "pass 2 arguments" is kept, so there is no ban.
- The QUALIFICATION heading "Historical: Stage 2 record bound to `66b3756`" stays. It names a historical record and was required verbatim by the reviewed Stage 1 plan.
- Add `\btask \d+\b` to the banned phrases. Every remaining occurrence in scope must be removed first; check with `git grep -niE '\btask [0-9]+\b'` on the in-scope docs.

**Check:**

- `check-docs.py`.
- A fixture "see Task 12" fails.

### G5 — L7: the Pre-release ban is too broad

**Fix:** anchor the pattern to `pre-release implementation`.

**Check:**

- A fixture "a pre-release version such as `0.2.0-alpha.1`" passes.
- "Pre-release implementation" fails.

### G6 — L8: unnamed or non-step workflow items

**Fix:**

- Parse step items by indentation inside each job's `steps:` block.
- A step item with a `run:` and no `name:` is an error. Unnamed `uses:` steps (for example `actions/checkout`) are ignored, which keeps the existing `test_only_job_steps_are_read` behavior.
- Items outside `steps:` (for example matrix `include`) are ignored.

**Check:**

- `check-docs.py` passes on the real `release-qualification.yml`.
- These fixtures fail:
  - appending `- run: make wasm-test` to a steps block;
  - a step whose `name:` follows its `run:`, where it must be associated correctly.
- These fixtures don't fail or add a step:
  - a `run: |` body line starting with `- `;
  - a matrix `include: - name:` item.

### G7 — L9: binary sidecar selection

**Fix:**

- Select tracked binaries from the `i/-text` column of `git ls-files --eol` (git's content classification), with no suffix list. These must carry a `.license` sidecar.
- The only existing exemption, `crates/core/tests/data/`, stays explicit. No vendor exemption is invented.

**Check:**

- `test-repository-audit.py` fixtures with real binary bytes (containing NUL) named `.PNG`, `.webp` and `.ttf` without a sidecar fail.
- A negative fixture shows that a text file is not selected.
- The existing `b'\x89PNG'` fixture is given real binary content.

### G8 — L10: Windows Python portability

**Fix:**

- **N2:** every `read_text`/`write_text`/`open`/`subprocess(text=True)` in all four scripts passes `encoding='utf-8'`: `check-docs.py`, `test-check-docs.py`, the whole of `check-repository.py` (including the older code at `:173`, `:368`, `:402-405` and `:413`) and the whole of `test-repository-audit.py`.
- The four `Makefile` `hygiene` lines for these scripts run `python3 -X warn_default_encoding -W error::EncodingWarning`, so `make check` keeps the protection.
- `test-check-docs.py` copies files instead of symlinking when `os.symlink` raises `OSError`.

**Check:**

- `python3 -X warn_default_encoding -W error::EncodingWarning` runs of `check-docs.py`, `test-check-docs.py`, `check-repository.py` and `test-repository-audit.py` pass. These fail before the fix on any OS, because every implicit-encoding `open`/`read_text`/`subprocess(text=True)` raises.
- The `subprocess` calls use `encoding='utf-8'`.
- A test forces `os.symlink` to raise `OSError` and asserts that the copy fallback works.

### G9 — L11: immutable crate metadata links to `blob/main`

**Fix:**

- In the six published crate READMEs (including `crates/cli/README.md:64`) and `crates/cli/Cargo.toml` `documentation`, repository doc links point to `blob/rust-v0.1.0/...`. A crate page then documents its own version.
- `check-docs.py` asserts:
  - packaged README and Cargo `documentation` links into the repository use `rust-v<workspace version>`;
  - **A2:** the pinned path exists in the tree and its `#anchor` resolves, reusing the link checker's anchor logic.

**Cost, stated visibly:** the links 404 on GitHub until the tag exists. This is the same trade-off already accepted for the derive docs.rs `0.1.0` pins. The root README keeps `main` links.

**Check:**

- `check-docs.py`.
- These fixtures fail:
  - a `blob/main` link in a crate README;
  - a pinned link to a missing path;
  - a pinned link to a missing anchor;
  - a `blob/main` Cargo `documentation` link.

### G10 — L12: the fuzz-stop issue set is hard-coded

**Fix:**

- Add a `fuzz_stop_targets` list field to the entries in `docs/known-limitations.json` (#6 and #11 today).
- The QUALIFICATION and FUZZING fuzz-stop rules derive the stop set from it.
- **A3:** the field is added to the exact-key schema check, as an optional key with a list-of-known-targets value. The check is not loosened otherwise, and an unknown target fails.

**Check:**

- Fixtures where the JSON names a stop the docs omit fail.
- Fixtures where the docs name a stop the JSON lacks fail.
- A fixture with an unknown `fuzz_stop_targets` value (not in `scripts/fuzz.py` `TARGETS`) fails.

### G11 — process jargon in published crate sources (pass-3 observation)

**Problem:** 26 `Task N` references in comments and Rustdoc of the six published crates' `src/` ship in the immutable 0.1.0 archives, and the ones on public items render on docs.rs. Examples: `crates/core/src/schema/mod.rs:130`, `crates/core/src/text/mod.rs:251`, `crates/host/src/tools/witness.rs:6`.

**Fix:**

- Reword each reference to name the feature it means, for example "the idn-hostname format validator" or "the derivative pattern engine".
- `crates/core/src/text/idna_data.rs` is generated: change the template in `scripts/generate-unicode.py` and regenerate it through the repository's normal generator. Its registered digest updates by that same means, not by hand.
- **P4-3:** also reword the `T5/F9` process tag at `crates/core/src/grammar/codec/xml.rs:1300`. The rule pattern is not widened to catch tags like that, to avoid false positives.
- **P4-2, scope boundary:** the change covers `src/` only, because only `src/` renders on docs.rs and defines the public API's documentation. Comments in `tests/`, `benches/` and `examples/` also ship in the archives but aren't rendered, and they stay as they are. This boundary is stated here so it isn't a silent cut.
- Algorithm stage names such as "stage 1 (canonicalize)"–"stage 5 (generation)" of the Attouche et al. pipeline are domain terminology and stay.
- **P4-4:** the rule lives in `check-docs.py`, whose docstring is extended to say it also scans published Rust sources. It shares one `\btask \d+\b` pattern constant with the Markdown ban, so the two can't drift.
- `check-docs.py` gains a `source-jargon` rule: no `\btask \d+\b` (case-insensitive) in `crates/<published>/src/**/*.rs`, with the published crates derived from `cargo metadata`. It has failing fixtures.

**Check:**

- `git grep -nE '\b[Tt]ask [0-9]+' -- 'crates/{core,derive,syntax,purrassist,host,cli}/src'` is empty.
- `python3 scripts/check-docs.py` and `python3 scripts/check-repository.py` pass (generated-data digests).
- **P4-1, a regeneration check that can fail:** after the edits, rerun `python3 scripts/generate-unicode.py` with its pinned inputs and require `git diff --exit-code crates/core/src vendor/unicode` to be clean against the staged result.
- The commit's diff of generated data touches only the header comment and `reason` line of `idna_data.rs`, plus that file's digest in `vendor/unicode/generated.json`. Both `unicode_data.rs` files are unchanged.
- `cargo clippy -p purrassist-core -p purrassist-host --all-targets -- -D warnings` and `cargo doc -p purrassist-core --no-deps` pass.

**Commit:** a fourth commit, after the checker commit.

### Not a branch gap

The social-preview upload is the owner step defined by the plan. It stays listed in the PR as not done.

## Commit boundaries

Boundaries are regrouped so that each commit owns a coherent file set. The checker rules for all gaps live in the same two scripts.

1. **Doc text for G1 (sentence removal) and G3:** release body and wording.
2. **Doc text for G4 and G9:** jargon rewording and version-pinned crate links.
3. **Checker changes for G1 and G2 (rules plus the A5 move), G3 (A6), G4, G5, G6, G7, G8 and G10, with fixtures.**

4. **G11:** source jargon, regenerated data, and the `source-jargon` rule.

Commit 4 is G11. Commits 1 and 2 are already landed: `4933cc4` (G1 sentence removal and the G3 text) and `d6fb1e0` (G4 rewording and G9 links). They contain every rewording that commit 3's bans depend on. Each commit is pushed and summarized on the issue and the PR. After all four, `make check` and `make package` run once on the final head. The combined reviewer then rechecks and gives a final completion verdict.
