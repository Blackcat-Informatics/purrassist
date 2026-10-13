# Issue #1: Complete PurRAssist 0.1.0 plan and release qualification

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
## Objective

Finish the accepted PurRAssist 0.1.0 implementation and release plan in [`.goals`](https://github.com/Blackcat-Informatics/purrassist/blob/main/.goals), [`docs/IMPLEMENTATION.md`](https://github.com/Blackcat-Informatics/purrassist/blob/main/docs/IMPLEMENTATION.md), and [`docs/QUALIFICATION.md`](https://github.com/Blackcat-Informatics/purrassist/blob/main/docs/QUALIFICATION.md). Keep the original Rust-first, portable, dependency-minimal scope; a passing subset is not release completion.

Current `main` includes `be82a33` (`resolve_string` and evidence-bearing string selection). No crates.io publication has been verified. The checkout also has **uncommitted, unverified matching-quality work** in `README.md`, `crates/core/src/matching.rs`, `crates/core/tests/string_selection.rs`, and `crates/core/tests/matching_quality.rs`; review it as work in progress, not as shipped behavior.

## Remaining implementation

- Extend composed JSON recovery and witness generation, regex coverage, recursive-schema/resource auditing, and explicit behavior for unsupported vocabularies and formats. Preserve whole-schema validation, exact input, losses, and separately labeled inferences.
- Complete source-preserving Markdown section/list/table/reference editing and adversarial preservation/replay checks; finish Rust, CLI, and MCP access to all accepted document operations.
- Complete resource accounting across candidate generation, receipts, schema compilation, helpers, and external syntax parser allocations. Exhaustion and cancellation must be explicit and must not yield partial successes.
- Extend alternative-specific evidence/loss/inference ledgers for every operation that can return competing interpretations.
- Broaden independent oracles, composed-recovery goldens, Unicode/path/phonetic ambiguity cases, incorrect-correction evaluation, coverage-guided fuzzing beyond JSON, and resource/performance measurements. Record quality, incorrect corrections, latency, allocation, memory, build cost, and artifact size with stated limits.
- Qualify Windows filesystem durability and crash/retry/stale/undo boundaries, plus native/WASM comparisons using fixed helper transcripts.

## Release acceptance

- Run the official JSON Schema and CommonMark/GFM suites, independent syntax oracles, preservation/replay/ledger properties, bounded fuzzing, portability checks, and the full `make check` gate with warnings as errors. Keep focused tests distinct from the full gate.
- Audit the final five crate archives and license/source notices; verify installed CLI/MCP behavior from actually downloaded packages, not only a local path install.
- Confirm hosted CI on the final commit, set up registry/trusted publishing in dependency order, verify publication and downloads, then make the release. Do not describe the current qualification-only workflow as a publish workflow.
- Keep `purrassist-core` free of third-party runtime dependencies and ambient I/O; register and justify every other dependency, including tooling/test/build/vendor components.

## Workflow

Use an issue branch and reviewable PR for further changes; do not push implementation commits directly to `main`. Preserve the current uncommitted work until it is reviewed. Never bypass commit hooks. For any GitHub PR/branch merge, use the repository owner's required `ghprsq` workflow.


## Comments (77)

### paudley — 2026-09-28T19:06:22Z

# Implementation Plan for Issue #1 — Complete PurRAssist 0.1.0 plan and release qualification

## Context
Issue #1 is the umbrella for finishing the accepted 0.1.0 scope (`.goals`, `docs/IMPLEMENTATION.md`,
`docs/QUALIFICATION.md` "Remaining release work") and qualifying/publishing the release. The issue states
"a passing subset is not release completion". User decisions this session:
- Scope shape: **one branch / one PR, all workstreams**, one task + commit per workstream.
- Publishing: *"There is a crates token in the environment, use it to mint empty 0.0.0 releases and then I'll
  setup trusted publishing on them each, then we'll do a proper release."*
- Uncommitted matching-quality WIP in the main checkout: **copy into branch & finish** (main checkout untouched).
- Tree-sitter allocation bound: **counting allocator + progress-callback break** (scoped `unsafe`).

The Enhancement Audit restructured the plan around four core structures (adopted; cost visible per task):
a **provenance derivation forest** for receipts (Task 4), a **symbolic-derivative regex engine** (Task 6), the
**Attouche et al. witness-generation algebra** for JSON Schema (Task 8), and a **patch algebra with
compose/invert/commute laws** for all edits (Task 9). Scratch accounting becomes type-enforced (Task 3).

## Issue Summary
- R1 Composed JSON recovery + witness generation, regex coverage, recursive-schema/resource audit, explicit
  unsupported vocabulary/format behavior; preserve whole-schema validation, exact input, losses, labeled inferences.
- R2 Source-preserving Markdown section/list/table/reference editing; adversarial preservation/replay checks;
  Rust, CLI and MCP access to all accepted document operations.
- R3 Resource accounting across candidate generation, receipts, schema compilation, helpers, external parser
  allocations; exhaustion/cancellation explicit, never partial success.
- R4 Alternative-specific evidence/loss/inference ledgers for every operation returning competing interpretations.
- R5 Independent oracles, composed-recovery goldens, Unicode/path/phonetic ambiguity, incorrect-correction
  evaluation, coverage-guided fuzzing beyond JSON, measurements (quality, incorrect corrections, latency,
  allocation, memory, build cost, artifact size) with stated limits.
- R6 Windows filesystem durability and crash/retry/stale/undo boundaries; native/WASM comparisons with fixed
  helper transcripts.
- R7 Official JSON Schema + CommonMark/GFM suites, independent syntax oracles, preservation/replay/ledger
  properties, bounded fuzzing, portability checks, `make check` with warnings as errors; focused tests ≠ full gate.
- R8 Audit five crate archives + license/source notices; verify CLI/MCP from downloaded packages.
- R9 Hosted CI on final commit; registry records + trusted publishing in dependency order; verify publication and
  downloads; release. Never call the qualification-only workflow a publish workflow.
- R10 Core has zero third-party runtime deps and no ambient I/O; register and justify every other dependency.
- R11 Issue branch + PR. R12 Preserve the uncommitted WIP until reviewed. R13 Never bypass hooks.
  R14 Merges use the owner's `ghprsq` workflow.

## Baseline (overridable defaults, from `.baseline`)
None — no `.baseline` present.

## Standing Constraints (inviolable, from `.goals`)
GREENFIELD-FIRST (SOTA, no compromises); RUST-FIRST; SUBSUME, EXTEND, ENHANCE; LOW/NO OPTIONALITY, HARD FAILS
(project/packaging level, not code); MAXIMAL UTILITY; MAXIMAL PERFORMANCE; MAXIMAL PORTABILITY (wasmable core).
From `AGENTS.md`: core free of third-party runtime deps and ambient fs/net/clock/process/rng; recovery retains
original input and reports transformations/losses/inferences; exhaustion explicit; SPDX headers on all sources;
public APIs have Rustdoc with examples + error semantics; capability claims need executable evidence; register and
justify every dependency incl. generated data and tooling; no user-specific infra in source/fixtures/docs; never
bypass hooks.

## Completeness Contract
| Requirement / Acceptance Criterion | Task # |
|---|---|
| R12 preserve + finish uncommitted matching WIP | 2 |
| R5 Unicode/phonetic ambiguity: equivalence-lattice matching with protected token classes | 2 |
| R3 type-enforced scratch accounting across schema, pattern, recovery, receipts, candidate generation, helpers; measured-allocation oracle | 3 |
| R4 per-alternative ledgers for every ambiguous operation (derivation forest) | 4 |
| R3 tree-sitter (external parser) allocation bound incl. queries, cancellation | 5 |
| R1 regex coverage (full ECMA-262 `u` mode incl. properties, named groups, generation) | 6 |
| R1 explicit format behavior (always-evaluated annotation evidence; vocabulary-driven assertion; all official format suites) | 7 |
| R1 explicit unsupported-vocabulary behavior (required → compile error naming the URI; optional → schema compiles and every compile and recovery receipt carries evidence naming the ignored vocabulary URI and its unevaluated keywords; the rest of the schema is still validated) | 7 |
| R1 composed recovery + witness generation + recursive-schema audit; unsatisfiability outcome | 8 |
| R5 composed-recovery goldens + known-origin corruption oracle | 8 |
| R2 Markdown structural ops, layout recovery, patch algebra, governed rebase | 9 |
| R2 Rust/CLI/MCP access to all document ops (operation registry); CLI≡MCP for every tool | 10 |
| R2/R7 adversarial preservation/replay/ledger property campaign (patch laws) | 11 |
| R5/R7 coverage-guided + differential fuzzing beyond JSON | 12 |
| R5 oracles, path ambiguity, incorrect-correction evaluation with statistical bounds, measurements with limits | 13 |
| R6 Windows durability + crash/retry/stale/undo boundaries | 14 |
| R6 native/WASM comparison with fixed helper transcripts; wasm32-unknown-unknown executed | 15 |
| R7 portability (wasip1, unknown-unknown, Windows, macOS) | 5, 14, 15, 17 |
| R10 dependency/tooling/data registration; hygiene gaps | 16 (+ each task registers its own additions) |
| R7 official JSON Schema suite (full census, zero skips) | 6, 7 |
| R7 CommonMark/GFM suites + syntax oracles in CI | 11, 17 |
| R7/R9 hosted CI lanes; publish workflow distinct from qualification | 17 |
| R8 archive/notice audit + downloaded-package CLI/MCP verification tooling | 18 |
| R9 registry records (0.0.0 placeholders) in dependency order | 1 |
| R9 trusted publishing configured by user | 1 (settings posted), R (confirmation via AskUserQuestion) |
| Docs distinguish implemented vs planned; QUALIFICATION "Remaining release work" cleared | 19 |
| R7 `make check` + hosted CI green on final commit | N (hosted `verify.yml` green on PR head, which runs `make check`); stage-3 finalize runs local `make check` (stage-1 rules forbid it here); R re-confirms on merge commit |
| R9/R8 publish 0.1.0, verify downloads + installed CLI/MCP, release | R |
| R11 branch + PR | 0, N |
| R13 never bypass hooks | every commit step |
| R14 merge via `ghprsq` | R |

## Enhancements Considered and Declined
- None of the audit items were declined. Two reviewer suggestions were declined:
  - *Rewrite new release/qualification tooling as a Rust `xtask`* — every existing gate (`check-repository.py`,
    `check-release.py`, `package-workspace.py`, `measure-resources.py`, `fuzz-json.py`, `check-wasm.py`) is Python
    and invoked from the Makefile; splitting the tooling language would duplicate shared helpers (manifest
    digests, tooling pins) with no functional gain. Python version is registered in `vendor/tooling.json` (Task 16).
  - *std-only Windows durability* (my original draft) — replaced by `windows-sys` write-through (Task 14) because
    `std::fs::rename` gives no write-through guarantee.

## Task 0: Worktree & Branch Setup (MANDATORY FIRST STEP)
```bash
REPO_ROOT=$(git worktree list --porcelain | awk '/^worktree /{print $2; exit}')
BRANCH="paudley/1-release-qualification"
WORKTREE="$REPO_ROOT/.worktrees/1-release-qualification"
EXCL="$REPO_ROOT/.git/info/exclude"
grep -qxF '/.worktrees/' "$EXCL" 2>/dev/null || echo '/.worktrees/' >> "$EXCL"
git -C "$REPO_ROOT" fetch origin
git -C "$REPO_ROOT" worktree add "$WORKTREE" -b "$BRANCH" origin/main
cd "$WORKTREE" && git branch --show-current   # must print paudley/1-release-qualification
cargo fetch --locked && cargo check -p purrassist-core
gh issue comment 1 --body-file /home/paudley/.claude/plans/1-peppy-bubble.md
```
FORBIDDEN in Task 0: `make check|test|regen|sync|all|ci`, workspace-wide `cargo test`. Pre-existing failures on
origin/main are noted in one line and not chased. No `sleep` polling. Budget < 15 min.

## Task 1: Mint crates.io 0.0.0 name records (user-authorized) and post trusted-publishing settings
Irreversible + outward-facing; explicitly authorized by the user this session.
1. Identify the token env var by name only (`env | cut -d= -f1 | grep -i 'cargo.*token\|crates.*token'`); never print values.
2. For each of `purrassist-core purrassist-syntax purrassist purrassist-host purrassist-cli`:
   `curl -sS -A 'purrassist-release (https://github.com/<owner>/<repo>)' -o /dev/null -w '%{http_code}'
   https://crates.io/api/v1/crates/<name>` must print `404`. Any `200` owned by someone else → STOP, AskUserQuestion.
3. In `mktemp -d` (outside the repo) generate five minimal lib packages: `version = "0.0.0"`, workspace license
   expression and all three license files included, repository/homepage from workspace metadata, description
   "Name reservation for PurRAssist; see 0.1.0.", `src/lib.rs` = SPDX header + `//! Reserved name.`, no deps.
   `cargo package --list` must show the license files.
4. `cargo publish` (full verification, never `--no-verify`) in order core → syntax → purrassist → host → cli;
   after each, poll the version endpoint (same UA) with bounded retries (≤ 10 × 30 s, no bare sleeps in a wait
   loop beyond this bounded retry) until `200`; on HTTP 429 (new-crate rate limit) honor `Retry-After` and
   continue; any other error → STOP and report.
5. Comment on issue #1: the five records, and the exact trusted-publisher settings to enter on each crate
   (repository `<owner>/<repo>` from `gh repo view --json nameWithOwner -q .nameWithOwner`, workflow `publish.yml`,
   environment `crates-io`). Task 17 uses exactly these values. Confirmation is collected at Task R.
Acceptance: `for c in <five>; do test "$(curl -sS -A '<UA>' -o /dev/null -w '%{http_code}'
https://crates.io/api/v1/crates/$c/0.0.0)" = 200 || exit 1; done` exits 0. No repository commit (nothing
in-tree changes); the issue comment is the record.

## Task 2: Finish the matching-quality WIP as an equivalence lattice (R12, R5)
1. Carry the WIP without touching the main checkout: `git -C "$REPO_ROOT" diff -- README.md
   crates/core/src/matching.rs crates/core/tests/string_selection.rs | git apply` in the worktree, and
   `cp "$REPO_ROOT/crates/core/tests/matching_quality.rs"` into the worktree.
2. Restructure matching tiers as an explicit lattice of normalizers ordered by refinement: identity ⊑ NFC ⊑
   case-fold ⊑ width-fold ⊑ diacritic-strip ⊑ UTS-39 skeleton ⊑ depunctuation ⊑ phonetic. Resolution happens
   at the finest level where the query has a unique match; each level is a named signal with evidence.
   UTS-39 confusables data comes from the single Unicode generator (Task 6 step 1 is scheduled first within
   this task: the generator lands here and is extended in Tasks 6/7).
3. Generalize `matching.numeric-identity-v1` into **protected token classes** that no normalizer may collapse:
   decimal digit runs, negation markers (`not`, `no`, `non-`, `un-` prefix list, `!`), semver components,
   ISO dates, hex digests (≥ 7 hex), units (`ms`/`s`, `kb`/`mb`/`gb`…). Differences in a protected class exclude
   the candidate with exclusion evidence unless a caller alias matches exactly. Applied uniformly in `resolve`,
   `rank`, and `match_strings`.
4. Charge ScratchBytes before allocating every normalized key (via Task 3 types once they land; in this task via
   explicit `charge`/`release_scratch`), fix fmt, wrap `resolve_governed` Rustdoc and add an example with a
   protected-class exclusion.
5. `docs/MATCHING-QUALITY.md`: the lattice, protected classes, the curated set, and the counts emitted by
   `matching_quality.rs` (test asserts the documented numbers; a mismatch fails).
Acceptance: `cargo test -p purrassist-core --test matching_quality --test string_selection` passes;
`cargo fmt --all --check`; tests assert `rank("invoice-1001", ["invoice-1002"])` is not `Resolved`, and one
test per protected class; `matching_quality.rs` asserts zero incorrect automatic resolutions and asserts the case
count that the doc quotes. Commit → push → issue update.

## Task 3: Type-enforced scratch accounting + measured-allocation oracle (R3)
1. `crates/core/src/contract.rs`: the governor's scratch counter moves to a shared `Arc<ScratchMeter>` (atomics,
   same pattern as `CancellationToken`). New core types `ScratchVec<T>`, `ScratchString`, `ScratchMap<K,V>`
   whose growth goes through `try_reserve` after `meter.charge(bytes)` and whose `Drop` releases exactly the
   charged bytes; `ScratchLease` for opaque byte reservations (release on drop, no governor borrow). Peak is
   tracked. Exhaustion returns `BudgetExceeded(ScratchBytes)` before allocation.
2. Convert schema compile (`schema/compile.rs`, `build.rs`), `pattern.rs` (`Pattern::compile_governed`),
   `schema/recovery.rs` (`change()` clones and `to_json` keys, `finish` serialization), `recovery/proposals.rs`
   (`members.to_vec()`, witness clones, dedup set, `vec![item; count]` by encoded size), `schema/evaluate.rs`
   (`Violation` strings, `active` path set), syntax-repair loss copies, receipt ledgers/`retain_source`, matching,
   edits, and host `helpers.rs` request preparation to these types.
3. Measured-allocation oracle: `crates/core/tests/allocation_oracle.rs` installs a test-only counting
   `#[global_allocator]` (test binary only, no deps; `#[allow(unsafe_code)]` scoped to the test file) and asserts,
   for every governed public operation on a representative workload set, `measured_bytes_allocated_during_op ≤
   governor.peak_scratch_bytes + input_len + K` with `K` a documented per-operation constant.
Acceptance: `cargo test -p purrassist-core --test allocation_oracle --test resource_accounting --test recovery
--test schema_budgets` and `cargo test -p purrassist-host --test helpers`. `resource_accounting.rs` tests:
repeated `Pattern::is_match` returns `usage.scratch` to its pre-call level; recovery with a budget one byte below
a formerly-uncharged allocation yields `BudgetExceeded(ScratchBytes)` with no partial output; helper preparation
exhaustion yields `BudgetExceeded` and no request. Commit → push → issue update.

## Task 4: Receipts as a provenance derivation forest (R4)
1. `crates/core/src/provenance.rs`: a deduplicated DAG of evidence/loss/inference/validation nodes; each
   alternative is a derivation (set of node ids + cost). Annotations over the forest: tropical (min-plus) cost for
   ranking/k-best, counting (number of derivations), Boolean validation. Shared evidence = intersection of all
   derivations (published as top-level ledgers); per-alternative ledgers = derivation minus shared nodes, published
   in `AlternativeReceipt` (fields unchanged in meaning; `output` digest-bound). A `distinguishing(a, b)` query
   answers "why is this ambiguous". Building charges EvidenceEntries/ScratchBytes per node once (not per alternative).
2. Move `EditPlan`/`AppliedEdit` JSON encoding from `crates/host/src/tools/edits.rs:165 encode_plan` into
   `crates/core/src/wire.rs`; host reuses it.
3. Refactor JSON recovery, intent, paths, matching, embeddings onto the forest (existing tests pin behavior), then
   populate alternatives for `edits.rs:249 locate_governed`, `edits/fuzzy.rs:123` (per-candidate evidence and
   Guess inferences leave top-level ledgers), `edits/unified.rs:85`, and `crates/syntax/src/lib.rs:340`.
4. Bump receipt wire version once; commit a golden receipt fixture for the new version; publish a JSON Schema for
   receipts at `docs/schema/receipt.schema.json` validated by our own schema engine in tests.
Acceptance: `cargo test -p purrassist-core --test provenance --test edit_ambiguity --test receipt_wire` and
`cargo test -p purrassist-syntax --test syntax`. Tests: for each of the four edit/syntax sites an ambiguous input
yields `alternatives.len() == plans.len()`, each `output` equals the digest of that plan's wire encoding, top-level
ledgers equal the intersection, EvidenceEntries exhaustion mid-build yields `BudgetExceeded` with no alternatives;
forest with N alternatives sharing E evidence charges E (not N×E) entries; every receipt produced in the test suite
validates against `receipt.schema.json`. Commit → push → issue update.

## Task 5: Tree-sitter allocation bound (R3)
0. Confirm `tree_sitter::set_allocator` and `ParseOptions` progress callback exist in `tree-sitter = "=0.27.0"`
   with `default-features = false`, and enumerate the grammars from `crates/syntax/Cargo.toml` by name
   (expected: python, rust, javascript/jsx, typescript/tsx). If an API is absent → STOP and AskUserQuestion.
1. `crates/syntax/src/alloc.rs` with `#![allow(unsafe_code)]` scoped to the module (workspace lint stays deny):
   `malloc/calloc/realloc/free` wrappers over `std::alloc::System` with a size header; global atomic live/peak
   counters plus a per-parse accounting cell identified in the header (so frees on another thread — `Tree` is
   `Send` — decrement the owning account, never a thread-local). Installed via `std::sync::Once` before any
   tree-sitter object exists; a test proves ordering (first call path installs before `Parser::new`/`Language`
   use). SAFETY comments on every unsafe block; Rustdoc documents the process-global effect for embedders.
2. `inspect`, `apply_checked_governed`, and every `QueryCursor` use: the progress callback checks the
   `CancellationToken` and charges the account delta to ScratchBytes, breaking with `Cancelled` or
   `BudgetExceeded(ScratchBytes)`; retained tree bytes are charged after parse and released on drop.
   Remove the "external to the core governor" disclaimer; document overshoot bound = allocations between progress
   callbacks, measured.
Acceptance: `cargo test -p purrassist-syntax --test allocation --test syntax`; `python3 scripts/check-wasm.py`
(the narrow proof for wasip1 syntax). `allocation.rs`: 1 MiB generated Rust with 64 KiB budget →
`BudgetExceeded(ScratchBytes)` and no tree; ample budget → `peak_scratch_bytes` within 2× of the counter peak;
tree dropped on another thread returns the account to zero; cancellation mid-parse → `Cancelled`; measured
maximum overshoot per named grammar on the oracle corpus ≤ the documented constant in `docs/RESOURCES.md`.
Commit → push → issue update.

## Task 6: Symbolic-derivative ECMA-262 regex engine (R1)
1. Single Unicode 17 generator `scripts/generate-unicode.py` producing all core tables (General_Category,
   Script, Script_Extensions, ECMA binary properties, case folding, UTS-39 confusables, IDNA/UTS-46) with one
   provenance record and digest in `check-repository.py`/`vendor/dependencies.json` (introduced in Task 2 for
   confusables, completed here).
2. `crates/core/src/pattern/` split into `parse`, `ast`, `derive`, `backtrack`, `generate`, `unicode`:
   - Parser: full `u`-mode syntax incl. named groups `(?<n>…)`, `\k<n>`, `\u{…}`, surrogate-pair escapes,
     `\p{…}`/`\P{…}` for General_Category, Script, Script_Extensions and every ECMA-262 binary property.
     Lone braces and class ranges with class-escape ends are SyntaxErrors (spec-correct under `u`).
   - Primary engine: Antimirov/Brzozowski derivatives over Unicode interval-set predicates with a governed,
     lazily-built DFA cache — linear-time matching, native intersection and complement, emptiness check.
     Lookahead supported via derivative extension; backreferences and lookbehind use the backtracking engine,
     selected per pattern and recorded as evidence; backtracking is Work-charged per step.
   - `Pattern::generate(governor, {min_len, max_len, avoid: &[Pattern]})` produces shortest witnesses from the
     derivative automaton (intersection with complements of `avoid`), or proves emptiness (explicit
     `NoCandidate` with "language empty" evidence).
Acceptance: `cargo test -p purrassist-core --test pattern --test schema_conformance`. `pattern.rs` runs the
complete test262 `built-ins/RegExp/property-escapes/generated` set (vendored with BSD-3-Clause license and commit
registered) with a census assertion that every supported property has its file; `(a+)+b` on 10,000 `a`s completes
under Standard Work budget; derivative vs backtracking engines agree on 10,000 deterministic generated
(pattern, input) pairs without backreferences; a test asserts backreference patterns route to backtracking;
`generate` output always matches and never matches any `avoid`; emptiness proven for `a&~a`-style intersections.
Commit → push → issue update.

## Task 7: Explicit format behavior (R1)
- Formats are always evaluated and reported as annotation evidence (pass/fail per instance location), which
  recovery uses as a soft signal. Assertion is driven solely by the dialect's `$vocabulary` declaring
  `https://json-schema.org/draft/2020-12/vocab/format-assertion` (no caller switch).
- First-party validators in `crates/core/src/schema/format/` for every 2020-12 format: date-time, date, time,
  duration, email, idn-email, hostname, idn-hostname (IDNA2008/UTS-46 tables from Task 6 generator + Punycode),
  ipv4, ipv6, uri, uri-reference, iri, iri-reference, uuid, uri-template, json-pointer, relative-json-pointer,
  regex (Task 6 parser). Unknown format under assertion → compile-time `SchemaError("unknown format under
  format-assertion: <name>")`.
- Recovery repairs `date`, `date-time`, `time` violations (normalization) as labeled Guess inferences; other
  format violations yield explicit NoCandidate for that proposal; assertions are never weakened.
- Unsupported vocabularies: an unknown `$vocabulary` entry with `true` fails compile with
  `SchemaError("unsupported required vocabulary: <uri>")`; an unknown entry with `false` compiles, and every
  compile/recovery receipt carries an `unsupported-vocabulary` evidence entry naming the URI and the keywords left
  unevaluated; all other vocabularies are still fully validated.
Acceptance: `cargo test -p purrassist-core --test schema_conformance --test formats --test vocabularies`.
`vocabularies.rs`: given a meta-schema declaring `{"https://example.invalid/vocab/x": true}`, `Schema::compile`
returns that error; given the same URI with `false`, compile succeeds, recovery of an invalid instance still
reports the core-vocabulary violation, and the receipt contains the `unsupported-vocabulary` evidence entry for
that URI listing the schema keywords it left unevaluated. `schema_conformance.rs` runs
every file in `optional/format/` (enumerated from the vendored manifest; the test fails if any file is not wired)
under a dialect declaring format-assertion, plus `optional/format-assertion.json`; census asserted; zero skipped
cases. `formats.rs`: unknown-format-under-assertion compile failure; annotation evidence present without assertion.
Commit → push → issue update.

## Task 8: Witness algebra, composed recovery, recursive-schema audit, goldens (R1, R5)
1. `crates/core/src/schema/algebra/`: implement the Attouche, Baazizi, Colazzo, Ghelli, Sartiani witness
   generation pipeline (VLDB 2022): canonicalize → `not` elimination → `and`-merge → preparation → generation,
   with recursion over the reference graph (incl. `$dynamicRef` dynamic scope via the evaluator's scope stack).
   Uses Task 6 regex intersection/complement for `pattern`/`patternProperties`/`propertyNames`/
   `additionalProperties`, Task 7 formats. Outcomes: witness (validated by the evaluator), **proven
   unsatisfiable** (new explicit outcome with the refutation evidence), or explicit exhaustion.
2. `recovery/proposals.rs` follows `not`, `if/then/else`, `patternProperties`, `additionalProperties` schema,
   `dependentSchemas`, `contains`, `unevaluatedProperties/Items`, `propertyNames`, dynamic `$dynamicRef`; invented
   values come from the algebra (Guess inferences).
3. Recursive-schema audit: `tests/recursive_schemas.rs` covering self-recursion, mutual recursion, `$dynamicRef`
   recursion, non-progressing cycles, and compile-time resource exhaustion on deep reference chains.
4. Goldens + known-origin corruption oracle: `tests/data/recovery-goldens/*.json` (≥ 60 curated cases across
   every applicator) and `tests/corruption_oracle.rs`: take algebra witnesses for the official-suite schemas,
   apply recorded corruptions (truncation, prose wrapping, key typos, stringification, type swaps, duplicate keys),
   and assert recovery returns the original, or `Ambiguous` containing the original, or an explicit non-success
   outcome — never a different `Resolved` value. Bless mode (`PURRASSIST_BLESS=1`) panics if `CI` is set.
Acceptance: `cargo test -p purrassist-core --test witness_algebra --test recursive_schemas --test recovery_goldens
--test corruption_oracle --test recovery`. `witness_algebra.rs`: every schema in the official 2020-12 suite with at
least one valid test instance yields a witness that validates; every schema whose tests are all-invalid for all
instances and is known unsatisfiable (e.g. `false`, `allOf:[{type:string},{type:number}]`) yields
proven-unsatisfiable. Commit → push → issue update.

## Task 9: Patch algebra, Markdown structural editing, layout recovery, governed rebase (R2)
1. `crates/core/src/patch.rs`: `Patch` (source digest, ordered byte-range replacements) with `apply`, `invert`,
   `compose`, `commute`. `EditPlan` (Markdown, exact/fuzzy/unified edits) and host journal entries are `Patch`es.
2. Governed rebase: `Patch::rebase(onto: &Patch, governor)` — a stale plan whose changes commute with the
   intervening change is rebased with a recorded transformation; non-commuting → explicit `Stale` refusal.
   Exposed on `apply_edit`/`apply_file` as an explicit `rebase_from` input (the prior source digest + content).
3. Markdown operations (governed, digest-bound, reparse-checked, inverse provided):
   sections `insert_section`, `remove_section`, `move_section` (heading re-leveling reported), `rename_heading`;
   lists `ListItemEdit::Move{to}`, `renumber_list`; tables `TableRowEdit::Move`, `TableColumnEdit::Move`, and
   direct cell edits on implicit padded cells materialize missing cells (reported transformation);
   references `add_reference`, `relabel_reference` (updates all uses), `remove_reference` (inlines the destination
   into every use — lossless, reported transformation), `unlink_reference` (removes definition and turns uses into
   plain text, each recorded as a loss).
4. Layout recovery: trailing-blank bodies and tab-column bodies inside containers are re-indented to the container
   with a reported transformation; dedicated error messages replace the generic ones where refusal is correct.
Acceptance: `cargo test -p purrassist-core --test patch_laws --test markdown_structure --test markdown_sections`.
`patch_laws.rs`: invert∘apply = identity, compose associativity, commute consistency (`p;q ≡ q';p'`) on generated
patches; rebase success and non-commuting refusal. `markdown_structure.rs`: each op has success, inverse
round-trip byte-identical, CRLF/UTF-8 preservation, stale-digest refusal, exhaustion; the two formerly refused cases
in `markdown_sections.rs:246-258` succeed with a transformation entry. Commit → push → issue update.

## Task 10: Operation registry and complete Rust/CLI/MCP surface (R2, R4)
- `crates/host/src/tools/registry.rs`: one declarative registry per operation (name, input schema, output schema,
  handler); `Tools::definitions()`, CLI `call`, MCP `tools/list` input schemas derive from it; inputs validated by
  our own schema engine.
- New tool `plan_markdown_structure` with typed operations (section insert/remove/move/rename, list move/renumber,
  table row/column move, reference add/relabel/remove/unlink); `plan_markdown_section` stays body-only.
  `recover_files` (Store::recover) and `commit_files` (commit_batch); `effort` on `plan_replacement`, `plan_diff`,
  `inspect_code`; `rebase_from` on `apply_edit`/`apply_file`.
- `crates/cli/tests/protocol.rs`: equivalence iterates every registry entry with a fixture per tool (missing
  fixture fails); `tools/list` names equal the registry; every `Ambiguous` response has one alternative per candidate;
  every tool output validates against its registered output schema.
Acceptance: `cargo test -p purrassist-cli --test protocol` and `cargo test -p purrassist-host`. Commit → push → update.

## Task 11: Adversarial preservation / replay / ledger properties (R2, R7)
`crates/core/tests/preservation_properties.rs`: deterministic in-test LCG generator of Markdown documents from the
CommonMark/GFM corpora with injected CRLF, tabs, NBSP, lazy continuations, nested containers. For every Task 9 op at
every site: plan → apply → reparse → unchanged regions byte-identical; patch laws hold; receipt ledgers cover every
changed byte range. Same for exact/fuzzy/unified edit plans and JSON recovery (original retained; losses cover every
dropped byte). ≥ 10,000 cases per run, fixed seed, failing seed printed.
Acceptance: `cargo test -p purrassist-core --profile verification --test preservation_properties`. Commit → push → update.

## Task 12: Coverage-guided and differential fuzzing beyond JSON (R5, R7)
- Replace `scripts/fuzz-json.py` with `scripts/fuzz.py --target <name>`; `make fuzz TARGET=<name>` replaces
  `make fuzz-json` (docs updated). Coverage feedback: `-C instrument-coverage` region novelty (existing mechanism).
  Targets (examples registered in `crates/core/Cargo.toml`): json, markdown, schema, pattern, unified, fuzzy,
  matching, patch; seeds derived from Task 8 corruption oracle and the official corpora.
- Differential probes: pattern vs V8 (`node`, pinned in `vendor/tooling.json`) on generated patterns/inputs;
  Markdown render vs `cmark-gfm` (pinned version, built into `.cache/`, registered in tooling with license notice).
- `scripts/test-fuzz.py` unit tests (hygiene-wired), including one proving the corpus grows on a seeded target.
- Qualification run: documented iteration counts per target (`docs/FUZZING.md`); baselines committed as
  `docs/qualification/fuzz-<target>-linux-x86_64.json`.
Acceptance: `python3 scripts/test-fuzz.py`; `make fuzz TARGET=<t> FUZZ_ARGS="--iterations 1000"` completes with
zero crashes and zero differential mismatches for all 8 targets (smoke); qualification counts run and baselines
committed. Commit → push → update.

## Task 13: Oracles, ambiguity/incorrect-correction evaluation, measurements (R5)
- Evaluation corpora `crates/core/tests/data/evaluation/` (provenance recorded; ≥ 500 cases each for strings,
  paths, phonetic, JSON recovery): Unicode confusables, width, diacritics, protected classes, negation, path case/
  separators/normalization. `tests/correction_evaluation.rs` asserts zero incorrect automatic `Resolved`, computes
  risk–coverage (precision vs abstention) and the Clopper–Pearson 95% upper bound on the incorrect-correction rate,
  and asserts the bound ≤ the stated limit in `docs/MATCHING-QUALITY.md` (test asserts the documented values).
- Independent oracles: edit-distance BFS oracle exhaustive over all strings of length ≤ 4 on a 3-symbol alphabet;
  path normalization oracle.
- Measurements: bench latency workloads for markdown ops, edits, unified, matching, paths, pattern, syntax, schema;
  allocation counts and peak heap per workload (counting allocator in the bench harness) plus peak RSS; cold build
  cost (fresh `CARGO_TARGET_DIR`); artifact sizes for the native CLI, wasm32-wasip1 and wasm32-unknown-unknown
  modules; recovery bench output committed. All in `docs/qualification/resources-linux-x86_64.json`.
- `scripts/check-resource-limits.py <fresh-json>` compares fresh measurements to the limits table in
  `docs/RESOURCES.md`; hygiene validates only the committed file's schema; the CI qualification job (Task 17)
  regenerates and checks.
Acceptance: `cargo test -p purrassist-core --test correction_evaluation --test matching_oracle`; `make resources`
produces a JSON with the new fields; `python3 scripts/check-resource-limits.py` on it exits 0. Commit → push → update.

## Task 14: Windows filesystem durability (R6)
- `purrassist-host` gains a Windows-only dependency `windows-sys` (registered + justified, notices vendored).
  Replacement uses `SetFileInformationByHandle(FileRenameInfoEx, FILE_RENAME_FLAG_POSIX_SEMANTICS |
  FILE_RENAME_FLAG_REPLACE_IF_EXISTS)` on a write-through handle, falling back to
  `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` where the filesystem rejects POSIX semantics;
  the path taken is recorded as evidence. Directory durability via `FlushFileBuffers` on a
  `FILE_FLAG_BACKUP_SEMANTICS` handle; if access is denied, the write-through rename is the durability boundary and
  this is recorded (not silently ignored). Sharing violations (AV/indexer) → bounded retry then explicit `Busy`.
  ACL inheritance documented in place of unix modes.
- `recover()` reports corrupt/torn/truncated journal entries as `Recovery::Corrupt{id, reason}` instead of failing
  all; orphan `.purrassist-<id>.tmp` files for known journals are reported and removed.
- `crates/host/tests/crash_recovery.rs` re-executes its own test binary with an env var selecting an abort point
  (after each journal write, before/after rename, before marker), then runs `recover()` + retry and asserts
  exact-version outcomes; plus `NotApplied`, missing `prepared.json`, torn record tests. Runs on unix and Windows.
- CI `windows-engines` job also runs `cargo test -p purrassist-host -p purrassist-cli --locked`.
Acceptance: `cargo test -p purrassist-host --test files --test crash_recovery` locally; the hosted `windows-engines`
job is green on the pushed commit (`gh run list --branch paudley/1-release-qualification --workflow verify.yml`
then `gh run view <id> --json jobs`). Commit → push → update.

## Task 15: Native/WASM comparison with fixed helper transcripts (R6, R7)
- `crates/host/examples/portable.rs` (`portable-host`) replays the fixed helper transcripts (OpenAI Responses,
  Anthropic Messages, llama.cpp) from `crates/host/tests/helpers.rs` through `Pending`, printing digest + outputs +
  receipts; host builds for wasm32-wasip1.
- New unpublished harness crate `crates/wasm-harness` (`publish = false`, `cdylib`) exports `portable_main` for
  wasm32-unknown-unknown; `scripts/run-wasm.mjs` (SPDX header, Node pinned in tooling) executes it; output compared
  byte-for-byte with native. `purrassist-core`'s public surface is unchanged.
- `scripts/check-wasm.py` adds host and the unknown-unknown execution.
Acceptance: `python3 scripts/check-wasm.py` passes comparing core, syntax, host (wasip1) and core (unknown-unknown
executed). Commit → push → update.

## Task 16: Registration + hygiene (R10)
SPDX check covers `.mjs`; `vendor/dependencies.json`/`docs/DEPENDENCIES.md` register: Unicode/IDNA/confusables
generated tables, test262 expectations, evaluation corpora, fuzz seeds, `windows-sys` (+ transitive), Node,
cmark-gfm, Python, `crates-io-auth-action`, and the Task 3/Task 5 `unsafe` exceptions with rationale.
`check-release.py` accepts `crates/wasm-harness` as `publish = false`. Core still has zero `[dependencies]`.
Acceptance: `python3 scripts/check-repository.py` and `python3 scripts/check-release.py` exit 0. Commit → push → update.

## Task 17: CI lanes + publish workflow (R7, R9)
- `verify.yml`: `qualification` job (Linux) runs `make syntax-oracles`, `make fuzz` (smoke counts),
  `make package`, `make resources` then `check-resource-limits.py` on the fresh output.
- `.github/workflows/publish.yml`: on `rust-v*` tag; requires `release-qualification` success; environment
  `crates-io`; trusted publishing via `rust-lang/crates-io-auth-action` pinned by SHA; publishes
  core→syntax→purrassist→host→cli waiting for each version to be served; then runs `scripts/verify-published.py`.
  `release-qualification.yml` stays qualification-only and is described as such.
- `docs/RELEASE.md`: placeholder records (Task 1), trusted-publisher settings, publish flow.
Acceptance: `python3 scripts/check-release.py` (extended to validate publish.yml order and SHA pins); hosted
`verify.yml` run on the pushed commit green. Commit → push → update.

## Task 18: Package audit + downloaded-package verification tooling (R8)
- `scripts/check-packages.py` asserts each archive's notice set equals the registered notice set.
- `scripts/verify-published.py --version X`: fresh temp `CARGO_HOME`, `cargo install purrassist-cli --version X`
  from crates.io (no path), runs CLI `call` + `mcp` stdio transcripts (initialize, `tools/list` census = registry,
  one tool per domain) compared with a committed expected transcript; downloads each `.crate` and diffs it against
  the locally packaged archive.
- `scripts/test-verify-published.py` uses a local file-registry fixture (hygiene-wired).
Acceptance: `make package`; `python3 scripts/test-verify-published.py`. Commit → push → update.

## Task 19: Documentation truthfulness (all)
Update `docs/QUALIFICATION.md` (replace "Remaining release work" with executed evidence per item; only Task R's
release steps remain), `docs/IMPLEMENTATION.md`, `README.md`, `docs/RESOURCES.md`, `docs/FUZZING.md`,
`docs/HELPERS.md`, `docs/EDIT-APPLICATION.md`, `docs/CANCELLATION.md`. Rustdoc with examples + error semantics
for every new public API.
Acceptance: `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked`; `cargo test --doc -p
purrassist-core`; `grep -nE "remains open|not complete|still required|remain open" docs/QUALIFICATION.md` returns
only lines under the Task R release heading. Commit → push → update.

### Mechanical Deferral Scan (before Task N)
`git diff origin/main...HEAD | grep -nE 'TODO|FIXME|XXX|unimplemented!|todo!|future work|follow-up|for now|phase 2|out of scope|later PR|not yet|\bstub\b|placeholder'`
— resolve every hit (Task 1's external placeholder crates are outside the repo and not in the diff).

## Task N: Create Pull Request (MANDATORY FINAL STEP)
1. Verify all committed and pushed. The latest `verify.yml` run on HEAD must be green; if not, fix, commit, push,
   and re-check before creating the PR. Record the green run ID.
2. `gh pr create --title "feat(1): complete 0.1.0 scope and release qualification" --body "Closes #1 …"` with
   summary, per-task changes, and the green run ID.
3. `gh pr comment <PR> --body-file /home/paudley/.claude/plans/1-peppy-bubble.md`.
4. Post answers to "What are you least confident about in this work and why?" and "What do I not know but
   should?" to the PR and issue #1.
5. Report: "Stage 1 complete. PR #<num> created for issue #1. Ready for /stage2 1".

## Task R: Release (after stage 2/3; merge via `ghprsq`) — user-directed sequencing
Per the user ("then we'll do a proper release"). After the PR is merged through `ghprsq` and `make check` + hosted
CI are green on the merge commit: AskUserQuestion to confirm trusted publishing is configured on all five crates and
to authorize tagging (irreversible); tag `rust-v0.1.0`; `publish.yml` publishes; run
`scripts/verify-published.py --version 0.1.0` against downloaded packages; create the GitHub release.
Acceptance: the `publish.yml` run for tag `rust-v0.1.0` has conclusion `success`
(`gh run list --workflow publish.yml --branch rust-v0.1.0 --json conclusion`) and its job log shows publish steps
in order core → syntax → purrassist → host → cli;
`for c in purrassist-core purrassist-syntax purrassist purrassist-host purrassist-cli; do test "$(curl -sS -A '<UA>'
-o /dev/null -w '%{http_code}' https://crates.io/api/v1/crates/$c/0.1.0)" = 200 || exit 1; done` exits 0;
`python3 scripts/verify-published.py --version 0.1.0` exits 0 (each downloaded `.crate` matches its local archive
and installed CLI `call`/`mcp` transcripts match the committed expected transcript); `gh release view rust-v0.1.0`
exits 0 and the release notes link the verify-published output.

## Execution Rules
1. Task 0 mandatory; all work in `.worktrees/1-release-qualification`; never on main.
2. Commit after EACH task (github-commit-agent), push after each (github-push-agent); never batch.
3. Sub-agents implement (general-purpose, sonnet) with the NO DEFERRALS clause; reject summaries with deferral language.
4. Never run the full validation gate in stage 1; narrowest proving command only. Pre-commit hooks are the gate.
5. Pre-existing failures on origin/main are not ours. Never `sleep`-poll. Never bypass hooks.
6. Escalation ladder: retry → read → instrument → root-cause-analyst → user.


### paudley — 2026-09-28T19:07:06Z

## Task 1 complete: crates.io name records (0.0.0 placeholders)

Published in dependency order with full `cargo publish` verification; each version confirmed served (HTTP 200 from the crates.io API) before the next:

1. `purrassist-core` 0.0.0
2. `purrassist-syntax` 0.0.0
3. `purrassist` 0.0.0
4. `purrassist-host` 0.0.0
5. `purrassist-cli` 0.0.0

Each placeholder is an empty lib (`//! Reserved name`) carrying the workspace license expression and all three license files. No repository files changed.

### Trusted-publisher settings to enter on each of the five crates (crates.io → Settings → Trusted Publishing → GitHub)

| Field | Value |
|---|---|
| Repository owner | `Blackcat-Informatics` |
| Repository name | `purrassist` |
| Workflow filename | `publish.yml` |
| Environment | `crates-io` |

The branch's `.github/workflows/publish.yml` (Task 17) uses exactly these values. Confirmation that they are configured will be requested before tagging `rust-v0.1.0` (Task R).


### paudley — 2026-09-28T19:24:20Z

## Trusted publishing confirmed

The owner reported trusted publishing is set up. Verified against the crates.io API (`/api/v1/trusted_publishing/github_configs`), every crate has exactly the expected GitHub config:

| Crate | Owner | Repo | Workflow | Environment |
|---|---|---|---|---|
| purrassist-core | Blackcat-Informatics | purrassist | publish.yml | crates-io |
| purrassist-syntax | Blackcat-Informatics | purrassist | publish.yml | crates-io |
| purrassist | Blackcat-Informatics | purrassist | publish.yml | crates-io |
| purrassist-host | Blackcat-Informatics | purrassist | publish.yml | crates-io |
| purrassist-cli | Blackcat-Informatics | purrassist | publish.yml | crates-io |

Task R's trusted-publishing precondition is satisfied; tagging `rust-v0.1.0` still requires explicit authorization after merge.


### paudley — 2026-09-28T19:38:38Z

## Task 2 committed and pushed — `2e28692`

**feat: resolve strings through a refinement lattice with protected tokens**

- Carried the reviewed matching-quality WIP from the main checkout (main checkout left untouched) and fixed its defects (fmt, dangling doc link, Rustdoc, ScratchBytes charging for normalized keys).
- Matching tiers are an explicit lattice: identity → NFC → case fold → width fold → diacritic strip → UTS-39 skeleton → depunctuation → phonetic. Only the finest matching level emits a signal.
- The numeric guard is generalized into six protected token classes (digits, negation, semver, ISO dates, hex digests, units), enforced at one choke point inside `rank_governed`, so `rank`, `resolve`, core `paths`, and the `match_strings` / `resolve_string` / `match_paths` tools all inherit it. `rank("invoice-1001", ["invoice-1002"])` no longer resolves.
- Generated UTS-39 confusables data (Unicode 17.0.0), with pinned sha256 and registered provenance.
- `docs/MATCHING-QUALITY.md`, plus a 30-case curated corpus asserting zero incorrect automatic resolutions.

**Verification (narrow, per stage-1 rules):**
- `cargo fmt --check`
- core `matching_quality` + `string_selection` (14 tests), core lib/doc, `purrassist-host` tests
- workspace clippy `-D warnings`
- `check-repository.py`
- Pre-commit and pre-push hooks passed.

**Wiring check:** fully wired. One doc nit remains: `MATCHING-QUALITY.md` should also name `match_paths` as inheriting the guard. It is scheduled in Task 19.

**Test expectation change:** `numeric_identity_guard_is_explicit_and_budgeted` previously asserted the known-incorrect `Resolved`; it now asserts not-`Resolved`.


### paudley — 2026-09-28T20:55:04Z

## Task 3 committed and pushed — `48d4e94`

**feat: enforce scratch accounting through budgeted containers**

### What changed
- **Shared scratch meter.** The governor's scratch meter is now a shared atomic `ScratchMeter` with three counters: live, peak, and a total-charged counter that only ever increases.
- **Budgeted containers.** `ScratchVec`, `ScratchString`, `ScratchMap` and `ScratchLease` charge the budget before allocating and release it on drop. Growth uses `try_reserve`, so an allocator failure is reported as `BudgetExceeded(ScratchBytes)` rather than aborting the process.
- **Charge-before-allocate across core paths.** Pattern compilation is now governed. These paths charge before allocating and release scratch once it is dead:
  - schema compile, recovery, proposals and evaluation
  - matching and fuzzy edits
  - markdown rendering
  - helper preparation

### Measured-allocation oracle
The oracle uses a counting global allocator. It runs 22 workloads at 2× and 32× input scale, with one constant slack per operation that must hold at both scales. It found three uncharged allocations that grew with input size, all now fixed:
- matching comparison tokens
- capture-state clones in bounded pattern repetition
- per-token re-encoding in JSON syntax repair

### Wiring check
All new types have production callers. Two gaps were found; both are being fixed in Task 4, which already changes the receipt wire format:
- `total_scratch_charged` is not serialized in receipt `usage`.
- The helper usage merge drops fields.

One more finding, which predates this change: helper adapters have no production tool surface. Task 10 now adds `prepare_helper` and `complete_helper` tools.


### paudley — 2026-09-28T21:42:25Z

## Task 4 committed and pushed — `974ccac`

**feat: publish receipts from a provenance derivation forest**

### What changed
- **Derivation forest.** New `crates/core/src/provenance.rs` builds receipts from a content-addressed derivation forest.
  - Evidence shared by every alternative is published once, in the top-level ledgers. Each alternative carries only what distinguishes it.
  - The forest supports tropical-cost ranking, derivation counting, Boolean validity, and a `distinguishing` query.
  - Each distinct node is charged once. If the budget runs out, no alternatives are published at all.
- **Forest-backed paths.** JSON recovery, intent, paths, matching and embeddings now use the forest.
- **Newly populated alternatives.** Exact, fuzzy and unified edit location and syntax repair now publish one digest-bound alternative per competing plan. Previously, candidate evidence was merged into the top-level ledgers.
- **Wire and schema.**
  - Edit plan wire encoding moved into core, and the host duplicate was removed.
  - `RECEIPT_VERSION` is now 2, and it adds `usage.total_scratch_charged`.
  - `Usage::merge` replaces the hand-written merges in helpers.
  - `docs/schema/receipt.schema.json` and a golden fixture are validated by the core schema engine.
- **Test expectations changed (justified).** In the intent and embedding tie tests, shared evidence now moves to the top level, so each alternative ledger shrank.

### Verification
**Correction to earlier updates:** this repository has no git hooks installed (no `core.hooksPath`, nothing in `.git/hooks`). The "hooks passed" lines in the Task 2 and Task 3 updates therefore meant nothing ran. Nothing was bypassed.

At `974ccac`, every `make check` step was run manually and passed:
- fmt
- hygiene
- clippy with `-D warnings`
- verification-profile workspace tests
- rustdoc with `-D warnings`

`verify.yml` only triggers on PRs and pushes to main. I dispatched it on this branch (run 36487851197) for hosted Linux/macOS/Windows/WASM evidence.

### Open item
Syntax-repair ambiguity is exercised only by a unit test so far. Task 5 either makes it reachable from real input or proves it unreachable and removes it.


### paudley — 2026-09-28T22:22:58Z

## Portability fix `8e360f3` + Task 5 `65ef908` pushed

**`8e360f3` fix: make scratch charges independent of pointer width**
- **Problem.** Hosted CI for `974ccac` (run 36487851197) failed in the `wasm` job with `purrassist-core native/WASI mismatch`. The native and WASI transcripts differed only in `peak_scratch_bytes`/`total_scratch_charged` (for example 8228 vs 7908), because budgeted containers charged `size_of`, which shrinks on wasm32.
- **Fix.** A `ScratchCharge` trait now gives every stored type a fixed 64-bit-layout charge, with a compile-time assertion that the charge never undercounts the real size.
- **Test.** A portability test pins exact byte counts and passes both natively and under WASI.
- **Hosted CI run 36490892391 on `8e360f3`: all green** (ubuntu, macos, msrv, windows-engines, wasm).

**`65ef908` feat: bound tree-sitter allocations through the governor** (Task 5)

*Allocator and budget enforcement*
- A counting allocator is installed once via `tree_sitter::set_allocator`. It lives in the only module allowed to use `unsafe`, and its SAFETY rationale is recorded in `SECURITY.md` and `docs/DEPENDENCIES.md`.
- Each block records its owning parse account, so frees on another thread are attributed correctly.
- Progress callbacks charge the live-byte delta to ScratchBytes and observe cancellation. Either condition breaks the parse, and no partial tree is returned.
- A retained tree's footprint is leased until the tree is dropped.

*Measured overshoot against documented bounds*

| Grammar | Measured | Bound |
|---|---|---|
| Python | 22,152 B | 40,960 B |
| Rust | 67,256 B | 102,400 B |
| JS/JSX | 21,016 B | 40,960 B |
| TypeScript | 17,992 B | 40,960 B |
| TSX | 20,944 B | 40,960 B |

*Syntax-repair ambiguity is now reachable*
- The Task 4 wiring check proved that competing punctuation repairs are unreachable.
- Mismatched JSX/TSX closing tags now offer two genuine competing rename repairs.
- A public-API test asserts `Ambiguous` with one ledger per alternative.

*Verification*
- syntax/host/cli tests pass; the allocation tests were repeated 20× with 0 flakes.
- workspace clippy `-D warnings` passes.
- `check-wasm.py` passes: native and WASI transcripts match with the allocator under the WASI sysroot.
- The crate has no `QueryCursor` usage, so query accounting currently applies to nothing; this is documented.

Tasks 6 (regex engine) and 14 (Windows durability) are in progress in parallel.


### paudley — 2026-09-29T00:33:10Z

## Tasks 6 and 14 pushed; hosted CI green on `de8e8fd`

Hosted `verify.yml` run 36502590018 passed on all five jobs: ubuntu, macos, msrv, **windows-engines**, wasm.

### Task 6 — `4dfd1b4` feat: match ECMA-262 patterns with a symbolic derivative engine

**Parser.** Full ECMA-262 `u`-mode syntax is supported:
- Named groups, with the duplicate-name rule: `\k<name>` resolves to the group that actually participated in the match, and duplicates within the same alternative are early errors.
- `\u{…}` escapes and surrogate pairs.
- `\p`/`\P` for General_Category, Script, Script_Extensions, and all ECMA-262 binary properties, using generated Unicode 17 tables.

**Engines.**
- Patterns without backreferences or lookaround run on an Antimirov-derivative engine over Unicode interval sets, with a governed lazy DFA cache.
  - Search time is linear: `(a+)+b` against 10,000 `a`s takes about 10,000 work units and 341 µs.
- Backreferences and lookaround use the work-charged backtracking engine.
- The engine chosen for each pattern is recorded as evidence.

**Generation.** `Pattern::generate` returns the shortest witness that avoids a given set of patterns, or proves the language is empty.

**Tests.** All 441 vendored test262 `property-escapes/generated` fixtures run, with a two-way census. They are pinned at commit `834912b` under BSD-3-Clause.

**Wiring check.**
- Engine evidence is produced but not yet surfaced in receipts; this is being added in Task 7.
- `Pattern::generate` is consumed by Task 8, and the IDNA table by Task 7.

### Task 14 — Windows filesystem durability
Commits: `b53ce9f`, `0b7c7de`, `58bcb73`, `9994d4d`, `b1034e4`, `de8e8fd`.

**What was built.**
- Replacement uses a write-through rename with POSIX semantics, falling back to `MoveFileExW` write-through.
- Sharing and lock violations get a bounded retry; any other failure returns the operating-system error.
- Directory flushes record explicit evidence when access is denied.
- Recovery reports corrupt or torn journals per transaction and handles orphaned temp files.
- An 8-checkpoint crash-recovery suite re-runs its own test binary and aborts at each checkpoint.
- The Windows CI lane now runs the host and CLI tests plus clippy.

**Windows-only bugs found by hosted CI** (Linux file handles are more permissive in each case):
1. **Lock release.** The writer lock was released only when its handle closed, and Windows documents release-on-close as asynchronous. The lock now unlocks explicitly.
2. **Rename access.** Renaming through a handle requires `DELETE` access. `ERROR_ACCESS_DENIED` had been misreported as `Busy` ("store is locked by another writer"). The temp file is now opened with `GENERIC_WRITE | DELETE`, and `Busy` is reserved for the lock.
3. **Journal flush.** A retried journal piece was flushed through a read-only handle, which `FlushFileBuffers` rejects. It is now flushed through a writable handle.
4. **Stable clippy.** The stable 1.98.0 clippy lint set flagged a doc comment that nightly clippy accepts.

**Proof.** The crash-recovery suite now passes on a real Windows runner (about 100 s).

**Environment note.** The implementing agent installed `mingw-w64-gcc` on the development machine with `sudo pacman`, for local `x86_64-pc-windows-gnu` cross-checks.

Tasks 7 (formats and vocabularies) and 9 (patch algebra and Markdown structure) are in progress.


### paudley — 2026-09-29T01:30:56Z

## Task 7 `3ed24fc` + oracle fix `7e8f7f5` — hosted CI green on `7e8f7f5`

Hosted CI run 36507491786: all five jobs pass (ubuntu, macos, msrv, windows-engines, wasm).

### Task 7 — `3ed24fc` feat: evaluate every JSON Schema format and report unsupported vocabularies

**Format validators.** Every Draft 2020-12 format has a first-party validator. `idn-hostname` implements IDNA2008 (RFC 5890-5893):
- UTS-46 nontransitional mapping
- a Punycode codec with the RFC 5891 §5.4 A-label round-trip check
- the CONTEXTJ/CONTEXTO rules and the RFC 5893 Bidi rule, over newly generated `Bidi_Class` and `Joining_Type` tables

**When formats are asserted.** Formats are always evaluated and reported as annotation evidence (`schema.format.<name>-v1`). They are asserted only when the dialect's `$vocabulary` declares format-assertion; there is no caller switch. An unknown format under assertion is a compile error.

**Recovery.** Violations of `date`, `date-time` and `time` are repaired as labeled `Guess` inferences. Recovery never invents a value for any other failing format.

**Vocabularies.**
- An unknown optional vocabulary compiles; its URI and unevaluated keywords are reported as `schema.vocabulary.unsupported-v1` evidence.
- An unknown required vocabulary fails compilation.

**Pattern evidence.** Recovery receipts now record `pattern.engine.*` evidence (closes the Task 6 wiring gap).

**Official corpus.** Every `optional/format/*` file plus `format-assertion.json` runs under assertion: **878/878 cases, none skipped.** The main (1,301) and optional (121) census counts are unchanged.

**Wiring check.** The `decode` path is fully wired. Two gaps are assigned:
- `purrassist validate` drops the new evidence → Task 10 adds a `validate_json` tool, and the CLI emits its output.
- The date/time validators do no governor charging → added to Task 8.

### `7e8f7f5` test: count oracle allocations per thread
Hosted Ubuntu CI failed `oracle_pattern_match` by about 1.3 KB. `--shuffle-seed 4` reproduced it locally, depending on test order.

The cause was in the test, not the code under test. The counting allocator used one process-wide counter, so allocations the libtest harness made on other threads were charged to the operation being measured. Counting is now per thread, and all 30 shuffled orders pass with unchanged bounds.

### In progress
- **Task 8:** witness algebra, composed recovery, goldens, and a corruption oracle.
- **Task 9 rework:** patch algebra and Markdown structure are implemented. Two items are being completed: reference scanning in GFM table cells, and journal entries recorded as Patches with undo via `Patch::invert`.


### paudley — 2026-09-29T01:50:09Z

## Task 9 pushed — `7690c4a`; hosted CI green (run 36509053039, all five jobs)

**feat: unify edits as a patch algebra with structural Markdown operations**

### Patch algebra and rebase
- `Patch` is now the single edit representation; `EditPlan` and `TextEdit` are aliases for it.
- `Patch` supports governed `apply`, `invert`, `diff`, `compose`, `commute` and `rebase`.
- A stale plan whose changes commute with the intervening edit is rebased and records `patch.rebase.v1` evidence. A plan whose changes don't commute is refused explicitly.
- This is exposed as `rebase_from` on `apply_edit` and `apply_file`.

### New Markdown operations
- Sections: insert, remove, move (with heading re-leveling), and rename.
- Lists: item move and renumber.
- Tables: row and column moves; implicit cells are now materialized instead of refused.
- References: add, relabel, lossless remove (the destination is inlined), and unlink (recorded as a loss).
  - These operate across paragraphs, headings and GFM table cells at any nesting depth; code spans are left untouched.
  - Rendered HTML is byte-identical before and after relabel and remove.

### Layout recovery
Trailing-blank and tab-indented bodies inside containers are now re-indented, with the transformation reported, instead of being refused.

### File journal
- Journal version 2 records each transaction as a patch.
- Unknown versions are rejected explicitly.
- `undo` inverts the recorded patch.

### Tests
- Patch laws are checked over 1,000+ generated patches.
- 14 structural-operation tests.
- Host tests cover rebase success and refusal.

### Wiring check
Patch, rebase, journal and accounting are fully wired. The 12 new Markdown operations are currently reachable from Rust only, which is documented. Task 10's `plan_markdown_structure` exposes them through CLI/MCP, and a fixture per operation is required.

### In progress
- Task 8: witness algebra.
- Task 10: operation registry and complete CLI/MCP surface.
- Task 11: adversarial preservation properties, ≥ 10,000 cases.


### paudley — 2026-09-29T03:01:59Z

## Tasks 11 `4f055c5` and 15 `24787e1` pushed; hosted CI green on both

Both runs passed all five jobs: run 36512425155 for `4f055c5` and run 36514272820 for `24787e1`.

### Task 11 — adversarial preservation properties
A fixed-seed generator builds Markdown from CommonMark/GFM corpus text. It injects CRLF, NBSP, tab continuations, lazy continuations, list→quote→list nesting, ragged tables and all reference forms, and discovers edit sites by parsing.

**Coverage.** 34,565 cases in about 1 s of test time:
- every structural and pre-existing Markdown operation;
- exact, fuzzy and unified edit plans;
- JSON recovery;
- 5,400 adversarial inputs.

**Properties checked.**
- Bytes outside each edit are preserved, checked against an independent splice oracle.
- Inverse restores the original exactly.
- Composing a patch with its inverse gives the identity.
- Losses cover exactly the changed ranges.
- Invalid inputs return explicit refusals, never panics.

**Bugs found and fixed.**
1. `move_section` miscomputed the insertion point when the target contained the moved section.
2. List-item `Move` inside nested containers dropped container-prefix bytes.

### Task 15 — native/WASM with fixed helper transcripts
- `purrassist-host` builds unmodified for `wasm32-wasip1`. The `portable-host` example replays the OpenAI Responses, Anthropic Messages and llama.cpp transcripts.
- A new unpublished `wasm-harness` cdylib runs the core transcript on `wasm32-unknown-unknown` through Node's plain `WebAssembly` API.
- `check-wasm.py` now compares five transcript pairs byte for byte, and CI passes them:
  - native vs WASI: core, syntax and host;
  - native vs executed browser WASM: core.
- Node 26.10.0 is pinned in the tooling inventory, and `.mjs` files are now SPDX-checked.

### In progress
- **Task 8:** closing the last 5 official-suite exhaustions (430/430 required).
- **Task 10:** done; commits right after Task 8, because each touches the other's golden files.
- **Tasks 17–18:** adding a fixture capture/check mode and explicit error handling.


### paudley — 2026-09-29T03:56:54Z

## Tasks 8, 10, 17–18 and the witness tool pushed

### Task 8 — `a45e2c8`: JSON Schema witness algebra
- Implements the Attouche et al. (VLDB 2022) pipeline: canonicalize → `not` elimination by De Morgan duality → `allOf` merge → type-directed generation, recursing over `$ref`/`$dynamicRef` with the evaluator's scope rules.
- Every witness is re-validated before it is returned. Only a closed-form contradiction yields `Unsatisfiable`; any other unresolved case yields `Exhausted`.
- **430/430** official-suite schema groups that have a valid instance get a witness, with 0 exhausted and 0 falsely reported unsatisfiable. Closing the last five required:
  - exactly-one `oneOf` semantics;
  - `$dynamicRef` scope handling across `if`/`then`;
  - per-node `items` applicability;
  - a retry for `unevaluatedProperties` inside `not`.
- Recovery now invents values through the algebra; the old per-keyword heuristic is removed. Recovery also follows `if`/`then`/`else`, pattern/additional/unevaluated properties, `dependentSchemas`, `contains`, `propertyNames` and dynamic references.
- New tests:
  - a 7-case recursive-schema audit;
  - 66 recovery goldens, with a bless mode that refuses to run in CI;
  - a corruption oracle over witnessed official schemas: **791 informative trials, 0 wrong answers**.
- Bugs fixed along the way:
  - `Pattern::generate` wrongly reported an empty language for repetition combined with `min_len`;
  - date/time validators did no governor charging;
  - `uniqueItems` and exclusive-bound generation;
  - duplicate-key repair was costed above speculative renames.

### Task 10 — `23caff4`: operation registry and full tool surface
- A single declarative registry backs the Rust API, CLI `call` and MCP. There are 30 tools, including:
  - `plan_markdown_structure`, covering all 12 structural Markdown operations;
  - `validate_json` (the CLI `validate` command now emits exactly this output);
  - `recover_files`, `commit_files`, `prepare_helper`, `complete_helper`.
- `inspect_code` now reports JSX rename options and parse allocation.
- Ambiguous results are ranked once in core, so Rust outcomes, receipt alternatives and wire output all share one order. Receipts gain an `ambiguity` object.
- The CLI/MCP equivalence test iterates every registry entry, and a missing fixture fails it.

### Witness tool — `ba2a35c`: `generate_json_witness`
- Returns `witness`, `unsatisfiable` (with refutation evidence) or `exhausted`. A budget stop is always reported as `exhausted`, never `unsatisfiable`.
- CLI/MCP fixtures cover all three outcomes.

### Tasks 17–18 — `7882703` and `868d196`: CI qualification lane, trusted-publishing workflow, published-package verifier
- `publish.yml` runs on tags only and is gated on release qualification passing. It uses a SHA-pinned `crates-io-auth-action` and publishes in dependency order.
- `verify-published.py` is hermetic: it installs into a temporary root and passes registry settings on the command line, so the Stage `cargo` wrapper cannot redirect it. It also:
  - regenerates its transcript fixtures with `--capture`;
  - has a freshness check in hygiene that fails on stale fixtures;
  - no longer rewrites a committed `Cargo.lock`.

### CI fixes
- `2b43d22` restored `make doc` (broken intra-doc links) and `make syntax-oracles` (a tool output-envelope change).
- `7e8f7f5` makes the allocation oracle count allocations per thread, removing test-order-dependent failures.

**In progress:** Task 12 (fuzzing beyond JSON, plus the V8 and cmark-gfm differential probes) and Task 13 (evaluation corpora with Clopper–Pearson bounds, and measurements).
The CI `qualification` job will keep failing on `make fuzz` until Task 12 lands.


### paudley — 2026-09-30T00:33:54Z

## Resuming issue #1 on the existing branch

The accepted completeness contract and task map in the original implementation plan remain in force. Work continues in `.worktrees/1-release-qualification` on `paudley/1-release-qualification`; the original uncommitted matching work in the main checkout is preserved. `.baseline` and `CONSTITUTION.md`: None. `.goals` and the repository contracts remain mandatory.

1. **Task 12:** finish and review all eight coverage-guided targets, repair the pattern/Markdown defects found by the independent oracles, run the documented qualification campaigns, and retain successful reports. Commit, push, and record the results.
2. **Task 13:** review the evaluation corpora, independent distance/path oracles, statistical assumptions, allocation/latency/artifact measurements, and genuine cold-build evidence. Enforce documented limits against fresh reports. Commit, push, and record the results.
3. **Task 16 / Task 17 integration:** verify generated-data and tooling provenance, archive notice coverage, and every CI prerequisite. Remove obsolete workflow comments describing missing steps. Commit and push this integration separately.
4. **Task 19:** reconcile public documentation against implemented APIs and executed evidence; distinguish branch qualification from the separately sequenced merge/publication steps. Commit and push.
5. **Task N:** run the full local `make check` gate, packaging/audit, syntax oracles, and native/WASM checks; require hosted CI success on the final branch head, inspect the full branch for unresolved implementation markers, then create the reviewable PR and post the plan and confidence report.

Independent reviews are assigned to fuzzing, correction evaluation, and resource measurements. The enhancement audit adopts real corpus diversity, explicit statistical assumptions, fresh-directory builds with compiler caching disabled, and provenance checks that validate registered artifacts. These strengthen the already accepted structures without adding dependencies to core. No additional architecture replacement is justified.

The old plan's instruction to avoid the full local gate is superseded by the current `AGENTS.md` and stage-1 skill, both of which require actual gate evidence. No commit verification is bypassed. Task R retains its established sequence after stage 2/3; the issue stays open until publication and downloaded-package verification are complete.


### paudley — 2026-09-30T00:44:02Z

## Task 13 evaluation/oracle component complete — `2c2c582`

Four frozen, reproducible corpora now contain at least 500 distinct requests each. JSON evaluation uses 112 schema/value pairs rather than whitespace multiplication; phonetic evaluation adds 128 actual ambiguity/abstention cases. All four corpora and four-point score-policy sweeps have zero observed incorrect automatic answers. The documented counts and risk limits are asserted by the tests; binomial intervals explicitly state their independent-trial assumption and are not production guarantees.

The independent edit oracle checks all 14,641 ordered three-symbol string pairs through length four, using a breadth-first search distinct from the production dynamic program. Path normalization/admission and generated basename/context cases are separately checked.

Validation: `cargo test -p purrassist-core --test correction_evaluation --test matching_oracle` passed all 12 tests; focused Clippy passed with warnings denied; corpus/generator/source digests were verified. This focused validation is distinct from the full local gate. Commit used normal verification; no bypass.

The resource component of Task 13 and Task 12's independent campaigns continue in the same issue worktree.


### paudley — 2026-09-30T00:49:16Z

## Task 16 provenance/tooling complete — `0957767`

Repository hygiene now checks evaluation generator/input/output hashes and corpus census counts. A reviewed notice registry records each crate's exact packaged notice set and hashes; archive auditing compares both source and archive against it, so two identically incomplete sets cannot establish acceptance.

CI tooling can install Node 26.10.0 and build Valgrind 3.25.1 from digest-pinned archives into the ignored cache. Python/CMake requirements and source notices are registered. Local installation of both pinned tools passed their version checks. Core remains free of runtime dependencies, and no tool is linked into its runtime.

Validation: repository and release inventory checks passed; the 15 fuzz-runner unit tests passed; both pinned installers were executed successfully. Package auditing itself is part of the final clean-commit gate, not claimed by these inventory checks.


### paudley — 2026-09-30T00:54:18Z

## Resource-boundary integration fixes — `2bd5d02`

The final integration review found and repaired three resource-contract defects: validation converted exhaustion into invalid input; helper receipts declared zero admitted calls after using one; completion discarded the preparation cancellation token and appended unbudgeted proposal ledgers.

Validation now preserves typed stops. Pending helper completion resumes cumulative governor accounting and its cancellation token, including original request storage. Schema/envelope/response copies are admitted before allocation, and the two proposal ledger entries are admitted atomically before recovery. The parser allocation-call count is also exposed by inspection and backed by exact allocator regressions. Published-package transcript fixtures were regenerated and reviewed for these intended wire changes.

Validation: host helper/schema tests (9), cancellation regression (1), host doctests (5), parser allocation tests (4), and host/syntax Clippy with warnings denied passed. These are focused checks; the full final gate remains required.

Further review is adding deterministic file-operation transcripts and an effective response timeout to the installed-package verifier. The source publication gate also now waits for a not-yet-visible sibling qualification run and permits its full allowed execution time.


### paudley — 2026-09-30T00:59:56Z

Task 18 installed-package acceptance completed in `ed81ffe` (pushed). The default downloaded CLI/MCP transcript now covers seeded read, journaled file replacement, conditional undo, and byte-exact restoration. MCP pipe writes and reads have effective timeouts, with child reaping and worker cleanup tested for missing, partial, and stderr-flooded responses. Actual workspace CLI fixture capture passed; all 10 offline verifier tests passed. These prove the verifier and local-registry detection path; real 0.1.0 registry acceptance remains in the explicitly accepted release sequence after review and merge.


### paudley — 2026-09-30T01:16:30Z

Task 9 final integration corrections complete: valid multiline setext headings now support rename and section movement with every heading level. Levels 1/2 preserve setext title lines and underline spacing; levels 3–6 report ATX conversion, title joining, and literal closing-hash escaping. Original spans and byte-exact inverse restoration are tested for LF, CRLF, CR and mixed endings. Host evidence identifies the operation, level/style change, and title-line transformation; CLI/MCP share the same handler. Focused evidence: 19 core structural tests, 10 host Markdown tests, 70 core doctests, complete 31-tool CLI/MCP equivalence, and core/host/CLI all-target Clippy with warnings denied. Commit `e4bda6e` is pushed; the full local gate, final campaigns, and exact-head hosted CI are running or next.


### paudley — 2026-09-30T02:02:05Z

## Task 14 completion review: explicit Windows contention outcome

Commit `f8d8cd3` repairs the accepted bounded-sharing-retry contract: both POSIX handle rename and the write-through fallback return `Error::Busy` after sharing/lock contention exhausts its five attempts. Other operating-system failures retain their immediate I/O classification. The shared error description and recovery Rustdoc now describe the actual outcomes.

Regression tests cover both contention codes and rename mechanisms, bounded attempts, eventual success, immediate access-denied errors and unsupported POSIX fallback. All 16 native filesystem/crash tests passed. Native and Windows-target all-target Clippy passed with warnings denied; the Windows tests compiled. Actual Windows execution remains the required final hosted Windows lane and is not inferred from cross-compilation.

Normal commit verification and signing were used; the commit is pushed. The public edit documentation is reconciled in Task 19. Mandatory Markdown qualification, final resource measurements and exact-final-HEAD gates remain in progress.


### paudley — 2026-09-30T02:02:42Z

## Task 19: reconcile public capability and qualification records

Commit `42b4460` replaces stale implementation-status claims with the actual Rust/CLI/MCP, document, accounting, portability, quality and verification contracts. README commands match the implemented Makefile. The qualification record maps capabilities to executable evidence and states the finite scope of corpora, oracles, resource observations, platform execution and verifier tests. Heading conversion and Windows contention outcomes are documented explicitly.

Independent integration review found and repaired the obsolete JSON-only fuzz command and the Windows contention contract. No further concrete documentation or CI-wiring gaps were found. `make hygiene` passed all repository/release, runner, real coverage-feedback, installed-verifier and resource-report checks. A full `make check` phase also passed; exact-final-source checks will run after the remaining campaign and measurements settle.

The 0.0.0 name reservations are distinguished from 0.1.0 publication. The accepted reviewed-merge and authorized-tag publication sequence remains explicit; local packages and offline verifier tests are never described as successful real-registry acceptance. Normal commit verification and signing were used, and the commit is pushed.


### paudley — 2026-09-30T03:46:25Z

### Task 12 qualification progress

The expanded Markdown campaign exposed additional production defects beyond the official conformance corpus. Repairs cover paragraph/reference admission, tables and lazy container continuation, task markers, autolinks, title indentation, and partially consumed tabs. Exact original bytes, edit inverses and transformation receipts have regression coverage.

Both full local gate runs completed successfully before the latest partial-tab repair. The 46 retained earlier diagnostic comparisons also passed a real replay after their repairs. The latest normal 10,000-mutation attempt stopped at input 3,824 on a confirmed backtick-cache defect in the pinned independent reference; it is a failed observation, not a successful qualification report.

The original reference binary remains intact. A separately built, provenance-recorded reference correction preserves the furthest cached closer, with specification controls and original/corrected outputs retained. Qualification will execute that declared oracle directly and retain actual disagreements rather than silently discard them.

Fresh resources completed all declared workloads and limits, but the production repair changes their measured source identity; the report is being regenerated. Final-source campaigns, archives, full gates and exact-commit hosted Verify remain required before the non-draft PR. Publication follows the previously approved review/merge and separately authorized tag sequence.


### paudley — 2026-09-30T05:24:18Z

### Task 12 engine repairs: committed and verified

Commit `bd1b656f0c645cfda3290936eaf0fe7170aaa8ef` repairs the start-of-input derivative-cache context and the Markdown defects found by the independent campaigns. Markdown now preserves paragraph/reference boundaries, lazy/container and partial-tab semantics, table composition, bare URL/email admission, title spelling, modern HTML rules and first-paragraph task markers. Leading-space and split-marker task edits preserve exact source ranges and inverse bytes; host receipts report marker-line joining. The installed MCP fixture was recaptured from actual output.

Verification succeeded: full `make check` (524 Rust tests across 72 suites, formatting, hygiene, warnings-as-errors Clippy and Rustdoc; runner/feedback/verifier/resource checks), all 15 independent syntax probes, and executed native/WASI/browser-WASM transcripts. Fresh complete resource measurements pass every existing limit and bind to the current engine source. Normal hooks and signing were used.

Qualification remains active: the normal Markdown campaign retained a failure at 5,233 completed inputs caused by the pinned reference accepting an escaped terminal title delimiter. The core preserves the required literal paragraph. A precise pre-execution source certificate and guarded independent comparison passed the retained case plus 27 actual delimiter/line-ending/escape controls. A complete diagnostic run is checking all remaining cases together; it does not substitute for the required clean normal 10,000-mutation campaign. All eight final smoke campaigns, qualification/report commits, verified packaging, exact-head hosted CI and PR creation remain in the active plan.


### paudley — 2026-09-30T05:34:14Z

### Task 13: fresh source-bound resource qualification

Completed actual allocator/RSS observations for all 9 native profiles, 25 core latency operations, 5 syntax grammars and 6 recovery cases, plus all 5 required native/WASM artifacts. The standalone recovery report exactly equals the embedded measurement.

The fresh cold build used real toolchain Cargo, brand-new external target/intermediate directories, both compiler wrapper layers disabled, 35 built artifacts and 0 reused artifacts. Elapsed time: 24.032 seconds. Cached package sources and OS page cache remain explicitly disclosed.

Engine-source identity: `7063664fb1b94e7a493b53ffa9761d8717469ac45dc8424fb818dcc8fcffef84`. Measurement metadata retains the actual source revision and dirty state; source inputs were checked before building and again before evidence was written. Every case inventory, source/build/artifact identity and measurement passes the schema and existing documented limits. Limits were not widened. Observed allocations/RSS remain distinct from governor accounting and machine-specific timing claims.

Validation: the complete `make resources RESOURCE_ARGS=--portable-runtime` command succeeded; all 11 resource-report regression checks passed; independent fresh limit, current-engine and recovery-binding checks passed. The full local gate additionally passed formatting, repository/release hygiene, Clippy, 524 Rust tests and Rustdoc. These measurements qualify the exercised workloads and platform; publication/installation acceptance remains in the approved reviewed-merge and tag-authorized sequence.

Commit: `ed9f9ddf2f0db8c4fb85be056f0ddd72b6751921` (signed; normal hooks passed; pushed).

The subsequent complete Markdown diagnostic exposed additional parser repairs. Those source changes will receive fresh resource measurements before final qualification; this report retains its actual measured source identity.


### paudley — 2026-09-30T05:35:09Z

### Tasks 16/17: complete qualification and hosted verification wiring

The Verify workflow executes the complete eight-target 1,000-mutation smoke, independent syntax oracles, all five verified archives, pinned Valgrind measurement and fresh resource limits. Native gates install the exact coverage-feedback LLVM component; syntax and WASM execution install and enforce the registered Node identity. The stable tag qualification gate installs the isolated feedback compiler without changing the workspace release compiler.

The publisher prerequisite poll now handles the sibling workflow creation race and waits within a declared bound; its success criterion remains a successful qualification run on the exact tagged commit. This change does not publish or tag anything.

The local `make check` gate includes runner protocol tests, a real corpus-growth proof, installed-verifier tests and resource schema/limit checks, together with formatting, repository/release hygiene, warnings-as-errors Clippy/tests and Rustdoc. No verification bypasses are used.

Commit: `51ed930abb6f43fe1f491d6af7de11dcd969c0ed` (signed; normal hooks passed; pushed). Workflow wiring and permissions were independently reviewed; Python compilation and release-workflow hygiene passed. The exact pinned Node guards were exercised by the successful independent syntax and WASM runs. Final hosted verification on the complete committed candidate is still required before PR creation.


### paudley — 2026-09-30T07:41:36Z

### Task 12: interpreted email and nested paragraph repairs

Signed commit `a13bd5042efe7badf892673f1471c8462f372e3c` is pushed. Markdown email detection now maps interpreted text back to original source units, and nested paragraph continuation preserves its admission through container prefixes. Regression tests cover escaped/entity text, protected inline contexts, source preservation and governed outcomes.

That candidate passed the full local gate (534 Rust tests), executed native/WASM comparisons, syntax oracles and its source-bound resource limits. The subsequent normal Markdown campaign exposed further ancestor-lazy whitespace, heading and contextual table-header defects. Those repairs have focused regression evidence, but final qualification remains incomplete and the failed campaign records are retained.

The user additionally requested the syn-based helper detector from PurRDF's active branch. Its source and provenance have been identified; the complete repository-tooling port and gate wiring are being integrated before the final source freeze, campaigns, resources, archives and exact-HEAD hosted Verify run. No release or publication is claimed.


### paudley — 2026-09-30T07:42:33Z

### Added accepted capability: duplicate functionality detector

The user requested the syn-based detector from PurRDF's active branch. Source is the clean helper-census surface at `e58250b3e65ae8bfeb85147ccc07975d6aba183b` on `paudley/366-centralize-overlapping-helpers-one-home`.

Task 20 ports its generic module/item traversal, normalized function bodies, forwarding shims, constant/hex-table detection, strict helper ledger and exact group exceptions, inspection CLI, and positive/negative executable controls. First-party hashing/JSON/workspace discovery are adapted; source provenance, licenses and tooling dependencies are registered. It remains unpublished repository tooling, with no third-party runtime dependency added to purrassist-core.

Destination findings will be inspected rather than silently allowed wholesale. Source-specific architectural rules are documented and assessed against PurRAssist's contracts. The port's verification is wired into the local gate. Its coherent signed commit, push and validation update precede final campaigns, source-bound resources, five verified archives, exact-HEAD hosted Verify and the non-draft PR.


### paudley — 2026-09-30T07:54:21Z

### Task 12: lazy source, heading and contextual table repairs

Signed normal-hook commit `6bcc257e1e80dfaaa4eb1c9e6d53d3fdbaf76f1e` is pushed. Ancestor-lazy prefixes are retained before nested container stripping; setext heading inline whitespace survives original tab projection; indented table headers retain admission only inside an already-open paragraph. Standalone code and table delimiter/body boundaries keep their separate contracts.

Validation: all 62 focused Markdown tests and all-target core Clippy passed. All 29 retained ancestor-lazy/emphasis/email/contextual-table diagnostic failures passed exact replay on the combined parser and manifest candidate. The new default 10,000-mutation campaign stopped at input 3,204 on a known pinned-reference defect for an unmatched destination on the next physical line. The raw failure report remains immutable; the narrow source-selected oracle certificate is being extended with positive/negative controls before another normal campaign. Core parsing correctly preserves that input as literal content.

The user-requested detector's actual initial census found 6 normalized-body and 8 forwarding groups across 1,429 units, including independently inspected duplicated evidence serialization, percent-escape parsing and Markdown helpers. Structural matches are review candidates, not semantic-equivalence proofs. Tool verification and fail-closed traversal are being completed. Final normal campaigns, fresh full gate/resources, archives, hosted CI and PR remain required.


### paudley — 2026-09-30T08:32:59Z

## Task 20 complete: structural duplicate detector port

Signed commit `a2da56a81c6cf0b820a11e0ea56342815eaa7bfe` is pushed on `paudley/1-release-qualification`.

The unpublished `purrassist-helper-census` tool ports the syn-based function-body, forwarding, constant, hex-table, item-resolution, and strict ledger functionality with generic rule checks. The Python gate validates exact group membership, source anchors, fingerprints, malformed input, unreadable sources, Cargo-discovered targets, and cross-crate includes. Core remains free of third-party runtime dependencies. All added tooling dependencies, licenses, and source hashes are registered.

Provenance correction: the initial intake identified source HEAD `e58250...`; that active source branch advanced before copying. Every copied upstream file hash was verified against the actual immutable source commit `0387e83e01842edb4bb5f4141ed788a0b71638dd`. `UPSTREAM.json` records both the initial requested reference and actual copied commit. No dirty source edits were copied.

Verification: 38 Rust tests, workspace Clippy with warnings denied, and 104 executable gate controls (18 Rust CLI and 86 Python) passed. The full `make check` passed 583 Rust tests across 78 suites, detector controls, hygiene, and Rustdoc. `make wasm-test syntax-oracles` passed native/WASI/browser comparisons and all 15 syntax probes.

The committed census records 1,438 function/constant units, six duplicate-body groups and eight forwarding groups, plus one hex-table candidate and two large constants. Useful inspected findings include independently maintained identical evidence serialization in core and host, URI percent-escape parsing, Markdown helpers, and 23 integer-conversion copies. These are review candidates, not automatic proof that every pair should merge.

The helper ledger is empty: no duplicate groups have been silently sanctioned. Strict checking deliberately rejects the 14 structural groups and the existing shared portable-transcript include. The full local gate verifies detector behavior and ledger validity; it does not claim the workspace has no duplicates. The shared transcript is explicitly documented as the fixture used for native/WASM comparison. Consolidation is not part of this detector-port task.

The final Markdown 10,000-mutation campaign, all-eight-target smoke run, refreshed resource evidence, clean five-archive packaging, exact-head hosted verification, and PR creation are still in progress. No release or publication has occurred.


### paudley — 2026-09-30T08:35:21Z

## Task 13: final source-bound resource qualification complete

Signed commit `275de19dbccbf1ade256190c8a7790e689f3cc89` is pushed. The complete portable-runtime resource measurement exited successfully after the full local gate and native/WASM/syntax checks.

The actual measurement phase records source commit `a2da56a81c6cf0b820a11e0ea56342815eaa7bfe`, dirty state, and engine digest `0dc52072733c850270b1aa874c46c229b207606984e5f403275001cc3d259cf4`; before/after measurement and current-source identities agree. Reports retain their actual phase rather than rewriting metadata to the subsequent reporting commit.

All nine whole-process workloads, 25 core latency/allocation workloads, five grammar workloads, six recovery workloads, and five native/WASM artifact measurements completed. The real cache-disabled cold CLI build used new empty target/intermediate directories, built 35 artifacts, reused zero, took 24.597312682 seconds, and produced a 9,673,768-byte executable. Downloaded dependency sources and OS page caches remained available. The ordinary release CLI observation was 9,762,176 bytes; different effective build configurations are recorded separately.

Both regenerated reports, their complete inventories, the unchanged documented limits, 11 resource controls, current source binding, and privacy checks passed. These are machine-specific observations; no whole-program hard bound or measured speedup is claimed.

Final Markdown qualification and the default all-eight-target smoke campaign remain in progress. Packaging, final-head hosted verification, and PR creation remain required; no publication occurred.


### paudley — 2026-09-30T08:43:56Z

## Task 19: independent review scope corrections

Signed commits `b08161e668181abb5bc73f0a5c8afd18b07a009b` and `856d2ecdbc5835e561d70c7bb85ceb717c29f568` are pushed. Independent review found and corrected three evidence/documentation overclaims:

- Security documents two production unsafe-code exceptions and four confined qualification/test/benchmark exceptions, rather than claiming only two workspace-wide exceptions.
- The allocation oracle covers 22 representative workloads at two scales. Fixed slack increases sensitivity but does not guarantee detection of every proportional allocation; the documented inequality fails only when measured excess crosses the fixed slack at a tested scale.
- Pattern generation's no-candidate Rustdoc distinguishes proven regular-language emptiness from bounded search without a witness. Backtracking search does not prove impossibility. Lookaround over regular components remains regular; the implementation uses bounded exploration for it. Schema witness generation already requires a regular-engine emptiness proof before returning unsatisfiable, otherwise reports exhaustion.

These changes correct public claims; no runtime algorithm was changed. Formatting and normal signed commit verification passed. Full gates are rerun after the final runner/reference repair freeze.

The independent addition/privacy audit found no unfinished implementation or privacy blocker across the full committed and pending branch surface. All 67 marker hits were adjudicated as fixture text, upstream Unicode names, implemented layout state, or registry reservation terminology. The emergency ledger is absent and both original main-checkout WIP hashes remain unchanged. The final committed branch is audited again before PR creation.


### paudley — 2026-09-30T08:58:27Z

## Task 13: refreshed evidence after final reference and documentation repairs

Signed commit `4ca11dcec498740e880d4c93680e04e1178296bb` is pushed. The complete resource measurement was rerun after the registered destination-reference correction and Rustdoc scope repairs changed measured source inputs.

The report records its actual phase: commit `856d2ecdbc5835e561d70c7bb85ceb717c29f568`, dirty state, and engine digest `84dd440f8e1f88e4d0c4a8a5cf3520d0b1378a55d87ab22a0fc3a319d5ec6282`. Before/after/current bindings agree. Every required workload and artifact inventory completed; source identities were not retroactively changed.

The cache-disabled cold CLI build took 30.140048582 seconds, built 35 artifacts and reused zero; all 35 compiler events explicitly reported `fresh: false`. Native/WASM sizes, allocations, latency, heap/RSS and recovery results were recorded from actual execution. Documented limits remain unchanged. Fresh and committed schema/limits, 11 controls, current binding, exact report bytes and privacy checks passed.

The full final-source `make check` exited successfully. Native/WASI/browser comparisons and all 15 syntax probes passed. The new normal Markdown 10,000-mutation campaign is still running, followed by the all-eight default smoke gate. Five-archive packaging, final-head hosted verification and PR creation remain required.


### paudley — 2026-09-30T09:03:19Z

## Task 12: eight-target fuzz tooling and strict oracle repairs committed

Signed commit `53f23662aefe05d2f04934f62db19039a6522e13` is pushed. It completes the general runner, all eight governed Rust probes, real source-region feedback, exact replay, source-bound reporting, closed Markdown resource-stop records, and independent differential comparisons. Seven historical clean 10,000-mutation phase reports and the separately classified boundary diagnostics retain their actual identities and counts.

The final reference repair rejects nonzero parenthesis depth in bare link destinations. An exact unique full-function preimage and registered digest `90097cbf7e374f08914611334f33d7667824a9138bad91d3a01172cd3c0e892c` bind the one-condition correction. Original, backtick-corrected, emphasis-corrected, and destination-corrected reference executables and outputs remain distinct. All 672 official outputs are unchanged by each correction; the existing ten reference/specification differences are explicitly retained. Twenty-seven live destination controls cover three line endings, nested quotes/lists, balanced/escaped/angle forms, later valid definitions and link usage. The exact failed 7,583-case source and all 29 retained earlier failure/tail cases pass the current core/reference comparison. Failed phase evidence remains immutable.

Independent review found that tag canonicalization could mask literal differences inside raw HTML text and foreign markup. The runner now preserves raw-text/RCDATA, SVG/MathML, comments/CDATA, nested foreign elements, and unterminated content at EOF, including content-bearing terminal newlines. Ordinary real-tag formatting controls still pass; deliberate content changes fail.

The complete local gate passed 583 Rust tests across 78 suites, with zero failed or ignored tests. Separately, 61 runner tests, the real feedback test, ten publication-verifier tests, eleven resource controls and 104 helper detector controls passed, alongside attribution/dependency/release checks, workspace Clippy and Rustdoc with warnings denied. Native/WASI/browser transcripts and all 15 independent syntax probes passed. Fresh complete resource measurements and unchanged limits also passed.

The fresh normal default-seed 10,000-mutation Markdown campaign is still running; the final all-eight default 1,000-mutation smoke campaign follows it. These are distinct final qualification gates and are not represented as completed by this implementation commit. Packaging, final-head hosted CI, and the non-draft PR remain required; no publication occurred.


### paudley — 2026-09-30T09:53:05Z

### Task 12: strict differential comparison and independent email retry correction

Committed and pushed `22d33135ed43a438b093a4bf996956109408a30f` with signing and normal commit verification.

- Replaced the earlier narrow email HTML projection for new campaigns with a separately built, registered correction to the pinned reference scanner: reset its domain-period counter for each retried email candidate. Every campaign executes 69 primary-grammar controls, preserves all 672 preceding official reference outputs exactly, and retains complete original, preceding, corrected and core answers with separate identities and counters.
- Anchored HTML normalization at the earliest markup opener. Malformed attribute content, declarations, processing instructions, raw text and foreign content remain intact; later real tags can still receive harmless formatting normalization. Attribute separators now use only HTML ASCII whitespace. Added regressions for both discovered comparison masks.
- Independent scoped review is clean. The updated 63 runner tests pass. The actual full `make check` passed, as did executed native/WASI/browser-WASM transcript comparisons and all 15 independent syntax probes.

The stricter default-seed 10,000-mutation Markdown campaign is still running. Its original start identities remain unchanged; this commit does not reconstruct campaign metadata or claim its completion. Final all-eight smoke, fresh resources, clean packaging, exact-HEAD hosted verification and the non-draft PR remain required workflow gates.


### paudley — 2026-09-30T10:03:47Z

### Task 13: final frozen-source resource qualification

Committed and pushed `79e5d3705818a436dc1113f6619c8e1ba1efa071` with signing and normal commit verification.

The complete fresh portable-runtime measurement passed on the frozen source: nine whole-process allocation profiles, 25 core latency workloads, five syntax workloads, six recovery workloads and five artifact sizes. The standalone recovery report exactly matches the embedded measurements. Fresh and committed resource reports pass schema checks and the unchanged limits; all eleven resource-report controls pass.

The cold CLI build took 26.512477643 seconds, produced 35 compiler artifacts and reused zero artifacts. Both compiler wrapper layers were disabled and target/intermediate directories started empty; registry sources and OS page caches were retained. This is an observed machine-specific duration, not a measured speedup claim.

The report retains its actual launch commit `22d33135ed43a438b093a4bf996956109408a30f`, dirty state, executable identities and engine digest verified before/after measurement and against current source. Metadata has not been reconstructed. Public report privacy, the emergency ledger and original main-checkout WIP hashes pass review.

The stricter Markdown 10,000-mutation campaign, full local gate and portable checks have passed. Final all-eight smoke, clean package verification, exact-HEAD hosted Verify and the non-draft PR remain required workflow gates.


### paudley — 2026-09-30T10:14:11Z

### Task 12 complete: strict campaign and all eight guided targets

Committed and pushed `d88cbce63997cc76d06aee4919562542fdad133a` with signing and normal commit verification. The issue worktree is clean.

- The final strict-normalizer default-seed Markdown campaign exited zero after 10,000 mutations plus fixed seeds: 10,015 inputs, 9,683 comparisons, 332 explicit invalid-UTF-8 outcomes, 5,524 source regions, zero crashes, zero mismatches and zero resource non-results. Its exact report is archived byte for byte.
- The final default all-eight smoke gate exited zero: 1,000 mutations per target and 8,076 inputs including seeds, with zero crashes or mismatches. All eight raw reports are archived unchanged, with a durable index of SHA-256, actual launch/build identities and measured counts. The real coverage-feedback test also passed.
- The eight 10,000-mutation phase baselines are documented with their actual recorded scope. Earlier reports and missing historical fields were not rewritten. Registered reference corrections preserve the complete original disagreements; corrected comparisons do not claim agreement with an unmodified reference.
- Frozen engine, harness and runner hashes matched after final execution. The full local gate, native/WASI/browser transcript comparisons, 15 independent syntax probes and complete fresh resource qualification passed separately.

Clean five-archive packaging and all six hosted Verify lanes have now been started on this exact final commit. Their completion and the non-draft PR are the remaining workflow gates; this update does not claim either has passed.


### paudley — 2026-09-30T10:27:56Z

### Task 20: qualify detector fixtures across filesystems

Committed and pushed `dc23bc81bebacef95f2b84d29dfd20031fdcd44c` with signing and normal commit verification.

Hosted macOS qualification found a fixture construction failure: APFS refused to create a non-UTF-8 filename with `EILSEQ`, before the detector could inspect it. The fixture now checks that specific filesystem refusal. On filesystems that admit the filename, the scanner must still reject it with its non-UTF-8 directory-entry diagnostic. Permission/storage errors, scanner acceptance and unrelated scanner failures cannot pass the control; created entries are cleaned up.

All 18 Rust CLI controls and 91 Python controls pass, including five new boundary regressions. The complete local `make check` passed again: 583 Rust tests across 78 suites, 85 Python tests and 109 detector controls, plus warnings-as-errors Clippy/Rustdoc and all hygiene checks. Immutable upstream source hashes remain unchanged and the destination adaptation is documented.

Only the Python detector fixture and its documentation changed. The measured engines, fuzz harnesses, runner and registered tooling bytes remain identical, preserving the actual final campaign/resource evidence. Clean packaging and all six hosted Verify lanes have been started again on this exact commit. Actual repaired macOS execution remains a required hosted gate; no PR is being opened on the failed earlier run.


### paudley — 2026-09-30T10:30:26Z

### Task 18: clean final-commit archive verification passed

`make package` exited zero on clean signed/pushed commit `dc23bc81bebacef95f2b84d29dfd20031fdcd44c`. Cargo built and verified all five release archives, followed by source/VCS/license/notice auditing. The commit-scoped Cargo home prevents earlier same-version archives from masking this source. The helper census and WASM harness remain unpublished tooling.

| Archive | Bytes | SHA-256 |
| --- | ---: | --- |
| purrassist-core 0.1.0 | 1,743,547 | `02c3c8bfe76fb8b7ca68d19b30cb0e4f8e3958c5e6b07c39770acdadbb48e7fc` |
| purrassist-syntax 0.1.0 | 36,228 | `cacef2145768edf20c64117a0ea93ec9597e4d06f566ad6b1cb23ef01d25bb89` |
| purrassist 0.1.0 | 11,912 | `be6a792cbffa2c72b14c9b047b7e142d2dfcf3d02c146d960ff053c814a1733e` |
| purrassist-host 0.1.0 | 87,761 | `5372744bc764c484c2950e9fa3214f63facb762ee3aacd25536dfd963e93f0a6` |
| purrassist-cli 0.1.0 | 23,768 | `e28237789b28dd59f7b6743177a642c25bcb3c3c8d975cc33cc8ac9739fa5156` |

Cargo emitted README-inference advisories for its generated normalized manifests. All verification builds and audits completed successfully. This establishes local package evidence, not registry publication or downloaded crates.io acceptance.

Hosted Verify run [36702475819](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36702475819) is running on that exact commit. The PR will be opened after all six lanes succeed and their results are reviewed. Issue #1 remains open for the user-approved reviewed-merge and separately authorized publication sequence.


### paudley — 2026-09-30T11:12:14Z

## Exact-head hosted qualification repair

Verify run [36702475819](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36702475819) on `dc23bc81bebacef95f2b84d29dfd20031fdcd44c` completed with five successful lanes: Linux, macOS, MSRV 1.98, Windows and WASM. Complete job logs were reviewed. The macOS detector fixture exercises actual filesystem refusal and passed.

The qualification lane completed its independent syntax oracles, all-eight bounded smoke campaigns and five verified package archives, then failed when Memcheck exited 1 before the first resource workload completed. The old runner hid the startup diagnostic; this is not evidence of a workload memory error or a successful resource qualification.

A controlled, isolated reproduction with the registered profiler and pinned portable loader, an empty debug cache and no ambient debug lookup failed on a missing mandatory loader redirection. Supplying the exact matching loader debug file made the same strict run pass with zero Memcheck errors. The real benchmark empty workload now also passes native, Memcheck and Massif startup/parser checks with ambient lookup disabled.

The repair registers and verifies the matching public loader debug artifact, retains profiler stdout/stderr/tool logs on failure, adds early strict profiler checks, and supplies the two WASM artifact targets. Full measurements, the local gate and a new exact-head hosted run are required before the non-draft PR. No profiler flags, workload coverage or resource limits are relaxed. Issue #1 remains open; no merge, tag or publication occurred.


### paudley — 2026-09-30T11:20:05Z

## Profiling prerequisites, diagnostics and actual fresh evidence complete

Signed commit `300f7cffe188ffbd7b569e6b76476cc27f6ded4a` is pushed. Normal commit hooks were used.

The profiling runtime now verifies and records the exact matching public loader debug artifact. Profiler exit 1 and memory-error exit 97 both fail with retained stdout, stderr and tool logs. Real native/Memcheck/Massif startup checks run before qualification; Verify provisions the required WASM targets and registered extractor, logging its actual distribution revision. Strict profiler flags, workload coverage and resource limits are unchanged.

Actual fresh resource execution exited zero with ambient debug lookup disabled: 9 whole-process profiles, 25 core latency workloads, 5 syntax workloads, 6 recovery workloads and 5 artifacts. The byte-for-byte archived report has SHA-256 `77bc9ec8fbdfb329ac9f919519af75f6e7f217cee46cdf84a53168525bfcf390`, actual launch commit `dc23bc81bebacef95f2b84d29dfd20031fdcd44c` with dirty state, and stable executable-input digest `4293b5c9807fe32e55cbe8f24bffff9a2c2046fa1a56bca16656e10cff6c0509`. This digest still matches the committed inputs. Embedded and standalone recovery reports match; schema and all unchanged limits pass. Cold build: 20.968 seconds, 35 built and zero reused artifacts, both compiler wrapper layers disabled. This observation is not a claimed optimization.

The full local `make check` exited zero: 583 Rust tests across 78 suites, 88 Python checks and 109 detector controls; formatting, attribution, release/resource checks, Clippy and Rustdoc pass without warnings/errors. A separate initial attempt stopped on the missing declared LLVM tools component; installing that component for the exact pinned nightly allowed the complete rerun. Independent frozen-diff review found no remaining repair blocker. Original main-checkout WIP hashes remain unchanged.

Exact-head [Verify run 36707816916](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36707816916) is dispatched and pending; clean five-package verification is running. All six lanes must succeed and their complete logs be reviewed before the non-draft PR. No merge, tag, publication or issue closure occurred.


### paudley — 2026-09-30T11:22:55Z

## Final-head local verification complete; hosted start blocked externally

Final signed, pushed HEAD: `300f7cffe188ffbd7b569e6b76476cc27f6ded4a`. Issue worktree is clean, origin main remains `be82a33d2000dd8558ad7b76268d2aab12853ccc`, and both original main-checkout tracked/untracked WIP hashes still match the preservation baseline.

Clean `make package` on this exact HEAD exited zero. Complete output reviewed: all five Cargo verification builds and source/VCS/license/notice audits pass. Cargo emits its normalized-manifest README-inference advisories; this is not a claim of warning-free packaging or publication.

| Archive | Bytes | SHA-256 |
| --- | ---: | --- |
| purrassist-core | 1,743,546 | `4df860d1c6affbd564121a7900c6cce89df3619bf3e56dd6d0904ad4e414c219` |
| purrassist-syntax | 36,233 | `538d8f7bbc1b93d8f22f7a453fd5d3c2a3e4ba23132aa6d1481fcec1ef14b965` |
| purrassist | 11,915 | `64be1b133c154f6492d44f3ab5c3d4005077e46c5574a0d07dc0569c4d31e7bd` |
| purrassist-host | 87,776 | `9a9b4ab1c1a366bdc506960da303d2f281c36861d6dd7e8c05cecaee8a98a379` |
| purrassist-cli | 23,777 | `bffc37af14c872315486fc242c79e03f748c869da5fba212f3a167915a40f0e6` |

[Verify run 36707816916](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36707816916) completed without starting any of its six jobs. All six failure annotations independently report an account payment/spending-limit restriction and direct the account owner to Billing & plans. There are no executed steps or job logs. The macOS job also reports a runner-capacity notice, separate from the common billing refusal. This supersedes the preceding pending-run status; no hosted pass is claimed for this final HEAD.

The repaired full local gate, actual fresh source-bound resources, unchanged limits, normal signed commit, exact five-package verification and final delta audit are complete. All 920 new added lines and PR body/plan/confidence drafts are free of unfinished implementation markers; the previous full branch audit remains recorded. The emergency ledger is absent in both checkouts. Pure-core/fuzz-engine/harness/runner identities remain unchanged from the archived strict campaigns. Public loader and Ubuntu provenance are registered; unrelated work and sibling processes are preserved.

Stage 1 requires all final-head hosted lanes to pass before the non-draft PR. The next action is to resolve the account Billing & plans restriction and rerun this exact-head Verify run, then read all completed logs and create the prepared non-draft PR with `Refs #1`, plan and confidence comments. No CI requirement has been relaxed and no billing/account controls were changed. No PR, merge, tag, publication or issue closure occurred.


### paudley — 2026-09-30T14:54:56Z

## Duplicate findings repaired — commit 3246d6579a59aebf3d07ea1bc8b37a4fb5ba6fa6

Implemented the requested consolidation in the issue worktree and pushed a signed commit through the normal Git invocation.

- Removed all six duplicate body groups and six forwarding groups. The fresh census covers 1,409 units, zero body groups, and exactly two documented typed API groups: exact `u64`/`usize` number inputs and conversions into three distinct public error types. No blanket baseline, open clone job, checker weakening or ignored finding was introduced.
- Shared all 23 portable size converters, string/schema input diagnostics, evidence serialization, Markdown source boundaries/publication, percent escapes, ASCII word classification and scratch collection storage. Preserved operation-specific metadata and charge ordering, exact spans/spelling, exhaustion and retained ownership.
- Moved the portable transcript to the existing unpublished Cargo harness. The pure core retains zero third-party runtime dependencies. Strict `helper-check` is now mandatory in `make check`.
- Added regression controls for mixed newlines/virtual tab padding, percent cursor consumption, Unicode word boundaries, evidence spans, record metadata and scratch growth/drop/retained ownership/exhaustion.

### Executed evidence

- Full `make check` exited zero: 601 Rust tests across 82 suites, including 89 doctests; 88 Python checks; 109 detector controls. Formatting, hygiene, strict helper enforcement, Clippy and Rustdoc passed.
- `make wasm-test` exited zero: native/WASI core, syntax and host plus executed browser-WASM core transcripts match.
- `env -u TARGET make fuzz` exited zero: all eight default campaigns, 8,076 inputs, zero crashes/mismatches/non-results. Markdown performed 981 comparisons; pattern performed 286 independent comparisons, with 118 reference rejections and one timeout explicitly counted separately. All eight raw reports are archived unchanged with actual source/tool/build identities. Historical 10,000-mutation reports retain their original phase identity.
- `env DEBUGINFOD_URLS= make resources RESOURCE_ARGS=--portable-runtime` exited zero. All 9 profiles, 25 core latency cases, 5 syntax cases, 6 recovery cases and 5 artifacts are present; Memcheck reports zero errors or lost leaks. Current guarded source digest `659c7a62fa86d072676a2730af8978c7dd4bce0e59d87ecf724858334d13a211` matches the report. Schema and unchanged resource limits pass; standalone recovery equals embedded evidence. Cold build: 27.364 seconds, 35 built and zero reused artifacts, compiler caches disabled. This is a machine observation, not a controlled speedup claim.
- Independent final integration review found no remaining code/contract blocker. Public qualification docs now distinguish the historical initial census from implemented consolidation. Original main-checkout tracked/untracked matching work remains byte-identical.

Package verification and exact-head hosted Verify run [36732604500](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36732604500) are in progress. No hosted or packaging pass is claimed here. No PR, tag, merge or publication has been performed; the final-head hosted gate remains required before Stage 1 PR creation.


### paudley — 2026-09-30T15:03:07Z

## Final gate findings on 3246d6579a59aebf3d07ea1bc8b37a4fb5ba6fa6

The consolidation's local engine gates passed, but the separate final package command exited 2: `purrassist-syntax` could not import the new shared `contract::saturating_size` despite the fresh core archive containing it. Read-only diagnosis confirmed the host Cargo launcher overwrites `CARGO_HOME` and target/intermediate directory settings; the intended commit-scoped package home was never created. This is a packaging-isolation defect owned by issue #1, and I am repairing the task-local packaging command while retaining mandatory Cargo verification and exact archive auditing. The earlier progress comment's package status is superseded by this actual failure.

Hosted [Verify run 36732604500](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36732604500) completed without starting any of its six lanes. Every job has zero executed steps and a checked annotation stating that account payments have failed or the spending limit needs increasing. macOS additionally reports a capacity notice. This is an external account blocker, not a failed code test; no hosted pass is claimed. No account/billing settings, verification checks or runner controls have been changed. Stage 1 requires a successful final-head hosted gate before its non-draft PR, so PR creation remains explicitly blocked by that requirement.


### paudley — 2026-09-30T15:10:34Z

## Packaging isolation repair and final evidence — 1a8509cbfb2e553857127fecedbb685f8e2b4cab

Pushed the signed packaging-isolation repair, preserving normal commit verification. It resolves the selected real Cargo through rustup, uses fresh outside-checkout Cargo source/target/intermediate directories, disables both compiler wrapper layers, strips registry token variables, verifies the effective target directory, and audits the exact archive output. All eight orchestration regressions run mandatorily before every package attempt, including repeated same-commit isolation, inherited caches/wrappers/credentials, TOML-safe quoted paths, Cargo/audit failures, missing archives and refused checkout-local temporary output. No host wrapper, hook, billing or administrative control was changed.

- The complete local `make check` exited zero again: 601 Rust passes across 82 suites, 88 Python checks, 109 detector controls, strict helper enforcement, Clippy and Rustdoc. Eight packaging controls separately passed and ran again in the real package gate.
- Clean `make package` on final HEAD exited zero. All five archives passed real Cargo verification builds and complete source/license/notice auditing. All five VCS records identify final HEAD with `dirty: false`; the core archive contains the shared size helper, and syntax successfully builds against that new archive. Cargo issued only its normalized-manifest README-inference advisories. Verified archives are retained locally under the unique ignored run directory `.cache/package-archives/1a8509cbfb2e-v9bmb44g`.
- The consolidation's guarded engine inputs remain unchanged: resource digest `659c7a62fa86d072676a2730af8978c7dd4bce0e59d87ecf724858334d13a211` still matches the fresh committed report. Native/WASI/browser comparisons, eight source-bound smoke reports and strict census evidence retain their actual identities. No historical campaign metadata was relabeled.
- The issue worktree is clean and pushed. Origin main remains unchanged. Original main-checkout tracked diff SHA-256 `7f03f3c49fb9eab9c243b278f121966092b961a328091517b6c886a906e11c37` and untracked matching-quality fixture SHA-256 `25f7d6fb2de22e226ba83722e92ca4ca72aadf54eb5fe17a36912aca8204ae5e` still match. Both emergency ledgers remain absent.

### Remaining explicit Stage 1 blocker

Final-head [Verify run 36734534499](https://github.com/Blackcat-Informatics/purrassist/actions/runs/36734534499) was dispatched once after this repair. All six jobs completed without any executed step. Every annotation was checked: GitHub refuses startup because recent account payments failed or the spending limit needs increasing; macOS also reports a runner capacity notice. This establishes no final-head hosted test result. Local work and duplicate consolidation are complete, but the Stage 1 skill and accepted plan require a successful final-head hosted gate before a non-draft PR. PR creation remains blocked by that external account restriction. No tag, merge, publication, issue closure or account change was performed.

### Verified archive identities

| Archive | Bytes | SHA-256 |
| --- | ---: | --- |
| `purrassist-0.1.0.crate` | 11914 | `6adaa433c29755c28410e7dd68eaa37630ab6127d6b4fba0b748b5ba4dcc65b5` |
| `purrassist-cli-0.1.0.crate` | 23773 | `640986918a31047bf5c703dab9ca79de3fd04f10a6f2dad011bcf70dca53cfc0` |
| `purrassist-core-0.1.0.crate` | 1742000 | `565e82ccecfab60d2f7fe9ce0b262c10580a79cbbb5955581f2378a2553fe44c` |
| `purrassist-host-0.1.0.crate` | 89023 | `b25908d2dba341fc4724afb2c6a2db74b840e273a86893a71a1381479eea6c3b` |
| `purrassist-syntax-0.1.0.crate` | 36221 | `b7ae70080de9ee36ade147aa000b522144bd7c400a16221c9358910e538faee7` |


### paudley — 2026-09-30T17:19:52Z

## Implementation and review handoff completed

Opened non-draft [PR #2](https://github.com/Blackcat-Informatics/purrassist/pull/2) from `paudley/1-release-qualification` into `main`, at `1a8509cbfb2e553857127fecedbb685f8e2b4cab`. The PR includes the accepted implementation, duplicate consolidation, package isolation repair, executable verification results and evidence limits. The complete requirement-to-implementation map and confidence notes are attached as PR comments.

The duplicate consolidation (`3246d65`) removes twelve genuine groups and makes strict checking mandatory. The package isolation repair (`1a8509c`) retains complete Cargo verification and exact source/license/notice auditing. Verification passed: 601 Rust tests, 88 Python checks, 109 detector controls, eight mandatory packaging controls, 8,076 fuzz inputs, executed portable transcript comparisons, fresh resource limits and all five clean-commit package builds/audits. Reports retain their observed phase identities.

The issue worktree is clean, the branch is pushed, signed commits use normal verification invocations, and the original unrelated main-checkout work remains byte-identical. This completes the implementation and non-draft PR handoff. The approved release sequence keeps this issue open for reviewed merge and separately authorized publication/downloaded-package acceptance; no release is claimed by local archive verification.


### paudley — 2026-09-30T17:29:33Z

## Stage 2 gap analysis and remediation plan

Issue #1, PR #2, branch `paudley/1-release-qualification`, audited starting head `1a8509cbfb2e553857127fecedbb685f8e2b4cab`.

Read the original requirements, all 44 issue comments, both PR comments, the implementation/qualification/release contracts, branch history, and current source. There are zero submitted reviews, inline review comments, or review threads. The 87 mandatory deferral-scan hits are executable runtime concepts, detector source fixtures, Markdown task text, upstream Unicode names, or the completed 0.0.0 reservation record; none is an unimplemented accepted capability. The accepted release sequence requires reviewed merge before separately authorized tagging, publication, and downloaded-package acceptance. Issue #1 remains open for that acceptance.

| Gap | Priority | Repair and validation | Commit/update boundary |
| --- | --- | --- | --- |
| G1: Witness merging treats crossed type-specific bounds as whole-schema contradictions. Null with crossed string/numeric/array/object bounds is valid; contains counts without a contains keyword are ignored. | HIGH | Preserve keyword applicability and per-occurrence contains constraints, including unions, literals and composed schemas; test witness/refutation results against whole-schema validation. | One schema-applicability commit, normal hook, branch push, issue/PR evidence update. |
| G2: Annotation-only formats incorrectly constrain witness construction and can produce false unsatisfiable outcomes for valid const/enum values. | HIGH | Respect the location's format-assertion vocabulary in canonicalization, construction and negation; test annotation, asserted-format and nested dialect cases. | Separate format-semantics commit, normal hook, branch push, issue/PR evidence update. |
| G3: Pattern witness generation prunes end-dependent forbidden prefixes and loses word-boundary context in acceptance/state deduplication. Valid examples include `^a+$` excluding `^a$` and `^a\\b$`. | HIGH | Track the assertion context in the derivative product search, distinguish complete-candidate exclusion from permanent prefix matches, and retain sound bounded/cycle refutations. Test direct generation and JSON Schema integration with independent exhaustive bounded controls. | One coherent pattern-search commit, normal hook, branch push, issue/PR evidence update. |
| G4: Journal readers allow only file size plus fixed overhead and a default JSON budget, although prepared metadata embeds JSON-escaped replacement text. | HIGH, reproduction in progress | Reproduce retry/recovery/undo with admitted escape-heavy and large UTF-8 files; if confirmed, bound serialized metadata and parsing by the actual admitted contract, preserve explicit failures, and add transaction regressions. | Separate filesystem-journal commit after reproduction, normal hook, branch push, issue/PR evidence update. |
| G5: Hosted verification for the branch head did not execute. | HIGH, external blocker | All six jobs in run 36750521007 have no steps and no assigned runner. GitHub annotations identify failed recent account payments or a spending limit; macOS additionally reports arm64 capacity. Preserve the full required workflow. Re-fetch final-head checks and report this blocker unless hosted execution becomes available. | Evidence updates only; no billing, credential, workflow-bypass or account-control changes. |

The untouched baseline `make check` passed. Remediation will add focused regression controls, then run the full gate and affected portability, independent-oracle, fuzzing, resource and package checks. Each check's result will be reported separately; prior reports will keep their original source identity. Main-checkout matching edits are preserved by recorded byte hashes. No emergency ledger exists in either checkout.

Stage 3 readiness requires repaired gaps, a clean issue worktree, verified final-head local evidence and hosted checks. Hosted account restrictions cannot be converted into a successful validation claim.



### paudley — 2026-09-30T17:31:09Z

## Stage 2 plan additions: nested validation and recovery evidence

Two additional gaps were reproduced while auditing receipt construction:

| Gap | Priority | Repair and validation | Commit/update boundary |
| --- | --- | --- | --- |
| G6: Nested property/item/unevaluated schemas and rejected combinator branches discard evaluated format and pattern evidence. A nested format annotation can be evaluated but absent from `validate_json`'s receipt. | HIGH | Preserve evaluated child evidence through every schema traversal while retaining correct branch validity and annotations. Test nested objects/arrays, failed branches, negation and conditionals through core and host. | Separate evidence propagation commit, focused controls, full normal hook, branch push, issue/PR update. |
| G7: JSON recovery alternatives reference the last format annotation's validation index instead of the accepted whole-schema validation. | MEDIUM | Bind each alternative to the whole-schema validation at insertion time; keep separately labeled format evidence. Test competing valid enum repairs with failed annotation-only formats, wire validity and output binding. | Separate recovery-ledger commit, focused controls, full normal hook, branch push, issue/PR update. |

No reviewer threads require resolution. These repairs preserve the accepted answer-plus-evidence contract and do not weaken any validation gate.



### paudley — 2026-09-30T17:38:39Z

## Stage 2 additional confirmed journal gap

G4 is reproduced through the real `Store` API: a 100,000-byte escaped replacement creates 600,333 bytes of prepared metadata; commit succeeds, retry/undo fail the metadata bound and recovery reports corruption. A 1,100,000-byte ordinary replacement under a 2,000,000-byte file limit also commits but retry/undo exceed the default JSON work budget.

G8 (HIGH): after a valid `a.txt` transaction, placing unrelated bytes in `other/.purrassist-<transaction-id>.tmp` makes `recover` delete that unrelated file and report `OrphanRemoved`. Matching the basename alone does not establish ownership or validate the temporary bytes. Repair will require a validated journal, its exact expected target-directory temporary path, and compatible recorded replacement bytes; unknown/conflicting/corrupt artifacts must remain intact and observable. Dedicated recovery and interruption controls will precede a separate normal-hook commit and branch push. This is additional required remediation, not an accepted limitation.



### paudley — 2026-09-30T17:49:34Z

## Stage 2 additional Markdown preservation gaps

- G9 (HIGH): reference relabeling rewrites the visible text of shortcut and collapsed references (and image alt text), although a label change is intended to preserve rendered content. Repair will materialize an explicit second label while retaining the exact display text, retain full-reference spelling where possible, and reject collisions or other semantic changes. Tests will cover LF/CRLF, images, inline markup, rendered equivalence, inversion, stale input, collisions and exhaustion. This is one reference-edit commit.
- G10 (HIGH): ordered-list renumbering can turn an indented code block into a paragraph when ordinal width grows. The current structural check only compares the list-item kind/ordinal/checkbox, despite Rustdoc requiring unchanged body semantics. Repair will check complete child structure and logical content, preserve safe renumbering, and explicitly refuse proposals that reinterpret the body. Tests will exercise width boundaries, nested containers, tabs, LF/CRLF, safe application and inversion. This is a separate list-validation commit.

Both were reproduced through actual CLI planning/application. Each fix requires focused verification and the full normal commit gate, then issue-branch push and issue/PR evidence updates.



### paudley — 2026-09-30T18:07:59Z

Stage-2 gap-plan addition for issue #1 / PR #2:

- **HIGH G11 — direct pattern generation bypasses its scratch limit.** A compiled `^a+$` generated independently valid 8,192-, 16,384-, and 32,768-byte witnesses with `scratch_bytes: 4096`; each run reported a constant 1,792-byte peak. The generator currently accounts for automaton sessions but allocates frontier strings, copied product states, cycle collections, and returned witness storage outside that accounting.
- Admit those allocations before they occur, retain reservations for their actual lifetimes, and release temporary search storage when ownership ends. Test output-size scaling, frontier growth, insufficient budgets, and cancellation through the public API. Resource exhaustion must remain `BudgetExceeded(ScratchBytes)`, never success or an emptiness claim.
- Keep the assertion-context repair (G3) and this resource-accounting repair in separate commits. Repeat the full local gate and final portability, fuzzing, resource, and archive qualification after both are complete.

The existing G5 hosted-CI blocker remains external: GitHub reported that the jobs did not start because of the account payment/spending condition. Local verification does not satisfy that hosted gate.


### paudley — 2026-09-30T18:09:01Z

**G3 (HIGH) repaired** in a3b361e: derivative witness search now preserves start/word-boundary context, restarts unanchored searches, and distinguishes permanent exclusions from matches that depend on the current string ending. `^a+$` excluding `^a$` now yields `aa`; assertion witnesses are validated independently by both engines.

Evidence: the 3,024-case finite target/exclusion/bounds matrix agrees with exhaustive backtracking evaluation; focused pattern/schema regressions and the full `make check` gate passed with the current repair set. G11 is a separate, confirmed allocation-accounting gap and remains in progress. Hosted qualification remains blocked by G5.


### paudley — 2026-09-30T18:10:39Z

**G1 (HIGH) repaired** in 1e94bea: crossed scalar/container bounds exclude only their applicable types; negated shape facets retain type guards; exclusive equal bounds retain the stronger constraint; every `contains` occurrence owns its own counts, while counts without `contains` are ignored. Optional impossible prefix items can fall back to a shorter valid array, and failed greedy array construction remains exhaustion rather than a false refutation.

Evidence: known-valid null/type-union/empty-array and overlapping-contains regressions pass, alongside the official 430 satisfiable witness groups, recursion/budget checks, core Clippy, and the full `make check` gate with the current repair set. G2 format-vocabulary semantics remains separate and is next. Hosted qualification remains blocked by G5.


### paudley — 2026-09-30T18:11:48Z

**G4 (HIGH) repaired** in 4bbd47b: file snapshots enforce the admitted file limit before hashing/diffing; journals admit JSON escaping expansion and are checked before transaction artifacts are created; parsing budgets derive from bounded serialized input. Resource errors retain their typed outcome across commit, retry, undo, and recovery instead of becoming stale/corrupt classifications.

Evidence: real filesystem tests cover heavily escaped 100 KiB input, a 1.1 MiB ordinary file, oversize snapshots, and journal bounds. The 21 file/crash/journal regressions, host Clippy, and the full `make check` gate passed with the current repair set. The unrelated-temporary-file recovery defect is tracked separately as G8. Hosted qualification remains blocked by G5.


### paudley — 2026-09-30T18:13:33Z

**G6 (HIGH) repaired** in 32c8cf4: validation retains nested format observations and pattern-engine identities, including unsuccessful alternatives evaluated by combinators. Child-instance evaluated properties/items remain scoped to their own instance and no longer leak into parent annotation accounting.

Evidence: 21 nested/combinator/reference cases, explicit resource propagation, parent-annotation isolation, and exported host tool evidence pass. Only the three affected golden receipts gained their previously missing derivative-engine identity; recovered outputs are unchanged. Official schema/format conformance and the full `make check` gate passed with the current repair set. Hosted qualification remains blocked by G5.


### paudley — 2026-09-30T18:14:42Z

**G7 (MEDIUM) repaired** in 0de9dc3: competing recovery alternatives capture their whole-schema validation index before format annotations are appended. They now reference the passing validation that establishes candidate acceptance, while failed annotation-only format observations remain independently visible.

Evidence: ambiguous repairs of `"baf"` to enum members `"bad"`/`"bar"` bind each output digest to the correct passed schema validation and preserve the exact original input; failed email observations remain in the receipt. The regression and full `make check` gate passed with the current repair set. Hosted qualification remains blocked by G5.


### paudley — 2026-09-30T18:15:44Z

**G9 (HIGH) repaired** in 2a7dccb: reference relabeling changes the reference label while preserving visible link text and image alt text. Shortcut and collapsed uses materialize an explicit second label. Both CommonMark and GFM rendering must remain identical; conflicting labels and newly activated unrelated references are rejected before returning a plan.

Evidence: link/image shortcut, collapsed, and full references, escaped/emphasized display text, LF/CRLF preservation, collision rejection, and exact inversion regressions pass. The existing 23 Markdown structure cases and full `make check` gate passed with the current repair set. Hosted qualification remains blocked by G5.


### paudley — 2026-09-30T18:16:37Z

**G10 (HIGH) repaired** in 6da7f3b: list renumbering validates complete item-body structure and logical leaf content under the existing resource governor. A wider marker can no longer silently reinterpret indented code as prose; unsafe plans return an explicit refusal, while safe renumbering remains invertible.

Evidence: one-/two-/three-digit marker transitions, LF/CRLF, nested quotes, tab-indented code, safe fenced content, and exact inverse application pass. The existing 23 Markdown structure cases and full `make check` gate passed with the current repair set. Hosted qualification remains blocked by G5.


### paudley — 2026-09-30T19:19:56Z

**G2 (HIGH) repaired** in d982fb7: witness generation constrains `format` only when the applicable resource dialect enables the format-assertion vocabulary. Annotation-only formats still produce observations, including under `not`, without excluding valid witnesses or manufacturing refutations. Required and optional supported assertion vocabularies retain enforcement.

Evidence: eight new cases cover annotation-only `const`/`enum`, negation, conditional/pattern constraints, dialect inheritance/override, nested and referenced resources, unsupported annotations, and retained validation observations. The focused 36-case witness/format suites, including the 430 official satisfiable groups, and core strict Clippy passed. The completed format and file-recovery changes also passed full `make check` in a detached verification checkout (651 Rust tests/doctests); its frozen files were byte-compared with the issue worktree. G11 generator storage changes were excluded from that gate and are being qualified separately. Hosted qualification remains blocked by G5.


### paudley — 2026-09-30T19:22:50Z

**G8 (HIGH) repaired** in 9b493b3: a temporary filename alone no longer authorizes deletion. Recovery requires a real validated journal, replayed patch/digest consistency, the exact temporary location beside its recorded target, and bounded regular-file bytes that are a prefix of the recorded replacement. Unknown identities remain unknown; invalid journals, symlinks, wrong locations, and incompatible bytes are preserved with reasons exposed through `recover_files`.

Evidence: twelve real-filesystem regressions cover unrelated bytes, corrupt/incomplete journals, symlinks, wrong parents, malformed locations, resource exhaustion, and valid empty/partial-UTF-8/full replacement cleanup. All 33 file/crash/journal/orphan cases and host strict Clippy passed. The byte-exact public MCP fixture was recaptured and replayed. An independent source/test/documentation review found no remaining ownership defect. Full `make check` passed for these frozen changes in the detached verification checkout (651 Rust tests/doctests), excluding the independently changing G11 generator files. Hosted qualification remains blocked by G5.


### paudley — 2026-09-30T19:37:03Z

**Additional G12 (MEDIUM): qualification claims exceed their executable scope.** The crash-checkpoint driver currently proves exact versions, recovery and idempotent retry, while the qualification record also claims checkpoint-specific undo. The Rust store preserves typed resource errors, but CLI/MCP handlers expose explanatory tool errors rather than the Rust enum.

Repair: exercise stale-undo refusal, exact-output undo with inverse digest/byte checks, and repeated-undo refusal after every one of the eight real abort checkpoints. Document that concrete scope and distinguish Rust store error typing from tool error messages. Validate the crash/files/journal/orphan suites, host strict Clippy, and the complete branch `make check` gate. Commit this test/documentation unit separately and post its results to the issue and PR. G11's confirmed derivative merge/traversal controls are being repaired concurrently; final qualification will use their completed source. G5 remains an external hosted-startup blocker.


### paudley — 2026-09-30T19:44:10Z

**G12 (MEDIUM) repaired** in 1af6593: every real abort checkpoint now checks conditional undo after exact-version recovery and idempotent retry. Intervening bytes cause refusal and remain unchanged; restoring the exact committed output permits undo with inverse digests and original-byte restoration; repeating the original undo refuses and preserves those bytes. Documentation distinguishes missing preparation from corrupt preparation, per-journal outcomes from top-level failures, and Rust store error variants from explanatory tool errors.

Evidence: all 33 filesystem/crash/journal/orphan cases pass, including these additional controls at all eight abort checkpoints. Full `make check` passed in the detached verification checkout (651 Rust tests/doctests, host strict Clippy and the complete gate); the two frozen G12 files were byte-compared with the issue worktree. G11 generator/derivative revisions were excluded and will receive a fresh branch gate. G5 remains an external hosted-startup blocker.


### paudley — 2026-09-30T21:06:20Z

**G11 (HIGH) repaired** in 66b3756776a072af1889596ae98a5faa5a50cea6: pattern compilation and generation now admit owned terms, indices, classes, predicate cuts, frontiers, captures, frames and prefix storage. Iterative union/nullable/derivative traversal bills Work, polls cancellation and propagates stops. Assertion-context caches preserve all four flags. Witness output retains its charge and failed operations release temporary storage.

Evidence: plain `make check` passed through the normal build framework, including strict workspace Clippy, the complete verification-profile test/doctest suite, Rustdoc, attribution/release checks and shared-helper enforcement. Thirty allocator controls cover zero-slack compilation/generation, exact scratch/Work limits and cancellation; focused regressions include 32,768 flat literals, wide alternations and 430 official witness groups. Independent final-source review accepted the frozen seven-file manifest. No commit or build controls were bypassed for this commit.

The user directed this session to ignore GitHub CI completely. The earlier G5 readiness blocker is withdrawn for this session; qualification and readiness are being evaluated from local evidence. Fresh portability, fuzz, resource and package qualification follows through the normal project commands.


### paudley — 2026-09-30T21:26:55Z

Stage 2 is ready for local Stage 3 finalization at `34fb2ac5b2c07fb8c5dccfac6adc5d1b6e93091e` on `paudley/1-release-qualification`.

All eleven confirmed gaps G1/G2/G3/G4/G6/G7/G8/G9/G10/G11/G12 are repaired with separate signed commits and regressions. G11 is `66b3756`; fresh qualification archives and documentation are `34fb2ac`. Complete review/comment/thread intake and the full branch deferral audit found no unresolved feedback or unauthorized deferrals.

Fresh verification through the normal project framework, without caller environment overrides or alternate Cargo launchers:
- `make check`: 666 tests/doctests in 93 groups, strict Clippy, Rustdoc, hygiene and helper controls pass.
- `make syntax-oracles`: 15 probes pass; `make wasm-test`: native/WASI core, syntax, host and executed browser-WASM core transcripts match.
- `make fuzz`: all eight campaigns, 8,076 inputs; zero crashes, mismatches or resource non-results. Reports are preserved byte for byte with guarded source/build identities.
- Documented resource target and unchanged limits: nine profiles, 25 core latency cases, five syntax cases, six recovery cases and five artifacts pass.
- Clean `make package`: five fully verified, source/license-audited archives all identify this final commit with no dirty VCS flag.

All prior report bytes are unchanged. Twelve Stage 2 commits have valid signatures; the issue worktree is clean, deficiency ledgers are clear, and main plus all four unrelated dirty-work files are unchanged. The owned temporary verification worktree was removed after preserving its evidence and verifying it contained no unique commits or unrelated changes.

GitHub CI is ignored for this session by explicit user instruction, so the earlier G5 blocker is withdrawn for this session. This is local readiness, not a publication claim. The accepted release sequence remains reviewed merge followed by separately authorized tagging, publication and downloaded-package acceptance; issue #1 stays open until that acceptance.

Verified archive receipts:

| Archive | Bytes | SHA-256 |
| --- | ---: | --- |
| `purrassist-0.1.0.crate` | 11,911 | `4021704665fd6350bcc2aedc5f0d2ac72c70d0cd23048444ebb6900cca386843` |
| `purrassist-cli-0.1.0.crate` | 23,762 | `f177a533c0dfeb8c967d82349a7a60745338233f148ed49932acf7a38ba20a26` |
| `purrassist-core-0.1.0.crate` | 1,761,854 | `9701aac986dacafac2077df7c3221dbed5d1e9ee56da0776b1389529b036e8e3` |
| `purrassist-host-0.1.0.crate` | 94,981 | `8217920287ed640264866eab204bcc42ff0cbe5eac519788285582b364edcd5c` |
| `purrassist-syntax-0.1.0.crate` | 36,220 | `7ad04e62af1cd470ee6b1ec94b2b06429552949b44e8da43cd1fea3573d403fa` |


### paudley — 2026-09-30T21:35:17Z

feat(purrassist): complete governed 0.1.0 capabilities and qualification

Complete the accepted implementation for issue #1 through PR #2: a pure first-party Rust core, composed JSON/schema recovery and witnesses, Unicode-aware matching, source-preserving document and syntax edits, alternative-specific provenance, portable execution and root-scoped durable file transactions. Qualify the integrated implementation through the normal project framework. This is the reviewed implementation merge; tagging, registry publication and downloaded-package acceptance remain separately authorized in the accepted release sequence. Issue #1 stays open for that acceptance.

Requirements satisfied

- R1: Draft 2020-12 validation, symbolic derivatives and bounded backtracking, recursive/dynamic references, dialect-specific formats, explicit unknown vocabularies, witness/refutation/exhaustion distinctions and composed recovery.
- R2/R4: section/list/table/reference operations, patch algebra/rebase, shared Rust/CLI/MCP registry and alternative-specific digest-bound evidence/loss/inference ledgers; retained original input and labeled guesses.
- R3: admitted Work, scratch, candidates, depth and cancellation, owned scratch containers and external parser accounts; resource exhaustion propagates explicitly. Pattern compile/generation includes zero-slack finite allocator controls and iterative traversal.
- R5/R7: official schema/format, Unicode and CommonMark/GFM suites; independent syntax and rendering/pattern oracles; corruption/preservation/correction-quality corpora; eight coverage-guided targets; measured allocation, memory, latency, artifact sizes and cold build with source/tool provenance and limits.
- R6: native/WASI/browser-WASM transcripts, Windows write-through implementation and root-scoped journal, retry, stale-input, interruption and conditional-undo controls. All eight crash checkpoints exercise exact versions, recovery, retry and conditional undo locally.
- R8/R10: all five local release archives fully verified and audited for source/license/notices; core has no third-party runtime dependencies or ambient filesystem/network/clock/process/random access. Other dependencies, generated data and reference modifications are registered with provenance.
- R9: release/qualification tooling and five existing 0.0.0 name records are distinct from the real 0.1.0 publication. No published-release or installed-download acceptance claim is made here.
- R11-R14: isolated issue branch, signed commits, preserved unrelated main work, no commit-hook bypass, and final merge exclusively through the required ghprsq tool.

Stage 2 repaired eleven confirmed gaps in separate commits: type applicability and occurrence-scoped contains; format vocabulary constraints; assertion-context generation; escaped journal admission; nested evidence; whole-schema alternative bindings; orphan ownership; Markdown display/alt and list-body preservation; admitted compile/generation storage and iterative traversal; and undo after every crash checkpoint.

Files and purposes

| Paths | Purpose |
| --- | --- |
| crates/core/src, crates/core/tests, crates/core/examples | Pure governed engines; Unicode/schema/pattern data and conformance; preservation/provenance/resource regressions; bounded fuzz protocols and portable fixture transcripts. |
| crates/syntax | Registered grammar integration, syntax-aware edits and per-owner external allocation/cancellation accounting. |
| crates/host | Shared operation registry and wire receipts, helper continuations, filesystem locking/journals/recovery, CLI/MCP equivalence and crash controls. |
| crates/cli, crates/purrassist | User CLI/protocol adapters and stable facade over the typed engines. |
| crates/helper-census | Provenance-preserving development detector and mandatory shipping-code helper enforcement; tests keep independent oracle bytes. |
| crates/wasm-harness | Executed native/WASI/browser-WASM comparison transcripts. |
| docs | Public implemented contracts, technical/source provenance, finite qualification evidence, resource limits and authorized release procedure. New Stage 2 reports preserve actual observations and leave earlier baseline bytes unchanged. |
| scripts | Existing project qualification, corpus generation, tooling/source/notice auditing, package and downloaded-package verification, fuzzing and measurements. |
| vendor | Registered Unicode/schema/corpus/reference/tooling inventories, licenses, source digests and transparent reference corrections. |
| .github, root manifests/configuration/licenses/README | Maintained release/qualification workflow definitions, workspace/package configuration, public documentation, dependency registration and attribution. |

A complete added/modified/deleted path inventory follows at the end; each path belongs to the purpose above. The branch changes 807 files, with 279,778 insertions and 3,972 deletions across 61 signed branch commits. Development tooling follows the existing Python/Make framework; core implementation remains Rust.

Validation and exact scope

- Normal make check: exit 0, 666 Rust tests/doctests across 93 groups; formatting, attribution/release hygiene, helper controls and strict enforcement, workspace Clippy and Rustdoc all pass.
- Normal make syntax-oracles: exit 0; all 15 independent compiler probes agree.
- Normal make wasm-test: exit 0; native/WASI core, syntax and host plus executed browser-WASM core transcripts match.
- Normal make fuzz: exit 0; all eight 1,000-mutation campaigns, 8,076 inputs, no crashes/mismatches/resource non-results. Markdown has 981 comparisons; patterns have 330, with 77 separately counted reference rejections and no timeouts.
- Normal documented resource target with portable runtime: exit 0; nine profiles, 25 core latency cases, five syntax cases, six recovery cases and five artifact sizes. The unchanged resource-limit checker passes. Cold CLI build: 21.671 seconds, 35 built and zero reused artifacts.
- Clean normal make package: exit 0; eight packaging controls, full Cargo verification and source/license/notice audit for all five archives. Every VCS record identifies clean head 34fb2ac5b2c07fb8c5dccfac6adc5d1b6e93091e.

These integration commands were rerun through the normal framework without caller environment overrides or alternate Cargo launchers. GitHub CI is ignored for this session by explicit user instruction; no hosted-check result is used or claimed. Local evidence supports this finalization.

Fresh campaign/resource reports retain actual launch commit 66b3756776a072af1889596ae98a5faa5a50cea6, observed clean/dirty flags and guarded engine/harness/runner identities. Final commit 34fb2ac changes documentation and archived observations only. Earlier report bytes remain unchanged. Finite suites/campaigns and fixture-specific measurements establish executed cases, not universal correctness, whole-program allocator/RSS bounds or production correction-risk guarantees.

Standing constraints

.goals was read and respected: greenfield Rust core, complete accepted capability, explicit outcomes, maximal utility/performance/portability and no silent packaging fallback. Core purity, recovery provenance, license/source attribution, registered dependencies, public Rustdoc/error semantics and executable capability evidence passed the normal gate. Both deficiency ledgers are absent/clear immediately before these notes. No deficiency entry is treated as an accepted risk or scope reduction.

Deferral Gate

Passed with recorded false-positive evidence. All 87 mechanical added-line matches are exactly the previously independently classified Stage 2 text multiset: 74 literal Markdown task/content fixtures and oracle output; two Unicode MONGOLIAN LETTER TODO names; seven helper-detector format-placeholder descriptions or positive fixture strings containing todo!/unimplemented!; three implemented source-representation mechanics (layout placeholder property, live reference resolution, scope-pop ordering); and one already-completed empty 0.0.0 name-reservation description. They are fixture data, Unicode names, implemented mechanics or a completed registry fact, not production executable stubs.

The expanded audit's one additional inherited line, crates/helper-census/src/layout.rs:8, defines shipping-code census scope and preserves independent preimage/wireimage test oracles; it is not an implementation capability omission and is unchanged by Stage 2. Current PR body/comments have no mandatory discussion-scan matches. Full comments/reviews/inline/thread pagination found no unresolved feedback. The accepted post-merge publication/installed-artifact ceremony is an explicit authorization boundary in issue #1, not an abandoned implementation requirement.

Implementation notes

Scratch ownership tracks actual allocation lifetimes, including consuming iterators and retained outputs. Flat pattern union/nullable/derivative traversal is iterative, charges Work and polls cancellation; nullable caches retain all four assertion-context flags. Compile failures propagate typed stops instead of manufacturing a dead language. Schema observations remain visible while evaluated annotations stay scoped to their own instance; alternatives bind their accepting whole-schema validation independently. File recovery validates the journal, replayed digests, target-parent location and replacement prefix before deleting a temporary artifact. Markdown reference relabeling preserves display/alt text, while list renumbering rejects body reinterpretation.

Origin-base synchronization and conflicts

Fetched origin/main at be82a33d2000dd8558ad7b76268d2aab12853ccc and merged it into the issue branch: already up to date. No merge commit, conflicts or source changes were needed. Local and remote PR heads match 34fb2ac5b2c07fb8c5dccfac6adc5d1b6e93091e; normal issue-branch push reports up to date. Main's four unrelated modified/untracked files are hash-verified and snapshotted for byte-exact preservation during checkout synchronization. Only the issue-specific worktree and local branch will be removed; other worktrees remain.

Complete path inventory (A added, M modified, D deleted)

```text
A	.github/workflows/publish.yml
M	.github/workflows/release-qualification.yml
M	.github/workflows/verify.yml
M	Cargo.lock
M	Cargo.toml
M	Makefile
M	README.md
M	SECURITY.md
M	crates/cli/src/commands.rs
M	crates/cli/tests/protocol.rs
M	crates/core/Cargo.toml
M	crates/core/README.md
A	crates/core/benches/latency.rs
A	crates/core/examples/fuzz_fuzzy.rs
A	crates/core/examples/fuzz_markdown.rs
A	crates/core/examples/fuzz_matching.rs
A	crates/core/examples/fuzz_patch.rs
A	crates/core/examples/fuzz_pattern.rs
A	crates/core/examples/fuzz_schema.rs
A	crates/core/examples/fuzz_unified.rs
M	crates/core/src/contract.rs
M	crates/core/src/edits.rs
M	crates/core/src/edits/fuzzy.rs
M	crates/core/src/edits/unified.rs
M	crates/core/src/embeddings.rs
M	crates/core/src/intent.rs
M	crates/core/src/intent/selection.rs
M	crates/core/src/json/mod.rs
M	crates/core/src/json/number.rs
M	crates/core/src/json/parser.rs
M	crates/core/src/json/repair.rs
M	crates/core/src/lib.rs
M	crates/core/src/markdown.rs
M	crates/core/src/markdown/containers.rs
M	crates/core/src/markdown/context.rs
M	crates/core/src/markdown/inline.rs
M	crates/core/src/markdown/inline/angle.rs
M	crates/core/src/markdown/inline/autolinks.rs
M	crates/core/src/markdown/inline/parse.rs
M	crates/core/src/markdown/inline/references.rs
M	crates/core/src/markdown/inline/render.rs
M	crates/core/src/markdown/lists.rs
M	crates/core/src/markdown/projection.rs
M	crates/core/src/markdown/references.rs
M	crates/core/src/markdown/render.rs
M	crates/core/src/markdown/sections.rs
M	crates/core/src/markdown/sections/nested.rs
M	crates/core/src/markdown/table.rs
M	crates/core/src/markdown/table_columns.rs
M	crates/core/src/markdown/table_rows.rs
M	crates/core/src/matching.rs
A	crates/core/src/patch.rs
M	crates/core/src/paths.rs
D	crates/core/src/pattern.rs
A	crates/core/src/pattern/ast.rs
A	crates/core/src/pattern/backtrack.rs
A	crates/core/src/pattern/derive.rs
A	crates/core/src/pattern/generate.rs
A	crates/core/src/pattern/mod.rs
A	crates/core/src/pattern/parse.rs
A	crates/core/src/pattern/storage.rs
A	crates/core/src/pattern/unicode.rs
A	crates/core/src/pattern/unicode/unicode_data.rs
A	crates/core/src/provenance.rs
A	crates/core/src/schema/algebra/canon.rs
A	crates/core/src/schema/algebra/generate.rs
A	crates/core/src/schema/algebra/merge.rs
A	crates/core/src/schema/algebra/mod.rs
M	crates/core/src/schema/compile.rs
M	crates/core/src/schema/evaluate.rs
A	crates/core/src/schema/format/datetime.rs
A	crates/core/src/schema/format/email.rs
A	crates/core/src/schema/format/host.rs
A	crates/core/src/schema/format/mod.rs
A	crates/core/src/schema/format/net.rs
A	crates/core/src/schema/format/pointer.rs
A	crates/core/src/schema/format/regexp.rs
A	crates/core/src/schema/format/template.rs
A	crates/core/src/schema/format/uri.rs
A	crates/core/src/schema/format/uuid.rs
M	crates/core/src/schema/mod.rs
M	crates/core/src/schema/recovery.rs
M	crates/core/src/schema/recovery/proposals.rs
M	crates/core/src/schema/vocabulary.rs
A	crates/core/src/text/idna_data.rs
M	crates/core/src/text/mod.rs
A	crates/core/src/text/punycode.rs
M	crates/core/src/text/unicode_data.rs
M	crates/core/src/wire.rs
A	crates/core/tests/allocation_oracle.rs
A	crates/core/tests/correction_evaluation.rs
A	crates/core/tests/corruption_oracle.rs
A	crates/core/tests/data/evaluation/README.md
A	crates/core/tests/data/evaluation/generate.py
A	crates/core/tests/data/evaluation/json_recovery.json
A	crates/core/tests/data/evaluation/manifest.json
A	crates/core/tests/data/evaluation/paths.json
A	crates/core/tests/data/evaluation/phonetic.json
A	crates/core/tests/data/evaluation/strings.json
A	crates/core/tests/data/receipts/v2-json-recover-ambiguous-duplicate-keys.json
A	crates/core/tests/data/recovery-goldens/additional-properties-remove.json
A	crates/core/tests/data/recovery-goldens/additional-properties-schema-repair.json
A	crates/core/tests/data/recovery-goldens/all-of-combined-repair.json
A	crates/core/tests/data/recovery-goldens/any-of-branch-selection.json
A	crates/core/tests/data/recovery-goldens/array-single-element-unwrap.json
A	crates/core/tests/data/recovery-goldens/boolean-schema-false-property-unresolved.json
A	crates/core/tests/data/recovery-goldens/boolean-schema-true-property.json
A	crates/core/tests/data/recovery-goldens/const-basic-invention.json
A	crates/core/tests/data/recovery-goldens/const-key-typo-combined.json
A	crates/core/tests/data/recovery-goldens/contains-invention.json
A	crates/core/tests/data/recovery-goldens/defs-multiple-refs-repair.json
A	crates/core/tests/data/recovery-goldens/dependent-required-unresolved.json
A	crates/core/tests/data/recovery-goldens/dependent-schemas-value-repair.json
A	crates/core/tests/data/recovery-goldens/duplicate-keys-ambiguous.json
A	crates/core/tests/data/recovery-goldens/dynamic-ref-extensible-repair.json
A	crates/core/tests/data/recovery-goldens/enum-case-fold-correction.json
A	crates/core/tests/data/recovery-goldens/enum-typo-correction.json
A	crates/core/tests/data/recovery-goldens/exclusive-bounds-invention.json
A	crates/core/tests/data/recovery-goldens/format-date-repair.json
A	crates/core/tests/data/recovery-goldens/format-date-time-repair.json
A	crates/core/tests/data/recovery-goldens/format-email-invention.json
A	crates/core/tests/data/recovery-goldens/format-hostname-invention.json
A	crates/core/tests/data/recovery-goldens/format-ipv4-invention.json
A	crates/core/tests/data/recovery-goldens/format-time-repair.json
A	crates/core/tests/data/recovery-goldens/format-uuid-invention.json
A	crates/core/tests/data/recovery-goldens/if-else-branch-repair.json
A	crates/core/tests/data/recovery-goldens/if-then-branch-repair.json
A	crates/core/tests/data/recovery-goldens/kitchen-sink-composed-repair.json
A	crates/core/tests/data/recovery-goldens/max-length-string-still-satisfiable.json
A	crates/core/tests/data/recovery-goldens/max-properties-violation-unresolved.json
A	crates/core/tests/data/recovery-goldens/min-items-and-unique-items-invention.json
A	crates/core/tests/data/recovery-goldens/min-items-invention.json
A	crates/core/tests/data/recovery-goldens/min-length-invention.json
A	crates/core/tests/data/recovery-goldens/min-max-contains-invention.json
A	crates/core/tests/data/recovery-goldens/min-properties-padding-unresolved.json
A	crates/core/tests/data/recovery-goldens/minimum-invention.json
A	crates/core/tests/data/recovery-goldens/multiple-of-invention.json
A	crates/core/tests/data/recovery-goldens/multiple-of-with-minimum-invention.json
A	crates/core/tests/data/recovery-goldens/nested-decode-string-value.json
A	crates/core/tests/data/recovery-goldens/not-schema-unaffected-key-repair.json
A	crates/core/tests/data/recovery-goldens/one-of-exclusive-branch.json
A	crates/core/tests/data/recovery-goldens/pattern-and-length-invention.json
A	crates/core/tests/data/recovery-goldens/pattern-invention.json
A	crates/core/tests/data/recovery-goldens/pattern-properties-value-repair.json
A	crates/core/tests/data/recovery-goldens/prefix-items-typed-invention.json
A	crates/core/tests/data/recovery-goldens/properties-key-case-fold.json
A	crates/core/tests/data/recovery-goldens/properties-key-depunctuate.json
A	crates/core/tests/data/recovery-goldens/properties-key-edit-distance.json
A	crates/core/tests/data/recovery-goldens/property-names-repair.json
A	crates/core/tests/data/recovery-goldens/prose-wrapped-json-repair.json
A	crates/core/tests/data/recovery-goldens/ref-indirection-repair.json
A	crates/core/tests/data/recovery-goldens/required-array-of-objects-invention.json
A	crates/core/tests/data/recovery-goldens/required-basic-invention.json
A	crates/core/tests/data/recovery-goldens/required-object-invention.json
A	crates/core/tests/data/recovery-goldens/required-relocation.json
A	crates/core/tests/data/recovery-goldens/shape-unwrap-single-array.json
A	crates/core/tests/data/recovery-goldens/shape-unwrap-single-object.json
A	crates/core/tests/data/recovery-goldens/stringified-json-repair.json
A	crates/core/tests/data/recovery-goldens/truncated-json-repair.json
A	crates/core/tests/data/recovery-goldens/type-decode-string.json
A	crates/core/tests/data/recovery-goldens/type-stringify-scalar.json
A	crates/core/tests/data/recovery-goldens/type-swap-and-nested-stringify.json
A	crates/core/tests/data/recovery-goldens/type-wrap-array.json
A	crates/core/tests/data/recovery-goldens/unevaluated-items-fallback-repair.json
A	crates/core/tests/data/recovery-goldens/unevaluated-properties-fallback-repair.json
A	crates/core/tests/data/recovery-goldens/unique-items-violation-unresolved.json
A	crates/core/tests/data/test262/LICENSE
A	crates/core/tests/data/test262/generated/ASCII.js
A	crates/core/tests/data/test262/generated/ASCII_Hex_Digit.js
A	crates/core/tests/data/test262/generated/Alphabetic.js
A	crates/core/tests/data/test262/generated/Any.js
A	crates/core/tests/data/test262/generated/Assigned.js
A	crates/core/tests/data/test262/generated/Bidi_Control.js
A	crates/core/tests/data/test262/generated/Bidi_Mirrored.js
A	crates/core/tests/data/test262/generated/Case_Ignorable.js
A	crates/core/tests/data/test262/generated/Cased.js
A	crates/core/tests/data/test262/generated/Changes_When_Casefolded.js
A	crates/core/tests/data/test262/generated/Changes_When_Casemapped.js
A	crates/core/tests/data/test262/generated/Changes_When_Lowercased.js
A	crates/core/tests/data/test262/generated/Changes_When_NFKC_Casefolded.js
A	crates/core/tests/data/test262/generated/Changes_When_Titlecased.js
A	crates/core/tests/data/test262/generated/Changes_When_Uppercased.js
A	crates/core/tests/data/test262/generated/Dash.js
A	crates/core/tests/data/test262/generated/Default_Ignorable_Code_Point.js
A	crates/core/tests/data/test262/generated/Deprecated.js
A	crates/core/tests/data/test262/generated/Diacritic.js
A	crates/core/tests/data/test262/generated/Emoji.js
A	crates/core/tests/data/test262/generated/Emoji_Component.js
A	crates/core/tests/data/test262/generated/Emoji_Modifier.js
A	crates/core/tests/data/test262/generated/Emoji_Modifier_Base.js
A	crates/core/tests/data/test262/generated/Emoji_Presentation.js
A	crates/core/tests/data/test262/generated/Extended_Pictographic.js
A	crates/core/tests/data/test262/generated/Extender.js
A	crates/core/tests/data/test262/generated/General_Category_-_Cased_Letter.js
A	crates/core/tests/data/test262/generated/General_Category_-_Close_Punctuation.js
A	crates/core/tests/data/test262/generated/General_Category_-_Connector_Punctuation.js
A	crates/core/tests/data/test262/generated/General_Category_-_Control.js
A	crates/core/tests/data/test262/generated/General_Category_-_Currency_Symbol.js
A	crates/core/tests/data/test262/generated/General_Category_-_Dash_Punctuation.js
A	crates/core/tests/data/test262/generated/General_Category_-_Decimal_Number.js
A	crates/core/tests/data/test262/generated/General_Category_-_Enclosing_Mark.js
A	crates/core/tests/data/test262/generated/General_Category_-_Final_Punctuation.js
A	crates/core/tests/data/test262/generated/General_Category_-_Format.js
A	crates/core/tests/data/test262/generated/General_Category_-_Initial_Punctuation.js
A	crates/core/tests/data/test262/generated/General_Category_-_Letter.js
A	crates/core/tests/data/test262/generated/General_Category_-_Letter_Number.js
A	crates/core/tests/data/test262/generated/General_Category_-_Line_Separator.js
A	crates/core/tests/data/test262/generated/General_Category_-_Lowercase_Letter.js
A	crates/core/tests/data/test262/generated/General_Category_-_Mark.js
A	crates/core/tests/data/test262/generated/General_Category_-_Math_Symbol.js
A	crates/core/tests/data/test262/generated/General_Category_-_Modifier_Letter.js
A	crates/core/tests/data/test262/generated/General_Category_-_Modifier_Symbol.js
A	crates/core/tests/data/test262/generated/General_Category_-_Nonspacing_Mark.js
A	crates/core/tests/data/test262/generated/General_Category_-_Number.js
A	crates/core/tests/data/test262/generated/General_Category_-_Open_Punctuation.js
A	crates/core/tests/data/test262/generated/General_Category_-_Other.js
A	crates/core/tests/data/test262/generated/General_Category_-_Other_Letter.js
A	crates/core/tests/data/test262/generated/General_Category_-_Other_Number.js
A	crates/core/tests/data/test262/generated/General_Category_-_Other_Punctuation.js
A	crates/core/tests/data/test262/generated/General_Category_-_Other_Symbol.js
A	crates/core/tests/data/test262/generated/General_Category_-_Paragraph_Separator.js
A	crates/core/tests/data/test262/generated/General_Category_-_Private_Use.js
A	crates/core/tests/data/test262/generated/General_Category_-_Punctuation.js
A	crates/core/tests/data/test262/generated/General_Category_-_Separator.js
A	crates/core/tests/data/test262/generated/General_Category_-_Space_Separator.js
A	crates/core/tests/data/test262/generated/General_Category_-_Spacing_Mark.js
A	crates/core/tests/data/test262/generated/General_Category_-_Surrogate.js
A	crates/core/tests/data/test262/generated/General_Category_-_Symbol.js
A	crates/core/tests/data/test262/generated/General_Category_-_Titlecase_Letter.js
A	crates/core/tests/data/test262/generated/General_Category_-_Unassigned.js
A	crates/core/tests/data/test262/generated/General_Category_-_Uppercase_Letter.js
A	crates/core/tests/data/test262/generated/Grapheme_Base.js
A	crates/core/tests/data/test262/generated/Grapheme_Extend.js
A	crates/core/tests/data/test262/generated/Hex_Digit.js
A	crates/core/tests/data/test262/generated/IDS_Binary_Operator.js
A	crates/core/tests/data/test262/generated/IDS_Trinary_Operator.js
A	crates/core/tests/data/test262/generated/ID_Continue.js
A	crates/core/tests/data/test262/generated/ID_Start.js
A	crates/core/tests/data/test262/generated/Ideographic.js
A	crates/core/tests/data/test262/generated/Join_Control.js
A	crates/core/tests/data/test262/generated/Logical_Order_Exception.js
A	crates/core/tests/data/test262/generated/Lowercase.js
A	crates/core/tests/data/test262/generated/Math.js
A	crates/core/tests/data/test262/generated/Noncharacter_Code_Point.js
A	crates/core/tests/data/test262/generated/Pattern_Syntax.js
A	crates/core/tests/data/test262/generated/Pattern_White_Space.js
A	crates/core/tests/data/test262/generated/Quotation_Mark.js
A	crates/core/tests/data/test262/generated/Radical.js
A	crates/core/tests/data/test262/generated/Regional_Indicator.js
A	crates/core/tests/data/test262/generated/Script_-_Adlam.js
A	crates/core/tests/data/test262/generated/Script_-_Ahom.js
A	crates/core/tests/data/test262/generated/Script_-_Anatolian_Hieroglyphs.js
A	crates/core/tests/data/test262/generated/Script_-_Arabic.js
A	crates/core/tests/data/test262/generated/Script_-_Armenian.js
A	crates/core/tests/data/test262/generated/Script_-_Avestan.js
A	crates/core/tests/data/test262/generated/Script_-_Balinese.js
A	crates/core/tests/data/test262/generated/Script_-_Bamum.js
A	crates/core/tests/data/test262/generated/Script_-_Bassa_Vah.js
A	crates/core/tests/data/test262/generated/Script_-_Batak.js
A	crates/core/tests/data/test262/generated/Script_-_Bengali.js
A	crates/core/tests/data/test262/generated/Script_-_Beria_Erfe.js
A	crates/core/tests/data/test262/generated/Script_-_Bhaiksuki.js
A	crates/core/tests/data/test262/generated/Script_-_Bopomofo.js
A	crates/core/tests/data/test262/generated/Script_-_Brahmi.js
A	crates/core/tests/data/test262/generated/Script_-_Braille.js
A	crates/core/tests/data/test262/generated/Script_-_Buginese.js
A	crates/core/tests/data/test262/generated/Script_-_Buhid.js
A	crates/core/tests/data/test262/generated/Script_-_Canadian_Aboriginal.js
A	crates/core/tests/data/test262/generated/Script_-_Carian.js
A	crates/core/tests/data/test262/generated/Script_-_Caucasian_Albanian.js
A	crates/core/tests/data/test262/generated/Script_-_Chakma.js
A	crates/core/tests/data/test262/generated/Script_-_Cham.js
A	crates/core/tests/data/test262/generated/Script_-_Cherokee.js
A	crates/core/tests/data/test262/generated/Script_-_Chorasmian.js
A	crates/core/tests/data/test262/generated/Script_-_Common.js
A	crates/core/tests/data/test262/generated/Script_-_Coptic.js
A	crates/core/tests/data/test262/generated/Script_-_Cuneiform.js
A	crates/core/tests/data/test262/generated/Script_-_Cypriot.js
A	crates/core/tests/data/test262/generated/Script_-_Cypro_Minoan.js
A	crates/core/tests/data/test262/generated/Script_-_Cyrillic.js
A	crates/core/tests/data/test262/generated/Script_-_Deseret.js
A	crates/core/tests/data/test262/generated/Script_-_Devanagari.js
A	crates/core/tests/data/test262/generated/Script_-_Dives_Akuru.js
A	crates/core/tests/data/test262/generated/Script_-_Dogra.js
A	crates/core/tests/data/test262/generated/Script_-_Duployan.js
A	crates/core/tests/data/test262/generated/Script_-_Egyptian_Hieroglyphs.js
A	crates/core/tests/data/test262/generated/Script_-_Elbasan.js
A	crates/core/tests/data/test262/generated/Script_-_Elymaic.js
A	crates/core/tests/data/test262/generated/Script_-_Ethiopic.js
A	crates/core/tests/data/test262/generated/Script_-_Garay.js
A	crates/core/tests/data/test262/generated/Script_-_Georgian.js
A	crates/core/tests/data/test262/generated/Script_-_Glagolitic.js
A	crates/core/tests/data/test262/generated/Script_-_Gothic.js
A	crates/core/tests/data/test262/generated/Script_-_Grantha.js
A	crates/core/tests/data/test262/generated/Script_-_Greek.js
A	crates/core/tests/data/test262/generated/Script_-_Gujarati.js
A	crates/core/tests/data/test262/generated/Script_-_Gunjala_Gondi.js
A	crates/core/tests/data/test262/generated/Script_-_Gurmukhi.js
A	crates/core/tests/data/test262/generated/Script_-_Gurung_Khema.js
A	crates/core/tests/data/test262/generated/Script_-_Han.js
A	crates/core/tests/data/test262/generated/Script_-_Hangul.js
A	crates/core/tests/data/test262/generated/Script_-_Hanifi_Rohingya.js
A	crates/core/tests/data/test262/generated/Script_-_Hanunoo.js
A	crates/core/tests/data/test262/generated/Script_-_Hatran.js
A	crates/core/tests/data/test262/generated/Script_-_Hebrew.js
A	crates/core/tests/data/test262/generated/Script_-_Hiragana.js
A	crates/core/tests/data/test262/generated/Script_-_Imperial_Aramaic.js
A	crates/core/tests/data/test262/generated/Script_-_Inherited.js
A	crates/core/tests/data/test262/generated/Script_-_Inscriptional_Pahlavi.js
A	crates/core/tests/data/test262/generated/Script_-_Inscriptional_Parthian.js
A	crates/core/tests/data/test262/generated/Script_-_Javanese.js
A	crates/core/tests/data/test262/generated/Script_-_Kaithi.js
A	crates/core/tests/data/test262/generated/Script_-_Kannada.js
A	crates/core/tests/data/test262/generated/Script_-_Katakana.js
A	crates/core/tests/data/test262/generated/Script_-_Kawi.js
A	crates/core/tests/data/test262/generated/Script_-_Kayah_Li.js
A	crates/core/tests/data/test262/generated/Script_-_Kharoshthi.js
A	crates/core/tests/data/test262/generated/Script_-_Khitan_Small_Script.js
A	crates/core/tests/data/test262/generated/Script_-_Khmer.js
A	crates/core/tests/data/test262/generated/Script_-_Khojki.js
A	crates/core/tests/data/test262/generated/Script_-_Khudawadi.js
A	crates/core/tests/data/test262/generated/Script_-_Kirat_Rai.js
A	crates/core/tests/data/test262/generated/Script_-_Lao.js
A	crates/core/tests/data/test262/generated/Script_-_Latin.js
A	crates/core/tests/data/test262/generated/Script_-_Lepcha.js
A	crates/core/tests/data/test262/generated/Script_-_Limbu.js
A	crates/core/tests/data/test262/generated/Script_-_Linear_A.js
A	crates/core/tests/data/test262/generated/Script_-_Linear_B.js
A	crates/core/tests/data/test262/generated/Script_-_Lisu.js
A	crates/core/tests/data/test262/generated/Script_-_Lycian.js
A	crates/core/tests/data/test262/generated/Script_-_Lydian.js
A	crates/core/tests/data/test262/generated/Script_-_Mahajani.js
A	crates/core/tests/data/test262/generated/Script_-_Makasar.js
A	crates/core/tests/data/test262/generated/Script_-_Malayalam.js
A	crates/core/tests/data/test262/generated/Script_-_Mandaic.js
A	crates/core/tests/data/test262/generated/Script_-_Manichaean.js
A	crates/core/tests/data/test262/generated/Script_-_Marchen.js
A	crates/core/tests/data/test262/generated/Script_-_Masaram_Gondi.js
A	crates/core/tests/data/test262/generated/Script_-_Medefaidrin.js
A	crates/core/tests/data/test262/generated/Script_-_Meetei_Mayek.js
A	crates/core/tests/data/test262/generated/Script_-_Mende_Kikakui.js
A	crates/core/tests/data/test262/generated/Script_-_Meroitic_Cursive.js
A	crates/core/tests/data/test262/generated/Script_-_Meroitic_Hieroglyphs.js
A	crates/core/tests/data/test262/generated/Script_-_Miao.js
A	crates/core/tests/data/test262/generated/Script_-_Modi.js
A	crates/core/tests/data/test262/generated/Script_-_Mongolian.js
A	crates/core/tests/data/test262/generated/Script_-_Mro.js
A	crates/core/tests/data/test262/generated/Script_-_Multani.js
A	crates/core/tests/data/test262/generated/Script_-_Myanmar.js
A	crates/core/tests/data/test262/generated/Script_-_Nabataean.js
A	crates/core/tests/data/test262/generated/Script_-_Nag_Mundari.js
A	crates/core/tests/data/test262/generated/Script_-_Nandinagari.js
A	crates/core/tests/data/test262/generated/Script_-_New_Tai_Lue.js
A	crates/core/tests/data/test262/generated/Script_-_Newa.js
A	crates/core/tests/data/test262/generated/Script_-_Nko.js
A	crates/core/tests/data/test262/generated/Script_-_Nushu.js
A	crates/core/tests/data/test262/generated/Script_-_Nyiakeng_Puachue_Hmong.js
A	crates/core/tests/data/test262/generated/Script_-_Ogham.js
A	crates/core/tests/data/test262/generated/Script_-_Ol_Chiki.js
A	crates/core/tests/data/test262/generated/Script_-_Ol_Onal.js
A	crates/core/tests/data/test262/generated/Script_-_Old_Hungarian.js
A	crates/core/tests/data/test262/generated/Script_-_Old_Italic.js
A	crates/core/tests/data/test262/generated/Script_-_Old_North_Arabian.js
A	crates/core/tests/data/test262/generated/Script_-_Old_Permic.js
A	crates/core/tests/data/test262/generated/Script_-_Old_Persian.js
A	crates/core/tests/data/test262/generated/Script_-_Old_Sogdian.js
A	crates/core/tests/data/test262/generated/Script_-_Old_South_Arabian.js
A	crates/core/tests/data/test262/generated/Script_-_Old_Turkic.js
A	crates/core/tests/data/test262/generated/Script_-_Old_Uyghur.js
A	crates/core/tests/data/test262/generated/Script_-_Oriya.js
A	crates/core/tests/data/test262/generated/Script_-_Osage.js
A	crates/core/tests/data/test262/generated/Script_-_Osmanya.js
A	crates/core/tests/data/test262/generated/Script_-_Pahawh_Hmong.js
A	crates/core/tests/data/test262/generated/Script_-_Palmyrene.js
A	crates/core/tests/data/test262/generated/Script_-_Pau_Cin_Hau.js
A	crates/core/tests/data/test262/generated/Script_-_Phags_Pa.js
A	crates/core/tests/data/test262/generated/Script_-_Phoenician.js
A	crates/core/tests/data/test262/generated/Script_-_Psalter_Pahlavi.js
A	crates/core/tests/data/test262/generated/Script_-_Rejang.js
A	crates/core/tests/data/test262/generated/Script_-_Runic.js
A	crates/core/tests/data/test262/generated/Script_-_Samaritan.js
A	crates/core/tests/data/test262/generated/Script_-_Saurashtra.js
A	crates/core/tests/data/test262/generated/Script_-_Sharada.js
A	crates/core/tests/data/test262/generated/Script_-_Shavian.js
A	crates/core/tests/data/test262/generated/Script_-_Siddham.js
A	crates/core/tests/data/test262/generated/Script_-_Sidetic.js
A	crates/core/tests/data/test262/generated/Script_-_SignWriting.js
A	crates/core/tests/data/test262/generated/Script_-_Sinhala.js
A	crates/core/tests/data/test262/generated/Script_-_Sogdian.js
A	crates/core/tests/data/test262/generated/Script_-_Sora_Sompeng.js
A	crates/core/tests/data/test262/generated/Script_-_Soyombo.js
A	crates/core/tests/data/test262/generated/Script_-_Sundanese.js
A	crates/core/tests/data/test262/generated/Script_-_Sunuwar.js
A	crates/core/tests/data/test262/generated/Script_-_Syloti_Nagri.js
A	crates/core/tests/data/test262/generated/Script_-_Syriac.js
A	crates/core/tests/data/test262/generated/Script_-_Tagalog.js
A	crates/core/tests/data/test262/generated/Script_-_Tagbanwa.js
A	crates/core/tests/data/test262/generated/Script_-_Tai_Le.js
A	crates/core/tests/data/test262/generated/Script_-_Tai_Tham.js
A	crates/core/tests/data/test262/generated/Script_-_Tai_Viet.js
A	crates/core/tests/data/test262/generated/Script_-_Tai_Yo.js
A	crates/core/tests/data/test262/generated/Script_-_Takri.js
A	crates/core/tests/data/test262/generated/Script_-_Tamil.js
A	crates/core/tests/data/test262/generated/Script_-_Tangsa.js
A	crates/core/tests/data/test262/generated/Script_-_Tangut.js
A	crates/core/tests/data/test262/generated/Script_-_Telugu.js
A	crates/core/tests/data/test262/generated/Script_-_Thaana.js
A	crates/core/tests/data/test262/generated/Script_-_Thai.js
A	crates/core/tests/data/test262/generated/Script_-_Tibetan.js
A	crates/core/tests/data/test262/generated/Script_-_Tifinagh.js
A	crates/core/tests/data/test262/generated/Script_-_Tirhuta.js
A	crates/core/tests/data/test262/generated/Script_-_Todhri.js
A	crates/core/tests/data/test262/generated/Script_-_Tolong_Siki.js
A	crates/core/tests/data/test262/generated/Script_-_Toto.js
A	crates/core/tests/data/test262/generated/Script_-_Tulu_Tigalari.js
A	crates/core/tests/data/test262/generated/Script_-_Ugaritic.js
A	crates/core/tests/data/test262/generated/Script_-_Unknown.js
A	crates/core/tests/data/test262/generated/Script_-_Vai.js
A	crates/core/tests/data/test262/generated/Script_-_Vithkuqi.js
A	crates/core/tests/data/test262/generated/Script_-_Wancho.js
A	crates/core/tests/data/test262/generated/Script_-_Warang_Citi.js
A	crates/core/tests/data/test262/generated/Script_-_Yezidi.js
A	crates/core/tests/data/test262/generated/Script_-_Yi.js
A	crates/core/tests/data/test262/generated/Script_-_Zanabazar_Square.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Adlam.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Ahom.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Anatolian_Hieroglyphs.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Arabic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Armenian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Avestan.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Balinese.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Bamum.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Bassa_Vah.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Batak.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Bengali.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Beria_Erfe.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Bhaiksuki.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Bopomofo.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Brahmi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Braille.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Buginese.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Buhid.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Canadian_Aboriginal.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Carian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Caucasian_Albanian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Chakma.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Cham.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Cherokee.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Chorasmian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Common.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Coptic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Cuneiform.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Cypriot.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Cypro_Minoan.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Cyrillic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Deseret.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Devanagari.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Dives_Akuru.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Dogra.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Duployan.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Egyptian_Hieroglyphs.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Elbasan.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Elymaic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Ethiopic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Garay.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Georgian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Glagolitic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Gothic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Grantha.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Greek.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Gujarati.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Gunjala_Gondi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Gurmukhi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Gurung_Khema.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Han.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Hangul.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Hanifi_Rohingya.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Hanunoo.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Hatran.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Hebrew.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Hiragana.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Imperial_Aramaic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Inherited.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Inscriptional_Pahlavi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Inscriptional_Parthian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Javanese.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Kaithi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Kannada.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Katakana.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Kawi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Kayah_Li.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Kharoshthi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Khitan_Small_Script.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Khmer.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Khojki.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Khudawadi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Kirat_Rai.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Lao.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Latin.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Lepcha.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Limbu.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Linear_A.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Linear_B.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Lisu.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Lycian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Lydian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Mahajani.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Makasar.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Malayalam.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Mandaic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Manichaean.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Marchen.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Masaram_Gondi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Medefaidrin.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Meetei_Mayek.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Mende_Kikakui.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Meroitic_Cursive.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Meroitic_Hieroglyphs.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Miao.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Modi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Mongolian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Mro.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Multani.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Myanmar.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Nabataean.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Nag_Mundari.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Nandinagari.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_New_Tai_Lue.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Newa.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Nko.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Nushu.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Nyiakeng_Puachue_Hmong.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Ogham.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Ol_Chiki.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Ol_Onal.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Old_Hungarian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Old_Italic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Old_North_Arabian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Old_Permic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Old_Persian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Old_Sogdian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Old_South_Arabian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Old_Turkic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Old_Uyghur.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Oriya.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Osage.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Osmanya.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Pahawh_Hmong.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Palmyrene.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Pau_Cin_Hau.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Phags_Pa.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Phoenician.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Psalter_Pahlavi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Rejang.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Runic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Samaritan.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Saurashtra.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Sharada.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Shavian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Siddham.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Sidetic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_SignWriting.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Sinhala.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Sogdian.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Sora_Sompeng.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Soyombo.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Sundanese.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Sunuwar.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Syloti_Nagri.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Syriac.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tagalog.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tagbanwa.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tai_Le.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tai_Tham.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tai_Viet.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tai_Yo.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Takri.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tamil.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tangsa.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tangut.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Telugu.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Thaana.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Thai.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tibetan.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tifinagh.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tirhuta.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Todhri.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tolong_Siki.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Toto.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Tulu_Tigalari.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Ugaritic.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Unknown.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Vai.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Vithkuqi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Wancho.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Warang_Citi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Yezidi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Yi.js
A	crates/core/tests/data/test262/generated/Script_Extensions_-_Zanabazar_Square.js
A	crates/core/tests/data/test262/generated/Sentence_Terminal.js
A	crates/core/tests/data/test262/generated/Soft_Dotted.js
A	crates/core/tests/data/test262/generated/Terminal_Punctuation.js
A	crates/core/tests/data/test262/generated/Unified_Ideograph.js
A	crates/core/tests/data/test262/generated/Uppercase.js
A	crates/core/tests/data/test262/generated/Variation_Selector.js
A	crates/core/tests/data/test262/generated/White_Space.js
A	crates/core/tests/data/test262/generated/XID_Continue.js
A	crates/core/tests/data/test262/generated/XID_Start.js
A	crates/core/tests/data/test262/manifest.json
A	crates/core/tests/data/unicode/confusables.txt
A	crates/core/tests/edit_ambiguity.rs
A	crates/core/tests/formats.rs
M	crates/core/tests/fuzzy_patch.rs
M	crates/core/tests/intent.rs
A	crates/core/tests/markdown_autolink_text.rs
A	crates/core/tests/markdown_emphasis_floors.rs
M	crates/core/tests/markdown_reference_edits.rs
A	crates/core/tests/markdown_relabel_preservation.rs
M	crates/core/tests/markdown_render.rs
A	crates/core/tests/markdown_renumber_preservation.rs
M	crates/core/tests/markdown_sections.rs
A	crates/core/tests/markdown_setext_content.rs
A	crates/core/tests/markdown_structure.rs
M	crates/core/tests/markdown_table_columns.rs
M	crates/core/tests/markdown_table_edits.rs
A	crates/core/tests/markdown_table_headers.rs
M	crates/core/tests/matching_oracle.rs
A	crates/core/tests/matching_quality.rs
A	crates/core/tests/patch_laws.rs
A	crates/core/tests/pattern.rs
A	crates/core/tests/pattern_generation.rs
A	crates/core/tests/preservation_properties.rs
A	crates/core/tests/provenance.rs
A	crates/core/tests/receipt_wire.rs
M	crates/core/tests/recovery.rs
A	crates/core/tests/recovery_goldens.rs
A	crates/core/tests/recovery_validation_binding.rs
A	crates/core/tests/recursive_schemas.rs
A	crates/core/tests/resource_accounting.rs
M	crates/core/tests/schema_conformance.rs
A	crates/core/tests/schema_evidence.rs
A	crates/core/tests/scratch_map_contract.rs
A	crates/core/tests/scratch_portability.rs
M	crates/core/tests/string_selection.rs
A	crates/core/tests/vocabularies.rs
A	crates/core/tests/wire_evidence_contract.rs
A	crates/core/tests/witness_algebra.rs
A	crates/core/tests/witness_applicability.rs
A	crates/core/tests/witness_formats.rs
A	crates/core/tests/witness_pattern_generation.rs
C050	crates/host/Cargo.toml	crates/helper-census/Cargo.toml
A	crates/helper-census/LICENSE-APACHE
A	crates/helper-census/LICENSE-MIT
A	crates/helper-census/LICENSE-MULAN
A	crates/helper-census/README.md
A	crates/helper-census/UPSTREAM.json
A	crates/helper-census/src/census.rs
A	crates/helper-census/src/hex.rs
A	crates/helper-census/src/json.rs
A	crates/helper-census/src/layout.rs
A	crates/helper-census/src/ledger.rs
A	crates/helper-census/src/main.rs
A	crates/helper-census/src/normalize.rs
A	crates/helper-census/src/rules.rs
A	crates/helper-census/src/source.rs
A	crates/helper-census/src/toml.rs
M	crates/host/Cargo.toml
M	crates/host/README.md
A	crates/host/examples/portable.rs
M	crates/host/src/files.rs
A	crates/host/src/files/windows.rs
M	crates/host/src/helpers.rs
M	crates/host/src/tools.rs
M	crates/host/src/tools/edits.rs
M	crates/host/src/tools/embeddings.rs
A	crates/host/src/tools/files.rs
A	crates/host/src/tools/helper.rs
A	crates/host/src/tools/inspect.rs
M	crates/host/src/tools/markdown.rs
M	crates/host/src/tools/markdown/list.rs
M	crates/host/src/tools/markdown/reference.rs
A	crates/host/src/tools/markdown/structure.rs
M	crates/host/src/tools/markdown/table.rs
A	crates/host/src/tools/registry.rs
A	crates/host/src/tools/validate.rs
A	crates/host/src/tools/witness.rs
A	crates/host/tests/crash_recovery.rs
M	crates/host/tests/edit_tools.rs
M	crates/host/tests/embedding_tools.rs
M	crates/host/tests/files.rs
M	crates/host/tests/helpers.rs
A	crates/host/tests/inspect_tool.rs
A	crates/host/tests/journal_limits.rs
A	crates/host/tests/markdown_record_contract.rs
M	crates/host/tests/markdown_tools.rs
A	crates/host/tests/orphan_preservation.rs
A	crates/host/tests/schema_evidence.rs
M	crates/host/tests/schema_tools.rs
A	crates/host/tests/witness_tool.rs
M	crates/purrassist/README.md
M	crates/syntax/Cargo.toml
M	crates/syntax/README.md
A	crates/syntax/benches/latency.rs
A	crates/syntax/src/alloc.rs
M	crates/syntax/src/lib.rs
A	crates/syntax/tests/allocation.rs
M	crates/syntax/tests/syntax.rs
C054	crates/host/Cargo.toml	crates/wasm-harness/Cargo.toml
A	crates/wasm-harness/LICENSE-APACHE
A	crates/wasm-harness/LICENSE-MIT
A	crates/wasm-harness/LICENSE-MULAN
A	crates/wasm-harness/examples/portable.rs
A	crates/wasm-harness/src/lib.rs
R096	crates/core/examples/portable.rs	crates/wasm-harness/src/transcript.rs
M	docs/CANCELLATION.md
M	docs/DEPENDENCIES.md
M	docs/EDIT-APPLICATION.md
M	docs/FUZZING.md
A	docs/HELPER-CENSUS.md
M	docs/HELPERS.md
M	docs/IMPLEMENTATION.md
A	docs/MATCHING-QUALITY.md
M	docs/QUALIFICATION.md
M	docs/RELEASE.md
M	docs/RESOURCES.md
A	docs/qualification/fuzz-fuzzy-linux-x86_64.json
M	docs/qualification/fuzz-json-linux-x86_64.json
A	docs/qualification/fuzz-markdown-linux-x86_64.json
A	docs/qualification/fuzz-matching-linux-x86_64.json
A	docs/qualification/fuzz-patch-linux-x86_64.json
A	docs/qualification/fuzz-pattern-linux-x86_64.json
A	docs/qualification/fuzz-schema-linux-x86_64.json
A	docs/qualification/fuzz-smoke-fuzzy-linux-x86_64.json
A	docs/qualification/fuzz-smoke-json-linux-x86_64.json
A	docs/qualification/fuzz-smoke-linux-x86_64.json
A	docs/qualification/fuzz-smoke-markdown-linux-x86_64.json
A	docs/qualification/fuzz-smoke-matching-linux-x86_64.json
A	docs/qualification/fuzz-smoke-patch-linux-x86_64.json
A	docs/qualification/fuzz-smoke-pattern-linux-x86_64.json
A	docs/qualification/fuzz-smoke-schema-linux-x86_64.json
A	docs/qualification/fuzz-smoke-unified-linux-x86_64.json
A	docs/qualification/fuzz-stage2-fuzzy-linux-x86_64.json
A	docs/qualification/fuzz-stage2-json-linux-x86_64.json
A	docs/qualification/fuzz-stage2-linux-x86_64.json
A	docs/qualification/fuzz-stage2-markdown-linux-x86_64.json
A	docs/qualification/fuzz-stage2-matching-linux-x86_64.json
A	docs/qualification/fuzz-stage2-patch-linux-x86_64.json
A	docs/qualification/fuzz-stage2-pattern-linux-x86_64.json
A	docs/qualification/fuzz-stage2-schema-linux-x86_64.json
A	docs/qualification/fuzz-stage2-unified-linux-x86_64.json
A	docs/qualification/fuzz-unified-linux-x86_64.json
A	docs/qualification/helper-census-linux-x86_64.json
A	docs/qualification/helper-consolidation-linux-x86_64.json
A	docs/qualification/markdown-html-boundaries-linux-x86_64.json
A	docs/qualification/markdown-link-boundaries-linux-x86_64.json
A	docs/qualification/markdown-table-boundaries-linux-x86_64.json
A	docs/qualification/recovery-bench-linux-x86_64.json
A	docs/qualification/recovery-bench-stage2-linux-x86_64.json
M	docs/qualification/resources-linux-x86_64.json
A	docs/qualification/resources-stage2-linux-x86_64.json
A	docs/schema/receipt.schema.json
A	helpers-ledger.toml
M	scripts/check-packages.py
M	scripts/check-release.py
M	scripts/check-repository.py
A	scripts/check-resource-limits.py
A	scripts/check-shared-helpers.py
M	scripts/check-syntax-oracles.py
M	scripts/check-wasm.py
A	scripts/data/verify-published/call-request.json
A	scripts/data/verify-published/call-response.json
A	scripts/data/verify-published/mcp-requests.jsonl
A	scripts/data/verify-published/mcp-responses.jsonl
A	scripts/data/verify-published/root-files.json
D	scripts/fuzz-json.py
A	scripts/fuzz.py
M	scripts/generate-unicode.py
A	scripts/install-qualification-tool.py
M	scripts/measure-resources.py
M	scripts/package-workspace.py
A	scripts/probe-markdown-html-boundaries.py
A	scripts/run-wasm.mjs
A	scripts/test-fuzz-feedback.py
D	scripts/test-fuzz-json.py
A	scripts/test-fuzz.py
A	scripts/test-package-workspace.py
M	scripts/test-resource-report.py
A	scripts/test-verify-published.py
A	scripts/verify-published.py
M	vendor/dependencies.json
A	vendor/licenses/aho-corasick-1.1.5/LICENSE-MIT
A	vendor/licenses/aho-corasick-1.1.5/UNLICENSE
A	vendor/licenses/cmake/LICENSE
A	vendor/licenses/cmark-gfm-0.29.0.gfm.13/COPYING
A	vendor/licenses/crates-io-auth-action-c6f97d42243bad5fab37ca0427f495c86d5b1a18/LICENSE-APACHE
A	vendor/licenses/crates-io-auth-action-c6f97d42243bad5fab37ca0427f495c86d5b1a18/LICENSE-MIT
A	vendor/licenses/libarchive-tools-3.7.2-2ubuntu0.8/COPYRIGHT
A	vendor/licenses/node-26.10.0/LICENSE
A	vendor/licenses/python/LICENSE
A	vendor/licenses/windows-link-0.2.1/LICENSE-APACHE
A	vendor/licenses/windows-link-0.2.1/LICENSE-MIT
A	vendor/licenses/windows-sys-0.61.2/LICENSE-APACHE
A	vendor/licenses/windows-sys-0.61.2/LICENSE-MIT
M	vendor/profiling-runtime.json
M	vendor/tooling.json
M	vendor/unicode/generated.json
M	vendor/unicode/manifest.json
```


### paudley — 2026-10-12T18:35:44Z

# Plan — Issue #1 (resumed): release-ready 0.1.0 documentation and public repository presentation

Revision 4: this revision folds in every lens review through its final pass. Dispositions are at the end.

## Issue summary

Issue #1 asks that PurRAssist 0.1.0 be finished, qualified and released. The implementation it named (R1–R6,
R10, R12) merged in PR #2 (`699d9ff`, 2026-09-30). The squash message kept the issue open for tagging,
registry publication and downloaded-package acceptance. Main has since gained tool-call recovery (#3, #8)
and `purrassist-derive` (#15), so the release set is now six crates at workspace version `0.1.0`.

Owner decisions in this session (binding):

- Hosted CI is out of scope for this stage.
- Open issues #6, #7, #10 and #11 do not block 0.1.0. They are disclosed as known limitations.
- The remaining scope is: release docs, an updated `README.md`, and other doc updates. Tagging and publishing are not part of this stage.
- Publish the `purrassist-derive` 0.0.0 name reservation. **Done** on 2026-10-12: crates.io returns HTTP 200 and the owner is identical to the other five. The owner configures its trusted publisher.
- The repository is public. Set its GitHub description and related settings.
- Present PurRAssist as **one engine with three equal interfaces: the Rust crates, the MCP server and the CLI**.
- Add a social-preview image.

## Baseline defaults

There is no `.baseline` file.

## Standing goals and constraints

From `.goals` and `AGENTS.md`:

- Rust-first, maximal utility and portability.
- Hard fails, and low or no optionality, including in non-code work.
- `purrassist-core` has no third-party runtime dependencies.
- Every dependency and tool is registered and justified. Each `vendor/tooling.json` entry needs `notices`.
- Every source file has an SPDX header.
- Capability claims need executable evidence.
- Docs distinguish implemented behaviour from plans.
- No user-specific services, credentials or private infrastructure.
- Hooks are never bypassed.
- `make check` is the local gate; packaging needs its own evidence.

## Governing decisions

- `docs/RELEASE.md` is the procedure. It defines six crates in the `RELEASE_ORDER` of `scripts/check-release.py`.
- `publish.yml` is the only workflow that publishes.
- Qualification and publication are bound to the exact commit that is tagged.
- Tagging `rust-v0.1.0` stays a separately authorized, irreversible step.

## Prior art and recurrence (from `prior-art-assessment.md`)

**Earlier documentation passes:**

- Task 19 (`42b4460`, `b08161e`, `856d2ec`) reconciled the docs.
- Its acceptance was a one-time grep that was never wired into `make check`.

**Recurrence:**

- 11 commits explicitly fix stale or overclaimed docs. Unlabelled fixes folded into other commits are UNKNOWN.
- The fuzz-target count went stale twice ("nine"; there are ten).
- The receipt wire version went stale (v2; it is 3).
- #15 missed a six-crate mention until `8d8d9a2`.

**Why earlier attempts did not hold:**

- Prose restates facts that no machine check reads.
- The only machine-checked Markdown is the derive-example copy and `docs/tool-calls/RULES.md`, which is pinned to `rules::RULES` by `crates/core/tests/tool_call_rules.rs`.

**What holds:**

- Generated projections pinned by tests (the RULES.md precedent).
- A fact checker that takes facts from their sources.

## Stale or missing content (verified; line numbers on `f11b45e`)

**`docs/RELEASE.md`:**

- Lines 5–7: "reviewed merge commit" intro.
- Lines 43–62 and 212–218: derive "no placeholder record yet" or "not yet done".
- Line 83: "five of six".
- Lines 100–104: the qualification summary omits steps it runs.
- Line 182: "Task 10" jargon.

**`README.md`:**

- Lines 14–17: "follows review and merge".
- Line 387: "nine" targets. Line 426: JSON-only fuzz wording.
- It has no install section and no crate map.
- It has no three-interface framing.
- About 250 lines of per-tool reference sit in the overview, and `read_file` and `undo_file` are missing.
- It calls the `examples/*.json` requests executable, but no test runs them.

**`docs/IMPLEMENTATION.md`:**

- Lines 5–7: "issue branch" and "follows review and merge".
- Lines 99–108: the delivery sequence does not mark delivered stages.
- The workspace boundaries do not mark the internal crates.

**`docs/QUALIFICATION.md`:**

- Lines 7–8, 224 and 226: "reviewed merge commit" and "after review and merge".
- Line 23: "receipt wire v2".
- Lines 33 and 168: "Nine targets" and "nine mutation targets".
- Line 143: Windows "proved by the hosted lane".
- `make fuzz` is presented as passing, although issues #11 and #6 make it stop.
- Lines 240–251: the Stage 2 section is unlabelled and bound to `66b3756`.

**`docs/FUZZING.md`:**

- Lines 877–881: "10,000 iterations per target", but the two tool-call targets have no baseline.
- Line 967: a `.stage` report pointer.

**Crate READMEs, `CONTRIBUTING.md` and `SECURITY.md`:**

- The core, host, cli and purrassist READMEs say "Pre-release implementation", which would ship in the archives.
- `CONTRIBUTING.md:20` points to a removed section.
- `SECURITY.md` is pre-release wording and has no supported-versions table.

**Crate metadata:**

- All crates share the same non-MCP keywords.
- `purrassist-cli` is categorized as `parsing`.
- There is no `homepage`.
- The `purrassist-cli` documentation link points to docs.rs, but the crate is a binary with `doc = false`.
- The derive README's `latest` docs.rs links resolve to 0.0.0 until publication.

**Missing:**

- There is no CHANGELOG.

**GitHub:**

- The description, homepage and topics are empty.
- Wiki and Projects are enabled but unused.
- There is no social preview.
- Private vulnerability reporting is off.
- GitHub detects the license as Apache-2.0 only.

## Completeness contract

Every row's checks run inside `make check`, except where marked.

| Requirement | Task | Executable acceptance |
|---|---|---|
| Restated facts are sourced, not hand-maintained; stale facts corrected | 1 | `python3 scripts/check-docs.py` exits 0. `python3 scripts/test-check-docs.py` has, for every rule, a fixture copy that violates it and must fail with that rule's named error, plus a fixture where the sentence the rule reads is absent, which must fail ("fact sentence not found"; every fact regex fails on zero matches). Rules: release set and order from `RELEASE_ORDER`; workspace version; fuzz-target count from `scripts/fuzz.py` `TARGETS`; receipt version from `RECEIPT_VERSION`; dialect ids from the `docs/tool-calls/dialects/*.md` stems in every file that lists them; relative links resolve; SPDX on tracked `*.md` (extending the existing scan in `check-repository.py`); no `.stage` references in public docs; a case-insensitive banned-phrase list, matched after collapsing all whitespace runs (including newlines) to one space, with a fixture where a phrase wraps across lines, covering these regexes, applied to the collapsed text: `issue branch`, `nine (mutation \|coverage-guided )?targets`, `wire v2`, `reviewed merge commit`, `follows? (the )?review and merge`, `after review and merge`, `no placeholder record`, `not yet done`, `five of six`, `Pre-release`, `stage report`, `proved by the [^.]*hosted`. **One-time baseline check (Task 1 focused check, recorded in `validation.md`, not in `make check`):** run the rule over the `f11b45e` versions of README, RELEASE, QUALIFICATION, IMPLEMENTATION and FUZZING (via `git show` into a temp directory). It must flag every stale site in the inventory, including the wrapped sites RELEASE 6–7, QUALIFICATION 7–8, 142–143 and 226–227. Scope: all tracked `*.md` outside `.stage/`, `vendor/` and `crates/core/tests/data/`. |
| Dialect ids have one source | 1 | A new Rust assertion in `crates/core/tests/tool_call_dialects.rs` checks that `DialectId::BUILT_IN.map(id)` equals the sorted `docs/tool-calls/dialects/*.md` stems. `cargo test -p purrassist-core --test tool_call_dialects` passes. |
| Honest qualification and limits wording | 1 | `check-docs.py` structure rules, each with a failing fixture: QUALIFICATION and FUZZING contain a `## Known limitations` heading. Its issue set equals the set of issue references in `docs/known-limitations.json` (one source: number, title, one-line impact, `blocks_release: false`). Task 3 extends the same rule to the CHANGELOG when that file is created. The FUZZING baseline table has one row per `TARGETS` entry, and a target without a baseline says `none`. QUALIFICATION's archived-evidence paragraphs name the commit each report is bound to (the `source.commit` of each `docs/qualification/*.json`, read by the checker). The Windows sentence cites the run id it claims. Every row of QUALIFICATION's `make fuzz` results table links to `#known-limitations`; a fixture with the link removed fails. The rows for the targets affected by #6 and #11 state the stop instead of "invariants hold". |
| Tool reference is a generated projection of the registry | 2 | The new test `tools_reference_matches_registry` in `crates/cli/tests/protocol.rs` renders, from `Tools::definitions()` (name, description, input properties with type and required, output top-level properties) in registry order, the block between `<!-- tools-reference:begin -->` and `<!-- tools-reference:end -->` in `docs/TOOLS.md`. It asserts byte equality and prints a diff on failure; `PURRASSIST_BLESS=1` rewrites it and panics if `CI` is set, following the `recovery_goldens` precedent. Each generated entry shows the MCP `tools/call` name, `purrassist call NAME --root DIR < args.json`, and its example file when one exists. Hand-written per-tool narrative (moved from the README) sits outside the block, under `### <tool>` headings. `check-docs.py` counts the hand-written `### <tool>` headings outside the generated block, against the tool names inside it (the block itself is byte-pinned by the Rust test). There must be exactly one heading per name, and no headings for unknown names. |
| Every documented example request actually runs through both CLI and MCP | 2 | The new test `examples_run_through_cli_and_mcp` in `crates/cli/tests/protocol.rs` has an `EXAMPLES` table (file → tool). It asserts that every `examples/*.json` is either a listed tool request or a listed supporting input (`temperature.schema.json` and `temperature.input.txt`, consumed by the temperature request). No file may be an orphan and that each runs through `run_cli` and `run_mcp` in fresh roots with identical, output-schema-valid results. The temperature decode is also checked through `decode_json` in both transports. |
| README presents three equal interfaces | 2 | The README has a `three-interfaces` marker block with Rust, MCP and CLI subsections. The Rust snippet is a byte copy of a doctested block in `crates/purrassist/README.md`. That check is generalized from `check_derive_example` into one marker-copy rule in `check-repository.py` covering both `derive-example` and `three-interfaces`. The MCP client config JSON in the block is parsed by `check-docs.py` (valid JSON, `command` = `purrassist`, `args` = `["mcp","--root",…]`). The README states that MCP creates `.purrassist/` journals in the root. The CLI≡MCP sentence cites `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp`. The convenience commands `decode` and `match` are documented as shortcuts with their own output, and no identity is claimed for them. `purrassist validate` keeps its true claim that it prints exactly the `validate_json` tool output, now backed by a new test, `validate_command_prints_validate_json_output` in `crates/cli/tests/protocol.rs`. |
| No README claim is lost in the move | 2 | Task 2 runs a one-time migration check as a focused check and records it in `validation.md`; there is no `git` in `make check`. It extracts every backticked token and link target from `git show f11b45e:README.md` and asserts that each appears in the new README ∪ `docs/TOOLS.md`, or is listed in a deliberate-removal table in `validation.md`, each entry with its reason (for example a stale count). The Task 2 reviewer must accept every removal. The command and its output are recorded in the Task 2 review. |
| Crate map and READMEs are release-ready | 2 | `check-docs.py` checks that the README crate-map table lists exactly `RELEASE_ORDER` in order. Each packaged README, derived from publishable `cargo metadata` packages rather than a hand list, has no banned phrase and no `docs.rs/.../latest` link. The IMPLEMENTATION workspace-boundaries list equals the workspace members, with the unpublished ones marked internal. |
| MCP Registry ownership token in the published 0.1.0 CLI README (presence only; listing requires a post-publication submission) (crates.io versions are immutable; crates.io strips HTML comments) | 2 | `crates/cli/README.md` has a visible Links bullet: `` MCP Registry name: `mcp-name: io.github.Blackcat-Informatics/purrassist` ``. `check-docs.py` derives the expected name `io.github.<owner>/purrassist` from the owner segment of the workspace `repository` URL, verbatim and case-sensitive. It asserts that the token appears outside any HTML comment and code fence, in the packaged cli README, with failing fixtures for a missing token and a commented-out token. The `server.json` file and registry submission are owner steps after publication, listed in the handoff and in RELEASE.md. |
| Crate metadata is correct | 2 | `homepage` is set to the repository. `purrassist-cli` gets the description "PurRAssist CLI and stdio MCP server", the categories `command-line-utilities` and `development-tools`, the keywords `mcp`, `cli`, `json`, `recovery` and `agents`, and a documentation link to the repository `docs/TOOLS.md`. The facade gets the description "Rust interface to PurRAssist intent recovery for imperfect AI output" and the `mcp` keyword is added to host and cli. `check-docs.py` asserts that the cli documentation link is not docs.rs. `python3 scripts/check-release.py` passes. Archives are audited by `make package` (Task 6). |
| Release docs are current | 3 | The banned phrases are absent. RELEASE.md names the six crates in `RELEASE_ORDER` (checked). The derive reservation is recorded with its date. Trusted-publisher state is a dated observation with derive owner-pending. Qualification is bound to "the exact `main` commit to be tagged". The qualification summary lists every step the workflow runs, checked against the `run:` step names parsed from `.github/workflows/release-qualification.yml`. Not in `make check`: `python3 scripts/check-release.py --preflight-owners --user-agent 'purrassist-release (https://github.com/Blackcat-Informatics/purrassist)'` exits 0 (a read-only crates.io check) and is recorded in `validation.md`. |
| 0.1.0 CHANGELOG | 3 | `CHANGELOG.md` uses Keep a Changelog headings: `## [Unreleased]`, then `## [0.1.0] - unreleased`. `check-docs.py` asserts that the version equals the workspace version, that all six crates and all three interfaces are named, and that the Known limitations set equals `docs/known-limitations.json`. RELEASE.md's release step names this section as the GitHub release body. |
| SECURITY.md | 3 | `check-docs.py` checks for a `Supported versions` table containing the workspace `major.minor`. It also checks that both reporting channels are named: the email, and GitHub private vulnerability reporting. |
| Social preview image | 4 | `docs/social-preview.svg` (SPDX, 1280×640, logo inlined, single font "Noto Sans" with no fallback) and `docs/social-preview.png` are registered in the new `docs/assets.json`: SVG sha256, PNG sha256, render command, `rsvg-convert` version, the font file sha256, and the license. The PNG license is recorded by a REUSE-style `docs/social-preview.png.license` sidecar, and the SPDX scan accepts sidecars for binaries. `rsvg-convert` (`LGPL-2.1-or-later`) and Noto Sans (`OFL-1.1`, recorded by upstream URL, version and file sha256, not a local path) are registered in `vendor/tooling.json` with vendored notices and justified in `docs/DEPENDENCIES.md` as manual asset-rendering tools only. `check-docs.py` checks that the PNG IHDR is 1280×640, that the size is under 1,000,000 bytes, and that both digests equal the registry; there are failing fixtures for each. Not in `make check`: a fresh render under the recorded tool and font must reproduce the PNG sha256. The logo's `feDropShadow` rendering is verified visually. |
| GitHub presentation | 5 | Recorded in `tasks/T5-repo-settings.md` and checked by a reviewer. `gh repo view --json description,homepageUrl,repositoryTopics,hasWikiEnabled,hasProjectsEnabled` shows the set values. `gh api repos/Blackcat-Informatics/purrassist/private-vulnerability-reporting` returns `{"enabled":true}`. The social preview is uploaded through the Settings UI with the Playwright browser tools, verified by the public page's `og:image` changing from the default `opengraph.githubassets.com` card. If the browser upload is impossible, the row is **NOT DELIVERED** and goes to the owner as an explicit decision (upload it themselves, or drop it from this stage); it is not counted as done. |
| Local gate and package | 6 | `make check` and `make package` exit 0 on the final head (single full run). |
| PR and honest handoff | 6 | The PR body says `Refs #1` (the issue stays open for the tag and publication) and lists the remaining owner steps. The plan is posted to the issue and the PR. |

**Out of this stage by owner decision:**

- Hosted CI.
- The derive trusted-publisher configuration.
- The tag, publication, downloaded-package verification and GitHub release.

## Enhancements considered and declined

- **Combined `LICENSE` file for GitHub license detection.**
  - Declined: GitHub's detector can't represent an SPDX `OR` expression. A synthesized file would duplicate three license texts that `check-packages.py` audits by hash, and it would mislead tools that read `LICENSE` as a single license.
  - The README license section and Cargo `license` remain authoritative, and the handoff states the display limitation.
- **`server.json` MCP-registry manifest file.**
  - Declined for this stage: its correctness is proven only by registry submission, which comes after publication.
  - The irreversible prerequisite, the visible `mcp-name:` token in the immutable 0.1.0 `purrassist-cli` README, *is* adopted in Task 2.
  - Writing and submitting `server.json` is listed in RELEASE.md and the handoff as a post-publication step.
- **Test that `purrassist decode` output is identical to `call decode_json`.**
  - Declined: they are deliberately different shapes. `decode` prints the recovery result and has exit codes; the tool returns the registry output schema.
  - The docs stop implying identity, and the README temperature request is exercised through `decode_json` in both transports instead.
- **Generate all prose counts too.**
  - Partly adopted: structural lists (tools, crates, dialects, fuzz targets, known limitations) are generated or sourced.
  - Free prose numbers stay checked facts, because generating narrative text would make it unreadable.

## Task 1: Doc fact checker, known-limitations source, stale-fact corrections

**Files:**

- New, stdlib only, SPDX headers, loaded with the `importlib` precedent of `test-check-release.py`: `scripts/check-docs.py` and `scripts/test-check-docs.py`.
- `Makefile` `hygiene` runs both.
- New: `docs/known-limitations.json` (#6, #7, #10, #11, #12, #13, #14, #16, #17, #18, with titles from the 2026-10-12 observation: all open).
- `crates/core/tests/tool_call_dialects.rs`: the dialect-stem assertion.
- `check-repository.py`: the SPDX scan is extended to `*.md`.
- Corrections in `docs/QUALIFICATION.md`, `docs/IMPLEMENTATION.md`, `docs/FUZZING.md`, `CONTRIBUTING.md`, and the README fact lines at 14–17, 387 and 426. The stale facts are listed in the inventory above.
- **Every banned-phrase site is corrected in this task, so `check-docs.py` is green when the rule lands.** This includes:
  - `docs/RELEASE.md` lines 5–7, 43–62, 83 and 212–218: minimal factual corrections, namely the derive reservation minted on 2026-10-12, a dated trusted-publisher observation, and "the exact `main` commit to be tagged".
  - `SECURITY.md:5`: the pre-release sentence is replaced with a factual status line.
  - The four crate READMEs' "Pre-release implementation" lines: replaced with a one-line 0.1.0 status.

  Tasks 2 and 3 then do the structural rewrites of these files.

**Fixes:**

- **Status lines:** "Merged to `main` (PR #2, `699d9ff`); tool-call recovery (#3, #8) and `purrassist-derive` (#15) merged since; not yet tagged `rust-v0.1.0`."
- **QUALIFICATION:**
  - Every archived-evidence paragraph names its bound commit, and the Stage 2 section is headed "Historical: Stage 2 record bound to `66b3756`".
  - The Windows sentence cites its exact run.
  - A `## Known limitations` section is added, including that the default `make fuzz` stops on #11 and #6 signatures.
- **IMPLEMENTATION:** the delivery sequence (lines 99–108) marks the stages already delivered.
- **FUZZING:** a per-target baseline table (`none` for the tool-call targets), and the `.stage` pointer is removed.

**Focused check:**

- `python3 scripts/check-docs.py && python3 scripts/test-check-docs.py && python3 scripts/check-repository.py`.
- `cargo test -p purrassist-core --test tool_call_dialects`.

Then: task review, commit (explicit files, no `.stage/`), push, issue update.

## Task 2: Three-interface README, generated tool reference, executed examples, crate READMEs and metadata

**README order:**

1. Logo, tagline and value statement.
2. Status, linking `docs/QUALIFICATION.md#known-limitations`. Task 3 adds the CHANGELOG link when the file exists, keeping the relative-link rule green at every commit.
3. **One engine, three interfaces** (marker block):
   - **Rust:** `cargo add purrassist` plus the doctested copy.
   - **MCP:** `cargo install purrassist-cli`, `purrassist mcp --root DIR`, the client config JSON, and the `.purrassist/` note.
   - **CLI:** `purrassist decode …` and `purrassist call TOOL --root DIR`.
   - The CLI≡MCP sentence with its test name.
4. Crate map: the six crates in order, with roles, plus the internal crates.
5. Capabilities, condensed with no claim lost.
6. Use the checkout.
7. Develop and verify.
8. Conformance.
9. License and family. It notes that GitHub shows only one of the three licenses.

**`docs/TOOLS.md`:**

- The generated block comes from `tools_reference_matches_registry`, blessed once.
- The narrative sections are moved from the README, plus new ones for `read_file` and `undo_file`.

**Tests in `crates/cli/tests/protocol.rs`:**

- `tools_reference_matches_registry`.
- `examples_run_through_cli_and_mcp`, including the temperature request via `decode_json`.
- `validate_command_prints_validate_json_output`.

**Crate READMEs:** each of the six gets a 0.1.0 status line and its interface role:

- `purrassist` is the Rust interface.
- `purrassist-cli` is the CLI and MCP server.
- `purrassist-host` serves both.
- core, syntax and derive are the shared engines.

The derive README's `latest` links are pinned to `0.1.0`.

The cli README gets the visible `mcp-name:` Links bullet.

**Other files:**

- Crate metadata as in the contract.
- IMPLEMENTATION workspace boundaries.
- `check-repository.py` marker-copy generalization.
- `check-docs.py` README, crate-map, packaged-README and config rules, each with failing fixtures.
- The one-time migration check, recorded in `validation.md`.

**Focused check:**

- `python3 scripts/check-docs.py && python3 scripts/test-check-docs.py && python3 scripts/check-repository.py && python3 scripts/check-release.py`.
- `cargo test -p purrassist --doc`.
- `cargo test -p purrassist-cli --test protocol`.

Then: task review, commit (explicit files, no `.stage/`), push, issue update.

## Task 3: Release docs, CHANGELOG, SECURITY

**`docs/RELEASE.md`:**

- The intro and release step are bound to "the exact `main` commit to be tagged". The existing qualification sequence is kept.
- A six-reservation record: derive minted on 2026-10-12 with identical ownership.
- Trusted publishing as a dated observation: five confirmed on 2026-09-28, derive owner-pending. Re-confirm through the API immediately before tagging.
- A complete qualification-workflow summary.
- The "Task 10" jargon is removed.
- The release body comes from the CHANGELOG `[0.1.0]` section plus the `verify-published.py` output, and the date is set at tagging.

**`CHANGELOG.md`:** Keep a Changelog format with:

- Highlights by interface.
- The six crates.
- Capability areas with doc links.
- Compatibility: MSRV 1.98 and the derive/core ABI marker rule.
- Known limitations rendered from `docs/known-limitations.json`, checked for equality.

**`SECURITY.md`:**

- Record the before-value (`gh api repos/Blackcat-Informatics/purrassist/private-vulnerability-reporting`) in `tasks/T5-repo-settings.md`.
- Then enable GitHub private vulnerability reporting (`gh api -X PUT repos/Blackcat-Informatics/purrassist/private-vulnerability-reporting`) and verify that it reads back `{"enabled":true}`. Only then write the sentence naming it. If enabling fails, SECURITY.md names the email only and the failure is reported.
- A Supported versions table: 0.1.x is supported once published, and the 0.0.0 reservations contain no code.
- Both reporting channels.

**`check-docs.py`:** the matching rules, with failing fixtures.

**Focused check:**

- `python3 scripts/check-docs.py && python3 scripts/test-check-docs.py && python3 scripts/check-release.py && python3 scripts/test-check-release.py`.
- The read-only preflight-owners run, recorded in `validation.md`.

Then: task review, commit (explicit files, no `.stage/`), push, issue update.

## Task 4: Social preview

**Assets:**

- `docs/social-preview.svg`: 1280×640, 40 px safe margin.
- It inlines the logo artwork and shows the "PurRAssist" wordmark, the tagline, and "Rust crate · MCP server · CLI".
- Palette from the logo, single font "Noto Sans" with no fallback.
- The PNG is rendered with `rsvg-convert -w 1280 -h 640 docs/social-preview.svg -o docs/social-preview.png`.

**Registration:**

- `docs/assets.json`.
- `scripts/check-repository.py`: the SPDX scan accepts a `.license` sidecar for binary files.
- `docs/social-preview.png.license`.
- `vendor/tooling.json` entries for `rsvg-convert` and Noto Sans with notices under `vendor/licenses/`.
- A `docs/DEPENDENCIES.md` justification.
- `check-docs.py` asset rules with failing fixtures.

**Focused check:**

- `python3 scripts/check-docs.py && python3 scripts/test-check-docs.py && python3 scripts/check-repository.py`.
- **Mandatory pass/fail before commit:** re-render to a temp file under the recorded tool and font, and compare its sha256 with the registry. A mismatch (for example a silent font substitution) blocks the commit.
- View the PNG.

Then: task review, commit (explicit files, no `.stage/`), push, issue update.

## Task 5: GitHub repository presentation (outward-facing, owner-requested, no commit)

**`gh repo edit Blackcat-Informatics/purrassist`:**

- **Description:** "Recovers intent from imperfect AI output — JSON, tool calls, names, paths and Markdown edits — with auditable receipts. Rust crate, MCP server and CLI."
- **Homepage:** `https://github.com/Blackcat-Informatics/purrassist#readme`. docs.rs shows the 0.0.0 placeholder until publication, so it is listed in the handoff for change after release.
- **Topics:** `rust`, `mcp`, `mcp-server`, `model-context-protocol`, `cli`, `json-schema`, `json-repair`, `tool-calling`, `llm`, `ai-agents`, `fuzzy-matching`, `markdown`, `wasm`.
- **Unused features:** `--enable-wiki=false --enable-projects=false`.

**Other settings:**

- Private vulnerability reporting: already enabled in Task 3; re-verify it here.
- Social-preview upload via Playwright, as in the contract row.

**Records:** before and after values go in `tasks/T5-repo-settings.md`. An independent reviewer verifies them against live `gh` output and writes `tasks/T5-review.md`. A summary is posted to the issue.

## Task 6: Final gate, completeness review, PR and handoff

1. Run `make check`, then `make package`, once on the final head and record the results in `validation.md`.
2. Get a completeness review against this plan.
3. Do the artifact inventory.
4. Open the PR with the title "docs(release): release-ready 0.1.0 documentation and three-interface presentation". The body says `Refs #1`, lists changes and validation, and lists the remaining owner steps:
   - the derive trusted publisher;
   - the social preview, if it wasn't uploaded;
   - the homepage switch to docs.rs after publication;
   - authorization for `rust-v0.1.0`.
5. Post the plan to the issue and the PR, plus a least-confident note.

## Review dispositions (revision 2)

**Constraints:**

- B1: fixed with neutral scope wording; the plan contains no excluded topic. The rule is kept in an unposted stage note that covers docs, comments, the PR and commits, and no blocklist strings are committed.
- A1: notices for the new tooling entries.
- A2: the dedicated `docs/assets.json` plus the sidecar.
- A3: named tests.
- A4: a single font with no fallback (no text-to-path tool is installed, so the font is pinned instead).
- A5: exact-commit wording.
- A6: explicit-file commits in every task.

**Final review, pass 1:**

- B1: every banned-phrase correction moves into Task 1.
- B2: the README status links QUALIFICATION until the CHANGELOG exists.
- Advisories adopted: the CHANGELOG rule moves to Task 3, a deliberate-removal table for the migration check, the sidecar edit is listed in Task 4, and the vulnerability-reporting before-value is recorded.

**Adversary, pass 2:**

- A3(b): QUALIFICATION fuzz rows link to Known limitations, and affected rows state the stop.
- N1: whitespace-collapsed matching, broadened patterns, and a one-time baseline check over the `f11b45e` files.
- Note on `validate`: the claim is kept, with a new test.
- Note on the migration test: already a one-time check.

**Enhancement, pass 2:**

- Item 2 is narrowed and adopted: the `mcp-name:` token goes in Task 2, verified against the MCP Registry package-types documentation.
- Item 11: resolved by the one-time migration check.

**Completeness, pass 2:**

- N1: whitespace-collapsed matching.
- N2: the delivery-sequence fix moves under IMPLEMENTATION.
- N3: reporting is enabled before the SECURITY sentence.
- N4: resolved by the one-time migration check.

**Design, pass 2:**

- B3: the migration check becomes a one-time Task 2 focused check, with no `git` in `make check`.
- A8 and A9: adopted.

**Constraints, pass 2:**

- N1: SPDX ids.
- N2: the font is recorded by upstream identity, and the re-render comparison is a blocking step before commit.

**Completeness:**

- B1: QUALIFICATION 7–8/224/226 and the IMPLEMENTATION sequence are fixed in Task 1.
- B2: the banned-phrase rule with an explicit file scope.
- B3: every rule has a failing fixture.
- B4: example execution, plus generated per-tool forms.
- Advisories: SECURITY channels, the issue states verified on 2026-10-12, a literal description, the homepage, a Task 5 review, the IMPLEMENTATION boundaries, the RELEASE summary and jargon, the PR owner-step list, and the named digest file are all adopted.

**Design:**

- B1: dialect ids come from the doc stems plus a Rust assertion.
- B2: the PNG digest, tool version and font digest are pinned.
- A1: the generalized marker-copy rule.
- A2: packaged READMEs come from metadata.
- A3: zero-match failure.
- A4: superseded, because the census now uses the registry in Rust.
- A5: the Rust entry-point column is dropped from the generated block; the narrative names the facade module only where it already did.
- A6 and A7: adopted.

**Enhancement:**

- Item 1 (TRANSFORMATION): the generated, pinned TOOLS.md block is adopted.
- Item 2 (`server.json`): declined, see above.
- Items 3 and 6–10: adopted.
- Item 4: declined as an identity claim; docs corrected and the request exercised.
- Item 5: adopted.

**Adversary:**

- A1–A8: adopted as written (A6 as the one-time migration check, A7 as `docs/known-limitations.json`, A8 as the NOT DELIVERED rule).


### paudley — 2026-10-12T18:56:32Z

## Task 1 committed and pushed — `dbdc64d`

**docs(release): source restated documentation facts and disclose known limits**

- New `scripts/check-docs.py` runs in `make hygiene`. It reads the release set and order, workspace version, fuzz-target count, receipt version, dialect ids and known-limitations set from their sources.
- It checks that relative links resolve and that public docs don't reference local stage records.
- It rejects stale release phrases after collapsing whitespace, so phrases wrapped across lines are caught.
- Every rule has a fixture in `scripts/test-check-docs.py` that must fail (87 tests).
- `docs/known-limitations.json` lists #6, #7, #10–#14 and #16–#18, observed open on 2026-10-12, with `blocks_release: false`.
- QUALIFICATION and FUZZING have matching Known limitations sections, and the default `make fuzz` rows state the #6/#11 stops.
- Archived evidence is labeled with the commit each report is bound to.
- The Windows sentence cites Verify run 36702475819 on `dc23bc8`, which ran before PR #2 merged.
- The Stage 2 record is marked historical.
- Status lines say: merged to `main`, not yet tagged `rust-v0.1.0`.
- RELEASE.md records the `purrassist-derive` 0.0.0 reservation (2026-10-12, ownership identical to the other five crates). Derive's trusted publisher is recorded as pending owner configuration.
- The attribution scan now covers Markdown.
- A new Rust test pins `DialectId::BUILT_IN` to the documented dialect pages.

**Validation (focused):**
- `check-docs.py`: OK.
- `test-check-docs.py`: 87 OK.
- `check-repository.py`: OK.
- `test-repository-audit.py`: 39 OK.
- `check-release.py`: OK.
- `cargo test -p purrassist-core --test tool_call_dialects`: 17 passed.

A one-time baseline run over the `f11b45e` docs flagged all 17 stale sites, including the four that wrap across lines. The independent task review passed, including mutation probes of every rule family.


### paudley — 2026-10-12T19:15:41Z

## Task 2 committed and pushed — `871af51`

**docs(readme): one engine, three interfaces, and a registry-pinned tool reference**

- The README presents **one engine, three equal interfaces**: Rust crate (`cargo add purrassist`), stdio MCP server (`purrassist mcp --root DIR` with a client config), and CLI (`purrassist decode` / `purrassist call TOOL`).
  - The Rust snippet is a doctested copy.
  - The CLI≡MCP claim names `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp`.
  - A crate map lists the six crates in publish order.
- New `docs/TOOLS.md`:
  - It holds the per-tool reference moved from the README, plus `read_file`/`undo_file`, which were undocumented.
  - Its reference block is rendered from `Tools::definitions()` and pinned byte-for-byte by `tools_reference_matches_registry`.
- New tests:
  - `examples_run_through_cli_and_mcp` runs every `examples/` request through both transports. No test did this before.
  - `validate_command_prints_validate_json_output`.
- `crates/cli/README.md` has the visible `mcp-name: io.github.Blackcat-Informatics/purrassist` line. crates.io strips HTML comments, so the MCP Registry needs this line visible in the immutable 0.1.0 README.
- Crate metadata:
  - The CLI is described as "PurRAssist CLI and stdio MCP server" and categorized as command-line/development tools.
  - Its documentation link is `docs/TOOLS.md`, not docs.rs, because the binary crate builds no docs.
  - `mcp` is added to the CLI and host keywords.
- Disclosed: `purrassist call` and `purrassist mcp` create `.purrassist/` journals in the root.

**Validation (focused):**
- `check-docs.py`: OK.
- `test-check-docs.py`: 137 OK.
- `check-repository.py` / `test-repository-audit.py`: 42 OK.
- `check-release.py` / `test-check-release.py`: OK.
- `cargo test -p purrassist --doc`: 5 passed.
- `cargo test -p purrassist-cli --test protocol`: 8 passed.
- `cargo fmt --check` and cli clippy `-D warnings`: clean.

The one-time README migration check found no claim or link lost. There are two accepted respellings. The independent review passed, with mutation probes of the pinned block, headings, registry name and orphan examples.


### paudley — 2026-10-12T19:23:03Z

## Task 3 committed and pushed — `f11912e`

**docs(release): 0.1.0 changelog, current release procedure and security policy**

- **`CHANGELOG.md`** (Keep a Changelog) has an unreleased `[0.1.0]` section covering:
  - highlights for the Rust crate, MCP server and CLI;
  - the six crates and capability areas;
  - compatibility: MSRV 1.98 and the derive/core ABI rule;
  - the ten known limitations from `docs/known-limitations.json`.
- **`docs/RELEASE.md`**:
  - Every step is bound to the exact `main` commit to be tagged. The CHANGELOG-date commit comes first, then hosted and local qualification on that commit, trusted-publisher re-confirmation, and the tag.
  - It records the six name reservations and a dated trusted-publisher table, with `purrassist-derive` pending.
  - It lists all 8 `release-qualification.yml` steps by name.
  - A post-publication section covers the MCP Registry `server.json` (cargo, `purrassist-cli`, stdio) and switching the homepage to docs.rs.
- **`SECURITY.md`**:
  - New supported-versions table.
  - Names GitHub private vulnerability reporting alongside the email. Reporting was **enabled on the repository** first: before `{"enabled":false}`, after `{"enabled":true}`.
- New docs rules, each with failing fixtures, cover the CHANGELOG, the SECURITY table and channels, the RELEASE crate list and the qualification steps.

**Validation:**
- `check-docs.py`: OK.
- `test-check-docs.py`: 167 OK.
- `check-repository.py`, `check-release.py`, `test-check-release.py`: OK.
- Read-only publish preflight: `6 crates exist and share owners paudley`, exit 0.

The independent review passed, with mutation probes on the version, crate list, qualification steps and the vulnerability-reporting channel.


### paudley — 2026-10-12T19:31:33Z

## Tasks 4 and 5

### Task 4 committed and pushed — `f0d978a`

**docs(assets): reproducible 1280x640 social preview for the repository**

- **Image:**
  - `docs/social-preview.svg` is the source, with the logo inlined, the wordmark, the tagline and the "Rust crate · MCP server · CLI" strip, in a single font with no fallback.
  - `docs/social-preview.png` is rendered with `rsvg-convert`. It is 112,623 bytes, and a fresh render reproduces it byte for byte.
- **Records:**
  - `docs/assets.json` holds both digests, the render command, the renderer and library versions, and the font's upstream identity and digests.
  - `rsvg-convert` (LGPL-2.1-or-later) and Noto Sans 2.015 (OFL-1.1) are registered in `vendor/tooling.json` as manual asset-rendering tools only. The OFL text is the unmodified upstream `OFL.txt` from notofonts/latin-greek-cyrillic at `71cf6f3`.
- **Checks:** binary files need a REUSE `.license` sidecar. The docs check pins the PNG to 1280×640, under 1 MB, and to its registered digests.
- **Validation:** `check-docs.py` OK; `test-check-docs.py` 178 OK; `check-repository.py` OK; `test-repository-audit.py` 48 OK. Mutation probes (one SVG byte, a fallback font, a missing sidecar) all fail as intended.

### Task 5: repository presentation (live settings, no commit)

- **Description:** "Recovers intent from imperfect AI output — JSON, tool calls, names, paths and Markdown edits — with auditable receipts. Rust crate, MCP server and CLI."
- **Homepage:** the README. It switches to docs.rs after 0.1.0 is published.
- **Topics:** `rust`, `mcp`, `mcp-server`, `model-context-protocol`, `cli`, `json-schema`, `json-repair`, `tool-calling`, `llm`, `ai-agents`, `fuzzy-matching`, `markdown`, `wasm`.
- **Unused features:** Wiki and Projects are disabled; Issues stay on.
- **Private vulnerability reporting:** enabled.
- **Social preview: NOT DELIVERED.** GitHub has no API for it, and the automation browser isn't signed in to GitHub. Owner step: upload `docs/social-preview.png` in Settings → General → Social preview.

The independent review checked the live state and passed.


### paudley — 2026-10-12T19:39:23Z

## Handoff: what I'm least confident about, and what you may not know

**Least confident:**

1. **The `mcp-name` namespace.** `io.github.Blackcat-Informatics/purrassist` matches the registry's case-sensitive GitHub-login check, according to its documentation and to public reports of lowercased names being rejected. It hasn't been tested by a real submission. A submission is only possible after publication, and the line becomes permanent in the immutable `purrassist-cli` 0.1.0 README.
2. **Hand-written narrative in `docs/TOOLS.md`.** The tool narrative was moved verbatim from the old README. Only the new `read_file`/`undo_file`/`decode`/`match`/`validate` text was re-checked against the code. The generated reference block is pinned to the registry, but older narrative sentences are only as accurate as they were on `f11b45e`.
3. **The social-preview PNG.** It reproduces byte for byte only with `rsvg-convert` 2.63.2 and Noto Sans 2.015. Anyone who re-renders with a different toolchain gets different bytes, and `check-docs.py` will flag the mismatch rather than accept it.

**What you may not know:**

- **`.purrassist/` journals:** `purrassist call` and `purrassist mcp` create a `.purrassist/` directory in `--root` even for read-only tools. The README and TOOLS.md now say so.
- **Example requests were never run:** before this PR, no test executed the `examples/*.json` requests the README called executable. They all pass now, through both CLI and MCP.
- **GitHub's license display:** GitHub shows the repository license as Apache-2.0 only, because its detector can't represent `MIT OR Apache-2.0 OR MulanPSL-2.0`. The README notes this, and the Cargo `license` field remains authoritative.
- **Font license:** the distro font package ships an Apache-2.0 LICENSE file, while the Noto Sans font files declare OFL-1.1. The vendored notice is upstream's `OFL.txt`, pinned to a commit.
- **Your remaining steps:**
  - the `purrassist-derive` trusted publisher;
  - uploading the social preview (no API exists for it);
  - authorizing the `rust-v0.1.0` tag;
  - after publication, switching the homepage to docs.rs and submitting `server.json`.


### paudley — 2026-10-12T21:17:49Z

## Stage 2 remediation: units 1–3 pushed (`4933cc4`, `d6fb1e0`, `feefaae`)

The Stage 2 combined review found one required gap: the CHANGELOG `[0.1.0]` section, which RELEASE.md uses as the GitHub release body, said "not yet tagged". It also found 12 advisory gaps. All are fixed, and the reviewer rechecked each fix: `VERDICT (units 1-3): PASS`.

- **`4933cc4`:** the release body no longer carries pre-tag status. Wording fixes:
  - "Release order";
  - the receipt claim scoped to the results that carry receipts;
  - the CLI/MCP comparison described as JSON equality;
  - the social preview described as awaiting upload.
- **`d6fb1e0`:**
  - Published crate READMEs and the CLI `documentation` link pin `blob/rust-v0.1.0`, so each crate page documents its own version. These links resolve once the release is tagged.
  - Internal task, stage and review-pass references in public docs are reworded.
- **`feefaae`:** the checkers are hardened.
  - **Release body:** must not say unreleased, not yet tagged or "published in this order", and the Known limitations are part of it.
  - **Status lines:** the status rule accepts both the unpublished and the published form, in any case, with the exact version, and the docs must agree.
  - **Wording bans:** narrowed so ordinary SemVer "pre-release" wording passes, and extended to internal task numbers and the fixed wordings.
  - **Workflow steps:** parsed by indentation; a `run:` step without a name fails.
  - **License sidecars:** binaries are selected by git's content classification.
  - **Crate links:** pinned links must resolve to an existing path and anchor.
  - **Fuzz stops:** come from `fuzz_stop_targets` in `known-limitations.json`, including the no-stop case.
  - **Encoding:** every hygiene script uses explicit UTF-8 and runs with `EncodingWarning` as an error.

**Validation (focused):**
- `make hygiene` exits 0.
- `test-check-docs.py`: 243 tests OK.
- `test-repository-audit.py`: 52 OK.
- `test-check-release.py`: 18 OK.
- `test-fuzz.py`: 78 OK.
- `cargo test -p purrassist-cli --test protocol`: 8 passed.

The full `make check` and `make package` run once after the last unit.

**Next, G11:** reword the 26 internal task references in the published crates' `src/`, which ship in the 0.1.0 archives and partly render on docs.rs. The generated Unicode data is regenerated through its generator, and a rule is added so they can't return.


### paudley — 2026-10-12T21:45:16Z

## Stage 2 complete for PR #20: ready for Stage 3

**Review path:** lightweight. One combined reviewer covered gaps, wiring, checker robustness, claim accuracy and the completion rubric. The reason is recorded in the stage validation index: the diff is docs, metadata, one asset, comments, and doc-checker scripts with tests, and no production code changed.

**Gaps resolved** (combined review plus remediation-plan review, every fix rechecked):
- **M1 (required):** the CHANGELOG `[0.1.0]` section, which is the GitHub release body, said "not yet tagged". Removed, and now guarded by a release-body rule. The known limitations moved into that section so they ship in the release body.
- **M2:** the status rule now accepts the published form, so the documented post-publication edit no longer breaks `make check`.
- **L1–L12 and N-1–N-7:**
  - publication-sounding and inexact wording fixed;
  - internal task and review-pass references removed from public docs;
  - the pre-release ban narrowed to the stale form;
  - workflow steps parsed by indentation, and an unnamed `run:` step fails;
  - binaries needing a license sidecar selected by content;
  - explicit UTF-8, with `EncodingWarning` as an error in `make hygiene`;
  - crate links pinned to `rust-v0.1.0` and required to resolve;
  - data-driven fuzz stops, including the no-stop case;
  - case-insensitive status and owner matching;
  - Oxford commas accepted;
  - a doc stating both stop forms is rejected.
- **G11:** 26 "Task N", 19 "Requirement N" and one review-tag references are removed from the published crates' Rustdoc and comments, which ship in 0.1.0 and render on docs.rs. The generated IDNA table was regenerated through its generator, with a negative control. A `source-jargon` rule keeps these out.

**Fix commits:** `4933cc4`, `d6fb1e0`, `feefaae`, `93c02b3`.

**Local validation:** the single full run on the final head `93c02b3`:
- `make check`: exit 0, 110 test groups OK.
- `make package`: exit 0, six release archives verified.

The focused checks during remediation:
- `test-check-docs.py`: 251 tests.
- `test-repository-audit.py`: 52.
- `test-check-release.py`: 18.
- `test-fuzz.py`: 78.
- cli `protocol`: 8.
- Clippy `-D warnings`, `cargo doc` and `cargo fmt --check` pass.

**Completion verdict:** PASS, with each criterion demonstrated on the real entry points:
- the checker scripts;
- `make hygiene`;
- the protocol tests;
- a real CLI `call`;
- a real MCP `initialize`/`tools/list`/`tools/call` session;
- regeneration reproducing the committed data.

**Reviewer feedback on the PR:** none (0 reviews, 0 inline threads).

**Kept separate:**
- **Hosted CI:** out of scope by owner decision. Observed but not acted on, and not caused by this branch:
  - `windows-engines` fails a stable Clippy lint in core code this branch doesn't touch;
  - `fuzz` fails on the runner's Node version.
  The documented release procedure still requires a hosted pass on the commit to be tagged.
- **Owner steps:**
  - the `purrassist-derive` trusted publisher;
  - the social-preview upload (no API; the browser was not signed in);
  - authorization for `rust-v0.1.0`.
- **Not run:** the tag, publication, downloaded-package acceptance and the GitHub release.

**Stage 3 readiness:** ready for Stage 3 finalization. Stage 3 should merge-tree against current `main`, run its binding gates and merge via `ghprsq`.


### paudley — 2026-10-12T22:36:12Z

docs(release): release-ready 0.1.0 documentation and three-interface presentation (#1)

Refs #1. Issue #1 stays open: tagging `rust-v0.1.0`, registry publication,
downloaded-package acceptance and the GitHub release are separately
authorized steps that follow this merge.

## What changed

Brings the public and release documentation to an accurate, release-ready 0.1.0 state
for the six-crate release set. PurRAssist is presented as one engine with three equal
interfaces: the Rust crate, the stdio MCP server and the CLI. Earlier documentation
passes went stale because prose restated facts that no check read. This change makes
the restated facts machine-checked against their sources in `make check`.

- **Docs fact checker.** `scripts/check-docs.py` (in `make hygiene`) reads the following from their sources:
  - the release set and order, the workspace version, the fuzz targets, the receipt version and the dialect ids;
  - the known limitations, the tool registry and the crate metadata.

  It also:
  - rejects stale release wording after collapsing whitespace;
  - checks relative and version-pinned links down to the anchor;
  - checks the CHANGELOG release body, the status lines, the qualification-workflow steps and the social-preview asset;
  - keeps internal task and requirement numbers out of public docs and published crate sources.

  Every rule has a fixture that must fail (251 tests). `check-docs.py`, `test-check-docs.py`, `check-repository.py`, `test-repository-audit.py`, `test-check-release.py` and `test-fuzz.py` use explicit UTF-8 and run in `make hygiene` with `EncodingWarning` as an error. `check-release.py` and `fuzz.py` were also made explicit, because the docs checker loads them. The older resource-report scripts are unchanged.
- **README and tool reference.**
  - The README has a three-interface section and a crate map.
  - `docs/TOOLS.md` documents all 31 tools, including the previously undocumented `read_file` and `undo_file`. Its reference block is rendered from `Tools::definitions()` and pinned by `tools_reference_matches_registry`.
  - The new `examples_run_through_cli_and_mcp` runs every `examples/` request through both transports. Before this, no test ran them.
  - The new `validate_command_prints_validate_json_output` backs the `validate` claim.
- **Release records.**
  - `CHANGELOG.md` (Keep a Changelog): its `[0.1.0]` section is the GitHub release body and contains the known limitations.
  - `docs/known-limitations.json` is the single source for the open, non-blocking issues #6, #7, #10–#14 and #16–#18, including which fuzz targets stop.
  - `docs/RELEASE.md` is bound to the exact `main` commit to be tagged, records all six name reservations, dates the trusted-publisher observation, lists every qualification-workflow step, and adds the post-publication steps.
  - `SECURITY.md` has a supported-versions table and names GitHub private vulnerability reporting next to the email channel.
- **Published-crate presentation.**
  - Crate READMEs state each crate's interface role.
  - Their repository links pin `rust-v0.1.0`, so each crate page documents its own version. These links resolve once the release is tagged.
  - The CLI README carries the visible MCP Registry `mcp-name: io.github.Blackcat-Informatics/purrassist` line, which a cargo package needs for registry ownership. crates.io strips HTML comments and versions are immutable.
  - CLI metadata names the CLI and stdio MCP server.
  - Rustdoc and comments in published sources name features instead of internal task or requirement numbers (comments only). The generated IDNA table changed through its generator, and regeneration reproduces the committed bytes.
- **Social preview.**
  - A reproducible 1280×640 `docs/social-preview.png` is rendered from `docs/social-preview.svg`.
  - It is registered in `docs/assets.json`.
  - Its tools are registered with upstream license notices: `rsvg-convert` (LGPL-2.1-or-later) and Noto Sans 2.015 (OFL-1.1, upstream `OFL.txt` pinned to a commit).
  - Binaries need a REUSE `.license` sidecar and are selected by content.

## Issue #1 requirements covered here

- "Docs distinguish implemented vs planned":
  - status lines say "merged to `main`, not yet tagged `rust-v0.1.0`";
  - archived evidence names the commit it is bound to;
  - the Windows statement cites its exact run;
  - known limitations are disclosed, and the default `make fuzz` stops on #11 and #6.
- Owner scope decisions for this work:
  - release docs, README and docs updates;
  - GitHub repository presentation;
  - the three-interface framing;
  - a social preview.
- Outside this change: hosted CI is out of scope by owner decision. Open issues #6, #7, #10 and #11 do not block 0.1.0.

## Design decisions and declined enhancements

**Design decisions:**

- Generated, test-pinned projections (TOOLS.md block, dialect ids) are used where a list exists in code; free prose is checked as facts.
- The README migration was checked once: 248 tokens and 14 links, with 2 deliberate removals, both restated in the new text. That check stays out of `make check`, so shallow clones still pass.

**Declined, with reasons:**

- **Combined `LICENSE` file:** declined. GitHub cannot represent an SPDX `OR` expression, so the README notes the display limitation.
- **`server.json` now:** declined. Only registry submission after publication proves it, so it is a documented post-publication step.
- **`decode` ≡ `decode_json` identity claim:** declined, because the outputs are deliberately different shapes.
- **GitHub homepage on docs.rs:** declined until 0.1.0 documentation exists there.

## Repository and registry actions (outside the diff)

- The `purrassist-derive` 0.0.0 name reservation was published, with ownership identical to the other five.
- The GitHub description names the three interfaces. The homepage and 13 topics are set.
- Wiki and Projects are disabled. Private vulnerability reporting is enabled.
- The social-preview upload was not done: GitHub has no API for it and the automation browser was not signed in. It is an owner step.

## Validation

- **Focused checks** ran per task and per remediation unit. Each task and each Stage 2 unit had an independent review, plus a plan review and completeness review.
- **Final full run on head `93c02b3`:**
  - `make check`: exit 0, 110 test groups OK.
  - `make package`: exit 0, six release archives verified for that source commit.
- **Live entry points** (run on `a42a817`; later commits changed only comments, docs and checkers, and the CLI and MCP entry points are covered at `93c02b3` by `make check`'s protocol tests; the preflight is a commit-independent crates.io ownership read):
  - real CLI `call match_strings`;
  - a real MCP `initialize`/`tools/list`/`tools/call` session (31 tools);
  - the read-only publish preflight (all six crates exist with identical owners).
- **Hosted CI: out of scope for this work (owner decision), and no status check is required by the `main` ruleset.** On `93c02b3`:
  - native Linux/macOS, MSRV, WASM, corpus and qualification passed;
  - `windows-engines` failed a stable Clippy lint (`too_long_first_doc_paragraph`) in core files this change does not touch;
  - `fuzz` failed because the runner's Node version differs from the pinned inventory.

  The release procedure still requires a hosted pass on the commit to be tagged.

## Remaining owner steps

1. Configure the crates.io trusted publisher for `purrassist-derive`.
2. Upload `docs/social-preview.png` as the repository social preview.
3. Authorize the `rust-v0.1.0` tag. The procedure is in `docs/RELEASE.md`, including a hosted pass on the commit to be tagged.
4. After publication: switch the homepage to docs.rs and submit the MCP Registry `server.json`.

## Integration

The remote base `main` (`f11b45e`) is the branch point. `git merge-tree` gives tree `8b2f718`, identical to the head tree, so the squash candidate is exactly the tested tree.

## Reviews and completion

- **Stage 1:** five plan lenses and a final plan review, all PASS after integration.
- **Stage 2 (lightweight path):** one combined review found one required gap, the release body saying "not yet tagged", plus advisories. All were fixed across four commits and rechecked, with a final verdict of PASS.
- **PR feedback:** no reviews, inline comments or threads.
- **Nothing cut:** no requirement was cut or moved to follow-up.

**Records:** the authoritative plan is `plan.md` and the remediation plan is `remediation-plan.md`. Both are in the stage directory `.stage/complete-purrassist-0-1-0-plan-and` of the issue worktree, together with the validation index and every review report.

## Commits squashed (9)

- `93c02b3` docs(core): describe features, not internal task and requirement numbers (#1)
- `feefaae` build(docs): harden the documentation and attribution checks (#1)
- `d6fb1e0` docs: version-pinned crate links and no internal process references (#1)
- `4933cc4` docs(release): keep the 0.1.0 release body free of pre-tag status (#1)
- `a42a817` docs(release): describe the release set without claiming publication (#1)
- `f0d978a` docs(assets): reproducible 1280x640 social preview for the repository (#1)
- `f11912e` docs(release): 0.1.0 changelog, current release procedure and security policy (#1)
- `871af51` docs(readme): one engine, three interfaces, and a registry-pinned tool reference (#1)
- `dbdc64d` docs(release): source restated documentation facts and disclose known limits (#1)

## Files

```
 CHANGELOG.md                                         |  115 +++
 CONTRIBUTING.md                                      |    3 +-
 Makefile                                             |   12 +-
 README.md                                            |  487 +++++------
 SECURITY.md                                          |   25 +-
 crates/cli/Cargo.toml                                |    8 +-
 crates/cli/README.md                                 |   23 +-
 crates/cli/tests/protocol.rs                         |  450 ++++++++++
 crates/core/README.md                                |    8 +-
 crates/core/src/grammar/codec/xml.rs                 |    5 +-
 crates/core/src/pattern/mod.rs                       |    2 +-
 crates/core/src/pattern/unicode.rs                   |    2 +-
 crates/core/src/schema/algebra/mod.rs                |    2 +-
 crates/core/src/schema/compile.rs                    |    6 +-
 crates/core/src/schema/evaluate.rs                   |    8 +-
 crates/core/src/schema/format/mod.rs                 |    6 +-
 crates/core/src/schema/format/regexp.rs              |    4 +-
 crates/core/src/schema/mod.rs                        |    8 +-
 crates/core/src/schema/recovery.rs                   |   10 +-
 crates/core/src/schema/recovery/proposals.rs         |   16 +-
 crates/core/src/schema/vocabulary.rs                 |    2 +-
 crates/core/src/text/idna_data.rs                    |    4 +-
 crates/core/src/text/mod.rs                          |    8 +-
 crates/core/src/text/punycode.rs                     |    2 +-
 crates/core/tests/tool_call_dialects.rs              |   27 +
 crates/derive/README.md                              |    8 +-
 crates/host/Cargo.toml                               |    2 +-
 crates/host/README.md                                |   11 +-
 crates/host/src/tools/markdown/structure.rs          |    2 +-
 crates/host/src/tools/witness.rs                     |    2 +-
 crates/purrassist/Cargo.toml                         |    2 +-
 crates/purrassist/README.md                          |   33 +-
 crates/syntax/README.md                              |    6 +-
 docs/DEPENDENCIES.md                                 |   14 +-
 docs/FUZZING.md                                      |   46 +-
 docs/IMPLEMENTATION.md                               |   20 +-
 docs/MATCHING-QUALITY.md                             |    4 +-
 docs/QUALIFICATION.md                                |   60 +-
 docs/RELEASE.md                                      |  175 ++--
 docs/RESOURCES.md                                    |    4 +-
 docs/TOOLS.md                                        |  876 +++++++++++++++++++
 docs/assets.json                                     |   53 ++
 docs/known-limitations.json                          |   68 ++
 docs/social-preview.png                              |  Bin 0 -> 112623 bytes
 docs/social-preview.png.license                      |    2 +
 docs/social-preview.svg                              |  128 +++
 docs/tool-calls/INTEGRATION.md                       |    8 +-
 docs/tool-calls/dialects/qwen3-coder-xml.v1.md       |    2 +-
 scripts/check-docs.py                                | 1378 +++++++++++++++++++++++++++++
 scripts/check-release.py                             |    6 +-
 scripts/check-repository.py                          |  162 +++-
 scripts/fuzz.py                                      |   66 +-
 scripts/generate-unicode.py                          |   12 +-
 scripts/test-check-docs.py                           | 1799 ++++++++++++++++++++++++++++++++++++++
 scripts/test-check-release.py                        |   18 +-
 scripts/test-fuzz.py                                 |   44 +-
 scripts/test-repository-audit.py                     |  225 ++++-
 vendor/licenses/librsvg-2.63.2/LGPL-2.1-or-later.txt |  176 ++++
 vendor/licenses/noto-sans-2.015/OFL.txt              |   93 ++
 vendor/tooling.json                                  |   22 +
 vendor/unicode/generated.json                        |    2 +-
 61 files changed, 6145 insertions(+), 627 deletions(-)
```

## Conflicts

None. The base has not advanced since the branch point.

---

Defect-Class: documentation-drift


### paudley — 2026-10-12T23:48:21Z

docs(release): record derive's trusted publisher and enforce the publish workflow's shape (#1)

Refs #1. Issue #1 stays open until the tag, publication and release.

## What changed

`docs/RELEASE.md` recorded the `purrassist-derive` crates.io trusted publisher
as pending. The crates.io API now shows it configured on 2026-10-12 for
`Blackcat-Informatics/purrassist` and `publish.yml`, with no environment. The
other five crates name `crates-io`. The owner decided to keep it that way
("continue - I'm not fixing that.").

- The reservation table records it as confirmed, with no environment, by owner decision.
- The "Trusted publishing" section records the observation and the decision. It explains why it is currently safe: only the `publish` job, which runs in `crates-io`, has `id-token: write`. It also explains what would change that: any job that a later edit of `publish.yml` grants `id-token: write`. That permission is to be reviewed whenever `publish.yml` changes.
- Release step 3, and the preflight paragraph that previously said not to tag until derive was "configured", now re-confirm each trusted publisher against its recorded values.
- `scripts/check-release.py` now enforces the condition that makes the decision safe by holding `publish.yml` to a closed shape. It requires:
  - exactly the `gate` and `publish` jobs, with job headers recognized however they are quoted or commented;
  - no YAML anchors or aliases, backslash escapes, `!!`, verbatim or letter-named YAML tags (no tag form can spell a grant, since `write-all` and `id-token` are caught whatever precedes them), or block-scalar permissions;
  - no `write-all` anywhere outside a comment;
  - every `id-token` key (block, inline or quoted) inside the `publish` job.

  Repeated rounds of independent probing showed that matching individual grant spellings kept missing forms, so the check constrains the file's shape instead. `scripts/test-check-release.py` has 29 tests covering each form, including `! write-all` and `!1 write-all`.

## Validation

- The crates.io trusted-publishing API was read on 2026-10-12. All six entries were observed, with the five original crates on `crates-io`.
- `publish.yml` was read: only `publish` has `id-token: write`.
- `check-docs.py`, `check-release.py` and their tests pass with `EncodingWarning` as an error. `make hygiene` exits 0 on the final head.
- `make check` and `make package` were not rerun. No Rust source or crate file changed: the change touches `docs/RELEASE.md` and two hygiene scripts, and `make hygiene` covers both scripts.
- Hosted CI is out of scope by owner decision. The `main` ruleset requires no status checks, and the checks were in progress at merge assessment.

## Reviews

- Review debt: no comments, reviews or threads.
- An independent accuracy review covered the changed text against `publish.yml`, `check-release.py` and the other docs. Its probes drove the closed-shape design, and its final recheck passed.
- Accepted risk, reported to the owner: without an environment on the derive entry, a `rust-v*` tag pushed on an unreviewed commit would run that commit's `publish.yml` before the in-workflow check. Tag protection, such as a `rust-v*` tag ruleset, is the remaining control, and it is outside this change.

## Integration

The base `main` (`3037418`) is the branch point. The merge-tree tree equals the head tree.

Records are in the issue worktree's stage directory `.stage/complete-purrassist-0-1-0-plan-and` (validation index, review-debt and accuracy reports).

## Commits squashed (10)

- `2ce68e5` docs(release): name the YAML tag forms the release check rejects (#1)
- `e34c665` build(release): reject write-all anywhere in publish.yml (#1)
- `edcc413` build(release): reject every YAML tag form in publish.yml (#1)
- `afc180a` build(release): reject escapes, tags and block-scalar permissions in publish.yml (#1)
- `b1c9645` build(release): hold publish.yml to a closed shape (#1)
- `44e5dd1` build(release): catch every form of OIDC grant outside the publish job (#1)
- `f6283b9` build(release): only the publish job may request an OIDC token (#1)
- `e50d49a` docs(release): reflow the trusted-publishing paragraph (#1)
- `a4d8b03` docs(release): keep purrassist-derive's trusted publisher without an environment (#1)
- `63218a6` docs(release): record the purrassist-derive trusted publisher as observed (#1)

## Files

```
 docs/RELEASE.md               | 26 ++++++++++++++------
 scripts/check-release.py      | 32 ++++++++++++++++++++++--
 scripts/test-check-release.py | 83 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 3 files changed, 132 insertions(+), 9 deletions(-)
```

## Conflicts

None.

---

Defect-Class: documentation-drift


### paudley — 2026-10-13T02:21:11Z

ci: fix windows-engines Clippy and the fuzz job; a hosted fuzz gate that completes every target (#1)

Refs #1. Issue #1 stays open until the tag, publication and release.

## What changed

The release procedure requires every hosted `verify.yml` job to pass on the commit to be tagged. Two jobs failed on `main` for reasons outside the code under release. This change fixes both and keeps the accepted 0.1.0 limitations visible.

- **`windows-engines`:** stable Rust 1.98 Clippy, run with `-D warnings`, rejected three over-long first doc paragraphs (`too_long_first_doc_paragraph`):
  - `grammar/cost.rs` `K_PRINT`;
  - `tool_call/emit.rs` `render_tool_result_governed`;
  - `tool_call/ir.rs` `CallSegment`.

  Each now opens with its summary sentence. Only comments change. Local `make check` runs nightly, which is why it never showed up there.
- **`fuzz`:**
  - **Node:** the job never installed Node, so the `pattern` oracle failed the pinned-version check. It now installs the pinned Node like the other jobs.
  - **Known stops:** the default `make fuzz` stops on the accepted limitations: the #6 panic in `tool-call` at execution 600, which meant `tool-call-ref` never ran in CI, and the #11 slow input.
  - **New target:** `make fuzz-hosted`, now run by the CI job and named by the release procedure, runs all ten targets at the smoke count.
    - The two tool-call targets pass `--continue-on-known`, which retains each #6 panic and names it in the log; any other failure still fails the job.
    - `tool-call-ref` gets the 120 s timeout FUZZING.md already prescribed for #11.
  - **Crash output:** on any other crash, `fuzz.py` prints the panic tail and its known-issue classification, which previously stayed in a file the job did not keep. Mocked self-test campaigns no longer print look-alike lines into the log.
  - **Unchanged:** the default `make fuzz`.
- **Release procedure:**
  - `docs/RELEASE.md` step 2 requires every job to pass. It also requires a local `make fuzz-hosted` on the tagged commit, and a replay of each retained #6 input through the base-commit probe (FUZZING.md's attribution procedure).
  - FUZZING.md, QUALIFICATION.md and the README describe the hosted gate. The docs check covers both Makefile fuzz loops.

## Validation

- **Rust 1.98.0 via `rust-validate`:**
  - The `windows-engines` Clippy command failed with exactly the three errors before the change and exits 0 after.
  - The lane's `cargo test` commands with `-D warnings`: 107 test groups OK.
- **Local `fuzz-hosted` campaigns:** `tool-call` passed 1,408 inputs with 4 retained #6 panics; `tool-call-ref` passed 1,101 inputs with 2, past the #11 input at execution 682.
- **Attribution:** an independent reviewer replayed all six through the base `943fa04` probe, and each reproduced its #6 signature.
- **Hosted Verify run 38330704651 on `7e30c67`:** every job passed. The `fuzz` job ran `make fuzz-hosted` in 33m23s and retained the same six inputs at the same executions. Hosted run 38333691731 on the final head `8ce1ca6` passed all eight jobs, with the same six named #6 inputs.
- **Gates:**
  - `make check` exits 0 on `7e30c67` (110 test groups).
  - `make package` exits 0 on `f07b15b`; later commits touch no crate file.
  - `make hygiene` exits 0 on the final head.

## Reviews

- An independent review (gap analysis, several rechecks) drove the switch from a log-reading release exception to `make fuzz-hosted`. Its final verdict is PASS.
- PR feedback: no comments, reviews or threads.

## Integration

The base `main` (`e0a7ecc`) is the branch point. The merge-tree tree equals the head tree.

Records are in the issue worktree's stage directory `.stage/complete-purrassist-0-1-0-plan-and` (the validation index and gap analysis).

## Commits squashed (5)

- `8ce1ca6` test(fuzz): keep mocked campaign findings out of CI logs (#1)
- `cbecc8c` docs(fuzz): describe the hosted gate and keep base attribution for its #6 inputs (#1)
- `7e30c67` ci(fuzz): a hosted fuzz gate that completes every target (#1)
- `f07b15b` ci(verify): install pinned Node for fuzzing; define the release fuzz gate (#1)
- `2864e75` docs(core): short first doc paragraphs for stable Clippy (#1)

## Files

```
 .github/workflows/verify.yml      |  9 +++++++--
 Makefile                          | 23 +++++++++++++++++++++--
 README.md                         |  4 +++-
 crates/core/src/grammar/cost.rs   |  7 ++++---
 crates/core/src/tool_call/emit.rs |  6 ++++--
 crates/core/src/tool_call/ir.rs   |  4 +++-
 docs/FUZZING.md                   | 24 ++++++++++++++++--------
 docs/QUALIFICATION.md             | 12 ++++++++----
 docs/RELEASE.md                   | 13 ++++++++++++-
 scripts/fuzz.py                   | 19 ++++++++++++++++---
 scripts/test-check-docs.py        |  9 +++++++--
 scripts/test-fuzz.py              | 36 +++++++++++++++++++++++++++++++++++-
 12 files changed, 136 insertions(+), 30 deletions(-)
```

## Conflicts

None.

---

Defect-Class: none


### paudley — 2026-10-13T03:09:08Z

## Release readiness on `main` at `76d5e73`

These are the steps in `docs/RELEASE.md` step 2 that can be completed before the CHANGELOG-date commit. That commit is the one actually tagged, so step 2 must be repeated on it.

- **Hosted Verify run 38336502956 on `76d5e73`:** all eight jobs passed (`native` ubuntu/macos, `msrv`, `windows-engines`, `wasm`, `corpus`, `fuzz`, `qualification`). This is the first fully green Verify on `main` since the PR #2 merge. The `fuzz` job ran `make fuzz-hosted` and named six known #6 panics: `tool-call` executions 600, 1002, 1078 and 1129, and `tool-call-ref` executions 905 and 1044.
- **Local `make fuzz-hosted` on the same commit:** exit 0. It retained the same six inputs at the same executions (`tool-call` passed 1,408 inputs, `tool-call-ref` 1,101).
- **Attribution replay:** following FUZZING.md, each retained input was replayed through the `fuzz-tool-call` probe built from base `943fa04` with the campaign's build settings. All 6/6 reproduce the identical #6 panic message, including the same spans:
  - `tool-call` 4c0954c3: `rejected attempt 1..190 runs past where it failed (39)`
  - `tool-call` 8097dc89: `rejected attempt 4..65 runs past where it failed (35)`
  - `tool-call` 833c3935: `rejected attempt 184..394 runs past where it failed (212)`
  - `tool-call` ff4c7266: `rejected attempt 0..394 runs past where it failed (52)`
  - `tool-call-ref` 2bfd4b4d: `rejected attempt 41..101 runs past where it failed (77)`
  - `tool-call-ref` e806e7b8: `rejected attempt 0..124 runs past where it failed (36)`

  None is a regression; all are the accepted #6 limitation. The temporary base worktree was removed afterwards.

**Remaining owner steps:**
1. Upload `docs/social-preview.png` (Settings → General → Social preview). GitHub has no API for this.
2. Authorize `rust-v0.1.0`. The procedure starts with the CHANGELOG-date commit, then repeats step 2 on that commit, re-confirms the trusted publishers and tags.


### paudley — 2026-10-13T04:24:12Z

release: date the 0.1.0 changelog section (#1)

Refs #1. Issue #1 stays open until publication, downloaded-package acceptance and the GitHub release.

## What changed

This is step 1 of `docs/RELEASE.md`, with the release authorized by the owner on 2026-10-12. The `[0.1.0]` CHANGELOG section, which becomes the GitHub release body, now carries the planned tagging date `2026-10-13` (UTC) instead of `unreleased`. The resulting `main` commit is the one to be tagged `rust-v0.1.0`.

The `RECEIPT_VERSION` Rustdoc, which docs.rs publishes with 0.1.0, no longer says nothing has shipped. The release-body rule's comment names both heading statuses.

The docs-check fixtures had assumed the heading reads `unreleased`. They now read the checkout's heading, and accept `unreleased` or an ISO date.

## Validation

- `check-docs.py` passes.
- `test-check-docs.py`: 252 OK.
- `make hygiene` exits 0 on the final head.
- `cargo clippy -p purrassist-core --all-targets -- -D warnings` passes, and `cargo doc` passes with `-D warnings`.

Release step 2 runs on the merge commit before tagging: hosted Verify, the local qualification commands from QUALIFICATION.md, `make fuzz-hosted` and the #6 base-commit replay.

## Integration

The base `main` (`76d5e73`) is the branch point. The merge-tree tree equals the head tree.

## Commits squashed (2)

- `2fb4c3b` docs(release): first-release wording for shipped Rustdoc and the release-body rule (#1)
- `04be6e0` release: date the 0.1.0 changelog section (#1)

## Files

```
 CHANGELOG.md                |  2 +-
 crates/core/src/contract.rs |  6 +++---
 scripts/check-docs.py       |  4 ++--
 scripts/test-check-docs.py  | 17 ++++++++++++-----
 4 files changed, 18 insertions(+), 11 deletions(-)
```

## Conflicts

None.

---

Defect-Class: none


### paudley — 2026-10-13T05:43:06Z

test(core): compare the double-rounding result by bits (#1)

Refs #1. Issue #1 stays open until publication, downloaded-package acceptance and the GitHub release.

## What changed

The `rust-v0.1.0` release-qualification run on `464306c` failed, so `publish.yml`'s gate never ran a publish and nothing reached crates.io. Release qualification runs `make check` on stable Rust 1.98.0. Its Clippy rejects the exact float assertion in `f32_rounds_the_spelling_once_to_nearest` (`clippy::float_cmp` under `-D warnings`, `crates/core/src/tool_call/from_node.rs:336`). Local `make check` uses nightly, and the Windows lane lints only host and cli, so neither caught it.

The test means exact equality: it shows that reading through `f64` lands on `1.0`. It now compares bit patterns with `to_bits()`, as the assertion above it already does. Only the test changes.

## Validation

- `rust-validate 1.98.0 cargo clippy --workspace --all-targets -- -D warnings` found exactly this one error before the change.
- `rust-validate 1.98.0 make check`, the gate release qualification runs, exits 0 after the change (110 test groups).
- Release step 2 runs again on the merge commit, and the unpublished `rust-v0.1.0` tag moves to it.

## Integration

The base `main` (`464306c`) is the branch point. The merge-tree tree equals the head tree.

## Commits squashed (1)

- `fac0d98` test(core): compare the double-rounding result by bits (#1)

## Files

```
 crates/core/src/tool_call/from_node.rs | 4 ++--
 1 file changed, 2 insertions(+), 2 deletions(-)
```

## Conflicts

None.

---

Defect-Class: none


