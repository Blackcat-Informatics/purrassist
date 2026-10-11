# T2 review: purrassist-derive crate

## Scope
Uncommitted changes vs 0fb0454: crates/derive/**, root Cargo.toml and Cargo.lock, scripts/check-release.py (RELEASE_ORDER), .github/workflows/publish.yml (loop and step name). Read-only review; no `make check`.

## Evidence
- `cargo test -q -p purrassist-derive --locked`: pass (unit and integration). `--doc`: 43 doctests pass.
- `cargo clippy -q --workspace --all-targets -- -D warnings`: exit 0 (one cargo lint warning, see A-1).
- `make hygiene` exit 0; `make helper-check` exit 0; `python3 scripts/check-release.py` exit 0 and lists core, derive, syntax, purrassist, host, cli. publish.yml loop and step name match that order.
- Zero deps: `cargo metadata --locked --no-deps` shows purrassist-derive with one dependency, kind dev, purrassist-core. No normal/build deps; no syn/quote; src uses only std and proc_macro. Cargo.lock diff adds only the purrassist-derive package (dependency purrassist-core).
- compile_fail reasons: extracted all 40 compile_fail and 40 twin snippets from `mod rejected_input` into a scratch crate and ran `cargo check` per bin. Every twin compiles; every compile_fail fails for the intended reason (spanned compile_error! messages for generics/where/tuple/union/keys/enum-vs-struct/variant data/empty enum/duplicate value/defaults/extra misuse/self-reference/name/duplicate param; E0277 for non-Node extra, Vec<Option>, Option<Option>, repeat on String, pattern on u8, unsupported type; E0391 cycle for indirect recursion). Snippets 30 and 32 also show a secondary rustc error (E0308, E0072) next to the intended one; harmless. Twins differ only in the offending construct (twin 24 changes both default token and field type; twin 2 changes the field type too; both acceptable).
- Expansion inspected with `-Zunpretty=expanded` (RUSTC_BOOTSTRAP=1): all paths absolute (::purrassist_core, ::core, ::std), locals `__pa_*` with mixed_site, single pass over `Members` (no Node::get), sorted name table plus order index emitted at expansion (deterministic, no hash maps), nested `.within(key)`, defaults lowered via `__derive_number` and converted with `from_node` in the absent arm, extra collected via `Slot::Undeclared`, `const _: usize = DEPTH` forced. Unit struct expands. No unwrap/expect/panic in generated code or non-test macro code except three `unreachable!` sites in parse.rs guarded by the empty-value rejection (A-3).
- Facade flavor: unit-tested in write.rs (paths equal the core flavor with root replaced by `::purrassist::__core`, and no core path leaks). End-to-end is Task 3.
- Name check: core's `is_valid_name` is non-empty ASCII alphanumeric plus `_ . : -`. An identifier holds only letters, digits and `_`, so for snake_case output the derive's check (non-empty and ASCII) is exactly equivalent: nothing core rejects can pass the derive. Non-ASCII output gets a compile_error! (snippet 39).

## Adjudication of open points
1. Unit/empty structs as zero-parameter tools: ACCEPT (tested, documented).
2. repeat + pattern combinable: ACCEPT (trait-checked; the generated block asserts the ToolPattern bound).
3. Enums accept only `#[tool(description)]`; variant doc comments ignored: ACCEPT with a note (A-2). It deviates from the user's wording ("structs, fields, enums and variants") because `Param::enumeration` has no per-value description; documented in lib.rs. The PR description should state this explicitly.
4. Self-reference scan skips `::`-qualified paths: ACCEPT. `Vec<crate::A>` still fails to compile (E0391) and `Option<crate::A>` (E0072); no silent success.
5. Empty enum, duplicate values, duplicate param names rejected: ACCEPT (twins pass).
6. Non-ASCII derived tool name requires explicit name: ACCEPT, equivalent to core as shown above.

## Findings
REQUIRED: none.

ADVISORY
- A-1: `cargo clippy` warns "unused workspace dependency purrassist-derive" (cargo::unused_workspace_dependencies) until Task 3 uses it from the facade. It does not fail `-D warnings` (cargo lint), but confirm the commit hook output is not treated as failure; consider that Task 3 resolves it.
- A-2: Variant doc comments ignored (see adjudication 3); surface in PR as a deliberate deviation.
- A-3: Three `unreachable!` in parse.rs (lines ~357, 387, 522) contradict bridge.rs's "Nothing here panics on any input". Invariants look sound (empty values rejected at lines ~301/553/574 before recursion), but returning a Diagnostic would make the claim unconditional.
- A-4: Explicit `name = ".."` on a tool is not checked with the core name rule at expansion; an invalid explicit name surfaces as `ToolSetError::InvalidDefinition` at `ToolSet::compile` (runtime), not compile time. Acceptable, but the docs only promise the derived-name check; consider a one-line mention.
- A-5: `__PA_NAMES` and `__PA_ORDER` consts are uppercase so skip the `__pa_` mixed_site re-span; they are function-local and collision-free in practice.
- A-6: `expansion_is_a_pure_function_of_the_input` compares two identically declared structs; it is a weak determinism check (the sort is deterministic by construction). Fine.
- A-7: rustdoc compile_fail error codes (E0277, E0391) are only enforced by nightly rustdoc; reasons verified manually here.

VERDICT: PASS

## Recheck

- (a) `is_valid_name` const fn rewrite: logic is the same rule (non-empty, every byte ASCII alphanumeric or `_ . : -`; there is no length limit). Compared old and new in a scratch crate: every Unicode scalar value 0..=0x10FFFF alone, prefixed, suffixed and embedded, the empty string, 2M random strings over a mixed alphabet (including space, NUL, DEL, `/`, `@`, `[`, digits, `-`, `.`, `:`), and a 1M-byte name: 6,448,258 cases, all equal; a `const` evaluation also works. No length limit exists in either version.
- (b) `cargo test -q -p purrassist-core --locked`: all suites pass; core diff is only `is_valid_name` (const fn), `__valid_name` (doc(hidden), delegates to it, doctest) and its re-export. No golden or test changed.
- (c) New compile errors checked as standalone crates: invalid tool name, invalid parameter alias, Unicode field name without `#[param(name)]` and empty alias each fail with E0080 "evaluation panicked: ... is not valid: use ASCII letters, digits and `_ . : -`", spanned at the derive; the twins (valid name, valid alias, `#[param(name = "e")]`) produce no error other than my scratch crates' missing `main`. The three `unreachable!` are gone (grep finds none). Derive tests (27 unit, 24 + 45 integration/doc, 42) pass, clippy has no errors, `make hygiene` exits 0.
- A-4 and A-3 from the first review are resolved. Remaining advisories are A-1, A-2, A-5 to A-7 unchanged (non-blocking).

VERDICT: PASS
