# Branch under review — issue #1

Branch `paudley/1-complete-purrassist-0-1-0-plan-and` against `origin/main` (f11b45e). Written by hand with the same git commands as `stagectl brief --mode review`, which crashed: its ADR scan decodes `git show HEAD:docs/social-preview.png` as UTF-8 (stagectl/brief.py:76). This repository has no ADRs (`adr_repo: false`).

## Commits (5)

- `a42a817` docs(release): describe the release set without claiming publication (#1)  _2026-10-12_
- `f0d978a` docs(assets): reproducible 1280x640 social preview for the repository (#1)  _2026-10-12_
- `f11912e` docs(release): 0.1.0 changelog, current release procedure and security policy (#1)  _2026-10-12_
- `871af51` docs(readme): one engine, three interfaces, and a registry-pinned tool reference (#1)  _2026-10-12_
- `dbdc64d` docs(release): source restated documentation facts and disclose known limits (#1)  _2026-10-12_

## Files changed

```
 CHANGELOG.md                                       |  115 ++
 CONTRIBUTING.md                                    |    3 +-
 Makefile                                           |    2 +
 README.md                                          |  487 ++++----
 SECURITY.md                                        |   25 +-
 crates/cli/Cargo.toml                              |    8 +-
 crates/cli/README.md                               |   21 +-
 crates/cli/tests/protocol.rs                       |  450 +++++++
 crates/core/README.md                              |    6 +-
 crates/core/tests/tool_call_dialects.rs            |   27 +
 crates/derive/README.md                            |    8 +-
 crates/host/Cargo.toml                             |    2 +-
 crates/host/README.md                              |   11 +-
 crates/purrassist/Cargo.toml                       |    2 +-
 crates/purrassist/README.md                        |   33 +-
 crates/syntax/README.md                            |    4 +
 docs/DEPENDENCIES.md                               |   12 +
 docs/FUZZING.md                                    |   42 +-
 docs/IMPLEMENTATION.md                             |   20 +-
 docs/QUALIFICATION.md                              |   60 +-
 docs/RELEASE.md                                    |  170 ++-
 docs/RESOURCES.md                                  |    2 +-
 docs/TOOLS.md                                      |  875 ++++++++++++++
 docs/assets.json                                   |   53 +
 docs/known-limitations.json                        |   66 ++
 docs/social-preview.png                            |  Bin 0 -> 112623 bytes
 docs/social-preview.png.license                    |    2 +
 docs/social-preview.svg                            |  128 ++
 scripts/check-docs.py                              | 1111 +++++++++++++++++
 scripts/check-repository.py                        |  128 +-
 scripts/test-check-docs.py                         | 1247 ++++++++++++++++++++
 scripts/test-repository-audit.py                   |  164 ++-
 .../licenses/librsvg-2.63.2/LGPL-2.1-or-later.txt  |  176 +++
 vendor/licenses/noto-sans-2.015/OFL.txt            |   93 ++
 vendor/tooling.json                                |   22 +
 35 files changed, 5109 insertions(+), 466 deletions(-)
```

## ADRs the changed files cite

None: the repository has no ADRs.
