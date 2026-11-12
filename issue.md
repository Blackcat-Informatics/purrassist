# Issue #181: Main hygiene is red since #174: four vendored PurRDF notices have no attribution

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

Main's `make check` hygiene step fails at 961b824 (merge of #174) with four errors from `scripts/check-repository.py`:

    vendor/licenses/purrdf-4.0.2/third-party/csvw/1-LICENSE.md: missing attribution
    vendor/licenses/purrdf-4.0.2/third-party/jsonld-w3c/4-LICENSE.md: missing attribution
    vendor/licenses/purrdf-4.0.2/third-party/rdfc/1-LICENSE.md: missing attribution
    vendor/licenses/purrdf-4.0.2/third-party/shacl/1-LICENSE.md: missing attribution

Evidence: the four upstream notices were added by #174 (commit 43da578); they are absent at b9c3d8c, the commit whose hosted Ubuntu job (run 43237133448) passed the checker. Local run of the checker on a detached copy of origin/main at 961b824 exits 1 with these four lines.

Cause: the checker requires SPDX headers on tracked Markdown, which upstream W3C/MIT notice text cannot carry without altering its bytes and its pinned digest.

Acceptance: a REUSE-style `<name>.license` sidecar beside each of the four files carrying the SPDX tags, identifiers read from the upstream text; the checker reads it; a test shows that a vendored licence with no sidecar still fails; `check-repository.py` and `test-repository-audit.py` pass on main.


## Comments (0)

