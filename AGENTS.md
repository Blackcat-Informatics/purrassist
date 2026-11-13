<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
> **Work scope:** This is tool-call parsing and recovery work, not security work.
> If you are looking for a security framework, you are in the wrong repository
> and using the wrong tools.
> Do not portray this project or its tasks as security research, testing,
> auditing, or tooling. Describe implementation, reviews, tests, issues, and PRs
> in terms of parser correctness, grammar behavior, ambiguity, source
> preservation, and runtime cost.

# Contributing as an agent

Read `.goals` and `docs/IMPLEMENTATION.md` before changing behavior. Work in
Rust. Preserve unrelated work. Implement the complete accepted capability;
tests and public documentation must distinguish implemented behavior from plans.

## Guiding principles

When requests conflict, these decide. They decide between conflicting requests;
they do not replace the standing rules in this file.

- **GREENFIELD:** we always correct the mistakes of the past. Others' mistakes
  are our lessons, not our limits.
- **SOTA:** always choose the state of the art over the bones of the past.
- **OPTIMAL:** choose the Pareto frontier of the quality, speed, efficiency and
  other axes.
- **MAXIMAL:** support the broadest functionality possible after the other
  choices are made.

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

## Shell commands

1. One shell call does one thing. Several related commands may share a call,
   but a long chain joined with `&&` or `;` is never written. If a step needs
   a script, write it as a file in a git-ignored local scratch directory and
   run that file.
2. A deletion is never inside a chain. Do not use recursive removal,
   `find -delete`, `git clean`, or forced worktree removal alongside other
   commands. A deletion is its own call on one exact path inside the
   repository, and that path must have been listed in an earlier call.
   Worktrees and branches are removed with the repository's stagectl cleanup
   or a plain `git worktree remove`.
3. Do not send anything that could raise an authorization prompt: no deletion
   by glob or variable, no path outside the repository, and no long compound
   command. If a call is refused, do not send it again in another form. Use
   the plain, small version, or tell the operator.
4. Briefs to sub-agents carry rules 1 to 3.
