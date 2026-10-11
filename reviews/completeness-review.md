# Completeness review: issue #15 (purrassist-derive)

Scope: `fccfbe7..d2410ed` (5 commits) on `paudley/15-purrassist-derive-zero-dependency`,
against `issue.md` (#4 Spec A plus the #15 objective), `plan.md` (the user decisions,
D-1..D-14, the completeness contract and the plan change), `validation.md`, the T1-T4
reviews, AGENTS.md and .goals. This review was read-only: scratch crates were built
only under `/tmp/cr15` and deleted afterwards, and `make check` was not run.

## Executed evidence

### 1. Downstream crate depending on `purrassist` only (`/tmp/cr15/facade`, path dependency)

This is a single binary that uses `#[derive(purrassist::ToolDefinition)]` and
`#[derive(purrassist::ToolEnum)]` with no `crate =` attribute. Its tool struct
`SearchTool` covers:

- every mapped type: `String`, `u8`, `i64`, `u128`, `isize`, `f64`,
  `json::Number`, `bool`, `Vec<String>`, `Option<ToolEnum>`, a nested derived
  struct, `Vec<nested>`, `Option<String>`, `Option<nested>`;
- naming: field `r#type`, and a struct and a field described by doc comments;
- attributes: `#[tool(alias, effects = read_only, policy = tolerant, unknown = error)]`,
  plus `#[param(alias, pattern)]`, `#[param(default = 10, description)]` (alongside a
  doc comment), `#[param(repeat)]`, an object default
  `{"w": 3, "h": 4}` on `Option<Rect>`, a string default with escapes
  `"s\"q\u{e9}"`, and an array default `[-2.5, 1e3]`;
- `Passer` with `#[tool(unknown = pass)]` and `#[param(extra)] rest: Node`;
- `HTTPRequest`.

Command: `cargo run -q` (target directory remapped by the wrapper). Observed:

```
def contains "explicit wins": true / "ignored doc": false / "What to look for.": true
def contains "A rectangle.": true (nested struct description falls back to the field)
def contains "Sort direction.": true / "ascending" (variant doc): false
effects=ReadOnly policy=Tolerant unknown=Error desc=Some("Search the\nindex.")
lookup alias find: Some("search_tool")
http tool: Some("http_request")
args json = {"query":"cats and dogs",...,"a_u128":340282366920938463463374607431768211455,...,"exact":1.000,...}
struct = SearchTool { query: "cats and dogs", limit: 10, ..., exact: Number("1.000"), ...,
         order: Some(Desc), rect: Rect { w: 2, h: None }, rects: [Rect { w: 1, h: Some(2) }],
         type: "kind", blob: Some(Rect { w: 3, h: Some(4) }), greeting: "s\"qé", floats: [-2.5, 1000.0] }
qwen call outcome = Resolved      (qwen3-coder-xml.v1, nested <parameter> items, repeated tags)
qwen struct = Ok(SearchTool { query: "hi", ..., exact: Number("2.50"), tags: ["a","b"], rect: Rect { w: 7 }, rects: [Rect { w: 8 }], ... })
passer known=k rest={"other":[1],"more":{"z":1}}
pattern violation outcome kind = Resolved/Unresolved { reason: SchemaValidation }
```

The hermes.v1 turn used the alias `q` for `query`; it resolved and converted.
`Option<json::Node>` as a field type was rejected with E0277 (`Node: ToolItem`) at the
field type. That matches the spec's type map, which lists no free-form `Node` field.

### 2. Downstream crate depending on `purrassist-derive` and `purrassist-core` (`/tmp/cr15/direct`)

`cargo tree -e normal,build` shows only `purrassist-core` and
`purrassist-derive (proc-macro)`. Observed from `cargo run --bin cr15-direct`:

```
HTTPRequest -> http_request
outcome BestGuess (unknown=drop dropped junk)   (qwen3-coder-xml.v1, called through alias "draw")
painter = Ok(Painter { color: Green, layers: [Inner { label: "bg", n: None }, Inner { label: "fg", n: Some(-4) }],
                       opacity: 1.5 (default filled), exact: Number("1e400"), big: 18446744073709551615 })
open a=3 extra={"zz":null,"yy":"v"}             (hermes.v1, unknown = pass)
u8 300 -> Err(FromNodeError { pointer: "/a", expected: "integer within the u8 range", found: "number 300" })
nested mismatch -> Err(FromNodeError { pointer: "/layers/0/label", expected: "string", found: "number 1" })
dup -> Err(FromNodeError { pointer: "/color", expected: "a single occurrence", found: "duplicate member" })
```

### 3. Rejection cases (`cargo check --bin <case>` in the direct crate)

| Case | Observed error | Span |
|---|---|---|
| `struct G<T>` | `generic parameters are not supported: a generic type has no single tool schema; use a concrete type` | `<` of `G<T>` |
| `struct T(String);` | `tuple structs are not supported: parameters need names; ...` | `(String)` |
| `union U` | `unions cannot describe tool arguments; ...` | `union` |
| `#[param(nmae = "x")]` | `unknown key `nmae` in `#[param(..)]` on a field; expected one of: name, alias, description, repeat, default, pattern, extra` | `nmae` |
| `Vec<Option<String>>` | E0277 `Option<String>: ToolItem` not satisfied | the field type |
| `Option<Box<S>>` in `S` | `field `child` refers to `S` itself; ... a recursive schema must be declared by hand` | the inner `S` |
| `A{b: Vec<B>}`, `B{a: Option<A>}` | E0391 cycle in `DEPTH` | `#[derive]` (documented) |
| `default = 0x10` | ``0x10` is not a JSON number: ...` | `0x10` |
| `unknown = pass` without `extra` | `needs exactly one field marked #[param(extra)] ...` | `pass` |
| `enum E { A, B(u8) }` | `variant `B` carries data: ...` | `(u8)` |
| `#[param(repeat)] v: String` | E0277 `String: ToolRepeatable` | `String` |
| field `naïve` | E0080 `parameter name "naïve" is not valid: use ASCII letters, digits and `_ . : -`` | `#[derive]` |

Every error was readable and pointed at the right token, except the two const-evaluated
ones, which point at the `#[derive]` as documented.

### 4. Repository checks

| Command | Result |
|---|---|
| `cargo test -q -p purrassist-derive -p purrassist --locked` | exit 0: 3 + 27 + 24 + 5 + 45 + 42 tests, all ok |
| `python3 scripts/check-release.py` | exit 0; release order core, derive, syntax, purrassist, host, cli; publish.yml verified |
| `python3 scripts/check-repository.py` | exit 0; core and derive boundaries verified |
| `python3 scripts/test-repository-audit.py` | exit 0, 29 tests |
| `python3 scripts/test-package-workspace.py` | exit 0, 8 tests |
| `python3 scripts/test-verify-published.py` | exit 0, 10 tests (proc-macro fixture built and installed) |
| `cargo metadata` | derive deps: `[('purrassist-core','dev')]`; target kind `proc-macro`; core deps `[]` |
| `sha256sum crates/*/LICENSE-{APACHE,MIT,MULAN}` | one distinct hash per license (byte-identical) |
| `grep` for the stale texts "five crates" and "issue #4" | none relevant; the remaining "five" hits are about unrelated grammars and extensions |
| `gh issue view 12..18` | each retargeted issue exists and is OPEN with the matching title |

## Weakened-test inspection (`git diff fccfbe7..HEAD`)

- **`test-package-workspace.py` token fixture.** It was restructured into a `tokens`
  tuple with sentinel values. The loop `for name in tokens: assertNotIn(name, env)` still
  asserts that all three variables (`CARGO_TOKEN`, `CARGO_REGISTRY_TOKEN`,
  `CARGO_REGISTRIES_OTHER_TOKEN`) are stripped. Not weakened.
- **`test-verify-published.py`.** The chain changed from single-edge to multi-edge.
  The fixture adds a real proc-macro crate that the facade expands. Assertions are not
  loosened.
- **`test-repository-audit.py`.** Additions only.
- **Core.** The `from_node.rs` and `toolset.rs` diffs change only visibility,
  documentation and constructors, and make `is_valid_name` a `const fn`. No existing
  test or golden file was edited.

## Requirement table

| Requirement | Evidence | Status |
|---|---|---|
| Crate layout: `proc-macro = true`, `[lints] workspace`, no `[dependencies]`, README, 3 byte-identical LICENSE files, SPDX | `crates/derive/Cargo.toml`; sha256; SPDX headers present; check-repository OK | MET |
| Zero dependencies (no syn/quote) | `cargo metadata`; audit `check_derive_closure`; scan of `crates/derive/src` | MET |
| First-party token-tree parser (attributes, visibility, named fields, `Vec`/`Option`/paths, raw idents) | `parse.rs`/`model.rs`/`bridge.rs`; facade crate with `r#type`, full paths `purrassist::json::Number` | MET |
| `compile_error!` with span for generics, tuple, union, unknown keys | §3 observed; compile_fail doctests (44 occurrences) with twins | MET (foreign attributes such as `#[serde]` are ignored by necessity; unknown keys inside `#[tool]`/`#[param]` are rejected) |
| Writer through `FromStr` | `write.rs`; escapes and Unicode in defaults and descriptions observed (`"s\"qé"`) | MET |
| Two macro pairs; facade `__core` and `Facade*` re-exports; no `crate =` | `crates/purrassist/src/lib.rs:21,55,76`; facade crate §1; host test without core | MET |
| Type map (String, the 12 integers, f64, Number, bool, Vec, Option, nested, enum) | §1 every type round-trips via hermes and qwen; derive test `every_mapped_type_*` | MET |
| `#[tool(name, alias, description, effects, policy, unknown=error/drop/pass)]` | §1 and §2 observed (alias lookup, ReadOnly/Tolerant/Error, drop gives BestGuess with junk removed, pass gives extra) | MET |
| `#[param(name, alias, description, repeat, default)]` (+ `pattern`, `extra` from plan) | §1 and §2 observed (alias `q`, repeat in qwen, the default kinds, pattern gives SchemaValidation, extra) | MET |
| User decision: naming | `http_request`, `search_tool`, `r#type`→`type`; tests cover `read_file2`/`read2_file` | MET |
| User decision: defaults filled in FromNode | `limit: 10`, `opacity: 1.5`, `blob: Some(Rect{3,4})` | MET |
| User decision: `unknown = pass` with `extra` | `rest={"other":[1],"more":{"z":1}}`; `pass` without `extra` is rejected | MET |
| User decision: doc comments for **structs, fields, enums and variants** | struct, field and enum docs work and the explicit key wins; **variant docs are accepted and ignored** (`"ascending"` absent) | **PARTIAL** (F-1) |
| FromNode single pass, errors (missing/duplicate/unknown/mismatch/nested pointer) | §2 observed; core `Members` counter test; duplicate is not reported as missing | MET |
| D-8 integer ranges by FromNode only | `u8 300` gives `integer within the u8 range` | MET |
| D-9 recursion (direct spanned, indirect E0391) | §3 observed | MET |
| I8 hygiene | derive test module with `const node`, `struct members`, `mod core`, `mod std` | MET |
| Registration: members, workspace dependency, facade dependency, RELEASE_ORDER (core→derive per D-10), publish.yml loop and name, NAMES, CRATE_NAMES/`dependencies_of`, `packaged_notices.derive = {}` | diff; the scripts exit 0 | MET (the order deviates from "derive first" for a recorded reason, D-10) |
| Audit (a) empty closure, (c) scan of `crates/derive/src`, fixtures | `check_derive_closure`; `AMBIENT_FREE_SOURCES`; 9 new fixtures | MET |
| Tests: derive.rs (equality, round-trip, nested, enums); compile_fail doctests; facade test without the attribute | §4 | MET |
| Docs: RELEASE.md (six, order, external steps marked not done), IMPLEMENTATION.md, DEPENDENCIES.md, RESOURCES.md, the verify-published docstring | diff reviewed | MET |
| Docs claims: INTEGRATION.md "Rust API" | it states `Option<T: ToolParameter>` (actual `T: ToolItem`) and omits `ToolPattern for Option<Vec<String>>` | **PARTIAL** (F-2) |
| Docs claims: root README, crate README, facade README, lib.rs | the examples agree with the observed behavior (facade README doctested; the root README duplicates it) | MET |
| Issue checks (6 commands) | all exit 0 (§4) | MET |
| Core purity: core never depends on derive | core deps `[]`; check-repository OK | MET |

## Narrowing judgements

- **Variant doc comments ignored.** This narrows the user's verbatim decision
  ("Yes, explicit attribute wins … for structs, fields, enums and variants"). The
  rationale is technical: `Param::enumeration` and `ValueShape::Enumeration` have no
  per-value description. The narrowing is disclosed in the crate docs (`lib.rs`: "A
  variant's doc comment is accepted and has no effect"), but it was decided by an agent
  (T2 adjudication 3), not by the user. It is not silent in the code, but it is an
  unauthorized cut of a user decision.

  A builder-only implementation is possible without core changes. For example, the
  derive could append the value docs to the enumeration parameter's description
  (`"Sort direction.\n- asc: ascending"`), so the model sees them. That would deliver
  the user's intent: descriptions reach the model.

- **Unicode field names now need `#[param(name)]`.** This is not a narrowing. Core's
  `ToolSet::compile` rejects any non-ASCII parameter name (`toolset.rs:1378`,
  `parameter name or alias … must match [A-Za-z0-9_.:-]+`), so such a derived tool
  could never compile into a `ToolSet`. The derive moves a certain runtime failure to
  compile time. Unicode variant values (`Éclair`) and Unicode descriptions still work
  (derive test `unicode_identifiers_names_and_values_convert`).

## Findings

### REQUIRED

- **F-1: variant doc comments (PARTIAL).** The user decision names variants. Either
  implement it within the builder, for example by folding each variant's description
  into the enumeration parameter's description (with an explicit `#[param(description)]`
  on a variant winning), or get the user's explicit authorization for the narrowing,
  record it, and disclose it in the PR body and on the issue. Agent adjudication is not
  user consent.
- **F-2: `docs/tool-calls/INTEGRATION.md` misstates two trait impls.**
  - It says `ToolParameter` is implemented for `Option<T: ToolParameter>`. The actual
    bound is `T: ToolItem` (`derive_support.rs:308`). As written, the doc implies
    `Option<Option<T>>` is supported, which contradicts the crate docs and the
    compile_fail doctest.
  - It lists `ToolPattern` for `String`, `Option<String>` and `Vec<String>`, but omits
    `Option<Vec<String>>` (`derive_support.rs:349`).

  This is a one-line correction.

### ADVISORY

- **A-1: const-evaluated name errors.** Errors such as the Unicode field `naïve`, an
  explicit invalid name or an empty alias are E0080, spanned at `#[derive]`. The message
  does not mention the remedy (`#[param(name = "..")]` or `#[tool(name = "..")]`), which
  the crate docs promise ("a field whose name is not valid needs `#[param(name = "..")]`").
  Consider adding the remedy to the panic text.
- **A-2: indirect recursion.** Indirect recursion reports E0391 at `#[derive]`, not at
  the field. This is documented and acceptable.
- **A-3: root `README.md` example.** It is not itself a doctest. It is the same as the
  doctested `crates/purrassist/README.md` example, so drift is possible but currently
  absent.
- **A-4: compile_fail error codes.** Rustdoc enforces `compile_fail` error codes
  (E0277/E0391) only on nightly. The reasons were verified manually here and in the T2
  review.
- **A-5: PR body disclosures.** The PR body should state:
  - the D-10 order deviation (core before derive, not "derive first");
  - the `Option<T>` + `default` semantics;
  - that crates.io name reservation and the trusted publisher for `purrassist-derive`
    are not done.

VERDICT: BLOCKED

## Recheck

This recheck covers the uncommitted fixes on top of d2410ed: `crates/derive/src/{lib,model,parse,write}.rs`,
`crates/derive/tests/derive.rs` and `docs/tool-calls/INTEGRATION.md`.

### F-1 (variant descriptions): RESOLVED

A fresh downstream crate, `/tmp/cr15r`, depends only on `purrassist` (path) and derives
through the facade. It has four kinds of enum:

- documented variants (a multi-line doc comment, an explicit
  `#[param(description)]` overriding a doc comment, and an undocumented variant);
- an enum whose `#[tool(description)]` replaces its own doc comment;
- an enum with variant docs only;
- an enum field with its own doc comment.

`ToolSet::compile` → `render_json_schema("list_items")` printed:

```
"order":{"type":"string","description":"Sort direction.\n- asc: Smallest first.\n- desc: Largest first","enum":["asc","desc","Natural"]}
"order2":{"type":"string","description":"Field own text","enum":["asc","desc","Natural"]}
"mode":{"type":"string","description":"Explicit enum text\n- Fast: Quick","enum":["Fast","Slow"]}
"only":{"type":"string","description":"- A: one\n- B: two","enum":["A","B"]}
```

This matches the documented rule in every detail:

- the enum's own part comes first;
- one `- <value>: <text>` line follows per described variant, using the renamed value;
- a multi-line doc comment collapses to one line;
- an explicit variant key wins over its doc comment;
- an undocumented variant is omitted;
- `#[tool(description)]` replaces the enum's doc comment;
- an enum with only variant docs gives just the lines;
- a field's own doc comment wins over the composed text, and a field without one
  falls back to it.

The new derive test (`DocumentedEnum`, `OnlyVariantDocs`, `EnumFallback`) asserts the
same against hand-built definitions. The single removed test line was the old
"variant docs have no schema sink" expectation; it was replaced, not loosened.

### F-2 (INTEGRATION.md): RESOLVED

INTEGRATION.md now reads `Option<T: ToolItem>` ("so `Option<Option<T>>` is rejected").
It lists `ToolPattern` for `String`, `Option<String>`, `Vec<String>` and
`Option<Vec<String>>`. This matches `derive_support.rs:308` and `:331-349`.

### A-1 (remedy in name errors): RESOLVED

- field `naïve`: E0080 `parameter name "naïve" is not valid: use ASCII letters, digits and `_ . : -`, or set it with #[param(name = "..")]`
- `struct Größe`: `the tool name `größe` derived from `Größe` is not a valid tool name (...); add #[tool(name = "..")]`, spanned at the ident
- `#[tool(alias = "bad name")]`: E0080 `... or set it with #[tool(alias = "..")]`

### Commands

- `cargo test -q -p purrassist-derive -p purrassist --locked`: exit 0
  (3 + 27 + 24 + 5 + 45 + 42 passed, 0 failed).
- `cargo fmt --check -p purrassist-derive`: exit 0.
- `cargo clippy -p purrassist-derive --all-targets --locked -- -D warnings`: exit 0.
- `/tmp/cr15r` deleted.

### New advisories (non-blocking)

- R-1: `crates/derive/src/lib.rs:113` has a stray blank (non-`//!`) line inside the
  module docs, in the middle of a sentence ("...A nested struct needs no attributes of
  its own:" / blank / "its tool-level keys..."). It renders, but it is unintended.
  Lines 108, 112 and 305 run 110-156 columns, beyond the file's prevailing wrap
  width. Rewrap them.
- R-2: The alias error's remedy, "or set it with #[tool(alias = "..")]", is mildly odd,
  because the alias was already set that way. "use a valid #[tool(alias = "..")]"
  would read better. This is cosmetic.

VERDICT: PASS
