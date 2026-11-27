# Issue #302: Move the purrdf crates to =5

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

## Result

The four purrdf crates this repository names move from `=4` to `=5` (5.0.0 on the registry) in one change on main.

## What moves

Read at main ad37d09a: `purrdf-core`, `purrdf-lex`, `purrdf-rdf` and `purrdf-shapes` at `=4` in the root manifest.

## Why now

purrdf 5.0 changes every canonical document and digest (purrdf 1383). The repositories that pin this one by revision move to purrdf `=5` in one event, and they need a revision of this repository on `=5` first, so that each lock file holds one purrdf major. #301 says to move no pin alone; this issue is the announcement of that move.

## In the same change

Any stored canonical identifier or frozen digest in a fixture is recomputed from source data in the 5.0 default form. The profile 5.0 keeps for the old form only verifies a stored value before it is recomputed.

## Acceptance

- Main names `=5` for the four crates, and the lock file holds one purrdf major.
- The commit that carries the move is posted here.


## Comments (1)

### paudley — 2026-11-22T18:29:45Z

Existing issues here on the same ground, read by title and opening text. This issue does not replace them: where one already carries a part, the work is done there.

- #221 (register `purrdf-text` 4.0.2 as a direct dependency) names `=4`. It moves to `=5` with the four crates here, or lands after this change.


