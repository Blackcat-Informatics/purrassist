DRAFT — not final. Stage 3 finalizes after its binding gates (merge-tree, review debt, merge-ready).

purrassist-derive: zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)] (#15)

Adds purrassist-derive, a proc-macro crate with no dependencies (no syn or
quote), so tools fixed at compile time declare their schema and typed
arguments with one Rust struct. Derived definitions equal hand-built ones;
the generated FromNode converts recovered arguments in a single pass with
exact FromNodeError pointers. Re-exported through the purrassist facade.

User decisions: snake_case tool names, field idents, verbatim variants;
defaults filled in FromNode; unknown = pass collects into an `extra` field;
explicit attributes win over doc comments; nested objects without an
`unknown` key drop undeclared members with a recorded loss.

Core support (purrassist_core::tool_call): ToolArguments, ToolParameter,
ToolItem, ToolRepeatable, ToolPattern, sealed ToolExtra; Members/Slot
(#[non_exhaustive]); FromNodeError constructors; f32 and Box<T>; doc-hidden
__sorted, __valid_name and the __DERIVE_ABI_<major>_<minor> marker.

Derive: #[tool(crate = path)] override; hygienic generated code; spanned
compile errors for every rejected construct, each pinned by a compile_fail
doctest with a passing twin.

Release: order core -> derive -> syntax -> purrassist -> host -> cli; a
publish preflight requires every release crate to exist on crates.io with
purrassist-core's owners before authentication; tested offline.

Validation: TO FINALIZE IN STAGE 3 (Stage 2: make check, make package,
workspace MSRV 1.98.0, make wasm-test all exit 0 on d0a99e9; later commits
prose-only). Hosted CI: did not run.

Residual: bridge span selection covered only by passing twins; version skew
caught at user compile (caret requirements); maintainer must reserve the
purrassist-derive name and configure trusted publishing before tagging.

Defect-Class: TODO (Stage 3)
Plan: .stage/purrassist-derive-zero-dependency/plan.md and remediation-plan.md
