# T1 review: core support for purrassist-derive

## Scope
Uncommitted diff vs fccfbe7: new `crates/core/src/tool_call/derive_support.rs`, edits to `from_node.rs` and `tool_call/mod.rs`. Spec: plan D-1, D-2, D-3, D-5, D-8, D-9, D-13, Task 1, FN1.

## Evidence
- `cargo test -q -p purrassist-core --lib derive_support`: 6 passed.
- `--doc derive_support`: 12 passed (includes compile_fail and should_panic cases). `--doc from_node`: 4 passed. `--lib from_node`: passes.
- `python3 scripts/check-repository.py`: OK (core boundary). `make helper-check`: OK.
- `cargo clippy -p purrassist-core --all-targets --locked -- -D warnings`: clean.
- from_node.rs diff is refactor-only: `mismatch` and `within` go private to pub with docs, three constructors added through one private `member` helper. No existing error text changed; no golden/test file modified (diff touches only the 3 source files).
- No ambient-access tokens (only `core::cell`/`core::str`); no new dependencies.

## Item checks
1. Present with planned semantics: ToolArguments, ToolParameter (parameter unrequired, REQUIRED, absent, DEPTH), ToolItem, ToolRepeatable (Vec<T>, Option<Vec<T>>, = `T::parameter().repeatable()`), ToolPattern (4 impls), ToolExtra (Node), const fn max_depth, `__derive_number` (doc(hidden), `# Panics` documented, should_panic doctest), FromNodeError::{mismatch,missing,duplicate,unknown,within}, Members/Slot.
2. Impls: String, bool, f64, Number, and all 12 integer types. Option<T: ToolItem> gives REQUIRED false and absent `Some(None)`; this rejects Option<Option<T>> (stricter than plan's `T: ToolParameter`, which would have allowed it; correct per D-13). Vec<T: ToolItem> rejects Vec<Option<T>>. DEPTH forwards for Vec/Option. Members: first occurrence Declared, later Duplicate, unknown Undeclared, is_present from `seen`. Binary search is correct (empty table, byte-order `str::cmp`, early exit); unsorted table and out-of-range order debug-assert; length mismatch asserts always. Pointer escaping goes through `pointer_segment` in `member` and `within`.
3. FN1 test: 64 names, 64 members in scrambled order, asserts visited == 64 and comparisons <= 384 (and >= 64). A linear scan averages roughly 2000 comparisons, so it would fail. Meaningful. A second test bounds undeclared lookups.
4. Rustdoc: every public item has an example; the module doc, ToolArguments hand-written impl (compiles, validates against schema, round-trips) and Members example run. Error semantics documented on `new`, `is_present`, and the constructors.
5. Practicality: a hand-written ToolArguments + FromNode over Members works (the ToolArguments doctest does exactly this). Extra collection is feasible: `Slot::Undeclared(&Member)` plus `Member: Clone` and `Node { value: Value::Object(Vec<Member>), span: None }` with pub fields, so generated code can build the extra Node. Defaults and absent handling are covered by `is_present(i)` plus `T::absent()`. Duplicate of a declared field is surfaced as a slot, so the generated code decides the error.

## Findings
REQUIRED: none.

ADVISORY:
- A1. `Members` docs say `new` "checks that the tables describe the same fields" and that `order` is "a permutation", but only the length (always) and `index < len` (debug) are checked; duplicate indices in `order` pass. Either add a debug permutation check or soften the wording.
- A2. Tests do not cover: non-ASCII or multi-byte key ordering in the table, pointer escaping (`~`, `/`) specifically for `missing`/`duplicate`/`unknown` (only `within` is shown in a doctest), the unsorted-table debug panic (`#[should_panic]`/debug_assertions), or a compile_fail for `Option<Option<T>>`/`Vec<Option<T>>` as ToolParameter.
- A3. Plan says Option impl is for `T: ToolParameter`; implementation uses `T: ToolItem` (an improvement). The plan/derive docs for Task 2 should reflect it.

VERDICT: PASS

## Orchestrator disposition
- A1: fixed. `Members::new` checks in debug builds that `order` is a permutation of 0..len; the docs state exactly what is checked in release and in debug builds.
- A2: fixed. Tests cover non-ASCII byte ordering, `~0`/`~1` escaping in missing/duplicate/unknown, the unsorted-table and non-permutation debug panics, and compile_fail doctests for `Option<Option<T>>` and `Vec<Option<T>>`.
- A3: fixed. The `Option<T>` impl documents its `T: ToolItem` bound.
- Rerun: `derive_support` lib (10), core doctests, clippy, fmt, helper-check and check-repository pass (implementer); `make hygiene` exit 0 and the full core test crate pass (orchestrator).
