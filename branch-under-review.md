# Branch under review — issue #15

Branch `paudley/15-purrassist-derive-zero-dependency` against `origin/main`. Gathered by `stagectl brief
--mode review`; every hash and path came from this repository.

## Commits (6)

- `e4850a806` feat(derive): variant descriptions compose into the enum description (#15)  _2026-10-10_
- `d2410edc2` docs(purrassist): drop a redundant intra-doc link target (#15)  _2026-10-10_
- `534f862e3` build(release): register purrassist-derive for packaging, audit and docs (#15)  _2026-10-10_
- `4085bf31a` feat(purrassist): re-export the tool derives through the facade (#15)  _2026-10-10_
- `e97a97367` feat(derive): zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)] (#15)  _2026-10-10_
- `0fb045418` feat(tool-call): core support for derived tool definitions (#15)  _2026-10-10_

## Files changed

```
.github/workflows/publish.yml                     |    4 +-
 .github/workflows/verify.yml                      |    1 +
 Cargo.lock                                        |    8 +
 Cargo.toml                                        |    3 +-
 README.md                                         |   41 +-
 crates/core/src/tool_call/derive_support.rs       |  761 ++++++++++++
 crates/core/src/tool_call/from_node.rs            |   76 +-
 crates/core/src/tool_call/mod.rs                  |    7 +
 crates/core/src/tool_call/toolset.rs              |   16 +-
 crates/{purrassist => derive}/Cargo.toml          |   16 +-
 crates/derive/LICENSE-APACHE                      |  202 +++
 crates/derive/LICENSE-MIT                         |   21 +
 crates/derive/LICENSE-MULAN                       |  130 ++
 crates/derive/README.md                           |   53 +
 crates/derive/src/bridge.rs                       |  149 +++
 crates/derive/src/lib.rs                          | 1238 +++++++++++++++++++
 crates/derive/src/model.rs                        |  905 ++++++++++++++
 crates/derive/src/parse.rs                        | 1214 ++++++++++++++++++
 crates/derive/src/write.rs                        |  707 +++++++++++
 crates/derive/tests/derive.rs                     | 1370 +++++++++++++++++++++
 crates/host/tests/derive_facade.rs                |   38 +
 crates/purrassist/Cargo.toml                      |    1 +
 crates/purrassist/README.md                       |   34 +
 crates/purrassist/src/lib.rs                      |   61 +
 crates/purrassist/tests/derive_facade.rs          |   93 ++
 docs/DEPENDENCIES.md                              |   10 +
 docs/IMPLEMENTATION.md                            |   15 +-
 docs/QUALIFICATION.md                             |   12 +-
 docs/RELEASE.md                                   |   43 +-
 docs/RESOURCES.md                                 |    2 +-
 docs/tool-calls/INTEGRATION.md                    |   34 +
 docs/tool-calls/README.md                         |   11 +-
 docs/tool-calls/dialects/anthropic-tool-use.v1.md |    2 +-
 scripts/check-packages.py                         |    8 +-
 scripts/check-release.py                          |    1 +
 scripts/check-repository.py                       |   34 +-
 scripts/package-workspace.py                      |    2 +-
 scripts/test-package-workspace.py                 |   11 +-
 scripts/test-repository-audit.py                  |   59 +
 scripts/test-verify-published.py                  |   75 +-
 scripts/verify-published.py                       |    2 +-
 vendor/dependencies.json                          |    1 +
 42 files changed, 7379 insertions(+), 92 deletions(-)
```

## ADRs the changed files cite

None of the changed files cite an ADR.

These are the settled decisions this branch touches. The question
for stage2 is whether the change fits them, not merely whether it
compiles.

