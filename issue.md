# Issue #172: Pin the released purrdf crates at "=4"

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

purrdf 4.0.1 is released on crates.io, and gmeow-ontology now pins the registry purrdf crates at "=4" (main `7a9e9fc173`, #2672). rust-agentic-foundations is making the same move.

purrassist main (`ffee73d4`) still depends on purrdf-core and purrdf-shapes by git at `742bbbfbe` (3.x), and open PR #171 moves them to git `511f8af`, not to the release.

Request: depend on the registry purrdf crates at "=4" (lock resolving 4.0.1), with no git source for purrdf, so that downstream workspaces that also use gmeow-ontology resolve a single purrdf major.

Acceptance: `cargo tree -d` in a consumer using purrassist-core together with gmeow-ontology at "=4" shows no purrdf 3, and purrassist's own checks pass on 4.0.1.


## Comments (0)

