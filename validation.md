# Validation: issue #15 (purrassist-derive)

## Task 1: core support
- `cargo test -p purrassist-core --lib derive_support` (10), core doctests (395+), and the full `cargo test -p purrassist-core` all pass.
- `check-repository.py`, `make helper-check` and `make hygiene` exit 0. clippy `-D warnings` and fmt are clean.
- MSRV: `rust-validate 1.98.0 cargo check -p purrassist-core --locked` passes.
- FN1 core test: 64 declared names and 64 members, one pass, at most 384 comparisons. A linear scan needs about 2000, so it would fail.
- Review `tasks/T1-review.md`: PASS; advisories A1-A3 fixed.

## Task 2: the purrassist-derive crate
- `cargo test -p purrassist-derive --locked`: 27 unit, 24 integration and 45 doctests, plus 42 compile_fail doctests. The reviewer compiled every compile_fail snippet and twin standalone: each fails for the intended reason (spanned compile_error!, E0277, E0391 or E0080), and each twin compiles.
- `cargo test -p purrassist-core` passes (1253). Core's `is_valid_name` became a `const fn`; the reviewer showed it equivalent over 6,448,258 cases.
- clippy `-D warnings`, fmt, `make helper-check` and `make hygiene` (exit 0, with the release order moved into this task) pass. MSRV: `rust-validate 1.98.0 cargo test --workspace --locked` passes.
- `cargo metadata`: derive has only a dev-dependency on `purrassist-core`; no syn or quote.
- Review `tasks/T2-review.md`: PASS, with recheck PASS.

## Task 3: facade integration
- `cargo test -p purrassist --locked` (unit, integration and doctests) passes. `cargo test -p purrassist-host --test derive_facade --locked` passes; host has no direct core dependency (`cargo tree`), so it proves the `::purrassist::__core` paths. A downstream /tmp crate with `use purrassist::*` plus the type and both derives compiles: no namespace collision.
- clippy `-D warnings` is clean (the unused-dependency warning is gone), and `make hygiene` exits 0. MSRV: `rust-validate 1.98.0 cargo test -p purrassist -p purrassist-host --locked` passes.
- `.github/workflows/verify.yml`: the Windows job runs the derive and facade tests.
- Known until Task 4: `test-verify-published.py` fails (its fixtures lack derive). That script is not part of `make hygiene`.
- Review `tasks/T3-review.md`: PASS.

## Task 4: release, packaging and audit registration, and docs
- check-release, check-repository, test-repository-audit (29), test-package-workspace (8) and test-verify-published (10; the fixture builds and installs a proc-macro crate) pass, as do `make hygiene` and `make helper-check`.
- The derive and facade tests pass. The facade README derive example runs as a doctest (5 doctests).
- Docs: RELEASE.md (six crates, the new order, derive's name reservation and trusted publisher stated as NOT done and required before tagging), IMPLEMENTATION.md, DEPENDENCIES.md, RESOURCES.md, QUALIFICATION.md evidence row, INTEGRATION.md Rust API, and the READMEs. Stale "#4" references are retargeted to #12-#18.
- Review `tasks/T4-review.md`: PASS.

## Task 5: full qualification
- First `make check` on 534f862: FAILED at rustdoc, a redundant explicit intra-doc link in the facade's re-export docs (Task 3). Fixed in d2410ed (one doc line).
- **`make check` on d2410ed: exit 0.**
- `make package` on 534f862: exit 0; 6 release archives verified (cargo package verification plus the source/license/notice audit).
- `make wasm-test` on 534f862: exit 0; native, WASI and browser-WASM transcripts match.
- MSRV: `rust-validate 1.98.0 cargo test --workspace --locked` on 534f862: exit 0.
- Reuse rationale: d2410ed changes only a doc comment in `crates/purrassist/src/lib.rs` (no code, manifest or packaging input that `make package`, `make wasm-test` or MSRV depend on beyond docs), so the 534f862 results apply to d2410ed.
- Not done (external): the crates.io name reservation and trusted publisher for `purrassist-derive`, stated in RELEASE.md as required before tagging. Hosted CI did not run.

## Completeness fixes and final gate
- The completeness review (`reviews/completeness-review.md`) first BLOCKED on F-1 (variant doc comments were ignored, against the user's decision) and F-2 (INTEGRATION.md misstated two impls). Both are fixed in e4850a8: variant descriptions compose into the enum description, and INTEGRATION.md is corrected. The name-error remedy hint was also added. The recheck PASSED, with the composed description verified in a compiled tool's rendered schema from a downstream crate.
- **Final head e4850a8: `make check` exit 0; `make package` exit 0 (6 archives verified).**
- `make wasm-test` and MSRV results (534f862) still apply: e4850a8 and d2410ed change only the derive crate's code, docs and tests and the facade docs, and core is unchanged. `make wasm-test` exercises core, syntax and host transcripts; MSRV coverage of the derive changes comes through `make check`'s build with the same proc_macro APIs, and MSRV was re-run on e4850a8 for the changed crates: `rust-validate 1.98.0 cargo test -p purrassist-derive -p purrassist --locked` exits 0.

# Stage 2

## Review path
**Full path.** The PR adds public API to `purrassist-core` (a shared contract), a new published crate, and release, packaging and audit changes.

## Evidence (raw/)
- PR #19 head e4850a8; base main fccfbe7, no drift. 0 reviews, 0 inline comments, 0 threads; 2 comments, both workflow posts.
- Hosted CI: the 8 Verify jobs did not run. Validation is local.
- `issue-analysis.md` from Stage 1 is reused: the issue has no new comments or requirements.

## Conditional specialists
- **Used:** system-architect (opus/high). The new crate has a first-party parser and writer and a new core trait surface; this review includes an injection check on user strings spliced into generated code.
- **Not used:**
  - security-engineer: the macro runs at compile time on the user's own source, with no network, filesystem, auth or secrets. Splicing user strings into generated code is covered by the structure review.
  - performance-engineer: the generated `FromNode` single-pass property already has a committed revert-detecting test (`Members` comparison bound). The gap analyzer adjudicates whether conversion allocation needs budget charging (#8 class).
  - quality-engineer: edge cases are covered by the gap analyzer and by the completion auditor's downstream-crate execution.

## Stage 2 remediation

### R1: generated-code hygiene (structure F-1, F-2, F-3, F-6 timing)
- Standalone builds (implementer and reviewer): names with `{name}`/`{}`/`{{` give the intended E0080 message; a user `macro_rules! assert` cannot disable the check. `-Zunpretty=expanded` under a module shadowing the primitives, prelude names, Node/Value/Member, assert/format/write and `mod core`/`std`/`alloc`/`purrassist_core` compiles, with no bare names in the output. Placeholder-named fields and variants convert. A non-ASCII derived parameter name is a spanned derive-time error.
- `cargo test -p purrassist-derive -p purrassist --locked` and `--doc` (45) pass, as do clippy, fmt, `make helper-check`, `make hygiene` and MSRV 1.98 for derive.
- Review `tasks/R1-review.md`: PASS.

### R2 / G1 (HIGH): nested structs state their unknown-member policy (user decision: drop with recorded loss)
- A derived `object_spec()` always emits its policy, defaulting to DropWithLoss; top-level `tool_definition()` is unchanged.
- Real compile → hermes recover → from_node tests:
  - cases 1-4 (error: the call is Unresolved UnknownParameter /inner/zz; drop: BestGuess with drop-unknown losses at /inner/zz and /items/0/zz, from_node Ok; pass+extra: Resolved; no key: equals drop);
  - the `Outer2` variants at drop and read_only.
- Fail-before: in a /tmp copy with HEAD's `write.rs`, the no-key case fails (the call came back Resolved, keeping `zz`); the reviewer reproduced this.
- Core, derive and facade tests pass (no other test or golden changed), as do clippy, `make hygiene` and MSRV.
- Review `tasks/R2-review.md`: PASS.

### Stage 2 R3 — `d46b996` refactor(tool-call): harden the derive support surface (F-4, F-5 core, F-6b, A-1 core)
- Focused: `cargo test -q -p purrassist-core -p purrassist-derive -p purrassist --locked` pass (core 143 unit / 401 doctests; derive 49 integration / 46 doctests); clippy -D warnings; fmt; `make hygiene`; `make helper-check`; `check-repository.py`; MSRV 1.98.0 core+derive tests — all pass (implementer), reviewer reran tests/clippy/hygiene/helper-check/check-repository — pass.
- A-1 evidence: standalone crate with `#[param(extra)] rest: String` → exactly one E0277, no E0308.
- F-6b: agreement test is a unit test in `crates/derive/src/model.rs` (validator is crate-private in a proc-macro crate); oracle is core's `Number` parser; hand corpus + exhaustive short spellings over `-01.eE+`.
- Review: `tasks/R3-review.md` VERDICT: PASS (cosmetic: long INTEGRATION.md lines → R6 G9).

### Stage 2 R4 — `9534a4f` feat(derive): crate path override and a versioned core marker (F-5 rest, G5)
- Focused (implementer, reviewer rerun): core/derive/facade tests incl. doctests 47–50 + twins; workspace clippy -D warnings; fmt; `make hygiene`; `make helper-check`; `check-repository.py`; `check-release.py`; `test-package-workspace.py`; `test-verify-published.py`; MSRV 1.98.0 core+derive — pass.
- Manual (reviewer, /tmp, not in-tree): renamed Cargo key `pa = { package = "purrassist-core" }` + `#[tool(crate = ::pa)]` compiles and converts; without the attribute → E0433. Simulated skew (core copy with marker renamed `_0_2`) → single E0425 naming `__DERIVE_ABI_0_1`. In-tree renamed dep is impossible (Cargo rejects one package under two names); re-export/alias/super/self/facade tests cover the path splice.
- Review: `tasks/R4-review.md` VERDICT: PASS.

### Stage 2 R5 — `b7475b4` build(release): publish preflight checks crate names and owners (G3)
- Focused: `scripts/test-check-release.py` 18 OK (validator vs real publish.yml + broken copies; preflight vs fake fetchers: 404, owner mismatch/subset, network error, non-JSON, bad shape, 500, missing core, multi-failure); `check-release.py`; `check-repository.py`; `make hygiene` (now runs the new test); `make helper-check`; actionlint on publish.yml/verify.yml — pass.
- Reviewer manual: `registry_fetch` against a local 127.0.0.1 http.server → 200/404/500 mapped to status, refused connection → OSError reported per crate, user agent present. Orchestrator follow-up: `http.client.HTTPException` (e.g. IncompleteRead) now also maps to OSError; tests and hygiene rerun — pass.
- Live registry path first runs on a tag push (tests are offline by design).
- Review: `tasks/R5-review.md` VERDICT: PASS.

### Stage 2 R6 — `d0a99e9` test(derive): close remaining evidence and doc gaps (G4, G6, G7, G9; A-1 verified)
- Focused: core/derive/facade tests + doctests (README doctest listed as `ReadmeDoctests`); fmt; workspace clippy -D warnings; `check-repository.py`; `test-repository-audit.py` 34 OK (5 new drift negatives); `make hygiene`; `make helper-check`; MSRV 1.98.0 core+derive — pass.
- Reviewer falsification: one-byte change inside the root README markers → flagged; wrong code (E0308) on each of the four newly coded doctests → nightly rustdoc fails "expected error codes were not found"; only doctest 32 emits an incidental code (E0072) among plain compile_fail cases, so it stays uncoded.
- Error-code table: implementer report (58 compile_fail blocks, each built standalone).
- Review: `tasks/R6-review.md` VERDICT: PASS.

## Stage 2 full qualification — head `d0a99e9`
- `make check` exit 0 (log /tmp/pa15-check.log).
- `make package` exit 0 — verified 6 release archives.
- MSRV `rust-validate 1.98.0 cargo test --workspace --locked` exit 0 (110 test binaries, none failed).
- `make wasm-test` exit 0 — native, WASI (core/syntax/host) and browser-WASM (core, executed) transcripts match.
- Hosted CI (`gh pr checks 19`, run 38120841826): all eight Verify jobs did not run (each ended in 2–6 s without executing steps). No hosted evidence exists; all validation is local.

## Stage 2 exit
- Exit completion audit (fresh, opus/xhigh): `completion-audit-exit.md`. First pass BLOCKED (stale PR body; LOW doc gaps) → fixed in PR body + `6cbb628`; recheck 1 BLOCKED (near-miss wording overstated under tolerant policy, escalated to MEDIUM) → fixed in `4d8a48e`; recheck 2 at `4d8a48e`: **VERDICT: PASS**. Optional nits (tolerant rename condition wording; PR body Drop bullet) fixed in `637018b` and the PR body.
- Demonstrations (real entry points, /tmp downstream crates with path deps): nested unknown policies through `recover` (Hermes, Qwen3-Coder XML) + `from_node`, native and WASI identical; hygiene under broad shadowing, both flavors; non-ASCII name errors; Cargo-renamed core + `crate = ::pa`; facade-only crate; simulated skew → E0425 naming `__DERIVE_ABI_0_1`; README example end to end incl. imperfect turn; `check-release.py --preflight-owners` against a 127.0.0.1 fake registry (pass, 404, owner mismatch, refused).
- Evidence reuse: full qualification on `d0a99e9` applies to head `637018b` — the three later commits change only Rustdoc/Markdown prose; derive doctests (55 + 50), rustdoc -D warnings, `check-repository.py`, `make hygiene` rerun on them and pass.
- Hosted CI: did not run (all eight Verify jobs). All validation is local.
- External (maintainer) steps before tagging: `purrassist-derive` crates.io name reservation (`0.0.0` placeholder) and trusted-publisher configuration (docs/RELEASE.md).
- Stage 3 obligation: append the D2 resolution record to `.deficiencies` with the merge SHA (G8).

## Stage 3 integration assessment
- Base `origin/main` = `fccfbe7` = merge-base of head `637018b`; the base has no changes since the branch point.
- `git merge-tree --write-tree --no-messages origin/main HEAD` exit 0; candidate tree `a2a05fd4` equals the head tree (`637018b^{tree}`). The integration candidate is byte-identical to the tested tree.
- Semantic assessment: the base side of the three-way comparison is empty, so there is no second change set whose interaction with shared APIs, consumers, schemas, dependencies, toolchain or build settings could be assessed; a separate semantic-assessment agent would have nothing to compare. Recorded rather than dispatched.
- Evidence reuse: the Stage 2 full qualification (`d0a99e9`: make check, make package, workspace MSRV 1.98.0, make wasm-test) plus the prose-only reruns on `6cbb628`/`4d8a48e`/`637018b` bind the candidate tree, because it equals the head tree and the later commits change only Rustdoc/Markdown prose.
- Completion audit reuse: `completion-audit-exit.md` (Recheck 2 at `4d8a48e`, VERDICT: PASS). Since then only `637018b` (two prose sentences implementing the auditor's own optional nit) and the PR body changed; no source, tests, feedback or criteria changed, so it still applies to head `637018b` and the identical candidate.
- Branch protection on `main`: none (API 404 "Branch not protected"); no formally required checks.
- Merge-plan review (`reviews/merge-plan-review.md`, VERDICT: PASS). F1 (stale `publish.yml` header naming the five-crate order) fixed in `8d8d9a2`, a comment-only change: `check-release.py`, `test-check-release.py` (OK), actionlint, `make hygiene` — pass. Head is now `8d8d9a2`; base still `fccfbe7` (merge-base), so the candidate tree (`66a18b6c`) again equals the head tree. F3: review-debt issue-comment count reconciled (14). F4: squash-notes file list regenerated with `--no-renames`. F6: `raw/pr-19.json` refreshed.
