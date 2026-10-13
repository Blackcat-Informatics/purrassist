# Merge-plan review: PR #23 (issue #1), head 04be6e0, base main 76d5e73

Reviewed plan/notes: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and.squash_notes`
Evidence read: `.stage/complete-purrassist-0-1-0-plan-and/validation.md`, `docs/RELEASE.md`,
`scripts/check-docs.py`, `scripts/test-check-docs.py` (head), `CHANGELOG.md` (head),
`Makefile`, `AGENTS.md`.

## How this was verified (no shell)

I had no shell, so I did not run `git diff origin/main...HEAD`. I rebuilt the diff by
comparing the worktree files at head 04be6e0 with the main checkout at
`/home/paudley/Active/purrassist` (`.git/HEAD` -> `refs/heads/main`). The result matches the
stat in the squash notes (2 files, +13/-6):

- `CHANGELOG.md:14`: `## [0.1.0] - unreleased` -> `## [0.1.0] - 2026-10-13`. Nothing else in
  the file changed. The `## [Unreleased]` / `No changes yet.` section above it is still there.
- `scripts/test-check-docs.py`:
  - new module constant `CHANGELOG_HEADING` (lines 56-59), read from the checkout's
    `CHANGELOG.md` with `(?m)^## \[0\.1\.0\] - .+$`;
  - the five fixture lines that replaced the literal `'## [0.1.0] - unreleased'` (base lines
    911, 923, 980, 985) now replace `CHANGELOG_HEADING` (head lines 917, 929, 986, 991);
  - `test_the_heading_carries_the_status_and_the_checkout_body_is_clean` used to assert
    `^## \[0\.1\.0\] - unreleased$`. It now asserts
    `^## \[0\.1\.0\] - (unreleased|\d{4}-\d{2}-\d{2})$` on the checkout heading, with a
    one-line comment.
- `scripts/check-docs.py` did not change.

I did not re-run the validation results (`check-docs.py` OK, 252 tests OK, `make hygiene`
exit 0). They come from validation.md and the notes.

## Coverage of the caller's focus

1. **Is step 1 correct and complete?** Yes. RELEASE.md step 1 says: "Set the date of the
   `[0.1.0]` heading in `CHANGELOG.md` (it reads `unreleased` until now) to the tagging date
   and merge that change". The PR does exactly that.
   - **Nothing else has to change in the tagged commit.**
     - The five "not yet tagged `rust-v0.1.0`" status lines (README, SECURITY,
       IMPLEMENTATION, QUALIFICATION, RELEASE) change only under "After publication". They
       stay true on the tagged commit, and `check_workspace_version` rejects any mix of the
       two forms.
     - A grep of the tree outside `.stage/` for `unreleased`, `tagging date` and
       `2026-10-13` finds no other document or script that restates the 0.1.0 status or date.
     - Among the scripts, workflows and the Makefile, only `check-docs.py` and
       `test-check-docs.py` read `CHANGELOG.md`, and `check_changelog` does not parse the
       heading date.
2. **The test changes do not weaken any check.**
   - `RELEASE_BODY_BANNED = (r'not yet tagged', r'unreleased', r'published in this order')` is
     unchanged, and `check_changelog` still applies it to the section body below the heading.
   - `test_the_release_body_must_not_say_unreleased` is unchanged and still expects the
     `'Unreleased'` failure. `test_another_section_may_say_unreleased` and
     `test_a_missing_unreleased_heading_still_needs_the_version` are also unchanged.
   - The fixtures still exercise their rules because `Tree.replace` -> `Tree.sub` raises
     `AssertionError` unless the old text occurs exactly once (`count=1`). If
     `CHANGELOG_HEADING` drifts, the test fails loudly instead of passing as a no-op.
   - The old status assertion was intentionally tied to `unreleased`, so it had to relax for
     the release commit. It still pins the exact heading shape, anchored at both ends.
3. **The notes are honest.**
   - They say `Refs #1` and "Issue #1 stays open until publication, downloaded-package
     acceptance and the GitHub release". There is no closing keyword (close/fix/resolve).
   - There is no co-author trailer, agent credit, or agent-vendor email address, and no
     `GhpRsq`-prefixed trailer. The only trailer is `Defect-Class: none`, which I took to be
     the pipeline's defect-class trailer rather than a GhpRsq trailer.
   - There is no wording about hosted-CI accounts or payments. "hosted Verify" names the
     `verify.yml` workflow.
   - The claims match the evidence. The file stat matches the reconstruction, and "merge-tree
     tree equals the head tree" holds trivially because 76d5e73 is the parent of 04be6e0.
     "Release step 2 runs on the merge commit before tagging" is consistent with RELEASE.md
     step 2.

## Findings

No BLOCKING findings.

**ADVISORY 1: the date is a forecast until the tag exists.** The notes say:
> "now carries the tagging date `2026-10-13` instead of `unreleased`"

RELEASE.md requires "the tagging date". Step 2 (hosted Verify, the local QUALIFICATION.md
commands, local `make fuzz-hosted`, and the #6 base-commit replay) and step 3 (re-confirming
trusted publishers) must finish first. If they slip past 2026-10-13 UTC, the release body will
carry a wrong date. Correcting it would take a new commit, which changes the commit that every
step-2 check names.
REWRITE TO (optional, in the notes): "now carries the planned tagging date `2026-10-13` (UTC)
instead of `unreleased`; if the tag is not created on that date, the heading is re-dated and
step 2 is repeated on the new commit before tagging."

**ADVISORY 2: blank-line layout around the new constant.** In `scripts/test-check-docs.py` at
head, three blank lines (53-55) come before the `CHANGELOG_HEADING` comment and only one blank
line (60) comes before `class Tree:`. That differs from the file's two-blank-line convention
(PEP 8 E303/E302). No linter in `make check` catches it, so it is cosmetic.
REWRITE TO: two blank lines before the comment block and two before `class Tree:`.

**ADVISORY 3: a stale comment in check-docs.py, outside the diff.** `scripts/check-docs.py:104-105`
says "The `- unreleased` in its heading is the status and is not part of the body." After this
PR the heading carries a date.
REWRITE TO: "The status or date in its heading (`- unreleased`, then `- YYYY-MM-DD`) is not
part of the body." This is optional, and so is a follow-up. It does not affect the tag.

(Observation only, not a finding, and not required by RELEASE.md: the rustdoc of
`RECEIPT_VERSION` at `crates/core/src/contract.rs:86-88` says "Unreleased addition within
version `2` (no new version number, since nothing has shipped yet)". That is accurate as
history, but it will render on docs.rs for the published 0.1.0. It is outside step 1's scope.)

VERDICT: PASS
