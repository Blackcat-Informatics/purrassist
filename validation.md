# Validation index — issue #1, PR #21 (trusted-publisher record)

Scope: `docs/RELEASE.md` only, in commits `63218a6`, `a4d8b03` and `e50d49a` on base `3037418`.

The change records the observed crates.io state of the `purrassist-derive` trusted publisher: configured for `Blackcat-Informatics/purrassist` and `publish.yml` on 2026-10-12, with no environment. It records the owner's decision to keep it that way: "continue - I'm not fixing that." Release step 3 now checks each trusted publisher against its recorded values.

## Review path

Lightweight: a single-file documentation change, with no code, tests or packaged files. A review-debt audit and an independent accuracy review were run.

## Evidence

- crates.io API (`/api/v1/trusted_publishing/github_configs`, authenticated read), 2026-10-12: all five original crates have `Blackcat-Informatics/purrassist`, `publish.yml` and `crates-io`. `purrassist-derive` has `Blackcat-Informatics/purrassist` and `publish.yml` with environment `null`, created `2026-10-12T20:16:01Z`.
- `.github/workflows/publish.yml`: only the `publish` job (environment `crates-io`) has `id-token: write`. Top-level permissions are `contents: read`; `gate` has `contents: read` and `actions: read`.
- `python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py` passes, and `scripts/test-check-docs.py` passes.
- `make hygiene` exits 0 on `a4d8b03`. `e50d49a` only reflows one paragraph, and `check-docs.py` passes on it.
- `make check` was not rerun: no Rust source, test or packaged file changed. `make package` was not rerun: `docs/RELEASE.md` is not in any crate archive.

## Integration

`origin/main` = `3037418` is the branch point. `git merge-tree --write-tree --no-messages origin/main HEAD` gave `2370742` = `HEAD^{tree}` at `e50d49a`; see the final-head evidence below for the current tree.

## Hosted CI

Out of scope by owner decision. The `main` ruleset requires no status checks. The checks were in progress at assessment time.

## Accuracy-review fixes (commit after `e50d49a`)

- `docs/RELEASE.md:61-62` said "do not tag until `purrassist-derive` is configured", which contradicted the record. It now requires each trusted publisher to match its recorded values.
- `scripts/check-release.py` rejects an `id-token` permission anywhere in `publish.yml` outside the `publish` job. This enforces the condition that makes the environment-less derive entry safe. Tests in `scripts/test-check-release.py`: `test_id_token_in_another_job_fails` and `test_workflow_wide_id_token_fails`.
- Checks: `check-release.py`, `test-check-release.py` and `check-docs.py` pass with `EncodingWarning` as an error. The `make hygiene` result follows.
hygiene exit=0

## Recheck fix

`check-release.py` now catches write-all grants, quoted and inline id-token keys, and digit-named jobs (the job-end pattern now matches any valid job name). `test-check-release.py`: 24 tests OK with EncodingWarning as an error. make hygiene exit 0.

## Final accuracy recheck

`gap-analysis.md` recheck 3: VERDICT PASS at `b1c9645`. The R5 hardening followed (escapes, tags, block-scalar permissions rejected; 29 tests). R6 (tag-time exposure without an environment) is an owner-accepted risk, reported to the owner as a possible `rust-v*` tag ruleset. make hygiene exit 0 on the final head.

R7 (other YAML tag forms) and R8 (re-wrap) applied in the final commit; test-check-release.py 29 OK; make hygiene exit 0 on the final head.

## Final-head evidence (commands run by the coordinator)

```
$ python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-release.py
publish.yml verified: tag trigger, crates-io environment, id-token permission, qualification gate, SHA-pinned actions, dependency order, and verify-published step.
$ python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py
Ran 29 tests in 0.037s
OK
$ python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py
Documentation facts, links, wording and known limitations match their sources.
$ make hygiene
exit=0
```

Final head `e34c665`. `git merge-tree --write-tree --no-messages origin/main HEAD` = `8c8f96f` = `HEAD^{tree}`, with origin/main `3037418`. This supersedes the earlier tree hash `2370742` recorded at `e50d49a`.

C1 wording ("letter-named") applied in the final docs-only commit; check-docs.py OK. merge-plan-review.md recheck: VERDICT PASS.
