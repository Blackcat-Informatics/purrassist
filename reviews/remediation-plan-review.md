# Review: Stage 2 remediation plan, PR #19 (issue #15, purrassist-derive)

**Reviewed:** `.stage/purrassist-derive-zero-dependency/remediation-plan.md` (head e4850a8).
**Sources checked against:** `gap-analysis.md`, `completion-audit.md`, `reviews/structure-review.md`, `historical-fit.md`; contract `issue.md`, `plan.md` (D-1..D-14, user decisions verbatim), `validation.md`; rules `AGENTS.md`, `.goals`, `docs/RELEASE.md`.
**Code verified:** `crates/derive/src/{write,model,lib}.rs`, `crates/core/src/tool_call/{derive_support,definition,from_node,toolset}.rs`, `crates/core/src/json/number.rs`, `crates/purrassist/{Cargo.toml,src/lib.rs}`, `crates/derive/{Cargo.toml,README.md}`, root `Cargo.toml`, `scripts/check-release.py`, `scripts/test-verify-published.py`, `.github/workflows/publish.yml`, `Makefile`, `verify.yml`.
Read-only review; nothing was built.

## Coverage: every source finding, where the plan maps it

| Source finding | Plan item | Mapped? |
|---|---|---|
| G1 nested PassThrough inheritance (HIGH) | R2 | yes, design incomplete (F1) |
| G2 hosted CI did not run | "reported; validation is local" | yes, no recheck step (F12) |
| G3 publish preflight | R5 | yes, design incomplete (F2) |
| G4 explicit enum description + variant lines test | R6 | yes |
| G5 same-version, `purrassist` name, usable/resolved docs | R4 | yes |
| G6 derive README doctest, root README drift | R6 | yes (F13) |
| G7 compile_fail error codes / diagnostics harness | R6 + Declined | partially; declination rationale inaccurate (F6) |
| G8 `.deficiencies` D2 record | Stage 3 | yes |
| G9 doc line widths | R6 | yes |
| Gap (b) optional ABI marker | R4 | yes |
| Gap (c) optional `seen` bitset note | none | not a gap; acceptable to omit |
| Audit A-1 E0308 noise on `extra` | R6 | yes, mechanism wrong (F4) |
| Audit A-2 PR body: variant doc whitespace collapse | **none** | **dropped** (F5) |
| Audit A-3 README not a doctest | R6 (G6) | yes |
| Audit A-4 plain compile_fail | R6 (G7) | yes |
| Structure F-1 format-string / non-absolute `assert!` | R1 | yes |
| Structure F-2 bare primitives | R1 | yes |
| Structure F-3 field/variant idents as text | R1 | yes |
| Structure F-4 sorted-table check, `Members` doc | R3 | yes |
| Structure F-5 `ToolExtra` unusable | R3 (seal) | yes |
| Structure F-5 `ToolParameter` defaults for `REQUIRED`/`absent`/`DEPTH`, "future items must be defaulted" | **none** (only "relation stated in docs") | **dropped** (F5) |
| Structure F-5 `Slot` non-exhaustive | R3 | yes |
| Structure F-5 consider `#[doc(hidden)]` for `Members`/`Slot` | **none** | **dropped** (F5) |
| Structure F-5 version skew | R4 | yes |
| Structure F-5 `f32`/`Box<T>` | R3 | yes (F7) |
| Structure F-5 crate path override | R4 | yes (F10) |
| Structure F-6a `arguments(ObjectSpec)` setter | Declined, reasoned | yes |
| Structure F-6b number grammar cross-check | R3 | yes |
| Structure F-6 inconsistent name-validation timing (parameter keys) | **none** | **dropped** (F5) |
| Historical-fit risks 1-14 | adjudicated in gap analysis; 10 (evidence reuse) covered by settle step | yes |

## Findings

### F1 REQUIRED. R2: the nested default is undecided, the acceptance is an either/or, and option (b) is adopted without the user decision the gap analysis called for

Facts verified in code:

- Core's tool-level default is effects-based: `Effects::default_unknown_parameters()` returns `Error` for `Mutating` and `DropWithLoss` for `ReadOnly` (`definition.rs:53-66`), applied by `ToolDefinition::resolved_unknown_parameters` (`definition.rs:616-619`). The derive's documented default effects is `mutating` (`lib.rs:95`).
- A nested `ObjectSpec` with `unknown: None` inherits the enclosing resolved handling at compile time (`toolset.rs:1542-1546`, `object.unknown.unwrap_or(inherited)`), and the derive emits `.unknown_parameters(..)` only when the key is present (`write.rs:163-175`, used at `:214` and `:218`).
- The top level has **no** mismatch: under `Error` recovery rejects the undeclared member (call not `Resolved`), under `DropWithLoss` recovery drops it (`BestGuess`, member absent), so the generated `FromNode` never meets an undeclared member in a `Resolved` top-level call. Only nested objects under a `PassThrough` ancestor do.
- Therefore the explicit value must be emitted in `object_spec()` only. If `tool_definition()` also emitted it, a `#[tool(effects = read_only)]` tool without `unknown` would flip from `DropWithLoss` to `Error`, and every derived-equals-hand-built equality test would break. The plan's "every derived `object_spec()`" is right; "keep top-level behaviour unchanged unless it has the same mismatch" should become an unconditional "`tool_definition()` is unchanged".

What the plan leaves open:

> "the declared value, or the derive's documented default when no `unknown` key is given."
> "Read `definition.rs` for the tool-level default and keep top-level behaviour unchanged unless it has the same mismatch; document the rule."

The "documented default" does not exist yet and is the whole decision. There are three coherent candidates, and they differ in user-visible recovery outcomes: (i) `Error` always; (ii) mirror the struct's own `effects` (unset/`mutating` -> `Error`, `read_only` -> `DropWithLoss`), which keeps `object_spec()` and `tool_definition()` of one struct consistent but contradicts D-4's "effects ... ignored for nesting"; (iii) `DropWithLoss` always (a reported loss, never a failed conversion). Each changes today's behaviour for a nested derived struct under a `read_only` tool (today it inherits `DropWithLoss`; under (i) or (ii) an extra nested member makes the whole call `Unresolved`). The gap analysis said of this option: "That changes recovery semantics for nested objects and needs a user decision." The plan adopts it silently and drops option (a) (`INHERITS_UNKNOWN` const plus a compile-time assertion in `unknown = pass` derives), which preserves inheritance for `error`/`drop` tools and fails fast at the only unsafe combination.

The acceptance is unfalsifiable as written:

> "Expected: recovery rejects or drops the nested extra member per the nested policy, the call outcome matches it, and `from_node` succeeds whenever the call is `Resolved`."

"Rejects or drops ... per the nested policy" accepts either outcome; "the call outcome matches it" is circular. Once the default is fixed the expectation is determinate.

REWRITE TO (R2):

- "**Decision (post to #15 before implementing, as a recorded decision beside D-1..D-14):** either (a) `ToolParameter::INHERITS_UNKNOWN: bool` (default `false`; `true` for a derived struct with no `unknown` key; `Vec`/`Option`/`Box` forward it) and a `#[tool(unknown = pass)]` derive emits per field `const _: () = ::core::assert!(!<F as ToolParameter>::INHERITS_UNKNOWN, "{}", "field `f` holds a struct that would inherit unknown = pass but has no #[param(extra)]; give it #[tool(unknown = error|drop)] or pass + extra")`; or (b) `object_spec()` always emits `.unknown_parameters(X)` where X is <one of (i)/(ii)/(iii), named>. `tool_definition()` is unchanged in both. Hand-built `PassThrough` tools embedding a derived `parameter()` are documented on `ObjectSpec`/`ToolParameter` and in INTEGRATION.md."
- "**Tests (derive, real `ToolSet::compile` -> `recover` hermes -> `from_node`), expectations written before running:** for `Outer { inner: Inner, items: Vec<Inner>, #[param(extra)] rest: Node }` at `unknown = pass` with the turn `{"inner":{"a":1,"zz":2},"items":[{"a":1,"zz":2}],"top":3}`:
  1. `Inner` with `#[tool(unknown = error)]`: outcome is not `Resolved`; the unresolved reason names `/inner/zz` (first failure).
  2. `Inner` with `#[tool(unknown = drop)]`: outcome `BestGuess` with a recorded loss at `/inner/zz` and `/items/0/zz`; `Outer::from_node` is `Ok` with `rest == {"top":3}`.
  3. `Inner` with `#[tool(unknown = pass)]` + its own `extra`: outcome `Resolved`; `from_node` is `Ok`, `inner.extra == {"zz":2}`, `items[0].extra == {"zz":2}`, `rest == {"top":3}`.
  4. `Inner` with no `unknown` key: under (a) a `compile_fail,E0080` doctest with the message above and a twin where `Inner` declares `unknown = error`; under (b) the outcome equals case <(i)->1, (ii)->1, (iii)->2> exactly.
  Repeat 4 with the top level at `unknown = drop` and at `effects = read_only` with no key. Each test fails on e4850a8 (the gap analysis already observed `Resolved` + `Err(/inner/zz)`)."
- "**Churn to list:** every hand-built nested `ObjectSpec` compared for equality (derive tests, facade tests, facade README `Window`, `derive_support.rs` doc examples) gains the explicit call under (b); `lib.rs:113-117` (D-4 text), `definition.rs:403-406` (`ObjectSpec` doc) and INTEGRATION.md are updated under either option."

### F2 REQUIRED. R5: the preflight is an either/or, and name existence alone does not close G3

> "It fails unless each name exists and is owned (reserved) and the trusted publisher is configured, as far as the public API shows. If ownership or trusted-publisher state cannot be checked unauthenticated, the step checks existence of the name and the docs state the limit."

Concrete answer to the open question, so the plan can commit:

- `GET https://crates.io/api/v1/crates/{name}` is public; 200 means the name exists (any owner). Already used by `publish.yml:99` with the `user_agent`.
- `GET https://crates.io/api/v1/crates/{name}/owners` is public (no token); it returns `users: [{login, kind: user|team, ...}]`. Ownership is therefore checkable unauthenticated.
- Trusted-publisher configuration (`/api/v1/trusted_publishing/github_configs?crate=`) requires an authenticated owner session; the short-lived token from `crates-io-auth-action` is minted if *any* crate in the registry trusts this repository/workflow/environment, so a successful auth step does not prove `purrassist-derive` is covered. This cannot be verified in CI; it stays a documented manual pre-tag check (RELEASE.md:172-178 already says so).
- Existence alone is insufficient for G3's purpose ("fail before core is published"): a squatted `purrassist-derive` returns 200 and the loop still fails at derive after core is public, exactly the half-publish G3 names. Ownership closes that.

REWRITE TO (R5): "A step **before** `Authenticate with crates.io trusted publishing` runs, for every crate in `RELEASE_ORDER`: `GET /api/v1/crates/{name}` must be 200, and the set of owner logins from `GET /api/v1/crates/{name}/owners` must equal the owner set of `purrassist-core` (no login is hard-coded in the repository; the release set must be owned identically). Both requests send the existing `user_agent`. Any failure exits non-zero before any `cargo publish`. Trusted-publisher state is not verifiable unauthenticated; RELEASE.md keeps it as the manual pre-tag step and the preflight's comment says so. `validate_publish_workflow()` requires the step, the owners URL and that the step precede both the auth step and the publish loop. **A new `scripts/test-check-release.py`** (none exists today; `scripts/test-*.py` has no check-release harness) runs `validate_publish_workflow()` against the real file and against copies with the preflight removed, moved after `cargo publish`, and with the owners check removed, expecting failure for each; it is added to `make check` (Makefile `hygiene` list beside `check-release.py`) and to `verify.yml`'s native job beside `test-verify-published.py`."

### F3 REQUIRED. R4: `=0.1.0` pinning conflicts with existing tooling the plan treats as hypothetical

> "`check-release.py` and the package checks are updated if they assume caret requirements."

They do. `check-release.py:127-129` rejects any internal requirement other than `^{version}`. `scripts/test-verify-published.py:242` builds a fixture dependency record with `'req': f'^{version}'` and `:116`/`:154` generate fixture manifests with `version = "{version}"` (caret). Also, the facade takes both crates through `[workspace.dependencies]` (`crates/purrassist/Cargo.toml:21-22`, root `Cargo.toml:19-20`); `workspace = true` cannot override `version`, so pinning in the workspace table pins **every** internal edge (`syntax -> core`, `host -> purrassist`, derive's dev edge), while pinning only the facade requires non-workspace dependency entries there. Either is defensible; the plan must choose and say which.

REWRITE TO (R4 version pinning): "The facade declares `purrassist-derive = { path = \"../derive\", version = \"=0.1.0\" }` and the same for `purrassist-core` as package-local entries (or: all `[workspace.dependencies]` internal entries become `=0.1.0`; state the choice and the user-facing consequence, since `=` on every edge forces downstream lockstep across the whole set). `check-release.py` is changed to require `={version}` on the facade's derive and core edges and `^{version}` elsewhere (or `=` everywhere, matching the choice), and the new `test-check-release.py` carries a negative case per rule. `test-verify-published.py` fixtures are changed to the same shape so the offline proc-macro fixture mirrors the real manifests. Acceptance: `check-release.py` exits 0; a copy of the facade manifest with `^` on the derive edge fails it; `test-verify-published.py` passes with the pinned fixture."

### F4 REQUIRED. R6 A-1: the proposed mechanism does not produce the claimed result

> "the derive emits a `ToolExtra` bound assertion on the extra field type before using it, so only the E0277 on the field type appears."

The bound assertion already exists (`write.rs:235-241`, `extra_check`). The E0308 comes from the struct literal `{ident}: Node::new(Value::Object(__pa_extra))` at `write.rs:306-311`; rustc reports both errors regardless of emission order. The audit's own suggestion is the correct one and dovetails with R3's sealing.

REWRITE TO: "`ToolExtra` (sealed in R3) gains `#[doc(hidden)] fn __from_members(members: Vec<Member>) -> Self`, implemented for `Node`. Generated code assigns `<{ty} as ToolExtra>::__from_members(__pa_extra)` so a wrong `extra` type yields only E0277 at the field type. Acceptance: compile_fail doctest 30 is annotated `E0277`, and the standalone build of snippet 30 (the audit's method) shows one error, not two."

### F5 REQUIRED. Four source findings are dropped without a decision

Each needs an owner line (adopt in an R-commit or move to "Declined (with reasons)"):

1. **Audit A-2:** the PR body describes the variant-description composition but not the whitespace collapse (`lib.rs:141-143` documents it). One sentence in the PR body; attach to R6.
2. **Structure F-5, `ToolParameter` defaults:** "give defaults (`REQUIRED = true`, `absent() = None`, `DEPTH = 0`), state the invariant, note future items must be defaulted". The plan keeps only "the `REQUIRED`/`absent()` relation is stated". Defaults are additive and cheap (`derive_support.rs:137-156`); adopt in R3 or decline with a reason.
3. **Structure F-6, name-validation timing:** `resolve_param` synthesizes the parameter key from the field ident (`model.rs:527-530`) without `is_valid_derived_name` (`model.rs:282`, used for tool names at `:475`), so a non-ASCII field name falls through to the const assertion spanned at the `#[derive]`, which the handoff called the weakest error experience. Applying the same check there gives a spanned error with the same remedy. Adopt in R1 (it is a hygiene/diagnostic change) or decline.
4. **Structure F-5, `#[doc(hidden)]` for `Members`/`Slot`:** decide and record. Given R3 makes `Slot` non-exhaustive and documents `Members` as codegen-shaped, "keep public, documented as generated-code support" is a fine answer, but it must be written down.

### F6 REQUIRED. G7 declination rests on an inaccurate rationale (PLANNED-BY-REFUSAL)

> "A first-party compile-diagnostics UI harness (G7, full form). It would duplicate the D-12 pure-layer message tests."

The pure-layer tests cover `parse.rs`/`model.rs` messages. They do not cover `bridge.rs` (token-to-span index, `compile_error!` emission). A bridge regression leaves every `compile_fail` doctest failing (for a different reason) and every twin passing, so it is invisible to the committed suite; the gap analysis's acceptance names exactly this ("changing ... the span index in `bridge.rs:34-39` makes it fail"). `proc_macro` cannot be driven from unit tests, so an external compile is the only route.

REWRITE TO (one of): "Commit `scripts/test-derive-diagnostics.py` (dependency-free; builds each `rejected_input` snippet in a throwaway crate; asserts the first error's message prefix and `line:col`), run in `verify.yml`'s `qualification` job, not `make check`." Or keep the declination but state the residual accurately: "The bridge's span selection and `compile_error!` emission are covered only by twins; a regression there would surface as a different first error. Disclosed in the PR body under residual risks." The second is acceptable for a LOW gap; the current wording is not.

### F7 ADVISORY. R3 `f32` and `Box<T>` specifics

- `Number` is an exact decimal spelling (`number.rs:9`); every `f32` value has a finite decimal expansion, so the *write* direction is exact. The *read* direction (`FromNode for f32`) is round-to-nearest, like `f64` (`from_node.rs:201-214`). "exact through `Number`" is misleading; say: `parameter()` is `Param::number()`; `from_node` parses the spelling with `str::parse::<f32>` directly (correctly rounded; do not go through `f64` then `as f32`, which double-rounds), rejects non-finite, `expected: "number within the finite f32 range"`.
- There is no `FromNode for Box<T>` (`from_node.rs` has `String`, `bool`, `Number`, `f64`, `Node`, `Option`, `Vec`, integers). `ToolParameter: FromNode`, so R3 must add `impl<T: FromNode> FromNode for Box<T>` plus `ToolParameter`/`ToolItem` for `Box<T: ToolItem>` forwarding `REQUIRED`, `absent`, `DEPTH`. Note in docs that `Box` does not enable recursion: `Option<Box<Self>>` is already a spanned error (audit row 15) and indirect cycles through `Box` remain E0391; add that compile_fail with a twin.

### F8 ADVISORY. R3 `Slot` non-exhaustive: list the other matchers and the wildcard's error shape

Core's own doctests (`derive_support.rs:65-74`, `:474-479`) and the hand-written `ToolArguments` example compile as external crates and will need a wildcard too. The generated wildcard must not panic (the crate claims "nothing here panics"); name the shape, e.g. `FromNodeError::mismatch("a classified member", __pa_node)` or a hidden constructor.

### F9 ADVISORY. R3 F-4 const assertion

`const _: () = ::core::assert!(…::__sorted_names(__PA_NAMES));` inside the fn body works (`__PA_NAMES` is a block-level `const`). Make the helper `#[doc(hidden)] pub const fn __sorted_names(&[&str]) -> bool` with a byte loop and a doctest; this also pins that the derive's sort key (`key.value`, `write.rs:269`) agrees with core's byte order. Keep the debug asserts.

### F10 ADVISORY. R4 `#[tool(crate = path)]` and the versioned marker

- Semantics to state: the path names a root that exposes `tool_call` and `json` (the writer uses only `{root}::tool_call` and `{root}::json`, `write.rs:52-53`); the facade root exposes both through `pub use purrassist_core::*`, so `crate = my_fw::purrassist` works for both flavors and users never need `__core`. Accept only a path token sequence (idents, `::`, optional leading `::`, `crate`/`self`/`super`), spliced from the original tokens; anything else is a spanned `compile_error!` (the plan's compile_fail covers this). `::core`/`::std` paths are unaffected.
- The issue's D12 says "Neither audience writes a `crate =` attribute" (and gap-analysis requirement 5 records "no `crate =`" as COMPLETE). The addition is optional and additive, so the statement stays true by default, but it is new scope inside a remediation stage: say so in the #15 post for R4.
- Marker: derive the name from `env!("CARGO_PKG_VERSION_MAJOR")`/`MINOR` at derive build time so derive and core cannot disagree by hand-edit; core declares the matching `#[doc(hidden)] pub const`; state the rule "bump when the generated-code contract changes (minor line pre-1.0)".

### F11 ADVISORY. R1 details

Pick one escaping form; both work in `const` (`assert!(c, "{}", "msg")` lowers to a `&str` const panic). `struct bool; struct usize; struct str;` are all legal (primitive names are not keywords); drop "where legal". Field splicing keeps `r#type` verbatim, which is what the struct literal needs.

### F12 ADVISORY. G2 needs a falsifiable close-out step

"reported; validation is local" has no action. Add to "Settle and exit": run `gh pr checks 19` on the final head and record the result in `validation.md`; if hosted CI still did not run, write exactly "Hosted CI did not run." (per the GitHub-text constraint) in the PR body and the #15 post.

### F13 ADVISORY. R6 G6/G7 details

- G6: choose `#[cfg(doctest)] #[doc = include_str!("../README.md")] struct ReadmeDoctest;` rather than making the README the crate-level doc (`lib.rs:4-225` already carries the full docs; doubling them on docs.rs is noise). Feasibility confirmed: the README's block uses `purrassist_core` and `purrassist_derive` paths (`README.md:17-19`), the same as the existing `lib.rs` doctests, and derive's only dev-dependency is core, so it compiles as a doctest. Add a negative fixture for the root-README drift check in `test-repository-audit.py` (the plan names only the positive run).
- G7: error-code annotations are enforced only by nightly rustdoc; `make check` uses the pinned nightly (audit row 16), so this is real evidence in the gate. Say so.

### F14 ADVISORY. Commit typing

R3 is `refactor(tool-call)` but adds public impls (`f32`, `Box<T>`) and a sealed-trait method; type it `feat` or split the additive part out.

## Direct answers to the review questions

- **R2 default and top-level effect:** effects-based (`Mutating` -> `Error`, `ReadOnly` -> `DropWithLoss`); emitting it explicitly in `object_spec()` only changes nested behaviour (and hand-built equality fixtures); emitting it in `tool_definition()` would change top-level `read_only` tools. See F1.
- **G1 acceptance either/or:** unfalsifiable as written; falsifiable once the default is fixed (rewrite in F1).
- **R3 `f32` exactness / FromNode semantics / `Box<T>`:** F7. **Sealing `ToolExtra`:** the only impl is core's `Node` (`derive_support.rs:244`); the only external references are the derive's bound check (`write.rs:238`) and INTEGRATION.md:1564; unpublished, so sealing breaks nothing; INTEGRATION.md must say "sealed". **`Slot` non-exhaustive:** wildcard required in generated code and in core's doctests; F8.
- **R4 `=0.1.0` vs `check-release.py`:** incompatible today (`:127`); also the workspace-table question; F3. **Marker / `crate =`:** F10.
- **R5 unauthenticated verification:** existence and owners are public endpoints; trusted-publisher state is not; the either/or is resolved in F2 and existence-only is insufficient.
- **R6 README doctest in a proc-macro crate:** works (existing doctests prove it); F13.

## Rubric

- **UNFALSIFIABLE CRITERION:** R2 "rejects or drops ... per the nested policy" (F1); R5 "as far as the public API shows / if ... cannot be checked" (F2); R4 "updated if they assume caret" (F3).
- **PRE-EMPTIVE DESCOPE:** R5's fallback to existence-only before establishing what is checkable (F2); G2 closed as "reported" with no recheck (F12).
- **UNMAPPED REQUIREMENT:** audit A-2; structure F-5 `ToolParameter` defaults; structure F-6 parameter-name timing; structure F-5 `doc(hidden)` consideration (F5). Also R2 adopts option (b) without the user decision the gap analysis required (F1).
- **PLANNED-BY-REFUSAL:** G7 harness declined on a rationale that does not hold for `bridge.rs` (F6).

## Verdict

The plan maps the HIGH and MEDIUM gaps to commits with real entry points, sequences core changes before derive changes, and reruns the full gate at the end. It cannot be executed as written: R2's central value is undecided and its acceptance accepts either outcome; R5 defers its own design and the fallback does not close G3; R4 contradicts `check-release.py` and the fixtures; A-1's mechanism does not work; four advisories are dropped silently; one declination misstates coverage.

VERDICT: NEEDS-REVISION

## Orchestrator dispositions (pass 1)
- REQUIRED 1 (R2): user decision "Drop with recorded loss (Recommended)". Option (b): `object_spec()` always emits X; X = DropWithLoss when there is no key; `tool_definition()` is unchanged. Cases 1-4 are concrete (4 equals 2) and the churn list is kept.
- REQUIRED 2 (R5): rewritten verbatim: existence plus owner-set equality before auth; new `scripts/test-check-release.py`.
- REQUIRED 3 (R4): chosen and recorded. A versioned ABI marker; `=` pinning declined with reasons (lockstep and caret-rule conflict; the marker already gives a compile error).
- REQUIRED 4 (A-1): rewritten verbatim: sealed `ToolExtra::__from_members`.
- REQUIRED 5 (dropped findings): A-2 → R6 PR body; `ToolParameter` defaults → R3; parameter-name validation timing → R1; `Members`/`Slot` doc(hidden) → declined with reason.
- REQUIRED 6 (G7): the residual is disclosed accurately (bridge span selection is covered only by twins) and goes in the PR body.
- ADVISORY: `f32`/`Box` specifics adopted in R3; `Slot` wildcards in core doctests; G2 recheck step with "did not run" phrasing.

## Recheck (pass 2)

Re-read the revised `remediation-plan.md` in full. Additional code read for the two verification questions: `crates/core/src/tool_call/resolve.rs` (`CallOutcome::classify` :633-658, `CallLedger::data_loss` :327-329, `Resolver::object`/`assign` :1905-2040, `options` :2113-2171, `failure` :2700-2716, `change` :2719-2767), `definition.rs` :466-621, and every `object_spec` reference in `crates/` (only `write.rs:245,249` and the hand-written example in `derive_support.rs:89,109`).

### Verification 1: X = DropWithLoss with `tool_definition()` unchanged alters no existing top-level behaviour

Confirmed. `object_spec()` has exactly one consumer in generated code: `Param::object(<Self as ToolArguments>::object_spec())` inside `parameter()` (`write.rs:249`), which is reached only when the struct is a field of another struct. `tool_definition()` builds its own chain (`write.rs:193-215`) and emits `.unknown_parameters(..)` only when the key is declared, so `ToolDefinition::resolved_unknown_parameters` (`definition.rs:616-619`) still falls through to `Effects::default_unknown_parameters` for top-level tools. No core setter accepts an `ObjectSpec` as a tool's argument scope (F-6a is declined), so the explicit value cannot leak upward. Top-level tools, `read_only` included, are unchanged.

The change is confined to nested derived structs without an `unknown` key. One consequence the plan should say out loud in its docs line (R2 "Docs"): a nested extra member under a top-level `unknown = error` (or `mutating`-default) tool is today `Unresolved{UnknownParameter}` by inheritance and becomes `BestGuess` with a loss. That is the user's decision; it only needs stating.

### Verification 2: `BestGuess` with a recorded loss is the actual core outcome for a dropped nested member

Confirmed, by code and by the gap analysis's control run.

- The nested scope carries its own resolved handling: `toolset.rs:1542-1546` builds the nested `ObjectShape` with `object.unknown.unwrap_or(inherited)`, stored as `ObjectShape.unknown_parameters` (`:1428`). With R2 the derived nested spec sets `Some(DropWithLoss)`, so `inherited` is irrelevant for it.
- Resolution runs per object scope: `Resolver::object` -> `assign` take the scope's `shape: &ObjectShape` (`resolve.rs:1905-1915`, `2006-2013`) and consult `options()` per undeclared member (`:2113-2171`). Under `DropWithLoss` the option is `Action::Drop` with rule `tool_call.parameter.drop-unknown` (`:2158-2161`).
- Applying `Drop` records a `Change` whose `original` is `"\"zz\":2"` and whose evidence destination is the member's source pointer, `self.site.child(&member.key, &member.key).source` (`:2735-2744`, `:2757-2762`): `/inner/zz` and `/items/0/zz` for the plan's turn. The member is omitted from the slots (`:2856`).
- The loss is `LossKind::Data` (default kind at `:419`), `CallLedger::data_loss()` is true (`:327-329`), and `CallOutcome::classify` returns `BestGuess` (`:653-654`). The gap analysis observed exactly this for a nested struct inheriting `DropWithLoss` ("the call is BestGuess with `zz` dropped and conversion returns Ok").
- Case 1 (`unknown = error`): `options()` returns no option, `failure()` yields `UnresolvedReason::UnknownParameter { pointer: "/inner/zz" }` (`:2700-2706`, the first undecided member), matching the plan's "the unresolved reason names `/inner/zz`".

One precondition for the tests to be falsifiable as written: `Action::Drop` is offered only when `tolerant || !near_miss` (`:2158`). `Outer` is `mutating` by default and so `Structural`; a nested extra key that `json.key.match` would rename to a declared name is `UnknownParameter`, not dropped. `zz` against the declared `a` is not a near miss, so the plan's turn is fine; the test comment should say the key was chosen for that reason so a future edit does not pick `aa`.

### Adjudication of pass-1 findings

| # | Finding | Status | Notes |
|---|---|---|---|
| F1 | R2 default undecided; acceptance either/or; option (b) without decision | **DISCHARGED** | Decision recorded verbatim; X = `DropWithLoss`; `tool_definition()` unchanged; cases 1-4 concrete and verified against `resolve.rs` above. Two text defects remain, below (N1, N2). |
| F2 | R5 preflight either/or; existence insufficient | **DISCHARGED** | Adopted verbatim: existence + owner-set equality before auth; `test-check-release.py` with three negative copies, wired into `make check` and `verify.yml`. Falsifiable. |
| F3 | R4 `=` pinning vs tooling | **DISCHARGED** | `=` declined with reasons (lockstep across six crates, workspace-table constraint, caret rule); marker chosen with the name derived from `CARGO_PKG_VERSION`. The declination is honest about the cost ("caught at the user's compile, not by Cargo's resolver"). The stated marker test is vacuous, below (N3). |
| F4 | A-1 mechanism | **DISCHARGED** | Adopted verbatim; acceptance names snippet 30 with `E0277` and a one-error standalone build. |
| F5 | Four dropped findings | **DISCHARGED** | A-2 in R6 (:95); `ToolParameter` defaults in R3 (:69); parameter-name timing in R1 (:38) with the right file reference; `doc(hidden)` declined with a reason (:107). R3 :69 and :71 now say the `REQUIRED`/`absent()` relation twice; cosmetic. |
| F6 | G7 declination rationale | **STILL-OPEN (text)** | R6 :93 now discloses the residual accurately. But "Declined" :109 still reads "It would duplicate the D-12 pure-layer message tests. Error codes, plus the unit-test mapping, cover the remaining evidence gap." That contradicts :93, and the "unit-test mapping" table it cites was removed from R6. See N4. |
| F7 | `f32`/`Box` specifics | **DISCHARGED** | R3 :67-68 adopt the direct `f32` parse, non-finite rejection, the `FromNode for Box<T>` impl and the Box-cycle compile_fail with twin. |
| F8 | `Slot` wildcards in core doctests; wildcard error shape | **PARTLY** (advisory) | Doctest wildcards adopted (:70). The generated wildcard still "reports an internal error" with no named shape; keep as advisory for the R3 task review (must not panic). |
| F9 | `__sorted_names` helper shape | advisory, not adopted | :62 still writes `assert!(__sorted(&TABLE))`; the emitted form should be `::core::assert!` for the same reason as R1. Advisory. |
| F10 | `crate =` semantics; D12 scope note | advisory, not adopted | :77 unchanged. The marker self-derivation was adopted. Carry into the R4 task review. |
| F11 | R1 "where legal" | advisory, not adopted | :41 unchanged; cosmetic. |
| F12 | G2 recheck step | **DISCHARGED** | :102 adds the `gh pr checks 19` step with the constrained phrasing. |
| F13 | G6 either/or; drift negative fixture | advisory, not adopted | :89 still "as the crate-level doc, or as a doctest module". The acceptance ("so its example runs") is falsifiable either way; advisory. |
| F14 | R3 commit type | advisory, not adopted | cosmetic. |

### New findings in the revised text

**N1 REQUIRED (falsifiability). R2 "Repeat 4 ..." cannot be met as written.**

> "Repeat 4 with the top level at `unknown = drop` and at `effects = read_only` with no key."

Case 4's struct is `Outer { ..., #[param(extra)] rest: Node }` and its expectation includes `rest == {"top":3}`. With the top level at `unknown = drop`, or at `effects = read_only` with no key (which resolves to `DropWithLoss`), an `extra` field is a `compile_error!` ("a #[param(extra)] field needs #[tool(unknown = pass)]", `model.rs:557-561`), and the undeclared `top` is dropped rather than collected. The repeat variants need their own struct and expectation.

REWRITE TO: "Repeat 4 with `Outer2 { inner: Inner, items: Vec<Inner> }` (no `extra`) at `#[tool(unknown = drop)]`, and again at `#[tool(effects = read_only)]` with no `unknown` key: outcome `BestGuess`; losses with rule `tool_call.parameter.drop-unknown` at `/top`, `/inner/zz` and `/items/0/zz`; `Outer2::from_node` is `Ok(Outer2 { inner: Inner { a: 1 }, items: vec![Inner { a: 1 }] })`. The undeclared keys `top` and `zz` are chosen because neither is a near miss of a declared name; under `Structural` a near miss is `UnknownParameter`, not dropped."

Also in R2: line 56 ends with a stray `"`; line 57 says "updated under either option" although only one option remains.

**N2 ADVISORY. R2 loss assertion shape.** "a recorded loss at `/inner/zz`" is assertable: `CallLedger.losses` entries with `loss.evidence.rule == "tool_call.parameter.drop-unknown"`, `loss.original == "\"zz\":2"`, and evidence destination `/inner/zz` / `/items/0/zz`. Name those fields in the test so the assertion is on the recorded loss, not only on the outcome.

**N3 REQUIRED (falsifiability). R4 marker test tests nothing.**

> "Test: a compile_fail doctest referencing a missing marker version, with a twin."

A doctest cannot pair the derive with a mismatched core; the derive always emits the marker of its own version and the workspace core always has it. A hand-written `compile_fail` that names `__DERIVE_ABI_0_0` proves only that a nonexistent const does not exist. The claim to test is that the emitted name and core's declared name are both computed from the package version and agree.

REWRITE TO: "Tests: (1) a `write.rs` unit test asserts the generated text contains `__DERIVE_ABI_{major}_{minor}` with the numbers taken from `env!("CARGO_PKG_VERSION_MAJOR")`/`env!("CARGO_PKG_VERSION_MINOR")`, not a literal; (2) a core unit test asserts the const's name is built the same way from core's own `CARGO_PKG_VERSION` (both come from the single workspace version, so they agree by construction); (3) the existing derive and facade integration tests, which compile generated code against core, prove the reference resolves. A simulated skew is not testable in-tree and is not claimed."

**N4 REQUIRED (consistency). "Declined" :109 contradicts R6 :93.** Replace the sentence with: "Declined; the residual it would have covered (bridge span selection and `compile_error!` emission are proven only by the passing twins) is disclosed in the PR body, per R6."

### Rubric (pass 2)

- UNFALSIFIABLE CRITERION: N1 (an expectation that cannot be met), N3 (a test that cannot fail for the claimed reason).
- PRE-EMPTIVE DESCOPE: none remaining.
- UNMAPPED REQUIREMENT: none remaining.
- PLANNED-BY-REFUSAL: N4 is the leftover wording of the pass-1 hit; the substance (:93) is fixed.

### Verdict (pass 2)

Every pass-1 REQUIRED finding is discharged in substance, both code questions verify (the nested `DropWithLoss` path yields `BestGuess` with a `tool_call.parameter.drop-unknown` data loss at the nested source pointer; `tool_definition()` is untouched so no top-level behaviour moves). The design needs no further change. Three text edits remain before the plan can be executed against its own acceptance criteria: N1 (the repeat variants' struct and expectation), N3 (the marker test), N4 (the stale declination sentence). These are one-paragraph edits; no new decision is required.

VERDICT: NEEDS-REVISION

## Recheck (pass 3)

Scope, per the coordinator: N1-N4 and F6 only. Re-read `remediation-plan.md` R2 (:44-59), R4 (:76-82), R6 (:87-98) and Declined (:106-110). No further code reads were needed; every claim in the new text was verified against `resolve.rs`, `model.rs` and `write.rs` in pass 2.

| # | Finding | Status | Evidence in the revised plan |
|---|---|---|---|
| N1 | R2 repeat variants impossible as written | **DISCHARGED** | :57 now uses `Outer2 { inner: Inner, items: Vec<Inner> }` (no `extra`) at `#[tool(unknown = drop)]` and at `#[tool(effects = read_only)]` with no key; expects `BestGuess`, `tool_call.parameter.drop-unknown` losses at `/top`, `/inner/zz`, `/items/0/zz`, and `Ok(Outer2 { inner: Inner { a: 1 }, items: vec![Inner { a: 1 }] })`; records why `top`/`zz` are not near misses. Each assertion names one observable; both variants resolve to `DropWithLoss` at the top level and `DropWithLoss` nested, so the expectation is reachable and single-valued. The stray `"` is gone and :58 no longer says "under either option". |
| N2 | Loss assertion shape | **DISCHARGED** (advisory) | :55 names `loss.evidence.rule`, `loss.original == "\"zz\":2"` and the evidence destination; these are the fields `change()` populates (`resolve.rs:2735-2762`). |
| N3 | R4 marker test vacuous | **DISCHARGED** | :80 replaces the compile_fail with (1) a `write.rs` unit test on `__DERIVE_ABI_{major}_{minor}` computed from `env!("CARGO_PKG_VERSION_MAJOR")`/`MINOR`, (2) a core unit test that the const's name is built the same way from core's version, (3) the existing integration tests as the resolution proof, and states that a simulated skew is not claimed. (1) and (2) fail if either side hard-codes a literal or the two computations diverge; (3) fails if the reference does not resolve. Falsifiable. |
| N4 | Declined entry contradicts R6 | **DISCHARGED** | :110 now reads "Declined. The residual it would have covered (bridge span selection and `compile_error!` emission are proven only by the passing twins) is disclosed in the PR body, per R6." It agrees with R6 :94 and no longer cites the removed mapping table. |
| F6 | G7 declination rationale (PLANNED-BY-REFUSAL) | **DISCHARGED** | Substance fixed in pass 2 (:94); wording fixed by N4. The declination now states what is not covered and where that is disclosed, which is the accurate form the pass-1 rewrite offered as acceptable for a LOW gap. |

Rubric (pass 3): no UNFALSIFIABLE CRITERION, PRE-EMPTIVE DESCOPE, UNMAPPED REQUIREMENT or PLANNED-BY-REFUSAL remains within the rechecked scope.

Nothing is still open that needs a user decision. The remaining advisories from pass 2 (F8 wildcard error shape, F9 `::core::assert!` in the sorted-table check, F10 `crate =` path semantics and the D12 scope note, F11, F13 G6 either/or, F14 commit type) do not affect executability or falsifiability; they belong to the per-commit task reviews (`tasks/R3-review.md`, `tasks/R4-review.md`, `tasks/R6-review.md`), and the plan already schedules those.

VERDICT: PASS
