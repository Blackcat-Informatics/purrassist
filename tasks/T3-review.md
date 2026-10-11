# T3 review

1. Re-exports: `#[doc(hidden)] pub use purrassist_core as __core;` and `FacadeToolDefinition as ToolDefinition`, `FacadeToolEnum as ToolEnum`, as D12. Downstream /tmp crate with `use purrassist::*;` plus `purrassist::tool_call::ToolDefinition` type and both derives compiled (only a dead-code lint from my scratch struct, under -D warnings). Macro and type namespaces do not collide.
2. `cargo tree -p purrassist-host -e normal,dev --depth 1`: only `purrassist`; no core. Host test therefore proves `::purrassist::__core`.
3. Tests: derived == hand-built definition via facade paths; compile/recover(Hermes)/from_node pipeline; pointer-bearing conversion error; no `crate =` anywhere.
4. Cargo.lock diff: single added `purrassist-derive` edge on `purrassist`.
5. verify.yml: valid YAML, new step `cargo test -p purrassist-derive -p purrassist --locked` in the Windows job.
6. Doctests (2 new) pass. `cargo test -p purrassist` ok, host derive_facade ok (1), clippy -D warnings clean, `make hygiene` ok.
Expected failures: check-packages.py (missing packaged purrassist-core .crate) and test-verify-published.py (purrassist-derive-9.9.9 archive not packaged/registered) are Task 4 registration/packaging, not Task 3 defects.

VERDICT: PASS
