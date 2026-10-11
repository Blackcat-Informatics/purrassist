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
