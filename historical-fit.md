# Historical fit: PR #19 / issue #15 (purrassist-derive)

Range fccfbe7..e4850a8 (6 commits, 42 files, +7379/-92). Sources: stage evidence files, local git history, code reads. No forge refetch, no builds run.

## 1. Precedent followed or departed from

### 1.1 Crate addition
- No publishable crate was added after release tooling existed. All five were created in `46e9d1a`; later crates (`24787e1` wasm-harness, `a2da56a` helper-census) are `publish = false`. This is the first time every hard-coded crate list is changed together. The prior-art checklist is the only precedent.
- Followed: workspace `members` plus `[workspace.dependencies]` entry (version "0.1.0"); package fields from workspace; three byte-identical LICENSE files, SPDX headers, README; `packaged_notices.derive = {}` (the `a2da56a` key convention); `Cargo.lock` adds one first-party package and one facade edge only.
- Departed (deliberate, justified in evidence): release order puts derive second, not first (check-release walks dev edges, derive dev-depends on core). `check_core_closure` could not be reused (walks dev edges), so `check_derive_closure` was added (normal/build empty; dev reaches only core). Reviewed against `f218e84`/`test-repository-audit.py` fixture style (T4: 29 audit tests).
- Departed: `check-packages.py` now also rejects build-dependencies and target-specific build deps for core as well as derive (hardening beyond the original core rule).
- Departed: `scan_ambient_source` message and `check_ambient_access` are parameterised by crate; derive sources are now held to the core ambient-token rule. A test-side HashMap -> BTreeMap edit in the audit test was accepted by T4 (scanner keeps string-literal contents, since a proc-macro emits code as strings).
- Not followed (external): precedent in `RELEASE.md` is "names reserved, trusted publisher configured" before a release. For derive neither exists. RELEASE.md says so explicitly (heading changed to "five of six", numbered steps, warning that publish.yml would stop after core is already published). Honest, but it is the first release prerequisite the repo cannot satisfy locally. Hosted CI did not run on the PR head.
- `.deficiencies` D2 ("derive not built, needs name reservation + trusted publisher") is in the main checkout only and is append-only. Nothing in the diff records its resolution or the remaining external work there. This is an expected process step, not a code defect.

### 1.2 Core public API additions vs. how API was added before
- Precedent (`fcf2342`, PR #5 typed definitions/FromNode; `#8` commits): new pub items land with Rustdoc plus runnable examples and are listed in `docs/tool-calls/INTEGRATION.md` "Compatibility > Rust API" under "Added". This PR follows it: a single "Added for typed (derived) tool definitions" block listing traits, `max_depth`, `Members`/`Slot`, the five `FromNodeError` constructors, `is_valid_name` const, hidden `__derive_number`/`__valid_name`, the crate and facade re-exports. T4 checked the list against every new core `pub` item (12 integer types verified). After e4850a8 the completeness review corrected two misstatements (F-2).
- Departure: `#[doc(hidden)] pub` items (`__derive_number`, `__valid_name`, facade `__core`) are new to core's surface. They are public and un-gated, with "no compatibility promise" in docs only. There is no earlier hidden-API precedent in `tool_call` (grep shows only `#[non_exhaustive]` on adapter/detect types).
- `Slot` and `Members` are public non-`#[non_exhaustive]` types (enum with 3 variants). Precedent mixed: `adapter.rs` and `detect.rs` use `non_exhaustive`; `Effects`/`RepairPolicy` do not. Adding a variant later is a break pre-0.1.0 only by policy (INTEGRATION says additive pre-0.1.0).
- `FromNodeError::mismatch/within` were private since `fcf2342`; now public with doctests. Existing message text and goldens unchanged (T1: diff touched only 3 core files, no golden changed).
- Trait-based dispatch (`ToolParameter`, `ToolItem` marker, `ToolRepeatable`, `ToolPattern`, `ToolExtra`) is new design; no prior marker-trait pattern in core. `Option<T: ToolItem>` is stricter than plan (`T: ToolParameter`); docs updated (T1 A3).
- `Members` carries `#[cfg(test)]` instrumentation fields on a public struct. There is in-file `cfg(test)` precedent (`ir.rs`, `recognizer.rs`), but not as fields of a pub struct.

### 1.3 First-party parsing precedent
- Doctrine: `.goals` SUBSUME/RUST-FIRST; DEPENDENCIES.md zero-dep core; precedents `helper-census/src/toml.rs` (strict subset reader that refuses rather than guesses), `grammar/syntax.rs` (Token/Lexer/Parser), `check-release.py` YAML-free validation. Derive follows this: own token model (`model.rs`), parser (`parse.rs`), writer (`write.rs`), single `proc_macro` touchpoint (`bridge.rs`); no syn/quote/proc-macro2 (cargo metadata: only a dev dep on core). Every non-subset input becomes a spanned `compile_error!`.
- Departures from the usual pure-Rust discipline: 3 `unreachable!` in `parse.rs` (T2 A-3) contradict `bridge.rs`'s doc "Nothing here panics on any input". `__derive_number` in core panics on generator defect (documented `# Panics`, `should_panic` doctest).
- Default values: Rust literal strings are spliced verbatim (no unescaping/JSON quoting in derive); number spelling checked textually. Avoids duplicating a core helper (helper-census concern).

### 1.4 Release tooling changes vs prior
- Prior: `de8db9d`/`7882703` hard-coded five names; `a68beeb..868d196` isolated packaging. This PR touches: `check-release.py` RELEASE_ORDER (1 line), `publish.yml` loop and step name (check-release enforces loop == RELEASE_ORDER), `package-workspace.py` comment, `test-package-workspace.py` NAMES, `test-verify-published.py` (CRATE_NAMES, LIB_NAMES, chain, `dependency_of` now multi-parent, a real `proc-macro = true` fixture exercised through a local registry), `verify-published.py` docstring, `check-packages.py`, `vendor/dependencies.json`.
- Followed the `3a19a2d`/`7136da7`/`c6ce650` pattern of refreshing replay/fixture scripts when shapes change (fixtures were red between T3 and T4 and fixed in 534f862).

### 1.5 Test conventions
- First `compile_fail` doctests in the repo (`git grep compile_fail fccfbe7` empty): 42 in derive, plus core trait cases. Each has a passing "twin" (new convention); T2 compiled all snippets standalone and confirmed intended failure reasons (spanned errors, E0277, E0391, E0080; snippets 30/32 show an extra secondary error).
- `should_panic` doctest for `__derive_number`; debug-assert panic tests for `Members::new`.
- Performance oracle: FN1 count-based test (64 names, comparisons <= 384; linear scan ~2000) in the style of #8 "failing-on-old-code" tests (count, not time). Equivalence oracle for `is_valid_name`: 6,448,258 cases (T2 reviewer; not a committed test).
- Derived == hand-built `assert_eq!` (builders derive PartialEq); end-to-end recovery via hermes and qwen3-coder-xml in `crates/derive/tests/derive.rs` (1370 lines), facade and host tests; host has no core dependency, so `crates/host/tests/derive_facade.rs` proves `::purrassist::__core`.
- Helper census: new crate is picked up automatically (strict `helper-check` in `make check`, passed). The committed `docs/qualification/helper-census-linux-x86_64.json` has no derive entry (lists only core, host, syntax) and was not regenerated; cli/purrassist are also absent, so this staleness predates the PR.
- Windows job in verify.yml gains `cargo test -p purrassist-derive -p purrassist --locked`. MSRV job runs `--workspace`, so derive is covered there.

## 2. Doctrine check
- AGENTS.md: core stays free of third-party deps and ambient access (additions pure; check-repository passes). Public APIs have Rustdoc with examples and error semantics (checked T1; hidden items documented too). "Capability claims require executable evidence": README derive example is a doctest (crates/purrassist/README.md, 5 facade doctests), root README example is not (T4 note, disposition added the facade copy). Dependencies registered: none new. License/provenance: three LICENSE files. Hooks never bypassed; commits ordinary. `make check` exit 0 and `make package` exit 0 (6 archives) at e4850a8 per validation.md; wasm-test and MSRV from 534f862 reused with a stated rationale (core unchanged since 0fb0454; derive MSRV re-run on e4850a8).
- Departure from "verification not reused" wording: `make check`/`make package` are at head; `make wasm-test` is reused from an earlier commit. Reasoned but is reuse.
- .goals: RUST-FIRST/SUBSUME honoured. "MAXIMAL PORTABILITY": proc-macro crate is host-only; core additions are wasm-clean (wasm-test passed at 534f862).
- IMPLEMENTATION.md, DEPENDENCIES.md, RESOURCES.md ("six"), QUALIFICATION.md (evidence row, "Six" archives, "all six"), tool-calls/README.md, dialect doc (#4 -> #17) updated. The "Still tracked in #4" lines were retargeted to #12-#18; those issue numbers are asserted by the plan and not re-verifiable here.
- Doctrine departure to flag: user decision "doc comments on structs, fields, enums and variants" was initially unmet (completeness F-1), fixed in e4850a8 by composing variant descriptions into the enum description (the schema has no per-value description). Behaviour differs from the literal wording; documented in lib.rs lines ~140-147.
- DEPENDENCIES.md claims derive's closure is audited and `crates/derive/src` scanned for ambient access; both implemented in check-repository.py.

## 3. Recurring failure patterns from history
- `performance` defect class (`GhpRsq-Defect-Class` x1, #8): quadratic or uncharged work (`3a19a2d`, `3111582`, `3743aed`, `bdb102a`, `c2f1916`). The branch addresses it by `Members` (O(n log k), no `Node::get` per field). Residual: `Members::new` allocates `vec![false; k]` and (debug) a permutation vec per object; `from_node` has no budget. Consistent with existing `FromNode` for `Vec<T>` (ungoverned), so not a regression, but "uncharged work" is the #8 pattern name. Deep/wide input is bounded only by the parser's earlier governance.
- #8 "no seeded hashing in core" (`342f2d4`): derive uses sorted slices and `BTreeMap` only; sorted name table emitted at expansion, deterministic (T2 expansion review, a test for pure-function expansion). `HashMap` appears only in a rejected-input doctest string.
- Stale fixtures (`7136da7`, `c6ce650`, `test-verify-published` red between T3 and T4): recurred transiently here; fixed in 534f862. Still the highest chance of recurrence at the next crate or flag addition.
- Doc claims without executable evidence (#8: strict trailer claim pinned in `8bca69e`; INTEGRATION compat statement `2484700`): here F-2 (INTEGRATION misstated two impls) and F-1 occurred and were fixed; root README example is not a doctest; "Nothing here panics" claim contradicts three `unreachable!`.
- Gate reuse: previous branches claim evidence for earlier heads. Here `make wasm-test` and MSRV-workspace are from 534f862.

## 4. Consumers of changed contracts and whether updated

| Contract / consumer | Updated? |
|---|---|
| `FromNodeError::{mismatch, within}` now pub; `missing`/`duplicate`/`unknown` new. Callers in `from_node.rs` Vec/Option/number impls | yes; messages unchanged; no golden changed |
| `is_valid_name` -> `const fn`. In-core callers: `toolset.rs:659` (tool/alias names), `toolset.rs:1378` (parameter literal names); re-export `mod.rs:108`; doc in INTEGRATION | yes; semantic equivalence argued by T2 (6.4M cases) but no committed equivalence test beyond doctest |
| New core traits and impls (`ToolParameter` etc.) | documented; no existing consumer; coherence risk only for downstream impls |
| `Members`/`Slot` | documented; used only by generated code and derive_support tests |
| Facade `purrassist`: `__core`, macro re-exports `ToolDefinition`/`ToolEnum` beside glob `pub use purrassist_core::*` | yes; macro namespace distinct from type; downstream `use purrassist::*` compile checked (T3); facade tests and README doctest |
| `purrassist-host`, `purrassist-cli`: no direct core dep; host test covers `::purrassist::__core`; CLI untouched | yes |
| `Cargo.toml`, `Cargo.lock` | yes |
| `check-release.py` RELEASE_ORDER and publish.yml loop | yes, equal |
| `release-qualification.yml` | no edit; refers to "all crates" generically (grep shows only a comment) |
| `verify.yml` | Windows job only; main jobs use workspace |
| `package-workspace.py`, `check-packages.py`, `verify-published.py`, `test-*` | yes |
| `check-wasm.py`, `measure-resources.py` (WASI_PACKAGES hard-code core/syntax/host) | not changed; derive is a host-only proc-macro; host builds it natively. Unverified beyond wasm-test pass |
| `vendor/dependencies.json` packaged_notices | yes (`derive: {}`) |
| `scripts/data/dependency-baseline.json` | unchanged (no third-party change) |
| `helpers-ledger.toml`, census JSON report | ledger unchanged; report not regenerated |
| Docs: RELEASE, IMPLEMENTATION, DEPENDENCIES, RESOURCES, QUALIFICATION, README(s), tool-calls/README, INTEGRATION, dialect doc | yes |
| `.deficiencies` D2 (main checkout, untracked) | not updated |
| crates.io name/trusted publisher | not done; documented as required before tagging |

## 5. Blast radius
- `is_valid_name` is the single name predicate for all tool, parameter and alias validation at `ToolSet::compile` and so for CLI/MCP/host callers. The rewrite is a loop with the same set; the empty check moved to the end. Result is identical for all inputs by inspection (non-ASCII bytes fail `is_ascii_alphanumeric`; empty returns false). Low risk, but its only protection is the doctest plus the T2 one-off equivalence run.
- Core changes are additive; `FromNodeError` visibility change is the only edit to existing items.
- Facade gains a build-time proc-macro dependency for every `purrassist` consumer (including host/cli and wasm targets that build the facade): a new compile-time edge, proc-macro host compilation, and cold-build cost (RESOURCES.md cold-compile ceiling text changed only five->six, not re-measured). `make resources` was not run.
- Release set expands from five to six; every release step (package, publish loop, verify-published) now depends on derive. A failed derive publish halts after core is public (documented).
- Generated code reaches core via `::purrassist_core` (direct flavor) or `::purrassist::__core` (facade flavor): version skew between a user's direct derive and core pins is not enforced by Cargo (no dependency from derive to core); lockstep exists only in the workspace.

## Risks for gap analysis
1. External release blockers unproven: crates.io name reservation and trusted publisher for `purrassist-derive` not done; hosted CI (8 Verify jobs) never ran on e4850a8; publish would half-release. Confirm docs say so everywhere (RELEASE.md, QUALIFICATION release sequence, README) and no "ready to tag" wording remains.
2. Derive/core version skew in the direct flavor: derive has no Cargo edge to core, but emits calls to hidden `__derive_number`/`__valid_name` and trait items. A mismatched pair yields an opaque error. Check if docs state the lockstep requirement and whether the hidden items' `doc(hidden)` status is promised stable enough (semver/0.x policy).
3. `is_valid_name` const rewrite: confirm a committed test (not just a one-off T2 run) covers empty, non-ASCII, each allowed punctuation and boundary bytes, and that both `toolset.rs` callers are still covered by existing negative tests.
4. #8 performance class: `Members::new` allocation per object (`seen`, debug permutation vec) and generated `from_node` recursion on untrusted trees are ungoverned; verify the FN1 oracle fails on a linear scan in release (counts are `#[cfg(test)]`) and that nested/wide derived structs stay O(n log k) and that derive expansion time and generated size are bounded for large enums or many fields, deep default literals, long alias lists (no quadratic parse in `parse.rs`, `accepted.contains` loops, `write.rs` sort).
5. Recursion bound: `DEPTH` const forces E0391 for indirect recursion, self-reference is scanned textually and skips `::`-qualified paths; confirm hand-written or `Box`-wrapped cycles, nesting deeper than ToolSet's depth Resource limit (toolset.rs ~1371-1545) produce a clear error rather than a late `compile` failure, and that DEPTH semantic matches the governor's depth unit.
6. Doc claim vs code: bridge.rs "Nothing here panics on any input" vs three `unreachable!` (T2 A-3, not fixed); `__derive_number` panics in core. Check whether this was resolved; AGENTS wants explicit outcomes rather than swallowed errors.
7. Claim of "derive checks names at compile time": explicit `name = ".."` on a tool is only validated at `ToolSet::compile` (T2 A-4); `__valid_name` const asserts are said to cover explicit tool, parameter and alias names in the docs (INTEGRATION hidden-item text). Verify which is true after e4850a8 (the name-error remedy hint was added in that commit).
8. Variant doc comments now compose into the enum description (e4850a8, user-decision deviation): need a test that exact composed text, ordering, trim/newline rules and interaction with an explicit enum description are asserted, and that derived-vs-hand-built equivalence includes this. e4850a8 was reviewed by the completeness recheck only (no per-task review), and compile_fail twins were not re-run after it.
9. compile_fail doctests are first in the repo: brittle to rustc wording changes (only intended-reason verification at T2); `make check` runs them on the pinned toolchain only. Check that snippets 30/32 secondary errors do not hide wrong-reason passes and that the committed tests include the twin for every rejection.
10. Evidence reuse: wasm-test and workspace MSRV are from 534f862; core unchanged but facade and derive changed afterwards. Re-run is cheap relative to risk.
11. Public surface hygiene: `Slot` and `Members` are not `non_exhaustive`; `Members` has `cfg(test)` fields in a public struct; `FacadeToolDefinition`/`FacadeToolEnum` are public macros of the derive crate (documented?). They should be recorded as semi-public in INTEGRATION (they are not named there) with a stability stance.
12. Process records: `.deficiencies` D2 (and any note that derive's `extra`/`default` semantics and variant-description deviation were user decisions) not updated; helper-census qualification JSON lacks derive; root README example not a doctest (T4 note); stale-fixture risk in `test-verify-published.py` multi-parent chain for the next crate change.
13. Scope claims in docs: issue numbers #12-#18 used for "still tracked" lists are asserted by the plan, not verified in this assessment; `INTEGRATION.md` says the additions "break nothing", but making `FromNodeError::mismatch/within` public and adding blanket-ish trait impls is a semver-visible surface expansion for downstream coherence.
14. Facade macro/type name overlap: `purrassist::ToolDefinition` (derive macro) vs `purrassist::tool_call::ToolDefinition` (type) relies on separate namespaces; a future `pub use tool_call::ToolDefinition` at the facade root (or a glob import in user code of both) would collide or shadow. Check docs warn; T3 checked only `use purrassist::*`.
