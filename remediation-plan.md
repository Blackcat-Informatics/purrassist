# Stage 2 remediation plan: PR #19 (issue #15, purrassist-derive)

**Sources:**
- `gap-analysis.md` (G1-G9, GAPS-OPEN);
- `completion-audit.md` (PASS, 4 advisories);
- `reviews/structure-review.md` (BLOCKED: F-1 REQUIRED, F-2 to F-6 advisory);
- `historical-fit.md`.

**Head:** e4850a8. Each R-commit gets a focused check, a task review (`tasks/RN-review.md`), a push, and a post to #15 and PR #19. Every commit runs `make hygiene` and the full `cargo test -p purrassist-derive -p purrassist -p purrassist-core`.

## Gap list

| ID | Sev | Gap | Commit |
|---|---|---|---|
| G1 | HIGH | A nested derived struct with no `unknown` key inherits the tool's PassThrough during recovery. The generated `FromNode` then rejects the extra member, so a `Resolved` call fails conversion (observed with a real hermes turn). | R2 |
| F-1 | HIGH (structure REQUIRED) | The name check emits `assert!(..., "<message>")`. The user's name goes into a format string, so braces break the diagnostic, and the non-absolute `assert!` can be shadowed by a user macro. | R1 |
| F-2 | LOW | Generated code uses bare `bool`/`usize`/`str`; a user type with one of those names breaks the expansion. | R1 |
| F-3 | LOW | Field and variant names are written as text, so a field named like the splice placeholder (`__pa_splice_0`) breaks the expansion. | R1 |
| F-4 | MEDIUM | `Members` checks its sorted name table only in debug builds; a decoder disagreement would misclassify members silently in release builds. The `Members` docs describe an `is_present` use the generated code does not make. | R3 |
| F-5 | MEDIUM | The core trait surface before the first publish: `ToolExtra` is implementable but unusable; `Slot` is exhaustive while generated code matches it without a wildcard; `f32`/`Box<T>` impls are missing and users cannot add them (orphan rule); caret requirements let a newer derive pair with an older core; renamed or re-exported dependencies cannot use the derive. | R3, R4 |
| F-6b | LOW | The derive's JSON-number grammar duplicates core's, with no cross-check. | R3 |
| G3 | MEDIUM | `publish.yml` has no preflight: with derive's name unreserved, core publishes and the release stops halfway. | R5 |
| G5 | LOW | Undocumented: derive and core must be the same version; the facade derive needs the dependency named `purrassist`; only usable or resolved calls should be converted. | R4 |
| G4 | LOW | No committed test for an enum with an explicit `#[tool(description)]` plus variant descriptions. | R6 |
| G6 | LOW | The `crates/derive/README.md` example is not a doctest, and nothing guards the root README copy against drift. | R6 |
| G7 | LOW | Most `compile_fail` doctests carry no error code. | R6 |
| A-1 (audit) | LOW | A wrongly-typed `extra` field also reports a confusing E0308 at the `#[derive]`. | R6 |
| G9 | LOW | Over-wide doc lines in `crates/derive/src/lib.rs`. | R6 |
| G8 | LOW | `.deficiencies` D2 (in the main checkout, outside git) becomes stale once #15 merges. | Stage 3 |
| G2 | MEDIUM | Hosted CI did not run on the PR head. | reported; validation is local |

## Fixes

### R1: `fix(derive): generated code is hygienic for names, primitives and field idents` (F-1, F-2, F-3)
- **Name check:** emit `::core::assert!(…, "{}", "<message>")`, passing the message as an argument rather than as the format string, or double `{`/`}`. Keep messages identical to today's.
- **Primitives:** spell all primitive types as `::core::primitive::{bool, usize, str, …}`.
- **Field and variant idents:** splice them from the original tokens, as the type name already is; never write them as text.
- **Derived parameter names (structure F-6):** a non-ASCII derived parameter name is a spanned compile error at the field, at derive time, as derived tool names already are (`model.rs:527-530`).
- **Tests:**
  - compile_fail doctests: a name containing `{name}` gives the intended "is not valid" message (error code E0080 asserted) with a passing twin; a user `macro_rules! assert` in scope does not disable the check (compile_fail with a twin).
  - Hygiene test module: add `struct bool; struct usize; struct str;` (where legal) and a field named `__pa_splice_0`. Both compile and convert.
- **Check:** `cargo test -p purrassist-derive --locked`, `--doc`.

### R2: `fix(derive): nested objects state their unknown-member policy explicitly` (G1)
- **User decision (verbatim choice, 2026-10-10):** "Drop with recorded loss (Recommended)". A derived nested struct that declares no `unknown` key uses `DropWithLoss`.
- **`object_spec()`** always emits `.unknown_parameters(X)`: X is the declared value, or `UnknownParameters::DropWithLoss` when there is no key. `tool_definition()` is unchanged, so top-level tools keep core's effects-based default (`definition.rs:53-66`). Hand-built `PassThrough` tools embedding a derived `parameter()` are documented on `ObjectSpec`, on `ToolParameter` and in INTEGRATION.md.
- **Default case (no key):** for `Outer { inner: Inner, items: Vec<Inner>, #[param(extra)] rest: Node }` at `unknown = pass`, with `Inner` declaring no `unknown` key and the turn `{"inner":{"a":1,"zz":2},"items":[{"a":1,"zz":2}],"top":3}`:
  - outcome `BestGuess`, with a recorded loss at `/inner/zz` and at `/items/0/zz`;
  - `Outer::from_node` is `Ok` with `rest == {"top":3}`.
  - This fails on e4850a8 (demonstrated).
- **Further tests (derive, real `ToolSet::compile` -> `recover` hermes -> `from_node`), expectations written before running:** for `Outer { inner: Inner, items: Vec<Inner>, #[param(extra)] rest: Node }` at `unknown = pass` with the turn `{"inner":{"a":1,"zz":2},"items":[{"a":1,"zz":2}],"top":3}`:
  1. `Inner` with `#[tool(unknown = error)]`: outcome is not `Resolved`; the unresolved reason names `/inner/zz` (first failure).
  2. `Inner` with `#[tool(unknown = drop)]`: outcome `BestGuess` with a recorded loss at `/inner/zz` and `/items/0/zz`; `Outer::from_node` is `Ok` with `rest == {"top":3}`.
  3. `Inner` with `#[tool(unknown = pass)]` + its own `extra`: outcome `Resolved`; `from_node` is `Ok`, `inner.extra == {"zz":2}`, `items[0].extra == {"zz":2}`, `rest == {"top":3}`.
  Loss assertions name the recorded fields: `CallLedger.losses` entries with `loss.evidence.rule == "tool_call.parameter.drop-unknown"`, `loss.original == "\"zz\":2"`, and evidence destination `/inner/zz` or `/items/0/zz`.
  4. `Inner` with no `unknown` key: the outcome equals case 2 exactly (`BestGuess`, with losses at `/inner/zz` and `/items/0/zz`; `from_node` is `Ok`), per the user's DropWithLoss decision.
  Repeat 4 with `Outer2 { inner: Inner, items: Vec<Inner> }` (no `extra`) at `#[tool(unknown = drop)]`, and again at `#[tool(effects = read_only)]` with no `unknown` key: outcome `BestGuess`; losses with rule `tool_call.parameter.drop-unknown` at `/top`, `/inner/zz` and `/items/0/zz`; `Outer2::from_node` is `Ok(Outer2 { inner: Inner { a: 1 }, items: vec![Inner { a: 1 }] })`. The undeclared keys `top` and `zz` are chosen because neither is a near miss of a declared name; under `Structural` a near miss is `UnknownParameter`, not dropped.
- **Churn to list:** every hand-built nested `ObjectSpec` compared for equality (derive tests, facade tests, facade README `Window`, `derive_support.rs` doc examples) gains the explicit call; `lib.rs:113-117` (D-4 text), `definition.rs:403-406` (`ObjectSpec` doc) and INTEGRATION.md are updated.
- **Docs:** the crate docs correct D-3's "only hand-made nodes" claim and state the nested default.

### R3: `refactor(tool-call): harden the derive support surface before first publish` (F-4, F-5 core part, F-6b)
- **F-4:**
  - Generated code asserts the declared-name table is sorted at compile time (`const _: () = assert!(__sorted(&TABLE))`, a `#[doc(hidden)] const fn` in core).
  - The `Members` docs are corrected to describe actual use.
- **F-5, core part:**
  - `ToolExtra` is sealed (a private supertrait) and documented as `json::Node` only.
  - `Slot` becomes `#[non_exhaustive]`, and generated matches gain a wildcard arm that reports an internal error.
  - `f32`: `parameter()` is `Param::number()`. `FromNode for f32` parses the spelling with `str::parse::<f32>` directly (correctly rounded; not through `f64`, which would round twice), rejects non-finite values, and reports `expected: "number within the finite f32 range"`. Writing is exact; reading is round-to-nearest, and the docs say so.
  - `Box<T>`: add `impl<T: FromNode> FromNode for Box<T>`, plus `ToolParameter`/`ToolItem` for `Box<T: ToolItem>` forwarding `REQUIRED`, `absent` and `DEPTH`. The docs state that `Box` does not enable recursion; a compile_fail doctest for an indirect cycle through `Box` (E0391) has a twin.
  - `ToolParameter` gets default items (`REQUIRED = true`, `absent() = None`, `DEPTH = 0`), and its docs state the relation between `REQUIRED` and `absent()`.
  - Core doctests that match `Slot` gain wildcard arms.
  - The `REQUIRED`/`absent()` relation is stated in the trait docs.
- **F-6b:** a test checks that the derive's number-spelling validator agrees with core's JSON number parser over a corpus of valid and invalid spellings. Derive cannot depend on core, so it lives in derive's tests, which dev-depend on core.
- **Check:** `cargo test -p purrassist-core -p purrassist-derive --locked`, `make helper-check`.

### R4: `feat(derive): crate path override and a versioned core marker` (F-5 rest, G5)
- **Crate path:**
  - `#[tool(crate = path)]` overrides the core path prefix, for renamed or re-exported dependencies, in both flavors.
  - Tests: a downstream rename through the direct flavor (a test crate with a renamed dependency, or a re-export module path), and a compile_fail doctest for a malformed path.
- **Version safety (decided):** generated code references a versioned marker in core, `#[doc(hidden)] pub const __DERIVE_ABI_0_1: () = ();`, whose name derives from derive's `CARGO_PKG_VERSION` major and minor. A derive paired with a mismatched core then fails to compile with an error naming the marker. Workspace and facade requirements stay `^`; see Declined. Tests: (1) a `write.rs` unit test asserts the generated text contains `__DERIVE_ABI_{major}_{minor}` with the numbers taken from `env!("CARGO_PKG_VERSION_MAJOR")`/`env!("CARGO_PKG_VERSION_MINOR")`, not a literal; (2) a core unit test asserts the const's name is built the same way from core's own `CARGO_PKG_VERSION` (both come from the single workspace version, so they agree by construction); (3) the existing derive and facade integration tests, which compile generated code against core, prove the reference resolves. A simulated skew is not testable in-tree and is not claimed.
- **G5 docs:** derive README, crate docs and INTEGRATION.md state the same-version rule, the `purrassist` name requirement or the `crate =` alternative, and "convert only usable/resolved calls".
- **Check:** derive and facade tests, `python3 scripts/check-release.py`, `test-package-workspace.py`, `test-verify-published.py`.

### R5: `build(release): publish preflight checks crate names and owners before publishing` (G3)
- A step **before** `Authenticate with crates.io trusted publishing` runs, for every crate in `RELEASE_ORDER`: `GET /api/v1/crates/{name}` must be 200, and the set of owner logins from `GET /api/v1/crates/{name}/owners` must equal the owner set of `purrassist-core` (no login is hard-coded in the repository; the release set must be owned identically). Both requests send the existing `user_agent`. Any failure exits non-zero before any `cargo publish`. Trusted-publisher state is not verifiable unauthenticated; RELEASE.md keeps it as the manual pre-tag step and the preflight's comment says so. `validate_publish_workflow()` requires the step, the owners URL and that the step precede both the auth step and the publish loop. **A new `scripts/test-check-release.py`** (none exists today; `scripts/test-*.py` has no check-release harness) runs `validate_publish_workflow()` against the real file and against copies with the preflight removed, moved after `cargo publish`, and with the owners check removed, expecting failure for each; it is added to `make check` (Makefile `hygiene` list beside `check-release.py`) and to `verify.yml`'s native job beside `test-verify-published.py`.

### R6: `test(derive): close remaining evidence and doc gaps` (G4, G6, G7, A-1, G9)
- **G4:** add the explicit enum description plus variant descriptions case to `doc_comments_are_descriptions_and_explicit_keys_win`.
- **G6:**
  - Include `crates/derive/README.md` as the crate-level doc, or as a doctest module, so its example runs.
  - A `check-repository.py` drift check: the root README derive example equals the doctested copy byte for byte.
- **G7:**
  - Add error codes to every `compile_fail` doctest whose error has one (E0277, E0391, E0080, E0308).
  - `compile_error!` cases have no code. Their messages are asserted by the pure-layer unit tests (D-12), but **the bridge's span selection and `compile_error!` emission are covered only by the passing twins**: a regression there would surface as a different first error. This residual is disclosed in the PR body under residual risks.
- **A-1:** `ToolExtra` (sealed in R3) gains `#[doc(hidden)] fn __from_members(members: Vec<Member>) -> Self`, implemented for `Node`. Generated code assigns `<{ty} as ToolExtra>::__from_members(__pa_extra)`, so a wrong `extra` type gives only E0277 at the field type. Acceptance: compile_fail doctest 30 is annotated `E0277`, and a standalone build of snippet 30 shows one error, not two.
- **Audit A-2:** the PR body states the whitespace collapse of variant descriptions.
- **G9:** rewrap prose lines over 100 columns (tables excepted).
- **Check:** derive tests and doctests, `check-repository.py`.

## Settle and exit
- After all R-commits: `make check` and `make package` once on the final head, MSRV `rust-validate 1.98.0 cargo test --workspace --locked`, and `make wasm-test` if core changed (it will).
- A fresh completion auditor (opus/xhigh) as the exit gate, re-executing the G1 scenario, F-1 and the R4 crate override on real entry points.
- **G2:** before exit, run `gh pr checks 19` once and record the state. Hosted CI is described only as having run or "did not run".
- **Stage 3:** append a D2 resolution record to `.deficiencies` with the merge SHA (G8).

## Declined (with reasons)
- **`=0.1.0` version pinning (structure F-5).** The versioned ABI marker (R4) already turns a derive/core mismatch into a compile error naming the marker. `=` on internal edges would force lockstep versions across all six crates, or need non-workspace entries in the facade, and it conflicts with `check-release.py`'s caret rule. Its cost is that a mismatched pair is caught at the user's compile, not by Cargo's resolver.
- **Making `Members`/`Slot` `doc(hidden)` (structure F-5).** They stay public and documented: hand-written `FromNode` impls use them as generated code does, and `Slot` is `#[non_exhaustive]` (R3).
- **F-6a `ToolDefinition::arguments(ObjectSpec)` setter.** Declined in the Stage 1 plan review and still declined. Generated code builds the parameter chain in one expansion with no runtime cost, and the setter adds public builder API with only this consumer.
- **A first-party compile-diagnostics UI harness (G7, full form).** Declined. The residual it would have covered (bridge span selection and `compile_error!` emission are proven only by the passing twins) is disclosed in the PR body, per R6.
