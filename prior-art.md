# Prior art — issue #26: CI: lint the whole workspace with the release toolchain on pull requests

Assembled by `stagectl brief`. Every hash and path below came from this
repository; nothing here is recalled or inferred. Read the Coverage
section at the bottom before concluding anything is absent — an empty
section can mean 'nothing found' or 'could not look'.

## Governing ADRs

The issue cites no ADRs.

## Linked issues (2-hop `#N` crawl)

- #24: test(core): compare the double-rounding result by bits  [MERGED]  (mentions)
- #26: CI: lint the whole workspace with the release toolchain on pull requests  [OPEN]  (closes)
- #1: Complete PurRAssist 0.1.0 plan and release qualification  [CLOSED]  (mentions)
- #28: ci: one release gate for pull requests and release qualification  [OPEN]  (mentions)
- #29: probe(#26): deliberate float_cmp failure, do not merge  [CLOSED]  (mentions)

## Prior attempts at this issue

Commits citing it (including inside squash merges):

- None found.

Other branches named for it:

- `origin/probe/26-float-cmp`

PRs citing it:

- #29: probe(#26): deliberate float_cmp failure, do not merge  [CLOSED]
- #28: ci: one release gate for pull requests and release qualification  [OPEN]  (this stage's own PR)

## Related items on the forge

- #4: Tool-call recovery, part 2: streaming recognizer, remaining dialects, purrassist-derive  [CLOSED]
- #1: Complete PurRAssist 0.1.0 plan and release qualification  [CLOSED]
- #3: Tool-call recovery: typed tool definitions, multi-dialect parsing, schema-guided repair, streaming  [CLOSED]

## Recurrence

Counted from `GhpRsq-Defect-Class` trailers — these are tallies, not estimates:

- `none`: 6 prior instance(s)
- `documentation-drift`: 2 prior instance(s)
- `performance`: 1 prior instance(s)

## Coverage

What was searched, and what was not:

- issue #26 + 3 comment(s) via github
- link crawl: 5 issue(s) within 2 hop(s) of `#N` references
- forge search on ['whole', 'workspace', 'release']: 3 other item(s)
- GhpRsq-Defect-Class trailers: 3 distinct class(es) in the last 200 commits
- prior attempts: 0 commit(s) citing #26 (second parents walked), 1 other branch(es) named for it
- PRs citing #26: 2
- requirement candidates: 22 line(s); 0 comment(s) matched no rule
- referenced artifacts: 30 path(s) (16 on disk), 26 identifier(s), 3 workspace package(s), 4 URL(s)
