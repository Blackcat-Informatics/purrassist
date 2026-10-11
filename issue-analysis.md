## Issue Analysis: #15

**Title:** purrassist-derive: zero-dependency #[derive(ToolDefinition)] and #[derive(ToolEnum)]
**State:** OPEN  **Author:** paudley  **Created:** 2026-10-10 (spec text is #4 "Spec A", pre-#8/#9)
**Labels:** none  **Comments:** 0  **Linked PRs:** none (from staged evidence; forge not refetched)
**Base:** main @ fccfbe7 (worktree branch `paudley/15-purrassist-derive-zero-dependency`)

### Summary
Add publishable crate `purrassist-derive` (`crates/derive`, `proc-macro = true`, zero
`[dependencies]`, only built-in `proc_macro`) with a first-party token-tree parser and
writer. It provides `ToolDefinition`/`ToolEnum` (emitting `::purrassist_core` paths) and
`FacadeToolDefinition`/`FacadeToolEnum` (emitting `::purrassist::__core`), re-exported by
the facade as `purrassist::{ToolDefinition, ToolEnum}`. Derived code = the same builder
calls as a hand-built `tool_call::ToolDefinition` plus a `FromNode` impl. Register the
crate as the sixth release crate across workspace, release scripts, publish workflow,
packaging/verification fixtures, inventory, audit, and docs.

---

### Code-grounded facts (main @ fccfbe7)

- Workspace members (7): core, syntax, purrassist, host, cli, wasm-harness (publish=false),
  helper-census (publish=false). Publishable/RELEASE_ORDER = 5 → "five" → "six" is correct.
- Toolchain: `rust-toolchain.toml` = nightly-2026-09-14 (rustc 1.100); `rust-version = "1.98"`;
  CI `msrv` job runs `cargo test --workspace --locked` on 1.98.0 with `-D warnings`
  (`.github/workflows/verify.yml:33-43`). Locally only the stage nightly is present; old-Rust
  validation goes through the `rust-validate <ver> <cmd>` wrapper.
- `crates/core/src/tool_call/definition.rs` (unchanged since dd73c0c, before #8/#9):
  - `ToolDefinition::new(name)`, `.alias()`, `.description()`, `.effects(Effects)`,
    `.policy(RepairPolicy)`, `.unknown_parameters(UnknownParameters)`, `.param(name, spec)`.
    Derives `PartialEq, Eq` → "derived == hand-built" is directly assertable.
  - `Param::{string, integer, number, boolean, enumeration(&[&str]), array(spec), object(ObjectSpec)}`.
  - `ParameterSpec::{required, alias, repeatable, default(Node), description, pattern}`.
  - `ObjectSpec::{new, param, unknown_parameters}`. No constructor of `ToolDefinition` from an
    `ObjectSpec` (`arguments` is `pub(super)`), so the tool body and nested-object body must be
    emitted separately.
  - Enums: `Effects::{ReadOnly, Mutating}` (default Mutating), `RepairPolicy::{Structural,
    Tolerant}`, `UnknownParameters::{Error, DropWithLoss, PassThrough}`.
  - Every attribute key in the spec has a builder: name→`new`/`.param(name,..)`, alias→`alias`,
    description→`description`, effects→`effects`, policy→`policy`, unknown→`unknown_parameters`
    (tool) / `ObjectSpec::unknown_parameters` (nested), repeat→`repeatable`, default→`default(Node)`.
    `pattern` exists but has no attribute in the spec.
  - Compile-time (ToolSet::compile) rules relevant to derive: `repeatable` on an array param is
    `InvalidDefinition`; an array item spec cannot be required/aliased/repeatable/defaulted
    (toolset.rs:1520-1527); empty or duplicate enumeration rejected; defaults validated against
    the declared type; required+default is NOT rejected.
- `crates/core/src/tool_call/from_node.rs`: `pub trait FromNode: Sized { fn from_node(node: &Node)
  -> Result<Self, FromNodeError>; }`. Impls: String, bool, `json::Number` (exact clone), f64
  (finite), `json::Node`, `Option<T>` (null→None), `Vec<T>` (index-prefixed pointer), all 12
  integer types (exact, range-checked; `usize`/`isize` are target-width → wasm32 differs).
  `FromNodeError { pub pointer: String, pub expected: &'static str, pub found: String }`;
  `mismatch`, `within`, `describe` are PRIVATE. `json::pointer_segment` is pub.
- `json::Node::get(key)` returns `None` for BOTH missing and duplicated members.
- `ToolCall.arguments` (recover.rs:1457) is "resolved and canonical when the call resolved" →
  aliases are already canonicalized in recovered arguments.
- Facade `crates/purrassist/src/lib.rs`: `pub use purrassist_core::*;` (+ Schema, RecoveryRequest,
  `syntax`). Core root has no `ToolDefinition` item (it is `tool_call::ToolDefinition`), so the
  root-level derive re-export does not collide; type vs macro namespaces differ anyway.
  `crates/purrassist/tests/` does not exist yet.
- No core trait exists for "type → ParameterSpec" or "type → ToolDefinition" (grep: none).
- `json::parse(&str) -> Result<Node, ParseFailure>`; `Node::new(Value)` is `const`;
  `Number: FromStr` (fallible); `Number::from_unsigned/from_count` exist.

---

### Spec items that no longer match the code / need adjustment

| # | Spec item | Problem against current code | Needed adjustment |
|---|-----------|------------------------------|-------------------|
| A1 | "`RELEASE_ORDER` lists derive first" | `check-release.py` requires every normal AND dev dependency to precede its user and have req `^0.1.0` (lines 118-128). Derive dev-depends on `purrassist-core` (spec Tests) → derive-first fails `check-release.py`. Path-only (version-less) dev-dep would be stripped on publish but fails the `^version` req check. | Order: `purrassist-core, purrassist-derive, purrassist-syntax, purrassist, purrassist-host, purrassist-cli` (derive second). Core never depends on derive, facade needs derive before it. Same order in `publish.yml` loop + step name (line 88) + RELEASE.md. |
| A2 | Audit (a) "its closure must be empty" | `check_core_closure` walks normal+build+**dev** edges; derive's dev edge to purrassist-core makes that closure non-empty. `check_core_dev_dependencies` forbids any dev-deps. | New `check_derive_closure`: normal+build closure empty; dev edges may reach only first-party `purrassist-core` (source None, itself closure-empty). Manifest check: no `[dependencies]`, `[build-dependencies]`, `target.*.dependencies`; `[dev-dependencies]` ⊆ {purrassist-core}. |
| A3 | (c) ambient scan "also covers crates/derive/src" | `check_ambient_access` hardcodes `crates/core/src`; message says "forbidden in purrassist-core". | Parameterize root list (core, derive) and label by crate. Fixture: a derive-labelled violation is reported with its path. |
| A4 | "nested derived struct → object; a ToolEnum → enum" via a syntactic type map | A plain path (`Mode`, `Range`, a `type` alias to `String`) cannot be classified syntactically as struct vs enum vs scalar. | Decide dispatch (Q1): core trait(s) implemented for scalars/Option/Vec and generated for derived types (recommended), or a `#[doc(hidden)]` inherent associated fn emitted by both derives with scalars recognized by last path segment. Spec never names the user entry point that returns the `ToolDefinition` either. |
| A5 | "generated code is builder calls only, plus FromNode" | Derived `FromNode` must build `FromNodeError`s: `mismatch`/`within`/`describe` are private, so each expansion would duplicate the `describe` match and pointer prefixing. | Make `FromNodeError::mismatch` and `within` public (or a `#[doc(hidden)]` support module) in core, with Rustdoc+examples; core stays dependency-free. |
| A6 | `Option<T>` → not required; FromNode | `Node::get` hides duplicates as `None`; an `Option` field would silently read a duplicated member as `None` (a loss). | Generated FromNode iterates `Value::Object` members: missing required → error, duplicate → error, absent Option → `None`. Fix the error `found` text format (Q6). |
| A7 | `#[param(repeat)]` | A repeatable `p: T` carries an array of T; `repeatable` on `Param::array` is InvalidDefinition. | `repeat` legal only on `Vec<T>` / `Option<Vec<T>>`; emits `<T spec>.repeatable()` (not `Param::array`). On any other type → `compile_error!` at the key span. |
| A8 | `default = <json literal>` | Syntax undefined; builders are infallible, `json::parse` is fallible → an `.expect` in generated code; core contract: defaults are "validated, never-applied", so FromNode must not fill them. | Q4: define accepted token forms and lowering (recommend `Node::new(Value::…)` construction from Rust tokens, JSON-number grammar checked at expansion); type conformance stays a `ToolSet::compile` error. Decide whether `default` requires an `Option<T>` field. |
| A9 | "integer types → integer" | `Param::integer()` has no range; a `u8` field accepts `300` in schema and recovery, then fails `FromNode`. No min/max builder exists. | Document the gap (FromNode reports it exactly) or file follow-up for range keywords; out of scope here. |
| A10 | `crates/purrassist/tests/derive_facade.rs` proves facade paths | Integration tests of `purrassist` see its normal dep `purrassist_core`, so a facade derive that wrongly emitted `::purrassist_core` would still compile there. | Also exercise `#[derive(purrassist::ToolDefinition)]` from a crate with no direct core dependency (e.g. `crates/cli/tests/derive_facade.rs` or `crates/host/tests/…`), or a `publish = false` fixture crate. |
| A11 | "five" → "six" only in RELEASE.md, RESOURCES.md, verify-published.py docstring | Also in `scripts/package-workspace.py:76`, `docs/QUALIFICATION.md:227`, `scripts/test-verify-published.py:6-7,79`, RELEASE.md lines 10-11/21/28/42/79-82/91, publish.yml step name line 88. RESOURCES.md: only line 1083 (214/339/491/1147 are unrelated counts). | Update all of them. Also remove `purrassist-derive` from "Still tracked in issue #4" in `docs/IMPLEMENTATION.md:56-59`, `docs/QUALIFICATION.md:116-119`, and `docs/tool-calls/README.md:32-34`. #4 is CLOSED; remaining items are in #12-#14 and #16-#18. |
| A12 | `dependency_of` in test-verify-published.py | It is a single-valued linear chain; the real facade depends on core, syntax and derive. | Make `dependency_of` multi-valued (tuple) and emit every dep in index entries. Fixture: `derive` member, `proc-macro = true` crate (exports e.g. `#[proc_macro] marker`), `mid` depends on syntax + derive and calls it; extend `members`, chain, `CRATE_NAMES`, docstrings. Required anyway: `verify-published.py` iterates the real `RELEASE_ORDER`, so a missing fixture archive fails. |
| A13 | Packaging audit | `check-packages.py` enforces "no runtime deps" for core only. It KeyErrors without `packaged_notices.derive` (covered by spec). | Extend the packaged-manifest dependency check to `purrassist-derive` (no deps / build-deps / target deps). |
| A14 | RELEASE.md external one-time steps | RELEASE.md says names "already minted" and trusted publishers "already configured" for five. Derive has neither. | Doc must state that derive's 0.0.0 reservation and trusted publisher are pending maintainer steps. Do not claim them done. First tag publish fails until they are done. |
| A15 | "compile_error! with a span" | `compile_fail` doctests pass on any failure and cannot assert message or span. No trybuild (third-party). | Q5: pair every compile_fail doctest with a near-identical passing doctest. Optionally add a first-party UI check (cargo build fixture + JSON diagnostics) for message+span. Otherwise document spans as unevidenced. |
| A16 | "unknown attributes → compile_error!" | Derive input carries every outer attribute (`#[doc]`, `#[allow]`, `#[cfg_attr]`, other derives' helpers such as `#[serde]`). | Error only on unknown/misplaced/duplicate KEYS inside `#[tool(...)]`/`#[param(...)]`. Ignore foreign attributes. |
| A17 | Cargo.lock | Not mentioned; every gate uses `--locked`. | Regenerate and commit `Cargo.lock` with the new first-party package. |

Items verified as still valid: all builder methods for every attribute key; `Param::array(Param::object(..))`
(arrays of nested objects, hardened in #9); `json::Number` FromNode is exact; `Option`/`Vec`
FromNode semantics; `ToolDefinition: PartialEq`; `packaged_notices` keyed by crate dir name
(`derive`); license-triple and SPDX checks iterate `crates/*` automatically; network
denylist (audit b) already covers every publishable crate automatically.

---

### Requirements → acceptance checks

Notation: every check is executed from the worktree root; "derive test" = `crates/derive/tests/derive.rs`
run by `cargo test -p purrassist-derive --locked`.

#### Explicit requirements

| ID | Requirement | Acceptance check (real entry point) |
|----|-------------|-------------------------------------|
| L1 | `crates/derive/Cargo.toml`: name `purrassist-derive`, `[lib] proc-macro = true`, workspace package fields + description/keywords/categories/documentation, `[lints] workspace = true`, no `[dependencies]`, dev-dep `purrassist-core.workspace = true` | `cargo metadata --no-deps --format-version 1 --locked` → derive target kind `["proc-macro"]`, deps = [purrassist-core kind dev]; extended `python3 scripts/check-repository.py` exits 0; mutated fixture (normal dep added) fails in `test-repository-audit.py` |
| L2 | README.md (SPDX comment) + byte-identical LICENSE-MIT/-APACHE/-MULAN | `python3 scripts/check-repository.py` (license loop over `crates/*`) ; `python3 scripts/check-packages.py <archives>` via `make package` (README/LICENSE present in archive) |
| L3 | SPDX headers on all `.rs`/`.toml` | `python3 scripts/check-repository.py` (crates/** scan) |
| L4 | Rustdoc with meaningful examples + error semantics on each exported derive | `make doc` (`-D warnings`, missing_docs) ; doctests via `cargo test -p purrassist-derive --doc` |
| P1 | First-party parser over `proc_macro::TokenStream`: attributes, visibility (`pub`, `pub(crate)`, `pub(in ..)`), named fields, raw idents, types `Vec<T>`, `Option<T>`, plain (multi-segment, `::`-prefixed) paths; angle-bracket depth tracked manually (`<`/`>` are Puncts) | derive test structs covering each shape compile and equal hand-built definitions |
| P2 | `compile_error!` with span for generics (type/lifetime/const/where), tuple structs, unions, unknown attribute keys; plus implied: enum to ToolDefinition, struct to ToolEnum, non-unit variants, unsupported field types, duplicate/malformed keys, `repeat` on non-Vec | `compile_fail` doctests in `crates/derive/src/lib.rs`, each paired with a passing twin; run by `cargo test -p purrassist-derive --doc`; span evidence per Q5 |
| P3 | First-party writer producing `TokenStream` via `FromStr` | Covered by all positive derive tests; no panic path (`LexError` unreachable): fuzz-like test of literal/ident edge cases (raw idents, Unicode idents, escapes in descriptions) |
| P4 | `ToolDefinition`/`ToolEnum` emit `::purrassist_core`; `FacadeToolDefinition`/`FacadeToolEnum` emit `::purrassist::__core` | derive test uses `purrassist_derive::{ToolDefinition, ToolEnum}` with only core; facade test (A10) uses `purrassist::{ToolDefinition, ToolEnum}` from a crate lacking a direct core dep |
| P5 | Facade: `#[doc(hidden)] pub use purrassist_core as __core;` + `pub use purrassist_derive::{FacadeToolDefinition as ToolDefinition, FacadeToolEnum as ToolEnum};`; facade depends on derive | `crates/purrassist/tests/derive_facade.rs` with `#[derive(purrassist::ToolDefinition)]`, no `crate =`; `cargo test -p purrassist --locked` |
| P6 | Core never depends on derive | `python3 scripts/check-repository.py` (core closure empty incl. dev) |
| T-map | `String`→`Param::string()`; `i8..i128,isize,u8..u128,usize`→`integer()`; `f64`→`number()`; `json::Number`→`number()` + exact FromNode; `bool`→`boolean()`; `Vec<T>`→`array(T)`; `Option<T>`→omit `.required()`; nested derived struct→`object(ObjectSpec)`; `ToolEnum`→`enumeration(&[..])`; non-Option → `.required()` | derive test: one struct with a field of every mapped type; `assert_eq!(<derived>, ToolDefinition::new(..).param(..)…)`; plus `ToolSet::compile([derived], Budget::default())` is Ok and `render_json_schema` contains expected fragments |
| ATTR-tool | `#[tool(name = "..", alias = ".." (repeatable), description = "..", effects = mutating\|read_only, policy = structural\|tolerant, unknown = error\|drop\|pass)]` → `new`/`alias`/`description`/`effects(Effects::{Mutating,ReadOnly})`/`policy(RepairPolicy::{Structural,Tolerant})`/`unknown_parameters(UnknownParameters::{Error,DropWithLoss,PassThrough})` | derive test: one equality assertion per key and value; `CompiledTool::{effects, policy, unknown_parameters, description}` and `ToolSet::lookup_exact(alias)` after compile |
| ATTR-param | `#[param(name, alias, description, repeat, default = <json literal>)]` → `.param(name, ..)`, `.alias`, `.description`, `.repeatable()`, `.default(Node)` | derive test: equality per key; compiled `argument_shape().arguments.resolve_name(alias)`; `tools.defaults(tool)[i].value.to_json()`; repeatable schema renders `{"type":"array","items":..}` |
| FN1 | Derived `FromNode` for structs: object required, canonical member names, required missing → error, Option absent → None, duplicates → error, nested errors pointer-prefixed (`/field/0/sub`) | derive test: full pipeline `ToolSet::compile` → `ToolSet::recover` on a model turn (e.g. `hermes.v1` or `openai-tool-calls.v1`) → `Args::from_node(&call.arguments) == expected`; negative cases assert exact `FromNodeError { pointer, expected, found }` |
| FN2 | Derived `FromNode` for `ToolEnum`: exact string match of declared values; else error | derive test: valid value converts; `"ASC"`/number/unknown fail with pointer |
| FN3 | Nested structs and enums | derive test: struct with nested derived struct, `Vec<Nested>`, `Option<Nested>`, enum field, `Vec<Enum>`; equality + recover→from_node |
| G1 | Root `Cargo.toml`: add `crates/derive` to `members`; `[workspace.dependencies] purrassist-derive = { path = "crates/derive", version = "0.1.0" }` | `cargo metadata --locked` succeeds; `python3 scripts/check-release.py` (req `^0.1.0`) |
| G2 | `RELEASE_ORDER` includes derive (position per A1) | `python3 scripts/check-release.py` exits 0 and prints six crates |
| G3 | `publish.yml` loop + step name/comment updated, identical to RELEASE_ORDER | `python3 scripts/check-release.py` (`validate_publish_workflow` compares loop tuple) |
| G4 | `NAMES` in `scripts/test-package-workspace.py` | `python3 scripts/test-package-workspace.py` exits 0 |
| G5 | `CRATE_NAMES` + `dependency_of` (+ fixture workspace per A12) in `scripts/test-verify-published.py` | `python3 scripts/test-verify-published.py` exits 0 (installs fixture CLI from local registry incl. a proc-macro crate; archive diff over six) |
| G6 | `vendor/dependencies.json` `packaged_notices.derive = {}` | `python3 scripts/check-repository.py`; `make package` (check-packages KeyErrors otherwise) |
| DOC1 | `docs/RELEASE.md`: "six crates", new order, external one-time steps (name reservation, trusted publisher) for derive | Review + `grep -n "five" docs/RELEASE.md` empty; `check-release.py` order matches text |
| DOC2 | `docs/IMPLEMENTATION.md` "Workspace boundaries" gains `purrassist-derive` bullet | Review |
| DOC3 | `docs/DEPENDENCIES.md` paragraph (zero deps, built-in `proc_macro` only, first-party dev-dep on core, not a core dependency) | Review |
| DOC4 | "five"→"six": `docs/RESOURCES.md:1083`, `scripts/verify-published.py` docstring line 9 | `python3 scripts/test-verify-published.py`; review |
| AU1 | (a) derive closure empty (per A2) | `python3 scripts/check-repository.py`; new `test-repository-audit.py` fixtures: derive + normal dep fails with path; derive + build dep fails; derive dev→purrassist-core passes; dev→third-party fails |
| AU2 | (c) ambient scan covers `crates/derive/src` | `python3 scripts/test-repository-audit.py`: fixture `crates/derive/src/x.rs` with `std::env` / `HashMap` fails with derive label; `check_ambient_access(ROOT) == []` |
| AU3 | New fixtures in `test-repository-audit.py` | `python3 scripts/test-repository-audit.py` exits 0 |
| T1 | `crates/derive/tests/derive.rs` (dev-dep core): derived == hand-built; FromNode round-trip; nested structs + enums | `cargo test -p purrassist-derive --locked` |
| T2 | `compile_fail` doctests: generics, tuple structs, unions, unknown attributes | `cargo test -p purrassist-derive --doc --locked` |
| T3 | `crates/purrassist/tests/derive_facade.rs`, `#[derive(purrassist::ToolDefinition)]`, no attribute | `cargo test -p purrassist --locked` |
| CHK | `cargo test -p purrassist-derive -p purrassist`; `check-release.py`; `check-repository.py`; `test-repository-audit.py`; `test-package-workspace.py`; `test-verify-published.py` all exit 0 | Run each; plus the full gate (below) |

#### Implicit requirements

| ID | Requirement | Acceptance check |
|----|-------------|------------------|
| I1 | Full gate, not focused tests (AGENTS.md) | `make check` (fmt, hygiene, helper-test, helper-check, clippy `--all-targets -D warnings`, `cargo test --workspace --locked` with `-D warnings`, `cargo doc -D warnings`) |
| I2 | MSRV 1.98: no `proc_macro` API stabilized after 1.98 (local is 1.100 nightly); no `#![feature]` | `RUSTFLAGS=-D warnings rust-validate 1.98.0 cargo test --workspace --locked` (mirrors CI `msrv`; CI job is authoritative) |
| I3 | Packaging evidence for six crates | `make package` (`check-release.py`, `test-package-workspace.py`, `cargo package` all publishable with verify, `check-packages.py`) |
| I4 | Lockfile updated | `cargo metadata --locked` OK; `git diff Cargo.lock` shows only `purrassist-derive` entry + facade dep edge |
| I5 | `check-packages.py` zero-dep check extended to derive (A13) | `make package` |
| I6 | Generated code clean under workspace lints in downstream test crates (clippy pedantic/nursery, missing_docs, unreachable_pub) | `make lint` (`cargo clippy --workspace --all-targets -- -D warnings`) |
| I7 | Determinism: output is a pure function of input tokens (declaration order, no hashing; ambient scan forbids `HashMap`/env/fs) | derive test: two identical structs in different modules produce equal definitions; AU2 |
| I8 | Hygiene: generated locals cannot capture user items (e.g. a `const node` in scope); paths absolute | derive test module defining `const node: u8`, `struct members;`, and similar shadowing items, still compiles and behaves |
| I9 | Remove "purrassist-derive" from "Still tracked in issue #4" sentences (A11); add QUALIFICATION.md evidence row naming the derive tests (capability claims need executable evidence) | Review; `grep -rn "purrassist-derive" docs` reflects shipped status |
| I10 | helper census strict check stays clean (derive src is under `crates/`; proc-macro target kind is walked) | `make helper-test helper-check` |
| I11 | Rustdoc in derive must not intra-doc-link core items (dev-deps unavailable to `cargo doc` of the lib → broken link → `-D warnings`) | `make doc` |

---

### Dependencies

#### Depends On
- #5 feat(3) typed definitions / FromNode (MERGED): satisfied; `FromNode` present at fccfbe7.

#### Blocks
- None stated.

#### Related Issues
- #4 (CLOSED; now tracking): origin of Spec A / D12. Its "still tracked" doc sentences must be updated.
- #3 (CLOSED): standing constraints (zero deps, network-free, governed linear cost, native==WASM, original input retained, guess never resolved).
- #12, #13, #14, #16, #17, #18 (OPEN siblings split from #4). They edit the same "Still tracked in issue #4"
  sentences in IMPLEMENTATION.md, QUALIFICATION.md, and tool-calls/README.md, which risks merge conflicts.

### Context from Discussion
#### Key Decisions (from issue body)
- D12: zero deps; only built-in `proc_macro`; two macro pairs; facade re-exports with `__core`; no
  `crate =` attribute for either audience; core never depends on derive.
#### Rejected Approaches
- Implicitly rejected: `syn`/`quote`/`proc-macro2` (workspace has them only for unpublished
  helper-census); a `crate = ..` path attribute.
#### Open Questions / decisions required
- Q1 Dispatch and entry point (A4): add core trait(s) (recommended). For example, `ToolArguments: FromNode {
  fn tool_definition() -> ToolDefinition; fn object_spec() -> ObjectSpec; }` and `ToolParameter: FromNode
  { fn parameter_spec() -> ParameterSpec; }` with impls for String/ints/f64/Number/bool/Option/Vec.
  This gives alias-safe resolution, spanned "trait not implemented" errors, and no missing_docs on
  generated items. The alternative is hidden inherent fns plus name-based scalar recognition. Either way,
  the spec's "builder calls only" needs amending.
- Q2 Default naming: tool name from struct ident verbatim or snake_case; parameter name from field
  ident with `r#` stripped; enum value spelling (verbatim variant ident vs lowercase); which
  attribute renames a variant (`#[param(name = "..")]`?). Recommend verbatim (no inference)
  plus explicit renames.
- Q3 Tool-level keys on a struct used only as a nested object (`name`/`alias`/`effects`/`policy`):
  reject or ignore? `unknown` → `ObjectSpec::unknown_parameters`; does the struct `description`
  apply to the nested field's `ParameterSpec`?
- Q4 `default` syntax: Rust tokens (`10`, `-1.5`, `true`, `null`, `"s"`, `[..]`, `{ "k": v }`) or a string
  containing JSON text. Must the field be `Option<T>` (FromNode never applies defaults)?
  Should obvious type mismatches be caught at expansion, or only at `ToolSet::compile`?
- Q5 Span evidence for `compile_error!`: compile_fail doctests only (cannot see the span) or a
  first-party UI harness.
- Q6 FromNodeError shape for missing/duplicate members and unknown members (ignore, since recovery
  already applied the policy, vs reject). Do aliases count in FromNode? (Recommend canonical only:
  recovered arguments are canonical.)
- Q7 Include `#[param(pattern = "..")]` (builder exists; "maximal utility") or leave it out of scope?
- Q8 Doc comments as description fallback, or only the explicit `description`?
- Q9 Recursive types (`struct T { c: Vec<T> }`) recurse forever when building the spec, causing a
  runtime stack overflow. Reject direct self-reference at expansion. Indirect recursion could
  be turned into a rustc cycle error through an associated const depth (trait approach).
- Q10 Placement: derive second in RELEASE_ORDER (A1) instead of first. Confirm.

---

### Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Derive-first order breaks `check-release.py` (dev-dep precedence) | HIGH | HIGH | A1: core, derive, syntax, ... |
| `FromStr`-built tokens carry only `call_site` spans, so type errors point at `#[derive]` rather than the field, and generated locals are unhygienic | HIGH | MED | Splice original field-type/literal tokens (keep spans); re-span generated locals to `Span::mixed_site()` (stable 1.45); test I8 |
| `compile_error!` spans unverifiable; `Span::join` unstable → only first-token spans | HIGH | LOW-MED | Q5; span the offending key/type head token; paired positive doctests |
| Using `proc_macro` API stabilized after 1.98 (e.g. `Literal::str_value`, Span line/column/file accessors) compiles on local 1.100 nightly but fails CI msrv | MED | HIGH | Avoid value extraction: pass literal tokens through verbatim (descriptions/names/enum match arms/`concat!` messages); run I2 locally |
| `proc_macro` types panic outside a macro, so parser unit tests are impossible | HIGH | MED | Test through integration tests/doctests, or parse into an internal token model tested separately |
| `compile_fail` doctests pass for the wrong reason (e.g. missing import) | MED | MED | Positive twin per case; minimal imports identical between twins |
| Facade test cannot detect wrong path prefix (A10) | HIGH | MED | Add a test in a crate without a direct core dep |
| Generated code trips workspace lints (clippy pedantic/nursery, missing_docs, unreachable_pub) in test crates under `-D warnings` | MED | MED | `#[automatically_derived]` trait impls; avoid generated inherent pub fns; I6 |
| Helper census flags derive helpers duplicating core helpers (e.g. JSON quoting/number grammar) because derive cannot depend on core | MED | MED | Lower defaults to `Node::new(Value::…)` constructors (no JSON quoting in derive); keep derive-internal helpers structurally distinct; run `make helper-check` early |
| Schema looser than Rust integer range (A9); `usize`/`isize` FromNode differs native vs wasm32 | MED | LOW | Document; recommend fixed-width ints in docs |
| Recursive argument types → runtime stack overflow (Q9) | LOW | HIGH | Reject direct self-reference; const-depth trick or documentation |
| Derive's crates.io name and trusted publisher do not exist → first tag publish fails midway (core published, derive fails), leaving a partial irreversible release | MED | HIGH | A14: document as pre-tag prerequisite; release checklist verifies via crates.io API before tagging |
| `tool`/`param` helper attribute names clash with other macros in scope (e.g. an imported `#[tool]` attribute macro from MCP SDKs) → "ambiguous" errors | LOW | MED | Document; possible future rename is out of scope |
| Option/Vec recognized by last path segment; user types named `Option`/`Vec` misclassified | LOW | LOW | Trait dispatch (Q1) makes classification irrelevant except for `required` |
| Duplicate-member masking via `Node::get` (A6) | MED | MED | Member iteration in generated FromNode; negative test |
| Resource evidence (`docs/qualification/resources-*.json`) digest covers `crates/**`; cold-build ceiling 180 s now includes a proc-macro crate | LOW | LOW | Re-measure at release qualification; RESOURCES.md:1083 text updated |
| Sibling issues editing the same doc sentences | MED | LOW | Rebase carefully |

---

### Technical Notes

#### Affected Areas
- New: `crates/derive/{Cargo.toml,README.md,LICENSE-MIT,LICENSE-APACHE,LICENSE-MULAN,src/lib.rs,src/*.rs,tests/derive.rs}`
- `Cargo.toml` (members, workspace.dependencies), `Cargo.lock`
- `crates/purrassist/{Cargo.toml,src/lib.rs,README.md}`, new `crates/purrassist/tests/derive_facade.rs`
  (+ A10 test location, e.g. `crates/cli/tests/` or `crates/host/tests/`)
- Possibly core (Q1/A5): `crates/core/src/tool_call/{from_node.rs,mod.rs}` (+ new trait module)
- `scripts/check-release.py` (RELEASE_ORDER), `.github/workflows/publish.yml` (loop line 94, step name line 88)
- `scripts/check-repository.py`, `scripts/test-repository-audit.py`, `scripts/check-packages.py`
- `scripts/test-package-workspace.py` (NAMES), `scripts/package-workspace.py` (comment line 76)
- `scripts/test-verify-published.py` (CRATE_NAMES, LIB_NAMES, dependency_of, fixture workspace, docstrings),
  `scripts/verify-published.py` (docstring line 9)
- `vendor/dependencies.json` (`packaged_notices.derive`)
- Docs: `docs/RELEASE.md`, `docs/IMPLEMENTATION.md` (boundaries + line 56-59), `docs/DEPENDENCIES.md`,
  `docs/RESOURCES.md:1083`, `docs/QUALIFICATION.md` (116-119, 227, evidence row), `docs/tool-calls/README.md:32-34`

#### Potential Challenges
- Writing a correct `proc_macro`-only parser: angle-bracket depth over `Punct`s, `>>` split into joint
  puncts, `pub(in path)`, `r#` idents, attribute argument lists `key = literal | ident | [..] | {..}`,
  trailing commas, `#[cfg_attr]` passthrough.
- Emitting literal tokens verbatim (no unescape) while still validating key shapes at expansion.
- Avoiding panics in the macro (a panic becomes an opaque "proc-macro derive panicked" error).
- Keeping the derived FromNode linear and allocation-light (it is ungoverned, like all core FromNode impls).
  Recursion depth is bounded by static type nesting, not by input depth.
- Producing exactly the hand-built builder chain (alias order, no redundant calls), so `PartialEq` holds.

### Out of Scope
Generics/lifetimes, tuple/unit structs, unions, data-carrying enum variants; serde interop; a `ToNode`/serialization
derive; numeric range constraints; the `crate =` attribute; executing the crates.io name reservation, trusted
publisher setup, tagging, and publication (external, irreversible, maintainer-authorized); streaming/dialect work
(#12-#14, #16-#18); `pattern` unless Q7 says otherwise.

### Related PRs
- None linked to #15. Prior: PR #5 (merged; typed definitions + FromNode), #8/#9 (merged; tool-call recovery refinements).
