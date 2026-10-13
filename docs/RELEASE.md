<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Rust release procedure

PurRAssist 0.1.0 has an implemented scope and executable qualification record
in `QUALIFICATION.md`. Status: merged to `main`, including tool-call recovery
and `purrassist-derive`; published as `rust-v0.1.0`. This procedure is run
against the exact `main` commit to be tagged: every check below, and the tag
itself, name that one commit. A published 0.1.0 is not claimed before registry
downloads and installed CLI/MCP behavior are verified.

## Release set

The release set is six crates, in this order: `purrassist-core`,
`purrassist-derive`, `purrassist-syntax`, `purrassist`, `purrassist-host`, then
`purrassist-cli`. `purrassist-derive` follows `purrassist-core` because it
dev-depends on it, and precedes `purrassist`, which depends on it.
`scripts/check-release.py` proves that this is exactly the publishable
workspace, all versions match, internal version requirements match, normal and
dev dependencies precede their users, and (see
below) that `.github/workflows/publish.yml` has the required trigger,
environment, permissions, qualification gate, SHA-pinned actions, publish
order, and verification step. It also proves gate identity. Both gate jobs
(release qualification's `package` job and `verify.yml`'s `release-gate` job)
are held to allowlists, so anything not explicitly allowed fails: only the
listed workflow and job keys, an `env` of exactly `RUSTUP_TOOLCHAIN` equal to
the job's stable install, the allowed step keys, a SHA-pinned checkout, and no
`GITHUB_ENV` or `GITHUB_PATH` write. `verify.yml` must keep an unfiltered
`pull_request:` trigger, so no pull request skips the gate. Release
qualification's `package` job has only the install, version-check and
`Verify the release gate` steps, `verify.yml`'s `release-gate` job runs the
same `make release-gate` with the same installs and environment, the Makefile
`release-gate` target has no prerequisites and exactly its five gates in order,
and `check` depends on `lint` and `test`. `make lint` is the two `--locked`
Clippy commands: the workspace, and the `test = false` corpus suites named in
`CORPUS_SUITES`, which must equal the workspace's `test = false` test targets.
It checks the toolchain pins as well: it reads every file in
`.github/workflows/`, so a pin in a new workflow is checked too; every pin must
be a literal version, never a `${{ }}` expression or a shell variable; every
stable pin is identical and matches the workspace `rust-version`; and every
nightly pin matches `rust-toolchain.toml`. `scripts/check-docs.py` holds every
`RUSTUP_TOOLCHAIN=` value and patch-form Rust version in this document, the
README and `docs/QUALIFICATION.md` to the stable pin. `check-release.py` also
rejects Makefile lines that would change the toolchain or ignore failures
(`RUSTUP_TOOLCHAIN`, `unexport`, `.IGNORE`, or `-i` in `MAKEFLAGS`).
`check-release.py` runs in `make check` and `make package`; `check-docs.py`
runs in `make check`.

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

## Name reservations

One-time, irreversible `0.0.0` name-reservation records exist on crates.io for
all six crates in the release set, with identical ownership. Each is a minimal
`lib` package, `version = "0.0.0"`, no dependencies, `//! Reserved name.`; the
`purrassist-derive` placeholder only holds the name, although the workspace
crate is a proc-macro crate. They are **not** the `0.1.0` release; they hold the
names and let a trusted publisher be configured before the first real
publication.

| Crate | Reservation | Trusted publisher |
|---|---|---|
| `purrassist-core` | `0.0.0`, minted first | confirmed 2026-09-28 |
| `purrassist-derive` | `0.0.0`, minted 2026-10-12 | confirmed 2026-10-12, no environment (owner decision) |
| `purrassist-syntax` | `0.0.0`, minted first | confirmed 2026-09-28 |
| `purrassist` | `0.0.0`, minted first | confirmed 2026-09-28 |
| `purrassist-host` | `0.0.0`, minted first | confirmed 2026-09-28 |
| `purrassist-cli` | `0.0.0`, minted first | confirmed 2026-09-28 |

`publish.yml` has a preflight step (see below) that fails before
`purrassist-core` is published when any release crate name is absent from
crates.io (HTTP 200 is required for every crate) or when the release set is
not owned identically. The preflight cannot see trusted-publisher
configuration, which is not verifiable unauthenticated; confirming it stays a
manual step. Re-confirm all six through the crates.io API immediately before
tagging; each must match the values recorded in "Trusted publishing".

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

## Trusted publishing

This section records observations, not standing guarantees. On 2026-09-28 each
of the original five crates (all except `purrassist-derive`) was confirmed to
have a crates.io trusted publisher configured against:

- Repository: `Blackcat-Informatics/purrassist`
- Workflow: `publish.yml`
- Environment: `crates-io`

On 2026-10-12 the `purrassist-derive` trusted publisher was observed through
the crates.io API with the same repository and workflow but no environment.
The owner decided to keep it without an environment. crates.io accepts a token
only from the workflow file it names, `publish.yml` (it reads the filename from
the OIDC `workflow_ref` claim), so no other workflow, including the MCP Registry
one, can obtain a crates.io publishing token. Within `publish.yml` only the
`publish` job, which runs in `crates-io`, has `id-token: write`. Without an
environment, any job that a later edit of `publish.yml` grants
`id-token: write` could obtain a `purrassist-derive` token too.
`scripts/check-release.py` therefore holds `publish.yml` to a closed shape:
exactly the `gate` and `publish` jobs, no YAML anchors or aliases, no backslash
escapes, no `!!`, verbatim or letter-named YAML tags, no block-scalar
permissions, no `write-all` anywhere outside a comment, and every `id-token` key
(block, inline or quoted) inside the `publish` job. Changing that shape needs a
matching change to the check.

Because a repository, workflow or environment rename would silently break
trusted publishing, re-confirm all six through the crates.io API immediately
before tagging.

A trusted publisher lets `publish.yml`'s `publish` job mint a short-lived
registry token via GitHub OIDC (`permissions: id-token: write`) instead of a stored
`CARGO_REGISTRY_TOKEN` secret. No registry credential is stored in this
repository or its CI configuration at any point.

## Qualification workflow (release-qualification.yml)

`release-qualification.yml` runs on pushes of `rust-v*` tags and on manual
dispatch. Its single `package` job (90-minute allowance, Rust 1.98.0) runs these
steps in order:

1. `Install release toolchain`: stable Rust 1.98.0 with Clippy and rustfmt.
2. `Install pinned coverage-feedback toolchain`: the registered nightly
   compiler and its LLVM tools, used only by the isolated coverage probe.
3. `Verify release set and version`: `scripts/check-release.py --tag` on a tag
   run (the tag must match the workspace version), plain
   `scripts/check-release.py` on a manual run.
4. `Verify the release gate`: `make release-gate`, the one gate. It runs
   `make check` (the full local gate; its `make lint` runs workspace Clippy
   with `--workspace --all-targets --locked -- -D warnings` plus Clippy over the
   `test = false` corpus suites), `make corpus`,
   `scripts/test-fuzz-feedback.py` (coverage-feedback corpus growth),
   `scripts/test-verify-published.py` (the published-crate verifier) and
   `make package`, in that order.

`verify.yml`'s `release-gate` job runs the same `make release-gate` with the
same two toolchains and environment on every pull request, so a lint or test
that fails only on the release toolchain fails before the tag, not at it. The
job is not yet a required status check on `main`, so a red run does not by
itself block a merge; making it required is a repository-owner setting.
Reproduce it locally with `RUSTUP_TOOLCHAIN=1.98.0 make release-gate`.

`make package` packages all six crates with Cargo verification enabled and
audits the archives and license notices. Every package attempt also runs the
packaging isolation and failure regressions before creating archives.
Packaging resolves the real toolchain Cargo through rustup and uses fresh Cargo
home, target and intermediate directories for each invocation. Both compiler
wrapper layers are disabled for these verification builds; previously packaged
same-version crates and build artifacts cannot satisfy them. The verified
archives are retained in a unique ignored `package-archives` run directory
printed by the command, after the complete source/license audit.

This workflow **never publishes, attests, or creates a GitHub release**, and it
holds no `id-token` permission and no registry credential. `verify.yml`'s
`qualification` job gathers the same class of evidence (independent syntax
oracles, coverage-guided/differential fuzzing, packaging, resource
measurement against the documented limits) on main pushes, pull requests, and
manual runs, so regressions are visible long before a tag is cut.

## Publishing (publish.yml): the only workflow that publishes crates

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
  e.g. a replaced registry, or a new `generate_json_witness` tool), the
  committed responses go stale and must be regenerated by hand:

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

## Release step

For the exact `main` commit to be tagged:

1. Set the date of the `[0.1.0]` heading in `CHANGELOG.md` (it reads
   `unreleased` until now) to the tagging date and merge that change; the
   resulting `main` commit is the one every following step names.
2. Verify hosted CI (`verify.yml`) for that commit, and run the portability,
   conformance, syntax-oracle, fuzz, performance, and package checks listed
   in `QUALIFICATION.md`. Every job must pass. The hosted `fuzz` job runs
   `make fuzz-hosted`: all ten targets at the smoke count, with the
   `tool-call` targets passing `--continue-on-known` so each issue #6 panic is
   retained and named in the log (`Known issue #6 panic at execution N`)
   while any other failure still fails the job, and `tool-call-ref` using the
   120 s probe timeout for the issue #11 slow input
   ([FUZZING.md](FUZZING.md#known-limitations)). Before tagging, run
   `make fuzz-hosted` locally on the same commit (campaigns are seeded, so it
   retains the same inputs) and replay every retained issue #6 input through
   the base-commit probe as FUZZING.md's attribution procedure describes;
   each must reproduce its #6 signature on the base. The default `make fuzz`
   still stops on those limitations.
3. Re-confirm all six trusted publishers through the crates.io API (see
   "Trusted publishing"); each must still match the values recorded there.
4. Tag `rust-vVERSION`. Tagging requires explicit authorization, because
   `publish.yml` then publishes, and a published version cannot be undone or
   overwritten. A tag whose release qualification fails publishes nothing and
   may be moved to the fixed commit.
5. After `publish.yml` succeeds, create the GitHub release. Its body is the
   `[0.1.0]` section of `CHANGELOG.md`, including its `### Known limitations`
   subsection, plus the output of `scripts/verify-published.py`.

## After publication

- Install `purrassist-cli` from crates.io into a clean root and run the
  documented decode, plan/application, and MCP examples against that download.
  A local `cargo install --path` proves only the checkout build.
- Update the status lines that read "not yet tagged `rust-vX.Y.Z`" to read
  "published as `rust-vX.Y.Z`", with the same version, in all five documents
  that carry one (`README.md`, `SECURITY.md`, `docs/IMPLEMENTATION.md`,
  `docs/QUALIFICATION.md` and this file). `scripts/check-docs.py` accepts
  either form, requires the workspace version in it, and rejects a mix of the
  two.
- Write an MCP Registry `server.json` for
  `io.github.Blackcat-Informatics/purrassist` and submit it to the MCP
  Registry. Its package entry has `registryType` `cargo`, `identifier`
  `purrassist-cli`, the published `version`, and an stdio `transport`; the
  file's top-level `version` is the same release. The
  registry verifies ownership of a Cargo package from the
  `mcp-name: io.github.Blackcat-Informatics/purrassist` line that
  `crates/cli/README.md` carries, so the published `purrassist-cli` README must
  contain it. Submit it by running the `MCP Registry` workflow
  (`.github/workflows/mcp-registry.yml`), which validates the file with the
  pinned `mcp-publisher`, authenticates with the workflow's GitHub OIDC token
  and publishes it. Bump the file's versions to the new release first.
- Switch the GitHub repository homepage to the docs.rs page once the 0.1.0
  documentation has been built there.

## 0.1.0 publication record

Observed on 2026-10-13:

- `rust-v0.1.0` (signed) names `main` commit
  `598d059f956aa46315b6a4890577684e536c04c8`. An earlier `rust-v0.1.0` on
  `464306c` failed release qualification (stable Rust 1.98 Clippy
  `float_cmp` in a core test), so the publish gate stopped before any
  `cargo publish`. The tag was moved to the fixed commit, and nothing had been
  published from it.
- Release qualification [run 38356797286](https://github.com/Blackcat-Informatics/purrassist/actions/runs/38356797286)
  passed. The [publish run 38356797476](https://github.com/Blackcat-Informatics/purrassist/actions/runs/38356797476)
  published all six crates at `0.1.0` through trusted publishing, and its
  `verify-published.py` step reported: installed CLI and MCP transcripts match
  the committed fixtures, and all six downloaded archives match freshly
  packaged copies.
- `purrassist-cli 0.1.0` was installed from crates.io into a clean root. The
  documented `decode`, `call match_strings`, `call apply_edit` and MCP
  `initialize`/`tools/list` (31 tools)/`tools/call` examples succeeded against
  that download.
- docs.rs built 0.1.0 for the five library crates, and the repository homepage
  now points to <https://docs.rs/purrassist>.
- The GitHub release
  [rust-v0.1.0](https://github.com/Blackcat-Informatics/purrassist/releases/tag/rust-v0.1.0)
  carries the `[0.1.0]` CHANGELOG section and the verification output.
- [`server.json`](../server.json) validates against the MCP Registry
  `2025-12-11` server schema, and `mcp-publisher validate` accepts it. A
  personal device-flow login could not publish it: the registry learns org
  ownership from the login token's own memberships, and the organization's
  third-party application restrictions hide `Blackcat-Informatics` from the
  registry's OAuth app. It is submitted instead by the manually triggered
  [MCP Registry workflow](../.github/workflows/mcp-registry.yml), whose GitHub
  OIDC token the registry maps to the `io.github.Blackcat-Informatics`
  namespace.
