# Gap analysis: PR #21 (docs/RELEASE.md trusted-publisher record)

Issue: #1. Branch: `paudley/1-complete-purrassist-0-1-0-plan-and` (HEAD `e50d49a`).
Base: `origin/main` (`3037418`). Files changed: 1 (`docs/RELEASE.md`, +13/-6).
Owner decision, verbatim: "continue - I'm not fixing that." (keep the
`purrassist-derive` crates.io trusted publisher without the `crates-io` environment).

## Changed sentences checked against sources

| RELEASE.md line | Claim | Source | Result |
|---|---|---|---|
| 50 | derive publisher "confirmed 2026-10-12, no environment (owner decision)" | validation.md: API shows repo + `publish.yml`, environment `null`, created 2026-10-12T20:16:01Z | True |
| 93-94 | observed through the API with the same repository and workflow, no environment | validation.md Evidence | True |
| 95 | owner decided to keep it without an environment | owner quote | True |
| 95-97 | only `publish` (runs in `crates-io`) has `id-token: write` | publish.yml:16-17 (top level `contents: read`), :32-34 (`gate`: `contents: read`, `actions: read`), :67-70 (`publish`: `environment: crates-io`, `id-token: write`) | True |
| 97-99 | without an environment, any job a later publish.yml edit grants `id-token: write` could mint a derive token; review on every publish.yml change | crates.io binding is repo + workflow file only for derive | True. This is a manual control: check-release.py:136-140 requires `id-token: write` in `publish` but does not reject it in other jobs |
| 101-103 | re-confirm all six through the API before tagging | unchanged intent | Coherent |
| 248-249 | step 3: each publisher must still match the values recorded in "Trusted publishing" | lines 89-99 record derive with no environment | Matches the owner decision |

`python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py`: exit 0.
`python3 scripts/check-release.py`: exit 0 (release set/order 0.1.0; publish.yml trigger,
`crates-io` environment, id-token, gate, SHA pins, order, verify-published step).

Cross-document grep (README.md, CHANGELOG.md, SECURITY.md, docs/*.md, crate READMEs):
nothing else says the derive publisher is pending or needs `crates-io`.
docs/QUALIFICATION.md:258 ("verified trusted-publisher settings"), docs/IMPLEMENTATION.md:156
and docs/DEPENDENCIES.md:76-80 (the auth action exchanges the OIDC token issued to the
job in the `crates-io` environment) are still accurate. That job still runs in
`crates-io`; the derive publisher just does not require it.

PR feedback: 0 conversation comments, 0 inline comments, 0 reviews (checked live via gh).

## Findings

1. **[MEDIUM, blocking] docs/RELEASE.md:61-62. A leftover pending-state precondition contradicts the record.**
   The unchanged sentence "Re-confirm all six through the crates.io API immediately before
   tagging, and do not tag until `purrassist-derive` is configured." was written when derive
   was "pending owner configuration against the same three values" (old line 93-94), so
   "configured" meant repository + workflow + `crates-io` environment. The PR updated
   step 3 (line 249) and the Trusted publishing section but missed this line. Now it
   either (a) says derive is still unconfigured, contradicting the table at line 50
   and lines 93-99, or (b) read in its original sense, blocks tagging until the
   environment is added, contradicting the owner decision. The reviewer was asked to
   catch exactly this kind of "pending" leftover.
   Fix: replace the sentence with
   "Re-confirm all six through the crates.io API immediately before tagging; each
   must match the values recorded in \"Trusted publishing\"."
   (or drop the derive clause). Re-run check-docs.py.

2. **[LOW, non-blocking, optional hardening] scripts/check-release.py:136-140.** The
   doc's new risk control ("review that permission whenever `publish.yml` changes") is
   manual only. check-release.py could also fail when any job other than `publish` (or
   the top-level `permissions:`) declares `id-token`. That would turn the review note
   into an executable check. The owner did not ask for this, and the PR does not claim
   it is enforced, so it is not required.

3. **[LOW, stage record only] .stage/complete-purrassist-0-1-0-plan-and/validation.md (Evidence, 2nd bullet).**
   It says "Top-level and `gate` permissions are `contents: read`". `gate` also has
   `actions: read` (publish.yml:34). The conclusion (no `id-token` outside `publish`)
   still holds. Correct the wording for accuracy.

## Requirement-weakening check

Step 3 changed from "`purrassist-derive` must be configured first" to "each must still
match the values recorded there". With derive recorded as repository + workflow + no
environment, that is exactly the owner's decision and nothing more. The five original
crates still need `crates-io`, and the pre-tag API re-confirmation of all six remains.
The only unresolved inconsistency is finding 1.

## Recommended actions

1. Edit docs/RELEASE.md:61-62 as described in finding 1, then re-run
   `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py`
   and `make hygiene`.
2. Optionally fix the validation.md wording (finding 3).

VERDICT: BLOCKED

---

## Recheck after f6283b9 ("build(release): only the publish job may request an OIDC token")

Scope: docs/RELEASE.md (+3/-2), scripts/check-release.py (+8), scripts/test-check-release.py (+16).
The commit message has no attribution trailer.

### Commands

| Command | Result |
|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | exit 0 |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py` | exit 0, 20 tests OK |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py` | exit 0 |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py` | 251 tests OK |

### Earlier findings

- Finding 1 (RELEASE.md:61-62): RESOLVED. The line now reads "...immediately before tagging; each
  must match the values recorded in "Trusted publishing"." It no longer contradicts line 50 or lines 93-99.
- Finding 3 (validation.md): RESOLVED. Line 14 now says `gate` has `contents: read` and `actions: read`.
- Finding 2 (hardening): ADOPTED, but it is incomplete. See R1.

### Probe of the new rule

I fed `validate_publish_workflow(path)` modified copies of publish.yml in a temporary directory,
outside the repository. GitHub Actions accepts every variant below as valid.

| Variant | What it grants | Result |
|---|---|---|
| unmodified publish.yml | - | accepted (correct) |
| `gate` block-style `id-token: write` | OIDC to gate | REJECTED (correct) |
| new job `extra:` appended after `publish`, block-style `id-token: write` | OIDC to extra | REJECTED (correct) |
| top-level `permissions: write-all` | every scope, including `id-token: write`, for any job without its own block | **ACCEPTED** |
| `gate` `permissions: write-all` | OIDC to gate | **ACCEPTED** |
| `gate` `permissions: { contents: read, actions: read, id-token: write }` (flow mapping) | OIDC to gate | **ACCEPTED** |
| `gate` block with quoted key `"id-token": write` | OIDC to gate | **ACCEPTED** |
| new job `job2:` appended after `publish`, block-style `id-token: write` | OIDC to job2 | **ACCEPTED** |

### Findings

R1. **[MEDIUM, blocking] docs/RELEASE.md:99-100 claims enforcement that scripts/check-release.py:146-152 does not provide.**
   RELEASE.md now says "`scripts/check-release.py` rejects an `id-token` permission anywhere in
   `publish.yml` outside the `publish` job." The rule only counts block-style lines matching
   `^\s*id-token:`. It misses three things:
   - `write-all`, at workflow level or job level. This is the most likely real edit, and it is
     exactly the "workflow-wide grant" that `test_workflow_wide_id_token_fails` is meant to cover.
   - Flow-mapping and quoted-key spellings.
   - Any job whose id contains a digit, when that job comes after `publish`. The job-boundary
     regex at check-release.py:134, `\n  [A-Za-z_-]+:\n`, stops at the first job id made only of
     letters, `_` and `-`. A job such as `job2:` is therefore folded into `publish_job_body`, and
     its `id-token` counts as the publish job's. This bug already existed for the other
     publish-body checks, but it now weakens a security control.
   AGENTS.md requires executable evidence for capability claims. The derive publisher has no
   environment, so this rule is the only thing confining its token, and the claim is currently
   broader than the evidence.
   Fix (preferred, keeps the doc claim true):
   - In check-release.py, fail on any `write-all` in publish.yml. Count `id-token` occurrences
     with a pattern that also catches quoted and flow forms, ignoring `#` comments, for example
     `re.findall(r'^[^#\n]*["\']?id-token["\']?\s*:', text, re.M)`, and compare that count with
     the same count over `publish_job_body`.
   - Widen the job-boundary regex to GitHub's job-id grammar: `\n  [A-Za-z_][A-Za-z0-9_-]*:\n`.
   - Add tests for top-level `write-all`, job-level `write-all`, a flow-mapping grant, a
     quoted-key grant, and an id-token job named with a digit after `publish`.
   Alternative: narrow RELEASE.md to describe what the check actually catches, and restore the
   "review that permission whenever `publish.yml` changes" note.

R2. **[LOW] scripts/test-check-release.py `test_workflow_wide_id_token_fails`.** Its name
   promises coverage of workflow-wide grants, but it only tests the block-style `id-token: write`
   key, not `write-all`. Fold the `write-all` case into this test or add a sibling test (see R1).

### Coherence

The release procedure is still consistent with publish.yml: the `publish` job, `crates-io`,
`id-token: write` and the gate are all unchanged. Other docs (README.md, CHANGELOG.md,
SECURITY.md, docs/*.md) still have no conflicting statements about pending, trusted publishers
or the `crates-io` environment. The owner's decision is recorded and nothing beyond it is weakened.

VERDICT: BLOCKED

---

## Recheck 2 after 44e5dd1 ("build(release): catch every form of OIDC grant outside the publish job")

The commit message has no attribution trailer.

### Commands

| Command | Result |
|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | exit 0 |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py` | exit 0, 24 tests OK |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py` | exit 0 |

### Probe table (temporary copies of publish.yml outside the repo, run through `validate_publish_workflow`)

| Variant | Expected | Result |
|---|---|---|
| unmodified publish.yml | accept | accepted |
| `gate` block-style `id-token: write` | reject | rejected |
| `extra:` job after `publish` with `id-token: write` | reject | rejected |
| top-level `permissions: write-all` | reject | rejected (write-all) |
| `gate` `permissions: write-all` | reject | rejected (write-all) |
| `gate` inline `{ contents: read, actions: read, id-token: write }` | reject | rejected |
| `gate` `"id-token": write` / `'id-token': write` | reject | rejected / rejected |
| `job2:` job after `publish` with `id-token: write` | reject | rejected |
| `gate` with commented `# id-token: write` | accept | accepted |
| `permissions:` then `write-all` on the next line (gate and top level) | reject | rejected / rejected |
| multi-line flow mapping containing `id-token: write` in `gate` | reject | rejected |
| publish `permissions: &oidc {...}`, gate `permissions: *oidc` (YAML anchor/alias, which GitHub Actions accepts) | reject | **ACCEPTED** |
| `extra: # helper` job header after `publish` with `id-token: write` | reject | **ACCEPTED** |
| `extra: ` header (trailing space) after `publish` with `id-token: write` | reject | **ACCEPTED** |
| `"extra":` quoted job id after `publish` with `id-token: write` | reject | **ACCEPTED** |

R1 and R2 are RESOLVED for every form I reported. The new probes found two remaining gaps.

### Findings

R3. **[MEDIUM, blocking] scripts/check-release.py:135. The job-end regex still misses valid job headers.**
   `\n  [A-Za-z_][A-Za-z0-9_-]*:\n` requires the job key to be followed immediately by a newline.
   So a job header with a trailing comment (`  extra: # helper`), trailing whitespace, or a quoted
   id (`"extra":`) is folded into `publish_job_body`, and its `id-token: write` counts as the
   publish job's. A trailing comment on a job header fits this repository's comment-heavy
   workflow style. The result is that docs/RELEASE.md:99-100 ("rejects an `id-token` permission
   anywhere in `publish.yml` outside the `publish` job") is still broader than the check.
   Fix: end the job body at the first non-comment line indented exactly two spaces:
   `match = re.search(r'\n  [^\s#]', publish_job[1])`. I verified that it finds the end for all
   four header forms above, and that on the current publish.yml the publish body still runs to EOF.
   Add tests for a trailing-comment header and a quoted job id after `publish`.

R4. **[MEDIUM, blocking] scripts/check-release.py:150-155. YAML anchors and aliases get around the count.**
   An anchor on the publish job's permissions (`permissions: &oidc`) and an alias in another job
   (`permissions: *oidc`) grant `id-token: write` without writing a second `id-token` key.
   GitHub Actions has supported YAML anchors and aliases in workflow files since 2025.
   Fix: reject anchors and aliases anywhere in publish.yml (the current file uses none), e.g.
   `re.search(r'^[^#\n]*(?:^\s*|[:\[{,-]\s*)[&*][A-Za-z0-9_]', text, re.M)`, with the error
   "publish.yml must not use YAML anchors or aliases". I checked this regex: it matches
   `permissions: *oidc`, `permissions: &oidc`, `gate: *job` and `- *step`. It does not match
   `>&2`, `&&` or `['rust-v*']`, and it finds nothing in the current publish.yml. Add a test for
   the anchor/alias case.

Alternative to R3 and R4: narrow RELEASE.md:99-100 to the forms the text check recognizes and
restore the "review that permission whenever `publish.yml` changes" note. Without a YAML parser
(which would need a dependency registration), a text check can only cover the forms it lists.
I recommend the two regex fixes, because together they close every form I have found.

### Coherence

Unchanged: the release procedure matches publish.yml, other docs do not contradict the record,
and nothing is weakened beyond the owner's decision.

VERDICT: BLOCKED

---

## Recheck 3 after b1c9645 ("build(release): hold publish.yml to a closed shape")

The commit message has no attribution trailer. docs/RELEASE.md:98-102 now describes the
enforced shape: exactly `gate` and `publish`, no anchors or aliases, no `write-all`, and every
`id-token` key (block, inline or quoted) inside `publish`. "Changing that shape needs a
matching change to the check." The `publish` job's end is found with `\n  [^\s#]` (the R3 fix),
and anchors and aliases are rejected (the R4 fix).

### Commands

| Command | Result |
|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | exit 0 |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py` | exit 0, 28 tests OK |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py` | exit 0 |

### Probe table (temporary copies of publish.yml outside the repo, run through `validate_publish_workflow`)

Plain spellings, meaning anything an ordinary edit would produce:

| Variant | Result |
|---|---|
| unmodified | accepted (correct) |
| `gate` block `id-token: write`; top-level `id-token: write` | rejected / rejected |
| `write-all` at top level, on `gate`, and on the line after `permissions:` | rejected (all three) |
| `gate` inline `{ ..., id-token: write }`; `"id-token":`; explicit key `? id-token` / `: write` | rejected (all three) |
| extra job as `extra:`, `extra: # helper`, `extra: ` (trailing space), `"extra":`, `'job2':` | rejected (all five, closed job set) |
| extra job with no permissions block (inherits top level) | rejected (closed job set) |
| duplicate `publish:` job | rejected (closed job set) |
| `permissions: &oidc` on publish + `permissions: *oidc` on gate | rejected (anchors/aliases) |
| `jobs: # comment`, `jobs:` with trailing spaces | rejected: job list parses as `[]`, so the check fails closed |
| `gate` turned into a reusable-workflow call (`uses: ./...`) and keeping its read-only permissions | accepted. Not a gap: a called workflow cannot exceed the calling job's permissions, so it gets no `id-token` |

Deliberately obfuscated YAML spellings:

| Variant | Result |
|---|---|
| `gate` `"id\x2dtoken": write` / `"id\u002dtoken": write` (double-quoted escapes) | accepted |
| `gate` `permissions: "write\x2dall"` | accepted |
| `gate` `permissions: !!str write-all` (explicit core tag) | accepted |
| `permissions: >-` or `\|-` block scalar with `write-all` on the next line (gate / top level) | accepted / accepted |

R3 and R4 are RESOLVED. The closed job set also removes the whole class of "another job"
spellings, rather than handling them one form at a time.

### Findings

R5. **[LOW, non-blocking] scripts/check-release.py:162-166 and docs/RELEASE.md:100-101. Obfuscated YAML scalars still get through.**
   A YAML parser would read every spelling in the second table as `id-token` or `write-all`, but
   none of them comes from an ordinary edit. Each needs deliberate escape sequences, tags or
   block scalars. Anyone who can make that edit can also change check-release.py in the same
   commit, so a text check in the repository cannot defend against it, and this does not block.
   The RELEASE.md phrase "(block, inline or quoted)" slightly overstates the coverage for
   escaped quoted keys.
   Optional cheap closure: reject backslash escapes, `!!` tags and block-scalar `permissions:`
   values outside comments, e.g. `re.search(r'\\|!!|^[^#\n]*\bpermissions:\s*[|>]', text, re.M)`
   (the current publish.yml has no `\` or `!!`). Or reword RELEASE.md to "in any plain, inline
   or quoted spelling without escapes".

R6. **[INFO, owner-accepted risk; no change requested in this PR] Runtime scope of the check.**
   check-release.py is a PR/CI-time guard (`make check`, verify.yml) and runs again inside the
   `publish` job. A `rust-v*` tag pushed on an unreviewed commit runs that commit's publish.yml,
   and any OIDC-capable `gate` in it would start before `publish` runs the check. A `crates-io`
   deployment rule on the derive publisher would have bounded that. Without the environment,
   the remaining runtime control is tag protection and review. This is the residual of the
   owner's decision ("continue - I'm not fixing that."), and the RELEASE.md text does not claim
   runtime enforcement. Consider a `rust-v*` tag ruleset outside this PR.

### Overall

- Every sentence this PR changes is true against publish.yml and against the API observation in validation.md.
- The release procedure is coherent with publish.yml and with check-release.py's checks.
- There are no contradictions in README.md, CHANGELOG.md, SECURITY.md or docs/*.md.
- Nothing is weakened beyond the owner's decision. The check is now stricter than on main.
- The remaining items are LOW and INFO only.

VERDICT: PASS

---

## Recheck 4 after afc180a ("build(release): reject escapes, tags and block-scalar permissions in publish.yml")

The commit message has no attribution trailer. check-release.py:163-166 adds
`re.search(r'\\|!!|^[^#\n]*\bpermissions:\s*[|>]', text, re.M)`. RELEASE.md:101-102 lists
"no escapes, YAML tags or block-scalar permissions" in the enforced shape.

### Commands

| Command | Result |
|---|---|
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py` | exit 0 |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py` | exit 0, 29 tests OK |
| `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py` | exit 0 |

### R5 probes and regressions (temporary copies outside the repo)

| Variant | Result |
|---|---|
| unmodified publish.yml | accepted (correct) |
| `gate` `"id\x2dtoken": write` / `"id\u002dtoken": write` | rejected / rejected |
| `gate` `permissions: "write\x2dall"` | rejected |
| `gate` `permissions: !!str write-all` | rejected |
| `gate` `permissions: >-` + `write-all`; top-level `permissions: \|-` + `write-all` | rejected / rejected |
| double-quoted `write-all` split with a `\` line continuation | rejected |
| Regressions: gate/top-level `id-token`, `write-all`, inline, single-quoted key, `extra: # helper` job, `"extra":` job, anchor/alias | all rejected |
| commented `# id-token: write` | accepted (correct) |
| `gate` `permissions: !<tag:yaml.org,2002:str> write-all` (verbatim tag) | **accepted** |

R5 is RESOLVED for every form I reported.

### Findings

R7. **[LOW, non-blocking] check-release.py:165 against RELEASE.md:101. "No YAML tags" covers only `!!`.**
   The verbatim tag `!<tag:yaml.org,2002:str>` and local tags such as `!str` are not rejected.
   This is the same deliberately obfuscated class as R5: no ordinary edit produces it, and
   whoever writes it can also edit the check. The doc's wording is still slightly broader than
   the check.
   Optional fix: replace `!!` with
   `^[^#\n]*(?:^|[\s:\[{,-])!(?:!|<|[A-Za-z])` (re.M). I verified that it matches `!!str`,
   `!<...>` and `!str`, does not match gate's shell `! echo "$runs"`, and finds nothing in the
   current publish.yml. The bare non-specific tag `! write-all` cannot be told apart from shell
   negation by a text check. Alternatively, say "no escapes, `!!` or `!<...>` tags" in the doc.

R8. **[LOW, cosmetic] docs/RELEASE.md:102** is 124 characters. The edit appended the clause
   without reflowing the paragraph, unlike the neighbouring prose. check-docs.py accepts it.
   Reflow if the project prefers wrapped prose.

R6 (INFO, owner-accepted runtime residual: tag protection, not the check, is the runtime
control without an environment) is unchanged.

### Overall

- The changed sentences are true against publish.yml and against the API observation in validation.md.
- The release procedure is coherent with publish.yml and check-release.py.
- No other doc contradicts the record.
- Nothing is weakened beyond the owner's decision, and the publish.yml check is now much stricter than on main.
- Remaining items are LOW and INFO only.

VERDICT: PASS
