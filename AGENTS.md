<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Contributing as an agent

Read `.goals` and `docs/IMPLEMENTATION.md` before changing behavior. Work in
Rust. Preserve unrelated work. Implement the complete accepted capability;
tests and public documentation must distinguish implemented behavior from plans.

## Verification

Never bypass commit hooks, including through `--no-verify`, `-n`, environment
variables, or alternative commit creation paths. Fix failed checks or stop
without committing. A focused test is not the full gate. `make check` is the
local verification gate; packaging and publication require their own evidence.

## Contracts

- Keep `purrassist-core` free of third-party runtime dependencies and ambient
  filesystem, network, clock, process, and random-number access.
- Recovery retains original input and reports transformations, losses, and
  inferences. A validated guess is still a guess.
- Resource exhaustion is an explicit outcome; it must not become a success,
  an unsupported claim of impossibility, or a swallowed error.
- Target optimizations must preserve the documented result contract.
- Register and justify third-party dependencies, including transitive,
  build-only, test-only, generated-data, and vendored dependencies.
- Preserve license and source provenance when reusing material.
- No user-specific services, credentials, or private infrastructure belong in
  source, fixtures, documentation, or publication metadata.

All source files carry SPDX headers. Public APIs have Rustdoc with meaningful
examples and error semantics. Capability claims require executable evidence.
