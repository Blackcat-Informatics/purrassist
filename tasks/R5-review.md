# R5 independent review

## Scope
Uncommitted diff at HEAD 9534a4f: .github/workflows/publish.yml, verify.yml, Makefile, docs/RELEASE.md, scripts/check-release.py, and untracked scripts/test-check-release.py. Spec: remediation-plan.md "R5" and gap-analysis.md G3. No source was modified, and no external network was used.

## Evidence
- `python3 scripts/test-check-release.py`: 18 tests OK.
- `python3 scripts/check-release.py`: exit 0.
- `python3 scripts/check-repository.py`: exit 0.
- `make hygiene`: exit 0. It includes the new test.
- `actionlint .github/workflows/*.yml`: exit 0.
- `registry_fetch('ua-test')` against a local http.server on 127.0.0.1:
  - 200 returns `(200, body)`.
  - 404 returns `(404, b'')`.
  - 500 returns `(500, b'')`.
  - Connection refused raises `OSError`.
  - The server saw `User-Agent: ua-test` on every request.

## Answers
- **HTTPError handling.** `registry_fetch` catches `HTTPError` and returns `(error.code, b'')`. `URLError` is converted to `OSError`. `preflight_owners` catches `OSError` and reports "returned HTTP 404, expected 200" per crate. It collects all failures and raises `SystemExit`. Nothing is unhandled for the observed cases.
- **Timeouts.** `urlopen(..., timeout=60)` is set. Socket timeouts are `OSError` subclasses and are handled. Minor: `http.client.IncompleteRead` is not an `OSError` and would surface as a traceback. It still exits non-zero, so it still fails the job, just not tidily.
- **URL safety.** The names come from the `RELEASE_ORDER` constants, not user input, and the URL is a fixed https base plus the name. Safe.
- **Injection.** The `run:` line uses only `${{ github.repository }}`, inside single quotes. No other expressions are used. No injection risk.
- **Python and order.** The step is after checkout, after `Verify release set` (which already uses `python3`), and before auth. `python3` is on the ubuntu-24.04 runner. The preflight uses stdlib only. Order is correct, and `validate_publish_workflow` enforces preflight < auth and preflight < first `cargo publish`.
- **Plan coverage.**
  - Owners are compared with core's, and no login is hard-coded.
  - Both requests send the user agent.
  - `validate_publish_workflow` requires the step, the invocation, the user agent and the trusted-publisher comment.
  - Mutation tests cover preflight removed, moved after auth, moved after publish, owners removed, user agent removed, and note removed.
  - The test is wired into the Makefile `hygiene` target and `verify.yml`, beside `test-verify-published.py`.
- **RELEASE.md name reservation.** crates.io cannot reserve a name without publishing a version. The repo's existing procedure is a one-time `0.0.0` placeholder publish, documented under "Placeholder name records" step 1 for purrassist-derive. This is the same mechanism used for the other five crates, and it makes the 200 hold. It is workable, and the plan assumed it (the plan's R5 treats reservation as a manual pre-tag step). The new text states that 200 is required and the name must be reserved first. Owners will match if the same account publishes the placeholder. The doc does not say this explicitly, but step 1 says "the same way as for the other crates". A first publish of a never-reserved crate fails preflight, which is intended fail-fast behavior.

## Findings (non-blocking)
1. There is no test for `registry_fetch` itself (HTTPError mapping). It was verified by hand above. A local-server test would be cheap.
2. `IncompleteRead` is not mapped to `OSError`.
3. The user agent comes from `github.repository` in the workflow but is hard-coded in verify-published.py. They are the same for this repo.
4. The `validate_publish_workflow` `text.find('cargo publish')` could match a comment. Cosmetic.

VERDICT: PASS
