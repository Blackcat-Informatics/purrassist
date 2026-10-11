# Plan review: issue #15 `purrassist-derive` (Stage 1)

**Reviewed:** `.stage/purrassist-derive-zero-dependency/plan.md` (branch
`paudley/15-purrassist-derive-zero-dependency`, base `main` @ `fccfbe7`).
**Date:** 2026-10-10.

## Coverage

Read in full: `plan.md`, `issue.md`, `issue-analysis.md`, `prior-art-assessment.md`,
`AGENTS.md`, `.goals`, `docs/IMPLEMENTATION.md`, `docs/RELEASE.md`,
`docs/DEPENDENCIES.md`, `crates/core/src/tool_call/{definition.rs,from_node.rs,mod.rs}`,
`crates/purrassist/src/lib.rs`, root `Cargo.toml`, `scripts/check-release.py`,
`scripts/check-repository.py`, `scripts/check-packages.py`,
`.github/workflows/publish.yml`, `vendor/dependencies.json`. Read in part (the
regions the plan relies on): `scripts/test-verify-published.py` (1-240),
`scripts/test-package-workspace.py` (1-60), `scripts/test-repository-audit.py`
(1-130), `docs/tool-calls/INTEGRATION.md` (1404-1513), `docs/QUALIFICATION.md`
(30-49, 108-127), `crates/core/src/json/mod.rs` (120-151). Grepped: every
`crates/*/Cargo.toml` dependency line, `verify.yml` test matrix, all "five" /
`purrassist-derive` / "issue #4" sites, `minimum|maximum` across
`crates/core/src/tool_call`, `json::parse` / `Number: FromStr` / `Node::new`
signatures, `shape.rs` integer shape.

Design claims checked against code:

- **Integer range builders:** none exist. `Param::integer()` is the only integer
  constructor (`definition.rs:331`), `ParameterSpec` has no min/max field
  (`definition.rs:146-154`), `ValueShape::Integer` is a unit variant
  (`shape.rs:175`), and no `minimum`/`maximum` keyword appears anywhere under
  `crates/core/src/tool_call`. D-8's conditional is therefore already decided.
- **`FromNode` shape:** `pub trait FromNode: Sized { fn from_node(&Node) -> Result<Self, FromNodeError> }`
  (`from_node.rs:96-103`); `Vec<T: FromNode>` and `Option<T: FromNode>` impls exist.
  `ToolParameter: FromNode` with blanket `Vec<T: ToolParameter>` /
  `Option<T: ToolParameter>` impls is compatible.
- **`FromNodeError` constructors:** `mismatch`, `within`, `describe` are private
  (`from_node.rs:33-46, 66-75`); D-2 is necessary.
- **`Node::get`** returns `None` for duplicates (`json/mod.rs:134-147`); D-2's
  `Members` is necessary to tell missing from duplicate.
- **`json::parse`** is a plain `pub fn` (`json/parser.rs:64`), not `const`.
- **Release order:** `check-release.py:118-128` walks every dependency kind
  (dev included) and requires `^0.1.0`; `publish.yml:94` loop must equal
  `RELEASE_ORDER` (`check-release.py:79-83`). D-10's order
  core → derive → syntax → purrassist → host → cli satisfies both; "derive
  first" would fail. D-10 is correct.
- **A10 host crate:** `crates/host/Cargo.toml:25-26` declares only
  `purrassist.workspace = true`; `crates/cli/Cargo.toml:20-22` declares
  `purrassist-host` and `purrassist`. Neither has a direct `purrassist-core`
  dependency, so `::purrassist_core` does not resolve in their test targets.
  A test in `crates/host/tests/` or `crates/cli/tests/` is a valid A10 location;
  no fixture crate is needed.
- **INTEGRATION.md** has a "Compatibility → Rust API" section
  (`INTEGRATION.md:1406-1513`) listing every Rust API change per release.
- **`cfg(test)` visibility:** a dependency is never compiled with the dependent's
  `cfg(test)`; see finding 2.
- **`proc_macro` outside a macro:** the D-12 first-party token model is the
  workable route (token construction panics without the bridge); feasible as
  planned, provided spans are modeled as indices (finding 12).

Constraints (`.goals`, `AGENTS.md`): no violation found. Core additions are pure
and first-party; no third-party dependency enters; hooks are not bypassed; the
external crates.io steps are not claimed. Note `.goals` line 6: the
no-optionality rule "DOES NOT APPLY TO CODE", so `Option<T>` fields and
optional schema parameters are not findings.

## Findings

### REQUIRED

**1. D-5 `default` lowering is incoherent and contradicts the plan's own evidence.**
Plan lines 58-60:

> The macro serializes them to JSON text, and the generated code parses that text with `json::parse` inside a `const` assertion helper in core (`tool_call::default_node(&'static str) -> Node`). That helper panics only on text the macro itself validated as JSON at expansion.
> ...
> A type mismatch against the field (e.g. `"x"` for `u32`) is caught by a generated test-free check: `ToolSet::compile` rejects it, and the derive test covers this.

Problems: (a) `json::parse` allocates and is not `const` (`parser.rs:64`), so
there is no "const assertion"; `default_node` would be a runtime `fn` with a
panic path in core's public API. (b) Serializing Rust tokens to JSON text
requires the derive to unescape Rust string literals (`\u{..}`, `\x..`, raw
strings; stable 1.98 has no `Literal::str_value`, per the analysis's MSRV risk
row) and then JSON-escape them, which is exactly the duplicated-helper risk
the analysis mitigates with "Lower defaults to `Node::new(Value::…)`
constructors (no JSON quoting in derive)". The plan adopts the opposite without
a reason. (c) "generated test-free check" is not a check. (d) Under the user
decision "fill the default in FromNode", a default that mismatches its field
type is a runtime `FromNodeError` when the caller never compiled a `ToolSet`;
the pointer and `found` text for that case are unspecified.

`REWRITE TO:`
> D-5 (Q4): `default` syntax is Rust-token JSON: `10`, `-1.5`, `true`, `null`, `"s"`, `[..]`, `{ "k": v }`. The macro lowers the structure to `::purrassist_core::json::Value` constructors; string literals are spliced verbatim as Rust literals (no unescaping or JSON quoting in derive); `true`/`false`/`null` map to `Value::Bool`/`Value::Null`; a number token must match the JSON number grammar textually (no suffix, no `_`, no radix prefix; one leading `-` allowed), checked at expansion, and is emitted as `json::Number::from_str("<spelling>")` through a `#[doc(hidden)]` core helper that documents its precondition and panic. Malformed or non-JSON tokens are `compile_error!` at the token. No `const` evaluation is claimed. Type conformance is checked twice: `ToolSet::compile` rejects a mismatched default (`InvalidDefinition`, tested), and the generated `FromNode` converts the default with `<T as FromNode>::from_node` and reports a mismatch as `FromNodeError { pointer: "/<field>", expected: <T's text>, found: <default's description> }` (tested with `"x"` for `u32`). If a single `json::parse` of macro-serialized text is kept instead, the plan must state how Rust literals become JSON text without implementing string escaping in derive, and `make helper-check` is a Task 2 focused check.

**2. FN1 single-pass acceptance is unfalsifiable as written.** Plan line 92:

> derive test: a struct with 64 fields and a node with 64 members takes at most 64·⌈log₂ 64⌉ member comparisons (a `cfg(test)` counter in core's `Members`). Reverting to per-field lookup fails it.

`crates/derive/tests/derive.rs` compiles `purrassist-core` as a dependency,
never with core's `cfg(test)`; the counter does not exist in that build. The
check cannot run.

`REWRITE TO:`
> Core unit test (in `Members`'s own `#[cfg(test)]` module): 64 declared names, 64 members, at most 64·⌈log₂ 64⌉ key comparisons through an injected comparison counter; one pass over `members` is asserted by the iteration count. `Members` requires its declared-name table sorted (the derive emits a sorted static table plus the declaration-order index; the sort happens at expansion, deterministically). Derive test: a node with a duplicated member is reported as `duplicate`, not `missing`; a `Node::get`-based conversion cannot produce that result because `get` yields `None` for both, so reverting to per-field lookup fails this test.

**3. D-8 defers a decision the code already makes.** Plan line 63:

> Each integer type maps to `Param::integer()`, plus its exact range if the builder supports min/max. If it does not, `FromNode` range errors are documented ... The implementer checks the builder; if range builders exist, they are used.

No range builder exists (see Coverage). A conditional acceptance criterion is
not falsifiable, and adding `minimum`/`maximum` to `ParameterSpec`,
`ValueShape` and the schema renderer would be a Rust API change outside #15.

`REWRITE TO:`
> D-8 (A9): the typed builder has no integer range keywords (`Param::integer()` only; `ValueShape::Integer` carries none). Every integer type maps to `Param::integer()`. Range is enforced only by `FromNode` (`from_node.rs:193-213`), which reports `expected: "integer within the u8 range"`. The derive's Rustdoc states this, recommends fixed-width integers, and notes that `usize`/`isize` ranges differ on wasm32. Derive test: a `u8` field recovered from a turn carrying `300` validates against the schema and then fails `from_node` with exactly that error. Range keywords are not added in this change.

**4. D-9's indirect-recursion mechanism is incomplete.** Plan line 66:

> Indirect recursion becomes a compile-time cycle error through an associated `const DEPTH: usize` on `ToolParameter` (each derived type's depth is one plus the maximum of its fields' depths), so it can never overflow the stack at runtime.

An associated const is evaluated only where it is used. If nothing forces
evaluation, two mutually nested derived structs compile, and
`tool_definition()` recurses at runtime. The acceptance doctest would catch an
implementation that forgets this, but the design text must name the mechanism.
`Ord::max` is also not `const`.

`REWRITE TO:`
> Indirect recursion is a compile-time cycle error: `ToolParameter` carries `const DEPTH: usize`; scalar impls are `0`, `Vec<T>`/`Option<T>` forward `T::DEPTH`, derived types are `1 + core::tool_call::max_depth(..)` (a `const fn` in core), and every derive emits `const _: usize = <Self as ToolParameter>::DEPTH;` so rustc evaluates it (E0391 cycle). The error is spanned at the `#[derive]` because `FromStr` output carries `call_site` spans; the derive's docs say so. Direct self-reference keeps its spanned `compile_error!`. Test: `compile_fail` doctest `A { b: Vec<B> }` / `B { a: Option<A> }`, with a passing twin where `B` holds `String` instead.

**5. Unmapped requirements from the plan's own cited evidence.** Plan line 100
lists the stale-reference grep set and line 136 the facade files; neither covers:

- `docs/tool-calls/INTEGRATION.md` "Compatibility → Rust API"
  (`INTEGRATION.md:1487`): new `pub` items in core (`ToolArguments`,
  `ToolParameter`, `FromNodeError::{mismatch, missing, duplicate, unknown, within}`,
  `Members`, any default helper, the facade re-exports and `__core`) must be
  listed there (prior-art §1 checklist and §7.2; `IMPLEMENTATION.md:122`).
- `docs/QUALIFICATION.md`: the evidence table needs a row naming the derive
  tests (analysis I9; `AGENTS.md`: "Capability claims require executable
  evidence"), and line 37 "Five Cargo-verified archives" is a "five" site.
- `scripts/package-workspace.py:76` ("five-crate release set") and
  `scripts/test-verify-published.py:6,79` ("five-crate") are outside the grep set.

`REWRITE TO (line 100, add):`
> `docs/tool-calls/INTEGRATION.md` "Rust API" lists every new core `pub` item and the facade re-exports; `docs/QUALIFICATION.md` gains an evidence row for `crates/derive/tests/derive.rs`, the compile-fail doctests and `crates/host/tests/derive_facade.rs`, and line 37 reads "Six". The stale-text grep (`five crates|five-crate|five publishable|all five|each of the five`) also covers `scripts/package-workspace.py` and `scripts/test-verify-published.py`.

**6. Hygiene of generated locals (analysis I8) is not in the contract.** The
`FromStr` writer gives every generated token a `call_site` span (analysis risk
table, HIGH likelihood). A user item named like a generated local (`const node: u8`,
`struct members;`) turns `let node = ..` into a refutable constant pattern.
Plan row P3 (line 84) covers raw idents, Unicode idents and escapes only.

`REWRITE TO (new row):`
> I8 hygiene: generated locals use `Span::mixed_site()` (re-spanned after `FromStr`) or `__`-prefixed names, and every path is absolute (`::core::option::Option`, `::std::vec::Vec`, `::purrassist_core::..`). Derive test module declares `const node: u8 = 0; struct members; mod core {} mod std {}` beside a derived struct; it compiles and converts.

### ADVISORY

**7. Name the A10 location and widen CI.** `crates/host/tests/derive_facade.rs`
is valid: host depends only on `purrassist` (`crates/host/Cargo.toml:25-26`), so a
facade derive that emitted `::purrassist_core` fails to compile there. No fixture
crate is required. `verify.yml:54-55` (Windows job) tests neither `purrassist` nor
derive; add `-p purrassist-derive -p purrassist` since a proc-macro is
host-compiled. Run the MSRV check as `rust-validate 1.98.0 cargo test --workspace --locked`
to mirror `verify.yml:43` rather than two `-p` flags (plan lines 101, 132, 137).

**8. Define the snake_case algorithm.** Plan line 35 fixes only
`ReadFile → read_file`. State the rule (a `_` before an uppercase letter that
follows a lowercase letter or digit, or that precedes a lowercase letter while
following an uppercase one; then lowercase) and test `HTTPServer`,
`ReadFile2`, `IOError`, `r#type`.

**9. Unstated type/attribute combinations.** (a) `Option<T>` with `default`:
absence is the only source of `None` under the schema, so the combination is
contradictory; make it a `compile_error!` (or state fill-to-`Some`). (b) D-6
(line 61) says `repeat` is valid "only on `Vec<T>`"; say whether
`Option<Vec<T>>` (an optional repeatable parameter, representable in core) is
allowed; recommend yes. (c) `Vec<Option<T>>`: the blanket impls make it
compile, the schema never admits a `null` item, but `FromNode` would accept one
from a hand-made node; reject at expansion (prior-art §7.6) or document.
(d) D-7 `pattern` on `Vec<String>` must be applied to the item spec
(`Param::array(Param::string().pattern(..))`); `.pattern()` on the array spec
is `InvalidDefinition` at `ToolSet::compile`.

**10. Trait-driven required/absent handling.** Add `const REQUIRED: bool` and
`fn absent() -> Option<Self>` (or equivalent) to `ToolParameter`, `false`/`Some(None)`
for `Option<T>`; the derive then never needs to recognize `Option` syntactically,
type aliases (`type Maybe = Option<String>`) behave correctly, and only `Vec`
recognition remains (for `repeat` and `pattern`). This removes an analysis-listed
misclassification risk at no API cost.

**11. Emit the parameter list once.** `ToolDefinition::arguments` is `pub(super)`,
so `tool_definition()` and `object_spec()` each emit the full `.param(..)` chain.
A core builder `ToolDefinition::parameters(self, ObjectSpec) -> Self` (merging
`ObjectSpec::unknown` into the tool) halves generated code and keeps the two in
lock-step. Also state that the `extra` field's `json::Node` type is enforced by
the generated assignment (a rustc type error on the field), not by matching the
path text `json::Node`, which would reject `Node` or `purrassist::json::Node`.

**12. D-12 pure layer: feasible, with two notes.** Model spans as indices into a
span table so span selection is unit-testable without `proc_macro`; message
tests then prove the pure layer, and the paired `compile_fail` doctests prove
the macro emits them. Keep the derive's `Rustdoc` free of intra-doc links to core
items (`make doc` builds the lib without dev-dependencies; analysis I11).

## Rubric

| Item | Result |
|---|---|
| UNFALSIFIABLE CRITERION | Findings 2 (counter invisible to the test), 3 (conditional on a fact already known) |
| PRE-EMPTIVE DESCOPE | None. Generics, tuple structs, unions, data variants and recursion are spec-mandated or justified by the builder's lack of `anyOf`/`$ref`; external crates.io steps are correctly not claimed |
| UNMAPPED REQUIREMENT | Findings 5 (INTEGRATION.md Rust API, QUALIFICATION.md evidence row, two "five" sites), 6 (I8 hygiene) |
| PLANNED-BY-REFUSAL | None. Every `compile_error!` corresponds to a spec rejection or a recorded user decision |
| Design claims vs code | Finding 1 (D-5 not const, escaping contradiction), 4 (DEPTH needs forcing); D-1, D-2, D-10, D-11 verified correct; A10 feasible in `crates/host/tests` |

**Rubric result:** FINDINGS-OPEN

VERDICT: NEEDS-REVISION

## Orchestrator dispositions (pass 1)
- REQUIRED 1-6: rewritten verbatim from the review (D-5 lowering, the FN1 core-unit and duplicate-vs-missing tests, the D-8 FromNode-only range plus the `u8`/300 test, D-9 forced DEPTH evaluation with `const fn max_depth`, the doc rows and grep set, the I8 hygiene row). Task 1's bullets are updated to match.
- ADVISORY, adopted: A10 moved to `crates/host/tests/derive_facade.rs`; derive and facade added to the Windows CI job; MSRV run as `--workspace`; snake_case algorithm (D-14); `Option`/`Vec`/`pattern`/`extra` combinations and trait-driven required/absent (D-13); `extra` type enforced through a `ToolExtra` marker trait; D-12 spans modelled as indices; `make helper-check` added to Task 2's focused checks.
- ADVISORY, declined: a `ToolDefinition::parameters(ObjectSpec)` builder. The generated code can call `.param` per field from a single expansion without duplicating anything, and a new builder adds public API surface without a consumer.

## Recheck (pass 2)

Re-read the revised `plan.md` in full against the pass-1 findings and against
`from_node.rs` (trait shape, `Option<T>`/`Vec<T>`/scalar impls) and
`definition.rs` (`ParameterSpec` modifiers, `Param::array`).

### Prior findings

| # | Finding | Status | Note |
|---|---|---|---|
| 1 | D-5 default lowering | DISCHARGED | Rewrite present (plan line 57). Two text nits, not reopening: the trailing sentence "If a single `json::parse` of macro-serialized text is kept instead, ..." was a conditional for the plan author and now reads as a second permitted design; delete it. Contract row 81 still names `default_node`, which no longer exists; rename to "the `#[doc(hidden)]` number helper". |
| 2 | FN1 single-pass unfalsifiable | DISCHARGED | Row 93: core unit test with injected counter, derive test on duplicate-vs-missing, sorted table requirement. Both are executable from their real entry points. |
| 3 | D-8 undecided | DISCHARGED | Line 60 states the gap and the `u8`/300 test; Task 1 states "D-8 adds no core code". |
| 4 | D-9 DEPTH forcing | DISCHARGED | Line 61: `const _: usize = <Self as ToolParameter>::DEPTH;`, `const fn max_depth`, spanned-at-derive note, paired doctest. (The sentence "Direct self-reference keeps its spanned `compile_error!`" appears twice; cosmetic.) |
| 5 | Unmapped docs | DISCHARGED | Row 102 adds INTEGRATION.md "Rust API", the QUALIFICATION.md evidence row and line 37, and the two script files in the grep set. |
| 6 | I8 hygiene | DISCHARGED | Row 95, verbatim. |
| 7 | A10 location, CI, MSRV | DISCHARGED | Task 3 names `crates/host/tests/derive_facade.rs` and the Windows job; row 103 and Task 2 run `--workspace`. |
| 8 | snake_case | DISCHARGED | D-14 (see note below). |
| 9 | Type/attribute combinations | STILL-OPEN | Decided in D-13, but the mechanism contradicts D-13's own premise; see below. |
| 10 | Trait-driven required/absent | DISCHARGED | D-13 `const REQUIRED`, `fn absent()`; checked against `FromNode` below. |
| 11 | `parameters(ObjectSpec)` builder; `extra` type enforcement | DISCHARGED | Builder declined with a reason; advisory, accepted. The `.param` chain is still emitted twice in the output (once per `tool_definition()` and `object_spec()`), from one generator loop, which is what the disposition means. `ToolExtra` marker resolves the `extra` half. |
| 12 | D-12 spans as indices | DISCHARGED | Line 73. |

### New text

**D-13 against `FromNode` and the scalar impls: coherent.**
`ToolParameter: FromNode` adds `const REQUIRED: bool` and `fn absent() -> Option<Self>`.
With `impl<T: FromNode> FromNode for Option<T>` mapping only `null` to `None`
(`from_node.rs:152-159`), absence has to be handled by the generated code, and
`absent()` is exactly that hook: `Some(None)` for `Option<T>`, `None` (then a
`missing` error) for every scalar, `Vec<T>`, derived struct and enum. `REQUIRED`
is `false` only for `Option<T>` and selects `.required()` in `tool_definition()`
/ `object_spec()`; a field with `default` omits `.required()` regardless, which
the derive knows syntactically. Nothing in `from_node.rs` conflicts: the twelve
integer impls, `String`, `bool`, `Number`, `f64` are all "required, no absent
value".

**`Option<T>` + `default` holding `Some(default)`: coherent.** The default node
is a `T` value (a `null` default fails `ToolSet::compile` against the declared
type), so `<Option<T> as FromNode>::from_node(default)` is `Some(default)`.
Absent → `Some(default)`; present → `Some(value)`; `None` arises only from an
explicit `null` in a hand-made node, which the schema never admits from
recovery. That is a consistent, if redundant, field type. The derive's Rustdoc
for `default` should say so in one sentence ("`Option<T>` with a default is never
`None` for recovered arguments").

**D-13 STILL-OPEN: item access contradicts "never recognises `Option` syntactically".**
Plan line 64 says the derive "never recognises `Option` syntactically", yet
three D-13 bullets need the item type beneath `Option` and/or `Vec`:

> - `Option<Vec<T>>` + `repeat`: the item `T` is repeatable.
> - `Vec<Option<T>>`: rejected (`Option` is not a JSON item type).
> - `pattern` on `Vec<String>` or `Option<String>`: applies to the string item.

With trait dispatch alone, `<Option<Vec<T>> as ToolParameter>::parameter()` is
the array spec; `.repeatable()` on it is `InvalidDefinition` (`definition.rs:203-237`),
and `.pattern()` on an array spec is likewise rejected (`definition.rs:274-291`).
`Vec<Option<T>>` compiles because `Option<T>: ToolParameter` satisfies the
blanket `Vec<T: ToolParameter>` bound. No mechanism is named for any of the three.

`REWRITE TO (choose one, state it in D-13):`
> (a) Trait-driven: `ToolParameter` also carries `type Item: ToolParameter` (`Self` for scalars, enums and derived structs; `T` for `Vec<T>`; `T::Item` for `Option<T>`) and `fn wrap(item: ParameterSpec) -> ParameterSpec` (identity except `Vec<T>`, which returns `Param::array(item)`). The derive emits `F::wrap(F::Item::parameter().pattern(..))` for `pattern`, `F::Item::parameter().repeatable()` for `repeat`, and `const _: () = assert!(<F as ToolParameter>::IS_ARRAY, "repeat requires Vec<T>");` for the `repeat` check. `Vec<Option<T>>` is rejected by bounding the array impl on a `ToolItem` marker trait that `Option<T>` does not implement (rustc "trait not implemented", spanned on the field type). Or
> (b) Syntactic for the two modifiers only: the parser peels `Option<..>` and `Vec<..>` solely to locate the item type for `repeat` and `pattern` and to reject `Vec<Option<..>>`; required/absent stay trait-driven (D-13). `repeat` on a type whose outermost non-`Option` layer is not `Vec` is a `compile_error!` at the key.

Either is coherent; (a) keeps D-13's promise for type aliases, (b) keeps the
spanned `compile_error!` the P2 row already lists for `repeat` on non-`Vec`.
The P2 row must then name `Vec<Option<T>>` as a rejection case with its twin.

**D-14: DISCHARGED, one clarification.** The rule covers lower→upper and
acronym ends; say what a digit→upper boundary does (`File2Reader` →
`file2_reader` or `file2reader`) and add D-14's own examples (`HTTPRequest`,
`ReadFile2`) plus `r#type` to the "User decisions: naming" test row (91), which
still lists only `ReadFile`. The fall-through to `compile_error!` when the
result fails core name validation (non-ASCII idents) is correct.

**Task 1 edits: DISCHARGED.** Contents now list `REQUIRED`/`absent()`,
`max_depth`, the number helper, and "D-8 adds no core code". If option (a)
above is chosen, add `type Item`, `wrap`, `IS_ARRAY` and `ToolItem` to this list
and to the INTEGRATION.md "Rust API" entry.

**Task 3 edits: DISCHARGED.** Host-crate test with the dependency fact stated;
Windows CI job; focused check `cargo test -p purrassist-host --test derive_facade --locked`.

### Rubric (pass 2)

| Item | Result |
|---|---|
| UNFALSIFIABLE CRITERION | None remaining |
| PRE-EMPTIVE DESCOPE | None |
| UNMAPPED REQUIREMENT | None remaining |
| PLANNED-BY-REFUSAL | None |
| Design claims vs code | One internal contradiction in D-13 (item access for `repeat`/`pattern`/`Vec<Option<T>>`); everything else verified |

Open: finding 9 (D-13 item mechanism). Nits, not blocking: D-5 alternative
sentence, row 81 `default_node` label, D-14 digit rule and test examples.

**Rubric result:** FINDINGS-OPEN

VERDICT: NEEDS-REVISION

## Recheck (pass 3)

Scope: finding 9 and the pass-2 nits only. Re-read plan lines 42-74 (D-1 to
D-14), row 82, row 85 (P2), row 92 (naming), and Task 1 (lines 114-128).

### Finding 9: DISCHARGED

D-13 (lines 64-70) adopts option (a) and is coherent against the core code:

- **`ToolItem` with the bound `Vec<T: ToolItem>`.** `ToolItem` is implemented
  for every `ToolParameter` type except `Option<T>`, so `Vec<Option<T>>` fails
  with rustc's "trait not implemented" on the field type, and `Vec<Vec<T>>`
  (arrays of arrays, which core's `Param::array` admits) still compiles. Since
  `ToolItem: ToolParameter: FromNode`, the existing `impl<T: FromNode> FromNode for Vec<T>`
  (`from_node.rs:161-174`) satisfies the `ToolParameter: FromNode` supertrait.
  No negative impl is needed because `ToolItem` is opt-in; the derive must emit
  `impl ToolItem for Self` beside `ToolParameter` for every derived struct and
  enum (implied by "every `ToolParameter` type"; stated below as a nit).
- **`ToolRepeatable::repeated()`** for `Vec<T>` and `Option<Vec<T>>` returns
  `T::parameter().repeatable()`, a non-array spec, which is exactly what
  `ParameterSpec::repeatable` requires (`definition.rs:203-237`: "repeatable
  only on non-array parameters"). The field's own `FromNode` (`Vec<T>` /
  `Option<Vec<T>>`) reads the array value; `REQUIRED` is `true` for `Vec` and
  `false` for `Option<Vec>`; `absent()` is `None` / `Some(None)`. `repeat` on
  any other type is a missing-trait error spanned on the field type. Coherent.
- **`ToolPattern::with_pattern(&'static str)`** for `String`, `Option<String>`
  and `Vec<String>`; for `Vec` it returns `Param::array(Param::string().pattern(p))`.
  An array item may carry `pattern` (only required/alias/repeatable/default are
  forbidden on items, `definition.rs:129-133`), and `pattern` applies to plain
  strings only (`definition.rs:274-291`), so both shapes compile under
  `ToolSet::compile`. Coherent.
- **`ToolExtra`** for `json::Node`: enforces the `extra` field's type by trait
  resolution. Coherent.
- **`Option<T>` + `default`** (line 70): states `Some(default)`, the `null`
  source of `None`, and the doc sentence. Matches the pass-2 analysis.
- **P2 row 85** lists `repeat` on non-`Vec`, `pattern` on a non-string and
  `Vec<Option<T>>`, each marked "(rustc trait error)" with a passing twin.
- **Row 82** lists `ToolItem`, `ToolRepeatable`, `ToolPattern`, `ToolExtra`
  under Task 1 with unit tests and doctests per public item; row 102's
  "every new core `pub` item" covers them in INTEGRATION.md.

### Pass-2 nits: all fixed

- D-5 (line 57) ends at the `"x"`-for-`u32` test; the alternative
  `json::parse` sentence is gone.
- Row 82 names "the number helper", not `default_node`.
- D-14 (line 71) adds the digit→upper rule with `Read2File` → `read2_file`;
  row 92 tests `ReadFile`, `HTTPRequest`, `ReadFile2`, `Read2File` and
  `r#type` → `"type"`.

### Residual editorial inconsistencies (not blocking, no user decision needed)

These are stale sentences that D-13 supersedes; the implementer should read
D-13 as authoritative. Listed so the plan can be tidied in the same edit that
posts it:

1. D-1 line 45 still says `Vec<T: ToolParameter>`; D-13 line 66 makes the bound
   `Vec<T: ToolItem>`. Change D-1 to match.
2. D-6 line 58 still says `repeat` "generates `<T as ToolParameter>::parameter().repeatable()`
   ... On any other type it is a `compile_error!`". Under D-13 it emits
   `<F as ToolRepeatable>::repeated()` and misuse is a rustc trait error (as
   row 85 already says). Replace D-6's body with "see D-13 `ToolRepeatable`".
3. D-13 line 64 says the derive "never inspects field types syntactically";
   D-9 line 61 still scans the field type tokens for the struct's own ident to
   give direct self-reference a better span. Both are fine together; say
   "never classifies field types syntactically (D-9's own-ident scan is the one
   exception, for span quality only)".
4. Task 1 contents (line 123) list only `REQUIRED`/`absent()` for D-13; add
   `ToolItem`, `ToolRepeatable`, `ToolPattern`, `ToolExtra` so Task 1 matches
   row 82.
5. Throughout, `fn parameter() -> Param` / `fn repeated() -> Param` /
   `fn with_pattern(..) -> Param`: the spec type is `ParameterSpec`
   (`definition.rs:146`); `Param` is the unit struct of constructors
   (`definition.rs:309`). Write `-> ParameterSpec`.
6. Implementer note: each derive emits `impl ToolItem for Self` in addition to
   `ToolParameter` (and `ToolArguments` for structs), otherwise a derived type
   cannot appear inside `Vec<..>`.

### Rubric (pass 3)

| Item | Result |
|---|---|
| UNFALSIFIABLE CRITERION | None |
| PRE-EMPTIVE DESCOPE | None |
| UNMAPPED REQUIREMENT | None |
| PLANNED-BY-REFUSAL | None |
| Design claims vs code | D-13 option (a) verified against `from_node.rs` and `definition.rs`; no contradiction remains beyond the six editorial leftovers above |

**Rubric result:** PASS

VERDICT: PASS
- Pass 3 residual editorial items applied: `ToolItem` in D-1, D-6 rewritten to the trait path, the D-9 carve-out, `ParameterSpec` return types, derives emit `impl ToolItem`, Task 1 lists all traits.
