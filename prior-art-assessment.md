# Prior-art assessment — issue #15: `purrassist-derive` (zero-dependency `#[derive(ToolDefinition)]` / `#[derive(ToolEnum)]`)

Base: `main` @ `fccfbe7`. Sources: `issue.md`, `prior-art.md`, `brief.json`, local
`git log`/`git show`/`git log -S`, the working tree, and the main checkout's
untracked `.deficiencies` (read-only). Forge not refetched. Section 6 is model
knowledge and is labeled as such. Everything else cites a hash or a path.

## 0. Headline findings (read first)

1. **The spec's "RELEASE_ORDER lists derive first" conflicts with its own test plan.**
   `crates/derive/tests/derive.rs` dev-depends on workspace `purrassist-core`.
   `scripts/check-release.py` `main()` walks every dependency, dev edges
   included (`packages[name]['dependencies']` from `cargo metadata --no-deps`).
   It then fails with `purrassist-derive: dev dependency purrassist-core does not
   precede it in release order`. Its header comment states the rule:
   "Both normal and dev dependencies must precede users". The workspace entry
   `purrassist-core = { path = "crates/core", version = "0.1.0" }` also carries a
   version. On a first release, crates.io could not resolve it if derive were
   published first.
   **Consistent order:** `purrassist-core` → `purrassist-derive` →
   `purrassist-syntax` → `purrassist` → `purrassist-host` → `purrassist-cli`.
   Derive only has to precede the facade. This needs an explicit decision.
   The alternatives are weaker: a path-only dev-dependency with no version
   (Cargo strips it on publish) would also need a check-release exemption from
   the `^{version}` requirement check.
2. **Audit (a), "closure must be empty", cannot reuse `check_core_closure` as is.**
   That check walks `{'normal','build','dev'}` edges
   (`scripts/check-repository.py:58-64`). Derive's dev edge to core is required
   by the spec. The derive check must cover normal and build edges only, or it
   must exempt first-party workspace path packages.
   `check_core_dev_dependencies` must not be applied to derive.
3. **`FromNodeError`'s constructors are private.** `mismatch`, `within` and
   `describe` are module-private (`crates/core/src/tool_call/from_node.rs:33-46,
   65-74`). Code generated outside core can build the struct literal, because
   its fields are pub and it is not `#[non_exhaustive]`. It cannot reproduce
   the `found` text ("object with N members", "number 256") without copying
   `describe`. The likely result is a small public addition to core: a
   documented constructor or helper set. A copy would drift and would trip
   the helper census (§2).
4. **The type map must be dispatched by trait (or by an inherent associated fn),
   not purely by syntax.** A macro cannot tell whether an unknown path `Foo` is a
   nested `ToolDefinition` struct or a `ToolEnum`. Both become "call something on
   `Foo`". A proc-macro crate can export only macros, so any shared trait has to
   live in `purrassist-core`. Again, this is a core API decision (§3.5).
5. **Generated `FromNode` must be single-pass.** The only recurrence class on
   record is `performance` (§5). `Node::get` is a linear scan that returns
   `None` on duplicates (`crates/core/src/json/mod.rs:136-148`). One
   `get` per field is therefore O(fields × members), the quadratic shape #8
   fixed repeatedly. It also cannot tell "missing" from "duplicate".

## 1. Precedent for adding a workspace crate

**No publishable crate has been added since the release machinery existed.**
All five published crates were created together in the foundation commit
`46e9d1a`. `git log --diff-filter=A -- 'crates/*/Cargo.toml'` lists only
`46e9d1a`, `24787e1` (wasm-harness) and `a2da56a` (helper-census). The last two
are `publish = false`. Release and packaging machinery came later and hard-codes
the five names:

| Commit | What it established |
|---|---|
| `46e9d1a` | the five crates, each with Cargo.toml, three LICENSE files, README, src and tests; root Cargo.toml `members`; `verify.yml`; DEPENDENCIES/IMPLEMENTATION/QUALIFICATION docs |
| `f218e84` | `check-repository.py` core closure / denylist / ambient / baseline checks plus `test-repository-audit.py` synthetic fixtures (the pattern for audit (a)/(c)) |
| `de8db9d` | `scripts/check-release.py` `RELEASE_ORDER`, `release-qualification.yml`, `docs/RELEASE.md`, `check-packages.py` |
| `7882703` | `.github/workflows/publish.yml` (the loop), `verify-published.py`, `test-verify-published.py`, `package-workspace.py`, `vendor/tooling.json` |
| `a68beeb`, `1a8509c`, `868d196` | packaging isolation; `test-package-workspace.py` (`NAMES`) |
| `24787e1`, `a2da56a` | unpublished-crate precedent: `publish = false`, three LICENSE files, `packaged_notices` key, `vendor/dependencies.json`, DEPENDENCIES paragraph (a2da56a) |

**Checklist: every file that enumerates the crates or must change** (current
tree, `grep purrassist-host` / `five`):

- [ ] `Cargo.toml`: add `"crates/derive"` to `members`. Add
  `purrassist-derive = { path = "crates/derive", version = "0.1.0" }` to
  `[workspace.dependencies]`.
- [ ] `Cargo.lock`: new first-party entry. There is no third-party change, so
  `scripts/data/dependency-baseline.json` and `vendor/dependencies.json`
  `cargo_packages`/`cargo_details` stay unchanged.
- [ ] `crates/derive/{Cargo.toml,README.md,LICENSE-MIT,LICENSE-APACHE,LICENSE-MULAN,src/lib.rs,tests/derive.rs}`:
  - `[lib] proc-macro = true`, `[lints] workspace = true`, no `[dependencies]`,
    `[dev-dependencies] purrassist-core.workspace = true`.
  - All package fields come from the workspace, as in `crates/purrassist/Cargo.toml`,
    plus `documentation = "https://docs.rs/purrassist-derive"`.
  - The LICENSE files are byte-identical to the root. `check-repository.py`
    iterates every `crates/*` that has a Cargo.toml.
  - SPDX headers go in the first 5 lines of every `.rs`/`.toml`
    (`check-repository.py` `main()`).
- [ ] `crates/purrassist/Cargo.toml`: `purrassist-derive.workspace = true`.
- [ ] `crates/purrassist/src/lib.rs`: add
  `#[doc(hidden)] pub use purrassist_core as __core;` and
  `pub use purrassist_derive::{FacadeToolDefinition as ToolDefinition, FacadeToolEnum as ToolEnum};`.
  There is no macro-namespace clash: core has no `#[macro_export]`, and the
  facade's `pub use purrassist_core::*` exports `tool_call::ToolDefinition`
  only as a path under the module.
- [ ] `crates/purrassist/tests/derive_facade.rs`. This is the facade's first
  `tests/` file; `check-packages.py:60-63` requires every `tests/` file to be
  packaged.
- [ ] `crates/purrassist/README.md`: it is included as crate docs
  (`#![doc = include_str!("../README.md")]`), so any derive example there becomes
  a doctest.
- [ ] `scripts/check-release.py`: `RELEASE_ORDER` (see §0.1 on order).
- [ ] `.github/workflows/publish.yml:94`: the `for crate in …` loop. check-release
  requires it to equal `RELEASE_ORDER`. Also update the step name at `:88`,
  "Publish core, syntax, purrassist, host, cli in dependency order".
- [ ] `scripts/test-package-workspace.py:22`: `NAMES`.
- [ ] `scripts/test-verify-published.py`:
  - `CRATE_NAMES` (`:51`), `LIB_NAMES` (`:52-57`), `dependency_of` (`:211-217`).
  - `build_fixture_workspace` `members`/`chain` (`:79-130`).
  - Docstrings at `:6-7` and `:79` ("five-crate").
  - `dependency_of` is single-parent today. The facade now has two parents,
    syntax and derive, so the map and the registry-index writer must take a
    list.
  - A proc-macro fixture cannot export `pub fn marker()`. The derive fixture
    should be `proc-macro = true` and expose a trivial macro that the facade
    fixture invokes, so the proc-macro packaging and registry path is really
    exercised.
- [ ] `scripts/verify-published.py:9`: docstring "each of the five". It loads
  `RELEASE_ORDER` itself (`:64`).
- [ ] `scripts/package-workspace.py:76`: comment "five-crate release set". The
  logic is generic over the publishable packages.
- [ ] `scripts/check-packages.py`: notices come from `packaged_notices[crate_root.name]`,
  so the key is the directory name `derive`. Recommended: mirror the
  `purrassist-core` check at `:68-70` ("packaged core gained runtime
  dependencies") for derive.
- [ ] `vendor/dependencies.json`: `packaged_notices.derive = {}`.
- [ ] `scripts/check-repository.py` and `scripts/test-repository-audit.py`:
  - (a) the derive closure check (normal and build edges; §0.2);
  - (c) `check_ambient_access` also scans `crates/derive/src`. The message
    text is hard-coded "forbidden in purrassist-core" (`:142,:147`), so
    parameterize the label;
  - new fixtures beside `Closure`/`Ambient` (`test-repository-audit.py:50-125`).
- [ ] `docs/RELEASE.md`:
  - `:10-11`, the release set sentence;
  - `:21`, `:28`, `:42`, `:91`, "five";
  - `:86`, the publish arrow chain;
  - a new subsection for the external one-time steps for `purrassist-derive`:
    a `0.0.0` name reservation and a trusted publisher (repo
    `Blackcat-Informatics/purrassist`, workflow `publish.yml`, environment
    `crates-io`).
  - **These steps cannot be done or evidenced locally. Do not claim them.**
  - `.deficiencies` D2 already records that the new crate "needs crates.io name
    reservation + trusted publisher".
- [ ] `docs/IMPLEMENTATION.md:88-93`: "Workspace boundaries" bullet list.
- [ ] `docs/DEPENDENCIES.md`: a paragraph modeled on "Structural helper census"
  (`:273+`) and "Network-free, dependency-free core" (`:239+`). Include the
  derive closure and ambient scan bullets.
- [ ] `docs/RESOURCES.md:1083` "five publishable crates". Also the counts the
  spec did not list: `docs/QUALIFICATION.md:37` ("Five Cargo-verified
  archives") and `:227` ("all five downloadable archives").
- [ ] Remove `purrassist-derive` from "Still tracked in issue #4" in 4 places:
  - `README.md:74-79`
  - `docs/IMPLEMENTATION.md:56-59`
  - `docs/tool-calls/README.md:32-34`
  - `docs/QUALIFICATION.md:116-119`
  
  #4 is now a closed tracking issue. The other items there should point to
  #12/#13/#14/#16/#17/#18.
- [ ] `docs/tool-calls/INTEGRATION.md` "Compatibility" → "Rust API": list any
  core additions (trait, `FromNodeError` constructors) and the facade
  re-exports.
- [ ] Optional: `.github/workflows/verify.yml` windows job package lists
  (`:54-57`). The MSRV job already runs `cargo test --workspace` on stable
  1.98.0 with `-D warnings`, so derive code and its doctests must build on
  **stable 1.98**.
- [ ] Helper census: it walks every package under `crates/`
  (`docs/HELPER-CENSUS.md:48-63`), and strict `helper-check` is in `make check`.
  - A derive helper whose normalized body matches a core helper (string
    escaping, pointer escaping, name validation, number lexing) fails until it
    is restructured or ledgered as `[[distinct]]` in `helpers-ledger.toml`.
  - Cross-package `#[path]` source sharing is rejected, so derive cannot
    `#[path]`-include core's code.

## 2. "Subsume" precedent and reusable or mirrorable code

- **Philosophy:**
  - `.goals`: "SUBSUME, EXTEND, ENHANCE … subsume its functionality"; "RUST-FIRST".
  - `docs/DEPENDENCIES.md:3-7`: JSON, decimals, schema, regex, normalization
    and edit distance are all first-party.
  - `:239-271`: the audited zero-dependency core. "No dependency may be added
    without explicit maintainer approval."
- **First-party replacements on record:**
  - `f218e84`: dependency-free, ambient-free core audit.
  - `ea50c89`: dependency-free coverage-guided JSON fuzzing instead of cargo-fuzz.
  - `a2da56a` `crates/helper-census/src/{toml.rs,json.rs,hex.rs}`: strict subset
    readers.
  - `check-release.py` `validate_publish_workflow()`: deliberately "without a
    YAML dependency".
  - `crates/core/tests/pattern.rs`: first-party parser of test262 JS literals.
  - Generated Unicode and entity tables instead of libraries.
- **Closest pattern to mirror: `crates/helper-census/src/toml.rs`.** It is a
  strict subset reader that "refuses with its line number rather than guessing",
  so the subset can never mean something different from the reference parser.
  The derive's attribute grammar should work the same way: an explicit subset,
  with every other token a `compile_error!` at that token's span.
- **Lexer/parser shape to mirror: `crates/core/src/grammar/syntax.rs`.**
  - It has a `Token` enum, `Lexer<'a>`, `Parser`, positioned nodes, and one
    error constructor `syntax(offset, msg)`.
  - Derive gets tokens pre-lexed from `proc_macro`, so it needs only the
    `Parser` half, over a cursor of `TokenTree`.
- **Not reusable as a dependency.** Derive has zero deps; core's
  `json::parse`, `is_valid_name` and `pointer_segment` cannot be called at
  expansion time.
  - **Names:** canonical names are `[A-Za-z0-9_.:-]+`, so `json::pointer_segment`
    escaping is a no-op for every valid name. Escaping can be resolved at
    expansion time.
  - **JSON defaults:** `#[param(default = …)]` needs either a minimal first-party
    JSON-token validator in derive, or emitted code that calls
    `::purrassist_core::json::parse(TEXT)` at run time. Builders "never fail"
    (`definition.rs:6-8`), so the emitted parse must be unreachable-by-construction:
    validated at expansion, with core still authoritative through
    `ToolSet::compile_governed` checking the default against the type.
  - **Literal strings:** Rust string escapes differ from JSON
    (`\u{1F600}` vs surrogate pairs). The derive must unescape Rust literals
    itself. Doc comments arrive as `#[doc = r"…"]` *raw* strings.
- **Do not use `syn`/`proc-macro2`.** They are already in
  `[workspace.dependencies]` (Cargo.toml:16-17) for the unpublished helper census
  only (`vendor/dependencies.json` justification: "No dependency of
  purrassist-core"). Writing `syn.workspace = true` would be easy. It is forbidden
  by the spec and would fail audit (a).

## 3. The APIs a derive would call (as of `fccfbe7`)

### 3.1 `crates/core/src/tool_call/definition.rs`

All builders are infallible, and every constraint is checked by
`ToolSet::compile_governed`.

```rust
impl ToolDefinition {
    pub fn new(name: impl Into<String>) -> Self;            // effects default Mutating
    pub fn name(&self) -> &str;
    pub fn alias(self, alias: impl Into<String>) -> Self;   // repeatable, ordered
    pub fn description(self, d: impl Into<String>) -> Self;
    pub const fn effects(self, e: Effects) -> Self;
    pub const fn policy(self, p: RepairPolicy) -> Self;
    pub const fn unknown_parameters(self, h: UnknownParameters) -> Self;
    pub fn param(self, name: impl Into<String>, spec: ParameterSpec) -> Self; // decl order = properties order
}
// derives Clone, Debug, PartialEq, Eq  → "derived == hand-built" is a plain assert_eq!
impl Param { pub const fn string()/integer()/number()/boolean() -> ParameterSpec;
             pub fn enumeration(values: &[&str]) -> ParameterSpec;
             pub fn array(items: ParameterSpec) -> ParameterSpec;
             pub const fn object(object: ObjectSpec) -> ParameterSpec; }
impl ParameterSpec { pub const fn required(self); pub fn alias(self, _); pub const fn repeatable(self);
                     pub fn default(self, value: json::Node); pub fn description(self, _);
                     pub fn pattern(self, _); }   // all -> Self, #[must_use]
impl ObjectSpec { pub const fn new() -> Self; pub fn param(self, name, spec) -> Self;
                  pub const fn unknown_parameters(self, h) -> Self; }  // unset = inherit from tool
pub enum Effects { ReadOnly, Mutating /*default*/ }
pub enum RepairPolicy { Structural, Tolerant }
pub enum UnknownParameters { Error, DropWithLoss, PassThrough }
```

Attribute → call map:

- `effects = mutating|read_only` → `Effects::{Mutating,ReadOnly}`.
- `policy = structural|tolerant` → `RepairPolicy::{Structural,Tolerant}`.
- `unknown = error|drop|pass` → `UnknownParameters::{Error,DropWithLoss,PassThrough}`.
- `repeat` → `.repeatable()`.
- `default = …` → `.default(Node)`.

`ParameterSpec` placement rules are compile-time errors in core. They should be
mirrored at expansion so the error carries a span:

- `repeatable` only on non-array parameters. A `#[param(repeat)]` field is
  `Vec<T>` and maps to `spec(T).repeatable()`, **not** `Param::array(spec(T))`.
- Array items cannot be required, aliased, repeatable or defaulted, so
  `Vec<Option<T>>` has no representation.
- `pattern` applies only to plain strings. The spec has no `pattern` attribute.
  That is a gap to note, not to invent.

Integer targets map to `Param::integer()` with no range keywords (none exist).
For example, `u8` accepts 300 at schema level and fails in `FromNode`. Document
this.

### 3.2 `crates/core/src/tool_call/from_node.rs` (shipped in PR #5 / `943fa04`)

```rust
pub trait FromNode: Sized { fn from_node(node: &json::Node) -> Result<Self, FromNodeError>; }
pub struct FromNodeError { pub pointer: String, pub expected: &'static str, pub found: String }
// impls: String, bool, json::Number, f64, json::Node, Option<T>, Vec<T>,
//        i8..i128, isize, u8..u128, usize   (no f32, no Box, no tuples)
```

- `Option<T>` maps only JSON `null` to `None`. **An absent member must be
  handled by the generated code.**
- `Vec<T>` prefixes the failing index through private `within`.
- The `expected` strings are `'static` literals. Generated code can supply
  literals such as `"object"`, or an enum's `"one of \"asc\", \"desc\""`.

### 3.3 `json` (`crates/core/src/json/mod.rs`)

- `Node { pub value: Value, pub span: Option<Range<usize>> }`.
- `Value::{Null,Bool,Number,String,Array(Vec<Node>),Object(Vec<Member>)}`.
- `Member { key, key_span, value }`.
- `Node::new(Value)`, `Node::get` (None on duplicates), `as_str`.
- `pub fn pointer_segment(&str) -> String`.
- `json::parse(&str) -> Result<Node, ParseFailure>`.
- `Number: FromStr` (number.rs:247).

### 3.4 How recovered arguments reach the struct

- `ToolCall.arguments: Node` is "resolved and canonical when the call resolved,
  otherwise the best tree resolution reached" (`recover.rs:1457-1459`).
  Canonical names mean aliases are already resolved, so derived `FromNode` reads
  canonical names only.
- `ToolCalls::usable()` (`recover.rs:1692`) is empty after a failed tool-choice
  check.
- A typed conversion must not launder a `BestGuess`. Examples and docs must
  gate on outcome/`usable()` (AGENTS.md: "A validated guess is still a guess").
- **Defaults are never applied by recovery** (`ParameterSpec::default` doc: "No
  repair ever applies it"; `ToolSet::defaults`). Decision needed: does derived
  `FromNode` apply a declared default for an absent non-`Option` field (a
  caller-side conversion, not a repair), or must defaulted fields be `Option<T>`?
- **Unknown members under `unknown = pass`:** these survive into resolved
  arguments, and a struct has nowhere to put them. The spec has no catch-all
  attribute. Decision needed: either reject `pass` on derived structs, or have
  `FromNode` error on undeclared members. Silently dropping them would be an
  unreported loss.

### 3.5 What the generated code needs that core does not yet expose

- **A dispatch point for non-primitive field types.**
  - Option A: a core trait. Examples: `ToolParameter { fn parameter_spec() -> ParameterSpec; }`
    implemented in core for the primitives and generated for derived structs
    (`Param::object(..)`) and enums (`Param::enumeration(..)`), plus
    `ToolArguments: FromNode { fn object_spec() -> ObjectSpec }` and an
    entry point for the top-level `ToolDefinition`.
  - Option B: inherent `#[doc(hidden)]` associated fns with a reserved name.
  - A trait also makes the map robust to aliases (`type Path = String`) and
    user types named `String`.
  - "Builder calls only, plus FromNode" (spec) does not name the entry point.
    This is an open design point.
- **Public `FromNodeError` constructors and pointer prefixing** (§0.3).
- A trait with an associated const (e.g. nesting depth) would also turn
  **indirect type recursion** into a compile-time cycle error instead of a
  run-time stack overflow (§3.6).

### 3.6 #8 / PR #9 (`fccfbe7`; feature commits `c168175`, `28b2b3a`, `248d07a`, `e93eaf8`, `a49e81b`) and what matters for derive

- **Arrays of nested-parameter objects** (`c168175`): `Vec<NestedStruct>` →
  `Param::array(Param::object(ObjectSpec…))` is now recovered correctly from
  Qwen3 repeated `<parameter=>` items, so the derive should support it and test it.
- **Scope graph** (`scope.rs:4-21`): "the scope graph of an **adapter-built**
  tool". `$ref`/`anyOf` from wire declarations only.
  - The typed builder (`ObjectSpec`) is a tree and has no `$ref`. A recursive
    derived type is therefore inexpressible, and `object_spec()` would recurse
    without bound.
  - Direct self-reference (`Vec<Self>`, `Vec<SameIdent>`) can be rejected
    syntactically. Indirect recursion needs the trait-const trick or must be
    documented.
  - An unbounded run-time recursion would violate "resource exhaustion is an
    explicit outcome".
- **Strict forced choice** (`28b2b3a`, `ToolChoice::ForcedStrict(StrictForced{..})`):
  this is request-side, not definition-side, so it needs no derive surface.
  Derived definitions do supply the `name` that `ForcedStrict.name` refers to.
  `ToolDefinition::name()` is the accessor.
- **Rust API break note** (`fccfbe7` body): new pub fields on `ArgumentShape`/
  `ParameterShape`. A derive emits only builder calls and must not construct
  shapes, which keeps it insulated.
- **`42899ab`**: one `by_name` binary-search lookup for tools and scope members
  (`tool_call/mod.rs:104-110`). Results never depend on declaration order
  (`mod.rs` docs). Derive must still emit fields in declaration order, because
  that order is the rendered `properties` order.

## 4. Prior attempts and deferral

- No derive code, branch or proc-macro has ever existed:
  - `git log --all -S 'ToolEnum'` → none;
  - `git log --all -S 'purrassist-derive'` → only docs (`cfe9366`) and the PR #9
    stage artifacts (`8a5cd64`);
  - the only `proc_macro`/`proc-macro2` use is the helper census.
- **First mention:** `cfe9366` (2026-10-01, docs). "Streaming recognition,
  purrassist-derive, and the remaining dialects are tracked in #4."
- **Decision record:** `/home/paudley/Active/purrassist/.deficiencies` D2
  (2026-10-01).
  - Text: "purrassist-derive crate not built (#[derive(ToolDefinition)]/ToolEnum,
    facade re-export, release registration)."
  - Decided by Patrick Audley, who answered "Split: slice now, rest to new
    issue (Recommended)".
  - "Shipped code offers the builder API and FromNode; no derive macro.
    Forward path: Deferred Spec A in issue #4 (new crate needs crates.io name
    reservation + trusted publisher)."
- `943fa04` (PR #5) body: "Issue #4 holds the maintainer-authorized remaining
  scope: streaming recognition, purrassist-derive, …".
- The later D3 revision (2026-10-02) left D1 and D2 unchanged. The approved
  stage-2 plan restated "Still in #4 … streaming, and derive."
- **Why it was deferred:** scope and time (a PR slicing decision), plus the
  external crates.io prerequisites. No technical objection is recorded.
  #15 now carries #4 "Spec A" and design "D12" verbatim. #4's own body was not
  refetched.

## 5. Recurrence

From `git log --format='%(trailers:key=GhpRsq-Defect-Class,valueonly)'`:

- Exactly **1 distinct class, `performance`, with 1 instance**, on `fccfbe7`
  (PR #9). This holds over the last 200 commits and the whole history (190
  commits). It agrees with `brief.json`.
- `943fa04` (PR #5) has GhpRsq trailers but no Defect-Class.
- No derive- or macro-related defect class exists. The recurrence is
  thematically relevant all the same: #8's stage 2 found several quadratic and
  uncharged paths.
- The derive's generated `FromNode` should be one pass over `members` with a
  generated `match key.as_str()`. That makes it O(members + fields) and tells
  missing from duplicate.
- Expansion cost should be linear in input tokens.

## 6. External prior art (model knowledge; not verified in this session; learn from, never depend on)

- **serde_derive:**
  - Hygiene by absolute paths into a hidden re-export (`serde::__private`). This
    is the same idea as the facade's `__core`.
  - Generated locals use `__`-prefixed names, so a user `const` named like a
    local cannot turn a `let` into a const pattern. With `proc_macro`, prefer
    `Span::mixed_site()` for generated locals.
  - `#[automatically_derived]` on impls.
  - Attribute errors are collected (a `Ctxt` that accumulates) and all reported,
    not just the first.
  - `rename`/`alias`/`default`/`skip`; `r#ident` fields are stripped of `r#`
    for the wire name. Derive must do the same: `Ident::to_string()` yields
    `"r#type"`.
  - The `#[serde(crate = "…")]` escape hatch is what D12 deliberately avoids
    with two macro pairs. The cost is that renamed dependencies
    (`foo = { package = "purrassist" }`) will not resolve. Document this.
- **schemars:**
  - Doc comments become `description`. Decide whether `///` feeds
    `description` when no attribute is given.
  - Handles recursion via `$defs`/`$ref`, which the typed builder lacks.
  - Respects serde attributes. Not applicable here.
- **Zero-dependency derive helpers:**
  - `virtue` (used by bincode 2): a dependency-free derive helper over
    `proc_macro`, with generation by building source text and `parse()`.
  - `nanoserde`'s derive: a hand-written parser over `proc_macro` and
    string-formatted output parsed via `FromStr`. This is exactly the D12
    "writer through FromStr" design, and it shows the weakness: all output
    spans are `call_site`.
  - `venial`: a declaration-only parser where types stay opaque token lists. It
    is a good scope model, though it depends on `proc-macro2`.
  - `litrs`: parses Rust literal tokens and unescapes strings, including raw
    and byte strings.
  - `manyhow` / `proc-macro-error`: emit `compile_error!` *alongside* a
    best-effort expansion so one error does not cascade into dozens of
    unrelated ones.
- **`proc_macro` pitfalls:**
  - `proc_macro` APIs panic outside a macro invocation. Unit tests in
    `crates/derive/src` cannot construct `TokenStream`. Keep the parser and
    writer logic over a small internal token model, converted at the boundary,
    so it can be unit-tested. Macro behavior is covered by integration tests and
    doctests.
  - `TokenStream::from_str` returns `Result<_, LexError>`. Build string literals
    with `Literal::string(..)`, never `format!("\"{}\"", s)` (escaping bugs).
  - `Group::set_span` sets only the delimiter span. Re-spanning output means
    walking it recursively.
  - Splice the user's original type tokens into the output rather than
    re-printing them. Re-printing loses spans, so a `T: FromNode` failure would
    point at the derive instead of the field.
  - `Delimiter::None` groups (types passed through `macro_rules!` `$t:ty`) must
    be entered transparently.
  - Derive input is already cfg-expanded. Foreign attributes (`#[doc]`,
    `#[serde]`, `#[allow]`) must be ignored, not rejected. "Unknown attributes
    → compile_error" should apply to unknown *keys inside* `#[tool(..)]` and
    `#[param(..)]`.
  - Declare helper attributes: `#[proc_macro_derive(ToolDefinition, attributes(tool, param))]`
    on both pairs.
  - The usual way to span an error: emit `compile_error!{"msg"}` with every
    token spanned at the offending token. Use first and last token spans to
    cover a range.
  - Do not use unstable `Diagnostic`, `Span::def_site`, or (verify on 1.98)
    `Span::join` or `Literal` value accessors. Derive must build on stable 1.98.
  - Generated code must not depend on the prelude: use `::core::option::Option`,
    `::core::result::Result`, `::std::vec::Vec`, `::std::string::String`. It
    should be warning-clean under the workspace's pedantic/nursery Clippy and
    `missing_docs`: generated public items need `#[doc(hidden)]` or docs.
  - Leading `::crate` paths require edition ≥ 2018 in the user crate.
- **`compile_fail` doctests:**
  - They pass on *any* error, including an unresolved import or a typo. Pair
    every `compile_fail` block with a near-identical passing block (a positive
    control) that differs only in the rejected construct.
  - Error-code checking (`compile_fail,E…`) does not apply to `compile_error!`
    and is nightly-only.
  - Doctests in a proc-macro crate can invoke its own macros and see
    dev-dependencies, so `purrassist_core` resolves.
  - No `compile_fail` doctest exists in the repo yet (`grep -rn compile_fail crates` → none). This sets the convention.
  - `trybuild` (exact-stderr UI tests) would be a new dependency and is
    excluded.
- **Cargo publish (knowledge):** dev-dependencies that have a `version` are kept
  in the published manifest and must resolve from the registry. Path-only
  dev-dependencies are stripped. This bears on §0.1.

## 7. Constraints for the implementation

1. **Zero dependencies, verified.** `crates/derive/Cargo.toml` has no
   `[dependencies]` or `[build-dependencies]`, including target-specific ones.
   There is no `syn`, `quote`, `proc-macro2`, `litrs`, `virtue` or `venial`, even
   though `syn`/`proc-macro2` sit in `[workspace.dependencies]`. The only
   `extern` is the built-in `proc_macro`. Audit (a) enforces an empty
   normal and build closure. The one permitted dev edge is first-party
   `purrassist-core`.
2. **Core stays derive-free.** `purrassist-core` never depends on derive, not
   even as a dev-dependency: `check_core_dev_dependencies` and
   `check_core_closure` (normal/build/dev) would fail. Any trait or
   `FromNodeError` helper added to core must:
   - be first-party and pure (no ambient access);
   - carry Rustdoc with meaningful examples and error semantics (AGENTS.md);
   - be listed in INTEGRATION.md "Compatibility".
3. **MSRV and toolchain.** Edition 2024, `rust-version = 1.98`. Derive and every
   doctest must build and pass on **stable 1.98.0** with `-D warnings` (verify.yml
   `msrv` and release-qualification), not only on `nightly-2026-09-14`. Use no
   unstable `proc_macro` APIs.
4. **Hygiene and paths.**
   - `ToolDefinition`/`ToolEnum` emit only `::purrassist_core::…`.
   - `FacadeToolDefinition`/`FacadeToolEnum` emit only `::purrassist::__core::…`.
   - Share one generator parameterized by the root path, so the pairs cannot
     drift.
   - Use fully qualified std/core paths, `Span::mixed_site()` for generated
     locals, `#[automatically_derived]`, and no reliance on user-scope names.
   - Renamed dependencies are unsupported by design, with no `crate =`
     attribute. Document this.
5. **Deterministic output.**
   - The same input tokens produce the same output text.
   - Fields, aliases and variants are emitted in declaration order, which is
     the rendered `properties` order.
   - No `HashMap`/`HashSet`/`RandomState`, no counters, no env, time, fs or
     process access. The ambient scan (c) extends to `crates/derive/src`.
   - Native and WASM output are identical, because the generated code is plain
     builder calls.
6. **Error spans.**
   - Every rejection is a `compile_error!` spanned at the offending token. This
     covers generics/lifetimes/where clauses, tuple structs, unions, data
     enum variants, empty enums, unknown keys in `#[tool]`/`#[param]`,
     duplicate keys, bad `effects`/`policy`/`unknown` values, `repeat` on
     non-`Vec`, `Vec<Option<T>>`, unsupported types (`f32`, `Box`, tuples,
     `Node`), invalid names (not `[A-Za-z0-9_.:-]+`), malformed JSON defaults,
     and direct self-recursion.
   - Accumulate errors rather than stopping at the first.
   - Splice user type tokens so trait-bound errors land on the field.
   - Decide on unit structs and `struct X {}`. A tool with zero parameters is
     valid in core.
7. **Contract fidelity.**
   - The derived definition `==` the hand-built one, and the generated code is
     builder calls only.
   - Effects default to `Mutating`. Never infer `read_only`.
   - `FromNode` is exact (no coercion) and single-pass (§0.5). It reports
     missing vs duplicate vs undeclared members with RFC 6901 pointers.
   - It never applies a default silently unless that is the decided and
     documented semantics.
   - Docs and examples gate typed conversion on the call outcome, because a
     guess stays a guess.
8. **Publish order and registration.**
   - Resolve §0.1. Recommended order: core → derive → syntax → purrassist →
     host → cli.
   - Change `RELEASE_ORDER`, the `publish.yml` loop and step name, `NAMES`,
     `CRATE_NAMES`/`LIB_NAMES`/`dependency_of` (multi-parent) and the
     fixture chain together.
   - Then run `check-release.py`, `test-package-workspace.py` and
     `test-verify-published.py`.
   - The crates.io name reservation and trusted publisher are external steps.
     Document them in RELEASE.md and claim neither.
9. **Dependency registration rules (AGENTS.md / DEPENDENCIES.md).**
   - No third-party package enters `Cargo.lock`, so `dependency-baseline.json`
     and the `cargo_packages`/`cargo_details` inventory stay unchanged.
   - Add `packaged_notices.derive = {}`.
   - Add a DEPENDENCIES.md paragraph stating derive's empty closure, the scope
     of the scan, and that no material is reused from external derive crates.
     Section 6 is design learning only. If any code is ported, record
     provenance as in `first_party_ports`/`UPSTREAM.json`.
10. **Repository gates.**
    - SPDX headers on every new `.rs`/`.toml`; three byte-identical LICENSE files.
    - Rustdoc with examples on both macro pairs (the crate's public API).
    - Strict `helper-check` must pass. Avoid normalized-body clones of core
      helpers, or ledger them with a reason.
    - `make check` is the gate (fmt, hygiene, clippy `--all-targets -D
      warnings`, helper-test/check, workspace test, doc). The six spec commands
      are necessary, not sufficient.
11. **Docs must match reality.** Make these changes:
    - update the "five" → "six" sites (§1, including the QUALIFICATION.md
      sites the spec missed);
    - remove `purrassist-derive` from the four "Still tracked in issue #4"
      sentences;
    - add the IMPLEMENTATION.md "Workspace boundaries" entry.
    
    Distinguish implemented behavior from the unexecuted publication steps.
