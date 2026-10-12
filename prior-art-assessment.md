# Prior-art assessment: issue #1, release-ready 0.1.0 documentation

Base: `f11b45e` (origin/main, merge of #19 / issue #15). Sources: local `git log`
and `git show`, the working tree at that base, and the already-gathered
`issue.md`, `prior-art.md` and `brief.json` in this directory. The forge was not
refetched. "UNKNOWN" means local evidence cannot settle the question.

Session scope (user decision): make the release docs, `README.md` and the other
docs accurately describe a release-ready 0.1.0. No tagging and no publishing.
Hosted CI is out of scope. Issues #6, #7, #10 and #11 do not block 0.1.0. The
`purrassist-derive` 0.0.0 name reservation was published this session; this
assessment takes that from the session record and did not re-verify it.

## 1. History: merges that changed the documented facts

| Merge | PR / issue | Effect on documented facts |
| --- | --- | --- |
| `699d9ff` | #2 / #1 | Issue #1 implementation. Five crates, eight fuzz targets. The Stage 2 record was archived at `34fb2ac`. |
| `943fa04` | #5 / #3 | Tool-call recovery and twelve dialects. Receipt wire v3 (`227d4eb`). Ninth fuzz target `tool-call` (`8496413`). |
| `fccfbe7` | #9 / #8 | Tenth fuzz target `tool-call-ref` (`8bca69e`). Compatibility statement (`2484700`). |
| `f11b45e` | #19 / #15 | Sixth release crate `purrassist-derive`. Release order is now core, derive, syntax, purrassist, host, cli. Publish preflight added (`b7475b4`). |

The PR #2 stage-3 comment in `issue.md` (2026-09-30T21:26:55Z) records **five**
verified archives. That receipt table exists only in the issue comment, not in
`docs/`.

## 2. Precedent for doc and release-record updates

### Dedicated reconciliation pass (issue #1, Task 19)

The plan's Task 19 (`issue.md` line 463) said: update QUALIFICATION,
IMPLEMENTATION, README, RESOURCES, FUZZING, HELPERS, EDIT-APPLICATION and
CANCELLATION. Its acceptance was a one-time
`grep -nE "remains open|not complete|still required|remain open" docs/QUALIFICATION.md`.

- `42b4460` (docs: reconcile implemented capabilities and qualification
  contracts) touched README, the core/host/purrassist/syntax READMEs,
  CANCELLATION, EDIT-APPLICATION, IMPLEMENTATION, QUALIFICATION (988 lines
  changed) and RELEASE. It:
  - replaced "Pre-release implementation in progress" status lines in README,
    IMPLEMENTATION and RELEASE;
  - replaced the stale `make fuzz-json` command with `make fuzz` ("eight
    coverage-guided targets");
  - removed QUALIFICATION's "Remaining release work" and "Markdown
    qualification in progress" sections;
  - added the "Capability and evidence" table and the "Release acceptance
    after review and merge" section.
- An independent review then corrected overclaims:
  - `b08161e` (SECURITY.md: two production plus four qualification-only
    `unsafe` exceptions, not "two workspace-wide"; RESOURCES.md: the allocation
    oracle covers the listed representative operations, not "every governed
    public operation");
  - `856d2ec` (RESOURCES.md: fixed slack `K` does not prove scale invariance;
    pattern docs: bounded search does not prove impossibility).

  `issue.md` (2026-09-30T08:43:56Z) records both as "three evidence/documentation
  overclaims".
- `34fb2ac` (docs(qualification): archive verified Stage 2 local evidence)
  added the `docs/qualification/*-stage2-*.json` reports and the QUALIFICATION
  "Stage 2 local integration record" section.

### Docs updated inside feature commits (the default habit)

- During #2, almost every `feat:` or `fix:` commit edited README and
  QUALIFICATION alongside the code: all 37 non-merge commits that touch README
  before `42b4460` are `feat`/`fix`/`test`/`build`, and none is `docs`.
- #3/#5:
  - each dialect commit (`1ba176c` … `b3e16ab`, nine commits) edited README,
    IMPLEMENTATION and QUALIFICATION lists;
  - `cfe9366` added `docs/tool-calls/` and its tests;
  - `ce66b7b` tightened the docs and fixed stale lists;
  - `5398ba1` fixed "the README's stale finish_reason sentence (F8) and every
    other stale statement".
- #8: `8bca69e` updated `docs/FUZZING.md` from "nine" to "ten" targets and the
  Makefile loop. It did not update README or QUALIFICATION. `2484700` added the
  INTEGRATION compatibility section.
- #15:
  - `534f862` (build(release): register purrassist-derive …): "The docs
    describe the six-crate release and its order". It also added the
    "derive's crates.io name reservation and trusted publisher are not yet in
    place" text to RELEASE.md and repointed references to the closed #4.
  - `b7475b4` documented the preflight.
  - `d0a99e9` added the README derive-example drift check.
  - `6cbb628`, `4d8a48e` and `637018b` are successive corrections of the same
    near-miss rule text.
  - `8d8d9a2` fixed the `publish.yml` header comment, which "still listed the
    five-crate order".

Conventions to follow:

- Subjects use `docs:` or `docs(<scope>):` with an `(#N)` suffix.
- There is one topic per commit, and the body names each file and the claim
  that changed.
- Overclaim fixes say what the evidence does *not* establish (`b08161e`,
  `856d2ec`).

## 3. Machine checks that guard the docs (state at `f11b45e`)

| Check | What it pins | In `make check`? |
| --- | --- | --- |
| `scripts/check-release.py` | `RELEASE_ORDER` equals the publishable workspace. Versions match. Internal requirements are `^version`. Dependencies precede their users. `publish.yml` shape: trigger, environment, permissions, SHA pins, the `for crate in …` loop order, preflight position and comment. It reads **no** Markdown; RELEASE.md appears only in a docstring (line 47). | Yes (`hygiene`) |
| `scripts/check-repository.py::check_derive_example` | Text between the `derive-example` markers in `README.md` equals `crates/purrassist/README.md` byte for byte (`d0a99e9`). This is the only README prose check. | Yes |
| `scripts/check-repository.py` SPDX scan | Only `crates/` and `scripts/` files with `.rs .py .js .mjs .toml .sh` suffixes. `.md` is **not** checked (all tracked `.md` files currently carry SPDX anyway). | Yes |
| `scripts/test-fuzz.py` (line ~247) | `docs/FUZZING.md` contains `tool-call-ref`, `--tools ref` and `--continue-on-known`. The Makefile loop lists `tool-call-ref`. No count is checked, and README and QUALIFICATION are not read. | Yes |
| `scripts/check-resource-limits.py` | The `## Resource limits` JSON fence in RESOURCES.md is the single source of the limits. `make check` runs it `--schema-only` on `resources-linux-x86_64.json`. | Yes (schema only) |
| `crates/core/tests/tool_call_cost.rs` | RESOURCES.md "Tool-call recovery cost bound" text and constants. | Yes |
| `crates/core/tests/tool_call_rules.rs` | `docs/tool-calls/RULES.md` equals the rule registry. The GRAMMAR.md example compiles and runs (`cfe9366`). | Yes |
| `crates/core/tests/correction_evaluation.rs` | MATCHING-QUALITY.md count rows. | **No.** This is a `test = false` corpus suite (`make corpus`). |
| Rustdoc `-D warnings` and doctests | The core, host, purrassist and derive READMEs are included as crate docs via `include_str!`, so their examples run and their intra-doc links are checked. The syntax and cli READMEs are not doctested. | Yes |
| `test-verify-published.py` fixture freshness | Committed CLI/MCP transcripts (`tools/list` census is currently 31). | Yes |
| Relative Markdown link check | **None** in the Makefile, scripts or workflows. A stdlib scan this session found 0 broken relative links in 38 `.md` files. | No |

Gap: nothing checks any prose in `README.md` (outside the derive block),
`docs/RELEASE.md`, `docs/QUALIFICATION.md`, `docs/IMPLEMENTATION.md` or
`CONTRIBUTING.md`. Counts and lists repeated in those files are hand-maintained
copies of facts whose single source of truth is checked: `RELEASE_ORDER`,
`fuzz.TARGETS`, `RECEIPT_VERSION`, `DialectId::BUILT_IN`.

## 4. How the docs drifted anyway: stale statements at `f11b45e` (verified)

| Location | Claim | Fact | Stale since |
| --- | --- | --- | --- |
| `README.md:387` | `make fuzz` runs "nine coverage-guided targets" | `scripts/fuzz.py` `TARGETS` has 10, and so does the Makefile loop | `8bca69e` (#8) |
| `docs/QUALIFICATION.md:33` | "Nine targets (including `tool-call`)" | 10, including `tool-call-ref` | `8bca69e` (#8) |
| `docs/QUALIFICATION.md:23` | "receipt wire v2" | `RECEIPT_VERSION: u32 = 3` (`crates/core/src/contract.rs:101`) | `227d4eb` (#3/#5) |
| `docs/RELEASE.md:43-56, 216-218` | "`purrassist-derive` has no placeholder record yet … must be completed before any release tag" | Name reserved this session (session record, not re-verified here) | this session |
| `docs/RELEASE.md:83-87` | "Trusted publishing (configured for five of six crates)" | UNKNOWN whether derive's trusted publisher has been configured | this session |
| `docs/IMPLEMENTATION.md:5` | "accepted implementation delivered on the issue branch" | Merged to main in `699d9ff` and extended by `943fa04`, `fccfbe7` and `f11b45e` | `699d9ff` |
| `CONTRIBUTING.md:20-21` | "`docs/QUALIFICATION.md` separates local checks from remaining release work" | `42b4460` removed the "Remaining release work" section | `42b4460` |
| `docs/qualification/*.json` | Archived fuzz/resource evidence | Bound to commits up to `66b3756` (Stage 2). No archived report for the `tool-call`/`tool-call-ref` targets. The "Stage 2 local integration record" describes the pre-#3 tree. `recovery-bench-linux-x86_64.json` was refreshed at `d689c1a`. | `943fa04` onward |
| `crates/{core,host,cli,purrassist}/README.md` | "Pre-release implementation" | These files ship in the crate archives and on crates.io, so their wording needs a release-time decision | at publication |
| README / IMPLEMENTATION "Still tracked: #12–#14, #16–#18" | Open-issue references | Issue states UNKNOWN (forge not refetched) | UNKNOWN |

Every verified stale item is in prose that no machine check reads. None of the
machine-checked regions listed in §3 is currently stale.

## 5. Recurrence (counted from `git log`, non-merge commits)

- **`docs`-prefixed commits: 14 total.**
  - **7 correct stale, overclaimed or over-broad statements:** `42b4460`,
    `b08161e`, `856d2ec`, `ce66b7b`, `6cbb628`, `4d8a48e`, `637018b`.
  - **7 do not:** `cfe9366` (new guide), `2484700` (new section), `34fb2ac`,
    `ecd3974` and `b2c9cba` (evidence records), and `de8e8fd` and `d2410ed`
    (lint).
- **Commits outside the `docs` prefix whose message or diff shows a stale-doc
  fix: 4.**
  - `5398ba1`: stale finish_reason sentences.
  - `534f862`: references to the closed #4.
  - `8d8d9a2`: five-crate `publish.yml` header.
  - `cfe9366`: "eight" changed to "nine" targets, left stale by `8496413` on
    the same branch.
- **Total with explicit evidence: 11.** Doc corrections folded silently into
  other `feat`/`fix` commits: UNKNOWN.
- **Same fact drifting repeatedly:** the fuzz-target count appears in README,
  QUALIFICATION and FUZZING. It went stale twice:
  - `8496413` left it stale; `cfe9366` fixed it.
  - `8bca69e` updated only FUZZING; README and QUALIFICATION are still stale.
- **Same text corrected repeatedly within one PR:** the #15 near-miss rule was
  written in `6cbb628` and narrowed twice within a day (`4d8a48e`, `637018b`).
- **Defect-class trailers:** only 2 exist in history (`f11b45e` `none`,
  `fccfbe7` `performance`). There is no documentation class, so trailer-based
  recurrence for doc drift is not recorded.

## 6. Prior attempts and why they did not hold

1. **Task 19 was a snapshot, not a guard.** `42b4460`, `b08161e` and `856d2ec`
   made the docs accurate as of `34fb2ac` (five crates, eight targets, receipt
   v2). The Task 19 acceptance grep ran once and was never added to `make
   check`. Three later merges changed those facts.
2. **Feature PRs update the nearest doc, not the summary copies.** `8bca69e`
   updated FUZZING.md but not README or QUALIFICATION. `227d4eb` bumped the
   receipt version without touching QUALIFICATION's "v2".
3. **The checks pin sources of truth, not their copies in prose.**
   `check-release.py` forced `RELEASE_ORDER` and the `publish.yml` loop to
   include derive. The RELEASE.md prose was edited by hand in `534f862`, and
   the `publish.yml` header comment was missed until `8d8d9a2`.
4. **External state was written as a present-tense fact.** "has no placeholder
   record yet" and "configured for five of six" become false when an action
   happens outside the repo, and no commit or check notices. "On the issue
   branch" went stale the moment the branch merged.
5. **Evidence archives are dated but not marked as superseded.** The
   QUALIFICATION table now cites tool-call and derive tests by name. The
   archived reports and the "Stage 2 local integration record" still describe
   the `66b3756` tree, and nothing marks them as historical.

## 7. Recommendations: making this update hold

Keep the tooling Python stdlib-only and wired into `make hygiene`. The #1 plan
declined an `xtask`, and core stays dependency-free. Add a negative test for
each new check, following the pattern of `test-repository-audit.py` for
`check_derive_example`.

1. **Derive restated facts from their source and check them.**
   - **Release order and count.** Wrap each prose listing in RELEASE.md (and
     the "Six … archives" cell in QUALIFICATION) in marker comments, as
     `check_derive_example` does, or match a fixed sentence. Assert it equals
     `RELEASE_ORDER` / `len(RELEASE_ORDER)` in `check-release.py`. Also check
     the `publish.yml` step name and header comment.
   - **Fuzz targets.** Extend the `test-fuzz.py` test that already reads
     FUZZING.md to require the count word or list in README, QUALIFICATION and
     FUZZING to match `len(TARGETS)` and the Makefile loop.
   - **Receipt version.** Either check `receipt wire v<N>` against
     `RECEIPT_VERSION` in a Rust test (`include_str!`, as in
     `tool_call_rules.rs`) or drop the number and point to
     `docs/schema/receipt.schema.json`.
   - **Dialect lists** in README, IMPLEMENTATION, QUALIFICATION and the host
     README: check them against `DialectId::BUILT_IN` in a Rust test, as
     `ce66b7b` did for the CLI help.
2. **Restate less.** Where a count adds nothing, link to the authoritative doc
   instead ("every target in FUZZING.md"). Each removed copy is one fewer thing
   to check.
3. **Record external state as an observation.** Write registry reservations
   and trusted-publisher state as dated observations naming the API used
   (e.g. "observed 2026-10-12 via `GET /api/v1/crates/<name>/owners`"). Leave
   anything unverifiable (derive's trusted publisher) explicitly unconfirmed
   unless the user confirms it. The `publish.yml` preflight stays the
   authority for names and owners.
4. **Write status relative to durable anchors.** Use "merged to main" and "not
   yet tagged `rust-v0.1.0`", not "on the issue branch". Label the Stage 2
   record and the `docs/qualification/*` archives with their bound commit, and
   say that later merges are covered by the named tests and not by those
   archives.
5. **Add cheap hygiene checks.** Add `.md` to the SPDX scan in
   `check-repository.py`. Add a stdlib relative-link check over tracked `.md`
   files (0 broken today, so it can go in without fixes).
6. **Make the shipped crate READMEs a deliberate release-time decision.** The
   "Pre-release implementation" text in the core/host/cli/purrassist READMEs is
   published inside the archives.
7. **Optionally, make `correction_evaluation`'s doc assertions visible to the
   normal gate.** They currently run only under `make corpus`, so
   MATCHING-QUALITY counts drift silently under `make check`.
