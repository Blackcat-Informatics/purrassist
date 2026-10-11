# R3 independent review

Scope: uncommitted diff over HEAD 64097ec (9 files: core derive_support.rs, from_node.rs, tool_call/mod.rs; derive lib.rs, model.rs, write.rs, tests/derive.rs; INTEGRATION.md; helpers-ledger.toml), against remediation-plan.md "### R3" plus R6 A-1 (core part and generated-code switch). No source modified.

## Commands and results
- `cargo test -q -p purrassist-core -p purrassist-derive -p purrassist --locked`: every suite and doctest `ok`, 0 failed (includes the new E0391/E0277 compile_fail doctests, which rustdoc verifies by error code, and their twins).
- `cargo clippy -q -p purrassist-core -p purrassist-derive --all-targets --locked -- -D warnings`: clean.
- `make hygiene`: exit 0. `make helper-check`: OK (3 `[[distinct]]` rows each equal one group). `python3 scripts/check-repository.py`: OK (core/derive boundaries verified).
- Standalone crate in /tmp (path deps on core and derive, removed afterwards):
  - `#[param(extra)] rest: String` gives exactly ONE error: `E0277 the trait bound String: ToolExtra is not satisfied` (no E0308). A-1 acceptance holds.
  - `struct A { a: Vec<f32>, b: Option<Box<f32>>, #[param(extra)] rest: json::Node }` with `unknown = pass`: `{"a":[0.1,2],"z":1}` converts (a=[0.1,2.0], b=None, rest holds `z`); `{"a":[1e39]}` gives `/a/0`, expected "number within the finite f32 range", found "number 1e39". f32 ToolItem, array and Option<Box> paths work end to end.

## Spec check (all present)
- F-4: `#[doc(hidden)] pub const fn __sorted` (strict byte order, const loop, doctest and unit test incl. agreement with `str` ordering), re-exported hidden; generated `const _: () = ::core::assert!(__sorted(__PA_NAMES), "{}", "...")` (message as an argument, consistent with R1); write.rs unit test pins it. `Members` docs now say derived code uses per-field slots and `absent()`, `is_present` for hand-written code. Debug asserts retained.
- F-5/A-1: private `sealed::Sealed` supertrait, `ToolExtra: Sealed + Sized` with `#[doc(hidden)] __from_members`; impl for Node only; docs say sealed and not an extension point; compile_fail,E0277 doctest for a downstream impl. The old `extra_check` const block is removed; generated code uses `<ty as ToolExtra>::__from_members(__pa_extra)`.
- `#[non_exhaustive] Slot`; generated match ends in `_ => return Err(FromNodeError::mismatch(...))` (no panic; test asserts no `panic!`). Core doctests carry wildcard arms (hand-written example returns an error; the Members doctest uses `unreachable!`, acceptable in a doctest). No other external Slot matcher exists (grep; resolve.rs has an unrelated private Slot).
- f32: `Param::number()`; `float_from_node!` parses with `str::parse::<Self>` directly, filters non-finite, expected text exactly "number within the finite f32 range". Tests: double-rounding contrast (value just above an f32 midpoint), `-0`, MAX, underflow to 0, overflow, and derive-level test. Docs say write exact, read round-to-nearest.
- `Box<T>`: FromNode, ToolParameter and ToolItem forwarding REQUIRED/absent/DEPTH; docs state no recursion; compile_fail,E0391 doctests with passing twins both in core and in derive (lib.rs case 46).
- ToolParameter defaults (`REQUIRED = true`, `absent() = None`, `DEPTH = 0`) with the REQUIRED/absent relation documented, a hand-written minimal impl doctest, and scalar/Vec macros pruned accordingly.
- INTEGRATION.md API list updated (f32, Box, defaults, sealed ToolExtra, `__sorted`, non_exhaustive Slot, Members behavior). derive lib.rs type table updated.

## Judgments
- Sealing soundness: the sealed trait and `__from_members` are `pub` in a public trait, so generated code in a downstream crate can name `<ty as ToolExtra>::__from_members` (verified by the standalone build). Downstream cannot implement it (E0277 doctest). Sound. Residual: the method is visible to downstream callers as a hidden method; acceptable and documented as generated-code-only.
- Wildcard message: `mismatch("a member classification known to this derive (internal error: rebuild against a matching purrassist-derive)", node)` renders as expected/found of the object; accurate for a derive/core skew, and it does not panic.
- F-6b placement: the plan said derive's `tests/` dev-depending on core, but `is_json_number` is crate-private in a proc-macro crate, which cannot be linked into an integration test. A `#[cfg(test)]` unit test in model.rs using core (already a dev-dependency; the check-repository boundary check still passes) as the oracle is the only way to call the real validator, so it closes F-6b soundly and is stronger than a corpus-only test: hand corpus (including unicode digits, whitespace, NaN/inf, leading zeros, bare exponents) plus an exhaustive sweep of ~800k+ strings over the 7-character grammar alphabet up to length 7, asserting agreement and that core preserves the spelling. Deviation from the plan's wording is justified and should be noted in the PR body.
- helpers-ledger: the `float_from_node!` macro genuinely merges the former f64 body with the new f32 one (removes duplication, not papering over). The `[[distinct]]` row (kind shim, Box::parameter vs Option::parameter) is a reasonable declaration: both are trait impls on different wrappers whose shared body (`T::parameter()`) cannot be shared, and the associated items differ by contract. It is somewhat generic (the checker's grouping fires on two one-line forwarders) but the reason text is specific and checkable; helper-check passes with exactly one group per row. Not a blocker.

## Findings (none blocking)
- N1 (cosmetic): INTEGRATION.md new list items contain a few lines over 80 columns (e.g. the `FromNode ... Box<T: FromNode>. The marker ...` run-on); G9-style rewrap can fold into R6.
- N2 (cosmetic): derive tests comment in `every_mapped_type_equals_the_hand_built_definition` wraps awkwardly across two comment lines.
- N3 (note): the `Members` doctest wildcard uses `unreachable!`; fine for a doctest, but the prose elsewhere says generated code never panics, which remains true.

VERDICT: PASS
