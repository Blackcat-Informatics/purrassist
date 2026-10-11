# R4 review: crate path override and versioned core marker

Reviewed: uncommitted worktree on HEAD d46b996 (PR #19, issue #15). Spec:
remediation-plan.md "### R4" and Declined, structure-review F-5, gap-analysis G5, AGENTS.md.
No source modified; only this file written.

## Scope read
core `derive_support.rs`/`mod.rs` (marker + test); derive `parse.rs` (`crate_path`, KEYWORDS,
`tool_attrs`), `model.rs` (`CratePath`), `write.rs` (`ABI_MARKER`, `generate`), `lib.rs` (docs,
doctests 47-50), README; facade `derive_facade.rs`; INTEGRATION.md; new `tests/crate_override.rs`.

## Evidence
| Command | Result |
|---|---|
| `cargo test -q -p purrassist-core -p purrassist-derive -p purrassist --locked` | all `test result` lines ok, none with failures |
| `cargo test -q --doc -p purrassist-derive --locked` | 50 passed (47-50 and their twins included) |
| `cargo clippy -q --workspace --all-targets --locked -- -D warnings` | exit 0, no output |
| `cargo fmt --check` | clean |
| `make hygiene` | exit 0 |
| `make helper-check` | exit 0 |
| `python3 scripts/check-repository.py` | exit 0 |
| `python3 scripts/check-release.py` | exit 0 (`^` requirements unchanged: diff touches no Cargo.toml) |

### Findings by claim
- **`crate =` mechanism:** `generate` builds `tc`/`js` from the override or `flavor.root()`;
  the marker line uses `writer.tc`. In non-test code the only `::purrassist_core` /
  `::purrassist::__core` literals are `Flavor::root` (write.rs:40-41), so nothing bypasses the
  override. Unit test `the_crate_override_replaces_the_prefix_of_both_flavors` checks every
  `::tool_call::`/`::json::` occurrence is preceded by the override, for both flavors and both
  derives. Accepted on enums too (`Site::Enum` list); rejected on `#[param]` (test
  `crate_is_not_a_field_or_variant_key`).
- **Errors:** `malformed_crate_paths_are_spanned_errors` covers empty, trailing `::`, bare `::`,
  literal, generics, `a:b`, `a b`, misplaced `crate`, keywords, `Self`, with exact messages and
  spans; duplicate in one attribute, split attributes and enum covered. Doctests 47 (trailing
  `::`), 48 (literal), 49 (duplicate), 50 (E0433 unresolved) each have a passing twin.
- **Integration tests:** `crate_override.rs` covers re-export module, `use` alias, absolute,
  `super::`, `self::`, mixed nesting with default-derived tools, structs and enums, and compares
  against a hand-built definition. Facade test covers `::purrassist`, `::purrassist::__core`
  and a re-export of the facade. SPDX header present on the new file.
- **ABI marker:** core has `#[doc(hidden)] pub const __DERIVE_ABI_0_1`, re-exported in
  `tool_call` and (via the glob) the facade. Derive `ABI_MARKER` is `concat!` of
  `env!("CARGO_PKG_VERSION_MAJOR")`/`MINOR`; every expansion starts with
  `const _: () = {tc}::{ABI_MARKER};` (test asserts prefix and exactly one occurrence for
  4 flavor/target pairs plus an override). Core test compares `stringify!` with
  `concat!`/`env!`, as the plan requires.
- **Marker error readability (simulated):** copied core to /tmp/r4chk/core2 with the marker
  renamed to `_0_2`, built a dependent with derive. Result: one error,
  `error[E0425]: cannot find value `__DERIVE_ABI_0_1` in module `::pa::tool_call`` pointing at the
  derive, plus a note "similarly named constant `__DERIVE_ABI_0_2`". It names the marker and
  hints at the version. Readable; matches the docs' claim.
- **Renamed Cargo dependency (out-of-tree, performed by me):** /tmp/r4chk/app with
  `pa = { package = "purrassist-core", path = <worktree>/crates/core }` plus a path dependency
  on purrassist-derive, `#[tool(crate = ::pa)]` on a `ToolEnum` and a `ToolDefinition`.
  `cargo run` printed `Ok(Run { path: "x", mode: Some(Slow) }) run`: compiles and converts.
  Same program with the attribute removed fails with `E0433: cannot find `purrassist_core` in
  the list of imported crates`, so the attribute is what makes the rename work. Judgment: the
  plan allows "a test crate with a renamed dependency, or a re-export module path"; the in-tree
  re-export/alias tests meet that wording, and the out-of-tree check confirms the real
  renamed-key case. The in-tree limitation (Cargo rejects one package under two names) is
  correctly disclosed in the test header and INTEGRATION.md. Not a blocker. (Not committed as a
  test; the PR body should cite it as manual evidence.)
- **Skew:** not testable in-tree per the plan; docs say so (INTEGRATION "Proved by"), and I
  simulated it above only as manual evidence.
- **G5 docs:** derive README, `lib.rs` "Using the derives" and INTEGRATION.md each state the
  same major.minor rule (patch may differ, caret requirements deliberate), that the dependency
  must have the default name or `crate =` be used, and "convert only usable calls".
  Verified names exist: `ToolCalls::usable`, `ToolCalls::usable_accepting_guesses`,
  `ToolCall::is_usable` (recover.rs:1526,1692,1714), `CallOutcome::Resolved`. Claims
  (facade re-exports `tool_call`/`json` at root, `::purrassist::__core`, errors at offending
  token, unresolved path is rustc's error) match tests and the simulation.
- Declined items respected: no `=` pins, `Members`/`Slot` still public, no Cargo.toml changes.

## Non-blocking observations
1. The renamed-dependency proof lives only in this review; consider citing it in the PR body.
2. The `__DERIVE_ABI_0_1` name is hand-maintained; the core test makes a stale name fail,
   which is the intended guard.

No missing required behavior found.

VERDICT: PASS
