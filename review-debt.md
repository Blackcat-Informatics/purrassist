# Review debt: PR #20 (head 93c02b3, issue #1)

Nothing was posted. State read on 2026-10-12 from `raw/pr-20-merge*.json` and re-checked live
with `gh` (PR open, not draft, `reviewDecision` empty, head `93c02b3a5836f06b15335e428b8cd1a703cca6db`).

## Ruleset facts (verified by API)

`GET /repos/Blackcat-Informatics/purrassist/rules/branches/main` returns: `deletion`,
`non_fast_forward`, `required_linear_history`, `required_signatures`, `pull_request`. There is
no `required_status_checks` rule, no required approvals and no required thread resolution.
Hosted CI is out of scope for this stage by owner decision (recorded in `validation.md`).

## 1. Feedback inventory

Counts: 4 PR comments, 0 review submissions (`raw/pr-20-merge-reviews.json` is `[]`;
`gh pr view` shows 0 reviews), 0 inline comments (`raw/pr-20-merge-inline.json` is `[]`;
`gh api pulls/20/comments` returns 0), 0 review threads (`raw/pr-20-merge-threads.json`
has an empty `nodes` list). No bot authored anything. All four comments are by the owner
(`paudley`) and are status or handoff text, not requests to change the PR.

| # | Item | Author | Disposition | Fix and evidence |
|---|---|---|---|---|
| C1 | PR comment 2026-10-12T19:39:18Z, "Plan: release-ready 0.1.0 documentation..." (revision 4 plan with review dispositions) | paudley | Addressed (no change requested) | Plan record. Its tasks 1-6 landed as `dbdc64d`, `871af51`, `f11912e`, `f0d978a`, `a42a817`. Lens-review dispositions are in its "Review dispositions" section. Stage 2 verdict `gap-analysis.md` ends `VERDICT: PASS`. |
| C2 | PR comment 2026-10-12T19:39:21Z, "Handoff: what I'm least confident about" | paudley | Addressed (no change requested) | Informational. Three confidence items, none requested a code change. (1) `mcp-name` namespace: needs a real registry submission after publication, which is a post-stage owner step. (2) TOOLS.md narrative: Stage 2 claim-accuracy review (`gap-analysis.md` section (e)) rechecked the claims, all fixed. (3) Social-preview PNG reproducibility: pinned in `vendor/tooling.json` (`rsvg-convert` 2.63.2, `noto-sans` 2.015) and enforced by `check-docs.py`. Owner steps listed there (trusted publisher, preview upload, tag authorization) are outside this stage and are not review debt. |
| C3 | PR comment 2026-10-12T21:17:52Z, "Stage 2 remediation: units 1-3 pushed" | paudley | Addressed | Commits `4933cc4`, `d6fb1e0`, `feefaae`. Units 1-3 recheck: `VERDICT (units 1-3): PASS` in `gap-analysis.md`. |
| C4 | PR comment 2026-10-12T21:45:19Z, "Stage 2 complete for PR #20: ready for Stage 3" | paudley | Addressed | Adds `93c02b3` (G11). `validation.md` "Stage 2 final gate": `make check` exit 0 (110 `test result: ok` groups) and `make package` exit 0 ("Verified 6 release archives") on `93c02b3`. Final `gap-analysis.md` verdict: PASS. |
| R | Review submissions | none | n/a | 0 present. |
| I | Inline comments | none | n/a | 0 present. |
| T | Review threads | none | n/a | 0 present, so none unresolved. |

Nothing is legitimately declined, because nothing was requested.

## 2. Checks in `pr-checks.txt`

Run `38311982068` on head `93c02b3`. Ruleset has no required status checks, so no check is
required.

| Check | State | Required by ruleset | Caused by this branch's diff |
|---|---|---|---|
| native (macos-14) | SUCCESS | No | n/a |
| native (ubuntu-24.04) | SUCCESS | No | n/a |
| msrv | SUCCESS | No | n/a |
| windows-engines | FAILURE | No | No (see below) |
| wasm | SUCCESS | No | n/a |
| fuzz | FAILURE | No | No (see below) |
| corpus | SUCCESS | No | n/a |
| qualification | SUCCESS | No | n/a |

### windows-engines: not caused by this branch

Failing log lines (`gh run view 38311982068 --log-failed`): clippy `first doc comment
paragraph is too long` (`too_long_first_doc_paragraph`) at `crates\core\src\grammar\cost.rs:93`,
`crates\core\src\tool_call\emit.rs:1130` and `crates\core\src\tool_call\ir.rs:600`, then
"could not compile `purrassist-core` (lib) due to 3 previous errors".

- `git diff --stat f11b45e..93c02b3 -- crates/core/src/grammar/cost.rs crates/core/src/tool_call scripts/fuzz.py`
  lists only `scripts/fuzz.py` (33 insertions, 33 deletions). `cost.rs` and `tool_call/` are
  untouched. `f11b45e` is the merge-base with `origin/main`.
- The flagged doc comments come from earlier merges (last commits to `cost.rs`: `8fb63a4`,
  `3813065`, `3961333`, all on main before this branch).
- The branch's 15 changed `crates/core/src` files (G11 comment rewording in `93c02b3`) do not
  include those three files.
- Pre-existing latent lint on main's code, surfaced by the Windows job's clippy.

### fuzz: not caused by this branch

Failing log lines: `ValueError: node version differs from inventory 26.11.1`, raised at
`raise ValueError(f'node version differs from inventory {node_expected}')`, then
`make: *** [Makefile:63: fuzz] Error 1`.

`scripts/fuzz.py` did change in `feefaae` (explicit encoding only). Judgement:

- The diff is 33 mechanical substitutions: `text=True` to `encoding='utf-8'`, and
  `read_text()`/`write_text()` gaining `encoding='utf-8'`. No comparison, version string or
  control flow changed.
- The raising check is `node_version != f'v{node_expected}'` where `node_version` is
  `check_output(['node','--version'], encoding='utf-8').strip()`. Decoding `v26.11.1\n` as
  UTF-8 versus the locale default yields the same ASCII string, so the substitution cannot
  change the result. The `node` entry in `vendor/tooling.json` (`26.11.1`) is not in the
  branch diff (the branch only adds the `rsvg-convert` and `noto-sans` entries).
- Locally, `node --version` prints `v26.11.1`, so the check passes with the branch's code.
- In the same job, the step `python3 -X warn_default_encoding -W error::EncodingWarning
  scripts/test-fuzz.py` ran at 21:39:38 and the job failed only later at 21:47:53 inside
  `make fuzz`, so the encoding change did not break the fuzz tests.
- `.github/workflows/verify.yml` (not in the branch diff) has no pinned-Node install step in
  the `fuzz` job, unlike the `corpus`/`qualification`/`wasm` jobs, which run
  `install-qualification-tool.py node`. The runner's own Node therefore need not equal the
  pinned version. This is a pre-existing workflow gap, independent of this branch.

Both failures are recorded in `validation.md` ("Hosted CI observation") and not acted on,
per the owner decision that hosted CI is out of scope for this work.

## 3. Open required feedback and required checks

- Open review threads: 0. Unanswered requested changes: 0. Required approvals: 0 (rule has none; none outstanding).
- Required status checks: none exist in the ruleset, so the two failing jobs do not block merge.
- Merge-state note: `mergeStateStatus` reads `UNSTABLE` only because of the two non-required
  failing jobs. It is not `BLOCKED`.
- Squash merge, signed commits and linear history are ruleset requirements for Stage 3 and are
  not review debt.

No blocking review debt.

VERDICT: PASS
