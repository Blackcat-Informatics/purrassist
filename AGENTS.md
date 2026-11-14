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

Read `docs/IMPLEMENTATION.md` before changing behavior. Work in Rust. Preserve
unrelated work. Implement the complete accepted capability; tests and public
documentation must distinguish implemented behavior from plans.

## Goals

`.goals` is the record; this is its content.

- **Greenfield first.** Take the best available design. Learn from the limits
  of earlier work without accepting them. Correct past mistakes when found.
- **Rust first.** All core functionality is high-quality Rust. Python, wasm
  and the C ABI are interfaces to it, not places where work happens.
- **Subsume, extend, enhance.** If something we use holds us back, take over
  its function, extend it, then improve it past the original.
- **Low optionality, hard failures.** No optional components, conditional
  imports or swallowed errors; fail early with an actionable message. This is
  a rule about project components and dependencies, not about code constructs.

Three maximums bound every choice: **utility** (err toward the broadest
function), **performance** (seek the optimal Rust solution) and
**portability** (code that compiles to wasm is the core value).

When requests conflict, decide in this order. This order settles conflicts; it
does not replace the standing rules in this file.

1. **Greenfield:** correct the mistake, do not preserve it.
2. **State of the art** over inherited practice.
3. **Optimal:** the Pareto frontier of quality, speed and efficiency.
4. **Maximal:** the broadest function left after the choices above.

## Contracts

- Keep `purrassist-core` free of third-party runtime dependencies and ambient
  filesystem, network, clock, process, and random-number access.
- Recovery retains original input and reports transformations, losses, and
  inferences. A validated guess is still a guess.
- Resource exhaustion is an explicit outcome; it must not become a success,
  an unsupported claim of impossibility, or a swallowed error.
- Target optimizations must preserve the documented result contract.
- Register and justify third-party dependencies in `docs/DEPENDENCIES.md`,
  including transitive, build-only, test-only, generated-data, and vendored
  dependencies. Pin exact versions.
- Preserve license and source provenance when reusing material. Research
  informs the work; it is never copied or closely paraphrased into it.
- No user-specific services, credentials, or private infrastructure belong in
  source, fixtures, documentation, or publication metadata.

All source files carry SPDX headers; files that cannot are annotated in
`REUSE.toml`. Public APIs have Rustdoc with meaningful examples and error
semantics. Capability claims require executable evidence.

## Rust

Optimize for reading, maintenance and compile-time guarantees.

**Workspace**

- Layering runs one way: `purrassist-core` (pure, no I/O) depends on no
  other workspace crate; `syntax` and `toolgraph` build on it; `purrassist`
  is the facade; `host` owns ambient access; `cli` is a thin binary that
  parses input and calls libraries. A lower crate never depends on a higher
  one.
- Prefer small crates. Keep hand-written files under 5,000 lines; split a
  file that grows past that when you are already changing it.
- Default to private, then `pub(crate)`. Every `pub` item is a maintenance
  commitment, and `unreachable_pub` is linted.
- Before writing a helper, look for an existing one: `docs/HELPER-CENSUS.md`,
  `docs/HELPERS.md` and `helpers-ledger.toml`. The gate rejects duplicates.
  For RDF work, use what the pinned purrdf crates provide before building
  a local equivalent.

**Types**

- Make illegal states unrepresentable. Parse into a domain type at the
  boundary and pass that type on; do not pass a raw `String` or integer that
  carries domain meaning across modules.
- Encode lifecycles in types, not in runtime booleans.
- Accept `&str`, `&[T]` and `impl AsRef<Path>` over `&String`, `&Vec<T>` and
  `&PathBuf`. Use `Cow` when a function rarely changes its input.
- Use `let ... else` for early returns. Put `#[must_use]` on pure functions,
  builders and result types.

**Errors and output**

- Library errors are typed enums a caller can match on, and they keep their
  source. Never flatten a cause into a string.
- New non-test code does not call `unwrap`. An `expect` there states the
  invariant that makes failure impossible.
- Libraries do not print. Only the binaries write to standard output or
  standard error.
- `unsafe_code` is denied workspace-wide. An exception is scoped to one
  file, justified there, and registered in `docs/DEPENDENCIES.md`.

## Tests

A permanent test runs on every change, forever, so it must earn its place.

- Each added test protects a named behavior, regression, boundary or
  invariant, and says which in its doc comment. If you cannot state that
  easily, do not add the test.
- Test behavior through the public entry point, not the implementation. No
  tests for accessors, plain data mappers, or what the compiler guarantees.
- Prefer one test over a generated range of inputs to many copied cases.
  Prefer an in-memory fake to a mock that asserts how it was called.
- Recovery changes test uncertainty, resource exhaustion, original-source
  retention and whole-output validation as well as the successful correction.
  Editing changes test stale inputs and exact undo.
- Remove tests that are outdated or overlap. Do not use mutation testing.

## Verification

- `make check` is the local gate: formatting, repository hygiene, the helper
  census, Clippy, tests and docs, all with warnings as errors. Packaging and
  publication require their own evidence (`docs/RELEASE.md`).
- While developing, run the narrowest command that proves the change: one
  test, one crate. Run `make check` when the change is ready to land, not
  after every edit. A focused test is not the gate.
- Evidence comes from the gate's own command, flag for flag. A hand-written
  cargo command with other flags proves nothing about the gate.
- Never bypass commit hooks: not by a commit option that skips verification,
  an environment variable, or another way of creating the commit. Fix the
  failed check or stop without committing.
- Take an exit status from the tool itself, never through a pipe. Write long
  output to a log with the status after it, and read the log once the command
  has ended. Start the log with the working directory and the commit.
- Run one build or test job at a time. Parallel workers share one lock file.
- Do not poll. Run a command in the foreground with a sufficient timeout, or
  in the background and act when it completes.

## Working practice

- Do git work in a worktree under `.worktrees/`. Keep notes, logs, evidence
  and scratch files in the repository's git-ignored scratch directory. Keep
  nothing you need in a temporary directory, and write nothing outside the
  repository.
- Track work in issues and pull requests. Work is done when it merges to
  `main`. Land open work, oldest first, before starting more.
- Keep the documentation current in the same change as the behavior.
- Check the design against SOLID as part of planning and review. Refactor
  what you touch when that is the better result.
- A brief to another agent is all it knows: the goal, what is ruled out, the
  files, the exact scope, the worktree and the rules that bind it. A reviewer
  gets the change and the requirement, never your conclusion. Treat any
  agent's report as a claim, and read its evidence before you rely on it.
- Take times from the clock, not from memory.

## Shell commands

1. One shell call does one thing. Related commands may share a call, but do
   not write a long chain joined with `&&` or `;`. If a step needs a script,
   write it as a file in the git-ignored scratch directory and run that
   file.
2. A deletion is its own call on one exact path inside the repository, and
   that path was listed in an earlier call. No recursive removal,
   `find -delete`, `git clean` or forced worktree removal inside a chain.
   Remove worktrees and branches with the repository's stagectl cleanup or a
   plain `git worktree remove`.
3. Send nothing that could raise an authorization prompt: no deletion by glob
   or variable, no path outside the repository, no long compound command. If
   a call is refused, do not resend it in another form. Use the plain, small
   version or tell the operator.
4. Briefs to other agents carry rules 1 to 3.
