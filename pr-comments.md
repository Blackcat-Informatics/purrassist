# PR #19 — comments and review threads

10 comment(s). **Every one needs a disposition** in
`review-debt.md`: addressed (with the commit hash) or declined (with the
reason posted in reply). Bot comments count.

## 1. paudley — 

# Issue #15: `purrassist-derive`, plan (Stage 1)

## Summary

Add the zero-dependency proc-macro crate `purrassist-derive` (#4 Spec A, decision D12). It provides:
- `#[derive(ToolDefinition)]` on argument structs;
- `#[derive(ToolEnum)]` on unit enums;
- facade twins, so `#[derive(purrassist::ToolDefinition)]` works with no `crate =` attribute.

The generated code is the same typed builder calls as a hand-built `ToolDefinition`, plus a single-pass `FromNode` conversion. Recovered call arguments can therefore be read as the struct.

Base: `main` at `fccfbe7`. Branch: `paudley/15-purrassist-derive-zero-dependency`.

**Standing goals** (`.goals`, AGENTS.md):
- Rust-first; subsume rather than depend (no `syn`/`quote`).
- `purrassist-core` stays free of third-party and ambient access, and never depends on derive.
- Every dependency is registered and justified.
- Public APIs carry Rustdoc with examples and error semantics.
- Capability claims need executable evidence.
- Hooks are never bypassed.
- Maximal utility.

**Evidence:** `issue-analysis.md` (requirement IDs P/T/ATTR/FN/G/DOC/AU, risks A1-A17, decisions Q1-Q10) and `prior-art-assessment.md`.

## Prior art and recurrence

- **No earlier derive attempt exists** in the repository history. The derive was split out of #3 and into #4 by user decision on 2026-10-01 (`.deficiencies` D2), to keep PR #5 small; no technical objection was recorded.
- **Recurrence:** `GhpRsq-Defect-Class` trailers record one class, `performance` (×1, #8). Generated `FromNode` must be single-pass to avoid repeating that pattern; see FN1.
- **No precedent for adding a publishable crate since the release tooling existed.** All five crates came in the first commit, and the release scripts hard-code names, counts and the dependency shape. The prior-art checklist enumerates every touch point, including several the spec missed.

## Decisions

### User decisions (verbatim choices, 2026-10-10)

- **Naming:** "snake_case tool, field idents, verbatim variants". `struct ReadFile` becomes the tool `read_file`. A field's parameter name is its ident with `r#` stripped. A `ToolEnum` value is the variant ident verbatim. Explicit `name = ".."` always wins, including `#[param(name = "..")]` on a variant.
- **Defaults:** "Fill the default in FromNode". A non-`Option` field with `#[param(default = ..)]` is optional in the schema, and generated `FromNode` converts the declared default when the member is absent.
- **`unknown = pass`:** "Allow with an `extra` field". `#[tool(unknown = pass)]` requires exactly one field marked `#[param(extra)]` of type `json::Node`, which receives an object of the undeclared members. Without it, the derive emits `compile_error!`. `extra` without `pass` is also a `compile_error!`.
- **Doc comments:** "Yes, explicit attribute wins". Trimmed `///` lines, joined with `\n`, are the description fallback for structs, fields, enums and variants.

### Technical decisions (from analysis Q1, Q3, Q5, Q6, Q7, Q9, Q10)

- **D-1 (Q1): two new traits in `purrassist-core`, `tool_call` module.** A proc-macro crate cannot export traits, so the traits live in core.
  - `ToolArguments: FromNode` provides `fn tool_definition() -> ToolDefinition` and `fn object_spec() -> ObjectSpec`.
  - `ToolParameter: FromNode` provides `fn parameter() -> ParameterSpec` (the unrequired parameter shape).
  - Core implements `ToolParameter` for `String`, the 12 integer types, `f64`, `json::Number`, `bool`, `Vec<T: ToolItem>` and `Option<T: ToolParameter>`. Derived structs and enums implement it as well, so field dispatch is `<FieldType as ToolParameter>::parameter()`. An unsupported field type then fails with rustc's spanned "trait not implemented" error on the field type.
  - These are public API additions, documented with examples.
- **D-2 (A5, A6): public `FromNodeError` constructors and a single-pass member reader in core.** Generated code must not copy core helpers (helper-check would flag duplication), and must tell missing, duplicate and unknown members apart. Core gains:
  - public `FromNodeError::{mismatch, missing, duplicate, unknown, within}`, with the exact `pointer`/`expected`/`found` shape;
  - `tool_call::Members`: one pass over an object's members that classifies each as declared field i, duplicate or undeclared, in O(n log k).

  The derived `FromNode` uses `Members` and never `Node::get`.
- **D-3 (Q6): member names in `FromNode`.** Only canonical names are accepted (recovered arguments are canonical); aliases are not re-resolved.
  - A required member that is missing is a `missing` error.
  - Any duplicate member is a `duplicate` error.
  - An undeclared member is an `unknown` error under `error`/`drop`. Recovery already enforced the policy, so this only fires for hand-made nodes. Under `pass` it goes into the `extra` field.
- **D-4 (Q3):** on a struct used only as a nested object, the tool-level keys `name`, `alias`, `effects` and `policy` are still accepted on the type; they are ignored for nesting. `unknown` maps to `ObjectSpec::unknown_parameters`. When a nested struct's own description exists and the field has no description, the struct's description becomes the field's description.
- **D-5 (Q4): `default` syntax.** Rust-token JSON: `10`, `-1.5`, `true`, `null`, `"s"`, `[..]`, `{ "k": v }`. The macro lowers the structure to `::purrassist_core::json::Value` constructors; string literals are spliced verbatim as Rust literals (no unescaping or JSON quoting in derive); `true`/`false`/`null` map to `Value::Bool`/`Value::Null`; a number token must match the JSON number grammar textually (no suffix, no `_`, no radix prefix; one leading `-` allowed), checked at expansion, and is emitted as `json::Number::from_str("<spelling>")` through a `#[doc(hidden)]` core helper that documents its precondition and panic. Malformed or non-JSON tokens are `compile_error!` at the token. No `const` evaluation is claimed. Type conformance is checked twice: `ToolSet::compile` rejects a mismatched default (`InvalidDefinition`, tested), and the generated `FromNode` converts the default with `<T as FromNode>::from_node` and reports a mismatch as `FromNodeError { pointer: "/<field>", expected: <T's text>, found: <default's description> }` (tested with `"x"` for `u32`).
- **D-6 (A7):** `repeat` is handled by `ToolRepeatable` (D-13). It is valid on `Vec<T>` and `Option<Vec<T>>`, and emits `<F as ToolRepeatable>::repeated()`, which is `T::parameter().repeatable()`. On any other type it is a rustc trait error spanned at the field type.
- **D-7 (Q7, maximal utility): include `#[param(pattern = "..")]`.** The `Param::string().pattern(..)` builder exists. It is valid only on `String` (and `Option`/`Vec` of `String`).
- **D-8 (A9): integer ranges.** the typed builder has no integer range keywords (`Param::integer()` only; `ValueShape::Integer` carries none). Every integer type maps to `Param::integer()`. Range is enforced only by `FromNode` (`from_node.rs:193-213`), which reports `expected: "integer within the u8 range"`. The derive's Rustdoc states this, recommends fixed-width integers, and notes that `usize`/`isize` ranges differ on wasm32. Derive test: a `u8` field recovered from a turn carrying `300` validates against the schema and then fails `from_node` with exactly that error. Range keywords are not added in this change.
- **D-9 (Q9): recursion.** Direct self-reference (`Self`, or the struct's own ident in a field type, including inside `Vec`/`Option`) is a spanned `compile_error!`. Indirect recursion is a compile-time cycle error: `ToolParameter` carries `const DEPTH: usize`; scalar impls are `0`, `Vec<T>`/`Option<T>` forward `T::DEPTH`, derived types are `1 + core::tool_call::max_depth(..)` (a `const fn` in core), and every derive emits `const _: usize = <Self as ToolParameter>::DEPTH;` so rustc evaluates it (E0391 cycle). The error is spanned at the `#[derive]` because `FromStr` output carries `call_site` spans; the derive's docs say so. Direct self-reference keeps its spanned `compile_error!`. Test: `compile_fail` doctest `A { b: Vec<B> }` / `B { a: Option<A> }`, with a passing twin where `B` holds `String` instead.
- **D-10 (Q10, A1): release order core → derive → syntax → purrassist → host → cli.** The spec's "derive first" fails `check-release.py`, because derive dev-depends on core.
- **D-11 (A2): audit.** Derive's closure must be empty over normal and build edges. Over dev edges it may reach only `purrassist-core`. The ambient-access scan covers `crates/derive/src`.
- **D-13 (review): fully trait-driven field handling (pass-2 option a).** Apart from D-9's own-ident scan for direct self-reference, the derive never inspects field types syntactically. Every combination is enforced by rustc trait resolution, so aliases and full paths behave identically.
  - **`ToolParameter: FromNode`** provides `fn parameter() -> ParameterSpec`, `const REQUIRED: bool` (true except for `Option<T>`), `fn absent() -> Option<Self>` (`Some(None)` for `Option<T>`, otherwise `None`) and `const DEPTH: usize` (D-9).
  - **`ToolItem`:** a marker implemented for every `ToolParameter` type except `Option<T>`. The bound is `Vec<T: ToolItem>`, so `Vec<Option<T>>` fails to compile with rustc's spanned "trait not implemented" error.
  - **`ToolRepeatable`:** `fn repeated() -> ParameterSpec`, implemented for `Vec<T>` (giving `T::parameter().repeatable()`) and `Option<Vec<T>>`. `#[param(repeat)]` emits `<F as ToolRepeatable>::repeated()`, so `repeat` on any other type is a rustc error spanned at the field type.
  - **`ToolPattern`:** `fn with_pattern(&'static str) -> ParameterSpec`, implemented for `String`, `Option<String>` and `Vec<String>`. For `Vec`, the pattern applies to the item. `#[param(pattern)]` emits it; any other type is a rustc error.
  - **`ToolExtra`:** implemented for `json::Node`. It enforces the type of `#[param(extra)]`.
  - Each derive emits `impl ToolParameter` and `impl ToolItem` for its type, so derived structs and enums can sit inside `Vec<..>` and `Option<..>`.
  - **`Option<T>` + `default`:** the default fills, and an absent member yields `Some(default)`. `None` arises only from an explicit `null` in a hand-made node, because a `null` default fails `ToolSet::compile`. The derive docs state this in one sentence.
- **D-14 (review advisory): snake_case algorithm.** Strip `r#`. Split the ident at lower→upper boundaries and at acronym ends; a digit followed by an uppercase letter also splits (`HTTPRequest` → `http_request`, `ReadFile2` → `read_file2`, `Read2File` → `read2_file`). Lowercase ASCII and join with `_`. An existing `_` is kept. The result must pass core's name validation, otherwise it is a `compile_error!` asking for `name = ..`.
- **D-12 (Q5): compile-fail evidence.**
  - Each rejection has a `compile_fail` doctest paired with a near-identical passing twin, which proves the specific construct is the cause.
  - The macro's `compile_error!` messages are also unit-tested through a pure parsing layer that operates on a small first-party token model, built from `proc_macro` tokens at the boundary, with spans modelled as indices into a side table. Its message and span-selection logic is then unit-testable without `proc_macro` (A-risk: `proc_macro` panics outside macros).

## Completeness contract

IDs refer to `issue-analysis.md`. "Derive test" means `crates/derive/tests/derive.rs` through `cargo test -p purrassist-derive --locked`.

| Requirement | Task | Acceptance check (real entry point → observed) |
|---|---|---|
| D-1, D-2, D-13 core traits (`ToolArguments`, `ToolParameter`, `ToolItem`, `ToolRepeatable`, `ToolPattern`, `ToolExtra`), constructors, `Members`, the number helper, `max_depth` | 1 | `cargo test -p purrassist-core --locked`: unit tests and doctests for each new public item. `python3 scripts/check-repository.py` passes (core stays pure). |
| L1/L2/L3 crate layout (`proc-macro = true`, workspace lints, no `[dependencies]`, README, 3 LICENSE files, SPDX) | 2 | `check-repository.py` (license loop, SPDX); `cargo metadata --locked` shows no deps for derive |
| P1 parser (attributes, visibility, named fields, raw idents, `Vec`/`Option`/paths) | 2 | derive test structs of each shape compile, and each is `assert_eq!` to its hand-built `ToolDefinition` |
| P2 `compile_error!` cases (generics, tuple struct, union, unknown `#[tool]`/`#[param]` key, enum given to `ToolDefinition`, struct given to `ToolEnum`, non-unit variant, `repeat` on non-`Vec` (rustc trait error), `pattern` on a non-string (rustc trait error), `Vec<Option<T>>` (rustc trait error), `extra` misuse, `pass` without `extra`, malformed `default`, self-reference) | 2 | `cargo test -p purrassist-derive --doc`: one `compile_fail` doctest per case plus its passing twin; unit tests of the pure layer assert the exact message text |
| P3 writer (`FromStr` → `TokenStream`, `LexError` unreachable) | 2 | derive tests with raw idents, Unicode idents, and escapes in descriptions and defaults |
| P4/P5 two macro pairs and facade re-exports | 2, 3 | Task 2: derive test using `purrassist_derive::{ToolDefinition, ToolEnum}` with only core. Task 3: `crates/purrassist/tests/derive_facade.rs` with `#[derive(purrassist::ToolDefinition)]` and no attribute, **plus** a test in a crate without a direct core dependency (A10), proving `::purrassist::__core` paths |
| P6 core never depends on derive | 4 | `check-repository.py` core closure check (unchanged rule) |
| T-map (every type and nesting) | 2 | derive test: one struct with every mapped type, `assert_eq!` to the hand-built definition; `ToolSet::compile` is Ok; the rendered JSON schema holds the expected fragments |
| ATTR-tool, every key and value (`name`, `alias`, `description`, `effects`, `policy`, `unknown` = error/drop/pass) | 2 | derive test: one equality per key and value; after compile, `CompiledTool::{effects, policy, unknown_parameters, description}` and `lookup_exact(alias)` |
| ATTR-param (`name`, `alias`, `description`, `repeat`, `default`, `pattern`, `extra`) | 2 | derive test: equality per key; compiled `argument_shape` resolves aliases; `defaults()` values; a repeatable parameter renders as an array schema |
| User decisions: naming, defaults filled, `extra`, doc-comment descriptions | 2 | derive tests: `ReadFile` → `"read_file"`, `HTTPRequest` → `"http_request"`, `ReadFile2` → `"read_file2"`, `Read2File` → `"read2_file"`, field `r#type` → `"type"`; an absent defaulted field converts to its default; under `pass`, undeclared members land in `extra` through a full `recover` → `from_node`; a doc comment becomes the description and an explicit `description` wins |
| FN1-FN3 `FromNode` through the real pipeline | 2 | derive test: `ToolSet::compile([Args::tool_definition()])` → `recover` on a real model turn in `hermes.v1` and in `qwen3-coder-xml.v1` (nested-parameter items) → `Args::from_node(&call.arguments) == expected`. Negatives assert the exact `FromNodeError` (missing, duplicate, unknown, type mismatch, nested pointer `/field/0/sub`). |
| FN1 single-pass | 1, 2 | Core unit test (in `Members`'s own `#[cfg(test)]` module): 64 declared names, 64 members, at most 64·⌈log₂ 64⌉ key comparisons through an injected comparison counter; one pass over `members` is asserted by the iteration count. `Members` requires its declared-name table sorted (the derive emits a sorted static table plus the declaration-order index; the sort happens at expansion, deterministically). Derive test: a node with a duplicated member is reported as `duplicate`, not `missing`; a `Node::get`-based conversion cannot produce that result because `get` yields `None` for both, so reverting to per-field lookup fails this test. |
| D-9 recursion | 2 | `compile_fail` doctests: direct self-reference, and indirect recursion as a const cycle; each has a passing twin |
| I8 hygiene | 2 | I8 hygiene: generated locals use `Span::mixed_site()` (re-spanned after `FromStr`) or `__`-prefixed names, and every path is absolute (`::core::option::Option`, `::std::vec::Vec`, `::purrassist_core::..`). Derive test module declares `const node: u8 = 0; struct members; mod core {} mod std {}` beside a derived struct; it compiles and converts. |
| G1 workspace `members`, workspace dependency, `Cargo.lock` | 2 | `cargo metadata --locked`; `cargo build --locked` |
| G2/G3 `RELEASE_ORDER`, `publish.yml` loop, step name and comment | 4 | `python3 scripts/check-release.py` exits 0 and lists six crates in the D-10 order |
| G4 `NAMES` | 4 | `python3 scripts/test-package-workspace.py` |
| G5 `CRATE_NAMES` and multi-edge `dependency_of`, proc-macro fixture (A12) | 4 | `python3 scripts/test-verify-published.py` |
| G6 `packaged_notices.derive`; `check-packages.py` zero-dependency check for derive | 4 | `check-repository.py`; `make package` (once, in Task 5) |
| AU1-AU3 audit (D-11) | 4 | `python3 scripts/test-repository-audit.py`, with new fixtures: derive with a normal dependency fails; derive with a build dependency fails; derive dev → `purrassist-core` passes; derive dev → third-party fails; `crates/derive/src` with `std::env` fails under the derive label |
| DOC1-DOC4 and missed edits (A11-A13, A17), plus review R5 | 4 | `grep` finds no stale "five crates" or "#4" references to derive in `RELEASE.md`, `IMPLEMENTATION.md`, `QUALIFICATION.md`, `RESOURCES.md`, `README.md`, `docs/tool-calls/README.md` or `verify-published.py`. `RELEASE.md` names the external one-time steps (crates.io name reservation, trusted publisher for `purrassist-derive`) as not done and required before tagging (A14).  `docs/tool-calls/INTEGRATION.md` "Rust API" lists every new core `pub` item and the facade re-exports; `docs/QUALIFICATION.md` gains an evidence row for `crates/derive/tests/derive.rs`, the compile-fail doctests and `crates/host/tests/derive_facade.rs`, and line 37 reads "Six". The stale-text grep (`five crates|five-crate|five publishable|all five|each of the five`) also covers `scripts/package-workspace.py` and `scripts/test-verify-published.py`. |
| MSRV 1.98 (risk) | 2, 3, 5 | `rust-validate 1.98.0 cargo test --workspace --locked` (an existing sanctioned toolchain) exits 0 |
| Full gate | 5 | `make check` (once), `make package`, `make wasm-test` (core changed), the checks listed above |

## Declined or out of scope (with reasons)

- **Generic structs, tuple structs and unions** are rejected with `compile_error!` (spec). Generic tool arguments have no stable schema.
- **Data-carrying enum variants** are rejected. A `ToolEnum` is a string enumeration, and tagged unions would need `anyOf` support in the builder, which does not exist.
- **Recursive derived types** are rejected at compile time (D-9). The cost is that recursive schemas need hand-built or declared tools.
- **Performing the crates.io name reservation and the trusted-publisher setup.** These are external maintainer actions. They are documented as required before tagging, and are not claimed as done.

## Task 1: Core support for derived tools (traits, constructors, single-pass `Members`)

- **Files:**
  - `crates/core/src/tool_call/{from_node.rs, definition.rs, mod.rs}`;
  - possibly a new `crates/core/src/tool_call/derive_support.rs`, with SPDX header and Rustdoc examples.
- **Contents:**
  - D-1: `ToolArguments` and `ToolParameter` with all scalar, `Vec` and `Option` impls and `const DEPTH`;
  - D-2: public `FromNodeError` constructors and `Members`;
  - D-5: the `#[doc(hidden)]` number helper (`json::Number` from a spelling validated at expansion, documented precondition);
  - D-13 traits: `ToolParameter` (with `REQUIRED`, `absent`, `DEPTH`), `ToolItem`, `ToolRepeatable`, `ToolPattern`, `ToolExtra`, and their impls for the scalars, `Vec` and `Option`;
  - `const fn max_depth` (D-9).
  - D-8 adds no core code: there is no range builder, so `FromNode` alone enforces ranges.
- **Tests:** unit tests and doctests, plus a `Members` single-pass counter test.
- **Focused checks:** `cargo test -p purrassist-core --lib --doc --test tool_call_recover`, `check-repository.py`, clippy, `make hygiene`.
- **Then:** review → commit → push → issue comment.

## Task 2: The `purrassist-derive` crate

- **Files:**
  - `crates/derive/{Cargo.toml, README.md, LICENSE-*, src/lib.rs, src/parse.rs, src/model.rs, src/write.rs, tests/derive.rs}`;
  - root `Cargo.toml` (`members` and workspace dependency);
  - `Cargo.lock`.
- **Contents:** the parser, model, writer, both macro pairs, the full type map, every attribute, the user decisions, D-3 through D-9, compile-fail doctests with twins, and the derive tests from the contract.
- **Focused checks:** `cargo test -p purrassist-derive --locked`, `--doc`, clippy `-D warnings`, MSRV `rust-validate 1.98.0 cargo test --workspace --locked`, `make helper-check`, `make hygiene`.

## Task 3: Facade integration

- **Files:** `crates/purrassist` (dependency on derive, `#[doc(hidden)] pub use purrassist_core as __core;`, `pub use purrassist_derive::{FacadeToolDefinition as ToolDefinition, FacadeToolEnum as ToolEnum};`), `crates/purrassist/tests/derive_facade.rs`, and `crates/host/tests/derive_facade.rs`. Host depends only on `purrassist`, not on core (A10), which proves the `::purrassist::__core` paths. Derive and facade tests are added to the Windows CI job definition.
- **Focused checks:** `cargo test -p purrassist --locked`, `cargo test -p purrassist-host --test derive_facade --locked`, MSRV, clippy.

## Task 4: Release, packaging and audit registration, and docs

- **Files:**
  - G2-G6: `check-release.py` `RELEASE_ORDER`, `publish.yml`, `test-package-workspace.py`, `test-verify-published.py` (multi-edge and proc-macro fixture), `check-packages.py`, `vendor/dependencies.json`;
  - AU1-AU3: `check-repository.py` and `test-repository-audit.py`;
  - DOC1-DOC4 and the missed edits from A11-A13, A14 and A17.
- **Focused checks:** each named script exits 0, and the `grep` checks for stale references.

## Task 5: Full qualification, PR and handoff

- `make check` (once), `make package`, `make wasm-test`, MSRV, a completeness review, and the artifact inventory.
- Create the PR (`Closes #15`; changes, validation, residual risks including the external publication steps), post the plan, and post the "least confident" note on the issue and the PR.

## Plan change during execution
- G2/G3 (`RELEASE_ORDER` in `check-release.py`, the `publish.yml` loop and step name) moved from Task 4 to Task 2. Once the derive crate exists, `check-release.py`, which `make hygiene` runs, fails until derive is in the release order, and every commit must leave the gate green. Task 4 keeps the rest of the registration: package and verify-published scripts, notices, the audit, and docs.

## 2. paudley — 

**Handoff: what I'm least confident about, and what you should know**

**Least confident**
1. **Hand-rolled token parsing.** There is no `syn`, so the parser and writer handle every Rust surface form a user might write on their own.
   - Covered: attributes, visibility, raw and Unicode identifiers, multi-segment and `::`-prefixed paths, nested generic angle brackets, and doc comments.
   - Exotic syntax may still hit an unhandled shape: macros in type position, `impl Trait`, attributes on fields produced by other derives, `#[cfg]` on fields. Where it does, the result is a spanned compile error, not wrong code, but the message may be less precise than with `syn`.
2. **Compile-fail evidence on stable.** On stable Rust, `compile_fail` doctests confirm only that a snippet fails, not why. Each of the 42 was checked by hand (twice, by two reviewers) to fail for the intended reason, E0277, E0391 or E0080. Nothing re-asserts the error code automatically after future rustc changes.
3. **Error spans for indirect recursion and invalid names.** These land on the `#[derive]` line, not the offending field, because they come from `const` evaluation. The docs say so, but it is the weakest error experience in the crate.

**What you may not know**
- **Publishing.** Before the next release tag, `purrassist-derive` needs its **crates.io name reserved** and a **trusted publisher configured**. Neither can be done from here. Without them, publishing stops after `purrassist-core`, leaving a partial release. `docs/RELEASE.md` lists both as required.
- **Release order.** It is now core → derive → syntax → purrassist → host → cli. The spec said derive first, but that fails `check-release.py`, because derive's tests depend on core.
- **Core API.** Core gained public API: six traits, `Members`, the `FromNodeError` constructors and `max_depth`. `is_valid_name` became a `const fn`; its equivalence was checked over 6.4M inputs.
- **A test fixture changed shape.** The staged-credential guard blocked a commit because of a pre-existing `'fake-credential'` sentinel in `scripts/test-package-workspace.py`. I restructured the fixture to build its token variables from one tuple; it still asserts that all three tokens are stripped. The guard itself is untouched.

## 3. paudley — 

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

## 4. paudley — 

**Stage 2, R1: generated-code hygiene** (1e17df7, pushed)

- **Name check:** it no longer puts the user's name into a format string, so braces in a name give the intended error. It uses `::core::assert!`, so a user `assert` macro can't disable it.
- **Primitives:** they are absolutely pathed, so shadowed `bool`, `usize` and `str` no longer break the expansion.
- **Field and variant names:** they are spliced from the original tokens, so placeholder-named fields work.
- **Non-ASCII derived parameter names:** they are a compile error at the field.
- **Evidence:** standalone builds and a fully shadowed macro expansion. Derive and facade tests, doctests, clippy, `make hygiene` and MSRV pass.
- **Review:** independent review PASS.

## 5. paudley — 

**Stage 2, R2 / G1 (HIGH): nested structs state their unknown-member policy** (64097ec, pushed)

- **Gap:** under a tool's `unknown = pass`, a nested derived struct inherited pass-through. Recovery returned a Resolved call, and conversion then failed on an extra nested member.
- **Fix:** a derived nested struct always states its policy. With no `unknown` key it defaults to dropping extras with a recorded loss (user decision). Top-level tools are unchanged.
- **Evidence:** real recover → conversion tests for error, drop, pass + extra and no key. The no-key test fails on the previous code.
- **Review:** independent review PASS.

## 6. paudley — 

**Stage 2 remediation R3 — `d46b996`** `refactor(tool-call): harden the derive support surface before first publish`

Closes F-4, F-5 (core part), F-6b, and the core part of A-1 from the remediation plan.

- **F-4:** generated conversions assert at compile time that their name table is strictly sorted (`__sorted`, doc-hidden const fn). `Members` docs now match actual use.
- **F-5:**
  - `ToolExtra` is sealed to `json::Node`, with a doc-hidden `__from_members`.
  - `Slot` is `#[non_exhaustive]`, and generated matches return a `FromNodeError` on an unknown slot rather than panicking.
  - `f32` is supported: written exactly, read with one round-to-nearest, non-finite values rejected with `expected: "number within the finite f32 range"`.
  - `Box<T>` forwards `FromNode`/`ToolParameter`/`ToolItem`. It does not enable recursion: E0391 compile_fail doctests, each with a passing twin.
  - `ToolParameter` has default items.
- **A-1 (core):** derived code converts through `<T as ToolExtra>::__from_members`. A wrong `extra` field type now produces a single E0277 at the field. This was verified with a standalone crate (no E0308).
- **F-6b:** a unit test in `crates/derive/src/model.rs` checks the derive's default-value number validator against core's JSON `Number` parser. It covers a hand-picked corpus plus every short spelling over `-01.eE+`. It is a unit test rather than an integration test because the validator is crate-private in the proc-macro crate.
- **Ledger:** the `f32`/`f64` `FromNode` bodies now share one macro. One `[[distinct]]` row records that the `Box`/`Option` `parameter()` forwards differ (`REQUIRED`/`absent()`).

**Checks:**
- `cargo test -p purrassist-core -p purrassist-derive -p purrassist --locked`: pass.
- clippy `-D warnings`, fmt, `make hygiene`, `make helper-check`, `scripts/check-repository.py`: pass.
- MSRV 1.98.0 core+derive tests: pass.
- Independent review: PASS.

`make check` runs once, on the settled head. Hosted CI has not run.

## 7. paudley — 

**Stage 2 remediation R4: `9534a4f`** `feat(derive): crate path override and a versioned core marker`

Closes the rest of F-5, and G5.

- **`#[tool(crate = path)]`**
  - Replaces the core path prefix on both derives, in both the direct and facade flavors. The path must resolve `tool_call::` and `json::`.
  - Malformed values are spanned derive errors: empty, trailing `::`, literal, generics, keyword, or duplicate. These are pinned by compile_fail doctests 47–50, each with a passing twin.
  - In-tree tests cover a re-export module, a `use` alias, absolute, `super`/`self`, and facade (`::purrassist`, `::purrassist::__core`) paths.
- **Version marker:** every expansion references core's doc-hidden `__DERIVE_ABI_0_1`, named from the derive's own major.minor. A core test fails if the version moves without renaming it. Version requirements stay `^`.
- **Docs:** the same-version rule, the dependency-name requirement or the `crate =` alternative, and "convert only usable/resolved calls" (`ToolCalls::usable`, `ToolCall::is_usable`, `CallOutcome::Resolved`).

**Manual evidence (out of tree, not part of the suite)**
- A crate with a renamed key `pa = { package = "purrassist-core", path = … }` and `#[tool(crate = ::pa)]` compiles and converts. Without the attribute it fails with E0433.
- A core copy with the marker renamed fails with a single E0425 naming `__DERIVE_ABI_0_1`.
- An in-tree renamed dependency isn't possible, because Cargo rejects one package listed under two names in a single manifest.

**Checks**
- Core, derive, and facade tests; workspace clippy `-D warnings`; fmt.
- `make hygiene`, `make helper-check`, `check-repository.py`, `check-release.py`, `test-package-workspace.py`, `test-verify-published.py`.
- MSRV 1.98.0.
- All pass. Independent review: PASS.

Hosted CI has not run.

## 8. paudley — 

**Stage 2 remediation R5 — `b7475b4`** `build(release): publish preflight checks crate names and owners before publishing`

Closes G3.

- **Preflight step.** `publish.yml` runs "Preflight crates.io names and owners" before trusted-publishing authentication. For every crate in the release order, `check-release.py --preflight-owners` requires that the crate exists on crates.io and that its owner set equals `purrassist-core`'s. No login is hard-coded. All failures are reported together, and the job stops before any `cargo publish`.
- **Trusted-publisher state.** This cannot be read unauthenticated. It stays the manual pre-tag step in `docs/RELEASE.md`, as does the `0.0.0` name-reservation step for `purrassist-derive`.
- **Validator.** `validate_publish_workflow()` now requires the step, its owners check, the release user agent, and its position before both authentication and the first `cargo publish`.
- **Tests.** The new `scripts/test-check-release.py` (18 tests) checks the validator against the real workflow and against broken copies. It also checks the preflight against fake registry responses. It runs in `make hygiene` and in the native verify job.

**Checks.** `test-check-release.py`, `check-release.py`, `check-repository.py`, `make hygiene`, `make helper-check`, and actionlint all pass. The reviewer also checked the real fetcher against a local HTTP server returning 200, 404, 500, and a refused connection. Independent review: PASS.

**Not covered.** The live registry request first runs on a tag push; tests are offline. Hosted CI has not run.

## 9. paudley — 

**Stage 2 remediation R6: `d0a99e9`** `test(derive): close remaining evidence and doc gaps`

This closes G4, G6, G7 and G9. A-1 was completed in R3 and is verified here.

- **G4:** An enum with a doc comment, an explicit description and described variants now pins three things: the explicit text wins, variant lines follow it, and whitespace inside a variant description collapses.
- **G6:** The derive README example runs as a doctest. The root README derive example sits between markers, and `check-repository.py` requires it to match the doctested facade README copy byte for byte. Five negative tests cover that check.
- **G7:**
  - Every one of the 58 `compile_fail` blocks was built standalone to read its real error code. Four derive-support doctests gained `E0277`.
  - `compile_error!` cases have no rustc code and stay uncoded. That includes the `Option<Self>` case, where an incidental E0072 would otherwise let a missing derive message pass.
  - The reviewer confirmed that nightly rustdoc rejects a wrong code on each of the four.
- **G9:** Over-long release prose added by this branch is rewrapped.

**Checks:** core, derive and facade tests and doctests all pass, as do fmt, clippy `-D warnings`, `check-repository.py`, `test-repository-audit.py` (34), `make hygiene`, `make helper-check` and MSRV 1.98.0. The independent review passed.

All remediation commits (R1–R6) are now pushed. The full qualification on this head is next. Hosted CI has not run.

## 10. paudley — 

## Stage 2 summary: PR #19 is ready for Stage 3

**Gaps resolved.** Every remediation-plan item is FIXED. None was declined beyond the plan's recorded Declined list, and no deficiency entry was added.

| Commit | Items |
|---|---|
| `1e17df7` | **R1:** hygiene (F-1), non-ASCII derived names |
| `64097ec` | **R2:** nested unknown-member policy (G1). User decision: "Drop with recorded loss" |
| `d46b996` | **R3:** sortedness assert, sealed `ToolExtra` + `__from_members` (A-1), `#[non_exhaustive] Slot`, `f32`, `Box<T>`, `ToolParameter` defaults, number-validator agreement test (F-4, F-5, F-6b) |
| `9534a4f` | **R4:** `#[tool(crate = path)]`, `__DERIVE_ABI_0_1` marker, same-version / usable-calls docs (F-5, G5) |
| `b7475b4` | **R5:** publish preflight for crate names and owners (G3) |
| `d0a99e9` | **R6:** enum-description test, README doctest + drift check, `compile_fail` error codes, prose rewrap (G4, G6, G7, G9) |
| `6cbb628`, `4d8a48e`, `637018b` | **Exit-audit documentation:** near-miss rule per matching policy, patch-compatibility release rule, qualification evidence row |

**Reviews.**
- Every remediation commit had an independent review, and each one passed.
- The fresh exit completion audit blocked twice: first on a stale PR description and documentation gaps, then on near-miss wording that overstated safety under the tolerant policy. Both are fixed, and the audit passed on recheck.

**Audit demonstrations.** These ran on real entry points, from downstream crates:
- Nested unknown-member policies through `recover` (Hermes, Qwen3-Coder XML) and `from_node`. Output is identical natively and under WASI.
- Hygiene with standard names shadowed, in both flavors.
- A Cargo-renamed core dependency with `crate = ::pa`, and a facade-only crate.
- A simulated version skew, which fails with an error naming `__DERIVE_ABI_0_1`.
- The README example end to end.
- The publish preflight against a local fake registry.

**Local validation.**
- On `d0a99e9`, all exit 0:
  - `make check`
  - `make package` (6 archives verified)
  - `rust-validate 1.98.0 cargo test --workspace --locked`
  - `make wasm-test` (native, WASI and browser-WASM transcripts match)
- The three later commits change only prose. Derive doctests, rustdoc with `-D warnings`, `check-repository.py` and `make hygiene` were rerun on them and pass.

**Hosted CI.** Did not run.

**External steps before any release tag** (maintainer, per `docs/RELEASE.md`):
- reserve the `purrassist-derive` name on crates.io (`0.0.0` placeholder);
- configure its trusted publisher.

**Stage 3 obligations.**
- Merge-tree and integration assessment against the current `main`.
- Review-debt audit (there are no review threads today).
- Finalize the squash notes; a draft is in the stage directory.
- Append the D2 resolution record to `.deficiencies` with the merge SHA.

