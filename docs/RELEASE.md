<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Rust release procedure

PurRAssist 0.1.0 has an implemented scope and executable qualification record
in `QUALIFICATION.md`. Publication acceptance is performed on the reviewed
merge commit using the steps below. A published 0.1.0 is not claimed before
registry downloads and installed CLI/MCP behavior are verified.

The release set is six crates, in this order: `purrassist-core`,
`purrassist-derive`, `purrassist-syntax`, `purrassist`, `purrassist-host`, then
`purrassist-cli`. `purrassist-derive` follows `purrassist-core` because it
dev-depends on it, and precedes `purrassist`, which depends on it.
`scripts/check-release.py` proves that this is exactly the publishable
workspace, all versions match, internal version requirements match, normal and
dev dependencies precede their users, and (see
below) that `.github/workflows/publish.yml` has the required trigger,
environment, permissions, qualification gate, SHA-pinned actions, publish
order, and verification step. It runs in `make check` and `make package`.

**Derive/core compatibility within a minor series.** Generated code references
`purrassist_core::tool_call::__DERIVE_ABI_<major>_<minor>`, so the derive and
core must share `major.minor`, and the documentation promises that their patch
versions may differ. To keep that promise, a patch release must not change what
generated code needs from core: any change to the derive support surface
(`tool_call::derive_support`, the `FromNodeError` constructors, or another item
generated code names) that an older core patch of the same minor lacks requires
a minor version bump, which renames the marker. Likewise, a core patch release
must not remove or change a `#[doc(hidden)]` support item that same-minor
derive output uses. The core unit test fails if the workspace major or minor
version moves without renaming the marker.

## Placeholder name records (already minted)

One-time, irreversible `0.0.0` name-reservation records exist on crates.io for
the original five crates (`purrassist-core`, `purrassist-syntax`,
`purrassist`, `purrassist-host`, `purrassist-cli`; minimal `lib` packages,
`version = "0.0.0"`, no dependencies, `//! Reserved name.`; see the issue #1
comment recording this for the exact mechanics). These are **not** the `0.1.0`
release; they only hold the names and let a trusted publisher be configured
before the first real publication.

**`purrassist-derive` has no placeholder record yet.** Its name has not been
reserved on crates.io. That is an external, one-time, irreversible maintainer
action that has **not** been done, and it must be completed before any release
tag is pushed:

1. Mint the `0.0.0` placeholder record for `purrassist-derive` the same way as
   for the other crates (a minimal `lib` package, `version = "0.0.0"`, no
   dependencies, `//! Reserved name.`; see the issue #1 comment). The crate is
   a proc-macro crate, but the placeholder only holds the name.
2. Configure the crates.io trusted publisher for `purrassist-derive`
   (repository, workflow and environment below).

Until both are done the release must not be tagged. `publish.yml` now has a
preflight step (see below) that fails before `purrassist-core` is published
when any release crate name is absent from crates.io (HTTP 200 is required
for `purrassist-derive` too, so its name must be reserved first) or when the
release set is not owned identically. The preflight cannot see trusted-publisher
configuration, which is not verifiable unauthenticated; confirming it stays the
manual step. Re-confirm both through the crates.io API immediately before
tagging.

## Publish preflight

Before authenticating, `publish.yml` calls `preflight_owners()` in
`scripts/check-release.py` for every crate in `RELEASE_ORDER` (the single
source of the order). For each crate, `GET https://crates.io/api/v1/crates/<name>`
must return 200, and the set of owner logins from
`GET https://crates.io/api/v1/crates/<name>/owners` must equal that of
`purrassist-core`. No login is hard-coded; the release set must simply be owned
identically. Both requests send the `purrassist-release` user agent. Any
failure (missing crate, mismatched owners, network error, malformed response)
exits non-zero before any `cargo publish`, so a missing reservation cannot
leave a half-published release. `scripts/check-release.py` requires the step
and its position, and `scripts/test-check-release.py` tests both offline with
fake registry responses.

The preflight does **not** verify trusted-publisher configuration: that state
is not visible to an unauthenticated request. Confirming that each crate's
trusted publisher is configured stays the manual pre-tag step below.

## Trusted publishing (configured for five of six crates)

Each of the original five crates (all except `purrassist-derive`, whose
trusted publisher is not yet configured; see above) has a crates.io trusted
publisher configured against:

- Repository: `Blackcat-Informatics/purrassist`
- Workflow: `publish.yml`
- Environment: `crates-io`

This lets `publish.yml`'s `publish` job mint a short-lived registry token via
GitHub OIDC (`permissions: id-token: write`) instead of a stored
`CARGO_REGISTRY_TOKEN` secret. No registry credential is stored in this
repository or its CI configuration at any point.

## Qualification (release-qualification.yml): qualification only

On `rust-vVERSION` tags, `release-qualification.yml` checks the tag against the
workspace, runs the full local gate on stable Rust 1.98.0, packages all six
crates with Cargo verification enabled, and audits the archives and license
notices. Every package attempt also runs the packaging isolation and failure
regressions before creating archives. Packaging resolves the real toolchain Cargo through rustup and uses
fresh Cargo home, target and intermediate directories for each invocation.
Both compiler wrapper layers are disabled for these verification builds;
previously packaged same-version crates and build artifacts cannot satisfy them.
The verified archives are retained in a unique ignored `package-archives`
run directory printed by the command, after the complete source/license audit.
This workflow **never publishes, attests, or creates a GitHub release**, and it
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
   set/order/tag with `scripts/check-release.py --tag`, runs the **preflight**
   (`scripts/check-release.py --preflight-owners --user-agent ...`, before
   authentication and before any `cargo publish`), authenticates via
   `rust-lang/crates-io-auth-action` (pinned by full commit SHA, registered in
   `vendor/tooling.json`), then publishes `purrassist-core` → `purrassist-derive`
   → `purrassist-syntax` → `purrassist` → `purrassist-host` →
   `purrassist-cli` in that fixed order
   with `cargo publish --locked` (full verification, never `--no-verify`). After
   each `cargo publish`, it polls
   `https://crates.io/api/v1/crates/<name>/<version>` with `curl` (explicit
   User-Agent, ≤ 10 attempts, 30s apart) until that version is served, before
   publishing the crate that depends on it. Once all six are published and
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
  make the archive-diff step fail). That test runs in the `native` job of
  `verify.yml` on every pull request, and in release qualification.
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
  it, failing the pull request's `native` CI job if they differ, so a stale
  fixture can never be merged silently.

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
publishing). The `purrassist-derive` name reservation and trusted publisher are
not yet done and must exist first (see "Placeholder name records"). Tagging
`rust-vVERSION` is irreversible and requires explicit authorization;
`publish.yml` then does the rest. After it succeeds, create
the GitHub release and link the `verify-published.py` output in the release
notes.

After publication, install `purrassist-cli` from crates.io into a clean root
and run the documented decode, plan/application, and MCP examples against that
download. A local `cargo install --path` proves only the checkout build.
