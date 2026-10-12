<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Contributing

Read `.goals`, `AGENTS.md`, and `docs/IMPLEMENTATION.md`. Work in Rust, preserve
unrelated changes, and keep implemented capabilities distinct from plans.

Run `make check` before committing. Never bypass commit hooks. Focused tests do
not replace the full gate. Changes to recovery should test uncertainty, resource
exhaustion, original-source retention, and whole-output validation as well as the
successful correction. Editing changes should test stale inputs and exact undo.

The core has zero third-party Cargo dependencies and no ambient I/O. Any external
dependency, generated dataset, test tool, or vendored source needs a recorded
purpose, exact identity, license, and provenance. Do not add a dependency merely
to simplify a small algorithm. Keep all source attribution intact.

Use meaningful Rustdoc examples and error semantics. Keep formatting and lint
warnings clean. Capability or performance claims need executable evidence;
`docs/QUALIFICATION.md` records the executable evidence and its limits, including its
Known limitations section.

First-party contributions use MIT OR Apache-2.0 OR MulanPSL-2.0. Add SPDX headers
to new source files. Do not include credentials, personal service endpoints, or
private infrastructure in examples, fixtures, metadata, or documentation.
