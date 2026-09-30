<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Rust release procedure

PurRAssist 0.1.0 has an implemented scope and executable qualification record
in `QUALIFICATION.md`. Publication acceptance is performed on the reviewed
merge commit using the steps below. A published 0.1.0 is not claimed before
registry downloads and installed CLI/MCP behavior are verified.

The release set is `purrassist-core`, `purrassist-syntax`, `purrassist`,
`purrassist-host`, then `purrassist-cli`. `scripts/check-release.py` proves that
this is exactly the publishable workspace, all versions match, internal version
requirements match, normal and dev dependencies precede their users, and (see
below) that `.github/workflows/publish.yml` has the required trigger,
environment, permissions, qualification gate, SHA-pinned actions, publish
order, and verification step. It runs in `make check` and `make package`.

## Placeholder name records (already minted)

One-time, irreversible `0.0.0` name-reservation records exist on crates.io for
all five crates (minimal `lib` packages, `version = "0.0.0"`, no dependencies,
`//! Reserved name.`; see the issue #1 comment recording this for the exact
mechanics). These are **not** the `0.1.0` release; they only hold the names and
let a trusted publisher be configured before the first real publication.

## Trusted publishing (already configured)

Each of the five crates has a crates.io trusted publisher configured against:

- Repository: `Blackcat-Informatics/purrassist`
- Workflow: `publish.yml`
- Environment: `crates-io`

This lets `publish.yml`'s `publish` job mint a short-lived registry token via
GitHub OIDC (`permissions: id-token: write`) instead of a stored
`CARGO_REGISTRY_TOKEN` secret. No registry credential is stored in this
repository or its CI configuration at any point.

## Qualification (release-qualification.yml): qualification only

On `rust-vVERSION` tags, `release-qualification.yml` checks the tag against the
workspace, runs the full local gate on stable Rust 1.98.0, packages all five
crates with Cargo verification enabled, and audits the archives and license
notices. Packaging uses a commit-scoped Cargo home so a previously packaged
crate at the same version cannot satisfy a later crate's verification. This
workflow **never publishes, attests, or creates a GitHub release**, and it
holds no `id-token` permission and no registry credential. `verify.yml`'s
`qualification` job gathers the same class of evidence (independent syntax
oracles, coverage-guided/differential fuzzing, packaging, resource
measurement against the documented limits) on main pushes, pull requests, and
manual runs, so
regressions are visible long before a tag is cut.

## Publishing (publish.yml): the only workflow that publishes

`.github/workflows/publish.yml` triggers only on push of tags matching
`rust-v*` (no `workflow_dispatch`, so it can never be triggered without a tag).
It has two jobs:

1. **`gate`** — blocks on a successful `release-qualification.yml` run for the
   exact commit the tag points at. Both workflows trigger on the same tag
   push and start concurrently, so this job polls the Actions API
   (`GET /repos/{repo}/actions/workflows/release-qualification.yml/runs?head_sha=<sha>`)
   with the ambient `GITHUB_TOKEN`, in a bounded loop (≤ 200 attempts, 30s
   apart), until it observes a run with `conclusion: success` for that SHA. If
   every recorded run for that SHA is completed and failed, it fails
   immediately. An empty API result waits within the same bound for the sibling
   tag-triggered run to become visible. The bounded wait covers the qualification
   job's 90-minute allowance with queue headroom. A `workflow_run`
   trigger was considered and rejected: it would tie `publish.yml`'s own
   trigger semantics to another workflow's completion event (in tension with
   the required "push of tags" trigger) and its head-SHA/permissions handling
   for tag pushes is less direct to reason about than a plain, bounded poll
   against the exact commit. This job holds no `id-token` permission and never
   touches crates.io.
2. **`publish`** — runs in the `crates-io` environment with
   `permissions: id-token: write, contents: read`. It re-verifies the release
   set/order/tag with `scripts/check-release.py --tag`, authenticates via
   `rust-lang/crates-io-auth-action` (pinned by full commit SHA, registered in
   `vendor/tooling.json`), then publishes `purrassist-core` → `purrassist-syntax`
   → `purrassist` → `purrassist-host` → `purrassist-cli` in that fixed order
   with `cargo publish --locked` (full verification, never `--no-verify`). After
   each `cargo publish`, it polls
   `https://crates.io/api/v1/crates/<name>/<version>` with `curl` (explicit
   User-Agent, ≤ 10 attempts, 30s apart) until that version is served, before
   publishing the crate that depends on it. Once all five are published and
   served, it runs `python3 scripts/verify-published.py --version <version>`,
   which installs the just-published `purrassist-cli` from crates.io into a
   fresh `CARGO_HOME`, replays fixed CLI/MCP transcripts against it, and diffs
   each downloaded `.crate` against a freshly `cargo package`d copy of the
   same commit.

A registry `404` is only a current observation, not a name reservation.
Publishing a version cannot be undone or overwritten. All actions in
`publish.yml` are pinned by full commit SHA with `persist-credentials: false`
on checkout.

## Verification tooling

- `scripts/check-packages.py` asserts every packaged crate archive's bundled
  third-party notices match the explicit reviewed `packaged_notices` set and
  hashes in `vendor/dependencies.json`. Both checkout and archive bytes are
  compared with those registered identities.
- `scripts/verify-published.py --version X` is the downloaded-package
  verifier described above. It accepts `--registry-index <path-or-file-url>`
  to redirect the fresh `CARGO_HOME`'s `crates-io` source to a local
  `local-registry`-format directory instead of the real registry, which is
  how `scripts/test-verify-published.py` exercises it fully offline
  (including a negative case: a tampered `.crate` in the local registry must
  make the archive-diff step fail). That test is wired into `make hygiene`.
- `scripts/data/verify-published/{mcp-requests,mcp-responses,call-request,
  call-response}` are committed transcript fixtures: fixed CLI/MCP requests
  and the exact stdout `purrassist` produced for them, compared byte-for-byte
  by `verify-published.py`'s ordinary (non-capture) run. Whenever the tool
  registry changes shape (a tool is added, removed, or its output changes —
  e.g. Task 10's registry replacement, or a new `generate_json_witness`
  tool), the committed responses go stale and must be regenerated by hand:

  ```sh
  python3 scripts/verify-published.py --capture
  ```

  This builds `purrassist-cli` from the current workspace (`cargo build -p
  purrassist-cli --locked`; pass `--binary <path>` to capture from an
  already-built binary instead) and rewrites `mcp-responses.jsonl` and
  `call-response.json` deterministically from the existing, hand-curated
  `mcp-requests.jsonl`/`call-request.json` (never touched by `--capture`).
  Review the diff — a growing/shrinking `tools/list` census or a changed
  receipt shape should be expected and intentional, not silent — before
  committing it. `scripts/test-verify-published.py`'s
  `CommittedFixtureFreshnessTests` is the check-mode counterpart: it builds
  the real current workspace CLI and replays the committed fixtures against
  it, failing `make hygiene` if they differ, so a stale fixture can never be
  committed silently.

  `root-files.json` seeds a disposable verification root. The default MCP
  transcript reads the file, applies a journaled replacement, conditionally
  undoes it, and reads it again. Verification requires byte-exact restoration.
  Pipe writes, response reads, and child exit have bounded timeouts; regression
  tests prove cleanup for missing responses, partial output, and stderr flooding.

## Release step (after merge)

Before a first publication, verify hosted CI for the exact merge commit, run
the portability, conformance, syntax-oracle, fuzz, performance, and package
checks listed in `QUALIFICATION.md`, and re-confirm the trusted-publisher
settings above via the crates.io API immediately before tagging (a repository
rename, workflow rename, or environment rename would silently break trusted
publishing). Tagging `rust-vVERSION` is irreversible and requires explicit
authorization; `publish.yml` then does the rest. After it succeeds, create
the GitHub release and link the `verify-published.py` output in the release
notes.

After publication, install `purrassist-cli` from crates.io into a clean root
and run the documented decode, plan/application, and MCP examples against that
download. A local `cargo install --path` proves only the checkout build.
