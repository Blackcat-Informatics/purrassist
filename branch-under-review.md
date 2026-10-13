# Branch under review — issue #26

Branch `paudley/26-lint-the-whole-workspace-with-the` against `origin/main`. Gathered by `stagectl brief
--mode review`; every hash and path came from this repository.

## Commits (2)

- `fdd10ffcf` docs(qualification): make fuzz-hosted is not part of the release gate (#26)  _2026-10-13_
- `221284490` ci: one release gate for pull requests and release qualification (#26)  _2026-10-13_

## Files changed

```
.github/workflows/release-qualification.yml |  12 +-
 .github/workflows/verify.yml                |  12 +-
 Makefile                                    |  15 +-
 docs/QUALIFICATION.md                       |  26 ++--
 docs/RELEASE.md                             |  29 +++-
 scripts/check-release.py                    | 233 ++++++++++++++++++++++++++++
 scripts/test-check-docs.py                  |   2 +-
 scripts/test-check-release.py               | 170 ++++++++++++++++++++
 8 files changed, 466 insertions(+), 33 deletions(-)
```

## ADRs the changed files cite

None of the changed files cite an ADR.

These are the settled decisions this branch touches. The question
for stage2 is whether the change fits them, not merely whether it
compiles.

