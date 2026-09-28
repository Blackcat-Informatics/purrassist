<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Rust release procedure

PurRAssist is not yet release-qualified or published. Follow the acceptance
criteria in `IMPLEMENTATION.md` and close the open work in `QUALIFICATION.md`
before creating a release tag or publishing an irreversible crate version.

The release set is `purrassist-core`, `purrassist-syntax`, `purrassist`,
`purrassist-host`, then `purrassist-cli`. `scripts/check-release.py` proves that
this is exactly the publishable workspace, all versions match, internal version
requirements match, and normal and dev dependencies precede their users. It
runs in `make check` and `make package`.

On `rust-vVERSION` tags, the release-qualification workflow checks the tag
against the workspace, runs the full local gate on stable Rust 1.98.0, packages
all five crates with Cargo verification enabled, and audits the archives and
license notices. Packaging uses a commit-scoped Cargo home so a previously
packaged crate at the same version cannot satisfy a later crate's verification.
It does **not** publish, attest, or create a GitHub release.
The former SLSA template produced provenance for placeholder text files; it
was removed because those files were unrelated to the crates.

Before a first publication, verify hosted CI for the exact source commit,
run the portability, conformance, syntax-oracle, fuzz, performance, and package
checks listed in `QUALIFICATION.md`, and inspect the generated `.crate` files.
Confirm the five crate names and current versions on crates.io immediately
before publishing. A registry 404 is only a current observation, not a name
reservation. Publishing a version cannot be undone or overwritten.

New crate records require a one-time manual, authenticated publication in the
checked dependency order. Cargo verification stays enabled; wait until each
version is served by crates.io before publishing its dependent. Do not put
registry credentials in this repository or CI. After the initial records exist,
configure a trusted publisher for each crate, bound to this repository and
the eventual publication workflow. Trusted publishing does not create the
first record. Add an automatic publishing lane only when its registry preflight,
partial-publish recovery, and downloaded-package verification have executable
coverage. The current tag lane is intentionally qualification-only.

After publication, install `purrassist-cli` from crates.io into a clean root
and run the documented decode, plan/application, and MCP examples against that
download. A local `cargo install --path` proves only the checkout build.
