# Review: MCP Registry workflow (`.github/workflows/mcp-registry.yml`)

Branch `paudley/1-complete-purrassist-0-1-0-plan-and`, `git diff origin/main...HEAD`:
`.github/workflows/mcp-registry.yml` (new), `vendor/tooling.json`,
`vendor/licenses/mcp-publisher-1.8.1/LICENSE` (new), `docs/DEPENDENCIES.md`,
`docs/RELEASE.md`.

Reviewed on 2026-10-13. No files were modified by this review.

## Evidence gathered

- Downloaded `mcp-publisher_linux_amd64.tar.gz` from the v1.8.1 release:
  sha256 `a06c9096dcb9727c13555b6be26c7effa707b01f06a4c561ba7a3635443cf2cc`.
  That matches the workflow pin, GitHub's asset `digest` field, and the line in
  upstream `registry_1.8.1_checksums.txt`.
- `cosign verify-blob` with the upstream sigstore bundle reported
  `Verified OK`. The identity was
  `https://github.com/modelcontextprotocol/registry/.github/workflows/release.yml@refs/tags/v1.8.1`
  and the issuer was `https://token.actions.githubusercontent.com`. The signed
  digest is the same sha256.
- The archive contains `LICENSE`, `README.md` and `mcp-publisher`. Its
  `LICENSE` hashes to `0382b005...be58a`, which is byte-identical to
  `vendor/licenses/mcp-publisher-1.8.1/LICENSE` and to the value in
  `vendor/tooling.json`.
- `git ls-remote` returned these results:
  - `actions/checkout` `11d5960a326750d5838078e36cf38b85af677262` is
    `refs/tags/v4` and `refs/tags/v4.4.0`. It is the same pin used by
    `publish.yml`, `verify.yml` and `release-qualification.yml`, and it is
    already registered.
  - `modelcontextprotocol/registry` `refs/tags/v1.8.1` exists, so the `source`
    URL resolves.
- crates.io source (`crates/crates_io_trustpub/src/github/claims.rs`,
  `workflows.rs`): the trusted-publisher match uses `repository_owner_id`,
  `repository`, `environment` and the workflow **filename** taken from the
  `workflow_ref` claim (regex `([^/]+\.(yml|yaml))(@.+)`). It does not use
  `job_workflow_ref`.
- MCP Registry v1.8.1 source (`internal/api/handlers/v0/auth/github_oidc.go`)
  checks the issuer and the audience. It then grants
  `io.github.<repository_owner>/*`. It does not check ref, event, workflow or
  environment.
- `mcp-publisher` v1.8.1 source (`cmd/publisher/auth/github-oidc.go`, `main.go`)
  reads `ACTIONS_ID_TOKEN_REQUEST_{TOKEN,URL}` and requests a token whose
  audience is derived from the registry URL. It exchanges that token for a
  registry JWT. `validate`, `login` and `publish` are real subcommands.
- These checks pass on the branch: `scripts/check-release.py`,
  `scripts/check-docs.py` and `scripts/check-repository.py`. I did not run the
  full `make check`.

## 1. Can this workflow's `id-token: write` publish crates or affect publish.yml's trust?

No.

- A job in `mcp-registry.yml` gets an OIDC token whose `workflow_ref` is
  `Blackcat-Informatics/purrassist/.github/workflows/mcp-registry.yml@<ref>`.
  crates.io takes the filename from `workflow_ref` (the top-level workflow,
  not `job_workflow_ref`). That filename is `mcp-registry.yml`, so it does not
  match `publish.yml` for any of the six crates.
  - This holds for the five crates that also require `environment: crates-io`.
  - It also holds for `purrassist-derive`, which has no environment. Its
    binding still requires the filename `publish.yml`.
- This stays true even if the third-party binary (which can read
  `ACTIONS_ID_TOKEN_REQUEST_*`) requests a token with a crates.io audience. The
  claims are minted by GitHub, and the job cannot change them.
- The workflow does not call `publish.yml` as a reusable workflow. Even if it
  did, crates.io would see the caller's `workflow_ref`, not `publish.yml`.
- It does not use the `crates-io` environment. It does not write
  `contents`/`actions`. It does not use caches or artifacts, and no other
  workflow restores caches. It does not change `publish.yml` or
  `scripts/check-release.py`. The publish lane's trust inputs are unchanged.
- Residual risk, not introduced here: the `purrassist-derive` binding depends
  only on the filename. Anyone with write access can push a branch that edits
  `publish.yml` itself. That is the existing owner decision documented in
  RELEASE.md, and this change neither widens nor narrows it.
- Advisory: the third-party binary now runs in a job that can mint repository
  OIDC tokens for any audience. If the organization has other OIDC federations
  that trust this repository by a broad `sub` pattern (cloud roles, for
  example), those trusts now cover this job too. Confirm there are none, or
  confirm they are scoped to environment or workflow.

## 2. Binary pinning

The pinning is sound.

- The version is in the URL. A literal sha256 is checked with
  `echo "<hash>  <path>" | sha256sum -c -` under `set -euo pipefail`.
  - A mismatch fails the step.
  - A missing file fails the step.
  - A malformed line fails the step ("no properly formatted checksum lines").
- `curl -f` fails on HTTP errors.
- Extraction happens only after verification, and only the single member
  `mcp-publisher` is extracted, into `$RUNNER_TEMP`.
- The release is not marked immutable on GitHub, so the asset could be
  replaced. The sha256 pin is the control that matters, and it holds. I
  independently confirmed the pinned digest against upstream's signed checksum
  data (section "Evidence gathered").
- Optional hardening: also run `cosign verify-blob --bundle` (or
  `gh attestation verify`) against the release workflow identity. This is not
  required, because the hash pin is already stronger than trusting on first use.

## 3. Trigger, permissions, persist-credentials, checkout pin

These are appropriate, with advisories.

- `workflow_dispatch` only. There are no `pull_request`/`pull_request_target`,
  `workflow_run` or `push` triggers, so untrusted forks cannot start it.
- Top-level `contents: read`. The job sets `contents: read` and
  `id-token: write`, which is the least privilege the registry login needs.
- `persist-credentials: false` is correct. The third-party binary runs later in
  the job, and `GITHUB_TOKEN` is not left in `.git/config` or passed to any
  step's environment.
- The checkout pin is a full SHA, which is the v4/v4.4.0 tag. Nit: the comment
  says `# v4`, while `v4.4.0` would be more precise. The same nit applies to
  the existing workflows.
- Advisory (A1): a dispatch can name any branch. The registry checks only
  `repository_owner`, so the `server.json` that gets published comes from
  whichever ref the operator picks.
  - Recommendation: add `if: github.ref == 'refs/heads/main'` to the job to
    prevent operator error.
  - Alternatively, use an environment such as `mcp-registry` with a deployment
    branch rule for `main` and optional required reviewers.
  - Neither is a hard boundary against a repository writer. A writer can add a
    push-triggered workflow with `id-token: write` on any branch, and the
    registry grants `io.github.Blackcat-Informatics/*` to any workflow in any
    repository of the organization. That is the registry's design, not
    something this change introduces.
- Advisory (A2): there is no `timeout-minutes`, so GitHub's 360-minute default
  applies. `publish.yml` sets explicit timeouts. Suggest
  `timeout-minutes: 10`.
- Advisory (A3): there is no `concurrency` group. Two concurrent dispatches are
  benign, because the registry rejects a duplicate version. A
  `concurrency: mcp-registry` group would still be tidy.

## 4. Injection / untrusted input

None found.

- `run:` blocks contain no `${{ ... }}` expressions. The only interpolations
  are shell variables whose values come from literal `env:` entries and
  runner-provided `$RUNNER_TEMP`/`$GITHUB_PATH`.
- There are no dispatch `inputs`.
- The only content-bearing input is `server.json` from the dispatched ref. It
  is parsed by the pinned binary and sent to the registry. Anyone who can
  choose that ref already has write access (see A1).
- `$GITHUB_PATH` is written with a runner-controlled path, so PATH injection
  through attacker data is not possible.
- The registry JWT that `login` writes goes to the runner's user config
  directory on an ephemeral runner. It is never uploaded.

## 5. Tooling registration accuracy

The registration is accurate, with one completeness gap.

- The following are correct:
  - name and version
  - `source` (the v1.8.1 tag exists)
  - the asset name and sha256 in `scope`
  - the notice path and digest (byte-identical to the archive's `LICENSE`)
  - the `notice_normalization` text (unmodified copy, plus a description of the
    MIT to Apache-2.0 transition and CC-BY-4.0 for docs)
  - the usage scope (only this workflow, not `make check`, not linked or
    packaged)
- `check-repository.py` verifies the notice digest, and the check passes.
- Gap (G1): `license: "Apache-2.0 AND MIT"` describes the registry source
  repository, not the executed artifact. The binary statically links the Go
  standard library and 52 third-party Go modules. The upstream SPDX SBOM
  `mcp-publisher_linux_amd64.tar.gz.sbom.json` (sha256
  `4620c9fa4d6cc8cacc52d0a5e266ebe5d240197dbdb987bc6ccf25f8801c41de`, listed
  in the signed checksums file) lists them, including:
  - Azure SDK
  - Google Cloud KMS
  - gRPC
  - OpenTelemetry
  - docker/cli
  - golang.org/x/*

  The `node` entry sets a precedent by stating that its bundled third-party
  components are not vendored or shipped. Suggest a matching sentence here,
  with the SBOM URL and digest, so the AGENTS.md "including transitive"
  requirement is met at the precedent's level. This is non-blocking because
  nothing is redistributed, and `python`/`cmake` are registered at tool level
  only.
- Gap (G2, consistency): the `node` entry records its downloaded binary in the
  machine-readable fields `binary_source`/`binary_sha256`. This entry puts the
  digest only in `scope` prose, and nothing checks that the workflow's
  `MCP_PUBLISHER_SHA256` matches the registry. Suggest adding those fields and,
  optionally, a `check-repository.py` assertion that the workflow pin equals
  the registered value. This is non-blocking.
- `docs/DEPENDENCIES.md`'s new paragraph is consistent with the entry.

## 6. Documentation accuracy: required fix

The change adds a second `id-token: write` holder and a second publishing
workflow. It also leaves the release document's security rationale stating the
opposite, and that rationale is now factually false:

- `docs/RELEASE.md` lines 95-97 say: "Today only the `publish` job, which runs
  in `crates-io`, has `id-token: write`, so it is the only job that can obtain a
  `purrassist-derive` publishing token."
  - The premise is now false across the repository, because
    `mcp-registry.yml`'s `submit` job also has `id-token: write`.
  - The conclusion still holds, but for a different reason that the document
    does not state. crates.io binds the publisher to the workflow filename
    taken from the OIDC `workflow_ref` claim (`publish.yml`).
  - Required: restate the premise as "within `publish.yml`, only the `publish`
    job has `id-token: write`". Also add that other workflows with
    `id-token: write` (currently `mcp-registry.yml`) cannot mint crates.io
    tokens because crates.io matches the `workflow_ref` filename. This
    document is the one a future editor relies on when deciding whether
    another `id-token` grant is safe.
- `docs/RELEASE.md` line 151 has the heading "Publishing (publish.yml): the
  only workflow that publishes". It is now inaccurate, because
  `mcp-registry.yml` publishes to the MCP Registry. Required: qualify it, for
  example "the only workflow that publishes crates".
- Optional: `publish.yml`'s header says "The one and only publish lane". Change
  it to "crate publish lane". Note that `check-release.py` closes the shape of
  `publish.yml`, and this edit is a comment, so the check is unaffected.

`scripts/check-docs.py` does not catch either required item.

## Recheck of 03be6d5 (ci(mcp-registry): main-only, bounded runs; state the real crates.io control)

- Required fix (section 6): resolved.
  - `docs/RELEASE.md` now names the real control: crates.io accepts a token
    only from `publish.yml`, which it reads from the OIDC `workflow_ref` claim.
    It states that no other workflow, including the MCP Registry one, can get
    a crates.io token.
  - The closed-shape premise is now scoped "within `publish.yml`".
  - The heading reads "the only workflow that publishes crates". No other
    repository-wide "only workflow that publishes" claim remains in `docs/`,
    `README.md` or `SECURITY.md`.
- A1 resolved: `if: github.ref == 'refs/heads/main'` is on the job. Dispatches
  from other branches or tags are skipped. As noted, this guards against
  operator error and is not a boundary against someone with write access.
- A2 resolved: `timeout-minutes: 10`.
- A3 resolved: workflow-level `concurrency: mcp-registry` with
  `cancel-in-progress: false`, so a submission that has started is never
  cancelled.
- G1 resolved: `scope` notes the statically linked Go standard library and the
  modules in upstream's signed SBOM, and says none of it is vendored or
  shipped. Nit: the SBOM digest
  (`4620c9fa...c41de`) is not recorded. That is optional, because the SBOM is
  listed in the signed checksums file.
- G2 resolved: `binary_source` and `binary_sha256` are present. `binary_sha256`
  equals the workflow's `MCP_PUBLISHER_SHA256` (`a06c9096...f2cc`), and that
  digest was confirmed independently against the release asset and the
  sigstore-verified upstream signature. Optional: nothing yet enforces that the
  two pins stay equal.
- Unchanged and still sound: the download/verify/extract steps, the
  permissions, `persist-credentials: false`, the checkout pin, and the absence
  of `${{ }}` in `run:` blocks.
- On 03be6d5, `scripts/check-release.py`, `scripts/check-docs.py` and
  `scripts/check-repository.py` pass. The coordinator reports
  `make hygiene` exits 0. I did not run the full `make check`.

## Summary

| Item | Result |
| --- | --- |
| id-token cannot publish crates / affect publish.yml trust | Pass (workflow_ref filename binding, now documented) |
| Binary pinning (version + literal sha256, `sha256sum -c`) | Pass (digest independently confirmed, sigstore-verified) |
| Trigger / permissions / persist-credentials / checkout pin | Pass (main-only, 10-minute timeout, concurrency group) |
| Injection / untrusted input | Pass |
| Tooling registration | Pass (structured binary pin, SBOM note) |
| Public documentation accuracy | Pass |

Remaining optional items:

- Record the SBOM digest.
- Add a check that the workflow's sha256 equals `binary_sha256`.
- Confirm that no other OIDC federation trusts this repository by a broad
  `sub` pattern.

VERDICT: PASS
